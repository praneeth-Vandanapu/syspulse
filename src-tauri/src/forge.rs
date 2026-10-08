// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/forge.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Tauri commands for PacketForge (TCP / UDP / serial terminal).
//           Session events are batched every 40 ms into one `forge-events`
//           emit so fast streams cannot flood the webview.
// =============================================================================

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use syspulse_core::forge::encode::{self, SendSpec};
use syspulse_core::forge::serial::{self, SerialAccess, SerialPortInfo};
use syspulse_core::forge::{Command, EventSink, Forge, ForgeEvent, SessionConfig, SessionInfo};
use syspulse_core::privileged::{run_one, HelperRequest};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{blocking, CmdResult};

pub struct ForgeState(pub Arc<Forge>);

const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_STORE: usize = 4 * 1024 * 1024;

/// Create the session manager and the event batcher.
pub fn init(app: &AppHandle) {
    let (tx, rx) = mpsc::channel::<ForgeEvent>();
    let tx = Mutex::new(tx);
    let sink: EventSink = Arc::new(move |e| {
        if let Ok(t) = tx.lock() {
            let _ = t.send(e);
        }
    });
    app.manage(ForgeState(Arc::new(Forge::new(sink))));
    let app = app.clone();
    std::thread::Builder::new()
        .name("forge-events".into())
        .spawn(move || {
            let mut batch = Vec::new();
            loop {
                match rx.recv_timeout(Duration::from_millis(40)) {
                    Ok(e) => {
                        batch.push(e);
                        // Drain whatever else is ready, then flush below on timeout/size.
                        while let Ok(e) = rx.try_recv() {
                            batch.push(e);
                            if batch.len() >= 2000 {
                                break;
                            }
                        }
                        if batch.len() < 2000 {
                            std::thread::sleep(Duration::from_millis(40));
                            while let Ok(e) = rx.try_recv() {
                                batch.push(e);
                                if batch.len() >= 2000 {
                                    break;
                                }
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if !batch.is_empty() {
                    let _ = app.emit("forge-events", &batch);
                    batch.clear();
                }
            }
        })
        .expect("forge event thread");
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn forge_ports() -> CmdResult<Vec<SerialPortInfo>> {
    blocking(|| Ok(serial::list_ports())).await
}

#[tauri::command]
pub fn forge_serial_access() -> SerialAccess {
    serial::access()
}

#[tauri::command]
pub async fn forge_grant_serial() -> CmdResult<String> {
    blocking(|| {
        run_one(HelperRequest::GrantSerialAccess)
            .map(|r| r.message)
            .map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn forge_open(
    state: State<'_, ForgeState>,
    config: SessionConfig,
) -> CmdResult<SessionInfo> {
    let f = state.0.clone();
    blocking(move || f.open(config).map_err(err)).await
}

#[tauri::command]
pub async fn forge_close(state: State<'_, ForgeState>, id: u32) -> CmdResult<()> {
    let f = state.0.clone();
    blocking(move || {
        f.close(id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn forge_sessions(state: State<'_, ForgeState>) -> Vec<SessionInfo> {
    state.0.list()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodePreview {
    pub hex: String,
    pub length: usize,
    pub error: Option<String>,
}

/// Live preview of the bytes a send would produce.
#[tauri::command]
pub fn forge_encode(spec: SendSpec) -> EncodePreview {
    match encode::encode(&spec) {
        Ok(b) => EncodePreview {
            hex: encode::to_hex(&b[..b.len().min(4096)]),
            length: b.len(),
            error: None,
        },
        Err(e) => EncodePreview {
            hex: String::new(),
            length: 0,
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn forge_send(
    state: State<'_, ForgeState>,
    id: u32,
    spec: SendSpec,
    target: Option<String>,
) -> CmdResult<usize> {
    let bytes = encode::encode(&spec).map_err(err)?;
    if bytes.is_empty() {
        return Err("nothing to send".into());
    }
    let n = bytes.len();
    state
        .0
        .command(id, Command::Send { bytes, target })
        .map_err(err)?;
    Ok(n)
}

#[tauri::command]
pub fn forge_repeat(
    state: State<'_, ForgeState>,
    id: u32,
    spec: SendSpec,
    target: Option<String>,
    interval_ms: u64,
    count: Option<u64>,
) -> CmdResult<()> {
    let bytes = encode::encode(&spec).map_err(err)?;
    if bytes.is_empty() {
        return Err("nothing to send".into());
    }
    state
        .0
        .command(
            id,
            Command::Repeat {
                bytes,
                target,
                interval_ms: interval_ms.max(1),
                count,
            },
        )
        .map_err(err)
}

#[tauri::command]
pub fn forge_stop_jobs(state: State<'_, ForgeState>, id: u32) -> CmdResult<()> {
    state.0.command(id, Command::StopJobs).map_err(err)
}

#[tauri::command]
pub async fn forge_send_file(
    state: State<'_, ForgeState>,
    id: u32,
    path: String,
    target: Option<String>,
    chunk: usize,
    delay_ms: u64,
) -> CmdResult<u64> {
    let f = state.0.clone();
    blocking(move || {
        let meta = std::fs::metadata(&path).map_err(err)?;
        if meta.len() > MAX_FILE {
            return Err(format!(
                "file is larger than {} MiB",
                MAX_FILE / 1024 / 1024
            ));
        }
        let data = std::fs::read(&path).map_err(err)?;
        let n = data.len() as u64;
        f.command(
            id,
            Command::SendFile {
                data,
                target,
                chunk,
                delay_ms,
            },
        )
        .map_err(err)?;
        Ok(n)
    })
    .await
}

#[tauri::command]
pub fn forge_lines(
    state: State<'_, ForgeState>,
    id: u32,
    dtr: Option<bool>,
    rts: Option<bool>,
) -> CmdResult<()> {
    state
        .0
        .command(id, Command::SetLines { dtr, rts })
        .map_err(err)
}

#[tauri::command]
pub fn forge_break(state: State<'_, ForgeState>, id: u32, ms: u64) -> CmdResult<()> {
    state.0.command(id, Command::Break { ms }).map_err(err)
}

#[tauri::command]
pub fn forge_log(state: State<'_, ForgeState>, id: u32, path: Option<String>) -> CmdResult<()> {
    if let Some(p) = &path {
        if !Path::new(p).is_absolute() {
            return Err("log path must be absolute".into());
        }
    }
    state.0.command(id, Command::Log { path }).map_err(err)
}

fn store_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("packetforge.json"))
}

/// Macros and saved session profiles (opaque JSON owned by the UI).
#[tauri::command]
pub fn forge_store_load(app: AppHandle) -> serde_json::Value {
    store_path(&app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::json!({}))
}

#[tauri::command]
pub fn forge_store_save(app: AppHandle, data: serde_json::Value) -> CmdResult<()> {
    let text = serde_json::to_string_pretty(&data).map_err(err)?;
    if text.len() > MAX_STORE {
        return Err("PacketForge store is too large".into());
    }
    let p = store_path(&app).ok_or("no config directory")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    std::fs::write(p, text).map_err(err)
}
