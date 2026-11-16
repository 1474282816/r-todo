# 实施计划 · Avalonia 桌面迁移与 Rust 同步服务

> 设计：`docs/superpowers/specs/2026-11-16-avalonia-rust-desktop-sync-design.md`
> 设计提交：`45cd97c`
> 纪律：每个任务先写失败测试，再写最少实现；每个任务独立验证、独立提交。
> 目标：新 Avalonia 客户端完整对齐现有桌面功能，桌面与手机通过新 Rust 同步服务双向同步，现有待办通过 r-todo JSON 保留。

## 执行原则

1. **阶段 0 是硬闸门。** Avalonia 挂件、系统托盘、Toast、安装和更新没有通过真机验证前，不开始大规模 UI 重写。
2. **业务只有一个入口。** UI 不访问 SQLite、同步 API 或互传文件内容；所有业务经 Engine RPC。
3. **协议先于实现。** 任务模型、同步协议和 UI RPC 先写契约测试，再写服务端和客户端。
4. **旧客户端保持可用。** 新客户端验收完成前，不删除 Tauri 实现，不停止旧 PocketBase 的备份。
5. **新 Rust 项目独立成 workspace。** 不把根目录直接改成 Cargo workspace，避免改变现有 Tauri 和移动端 FRB 的 `target` 与构建行为。
6. **一次提交只解决一个任务。** 提交消息使用中文，格式为 `feat(scope): ...`、`test(scope): ...`、`docs(scope): ...`。

## 前置环境

### E0.1 恢复 Rust 默认工具链

```powershell
rustup default stable
rustup component add rustfmt clippy
rustc --version
cargo --version
```

验收：两个版本命令成功，且 `rustup show active-toolchain` 不再提示未配置。

### E0.2 安装 .NET 10 SDK 和 Windows 构建工具

安装：

- .NET 10 SDK，最低 `10.0.115`
- Visual Studio 2022 Build Tools 或 Visual Studio 2026
- Desktop development with C++
- Windows 11 SDK
- Windows App Runtime 最新稳定版，仅用于 WinUI 对照验证，不进入主项目

验证：

```powershell
dotnet --list-sdks
dotnet --info
```

验收：列出 10.0 SDK，`dotnet --info` 的 OS 和架构为 Windows x64。

### E0.3 确认 Node、pnpm 和旧客户端构建

```powershell
node --version
pnpm --version
pnpm --filter desktop build
```

验收：旧前端仍可构建。后续新项目不得修改该命令的语义。

## 阶段 0 · Avalonia 高风险验证

### T0.1 建立独立 Avalonia Spike 项目

文件：

```text
apps/desktop-avalonia/
├── r-todo.slnx
├── Directory.Build.props
├── Directory.Packages.props
├── src/
│   ├── RTodo.Desktop/
│   │   ├── RTodo.Desktop.csproj
│   │   ├── Program.cs
│   │   ├── App.axaml
│   │   └── App.axaml.cs
│   └── RTodo.Desktop.Tests/
│       └── RTodo.Desktop.Tests.csproj
└── spike/
    └── SPIKE.md
```

固定依赖：

```text
Avalonia 12.1.6
Avalonia.Desktop 12.1.6
Avalonia.Themes.Fluent 12.1.6
Avalonia.Fonts.Inter 12.1.6
CommunityToolkit.Mvvm 8.4.2
Microsoft.Extensions.DependencyInjection 10.0.15
Avalonia.Headless.XUnit 12.1.6
xunit 2.9.3
xunit.runner.visualstudio 4.0.1
Microsoft.NET.Test.Sdk 18.12.0
```

验证：

```powershell
dotnet restore apps/desktop-avalonia/r-todo.slnx
dotnet build apps/desktop-avalonia/r-todo.slnx -c Debug
dotnet test apps/desktop-avalonia/r-todo.slnx -c Debug
```

提交：`feat(avalonia): 建立桌面客户端项目骨架`

### T0.2 验证透明无边框窗口与高 DPI

文件：

```text
src/RTodo.Desktop/Windows/MainWindow.axaml
src/RTodo.Desktop/Windows/MainWindow.axaml.cs
src/RTodo.Desktop/Services/DpiProbe.cs
src/RTodo.Desktop.Tests/Windows/WindowOptionsTests.cs
```

实现：

- `SystemDecorations=None`
- 透明背景
- 自绘拖动区
- 最小尺寸 320×480
- 初始尺寸 380×620
- 读取每显示器缩放
- 在 100%、125%、150%、200% 下记录实际像素边界

测试：

- ViewModel 在四种 DPI 下生成正确逻辑尺寸。
- 窗口外框关闭时仍可拖动、缩放和关闭。

手工验收：

- 在多显示器不同 DPI 间移动，文字和边框不模糊。
- 拖动和缩放不闪白、不黑边。

记录到 `spike/SPIKE.md`：操作系统版本、显示器、缩放比例、截图、异常。

提交：`feat(avalonia): 验证无边框窗口与高 DPI`

### T0.3 验证 Win11 材质

文件：

```text
src/RTodo.Desktop/Services/WindowMaterialService.cs
src/RTodo.Desktop/Windows/MainWindow.axaml
src/RTodo.Desktop.Tests/Services/WindowMaterialTests.cs
```

