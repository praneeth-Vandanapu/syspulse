// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/examples/rec_check.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Manual recorder check: lists sources/encoders and records a short
//           clip with a pause to verify there is no gap in the output.
//             cargo run -p syspulse-core --example rec_check -- [sources|pause]
// =============================================================================

use std::sync::Arc;
use std::time::Duration;

use syspulse_core::recorder::{
    sources, AudioConfig, CaptureArea, Container, Quality, RecordConfig, Recorder, VideoConfig,
};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("sources") {
        let s = sources::sources();
        println!("session {} supported {}", s.session_type, s.supported);
        for m in &s.monitors {
            println!("monitor {:?}", m);
        }
        for w in s.windows.iter().take(8) {
            println!("window {} {:?} {}x{}", w.xid, w.title, w.width, w.height);
        }
        for a in &s.audio {
            println!(
                "audio monitor={} default={} available={} bt={} {}",
                a.is_monitor, a.is_default, a.available, a.is_bluetooth, a.description
            );
        }
        for e in &s.encoders {
            println!("encoder {} {} hw={}", e.element, e.codec, e.hardware);
        }
        println!("missing {:?}", s.missing);
        return;
    }
    let dir = std::env::temp_dir().join("syspulse-rec-check");
    let cfg = RecordConfig {
        area: CaptureArea::Region {
            x: 0,
            y: 0,
            width: 640,
            height: 360,
        },
        audio: AudioConfig {
            system: sources::default_audio().0,
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
        output_dir: dir.display().to_string(),
    };
    let mut r = Recorder::start(&cfg, Arc::new(|_| {})).unwrap();
    println!(
        "recording with {} -> {} audio={:?}",
        r.info.encoder, r.info.path, r.info.audio
    );
    std::thread::sleep(Duration::from_secs(2));
    r.pause().unwrap();
    std::thread::sleep(Duration::from_secs(2));
    r.resume().unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let res = r.stop().unwrap();
    println!("result {:?}", res);
    let out = std::process::Command::new("/usr/bin/gst-discoverer-1.0")
        .arg(&res.path)
        .output()
        .unwrap();
    for l in String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains("Duration") || l.contains("#"))
    {
        println!("  {l}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
