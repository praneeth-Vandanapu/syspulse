// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/recorder/sources.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: What can be recorded and with what: monitors (xrandr), windows
//           (EWMH via xprop/xwininfo), audio devices (PulseAudio / PipeWire
//           via pactl), and which video encoders actually work on this
//           machine (each candidate is probed with a tiny test pipeline).
// =============================================================================

use std::process::Command;
use std::sync::OnceLock;

use gstreamer as gst;
use gstreamer::prelude::*;
use serde::Serialize;

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// X11 window id (hex string, e.g. "0x3a00007").
    pub xid: String,
    pub title: String,
    pub app: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Special device values that follow the system's current default devices.
pub const FOLLOW_OUTPUT: &str = "@DEFAULT_MONITOR@";
pub const FOLLOW_INPUT: &str = "@DEFAULT_SOURCE@";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    /// PulseAudio source name (passed to pulsesrc device=).
    pub name: String,
    pub description: String,
    /// true = "monitor" of an output (system sound); false = input (microphone).
    pub is_monitor: bool,
    /// The current default output's monitor / the current default input.
    pub is_default: bool,
    /// false when the active port reports nothing plugged in (e.g. headset jack).
    pub available: bool,
    pub is_bluetooth: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    pub devices: Vec<AudioDevice>,
    /// Device that "follow current output" resolves to right now.
    pub current_output: Option<AudioDevice>,
    pub current_input: Option<AudioDevice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderInfo {
    /// GStreamer element name.
    pub element: String,
    /// h264 | h265 | vp8
    pub codec: String,
    pub hardware: bool,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sources {
    pub session_type: String,
    pub supported: bool,
    pub monitors: Vec<MonitorInfo>,
    pub windows: Vec<WindowInfo>,
    pub audio: Vec<AudioDevice>,
    pub encoders: Vec<EncoderInfo>,
    /// Elements that are missing (with the package that provides them).
    pub missing: Vec<String>,
}

pub fn init_gst() -> Result<()> {
    static INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| gst::init().map_err(|e| e.to_string()))
        .clone()
        .map_err(|e| CoreError::Other(format!("GStreamer could not start: {e}")))
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Parse "WxH+X+Y".
fn parse_geom(s: &str) -> Option<(u32, u32, i32, i32)> {
    let (wh, rest) = s.split_once('+')?;
    let (w, h) = wh.split_once('x')?;
    let (x, y) = rest.split_once('+')?;
    Some((
        w.parse().ok()?,
        h.parse().ok()?,
        x.parse().ok()?,
        y.parse().ok()?,
    ))
}

pub fn monitors() -> Vec<MonitorInfo> {
    let Some(out) = run("xrandr", &["--current"]) else {
        return Vec::new();
    };
    let mut v: Vec<MonitorInfo> = out
        .lines()
        .filter(|l| l.contains(" connected"))
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let name = parts.next()?.to_string();
            let primary = l.contains(" primary ");
            let (w, h, x, y) = l.split_whitespace().find_map(parse_geom)?;
            Some(MonitorInfo {
                name,
                x,
                y,
                width: w,
                height: h,
                primary,
            })
        })
        .collect();
    v.sort_by_key(|m| (!m.primary, m.x, m.y));
    v
}

fn xprop_value(out: &str, key: &str) -> Option<String> {
    out.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_once(" = ").map(|(_, v)| v.trim().to_string()))
}

