# RTools Tauri 桌面重建交付说明

日期：2026-11-17
状态：已完成，可用于日常自用
版本：0.3.0

## 1. 交付结果

桌面端已从 Avalonia 表现层切换到 Tauri 2 + React + TypeScript。Rust
`rtools-engine`、SQLite 数据、同步服务、MCP、互传和移动端协议保持不变。

运行入口：

```text
E:\artifacts\release-final-tauri\rtools-desktop-tauri.exe
```

安装包：

```text
E:\artifacts\release-final-tauri\RTools_0.3.0_x64-setup.exe
```

当前自用构建安装包 SHA-256：

```text
0DF7090286CBC45407D710A6044BF16AF27689227A96ED1CBF961288A98D4CDC
```

## 2. 架构

```text
React UI
  ↓ Tauri invoke
Tauri Rust shell
  ↓ 长度前缀 JSON-RPC / Windows named pipe
rtools-engine.exe
  ↓
%LOCALAPPDATA%\rtools\engine\rtools.db
```

UI 不直接访问 SQLite。Engine 负责：

- 任务、排序、完成、软删除和 RRULE。
- 本地 outbox、增量同步和设备管理。
- 提醒调度。
- MCP 服务。
- LocalSend 兼容互传和聊天记录。
- 导入导出。

Tauri Rust 壳负责：

- Engine 进程生命周期和 RPC 代理。
- Windows Credential Manager 凭据恢复。
- 桌面挂件 owner、置底、Win+D、Explorer 重启恢复。
- Mica / Acrylic 材质。
- 吸附、收起、托盘、单实例、通知、文件对话框和更新。

## 3. 功能验收

| 功能 | 结果 | 验证方式 |
|---|---|---|
| 真实 39 条任务加载 | 通过 | Release 实例读取真实数据库 |
| 29 条未完成任务展示 | 通过 | 挂件截图 |
| 快速新增 | 通过 | 隔离数据库副本 UI 操作 |
| 编辑面板 | 通过 | 隔离数据库副本 UI 操作 |
| 完成与取消完成 | 通过 | 隔离数据库副本 UI 操作 |
| 删除二次确认 | 通过 | 挂件 UI 截图与自动取消 |
| 拖拽排序 | 已实现 | 采用旧版 Reorder 交互，待最终手势回归 |
| 悬浮删除渐显渐隐 | 通过 | 180ms opacity 过渡 |
| 删除按钮变红 | 通过 | 350ms foreground 过渡 |
| 圆环悬浮高亮 | 通过 | 挂件截图 |
| 设置面板 | 通过 | Release UI 截图 |
| 窗口材质 | 通过 | Windows 11 原生 SystemBackdrop |
| 同步状态和设备列表 | 通过 | `https://todo.renrunqi.com`，三台设备 |
| MCP | 通过 | Engine 运行中，token 和四个模板可见 |
| 互传启动与停止 | 通过 | 新端口 `54417` 端到端调用 |
| 互传窗口 | 通过 | 单实例窗口截图 |
| 月视图窗口 | 通过 | Tauri 动态窗口命令 |
| JSON 导出 | 通过 | Engine `tasks.export_json` |
| iCalendar / TickTick CSV | 通过 | 新壳本地格式导出 |
| 托盘 | 通过 | 显示、隐藏、互传、退出 |
| 单实例 | 通过 | Tauri single-instance 插件 |
| 开机自启 | 通过 | 现有设置和插件 |
| 更新 | 待发布验证 | GitHub Release endpoint 已配置 |

## 4. 自动测试

```text
Tauri UI TypeScript：4 passed
Rust workspace：10 passed
transfer-core：28 passed
Flutter 互传相关：34 passed
Flutter analyze：No issues found
```

桌面 release 构建成功。Rust 壳 `cargo check` 零错误、零警告。

## 5. 数据安全

- 真实数据库：`%LOCALAPPDATA%\rtools\engine\rtools.db`
- 真实数据库验收前有 39 条未删除任务，其中 29 条未完成。
- 破坏性 CRUD 在数据库副本上执行。
- 副本执行创建、更新、完成、删除后，未删除任务数仍为 39，未完成数仍为 29。
- 真实数据库未执行删除确认。
- 旧 Avalonia 安装包保留，可回退。

## 6. 互传端口调整

Windows Hyper-V/WSL 将 `53317` 和 `53318` 纳入 TCP 保留段。RTools 默认互传
端口改为 `54417`。发现协议携带实际端口，因此桌面和手机无需依赖固定端口。

## 6.1 窗口材质

主窗口、月视图和互传窗口均使用 Windows 11 DWM `SystemBackdrop`：

- “实时透明”和“亚克力”使用 `DWMSBT_TRANSIENTWINDOW`，会实时反映窗口背后的
  内容和壁纸。
- Mica 和 Mica Alt 使用微软原生壁纸取色语义，不反映窗口背后的应用内容。
- 已移除旧版“读取壁纸、高斯模糊、转成 PNG data URI”的静态模拟路径。
- WebView 只叠加固定轻量 tint，背景保持透明。
- 原“透明度”滑杆已移除，避免提供用户难以感知且语义不准确的调节项。

## 7. 已知限制

- Windows 防火墙首次启动会询问是否允许 `rtools-engine.exe` 访问网络。允许后
  互传和同步使用正常。
- 当前安装包为个人自用构建，未做商业代码签名。Windows 可能显示未知发布者。
- 安装包由 Inno Setup 生成，覆盖安装不会删除 Engine 数据目录。
- 更新检查已有代码路径，但尚无已签名的 Release 清单和签名公钥，正式发布前
  需要完成签名配置。
- 本机互传已通过启动、监听和停止测试；跨设备发送与接收仍需在允许 Windows
  防火墙后做一次真机回归。

## 8. 回退

若新壳出现不可恢复问题：

1. 退出 RTools。
2. 启动 `E:\artifacts\release-final\RTools.Desktop.exe`。
3. 数据目录保持不变，无需重新导入。
4. Avalonia 版仍会读取同一 SQLite 数据库。
