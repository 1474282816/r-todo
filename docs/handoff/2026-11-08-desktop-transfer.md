# Handoff · 桌面端局域网互传（LocalSend 兼容）

> 基线：`master`（2026-11-08，含 `crates/transfer-core` skeleton）
> 目标读者：接手实现的 agent
> 目标平台：Windows（Tauri 2）
> 预计工作量：MVP 1.5~2 周，可日常使用 3~4 周
> 配套文档：`docs/handoff/2026-11-08-mobile-transfer.md`
> 共享 crate：`crates/transfer-core`（skeleton 已合入 master，契约测试通过）

---

## 1. 任务目标

在现有 r-todo 桌面端加入**局域网文件互传**，协议兼容官方 LocalSend：

- 发现同一局域网内的设备（组播 + 手动 IP）
- 发送文件 / 多文件，显示进度，可取消
- 接收文件，接收前人工确认，保存到本地
- 与官方 LocalSend（Windows / Android）双向互通
- 与后续的手机端 r-todo 互通（共用同一套 Rust core）

### 非目标（本期不要做）

- 手机端实现（另有 handoff）
- 手环导航 / MiNavigateRing 合并
- WebRTC 传输、浏览器 Web 分享页
- macOS / Linux 适配
- 修改 updater、签名密钥、`latest.json`、发版脚本
- 改造主窗口 380×620 的挂件形态

---

## 2. 必读现状

### 2.1 桌面端技术栈

- Tauri 2 + React 18 + TypeScript + Tailwind
- Rust 侧已有：`tauri-plugin-sql`、`notification`、`window-state`、`autostart`、`single-instance`、`dialog`、`fs`、`updater`、`process`
- 已有长驻服务范例：`apps/desktop/src-tauri/src/mcp_server.rs`（axum + CancellationToken + 错误槽），互传服务可参考它的启停/错误上报方式
- 已有独立窗口范例：`apps/desktop/src-tauri/src/month_window.rs`
- 前端路由分发：`apps/desktop/src/main.tsx`（`?view=month`）
- 设置面板：`apps/desktop/src/components/settings/SettingsPanel.tsx`
- 文件选择/写入范例：`apps/desktop/src/lib/io_ui.ts`

### 2.2 当前版本与依赖

- 桌面端版本：`0.0.9`
- `Cargo.lock` 已有：`tokio 1.53.0`、`hyper 1.10.1`、`rustls 0.23.42`、`reqwest 0.13.4`、`ring 0.17.14`、`axum 0.8.9`
- `base64` 当前是 `0.22`；LocalSend core 用 `0.23`，两个版本可以共存
- 新增依赖会 bump hyper/rustls 的 patch/minor，属正常

### 2.3 权限与窗口

- `apps/desktop/src-tauri/capabilities/default.json` 的 `windows` 目前是 `["main", "month"]`
- 新增 `transfer` 独立窗口后，**必须把 `"transfer"` 加进该数组**
- 自定义 Tauri command 不需要 ACL；但 dialog/fs/event 等插件 API 需要对应权限

### 2.4 共享 core skeleton

`crates/transfer-core` 已合入 master，包含：

- `TransferConfig`（默认端口 53317）
- `DeviceInfo` / `DeviceChannel` / `DeviceSummary`
- `TransferEvent` + `EventEnvelope`（`SCHEMA_VERSION = 1`）
- `ErrorCode` / `ErrorScope` / `TransferError`
- `TransferService` 骨架：`start` 只保存配置与事件通道，`send` / `respond_*` 返回 `NotImplemented`
- 契约测试：`cargo test` 通过

**Desktop agent 负责实现 core 内部**；不要单方面改事件字段、错误码或 `SCHEMA_VERSION`。

### 2.5 不要动的部分

- `apps/desktop/src-tauri/src/mcp_*.rs`
- `apps/desktop/src/lib/sync.ts`、`db.ts`、`store.ts`（互传不参与任务同步）
- `apps/desktop/src-tauri/tauri.conf.json` 的 `plugins.updater`
- `apps/desktop/latest.json`、`scripts/release-desktop.ps1`
- `apps/desktop/src-tauri/src/desktop_embed.rs`、`window_effect.rs`、`wallpaper_mica.rs`

---

## 3. 已定决策（不要重新讨论）

1. **复用 LocalSend 的 Rust core**，不重新实现协议。
   - 上游：`https://github.com/localsend/localsend`
   - crate：`packages/core`，包名 `localsend`
   - 用 git 依赖并 **pin 到具体 rev**，不要跟 `main`
   - `features = ["discovery"]` 即可（会带上 `crypto + http + multicast`）
   - 不开 WebRTC；不开 Web 分享页
