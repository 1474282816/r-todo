# 桌面端内置 MCP Server 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让外部 AI（Claude Code / Claude Desktop / opencode / codex 等）通过 MCP 协议操作 r-todo 桌面端的待办（查询/新建/编辑/完成/删除），UI 实时刷新、同步链路复用。

**Architecture:** Tauri 应用内置一个 streamable HTTP MCP server（rmcp 3.3 + axum），监听 `127.0.0.1:18765/mcp`，Bearer token 认证。工具调用经 rusqlite 直写应用自己的 `r-todo.db`（SQL 语义与前端 `db.ts` 对齐，含 `pending_ops` 离线队列），写操作后 emit `tasks-changed` 事件让前端重拉刷新。设置面板提供开关、token 管理、各 agent 配置模板。

**Tech Stack:** Rust（rmcp 3.3 / axum 0.8 / rusqlite 0.40 bundled / rand 0.10）、Tauri 2、TypeScript（前端仅小改）。

**Spec:** [docs/superpowers/specs/2026-09-13-mcp-server-design.md](../specs/2026-09-13-mcp-server-design.md) — 计划依据 spec 论证，实施时两者同读。

## Global Constraints

- 端口固定 **18765**，只绑定 `127.0.0.1`。
- 开关**默认关闭**；token 前缀 `rtok_`，32 字节随机 hex；token **不自动轮换**，仅主动「重置」时变更。
- Bearer token 认证，比较须为恒时比较；未启用时端口不监听。
- 写操作 SQL 语义与前端 `src/lib/db.ts` 逐条对齐：新任务 `sort_order = MIN(sort_order) - 1000` 置顶；删除为软删（`deleted=1, deleted_at=now`）；每个写操作 = 任务变更 + `pending_ops` 写入，**同一事务**。
- `pending_ops.payload` 用 PocketBase RemoteTask 字段名（`id,user,title,priority,remind_at,remind_offset_min,repeat_rule,sort_order,deleted,completed_at`），其中 `user` 字段**不写**（flush 时由应用注入，见 Task 3）。
- 写操作成功后 emit 事件名 **`tasks-changed`**；只读工具不触发。
- Task JSON 字段与前端 `src/lib/types.ts` 一致：`id,title,priority,remind_at,remind_offset_min,repeat_rule,sort_order,deleted,completed_at,created_at,updated_at`。
- MCP 错误规范：参数校验失败 / 未找到任务 → `Ok(CallToolResult::structured_error(...))`（工具级错误，AI 可见）；协议畸形 → `Err(ErrorData::invalid_params(...))`。
- 前端不动同步逻辑（`sync.ts` flush 队列天然覆盖外部写入），唯一允许的改动：`flushPendingOps` 里 create 时补 `user` 字段。

## 文件结构

| 文件 | 责任 | 动作 |
|---|---|---|
| `apps/desktop/src-tauri/Cargo.toml` | 依赖声明 | 修改（Task 1/2/3） |
| `apps/desktop/src-tauri/src/mcp_config.rs` | MCP 配置读写（纯函数）+ 2 个只读 command + agent 模板生成 | 新建（Task 1） |
| `apps/desktop/src-tauri/src/mcp_server.rs` | axum + rmcp HTTP server、认证、handler、启停管理 | 新建（Task 2/3） |
| `apps/desktop/src-tauri/src/mcp_tools.rs` | rusqlite 数据层：6 个工具的业务逻辑 | 新建（Task 3） |
| `apps/desktop/src-tauri/src/lib.rs` | mod 声明、state 管理、setup 启动、command 注册 | 修改（Task 1/2） |
| `apps/desktop/src/lib/store.ts` | 抽 `refreshFromDb` + 订阅 `tasks-changed` | 修改（Task 4） |
| `apps/desktop/src/lib/mcp.ts` | 前端 invoke 封装 | 新建（Task 5） |
| `apps/desktop/src/components/settings/SettingsPanel.tsx` | MCP 设置区 UI | 修改（Task 5） |
| `apps/desktop/src/lib/sync.ts` | flush 时注入 user 字段 | 修改（Task 3，3 行） |
| `README.md` | MCP 使用说明 | 修改（Task 6） |

Rust 内部依赖关系：`mcp_server.rs`（HTTP + 认证 + handler）→ `mcp_tools.rs`（数据层）；`lib.rs`（state + command 注册）→ 两者；`mcp_config.rs` 独立。

---

### Task 1: MCP 配置管理（开关 / token / agent 模板）

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp_config.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`（加 `rand`）
- Modify: `apps/desktop/src-tauri/src/lib.rs`（mod + command 注册）

**Interfaces:**
- Consumes: Tauri 2 `AppHandle`（`app.path().app_config_dir()`）；配置路径为 `app_config_dir/mcp-config.json`（与 tauri-plugin-sql 同一目录，已核实其 `wrapper.rs:79-86`）。
- Produces:
  - `pub struct McpConfig { pub enabled: bool, pub token: String }`（`Serialize/Deserialize/Clone/PartialEq/Default`，Default = 全关）
  - `pub fn config_path(app: &tauri::AppHandle) -> std::path::PathBuf`
  - `pub fn load_from(path: &std::path::Path) -> McpConfig`（文件不存在/解析失败 → Default）
  - `pub fn save_to(path: &std::path::Path, cfg: &McpConfig) -> Result<(), String>`（创建父目录）
  - `pub fn generate_token() -> String`（`rtok_` + 32 字节 hex）
  - `pub struct McpTemplates { pub claude_code: String, pub claude_desktop: String, pub opencode: String, pub codex: String }`
  - `pub fn build_templates(token: &str) -> McpTemplates`
  - commands：`get_mcp_status(app) -> McpConfig`、`get_mcp_client_templates(app) -> Result<McpTemplates, String>`（未启用时报错）。**启停联动命令 `set_mcp_enabled` / `rotate_mcp_token` 在 Task 2 定义**（需要启停 server 和热更新 token 槽），本任务不定义，避免与 Task 2 重复。

- [ ] **Step 1: 加依赖**

在 `apps/desktop/src-tauri/Cargo.toml` 的 `[dependencies]` 末尾加：

```toml
# MCP 配置 token 生成
rand = "0.10"
```

- [ ] **Step 2: 写失败测试（config 往返 / token / 模板）**

新建 `apps/desktop/src-tauri/src/mcp_config.rs`，末尾带 `#[cfg(test)] mod tests`：

