//! 挂件材质：壁纸高斯模糊模拟 Mica
//!
//! 为什么不用 DWM SystemBackdrop（真 Mica/Acrylic）：
//! Tauri/WebView2 窗口失焦时 SystemBackdrop 渲染为纯黑（上游已知问题，
//! tauri-apps/window-vibrancy#183 同款）。挂件（置底、不激活）永远失焦，
//! 黑屏不可接受。
//!
//! Mica 的官方语义本来就是"壁纸快照 + 取色"（Microsoft Learn: Mica only
//! captures the background wallpaper once），所以用壁纸模糊模拟在视觉和
//! 语义上都等价，且不受激活状态影响。
//!
//! 实时性：
//! - 壁纸变化：desktop_embed 的 widget_wndproc 收到 WM_SETTINGCHANGE 时
//!   emit("wallpaper-changed")，前端重新调用本命令 —— 无需重启。
//! - 模糊图与窗口位置无关（cover 铺满窗口），窗口移动无需更新。

#![cfg(target_os = "windows")]

use base64::Engine as _;
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// 取当前壁纸绝对路径。幻灯片模式下返回当前正在显示的那张。
fn get_wallpaper_path() -> Result<String, String> {
    // MAX_PATH 可能不够（某些主题用长路径），给 1024
    let mut buf = [0u16; 1024];
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr() as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
    };
    if !ok {
        return Err("SystemParametersInfoW(SPI_GETDESKWALLPAPER) 失败".into());
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let path = String::from_utf16_lossy(&buf[..len]);
    if path.is_empty() {
        return Err("壁纸路径为空（纯色背景？）".into());
    }
    Ok(path)
}

/// Tauri 命令：返回壁纸模糊图的 data URI（PNG base64）。
/// `sigma` 控制模糊强度，由前端按材质档位传入（acrylic=32 / mica_alt=20 / mica=12）。
#[tauri::command]
pub async fn get_wallpaper_mica(sigma: Option<f32>) -> Result<String, String> {
    // 图像解码/模糊是 CPU 密集操作，丢到阻塞线程池
    let sigma = sigma.unwrap_or(24.0).clamp(2.0, 80.0);
    tauri::async_runtime::spawn_blocking(move || render_blurred_wallpaper(sigma))
        .await
        .map_err(|e| format!("join 失败: {e}"))?
}

fn render_blurred_wallpaper(sigma: f32) -> Result<String, String> {
    let path = get_wallpaper_path()?;

    // 目标宽 720px：380 逻辑宽窗口的 cover 铺法下足够清晰，模糊计算量小
    const TARGET_W: u32 = 720;

    let img = image::open(&path).map_err(|e| format!("壁纸解码失败 {path}: {e}"))?;
    let scale = TARGET_W as f32 / img.width().max(1) as f32;
    let small = if scale < 1.0 {
        img.resize(TARGET_W, u32::MAX, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let blurred = small.blur(sigma);

    let mut png = Vec::with_capacity(512 * 1024);
    let mut writer = std::io::Cursor::new(&mut png);
    blurred
        .to_rgb8()
        .write_to(&mut writer, image::ImageFormat::Png)
        .map_err(|e| format!("PNG 编码失败: {e}"))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(format!("data:image/png;base64,{b64}"))
}