2. **参考 `packages/localsend_isolates/rust/src/api/` 的高层封装**：
   - `server.rs`（约 29KB）：接收会话、落盘、进度
   - `discovery.rs`（约 17KB）：设备合并、事件流
   - `http.rs`（约 8KB）：发送
   - 这些文件是 Apache-2.0，可以复制/改写；它们和 `flutter_rust_bridge` 强耦合，需要把 `StreamSink` 换成 `tokio::mpsc` / Tauri event，并去掉 `#[frb]` 宏。
3. **协议走 HTTP(S) v2**（必要时补 v3），端口默认 **53317**，HTTPS 自签证书 + fingerprint，不做明文默认。
4. **传输 UI 用独立窗口**，不要塞进主挂件：
   - 主窗口只加托盘菜单入口 / 设置入口
   - 独立窗口参考 `month_window.rs`，约 720×520，有系统边框
5. **接收必须人工确认**；收藏设备可配置自动接受，默认关闭。
6. **文件保存**：默认 `Downloads\r-todo`；用户可改接收目录；文件名由接收方清洗生成，绝不信任发送方路径。
7. **许可证**：Apache-2.0，保留 `LICENSE` / `NOTICE`，注明修改；不要使用 LocalSend 的名称/图标做产品名。
8. **UI 沿用 r-todo 现有风格，不要被 LocalSend 的界面误导**；LocalSend 只作为协议/传输逻辑参考。

---

## 4. 目标架构

```
crates/transfer-core/                # 新增：独立 Rust crate，桌面/手机共用
  Cargo.toml
  src/lib.rs
  src/config.rs
  src/identity.rs
  src/discovery.rs
  src/server.rs
  src/client.rs
  src/event.rs
  src/path.rs

apps/desktop/src-tauri/src/transfer/
  mod.rs                               # Tauri 状态、commands、启停
  events.rs                            # event 名与 payload 映射

apps/desktop/src/features/transfer/
  TransferWindow.tsx
  DeviceList.tsx
  SendPanel.tsx
  ReceiveDialog.tsx
  TransferProgress.tsx
  useTransfer.ts
```

说明：

- `crates/transfer-core` 是 **standalone Rust crate**，不属于 pnpm 包；桌面 Tauri 用 path 依赖，手机端 FRB 也用 path 依赖。
- 不要把它做成根 Cargo workspace（会改变 Tauri 的 target 目录和构建行为）。
- 桌面端只负责“Tauri 状态/事件/窗口/UI”，协议与文件流在 core 里。
- 依赖写法：

```toml
# apps/desktop/src-tauri/Cargo.toml
rtodo-transfer-core = { path = "../../../crates/transfer-core" }
```

---

## 5. 对外契约

### 5.1 Tauri commands

| command | 入参 | 返回 | 说明 |
|---|---|---|---|
| `transfer_start` | `{}` | `{running, port, alias, fingerprint}` | 启动 discovery + HTTP server |
| `transfer_stop` | `{}` | `{}` | 停止并等待端口释放 |
| `transfer_get_state` | `{}` | `{running, port, alias, fingerprint, settings, error}` | 设置面板/窗口初始化 |
| `transfer_discover` | `{timeoutMs?: number}` | `{}` | 主动发现；结果走事件 |
| `transfer_add_manual_ip` | `{host, port?, protocol?}` | `{device?}` | 手动 IP 兜底 |
| `transfer_send` | `{files: string[], fingerprint: string, pin?: string}` | `{sessionId}` | 发送文件路径列表 |
| `transfer_respond_prepare` | `{sessionId, acceptedFileIds: string[]}` | `{}` | 接收确认 |
| `transfer_respond_file` | `{sessionId, fileId, savePath?: string}` | `{}` | 指定落盘路径；缺省用接收目录 |
| `transfer_reject` | `{sessionId}` | `{}` | 拒绝整个会话 |
| `transfer_cancel` | `{sessionId}` | `{}` | 取消发送/接收 |
| `transfer_set_settings` | `{alias?, port?, receiveDir?, pin?, autoAcceptFavorites?}` | `{settings}` | 持久化到本地配置 |

### 5.2 Tauri events（桌面端与手机端保持一致）

事件名用连字符，避免 `:`/`//` 兼容问题：

```text
transfer-devices
transfer-incoming
transfer-progress
transfer-completed
transfer-error
transfer-session-ended
```

Payload 约定（`schema_version: 1`）：