实现：

- 用 Avalonia 的透明层或原生 DWM 调用设置 Mica。
- 提供 Mica、Mica Alt、Acrylic、半透明和关闭五个档位。
- 窗口失焦后保持可读，不出现纯黑。
- 无法使用系统材质时回退到半透明。

测试：

- 材质名称解析。
- 不支持时选择回退档位。
- 用户显式选择和默认值迁移。

手工验收：

- 聚焦和失焦截图。
- 更换深浅主题、壁纸和显示器缩放后复查。
- 记录每种材质的 GPU 使用和拖动表现。

提交：`feat(avalonia): 验证窗口材质与回退`

### T0.4 验证桌面挂件

文件：

```text
src/RTodo.Desktop/Interop/Win32Native.cs
src/RTodo.Desktop/Interop/DesktopWidgetController.cs
src/RTodo.Desktop.Tests/Interop/DesktopWidgetTests.cs
```

实现：

- 获取 Avalonia 顶层窗口 `HWND`。
- 设置 owner 为 `Progman`。
- 清除 `WS_EX_APPWINDOW`，设置 `WS_EX_TOOLWINDOW`。
- 非前台时置 `HWND_BOTTOM`。
- 用 `Win32Properties.AddWndProcHookCallback` 拦截 `SC_MINIMIZE`。
- 监听 `EVENT_SYSTEM_FOREGROUND`。
- 监听 `TaskbarCreated`。
- 挂件不可见时回滚。

测试：

- 样式位计算的纯函数测试。
- owner 重挂状态机测试。
- 前台窗口等于自身时不置底。
- 隐藏后回滚。

手工验收：

- 普通桌面和 Win+D。
- 点击挂件、点击其他应用、最小化其他应用。
- Explorer 通过任务管理器重启。
- 多显示器和跨显示器拖动。
- 任务栏、Alt+Tab、UAC 提示和全屏应用。

这是硬闸门。任一核心行为不稳定，停止后续迁移并先修复；若框架限制无法修复，按设计文档重新评估方案。

提交：`feat(avalonia): 验证桌面挂件 Win32 行为`

### T0.5 验证托盘、Toast、单实例和开机启动

文件：

```text
src/RTodo.Desktop/Services/TrayService.cs
src/RTodo.Desktop/Services/NotificationService.cs
src/RTodo.Desktop/Services/SingleInstanceService.cs
src/RTodo.Desktop/Services/StartupService.cs
src/RTodo.Desktop.Tests/Services/ShellIntegrationTests.cs
```

实现：

- 托盘菜单：显示、隐藏、打开互传、退出。
- 双击托盘显示主窗口。
- Toast 点击回调携带任务 ID。
- 第二实例激活已有窗口。
- 开机启动开关写入当前用户启动项。

测试：

- 单实例参数解析。
- Toast 回调载荷解析。
- 启动项命令引号处理。
- 托盘菜单动作映射。

手工验收：

- 开机后不出现两个窗口。
- 隐藏窗口退出后重启可见。
- 通知权限关闭时给出可操作提示。

提交：`feat(avalonia): 验证托盘通知与单实例`

### T0.6 验证安装与更新链路

文件：

```text
installer/RTodo.Installer.wixproj
installer/Package.wxs
scripts/release-desktop-avalonia.ps1
src/RTodo.Desktop/Services/UpdateService.cs
src/RTodo.Desktop.Tests/Services/UpdateServiceTests.cs
```

实现：

- 单个安装包包含 `rtodo-ui.exe` 和 `rtodo-engine.exe`。
- 安装到当前用户目录。
- 创建开始菜单和桌面快捷方式。
- 注册卸载信息。
- 更新清单包含版本、架构、下载地址、SHA-256 和签名。
- 下载完成后校验签名和哈希。
- 安装失败时保留当前版本。

测试：

- 清单解析、签名失败、哈希失败、版本比较。
- 安装器路径和文件名契约。

手工验收：

- 全新用户安装。
- 普通用户升级。
- 安装过程中应用运行。
- 更新包损坏。
- 卸载后保留用户数据。

提交：`feat(avalonia): 验证安装与更新链路`

### T0.7 记录阶段 0 结论

文件：

```text
spike/SPIKE.md
docs/decisions/004-avalonia-desktop.md
```

内容：

- 环境版本。
- 所有验收截图和步骤。
- 挂件、材质、DPI、托盘、Toast、安装和更新的结果。
- 与 Tauri 的对比。
- 是否继续迁移的明确结论。

提交：`docs(avalonia): 记录迁移可行性验证`

## 阶段 1 · 共享核心与桌面 Engine

### T1.1 建立独立 Rust workspace

文件：

```text
crates/rtodo-workspace/Cargo.toml
crates/rtodo-workspace/Cargo.lock
crates/rtodo-domain/Cargo.toml
crates/rtodo-domain/src/lib.rs
crates/rtodo-sync-proto/Cargo.toml
crates/rtodo-sync-proto/src/lib.rs
crates/rtodo-engine/Cargo.toml
crates/rtodo-engine/src/main.rs
crates/rtodo-syncd/Cargo.toml
crates/rtodo-syncd/src/main.rs
```

