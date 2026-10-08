// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/state.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Shared application state and the background telemetry thread that
//           pushes one `telemetry` event per tick to the UI and raises
//           threshold alerts (event + desktop notification).
// =============================================================================

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use syspulse_core::alerts::{AlertConfig, AlertEngine};
use syspulse_core::gpu::GpuManager;
use syspulse_core::models::{GpuSample, HwSection};
use syspulse_core::monitor::SystemSampler;
use syspulse_core::process::ProcessManager;
use tauri::{AppHandle, Emitter, Manager};

pub const MIN_INTERVAL_MS: u64 = 250;
pub const MAX_INTERVAL_MS: u64 = 10_000;

pub struct AppState {
    pub interval_ms: Arc<AtomicU64>,
    pub processes: Arc<Mutex<ProcessManager>>,
    /// Latest GPU sample, shared with the hardware inventory.
    pub gpus: Arc<Mutex<Vec<GpuSample>>>,
    pub hardware: Arc<Mutex<Option<Vec<HwSection>>>>,
    /// Cancellation flag for the running large-file scan.
    pub scan_cancel: Arc<AtomicBool>,
    /// Cancellation flag for the running file search.
    pub search_cancel: Arc<AtomicBool>,
    /// Alert thresholds (persisted in the app config directory).
    pub alerts: Arc<Mutex<AlertConfig>>,
    /// Running speed-test server (Server mode), if any.
    pub perf_server: Mutex<Option<syspulse_core::netperf::server::PerfServer>>,
    pub perf_cancel: Arc<AtomicBool>,
    /// Cancels the running ping / trace.
    pub tool_cancel: Arc<AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            interval_ms: Arc::new(AtomicU64::new(1000)),
            processes: Arc::new(Mutex::new(ProcessManager::new())),
            gpus: Arc::new(Mutex::new(Vec::new())),
            hardware: Arc::new(Mutex::new(None)),
            scan_cancel: Arc::new(AtomicBool::new(false)),
            search_cancel: Arc::new(AtomicBool::new(false)),
            alerts: Arc::new(Mutex::new(AlertConfig::default())),
            perf_server: Mutex::new(None),
            perf_cancel: Arc::new(AtomicBool::new(false)),
            tool_cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Spawn the sampling loop. It owns the sampler and GPU manager, so no locks
/// are held while hardware is being read.
fn alert_config_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("alerts.json"))
}

/// Load saved alert thresholds into the shared state (defaults if absent).
pub fn load_alert_config(app: &AppHandle) {
    let Some(path) = alert_config_path(app) else {
        return;
    };
    if let Some(cfg) = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<AlertConfig>(&t).ok())
    {
        if let Ok(mut c) = app.state::<AppState>().alerts.lock() {
            *c = cfg;
        }
    }
}

pub fn save_alert_config(app: &AppHandle, cfg: &AlertConfig) -> std::io::Result<()> {
    let path =
        alert_config_path(app).ok_or_else(|| std::io::Error::other("no config directory"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg).unwrap_or_default())
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("desktop notification failed: {e}");
    }
}

pub fn start_telemetry(app: AppHandle) {
    let state = app.state::<AppState>();
    let interval = state.interval_ms.clone();
    let gpus_shared = state.gpus.clone();
    let alert_cfg = state.alerts.clone();

    std::thread::Builder::new()
        .name("telemetry".into())
        .spawn(move || {
            let mut sampler = SystemSampler::new();
            let mut gpus = GpuManager::new();
            let mut engine = AlertEngine::new(AlertConfig::default());
            // Let CPU counters accumulate once before the first sample.
            std::thread::sleep(Duration::from_millis(300));
            loop {
                let started = Instant::now();
                let mut t = sampler.sample();
                t.gpus = gpus.sample();
                if let Ok(mut g) = gpus_shared.lock() {
                    *g = t.gpus.clone();
                }
                if let Err(e) = app.emit("telemetry", &t) {
                    log::warn!("emit telemetry failed: {e}");
                }
                if let Ok(cfg) = alert_cfg.lock() {
                    if engine.config != *cfg {
                        engine.config = cfg.clone();
                    }
                }
                for alert in engine.check(&t) {
                    log::info!("alert: {} - {}", alert.title, alert.body);
                    notify(&app, &alert.title, &alert.body);
                    let _ = app.emit("alert", &alert);
                }
                let wait = Duration::from_millis(interval.load(Ordering::Relaxed));
                std::thread::sleep(wait.saturating_sub(started.elapsed()));
            }
        })
        .expect("failed to spawn telemetry thread");
}
