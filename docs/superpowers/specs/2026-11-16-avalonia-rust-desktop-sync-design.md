# r-todo Avalonia 桌面迁移与 Rust 同步服务设计

日期：2026-11-16
状态：待审阅

## 1. 背景

r-todo 当前桌面端使用 Tauri 2、React、TypeScript 和 Rust，已经发布到 `v0.0.9`。现有功能包括任务 CRUD、提醒、重复任务、月视图、PocketBase 同步、MCP、LocalSend 兼容互传、聊天式互传、托盘、开机启动、更新、桌面挂件、窗口吸附和收起。

当前主要问题不是 Rust 核心不可用，而是：

1. Tauri/WebView2 在桌面挂件、失焦材质和窗口时序上存在平台限制。
2. 桌面端与手机端各自复制了一套 PocketBase 全量同步、客户端时间戳冲突处理和离线队列逻辑。
3. PocketBase 的用户名密码和短期 token 与单用户自托管场景不完全匹配。
4. 任务业务同时分布在 TypeScript 和 Rust 中，Rust 尚未成为完整的数据核心。

本次迁移的目标是：

- Windows UI 改用 Avalonia 12 和 C#。
- Rust 保留并扩展为独立桌面 Engine。
- 新写一个单用户 Rust 同步服务，替换 PocketBase。
- 桌面端完整对齐现有功能，不要求视觉和控件结构一致。
- 保留用户现有待办数据，通过现有 r-todo JSON 一次性导入。
- 不兼容旧桌面设置、登录态、MCP 配置、互传身份、聊天记录和旧服务端协议。
- Windows 支持范围跟随 Avalonia 12 官方支持矩阵；当前验收环境为 Windows 11。

## 2. 目标与非目标

### 2.1 目标

- 提供可独立发布和运行的 Avalonia Windows 客户端。
- 保留任务、完成状态、提醒、重复规则、手动排序和软删除。
- 桌面端与手机端继续双向同步同一份待办数据。
- 日常离线可用，恢复网络后自动同步。
- 服务端支持单用户、自托管、设备配对和独立撤销。
- Rust 统一拥有任务数据、同步协议、提醒、MCP、互传和聊天历史。
- 桌面挂件继续支持 owner=Progman、始终置底、Win+D、前台回落和 Explorer 重启恢复。
- 保留现有导入导出格式：r-todo JSON、iCalendar 和 TickTick CSV。
- 现有待办可以从旧桌面端导出的 r-todo JSON 无损导入新客户端。

### 2.2 非目标

- 不保证像素级复刻现有 React 界面。
- 不支持从 `v0.0.9` 原地升级。
- 不迁移旧桌面设置、窗口位置、同步登录态、MCP token、互传身份、聊天记录或手机本地数据。
- 不兼容旧 PocketBase 服务端协议。
- 不支持多用户、注册、邀请、共享清单或团队协作。
- 不采用 CRDT；冲突继续使用服务端统一裁决的 LWW。
- 不提供 macOS 或 Linux 桌面客户端。
- 不提供 Web 管理后台。
- 不把大文件内容通过 UI 与 Engine 之间的 JSON 或 base64 传输。

## 3. 总体架构

