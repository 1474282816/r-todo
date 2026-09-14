use std::{path::PathBuf, sync::{Arc, RwLock}};

use axum::{
    Router,
    extract::Request,
    http::{StatusCode, header::AUTHORIZATION},
    middleware::{from_fn, Next},
    response::Response,
};
use rmcp::handler::server::tool::parse_json_object;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use rmcp::schemars::JsonSchema;
use rmcp::model::ErrorCode;
use rmcp::ErrorData as McpError;
use rmcp::ServerHandler;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use serde::Deserialize;
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

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult {
            tools: tools(),
            next_cursor: None,
            ..Default::default()
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResponse, McpError>> + Send + '_ {
        let name = request.name.to_string();
        let args = request.arguments.unwrap_or_default();
        let this = self.clone();
        Box::pin(async move {
            // 错误分级：parse 失败 → 协议级 invalid_params（parse_json_object 已返回）；
            // db 基础设施失败 → internal_error；业务失败（任务不存在/校验）→ 工具级 structured_error（AI 可见）
            let outcome: Result<CallToolResult, McpError> = match name.as_str() {
                "list_tasks" => {
                    let input: ListTasksInput = parse_json_object(args)?;
                    let conn = this.db().map_err(infra)?;
                    let list = crate::mcp_tools::list_tasks(
                        &conn,
                        input.filter.as_deref().unwrap_or("active"),
                        input.keyword.as_deref(),
                        input.priority.as_deref(),
                    )
                    .map_err(infra)?;
                    Ok(CallToolResult::structured(serde_json::to_value(list).expect("json")))
                }
                "create_task" => {
                    let input: CreateTaskInput = parse_json_object(args)?;
                    let mut conn = this.db().map_err(infra)?;
                    match crate::mcp_tools::create_task(
                        &mut conn,
                        &input.title,
                        input.priority.as_deref(),
                        input.remind_at.as_deref(),
                        input.remind_offset_min,
                        input.repeat_rule.as_deref(),
                    ) {
                        Ok(t) => {
                            this.notify_changed();
                            Ok(CallToolResult::structured(serde_json::to_value(t).expect("json")))
                        }
                        Err(msg) => Ok(CallToolResult::structured_error(serde_json::json!({ "error": msg }))),
                    }
                }
                "update_task" => {
                    let input: UpdateTaskInput = parse_json_object(args)?;
                    let mut conn = this.db().map_err(infra)?;
                    // 三态：absent=不变(null→清除、有值→设置)，空串在数据层视为清除
                    let remind_at = input.remind_at.as_ref().map(|v| v.as_deref());
                    let repeat_rule = input.repeat_rule.as_ref().map(|v| v.as_deref());
                    match crate::mcp_tools::update_task(
                        &mut conn,
                        &input.id,
                        input.title.as_deref(),
                        input.priority.as_deref(),
                        remind_at,
                        repeat_rule,
                    ) {
                        Ok(t) => {
                            this.notify_changed();
                            Ok(CallToolResult::structured(serde_json::to_value(t).expect("json")))
                        }
                        Err(msg) => Ok(CallToolResult::structured_error(serde_json::json!({ "error": msg }))),
                    }
                }
                "complete_task" | "uncomplete_task" => {
                    let input: IdInput = parse_json_object(args)?;
                    let mut conn = this.db().map_err(infra)?;
                    match crate::mcp_tools::set_completed(&mut conn, &input.id, name == "complete_task") {
                        Ok(t) => {
                            this.notify_changed();
                            Ok(CallToolResult::structured(serde_json::to_value(t).expect("json")))
                        }
                        Err(msg) => Ok(CallToolResult::structured_error(serde_json::json!({ "error": msg }))),
                    }
                }
                "delete_task" => {
                    let input: IdInput = parse_json_object(args)?;
                    let mut conn = this.db().map_err(infra)?;
                    match crate::mcp_tools::soft_delete(&mut conn, &input.id) {
                        Ok(()) => {
                            this.notify_changed();
                            Ok(CallToolResult::structured(serde_json::json!({ "deleted": true, "id": input.id })))
                        }
                        Err(msg) => Ok(CallToolResult::structured_error(serde_json::json!({ "error": msg }))),
                    }
                }
                other => Ok(CallToolResult::structured_error(serde_json::json!({
                    "error": format!("未知工具: {other}")
                }))),
            };
            Ok(outcome?.into())
        })
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

/// 基础设施错误（db 打开/查询失败）→ 协议级错误
fn infra(e: String) -> McpError {
    McpError::new(ErrorCode::INTERNAL_ERROR, e, None)
}

// ── 工具输入结构（schemars 自动生成 JSON Schema） ──
// 注：schemars 非直接依赖，derive 生成代码默认引用顶层 `schemars` crate，
// 需用 crate 属性指向 rmcp 的重导出（rmcp::schemars）
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListTasksInput {
    /// active=未完成（默认）| completed=已完成 | all=全部
    pub filter: Option<String>,
    /// 标题包含关键字
    pub keyword: Option<String>,
    /// 按优先级过滤: high | medium | low
    pub priority: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateTaskInput {
    /// 任务标题（必填，非空）
    pub title: String,
    pub priority: Option<String>,
    /// 提醒时间 ISO8601
    pub remind_at: Option<String>,
    /// 提前提醒分钟数（默认 15）
    pub remind_offset_min: Option<i64>,
    /// RRULE 重复规则
    pub repeat_rule: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct IdInput {
    pub id: String,
}

/// 区分「缺省/absent / null / 有值」的自定义反序列化器：
/// 缺省 → 外层 None（数据层=不变）；null → Some(None)（=清除）；有值 → Some(Some(s))（=设置）
/// 注：不能写成 `Option<Option<String>>` 默认反序列化（null 会塌缩成外层 None，无法区分 null 与缺省）。
/// 用 deserialize_with 而非 serde with：schemars derive 会把 `with` 的模块路径当 schema 类型解析导致编译失败。
mod nullable_opt {
    use serde::de::{self, Deserializer, Visitor};
    use serde::Deserialize;
    use std::fmt;

    pub fn deserialize<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Option<Option<String>>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string or null")
            }
            fn visit_some<D2>(self, d: D2) -> Result<Self::Value, D2::Error>
            where
                D2: Deserializer<'de>,
            {
                Ok(Some(Option::<String>::deserialize(d)?))
            }
            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Some(None))
            }
        }
        d.deserialize_option(V)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct UpdateTaskInput {
    pub id: String,
    pub title: Option<String>,
    pub priority: Option<String>,
    /// 传 null 清除提醒（缺省=不变）
    #[serde(default, deserialize_with = "nullable_opt::deserialize")]
    pub remind_at: Option<Option<String>>,
    /// 传 null 清除重复规则（缺省=不变）
    #[serde(default, deserialize_with = "nullable_opt::deserialize")]
    pub repeat_rule: Option<Option<String>>,
}

