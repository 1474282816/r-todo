# 技术栈说明

## 客户端

| 端 | 语言 | UI 层 | 关键库 |
|---|---|---|---|
| Windows | Rust + TypeScript | Tauri 2 + React + Tailwind | window-vibrancy, windows-rs |
| Android / iPad | Dart | Flutter | home_widget, drift, pocketbase, rrule |

## 服务端

**PocketBase 0.22+**（Docker 部署）
- 单二进制 Go 应用
- SQLite 存储
- 自带 REST + WebSocket + 管理后台
- JS Hooks 扩展点

## 同步

- 本地存储：Drift (Flutter) / SQLite via Tauri plugin (桌面)
- 云端：PocketBase
- 冲突：Last-Write-Wins（`updated_at`）
- 实时：PocketBase Realtime WebSocket

## Monorepo 布局

见 `docs/decisions/001-monorepo-layout.md`。

## 各端目录

- `apps/desktop/` — Tauri 桌面端
- `apps/mobile/` — Flutter 移动端
- `packages/schema/` — 数据模型 JSON Schema
- `packages/api-contract/` — 同步协议文档
- `packages/design-tokens/` — 视觉规范
- `server/` — PocketBase 部署配置