```text
┌──────────────────────── Windows ────────────────────────┐
│                                                         │
│  rtodo-ui.exe                    Avalonia 12 / C#       │
│  ├─ 页面、主题、动画、MVVM                               │
│  ├─ 主窗口、月视图、互传窗口                             │
│  ├─ 托盘、通知、单实例、开机启动、更新                   │
│  ├─ 无边框窗口、DPI、吸附、收起                          │
│  └─ Win32：owner、HWND_BOTTOM、WndProc、Explorer 恢复    │
│                    │                                    │
│                    │ Named Pipe，版本化 JSON-RPC         │
│                    ▼                                    │
│  rtodo-engine.exe                 Rust                 │
│  ├─ SQLite、任务事务、outbox                             │
│  ├─ 同步客户端、设备令牌、增量游标                       │
│  ├─ 提醒调度、MCP、LocalSend、会话式互传                 │
│  ├─ JSON / ICS / TickTick CSV 导入导出                    │
│  └─ 日志、健康状态、崩溃恢复                             │
│                                                         │
└────────────────────────┬────────────────────────────────┘
                         │ HTTPS + WebSocket
                         ▼
┌──────────────────── Linux 同步服务 ─────────────────────┐
│  rtodo-syncd                       Rust                 │
│  ├─ axum + tokio                                       │
│  ├─ SQLite                                             │
│  ├─ 一次性配对码、设备令牌、设备撤销                    │
│  ├─ 操作幂等、revision、全局游标、LWW 裁决              │
│  └─ WebSocket 变更通知                                  │
└────────────────────────┬────────────────────────────────┘
                         │
                    SQLite 数据文件
```

### 3.1 进程边界

- `rtodo-ui` 负责所有窗口和 Windows Shell 集成。
- `rtodo-engine` 是任务、同步、提醒、MCP 和互传数据的唯一读写入口。
- `rtodo-syncd` 只负责单用户待办同步，不承担通用账号平台职责。
- UI 不直接访问 SQLite、同步服务或互传文件内容。
- Engine 不创建或控制 UI 窗口，只通过事件请求 UI 展示通知或打开窗口。
- 服务端不保存文件互传内容；LocalSend 和聊天互传继续在设备之间直接通信。

### 3.2 技术选型

| 区域 | 选型 |
|---|---|
| Windows UI | Avalonia 12、C#、.NET 10 LTS |
| UI 状态 | MVVM，使用强类型 ViewModel |
| Windows 互操作 | P/Invoke 或 CsWin32 |
| UI 与 Engine | 当前用户专属 Named Pipe |
| 桌面 Engine | Rust、tokio、rusqlite、serde |
| 同步服务端 | Rust、axum、tokio、SQLite |
| 同步传输 | HTTPS REST + WebSocket 通知 |
| 手机端 | 保留 Flutter 和 Drift，替换 PocketBase 同步客户端 |
| Windows 安装 | 单个 NSIS 安装包，包含 UI 与 Engine |
| Windows 更新 | 保留签名清单和被动安装模式，由独立更新组件完成 |
| 服务端部署 | 单个 `rtodo-syncd` Linux 二进制，SQLite 文件持久化 |
| TLS | Caddy 或现有 OpenResty 终止 TLS |

## 4. 工作区与 crate 划分

```text
crates/
├── rtodo-domain/         任务模型、校验、排序、RRULE 规则
├── rtodo-sync-proto/     同步 API、事件、错误码、游标类型
├── rtodo-engine/         桌面 Engine 实现
└── rtodo-syncd/          单用户同步服务

apps/
├── desktop-avalonia/     Avalonia UI、ViewModel、Windows 互操作
├── desktop/              旧 Tauri 客户端，迁移完成后归档
└── mobile/               Flutter 手机端
```

### 4.1 共享 crate 的职责

`rtodo-domain`：

- 定义任务、优先级、时间戳和字段校验。
- 定义排序、完成、提醒和重复规则语义。
- 被 Engine、服务端和测试共同使用。

`rtodo-sync-proto`：

- 定义配对、设备、操作、变更、游标和 WebSocket 事件。
- 定义稳定错误码。
- 冻结 JSON 契约测试。
- 被 Engine、Syncd 和 Flutter 协议测试样例共同使用。

UI 与 Engine 的 RPC 契约单独放在 Avalonia 项目或 `rtodo-engine` 的公开模块中，不把 UI 类型放进同步协议。

## 5. 任务数据模型

