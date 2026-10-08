// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/recorder.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Screen-recorder integration: Tauri commands, countdown, hiding
//           the main window while recording, the top-bar (tray / app
//           indicator) menu with Start / Pause / Resume / Stop & save /
//           Cancel, global hotkeys (Ctrl+Alt+R, Ctrl+Alt+P), the region
//           picker window and a 1 s ticker that updates UI and tray.
// =============================================================================

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use syspulse_core::recorder::sources::{self, Sources};
use syspulse_core::recorder::{
    AudioConfig, AudioMeter, CaptureArea, Container, Quality, RecState, RecordConfig, Recorder,
    RecorderEvent, RecorderSink, RecordingFile, RecordingInfo, VideoConfig,
};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

use crate::commands::{blocking, CmdResult};

pub const HOTKEY_TOGGLE: &str = "ctrl+alt+r";
pub const HOTKEY_PAUSE: &str = "ctrl+alt+p";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOptions {
    pub config: RecordConfig,
    #[serde(default = "three")]
    pub countdown: u32,
    #[serde(default = "yes")]
    pub hide_window: bool,
}

fn three() -> u32 {
    3
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// idle | countdown | recording | paused
    pub state: &'static str,
    pub elapsed: f64,
    pub size: u64,
    pub info: Option<RecordingInfo>,
}

struct TrayItems {
    tray: TrayIcon,
    status: MenuItem<tauri::Wry>,
    start: MenuItem<tauri::Wry>,
    pause: MenuItem<tauri::Wry>,
    stop: MenuItem<tauri::Wry>,
    cancel: MenuItem<tauri::Wry>,
}

#[derive(Default)]
pub struct RecorderState {
    rec: Mutex<Option<Recorder>>,
    meter: Mutex<Option<AudioMeter>>,
    defaults: Mutex<Option<StartOptions>>,
    counting: AtomicBool,
    cancel_countdown: AtomicBool,
    hidden_by_us: AtomicBool,
    tray: Mutex<Option<TrayItems>>,
    region_snapshot: Mutex<Option<RegionSnapshot>>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn sink(app: &AppHandle) -> RecorderSink {
    let app = app.clone();
    Arc::new(move |e: RecorderEvent| {
        let _ = app.emit("recorder-event", &e);
    })
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn fmt_elapsed(secs: f64) -> String {
    let s = secs as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

pub fn status(app: &AppHandle) -> Status {
    let st = app.state::<RecorderState>();
    if st.counting.load(Ordering::Relaxed) {
        return Status {
            state: "countdown",
            elapsed: 0.0,
            size: 0,
            info: None,
        };
    }
    let rec = st.rec.lock().unwrap();
    match rec.as_ref() {
        Some(r) => Status {
            state: if r.state() == RecState::Paused {
                "paused"
            } else {
                "recording"
            },
            elapsed: r.elapsed().as_secs_f64(),
            size: r.size(),
            info: Some(r.info.clone()),
        },
        None => Status {
            state: "idle",
            elapsed: 0.0,
            size: 0,
            info: None,
        },
    }
}

/// Reflect the current state in the tray menu, icon and label.
fn refresh_tray(app: &AppHandle) {
    let s = status(app);
    let st = app.state::<RecorderState>();
    let tray = st.tray.lock().unwrap();
    let Some(t) = tray.as_ref() else { return };
    let active = matches!(s.state, "recording" | "paused");
    let _ = t.start.set_enabled(s.state == "idle");
    let _ = t.pause.set_enabled(active);
    let _ = t.pause.set_text(if s.state == "paused" {
        "Resume"
    } else {
        "Pause"
    });
    let _ = t.stop.set_enabled(active);
    let _ = t.cancel.set_enabled(active || s.state == "countdown");
    let label = match s.state {
        "recording" => format!("REC {}", fmt_elapsed(s.elapsed)),
        "paused" => format!("❚❚ {}", fmt_elapsed(s.elapsed)),
        "countdown" => "Starting…".to_string(),
        _ => "Not recording".to_string(),
    };
    let _ = t.status.set_text(&label);
    let _ = t
        .tray
        .set_title(if active { Some(label.as_str()) } else { None });
    let icon = if active {
        record_icon(s.state == "paused")
    } else {
        app.default_window_icon().cloned().map(|i| i.to_owned())
    };
    if let Some(icon) = icon {
        let _ = t.tray.set_icon(Some(icon));
    }
}

/// A 32x32 red dot (recording) or amber pause icon, drawn in code.
fn record_icon(paused: bool) -> Option<Image<'static>> {
    let (w, h) = (32u32, 32u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let d = (dx * dx + dy * dy).sqrt();
            let i = ((y * w + x) * 4) as usize;
            let inside = d <= 13.0;
            let bar =
                paused && ((10..14).contains(&x) || (18..22).contains(&x)) && (9..23).contains(&y);
            if inside {
                let (r, g, b) = if paused {
                    (0xF0, 0xA0, 0x10)
                } else {
                    (0xE5, 0x2B, 0x2B)
                };
                let a = ((13.0 - d).clamp(0.0, 1.0) * 255.0) as u8;
                rgba[i..i + 4].copy_from_slice(&if bar {
                    [255, 255, 255, a]
                } else {
                    [r, g, b, a]
                });
            }
        }
    }
    Some(Image::new_owned(rgba, w, h))
}

pub fn default_options() -> StartOptions {
    let mon = sources::monitors().into_iter().next();
    let area = match mon {
        Some(m) => CaptureArea::Monitor {
            name: m.name,
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
        },
        None => CaptureArea::Region {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
    };
    StartOptions {
        config: RecordConfig {
            area,
            // Follow whatever output is in use when recording starts (speakers, Bluetooth, HDMI...).
            audio: AudioConfig {
                system: Some(sources::FOLLOW_OUTPUT.into()),
                mic: None,
                system_volume: 1.0,
                mic_volume: 1.0,
            },
            video: VideoConfig {
                container: Container::Mp4,
                codec: "h264".into(),
                fps: 30,
                quality: Quality::High,
                encoder: "auto".into(),
                show_pointer: true,
            },
            output_dir: "~/Videos/SysPulse".into(),
        },
        countdown: 3,
        hide_window: true,
    }
}

fn defaults_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("recorder.json"))
}

