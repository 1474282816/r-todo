//! Windows 圆角窗口
//!
//! 只走 Win11 DWM 路线。SetWindowRgn 兜底方案已废弃——它会禁用 DWM 合成，
//! 导致 Acrylic 材质失效 + 无边框窗口回退到经典标题栏（用户验证过）。
//!
//! Win10 上此函数静默失败，接受"直角 + 完美 Acrylic"这个折中。

#![cfg(target_os = "windows")]

use tauri::WebviewWindow;
use windows::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    },
};

pub fn apply_rounded_corners(window: &WebviewWindow) {
    let Ok(raw) = window.hwnd() else { return };
    let hwnd = HWND(raw.0 as *mut _);
    let pref = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const _,
            std::mem::size_of_val(&pref) as u32,
        );
    }
}
