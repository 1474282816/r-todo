//! r-todo 桌面端 Tauri 入口

mod db;
mod desktop_embed;
mod mcp_config;
mod mcp_server;
mod mcp_tools;
mod month_window;
mod notifier;
mod snap;

#[cfg(target_os = "windows")]
mod rounded;

#[cfg(target_os = "windows")]
mod wallpaper_mica;

#[cfg(target_os = "windows")]
mod window_effect;

use tauri::Manager;
use tauri::tray::{TrayIconBuilder, MouseButton, MouseButtonState, TrayIconEvent};
use tauri::menu::{MenuBuilder, MenuItemBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        // 单实例：开机自启 + 手动启动时聚焦已有窗口
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
        // 只恢复几何状态（位置/尺寸/最大化），**不恢复 visible**。
        // 原因：托盘"隐藏窗口"后退出，window-state 会把 visible:false 落盘，
        // 下次启动恢复成隐藏 → 用户双击图标"什么都没出现"（实测踩过）。
        // 可见性由 setup 里的显式 show() 决定。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        & !tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        // SQLite + tasks 表 migration
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:r-todo.db", db::migrations())
                .build(),
        );

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                None,
            ))
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_process::init());
    }

    builder
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window missing");

            // 材质（Mica/Acrylic/Mica Alt）由前端按用户偏好调用
            // set_window_backdrop 设置（走 Win11 SystemBackdrop，见 window_effect.rs）。
            // 此处只处理 Win11 圆角（仅主窗口；月视图有系统边框，强加圆角会露白尖）。
            #[cfg(target_os = "windows")]
            {
                rounded::apply_rounded_corners(&window);
            }

            // 启动必显：window-state 只恢复几何状态，可见性在这里强制打开，
            // 避免"上次隐藏 → 这次启动看不见窗口"。
            // 另外兜底：若上次位置落在屏幕外（拔过显示器/改过分辨率），拉回居中。
            if let Ok(pos) = window.outer_position() {
                if pos.x < -10_000 || pos.y < -10_000 {
                    let _ = window.center();
                }
            }
            let _ = window.show();
            let _ = window.unminimize();

            // ── 系统托盘 ──
            let show_item = MenuItemBuilder::with_id("show", "显示主窗口").build(app)?;
            let hide_item = MenuItemBuilder::with_id("hide", "隐藏窗口").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;

            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&hide_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("r-todo")
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
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
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            app.manage(mcp_server::McpState::default());
            if let Err(e) = mcp_server::start_mcp(app.handle()) {
                eprintln!("[mcp] start failed: {e}");
            }

            let _ = window;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_embed::embed_to_desktop,
            desktop_embed::unembed_from_desktop,
            #[cfg(target_os = "windows")]
            window_effect::set_window_backdrop,
            #[cfg(target_os = "windows")]
            wallpaper_mica::get_wallpaper_mica,
            notifier::schedule_reminder,
            notifier::cancel_reminder,
            snap::get_snap_target,
            snap::get_collapse_target,
            month_window::open_month_window,
            mcp_config::get_mcp_status,
            mcp_config::get_mcp_client_templates,
            mcp_server::set_mcp_enabled,
            mcp_server::rotate_mcp_token,
            mcp_server::get_mcp_running,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
