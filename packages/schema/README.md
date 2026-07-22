# Schema

**任务数据模型的唯一真源。**

## 文件

- `task.schema.json` — JSON Schema (Draft 07)

## 使用规范

修改 `task.schema.json` 时，**必须**同步更新以下位置：

| 位置 | 文件 |
|---|---|
| Rust 侧（Tauri）| `apps/desktop/src-tauri/src/models/task.rs`（尚未创建） |
| TypeScript 侧（Tauri 前端）| `apps/desktop/src/lib/task.ts`（尚未创建） |
| Dart 侧（Flutter）| `apps/mobile/lib/shared/models/task.dart` |
| PocketBase Collection | `server/pb_migrations/*.js`（新增迁移文件） |

## 为什么不自动生成？

- 三端语言生态差异大，代码生成器（openapi-generator 等）会引入构建步骤
- 手动同步 + AI 辅助，改动即时可见、无黑箱

## 生成 TS 类型（可选，喜欢的话）

```powershell
npx quicktype packages/schema/task.schema.json -o apps/desktop/src/lib/task.ts --lang typescript
```