/// Tool 为 #[non_exhaustive]（跨 crate 禁止 struct 表达式构造），且 rmcp 未实现
/// Into<Arc<JsonObject>>（RootSchema 不能直接传），故 schema_for! 序列化后 Arc::new(JsonObject)
/// 走官方 builder API（Tool::new + with_title）。
fn tool_def(name: &'static str, title: &'static str, description: &'static str) -> Tool {
    let schema = match name {
        "list_tasks" => rmcp::schemars::schema_for!(ListTasksInput),
        "create_task" => rmcp::schemars::schema_for!(CreateTaskInput),
        "update_task" => rmcp::schemars::schema_for!(UpdateTaskInput),
        "complete_task" | "uncomplete_task" | "delete_task" => rmcp::schemars::schema_for!(IdInput),
        _ => unreachable!(),
    };
    let input_schema = Arc::new(
        serde_json::to_value(&schema)
            .expect("schema json")
            .as_object()
            .expect("schema object")
            .clone(),
    );
    Tool::new(name, description, input_schema).with_title(title)
}

fn tools() -> Vec<Tool> {
    vec![
        tool_def("list_tasks", "列出待办", "按条件列出待办任务：filter(active/completed/all)、keyword 标题包含、priority 过滤"),
        tool_def("create_task", "新建待办", "新建一个待办任务，置顶显示。title 必填；支持优先级、提醒时间、RRULE 重复规则"),
        tool_def("update_task", "编辑待办", "编辑任务标题/优先级/提醒时间/重复规则；remind_at 传 null 清除提醒"),
        tool_def("complete_task", "完成待办", "将任务标记为已完成"),
        tool_def("uncomplete_task", "恢复待办", "将已完成任务恢复为未完成"),
        tool_def("delete_task", "删除待办", "删除任务（软删除，可随同步恢复）"),
    ]
}

impl McpHandler {
    fn db(&self) -> Result<rusqlite::Connection, String> {
        crate::mcp_tools::open_db(&self.db_path)
    }

    fn notify_changed(&self) {
        (self.notify)();
    }
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

    #[test]
    fn tools_are_complete_and_schemas_valid() {
        let ts = tools();
        let names: Vec<&str> = ts.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(
            names,
            vec!["list_tasks", "create_task", "update_task", "complete_task", "uncomplete_task", "delete_task"]
        );
        for t in &ts {
            assert!(t.description.is_some(), "{} 缺描述", t.name);
            assert!(!t.input_schema.is_empty(), "{} 缺参数 schema", t.name);
        }
    }
}
