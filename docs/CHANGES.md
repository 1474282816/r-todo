# 改动日志

## 2026-11-08 · 桌面端局域网互传（LocalSend 兼容）

### 背景

给桌面端加局域网文件互传，与官方 LocalSend、后续手机端 r-todo 互通。协议/传输逻辑
复用 LocalSend 官方 Rust core，不重新实现；UI 沿用 r-todo 现有风格。

### 改动

- `crates/vendor/localsend-core`（新）：上游 `packages/core` 原样 vendor
  （rev `58955fd`，Apache-2.0，未改代码，含 `LICENSE` + `VENDORED.md`）。
  原因是上游带 Flutter 子模块，cargo 的 git 依赖会递归 fetch 子模块必然失败。
- `crates/transfer-core`（实现）：身份持久化（自签证书 + fingerprint）、组播/手动 IP 发现、
  发送（register → prepare-upload → upload）、接收（人工确认 + `.part` 落盘 + 重名处理）、
  进度节流、取消/拒绝、错误码映射；新增 `identity_dir` / `verify_checksums` 配置字段。
- `crates/transfer-core/tests/loopback.rs`（新）：两个实例 HTTPS 真实互传一个文件的集成测试。
- `apps/desktop/src-tauri/src/transfer/`（新）：设置持久化（`transfer-config.json`）、
  14 个 `transfer_*` command、core 事件 → `transfer-*` Tauri 事件桥、独立窗口、
  托盘「打开互传」、退出时停服等端口释放。
- 前端：`src/lib/transfer.ts` + `src/features/transfer/*`（窗口/设备列表/发送/接收确认/进度/设置）；
  `main.tsx` 增加 `?view=transfer`；主窗口顶部加「互传」按钮；设置面板加「局域网互传」开关。
- `capabilities/default.json` 的 `windows` 增加 `"transfer"`。

### 实现期间修复

- Windows 端口落在 Hyper-V/WSL 排除段时组播整体失败 → 错误提示改为“端口可能被系统保留，
  换端口或用手动 IP”（`netsh int ipv4 show excludedportrange protocol=udp` 可查）。
- 互传窗口关闭期间错过 `Incoming` 事件会导致发送方一直等 → 新增 `transfer_pending`，
  窗口重开时恢复接收弹窗。
- 收到文件请求时若互传窗口未打开，事件桥自动开窗并延迟补发一次事件。
- 窗口在后台时，接收完成发系统通知。
- PIN / 拒绝 / 对端忙 / 校验失败的错误文案改为中文，不再透出 `HTTP 403`。
- 端口 0（系统分配）时发现/发送使用真实端口；退出时 discovery/server 各加 2s 超时。
- 发送面板补「对方 PIN」输入框（可选），协议层两条路径已加回环测试覆盖。
- 修复接收中途取消无效：原来只清会话槽，body 继续写、文件照样完整落盘。现在会停进度、
  反向给发送方发协议级 cancel、删除 `.part` 并丢弃半成品（新增回归测试）。
- 离线设备报错改为中文提示，并自动从设备列表移除。
- 修复发行版单文件 exe 白屏：`cargo build --release` 未带 `custom-protocol` 时会把
  `devUrl(http://localhost:1420)` 编进产物，拷到无 vite 的机器上白屏。Cargo.toml 补
  `[features] custom-protocol = ["tauri/custom-protocol"]`，产物改用 `--features custom-protocol`
  构建，并已用「停 vite + 1420 探针 + PrintWindow 截图」验证内嵌资源生效。
- 新增调试工具：`crates/transfer-core/examples/{multicast_probe,send,serve}.rs`。

### 已知限制

- 真机互通（官方 LocalSend 双向、真实组播、防火墙、睡眠唤醒、1GB+）待用户验收。
- 收藏设备自动接受暂未开放；`auto_accept_favorites` 只持久化不生效。
- LocalSend v2 服务端单会话，多设备同时发送时后到者会失败。