规则：

- workspace 位于 `crates/rtodo-workspace`，成员为四个新 crate。
- `crates/transfer-core` 保持独立，由新 Engine 通过 path 依赖引用。
- 不在仓库根目录增加 `[workspace]`。

验证：

```powershell
cargo test --manifest-path crates/rtodo-workspace/Cargo.toml
cargo clippy --manifest-path crates/rtodo-workspace/Cargo.toml --all-targets -- -D warnings
```

提交：`feat(core): 建立独立 Rust workspace`

### T1.2 实现任务领域模型

文件：

```text
crates/rtodo-domain/src/task.rs
crates/rtodo-domain/src/priority.rs
crates/rtodo-domain/src/validation.rs
crates/rtodo-domain/tests/task_contract.rs
```

先写失败测试：

- 标题去空白后为空。
- 标题超过 500 字符。
- 非法优先级。
- 负提醒偏移。
- UTC 时间解析。
- 软删除判定。
- 活跃任务排序。
- 已完成任务排序。

实现：

- `Task`
- `Priority`
- `TaskPatch`
- `TaskValidationError`
- `TaskSort`

验证：

```powershell
cargo test --manifest-path crates/rtodo-workspace/Cargo.toml -p rtodo-domain
```

提交：`feat(domain): 实现任务模型与校验`

### T1.3 实现 JSON 导入契约

文件：

```text
crates/rtodo-domain/src/import.rs
crates/rtodo-domain/tests/json_import.rs
crates/rtodo-domain/testdata/r-todo-v1.json
```

先写失败测试：

- 正确格式导入。
- 错误 format。
- 不支持的 version。
- todos 非数组。
- 缺字段。
- 重复 ID。
- `deleted=true` 保留为 tombstone。
- 预览不写数据库。
- 整批校验失败时没有部分提交。

实现：

- `parse_r_todo_json`
- `ImportPreview`
- `ImportIssue`
- `ImportPlan`
- `plan_merge`

合并规则：

- 空库全部新增。
- 非空库按 ID 和 `updated_at` 生成新增、更新、跳过。
- 同一批内重复 ID 报错。

提交：`feat(domain): 实现待办 JSON 导入计划`

### T1.4 实现 Engine SQLite schema

文件：

```text
crates/rtodo-engine/src/db/mod.rs
crates/rtodo-engine/src/db/migrations.rs
crates/rtodo-engine/tests/db_migrations.rs
```

表：

```text
tasks
outbox
sync_state
settings
reminders
conversations
messages
transfer_outbox
trusted_peers
schema_meta
```

先写失败测试：

- 从空库迁移到最新版本。
- 重复启动幂等。
- 未知更高版本拒绝写入。
- WAL 和外键启用。
- 事务回滚。
- SQLite 备份 API。

实现：

- `Database::open`
- `Database::migrate`
- `Database::backup_to`
- `Database::integrity_check`

提交：`feat(engine): 实现 SQLite schema 与迁移`

### T1.5 实现任务服务和 outbox

文件：

```text
crates/rtodo-engine/src/tasks/mod.rs
crates/rtodo-engine/src/tasks/service.rs
crates/rtodo-engine/src/outbox/mod.rs
crates/rtodo-engine/tests/tasks.rs
crates/rtodo-engine/tests/outbox.rs
```

先写失败测试：

- 新增任务置顶。
- 编辑任务更新时间。
- 完成和取消完成。
- 软删除。
- 重排。
- 任务和 outbox 在同一事务。
- 写失败不产生 outbox。
- 未登录也可写 outbox。
- 重复操作 ID 不重复入队。

实现：

- `TaskService`
- `TaskCommand`
- `TaskEvent`
- `OutboxEntry`
- `operation_id`

提交：`feat(engine): 实现任务事务与离线队列`

### T1.6 实现 JSON、ICS 和 CSV 导入导出

文件：

```text
crates/rtodo-engine/src/io/json.rs
crates/rtodo-engine/src/io/ical.rs
crates/rtodo-engine/src/io/ticktick.rs
crates/rtodo-engine/tests/io_import.rs
```

先写失败测试：

- r-todo JSON 往返。
- 预览、确认和重复导入。
- ICS VTODO 往返。
- TickTick CSV 导入。
- 格式错误。
- 事务回滚。

实现：

- `preview_import`
- `commit_import`
- `export_json`
- `export_ical`

提交：`feat(engine): 实现待办导入导出`

### T1.7 实现 Engine 进程和 Named Pipe

文件：

```text
crates/rtodo-engine/src/main.rs
crates/rtodo-engine/src/rpc/mod.rs
crates/rtodo-engine/src/rpc/pipe.rs
crates/rtodo-engine/src/rpc/protocol.rs
crates/rtodo-engine/tests/rpc_contract.rs
crates/rtodo-engine/tests/rpc_pipe.rs
```

协议：

```text
4-byte big-endian length + UTF-8 JSON
```

消息：

```text
protocol_version
message_type
request_id?
method?
payload
```

先写失败测试：

- 请求和响应关联。
- 未知方法返回稳定错误码。
- 畸形长度拒绝。
- 超大消息拒绝。
- 事件没有请求 ID。
- 管道断开。
- 客户端重连。
- 只允许当前用户。

