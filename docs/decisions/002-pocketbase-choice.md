# ADR 002: 后端选择 PocketBase

**状态**: Accepted
**日期**: 2026-07-18

## 上下文

r-todo 需要一个后端提供：

- 三端数据同步
- 支持自托管（Docker 一键起）
- 用户以 "URL + 密钥" 形式登录（无邮箱注册体系是产品需求 4.8）
- 实时推送优先

规模预估：单人使用，最多几百条 task。

## 决策

使用 **PocketBase 0.22+**，Docker 部署，SQLite 存储。

## 理由

1. **部署极简**：官方镜像一行 docker compose up 即启
2. **自带 REST + WebSocket Realtime**：无需自撸同步层
3. **自带管理后台**：省一个前端项目
4. **JS Hooks 灵活**：如果需要"自定义长期 API Key"功能，可在 hooks 里实现
5. **单文件数据**：`pb_data/` 打包即备份
6. **合适的规模**：SQLite 单人用绰绰有余

## 候选方案对比

| 方案 | 淘汰理由 |
|---|---|
| Supabase 自托管 | Docker Compose 十几个容器，运维负担重 |
| 自撸 Go/Rust 后端 | 实时推送要自己写，工作量大 |
| Firebase | 非自托管，与产品需求"自托管友好"冲突 |
| Appwrite | 部署复杂度接近 Supabase，无 PocketBase 简洁 |
| ElectricSQL | 过度设计（CRDT 对单人待办场景无意义） |

## 后果

- **优点**：开发速度快、运维成本几乎为零
- **缺点**：SQLite 单写并发差（对单用户无影响）
- **迁移路径**：若未来规模扩大，PocketBase 支持切换到 Postgres 后端