```json
// transfer-devices
{ "schema_version": 1, "devices": [
  { "alias": "PC", "version": "2.2", "device_model": "Windows",
    "device_type": "desktop", "fingerprint": "ABC...",
    "channels": [ { "host": "192.168.1.5", "port": 53317, "protocol": "https" } ],
    "download": false }
] }

// transfer-incoming
{ "schema_version": 1, "session_id": "...", "ip": "192.168.1.9",
  "device": { "alias": "Phone", "fingerprint": "..." },
  "files": [ { "id": "f1", "file_name": "a.pdf", "size": 12345,
               "file_type": "application/pdf", "sha256": null } ] }

// transfer-progress
{ "schema_version": 1, "session_id": "...", "file_id": "f1",
  "direction": "send", "transferred": 1024, "total": 12345, "percent": 8.3 }

// transfer-completed
{ "schema_version": 1, "session_id": "...", "file_id": "f1",
  "direction": "receive", "path": "C:\\Users\\...\\Downloads\\r-todo\\a.pdf" }

// transfer-error
{ "schema_version": 1, "scope": "send|receive|discovery|server",
  "code": "PORT_IN_USE|FIREWALL|MULTICAST_UNAVAILABLE|TLS|PIN_REQUIRED|PIN_INVALID|REJECTED|CHECKSUM_MISMATCH|CANCELLED|IO|UNKNOWN",
  "message": "..." }

// transfer-session-ended
{ "schema_version": 1, "session_id": "...", "reason": "completed|cancelled|rejected|error" }
```

### 5.3 core 的职责边界

`transfer-core` 提供平台无关 API，桌面/手机各自做壳：

```rust
pub struct TransferConfig { /* alias, port, receive_dir, pin, auto_accept_favorites */ }

pub enum TransferEvent { /* Devices, Incoming, Progress, Completed, Error, SessionEnded */ }

pub struct TransferService { /* ... */ }

impl TransferService {
    pub async fn start(config: TransferConfig, event_tx: mpsc::Sender<TransferEvent>) -> Result<Self>;
    pub async fn send(&self, files: Vec<PathBuf>, target: DeviceTarget, pin: Option<String>) -> Result<SendSession>;
    pub async fn respond_prepare(&self, session_id: &str, accepted: Vec<String>) -> Result<()>;
    pub async fn respond_file(&self, session_id: &str, file_id: &str, save_path: &Path) -> Result<()>;
    pub async fn reject(&self, session_id: &str);
    pub async fn cancel(&self, session_id: &str);
    pub async fn stop(self);
}
```

具体签名可按实现调整，但**事件模型与错误码必须与手机端一致**。

---

## 6. 分阶段任务

### Phase 0 · Spike（0.5~1 天）

- [ ] 在 `apps/desktop/src-tauri/Cargo.toml` 加 `localsend` git 依赖，pin 到具体 rev
- [ ] `features = ["discovery"]`
- [ ] `cargo check` 通过
- [ ] 确认 core 的 `lib.rs` 里无条件 `pub mod webrtc` 是否会导致编译失败；如失败，记录并决定 patch / vendor / 开 feature
- [ ] 写一个最小 example：起 server（临时端口）+ 用 `LsHttpClient` 回环发一个文件
- [ ] 用官方 LocalSend 手机端做一次真实互传

**产出**：`docs/handoff/notes/desktop-transfer-spike.md`，记录实际 API、版本、坑。

### Phase 1 · `crates/transfer-core`（2~3 天）

- [ ] 新建 standalone crate
- [ ] 身份：生成/加载 PEM 证书 + 私钥，fingerprint（大写 hex）
- [ ] Discovery：`start`、`announce`、`discover_staged`、`scan_subnet`、`add_device`、设备合并
- [ ] Server：`start_with_port`、`ServerConfigV2`、`ServerEventV2` 转 `TransferEvent`
- [ ] Client：`register` → `prepare_upload` → `upload` → `cancel`
- [ ] 接收会话：`respond_prepare`、`respond_file`、`reject`、`cancel`
- [ ] 进度：累计字节 → 百分比；100ms 节流
- [ ] 错误码映射
- [ ] 单元测试：路径清洗、事件序列化、会话状态

**注意**：`localsend_isolates` 的 `pending_decision` 是单槽，桌面端改成按 `session_id` 的 map，支持多设备同时请求。

### Phase 2 · Tauri 集成（2~3 天）

- [ ] `transfer/mod.rs`：`tauri::State` 持有 `TransferService`
- [ ] 注册 commands（见 §5.1）
- [ ] 启动时按设置决定是否自动启动服务
- [ ] 应用退出时 `stop()`，等待端口释放
- [ ] `capabilities/default.json` 的 `windows` 加 `"transfer"`
- [ ] 事件桥接：core event → `app.emit("transfer-*")`

### Phase 3 · UI（3~5 天）

