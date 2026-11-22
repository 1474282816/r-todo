# rtools

个人跨端工具集。当前包含 Windows 待办、局域网互传、无线设备电量挂件，Android 待办，以及 Rust 自托管同步服务。

## 技术栈

| 端 | 技术 |
|---|---|
| Windows | Tauri 2 + React + Rust Engine |
| Android / iPad | Flutter |
| 同步服务 | Rust + Axum + SQLite（Docker） |

## 目录结构

```
rtools/
├── apps/
│   ├── desktop/          # 当前桌面端（Tauri 2 + React + Rust）
│   ├── desktop-tauri/    # 历史 Tauri 重建稿（归档，不再构建）
│   ├── desktop-avalonia/ # 历史 Avalonia 实现（已废弃）
│   └── mobile/           # Flutter 移动端
├── crates/
│   ├── rtools-workspace/ # Rust Engine / 同步服务 / 无线设备电量
│   └── transfer-core/    # LocalSend 兼容互传核心
├── packages/
│   └── schema/           # 数据模型 JSON Schema（唯一真源）
├── deploy/               # rtools-syncd 部署
├── docs/                 # 需求 / 架构 / ADR
└── scripts/              # 便利脚本
```

## 环境要求

| 工具 | 用途 | 检查命令 |
|---|---|---|
| Rust (stable) | Engine / 同步服务 | `rustc --version` |
| Node 20+ / pnpm 10.33 | 桌面端前端 | `pnpm --version` |
| Flutter (stable) | 移动端 | `flutter --version` |
| Docker | 部署同步服务（**在 Linux 服务器上**，非 Windows 开发机） | `docker --version` |

Windows 打包用 Tauri 自带的 NSIS 目标，首次构建会自动下载 NSIS 工具链，无需手工安装。

## 快速开始

```powershell
# 仓库根目录
pnpm install

# 起桌面端（等价于 cd apps/desktop && pnpm tauri dev）
pnpm dev:desktop

# 打 Windows 安装包（产物在 apps/desktop/src-tauri/target/release/bundle/nsis）
pnpm build:desktop

# 跑 Rust 侧测试
pnpm test:rust

# 起移动端（需已启动模拟器或连真机）
pnpm dev:mobile
```

## AI 操作待办（MCP）

桌面端内置 MCP 服务器（默认关闭），让 Claude Code / Claude Desktop / opencode / codex 等 AI 直接读写待办。

启用：设置 →「MCP 服务器」→ 打开开关 → 复制对应 agent 的配置模板并执行一次（Claude Code 为 `claude mcp add` 命令）。

- 地址：`http://127.0.0.1:18765/mcp`（仅本机，Bearer token 认证）
- 工具：`list_tasks` / `create_task` / `update_task` / `complete_task` / `uncomplete_task` / `delete_task`
- AI 的写操作实时刷新桌面 UI，并进入同步队列（在线时同步到 rtools-syncd）
- 安全：开关默认关；token 在设置面板可重置；端口仅监听 127.0.0.1
- opencode / codex 的配置模板已按官方格式内置，但未经真机验证（本机未安装）；如报 schema 错误，请按官方文档格式手动调整后反馈

## 局域网互传（LocalSend 兼容）

桌面端内置局域网文件互传（**默认关闭**，与待办同步互不影响）：

- 入口：主窗口右上「互传」按钮 / 设置 →「局域网互传」/ 托盘「打开互传」
- 协议：LocalSend **HTTP(S) v2**，端口 **53317**，HTTPS 自签证书 + fingerprint 固定
- 发现：UDP 组播 + 手动 IP（多网卡/虚拟网卡发现不到时用手动 IP）
- 可与官方 LocalSend（Windows / Android）及其他 rtools 设备双向互传
- 首次接收需要确认，确认后可对已信任设备自动接收；默认保存到 `下载/rtools`
- 协议实现复用 LocalSend 官方 Rust core：`crates/vendor/localsend-core`（Apache-2.0，pin rev）

## 无线设备电量挂件（Windows）

独立小窗口常驻桌面，一行一台设备：图标 + 名称 + 电量条 + 百分比。
入口：主窗口右上电池图标 / 托盘菜单。

- 已支持：罗技 G502（HID++ 2.0）、Leobog A75、迈从（Compx 厂商 Feature Report）、通用 HID 电量字段
- 充电状态：G502 读报文标志位 `0x80`；A75/迈从靠设备插拔线时主动推的状态帧（位掩码 `0x10`），
  由常驻只读线程守着，不写任何厂商命令
- 源码：核心 `crates/rtools-workspace/rtools-battery`，窗口 `apps/desktop/src/windows/BatteryWidget.tsx`
- 新增设备：在 `crates/rtools-workspace/rtools-battery/src/profile.rs` 加一条设备档案即可，ID 按产品名而非 PID（罗技插线会换 PID）
- 抓报文：`cargo run -p rtools-battery --example a75_watch -- 300`（只读，30 秒内插拔一次线就能看到状态帧）

## 待办数据位置

`%APPDATA%\dev.rtodo.desktop\r-todo.db`（SQLite），同步队列与本地状态都在里面。
删数据库 = 清空待办，升级框架不影响该文件。

## 文档

- [需求文档 v1.0](docs/requirements.md)
- [迁移设计](docs/superpowers/specs/2026-11-16-avalonia-rust-desktop-sync-design.md)
- [架构决策记录 (ADR)](docs/decisions/)
- [互传 handoff（桌面）](docs/handoff/2026-11-08-desktop-transfer.md)
- [互传 handoff（手机）](docs/handoff/2026-11-08-mobile-transfer.md)

## 状态

当前版本为 `0.3.4`。桌面端已切换为 Tauri 2，Rust Engine、Android、
同步服务与互传继续共用。安装包见
[Tauri 桌面重建交付说明](docs/handoff/2026-11-17-tauri-desktop-rebuild.md)。