## 2026-09-14 · MCP 服务器（AI 操作待办）

### 背景

应用安装后即可通过 AI（Claude Code / Claude Desktop / opencode / codex 等）直接读写待办。桌面端内置 MCP server（默认关闭、可开关），`settings` 面板可配置 token 与各 agent 的配置模板。

### 改动

- `apps/desktop/src-tauri/src/mcp_server.rs`（新）：rmcp streamable HTTP server（`127.0.0.1:18765/mcp`）+ Bearer 恒时认证 + 启停生命周期 + 运行态/错误上报。
- `apps/desktop/src-tauri/src/mcp_config.rs`（新）：开关/token（`rtok_` 前缀、稳定不自动轮换）/agent 模板（Claude Code/Desktop/opencode/codex）。
- `apps/desktop/src-tauri/src/mcp_tools.rs`（新）：rusqlite 数据层 + 6 个工具（list/create/update/complete/uncomplete/delete），SQL 语义与前端 `db.ts` 逐条对齐；写操作同事务写 `pending_ops`（PocketBase 字段名、不含 user，flush 时注入）。
- 前端：`refreshFromDb` 抽取（store.ts）+ 订阅 `tasks-changed` 事件实时刷新；设置面板 MCP 区块（开关/地址/token 掩码复制/模板切换复制/重置/运行态/错误重试）。
- `db.ts` 修复 `enqueueOp` 参数错位 bug（op_type 拿到 `"tasks"`，离线队列写不进去）。
- README 增「AI 操作待办（MCP）」节。

### 已知限制

- opencode / codex 模板按官方格式内置但未真机验证（本机未装）；报 schema 错误请按官方文档调整。
- 云同步路径（登录态）未端到端实测，逻辑由既有 flush 队列覆盖。

## 2026-08-07 · 桌面嵌入重做：WorkerW 挂载（Win+D 不收起）

### 背景

M2 的"嵌入桌面"一直是妥协实现（`WS_EX_TOOLWINDOW` + `HWND_BOTTOM`，按 Win+D 会被收起）。
Backlog OPT-002 记录过 4 个失败方案（subclass、替换 WndProc、挂 Progman+WS_CHILD 等）并标"放弃"。

### 结论

**Tauri 完全能做到**，之前失败是算法没试对：正确做法不是挂 Progman，而是向 Progman 发
`0x052C` 让 explorer 创建"空 WorkerW（壁纸层）"，然后 `SetParent` 挂到它上面，且**保持
WS_POPUP、不设 WS_CHILD**（MSDN 明确 SetParent 不改这两种样式）。挂载后窗口不再是顶层
窗口，Win+D 只最小化顶层窗口，所以收不走它。外部证据：electron-as-wallpaper（同款算法）、
tauri issue #10345（Tauri 2 用户实测可行）、tauri issue #4261（官方确认需自实现）。

### 改动

- `apps/desktop/src-tauri/src/desktop_embed.rs`（重写）：
  - `find_worker_w()`：`FindWindowW("Progman")` → `SendMessageTimeoutW(0x052C)` →
    `EnumWindows` 找含 `SHELLDLL_DefView` 的 WorkerW 之后那个空 WorkerW（fallback 链：
    任意空 WorkerW → 图标层 WorkerW）
  - `embed_to_desktop`：`SetParent(hwnd, workerw)`（不加 WS_CHILD）→
    `SetWindowPos(SWP_FRAMECHANGED|SWP_SHOWWINDOW)` 刷新 →
    `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` 恢复 DPI（跨进程 SetParent 会 forced reset）
  - `unembed_from_desktop`：`SetParent(hwnd, NULL)` 恢复顶层 + 恢复 `WS_EX_APPWINDOW`
- `apps/desktop/src-tauri/Cargo.toml`：windows 增加 `Win32_UI_HiDpi` feature
- `docs/backlog/OPT-002-embed-strategy-switch.md`：状态"放弃"→"已重开并实现"，记录新方案
  与实机验证清单