- [ ] `main.tsx` 加 `?view=transfer` 路由
- [ ] `TransferWindow.tsx`：设备列表 / 发送 / 接收 / 设置
- [ ] 托盘菜单加“打开互传 / 发送文件”
- [ ] 主窗口设置面板加入口和开关（不塞传输详情）
- [ ] 发送：文件选择 + 拖拽（`onDragDropEvent`）+ 目标设备 + 进度 + 取消
- [ ] 接收：确认弹窗（设备/文件/大小）+ 保存目录 + 完成通知
- [ ] 空态/错误态：无设备、防火墙、端口占用、组播不可用

### Phase 4 · 互通与边界（2~3 天）

- [ ] 官方 LocalSend Windows/Android 双向
- [ ] 两台 r-todo 桌面互传
- [ ] PIN 正确/错误/缺失
- [ ] 0 字节、1GB+、中文名、重名、多文件
- [ ] 取消 / 拒绝 / 断线
- [ ] 端口冲突、防火墙拒绝、VPN/虚拟网卡
- [ ] 关闭窗口/退出应用后端口释放

---

## 7. 关键实现细节

### 7.1 身份与 TLS

- 首次启动生成证书 + 私钥，存 `app_data_dir/transfer/`
- fingerprint 用证书 SHA-256 大写 hex
- 对端 fingerprint 用于 TLS 固定，不要只信响应体
- 关闭 TLS 只用于本地调试，生产默认 HTTPS

### 7.2 发现

- 默认组播 `224.0.0.167:53317`；同时支持手动 IP / 收藏
- `discover_staged` 的推荐流程：先探测收藏，再回退扫 `/24`
- Windows 多网卡/虚拟网卡很多，提供网卡白名单/黑名单设置
- 组播绑定失败不应让整个功能挂掉，保留手动 IP

### 7.3 发送

- `LsHttpClient::register` → `prepare_upload` → 逐文件 `upload`
- `upload` 的 progress 是**累计字节**，不是增量
- 用户取消用 `CancellationToken`，并调用 `cancel`
- 发送前校验文件存在/可读/大小；失败不要开始会话

### 7.4 接收

- `PrepareUpload` 事件 → 前端弹确认 → `respond_prepare_upload(accepted_ids)`
- 每个 `FileUpload` → 生成安全文件名 → `respond_file_upload(path)`
- `SessionEnd` 清理 pending 状态
- 接收方取消用 `ServerHandle::cancel_v2_session`
- 文件名清洗：去掉路径分隔符、`..`、控制字符；重名加 ` (1)`；不覆盖已有文件
- 先写 `.part` 临时文件，成功后 rename；失败删除

### 7.5 服务生命周期

- 默认不随应用启动，用户在设置里打开“互传服务”
- 打开后：discovery + server 一起启动
- 关闭：先 stop discovery，再 `wait_stopped()`，确保 53317 可重绑
- 端口冲突：捕获 bind 错误，事件 `PORT_IN_USE`，UI 给“关闭官方 LocalSend / 换端口”指引

---

### 7.6 UI 风格约束（重要）

LocalSend 只提供协议和传输逻辑，**不要参考它的界面、配色、图标和布局**。传输窗口必须像 r-todo 原有功能的一部分：

- 参考 `apps/desktop/src/App.tsx`、`components/task/TaskRow.tsx`、`components/settings/SettingsPanel.tsx`、`components/ui/IconButton.tsx`
- 继续用 Tailwind `neutral` 色系 + `dark:`；11~12px 小字号、`rounded-lg`、`bg-black/5 dark:bg-white/5`、细边框
- 图标继续用 `lucide-react` 14~16px；不要引入第二套图标库
- 按钮、开关、胶囊选项复用 `SettingsPanel` 的样式；不要新造一套组件
- 主窗口 380×620 的挂件视觉不变；传输窗口可以更大，但字体、间距、圆角、颜色要一致
- 窗口标题用「互传 · r-todo」，不要出现 LocalSend 字样
- 深浅色都要检查；避免大面积模糊/重毛玻璃，传输列表不依赖 `backdrop-blur`

禁止：

- Material AppBar / 蓝色主色 / 大圆角卡片 / LocalSend 的绿色品牌色和 logo
- 直接复制 LocalSend 的页面结构或文案
- 为传输功能引入新的设计系统

---

## 8. Windows 特有问题

