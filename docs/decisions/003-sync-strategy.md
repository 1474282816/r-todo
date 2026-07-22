# ADR 003: 同步策略

**状态**: Accepted
**日期**: 2026-07-18

## 上下文

r-todo 三端需要同步 task 数据。约束：

- 本地优先：断网也能完整使用
- 自托管：客户端连接的是用户自己的 PocketBase 实例
- 单用户为主：不存在多人协作编辑同一 task
- 需求 4.8 明确"实时同步能做到最好"

## 决策

**冲突策略**：Last-Write-Wins（LWW）by `updated_at`
**传输通道**：PocketBase Realtime WebSocket + REST 兜底
**登录方式**：初期用 PocketBase 内置的邮箱密码登录，客户端保存 token 后自动刷新；后续视需要扩展为长期 API Key

## 理由

### 为什么 LWW 而不是 CRDT

- 单人使用，同一 task 极少并发编辑
- LWW 实现简单：`if remote.updated_at > local.updated_at then overwrite`
- CRDT（如 Yjs）会引入大量额外复杂度和数据体积，收益微乎其微

### 为什么 WebSocket + REST

- WebSocket 保证低延迟（改动秒级同步）
- REST 用于：初次全量拉取、离线恢复后批量补传
- PocketBase Realtime 原生就是这个组合

### 为什么不做"URL + 永久 Key"路径 1

产品需求希望"填 URL + 密钥"即可登录，但 PocketBase 原生的登录 token 有 7 天过期。三条路径：

1. **邮箱密码登录 + 客户端自动刷新** ← 当前选择
2. 写 JS Hook 增加长期 API Key collection
3. 客户端直接保存邮箱密码，不用 token

选路径 1 的理由：
- 骨架阶段先跑通 MVP
- 用户实际感知与"永久密钥"接近（只要 App 保持登录）
- 待用户反馈体验不佳时再迭代到路径 2

## 后果

- **优点**：实现简单、生态成熟、Bug 少
- **缺点**：Token 刷新失败时用户需要重新输入邮箱密码（低频事件）
- **风险**：当用户长期未打开 App（例如换机后），token 已过期需要输入密码 → 记住密码或走路径 2 解决

## 数据流示例

### 用户创建一个 task

```
[UI] 点击 "+ 加任务"
  → [本地 SQLite] INSERT INTO tasks (status='pending_create')
  → [UI] 立即渲染
  → [Sync Worker] POST /api/collections/tasks/records
    ├─ 成功 → 更新本地 status='synced'
    └─ 失败 → 保留 pending_create, 下次网络恢复重试
```

### 服务端推送变更

```
[PB Realtime] receive event "update" for tasks/xyz
  → [Sync Worker] 比较 remote.updated_at vs local.updated_at
    ├─ remote 新 → 覆盖本地, UI 刷新
    └─ local 新 → 忽略（本地版本会稍后推送到服务端）
```
