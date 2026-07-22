# 改动日志

## 2026-07-18 · 用户睡觉时的一次性推进（M2~M5）

### 用户上下文

用户明确授权："算了，你先开发下一步吧，这个先记为待优化项。我要睡觉去了，你不要等我一步一步确认了，就往后一直开发吧，我睡醒起来再帮你验证"

### 已完成里程碑

| 里程碑 | 状态 |
|---|---|
| **M1** Windows Hello World | ✅ 已完成（用户已亲验 Mica/Acrylic + 缩放） |
| **M2** WorkerW 桌面嵌入 + 位置记忆 + 开机自启 + 单实例 | ✅ Rust 编译通过，运行时未验证 |
| **M3** SQLite 本地存储 + Task CRUD + Things 3 风任务列表 UI | ✅ 前后端编译通过，运行时未验证 |
| **M4** RRULE 重复规则 + 提醒调度 + 系统通知 + 编辑面板 | ✅ 前后端编译通过，运行时未验证 |
| **M5** 视觉打磨（微动画 + 深浅色 + 快捷键 + 零态） | ✅ 前后端编译通过，运行时未验证 |

### 待优化（M1 遗留）

- **OPT-001** 拖动跟手：改回 `data-tauri-drag-region` + `select-none`（当前版本），待醒后验证；如仍飘则走 Backlog 备选方案

### 详细技术改动

#### Rust 侧

**新增文件**：
- `apps/desktop/src-tauri/src/db.rs` — SQLite migration 定义（tauri-plugin-sql）
- `apps/desktop/src-tauri/src/notifier.rs` — 提醒调度模块，tokio 定时任务 + tauri-plugin-notification

**修改文件**：
- `apps/desktop/src-tauri/Cargo.toml`：
  - `window-vibrancy` 0.5 → 0.6
  - 新增：`tauri-plugin-sql` (sqlite feature), `tauri-plugin-notification`, `tauri-plugin-window-state`, `tauri-plugin-autostart`, `tauri-plugin-single-instance`
  - 新增：`chrono`, `uuid`, `tokio`, `log`
  - Windows 特性：新增 `Win32_UI_Shell` / `Win32_System_LibraryLoader`
- `apps/desktop/src-tauri/src/lib.rs`：
  - 集成上述 5 个插件
  - Mica → Acrylic 降级链（Win10 也能有毛玻璃）
- `apps/desktop/src-tauri/src/desktop_embed.rs`：
  - **实装** WorkerW 挂载算法（原骨架只留 stub）
  - `find_worker_w()`：发 0x052C → EnumWindows → 找到有 SHELLDLL_DefView 兄弟的 WorkerW
  - `embed_to_desktop` / `unembed_from_desktop` command
  - 适配 windows 0.58 的 HWND 新签名（`*mut c_void`）
- `apps/desktop/src-tauri/tauri.conf.json`：新增 `plugins.sql.preload`
- `apps/desktop/src-tauri/capabilities/default.json`：加所有插件权限

**删除文件**：
- `apps/desktop/src-tauri/src/models.rs` — 前端直接读写 SQLite，不经 Rust CRUD 层，删除无用 Rust struct

#### 前端 TypeScript 侧

**新增依赖**：`@tauri-apps/plugin-sql` `@tauri-apps/plugin-notification` `@tauri-apps/plugin-window-state` `@tauri-apps/plugin-autostart` `zustand` `immer` `rrule` `nanoid` `date-fns` `lucide-react` `framer-motion` `clsx`

**新增文件**：

域层（`src/lib/`）：
- `types.ts` — Task/Priority/常量
- `db.ts` — SQLite CRUD（tauri-plugin-sql）
- `store.ts` — Zustand + Immer store，所有 mutation 走这里
- `reminder.ts` — 提醒调度前端桥（invoke Rust command）
- `rrule.ts` — RRULE 构建/解析/推进（rrule.js），含"完成后 rollNext"
- `system.ts` — 桌面嵌入 + 自启前端桥
- `datetime.ts` — "今天/明天/6月20日" 时间格式化 + 次日隐藏判定
- `cn.ts` — className 合并
- `hooks/useHotkeys.ts` — Ctrl+N / Escape 全局快捷键

UI 组件（`src/components/`）：
- `ui/Checkbox.tsx` — Things 3 风空心圆勾选，带 tap 缩放动画
- `ui/PriorityBar.tsx` — 任务左侧优先级竖条
- `ui/IconButton.tsx` — 通用图标按钮
- `task/QuickAdd.tsx` — 顶部快速添加，含优先级选择器，暴露 focus ref
- `task/TaskRow.tsx` — 单行任务，含提醒/重复徽标 + hover 删除
- `task/TaskEditor.tsx` — 底部弹出编辑面板：标题/优先级/提醒时间/提前分钟/重复规则（预设 + 周几）
- `settings/SettingsPanel.tsx` — 设置面板：嵌入桌面 / 开机自启 开关

**重写文件**：
- `App.tsx` — 完整主界面，包含拖动条、快速添加、列表分区（未完成 + 今日已完成）、编辑面板、设置面板
- `styles.css` — 全局 select-none + 极简滚动条 + 深色 datetime input

### 编译验证结果

| 项 | 命令 | 结果 |
|---|---|---|
| TypeScript 类型 | `pnpm --filter desktop build` | ✅ 通过，无警告 |
| Vite 打包 | 同上 | ✅ 6.6s, JS 336KB (gzip 108KB) |
| Rust 检查 | `cargo check` | ✅ 通过，0 警告 |
| Rust 完整编译 | `cargo build` | ✅ 23.91s，产物 22MB `r-todo-desktop.exe` |

### 手工 QA 未覆盖（用户睡醒需验证）

以下功能已实现且编译通过，但**运行时行为未由 AI 亲眼验证**（GUI 应用限制）：

1. **数据 CRUD**：添加/编辑/勾选/删除任务 → SQLite 持久化
2. **重复任务**：设置每天/工作日等 → 勾选完成后自动生成下一实例
3. **提醒**：设置提醒时间 + 提前分钟数 → 到点弹系统通知
4. **桌面嵌入**：设置面板打开"嵌入桌面" → 窗口挂到 WorkerW（Win+D 不隐藏）
5. **开机自启**：设置面板打开 → 重启后自动启动
6. **位置/尺寸记忆**：拖动/缩放窗口 → 关闭再打开保留位置
7. **快捷键**：Ctrl+N 聚焦输入框，Esc 关闭浮层
8. **视觉**：Things 3 风格的极简列表、优先级色标、深浅色跟随系统

### 用户第二天的验收路径

```powershell
pnpm dev:desktop
```

预期看到：
- 380x620 无边框毛玻璃窗口
- 顶部 "r-todo" 标 + 右上设置按钮
- 输入框 + 3 个优先级圆点
- 空列表（首次运行）"今天没有任务"

功能测试建议顺序：
1. 加一条任务，勾选，取消勾选 → SQL 写入生效？
2. 点任务打开编辑面板，改优先级 + 加提醒（选 2 分钟后）→ 到点弹通知？
3. 设置面板 → 嵌入桌面 → 窗口应该"沉入"桌面
4. 设置面板 → 开机自启 → 打开
5. Ctrl+N 应聚焦输入框；有编辑面板时按 Esc 应关闭

### 下一步方向

- 用户验收后修 OPT-001 或其他运行时 bug
- M6~M9 移动端（Flutter）
- M10 PocketBase 部署 + 同步
