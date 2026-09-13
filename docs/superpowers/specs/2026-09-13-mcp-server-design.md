# r-todo 桌面端内置 MCP Server 设计

日期：2026-09-13
状态：待审阅

## 1. 背景与目标

r-todo 是跨端待办应用（Tauri 2 桌面端 + Flutter 移动端 + PocketBase 自托管同步）。用户希望**外部 AI（Claude Code / Claude Desktop）能够直接操作桌面端待办**：新建、编辑、完成、删除、查询。

硬性约束：

1. **应用安装后即带 MCP 能力**——不要求用户额外安装任何程序。
2. **设置面板可开关**——默认关闭，用户显式启用。

## 2. 需求

### 目标

- 桌面端内置 streamable HTTP MCP server，监听 `127.0.0.1:18765/mcp`，随应用进程启停。
- 设置面板新增「MCP 服务器」开关（默认关）；开启时展示 token 与**各主流 agent 的配置模板**（Claude Code / Claude Desktop / opencode / codex），一键复制。
- 暴露工具：`list_tasks` / `create_task` / `update_task` / `complete_task` / `uncomplete_task` / `delete_task`。
- AI 的写操作与 UI **实时一致**：Rust 侧写入后通过 Tauri 事件通知前端重拉，无需轮询。
- AI 的写操作**进入现有同步链路**：写入 `pending_ops` 队列，由现有 flush 逻辑同步到 PocketBase，手机端自动受益。
- 只读操作（list）不产生事件、不触发刷新。

### 非目标

- 不做 CLI 子命令（MCP 已覆盖，CLI 的解析脆弱性无收益）。
- 不做自动改写用户 `~/.claude.json` / `claude_desktop_config.json` 等任意 agent 全局配置（侵入且各 agent 格式升级易失效，只提供复制模板）。
- 不做 Claude Code 插件自动注册（多 agent 使用场景下收益仅覆盖单一客户端，模板复制已覆盖配置成本，不引入第二套机制）。
- 不做手机端 MCP（移动端无常驻进程场景）。
- 不做应用内嵌 AI 对话（独立需求，不在本迭代）。

## 3. 方案选型摘要

### MCP vs CLI → MCP

| 维度 | MCP | CLI |
|---|---|---|
| 参数可靠性 | 类型化 schema，AI 生成准确 | 自由文本解析，中文/引号易翻车 |
| 错误返回 | 结构化，AI 可继续处理 | exit code + stderr 解析，脆弱 |
| 生态 | Claude Code / Desktop / 其他客户端复用 | 仅能跑 shell 的 agent |
| 多操作 | 独立 tool 组合调用 | 子命令堆砌 + 输出解析 |

### 独立进程 vs 应用内置 → 应用内置

| | 独立 stdio 进程 | 应用内置 HTTP（选定） |
|---|---|---|
| 安装即带 | 需打包二进制 + 客户端注册命令 | 应用自带端点，注册只需 URL + token |
| 开关 | 管不到独立进程，只能让 tools 报错 | 进程不监听端口，天然干净 |
| UI 刷新 | 需轮询 | 进程内事件，实时且零开销 |
| 常驻要求 | 无 | 需应用运行——该应用本就开机自启 + 桌面常驻，是常态 |

## 4. 架构总览

```
┌─ Tauri 应用进程（dev.rtodo.desktop）──────────────────────────────┐
│                                                                    │
│  SettingsPanel（开关: MCP 服务器，默认关）                          │
│      │ set_mcp_enabled / get_mcp_status / rotate_token (command)  │
│      ▼                                                            │
│  Rust: mcp_config.rs（app_config_dir/mcp-config.json）             │
│      │                                                             │
│      ▼ 开关=开 → spawn                                          │
│  Rust: mcp_server.rs（rmcp + transport-streamable-http-server）    │
│      │  127.0.0.1:18765/mcp，Bearer token 认证                     │
│      ├── tools/list, tools/call → mcp_tools.rs                     │
│      │       └─ rusqlite 直写 app_config_dir/r-todo.db             │
│      │          （SQL 语义与前端 src/lib/db.ts 对齐）               │
│      ├── 写操作后 emit "tasks-changed"                             │
│      └── flush pending_ops（现有 sync.ts，无需改动）                │
│              ▲                                                    │
│  前端 store ─┘ 订阅事件 → refreshFromDb()：重拉 + 重排提醒          │
│                                                                    │
└──────────────┬─────────────────────────────────────────────────────┘
               │  HTTP (streamable)
Claude Code / Claude Desktop
```

数据流：AI 工具调用 → Rust SQL 写入（tasks + pending_ops）→ emit 事件 → 前端重拉 UI 一致；应用在线时 flush 队列 → PocketBase → Realtime → 手机端。

## 5. 组件详设

### 5.1 MCP HTTP Server（Rust，`src-tauri/src/mcp_server.rs`）