```text
Task
├── id: string
├── title: string
├── priority: high | medium | low
├── remind_at: timestamp?
├── remind_offset_min: integer
├── repeat_rule: string?
├── sort_order: number
├── completed_at: timestamp?
├── deleted_at: timestamp?
├── created_at: timestamp
├── updated_at: timestamp
└── revision: integer
```

规则：

- `id` 在首次创建时生成，导入时保留旧 ID。
- 时间统一使用 UTC ISO 8601 存储，UI 按本地时区显示。
- `revision` 由服务端分配；从未同步的本地任务初始值为 0。
- `deleted_at` 非空即视为软删除。
- 服务端不把已删除任务返回为活跃任务，但通过变更日志传播 tombstone。
- 标题长度为 1 至 500 个字符，去除首尾空白后不得为空。
- `remind_offset_min` 大于等于 0。
- 手动排序沿用数字越小越靠前的语义。
- RRULE 使用 RFC 5545 字符串，不转换成私有规则格式。

## 6. 桌面 Engine

### 6.1 数据目录

新客户端使用独立目录，避免误读旧 Tauri 配置：

```text
%LOCALAPPDATA%\r-todo\
├── engine\
│   ├── r-todo.db
│   ├── device.json
│   └── logs\
├── ui\
│   └── logs\
└── updates\
```

设备令牌使用 Windows 凭据管理器保存，不写入普通 JSON 或日志。

### 6.2 SQLite

- 启用 WAL、外键、busy timeout 和完整性检查。
- 所有任务写操作在事务中同时写任务和 outbox。
- 使用显式 schema migration，不依赖旧桌面 migration。
- 数据库备份使用 SQLite backup API，不使用直接复制活动 WAL 文件。
- 启动时检查 schema 版本；未知新版本拒绝写入并提示更新客户端。

核心表：

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
```

### 6.3 提醒

- Engine 根据 `remind_at - remind_offset_min` 计算触发时间。
- 提醒变化时原子更新任务与提醒表。
- 触发后向 UI 发送 `reminder.due`。
- 完成任务或删除任务时取消对应提醒。
- 重复任务完成后由 Engine 创建下一次实例，并沿用现有 RRULE 语义。
- 应用重启后从 SQLite 重建未来提醒，过期提醒不补弹。

### 6.4 MCP

- 保留 `127.0.0.1:18765/mcp` 和 Bearer token。
- MCP 写操作调用 Engine 的同一任务服务，不直接执行独立 SQL。
- 写操作生成 outbox，并向 UI 发送 `tasks.changed`。
- MCP token 使用新配置格式，重新生成，不迁移旧 token。
- 端口占用或启动失败只标记 MCP 不可用，不影响其他功能。

### 6.5 LocalSend 与聊天式互传

- 继续复用 `crates/transfer-core`。
- 文件发送和接收由 Engine 完成。
- UI 只接收设备、进度、请求、错误和文件路径。
- 会话、消息、未读、信任设备和转发记录存入 Engine SQLite。
- 不迁移旧聊天记录和旧信任设备。
- 文件默认目录、重名处理、取消、拒绝、前台服务和手机端协议保持现有语义。

### 6.6 导入导出

- r-todo JSON：导入和导出任务全部字段。
- iCalendar：导入和导出 VTODO。
- TickTick CSV：只导入。
- 导入先生成预览，再在单个事务中提交。
- 预览显示新增、更新、跳过和错误行。
- 导入失败时整批回滚。
- 迁移入口默认选择 r-todo JSON。

## 7. UI 与 Engine 协议

### 7.1 传输

- Engine 由 UI 启动。
- UI 生成随机管道后缀，并作为参数传给 Engine。
- 管道名包含当前 Windows 用户 SID，ACL 只允许当前用户。
- 消息使用长度前缀加 UTF-8 JSON，避免换行和转义问题。
- 每条消息包含协议版本、消息类型、请求 ID 和载荷。
- 请求与响应通过 `request_id` 关联。
- 领域事件不包含请求 ID。

### 7.2 命令

```text
system.health
system.shutdown

