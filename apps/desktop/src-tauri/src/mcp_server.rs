use std::{path::PathBuf, sync::{Arc, RwLock}};

use axum::{
    Router,
    extract::Request,
    http::{StatusCode, header::AUTHORIZATION},
    middleware::{from_fn, Next},
    response::Response,
};
use rmcp::{ServerHandler, model::{Implementation, ServerCapabilities, ServerInfo}};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

/// 写操作后通知前端刷新（生产=emit "tasks-changed"，测试=空操作）
pub type NotifyFn = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct McpHandler {
    pub db_path: PathBuf,
    pub notify: NotifyFn,
}

impl ServerHandler for McpHandler {
    fn get_info(&self) -> ServerInfo {
        // rmcp 3.3 的 ServerInfo(InitializeResult) 为 #[non_exhaustive]，
        // 本工具链(rustc 1.97)禁止跨 crate 用 struct 表达式构造（含 ..Default），故用官方 builder API；
        // from_build_env 的 name 固定为 "rmcp"，需显式 with_server_info 以满足测试对 "r-todo-desktop" 的断言
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("r-todo 待办：列出、创建、编辑、完成、删除任务")
            .with_server_info(Implementation::new("r-todo-desktop", env!("CARGO_PKG_VERSION")))
    }
}

/// 恒时字符串比较（防时序侧信道）
fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// axum 认证中间件：校验 Authorization: Bearer <token>（token 从共享槽动态读，rotate 即时生效）
/// axum 0.8 中 Next 不再泛型化，Request 即 axum::extract::Request(Request<Body>)
async fn auth_middleware(
    req: Request,
    next: Next,
    token_slot: Arc<RwLock<String>>,
) -> Result<Response, StatusCode> {
    let token = token_slot.read().expect("token slot").clone();
    let ok = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| constant_time_eq(v, &format!("Bearer {token}")));
    if ok {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// 在 127.0.0.1:port 起 MCP HTTP server（阻塞直至停止或出错）
pub async fn serve_mcp(
    db_path: PathBuf,
    token_slot: Arc<RwLock<String>>,
    notify: NotifyFn,
    port: u16,
    cancel: CancellationToken,
) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("MCP 端口 {port} 绑定失败: {e}"))?;

    let handler = McpHandler { db_path, notify };
    // rmcp 3.3 默认 legacy_session_mode=true（走 session 路径，NeverSessionManager 会拒建 session→500）；
    // 本服务是无状态 streamable HTTP，需显式关掉 legacy 模式走 stateless 路径
    let config = StreamableHttpServerConfig::default()
        .with_cancellation_token(cancel)
        .with_json_response(true)
        .with_legacy_session_mode(false);
    let service = StreamableHttpService::<McpHandler, NeverSessionManager>::new(
        move || Ok(handler.clone()),
        Arc::new(NeverSessionManager::default()),
        config,
    );
    // route_service + layer：layer 对 route 生效是 axum 的确定行为（fallback 不受 layer 影响，故不用 fallback）
    let router = Router::new()
        .route_service("/mcp", service)
        .layer(from_fn(move |req, next| auth_middleware(req, next, token_slot.clone())));
    axum::serve(listener, router).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// 运行中 MCP server 的句柄与 token 槽（rotate 时热更新）
#[derive(Default)]
pub struct McpState {
    handle: std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub token_slot: Arc<RwLock<String>>,
    cancel: CancellationToken,
}

pub fn start_mcp(app: &tauri::AppHandle) -> Result<(), String> {
    let cfg = crate::mcp_config::load_from(&crate::mcp_config::config_path(app));
    if !cfg.enabled {
        return Ok(());
    }
    let state = app.state::<McpState>();
    if state.handle.lock().expect("state").is_some() {
        return Ok(()); // 已运行
    }
    *state.token_slot.write().expect("slot") = cfg.token.clone();
    let db_path = crate::mcp_config::config_path(app)
        .parent()
        .map(|p| p.join("r-todo.db"))
        .unwrap_or_else(|| PathBuf::from("r-todo.db"));
    let slot = state.token_slot.clone();
    let cancel = state.cancel.clone();
    let app_handle = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let notify: NotifyFn = Arc::new(move || {
            let _ = app_handle.emit("tasks-changed", ());
        });
        let _ = serve_mcp(db_path, slot, notify, crate::mcp_config::MCP_PORT, cancel).await;
    });
    *state.handle.lock().expect("state") = Some(handle);
    Ok(())
}

