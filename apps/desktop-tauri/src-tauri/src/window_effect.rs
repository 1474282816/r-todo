//! Win11 窗口材质（DWM SystemBackdrop）
//!
//! 直接调 `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)`，四档：
//!   DWMSBT_NONE(1)          无材质
//!   DWMSBT_MAINWINDOW(2)    Mica（跟随系统深浅色和壁纸取色）
//!   DWMSBT_TRANSIENTWINDOW(3) Acrylic（实时毛玻璃，浮窗级）
//!   DWMSBT_TABBEDWINDOW(4)  Mica Alt（更强壁纸取色，层级底色）
//!
//! Mica 的语义本来就是壁纸取色，不会透出窗口背后的应用内容；需要随窗口
//! 背后内容实时变化时应使用 Acrylic。RTools 默认材质映射到 Acrylic。
//!
//! 不走 tauri 内置 set_effects：它的 Acrylic 用的是 Win10 时代的
//! SetWindowCompositionAttribute(ACCENT_ENABLE_ACRYLICBLURBEHIND)，
//! Win11 上拖动窗口有明显卡顿；SystemBackdrop 是 Win11 官方实现，无此问题。
//!
//! 前提：窗口 `transparent: true`（tauri.conf.json 已配置），且 WebView
//! 的 HTML 背景是透明的（styles.css 里 html/body 已 transparent）。前端只
//! 叠加一层很薄的 tint 保证文字对比度，不能盖住系统合成结果。

#![cfg(target_os = "windows")]

use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW, DWMSBT_NONE,
    DWMSBT_TABBEDWINDOW, DWMSBT_TRANSIENTWINDOW,
};

/// 材质档位（与前端 localStorage `rtools/backdrop` 的值一一对应）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backdrop {
    None,
    Mica,
    MicaAlt,
    Acrylic,
}

impl Backdrop {
    pub fn parse(s: &str) -> Option<Backdrop> {
        match s {
            "none" => Some(Backdrop::None),
            "mica" => Some(Backdrop::Mica),
            "mica_alt" => Some(Backdrop::MicaAlt),
            "acrylic" => Some(Backdrop::Acrylic),
            _ => None,
        }
    }
}

/// 应用材质到窗口。返回 Ok 表示 DWM 接受了该属性。
pub fn apply(hwnd: windows::Win32::Foundation::HWND, backdrop: Backdrop) -> Result<(), String> {
    let value = match backdrop {
        Backdrop::None => DWMSBT_NONE,
        Backdrop::Mica => DWMSBT_MAINWINDOW,
        Backdrop::Acrylic => DWMSBT_TRANSIENTWINDOW,
        Backdrop::MicaAlt => DWMSBT_TABBEDWINDOW,
    };
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &value as *const _ as *const _,
            std::mem::size_of_val(&value) as u32,
        )
        .map_err(|e| format!("DwmSetWindowAttribute(SYSTEMBACKDROP) 失败: {e}"))
    }
}

/// Tauri 命令：设置主窗口材质。`backdrop` 取值 none|mica|mica_alt|acrylic
#[tauri::command]
pub async fn set_window_backdrop(
    window: tauri::Window,
    backdrop: String,
) -> Result<(), String> {
    let b = Backdrop::parse(&backdrop).ok_or_else(|| format!("未知材质: {backdrop}"))?;
    let raw = window.hwnd().map_err(|e| format!("取窗口句柄失败: {e}"))?;
    let hwnd = windows::Win32::Foundation::HWND(raw.0 as *mut _);
    apply(hwnd, b)
}
