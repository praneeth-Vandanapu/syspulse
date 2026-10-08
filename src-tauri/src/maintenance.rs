// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/maintenance.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Tauri commands for Phase 2 maintenance tools: software manager,
//           cache cleaner and large-file finder. Root-only work is routed
//           through the privileged helper (pkexec).
// =============================================================================

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use syspulse_core::cleaner::{self, CleanReport, CleanTarget};
use syspulse_core::largefiles::{self, FileActionReport, ScanOptions, ScanResult};
use syspulse_core::privileged::{run_one, HelperRequest};
use syspulse_core::software::{self, icons, Package, PackageSource, RemovalPlan};
use tauri::{AppHandle, Emitter, State};

use crate::commands::{blocking, CmdResult};
use crate::state::AppState;

fn home() -> CmdResult<PathBuf> {
    syspulse_core::fsutil::home_dir().ok_or_else(|| "cannot determine home directory".to_string())
}

// -------------------------------------------------------------- software ---

#[tauri::command]
pub async fn list_packages() -> CmdResult<Vec<Package>> {
    blocking(|| Ok(software::list_packages())).await
}

#[tauri::command]
pub async fn package_icon(icon: String) -> CmdResult<Option<String>> {
    blocking(move || Ok(icons::icon_data_url(&icon))).await
}

#[tauri::command]
pub async fn plan_package_removal(
    source: PackageSource,
    names: Vec<String>,
) -> CmdResult<RemovalPlan> {
    blocking(move || software::plan_removal(source, &names).map_err(|e| e.to_string())).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalOutcome {
    pub message: String,
    pub output: String,
}

#[tauri::command]
pub async fn remove_packages(
    source: PackageSource,
    names: Vec<String>,
    purge: bool,
) -> CmdResult<RemovalOutcome> {
    blocking(move || {
        let plan = software::plan_removal(source, &names).map_err(|e| e.to_string())?;
        if !plan.blockers.is_empty() {
            return Err(plan.blockers.join("; "));
        }
        if source == PackageSource::Flatpak && !plan.needs_admin {
            let output = software::remove_user_flatpaks(&names).map_err(|e| e.to_string())?;
            return Ok(RemovalOutcome {
                message: format!("Removed {}", names.join(", ")),
                output,
            });
        }
        let r = run_one(HelperRequest::RemovePackages {
            source,
            names: names.clone(),
            purge,
        })
        .map_err(|e| e.to_string())?;
        Ok(RemovalOutcome {
            message: format!("Removed {}", names.join(", ")),
            output: r.output,
        })
    })
    .await
}

// --------------------------------------------------------------- cleaner ---

#[tauri::command]
pub async fn cleaner_scan() -> CmdResult<Vec<CleanTarget>> {
    blocking(|| Ok(cleaner::scan(&home()?))).await
}

#[tauri::command]
pub async fn cleaner_clean(ids: Vec<String>) -> CmdResult<CleanReport> {
    blocking(move || {
        let (system, user): (Vec<String>, Vec<String>) = ids
            .into_iter()
            .partition(|id| cleaner::SYSTEM_TARGET_IDS.contains(&id.as_str()));
        let mut report = cleaner::clean_user(&user, &home()?);
        if !system.is_empty() {
            match run_one(HelperRequest::CleanSystem {
                targets: system.clone(),
            }) {
                Ok(r) => {
                    report.freed_bytes += r.freed_bytes;
                    report.cleaned.extend(system);
                }
                Err(e) => report.errors.push(format!("System items: {e}")),
            }
        }
        Ok(report)
    })
    .await
}

// ----------------------------------------------------------- large files ---

#[tauri::command]
pub async fn large_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    options: ScanOptions,
) -> CmdResult<ScanResult> {
    let cancel = state.scan_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    blocking(move || {
        largefiles::scan(&options, &cancel, |p| {
            let _ = app.emit("largefiles-progress", p);
        })
        .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn large_cancel(state: State<'_, AppState>) {
    state.scan_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub async fn files_trash(paths: Vec<String>) -> CmdResult<FileActionReport> {
    blocking(move || Ok(largefiles::trash_files(&paths))).await
}

#[tauri::command]
pub async fn files_delete(paths: Vec<String>, as_admin: bool) -> CmdResult<FileActionReport> {
    blocking(move || {
        if !as_admin {
            return Ok(largefiles::delete_files(&paths));
        }
        let r = run_one(HelperRequest::DeleteFiles {
            paths: paths.clone(),
        })
        .map_err(|e| e.to_string())?;
        Ok(FileActionReport {
            done: paths,
            freed_bytes: r.freed_bytes,
            ..Default::default()
        })
    })
    .await
}

#[tauri::command]
pub async fn reveal_path(path: String) -> CmdResult<()> {
    blocking(move || tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| e.to_string()))
        .await
}
