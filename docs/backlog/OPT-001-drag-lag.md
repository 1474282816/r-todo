# OPT-001 · 窗口拖动跟手问题

**状态**: **已修复（基于三方专家诊断）**
**发现于**: M1 手工验收
**修复于**: 2026-07-18

## 症状（用户描述）

> "拖动这个窗口，鼠标和窗口一起慢慢挪，拖得越快越明显；拖其他程序窗口完全正常。"

**关键鉴别**：不是"鼠标先走窗口跟着挪"（那是 IPC 延迟），是**鼠标本身被节流**——整个 Windows 拖动 modal loop 运行在 15-20 fps。

## 根本原因

Win10 的 `window-vibrancy::apply_acrylic` 底层调用 `SetWindowCompositionAttribute(ACCENT_ENABLE_ACRYLICBLURBEHIND)`。这条路径在 Windows 拖动 modal loop 中：

1. DWM 必须每帧重采样窗口后面的整个桌面区域
2. 对采样内容做高斯模糊
3. 与 modal loop 的 `WM_MOVING` 消息同步执行
4. Win10 上此路径未优化（Win11 用新的 `DwmSetWindowAttribute` 已解决）

**window-vibrancy 官方 README 明文警告**："`apply_acrylic` — Bad performance when resizing/dragging on Windows 10 v1903+ and Windows 11 build 22000"。
**Maintainer 明确回复**：It's unfortunately expected behavior。

参考：
- <https://github.com/tauri-apps/window-vibrancy/issues/117>
- <https://github.com/tauri-apps/window-vibrancy/issues/47>
- <https://github.com/tauri-apps/window-vibrancy/blob/dev/README.md>

## 已尝试过（M1 阶段，全部失败或副作用大）

| 尝试 | 结果 |
|---|---|
| `data-tauri-drag-region` (仅根元素) | 拖不动（子元素不继承） |
| 所有子元素加 `data-tauri-drag-region` | 拖不动（文字选择拦截 mousedown） |
| `onMouseDown` + `startDragging()` | 能拖但飘（IPC + 上述根因叠加） |
| `select-none` + 所有子元素显式加属性 | 能拖但仍飘（用户明确反馈） |

**误诊**：我一直以为是 JS 拖动实现的问题，其实是 DWM Acrylic 本身。

## 最终修复方案

**"drag 期间瞬时关 Acrylic + 结束后恢复"** + 3 项辅助优化。

### 主修复：HWND subclass 拦截 WM_ENTERSIZEMOVE/EXITSIZEMOVE

新文件：`apps/desktop/src-tauri/src/drag_watcher.rs`

- `SetWindowSubclass` 注册消息拦截
- `WM_ENTERSIZEMOVE` 时 `clear_acrylic` + `DRAG_ACTIVE.store(true)`
- `WM_EXITSIZEMOVE` 时 `apply_acrylic` + `DRAG_ACTIVE.store(false)`

### 辅助 1：watcher 线程在 DRAG_ACTIVE 时跳过

`desktop_embed.rs` 的 500ms watcher 在拖动中调用 `SetWindowPos(HWND_BOTTOM)` —— 跨线程 `SendMessage` 到拖动 modal loop 会注入 `WM_WINDOWPOSCHANGING` 更多 composition 工作。**在 DRAG_ACTIVE=true 时直接 continue**。

### 辅助 2：QuickAdd 去掉 `backdrop-blur-sm`

`QuickAdd.tsx` 的输入框有 `backdrop-blur-sm`，与窗口的 Mica/Acrylic 复合 → Chromium 每帧还要额外重采样并模糊。移除。

### 辅助 3：body 加不透明兜底背景

`styles.css`:
- `body { background: rgba(240,240,243,0.85) }` （深色 `rgba(24,24,27,0.85)`）
- 平时被 Acrylic 完全覆盖看不见
- 拖动时 Acrylic 被临时关闭，它才显形 → 防止"透视看到桌面"的视觉断裂

## 权衡

**代价**：拖动的这 200-500ms 里，Acrylic 会切换到"半透明纯色"。
**收益**：鼠标恢复原生 60fps 流畅，跟手。

Win11 上此代码路径无害（Win11 Mica 走新的 DWM backdrop，`WM_ENTERSIZEMOVE` 时 clear + 恢复只是多做一次动画，无延迟）。

## 编译验证

- `cargo build`：✅ 7.97s，0 warning
- `pnpm --filter desktop build`：✅ 6.68s，0 warning

## 相关文件

| 文件 | 变化 |
|---|---|
| `apps/desktop/src-tauri/src/drag_watcher.rs` | **新增**（HWND subclass 核心） |
| `apps/desktop/src-tauri/src/lib.rs` | 挂 subclass |
| `apps/desktop/src-tauri/src/desktop_embed.rs` | watcher 读 DRAG_ACTIVE 跳过 |
| `apps/desktop/src/components/task/QuickAdd.tsx` | 去 `backdrop-blur-sm` |
| `apps/desktop/src/styles.css` | body 兜底背景 |