```rust
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

pub const MCP_PORT: u16 = 18765;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpConfig {
    pub enabled: bool,
    pub token: String,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self { enabled: false, token: String::new() }
    }
}

pub fn config_path(app: &tauri::AppHandle) -> PathBuf {
    app.path().app_config_dir()
        .map(|d| d.join("mcp-config.json"))
        .unwrap_or_else(|_| std::env::temp_dir().join("mcp-config.json"))
}

pub fn load_from(path: &std::path::Path) -> McpConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_to(path: &std::path::Path, cfg: &McpConfig) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub fn generate_token() -> String {
    let bytes: [u8; 32] = rand::random();
    format!("rtok_{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct McpTemplates {
    pub claude_code: String,
    pub claude_desktop: String,
    pub opencode: String,
    pub codex: String,
}

pub fn build_templates(token: &str) -> McpTemplates {
    let url = format!("http://127.0.0.1:{MCP_PORT}/mcp");
    let bearer = format!("Bearer {token}");
    McpTemplates {
        claude_code: format!(
            "claude mcp add r-todo --transport http {url} --header \"Authorization: {bearer}\""
        ),
        claude_desktop: serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": { "r-todo": { "url": url, "headers": { "Authorization": bearer } } }
        })).expect("json"),
        opencode: serde_json::to_string_pretty(&serde_json::json!({
            "$schema": "https://opencode.ai/config.json",
            "mcp": { "r-todo": { "type": "http", "url": url, "headers": { "Authorization": bearer } } }
        })).expect("json"),
        codex: format!(
            "[mcp_servers.r-todo]\ntype = \"http\"\nurl = \"{url}\"\nheaders = {{ Authorization = \"{bearer}\" }}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_default() {
        let path = std::env::temp_dir().join(format!("rtodo_mcp_test_missing_{}", std::process::id()));
        let cfg = load_from(&path);
        assert!(!cfg.enabled);
        assert!(cfg.token.is_empty());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let path = std::env::temp_dir().join(format!("rtodo_mcp_test_round_{}", std::process::id()));
        let cfg = McpConfig { enabled: true, token: "rtok_abc123".into() };
        save_to(&path, &cfg).expect("save");
        let loaded = load_from(&path);
        assert_eq!(loaded, cfg);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn token_has_prefix_and_64_hex_chars() {
        let t = generate_token();
        assert!(t.starts_with("rtok_"));
        assert_eq!(t.len(), 5 + 64); // "rtok_" + 32 bytes hex
    }

    #[test]
    fn templates_embed_token_and_url() {
        let tpl = build_templates("rtok_testtoken");
        assert!(tpl.claude_code.contains("http://127.0.0.1:18765/mcp"));
        assert!(tpl.claude_code.contains("Bearer rtok_testtoken"));
        assert!(tpl.claude_desktop.contains("Bearer rtok_testtoken"));
        assert!(tpl.opencode.contains("Bearer rtok_testtoken"));
        assert!(tpl.codex.contains("Bearer rtok_testtoken"));
    }
}
```

- [ ] **Step 3: 运行测试确认失败（编译错误即失败）**

Run: `cd apps/desktop/src-tauri && cargo test mcp_config 2>&1 | tail -5`
Expected: `error[E0432]: unresolved import crate 'rand'`（rand 已声明但此文件未引用，实际失败点是文件里 `tauri`/`rand` 引用；若 `mod mcp_config` 未注册则先做 Step 4 再回来跑）

- [ ] **Step 4: 注册 mod + commands（lib.rs）**

`apps/desktop/src-tauri/src/lib.rs` 第 3-10 行 mod 区加一行：

```rust
mod mcp_config;
```

在 `mcp_config.rs` 末尾（`mod tests` 之前）追加 commands（**仅两个无状态命令**；启停联动命令在 Task 2 定义）：

```rust
#[tauri::command]
pub fn get_mcp_status(app: tauri::AppHandle) -> McpConfig {
    load_from(&config_path(&app))
}

#[tauri::command]
pub fn get_mcp_client_templates(app: tauri::AppHandle) -> Result<McpTemplates, String> {
    let cfg = load_from(&config_path(&app));
    if cfg.token.is_empty() {
        return Err("MCP 未启用，请先打开「MCP 服务器」开关".into());
    }
    Ok(build_templates(&cfg.token))
}
```

`lib.rs` 的 `invoke_handler`（第 119-127 行）加：

```rust
mcp_config::get_mcp_status,
mcp_config::get_mcp_client_templates,
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cd apps/desktop/src-tauri && cargo test mcp_config 2>&1 | tail -8`
Expected: `test result: ok. 4 passed`

- [ ] **Step 6: 编译全量确认**

Run: `cd apps/desktop/src-tauri && cargo check 2>&1 | tail -3`
Expected: `Finished`（无错误）

- [ ] **Step 7: 提交**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/mcp_config.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): MCP 配置管理（开关/token/agent 模板 commands）+ 单元测试"
```

---

### Task 2: HTTP MCP Server 骨架（axum + rmcp + 认证 + 启停）

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp_server.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`（rmcp / axum / tokio-util / tokio net）
- Modify: `apps/desktop/src-tauri/src/lib.rs`（state + setup 启动 + command 注册）

**Interfaces:**
- Consumes: `mcp_config::{McpConfig, config_path}`（Task 1）；`AppHandle` 用于 `app.emit`（Task 3 才用）。
- Produces:
  - `pub struct McpHandler { pub db_path: std::path::PathBuf, pub notify: std::sync::Arc<dyn Fn() + Send + Sync> }`（`Clone`；Task 3 在此实现 tools）
  - `pub fn start_mcp(app: &tauri::AppHandle) -> Result<(), String>`（读 config，enabled 才起；已运行则忽略）
  - `pub fn stop_mcp(app: &tauri::AppHandle)`
  - `pub fn mcp_running(app: &tauri::AppHandle) -> bool`
  - `pub fn set_mcp_token(app: &tauri::AppHandle, token: &str)`（运行中 server 的 token 槽更新，供 rotate 用）
  - `pub struct McpState`（内部，`app.manage()` 管理）

- [ ] **Step 1: 加依赖**

`apps/desktop/src-tauri/Cargo.toml` `[dependencies]` 追加：

```toml
# MCP server（streamable HTTP）
rmcp = { version = "3.3", features = ["transport-streamable-http-server"] }
axum = "0.8"
tokio-util = { version = "0.7", features = ["sync"] }
rusqlite = { version = "0.40", features = ["bundled"] }
```

`[dependencies]` 里现有 `tokio` 行改为（追加 net feature）：

```toml
tokio = { version = "1", features = ["rt-multi-thread", "sync", "time", "macros", "net"] }
```

`[dev-dependencies]` 新增：