实现：

- `EngineServer`
- `RpcDispatcher`
- `PipeSecurity`
- `EngineEventSink`
- `system.health`
- `system.shutdown`

提交：`feat(engine): 实现桌面 RPC 与命名管道`

### T1.8 实现提醒

文件：

```text
crates/rtodo-engine/src/reminders/mod.rs
crates/rtodo-engine/src/reminders/scheduler.rs
crates/rtodo-engine/tests/reminders.rs
```

先写失败测试：

- 提前提醒。
- 准点提醒。
- 修改时间后重排。
- 完成任务取消提醒。
- 删除任务取消提醒。
- 重启重建未来提醒。
- 过期提醒不补弹。
- RRULE 下一实例。

实现：

- `ReminderScheduler`
- `reminder.due`
- SQLite 持久化

提交：`feat(engine): 实现持久化提醒调度`

## 阶段 2 · Rust 同步服务

### T2.1 定义同步协议

文件：

```text
crates/rtodo-sync-proto/src/lib.rs
crates/rtodo-sync-proto/src/auth.rs
crates/rtodo-sync-proto/src/operations.rs
crates/rtodo-sync-proto/src/changes.rs
crates/rtodo-sync-proto/src/events.rs
crates/rtodo-sync-proto/src/error.rs
crates/rtodo-sync-proto/tests/contract.rs
```

冻结：

- 配对请求和响应。
- 设备列表。
- 操作批提交。
- 变更拉取。
- 快照。
- WebSocket 通知。
- 稳定错误码。

先写失败契约测试：

- 字段命名。
- 可空字段。
- 时间格式。
- 枚举值。
- 未知字段策略。
- schema version。

提交：`feat(sync-proto): 冻结同步协议契约`

### T2.2 建立同步服务数据库

文件：

```text
crates/rtodo-syncd/src/db/mod.rs
crates/rtodo-syncd/src/db/migrations.rs
crates/rtodo-syncd/tests/db_migrations.rs
```

表：

```text
tasks
changes
operations
devices
pairing_codes
server_meta
```

先写失败测试：

- 空库迁移。
- 幂等启动。
- 未知 schema。
- 事务回滚。
- 备份。
- 完整性检查。

提交：`feat(syncd): 实现服务端 SQLite`

### T2.3 实现配对码和设备令牌

文件：

```text
crates/rtodo-syncd/src/auth/mod.rs
crates/rtodo-syncd/src/auth/pairing.rs
crates/rtodo-syncd/src/auth/tokens.rs
crates/rtodo-syncd/tests/auth.rs
```

先写失败测试：

- 8 位 Base32 接入码。
- 10 分钟过期。
- 单次使用。
- 错误接入码。
- 限速。
- 令牌哈希保存。
- Bearer token 校验。
- 撤销设备。
- 撤销后立即拒绝。

实现：

- `PairingCodeService`
- `DeviceAuthService`
- `AuthenticatedDevice`

命令行：

```text
rtodo-syncd pair
rtodo-syncd devices
rtodo-syncd revoke <device-id>
```

提交：`feat(syncd): 实现设备配对与令牌`

### T2.4 实现任务操作和幂等

文件：

```text
crates/rtodo-syncd/src/tasks/mod.rs
crates/rtodo-syncd/src/tasks/operations.rs
crates/rtodo-syncd/src/tasks/conflicts.rs
crates/rtodo-syncd/tests/operations.rs
```

先写失败测试：

- 创建任务。
- 更新任务。
- 完成和取消完成。
- 删除 tombstone。
- 取消删除。
- 重复 operation_id。
- base_revision 相同。
- base_revision 落后。
- 同任务并发更新。
- 操作和变更日志原子提交。

提交：`feat(syncd): 实现幂等任务操作与冲突裁决`

### T2.5 实现游标、变更和快照

文件：

```text
crates/rtodo-syncd/src/changes/mod.rs
crates/rtodo-syncd/src/changes/cursor.rs
crates/rtodo-syncd/src/changes/snapshot.rs
crates/rtodo-syncd/tests/changes.rs
```

先写失败测试：

- 游标单调递增。
- 按游标升序返回。
- 同一游标重复拉取幂等。
- tombstone 传播。
- 清理窗口。
- 过期游标返回 `resync_required`。
- 快照包含活跃任务。
- 快照版本与游标一致。

提交：`feat(syncd): 实现增量游标与快照`

### T2.6 实现 HTTP API

文件：

```text
crates/rtodo-syncd/src/http/mod.rs
crates/rtodo-syncd/src/http/auth_middleware.rs
crates/rtodo-syncd/src/http/routes.rs
crates/rtodo-syncd/tests/http_api.rs
```

接口：

```text
GET    /api/v1/health
POST   /api/v1/pair
POST   /api/v1/pairing-codes
GET    /api/v1/devices
DELETE /api/v1/devices/:id
GET    /api/v1/changes
GET    /api/v1/snapshot
POST   /api/v1/operations
```

先写失败测试：

- 无 token 401。
- 无效 token 401。
- 撤销 token 401。
- 健康检查无认证。
- 配对限速。
- 请求体限制。
- 错误 JSON 稳定。
- 数据库失败返回可重试错误。

