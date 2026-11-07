# Win11 挂件窗口材质：默认半透明，实时模糊为可选增强

日期：2026-10-29
状态：已实施（v0.0.8）

## 背景

挂件（widget）形态下窗口 **永不激活**（置底 + `HWND_BOTTOM` + 前台变化监听重压底），
而 Tauri/WebView2 窗口的 DWM SystemBackdrop **失焦时渲染为纯黑**
（上游 bug，tauri-apps/window-vibrancy#183 同款）。所以：

- `DWMWA_SYSTEMBACKDROP_TYPE(38)` 的 mica / acrylic / mica_alt 都不能直接用；
- 只能自己在窗口内部画背景。

两条内部绘制路线：

| 路线 | 实现 | 代价 | 风险 |
| --- | --- | --- | --- |
| **A 半透明** | CSS `rgb(var(--win-rgb-*) / var(--win-alpha))`，窗口 `transparent: true` | 0 | 无 |
| **B 实时模糊** | Rust 读壁纸 → 缩放 → 高斯模糊 → PNG base64 → CSS 背景图 | 每档一次 CPU 模糊（~720px 宽），换壁纸时重算 | 壁纸为纯色/幻灯片无文件时失效 → 自动降级 A |

## 决定

**A 为默认档（translucent），B 保留为可选项。** 理由：
1. A 在任何机器、任何壁纸（含纯色、幻灯片、HEIC/动态壁纸）下都成立，零依赖；
2. B 依赖 `SystemParametersInfoW(SPI_GETDESKWALLPAPER)` 能拿到可读图片文件，
   失败场景不可穷举 —— 这类降级不能作为默认路径。

## 实现要点

- `styles.css`
  - 窗口底色抽成变量：`--win-rgb-light: 240 240 243` / `--win-rgb-dark: 24 24 27`，
    `--win-alpha`（默认 0.82）。用现代 `rgb(R G B / A)` 语法，避免 rgba 里没法拼变量。
  - `[data-backdrop="translucent"] #root` 单图层；
  - 其余档位 `[data-backdrop] #root` 叠薄底色 + `var(--wp-mica)` 模糊图（`cover` 铺满）。
- `system.ts`
  - `Backdrop = "translucent" | "acrylic" | "mica" | "mica_alt" | "none"`，默认 `translucent`。
  - **偏好迁移**：`r-todo/backdrop-chosen` 标记用户是否显式点过按钮。
    没点过（老版本的默认值是程序写死的）→ 跟随当前版本的默认档；
    点过 → 尊重用户选择。避免老版本写死的默认值挡住新版默认。
  - `setWindowOpacity(alpha)` 写 `--win-alpha` + localStorage `r-todo/opacity`（0.2~1.0）。
  - B 档位取失败时自动把 `data-backdrop` 改成 `translucent`，用户无感知。
- 设置面板：材质五档按钮（三列网格）+ **不透明度滑杆**（所有档位共用）。
- 实时性：Rust 侧 `WM_SETTINGCHANGE` → `emit("wallpaper-changed")` → 前端重算，
  只有 B 档位需要；A 档位不受壁纸变化影响。

## 什么时候能切回真系统材质

等 WebView2 / Tauri 修掉失焦黑屏后：
1. `window_effect.rs` 的 `set_window_backdrop` 已在 lib.rs 注册，命令名无需改动；
2. `system.ts:refreshWindowVisual()` 里把 `safeInvoke("set_window_backdrop", { backdrop: "none" })`
   改成传当前 kind，并跳过 `--wp-mica` 逻辑即可。