| 问题 | 处理 |
|---|---|
| 端口 53317 被官方 LocalSend 占用 | 明确报错；不要静默换端口导致对端发现不到 |
| Windows 防火墙弹窗/拒绝 | 捕获 `ListenerFailed`，设置页给放行指引 |
| 公用网络限制入站 | 提示切“专用网络”或用手动 IP |
| Hyper-V/WSL/Docker/VPN 虚拟网卡 | 提供网卡过滤；组播失败时回退手动 IP |
| 中文/长路径/重名 | Rust 侧统一清洗；必要时加 `\\?\` 前缀 |
| 睡眠/唤醒后 socket 失效 | 监听 `ListenerFailed`，自动重启服务并提示 |
| 主窗口是置底挂件 | 传输窗口必须独立、可获得焦点 |

---

## 9. 测试与验收

### 必跑命令

```powershell
cd apps/desktop/src-tauri
cargo check
cargo test

cd ../../..
pnpm --filter desktop build
pnpm dev:desktop
```

### 验收清单

- [ ] r-todo → 官方 LocalSend：单文件 / 多文件 / 中文名 / 0 字节 / 1GB+
- [ ] 官方 LocalSend → r-todo：确认、进度、取消、完成通知
- [ ] 两台 r-todo 桌面互传
- [ ] PIN 正确/错误/缺失
- [ ] 接收方拒绝 / 发送方取消 / 接收方取消
- [ ] 关闭应用后端口释放，重开可监听
- [ ] 端口占用、防火墙拒绝、组播失败都有明确 UI 提示
- [ ] 传输不影响主窗口拖动/置底/材质
- [ ] 传输窗口与现有主窗口/设置面板风格一致，没有 LocalSend 的视觉元素
- [ ] 不修改 updater / 发版脚本 / 任务同步

---

## 10. 风险

1. **LocalSend core 是 git 依赖，接口会变**：必须 pin rev；建议在 `docs/handoff/notes/` 记录 rev 与 API。
2. **`lib.rs` 无条件 `pub mod webrtc`**：可能和 optional 依赖冲突；Spike 必须先验证。
3. **单会话限制**：官方 v2 server 同时只允许一个上传会话；UI 要处理“设备忙”。
4. **组播在 Windows 不稳定**：手动 IP / 收藏必须从一开始就有，不能只依赖组播。
5. **协议 v3**：最新官方客户端可能优先 v3；Spike 用最新版官方 App 验证，不要只测 v2。
6. **路径安全**：绝不能用发送方提供的文件名直接落盘。
7. **许可证**：复制 `localsend_isolates` 代码时保留 Apache-2.0 声明与修改说明。

---

## 11. 交付与协作约定

- 分支：`feat/desktop-transfer`
- 提交粒度：Spike / core / Tauri / UI / tests 分开
- 不要提交：`target/`、`dist/`、`.sig`、私钥、`tmp/`
- `crates/transfer-core` 是桌面/手机共用 crate：
  - 桌面 agent 先建；手机 agent 通过 path 依赖消费
  - 任何 API/事件变更必须同步手机 handoff 的消费者
- 完成后在 handoff 文档追加“实现记录”：实际 API、rev、已知差异、未完成项
- 遇到需要用户决策的点（端口、默认接收目录、是否自动接受），先记录，不要擅自改产品行为

---

## 12. 并行开发约定（桌面 + 手机）

两端可以同时开发，前提是**共享 core 单归属**：

| 范围 | Owner | 规则 |
|---|---|---|
| `crates/transfer-core` 内部实现 | Desktop agent | 只有 desktop agent 改 `service.rs` / 协议实现 |
| `apps/mobile`、FRB、Kotlin | Mobile agent | 只消费 core API；需要新 API 先提给 desktop agent |
| 事件/错误码/`SCHEMA_VERSION` | 双方 | 任何改动必须同步两端 + `tests/contract.rs` |

工作区：

```powershell
# 由主仓库统一创建，两个 agent 各进自己的目录
git worktree add .worktrees/desktop -b feat/desktop-transfer
git worktree add .worktrees/mobile  -b feat/mobile-transfer
```

> 主仓库已创建这两个 worktree，并补齐 Android `local.properties` 与 cargo vendor 链接。
> 依赖安装（`pnpm install` / `flutter pub get`）由各 agent 首次执行。

合并顺序：

1. `crates/transfer-core` skeleton（已在 master）
2. Desktop 的 core 实现 + Tauri
3. Mobile rebase 后接真实 core（之前可用 skeleton/mock 开发 UI）

硬性禁止：

- 两个 agent 同时修改 `crates/transfer-core`
- 各自复制一套协议实现
- 单方面改事件字段/错误码

契约测试：

```powershell
cd crates/transfer-core
cargo test
```

---

## 13. 实现记录（2026-11-08，desktop agent）

### 13.1 状态

Phase 0 / 1 / 2 / 3 已完成并在本机编译 + 单测通过；Phase 4（真机互通验收）未做，
需要用户用官方 LocalSend 与真机跑一遍 §9 清单。

验证命令与结果：

| 命令 | 结果 |
|---|---|
| `cd crates/transfer-core && cargo test` | ✅ 17 个测试通过（含 4 个真实 HTTPS 回环互传：普通/自定义落盘路径/PIN/中途取消） |
| `cd apps/desktop/src-tauri && cargo check` | ✅ 通过 |
| `cd apps/desktop/src-tauri && cargo test --lib` | ✅ 26 个测试通过（含事件映射/设置读写） |
| `pnpm --filter desktop build` | ✅ tsc + vite 通过 |

### 13.2 与本文档的差异（需要同步给手机端）

1. **LocalSend core 不用 git 依赖，改为 vendored 源码**
   - 原因：上游带 `support/submodules/flutter` 子模块，cargo 解析 git 依赖会递归 fetch，
     必然失败；`[patch]` 也无法绕过。详见 `docs/handoff/notes/desktop-transfer-spike.md`。
   - 位置：`crates/vendor/localsend-core`，rev `58955fd54ef1e18e89ec30e60f46754084a80942`，
     未改一行代码，保留 `LICENSE`，来源见 `VENDORED.md`。
   - 手机端 path 依赖 `crates/transfer-core` 即可，不需要关心这层。
2. **`TransferConfig` 新增两个字段**（均 `serde(default)`，向后兼容）：
   - `identity_dir: PathBuf`：证书/私钥目录。桌面端 `app_data_dir/transfer`；手机端传 App 私有目录。
     空则退化到系统临时目录（身份每次重建）。
   - `verify_checksums: bool`：默认 `true`。
   - 事件结构与错误码**未改**，`SCHEMA_VERSION` 仍为 1。
3. **`transfer_get_state` 额外带了 `settings`**（本文档 §5.1 已写），实际返回
   `{running, port, alias, fingerprint, settings, error}`，`settings` 为 camelCase。
4. **托盘菜单只加了「打开互传」**，没有单独的「发送文件」项：发送需要文件选择器，
   走窗口内选择/拖拽更稳，避免在 Rust 里再拉一次 dialog。
5. **收藏设备自动接受未实现**：`auto_accept_favorites` 字段保留并持久化，但没有收藏列表，
   当前所有接收都走人工确认（默认安全）。
6. **`respond_file` 的 `savePath` 可选**：不传就用接收目录 + 核心生成的安全文件名；
   传了则记录到会话里，等该文件的上传请求到达时使用。

### 13.3 代码结构

```text
crates/transfer-core/
  src/lib.rs          # 导出契约
  src/config.rs       # TransferConfig（新增 identity_dir / verify_checksums）
  src/identity.rs     # 自签证书 + fingerprint 持久化
  src/discovery.rs    # 组播/手动 IP/子网扫描，DiscoveredDevice → DeviceInfo
  src/server.rs       # start_with_port + TlsConfig + 端口占用/防火墙错误映射
  src/client.rs       # register → prepare-upload → upload → cancel、错误码映射
  src/path.rs         # 文件名清洗、重名加序号、.part 临时文件
  src/service.rs      # 会话状态机、事件循环、进度节流、取消/拒绝
  src/event.rs        # 事件信封 + EventSender
  tests/contract.rs   # JSON 形状冻结
  tests/loopback.rs   # 两实例真实 HTTPS 互传

