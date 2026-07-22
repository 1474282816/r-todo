//! r-todo 桌面端 Tauri 入口

mod db;
mod desktop_embed;
mod month_window;
mod notifier;
mod snap;

#[cfg(target_os = "windows")]
mod rounded;

use tauri::Manager;

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
        .plugin(tauri_plugin_window_state::Builder::default().build())
        // SQLite + tasks 表 migration
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:r-todo.db", db::migrations())
                .build(),
        );

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ));
    }

    builder
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window missing");

            // 不用 Acrylic/Mica —— Win10 上 DWM 卡顿代价太大（见 backlog OPT-001）。
            // 改为纯 CSS 半透明背景，跨版本一致、拖动丝滑。
            //
            // Win11 圆角仅应用到主窗口（透明无边框）；月视图有系统边框，
            // 强加 DWM 圆角会露出四个白色圆角尖。
            #[cfg(target_os = "windows")]
            {
                rounded::apply_rounded_corners(&window);
            }

            let _ = window;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_embed::embed_to_desktop,
            desktop_embed::unembed_from_desktop,
            notifier::schedule_reminder,
            notifier::cancel_reminder,
            snap::get_snap_target,
            snap::get_collapse_target,
            month_window::open_month_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
