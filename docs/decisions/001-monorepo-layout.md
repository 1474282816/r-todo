# ADR 001: Monorepo 布局

**状态**: Accepted
**日期**: 2026-07-18

## 上下文

r-todo 需要维护两个客户端：Tauri 2 (Rust + TypeScript) 桌面端与 Flutter 移动端。二者需要共享数据模型定义、视觉规范和同步协议约定。

## 决策

采用 **纯目录 + pnpm workspace** 的 monorepo 布局，不引入 Nx / Turborepo / Bazel 等重型 monorepo 工具。

```
apps/       # 应用（desktop, mobile）
packages/   # 共享约定（非代码共享）
server/     # PocketBase 部署配置
docs/       # 文档与 ADR
```

## 理由

1. **两个客户端使用完全不同的构建工具链**（Cargo + Vite vs Flutter），任何 monorepo 工具都无法真正统一其构建流程
2. **共享的不是代码，是"约定"**：JSON Schema、颜色 tokens、API 文档。这些是数据，不需要构建
3. **pnpm workspace 足够**管理 Tauri 前端的 JS 依赖
4. **vibecoding 友好**：AI 工具一次能看到全仓库上下文，改动传播直观
5. **零学习成本**：不需要学 Nx generators / Turbo pipelines

## 后果

- **优点**：结构直观、依赖清晰、单人开发心智负担低
- **缺点**：无跨包统一 CI 缓存（对个人项目无关紧要）
- **风险**：改 schema 时需要人工同步到各端 —— 用 AI 辅助 + 明确的 README 规范来缓解

## 替代方案（已否决）

- **Nx**: 对 Rust/Flutter 支持有限，配置复杂度高
- **Turborepo**: 主要给 JS 项目，Rust/Flutter 用不上
- **Bazel**: 学习曲线太陡，个人项目过度设计