tasks.list
tasks.get
tasks.create
tasks.update
tasks.complete
tasks.uncomplete
tasks.delete
tasks.reorder
tasks.import_preview
tasks.import_commit
tasks.export

reminders.reschedule

sync.status
sync.pair
sync.sync_now
sync.logout
sync.devices
sync.revoke_device

transfer.get_state
transfer.start
transfer.stop
transfer.discover
transfer.send
transfer.respond
transfer.reject
transfer.cancel

chat.list_conversations
chat.list_messages
chat.send_text
chat.send_files
chat.delete_file
chat.clear_history
chat.revoke_trust

mcp.status
mcp.set_enabled
mcp.rotate_token
mcp.templates

settings.get
settings.set
```

### 7.3 事件

```text
tasks.changed
reminder.due
sync.changed
transfer.event
chat.changed
mcp.changed
engine.warning
engine.fatal
```

### 7.4 大文件约束

- 文件内容不经过 Named Pipe。
- UI 传文件路径给 Engine，Engine 读取并发送。
- Engine 接收文件后传回路径、大小、MIME、预览路径和进度。
- 缩略图只传受限大小的临时文件路径。

## 8. Avalonia UI

### 8.1 页面

完整对齐现有功能，允许重新组织页面和控件：

- 主任务列表。
- 快速新增。
- 任务编辑。
- 完成、取消完成、软删除。
- 手动排序。
- 月视图。
- 提醒设置。
- 重复规则。
- 设置。
- 同步与设备管理。
- MCP。
- 局域网互传。
- 聊天式互传。
- 导入导出。
- 更新。

### 8.2 窗口与系统集成

- 无边框透明主窗口。
- 高 DPI 和跨显示器缩放。
- 窗口位置和尺寸记忆，但不恢复“隐藏”状态。
- 主窗口吸附和收起。
- 月视图独立窗口。
- 托盘显示、隐藏、打开互传和退出。
- 单实例激活已有窗口。
- 开机自启动。
- Windows Toast 和点击回到任务。
- 自更新检查、下载、校验和安装。

### 8.3 桌面挂件

挂件逻辑保持在 UI 进程：

- 窗口保持 top-level。
- `GWLP_HWNDPARENT` 设置为 Progman，形成 shell ownership。
- 清除 `WS_EX_APPWINDOW`，设置 `WS_EX_TOOLWINDOW`。
- 非前台时使用 `HWND_BOTTOM`。
- `WndProc` 拦截 `SC_MINIMIZE`。
- 监听 `EVENT_SYSTEM_FOREGROUND`，前台切换到其他应用后回落。
- 监听 `TaskbarCreated`，Explorer 重启后重新设置 owner 和置底。
- 挂件不可见时自动回滚为普通窗口。
- Avalonia 直接应用 Mica、Acrylic 或 Mica Alt；不沿用 WebView2 失焦黑屏的模拟方案。

## 9. 同步服务

### 9.1 服务范围

- 单个待办空间。
- 不提供注册和多用户隔离。
- 每个已配对设备拥有独立长期令牌。
- 服务端保存设备名称、令牌哈希、创建时间、最后使用时间和撤销时间。
- 日常同步使用高熵 Bearer token，不保存明文令牌。

### 9.2 首次配对

首次启动时，服务端在本地 stdout 输出一次接入码，也可通过命令行重新生成：

```text
rtodo-syncd pair
```

接入码规则：

- 8 位 Base32 字符，分组显示。
- 默认 10 分钟有效。
- 只能使用一次。
- 服务端只保存接入码哈希。
- 配对失败进行速率限制。
- 配对成功后立即作废。

客户端流程：

1. 用户输入服务器 URL 和接入码。
2. 客户端生成设备 ID、设备名称和随机设备密钥材料。
3. 客户端通过 HTTPS 调用配对接口。
4. 服务端返回长期设备令牌。
5. 客户端把令牌存入 Windows 凭据管理器或手机安全存储。
6. 后续请求使用 `Authorization: Bearer <token>`。

配对接口在开发环境允许 `http://127.0.0.1`，生产环境必须使用 HTTPS。