```toml
[dev-dependencies]
reqwest = { version = "0.12", default-features = false }
```

- [ ] **Step 2: 写失败集成测试（认证 + initialize 握手）**

新建 `apps/desktop/src-tauri/src/mcp_server.rs`（骨架 + 测试一起写，测试先跑失败）：

```rust
use std::{path::PathBuf, sync::{Arc, RwLock}};

use axum::{
    Router,
    http::{Request, StatusCode, header::AUTHORIZATION},
    middleware::{from_fn, Next},
    response::Response,
};
use rmcp::{RoleServer, ServerHandler, model::{ServerInfo, ServerCapabilities}};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::NeverSessionManager,
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
        ServerInfo {
            instructions: Some("r-todo 待办：列出、创建、编辑、完成、删除任务".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
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
async fn auth_middleware<B>(
    req: Request<B>,
    next: Next<B>,
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
    let config = StreamableHttpServerConfig::default()
        .with_cancellation_token(cancel)
        .with_json_response(true);
    let service = StreamableHttpService::<McpHandler, NeverSessionManager>::new(
        || Ok(handler.clone()),
        NeverSessionManager::default(),
        config,
    );
    // route_service + layer：layer 对 route 生效是 axum 的确定行为（fallback 不受 layer 影响，故不用 fallback）
    let router = Router::new()
        .route_service("/mcp", service)
        .layer(from_fn(move |req, next| auth_middleware(req, next, token_slot.clone())));
    axum::serve(listener, router).await.map_err(|e| e.to_string())?;
    Ok(())
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
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cd apps/desktop/src-tauri && cargo test mcp_server 2>&1 | tail -8`
Expected: 编译错误（`serve_mcp` 等未定义）或测试失败——此时 `mod mcp_server` 尚未注册，先做 Step 5 再跑。

- [ ] **Step 4: 注册 mod + state + 启停 + setup**

`lib.rs` mod 区加 `mod mcp_server;`。state 定义与启停逻辑放在 `mcp_server.rs` 中（`serve_mcp` 之后、`#[cfg(test)] mod tests` 之前）：

```rust
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
    if let Some(h) = state.handle.lock().expect("state").take() {
        state.cancel.cancel();
        h.abort();
    }
}

pub fn mcp_running(app: &tauri::AppHandle) -> bool {
    let state = app.state::<McpState>();
    state.handle.lock().expect("state").is_some()
}

/// rotate 后热更新运行中 server 的 token
pub fn set_mcp_token(app: &tauri::AppHandle, token: &str) {
    let state = app.state::<McpState>();
    *state.token_slot.write().expect("slot") = token.to_string();
}
```

`lib.rs` 的 `builder.setup(|app| ...)` 里（`let _ = window;` 之前）加：

```rust
app.manage(mcp_server::McpState::default());
if let Err(e) = mcp_server::start_mcp(app.handle()) {
    eprintln!("[mcp] start failed: {e}");
}
```

`invoke_handler` 追加（Task 2 阶段注册启停 command，供 Task 5 前端使用）：

```rust
mcp_server::set_mcp_enabled_cmd,
mcp_server::rotate_mcp_token_cmd,
```

在 `mcp_server.rs`（`mod tests` 之前）定义这两个 command——它们要联动启停，故不放 config 侧：

```rust
#[tauri::command]
pub fn set_mcp_enabled(app: tauri::AppHandle, enabled: bool) -> Result<crate::mcp_config::McpConfig, String> {
    let mut cfg = crate::mcp_config::load_from(&crate::mcp_config::config_path(&app));
    cfg.enabled = enabled;
    if enabled && cfg.token.is_empty() {
        cfg.token = crate::mcp_config::generate_token();
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
```

`lib.rs` 的 `invoke_handler` 追加这两个 command（连同 Task 1 的两个，共注册 4 个 MCP command）：

- [ ] **Step 5: 运行测试**

Run: `cd apps/desktop/src-tauri && cargo test mcp_server 2>&1 | tail -8`
Expected: `test result: ok. 3 passed`（401 ×2 + initialize 成功）

- [ ] **Step 6: 全量编译**

Run: `cd apps/desktop/src-tauri && cargo check 2>&1 | tail -3`
Expected: `Finished`

- [ ] **Step 7: 提交**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/mcp_server.rs apps/desktop/src-tauri/src/mcp_config.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): MCP HTTP server 骨架（rmcp streamable HTTP + Bearer 认证 + 启停管理）+ 集成测试"
```

---

### Task 3: 数据层 + 6 个工具（rusqlite + handler 接线 + 队列 user 注入）

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp_tools.rs`
- Modify: `apps/desktop/src-tauri/src/mcp_server.rs`（handler 实现 list_tools/call_tool）
- Modify: `apps/desktop/src-tauri/src/lib.rs`（`mod mcp_tools;`）
- Modify: `apps/desktop/src/lib/sync.ts`（flush 注入 user）

**Interfaces:**
- Consumes: 前端 `db.ts` 的 SQL 语义（`sort_order` 置顶 = `MIN(sort_order)-1000`、软删、`pending_ops` 结构）；`McpHandler`（Task 2，含 `db_path` / `notify`）。
- Produces:
  - `pub struct TaskJson { id, title, priority, remind_at, remind_offset_min, repeat_rule, sort_order, deleted, completed_at, created_at, updated_at }`（`Serialize`，字段与前端 `types.ts` 一致）
  - `pub fn open_db(path: &std::path::Path) -> Result<rusqlite::Connection, String>`（WAL + busy_timeout 5000）
  - `pub fn list_tasks(conn: &rusqlite::Connection, filter: &str, keyword: Option<&str>, priority: Option<&str>) -> Result<Vec<TaskJson>, String>`
  - `pub fn create_task(conn: &mut rusqlite::Connection, title: &str, priority: Option<&str>, remind_at: Option<&str>, remind_offset_min: Option<i64>, repeat_rule: Option<&str>) -> Result<TaskJson, String>`
  - `pub fn update_task(conn: &mut rusqlite::Connection, id: &str, title: Option<&str>, priority: Option<&str>, remind_at: Option<&str>, repeat_rule: Option<&str>) -> Result<TaskJson, String>`
  - `pub fn set_completed(conn: &mut rusqlite::Connection, id: &str, completed: bool) -> Result<TaskJson, String>`
  - `pub fn soft_delete(conn: &mut rusqlite::Connection, id: &str) -> Result<(), String>`
  - 内部 `fn push_pending(conn: &rusqlite::Connection, op_type: &str, record_id: &str, payload: Option<&serde_json::Value>) -> Result<(), String>`（payload 用 RemoteTask 字段名，不含 `user`）
  - `pub fn now_iso() -> String`