fn load_defaults(app: &AppHandle) -> StartOptions {
    defaults_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(default_options)
}

/// Countdown -> hide window -> start. Runs on its own thread.
pub fn start_flow(app: &AppHandle, opts: StartOptions) -> Result<(), String> {
    let st = app.state::<RecorderState>();
    if st.rec.lock().unwrap().is_some() || st.counting.swap(true, Ordering::SeqCst) {
        return Err("a recording is already in progress".into());
    }
    st.cancel_countdown.store(false, Ordering::SeqCst);
    *st.defaults.lock().unwrap() = Some(opts.clone());
    // Release the audio meter: the recording opens the same devices.
    st.meter.lock().unwrap().take();
    let app = app.clone();
    std::thread::spawn(move || {
        let st = app.state::<RecorderState>();
        let window_visible = app
            .get_webview_window("main")
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        if opts.countdown > 0 && !window_visible {
            notify(
                &app,
                "Screen recording",
                &format!("Recording starts in {} s", opts.countdown),
            );
        }
        refresh_tray(&app);
        for n in (1..=opts.countdown.min(30)).rev() {
            let _ = app.emit("recorder-countdown", n);
            std::thread::sleep(Duration::from_secs(1));
            if st.cancel_countdown.load(Ordering::SeqCst) {
                st.counting.store(false, Ordering::SeqCst);
                let _ = app.emit("recorder-countdown", 0);
                refresh_tray(&app);
                return;
            }
        }
        let _ = app.emit("recorder-countdown", 0);
        if opts.hide_window && window_visible {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
                st.hidden_by_us.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(400)); // let the compositor repaint
            }
        }
        let result = Recorder::start(&opts.config, sink(&app));
        st.counting.store(false, Ordering::SeqCst);
        match result {
            Ok(r) => {
                *st.rec.lock().unwrap() = Some(r);
            }
            Err(e) => {
                if st.hidden_by_us.swap(false, Ordering::SeqCst) {
                    show_main(&app);
                }
                let _ = app.emit("recorder-error", e.to_string());
                notify(&app, "Recording could not start", &e.to_string());
            }
        }
        let _ = app.emit("recorder-status", status(&app));
        refresh_tray(&app);
    });
    Ok(())
}