### 9.3 新增设备和撤销

- 已授权设备可以请求服务端生成新的短期配对码。
- 手机端使用二维码或手动输入配对码。
- 设备管理页面展示设备名称、最后使用时间和状态。
- 撤销设备后，其令牌立即失效。
- 撤销一台设备不影响其他设备。

### 9.4 API

```text
GET    /api/v1/health
POST   /api/v1/pair
POST   /api/v1/pairing-codes
GET    /api/v1/devices
DELETE /api/v1/devices/:id

GET    /api/v1/changes?after=<cursor>
GET    /api/v1/snapshot
POST   /api/v1/operations

GET    /api/v1/events
```

`GET /api/v1/events` 升级为 WebSocket，只发送以下轻量通知：

```json
{
  "type": "changes.available",
  "cursor": 12345
}
```

客户端收到通知后必须通过 REST 拉取变更；WebSocket 断线不会导致数据丢失。

### 9.5 操作与幂等

客户端批量提交操作：

```json
{
  "device_id": "desktop-...",
  "operations": [
    {
      "operation_id": "019...",
      "task_id": "task-...",
      "base_revision": 4,
      "client_updated_at": "2026-11-16T10:00:00Z",
      "patch": {
        "title": "新标题",
        "completed_at": null
      }
    }
  ]
}
```

服务端规则：

- `operation_id` 全局唯一，重复提交返回第一次处理的结果。
- 服务端为每次实际变更分配全局递增 `server_cursor`。
- 服务端为任务分配单调递增 `revision`。
- 同一操作在一个数据库事务内完成幂等记录、任务更新和变更日志写入。
- `base_revision` 等于当前 revision 时直接应用。
- `base_revision` 落后时由服务端执行 LWW 裁决，返回最终任务和 revision。
- 服务端接收顺序决定并发写入胜负；客户端时间只用于诊断，不参与裁决。
- 同一操作不能部分成功。

### 9.6 拉取与游标

- 客户端保存最后成功应用的 `server_cursor`。
- 拉取和本地应用在同一事务中提交游标。
- 变更按游标升序返回。
- 服务端返回 tombstone，让离线设备删除对应任务。
- 变更日志至少保留 90 天。
- tombstone 在删除 30 天后从活跃数据清理，但对应变更记录在保留期内仍可传播。
- 客户端游标早于服务端保留范围时返回 `resync_required`。
- 重新同步前先提交本地 outbox，再拉取快照并按 ID 与 revision 合并。
- 手机端离线超过保留期时同样执行快照重同步。

### 9.7 冲突语义

- 单字段更新和整任务更新都使用服务端接收顺序 LWW。
- 删除是 tombstone，不硬删。
- 对已删除任务的普通更新不会复活任务，除非操作显式取消删除。
- 重复提交、超时重试和网络切换不会产生重复任务或重复 revision。

## 10. 手机端改造

- 保留 Flutter、Drift 和现有页面结构。
- 删除 `pocketbase` 依赖和 PocketBase 专用认证代码。
- 新同步客户端实现同一套操作、游标和事件协议。
- 设备令牌存入 Android Keystore 支持的安全存储。
- 本地 SQLite 增加 revision 和 outbox 所需字段。
- 同步页面改为“服务器 URL + 配对码”。
- 提供立即同步、待同步数量、最后同步时间、设备名称和撤销入口。
- 任务页面、提醒、重复规则、桌面小组件和互传功能保持现有行为。
- 手机端不迁移旧 PocketBase 登录态。
- 手机端首次配对后从服务端拉取由桌面 JSON 导入的待办数据。

## 11. 数据迁移

