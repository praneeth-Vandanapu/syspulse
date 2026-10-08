// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/mod.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: NetLens - packet capture and protocol analysis (Wireshark-like).
//
//             capture    live capture through the `syspulse-capture` helper
//             sniffer    the helper's AF_PACKET loop (runs in the helper)
//             pcapfile   pcap / pcapng read and write
//             dissect/*  protocol dissectors (detail tree, fields, facts)
//             filter     Wireshark-style display filters
//             store      frames + incremental analysis
//             views      list rows, conversations, apps, stats, follow
//             explain    plain-English frame summaries
//             procmap    connection -> application attribution
//             wireshark  optional tshark / Wireshark integration
//
//           `NetLens` below is the engine the UI talks to. A reader thread
//           ingests helper records into the store under a write lock; UI
//           queries take read locks and only dissect what is on screen.
// =============================================================================

pub mod capture;
pub mod dissect;
pub mod explain;
pub mod filter;
pub mod pcapfile;
pub mod procmap;
#[cfg(target_os = "linux")]
pub mod sniffer;
pub mod store;
pub mod views;
pub mod wireshark;

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use capture::{CaptureProcess, CaptureSpec, Record};
use filter::Filter;
use pcapfile::{CaptureReader, IfaceDesc, PcapNgWriter};
use store::Store;

/// Local time "YYYY-MM-DD HH:MM:SS.nnnnnnnnn".
pub fn fmt_time(ts_ns: u64) -> String {
    let secs = (ts_ns / 1_000_000_000) as libc::time_t;
    let nanos = ts_ns % 1_000_000_000;
    // SAFETY: localtime_r writes into the zeroed tm we own.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{nanos:09}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Limits {
    /// Stop after this many packets (0 = no limit).
    pub max_packets: u64,
    /// Stop when the stored capture reaches this size in MB.
    pub max_mb: u64,
    /// Stop after this many seconds (0 = no limit).
    pub max_seconds: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_packets: 0,
            max_mb: 1024,
            max_seconds: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// idle, capturing, loading, stopped
    pub state: &'static str,
    pub source: String,
    pub interfaces: Vec<String>,
    pub capture_filter: String,
    pub packets: u64,
    pub shown: u64,
    pub bytes: u64,
    pub memory: u64,
    pub dropped: u64,
    pub duration: f64,
    pub packets_per_sec: u64,
    pub bits_per_sec: u64,
    pub messages: Vec<String>,
    pub display_filter: String,
    pub unsaved: bool,
    pub progress: f64,
    pub stop_reason: String,
}

#[derive(Default)]
struct Info {
    state: &'static str,
    messages: Vec<String>,
    started: Option<Instant>,
    capture_filter: String,
    progress: f64,
    stop_reason: String,
}

struct Live {
    stop: Arc<AtomicBool>,
    proc: Arc<Mutex<Option<CaptureProcess>>>,
    threads: Vec<JoinHandle<()>>,
}

pub struct NetLens {
    store: Arc<RwLock<Store>>,
    info: Arc<Mutex<Info>>,
    live: Mutex<Option<Live>>,
}

impl Default for NetLens {
    fn default() -> Self {
        NetLens {
            store: Arc::new(RwLock::new(Store::default())),
            info: Arc::new(Mutex::new(Info {
                state: "idle",
                ..Default::default()
            })),
            live: Mutex::new(None),
        }
    }
}

fn poisoned<T>(_: T) -> CoreError {
    CoreError::Other("NetLens state lock poisoned".into())
}

fn local_ips() -> HashSet<IpAddr> {
    let mut s: HashSet<IpAddr> = capture::interfaces()
        .into_iter()
        .flat_map(|i| i.addresses)
        .filter_map(|a| a.parse().ok())
        .collect();
    s.insert(IpAddr::from([127, 0, 0, 1]));
    s.insert("::1".parse::<IpAddr>().unwrap_or(IpAddr::from([127, 0, 0, 1])));
    s
}

impl NetLens {
    fn set_info(&self, f: impl FnOnce(&mut Info)) {
        if let Ok(mut i) = self.info.lock() {
            f(&mut i);
        }
    }

