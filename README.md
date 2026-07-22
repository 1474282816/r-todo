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

## 文档

- [需求文档 v1.0](docs/requirements.md)
- [技术栈说明](docs/tech-stack.md)
- [架构决策记录 (ADR)](docs/decisions/)

## 状态

🚧 骨架搭建完成，业务代码待实现。
