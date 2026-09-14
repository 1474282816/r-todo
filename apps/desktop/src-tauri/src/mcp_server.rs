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

/// 在 127.0.0.1:port 起 MCP HTTP server
/// 注意：cancel token 不参与 bind/serve 判断，只派生请求级子令牌 —— 真正终止监听的是 stop() 的 abort()。
/// 复用已取消的 token 会让监听正常建立但每个请求返回 500（rmcp 的 child_token 立即取消），
/// 故每次启动必须新建 token（见 McpState::start）。
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
    handle: Arc<std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    token_slot: Arc<RwLock<String>>,
    /// 每次 start 新建一个 —— 复用已取消的 token 会让监听正常建立但每个请求返回 500
    cancel: Arc<std::sync::Mutex<Option<CancellationToken>>>,
    /// 启动失败原因；每次启动尝试开始时清空。设置面板读取（失败上报 + 重试）
    error_slot: Arc<RwLock<Option<String>>>,
}

impl McpState {
    /// 惰性移除已结束任务留下的死句柄，返回是否仍在运行。
    /// 绑定失败发生在任务内、start 已 Ok 返回，若不识别死句柄，
    /// 运行态会永久报 true 且守卫会挡住用户重试（spec §8 的重试路径）。
    fn sync_running(&self) -> bool {
        let mut guard = self.handle.lock().expect("handle");
        // tauri 的 JoinHandle 是 Tokio(tokio JoinHandle) 的包装枚举，须经 inner() 取 tokio 句柄
        if guard.as_ref().is_some_and(|h| h.inner().is_finished()) {
            guard.take();
        }
        guard.is_some()
    }

    /// 生命周期核心（不依赖 AppHandle，便于单测 start→stop→start 往返）。已运行时 no-op。
    fn start(
        &self,
        db_path: PathBuf,
        notify: NotifyFn,
        port: u16,
        token: String,
    ) -> Result<(), String> {
        if self.sync_running() {
            // 已在运行：清掉历史报错，避免面板对健康 server 显示过期错误
            *self.error_slot.write().expect("error") = None;
            return Ok(());
        }
        // 新尝试开始即同步清错：spawn 是异步的，若放进任务体，no-op 守卫路径与快速重试会残留旧报错
        *self.error_slot.write().expect("error") = None;
        *self.token_slot.write().expect("slot") = token;
        let cancel = CancellationToken::new();
        *self.cancel.lock().expect("cancel") = Some(cancel.clone());
        let slot = self.token_slot.clone();
        let error_slot = self.error_slot.clone();
        let handle = tauri::async_runtime::spawn(async move {
            if let Err(e) = serve_mcp(db_path, slot, notify, port, cancel).await {
                *error_slot.write().expect("error") = Some(e);
            }
            // 句柄不在此清理：spawn 可能已同步结束（bind 失败），写回会与主线程的
            // Some(handle) 交错成僵尸句柄 → 统一由 sync_running 惰性清理
        });
        *self.handle.lock().expect("handle") = Some(handle);
        Ok(())
    }

    fn stop(&self) {
        // 用户主动停止：清掉历史报错，避免 get_mcp_running 对已停止的 server 返回过期错误
        *self.error_slot.write().expect("error") = None;
        // MutexGuard 临时值先绑定到局部变量（tail/if-let 里直接取会撞上 State guard 的 Drop 顺序）
        let handle = self.handle.lock().expect("handle").take();
        if let Some(cancel) = self.cancel.lock().expect("cancel").take() {
            cancel.cancel();
        }
        if let Some(h) = handle {
            h.abort();
        }
    }
}

/// 写操作后通知前端刷新
fn notify_from(app: tauri::AppHandle) -> NotifyFn {
    Arc::new(move || {
        let _ = app.emit("tasks-changed", ());
    })
}