- [ ] **Step 1: 写失败测试（数据层全语义）**

新建 `apps/desktop/src-tauri/src/mcp_tools.rs`（结构 + 测试）：

```rust
use std::path::Path;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskJson {
    pub id: String,
    pub title: String,
    pub priority: String,
    pub remind_at: Option<String>,
    pub remind_offset_min: i64,
    pub repeat_rule: Option<String>,
    pub sort_order: f64,
    pub deleted: bool,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn open_db(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_millis(5000)).map_err(|e| e.to_string())?;
    Ok(conn)
}

/// 用与应用一致的 schema 建表（对齐 src-tauri/src/db.rs migrations 1-3）
pub fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL CHECK(length(title) > 0),
    priority TEXT NOT NULL CHECK(priority IN ('high', 'medium', 'low')),
    remind_at TEXT,
    remind_offset_min INTEGER NOT NULL DEFAULT 15,
    repeat_rule TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_completed ON tasks(completed_at);
CREATE INDEX IF NOT EXISTS idx_tasks_updated ON tasks(updated_at);
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS sort_order REAL NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS deleted INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS deleted_at TEXT;
CREATE INDEX IF NOT EXISTS idx_tasks_deleted ON tasks(deleted);
CREATE TABLE IF NOT EXISTS pending_ops (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    op_type TEXT NOT NULL CHECK(op_type IN ('create', 'update', 'delete')),
    collection TEXT NOT NULL DEFAULT 'tasks',
    record_id TEXT NOT NULL,
    payload TEXT,
    created_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
CREATE INDEX IF NOT EXISTS idx_pending_ops_fifo ON pending_ops(id);
"#,
    )
    .map_err(|e| e.to_string())
}

fn push_pending(conn: &Connection, op_type: &str, record_id: &str, payload: Option<&serde_json::Value>) -> Result<(), String> {
    conn.execute(
        "INSERT INTO pending_ops (op_type, collection, record_id, payload, created_at)
         VALUES (?1, 'tasks', ?2, ?3, ?4)",
        rusqlite::params![
            op_type,
            record_id,
            payload.map(|p| p.to_string()),
            chrono::Utc::now().timestamp_millis()
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 未完成任务里最小的 sort_order；空表返回 0（对齐前端 db.ts getMinSortOrder）
fn min_sort_order(conn: &Connection) -> Result<f64, String> {
    conn.query_row(
        "SELECT COALESCE(MIN(sort_order), 0) FROM tasks WHERE completed_at IS NULL AND deleted = 0",
        [],
        |row| row.get::<_, f64>(0),
    )
    .map_err(|e| e.to_string())
}

fn row_to_task(row: &rusqlite::Row) -> Result<TaskJson, rusqlite::Error> {
    Ok(TaskJson {
        id: row.get(0)?,
        title: row.get(1)?,
        priority: row.get(2)?,
        remind_at: row.get(3)?,
        remind_offset_min: row.get(4)?,
        repeat_rule: row.get(5)?,
        sort_order: row.get(6)?,
        deleted: row.get::<_, i64>(7)? != 0,
        completed_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

const SELECT_TASK: &str = "SELECT id, title, priority, remind_at, remind_offset_min, repeat_rule,
    sort_order, deleted, completed_at, created_at, updated_at FROM tasks WHERE id = ?1";

pub fn list_tasks(conn: &Connection, filter: &str, keyword: Option<&str>, priority: Option<&str>) -> Result<Vec<TaskJson>, String> {
    let mut sql = String::from(
        "SELECT id, title, priority, remind_at, remind_offset_min, repeat_rule,
                sort_order, deleted, completed_at, created_at, updated_at FROM tasks WHERE deleted = 0",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    match filter {
        "completed" => sql.push_str(" AND completed_at IS NOT NULL"),
        "all" => {}
        _ => sql.push_str(" AND completed_at IS NULL"),
    }
    if let Some(kw) = keyword {
        params.push(Box::new(format!("%{}%", kw.replace('%', "\\%").replace('_', "\\_"))));
        sql.push_str(" AND title LIKE ? ESCAPE '\\'");
    }
    if let Some(p) = priority {
        params.push(Box::new(p.to_string()));
        sql.push_str(" AND priority = ?");
    }
    sql.push_str(" ORDER BY CASE WHEN completed_at IS NULL THEN 0 ELSE 1 END,
        CASE WHEN completed_at IS NULL THEN sort_order ELSE NULL END ASC,
        CASE WHEN completed_at IS NOT NULL THEN completed_at ELSE NULL END DESC,
        created_at ASC");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref() as &dyn rusqlite::ToSql).collect();
    let rows = stmt.query_map(refs.as_slice(), row_to_task).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn create_task(
    conn: &mut Connection,
    title: &str,
    priority: Option<&str>,
    remind_at: Option<&str>,
    remind_offset_min: Option<i64>,
    repeat_rule: Option<&str>,
) -> Result<TaskJson, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("title 不能为空".into());
    }
    let priority = priority.unwrap_or("medium").to_string();
    if !["high", "medium", "low"].contains(&priority.as_str()) {
        return Err(format!("priority 非法: {priority}"));
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_iso();
    let sort_order = min_sort_order(&tx)? - 1000.0;
    let remind_offset = remind_offset_min.unwrap_or(15);
    tx.execute(
        "INSERT INTO tasks (id, title, priority, remind_at, remind_offset_min, repeat_rule,
            sort_order, deleted, completed_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10)",
        rusqlite::params![id, title, priority, remind_at, remind_offset, repeat_rule, sort_order, Option::<String>::None, now, now],
    )
    .map_err(|e| e.to_string())?;
    let payload = serde_json::json!({
        "id": id, "title": title, "priority": priority,
        "remind_at": remind_at.unwrap_or(""), "remind_offset_min": remind_offset,
        "repeat_rule": repeat_rule.unwrap_or(""),
        "sort_order": sort_order, "deleted": false,
        "completed_at": ""
    });
    push_pending(&tx, "create", &id, Some(&payload))?;
    tx.commit().map_err(|e| e.to_string())?;
    get_by_id(conn, &id)
}

fn get_by_id(conn: &Connection, id: &str) -> Result<TaskJson, String> {
    conn.query_row(SELECT_TASK, [id], row_to_task)
        .map_err(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
            format!("任务不存在: {id}")
        } else {
            e.to_string()
        })
}

pub fn update_task(
    conn: &mut Connection,
    id: &str,
    title: Option<&str>,
    priority: Option<&str>,
    remind_at: Option<&str>,
    repeat_rule: Option<&str>,
) -> Result<TaskJson, String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let now = now_iso();
    let cur: TaskJson = get_by_id(&tx, id)?;
    if cur.deleted {
        return Err(format!("任务不存在: {id}"));
    }
    let new_title = title.unwrap_or(cur.title.as_str()).trim().to_string();
    if new_title.is_empty() {
        return Err("title 不能为空".into());
    }
    let new_priority = priority.unwrap_or(cur.priority.as_str()).to_string();
    if !["high", "medium", "low"].contains(&new_priority.as_str()) {
        return Err(format!("priority 非法: {new_priority}"));
    }
    let new_remind = remind_at.map(|s| s.to_string()).or(cur.remind_at.clone());
    let new_repeat = repeat_rule.map(|s| s.to_string()).or(cur.repeat_rule.clone());
    tx.execute(
        "UPDATE tasks SET title = ?1, priority = ?2, remind_at = ?3, repeat_rule = ?4, updated_at = ?5
         WHERE id = ?6 AND deleted = 0",
        rusqlite::params![new_title, new_priority, new_remind, new_repeat, now, id],
    )
    .map_err(|e| e.to_string())?;
    let payload = serde_json::json!({
        "id": id, "title": new_title, "priority": new_priority,
        "remind_at": new_remind.unwrap_or_default(), "remind_offset_min": cur.remind_offset_min,
        "repeat_rule": new_repeat.unwrap_or_default(),
        "sort_order": cur.sort_order, "deleted": false,
        "completed_at": cur.completed_at.unwrap_or_default()
    });
    push_pending(&tx, "update", id, Some(&payload))?;
    tx.commit().map_err(|e| e.to_string())?;
    get_by_id(conn, id)
}

pub fn set_completed(conn: &mut Connection, id: &str, completed: bool) -> Result<TaskJson, String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let now = now_iso();
    let cur: TaskJson = get_by_id(&tx, id)?;
    if cur.deleted {
        return Err(format!("任务不存在: {id}"));
    }
    let completed_at = if completed { Some(now.clone()) } else { None };
    tx.execute(
        "UPDATE tasks SET completed_at = ?1, updated_at = ?2 WHERE id = ?3 AND deleted = 0",
        rusqlite::params![completed_at, now, id],
    )
    .map_err(|e| e.to_string())?;
    let payload = serde_json::json!({
        "id": id, "title": cur.title, "priority": cur.priority,
        "remind_at": cur.remind_at.unwrap_or_default(), "remind_offset_min": cur.remind_offset_min,
        "repeat_rule": cur.repeat_rule.unwrap_or_default(),
        "sort_order": cur.sort_order, "deleted": false,
        "completed_at": completed_at.unwrap_or_default()
    });
    push_pending(&tx, "update", id, Some(&payload))?;
    tx.commit().map_err(|e| e.to_string())?;
    get_by_id(conn, id)
}

pub fn soft_delete(conn: &mut Connection, id: &str) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let now = now_iso();
    let cur: TaskJson = get_by_id(&tx, id)?;
    if cur.deleted {
        return Err(format!("任务不存在: {id}"));
    }
    tx.execute(
        "UPDATE tasks SET deleted = 1, deleted_at = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![now, now, id],
    )
    .map_err(|e| e.to_string())?;
    push_pending(&tx, "delete", id, None)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let path = std::env::temp_dir().join(format!("rtodo_mcp_tools_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = open_db(&path).expect("open");
        init_schema(&conn).expect("schema");
        conn
    }

    #[test]
    fn create_lists_and_sorts_newest_first() {
        let mut conn = test_conn();
        let a = create_task(&mut conn, "任务A", None, None, None, None).expect("a");
        let b = create_task(&mut conn, "任务B", Some("high"), None, None, None).expect("b");
        let all = list_tasks(&conn, "active", None, None).expect("list");
        assert_eq!(all.len(), 2);
        // 后创建的 sort_order 更小 → 排最前
        assert_eq!(all[0].id, b.id);
        assert_eq!(all[0].priority, "high");
        assert_eq!(all[1].id, a.id);
        // pending_ops 有 create 记录
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM pending_ops", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        let _ = a;
    }

    #[test]
    fn complete_and_uncomplete() {
        let mut conn = test_conn();
        let t = create_task(&mut conn, "完成我", None, None, None, None).expect("t");
        let done = set_completed(&mut conn, &t.id, true).expect("done");
        assert!(done.completed_at.is_some());
        let active = list_tasks(&conn, "active", None, None).expect("active");
        assert!(active.is_empty());
        let done_list = list_tasks(&conn, "completed", None, None).expect("done list");
        assert_eq!(done_list.len(), 1);
        let undone = set_completed(&mut conn, &t.id, false).expect("undone");
        assert!(undone.completed_at.is_none());
    }

    #[test]
    fn update_changes_fields_and_filters_by_keyword() {
        let mut conn = test_conn();
        let t = create_task(&mut conn, "买牛奶", None, None, None, None).expect("t");
        let up = update_task(&mut conn, &t.id, Some("买豆浆"), Some("low"), None, Some("FREQ=DAILY"))
            .expect("up");
        assert_eq!(up.title, "买豆浆");
        assert_eq!(up.priority, "low");
        assert_eq!(up.repeat_rule.as_deref(), Some("FREQ=DAILY"));
        let found = list_tasks(&conn, "active", Some("豆浆"), None).expect("kw");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, t.id);
    }

    #[test]
    fn soft_delete_removes_from_list_and_queues() {
        let mut conn = test_conn();
        let t = create_task(&mut conn, "删我", None, None, None, None).expect("t");
        soft_delete(&mut conn, &t.id).expect("del");
        assert!(list_tasks(&conn, "all", None, None).expect("list").is_empty());
        let row: Option<String> = conn
            .query_row(
                "SELECT op_type FROM pending_ops WHERE record_id = ?1 AND op_type = 'delete'",
                [&t.id],
                |r| r.get(0),
            )
            .ok();
        assert_eq!(row.as_deref(), Some("delete"));
    }

    #[test]
    fn empty_title_and_unknown_id_are_errors() {
        let mut conn = test_conn();
        assert!(create_task(&mut conn, "   ", None, None, None, None).is_err());
        assert!(update_task(&mut conn, "no-such-id", None, None, None, None).is_err());
        assert!(set_completed(&mut conn, "no-such-id", true).is_err());
        assert!(soft_delete(&mut conn, "no-such-id").is_err());
    }

    #[test]
    fn filter_priority_works() {
        let mut conn = test_conn();
        create_task(&mut conn, "高", Some("high"), None, None, None).expect("h");
        create_task(&mut conn, "低", Some("low"), None, None, None).expect("l");
        let highs = list_tasks(&conn, "active", None, Some("high")).expect("high");
        assert_eq!(highs.len(), 1);
        assert_eq!(highs[0].title, "高");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd apps/desktop/src-tauri && cargo test mcp_tools 2>&1 | tail -8`