apps/desktop/src-tauri/src/transfer/
  mod.rs              # 设置持久化、Tauri 状态、commands、窗口、退出清理
  events.rs           # core 事件 → transfer-* 事件名/payload

apps/desktop/src/
  lib/transfer.ts                        # invoke 封装 + 事件订阅 + 类型
  features/transfer/TransferWindow.tsx   # 窗口：服务开关/发送/设备/设置
  features/transfer/DeviceList.tsx
  features/transfer/SendPanel.tsx
  features/transfer/ReceiveDialog.tsx
  features/transfer/TransferProgress.tsx
  features/transfer/useTransfer.ts       # 状态与事件归约
```

入口：主窗口顶部「互传」按钮、设置面板「局域网互传」开关、托盘「打开互传」、
`?view=transfer` 路由。窗口标题「互传 · r-todo」，760×560，有系统边框。

### 13.4 已知限制 / 未完成

- **真机互通未验证**：官方 LocalSend（Windows/Android）双向、真实组播、防火墙弹窗、
  睡眠唤醒后 socket 失效、1GB+、中文/重名/多文件，需要用户实机跑 §9。
- 组播在 Windows 多网卡/虚拟网卡环境不稳定；已提供手动 IP 兜底，网卡白/黑名单设置未做。
- v2 服务端单会话：多设备同时发送时后到者会收到 409（UI 显示为未知错误 + 消息）。
- 发送侧进度事件不带文件名（契约限制），UI 按选择顺序补位显示。
- 接收文件落盘用 `.part` + rename；显式指定已存在路径时会覆盖（保存对话框已确认过）。
- 应用退出用 `RunEvent::Exit` + 3s 超时停服，等待端口释放；异常退出由 OS 回收。
- `apps/desktop/src-tauri/vendor/` 已重新生成（699 包）以包含 localsend 的新依赖；
  该目录 gitignore，不进仓库。

---

## 14. 运行态验收记录（2026-11-08 深夜，自动化）

用独立 identifier（`dev.rtodo.desktop.smoke`，端口 53318，接收目录 `E:\r-todo\tmp\transfer-inbox`）
跑真实 app 二进制，通过 WebView2 CDP 驱动，配两个命令行工具（`examples/send.rs`、`examples/serve.rs`）
做双向验收。**没有碰用户的正式配置目录与 D:\ 上正在运行的实例。**

### 14.1 已验证（有证据）

| 项 | 结果 |
|---|---|
| 设置 `enabled=true` → 启动自动开服务 | ✅ 进程起来后 TCP 53318 监听 |
| `transfer_get_state` | ✅ `{running, port, alias, fingerprint, settings{...}, error}` |
| 互传窗口 UI 渲染 | ✅ 截图 + DOM：标题/端口/设备列表/选择文件/错误条都在（暗色） |
| `transfer_add_manual_ip` 跨进程发现 | ✅ 返回对端 `r-todo peer` + fingerprint + `https` channel |
| app 发送 → 对端接收 | ✅ 1.5 MB 文件 SHA-256 完全一致，对端 `ENDED=Completed` |
| 对端发送 → app 接收 | ✅ `transfer_pending` 列出请求 → `transfer_respond_prepare` → 落盘 SHA-256 一致 |
| `transfer_reject` | ✅ 对端收到 403 → `REJECTED` |
| 接收中途取消 | ✅ 3 GB 传到 23% 取消：进度立即停、对端 `会话结束: 已取消`、接收目录无残留 |
| 关窗恢复 | ✅ 关闭互传窗口后重开，接收弹窗仍在（`transfer_pending`） |
| 收到请求自动开窗 | ✅ 窗口关闭状态下发文件，互传窗口自动弹出 |
| `transfer_stop` / `transfer_start` | ✅ 停止后端口释放，再次启动端口/身份不变 |
| 组播 | ✅ 端口 53318 正常（`multicast_probe` 验证） |

### 14.2 踩到的坑（已修）

1. **Windows 端口排除段会让组播整体失败**：`53999` 落在 Hyper-V/WSL 的 UDP 排除范围
   （`netsh int ipv4 show excludedportrange protocol=udp` → 53976–54075），
   `multicast::start` 返回 `No network interface available for multicast`。
   已把三条错误提示改成明确的“端口可能被系统保留/被占用，请换端口或用手动 IP”：
   - `MULTICAST_UNAVAILABLE`（service.rs）
   - `PORT_IN_USE` / `FIREWALL`（server.rs）
2. **窗口重开后接收弹窗丢失**：`Incoming` 事件只在当次窗口生命周期内有效，关掉窗口后发送方
   会一直等。新增 `transfer_pending` 命令 + 前端挂载时补拉，恢复弹窗。
3. **收到请求时互传窗口没开**：事件桥里加“自动开窗 + 延迟补发一次事件”（新窗口 JS 还没来得及订阅）。
4. **接收完成通知**：窗口在后台时发系统通知（`@tauri-apps/plugin-notification`）。
5. 错误文案本地化：PIN/拒绝/对端忙/校验失败给中文提示，不再直接透出 `HTTP 403`。
6. 端口 0（系统分配）时 `refresh`/`send` 用的是真实端口而不是配置端口。
7. `stop()` 里 discovery / server 各加 2s 超时，避免退出被卡住。
8. **接收中途取消无效**（真机测试发现）：`ServerHandle::cancel_v2_session` 只是把会话槽置空，
   既不会中止正在写入的 body，也不会通知发送方；结果进度继续涨、文件照样完整落盘。
   现在取消会：标记会话 → 停进度上报 → 清未回复的上传请求 → **反向给发送方发协议级
   `cancel`** → 删除 `.part` 并在落盘任务收尾时丢弃半成品（不再上报完成）。
   回归测试：`loopback_receiver_cancel_discards_file`（64 MB 中途取消，断言无残留）。
9. 设备离线时的报错原来是 `Send/Unknown: error sending request for url (...)`：
   现改为“连不上该设备：可能已离线或端口不对…”，并自动把该设备从列表摘掉
   （已离线设备常见于对端刚退出）。

### 14.3 关于“异步命令创建窗口”

排查过程中一度怀疑 async command 创建的窗口不出现，最后用同一份代码路径的
`open_month_window` 复现了同样现象：**当前会话处于锁屏状态时，新建窗口的 HWND 不会
真正落地**（`hwnd Unavailable` / `FailedToReceiveMessage`），与我们的实现无关。
`open_transfer_window` 保持与 `open_month_window` 完全一致的写法（async + 单例），
在正常解锁的桌面上可用（首次 CDP 验收时窗口确实出现了）。

### 14.4 新增的工具（可复用）

```powershell
cd crates/transfer-core
# 主动发现问题（列网卡组播能力）
cargo run --example multicast_probe -- 53317