提交：`feat(syncd): 实现同步 HTTP API`

### T2.7 实现 WebSocket 通知

文件：

```text
crates/rtodo-syncd/src/http/events.rs
crates/rtodo-syncd/tests/websocket.rs
```

先写失败测试：

- 认证后连接。
- 新变更发送 `changes.available`。
- 连接断开。
- 重连。
- 多设备同时订阅。
- 通知不包含任务正文。

提交：`feat(syncd): 实现 WebSocket 变更通知`

### T2.8 服务端部署与恢复

文件：

```text
crates/rtodo-syncd/src/cli.rs
crates/rtodo-syncd/src/backup.rs
crates/rtodo-syncd/tests/backup_restore.rs
deploy/rtodo-syncd/compose.yaml
deploy/rtodo-syncd/Caddyfile
deploy/rtodo-syncd/README.md
```

命令：

```text
rtodo-syncd serve
rtodo-syncd pair
rtodo-syncd backup
rtodo-syncd check
```

验证：

- 全新目录启动。
- 每日备份。
- 从备份恢复。
- 服务重启。
- 只读数据库目录错误。
- 日志轮转。

提交：`feat(syncd): 完成部署备份与恢复`

## 阶段 3 · Avalonia 主客户端

### T3.1 实现 Engine 生命周期和 RPC 客户端

文件：

```text
src/RTodo.Desktop/Services/EngineProcess.cs
src/RTodo.Desktop/Services/EngineClient.cs
src/RTodo.Desktop/Services/EngineRpcModels.cs
src/RTodo.Desktop.Tests/Services/EngineClientTests.cs
```

先写失败测试：

- 启动 Engine。
- 随机管道名。
- 请求超时。
- 断线。
- 有限重启。
- 事件订阅。
- 协议不兼容。
- 退出清理。

提交：`feat(ui): 实现 Engine 生命周期与 RPC 客户端`

### T3.2 实现应用壳、导航和主题

文件：

```text
src/RTodo.Desktop/App.axaml
src/RTodo.Desktop/ViewModels/ShellViewModel.cs
src/RTodo.Desktop/Views/ShellView.axaml
src/RTodo.Desktop/Themes/Tokens.axaml
src/RTodo.Desktop.Tests/ViewModels/ShellViewModelTests.cs
```

实现：

- 主窗口
- 设置页
- 月视图
- 互传页
- 聊天页
- 主题和间距 token
- 错误提示区

先写 ViewModel 失败测试：

- 初始加载。
- 页面切换。
- Engine 不可用。
- 同步状态。

提交：`feat(ui): 建立 Avalonia 应用壳`

### T3.3 实现任务列表与快速新增

文件：

```text
src/RTodo.Desktop/Features/Tasks/TaskListViewModel.cs
src/RTodo.Desktop/Features/Tasks/TaskRowViewModel.cs
src/RTodo.Desktop/Features/Tasks/QuickAddViewModel.cs
src/RTodo.Desktop/Views/Tasks/TaskListView.axaml
src/RTodo.Desktop/Views/Tasks/TaskRowView.axaml
src/RTodo.Desktop.Tests/Tasks/TaskListViewModelTests.cs
```

先写失败测试：

- 加载任务。
- 活跃和完成排序。
- 新增。
- 乐观更新。
- 写失败回滚。
- 空标题。
- 大量任务虚拟化。

提交：`feat(ui): 实现任务列表与快速新增`

### T3.4 实现任务编辑、完成和删除

文件：

```text
src/RTodo.Desktop/Features/Tasks/TaskEditorViewModel.cs
src/RTodo.Desktop/Views/Tasks/TaskEditorView.axaml
src/RTodo.Desktop.Tests/Tasks/TaskEditorViewModelTests.cs
```

先写失败测试：

- 标题。
- 优先级。
- 提醒。
- 重复规则。
- 完成。
- 取消完成。
- 删除。
- 编辑失败。

提交：`feat(ui): 实现任务编辑与状态操作`

### T3.5 实现拖拽排序

文件：

```text
src/RTodo.Desktop/Features/Tasks/TaskReorderController.cs
src/RTodo.Desktop.Tests/Tasks/TaskReorderTests.cs
```

先写失败测试：

- 移动到顶部。
- 移动到底部。
- 相邻交换。
- 乐观排序。
- 写入失败回滚。

提交：`feat(ui): 实现任务拖拽排序`

### T3.6 实现月视图

文件：

```text
src/RTodo.Desktop/Features/Calendar/MonthViewModel.cs
src/RTodo.Desktop/Views/Calendar/MonthWindow.axaml
src/RTodo.Desktop.Tests/Calendar/MonthViewModelTests.cs
```

先写失败测试：

- 当前月。
- 跨年切换。
- 已完成记录。
- 独立窗口单实例。
- 窗口关闭后重开。

提交：`feat(ui): 实现月视图`

### T3.7 实现提醒与通知

文件：

```text
src/RTodo.Desktop/Services/ReminderNotificationService.cs
src/RTodo.Desktop.Tests/Services/ReminderNotificationTests.cs
```

先写失败测试：