### 11.1 用户路径

1. 在旧桌面端导出 r-todo JSON。
2. 安装新客户端。
3. 新客户端选择“从 r-todo JSON 导入”。
4. 预览任务数量、重复 ID、无效字段和软删除记录。
5. 确认后在一个事务中写入新 SQLite。
6. 首次同步把任务提交到新服务端。
7. 手机端重新配对并从服务端拉取任务。

### 11.2 JSON 契约

```json
{
  "format": "r-todo/todo",
  "version": 1,
  "exported_at": "2026-11-16T10:00:00Z",
  "todos": []
}
```

迁移规则：

- 只接受 `format = "r-todo/todo"` 且 `version = 1`。
- 校验每个任务的必填字段和值域。
- 保留任务 ID、完成时间、提醒、重复规则和排序。
- `deleted = true` 的记录作为 tombstone 导入，不在列表中显示。
- 重复 ID 在导入预览中报错，不静默覆盖。
- 空库允许直接导入；非空库使用 ID 和 `updated_at` 生成合并预览。
- 导入完成后记录一次性迁移标记，避免每次启动重复导入。

旧 PocketBase 数据不作为运行时数据源。上线前从旧桌面端导出一份 JSON 作为权威迁移输入，并保留旧服务端备份。

## 12. 失败处理

| 场景 | 行为 |
|---|---|
| Engine 启动失败 | UI 显示错误，可重试、查看日志或退出 |
| Engine 异常退出 | UI 保留窗口，有限次自动重启并恢复订阅 |
| Named Pipe 断开 | 未完成请求返回错误；重连后重拉状态，不重放旧 UI 请求 |
| SQLite 忙 | 有界重试，超时后返回可重试错误 |
| SQLite 损坏 | 停止写入，提示从备份恢复 |
| 本地写失败 | UI 回滚乐观更新 |
| 同步离线 | 本地继续可用，outbox 保留 |
| 服务端不可用 | 指数退避重连，不阻塞本地功能 |
| 设备令牌失效 | 停止同步，保留本地数据并提示重新配对 |
| 配对码错误或过期 | 提示重新生成，不泄漏内部细节 |
| 并发修改 | 服务端 LWW 裁决并返回最终 revision |
| 重复提交 | `operation_id` 保证幂等 |
| WebSocket 重连 | 按最后游标补拉 |
| 游标过期 | 返回 `resync_required`，执行快照重同步 |
| 互传端口被占用 | 提示冲突，允许换端口或停用 |
| 文件写入失败 | 保留临时状态，报告错误并允许清理 |
| MCP 端口被占用 | 只标记 MCP 启动失败 |
| MCP 畸形请求 | 返回协议错误，不 panic |
| UI 与 Engine 版本不兼容 | 拒绝启动组合并提示安装完整发行包 |
| 日志损坏或写满 | 轮转和限长，不影响核心功能 |
| 更新下载失败 | 保留当前版本，可重试 |
| 更新安装失败 | 回滚当前版本，保留用户数据 |
| JSON 校验失败 | 显示行号和字段错误，不写数据库 |
| JSON 导入中途失败 | 整批事务回滚 |
| Explorer 重启 | 重新设置 Progman owner 和置底 |
| 挂件化后不可见 | 自动回滚为普通窗口 |

日志要求：

- UI、Engine 和 Syncd 分开记录。
- 按大小轮转。
- 不记录设备令牌、MCP token、配对码、文件内容或登录凭据。
- 同步错误记录游标、操作 ID 和稳定错误码。

## 13. 安全

- 生产配对和同步必须使用 HTTPS。
- 设备令牌至少 256 位随机值。
- 服务端只保存令牌哈希。
- 配对码短期有效、单次使用并限速。
- Named Pipe ACL 只允许当前 Windows 用户。
- MCP 继续只监听回环地址，并使用独立 Bearer token。
- 日志自动脱敏。
- 更新包和更新清单必须签名验证。
- 服务端默认只开放 HTTPS 入口，数据库端口不暴露公网。
- SQLite 文件只允许服务账号读写。

