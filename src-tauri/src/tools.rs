// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/tools.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Tauri commands for Phase 3: file search, services, startup apps,
//           network connections, alert settings and file export.
// =============================================================================

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use syspulse_core::alerts::AlertConfig;
use syspulse_core::netconn::{self, Socket};
use syspulse_core::privileged::{run_one, HelperRequest};
use syspulse_core::search::{self, SearchOptions, SearchSummary};
use syspulse_core::services::{self, Service, ServiceAction, ServiceDetails};
use syspulse_core::startup::{self, EntryInput, StartupEntry};
use tauri::{AppHandle, Emitter, State};

use crate::commands::{blocking, CmdResult};
use crate::state::{save_alert_config, AppState};

fn home() -> CmdResult<PathBuf> {
    syspulse_core::fsutil::home_dir().ok_or_else(|| "cannot determine home directory".to_string())
}

// ---------------------------------------------------------------- search ---

#[tauri::command]
pub async fn search_files(
    app: AppHandle,
    state: State<'_, AppState>,
    options: SearchOptions,
) -> CmdResult<SearchSummary> {
    let cancel = state.search_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    blocking(move || {
        search::search(&options, &cancel, |p| {
            let _ = app.emit("search-progress", p);
        })
        .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn search_cancel(state: State<'_, AppState>) {
    state.search_cancel.store(true, Ordering::SeqCst);
}

/// Open a file or folder with the default application.
#[tauri::command]
pub async fn open_path(path: String) -> CmdResult<()> {
    if !Path::new(&path).exists() {
        return Err(format!("{path} no longer exists"));
    }
    blocking(move || tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string()))
        .await
}

// -------------------------------------------------------------- services ---

#[tauri::command]
pub async fn list_services(user: bool) -> CmdResult<Vec<Service>> {
    blocking(move || Ok(services::list(user))).await
}

#[tauri::command]
pub async fn service_details(unit: String, user: bool) -> CmdResult<ServiceDetails> {
    blocking(move || services::details(&unit, user).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn service_action(unit: String, action: ServiceAction, user: bool) -> CmdResult<String> {
    blocking(move || {
        if user {
            return services::apply(&unit, action, true).map_err(|e| e.to_string());
        }
        if !services::valid_unit(&unit) {
            return Err(format!("invalid unit {unit}"));
        }
        run_one(HelperRequest::Service { unit, action })
            .map(|r| r.message)
            .map_err(|e| e.to_string())
    })
    .await
}

// --------------------------------------------------------------- startup ---

#[tauri::command]
pub async fn list_startup() -> CmdResult<Vec<StartupEntry>> {
    blocking(|| Ok(startup::list(&home()?))).await
}

#[tauri::command]
pub async fn set_startup_enabled(id: String, enabled: bool) -> CmdResult<()> {
    blocking(move || startup::set_enabled(&home()?, &id, enabled).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn save_startup(entry: EntryInput) -> CmdResult<String> {
    blocking(move || startup::save(&home()?, &entry).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn remove_startup(id: String) -> CmdResult<()> {
    blocking(move || startup::remove(&home()?, &id).map_err(|e| e.to_string())).await
}

// ----------------------------------------------------------- connections ---

#[tauri::command]
pub async fn list_connections() -> CmdResult<Vec<Socket>> {
    blocking(|| Ok(netconn::list())).await
}

// ---------------------------------------------------------------- alerts ---

#[tauri::command]
pub fn get_alert_config(state: State<'_, AppState>) -> AlertConfig {
    state.alerts.lock().map(|c| c.clone()).unwrap_or_default()
}

#[tauri::command]
pub fn set_alert_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AlertConfig,
) -> CmdResult<()> {
    if let Ok(mut c) = state.alerts.lock() {
        *c = config.clone();
    }
    save_alert_config(&app, &config).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- export ---

/// Write a CSV / text export chosen through the save dialog.
#[tauri::command]
pub async fn export_text(path: String, contents: String) -> CmdResult<()> {
    blocking(move || {
        let p = Path::new(&path);
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "csv" | "txt") || !p.is_absolute() {
            return Err("exports must be .csv or .txt files".into());
        }
        std::fs::write(p, contents).map_err(|e| e.to_string())
    })
    .await
}
