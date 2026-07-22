//! Windows 桌面挂件基础模式
//!
//! `WS_EX_TOOLWINDOW`（不出现在任务栏 / Alt+Tab）+ `HWND_BOTTOM`（置底）。
//! Win+D 会和其他普通窗口一样被隐藏，暂时接受这个妥协。

#![cfg(target_os = "windows")]

use tauri::{Manager, WebviewWindow};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_BOTTOM,
        HWND_NOTOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_APPWINDOW,
        WS_EX_TOOLWINDOW,
    },
};

fn get_hwnd(window: &WebviewWindow) -> Result<HWND, String> {
    let raw = window.hwnd().map_err(|e| format!("cannot get HWND: {e}"))?;
    Ok(HWND(raw.0 as *mut _))
}

fn apply_embedded_style(hwnd: HWND) {
    unsafe {
        // Toolwindow：不出现在任务栏 / Alt+Tab
        let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let mut new = cur;
        new |= WS_EX_TOOLWINDOW.0 as isize;
        new &= !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);

        // 置底
        let _ = SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

fn apply_floating_style(hwnd: HWND) {
    unsafe {
        let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let mut new = cur;
        new &= !(WS_EX_TOOLWINDOW.0 as isize);
        new |= WS_EX_APPWINDOW.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);

        let _ = SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

#[tauri::command]
pub async fn embed_to_desktop(window: tauri::Window) -> Result<(), String> {
    let hwnd = get_hwnd(
        &window
            .get_webview_window(window.label())
            .ok_or_else(|| "webview window missing".to_string())?,
    )?;
    apply_embedded_style(hwnd);
    log::info!("embed mode ON (Toolwindow + HWND_BOTTOM)");
    Ok(())
}

#[tauri::command]
pub async fn unembed_from_desktop(window: tauri::Window) -> Result<(), String> {
    let hwnd = get_hwnd(
        &window
            .get_webview_window(window.label())
            .ok_or_else(|| "webview window missing".to_string())?,
    )?;
    apply_floating_style(hwnd);
    log::info!("embed mode OFF");
    Ok(())
}
