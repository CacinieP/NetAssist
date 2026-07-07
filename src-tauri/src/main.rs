// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The platform-abstraction and model layers intentionally expose APIs that
// aren't always called on every target (e.g. macOS-only diagnostics, or
// gateway resolution kept for future use). Silence dead-code linting at the
// crate level so CI's `cargo clippy -D warnings` stays green without having
// to scatter per-item #[allow(dead_code)] annotations across the codebase.
#![allow(dead_code)]

mod commands;
mod core;
mod models;
mod platform;

use commands::settings::load_settings_from_file;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

fn main() {
    // Initialize tracing with debug level for process name resolution
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .setup(|app| {
            // ---- System tray ----
            // Menu items: toggle window visibility + quit.
            let show_item = MenuItem::with_id(app, "show", "显示/隐藏", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("NetAssist")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // ---- Autostart sync ----
            // Reflect the persisted auto_start setting into the OS on launch,
            // in case the user toggled it from System Settings directly.
            if let Ok(settings) = load_settings_from_file() {
                let autostart_manager = app.autolaunch();
                let currently_enabled = autostart_manager.is_enabled().unwrap_or(false);
                if settings.auto_start && !currently_enabled {
                    let _ = autostart_manager.enable();
                } else if !settings.auto_start && currently_enabled {
                    let _ = autostart_manager.disable();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Minimize-to-tray: if the setting is on, intercept the close and
            // hide the window to the tray instead of quitting.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let minimize_to_tray = load_settings_from_file()
                    .map(|s| s.minimize_to_tray)
                    .unwrap_or(false);
                if minimize_to_tray {
                    // prevent the app from actually closing
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::ip_info::get_ip_info,
            commands::ip_info::get_network_status,
            commands::traffic::get_realtime_traffic,
            commands::traffic::get_app_traffic_ranking,
            commands::traffic_history::get_cumulative_traffic,
            commands::traffic_history::get_traffic_history,
            commands::traffic_history::record_traffic_point,
            commands::traffic_history::get_traffic_alerts,
            commands::traffic_history::update_traffic_alert,
            commands::traffic_history::add_traffic_alert,
            commands::traffic_history::delete_traffic_alert,
            commands::traffic_history::check_traffic_alerts,
            commands::dns::test_dns,
            commands::dns::get_dns_servers,
            commands::network_quality::ping,
            commands::network_quality::test_http_connectivity,
            commands::network_quality::traceroute,
            commands::connections::get_active_connections,
            commands::connections::kill_connection,
            commands::emergency::run_diagnostics,
            commands::emergency::apply_quick_fix,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::reset_settings,
            commands::settings::set_autostart,
            commands::settings::check_platform_permissions,
            #[cfg(target_os = "macos")]
            commands::settings::get_macos_diagnostics,
            #[cfg(target_os = "macos")]
            commands::settings::get_interface_changes,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("Tauri application error: {}", e);
            std::process::exit(1);
        });
}