- 收到 `reminder.due`。
- 点击通知定位任务。
- 权限拒绝。
- Engine 重启后恢复订阅。

提交：`feat(ui): 接入提醒与任务通知`

### T3.8 实现同步、配对和设备管理

文件：

```text
src/RTodo.Desktop/Features/Sync/SyncViewModel.cs
src/RTodo.Desktop/Features/Sync/DeviceListViewModel.cs
src/RTodo.Desktop/Views/Sync/SyncSettingsView.axaml
src/RTodo.Desktop.Tests/Sync/SyncViewModelTests.cs
```

先写失败测试：

- 服务器 URL。
- 配对码。
- 配对成功。
- 配对失败。
- 离线状态。
- 立即同步。
- 设备列表。
- 撤销设备。
- 令牌失效。

提交：`feat(ui): 实现同步配对与设备管理`

### T3.9 实现导入导出

文件：

```text
src/RTodo.Desktop/Features/ImportExport/ImportExportViewModel.cs
src/RTodo.Desktop/Views/Settings/ImportExportView.axaml
src/RTodo.Desktop.Tests/ImportExport/ImportExportViewModelTests.cs
```

先写失败测试：

- JSON 文件选择。
- 预览。
- 字段错误。
- 确认导入。
- 取消导入。
- JSON 导出。
- ICS 导出。
- TickTick CSV 导入。

提交：`feat(ui): 实现数据导入导出`

### T3.10 实现设置、托盘和窗口状态

文件：

```text
src/RTodo.Desktop/Features/Settings/SettingsViewModel.cs
src/RTodo.Desktop/Services/WindowStateService.cs
src/RTodo.Desktop.Tests/Settings/SettingsViewModelTests.cs
src/RTodo.Desktop.Tests/Services/WindowStateTests.cs
```

实现：

- 外观、透明度、挂件模式。
- 开机启动。
- 同步。
- MCP。
- 互传。
- 更新。
- 不恢复隐藏状态。
- 屏幕外窗口回中心。

提交：`feat(ui): 实现设置与窗口状态`

## 阶段 4 · 桌面扩展能力

### T4.1 将 Spike 挂件代码产品化

文件：

```text
src/RTodo.Desktop/Interop/DesktopWidgetController.cs
src/RTodo.Desktop/Features/Widget/WidgetViewModel.cs
src/RTodo.Desktop.Tests/Widget/WidgetStateTests.cs
```

先写失败测试：

- 进入和退出挂件。
- owner 丢失后重挂。
- 不可见回滚。
- 材质切换。
- 窗口状态恢复。

提交：`feat(ui): 完成桌面挂件模式`

### T4.2 实现吸附和收起

文件：

```text
src/RTodo.Desktop/Interop/WindowSnapService.cs
src/RTodo.Desktop.Tests/Interop/WindowSnapTests.cs
```

先写失败测试：

- 左边缘。
- 右边缘。
- 上边缘。
- 多显示器。
- 收起和展开。
- 屏幕工作区变化。

提交：`feat(ui): 实现窗口吸附与收起`

### T4.3 接入 MCP

文件：

```text
crates/rtodo-engine/src/mcp/mod.rs
crates/rtodo-engine/src/mcp/server.rs
crates/rtodo-engine/src/mcp/tools.rs
crates/rtodo-engine/src/mcp/config.rs
crates/rtodo-engine/tests/mcp.rs
src/RTodo.Desktop/Features/Mcp/McpViewModel.cs
```

先写失败测试：

- 默认关闭。
- 开启。
- 关闭。
- 轮换 token。
- 认证。
- 六个工具。
- 写操作产生 outbox。
- 写操作通知 UI。
- 畸形请求。

提交：`feat(mcp): 将 MCP 迁移到 Engine`

### T4.4 接入 LocalSend 和聊天式互传

文件：

```text
crates/rtodo-engine/src/transfer/mod.rs
crates/rtodo-engine/src/transfer/settings.rs
crates/rtodo-engine/src/transfer/events.rs
crates/rtodo-engine/src/chat/mod.rs
crates/rtodo-engine/src/chat/store.rs
crates/rtodo-engine/tests/transfer_contract.rs
src/RTodo.Desktop/Features/Transfer/TransferViewModel.cs
src/RTodo.Desktop/Features/Chat/ChatViewModel.cs
```

先写失败测试：

- transfer-core 事件映射。
- 启动和停止。
- 设备发现。
- 手动 IP。
- 发送。
- 接收确认。
- 拒绝和取消。
- 会话。
- 未读。
- 信任设备。
- 转发。
- 文件路径不经过 RPC 正文。

提交：`feat(transfer): 迁移互传与聊天到 Engine`

### T4.5 接入更新

文件：

```text
src/RTodo.Desktop/Features/Updates/UpdateViewModel.cs
src/RTodo.Desktop/Services/UpdateInstaller.cs
src/RTodo.Desktop.Tests/Updates/UpdateViewModelTests.cs
```

先写失败测试：

- 无更新。
- 有更新。
- 下载失败。
- 校验失败。
- 安装失败。
- 用户取消。

提交：`feat(ui): 接入桌面自动更新`

## 阶段 5 · 手机端同步迁移

### T5.1 增加同步协议客户端和契约样例

