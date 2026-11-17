# RTools Tauri 2 桌面端视觉重建实施计划

日期：2026-11-17
状态：待审阅
目标版本：下一桌面发布版本

## 1. 目标

使用 Tauri 2、React、TypeScript 和 CSS 重建 Windows 桌面表现层，保留现有
Rust Engine、SQLite 数据、同步服务和移动端协议。

完成后必须同时满足：

1. 桌面挂件的视觉精度和交互质感达到或超过旧 Tauri 版本。
2. Avalonia 版现有全部桌面功能在新壳中可用。
3. 现有真实待办数据无需重新导入即可继续使用。
4. 桌面与手机继续通过 `rtools-syncd` 双向同步。
5. Windows 11 的 Mica、Acrylic、透明、高 DPI、置底挂件、吸附和收起正常。
6. 新安装、覆盖安装、卸载重装和回退旧版都不会破坏任务数据库。

## 2. 已固定的决策

### 2.1 保留现有数据核心

继续使用：

```text
Rust rtools-engine.exe
  ├─ %LOCALAPPDATA%\rtools\engine\rtools.db
  ├─ named-pipe JSON-RPC
  ├─ sync client
  ├─ MCP server
  ├─ LocalSend / chat transfer
  └─ import / export
```

Tauri 只替换：

```text
Avalonia C# UI
  ↓
Tauri 2 shell
  ↓
React / TypeScript UI
```

不迁移数据库，不重新导入 JSON，不改变服务端协议。

### 2.2 新建独立桌面壳

新建 `apps/desktop-tauri/`，保留 `apps/desktop/` 作为旧视觉和 Windows
互操作参考，直到新壳完成验收。

不直接在旧 Tauri 目录上就地重写。原因是旧目录仍包含旧的 SQLite、
PocketBase 同步、MCP、互传和业务状态逻辑，直接改造容易把这些旧实现重新
接回新客户端。

### 2.3 前端不直接操作数据和系统文件

React 只通过有类型的 Tauri command 访问 Engine 和 Windows 能力：

```text
React UI
  ↓ invoke()
Tauri Rust shell
  ├─ EngineRpcClient
  │    ↓ named pipe
  │  rtools-engine.exe
  ├─ Windows widget / snap / material
  ├─ tray / autostart / notification / updater
  └─ credential manager / file dialogs
```

不在新壳中恢复 `tauri-plugin-sql`、PocketBase、本地 TypeScript `tasks` 表或
第二套同步实现。

### 2.4 视觉实现原则

- 使用 React 自建原子组件，不采用会带入后台管理风格的整套组件库。
- 使用 CSS variables、Tailwind 和少量自定义 CSS 精确控制行高、间距、悬浮和
  圆环。
- 使用 Motion/Framer Motion 处理行出现、删除、拖拽和面板过渡。
- 拖动排序使用真实行测量结果，不假设固定行高。
- 字体使用 Segoe UI Variable，中文回退 Microsoft YaHei UI。
- 桌面挂件和完整窗口共享 token，但保持不同的信息密度。

## 3. 保留与替换清单

| 区域 | 处理方式 |
|---|---|
| Rust Engine、SQLite、outbox | 原样保留 |
| `rtools-syncd` 与移动端同步协议 | 原样保留 |
| Windows Credential Manager 中 `rtools:*` 项 | 新壳继续读取和写入同一组名称 |
| 任务 JSON / iCalendar / TickTick CSV | 通过 Engine 继续支持 |
| MCP 服务、LocalSend、聊天互传 | 通过 Engine 继续支持 |
| Avalonia `.axaml`、ViewModel、C# 控件模板 | 不再作为运行代码 |
| 旧 Tauri React 视觉组件 | 作为视觉参考，按新数据接口重建 |
| 旧 Tauri SQL、PocketBase、Zustand 数据逻辑 | 不迁入新壳 |
| 旧 Tauri 挂件、材质、吸附、托盘 Rust 代码 | 逐项审查后复用或改写 |

## 4. 目标目录

```text
apps/desktop-tauri/
├─ src/
│  ├─ app/
│  ├─ components/
│  ├─ features/
│  │  ├─ tasks/
│  │  ├─ calendar/
│  │  ├─ settings/
│  │  ├─ sync/
│  │  ├─ mcp/
│  │  ├─ transfer/
│  │  └─ updates/
│  ├─ lib/
│  │  ├─ engine-api.ts
│  │  ├─ engine-events.ts
│  │  ├─ system.ts
│  │  └─ types.ts
│  ├─ stores/
│  ├─ styles/
│  └─ main.tsx
├─ src-tauri/
│  ├─ src/
│  │  ├─ engine/
│  │  ├─ windows/
│  │  ├─ credentials.rs
│  │  ├─ tray.rs
│  │  └─ lib.rs
│  ├─ capabilities/
│  ├─ Cargo.toml
│  └─ tauri.conf.json
├─ tests/
└─ package.json
```

