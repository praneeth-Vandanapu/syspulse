// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/recorder/mod.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Screen recorder (X11) built on GStreamer.
//             video : ximagesrc (monitor / window / region) -> encoder
//                     (NVENC / VA-API when they work, x264/x265/VP8 otherwise)
//             audio : system sound (output monitor) and/or microphone via
//                     pulsesrc, each with volume + level meter, mixed into
//                     one AAC (MP4/MKV) or Opus (WebM) track
//             mux   : MP4, MKV or WebM
//           Pause/resume pauses the live pipeline: running time stops, so
//           the file continues seamlessly. Stop sends EOS so the container
//           is finalised (MP4 needs this to be playable).
// =============================================================================

pub mod sources;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use sources::{encoders, init_gst};

// ---------------------------------------------------------------- config ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CaptureArea {
    /// A whole monitor (by its geometry).
    #[serde(rename_all = "camelCase")]
    Monitor {
        name: String,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    /// One window (follows the window's contents).
    #[serde(rename_all = "camelCase")]
    Window {
        xid: String,
        title: String,
        width: u32,
        height: u32,
    },
    /// A rectangle of the screen (absolute coordinates).
    #[serde(rename_all = "camelCase")]
    Region {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioConfig {
    /// pulse source name of an output monitor (system sound), if recorded.
    pub system: Option<String>,
    pub mic: Option<String>,
    #[serde(default = "one")]
    pub system_volume: f64,
    #[serde(default = "one")]
    pub mic_volume: f64,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Container {
    Mp4,
    Mkv,
    Webm,
}

impl Container {
    pub fn ext(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
            Container::Webm => "webm",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Standard,
    High,
    Max,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoConfig {
    pub container: Container,
    /// h264 | h265 | vp8
    pub codec: String,
    pub fps: u32,
    pub quality: Quality,
    /// "auto" (best working encoder, GPU first), "cpu", or an element name.
    #[serde(default = "auto")]
    pub encoder: String,
    #[serde(default = "yes")]
    pub show_pointer: bool,
}

fn auto() -> String {
    "auto".into()
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordConfig {
    pub area: CaptureArea,
    pub audio: AudioConfig,
    pub video: VideoConfig,
    pub output_dir: String,
}

// ---------------------------------------------------------------- events ---

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RecorderEvent {
    /// Audio peak levels in dBFS (-100 .. 0), ~10 per second.
    #[serde(rename_all = "camelCase")]
    Level {
        system: Option<f64>,
        mic: Option<f64>,
    },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
}

pub type RecorderSink = Arc<dyn Fn(RecorderEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingInfo {
    pub path: String,
    pub encoder: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    /// Human-readable audio devices actually recorded.
    pub audio: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingResult {
    pub path: String,
    pub duration_secs: f64,
    pub size: u64,
    pub encoder: String,
}

// -------------------------------------------------------------- pipeline ---

fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

/// Quote a value for gst-launch syntax.
fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn valid_device(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 256
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-:@".contains(c))
}

/// Pick the encoder element for a codec.
fn pick_encoder(codec: &str, pref: &str) -> Result<(String, bool)> {
    let list = encoders();
    let of_codec: Vec<_> = list.iter().filter(|e| e.codec == codec).collect();
    let chosen = match pref {
        "auto" => of_codec.first().copied(),
        "cpu" => of_codec.iter().find(|e| !e.hardware).copied(),
        el => of_codec.iter().find(|e| e.element == el).copied(),
    };
    chosen
        .map(|e| (e.element.clone(), e.hardware))
        .ok_or_else(|| {
            CoreError::Other(format!(
                "no working {codec} encoder found (install gstreamer1.0-plugins-ugly for x264)"
            ))
        })
}

pub struct Plan {
    pub description: String,
    pub info: RecordingInfo,
}

/// Build the gst-launch description for a recording.
pub fn plan(cfg: &RecordConfig, path: &Path) -> Result<Plan> {
    let fps = cfg.video.fps.clamp(5, 60);
    // Capture geometry and source properties.
    let (src_props, w, h) = match &cfg.area {
        CaptureArea::Monitor {
            x,
            y,
            width,
            height,
            ..
        }
        | CaptureArea::Region {
            x,
            y,
            width,
            height,
        } => {
            let (w, h) = (even(*width), even(*height));
            if w < 16 || h < 16 {
                return Err(CoreError::Refused("the capture area is too small".into()));
            }
            (
                format!(
                    "startx={x} starty={y} endx={} endy={}",
                    x + w as i32 - 1,
                    y + h as i32 - 1
                ),
                w,
                h,
            )
        }
        CaptureArea::Window {
            xid, width, height, ..
        } => {
            let id = xid.trim_start_matches("0x");
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(CoreError::Refused("invalid window id".into()));
            }
            (
                format!("xid={}", u64::from_str_radix(id, 16).unwrap_or(0)),
                even(*width),
                even(*height),
            )
        }
    };
    let codec = match (cfg.video.container, cfg.video.codec.as_str()) {
        (Container::Webm, _) => "vp8",
        (_, "h265") => "h265",
        _ => "h264",
    };
    let (enc_el, hw) = pick_encoder(codec, &cfg.video.encoder)?;
    let bpp = match cfg.video.quality {
        Quality::Standard => 0.06,
        Quality::High => 0.10,
        Quality::Max => 0.20,
    } * if codec == "h265" { 0.7 } else { 1.0 };
    let kbps = ((f64::from(w) * f64::from(h) * f64::from(fps) * bpp) / 1000.0)
        .clamp(1000.0, 80_000.0) as u32;
    let gop = fps * 2;
    let (fmt, enc) = match enc_el.as_str() {
        "nvh264enc" | "nvh265enc" => ("NV12", format!("{enc_el} preset=low-latency-hq rc-mode=vbr bitrate={kbps} gop-size={gop}")),
        "vah264enc" | "vah265enc" => ("NV12", format!("{enc_el} bitrate={kbps} key-int-max={gop}")),
        "vaapih264enc" | "vaapih265enc" => ("NV12", format!("{enc_el} rate-control=vbr bitrate={kbps} keyframe-period={gop}")),
        "x264enc" => ("I420", format!("x264enc speed-preset=veryfast bitrate={kbps} key-int-max={gop} ! video/x-h264,profile=high")),
        "x265enc" => ("I420", format!("x265enc speed-preset=superfast bitrate={kbps} key-int-max={gop}")),
        _ => ("I420", format!("vp8enc deadline=1 cpu-used=8 threads=8 target-bitrate={} keyframe-max-dist={gop}", kbps * 1000)),
    };
    let parse = match codec {
        "h264" => " ! h264parse",
        "h265" => " ! h265parse",
        _ => "",
    };
    let pointer = cfg.video.show_pointer;
    let scale = if matches!(cfg.area, CaptureArea::Window { .. }) {
        format!(" ! videoscale ! video/x-raw,width={w},height={h}")
    } else {
        String::new()
    };
    let mut d = format!(
        "ximagesrc use-damage=false show-pointer={pointer} {src_props} ! video/x-raw,framerate={fps}/1 ! videoconvert{scale} ! video/x-raw,format={fmt} \
         ! queue max-size-buffers=0 max-size-bytes=0 max-size-time=2000000000 ! {enc}{parse} ! queue ! mux. "
    );

    // Audio: each source -> volume -> level -> (mixer) -> encoder.
    let audio_enc = if cfg.video.container == Container::Webm {
        "opusenc bitrate=160000"
    } else {
        "avenc_aac bitrate=192000 ! aacparse"
    };
    let mut srcs = Vec::new();
    for (key, dev, vol) in [
        ("sys", &cfg.audio.system, cfg.audio.system_volume),
        ("mic", &cfg.audio.mic, cfg.audio.mic_volume),
    ] {
        if let Some(dev) = dev {
            if !valid_device(dev) {
                return Err(CoreError::Refused(format!(
                    "invalid audio device name {dev}"
                )));
            }
            srcs.push(format!(
                "pulsesrc device={} ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! volume name=vol_{key} volume={:.2} ! level name=lvl_{key} interval=100000000 post-messages=true ! queue",
                q(dev),
                vol.clamp(0.0, 4.0)
            ));
        }
    }
    match srcs.len() {
        0 => {}
        1 => d.push_str(&format!("{} ! {audio_enc} ! queue ! mux. ", srcs[0])),
        _ => {
            d.push_str(&format!(
                "audiomixer name=amix ! audioconvert ! {audio_enc} ! queue ! mux. "
            ));
            for s in srcs {
                d.push_str(&format!("{s} ! amix. "));
            }
        }
    }
    let mux = match cfg.video.container {
        Container::Mp4 => "mp4mux",
        Container::Mkv => "matroskamux",
        Container::Webm => "webmmux",
    };
    d.push_str(&format!(
        "{mux} name=mux ! filesink location={}",
        q(&path.display().to_string())
    ));
    let enc_label = sources::encoders()
        .into_iter()
        .find(|e| e.element == enc_el)
        .map(|e| e.label)
        .unwrap_or(enc_el);
    let _ = hw;
    Ok(Plan {
        description: d,
        info: RecordingInfo {
            path: path.display().to_string(),
            encoder: enc_label,
            width: w,
            height: h,
            fps,
            bitrate_kbps: kbps,
            audio: Vec::new(),
        },
    })
}

// -------------------------------------------------------------- recorder ---

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RecState {
    Recording,
    Paused,
}

struct Shared {
    eos: Mutex<bool>,
    cv: Condvar,
    failed: Mutex<Option<String>>,
}

pub struct Recorder {
    pipeline: gst::Element,
    pub info: RecordingInfo,
    state: RecState,
    started: Instant,
    paused_at: Option<Instant>,
    paused_total: Duration,
    shared: Arc<Shared>,
    stop_bus: Arc<AtomicBool>,
    bus_thread: Option<JoinHandle<()>>,
}

/// Peak dBFS (max over channels) from a `level` element message.
fn peak_db(s: &gst::StructureRef) -> Option<f64> {
    let arr = s.get::<gst::glib::ValueArray>("peak").ok()?;
    arr.iter()
        .filter_map(|v| v.get::<f64>().ok())
        .fold(None, |a: Option<f64>, b| Some(a.map_or(b, |a| a.max(b))))
}

fn spawn_bus(
    pipeline: &gst::Element,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    sink: RecorderSink,
) -> JoinHandle<()> {
    let bus = pipeline.bus().expect("pipeline bus");
    std::thread::spawn(move || {
        let (mut sys, mut mic) = (None, None);
        let mut last = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) else {
                continue;
            };
            match msg.view() {
                gst::MessageView::Eos(_) => {
                    *shared.eos.lock().unwrap() = true;
                    shared.cv.notify_all();
                }
                gst::MessageView::Error(e) => {
                    let text = format!(
                        "{} ({})",
                        e.error(),
                        e.debug().map(|d| d.to_string()).unwrap_or_default()
                    );
                    *shared.failed.lock().unwrap() = Some(text.clone());
                    *shared.eos.lock().unwrap() = true;
                    shared.cv.notify_all();
                    sink(RecorderEvent::Error {
                        message: e.error().to_string(),
                    });
                }
                gst::MessageView::Element(el) => {
                    if let (Some(s), Some(src)) = (el.structure(), msg.src()) {
                        if s.name() == "level" {
                            let v = peak_db(s);
                            match src.name().as_str() {
                                "lvl_sys" => sys = v,
                                "lvl_mic" => mic = v,
                                _ => {}
                            }
                            if last.elapsed() >= Duration::from_millis(90) {
                                last = Instant::now();
                                sink(RecorderEvent::Level { system: sys, mic });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    })
}

/// Unique output path like ~/Videos/SysPulse/SysPulse_2026-10-08_14-30-05.mp4
pub fn output_path(dir: &str, ext: &str) -> Result<PathBuf> {
    let dir = if let Some(rest) = dir.strip_prefix('~') {
        crate::fsutil::home_dir()
            .ok_or_else(|| CoreError::Other("no home directory".into()))?
            .join(rest.trim_start_matches('/'))
    } else {
        PathBuf::from(dir)
    };
    if !dir.is_absolute() {
        return Err(CoreError::Refused(
            "output folder must be an absolute path".into(),
        ));
    }
    std::fs::create_dir_all(&dir)?;
    let stamp = crate::forge::iso_utc(local_now_ms())
        .replace('T', "_")
        .replace(':', "-");
    let stamp = &stamp[..19];
    let mut p = dir.join(format!("SysPulse_{stamp}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("SysPulse_{stamp}_{n}.{ext}"));
        n += 1;
    }
    Ok(p)
}

/// Current time shifted by the local UTC offset (for file names).
fn local_now_ms() -> f64 {
    let utc = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    #[cfg(unix)]
    {
        // SAFETY: localtime_r writes into our tm struct only.
        unsafe {
            let t = (utc / 1000.0) as libc::time_t;
            let mut tm: libc::tm = std::mem::zeroed();
            if !libc::localtime_r(&t, &mut tm).is_null() {
                return utc + tm.tm_gmtoff as f64 * 1000.0;
            }
        }
    }
    utc
}

impl Recorder {
    pub fn start(cfg: &RecordConfig, sink: RecorderSink) -> Result<Recorder> {
        init_gst()?;
        if std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland") {
            return Err(CoreError::Unsupported(
                "screen recording on Wayland (this version supports X11 sessions)",
            ));
        }
        // "Follow current output/input" -> the devices in use at this moment.
        let mut cfg = cfg.clone();
        let mut audio_desc = Vec::new();
        let devices = sources::audio_devices();
        for (slot, label) in [
            (&mut cfg.audio.system, "System sound"),
            (&mut cfg.audio.mic, "Microphone"),
        ] {
            if let Some(d) = slot.take() {
                let real = sources::resolve_device(&d)
                    .ok_or_else(|| CoreError::Other(format!("{label}: no default audio device")))?;
                let desc = devices
                    .iter()
                    .find(|x| x.name == real)
                    .map(|x| x.description.clone())
                    .unwrap_or_else(|| real.clone());
                audio_desc.push(format!("{label}: {desc}"));
                *slot = Some(real);
            }
        }
        let cfg = &cfg;
        let path = output_path(&cfg.output_dir, cfg.video.container.ext())?;
        let mut plan = plan(cfg, &path)?;
        plan.info.audio = audio_desc;
        log::info!("recorder pipeline: {}", plan.description);
        let pipeline = gst::parse::launch(&plan.description)
            .map_err(|e| CoreError::Other(format!("pipeline: {e}")))?;
        let shared = Arc::new(Shared {
            eos: Mutex::new(false),
            cv: Condvar::new(),
            failed: Mutex::new(None),
        });
        let stop_bus = Arc::new(AtomicBool::new(false));
        let bus_thread = spawn_bus(&pipeline, shared.clone(), stop_bus.clone(), sink);
        if let Err(e) = pipeline.set_state(gst::State::Playing) {
            stop_bus.store(true, Ordering::SeqCst);
            let _ = pipeline.set_state(gst::State::Null);
            let _ = std::fs::remove_file(&path);
            return Err(CoreError::Other(format!("could not start recording: {e}")));
        }
        // Surface immediate failures (e.g. a device that cannot be opened).
        std::thread::sleep(Duration::from_millis(300));
        if let Some(err) = shared.failed.lock().unwrap().clone() {
            stop_bus.store(true, Ordering::SeqCst);
            let _ = pipeline.set_state(gst::State::Null);
            let _ = std::fs::remove_file(&path);
            return Err(CoreError::Other(format!("recording failed: {err}")));
        }
        Ok(Recorder {
            pipeline,
            info: plan.info,
            state: RecState::Recording,
            started: Instant::now(),
            paused_at: None,
            paused_total: Duration::ZERO,
            shared,
            stop_bus,
            bus_thread: Some(bus_thread),
        })
    }

    pub fn state(&self) -> RecState {
        self.state
    }

    /// Recorded time excluding pauses.
    pub fn elapsed(&self) -> Duration {
        let paused_now = self.paused_at.map(|p| p.elapsed()).unwrap_or_default();
        self.started
            .elapsed()
            .saturating_sub(self.paused_total + paused_now)
    }

    pub fn size(&self) -> u64 {
        std::fs::metadata(&self.info.path)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    pub fn failed(&self) -> Option<String> {
        self.shared.failed.lock().ok().and_then(|f| f.clone())
    }

    pub fn pause(&mut self) -> Result<()> {
        if self.state == RecState::Paused {
            return Ok(());
        }
        self.pipeline
            .set_state(gst::State::Paused)
            .map_err(|e| CoreError::Other(format!("pause: {e}")))?;
        self.state = RecState::Paused;
        self.paused_at = Some(Instant::now());
        Ok(())
    }

    pub fn resume(&mut self) -> Result<()> {
        if self.state == RecState::Recording {
            return Ok(());
        }
        self.pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| CoreError::Other(format!("resume: {e}")))?;
        if let Some(p) = self.paused_at.take() {
            self.paused_total += p.elapsed();
        }
        self.state = RecState::Recording;
        Ok(())
    }

    fn shutdown(&mut self, finalize: bool) -> Result<()> {
        if finalize {
            if self.state == RecState::Paused {
                let _ = self.resume();
            }
            self.pipeline.send_event(gst::event::Eos::new());
            let guard = self.shared.eos.lock().unwrap();
            let (_g, timeout) = self
                .shared
                .cv
                .wait_timeout_while(guard, Duration::from_secs(15), |eos| !*eos)
                .unwrap();
            if timeout.timed_out() {
                log::warn!("recorder: EOS timed out; file may be incomplete");
            }
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        self.stop_bus.store(true, Ordering::SeqCst);
        if let Some(t) = self.bus_thread.take() {
            let _ = t.join();
        }
        Ok(())
    }

    /// Finish the file and return its details.
    pub fn stop(mut self) -> Result<RecordingResult> {
        let duration = self.elapsed().as_secs_f64();
        self.shutdown(true)?;
        if let Some(err) = self.failed() {
            return Err(CoreError::Other(format!("recording failed: {err}")));
        }
        Ok(RecordingResult {
            size: self.size(),
            path: self.info.path.clone(),
            duration_secs: duration,
            encoder: self.info.encoder.clone(),
        })
    }

    /// Abort and delete the partial file.
    pub fn cancel(mut self) -> Result<()> {
        self.shutdown(false)?;
        let _ = std::fs::remove_file(&self.info.path);
        Ok(())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if self.bus_thread.is_some() {
            // Dropped without stop(): finalise so the file stays playable.
            let _ = self.shutdown(true);
        }
    }
}

// ----------------------------------------------------------- level meter ---

/// Audio-only pipeline that just reports levels (for the setup screen).
pub struct AudioMeter {
    pipeline: gst::Element,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioMeter {
    pub fn start(
        system: Option<&str>,
        mic: Option<&str>,
        sink: RecorderSink,
    ) -> Result<Option<AudioMeter>> {
        init_gst()?;
        let mut parts = Vec::new();
        let resolved: Vec<(&str, Option<String>)> = vec![
            ("sys", system.and_then(sources::resolve_device)),
            ("mic", mic.and_then(sources::resolve_device)),
        ];
        for (key, dev) in resolved.iter().map(|(k, d)| (*k, d.as_deref())) {
            if let Some(d) = dev {
                if !valid_device(d) {
                    return Err(CoreError::Refused(format!("invalid audio device name {d}")));
                }
                parts.push(format!("pulsesrc device={} ! audioconvert ! level name=lvl_{key} interval=100000000 post-messages=true ! fakesink sync=false", q(d)));
            }
        }
        if parts.is_empty() {
            return Ok(None);
        }
        let pipeline =
            gst::parse::launch(&parts.join(" ")).map_err(|e| CoreError::Other(e.to_string()))?;
        let shared = Arc::new(Shared {
            eos: Mutex::new(false),
            cv: Condvar::new(),
            failed: Mutex::new(None),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let thread = spawn_bus(&pipeline, shared, stop.clone(), sink);
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| CoreError::Other(format!("audio meter: {e}")))?;
        Ok(Some(AudioMeter {
            pipeline,
            stop,
            thread: Some(thread),
        }))
    }
}

impl Drop for AudioMeter {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ------------------------------------------------------------ recordings ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

pub fn list_recordings(dir: &str) -> Vec<RecordingFile> {
    let dir = match dir.strip_prefix('~') {
        Some(rest) => match crate::fsutil::home_dir() {
            Some(h) => h.join(rest.trim_start_matches('/')),
            None => return Vec::new(),
        },
        None => PathBuf::from(dir),
    };
    let mut v: Vec<RecordingFile> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(std::result::Result::ok)
                .filter(|e| {
                    matches!(
                        e.path().extension().and_then(|x| x.to_str()),
                        Some("mp4" | "mkv" | "webm")
                    )
                })
                .filter_map(|e| {
                    let m = e.metadata().ok()?;
                    Some(RecordingFile {
                        path: e.path().display().to_string(),
                        name: e.file_name().to_string_lossy().into_owned(),
                        size: m.len(),
                        modified: m
                            .modified()
                            .ok()?
                            .duration_since(std::time::UNIX_EPOCH)
                            .ok()?
                            .as_secs(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|f| std::cmp::Reverse(f.modified));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(area: CaptureArea, container: Container, audio: bool) -> RecordConfig {
        RecordConfig {
            area,
            audio: AudioConfig {
                system: audio.then(|| "alsa_output.x.monitor".into()),
                mic: audio.then(|| "alsa_input.y".into()),
                system_volume: 1.0,
                mic_volume: 0.8,
            },
            video: VideoConfig {
                container,
                codec: "h264".into(),
                fps: 30,
                quality: Quality::High,
                encoder: "cpu".into(),
                show_pointer: true,
            },
            output_dir: "/tmp".into(),
        }
    }

    #[test]
    fn plans_pipelines() {
        let p = plan(
            &cfg(
                CaptureArea::Region {
                    x: 10,
                    y: 20,
                    width: 641,
                    height: 481,
                },
                Container::Mp4,
                true,
            ),
            Path::new("/tmp/a.mp4"),
        )
        .unwrap();
        assert!(
            p.description
                .contains("startx=10 starty=20 endx=649 endy=499"),
            "odd sizes rounded to even: {}",
            p.description
        );
        assert!(p.description.contains("audiomixer name=amix"));
        assert!(p.description.contains("x264enc"));
        assert!(p.description.contains("mp4mux"));
        assert_eq!((p.info.width, p.info.height), (640, 480));
        let w = plan(
            &cfg(
                CaptureArea::Window {
                    xid: "0x3a00007".into(),
                    title: "x".into(),
                    width: 800,
                    height: 600,
                },
                Container::Webm,
                false,
            ),
            Path::new("/tmp/a.webm"),
        )
        .unwrap();
        assert!(
            w.description.contains("xid=60817415")
                && w.description.contains("vp8enc")
                && w.description.contains("webmmux")
        );
        assert!(!w.description.contains("pulsesrc"));
    }

    #[test]
    fn rejects_injection() {
        let mut c = cfg(
            CaptureArea::Region {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            Container::Mp4,
            true,
        );
        c.audio.mic = Some("x ! filesink location=/etc/passwd".into());
        assert!(plan(&c, Path::new("/tmp/a.mp4")).is_err());
        let bad = cfg(
            CaptureArea::Window {
                xid: "0x12; rm".into(),
                title: "x".into(),
                width: 100,
                height: 100,
            },
            Container::Mp4,
            false,
        );
        assert!(plan(&bad, Path::new("/tmp/a.mp4")).is_err());
    }

    /// Real recording of a small screen region (X11 only), with pause/resume.
    #[test]
    fn records_region_with_pause() {
        if std::env::var("DISPLAY").is_err()
            || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("syspulse-rec-{}", std::process::id()));
        let mut c = cfg(
            CaptureArea::Region {
                x: 0,
                y: 0,
                width: 320,
                height: 240,
            },
            Container::Mp4,
            false,
        );
        c.output_dir = dir.display().to_string();
        let mut r = Recorder::start(&c, Arc::new(|_| {})).unwrap();
        std::thread::sleep(Duration::from_millis(800));
        r.pause().unwrap();
        std::thread::sleep(Duration::from_millis(600));
        r.resume().unwrap();
        std::thread::sleep(Duration::from_millis(800));
        let res = r.stop().unwrap();
        assert!(res.size > 1000, "file has data: {res:?}");
        assert!(
            (1.2..2.4).contains(&res.duration_secs),
            "pause excluded from duration: {}",
            res.duration_secs
        );
        // The finalised MP4 must be readable and about as long as recorded.
        let out = std::process::Command::new("/usr/bin/gst-discoverer-1.0")
            .arg(&res.path)
            .output();
        if let Ok(o) = out {
            let text = String::from_utf8_lossy(&o.stdout);
            assert!(text.contains("H.264"), "{text}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
