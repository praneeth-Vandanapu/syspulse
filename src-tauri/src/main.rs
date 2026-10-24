// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/main.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Application entry point: builds the Tauri app, registers state and
//           commands, and starts the background telemetry loop.
// =============================================================================

// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod forge;
mod launchers;
mod maintenance;
mod netlens;
mod network;
mod recorder;
mod state;
mod tools;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(state::AppState::default())
        .manage(recorder::RecorderState::default())
        .manage(netlens::NetLensState::default())
        .setup(|app| {
            state::load_alert_config(app.handle());
            forge::init(app.handle());
            state::start_telemetry(app.handle().clone());
            if !recorder::ENABLED {
                // Screen recorder hidden in this release: no tray, no hotkeys.
            } else if let Err(e) = recorder::init(app.handle()) {
                log::warn!("recorder tray/hotkeys unavailable: {e}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system_summary,
            commands::hardware_inventory,
            commands::list_processes,
            commands::process_details,
            commands::signal_process,
            commands::set_refresh_interval,
            commands::process_resources,
            commands::set_process_priority,
            maintenance::list_packages,
            maintenance::package_icon,
            maintenance::plan_package_removal,
            maintenance::remove_packages,
            maintenance::cleaner_scan,
            maintenance::cleaner_clean,
            maintenance::large_scan,
            maintenance::large_cancel,
            maintenance::files_trash,
            maintenance::files_delete,
            maintenance::reveal_path,
            tools::search_files,
            tools::search_cancel,
            tools::open_path,
            tools::list_services,
            tools::service_details,
            tools::service_action,
            tools::list_startup,
            tools::set_startup_enabled,
            tools::save_startup,
            tools::remove_startup,
            tools::list_connections,
            tools::get_alert_config,
            tools::set_alert_config,
            tools::export_text,
            network::net_devices,
            network::net_connections,
            network::net_connection_settings,
            network::net_update_connection,
            network::net_activate,
            network::wifi_scan,
            network::wifi_radio,
            network::net_ping,
            network::net_trace,
            network::net_tool_cancel,
            network::net_lookup,
            network::net_ports,
            network::perf_server_start,
            network::perf_server_stop,
            network::perf_server_status,
            network::perf_discover,
            network::perf_run,
            network::perf_cancel,
            forge::forge_ports,
            forge::forge_serial_access,
            forge::forge_grant_serial,
            forge::forge_open,
            forge::forge_close,
            forge::forge_sessions,
            forge::forge_encode,
            forge::forge_send,
            forge::forge_repeat,
            forge::forge_stop_jobs,
            forge::forge_send_file,
            forge::forge_lines,
            forge::forge_break,
            forge::forge_log,
            forge::forge_store_load,
            forge::forge_store_save,
            recorder::rec_sources,
            recorder::rec_audio,
            recorder::rec_defaults,
            recorder::rec_save_defaults,
            recorder::rec_start,
            recorder::rec_toggle_pause,
            recorder::rec_stop,
            recorder::rec_status,
            recorder::rec_list,
            recorder::rec_meter,
            recorder::rec_pick_region,
            recorder::rec_region_snapshot,
            recorder::rec_region_done,
            netlens::netlens_setup,
            netlens::netlens_interfaces,
            netlens::netlens_grant,
            netlens::netlens_start,
            netlens::netlens_stop,
            netlens::netlens_clear,
            netlens::netlens_status,
            netlens::netlens_rows,
            netlens::netlens_detail,
            netlens::netlens_set_filter,
            netlens::netlens_check_filter,
            netlens::netlens_check_capture_filter,
            netlens::netlens_fields,
            netlens::netlens_conversations,
            netlens::netlens_apps,
            netlens::netlens_stats,
            netlens::netlens_insights,
            netlens::netlens_rtp,
            netlens::netlens_follow,
            netlens::netlens_locate,
            netlens::netlens_open,
            netlens::netlens_save,
            netlens::netlens_wireshark_decode,
            netlens::netlens_open_wireshark,
            launchers::launcher_users,
            launchers::launcher_scan,
            launchers::launcher_check,
            launchers::launcher_icon_preview,
            launchers::launcher_create,
            launchers::launcher_list,
            launchers::launcher_remove,
            launchers::launcher_test_run,
        ])
        .build(tauri::generate_context!())
        .expect("error while building SysPulse");
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            // Finalise an active recording so the file stays playable.
            recorder::shutdown(handle);
            if let Some(n) = tauri::Manager::try_state::<netlens::NetLensState>(handle) {
                netlens::shutdown(&n);
            }
        }
    });
}