    pub fn is_busy(&self) -> bool {
        self.live.lock().map(|l| l.is_some()).unwrap_or(false)
    }

    /// Start a live capture (replaces the current capture).
    pub fn start(&self, spec: CaptureSpec, limits: Limits) -> Result<()> {
        self.stop();
        let (proc, mut reader, ifaces) = CaptureProcess::start(&spec)?;
        let snap = if spec.snaplen == 0 { capture::MAX_SNAPLEN } else { spec.snaplen };
        let descs: Vec<IfaceDesc> = ifaces
            .iter()
            .map(|(n, lt)| IfaceDesc {
                name: n.clone(),
                linktype: *lt,
                snaplen: snap,
            })
            .collect();
        {
            let mut st = self.store.write().map_err(poisoned)?;
            let filter = st.filter.take();
            let mut fresh = Store::new(descs, &format!("Live capture on {}", spec.interfaces.join(", ")));
            fresh.local_ips = local_ips();
            if let Some(f) = filter {
                fresh.view = Some(Vec::new());
                fresh.filter = Some(f);
            }
            *st = fresh;
        }
        self.set_info(|i| {
            i.state = "capturing";
            i.messages.clear();
            i.started = Some(Instant::now());
            i.capture_filter = spec.filter.clone();
            i.progress = 0.0;
            i.stop_reason.clear();
        });

        let stop = Arc::new(AtomicBool::new(false));
        let proc = Arc::new(Mutex::new(Some(proc)));
        let mut threads = Vec::new();

        // Reader: helper stream -> store.
        {
            let store = self.store.clone();
            let info = self.info.clone();
            let stop = stop.clone();
            let proc = proc.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("netlens-reader".into())
                    .spawn(move || {
                        let max_bytes = limits.max_mb.max(16) * 1024 * 1024;
                        let mut batch: Vec<Record> = Vec::with_capacity(1024);
                        loop {
                            batch.clear();
                            match capture::read_record(&mut reader) {
                                Ok(Some(r)) => batch.push(r),
                                _ => break,
                            }
                            while !reader.buffer().is_empty() && batch.len() < 1024 {
                                match capture::read_record(&mut reader) {
                                    Ok(Some(r)) => batch.push(r),
                                    _ => break,
                                }
                            }
                            let mut limit_hit = None;
                            if let Ok(mut st) = store.write() {
                                for r in batch.drain(..) {
                                    match r {
                                        Record::Packet { iface, outgoing, ts_ns, orig_len, data } => {
                                            st.add(iface, ts_ns, orig_len, Some(outgoing), &data);
                                        }
                                        Record::Stats { received, dropped, .. } => {
                                            st.received += received as u64;
                                            st.dropped += dropped as u64;
                                        }
                                        Record::Message(m) => {
                                            if let Ok(mut i) = info.lock() {
                                                i.messages.push(m);
                                            }
                                        }
                                        Record::Ready(n) => {
                                            if n == 0 {
                                                if let Ok(mut i) = info.lock() {
                                                    i.stop_reason = "No interface could be opened".into();
                                                }
                                            }
                                        }
                                    }
                                }
                                if limits.max_packets > 0 && st.packets.len() as u64 >= limits.max_packets {
                                    limit_hit = Some(format!("Stopped after {} packets (limit)", limits.max_packets));
                                } else if st.bytes_stored >= max_bytes {
                                    limit_hit = Some(format!("Stopped at the {} MB memory limit", limits.max_mb));
                                }
                            }
                            if let Some(reason) = limit_hit {
                                if let Ok(mut i) = info.lock() {
                                    i.stop_reason = reason;
                                }
                                stop.store(true, Ordering::SeqCst);
                                if let Ok(mut p) = proc.lock() {
                                    if let Some(mut c) = p.take() {
                                        c.stop();
                                    }
                                }
                                break;
                            }
                        }
                        if let Ok(mut i) = info.lock() {
                            i.state = "stopped";
                        }
                        stop.store(true, Ordering::SeqCst);
                    })
                    .map_err(CoreError::Io)?,
            );
        }

