//! 月视图窗口：独立窗口，点击主窗口📅按钮打开。
//!
//! 单例：如果已经打开就聚焦；否则动态创建。
//! 独立窗口 = 有任务栏图标 + Alt+Tab 可切换，方便"翻阅"场景。

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "month";

#[tauri::command]
pub async fn open_month_window(app: tauri::AppHandle) -> Result<(), String> {
    // 单例：已存在则聚焦
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }

    // 用 URL query 告诉前端渲染月视图路由
    let url = WebviewUrl::App("index.html?view=month".into());

    let window = WebviewWindowBuilder::new(&app, LABEL, url)
        .title("月视图 · RTools")
        .inner_size(1200.0, 800.0)
        .min_inner_size(800.0, 600.0)
        .center()
        .resizable(true)
        .decorations(true)
        .transparent(true)
        .visible(true)
        .build()
        .map_err(|e| format!("failed to create month window: {e}"))?;

    #[cfg(target_os = "windows")]
    {
        crate::rounded::apply_rounded_corners(&window);
        if let Ok(raw) = window.hwnd() {
            let hwnd = windows::Win32::Foundation::HWND(raw.0 as *mut _);
            let _ = crate::window_effect::apply(
                hwnd,
                crate::window_effect::Backdrop::Acrylic,
            );
        }
    }

    Ok(())
}
