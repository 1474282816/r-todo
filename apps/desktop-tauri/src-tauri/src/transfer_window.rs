use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "transfer";

#[tauri::command]
pub async fn open_transfer_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        &app,
        LABEL,
        WebviewUrl::App("index.html?view=transfer".into()),
    )
    .title("互传 · RTools")
    .inner_size(980.0, 720.0)
    .min_inner_size(760.0, 560.0)
    .center()
    .resizable(true)
    .decorations(true)
    .transparent(true)
    .visible(true)
    .build()
    .map_err(|error| format!("创建互传窗口失败: {error}"))?;

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

#[tauri::command]
pub async fn flash_transfer_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(())
}