- 依赖：`rmcp = { version = "3.3", features = ["transport-streamable-http-server"] }`（已核实 crates.io 3.3.0，rust-version 1.88 ≤ 本机 1.97.1）。
- 端口固定 **18765**（单实例插件保证全机唯一进程，无冲突风险；固定端口让注册命令稳定可复制）。
- 生命周期：`setup` 中读取开关，开则 `tokio::spawn` 服务；关闭时通过 `tokio::sync::watch` 或 `CancellationToken` 通知停止。随进程退出自然终结。
- 认证：`Authorization: Bearer <token>`，恒时比较（ct_eq）。token 由 `mcp_config.rs` 管理（见 5.4）。
- 实现 `ServerHandler` trait 的 `list_tools` / `call_tool`。

### 5.2 数据访问层（Rust，`src-tauri/src/mcp_tools.rs`）

- 连接：rusqlite 打开 **`app.path().app_config_dir().join("r-todo.db")`** —— 已核实 tauri-plugin-sql 2.4.1 的 `wrapper.rs:79-86`，`sqlite:r-todo.db` 即解析为 `app_config_dir()`（Windows：`%APPDATA%\dev.rtodo.desktop\r-todo.db`）。同一文件，多连接共存。
- 连接 PRAGMA：`journal_mode=WAL`、`busy_timeout=5000`，支持多进程/多连接并发写。
- SQL 语义与前端 `src/lib/db.ts` **逐条对齐**：
  - `create_task`：`sort_order = MIN(sort_order) - 1000`（与 `getMinSortOrder` 同语义，新任务置顶）；INSERT tasks + INSERT pending_ops(create, payload=完整 Task JSON)。
  - `update_task`：UPDATE 指定字段 + updated_at=now（仅 `deleted=0`）；INSERT pending_ops(update, payload=最新 Task JSON)。
  - `complete_task` / `uncomplete_task`：UPDATE completed_at；INSERT pending_ops(update)。
  - `delete_task`：**软删**（UPDATE deleted=1, deleted_at=now）；INSERT pending_ops(delete)。
  - `list_tasks`：与前端 `listTasks` 排序一致（未完成按 sort_order asc，已完成按 completed_at desc，deleted=0 过滤）。
  - 每个写操作包裹在**事务**中（任务变更 + 队列写入原子）。
- 写操作不需要应用登录 PocketBase 也可执行（离线可用，队列等下次 flush —— 现有逻辑天然覆盖）。

### 5.3 变更通知（前端，`src/lib/store.ts` + 新增监听）

- Rust 写操作成功后 `app.emit("tasks-changed")`。
- 前端在 `store.ts` 的 `hydrate` 中订阅该事件（与现有 `onRemoteChange` 订阅并列），回调调用公共函数 `refreshFromDb()`：
  - 从现有 `hydrate` 中抽取：`listTasks()` → set tasks + 对新增未完成且带 remind_at 的任务 `rescheduleReminder`。
  - `hydrate` 与事件刷新共用该函数，消除重复逻辑。
- 只读工具不触发事件。

### 5.4 配置与开关（Rust，`src-tauri/src/mcp_config.rs`）

- 配置文件：`app_config_dir/mcp-config.json`，字段：`{ enabled: bool, token: string, port: 18765 }`。
- Tauri command（`invoke_handler` 注册）：
  - `get_mcp_status() -> { enabled, token_masked, port }`
  - `set_mcp_enabled(bool)` —— 开时若无 token 则生成（rand 32 字节 hex）；启停 server。
  - `rotate_token()` —— 主动重置（旧 token 立即失效）。**token 默认稳定不自动轮换**：多 agent 场景下已配好的客户端无需维护；只有用户主动「重置 token」才变更。
  - `get_mcp_client_templates()` —— 返回各 agent 的配置模板文本（见 5.5）。
- token 首次生成时机：开关开启时。token 不落 UI 日志（可脱敏展示）。

### 5.5 设置面板 UI（`src/components/settings/SettingsPanel.tsx`）

- 新增 `Row`：「MCP 服务器」，desc「允许 AI（Claude 等）读取和修改待办」，复用现有开关组件（含 `busy` 状态）。
- 开启后展示：
  - 状态行：`http://127.0.0.1:18765/mcp` + 端口占用/启动错误提示。
  - token：脱敏显示（`rtok_****…ab12`）+「复制」按钮。
  - 「配置模板」区：各 agent 模板，标签切换 + 一键复制（数据来自 `get_mcp_client_templates()`）：
    - Claude Code：`claude mcp add r-todo --transport http http://127.0.0.1:18765/mcp --header "Authorization: Bearer <token>"`
    - Claude Desktop：`claude_desktop_config.json` 的 `mcpServers` JSON 片段（url + headers）。
    - opencode / codex：各自配置文件的 `mcp` / `mcp_servers` 片段（实现时按官方文档格式逐项核实并硬编码模板）。
  - 「重置 token」按钮（重置后需在已配置的 agent 处同步更新，提示文案说明）。

