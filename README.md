# rtools

个人跨端工具集。当前包含 Windows 待办与局域网互传、Android 待办，以及 Rust 自托管同步服务。

## 技术栈

| 端 | 技术 |
|---|---|
| Windows | Avalonia 12 + C# + Rust Engine |
| Android / iPad | Flutter |
| 同步服务 | Rust + Axum + SQLite（Docker） |

## 目录结构

```
rtools/
├── apps/
│   ├── desktop-avalonia/ # Avalonia 桌面端
│   ├── desktop/          # 旧 Tauri 实现（归档）
│   └── mobile/           # Flutter 移动端
├── crates/
│   ├── rtools-workspace/ # Rust Engine 与同步服务
│   └── transfer-core/    # LocalSend 兼容互传核心
├── packages/
│   ├── schema/           # 数据模型 JSON Schema（唯一真源）
│   ├── api-contract/     # 同步协议约定
│   └── design-tokens/    # 视觉规范（颜色、间距）
├── deploy/               # rtools-syncd 部署
├── docs/                 # 需求 / 架构 / ADR
└── scripts/              # 便利脚本
```

## 环境要求

| 工具 | 用途 | 检查命令 |
|---|---|---|
| .NET 10 | Avalonia 桌面端 | `dotnet --version` |
| Rust (stable) | Engine / 同步服务 | `rustc --version` |
| Flutter (stable) | 移动端 | `flutter --version` |
| Docker | 部署同步服务（**在 Linux 服务器上**，非 Windows 开发机） | `docker --version` |

Windows 发布还需要 Inno Setup 6。

## 快速开始

```powershell
# 构建 Rust Engine
cd crates/rtools-workspace
cargo build -p rtools-engine

# 起 Avalonia 桌面端
cd ../../apps/desktop-avalonia
dotnet run --project src/RTools.Desktop/RTools.Desktop.csproj

# 起移动端（需已启动模拟器或连真机）
cd ../mobile
flutter run
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

## 文档

- [需求文档 v1.0](docs/requirements.md)
- [迁移设计](docs/superpowers/specs/2026-11-16-avalonia-rust-desktop-sync-design.md)
- [架构决策记录 (ADR)](docs/decisions/)
- [互传 handoff（桌面）](docs/handoff/2026-11-08-desktop-transfer.md)
- [互传 handoff（手机）](docs/handoff/2026-11-08-mobile-transfer.md)

## 状态

当前版本为 `0.2.0`。桌面、Android、同步服务与互传均已接入；安装包见 [发布说明](docs/release-notes-rtools-0.2.0.md)。