        // Attribution + time limit.
        {
            let store = self.store.clone();
            let info = self.info.clone();
            let stop = stop.clone();
            let proc = proc.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("netlens-apps".into())
                    .spawn(move || {
                        let mut map = procmap::ProcMap::default();
                        let mut attempts: HashMap<u32, u8> = HashMap::new();
                        let started = Instant::now();
                        while !stop.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(400));
                            if limits.max_seconds > 0 && started.elapsed().as_secs() >= limits.max_seconds {
                                if let Ok(mut i) = info.lock() {
                                    i.stop_reason = format!("Stopped after {} s (limit)", limits.max_seconds);
                                }
                                if let Ok(mut p) = proc.lock() {
                                    if let Some(mut c) = p.take() {
                                        c.stop();
                                    }
                                }
                                break;
                            }
                            let pending = store.read().map(|s| s.pending_apps()).unwrap_or_default();
                            if pending.is_empty() {
                                continue;
                            }
                            map.refresh(Duration::from_millis(300));
                            type Resolved = (u32, Option<(String, u32)>, bool);
                            let resolved: Vec<Resolved> = pending
                                .into_iter()
                                .map(|(id, tcp, port)| {
                                    let n = attempts.entry(id).or_insert(0);
                                    *n += 1;
                                    (id, map.lookup(tcp, port), *n >= 6)
                                })
                                .collect();
                            if let Ok(mut st) = store.write() {
                                for (id, app, give_up) in resolved {
                                    st.resolve_app(id, app, give_up);
                                }
                            }
                        }
                    })
                    .map_err(CoreError::Io)?,
            );
        }

        if let Ok(mut l) = self.live.lock() {
            *l = Some(Live { stop, proc, threads });
        }
        Ok(())
    }

    /// Stop the live capture or file loading.
    pub fn stop(&self) {
        let live = self.live.lock().ok().and_then(|mut l| l.take());
        if let Some(live) = live {
            live.stop.store(true, Ordering::SeqCst);
            if let Ok(mut p) = live.proc.lock() {
                if let Some(mut c) = p.take() {
                    c.stop();
                }
            }
            for t in live.threads {
                let _ = t.join();
            }
            self.set_info(|i| {
                if i.state != "idle" {
                    i.state = "stopped";
                }
            });
        }
    }

    pub fn clear(&self) -> Result<()> {
        self.stop();
        let mut st = self.store.write().map_err(poisoned)?;
        let filter = st.filter.take();
        *st = Store::default();
        if let Some(f) = filter {
            st.view = Some(Vec::new());
            st.filter = Some(f);
        }
        drop(st);
        self.set_info(|i| {
            i.state = "idle";
            i.messages.clear();
            i.stop_reason.clear();
            i.started = None;
        });
        Ok(())
    }

    /// Load a pcap/pcapng file in the background.
    pub fn open_file(&self, path: &Path) -> Result<()> {
        self.stop();
        let mut reader = CaptureReader::open(path)?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        {
            let mut st = self.store.write().map_err(poisoned)?;
            let filter = st.filter.take();
            let mut fresh = Store::new(reader.interfaces.clone(), &name);
            if let Some(f) = filter {
                fresh.view = Some(Vec::new());
                fresh.filter = Some(f);
            }
            *st = fresh;
        }
        self.set_info(|i| {
            i.state = "loading";
            i.messages.clear();
            i.progress = 0.0;
            i.started = None;
            i.capture_filter.clear();
            i.stop_reason.clear();
        });
        let stop = Arc::new(AtomicBool::new(false));
        let store = self.store.clone();
        let info = self.info.clone();
        let stop2 = stop.clone();
        let t = std::thread::Builder::new()
            .name("netlens-load".into())
            .spawn(move || {
                let mut batch = Vec::with_capacity(2048);
                let mut done = false;
                let mut error = None;
                while !done && !stop2.load(Ordering::SeqCst) {
                    batch.clear();
                    while batch.len() < 2048 {
                        match reader.next_packet() {
                            Ok(Some(p)) => batch.push(p),
                            Ok(None) => {
                                done = true;
                                break;
                            }
                            Err(e) => {
                                error = Some(e.to_string());
                                done = true;
                                break;
                            }
                        }
                    }
                    if let Ok(mut st) = store.write() {
                        if st.interfaces.len() != reader.interfaces.len() {
                            st.interfaces = reader.interfaces.clone();
                        }
                        for p in batch.drain(..) {
                            let iface = p.iface.min(255) as u8;
                            st.add(iface, p.ts_ns, p.orig_len, p.outgoing, &p.data);
                        }
                        if st.bytes_stored > 4 * 1024 * 1024 * 1024 {
                            error = Some("File too large: stopped at 4 GB".into());
                            done = true;
                        }
                    }
                    if let Ok(mut i) = info.lock() {
                        i.progress = if reader.size > 0 { reader.read as f64 / reader.size as f64 } else { 0.0 };
                    }
                }
                if let Ok(mut st) = store.write() {
                    st.unsaved = false;
                }
                if let Ok(mut i) = info.lock() {
                    i.state = "stopped";
                    i.progress = 1.0;
                    if let Some(e) = error {
                        i.messages.push(e);
                    }
                }
            })
            .map_err(CoreError::Io)?;
        if let Ok(mut l) = self.live.lock() {
            *l = Some(Live {
                stop,
                proc: Arc::new(Mutex::new(None)),
                threads: vec![t],
            });
        }
        Ok(())
    }

    /// Write the capture (or only the displayed frames) as pcapng.
    pub fn save(&self, path: &Path, displayed_only: bool) -> Result<u64> {
        let st = self.store.read().map_err(poisoned)?;
        let mut w = PcapNgWriter::create(path, &st.interfaces)?;
        let mut n = 0u64;
        let mut write = |i: usize| -> Result<()> {
            let p = &st.packets[i];
            let out = match p.dir {
                1 => Some(false),
                2 => Some(true),
                _ => None,
            };
            w.write(p.iface as u32, p.ts_ns, p.orig_len, out, st.data(p))?;
            n += 1;
            Ok(())
        };
        match (&st.view, displayed_only) {
            (Some(v), true) => {
                for &i in v {
                    write(i as usize)?;
                }
            }
            _ => {
                for i in 0..st.packets.len() {
                    write(i)?;
                }
            }
        }
        w.finish()?;
        let all = !displayed_only || st.view.is_none();
        drop(st);
        if all {
            if let Ok(mut st) = self.store.write() {
                st.unsaved = false;
            }
        }
        Ok(n)
    }

    pub fn validate_filter(text: &str) -> std::result::Result<(), String> {
        if text.trim().is_empty() {
            return Ok(());
        }
        Filter::parse(text).map(|_| ())
    }

    /// Apply a display filter. The heavy pass runs under a read lock so the
    /// capture keeps flowing; frames added meanwhile are checked afterwards.
    pub fn set_filter(&self, text: &str) -> std::result::Result<u64, String> {
        if text.trim().is_empty() {
            let mut st = self.store.write().map_err(|e| e.to_string())?;
            st.set_filter(None);
            return Ok(st.packets.len() as u64);
        }
        let f = Filter::parse(text)?;
        let (mut view, upto) = {
            let st = self.store.read().map_err(|e| e.to_string())?;
            let n = st.packets.len();
            use rayon::prelude::*;
            let v: Vec<u32> = (0..n as u32).into_par_iter().filter(|&i| st.matches(&f, i)).collect();
            (v, n)
        };
        let mut st = self.store.write().map_err(|e| e.to_string())?;
        for i in upto..st.packets.len() {
            if st.matches(&f, i as u32) {
                view.push(i as u32);
            }
        }
        st.view = Some(view);
        st.filter = Some(f);
        Ok(st.shown() as u64)
    }

    pub fn status(&self) -> Status {
        let info = self.info.lock().map(|i| {
            (
                i.state,
                i.messages.clone(),
                i.started,
                i.capture_filter.clone(),
                i.progress,
                i.stop_reason.clone(),
            )
        });
        let (state, messages, started, cfilter, progress, reason) =
            info.unwrap_or(("idle", Vec::new(), None, String::new(), 0.0, String::new()));
        let Ok(st) = self.store.read() else {
            return Status {
                state,
                source: String::new(),
                interfaces: Vec::new(),
                capture_filter: cfilter,
                packets: 0,
                shown: 0,
                bytes: 0,
                memory: 0,
                dropped: 0,
                duration: 0.0,
                packets_per_sec: 0,
                bits_per_sec: 0,
                messages,
                display_filter: String::new(),
                unsaved: false,
                progress,
                stop_reason: reason,
            };
        };
        let duration = match (state, started) {
            ("capturing", Some(s)) => s.elapsed().as_secs_f64(),
            _ => match (st.packets.first(), st.packets.last()) {
                (Some(a), Some(b)) => b.ts_ns.saturating_sub(a.ts_ns) as f64 / 1e9,
                _ => 0.0,
            },
        };
        let last = if st.io.len() >= 2 { st.io[st.io.len() - 2] } else { st.io.last().copied().unwrap_or((0, 0)) };
        Status {
            state,
            source: st.source.clone(),
            interfaces: st.interfaces.iter().map(|i| i.name.clone()).collect(),
            capture_filter: cfilter,
            packets: st.packets.len() as u64,
            shown: st.shown() as u64,
            bytes: st.total_bytes,
            memory: st.memory(),
            dropped: st.dropped,
            duration,
            packets_per_sec: if state == "capturing" { last.0 as u64 } else { 0 },
            bits_per_sec: if state == "capturing" { last.1 * 8 } else { 0 },
            messages,
            display_filter: st.filter.as_ref().map(|f| f.text.clone()).unwrap_or_default(),
            unsaved: st.unsaved,
            progress,
            stop_reason: reason,
        }
    }

    pub fn with_store<T>(&self, f: impl FnOnce(&Store) -> T) -> Result<T> {
        let st = self.store.read().map_err(poisoned)?;
        Ok(f(&st))
    }

    /// Frame number -> position in the current view.
    pub fn locate(&self, no: u64) -> Option<usize> {
        let st = self.store.read().ok()?;
        st.position_of(no.checked_sub(1)? as usize)
    }

    fn cache_dir() -> PathBuf {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        let d = base.join("syspulse").join("netlens");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// Decode one frame with tshark (Wireshark's dissectors).
    pub fn wireshark_decode(&self, no: u64) -> Result<Vec<dissect::Node>> {
        let path = Self::cache_dir().join("frame.pcapng");
        {
            let st = self.store.read().map_err(poisoned)?;
            let i = no.checked_sub(1).ok_or(CoreError::Refused("bad frame".into()))? as usize;
            let p = st.packets.get(i).ok_or(CoreError::Refused("no such frame".into()))?;
            let mut w = PcapNgWriter::create(&path, &st.interfaces)?;
            w.write(p.iface as u32, p.ts_ns, p.orig_len, None, st.data(p))?;
            w.finish()?;
        }
        wireshark::decode(&path)
    }

    /// Save to a temporary file and open it in Wireshark.
    pub fn open_in_wireshark(&self, displayed_only: bool) -> Result<String> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = Self::cache_dir().join(format!("netlens-{ts}.pcapng"));
        // Keep the cache small: remove older exports.
        if let Ok(rd) = std::fs::read_dir(Self::cache_dir()) {
            for e in rd.filter_map(|e| e.ok()) {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with("netlens-") && n.ends_with(".pcapng") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        self.save_copy(&path, displayed_only)?;
        wireshark::open_gui(&path)?;
        Ok(path.to_string_lossy().to_string())
    }

    fn save_copy(&self, path: &Path, displayed_only: bool) -> Result<()> {
        let unsaved = self.store.read().map(|s| s.unsaved).unwrap_or(false);
        self.save(path, displayed_only)?;
        if let Ok(mut st) = self.store.write() {
            st.unsaved = unsaved;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an Ethernet/IPv4/TCP frame.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tcp_frame(src: [u8; 4], dst: [u8; 4], sp: u16, dp: u16, seq: u32, ack: u32, flags: u8, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0x08, 0x00];
        let tot = 20 + 20 + payload.len();
        f.extend_from_slice(&[0x45, 0, (tot >> 8) as u8, tot as u8, 0, 1, 0x40, 0, 64, 6, 0, 0]);
        f.extend_from_slice(&src);
        f.extend_from_slice(&dst);
        f.extend_from_slice(&sp.to_be_bytes());
        f.extend_from_slice(&dp.to_be_bytes());
        f.extend_from_slice(&seq.to_be_bytes());
        f.extend_from_slice(&ack.to_be_bytes());
        f.extend_from_slice(&[0x50, flags, 0xff, 0xff, 0, 0, 0, 0]);
        f.extend_from_slice(payload);
        f
    }

    pub(crate) fn udp_frame(src: [u8; 4], dst: [u8; 4], sp: u16, dp: u16, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0x08, 0x00];
        let tot = 20 + 8 + payload.len();
        f.extend_from_slice(&[0x45, 0, (tot >> 8) as u8, tot as u8, 0, 1, 0, 0, 64, 17, 0, 0]);
        f.extend_from_slice(&src);
        f.extend_from_slice(&dst);
        f.extend_from_slice(&sp.to_be_bytes());
        f.extend_from_slice(&dp.to_be_bytes());
        f.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
        f.extend_from_slice(&[0, 0]);
        f.extend_from_slice(payload);
        f
    }

    fn store() -> Store {
        let mut s = Store::new(vec![IfaceDesc { name: "eth0".into(), linktype: 1, snaplen: 65535 }], "test");
        s.local_ips.insert(IpAddr::from([10, 0, 0, 2]));
        s
    }

    const C: [u8; 4] = [10, 0, 0, 2];
    const S: [u8; 4] = [93, 184, 216, 34];
    const SYN: u8 = 0x02;
    const ACK: u8 = 0x10;
    const PSH: u8 = 0x08;

    #[test]
    fn http_session_analysis() {
        let mut st = store();
        let t = |ms: u64| 1_700_000_000_000_000_000 + ms * 1_000_000;
        let req = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\n";
        let resp = b"HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\nContent-Length: 5\r\n\r\nnope!";
        st.add(0, t(0), 0, None, &tcp_frame(C, S, 50000, 80, 1000, 0, SYN, &[]));
        st.add(0, t(10), 0, None, &tcp_frame(S, C, 80, 50000, 7000, 1001, SYN | ACK, &[]));
        st.add(0, t(11), 0, None, &tcp_frame(C, S, 50000, 80, 1001, 7001, ACK, &[]));
        st.add(0, t(12), 0, None, &tcp_frame(C, S, 50000, 80, 1001, 7001, ACK | PSH, req));
        // retransmission of the request 300 ms later
        st.add(0, t(312), 0, None, &tcp_frame(C, S, 50000, 80, 1001, 7001, ACK | PSH, req));
        st.add(0, t(340), 0, None, &tcp_frame(S, C, 80, 50000, 7001, 1001 + req.len() as u32, ACK | PSH, resp));

        assert_eq!(st.streams.len(), 1);
        assert_eq!(st.counters.retrans, 1);
        assert_eq!(st.counters.http_client_err, 1);
        assert_eq!(st.streams[0].host.as_deref(), Some("example.com"));
        assert_eq!(st.packets[4].analysis & dissect::net::an::RETRANSMISSION, dissect::net::an::RETRANSMISSION);
        assert!(st.packets[5].resp_us != store::NONE);

        let rows = st.rows(0, 10, false);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[3].proto, "HTTP");
        assert!(rows[3].info.starts_with("GET /index.html"), "{}", rows[3].info);
        assert!(rows[4].info.contains("[Retransmission]"));
        assert!(rows[0].info.contains("[SYN]"));
        assert!(rows[1].info.contains("Seq=0 Ack=1"), "{}", rows[1].info);

        let d = st.detail(5).unwrap();
        assert!(d.summary.contains("404"), "{}", d.summary);
        assert!(d.tree.iter().any(|n| n.field == "http"));

        st.set_filter(Some(Filter::parse("http.response.code == 404").unwrap()));
        assert_eq!(st.shown(), 1);
        st.set_filter(Some(Filter::parse("tcp.analysis.retransmission").unwrap()));
        assert_eq!(st.shown(), 1);
        st.set_filter(Some(Filter::parse("http.host contains \"example\" && ip.addr == 10.0.0.0/24").unwrap()));
        assert_eq!(st.shown(), 2);
        st.set_filter(None);

        let f = st.follow(0).unwrap();
        assert_eq!(f.chunks.len(), 2, "retransmitted bytes must not repeat");
        assert_eq!(f.bytes_a, req.len() as u64);
        assert_eq!(f.bytes_b, resp.len() as u64);

        let ins = st.insights();
        assert!(ins.iter().any(|i| i.id == "retrans"));
        let stats = st.stats();
        assert!(stats.hierarchy.iter().any(|h| h.path == "eth/ip/tcp/http"));
    }

    #[test]
    fn rtsp_registers_rtp() {
        let mut st = store();
        let cam = [192, 168, 1, 64];
        let t = |ms: u64| 1_700_000_000_000_000_000 + ms * 1_000_000;
        let describe_resp = b"RTSP/1.0 200 OK\r\nCSeq: 2\r\nContent-Type: application/sdp\r\nContent-Length: 60\r\n\r\nv=0\r\nm=video 0 RTP/AVP 96\r\na=rtpmap:96 H264/90000\r\na=control:trackID=1\r\n";
        let setup = b"SETUP rtsp://192.168.1.64/stream1/trackID=1 RTSP/1.0\r\nCSeq: 3\r\nTransport: RTP/AVP;unicast;client_port=50000-50001\r\n\r\n";
        let setup_resp = b"RTSP/1.0 200 OK\r\nCSeq: 3\r\nSession: 1234;timeout=60\r\nTransport: RTP/AVP;unicast;client_port=50000-50001;server_port=6970-6971\r\n\r\n";
        st.add(0, t(0), 0, None, &tcp_frame(cam, C, 554, 40000, 1, 1, ACK | PSH, describe_resp));
        st.add(0, t(5), 0, None, &tcp_frame(C, cam, 40000, 554, 1, 200, ACK | PSH, setup));
        st.add(0, t(9), 0, None, &tcp_frame(cam, C, 554, 40000, 200, 300, ACK | PSH, setup_resp));
        assert_eq!(st.shared.rtpmap.get(&96).map(|x| x.0.as_str()), Some("H264"));
        assert_eq!(st.shared.rtp_ports.get(&(IpAddr::from(C), 50000)), Some(&false));
        assert!(st.packets[2].resp_us != store::NONE, "SETUP response time");

        // RTP: IDR FU-A start, then a gap of one packet.
        let rtp = |seq: u16, ts: u32| {
            let mut p = vec![0x80, 96, (seq >> 8) as u8, seq as u8];
            p.extend_from_slice(&ts.to_be_bytes());
            p.extend_from_slice(&0xdeadbeefu32.to_be_bytes());
            p.extend_from_slice(&[0x7c, 0x85, 1, 2, 3]);
            p
        };
        st.add(0, t(20), 0, None, &udp_frame(cam, C, 6970, 50000, &rtp(100, 9000)));
        st.add(0, t(53), 0, None, &udp_frame(cam, C, 6970, 50000, &rtp(101, 12000)));
        st.add(0, t(120), 0, None, &udp_frame(cam, C, 6970, 50000, &rtp(103, 18000)));
        let rows = st.rows(3, 3, false);
        assert_eq!(rows[0].proto, "RTP");
        assert!(rows[0].info.contains("PT=H264"), "{}", rows[0].info);
        assert!(rows[0].info.contains("IDR"), "{}", rows[0].info);
        let streams = st.rtp_streams();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].lost, 1);
        assert_eq!(streams[0].codec, "H264");
        assert_eq!(streams[0].key_frames, 3);
        let rows = st.rows(0, 1, false);
        assert_eq!(rows[0].proto, "RTSP/SDP");
    }

    #[test]
    fn dns_and_names() {
        let mut st = store();
        let dns_server = [8, 8, 8, 8];
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(b"\x06github\x03com\x00\x00\x01\x00\x01");
        let mut r = vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0];
        r.extend_from_slice(b"\x06github\x03com\x00\x00\x01\x00\x01");
        r.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 140, 82, 112, 3]);
        st.add(0, 1_000_000_000, 0, None, &udp_frame(C, dns_server, 53000, 53, &q));
        st.add(0, 1_020_000_000, 0, None, &udp_frame(dns_server, C, 53, 53000, &r));
        assert_eq!(st.hostnames.get(&IpAddr::from([140, 82, 112, 3])).map(|s| s.as_str()), Some("github.com"));
        let rows = st.rows(0, 2, false);
        assert!(rows[1].info.contains("A 140.82.112.3"), "{}", rows[1].info);
        let d = st.detail(1).unwrap();
        assert!(d.summary.contains("github.com is 140.82.112.3"), "{}", d.summary);
        assert!(d.summary.contains("20.0 ms"), "{}", d.summary);
        st.set_filter(Some(Filter::parse("dns.time > 0.01").unwrap()));
        assert_eq!(st.shown(), 1);
        st.set_filter(Some(Filter::parse("dns.a == 140.82.112.3").unwrap()));
        assert_eq!(st.shown(), 1);
    }

    #[test]
    fn tls_client_hello_sni() {
        // Minimal ClientHello with SNI "example.org".
        let name = b"example.org";
        let mut sni = Vec::new();
        sni.extend_from_slice(&((name.len() + 3) as u16).to_be_bytes());
        sni.push(0);
        sni.extend_from_slice(&(name.len() as u16).to_be_bytes());
        sni.extend_from_slice(name);
        let mut ext = Vec::new();
        ext.extend_from_slice(&0u16.to_be_bytes());
        ext.extend_from_slice(&(sni.len() as u16).to_be_bytes());
        ext.extend_from_slice(&sni);
        let mut body = vec![3, 3];
        body.extend_from_slice(&[0u8; 32]);
        body.push(0); // session id
        body.extend_from_slice(&[0, 2, 0x13, 0x01]); // one cipher
        body.extend_from_slice(&[1, 0]); // compression
        body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        body.extend_from_slice(&ext);
        let mut hs = vec![1];
        hs.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        hs.extend_from_slice(&body);
        let mut rec = vec![22, 3, 1];
        rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
        rec.extend_from_slice(&hs);
        let mut st = store();
        st.add(0, 5, 0, None, &tcp_frame(C, S, 51000, 443, 1, 1, ACK | PSH, &rec));
        let rows = st.rows(0, 1, false);
        assert!(rows[0].proto.starts_with("TLS"), "{}", rows[0].proto);
        assert!(rows[0].info.contains("Client Hello (SNI=example.org)"), "{}", rows[0].info);
        assert_eq!(st.streams[0].host.as_deref(), Some("example.org"));
        st.set_filter(Some(Filter::parse("tls.handshake.extensions_server_name == \"example.org\"").unwrap()));
        assert_eq!(st.shown(), 1);
    }

    #[test]
    fn fuzz_never_panics() {
        // Random bytes through every link type must not panic.
        let mut x: u64 = 0x9e3779b97f4a7c15;
        let mut st = store();
        for n in 0..3000 {
            let len = (n % 200) + 1;
            let mut v = Vec::with_capacity(len);
            for _ in 0..len {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                v.push(x as u8);
            }
            // Make many of them look like IPv4/TCP/UDP to reach deeper layers.
            if n % 3 == 0 && v.len() > 40 {
                v[12] = 0x08;
                v[13] = 0x00;
                v[14] = 0x45;
                v[23] = if n % 2 == 0 { 6 } else { 17 };
            }
            for lt in [1u16, 101, 113, 276, 0] {
                let meta = dissect::Meta::default();
                let _ = dissect::dissect(&v, lt, dissect::Want { tree: true, fields: true, info: true }, &meta, &st.shared);
            }
            st.add(0, n as u64, 0, None, &v);
        }
        let _ = st.rows(0, 500, true);
        for i in 0..50 {
            let _ = st.detail(i);
        }
        let _ = st.stats();
        let _ = st.conversations();
        for s in 0..st.streams.len().min(50) {
            let _ = st.follow(s as u32);
        }
    }

    #[test]
    fn time_format() {
        let s = fmt_time(1_700_000_000_123_456_789);
        assert!(s.ends_with(".123456789"), "{s}");
        assert_eq!(s.len(), 29);
    }
}