Expected: 编译失败（函数未实现，此时 `mod mcp_tools` 未注册先做 Step 4）

- [ ] **Step 3: 注册 mod**

`lib.rs` mod 区加 `mod mcp_tools;`。

- [ ] **Step 4: 运行测试至通过**

Run: `cd apps/desktop/src-tauri && cargo test mcp_tools 2>&1 | tail -8`
Expected: `test result: ok. 6 passed`。若有失败：优先核对 `create_task` 的 INSERT 参数与占位符一一对应（此步的已知易错点）。

- [ ] **Step 5: handler 接线（mcp_server.rs 实现 tools）**

`mcp_server.rs` 的 `impl ServerHandler for McpHandler` 追加 `list_tools` 与 `call_tool`：

```rust
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult, Tool,
};
use rmcp::service::RequestContext;
use rmcp::handler::server::tool::parse_json_object;
use rmcp::{ErrorData as McpError, ErrorCode};
use serde::Deserialize;
use schemars::JsonSchema;

/// 基础设施错误（db 打开/查询失败）→ 协议级错误
fn infra(e: String) -> McpError {
    McpError::new(ErrorCode::INTERNAL_ERROR, e, None)
}

// ── 工具输入结构（schemars 自动生成 JSON Schema） ──
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTasksInput {
    /// active=未完成（默认）| completed=已完成 | all=全部
    pub filter: Option<String>,
    /// 标题包含关键字
    pub keyword: Option<String>,
    /// 按优先级过滤: high | medium | low
    pub priority: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
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
pub struct IdInput {
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateTaskInput {
    pub id: String,
    pub title: Option<String>,
    pub priority: Option<String>,
    /// 传 null 清除提醒
    pub remind_at: Option<String>,
    pub repeat_rule: Option<String>,
}

fn tool_def(name: &'static str, title: &str, description: &str) -> Tool {
    let schema = match name {
        "list_tasks" => schemars::schema_for!(ListTasksInput),
        "create_task" => schemars::schema_for!(CreateTaskInput),
        "update_task" => schemars::schema_for!(UpdateTaskInput),
        "complete_task" | "uncomplete_task" | "delete_task" => schemars::schema_for!(IdInput),
        _ => unreachable!(),
    };
    Tool {
        name: name.into(),
        title: Some(title.into()),
        description: Some(description.into()),
        input_schema: Arc::new(serde_json::from_value(serde_json::to_value(schema).expect("schema json")).expect("json object")),
        ..Default::default()
    }
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
```