文件：

```text
apps/mobile/lib/sync/sync_api.dart
apps/mobile/lib/sync/sync_models.dart
apps/mobile/test/sync_contract_test.dart
apps/mobile/test/fixtures/sync/
```

先写失败测试：

- 配对请求。
- 操作批。
- 变更页。
- 快照。
- 错误码。
- 未知事件。

提交：`feat(mobile-sync): 增加新同步协议客户端`

### T5.2 迁移手机 SQLite

文件：

```text
apps/mobile/lib/data/database.dart
apps/mobile/lib/data/database.g.dart
apps/mobile/lib/data/task.dart
apps/mobile/test/data/sync_migration_test.dart
```

先写失败测试：

- 任务增加 revision。
- outbox 增加 operation_id。
- 同步游标持久化。
- 旧 tasks 表数据保留。
- 旧 PocketBase token 清除。

提交：`feat(mobile): 迁移本地数据库以支持新同步`

### T5.3 实现安全令牌存储

文件：

```text
apps/mobile/lib/sync/device_token_store.dart
apps/mobile/test/sync/device_token_store_test.dart
apps/mobile/pubspec.yaml
```

先写失败测试：

- 保存和读取。
- 覆盖。
- 删除。
- 设备撤销后清除。

实现：

- 使用 Android Keystore 支持的 `flutter_secure_storage`。
- iOS 使用 Keychain。

提交：`feat(mobile): 安全保存设备令牌`

### T5.4 实现手机同步引擎

文件：

```text
apps/mobile/lib/sync/sync_service.dart
apps/mobile/lib/sync/outbox_flusher.dart
apps/mobile/lib/sync/realtime_client.dart
apps/mobile/test/sync/sync_service_test.dart
```

先写失败测试：

- 登录和配对。
- 本地写 outbox。
- 批量提交。
- 幂等重试。
- 游标拉取。
- WebSocket 通知。
- 断线重连。
- 快照重同步。
- 撤销设备。

提交：`feat(mobile-sync): 实现增量同步客户端`

### T5.5 更新手机设置页

文件：

```text
apps/mobile/lib/ui/settings_page.dart
apps/mobile/test/sync/settings_page_test.dart
```

实现：

- 服务器 URL。
- 配对码。
- 设备名称。
- 立即同步。
- 待同步数量。
- 最后同步时间。
- 设备撤销。

先写失败测试：

- 未配对。
- 配对中。
- 已连接。
- 离线。
- 令牌失效。
- 配对码错误。

提交：`feat(mobile): 更新同步设置页面`

### T5.6 移除 PocketBase 客户端

文件：

```text
apps/mobile/pubspec.yaml
apps/mobile/pubspec.lock
apps/mobile/lib/sync/sync_service.dart
```

验证：

```powershell
rg -n "pocketbase|authStore|PB_CONNECT" apps/mobile/lib apps/mobile/pubspec.yaml
flutter analyze
flutter test
```

验收：除历史迁移说明外，手机运行代码没有 PocketBase 依赖。

提交：`refactor(mobile): 移除 PocketBase 客户端`

## 阶段 6 · 端到端验收与发布

### T6.1 实现 CLI 同步验证工具

文件：

```text
crates/rtodo-syncd/src/bin/rtodo-sync-test.rs
crates/rtodo-syncd/tests/e2e_sync.rs
```

命令：

```text
rtodo-sync-test pair
rtodo-sync-test push
rtodo-sync-test pull
rtodo-sync-test conflict
rtodo-sync-test reconnect
```

先写失败测试：

- 两台模拟设备。
- 离线修改。
- 重连。
- 冲突。
- 快照。
- 撤销。

提交：`test(sync): 增加双设备端到端验证工具`

### T6.2 完成 JSON 迁移验收

文件：

```text
docs/handoff/2026-11-16-r-todo-data-migration.md
scripts/verify-todo-json.ps1
```

验证：

- 从旧客户端导出真实 JSON。
- 统计任务总数。
- 统计完成和未完成数量。
- 统计提醒和重复任务数量。
- 新客户端导入。
- 新客户端统计与旧 JSON 一致。
- 桌面同步到服务端。
- 手机配对后拉取。
- 手机任务总数与桌面一致。

提交：`docs(migration): 记录待办 JSON 迁移验收`

### T6.3 完成桌面功能对齐验收

文件：

```text
docs/handoff/2026-11-16-avalonia-feature-parity.md
```

逐项核对：

- 任务 CRUD。
- 快速新增。
- 完成和取消完成。
- 手动排序。
- 提醒。
- 重复任务。
- 月视图。
- 设置。
- 同步。
- 设备管理。
- MCP。
- 互传。
- 聊天。
- 导入导出。
- 托盘。
- 自启动。
- 单实例。
- 窗口状态。
- 吸附。
- 收起。
- 挂件。
- 材质。
- 更新。

每一项记录自动化测试、真机步骤和结果。不能只写“已实现”。

提交：`docs(avalonia): 记录功能对齐验收`

### T6.4 完成同步和互传真机验收

文件：

```text
docs/handoff/2026-11-16-avalonia-sync-transfer-acceptance.md
```

覆盖设计文档第 14.2 节全部场景，并记录：

