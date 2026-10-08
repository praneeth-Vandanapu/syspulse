// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/network.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Tauri commands for network settings (NetworkManager), network
//           tools (ping / trace / DNS / ports) and the speed test (server
//           mode, LAN discovery, test runs with live progress events).
// =============================================================================

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use syspulse_core::netconfig::{self, ConnectionSettings, NetConnection, NetDevice, WifiNetwork};
use syspulse_core::netperf::client::{self, DiscoveredServer, PerfResult};
use syspulse_core::netperf::server::{self, ServerInfo};
use syspulse_core::netperf::TestSpec;
use syspulse_core::nettools::{self, DnsResult, Hop, PingSummary, PortResult};
use tauri::{AppHandle, Emitter, State};

use crate::commands::{blocking, CmdResult};
use crate::state::AppState;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// -------------------------------------------------------------- settings ---

#[tauri::command]
pub async fn net_devices() -> CmdResult<Vec<NetDevice>> {
    blocking(|| netconfig::devices().map_err(err)).await
}

#[tauri::command]
pub async fn net_connections() -> CmdResult<Vec<NetConnection>> {
    blocking(|| netconfig::connections().map_err(err)).await
}

#[tauri::command]
pub async fn net_connection_settings(uuid: String) -> CmdResult<ConnectionSettings> {
    blocking(move || netconfig::connection_settings(&uuid).map_err(err)).await
}

#[tauri::command]
pub async fn net_update_connection(
    settings: ConnectionSettings,
    apply_now: bool,
) -> CmdResult<String> {
    blocking(move || netconfig::update_connection(&settings, apply_now).map_err(err)).await
}

#[tauri::command]
pub async fn net_activate(uuid: String, up: bool) -> CmdResult<()> {
    blocking(move || {
        if up {
            netconfig::activate(&uuid)
        } else {
            netconfig::deactivate(&uuid)
        }
        .map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn wifi_scan(rescan: bool) -> CmdResult<Vec<WifiNetwork>> {
    blocking(move || netconfig::wifi_list(rescan).map_err(err)).await
}

#[tauri::command]
pub async fn wifi_radio(on: Option<bool>) -> CmdResult<bool> {
    blocking(move || {
        if let Some(on) = on {
            netconfig::wifi_radio(on).map_err(err)?;
        }
        Ok(netconfig::wifi_radio_enabled())
    })
    .await
}

// ----------------------------------------------------------------- tools ---

#[tauri::command]
pub async fn net_ping(
    app: AppHandle,
    state: State<'_, AppState>,
    host: String,
    count: u32,
    interval_ms: u32,
) -> CmdResult<PingSummary> {
    let cancel = state.tool_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    blocking(move || {
        nettools::ping(&host, count, interval_ms, &cancel, |l| {
            let _ = app.emit("nettool-line", l);
        })
        .map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn net_trace(
    app: AppHandle,
    state: State<'_, AppState>,
    host: String,
) -> CmdResult<Vec<Hop>> {
    let cancel = state.tool_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    blocking(move || {
        nettools::trace(&host, &cancel, |l| {
            let _ = app.emit("nettool-line", l);
        })
        .map_err(err)
    })
    .await
}

#[tauri::command]
pub fn net_tool_cancel(state: State<'_, AppState>) {
    state.tool_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub async fn net_lookup(host: String) -> CmdResult<DnsResult> {
    blocking(move || nettools::lookup(&host).map_err(err)).await
}

#[tauri::command]
pub async fn net_ports(
    host: String,
    ports: Vec<u16>,
    timeout_ms: u64,
) -> CmdResult<Vec<PortResult>> {
    blocking(move || nettools::check_ports(&host, &ports, timeout_ms).map_err(err)).await
}

// ------------------------------------------------------------ speed test ---

#[tauri::command]
pub fn perf_server_start(
    app: AppHandle,
    state: State<'_, AppState>,
    port: u16,
    code: Option<String>,
) -> CmdResult<ServerInfo> {
    let mut slot = state.perf_server.lock().map_err(err)?;
    if let Some(s) = slot.as_ref() {
        return Ok(s.info.clone());
    }
    let sink: server::EventSink = Arc::new(move |e| {
        let _ = app.emit("perf-server-event", &e);
    });
    let srv = server::start(port, code, sink).map_err(err)?;
    let info = srv.info.clone();
    *slot = Some(srv);
    Ok(info)
}

#[tauri::command]
pub async fn perf_server_stop(state: State<'_, AppState>) -> CmdResult<()> {
    let srv = state.perf_server.lock().map_err(err)?.take();
    if let Some(s) = srv {
        blocking(move || {
            s.stop();
            Ok(())
        })
        .await?;
    }
    Ok(())
}

#[tauri::command]
pub fn perf_server_status(state: State<'_, AppState>) -> Option<ServerInfo> {
    state
        .perf_server
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|s| s.info.clone()))
}

#[tauri::command]
pub async fn perf_discover(port: u16) -> CmdResult<Vec<DiscoveredServer>> {
    blocking(move || Ok(client::discover(port, Duration::from_millis(1500)))).await
}

#[tauri::command]
pub async fn perf_run(
    app: AppHandle,
    state: State<'_, AppState>,
    target: String,
    port: u16,
    code: Option<String>,
    spec: TestSpec,
) -> CmdResult<PerfResult> {
    let cancel = state.perf_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    blocking(move || {
        client::run_test(&target, port, code, &spec, &cancel, |p| {
            let _ = app.emit("perf-progress", &p);
        })
        .map_err(err)
    })
    .await
}

#[tauri::command]
pub fn perf_cancel(state: State<'_, AppState>) {
    state.perf_cancel.store(true, Ordering::SeqCst);
}