# 向某个对端发文件（用于验收对方的接收 UI / 排查发现不到）
cargo run --example send -- 127.0.0.1 53317 .\a.pdf

# 起一个自动接收的对端（用于验收本端“发送”链路）
cargo run --example serve -- E:\tmp\inbox 53319
```

### 14.5 官方 LocalSend 客户端双向互通（自动化，2026-11-08）

下载官方 **LocalSend CLI 1.18.2 Windows x86-64**（9.3 MB，与官方 Flutter 客户端共用
同一套 `packages/core` 协议实现），在同一台机器上用两个端口跑双向验收：

| 方向 | 做法 | 结果 |
|---|---|---|
| r-todo → 官方客户端 | `examples/send.rs` → 官方 CLI（TCP 53318）；用 CLI 的 `paired-v2.json` 预置我方 fingerprint 实现自动接受 | ✅ `official-interop.txt`（32 B）落到 CLI 的 destination，发送方 `会话结束: 完成` |
| 官方客户端 → r-todo | 官方 CLI `-f <file>` + 设备列表 Enter；我方 `examples/serve.rs`（TCP 53317）自动接受 | ✅ `official-to-rtodo.txt`（32 B）落盘，`INCOMING / SAVED / ENDED=Completed` |

细节与坑：

- 官方 CLI 的 UDP 组播端口固定 **53317**，`--port` 只改 TCP 端口；两台进程可以共用
  53317 的 UDP 组播绑定。
- 本机多网卡（WLAN + WSL + ZeroTier + Tailscale）+ 回环导致**同机组播互相发现不到**，
  所以用官方 CLI 的 `paired-v2.json`（配对设备 = 官方客户端的“手动 IP”）指定
  `127.0.0.1:53317 / HTTPS`。真实手机在同一 Wi-Fi 下走正常组播。
- `paired-v2.json` 格式：`{"version":1,"devices":{"<证书 fingerprint>":{"alias":"...","channels":[{"host":"...","port":...,"protocol":"HTTPS"}]}}}`，
  放在 `%USERPROFILE%\.config\localsend-cli\`（或 `XDG_CONFIG_HOME`）。
- 我方两端都复用官方 `localsend` core 的 client/server，协议版本 `2.2`，因此与官方
  客户端互通由构造保证；真机测试主要验证的是 Flutter 客户端 UI、Android 存储/网络行为和
  真实 Wi-Fi 组播。

### 14.6 交付给真机测试的单文件 exe

- 构建：`cargo build --release --bin r-todo-desktop`（release profile，前端由 tauri-build 内嵌）
- 产物：`apps/desktop/src-tauri/target/release/r-todo-desktop.exe`，29.9 MB
- 打包：`E:\r-todo\tmp\r-todo-transfer-preview.zip`，10 MB（免安装，解压即用）
- 已冒烟：启动后窗口正常、按 `transfer-config.json` 自动起服务、53317 监听
- 注意：identifier 仍是 `dev.rtodo.desktop` 且开了**单实例**插件，运行前必须退出自己电脑上
  已安装的正式版，否则会被旧实例接管；同时会共用 `%APPDATA%\dev.rtodo.desktop` 配置目录

### 14.7 仍需真机确认（无法自动化）

- 官方 LocalSend（Windows/Android）双向互通、真实组播、防火墙首次弹窗
- 带 PIN 的对端实测（协议层已用回环测试覆盖 `PIN_REQUIRED` / 正确 PIN 两条路径；
  发送面板已提供“对方 PIN”输入框，但真实设备上的输入体验待确认）
- 1 GB+ 文件、睡眠唤醒后 socket 失效
- 解锁桌面后人工点一次「互传」按钮、托盘「打开互传」、接收弹窗的实际观感
- 多网卡（WLAN + ZeroTier + Tailscale + WSL）下实际选中的网卡是否符合预期