/// Top-level application windows in stacking order (topmost first).
pub fn windows() -> Vec<WindowInfo> {
    let Some(root) = run("xprop", &["-root", "_NET_CLIENT_LIST_STACKING"]) else {
        return Vec::new();
    };
    let ids: Vec<String> = root
        .split_once('#')
        .map(|(_, list)| {
            list.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| s.starts_with("0x"))
                .collect()
        })
        .unwrap_or_default();
    let mut out = Vec::new();
    for id in ids.iter().rev() {
        let Some(p) = run(
            "xprop",
            &[
                "-id",
                id,
                "_NET_WM_NAME",
                "WM_CLASS",
                "_NET_WM_WINDOW_TYPE",
                "_NET_WM_STATE",
            ],
        ) else {
            continue;
        };
        let ty = xprop_value(&p, "_NET_WM_WINDOW_TYPE").unwrap_or_default();
        if ty.contains("DOCK") || ty.contains("DESKTOP") || ty.contains("SPLASH") {
            continue;
        }
        if xprop_value(&p, "_NET_WM_STATE")
            .unwrap_or_default()
            .contains("HIDDEN")
        {
            continue; // minimised
        }
        let title = xprop_value(&p, "_NET_WM_NAME")
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let app = xprop_value(&p, "WM_CLASS")
            .and_then(|c| {
                c.split(',')
                    .nth(1)
                    .map(|s| s.trim().trim_matches('"').to_string())
            })
            .unwrap_or_default();
        let Some(info) = run("xwininfo", &["-id", id]) else {
            continue;
        };
        let num = |k: &str| {
            info.lines().find_map(|l| {
                l.trim()
                    .strip_prefix(k)
                    .and_then(|v| v.trim().parse::<i64>().ok())
            })
        };
        let (Some(x), Some(y), Some(w), Some(h)) = (
            num("Absolute upper-left X:"),
            num("Absolute upper-left Y:"),
            num("Width:"),
            num("Height:"),
        ) else {
            continue;
        };
        if w < 50 || h < 50 || title.is_empty() {
            continue;
        }
        out.push(WindowInfo {
            xid: id.clone(),
            title,
            app,
            x: x as i32,
            y: y as i32,
            width: w as u32,
            height: h as u32,
        });
    }
    out
}

/// For each block of `pactl list sinks|sources`: name -> active port available?
fn port_availability(kind: &str) -> std::collections::HashMap<String, bool> {
    let Some(text) = run("pactl", &["list", kind]) else {
        return Default::default();
    };
    let header = if kind == "sinks" {
        "Sink #"
    } else {
        "Source #"
    };
    text.split(header)
        .skip(1)
        .filter_map(|block| {
            let name = block
                .lines()
                .find_map(|l| l.trim().strip_prefix("Name:").map(|v| v.trim().to_string()))?;
            let active = block.lines().find_map(|l| {
                l.trim()
                    .strip_prefix("Active Port:")
                    .map(|v| v.trim().to_string())
            });
            let available = match active {
                // "[In] Mic2" -> find the port line "[In] Mic2: ... not available)"
                Some(port) => !block.lines().any(|l| {
                    l.trim().starts_with(&format!("{port}:")) && l.contains("not available")
                }),
                None => true,
            };
            Some((name, available))
        })
        .collect()
}

