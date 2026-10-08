// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/forge/mod.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: PacketForge - a TCP / UDP / serial terminal for working with
//           devices and protocols (Hercules / Packet Sender style).
//
//           Every open session is driven by one control thread that handles
//           commands (send, repeat, send file, modem lines, logging) and owns
//           the link; links spawn their own reader threads. All traffic goes
//           through a shared `Ctx` that emits events and writes the log file,
//           so the UI and the log always see the same bytes.
// =============================================================================

pub mod encode;
mod links;
pub mod serial;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

// ---------------------------------------------------------------- config ---

fn default_true() -> bool {
    true
}
fn default_max_clients() -> u16 {
    32
}
fn default_bind() -> String {
    "0.0.0.0".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionConfig {
    #[serde(rename_all = "camelCase")]
    TcpClient {
        host: String,
        port: u16,
        #[serde(default)]
        auto_reconnect: bool,
    },
    #[serde(rename_all = "camelCase")]
    TcpServer {
        #[serde(default = "default_bind")]
        bind: String,
        port: u16,
        #[serde(default = "default_max_clients")]
        max_clients: u16,
    },
    #[serde(rename_all = "camelCase")]
    Udp {
        #[serde(default = "default_bind")]
        bind: String,
        local_port: u16,
        remote_host: Option<String>,
        remote_port: Option<u16>,
        #[serde(default)]
        broadcast: bool,
        multicast_group: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Serial {
        path: String,
        baud: u32,
        #[serde(default = "eight")]
        data_bits: u8,
        /// none | odd | even
        #[serde(default = "none_str")]
        parity: String,
        #[serde(default = "one")]
        stop_bits: u8,
        /// none | hardware | software
        #[serde(default = "none_str")]
        flow: String,
        #[serde(default = "default_true")]
        dtr: bool,
        #[serde(default)]
        rts: bool,
    },
}

fn eight() -> u8 {
    8
}
fn one() -> u8 {
    1
}
fn none_str() -> String {
    "none".into()
}

impl SessionConfig {
    pub fn title(&self) -> String {
        match self {
            SessionConfig::TcpClient { host, port, .. } => format!("TCP → {host}:{port}"),
            SessionConfig::TcpServer { bind, port, .. } => format!("TCP server {bind}:{port}"),
            SessionConfig::Udp {
                local_port,
                remote_host,
                remote_port,
                ..
            } => match (remote_host, remote_port) {
                (Some(h), Some(p)) if !h.is_empty() => format!("UDP :{local_port} → {h}:{p}"),
                _ => format!("UDP :{local_port}"),
            },
            SessionConfig::Serial {
                path,
                baud,
                data_bits,
                parity,
                stop_bits,
                ..
            } => {
                format!(
                    "{path} {baud} {data_bits}{}{stop_bits}",
                    parity.chars().next().unwrap_or('n').to_ascii_uppercase()
                )
            }
        }
    }
}

// ---------------------------------------------------------------- events ---

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Rx,
    Tx,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ForgeEvent {
    /// Bytes received or sent. `data` is base64; `t` is Unix time in ms.
    #[serde(rename_all = "camelCase")]
    Data {
        session: u32,
        dir: Dir,
        peer: Option<String>,
        t: f64,
        data: String,
    },
    /// connecting | open | listening | closed | error
    #[serde(rename_all = "camelCase")]
    State {
        session: u32,
        state: &'static str,
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    Clients { session: u32, clients: Vec<String> },
    /// Serial modem-control lines.
    #[serde(rename_all = "camelCase")]
    Lines {
        session: u32,
        cts: bool,
        dsr: bool,
        dcd: bool,
        ri: bool,
        dtr: bool,
        rts: bool,
    },
    /// Repeat / file-send progress.
    #[serde(rename_all = "camelCase")]
    Job {
        session: u32,
        kind: &'static str,
        active: bool,
        sent: u64,
        total: Option<u64>,
    },
}

pub type EventSink = Arc<dyn Fn(ForgeEvent) + Send + Sync>;

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// ISO-8601 UTC timestamp with milliseconds (no external date crate).
pub fn iso_utc(ms: f64) -> String {
    let total = (ms / 1000.0).floor() as i64;
    let millis = (ms - (total as f64) * 1000.0).round().clamp(0.0, 999.0) as i64;
    let (days, secs) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    // Civil from days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// Per-session shared context: event emission, logging and counters.
pub struct Ctx {
    pub id: u32,
    sink: EventSink,
    log: Mutex<Option<BufWriter<File>>>,
    pub rx_bytes: AtomicU64,
    pub tx_bytes: AtomicU64,
}

impl Ctx {
    pub fn data(&self, dir: Dir, peer: Option<String>, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let t = now_ms();
        match dir {
            Dir::Rx => self
                .rx_bytes
                .fetch_add(bytes.len() as u64, Ordering::Relaxed),
            Dir::Tx => self
                .tx_bytes
                .fetch_add(bytes.len() as u64, Ordering::Relaxed),
        };
        if let Ok(mut log) = self.log.lock() {
            if let Some(w) = log.as_mut() {
                let _ = writeln!(
                    w,
                    "{} {} {}[{}] {}  |{}|",
                    iso_utc(t),
                    if dir == Dir::Rx { "RX" } else { "TX" },
                    peer.as_deref().map(|p| format!("{p} ")).unwrap_or_default(),
                    bytes.len(),
                    encode::to_hex(bytes),
                    encode::to_escaped(bytes)
                );
            }
        }
        (self.sink)(ForgeEvent::Data {
            session: self.id,
            dir,
            peer,
            t,
            data: crate::util::base64(bytes),
        });
    }

    pub fn state(&self, state: &'static str, message: impl Into<String>) {
        let message = message.into();
        if let Ok(mut log) = self.log.lock() {
            if let Some(w) = log.as_mut() {
                let _ = writeln!(w, "{} -- {state}: {message}", iso_utc(now_ms()));
                let _ = w.flush();
            }
        }
        (self.sink)(ForgeEvent::State {
            session: self.id,
            state,
            message,
        });
    }

    pub fn emit(&self, e: ForgeEvent) {
        (self.sink)(e);
    }

    fn set_log(&self, path: Option<&str>) -> Result<()> {
        let mut log = self
            .log
            .lock()
            .map_err(|_| CoreError::Other("log lock poisoned".into()))?;
        if let Some(mut w) = log.take() {
            let _ = w.flush();
        }
        if let Some(p) = path {
            let f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)?;
            *log = Some(BufWriter::new(f));
        }
        Ok(())
    }

    fn flush_log(&self) {
        if let Ok(mut log) = self.log.lock() {
            if let Some(w) = log.as_mut() {
                let _ = w.flush();
            }
        }
    }
}

// --------------------------------------------------------------- commands ---

#[derive(Debug)]
pub enum Command {
    Send {
        bytes: Vec<u8>,
        target: Option<String>,
    },
    Repeat {
        bytes: Vec<u8>,
        target: Option<String>,
        interval_ms: u64,
        count: Option<u64>,
    },
    SendFile {
        data: Vec<u8>,
        target: Option<String>,
        chunk: usize,
        delay_ms: u64,
    },
    StopJobs,
    SetLines {
        dtr: Option<bool>,
        rts: Option<bool>,
    },
    Break {
        ms: u64,
    },
    Log {
        path: Option<String>,
    },
    Close,
}

/// A transport. Readers are spawned by the implementation and report through `Ctx`.
pub(crate) trait Link: Send {
    fn send(&mut self, data: &[u8], target: Option<&str>) -> Result<()>;
    /// Called every few ms by the control thread (reconnects, modem lines...).
    fn tick(&mut self) {}
    fn set_lines(&mut self, _dtr: Option<bool>, _rts: Option<bool>) -> Result<()> {
        Err(CoreError::Unsupported("modem lines on this session type"))
    }
    fn send_break(&mut self, _ms: u64) -> Result<()> {
        Err(CoreError::Unsupported("break on this session type"))
    }
    fn close(&mut self);
}

struct RepeatJob {
    bytes: Vec<u8>,
    target: Option<String>,
    interval: Duration,
    remaining: Option<u64>,
    next: Instant,
    sent: u64,
}

struct FileJob {
    data: Vec<u8>,
    target: Option<String>,
    pos: usize,
    chunk: usize,
    delay: Duration,
    next: Instant,
}

fn control_loop(ctx: Arc<Ctx>, mut link: Box<dyn Link>, rx: Receiver<Command>) {
    let mut repeat: Option<RepeatJob> = None;
    let mut file: Option<FileJob> = None;
    let mut last_job_event = Instant::now();
    loop {
        let wait = [
            repeat.as_ref().map(|r| r.next),
            file.as_ref().map(|f| f.next),
        ]
        .into_iter()
        .flatten()
        .min()
        .map(|t| t.saturating_duration_since(Instant::now()))
        .unwrap_or(Duration::from_millis(50))
        .min(Duration::from_millis(50));
        match rx.recv_timeout(wait) {
            Ok(Command::Send { bytes, target }) => match link.send(&bytes, target.as_deref()) {
                Ok(()) => ctx.data(Dir::Tx, target, &bytes),
                Err(e) => ctx.state("error", format!("send failed: {e}")),
            },
            Ok(Command::Repeat {
                bytes,
                target,
                interval_ms,
                count,
            }) => {
                repeat = Some(RepeatJob {
                    bytes,
                    target,
                    interval: Duration::from_millis(interval_ms.max(1)),
                    remaining: count,
                    next: Instant::now(),
                    sent: 0,
                });
            }
            Ok(Command::SendFile {
                data,
                target,
                chunk,
                delay_ms,
            }) => {
                let total = data.len() as u64;
                file = Some(FileJob {
                    data,
                    target,
                    pos: 0,
                    chunk: chunk.clamp(1, 65_536),
                    delay: Duration::from_millis(delay_ms),
                    next: Instant::now(),
                });
                ctx.emit(ForgeEvent::Job {
                    session: ctx.id,
                    kind: "file",
                    active: true,
                    sent: 0,
                    total: Some(total),
                });
            }
            Ok(Command::StopJobs) => {
                if let Some(r) = repeat.take() {
                    ctx.emit(ForgeEvent::Job {
                        session: ctx.id,
                        kind: "repeat",
                        active: false,
                        sent: r.sent,
                        total: None,
                    });
                }
                if let Some(f) = file.take() {
                    ctx.emit(ForgeEvent::Job {
                        session: ctx.id,
                        kind: "file",
                        active: false,
                        sent: f.pos as u64,
                        total: Some(f.data.len() as u64),
                    });
                }
            }
            Ok(Command::SetLines { dtr, rts }) => {
                if let Err(e) = link.set_lines(dtr, rts) {
                    ctx.state("error", e.to_string());
                }
            }
            Ok(Command::Break { ms }) => {
                if let Err(e) = link.send_break(ms) {
                    ctx.state("error", e.to_string());
                }
            }
            Ok(Command::Log { path }) => {
                if let Err(e) = ctx.set_log(path.as_deref()) {
                    ctx.state("error", format!("cannot open log: {e}"));
                }
            }
            Ok(Command::Close) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        let now = Instant::now();
        if let Some(r) = repeat.as_mut() {
            if now >= r.next {
                match link.send(&r.bytes, r.target.as_deref()) {
                    Ok(()) => {
                        ctx.data(Dir::Tx, r.target.clone(), &r.bytes);
                        r.sent += 1;
                    }
                    Err(e) => {
                        ctx.state("error", format!("repeat stopped: {e}"));
                        r.remaining = Some(0);
                    }
                }
                r.remaining = r.remaining.map(|n| n.saturating_sub(1));
                r.next += r.interval;
                if r.next < now {
                    r.next = now + r.interval; // fell behind: do not burst
                }
                let done = r.remaining == Some(0);
                if done || last_job_event.elapsed() > Duration::from_millis(250) {
                    ctx.emit(ForgeEvent::Job {
                        session: ctx.id,
                        kind: "repeat",
                        active: !done,
                        sent: r.sent,
                        total: None,
                    });
                    last_job_event = Instant::now();
                }
                if done {
                    repeat = None;
                }
            }
        }
        if let Some(f) = file.as_mut() {
            if now >= f.next {
                let end = (f.pos + f.chunk).min(f.data.len());
                let part = f.data[f.pos..end].to_vec();
                match link.send(&part, f.target.as_deref()) {
                    Ok(()) => {
                        ctx.data(Dir::Tx, f.target.clone(), &part);
                        f.pos = end;
                    }
                    Err(e) => {
                        ctx.state("error", format!("file send stopped: {e}"));
                        f.pos = f.data.len();
                    }
                }
                f.next = now + f.delay;
                let done = f.pos >= f.data.len();
                if done || last_job_event.elapsed() > Duration::from_millis(250) {
                    ctx.emit(ForgeEvent::Job {
                        session: ctx.id,
                        kind: "file",
                        active: !done,
                        sent: f.pos as u64,
                        total: Some(f.data.len() as u64),
                    });
                    last_job_event = Instant::now();
                }
                if done {
                    file = None;
                }
            }
        }
        link.tick();
        ctx.flush_log();
    }
    link.close();
    let _ = ctx.set_log(None);
    ctx.state("closed", "session closed");
}

// ---------------------------------------------------------------- manager ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: u32,
    pub title: String,
    pub config: SessionConfig,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

struct Handle {
    tx: Sender<Command>,
    config: SessionConfig,
    ctx: Arc<Ctx>,
    thread: Option<JoinHandle<()>>,
}

/// Owns every open PacketForge session.
pub struct Forge {
    sessions: Mutex<HashMap<u32, Handle>>,
    next: AtomicU32,
    sink: EventSink,
}

pub const MAX_SESSIONS: usize = 32;

impl Forge {
    pub fn new(sink: EventSink) -> Self {
        Forge {
            sessions: Mutex::new(HashMap::new()),
            next: AtomicU32::new(1),
            sink,
        }
    }

    /// Open a session. Connection errors are returned immediately.
    pub fn open(&self, config: SessionConfig) -> Result<SessionInfo> {
        if self.sessions.lock().map(|s| s.len()).unwrap_or(0) >= MAX_SESSIONS {
            return Err(CoreError::Refused(format!(
                "at most {MAX_SESSIONS} sessions can be open"
            )));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let ctx = Arc::new(Ctx {
            id,
            sink: self.sink.clone(),
            log: Mutex::new(None),
            rx_bytes: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
        });
        let link = links::open(&config, ctx.clone())?;
        let (tx, rx) = mpsc::channel();
        let c2 = ctx.clone();
        let thread = std::thread::Builder::new()
            .name(format!("forge-{id}"))
            .spawn(move || control_loop(c2, link, rx))?;
        let info = SessionInfo {
            id,
            title: config.title(),
            config: config.clone(),
            rx_bytes: 0,
            tx_bytes: 0,
        };
        self.sessions
            .lock()
            .map_err(|_| CoreError::Other("session table poisoned".into()))?
            .insert(
                id,
                Handle {
                    tx,
                    config,
                    ctx,
                    thread: Some(thread),
                },
            );
        Ok(info)
    }

    pub fn command(&self, id: u32, cmd: Command) -> Result<()> {
        let s = self
            .sessions
            .lock()
            .map_err(|_| CoreError::Other("session table poisoned".into()))?;
        let h = s
            .get(&id)
            .ok_or_else(|| CoreError::Other(format!("session {id} is not open")))?;
        h.tx.send(cmd)
            .map_err(|_| CoreError::Other("session has stopped".into()))
    }

    pub fn close(&self, id: u32) {
        let h = self.sessions.lock().ok().and_then(|mut s| s.remove(&id));
        if let Some(mut h) = h {
            let _ = h.tx.send(Command::Close);
            if let Some(t) = h.thread.take() {
                let _ = t.join();
            }
        }
    }

    pub fn close_all(&self) {
        let ids: Vec<u32> = self
            .sessions
            .lock()
            .map(|s| s.keys().copied().collect())
            .unwrap_or_default();
        for id in ids {
            self.close(id);
        }
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        let s = match self.sessions.lock() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let mut v: Vec<SessionInfo> = s
            .iter()
            .map(|(id, h)| SessionInfo {
                id: *id,
                title: h.config.title(),
                config: h.config.clone(),
                rx_bytes: h.ctx.rx_bytes.load(Ordering::Relaxed),
                tx_bytes: h.ctx.tx_bytes.load(Ordering::Relaxed),
            })
            .collect();
        v.sort_by_key(|i| i.id);
        v
    }
}

impl Drop for Forge {
    fn drop(&mut self) {
        self.close_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact JSON shapes ui/src/lib/forge.svelte.ts sends.
    #[test]
    fn ui_contract() {
        let cfgs = [
            r#"{"kind":"tcpClient","host":"10.0.0.5","port":502,"autoReconnect":true}"#,
            r#"{"kind":"tcpServer","bind":"0.0.0.0","port":5000,"maxClients":32}"#,
            r#"{"kind":"udp","bind":"0.0.0.0","localPort":5000,"remoteHost":null,"remotePort":null,"broadcast":false,"multicastGroup":null}"#,
            r#"{"kind":"serial","path":"/dev/ttyUSB0","baud":115200,"dataBits":8,"parity":"even","stopBits":1,"flow":"hardware","dtr":true,"rts":false}"#,
        ];
        for c in cfgs {
            serde_json::from_str::<SessionConfig>(c).unwrap_or_else(|e| panic!("{c}: {e}"));
        }
        for cs in [
            "none",
            "crc16-modbus",
            "crc16-ccitt",
            "crc32",
            "xor8",
            "sum8",
            "lrc",
        ] {
            let j = format!(
                r#"{{"mode":"hex","data":"01 02","lineEnding":"crlf","checksum":"{cs}","hercules":false}}"#
            );
            serde_json::from_str::<encode::SendSpec>(&j).unwrap_or_else(|e| panic!("{cs}: {e}"));
        }
        let e = serde_json::to_value(ForgeEvent::Data {
            session: 1,
            dir: Dir::Rx,
            peer: None,
            t: 1.0,
            data: "QQ==".into(),
        })
        .unwrap();
        assert_eq!(e["type"], "data");
        assert_eq!(e["dir"], "rx");
        let j = serde_json::to_value(ForgeEvent::Job {
            session: 1,
            kind: "repeat",
            active: true,
            sent: 2,
            total: None,
        })
        .unwrap();
        assert_eq!(j["type"], "job");
    }

    #[test]
    fn iso_timestamps() {
        assert_eq!(iso_utc(0.0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_utc(1_760_425_792_640.0), "2025-10-14T07:09:52.640Z");
    }
}