pub fn stop_flow(app: &AppHandle, save: bool) -> Result<(), String> {
    let st = app.state::<RecorderState>();
    if st.counting.load(Ordering::SeqCst) {
        st.cancel_countdown.store(true, Ordering::SeqCst);
        return Ok(());
    }
    let Some(rec) = st.rec.lock().unwrap().take() else {
        return Err("not recording".into());
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let st = app.state::<RecorderState>();
        let _ = app.emit(
            "recorder-status",
            Status {
                state: "idle",
                elapsed: 0.0,
                size: 0,
                info: None,
            },
        );
        if save {
            match rec.stop() {
                Ok(res) => {
                    let _ = app.emit("recorder-finished", &res);
                    notify(
                        &app,
                        "Recording saved",
                        &format!(
                            "{} · {}",
                            res.path.rsplit('/').next().unwrap_or(""),
                            fmt_elapsed(res.duration_secs)
                        ),
                    );
                }
                Err(e) => {
                    let _ = app.emit("recorder-error", e.to_string());
                }
            }
        } else {
            let _ = rec.cancel();
            let _ = app.emit("recorder-cancelled", ());
        }
        if st.hidden_by_us.swap(false, Ordering::SeqCst) {
            show_main(&app);
            let _ = app.emit("navigate", "recorder");
        }
        refresh_tray(&app);
    });
    Ok(())
}

pub fn toggle_pause(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<RecorderState>();
    let mut rec = st.rec.lock().unwrap();
    let r = rec.as_mut().ok_or("not recording")?;
    if r.state() == RecState::Paused {
        r.resume()
    } else {
        r.pause()
    }
    .map_err(err)?;
    drop(rec);
    let _ = app.emit("recorder-status", status(app));
    refresh_tray(app);
    Ok(())
}