## 5. 实施阶段

## 阶段 0：冻结基线

### T0.1 记录 Avalonia 发布基线

- 运行中的安装目录记录到验收文档。
- 记录当前版本、构建时间、Engine 版本和真实数据库路径。
- 保留当前安装包，不删除可运行的 Avalonia 回退版本。
- 记录当前任务总数、未完成数、今日完成数、提醒任务数和重复任务数。

### T0.2 建立视觉对照集

在 100%、125%、150% 和 175% 缩放下截取：

- 旧 Tauri 挂件空列表。
- 旧 Tauri 挂件 1 条、5 条、20 条任务。
- 输入框聚焦和已输入状态。
- 行悬浮、圆环悬浮、删除按钮渐显。
- 删除二次确认。
- 拖拽中间态和落位后。
- Avalonia 对应状态的当前版本。

截图脚本必须调用：

```text
SetProcessDpiAwarenessContext(-4)
```

### T0.3 冻结五项视觉验收

第一轮通过标准：

1. 待办项的行高、上下内边距、文字大小和优先级条间距与旧版持平。
2. 行悬浮只改变行背景，不改变标题文字背景和文字布局。
3. 删除按钮悬浮后约 180ms 渐显，离开后渐隐；二次确认自动取消。
4. 圆环悬浮时高亮，按下和完成后有连续反馈。
5. 添加区与列表左右边界严格对齐，输入框、加号、优先级点全部垂直居中。

附加标准：

- 右上角设置图标和月历图标不变形。
- 拖动标题可排序，点击标题进入编辑，两种操作不冲突。
- 去掉挂件底部 Engine/同步状态文字。
- 小数缩放和无小数缩放时不出现一像素错位。

### T0.4 建立功能清单

从 Avalonia 客户端逐项抄录命令和交互，形成下表：

| 功能组 | 验收项 |
|---|---|
| 任务 | 列表、创建、编辑、完成、取消完成、删除、排序 |
| 快速新增 | 标题、优先级、Enter、输入法组合键 |
| 提醒与重复 | 提醒时间、提前量、RRULE、完成后的下一次实例 |
| 视图 | 今日列表、已完成分区、月视图 |
| 外观 | 挂件/浮空、材质、透明度、字体缩放、多 DPI |
| 系统 | 托盘、开机自启、单实例、窗口几何、通知、更新 |
| 同步 | 配对、立即同步、退出、设备列表、撤销、新设备码 |
| MCP | 开关、状态、token、复制、轮换、四个客户端模板 |
| 互传 | 启停、发现、手动 IP、发送、接收确认、拒绝、取消、进度 |
| 聊天 | 会话、消息、未读、信任、撤销信任、文件发送、清理 |
| 数据 | JSON、iCalendar、TickTick CSV 导入导出 |

提交：`docs(desktop): 冻结 Tauri 视觉重建基线`

## 阶段 1：Tauri 壳和 Engine 桥

### T1.1 新建 Tauri 2 应用

文件：

```text
apps/desktop-tauri/package.json
apps/desktop-tauri/vite.config.ts
apps/desktop-tauri/src-tauri/Cargo.toml
apps/desktop-tauri/src-tauri/tauri.conf.json
apps/desktop-tauri/src-tauri/capabilities/default.json
```

要求：

- React、TypeScript、Vite、Tailwind。
- 仅保留实际需要的 Tauri capability。
- 窗口无边框、透明、可拖动、无系统阴影。
- 设置 Windows 11 为主要目标。
- Windows 10 支持边界以 Tauri 2、WebView2 和所用插件官方范围为上限。

### T1.2 实现 Engine 生命周期

文件：

```text
src-tauri/src/engine/process.rs
src-tauri/src/engine/rpc.rs
src-tauri/src/engine/mod.rs
```

行为：

- 启动 `rtools-engine.exe serve --data-dir ... --pipe ...`。
- 使用随机管道名，仅允许当前用户。
- 采用四字节长度前缀和 UTF-8 JSON 帧。
- 支持请求超时、进程退出、有限重连和明确错误码。
- Engine 退出时，UI 提示可恢复错误，不自动无限重启。
- 记录 stdout/stderr 到 `%LOCALAPPDATA%\rtools\engine\logs`。