pub fn stop_mcp(app: &tauri::AppHandle) {
    let state = app.state::<McpState>();
    // MutexGuard 临时值先绑定到局部变量（tail/if-let 里直接取会撞上 State guard 的 Drop 顺序）
    let handle = state.handle.lock().expect("state").take();
    if let Some(h) = handle {
        state.cancel.cancel();
        h.abort();
    }
}

pub fn mcp_running(app: &tauri::AppHandle) -> bool {
    let state = app.state::<McpState>();
    let running = state.handle.lock().expect("state").is_some();
    running
}

/// rotate 后热更新运行中 server 的 token
pub fn set_mcp_token(app: &tauri::AppHandle, token: &str) {
    let state = app.state::<McpState>();
    *state.token_slot.write().expect("slot") = token.to_string();
}

#[tauri::command]
pub fn set_mcp_enabled(app: tauri::AppHandle, enabled: bool) -> Result<crate::mcp_config::McpConfig, String> {
    let mut cfg = crate::mcp_config::load_from(&crate::mcp_config::config_path(&app));
    cfg.enabled = enabled;
    if enabled && cfg.token.is_empty() {
        cfg.token = crate::mcp_config::generate_token();
    }
    // 交接约束：关掉开关即清空 token，get_mcp_client_templates 的"未启用"不变量立即可恢复
    if !enabled {
        cfg.token = String::new();
    }
    crate::mcp_config::save_to(&crate::mcp_config::config_path(&app), &cfg)?;
    if enabled {
        start_mcp(&app)?;
    } else {
        stop_mcp(&app);
    }
    Ok(cfg)
}

#[tauri::command]
pub fn rotate_mcp_token(app: tauri::AppHandle) -> Result<crate::mcp_config::McpConfig, String> {
    let mut cfg = crate::mcp_config::load_from(&crate::mcp_config::config_path(&app));
    cfg.token = crate::mcp_config::generate_token();
    crate::mcp_config::save_to(&crate::mcp_config::config_path(&app), &cfg)?;
    set_mcp_token(&app, &cfg.token); // 运行中立即生效
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop() -> NotifyFn {
        Arc::new(|| {})
    }

    async fn start_test_server(port: u16, token: &str) -> tokio::task::JoinHandle<Result<(), String>> {
        let slot = Arc::new(RwLock::new(token.to_string()));
        let db = std::env::temp_dir().join(format!("rtodo_mcp_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let cancel = CancellationToken::new();
        let handle = tokio::spawn(serve_mcp(db, slot, noop(), port, cancel));
        // 等端口就绪
        for _ in 0..50 {
            if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                return handle;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("server did not start on port {port}");
    }

    async fn initialize(port: u16, token: Option<&str>) -> (u16, serde_json::Value) {
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        });
        let client = reqwest::Client::new();
        let mut req = client.post(format!("http://127.0.0.1:{port}/mcp"))
            .header("Content-Type", "application/json")
            // rmcp 3.3 校验 Accept 必须同时含 application/json 与 text/event-stream，缺则 406（MCP streamable HTTP 客户端标准行为）
            .header("Accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(t) = token {
            req = req.header(AUTHORIZATION, format!("Bearer {t}"));
        }
        let resp = req.send().await.expect("http");
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        let json = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    #[tokio::test]
    async fn missing_token_returns_401() {
        let handle = start_test_server(18766, "secret").await;
        let (status, _) = initialize(18766, None).await;
        assert_eq!(status, 401);
        handle.abort();
    }

    #[tokio::test]
    async fn wrong_token_returns_401() {
        let handle = start_test_server(18767, "secret").await;
        let (status, _) = initialize(18767, Some("wrong")).await;
        assert_eq!(status, 401);
        handle.abort();
    }

    #[tokio::test]
    async fn correct_token_initializes() {
        let handle = start_test_server(18768, "secret").await;
        let (status, json) = initialize(18768, Some("secret")).await;
        assert_eq!(status, 200);
        assert_eq!(json["result"]["serverInfo"]["name"], "r-todo-desktop");
        handle.abort();
    }
}
