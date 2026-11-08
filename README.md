# r-todo

极简跨端待办工具 —— Windows / Android / iPad 三端 + 桌面常驻 + 自托管同步。

## 技术栈

| 端 | 技术 |
|---|---|
| Windows | Tauri 2 (Rust) + React + TypeScript + Tailwind CSS |
| Android / iPad | Flutter |
| 后端 | PocketBase (Docker) |

## 目录结构

```
r-todo/
├── apps/
│   ├── desktop/          # Tauri 2 桌面端
│   └── mobile/           # Flutter 移动端
├── packages/
│   ├── schema/           # 数据模型 JSON Schema（唯一真源）
│   ├── api-contract/     # 同步协议约定
│   └── design-tokens/    # 视觉规范（颜色、间距）
├── server/               # PocketBase 部署配置
├── docs/                 # 需求 / 架构 / ADR
└── scripts/              # 便利脚本
```

## 环境要求

| 工具 | 用途 | 检查命令 |
|---|---|---|
| Node.js ≥ 20 | Tauri 前端 | `node --version` |
| pnpm ≥ 10 | 包管理 | `pnpm --version` |
| Rust (stable) | Tauri 后端 | `rustc --version` |
| Flutter (stable) | 移动端 | `flutter --version` |
| Docker | 部署 PocketBase（**在 Linux 服务器上**，非 Windows 开发机） | `docker --version` |

Windows 编译 Tauri 还需要：**Microsoft C++ Build Tools** 和 **WebView2 Runtime**（Win11 已内置）。

## 快速开始

```powershell
# 装依赖
pnpm install

# 起 PocketBase (Docker)
pnpm dev:server

# 起桌面端
pnpm dev:desktop

# 起移动端（需已启动模拟器或连真机）
pnpm dev:mobile
```

## AI 操作待办（MCP）

桌面端内置 MCP 服务器（默认关闭），让 Claude Code / Claude Desktop / opencode / codex 等 AI 直接读写待办。

启用：设置 →「MCP 服务器」→ 打开开关 → 复制对应 agent 的配置模板并执行一次（Claude Code 为 `claude mcp add` 命令）。

- 地址：`http://127.0.0.1:18765/mcp`（仅本机，Bearer token 认证）
- 工具：`list_tasks` / `create_task` / `update_task` / `complete_task` / `uncomplete_task` / `delete_task`
- AI 的写操作实时刷新桌面 UI，并进入现有同步链路（在线时自动同步到 PocketBase，手机端同步可见）
- 安全：开关默认关；token 在设置面板可重置；端口仅监听 127.0.0.1
- opencode / codex 的配置模板已按官方格式内置，但未经真机验证（本机未安装）；如报 schema 错误，请按官方文档格式手动调整后反馈

## 局域网互传（LocalSend 兼容）

桌面端内置局域网文件互传（**默认关闭**，与待办同步互不影响）：

- 入口：主窗口右上「互传」按钮 / 设置 →「局域网互传」/ 托盘「打开互传」
- 协议：LocalSend **HTTP(S) v2**，端口 **53317**，HTTPS 自签证书 + fingerprint 固定
- 发现：UDP 组播 + 手动 IP（多网卡/虚拟网卡发现不到时用手动 IP）
- 可与官方 LocalSend（Windows / Android）及其他 r-todo 设备双向互传
- 接收前**必须人工确认**；默认保存到 `下载/r-todo`（可在互传窗口修改），重名自动加序号
- 协议实现复用 LocalSend 官方 Rust core：`crates/vendor/localsend-core`（Apache-2.0，pin rev）

## 文档

- [需求文档 v1.0](docs/requirements.md)
- [技术栈说明](docs/tech-stack.md)
- [架构决策记录 (ADR)](docs/decisions/)
- [互传 handoff（桌面）](docs/handoff/2026-11-08-desktop-transfer.md)
- [互传 handoff（手机）](docs/handoff/2026-11-08-mobile-transfer.md)

## 状态

🚧 待办（桌面/手机/同步）+ 桌面互传 MVP 已实现；互传真机互通待验收。