然后在 `impl ServerHandler for McpHandler` 块内追加两个方法（放在 `get_info` 之后）：

```rust
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
                    match crate::mcp_tools::update_task(
                        &mut conn,
                        &input.id,
                        input.title.as_deref(),
                        input.priority.as_deref(),
                        input.remind_at.as_deref(),
                        input.repeat_rule.as_deref(),
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
```

`parse_json_object` 返回 `Result<T, ErrorData>`，`?` 直接透传协议错误 ✓。`CallToolResult::structured(value)` 返回带结构化内容的成功结果（已核实 rmcp 3.3.0 `model.rs:4030`）✓。`CallToolResponse` 由 `From<CallToolResult>` 得到（已核实 `model/mrtr.rs:115`）✓。`ListToolsResult { tools, ..Default::default() }` 是 `paginated_result!` 宏生成的（含 next_cursor）✓。`ErrorData::new(ErrorCode::INTERNAL_ERROR, msg, None)`（已核实 `model.rs:673`）✓。

注意：`call_tool` 的 `_context` 参数保持下划线命名（未使用）。

`mcp_server.rs` 顶部 use 区需要补充（追加到现有 use 之后）：

```rust
use rmcp::handler::server::tool::parse_json_object;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, ErrorCode};
use schemars::JsonSchema;
use serde::Deserialize;
```

`McpHandler` 顶部已 `use std::sync::{Arc, RwLock}`（Task 2 修订后），工具定义中的 `Arc::new` 直接可用，无需别名。

`mcp_server.rs` 的 `#[cfg(test)] mod tests`（Task 2 已有）末尾追加工具定义测试：

```rust
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
```

- [ ] **Step 6（原 Step 6）: 编译 + 跑全部 Rust 测试**

- [ ] **Step 6: 编译 + 跑全部 Rust 测试**

Run: `cd apps/desktop/src-tauri && cargo test 2>&1 | tail -10`
Expected: 全部通过（mcp_config 4 + mcp_server 4 + mcp_tools 6）

- [ ] **Step 7: 前端 sync.ts 补 user 注入（3 行）**

`apps/desktop/src/lib/sync.ts` 的 `flushPendingOps`（第 203-211 行循环内），在 `if (op.op_type === "create")` 分支改为：

```ts
if (op.op_type === "create") {
  const payload = JSON.parse(op.payload ?? "{}") as Record<string, unknown>;
  // MCP 等外部写入的队列条目不含 user，这里补上当前登录用户
  payload.user = pb.authStore.record.id;
  await pb.collection("tasks").create(payload);
}
```

- [ ] **Step 8: 前端编译确认 + 提交**

Run: `cd apps/desktop && pnpm build 2>&1 | tail -5`
Expected: `vite build` 成功

```bash
git add apps/desktop/src-tauri/src/mcp_tools.rs apps/desktop/src-tauri/src/mcp_server.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src/lib/sync.ts
git commit -m "feat(desktop): MCP 数据层 + 6 个工具（rusqlite + pending_ops 队列，flush 注入 user）"
```

---

### Task 4: 前端刷新链路（refreshFromDb 抽取 + tasks-changed 订阅）

**Files:**
- Modify: `apps/desktop/src/lib/store.ts`

**Interfaces:**
- Consumes: Rust 侧 `app.emit("tasks-changed")`（Task 2/3）；现有 `hydrate` 中的重拉逻辑（store.ts:41-62）。
- Produces: `refreshFromDb(): Promise<void>`（store 内部函数，hydrate 与事件回调共用）。

- [ ] **Step 1: 抽取 refreshFromDb**

`store.ts` 的 `hydrate`（第 41-62 行）中，将「重拉 + 重排提醒」逻辑抽出为模块内函数，并让 `hydrate` 与远端变更回调共用。`listen` 的 import 加在文件顶部 import 区（与 `./db` 等并列）：

