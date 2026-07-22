# 同步协议约定

## 概述

r-todo 使用 **PocketBase** 作为后端，同步走两条通道：

1. **REST**（`/api/collections/tasks/records`）—— 读写、初次全量拉取
2. **WebSocket Realtime**（`/api/realtime`）—— 服务端推送变更事件

## 客户端登录流程

```
[用户输入 URL + 邮箱 + 密码]
         │
         ▼
   POST /api/collections/users/auth-with-password
         │
         ▼
   保存 auth token 到本地安全存储
         │
         ▼
   订阅 tasks collection realtime
```

## 同步策略：Last-Write-Wins

- 客户端每次编辑 task 时，本地立即更新，并推送 REST 请求
- 网络失败则排队重试（本地队列）
- 服务端以 `updated_at` 为准；`updated_at` 较新的写覆盖旧的
- WebSocket 推送到达时，比较本地记录的 `updated_at`：
  - 服务端更新 → 覆盖本地
  - 本地更新 → 忽略（本地版本更新，等待推送同步过去）

## Realtime 订阅事件

| 事件 | 载荷 |
|---|---|
| `create` | 新 task 记录 |
| `update` | 修改后的 task 记录 |
| `delete` | 被删除的 task id |

## 离线策略

- 本地 SQLite / Drift 是唯一读源
- 写操作先写本地，再同步到服务端
- 未同步的写操作放在 `pending_ops` 队列（表结构待定）

## 相关文档

- PocketBase Realtime: <https://pocketbase.io/docs/api-realtime/>
- 决策记录: `docs/decisions/003-sync-strategy.md`
