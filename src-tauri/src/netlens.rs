// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/netlens.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Tauri commands for NetLens (packet capture and analysis). The UI
//           pulls everything: a status poll while capturing, and only the
//           visible window of packet rows, so capture rate never floods the
//           webview.
// =============================================================================

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use syspulse_core::netlens::capture::{self, CaptureAccess, CaptureSpec, InterfaceInfo};
use syspulse_core::netlens::dissect::{linktype, Node};
use syspulse_core::netlens::filter::{field_list, FieldInfo};
use syspulse_core::netlens::store::RtpStat;
use syspulse_core::netlens::views::{AppUsage, Conversation, FollowResult, Insight, PacketDetail, Row, Stats};
use syspulse_core::netlens::wireshark::{self, WiresharkTools};
use syspulse_core::netlens::{Limits, NetLens, Status};
use syspulse_core::privileged::{run_one, HelperRequest};
use tauri::State;

use crate::commands::{blocking, CmdResult};

#[derive(Default)]
pub struct NetLensState(pub Arc<NetLens>);

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetLensSetup {
    pub access: CaptureAccess,
    pub tools: WiresharkTools,
    pub interfaces: Vec<InterfaceInfo>,
}

#[tauri::command]
pub async fn netlens_setup() -> CmdResult<NetLensSetup> {
    blocking(|| {
        Ok(NetLensSetup {
            access: capture::access(),
            tools: wireshark::tools(),
            interfaces: capture::interfaces(),
        })
    })
    .await
}

#[tauri::command]
pub async fn netlens_interfaces() -> CmdResult<Vec<InterfaceInfo>> {
    blocking(|| Ok(capture::interfaces())).await
}

#[tauri::command]
pub async fn netlens_grant(grant: bool) -> CmdResult<String> {
    blocking(move || {
        run_one(HelperRequest::SetCaptureAccess { grant })
            .map(|r| r.message)
            .map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn netlens_start(state: State<'_, NetLensState>, spec: CaptureSpec, limits: Limits) -> CmdResult<()> {
    let n = state.0.clone();
    blocking(move || n.start(spec, limits).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_stop(state: State<'_, NetLensState>) -> CmdResult<()> {
    let n = state.0.clone();
    blocking(move || {
        n.stop();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn netlens_clear(state: State<'_, NetLensState>) -> CmdResult<()> {
    let n = state.0.clone();
    blocking(move || n.clear().map_err(err)).await
}

#[tauri::command]
pub fn netlens_status(state: State<'_, NetLensState>) -> Status {
    state.0.status()
}

#[tauri::command]
pub async fn netlens_rows(state: State<'_, NetLensState>, start: usize, count: usize, resolve: bool) -> CmdResult<Vec<Row>> {
    let n = state.0.clone();
    blocking(move || n.with_store(|s| s.rows(start, count, resolve)).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_detail(state: State<'_, NetLensState>, no: u64) -> CmdResult<Option<PacketDetail>> {
    let n = state.0.clone();
    blocking(move || {
        n.with_store(|s| no.checked_sub(1).and_then(|i| s.detail(i as usize)))
            .map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn netlens_set_filter(state: State<'_, NetLensState>, text: String) -> CmdResult<u64> {
    let n = state.0.clone();
    blocking(move || n.set_filter(&text)).await
}

/// Validation while typing: None when the filter is valid.
#[tauri::command]
pub fn netlens_check_filter(text: String) -> Option<String> {
    NetLens::validate_filter(&text).err()
}

#[tauri::command]
pub async fn netlens_check_capture_filter(text: String) -> CmdResult<Option<String>> {
    blocking(move || {
        if text.trim().is_empty() {
            return Ok(None);
        }
        Ok(capture::compile_bpf(text.trim(), linktype::ETHERNET).err().map(|e| e.to_string()))
    })
    .await
}

#[tauri::command]
pub fn netlens_fields() -> Vec<FieldInfo> {
    field_list()
}

#[tauri::command]
pub async fn netlens_conversations(state: State<'_, NetLensState>) -> CmdResult<Vec<Conversation>> {
    let n = state.0.clone();
    blocking(move || n.with_store(|s| s.conversations()).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_apps(state: State<'_, NetLensState>) -> CmdResult<Vec<AppUsage>> {
    let n = state.0.clone();
    blocking(move || n.with_store(|s| s.apps()).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_stats(state: State<'_, NetLensState>) -> CmdResult<Stats> {
    let n = state.0.clone();
    blocking(move || n.with_store(|s| s.stats()).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_insights(state: State<'_, NetLensState>) -> CmdResult<Vec<Insight>> {
    let n = state.0.clone();
    blocking(move || n.with_store(|s| s.insights()).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_rtp(state: State<'_, NetLensState>) -> CmdResult<Vec<RtpStat>> {
    let n = state.0.clone();
    blocking(move || n.with_store(|s| s.rtp_streams()).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_follow(state: State<'_, NetLensState>, stream: u32) -> CmdResult<Option<FollowResult>> {
    let n = state.0.clone();
    blocking(move || n.with_store(|s| s.follow(stream)).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_locate(state: State<'_, NetLensState>, no: u64) -> CmdResult<Option<usize>> {
    let n = state.0.clone();
    blocking(move || Ok(n.locate(no))).await
}

#[tauri::command]
pub async fn netlens_open(state: State<'_, NetLensState>, path: String) -> CmdResult<()> {
    let n = state.0.clone();
    blocking(move || n.open_file(&PathBuf::from(path)).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_save(state: State<'_, NetLensState>, path: String, displayed_only: bool) -> CmdResult<u64> {
    let n = state.0.clone();
    blocking(move || {
        let mut p = PathBuf::from(path);
        if p.extension().is_none() {
            p.set_extension("pcapng");
        }
        n.save(&p, displayed_only).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn netlens_wireshark_decode(state: State<'_, NetLensState>, no: u64) -> CmdResult<Vec<Node>> {
    let n = state.0.clone();
    blocking(move || n.wireshark_decode(no).map_err(err)).await
}

#[tauri::command]
pub async fn netlens_open_wireshark(state: State<'_, NetLensState>, displayed_only: bool) -> CmdResult<String> {
    let n = state.0.clone();
    blocking(move || n.open_in_wireshark(displayed_only).map_err(err)).await
}

/// Stop capturing when the app exits.
pub fn shutdown(state: &NetLensState) {
    state.0.stop();
}