/// Tray menu, hotkeys and the 1 s ticker. Called once from setup().
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let st = app.state::<RecorderState>();
    *st.defaults.lock().unwrap() = Some(load_defaults(app));

    let status_item = MenuItem::with_id(app, "rec_status", "Not recording", false, None::<&str>)?;
    let start = MenuItem::with_id(
        app,
        "rec_start",
        "Start recording\tCtrl+Alt+R",
        true,
        None::<&str>,
    )?;
    let pause = MenuItem::with_id(app, "rec_pause", "Pause\tCtrl+Alt+P", false, None::<&str>)?;
    let stop = MenuItem::with_id(app, "rec_stop", "Stop and save", false, None::<&str>)?;
    let cancel = MenuItem::with_id(app, "rec_cancel", "Cancel recording", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open SysPulse", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit SysPulse", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &sep1,
            &start,
            &pause,
            &stop,
            &cancel,
            &sep2,
            &open,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("syspulse")
        .menu(&menu)
        .tooltip("SysPulse")
        .on_menu_event(|app, ev| {
            let r = match ev.id().as_ref() {
                "rec_start" => {
                    let opts = app
                        .state::<RecorderState>()
                        .defaults
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(default_options);
                    start_flow(app, opts)
                }
                "rec_pause" => toggle_pause(app),
                "rec_stop" => stop_flow(app, true),
                "rec_cancel" => stop_flow(app, false),
                "open" => {
                    show_main(app);
                    Ok(())
                }
                "quit" => {
                    app.exit(0);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(e) = r {
                notify(app, "SysPulse", &e);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    *st.tray.lock().unwrap() = Some(TrayItems {
        tray,
        status: status_item,
        start,
        pause,
        stop,
        cancel,
    });

    // Global hotkeys (failures are logged, e.g. when another app owns them).
    {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
        let gs = app.global_shortcut();
        for (key, action) in [(HOTKEY_TOGGLE, "toggle"), (HOTKEY_PAUSE, "pause")] {
            let r = gs.on_shortcut(key, move |app, _sc, ev| {
                if ev.state() != ShortcutState::Pressed {
                    return;
                }
                let st = app.state::<RecorderState>();
                let busy = st.rec.lock().unwrap().is_some() || st.counting.load(Ordering::SeqCst);
                let res = match (action, busy) {
                    ("toggle", true) => stop_flow(app, true),
                    ("toggle", false) => {
                        let opts = st
                            .defaults
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(default_options);
                        start_flow(app, opts)
                    }
                    _ => toggle_pause(app),
                };
                if let Err(e) = res {
                    log::info!("hotkey: {e}");
                }
            });
            if let Err(e) = r {
                log::warn!("could not register global shortcut {key}: {e}");
            }
        }
    }

    // Ticker: elapsed time to UI and tray, failure detection.
    let app2 = app.clone();
    std::thread::Builder::new()
        .name("rec-ticker".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            let st = app2.state::<RecorderState>();
            let failed = st.rec.lock().unwrap().as_ref().and_then(|r| r.failed());
            if let Some(msg) = failed {
                let _ = stop_flow(&app2, true);
                notify(&app2, "Recording stopped", &msg);
                continue;
            }
            let s = status(&app2);
            if s.state != "idle" {
                let _ = app2.emit("recorder-status", &s);
                refresh_tray(&app2);
            }
        })?;
    Ok(())
}

/// Finish a running recording before the app exits.
pub fn shutdown(app: &AppHandle) {
    let st = app.state::<RecorderState>();
    if let Some(r) = st.rec.lock().unwrap().take() {
        let _ = r.stop();
    }
    st.meter.lock().unwrap().take();
}

// ---------------------------------------------------------------- commands ---

#[tauri::command]
pub async fn rec_sources() -> CmdResult<Sources> {
    blocking(|| Ok(sources::sources())).await
}

/// Audio devices and what "follow current output/input" means right now (cheap; polled by the UI).
#[tauri::command]
pub async fn rec_audio() -> CmdResult<sources::AudioState> {
    blocking(|| Ok(sources::audio_state())).await
}

#[tauri::command]
pub fn rec_defaults(state: State<'_, RecorderState>) -> StartOptions {
    state
        .defaults
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(default_options)
}

/// Remember the settings used by the tray menu and hotkeys.
#[tauri::command]
pub fn rec_save_defaults(
    app: AppHandle,
    state: State<'_, RecorderState>,
    options: StartOptions,
) -> CmdResult<()> {
    *state.defaults.lock().unwrap() = Some(options.clone());
    let p = defaults_path(&app).ok_or("no config directory")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(&options).map_err(err)?).map_err(err)
}

#[tauri::command]
pub fn rec_start(app: AppHandle, options: StartOptions) -> CmdResult<()> {
    start_flow(&app, options)
}

#[tauri::command]
pub fn rec_toggle_pause(app: AppHandle) -> CmdResult<()> {
    toggle_pause(&app)
}

#[tauri::command]
pub fn rec_stop(app: AppHandle, save: bool) -> CmdResult<()> {
    stop_flow(&app, save)
}

#[tauri::command]
pub fn rec_status(app: AppHandle) -> Status {
    status(&app)
}

#[tauri::command]
pub async fn rec_list(dir: String) -> CmdResult<Vec<RecordingFile>> {
    blocking(move || Ok(syspulse_core::recorder::list_recordings(&dir))).await
}

/// Live audio level preview for the setup screen (None/None stops it).
#[tauri::command]
pub fn rec_meter(
    app: AppHandle,
    state: State<'_, RecorderState>,
    system: Option<String>,
    mic: Option<String>,
) -> CmdResult<()> {
    let mut m = state.meter.lock().unwrap();
    m.take();
    if state.rec.lock().unwrap().is_some() {
        return Ok(()); // the recording itself reports levels
    }
    *m = AudioMeter::start(system.as_deref(), mic.as_deref(), sink(&app)).map_err(err)?;
    Ok(())
}

/// Open the full-screen region picker on a monitor (main window hidden).
#[tauri::command]
pub async fn rec_pick_region(
    app: AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> CmdResult<()> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    std::thread::sleep(Duration::from_millis(350));
    let snap = blocking(move || sources::snapshot(x, y, width, height).map_err(err)).await;
    let snap = match snap {
        Ok(s) => s,
        Err(e) => {
            show_main(&app);
            return Err(e);
        }
    };
    *app.state::<RecorderState>().region_snapshot.lock().unwrap() = Some(RegionSnapshot {
        image: snap,
        x,
        y,
        width,
        height,
    });
    if let Some(old) = app.get_webview_window("region") {
        let _ = old.close();
    }
    let built =
        WebviewWindowBuilder::new(&app, "region", WebviewUrl::App("index.html#region".into()))
            .title("Select region")
            .position(x as f64, y as f64)
            .inner_size(width as f64, height as f64)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(true)
            .build();
    match built {
        Ok(w) => {
            let _ = w.set_fullscreen(true);
            Ok(())
        }
        Err(e) => {
            show_main(&app);
            Err(err(e))
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegionSnapshot {
    pub image: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[tauri::command]
pub fn rec_region_snapshot(state: State<'_, RecorderState>) -> CmdResult<RegionSnapshot> {
    state
        .region_snapshot
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "no snapshot".into())
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Called by the picker: Some(rect) in absolute screen pixels, or None to cancel.
#[tauri::command]
pub fn rec_region_done(app: AppHandle, state: State<'_, RecorderState>, rect: Option<Rect>) {
    state.region_snapshot.lock().unwrap().take();
    if let Some(w) = app.get_webview_window("region") {
        let _ = w.close();
    }
    show_main(&app);
    let _ = app.emit("region-selected", rect);
}