```ts
import { listen } from "@tauri-apps/api/event";
```

模块内（`useTaskStore` 定义之前）新增：

```ts
// hydrate 与 MCP/远端变更共用的重拉逻辑
async function refreshFromDb(set: (fn: (s: TaskState) => void) => void) {
  const list = await db.listTasks();
  set((s) => {
    s.tasks = list;
  });
  // 重新排队所有未完成的提醒
  for (const t of list) {
    if (!t.completed_at && t.remind_at) {
      rescheduleReminder(t).catch(() => {
        /* ignore */
      });
    }
  }
}
```

`hydrate` 改造为：

```ts
hydrate: async () => {
  if (get().loaded) return;
  set((s) => {
    s.loading = true;
  });
  await refreshFromDb(set);
  set((s) => {
    s.loading = false;
    s.loaded = true;
  });

  // 远端有变更时重拉本地
  onRemoteChange(() => refreshFromDb(set));
},
```

注意：`rescheduleReminder` 的 import 已在 store.ts 顶部（第 6 行 `import { rescheduleReminder, cancelReminder } from "./reminder";`）✓。

- [ ] **Step 2: 订阅 tasks-changed**

`store.ts` 的 `hydrate` 内、`onRemoteChange` 订阅旁追加：

```ts
// MCP（AI 写入）后 Rust 侧通知重拉
listen("tasks-changed", () => refreshFromDb(set)).catch(() => {
  /* 非 Tauri 环境忽略 */
});
```

- [ ] **Step 3: 编译 + 手动验证**

Run: `cd apps/desktop && pnpm build 2>&1 | tail -5`
Expected: build 成功

手动验证（需先完成 Task 5 才能从 UI 开关；本任务内可用临时手段：在 DevTools console 里 `await window.__TAURI__?.event.emit?.("tasks-changed")` 或直接改 config 文件 + 重启应用）：
1. 启动 `pnpm tauri dev`，新增一条任务 → UI 正常。
2. 手工修改 `%APPDATA%\dev.rtodo.desktop\r-todo.db`（SQLite 插入一条）后触发 `tasks-changed`（console 模拟）→ UI 立即出现该任务。

- [ ] **Step 4: 提交**

```bash
git add apps/desktop/src/lib/store.ts
git commit -m "feat(desktop): 抽取 refreshFromDb 并订阅 MCP tasks-changed 事件"
```

---

### Task 5: 设置面板 UI（开关 / token / agent 模板）

**Files:**
- Create: `apps/desktop/src/lib/mcp.ts`
- Modify: `apps/desktop/src/components/settings/SettingsPanel.tsx`

**Interfaces:**
- Consumes: commands `get_mcp_status` / `set_mcp_enabled` / `rotate_mcp_token` / `get_mcp_client_templates`（Rust，Task 1/2）；`McpTemplates { claude_code, claude_desktop, opencode, codex }`。
- Produces: 无（终端组件）。

- [ ] **Step 1: 前端 invoke 封装**

新建 `apps/desktop/src/lib/mcp.ts`：

```ts
import { invoke } from "@tauri-apps/api/core";

export interface McpStatus {
  enabled: boolean;
  token: string;
}

export interface McpTemplates {
  claude_code: string;
  claude_desktop: string;
  opencode: string;
  codex: string;
}

export const MCP_URL = "http://127.0.0.1:18765/mcp";

export async function getMcpStatus(): Promise<McpStatus> {
  return invoke("get_mcp_status");
}

export async function setMcpEnabled(enabled: boolean): Promise<McpStatus> {
  return invoke("set_mcp_enabled", { enabled });
}

export async function rotateMcpToken(): Promise<McpStatus> {
  return invoke("rotate_mcp_token");
}

export async function getMcpTemplates(): Promise<McpTemplates> {
  return invoke("get_mcp_client_templates");
}

/** 复制到剪贴板（WebView2，用户手势上下文可用） */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
```

- [ ] **Step 2: SettingsPanel 状态与开关**

`SettingsPanel.tsx`：
- import 区追加 `import { MCP_URL, getMcpStatus, setMcpEnabled, rotateMcpToken, getMcpTemplates, copyText, type McpTemplates } from "../../lib/mcp";`
- 状态区（第 60 行附近）追加：

```ts
const [mcpEnabled, setMcpEnabledState] = useState(false);
const [mcpToken, setMcpToken] = useState("");
const [mcpTemplates, setMcpTemplates] = useState<McpTemplates | null>(null);
const [mcpError, setMcpError] = useState<string | null>(null);
const [mcpBusy, setMcpBusy] = useState(false);
const [activeTemplate, setActiveTemplate] = useState<keyof McpTemplates>("claude_code");
```

- `useEffect(() => { if (open) getMcpStatus().then((s) => { setMcpEnabledState(s.enabled); setMcpToken(s.token); }); }, [open]);`
- 开关回调（复用现有 `busy` 模式）：

```ts
const toggleMcp = async () => {
  setMcpBusy(true);
  setMcpError(null);
  try {
    const s = await setMcpEnabled(!mcpEnabled);
    setMcpEnabledState(s.enabled);
    setMcpToken(s.token);
    if (s.enabled) {
      setMcpTemplates(await getMcpTemplates());
    } else {
      setMcpTemplates(null);
    }
  } catch (e) {
    setMcpError(String(e));
  } finally {
    setMcpBusy(false);
  }
};

const doRotateToken = async () => {
  setMcpBusy(true);
  setMcpError(null);
  try {
    const s = await rotateMcpToken();
    setMcpToken(s.token);
    setMcpTemplates(await getMcpTemplates());
    setToast("token 已重置，请在已配置的 agent 处同步更新");
  } catch (e) {
    setMcpError(String(e));
  } finally {
    setMcpBusy(false);
  }
};
```

- [ ] **Step 3: UI 区块**

在「吸附后收起为边条」Row（第 210-220 行）之后插入：

