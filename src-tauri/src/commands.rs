// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/commands.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Tauri command handlers. Thin wrappers around syspulse-core; any
//           blocking work runs on a worker thread so the UI never stalls.
// =============================================================================

use std::sync::atomic::Ordering;

use syspulse_core::models::{
    HwSection, KillOutcome, ProcessDetails, ProcessInfo, Signal, SystemSummary,
};
use syspulse_core::process::ProcessResources;
use syspulse_core::{hardware, monitor, process};
use tauri::State;

use crate::state::{AppState, MAX_INTERVAL_MS, MIN_INTERVAL_MS};

pub type CmdResult<T> = Result<T, String>;

pub async fn blocking<T, F>(f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CmdResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn system_summary() -> CmdResult<SystemSummary> {
    blocking(|| Ok(monitor::system_summary())).await
}

#[tauri::command]
pub async fn hardware_inventory(
    state: State<'_, AppState>,
    refresh: bool,
) -> CmdResult<Vec<HwSection>> {
    let cache = state.hardware.clone();
    let gpus = state.gpus.clone();
    blocking(move || {
        let mut cache = cache.lock().map_err(|e| e.to_string())?;
        if refresh || cache.is_none() {
            let gpus = gpus.lock().map(|g| g.clone()).unwrap_or_default();
            *cache = Some(hardware::collect(&gpus));
        }
        Ok(cache.clone().unwrap_or_default())
    })
    .await
}

#[tauri::command]
pub async fn list_processes(state: State<'_, AppState>) -> CmdResult<Vec<ProcessInfo>> {
    let pm = state.processes.clone();
    blocking(move || Ok(pm.lock().map_err(|e| e.to_string())?.list())).await
}

#[tauri::command]
pub async fn process_details(state: State<'_, AppState>, pid: u32) -> CmdResult<ProcessDetails> {
    let pm = state.processes.clone();
    blocking(move || {
        pm.lock()
            .map_err(|e| e.to_string())?
            .details(pid)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn signal_process(pid: u32, signal: Signal) -> CmdResult<KillOutcome> {
    // May block on the pkexec password dialog.
    blocking(move || process::send_signal(pid, signal).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub fn set_refresh_interval(state: State<'_, AppState>, ms: u64) {
    state.interval_ms.store(
        ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS),
        Ordering::Relaxed,
    );
}

#[tauri::command]
pub async fn process_resources(pid: u32) -> CmdResult<ProcessResources> {
    blocking(move || process::resources(pid).map_err(|e| e.to_string())).await
}

/// Returns true when admin rights were needed.
#[tauri::command]
pub async fn set_process_priority(pid: u32, nice: i32) -> CmdResult<bool> {
    blocking(move || process::set_priority(pid, nice).map_err(|e| e.to_string())).await
}