### T1.3 实现 Tauri 前后端桥

文件：

```text
src-tauri/src/engine/commands.rs
src/lib/engine-api.ts
src/lib/engine-types.ts
src/lib/engine-events.ts
```

接口：

```text
engine_health
engine_call(method, payload)
tasks_changed
reminder_due
transfer_event
chat_changed
```

`engine_call` 只允许 Engine RPC 方法白名单，防止前端获得任意远程调用面。

### T1.4 恢复同步凭据

文件：

```text
src-tauri/src/credentials.rs
src-tauri/src/engine/process.rs
```

读取 Windows Credential Manager：

```text
rtools:sync-server-url
rtools:sync-device-id
rtools:sync-device-name
rtools:sync-token
```

四项齐全时调用 `sync.restore_credentials`。缺少任何一项时按未配对处理。
配对成功、退出同步时继续写入或删除同一组凭据。

### T1.5 契约与集成测试

新增测试：

- Engine 不存在时返回可读错误。
- 启动后 `system.health` 成功。
- 请求 ID、超时、断线和非法帧处理正确。
- `tasks.changed` 事件送达 React。
- 临时 data-dir 使用全新数据库。
- 真实数据库只做只读列表检查。

验收命令：

```powershell
pnpm --filter desktop-tauri test
pnpm --filter desktop-tauri build
cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml
```

提交：`feat(desktop): 建立 Tauri Engine 桥`

## 阶段 2：挂件视觉样机

本阶段是继续投资的硬门槛。

### T2.1 建立视觉 token

文件：

```text
src/styles/tokens.css
src/styles/widget.css
tailwind.config.ts
```

Token 覆盖：

- 4/8px 间距体系。
- 行高、标题字号、辅助信息字号。
- 浅色和深色表面。
- 文本一级、二级、三级。
- 高、中、低优先级颜色。
- 完成色、危险色。
- 圆环边框、悬浮、填充。
- 半透明、Acrylic、Mica 的背景层。

### T2.2 重建挂件原子组件

文件：

```text
src/components/ui/Checkbox.tsx
src/components/ui/PriorityBar.tsx
src/components/ui/IconButton.tsx
src/components/tasks/QuickAdd.tsx
src/components/tasks/TaskRow.tsx
```

要求：

- 旧版组件作为视觉源，不复用旧 store、db、sync import。
- 删除按钮只在行悬浮时显示。
- 删除按钮二次确认 3 秒后自动复位。
- 行离开鼠标立即取消删除确认。
- 圆环按钮、删除按钮、确认按钮都阻止拖拽启动。
- 标题短按编辑，移动后拖拽排序。

### T2.3 重建拖拽排序

文件：

```text
src/features/tasks/useTaskReorder.ts
src/features/tasks/reorder-geometry.ts
```

要求：

- 使用测量后的行矩形计算目标位置。
- 支持相邻移动、跨多条移动、快速来回拖动。
- 落位后只向 Engine 提交一次 `tasks.reorder`。
- 写入失败时恢复原顺序并显示轻量错误。

### T2.4 接真实任务

文件：

```text
src/stores/task-store.ts
src/features/tasks/task-actions.ts
```

要求：

- 启动通过 `tasks.list` 获取数据。
- 创建、完成、取消完成、删除、编辑、排序均经 Engine。
- 保留今天完成的显示和历史完成项次日隐藏规则。
- 收到 `tasks.changed` 后重拉任务。
- UI 可做短暂乐观状态，失败必须回滚。

### T2.5 挂件窗口能力

复用并审查旧实现：

```text
apps/desktop/src-tauri/src/desktop_embed.rs
apps/desktop/src-tauri/src/snap.rs
apps/desktop/src-tauri/src/wallpaper_mica.rs
apps/desktop/src-tauri/src/window_effect.rs
```

职责：

- top-level owner = Progman。
- `WS_EX_TOOLWINDOW`，不显示任务栏和 Alt+Tab。
- 非前台回落 `HWND_BOTTOM`。
- 拦截最小化。
- 响应 Explorer 重启和前台切换。
- 不可见时回滚为普通窗口。
- 记录位置和尺寸，不恢复隐藏状态。
- 吸附边缘和可选收起。

### T2.6 视觉门槛验收

在同一批任务、相同缩放下并排比较旧 Tauri 和新壳：

1. 行密度。
2. 悬浮过渡。
3. 删除确认。
4. 拖拽手感。
5. 添加区和整体材质。

只有五项全部达到标准才进入阶段 3。若失败，停留在挂件样机修正，不继续
铺开设置、同步、互传等功能。