### 状态

✅ `cargo check` 编译通过；**运行时行为需用户实机验证**（Win10 20H2 + Win11，验证清单见 OPT-002）。

### 已知限制（后续项）

- explorer 崩溃/重启、壁纸切换 → WorkerW 重建，挂载丢失，后续加低频 `GetParent` 检查自动重挂
- WebView2 输入焦点在个别机器可能异常（tauri#10345 用户实测可用）

## 2026-07-18 · 用户睡觉时的一次性推进（M2~M5）

### 用户上下文

用户明确授权："算了，你先开发下一步吧，这个先记为待优化项。我要睡觉去了，你不要等我一步一步确认了，就往后一直开发吧，我睡醒起来再帮你验证"

### 已完成里程碑

| 里程碑 | 状态 |
|---|---|
| **M1** Windows Hello World | ✅ 已完成（用户已亲验 Mica/Acrylic + 缩放） |
| **M2** WorkerW 桌面嵌入 + 位置记忆 + 开机自启 + 单实例 | ✅ Rust 编译通过，运行时未验证 |
| **M3** SQLite 本地存储 + Task CRUD + Things 3 风任务列表 UI | ✅ 前后端编译通过，运行时未验证 |
| **M4** RRULE 重复规则 + 提醒调度 + 系统通知 + 编辑面板 | ✅ 前后端编译通过，运行时未验证 |
| **M5** 视觉打磨（微动画 + 深浅色 + 快捷键 + 零态） | ✅ 前后端编译通过，运行时未验证 |

### 待优化（M1 遗留）

- **OPT-001** 拖动跟手：改回 `data-tauri-drag-region` + `select-none`（当前版本），待醒后验证；如仍飘则走 Backlog 备选方案

### 详细技术改动

#### Rust 侧

**新增文件**：
- `apps/desktop/src-tauri/src/db.rs` — SQLite migration 定义（tauri-plugin-sql）
- `apps/desktop/src-tauri/src/notifier.rs` — 提醒调度模块，tokio 定时任务 + tauri-plugin-notification

**修改文件**：
- `apps/desktop/src-tauri/Cargo.toml`：
  - `window-vibrancy` 0.5 → 0.6
  - 新增：`tauri-plugin-sql` (sqlite feature), `tauri-plugin-notification`, `tauri-plugin-window-state`, `tauri-plugin-autostart`, `tauri-plugin-single-instance`
  - 新增：`chrono`, `uuid`, `tokio`, `log`
  - Windows 特性：新增 `Win32_UI_Shell` / `Win32_System_LibraryLoader`
- `apps/desktop/src-tauri/src/lib.rs`：
  - 集成上述 5 个插件
  - Mica → Acrylic 降级链（Win10 也能有毛玻璃）
- `apps/desktop/src-tauri/src/desktop_embed.rs`：
  - **实装** WorkerW 挂载算法（原骨架只留 stub）
  - `find_worker_w()`：发 0x052C → EnumWindows → 找到有 SHELLDLL_DefView 兄弟的 WorkerW
  - `embed_to_desktop` / `unembed_from_desktop` command
  - 适配 windows 0.58 的 HWND 新签名（`*mut c_void`）
- `apps/desktop/src-tauri/tauri.conf.json`：新增 `plugins.sql.preload`
- `apps/desktop/src-tauri/capabilities/default.json`：加所有插件权限

**删除文件**：
- `apps/desktop/src-tauri/src/models.rs` — 前端直接读写 SQLite，不经 Rust CRUD 层，删除无用 Rust struct

#### 前端 TypeScript 侧

**新增依赖**：`@tauri-apps/plugin-sql` `@tauri-apps/plugin-notification` `@tauri-apps/plugin-window-state` `@tauri-apps/plugin-autostart` `zustand` `immer` `rrule` `nanoid` `date-fns` `lucide-react` `framer-motion` `clsx`

**新增文件**：