## 14. 测试

### 14.1 自动化测试

- `rtodo-domain`：字段校验、优先级、排序、提醒和 RRULE 边界。
- `rtodo-sync-proto`：请求、响应和事件 JSON 契约冻结测试。
- `rtodo-engine`：SQLite migration、CRUD、事务回滚、outbox、幂等、JSON 导入和提醒。
- `rtodo-syncd`：配对、令牌撤销、游标、并发冲突、tombstone、重复请求和快照重同步。
- Named Pipe：请求关联、事件顺序、断线重连和协议版本不兼容。
- Flutter 同步客户端：与新服务端的端到端同步测试。
- Avalonia ViewModel：加载、乐观更新、错误回滚和同步状态。

### 14.2 真机验收

1. 从旧桌面端导出 JSON，在新客户端预览并导入，核对任务总数、完成状态、提醒、重复规则和排序。
2. 桌面端首次配对服务端，确认接入码一次失效。
3. 手机端配对同一服务端，确认两台设备拥有独立令牌。
4. 桌面新增、编辑、完成、删除和排序，手机端数秒内看到变化。
5. 手机关闭网络修改任务，恢复网络后自动上传。
6. 两端并发修改同一任务，确认服务端裁决后最终一致。
7. 重复提交和断网重试不产生重复任务。
8. 撤销手机令牌后，手机停止同步且不能继续访问数据，桌面端不受影响。
9. 服务端重启后，客户端自动重连并按游标补齐。
10. 删除任务后 30 天内，其他离线设备仍能同步 tombstone。
11. 离线超过保留期后执行快照重同步，不丢本地 outbox。
12. MCP 创建、修改、完成和删除任务后，桌面 UI 与同步链路立即更新。
13. 桌面和手机双向互传文件、文本、取消和拒绝流程正常。
14. 挂件在 Win+D、切换应用和 Explorer 重启后保持正确层级。
15. 主窗口和挂件在 100%、125%、150% 和 200% DPI 下布局正常。
16. 模拟 Engine 崩溃、数据库锁和数据库损坏，不出现误报成功。
17. 更新安装失败时，旧版本和数据仍可恢复。
18. 全新机器安装后不依赖开发环境即可运行。
19. 服务端从全新数据库部署、备份、恢复并重新启动。
20. 现有 JSON、ICS 和 TickTick CSV 导入导出保持可用。

### 14.3 性能门槛

- 冷启动到可交互目标 2 秒以内，首次安装另算。
- 1,000 条任务列表滚动无可感知卡顿。
- 本地新增、完成和删除在 100 毫秒内完成落库。
- 同步 100 条变更，局域网 3 秒内完成，公网正常网络 10 秒内完成。
- WebSocket 断线后 30 秒内重新连接并补齐。
- 挂件前台切换不闪烁、不遮挡当前应用、不抢占输入焦点。
- 挂件与 Engine 空闲 CPU 合计目标低于 1%。
- UI、Engine 和 Syncd 分别设置稳定内存上限并持续监控。

## 15. 实施拆分

项目规模较大，拆成以下可独立验收的阶段。每个阶段都有可运行产物和明确退出条件。

### 阶段 0：Avalonia 高风险验证

- 创建最小 Avalonia 12 应用。
- 验证透明无边框窗口、高 DPI、Mica 和 Acrylic。
- 验证 Progman owner、置底、Win+D、`WndProc` 和 Explorer 恢复。
- 验证托盘、Toast、自启动、单实例和更新安装。
- 验证 Engine 启动、Named Pipe 和异常恢复。

退出条件：挂件和发布链路达到可用标准。如果挂件无法稳定工作，暂停后续迁移并重新评估 Avalonia。

### 阶段 1：共享核心