提交：`feat(desktop): 重建桌面挂件视觉与交互`

## 阶段 3：任务、编辑、提醒和月视图

### T3.1 快速添加与输入法

- Enter 提交；中文输入法组合期间不提交。
- 提交后清空标题并恢复默认低优先级。
- 焦点回到输入框。
- 输入框、图标和优先级点垂直居中。

### T3.2 任务编辑

重建：

- 标题。
- 优先级。
- 提醒时间。
- 默认提前量。
- RRULE 预设与说明。

编辑采用独立 sheet 或浮层，与挂件列表解耦。

### T3.3 完成与重复任务

- 完成和取消完成调用 Engine。
- 完成后提醒取消。
- 有 RRULE 时由 Engine 创建下一次实例。
- 已完成任务放在分区下方。
- 昨天及以前完成的任务不进入挂件可见列表。

### T3.4 月视图

- 独立窗口单实例。
- 当前月、上月、下月。
- 月网格和完成记录。
- 多 DPI 和窗口几何恢复。
- 从挂件按钮打开，关闭后再次打开正常。

提交：`feat(desktop): 对齐任务编辑与月视图`

## 阶段 4：设置、同步、MCP、互传和数据

### T4.1 设置壳

设置项：

- 挂件模式。
- 材质：半透明、Acrylic、Mica、Mica Alt、无。
- 透明度 20% 至 100%。
- 默认提醒提前量。
- 吸附后收起。
- 开机自启。
- 托盘显示、隐藏和退出。
- 版本与更新。

采用分段设置页或侧滑 sheet；低频项不挤占挂件首屏。

### T4.2 同步

界面：

- 服务端 URL。
- 一次性配对码。
- 设备名称。
- 当前状态、最后同步时间。
- 立即同步。
- 退出同步。
- 设备列表与撤销。
- 已配对设备生成新设备接入码。

同步全程使用 Engine RPC，不在前端访问 HTTP 同步接口。

### T4.3 MCP

- 默认关闭。
- 开启后显示运行状态、端口、token。
- 复制 token。
- 轮换 token。
- Claude Code、Claude Desktop、OpenCode、Codex 模板。
- 端口占用只显示局部错误，不影响任务使用。

### T4.4 互传和聊天窗口

- 独立窗口单实例。
- 服务启停。
- 自动发现、手动 IP、发送文件。
- 接收确认、拒绝、取消和进度。
- 信任设备自动接收。
- 会话列表、消息列表、未读、文本、文件、撤销信任和清理。
- 大文件只传路径，不把正文或 base64 穿过 RPC。

### T4.5 导入导出

- r-todo JSON 导入导出。
- iCalendar 导入导出。
- TickTick CSV 导入。
- 导入前展示新增、覆盖、跳过和错误。
- 提交由 Engine 单事务完成。

提交：`feat(desktop): 对齐桌面完整功能`

## 阶段 5：Windows 系统集成

### T5.1 托盘与单实例

- 托盘显示、隐藏、打开互传、退出。
- 再次启动激活已有窗口，不创建第二实例。
- 隐藏后退出、下次启动仍可见。

### T5.2 通知和提醒

- 接收 `reminder.due`。
- 显示 Windows Toast。
- 点击通知定位任务。
- 提醒任务被完成、删除或清空提醒后不继续弹旧提醒。

### T5.3 开机自启和文件对话框

- 开机自启用当前用户注册项。
- 文件导入、导出和发送使用 Tauri 对话框。
- 只向 Engine 传本地路径。

### T5.4 更新

- 检查 GitHub Release。
- 下载安装包。
- 显示进度。
- 安装前不破坏 Engine 数据目录。
- 失败保留当前版本。

### T5.5 新材料与回退

- Win11 优先应用 DWM 材质。
- 挂件置底状态下验证失焦材质不黑屏。
- 材质失败时降级到半透明。
- Windows 10 使用 Tauri/WebView2 支持协议仍有效的回退渲染。

提交：`feat(desktop): 完成 Windows 系统集成`

## 阶段 6：全量验证与发布

### T6.1 数据安全验证

使用真实数据库副本进行写入和破坏性测试：

- 启动前后任务总数一致。
- 创建、编辑、完成、取消完成、排序、软删除。
- 同步后任务字段一致。
- JSON、ICS、CSV 导入导出。
- 应用崩溃后 SQLite 和 outbox 可恢复。

真实数据库只做只读读取和启动验证。删除确认测试不得点击最终确认。

### T6.2 功能对齐验证