pub fn audio_devices() -> Vec<AudioDevice> {
    let Some(text) = run("pactl", &["list", "sources"]) else {
        return Vec::new();
    };
    let default_source = run("pactl", &["get-default-source"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let default_monitor = run("pactl", &["get-default-sink"])
        .map(|s| format!("{}.monitor", s.trim()))
        .unwrap_or_default();
    let sinks = port_availability("sinks");
    let sources = port_availability("sources");
    let mut v = Vec::new();
    for block in text.split("Source #").skip(1) {
        let get = |k: &str| {
            block
                .lines()
                .find_map(|l| l.trim().strip_prefix(k).map(|v| v.trim().to_string()))
        };
        let Some(name) = get("Name:") else { continue };
        let is_monitor = get("Monitor of Sink:").is_some_and(|m| m != "n/a");
        let available = if is_monitor {
            sinks
                .get(name.trim_end_matches(".monitor"))
                .copied()
                .unwrap_or(true)
        } else {
            sources.get(&name).copied().unwrap_or(true)
        };
        v.push(AudioDevice {
            description: get("Description:").unwrap_or_else(|| name.clone()),
            is_default: name == default_source || name == default_monitor,
            is_bluetooth: name.starts_with("bluez_"),
            is_monitor,
            available,
            name,
        });
    }
    v.sort_by_key(|d| (d.is_monitor, !d.is_default, !d.available));
    v
}

pub fn audio_state() -> AudioState {
    let devices = audio_devices();
    let (out, inp) = default_audio();
    let find = |n: &Option<String>| {
        n.as_ref()
            .and_then(|n| devices.iter().find(|d| &d.name == n).cloned())
    };
    AudioState {
        current_output: find(&out),
        current_input: find(&inp),
        devices,
    }
}

/// The default output's monitor (what you hear) and default input (microphone).
pub fn default_audio() -> (Option<String>, Option<String>) {
    let mon = run("pactl", &["get-default-sink"]).map(|s| format!("{}.monitor", s.trim()));
    let mic = run("pactl", &["get-default-source"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.ends_with(".monitor"));
    (mon, mic)
}

/// Turn "follow" values into the concrete devices in use right now.
pub fn resolve_device(dev: &str) -> Option<String> {
    let (out, inp) = default_audio();
    match dev {
        FOLLOW_OUTPUT => out,
        FOLLOW_INPUT => inp,
        other => Some(other.to_string()),
    }
}

pub fn element_exists(name: &str) -> bool {
    init_gst().is_ok() && gst::ElementFactory::find(name).is_some()
}

/// Does a 2-frame encode with this element succeed? (NVENC can be present but
/// unusable, e.g. GStreamer 1.20 with a newer NVIDIA driver.)
fn probe_encoder(element: &str, input_format: &str) -> bool {
    if !element_exists(element) {
        return false;
    }
    let desc = format!("videotestsrc num-buffers=2 ! video/x-raw,width=320,height=240,framerate=30/1 ! videoconvert ! video/x-raw,format={input_format} ! {element} ! fakesink");
    let Ok(p) = gst::parse::launch(&desc) else {
        return false;
    };
    if p.set_state(gst::State::Playing).is_err() {
        let _ = p.set_state(gst::State::Null);
        return false;
    }
    let bus = p.bus().expect("pipeline bus");
    let ok = matches!(
        bus.timed_pop_filtered(
            gst::ClockTime::from_seconds(5),
            &[gst::MessageType::Eos, gst::MessageType::Error]
        )
        .map(|m| m.type_()),
        Some(gst::MessageType::Eos)
    );
    let _ = p.set_state(gst::State::Null);
    ok
}

/// Working encoders, best first per codec (probed once per process).
pub fn encoders() -> Vec<EncoderInfo> {
    static CACHE: OnceLock<Vec<EncoderInfo>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let candidates: [(&str, &str, bool, &str, &str); 9] = [
                ("nvh264enc", "h264", true, "NVIDIA NVENC H.264", "NV12"),
                (
                    "vah264enc",
                    "h264",
                    true,
                    "VA-API H.264 (Intel/AMD GPU)",
                    "NV12",
                ),
                (
                    "vaapih264enc",
                    "h264",
                    true,
                    "VA-API H.264 (Intel/AMD GPU)",
                    "NV12",
                ),
                ("x264enc", "h264", false, "x264 H.264 (CPU)", "I420"),
                ("nvh265enc", "h265", true, "NVIDIA NVENC H.265", "NV12"),
                (
                    "vah265enc",
                    "h265",
                    true,
                    "VA-API H.265 (Intel/AMD GPU)",
                    "NV12",
                ),
                (
                    "vaapih265enc",
                    "h265",
                    true,
                    "VA-API H.265 (Intel/AMD GPU)",
                    "NV12",
                ),
                ("x265enc", "h265", false, "x265 H.265 (CPU)", "I420"),
                ("vp8enc", "vp8", false, "VP8 (CPU)", "I420"),
            ];
            if init_gst().is_err() {
                return Vec::new();
            }
            candidates
                .iter()
                .filter(|(el, _, _, _, fmt)| probe_encoder(el, fmt))
                .map(|(el, codec, hw, label, _)| EncoderInfo {
                    element: el.to_string(),
                    codec: codec.to_string(),
                    hardware: *hw,
                    label: label.to_string(),
                })
                .collect()
        })
        .clone()
}

/// Elements the recorder relies on, with the Ubuntu package that ships them.
pub fn missing_elements() -> Vec<String> {
    [
        ("ximagesrc", "gstreamer1.0-x"),
        ("pulsesrc", "gstreamer1.0-pulseaudio"),
        ("x264enc", "gstreamer1.0-plugins-ugly"),
        ("avenc_aac", "gstreamer1.0-libav"),
        ("mp4mux", "gstreamer1.0-plugins-good"),
        ("audiomixer", "gstreamer1.0-plugins-base"),
        ("level", "gstreamer1.0-plugins-good"),
        ("jpegenc", "gstreamer1.0-plugins-good"),
    ]
    .iter()
    .filter(|(el, _)| !element_exists(el))
    .map(|(el, pkg)| format!("{el} (install {pkg})"))
    .collect()
}

pub fn sources() -> Sources {
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
    let supported = session_type != "wayland" && std::env::var("DISPLAY").is_ok();
    Sources {
        supported,
        monitors: monitors(),
        windows: windows(),
        audio: audio_devices(),
        encoders: encoders(),
        missing: missing_elements(),
        session_type,
    }
}

/// JPEG snapshot of a screen area as a data URL (for the region picker).
pub fn snapshot(x: i32, y: i32, w: u32, h: u32) -> Result<String> {
    init_gst()?;
    let tmp = std::env::temp_dir().join(format!("syspulse-snap-{}.jpg", std::process::id()));
    let desc = format!(
        "ximagesrc use-damage=false show-pointer=false num-buffers=1 startx={x} starty={y} endx={} endy={} ! videoconvert ! jpegenc quality=88 ! filesink location=\"{}\"",
        x + w as i32 - 1,
        y + h as i32 - 1,
        tmp.display()
    );
    let p = gst::parse::launch(&desc).map_err(|e| CoreError::Other(e.to_string()))?;
    p.set_state(gst::State::Playing)
        .map_err(|e| CoreError::Other(format!("snapshot: {e}")))?;
    let msg = p.bus().expect("bus").timed_pop_filtered(
        gst::ClockTime::from_seconds(5),
        &[gst::MessageType::Eos, gst::MessageType::Error],
    );
    let _ = p.set_state(gst::State::Null);
    if msg.map(|m| m.type_()) != Some(gst::MessageType::Eos) {
        return Err(CoreError::Other("could not capture the screen".into()));
    }
    let bytes = std::fs::read(&tmp)?;
    let _ = std::fs::remove_file(&tmp);
    Ok(format!(
        "data:image/jpeg;base64,{}",
        crate::util::base64(&bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry() {
        assert_eq!(parse_geom("1920x1080+0+0"), Some((1920, 1080, 0, 0)));
        assert_eq!(parse_geom("2560x1440+1920+0"), Some((2560, 1440, 1920, 0)));
        assert_eq!(parse_geom("primary"), None);
    }

    #[test]
    fn follow_values_resolve_to_current_defaults() {
        let (out, inp) = default_audio();
        assert_eq!(resolve_device(FOLLOW_OUTPUT), out);
        assert_eq!(resolve_device(FOLLOW_INPUT), inp);
        assert_eq!(
            resolve_device("some.device").as_deref(),
            Some("some.device")
        );
        if let Some(o) = out {
            assert!(
                o.ends_with(".monitor"),
                "system sound comes from an output monitor: {o}"
            );
        }
    }

    #[test]
    fn software_encoder_always_found() {
        let e = encoders();
        assert!(e.iter().any(|x| x.element == "x264enc"), "{e:?}");
    }
}
