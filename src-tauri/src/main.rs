// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod core;
mod models;
mod platform;

fn main() {
    // Initialize tracing with debug level for process name resolution
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
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
            commands::settings::check_platform_permissions,
            #[cfg(target_os = "macos")]
            commands::settings::get_macos_diagnostics,
            #[cfg(target_os = "macos")]
            commands::settings::get_interface_changes,
        ])
        .run(tauri::generate_context!())
        .map_err(|e| {
            eprintln!("Tauri application error: {}", e);
            std::process::ExitCode::from(1)
        })
        .expect("Failed to run Tauri application");
}
