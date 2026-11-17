mod desktop_embed;
mod engine;
mod month_window;
mod rounded;
mod snap;
mod system;
mod transfer_window;
mod wallpaper_mica;
mod window_effect;

use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

use engine::EngineBridge;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
                let _ = window.unminimize();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        & !tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .ok_or("主窗口不存在")?;

            #[cfg(target_os = "windows")]
            rounded::apply_rounded_corners(&window);

            if let Ok(position) = window.outer_position() {
                if position.x < -10_000 || position.y < -10_000 {
                    let _ = window.center();
                }
            }
            let _ = window.show();
            let _ = window.unminimize();

            let show_item = MenuItemBuilder::with_id("show", "显示主窗口").build(app)?;
            let hide_item = MenuItemBuilder::with_id("hide", "隐藏窗口").build(app)?;
            let transfer_item = MenuItemBuilder::with_id("transfer", "打开互传").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&hide_item)
                .separator()
                .item(&transfer_item)
                .separator()
                .item(&quit_item)
                .build()?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().ok_or("缺少应用图标")?)
                .menu(&menu)
                .tooltip("RTools")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "hide" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    "transfer" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ =
                                transfer_window::open_transfer_window(app).await;
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            let bridge = EngineBridge::new(app.handle().clone());
            app.manage(bridge);
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let bridge = app_handle.state::<EngineBridge>();
                if let Err(error) = bridge.ensure_started().await {
                    log::error!("start rtools-engine failed: {error}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            engine::engine_call,
            engine::engine_health,
            engine::clear_sync_credentials,
            desktop_embed::embed_to_desktop,
            desktop_embed::unembed_from_desktop,
            #[cfg(target_os = "windows")]
            window_effect::set_window_backdrop,
            #[cfg(target_os = "windows")]
            wallpaper_mica::get_wallpaper_mica,
            snap::get_snap_target,
            snap::get_collapse_target,
            month_window::open_month_window,
            transfer_window::open_transfer_window,
            transfer_window::flash_transfer_window,
            system::read_file_bytes,
            system::copy_file,
            system::file_exists,
            system::show_in_folder,
            system::open_path,
        ])
        .build(tauri::generate_context!())
        .expect("构建 RTools Tauri 应用失败")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                tauri::async_runtime::block_on(async {
                    app.state::<EngineBridge>().shutdown().await;
                });
            }
        });
}