/// 按配置文件启动 MCP server；未启用时为 no-op。
/// token 为空时生成并持久化 —— 否则 auth_middleware 会放行 `Authorization: Bearer `（空 token）
pub fn start_mcp(app: &tauri::AppHandle) -> Result<(), String> {
    let path = crate::mcp_config::config_path(app);
    let mut cfg = crate::mcp_config::load_from(&path);
    if !cfg.enabled {
        return Ok(());
    }
    if cfg.token.is_empty() {
        cfg.token = crate::mcp_config::generate_token();
        crate::mcp_config::save_to(&path, &cfg)?;
    }
    let db_path = path
        .parent()
        .map(|p| p.join("r-todo.db"))
        .unwrap_or_else(|| PathBuf::from("r-todo.db"));
    app.state::<McpState>().start(
        db_path,
        notify_from(app.clone()),
        crate::mcp_config::MCP_PORT,
        cfg.token,
    )
}

pub fn stop_mcp(app: &tauri::AppHandle) {
    app.state::<McpState>().stop();
}

/// 运行态与启动失败原因（配置态由 mcp_config::get_mcp_status 提供）
#[derive(Clone, serde::Serialize)]
pub struct McpRunState {
    pub running: bool,
    pub error: Option<String>,
}

/// 设置面板读取：暴露启动失败原因 + 运行态（spec §8）
#[tauri::command]
pub fn get_mcp_running(app: tauri::AppHandle) -> McpRunState {
    let state = app.state::<McpState>();
    // 先落到局部变量：struct 字面量里的临时 guard 活到块尾，晚于 state 析构（E0597）
    let running = state.sync_running();
    let error = state.error_slot.read().expect("error").clone();
    McpRunState { running, error }
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
    // spec §5.4：token 稳定不自动轮换 —— 关闭仅停止监听、不清 token，
    // 重新开启复用同一 token，已配置 agent 无需重配
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
    // spec §7：重置 token 是泄露应对手段；关闭态（未监听）同样允许重置，重新开启后生效
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

    async fn wait_port(port: u16) {
        for _ in 0..50 {
            if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("server did not start on port {port}");
    }

    async fn wait_port_closed(port: u16) {
        for _ in 0..50 {
            if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("port {port} still open after stop");
    }

    async fn start_test_server(port: u16, token: &str) -> tokio::task::JoinHandle<Result<(), String>> {
        let slot = Arc::new(RwLock::new(token.to_string()));
        let db = std::env::temp_dir().join(format!("rtodo_mcp_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let cancel = CancellationToken::new();
        let handle = tokio::spawn(serve_mcp(db, slot, noop(), port, cancel));
        wait_port(port).await;
        handle
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

    #[tokio::test]
    async fn start_stop_start_serves_again() {
        // 回归：复用已取消的 CancellationToken 会让监听正常建立、但每个请求返回 500
        let state = McpState::default();
        let db = std::env::temp_dir().join(format!(
            "rtodo_mcp_test_{}_restart.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db);

        state.start(db.clone(), noop(), 18770, "secret".into()).unwrap();
        wait_port(18770).await;
        let (status, json) = initialize(18770, Some("secret")).await;
        assert_eq!(status, 200);
        assert_eq!(json["result"]["serverInfo"]["name"], "r-todo-desktop");

        state.stop();
        wait_port_closed(18770).await;
        assert!(!state.sync_running());

        state.start(db.clone(), noop(), 18770, "secret".into()).unwrap();
        wait_port(18770).await;
        assert!(state.sync_running());
        let (status, json) = initialize(18770, Some("secret")).await;
        assert_eq!(status, 200, "重启后应正常响应，而非 500");
        assert_eq!(json["result"]["serverInfo"]["name"], "r-todo-desktop");
        state.stop();
    }

    #[tokio::test]
    async fn bind_failure_sets_error_slot() {
        // 回归：绑定失败发生在任务内，start 已 Ok 返回 → 必须由错误槽上报、句柄被惰性清理
        let state = McpState::default();
        let db = std::env::temp_dir().join(format!(
            "rtodo_mcp_test_{}_bindfail.db",
            std::process::id()
        ));

        // 先占住 18771，再让 start 去 bind 同一端口
        let blocker = start_test_server(18771, "secret").await;
        assert!(state.start(db, noop(), 18771, "secret".into()).is_ok());

        // 等任务退出并写回错误槽
        for _ in 0..50 {
            if state.error_slot.read().expect("error").is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(state.sync_running() == false, "死句柄应被惰性清理，而非永久报运行中");
        let err = state.error_slot.read().expect("error").clone().expect("error recorded");
        assert!(err.contains("18771"), "错误信息应含端口: {err}");
        blocker.abort();
    }
}