```tsx
<Row
  label="MCP 服务器"
  desc="允许 AI（Claude、opencode、codex 等）读取和修改待办"
  checked={mcpEnabled}
  onToggle={toggleMcp}
  disabled={busy || mcpBusy}
/>

{/* MCP 详情（开启后展开） */}
{mcpEnabled && (
  <div className="rounded-lg bg-black/5 px-3 py-2.5 dark:bg-white/5">
    <div className="text-[11px] text-neutral-700 dark:text-neutral-200">
      地址 <span className="font-mono text-[10px]">{MCP_URL}</span>
    </div>
    <div className="mt-1.5 flex items-center gap-1.5">
      <span className="font-mono text-[10px] text-neutral-500">
        {mcpToken ? `${mcpToken.slice(0, 8)}****…${mcpToken.slice(-4)}` : "…"}
      </span>
      <button
        onClick={async () => {
          if (await copyText(mcpToken)) setToast("token 已复制");
          else setMcpError("复制失败，请手动复制");
        }}
        className="rounded bg-black/5 px-2 py-0.5 text-[10px] hover:bg-black/10 dark:bg-white/5 dark:hover:bg-white/10"
      >
        复制 token
      </button>
      <button
        onClick={doRotateToken}
        disabled={mcpBusy}
        className="rounded bg-black/5 px-2 py-0.5 text-[10px] hover:bg-black/10 disabled:opacity-50 dark:bg-white/5 dark:hover:bg-white/10"
      >
        重置 token
      </button>
    </div>
    <div className="mt-2.5 text-[10px] font-medium uppercase tracking-wide text-neutral-400">
      配置模板
    </div>
    <div className="mt-1 flex flex-wrap gap-1">
      {(["claude_code", "claude_desktop", "opencode", "codex"] as const).map((k) => (
        <button
          key={k}
          onClick={() => setActiveTemplate(k)}
          className={cn(
            "rounded-full px-2 py-0.5 text-[10px] transition-colors",
            activeTemplate === k
              ? "bg-neutral-800 text-white dark:bg-white dark:text-neutral-900"
              : "text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5",
          )}
        >
          {k === "claude_code" ? "Claude Code" : k === "claude_desktop" ? "Claude Desktop" : k}
        </button>
      ))}
    </div>
    {mcpTemplates && (
      <div className="mt-1.5 space-y-1.5">
        <pre className="max-h-28 overflow-auto whitespace-pre-wrap rounded bg-white/60 p-2 font-mono text-[9px] text-neutral-600 dark:bg-black/30 dark:text-neutral-300">
          {mcpTemplates[activeTemplate]}
        </pre>
        <button
          onClick={async () => {
            if (mcpTemplates && (await copyText(mcpTemplates[activeTemplate]))) {
              setToast("模板已复制");
            } else {
              setMcpError("复制失败，请手动复制");
            }
          }}
          className="rounded bg-black/5 px-2 py-0.5 text-[10px] hover:bg-black/10 dark:bg-white/5 dark:hover:bg-white/10"
        >
          复制模板
        </button>
      </div>
    )}
    {mcpError && <div className="mt-1.5 text-[10px] text-red-500">{mcpError}</div>}
  </div>
)}
```

`MCP_URL` 已从 `../../lib/mcp` 导入（Step 1 的 import 行包含）。`cn` 已在 SettingsPanel import ✓。条件渲染直接用 `{mcpEnabled && (...)}`，无需 AnimatePresence 包装。

- [ ] **Step 4: 编译**

Run: `cd apps/desktop && pnpm build 2>&1 | tail -5`
Expected: build 成功

- [ ] **Step 5: 手动验证清单**

`pnpm tauri dev` 后逐项：
1. 设置 → 打开「MCP 服务器」→ 出现地址/token/模板；`%APPDATA%\dev.rtodo.desktop\mcp-config.json` 生成且 enabled=true。
2. `netstat -ano | findstr 18765` → 监听 `127.0.0.1:18765`。
3. 复制 token → 粘贴成功。
4. 「重置 token」→ token 变化、模板变化。
5. 关闭开关 → 端口停止监听（netstat 无 18765）。
6. 重新打开 → token 保持（不重新生成）。

- [ ] **Step 6: 提交**

```bash
git add apps/desktop/src/lib/mcp.ts apps/desktop/src/components/settings/SettingsPanel.tsx
git commit -m "feat(desktop): 设置面板 MCP 开关/token/agent 配置模板"
```

---

### Task 6: 端到端验证 + 文档

**Files:**
- Modify: `README.md`（MCP 使用说明）

- [ ] **Step 1: 全量质量门**

Run: `cd apps/desktop/src-tauri && cargo test 2>&1 | tail -5`
Expected: `test result: ok. 14 passed`（4 config + 4 server + 6 tools）

Run: `cd apps/desktop && pnpm build 2>&1 | tail -3`
Expected: build 成功

- [ ] **Step 2: 真机端到端（Claude Code）**

1. `pnpm tauri dev`，设置面板打开「MCP 服务器」，复制 Claude Code 模板命令并运行。
2. `claude mcp list` → 出现 `r-todo`（http）。
3. 本会话外另开一个 Claude Code 会话，输入：
   - 「列出我的待办」→ 工具 `list_tasks` 被调用，返回任务列表。
   - 「加一个待办：周五交周报，高优先级」→ `create_task`；桌面 UI 立即出现该任务（tasks-changed 刷新）。
   - 「把『买牛奶』标记完成」「删除『过期任务』」「把『周报』改成明天提醒」→ 对应工具。
   - 「我的待办还有哪些没完成」→ list_tasks(filter=active)。
4. 若已登录云同步：PocketBase 管理后台 tasks 集合出现 AI 创建/更新的记录（user 字段为当前用户）；手机端 Realtime 同步出现。

- [ ] **Step 3: 真机端到端（opencode / codex，若有安装）**

若本机装有 opencode / codex：按模板配置后各发起一次 `list_tasks` 验证。**若模板格式与该工具实际要求不符**（工具报 schema 错误），按报错修正 `mcp_config.rs::build_templates` 中的对应格式，并补充 `templates_embed_token_and_url` 测试断言后提交（这是模板格式兜底修正点，非占位）。

- [ ] **Step 4: README 文档**

`README.md` 追加一节（放在「文档」之前）：

```markdown
## AI 操作待办（MCP）

桌面端内置 MCP 服务器（默认关闭），让 Claude Code / Claude Desktop / opencode / codex 等 AI 直接读写待办。

启用：设置 →「MCP 服务器」→ 打开开关 → 复制对应 agent 的配置模板并执行一次（Claude Code 为 `claude mcp add` 命令）。

- 地址：`http://127.0.0.1:18765/mcp`（仅本机，Bearer token 认证）
- 工具：`list_tasks` / `create_task` / `update_task` / `complete_task` / `uncomplete_task` / `delete_task`
- AI 的写操作实时刷新桌面 UI，并进入现有同步链路（在线时自动同步到 PocketBase，手机端同步可见）
- 安全：开关默认关；token 在设置面板可重置；端口仅监听 127.0.0.1
```

- [ ] **Step 5: 提交**

```bash
git add README.md
git commit -m "docs: README 增加 MCP 使用说明"
```