域层（`src/lib/`）：
- `types.ts` — Task/Priority/常量
- `db.ts` — SQLite CRUD（tauri-plugin-sql）
- `store.ts` — Zustand + Immer store，所有 mutation 走这里
- `reminder.ts` — 提醒调度前端桥（invoke Rust command）
- `rrule.ts` — RRULE 构建/解析/推进（rrule.js），含"完成后 rollNext"
- `system.ts` — 桌面嵌入 + 自启前端桥
- `datetime.ts` — "今天/明天/6月20日" 时间格式化 + 次日隐藏判定
- `cn.ts` — className 合并
- `hooks/useHotkeys.ts` — Ctrl+N / Escape 全局快捷键

UI 组件（`src/components/`）：
- `ui/Checkbox.tsx` — Things 3 风空心圆勾选，带 tap 缩放动画
- `ui/PriorityBar.tsx` — 任务左侧优先级竖条
- `ui/IconButton.tsx` — 通用图标按钮
- `task/QuickAdd.tsx` — 顶部快速添加，含优先级选择器，暴露 focus ref
- `task/TaskRow.tsx` — 单行任务，含提醒/重复徽标 + hover 删除
- `task/TaskEditor.tsx` — 底部弹出编辑面板：标题/优先级/提醒时间/提前分钟/重复规则（预设 + 周几）
- `settings/SettingsPanel.tsx` — 设置面板：嵌入桌面 / 开机自启 开关

**重写文件**：
- `App.tsx` — 完整主界面，包含拖动条、快速添加、列表分区（未完成 + 今日已完成）、编辑面板、设置面板
- `styles.css` — 全局 select-none + 极简滚动条 + 深色 datetime input

### 编译验证结果

| 项 | 命令 | 结果 |
|---|---|---|
| TypeScript 类型 | `pnpm --filter desktop build` | ✅ 通过，无警告 |
| Vite 打包 | 同上 | ✅ 6.6s, JS 336KB (gzip 108KB) |
| Rust 检查 | `cargo check` | ✅ 通过，0 警告 |
| Rust 完整编译 | `cargo build` | ✅ 23.91s，产物 22MB `r-todo-desktop.exe` |

### 手工 QA 未覆盖（用户睡醒需验证）

以下功能已实现且编译通过，但**运行时行为未由 AI 亲眼验证**（GUI 应用限制）：

1. **数据 CRUD**：添加/编辑/勾选/删除任务 → SQLite 持久化
2. **重复任务**：设置每天/工作日等 → 勾选完成后自动生成下一实例
3. **提醒**：设置提醒时间 + 提前分钟数 → 到点弹系统通知
4. **桌面嵌入**：设置面板打开"嵌入桌面" → 窗口挂到 WorkerW（Win+D 不隐藏）
5. **开机自启**：设置面板打开 → 重启后自动启动
6. **位置/尺寸记忆**：拖动/缩放窗口 → 关闭再打开保留位置
7. **快捷键**：Ctrl+N 聚焦输入框，Esc 关闭浮层
8. **视觉**：Things 3 风格的极简列表、优先级色标、深浅色跟随系统

### 用户第二天的验收路径

```powershell
pnpm dev:desktop
```

预期看到：
- 380x620 无边框毛玻璃窗口
- 顶部 "r-todo" 标 + 右上设置按钮
- 输入框 + 3 个优先级圆点
- 空列表（首次运行）"今天没有任务"

功能测试建议顺序：
1. 加一条任务，勾选，取消勾选 → SQL 写入生效？
2. 点任务打开编辑面板，改优先级 + 加提醒（选 2 分钟后）→ 到点弹通知？
3. 设置面板 → 嵌入桌面 → 窗口应该"沉入"桌面
4. 设置面板 → 开机自启 → 打开
5. Ctrl+N 应聚焦输入框；有编辑面板时按 Esc 应关闭

### 下一步方向

- 用户验收后修 OPT-001 或其他运行时 bug
- M6~M9 移动端（Flutter）
- M10 PocketBase 部署 + 同步
