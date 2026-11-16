# rtools 0.2.0 发布说明

发布日期：2026-11-17

rtools 0.2.0 把原 r-todo 桌面端迁移到 Avalonia 12 和 C#，同时保留 Rust Engine。桌面端和 Android 端已经切换到新的 Rust 同步服务，现有待办已迁移到新数据库。

## 安装文件

- Windows 安装包：`artifacts/release/rtools-0.2.0-setup.exe`
- Windows 便携压缩包：`artifacts/release/rtools-0.2.0-win-x64.zip`
- Android APK：`artifacts/release/rtools-0.2.0-android.apk`

## 主要功能

- 待办新增、编辑、完成、取消完成、删除、上移排序和跨端增量同步。
- 提醒时间、提前提醒、重复任务和月视图完成记录。
- 一次性接入码配对、设备列表、设备撤销和 Windows 凭据管理器令牌存储。
- 本地 MCP 服务，提供 `list_tasks`、`create_task`、`update_task`、`complete_task`、`uncomplete_task`、`delete_task`。
- LocalSend 兼容互传、文件发送、接收确认、拒绝、取消、进度和会话消息。
- r-todo JSON 导入、JSON/iCalendar/TickTick CSV 导出和 TickTick CSV 导入。
- 托盘、开机自启、单实例、窗口位置记忆、窗口吸附和桌面挂件。
- Android 端的同步、提醒、重复任务、小组件和互传功能。

## 数据与兼容

- 旧待办从观星云 PocketBase 导出后导入新同步服务，共 39 条。
- 新桌面端使用 `%LOCALAPPDATA%\rtools\engine\rtools.db`。
- 不兼容旧桌面设置、旧登录态、旧 MCP token 和旧互传身份。
- 旧 PocketBase 数据保留在服务器备份目录，新客户端不再依赖 PocketBase。

## 发布校验

- Rust 测试：`cargo test -p rtools-domain -p rtools-engine -p rtools-syncd` 通过。
- Flutter：`flutter analyze` 无问题，全部测试通过。
- .NET：7 项测试通过。
- Windows 便携包和 Inno Setup 安装包均已实际安装、启动并连接 Engine 与同步服务。
- 同步服务：`https://todo.renrunqi.com/api/v1/health` 返回正常，SQLite 完整性为 `ok`。

## 校验值

- Windows 安装包 SHA-256：`47A94ABD7D41AF6A3661DCC96F4541105DCCFE00533B5DEB463320C71F7BC21F`
- Windows 便携压缩包 SHA-256：`9B8F6C80818BE1CC24943CFF71F0CDAC7E0450A96D90219E9BCDBB32E43E3392`
- Android APK SHA-256：`D9488427C2CF661981E2DA9DF4B5CDA429DD2A8B9B3667AC3897484C2364DE42`