每项填写：

| 功能 | 自动测试 | Windows 真机步骤 | 结果 | 证据 |
|---|---|---|---|---|
| 挂件启动与置底 | | | | |
| Win+D | | | | |
| Explorer 重启恢复 | | | | |
| 前台切换回落 | | | | |
| 多 DPI | | | | |
| 吸附与收起 | | | | |
| 材质与透明度 | | | | |
| 任务 CRUD | | | | |
| 拖拽排序 | | | | |
| 提醒与 RRULE | | | | |
| 月视图 | | | | |
| 同步配对与设备 | | | | |
| MCP | | | | |
| 互传与聊天 | | | | |
| 导入导出 | | | | |
| 托盘与自启 | | | | |
| 更新与重装 | | | | |

### T6.3 性能与资源验证

记录：

- 冷启动到任务可见。
- 输入到任务出现的延迟。
- 20 条和 100 条任务滚动。
- 拖拽排序 GPU/CPU 占用。
- 空闲 CPU、UI 内存、Engine 内存。
- 挂件前台切换。
- Mica/Acrylic 开启与半透明模式对比。

### T6.4 打包和发布

- 构建 Tauri UI 和 Rust Engine。
- NSIS 安装包同时包含两个 exe。
- 安装器不删除 `%LOCALAPPDATA%\rtools`。
- 升级不重复导入数据。
- 保留 Avalonia 安装包作为回退。
- 发布前运行全部检查：

```powershell
pnpm --filter desktop-tauri lint
pnpm --filter desktop-tauri test
pnpm --filter desktop-tauri build
cargo fmt --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml
cargo test --manifest-path crates/rtools-workspace/Cargo.toml
git diff --check
```

### T6.5 归档 Avalonia 运行壳

新版本验收通过后再归档：

- README 主技术栈改成 Tauri 2 + Rust Engine。
- `apps/desktop-avalonia` 标记为历史实现。
- 文档保留迁移原因、视觉对照和回退方式。
- 不删除 Rust Engine、服务端、移动端和旧数据。

提交：`docs(desktop): 记录 Tauri 重建验收与发布`

## 6. 回退点

### 挂件视觉门槛失败

- 不进入完整功能迁移。
- 保留 Avalonia 可用版本。
- 在阶段 2 内继续修正 CSS、Motion、拖拽和窗口材质。

### Engine 桥失败

- 不继续重建功能页。
- 修复进程生命周期和协议；必要时在 Tauri Rust 层继续调用现有 C# 客户端
  作为临时验证，但不作为最终运行架构。

### 同步或互传失败

- 继续使用 Avalonia 桌面端。
- 服务端、手机和数据库不变。
- 新 Tauri 壳仅保留为测试构建。

### 发布失败

- 不卸载 Avalonia 版。
- 不删除新安装包和日志。
- 用同一真实数据库重新启动 Avalonia 版验证。

## 7. 明确不做

- 不重写 Rust Engine。
- 不替换 `rtools-syncd`。
- 不修改移动端同步协议。
- 不重新导入现有真实待办。
- 不恢复 PocketBase。
- 不恢复前端直接 SQLite。
- 不在第一阶段重做全部设置和互传页面。
- 不为了迁移而更换现有任务数据目录。
- 不继续在 Avalonia 中追逐旧 Tauri 的视觉精度。

## 8. 第一执行顺序

1. 冻结 Avalonia 与旧 Tauri 的视觉和功能基线。
2. 新建 `apps/desktop-tauri`。
3. 打通 Engine named-pipe 桥与 Credential Manager。
4. 只实现挂件、快速添加、任务行、删除确认和拖拽。
5. 执行五项视觉门槛验收。
6. 通过后依次实现编辑、月视图、设置、同步、MCP、互传、导入导出和系统能力。
7. 用真实数据副本和 Windows 真机完成全量对齐。
8. 发布后归档 Avalonia 运行壳，保留回退安装包。

## 9. 完成定义

只有以下条件全部成立才视为完成：

1. 挂件五项视觉验收通过，并有多 DPI 截图证据。
2. 新壳所有任务操作都通过 Rust Engine。
3. 真实待办无需导入即可显示和管理。
4. 桌面与手机双向同步通过。
5. MCP、互传、聊天、提醒、月视图、导入导出、托盘、自启和更新通过。
6. 安装包在全新环境可运行。
7. 页面样式、system integration、拖动排序、材质回退、更新等均检查通过。
8. 应用样式检查确认没有遗留 Avalonia 运行依赖。
9. Avalonia 安装包仍可作为数据无损回退。