- 设备型号。
- Windows 和 Android 版本。
- 网络环境。
- 服务端版本。
- 日志摘要。
- 失败复现步骤。

提交：`docs(sync): 记录桌面手机同步与互传验收`

### T6.5 完成性能验收

文件：

```text
docs/handoff/2026-11-16-avalonia-performance.md
scripts/measure-desktop.ps1
```

测量：

- 冷启动。
- 任务列表滚动。
- 本地操作落库。
- 100 条增量同步。
- WebSocket 重连。
- 空闲 CPU。
- UI、Engine 和 Syncd 内存。
- 挂件前台切换。

提交：`perf(desktop): 记录 Avalonia 性能验收`

### T6.6 建立正式发布脚本

文件：

```text
scripts/release-desktop-avalonia.ps1
scripts/release-syncd.ps1
docs/release/desktop-avalonia.md
docs/release/syncd.md
```

要求：

- 发布前工作区干净。
- 构建 UI 和 Engine。
- 运行全部 Rust、.NET 和 Flutter 测试。
- 生成安装包。
- 校验包内容。
- 签名更新清单。
- 上传 GitHub Release。
- 发布服务端二进制和 SHA-256。
- 不输出令牌或签名私钥。

提交：`build(release): 增加 Avalonia 与服务端发布流程`

### T6.7 部署新同步服务

文件：

```text
docs/handoff/2026-11-16-rtodo-syncd-deployment.md
deploy/rtodo-syncd/compose.yaml
deploy/rtodo-syncd/Caddyfile
```

步骤：

- 保留 PocketBase 数据和容器。
- 新服务使用独立端口和域名。
- 配置 TLS。
- 创建首次配对码。
- 完成桌面和手机配对。
- 完成 JSON 导入。
- 验证双向同步。
- 配置每日备份。
- 演练恢复。

提交：`docs(deploy): 记录新同步服务部署`

### T6.8 归档旧桌面端

文件：

```text
README.md
docs/tech-stack.md
docs/decisions/README.md
docs/decisions/005-rust-sync-service.md
```

内容：

- 新桌面技术栈。
- 新同步架构。
- 旧 Tauri 客户端的归档状态。
- 旧 PocketBase 的停用日期和备份位置。
- 新客户端安装和数据迁移入口。

提交：`docs: 更新正式技术栈与迁移状态`

## 验证矩阵

每个阶段结束必须运行：

```powershell
# Rust
cargo fmt --manifest-path crates/rtodo-workspace/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/rtodo-workspace/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/rtodo-workspace/Cargo.toml
cargo test --manifest-path crates/transfer-core/Cargo.toml

# Avalonia
dotnet format apps/desktop-avalonia/r-todo.slnx --verify-no-changes
dotnet build apps/desktop-avalonia/r-todo.slnx -c Release
dotnet test apps/desktop-avalonia/r-todo.slnx -c Release

# 手机
flutter analyze
flutter test
flutter build apk --debug

# 旧客户端回归
pnpm --filter desktop build

# 仓库
git diff --check
```

阶段 0、4、6 另需 Windows 真机验收；阶段 5、6 另需 Android 真机验收。

## 完成定义

只有同时满足以下条件，项目才算完成：

1. 新 Avalonia 客户端通过功能对齐清单。
2. 桌面挂件通过 Win+D、前台切换、Explorer 重启和多 DPI 验收。
3. 新客户端从旧 JSON 导入的任务数量与关键字段一致。
4. 桌面和手机通过新同步服务双向同步。
5. 离线、冲突、重试、撤销设备和快照重同步通过。
6. MCP、LocalSend 和聊天式互传通过真机验收。
7. 全新机器可安装和运行，不依赖开发环境。
8. 服务端可部署、备份、恢复和重启。
9. 安装和更新失败可回退，用户数据不丢失。
10. 旧客户端和新服务端切换过程有可执行的回退路径。

## 回退点

### 阶段 0 失败

- 不继续 UI 重写。
- 保留 Tauri。
- 根据 Spike 结果重新选择 Avalonia、WinUI 3 或 gpui-ce。

### 阶段 2 失败

- 新客户端继续使用本地 SQLite。
- 暂不切换手机同步。
- 保留 PocketBase，重新评估同步协议。

### 阶段 3 或 4 失败

- 新客户端与旧客户端并存。
- 继续使用旧 PocketBase。
- 不卸载旧客户端，不删除旧数据。

### 阶段 5 失败

- 桌面端可先使用新客户端。
- 手机端暂时继续旧同步。
- 在新旧同步之间提供一次性 JSON 中转，不长期双写。

### 阶段 6 失败

- 不发布正式替代版本。
- 旧 PocketBase 和新服务端都保留。
- 修复后重新执行对应验收。

## 建议执行顺序

1. E0.1 至 E0.3。
2. T0.1 至 T0.7，全部通过后继续。
3. T1.1 至 T1.8。
4. T2.1 至 T2.8。
5. T3.1 至 T3.10。
6. T4.1 至 T4.5。
7. T5.1 至 T5.6。
8. T6.1 至 T6.8。

不要把 UI、同步协议和手机端同时改成半成品。每个阶段都要留下可运行状态和明确回退点。
