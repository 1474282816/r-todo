# OPT-003 · 圆角 vs Acrylic 二选一 (Win10)

**发现于**: M2 视觉打磨阶段
**优先级**: 已关闭（接受现状）
**决策**: **保留 Acrylic 毛玻璃，接受方角**

## 背景

用户希望窗口有圆角。尝试了三种方案，全部在 Win10 上失败：

| 方案 | 结果 |
|---|---|
| CSS `border-radius: 12px` | 只有 `#root` 元素圆，下面的 Acrylic 层仍是方形，看不出效果 |
| Win11 DWM `DWMWA_WINDOW_CORNER_PREFERENCE` | Win10 上静默失败，无效果 |
| Win32 `SetWindowRgn` 硬裁剪 | **禁用 DWM 合成**，Acrylic 变白 + 无边框失效 + 出现经典 Win95 标题栏 |

## 根本原因

Win10 的 DWM 合成器和 `SetWindowRgn` 互斥：一旦调用 `SetWindowRgn`，窗口就退回 GDI 直接绘制，所有 DWM 特性（Acrylic、无边框、投影）全部失效。

Win11 引入了 `DWMWA_WINDOW_CORNER_PREFERENCE`，解决了这个冲突。但我们是 Win10。

## 最终选择

**保留 Acrylic 毛玻璃 + 接受方角**。

理由：
- Acrylic 是产品视觉的核心（对标 Things 3 的质感）
- 方角只是 4 个像素级别的差异
- 用户跨到 Win11 后 DWM 会自动给圆角（`rounded.rs` 已就绪）

## 代码状态

- `apps/desktop/src-tauri/src/rounded.rs` — 保留，仅调 Win11 DWM API（Win10 无效，Win11 生效）
- `apps/desktop/src/styles.css` — 保留 `#root { border-radius: 12px }`，无害
- `Cargo.toml` — 撤掉 `Win32_Graphics_Gdi` feature

## 未来重开的条件

用户主要开发机升级到 Win11，或者用户愿意放弃 Acrylic 换纯半透明色 + CSS 圆角。