- 建立 `rtodo-domain`、`rtodo-sync-proto`。
- 建立 Engine SQLite schema、任务服务和 outbox。
- 完成 JSON、ICS 和 TickTick CSV 导入导出。
- 完成 Named Pipe 协议和契约测试。

退出条件：无 UI 情况下可以通过测试完整管理任务和导入 JSON。

### 阶段 2：同步服务

- 建立 `rtodo-syncd`。
- 完成配对码、设备令牌、撤销、操作幂等、游标、LWW、tombstone 和 WebSocket。
- 完成备份、恢复、日志和部署文档。
- 用 Rust 集成测试和命令行客户端验证。

退出条件：两台模拟设备可以离线、重连、冲突和增量同步。

### 阶段 3：Avalonia 主客户端

- 完成主列表、快速新增、任务编辑、完成、删除、排序和设置。
- 完成提醒、月视图、托盘、开机启动、窗口吸附和收起。
- 完成同步状态、配对和设备管理。
- 完成导入导出。

退出条件：单机任务功能完整，桌面端可连接新同步服务。

### 阶段 4：桌面扩展能力

- 完成挂件、材质、Explorer 恢复和 DPI 回归。
- 完成 MCP。
- 完成 LocalSend 和聊天式互传。
- 完成更新和安装。

退出条件：与旧桌面端功能清单逐项对齐。

### 阶段 5：手机同步迁移

- Flutter 替换 PocketBase 客户端。
- 增加安全令牌存储、revision、outbox、游标和设备管理。
- 保留任务、小组件、提醒和互传功能。
- 完成桌面与手机双向同步验收。

退出条件：手机和桌面端稳定双向同步。

### 阶段 6：发布与归档

- 生成新 Windows 安装包。
- 完成服务端部署和备份恢复演练。
- 完成 JSON 数据迁移。
- 完成性能和安全检查。
- 将旧 Tauri 桌面端标记为归档实现。

退出条件：新客户端作为正式桌面端发布，旧客户端不再作为主开发线。

## 16. 发布与回退

- 新客户端使用新应用数据目录和新安装标识。
- 首版允许与旧客户端并存，便于迁移核对。
- 用户完成 JSON 导入并确认任务数量后，再手动卸载旧客户端。
- 新服务端使用独立端口和独立域名入口，旧 PocketBase 在验证期保持只读备份。
- 新同步验证通过前，不删除旧 `pb_data`。
- 回退方式为继续运行旧 Tauri 客户端和旧 PocketBase；新客户端本地数据不会写回旧格式。
- 验证完成后停止旧服务并归档旧部署配置。

## 17. 风险

| 风险 | 缓解 |
|---|---|
| Avalonia 挂件行为与 Win32 时序冲突 | 阶段 0 先做真机 Spike |
| C# 与 Rust 双栈增加维护成本 | 严格进程边界，UI 不复制业务规则 |
| 自建同步服务引入运维责任 | 单二进制、SQLite、备份和恢复演练 |
| 同步协议一次改动过大 | 冻结契约测试，先命令行双设备验证 |
| 手机端改造影响现有任务功能 | 保留 UI 与 Drift，只替换同步层 |
| JSON 导入数据不完整 | 导入预览、字段校验和整批回滚 |
| 旧服务端提前下线 | 验证期保留 PocketBase 只读备份和回退路径 |
| 发布链路重写导致安装失败 | 阶段 0 提前验证安装和更新 |

## 18. 验收结论

本设计完成的最低交付标准是：

- 新 Avalonia 客户端完整对齐旧桌面端功能。
- Rust Engine 成为唯一桌面数据核心。
- 桌面与手机通过新 Rust 同步服务双向同步。
- 用户待办通过现有 r-todo JSON 保留。
- 旧 PocketBase 在验证完成后退出主链路。
- 旧 Tauri 客户端保留到新客户端通过全部真机验收。