## 6. MCP 工具契约

Task 对象 JSON 字段与前端 `src/lib/types.ts` 一致：

```json
{ "id": "…", "title": "…", "priority": "high|medium|low",
  "remind_at": "ISO|null", "remind_offset_min": 15,
  "repeat_rule": "RRULE|null", "sort_order": 0,
  "deleted": false, "completed_at": "ISO|null",
  "created_at": "ISO", "updated_at": "ISO" }
```

| 工具 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `list_tasks` | `filter`: `active`(默认)/`completed`/`all`；`keyword`?: 标题包含；`priority`?: 过滤 | `Task[]` | 只读，不触发事件 |
| `create_task` | `title`*；`priority`?；`remind_at`? ISO；`remind_offset_min`?；`repeat_rule`? RRULE | `Task` | 置顶（同 UI 行为） |
| `update_task` | `id`*；`title`?；`priority`?；`remind_at`?（传 null 清除）；`repeat_rule`? | `Task` | 仅 `deleted=0`；未知 id → NotFound 错误 |
| `complete_task` | `id`* | `Task` | 已完成则幂等返回 |
| `uncomplete_task` | `id`* | `Task` | 未完成则幂等返回 |
| `delete_task` | `id`* | `{ "deleted": true }` | 软删；未知 id → NotFound 错误 |

参数校验：`title` 非空（trim 后长度 > 0，对齐 DB CHECK 约束）、`priority` 枚举、`remind_at` ISO8601。非法参数 → `InvalidArgument` 错误。

## 7. 安全模型

- 仅绑定 `127.0.0.1`，不接受外部网络访问。
- Bearer token 必检（恒时比较），无 token/错误 token → 401。
- 开关**默认关闭**；未启用时端口不监听。
- 工具集只覆盖 tasks 表读写，无文件、无 shell、无任意 SQL。
- token 轮换机制应对泄露（主动重置，默认保持稳定）。
- 不做应用级登录态复用（MCP 认证与应用同步登录解耦，避免把同步凭证暴露给本地服务）。

## 8. 边界情况与错误处理

| 场景 | 处理 |
|---|---|
| 开关开启但端口被占 | server 启动失败 → 状态上报前端，设置面板显示错误 + 重试 |
| 应用退出 | 进程终结即停止监听，Claude 侧连接断开报错，可接受 |
| db 被前端连接占用写锁 | WAL + busy_timeout=5000，短时竞争自动等待 |
| 同步未登录 / 离线 | 本地写入照常，pending_ops 等下次 flush（现有逻辑） |
| AI 与用户同时编辑同一任务 | Last-Write-Wins 由现有同步处理；UI 经事件刷新后展示最新 |
| 带提醒任务被 AI 创建 | 前端重拉时 `refreshFromDb` 重排提醒（复用 hydrate 逻辑） |
| token 泄露 | 设置面板「重置 token」，旧配置立即失效；已配置 agent 需同步更新一次 |
| AI 请求畸形 JSON-RPC | 按协议返回错误，不 panic |

## 9. 测试策略

- **Rust 单元测试**（`mcp_tools.rs`）：临时 db 文件，验证 create/update/complete/delete/list 的 SQL 语义与 `db.ts` 对齐（sort_order 置顶、软删过滤、排序一致、pending_ops 记录正确）。
- **MCP 协议集成测试**：启动 server（测试端口），用 HTTP 裸调用模拟 initialize → tools/list → tools/call 全流程；验证认证 401 / 参数错误 / NotFound。
- **前端**：`refreshFromDb` 抽取后，手工验证：MCP 写入 → UI 即时刷新；提醒重排触发。
- **端到端（手动）**：`claude mcp add` 真机连接，AI 自然语言新建/完成/删除任务，观察 UI 与手机端同步。

## 10. 实施里程碑（供 writing-plans 分解）

1. **M1 骨架**：`mcp_config.rs`（开关 + token + commands）、`mcp_server.rs`（rmcp 空 handler + 认证 + 启停）、设置面板开关 UI。
2. **M2 数据层**：`mcp_tools.rs`（rusqlite + SQL 对齐 + 事务 + 单元测试）、6 个工具接线。
3. **M3 刷新链路**：前端 `refreshFromDb` 抽取 + 事件订阅。
4. **M4 面板完整**：token 展示/复制/轮换、注册命令复制。
5. **M5 端到端**：Claude Code 真机验证 + 文档（README / 设置面板帮助文案）。

## 11. 未来扩展（不在本迭代）

- 更多工具（undo、批量导入、按日期查询）。
- CLI 子命令（若 MCP 不可用场景出现再议）。
- 应用内嵌 AI 对话。
- 端口动态化 + 客户端自动发现（仅当固定端口出问题时）。
