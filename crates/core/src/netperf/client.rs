// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netperf/client.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Speed-test client: LAN discovery of SysPulse servers and running
//           TCP / UDP / latency tests against a server, with live interval
//           progress and a detailed final result.
// =============================================================================

use std::io::{BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredServer {
    pub address: String,
    pub host: String,
    pub port: u16,
    pub code_required: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfProgress {
    /// upload | download | latency
    pub dir: &'static str,
    pub index: u32,
    pub t: f64,
    pub mbps: f64,
    pub bytes: u64,
    pub retransmits: Option<u64>,
    pub rtt_ms: Option<f64>,
    pub lost: Option<u64>,
    pub jitter_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DirectionResult {
    pub direction: String,
    pub bytes: u64,
    pub seconds: f64,
    pub mbps_avg: f64,
    pub mbps_min: f64,
    pub mbps_max: f64,
    pub mbps_stddev: f64,
    pub intervals: Vec<IntervalStat>,
    pub retransmits: Option<u64>,
    pub rtt_ms: Option<f64>,
    pub packets_sent: Option<u64>,
    pub packets_received: Option<u64>,
    pub lost: Option<u64>,
    pub loss_percent: Option<f64>,
    pub out_of_order: Option<u64>,
    pub jitter_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LatencyResult {
    pub sent: u64,
    pub received: u64,
    pub loss_percent: f64,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub max_ms: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    pub stddev_ms: f64,
    /// Mean difference between consecutive RTTs.
    pub jitter_ms: f64,
    pub samples: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfResult {
    pub server: String,
    pub server_host: String,
    pub spec: TestSpec,
    pub started_at: u64,
    pub upload: Option<DirectionResult>,
    pub download: Option<DirectionResult>,
    pub latency: Option<LatencyResult>,
    pub cancelled: bool,
}

// ------------------------------------------------------------ discovery ---

fn broadcast_addresses() -> Vec<Ipv4Addr> {
    let mut v = vec![Ipv4Addr::BROADCAST];
    let nets = sysinfo::Networks::new_with_refreshed_list();
    for d in nets.list().values() {
        for n in d.ip_networks() {
            if let IpAddr::V4(a) = n.addr {
                if a.is_loopback() || n.prefix == 0 || n.prefix >= 32 {
                    continue;
                }
                let mask = u32::MAX << (32 - u32::from(n.prefix));
                v.push(Ipv4Addr::from(u32::from(a) | !mask));
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

/// Find SysPulse servers on the local network (UDP broadcast).
pub fn discover(port: u16, wait: Duration) -> Vec<DiscoveredServer> {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return Vec::new();
    };
    let _ = sock.set_broadcast(true);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(100)));
    let mut probe = vec![PKT_DISCOVER];
    probe.extend_from_slice(DISCOVER_MAGIC);
    for b in broadcast_addresses() {
        let _ = sock.send_to(&probe, (b, port));
    }
    let mut found: Vec<DiscoveredServer> = Vec::new();
    let mut buf = [0u8; 2048];
    let end = Instant::now() + wait;
    while Instant::now() < end {
        let Ok((n, src)) = sock.recv_from(&mut buf) else {
            continue;
        };
        if n < 2 || buf[0] != PKT_DISCOVER_REPLY {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf[1..n]) else {
            continue;
        };
        let addr = src.ip().to_string();
        if found.iter().any(|f| f.address == addr) {
            continue;
        }
        found.push(DiscoveredServer {
            address: addr,
            host: v
                .get("host")
                .and_then(|h| h.as_str())
                .unwrap_or("")
                .to_string(),
            port: v
                .get("port")
                .and_then(|p| p.as_u64())
                .unwrap_or(u64::from(port)) as u16,
            code_required: v
                .get("codeRequired")
                .and_then(|c| c.as_bool())
                .unwrap_or(false),
        });
    }
    found
}

// ---------------------------------------------------------------- client ---

fn hello(
    role: Role,
    session: Option<u64>,
    stream: Option<u8>,
    dir: Option<Direction>,
    code: &Option<String>,
) -> Msg {
    Msg::Hello {
        protocol: PROTOCOL.into(),
        role,
        session,
        stream,
        dir,
        code: code.clone(),
        host: Some(crate::netconfig::hostname()),
    }
}

fn summarize(
    direction: &str,
    intervals: Vec<IntervalStat>,
    bytes: u64,
    seconds: f64,
) -> DirectionResult {
    let rates: Vec<f64> = intervals.iter().map(|i| i.mbps).collect();
    let avg_iv = if rates.is_empty() {
        0.0
    } else {
        rates.iter().sum::<f64>() / rates.len() as f64
    };
    let var = if rates.len() > 1 {
        rates.iter().map(|r| (r - avg_iv).powi(2)).sum::<f64>() / (rates.len() - 1) as f64
    } else {
        0.0
    };
    DirectionResult {
        direction: direction.into(),
        bytes,
        seconds,
        mbps_avg: mbps(bytes, seconds),
        mbps_min: if rates.is_empty() {
            0.0
        } else {
            rates.iter().copied().fold(f64::INFINITY, f64::min)
        },
        mbps_max: rates.iter().copied().fold(0.0, f64::max),
        mbps_stddev: var.sqrt(),
        intervals,
        ..Default::default()
    }
}

struct Control {
    w: TcpStream,
    rx: Receiver<Msg>,
    session: u64,
    udp_port: u16,
    host: String,
    addr: SocketAddr,
}

fn connect_control(
    target: &str,
    port: u16,
    code: &Option<String>,
    spec: &TestSpec,
) -> Result<Control> {
    let addr = (target, port)
        .to_socket_addrs()
        .map_err(|e| CoreError::Other(format!("{target}: {e}")))?
        .next()
        .ok_or_else(|| CoreError::Other(format!("{target}: no address")))?;
    let mut w = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| {
        CoreError::Other(format!(
            "cannot reach {addr}: {e}. Is Server mode running on that PC, and is port {port} allowed by its firewall?"
        ))
    })?;
    w.set_nodelay(true)?;
    let mut reader = BufReader::new(w.try_clone()?);
    reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(10)))?;
    write_msg(&mut w, &hello(Role::Control, None, None, None, code))?;
    write_msg(&mut w, &Msg::Start { spec: spec.clone() })?;
    let (session, udp_port, host) = match read_msg(&mut reader) {
        Ok(Msg::Ready {
            session,
            udp_port,
            host,
        }) => (session, udp_port, host),
        Ok(Msg::Error { message }) => return Err(CoreError::Other(format!("server: {message}"))),
        Ok(_) => return Err(CoreError::Other("unexpected reply from server".into())),
        Err(e) => return Err(CoreError::Other(format!("no reply from server: {e}"))),
    };
    reader.get_ref().set_read_timeout(Some(Duration::from_secs(
        u64::from(spec.duration_secs) + 40,
    )))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(m) = read_msg(&mut reader) {
            if tx.send(m).is_err() {
                break;
            }
        }
    });
    Ok(Control {
        w,
        rx,
        session,
        udp_port,
        host,
        addr,
    })
}

fn finish(
    ctl: &mut Control,
    sent_packets: Option<u64>,
    sent_bytes: Option<u64>,
    late: &mut Vec<(Direction, IntervalStat)>,
) -> (Option<ReceiveStats>, Option<SendStats>) {
    let _ = write_msg(
        &mut ctl.w,
        &Msg::Done {
            sent_packets,
            sent_bytes,
        },
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        match ctl.rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Msg::Result {
                upload,
                download_sent,
            }) => return (upload, download_sent),
            Ok(Msg::Interval { dir, stat }) => late.push((dir, stat)),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
    }
    (None, None)
}

/// Run one test against `target` (IP or host name).
pub fn run_test(
    target: &str,
    port: u16,
    code: Option<String>,
    spec: &TestSpec,
    cancel: &AtomicBool,
    on_progress: impl Fn(PerfProgress),
) -> Result<PerfResult> {
    spec.validate()?;
    let mut ctl = connect_control(target, port, &code, spec)?;
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut result = PerfResult {
        server: ctl.addr.to_string(),
        server_host: ctl.host.clone(),
        spec: spec.clone(),
        started_at,
        upload: None,
        download: None,
        latency: None,
        cancelled: false,
    };
    match spec.kind {
        TestKind::Tcp => run_tcp(&mut ctl, &code, spec, cancel, &on_progress, &mut result)?,
        TestKind::Udp => run_udp(&mut ctl, spec, cancel, &on_progress, &mut result)?,
        TestKind::Latency => run_latency(&mut ctl, spec, cancel, &on_progress, &mut result)?,
    }
    result.cancelled = cancel.load(Ordering::Relaxed);
    Ok(result)
}

fn open_data(
    ctl: &Control,
    code: &Option<String>,
    stream: u8,
    dir: Direction,
) -> Result<TcpStream> {
    let mut s = TcpStream::connect_timeout(&ctl.addr, Duration::from_secs(5))?;
    write_msg(
        &mut s,
        &hello(Role::Data, Some(ctl.session), Some(stream), Some(dir), code),
    )?;
    Ok(s)
}

fn run_tcp(
    ctl: &mut Control,
    code: &Option<String>,
    spec: &TestSpec,
    cancel: &AtomicBool,
    on_progress: &impl Fn(PerfProgress),
    out: &mut PerfResult,
) -> Result<()> {
    let start = Instant::now();
    let deadline = start + Duration::from_secs(u64::from(spec.duration_secs));
    let iv = Duration::from_millis(u64::from(spec.interval_ms));
    let stop = Arc::new(AtomicBool::new(false));

    // Upload senders.
    let up_socks: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
    let mut senders = Vec::new();
    if spec.direction.up() {
        for i in 0..spec.streams {
            let mut s = open_data(ctl, code, i, Direction::Upload)?;
            if let Ok(c) = s.try_clone() {
                up_socks.lock().unwrap().push(c);
            }
            let stop = stop.clone();
            senders.push(std::thread::spawn(move || {
                let buf: Vec<u8> = (0..TCP_BUF).map(|i| (i % 253) as u8).collect();
                let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
                while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
                    if s.write(&buf).is_err() {
                        break;
                    }
                }
                let _ = s.shutdown(std::net::Shutdown::Write);
                // Drain until the server closes, so close() does not reset unread data.
                let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
                let mut sink = [0u8; 1024];
                while matches!(s.read(&mut sink), Ok(n) if n > 0) {}
            }));
        }
    }
    // Download receivers.
    let down_bytes = Arc::new(AtomicU64::new(0));
    let down_last: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    let mut receivers = Vec::new();
    if spec.direction.down() {
        for i in 0..spec.streams {
            let mut s = open_data(ctl, code, i, Direction::Download)?;
            let (bytes, last, stop) = (down_bytes.clone(), down_last.clone(), stop.clone());
            receivers.push(std::thread::spawn(move || {
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = vec![0u8; TCP_BUF];
                loop {
                    match s.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            bytes.fetch_add(n as u64, Ordering::Relaxed);
                            if let Ok(mut l) = last.lock() {
                                *l = Some(Instant::now());
                            }
                        }
                        Err(_) => break,
                    }
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }));
        }
    }

    let mut up_iv: Vec<IntervalStat> = Vec::new();
    let mut down_iv: Vec<IntervalStat> = Vec::new();
    let mut server_down_iv: Vec<IntervalStat> = Vec::new();
    let (mut prev_down, mut prev_retrans) = (0u64, 0u64);
    let mut index = 0u32;
    let mut next = start + iv;
    while Instant::now() < deadline + Duration::from_millis(200) {
        if cancel.load(Ordering::Relaxed) {
            stop.store(true, Ordering::SeqCst);
            break;
        }
        // Server-side upload intervals.
        while let Ok(m) = ctl.rx.try_recv() {
            if let Msg::Interval { dir, mut stat } = m {
                match dir {
                    Direction::Upload => {
                        // Attach our sender-side TCP_INFO.
                        if let Ok(socks) = up_socks.lock() {
                            let infos: Vec<(u64, f64)> =
                                socks.iter().filter_map(tcp_info).collect();
                            if !infos.is_empty() {
                                let r: u64 = infos.iter().map(|i| i.0).sum();
                                stat.retransmits = Some(r - prev_retrans.min(r));
                                prev_retrans = r;
                                stat.rtt_ms = Some(
                                    infos.iter().map(|i| i.1).sum::<f64>() / infos.len() as f64,
                                );
                            }
                        }
                        on_progress(PerfProgress {
                            dir: "upload",
                            index: stat.index,
                            t: stat.t,
                            mbps: stat.mbps,
                            bytes: stat.bytes,
                            retransmits: stat.retransmits,
                            rtt_ms: stat.rtt_ms,
                            lost: None,
                            jitter_ms: None,
                        });
                        up_iv.push(stat);
                    }
                    _ => server_down_iv.push(stat),
                }
            }
        }
        // Our download intervals.
        let now = Instant::now();
        if spec.direction.down() && now >= next && next <= deadline {
            let total = down_bytes.load(Ordering::Relaxed);
            let bytes = total - prev_down.min(total);
            prev_down = total;
            let st = IntervalStat {
                index,
                t: next.duration_since(start).as_secs_f64(),
                bytes,
                mbps: mbps(bytes, iv.as_secs_f64()),
                ..Default::default()
            };
            on_progress(PerfProgress {
                dir: "download",
                index,
                t: st.t,
                mbps: st.mbps,
                bytes,
                retransmits: None,
                rtt_ms: None,
                lost: None,
                jitter_ms: None,
            });
            down_iv.push(st);
        }
        if now >= next {
            index += 1;
            next += iv;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for s in senders {
        let _ = s.join();
    }
    for r in receivers {
        let _ = r.join();
    }
    let mut late = Vec::new();
    let (upload, dl_sent) = finish(ctl, None, None, &mut late);
    for (dir, stat) in late {
        if dir == Direction::Upload && !up_iv.iter().any(|i| i.index == stat.index) {
            up_iv.push(stat);
        } else if dir == Direction::Download {
            server_down_iv.push(stat);
        }
    }

    if spec.direction.up() {
        let (bytes, secs, server_iv) = match &upload {
            Some(u) => (u.bytes, u.seconds, u.intervals.clone()),
            None => (
                up_iv.iter().map(|i| i.bytes).sum(),
                spec.duration_secs as f64,
                Vec::new(),
            ),
        };
        // Prefer the server's authoritative intervals, enriched with our TCP_INFO.
        let mut ivs = if server_iv.is_empty() {
            up_iv.clone()
        } else {
            server_iv
        };
        for i in ivs.iter_mut() {
            if let Some(mine) = up_iv.iter().find(|m| m.index == i.index) {
                i.retransmits = mine.retransmits;
                i.rtt_ms = mine.rtt_ms;
            }
        }
        let mut r = summarize("upload", ivs, bytes, secs);
        r.retransmits = r
            .intervals
            .iter()
            .filter_map(|i| i.retransmits)
            .reduce(|a, b| a + b);
        r.rtt_ms = avg(r.intervals.iter().filter_map(|i| i.rtt_ms));
        out.upload = Some(r);
    }
    if spec.direction.down() {
        let total = down_bytes.load(Ordering::Relaxed);
        let secs = down_last
            .lock()
            .ok()
            .and_then(|l| *l)
            .map(|l| l.duration_since(start).as_secs_f64())
            .unwrap_or(0.0)
            .max(0.001);
        for i in down_iv.iter_mut() {
            if let Some(s) = server_down_iv.iter().find(|s| s.index == i.index) {
                i.retransmits = s.retransmits;
                i.rtt_ms = s.rtt_ms;
            }
        }
        let mut r = summarize("download", down_iv, total, secs);
        r.retransmits = dl_sent.as_ref().and_then(|d| d.retransmits).or_else(|| {
            r.intervals
                .iter()
                .filter_map(|i| i.retransmits)
                .reduce(|a, b| a + b)
        });
        r.rtt_ms = avg(r.intervals.iter().filter_map(|i| i.rtt_ms));
        out.download = Some(r);
    }
    Ok(())
}

fn avg(it: impl Iterator<Item = f64>) -> Option<f64> {
    let v: Vec<f64> = it.collect();
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn udp_socket(ctl: &Control) -> Result<UdpSocket> {
    let bind = if ctl.addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let s = UdpSocket::bind(bind)?;
    s.connect(SocketAddr::new(ctl.addr.ip(), ctl.udp_port))?;
    Ok(s)
}

fn run_udp(
    ctl: &mut Control,
    spec: &TestSpec,
    cancel: &AtomicBool,
    on_progress: &impl Fn(PerfProgress),
    out: &mut PerfResult,
) -> Result<()> {
    let start = Instant::now();
    let deadline = start + Duration::from_secs(u64::from(spec.duration_secs));
    let iv = Duration::from_millis(u64::from(spec.interval_ms));
    let size = usize::from(spec.packet_size).max(UDP_HEADER);
    let sock = udp_socket(ctl)?;
    let epoch = Instant::now();

    // Upload sender thread.
    let sent = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let sender = spec.direction.up().then(|| {
        let (sock, sent, stop, session) =
            (sock.try_clone(), sent.clone(), stop.clone(), ctl.session);
        let pps = spec.bitrate_mbps * 1e6 / (size as f64 * 8.0);
        std::thread::spawn(move || {
            let Ok(sock) = sock else { return };
            let mut buf = vec![0u8; size];
            let t0 = Instant::now();
            let mut seq = 0u64;
            while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
                let due = (t0.elapsed().as_secs_f64() * pps) as u64;
                let mut burst = 0;
                while seq < due && burst < 2000 {
                    encode_header(
                        &mut buf,
                        PKT_DATA,
                        session,
                        seq,
                        epoch.elapsed().as_nanos() as u64,
                    );
                    let _ = sock.send(&buf);
                    seq += 1;
                    burst += 1;
                }
                sent.store(seq, Ordering::Relaxed);
                std::thread::sleep(Duration::from_micros(200));
            }
            sent.store(seq, Ordering::Relaxed);
            for _ in 0..3 {
                encode_header(&mut buf, PKT_FIN, session, seq, 0);
                let _ = sock.send(&buf[..UDP_HEADER]);
            }
        })
    });

    // Download receiver (main thread).
    let mut recv = UdpRecv::default();
    let mut down_iv: Vec<IntervalStat> = Vec::new();
    let mut up_iv: Vec<IntervalStat> = Vec::new();
    let mut hello_buf = vec![0u8; UDP_HEADER];
    encode_header(&mut hello_buf, PKT_HELLO, ctl.session, 0, 0);
    let mut last_hello = Instant::now() - Duration::from_secs(1);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(20)));
    let mut buf = vec![0u8; 65536];
    let mut index = 0u32;
    let mut next = start + iv;
    let mut last_rx: Option<Instant> = None;
    let end = deadline + Duration::from_millis(if spec.direction.down() { 800 } else { 200 });
    while Instant::now() < end {
        if cancel.load(Ordering::Relaxed) {
            stop.store(true, Ordering::SeqCst);
            break;
        }
        if spec.direction.down() {
            if recv.packets == 0 && last_hello.elapsed() > Duration::from_millis(200) {
                let _ = sock.send(&hello_buf);
                last_hello = Instant::now();
            }
            if let Ok(n) = sock.recv(&mut buf) {
                if let Some((PKT_DATA, s, seq, ts)) = decode_header(&buf[..n]) {
                    if s == ctl.session {
                        recv.on_packet(seq, ts, epoch.elapsed().as_nanos() as u64, n);
                        last_rx = Some(Instant::now());
                    }
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
        while let Ok(m) = ctl.rx.try_recv() {
            if let Msg::Interval {
                dir: Direction::Upload,
                stat,
            } = m
            {
                on_progress(PerfProgress {
                    dir: "upload",
                    index: stat.index,
                    t: stat.t,
                    mbps: stat.mbps,
                    bytes: stat.bytes,
                    retransmits: None,
                    rtt_ms: None,
                    lost: stat.lost,
                    jitter_ms: stat.jitter_ms,
                });
                up_iv.push(stat);
            }
        }
        let now = Instant::now();
        if now >= next && next <= deadline {
            if spec.direction.down() {
                let (bytes, packets, lost) = recv.take_interval();
                let st = IntervalStat {
                    index,
                    t: next.duration_since(start).as_secs_f64(),
                    bytes,
                    mbps: mbps(bytes, iv.as_secs_f64()),
                    packets: Some(packets),
                    lost: Some(lost),
                    jitter_ms: Some(recv.jitter_ms()),
                    ..Default::default()
                };
                on_progress(PerfProgress {
                    dir: "download",
                    index,
                    t: st.t,
                    mbps: st.mbps,
                    bytes,
                    retransmits: None,
                    rtt_ms: None,
                    lost: st.lost,
                    jitter_ms: st.jitter_ms,
                });
                down_iv.push(st);
            }
            index += 1;
            next += iv;
        }
    }
    if let Some(s) = sender {
        let _ = s.join();
    }
    let sent_packets = sent.load(Ordering::Relaxed);
    let mut late = Vec::new();
    let (upload, dl_sent) = finish(
        ctl,
        spec.direction.up().then_some(sent_packets),
        spec.direction.up().then_some(sent_packets * size as u64),
        &mut late,
    );
    for (dir, stat) in late {
        if dir == Direction::Upload && !up_iv.iter().any(|i| i.index == stat.index) {
            up_iv.push(stat);
        }
    }

    if spec.direction.up() {
        let u = upload.unwrap_or_default();
        let ivs = if u.intervals.is_empty() {
            up_iv
        } else {
            u.intervals.clone()
        };
        let mut r = summarize("upload", ivs, u.bytes, u.seconds.max(0.001));
        r.packets_sent = Some(sent_packets);
        r.packets_received = u.packets;
        r.lost = u.lost;
        r.loss_percent = u.lost.map(|l| {
            if sent_packets > 0 {
                l as f64 * 100.0 / sent_packets as f64
            } else {
                0.0
            }
        });
        r.out_of_order = u.out_of_order;
        r.jitter_ms = u.jitter_ms;
        out.upload = Some(r);
    }
    if spec.direction.down() {
        let secs = last_rx
            .map(|l| l.duration_since(start).as_secs_f64())
            .unwrap_or(0.001)
            .max(0.001);
        let mut r = summarize("download", down_iv, recv.bytes, secs);
        let server_sent = dl_sent
            .and_then(|d| d.packets)
            .unwrap_or(recv.expected())
            .max(recv.expected());
        let lost = server_sent.saturating_sub(recv.packets);
        r.packets_sent = Some(server_sent);
        r.packets_received = Some(recv.packets);
        r.lost = Some(lost);
        r.loss_percent = Some(if server_sent > 0 {
            lost as f64 * 100.0 / server_sent as f64
        } else {
            0.0
        });
        r.out_of_order = Some(recv.out_of_order);
        r.jitter_ms = Some(recv.jitter_ms());
        out.download = Some(r);
    }
    Ok(())
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn run_latency(
    ctl: &mut Control,
    spec: &TestSpec,
    cancel: &AtomicBool,
    on_progress: &impl Fn(PerfProgress),
    out: &mut PerfResult,
) -> Result<()> {
    let sock = udp_socket(ctl)?;
    let _ = sock.set_read_timeout(Some(Duration::from_millis(10)));
    let start = Instant::now();
    let deadline = start + Duration::from_secs(u64::from(spec.duration_secs));
    let gap = Duration::from_secs_f64(1.0 / f64::from(spec.latency_pps));
    let iv = Duration::from_millis(u64::from(spec.interval_ms));
    let total = (f64::from(spec.duration_secs) * f64::from(spec.latency_pps)) as usize;
    let mut samples: Vec<Option<f64>> = vec![None; total];
    let mut sent = 0usize;
    let mut next_send = start;
    let mut next_iv = start + iv;
    let (mut index, mut iv_from) = (0u32, 0usize);
    let mut pkt = vec![0u8; 64];
    let mut buf = [0u8; 256];
    let end = deadline + Duration::from_secs(1);
    while Instant::now() < end {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let now = Instant::now();
        if sent < total && now >= next_send && now < deadline {
            encode_header(
                &mut pkt,
                PKT_ECHO_REQ,
                ctl.session,
                sent as u64,
                start.elapsed().as_nanos() as u64,
            );
            let _ = sock.send(&pkt);
            sent += 1;
            next_send += gap;
        }
        if let Ok(n) = sock.recv(&mut buf) {
            if let Some((PKT_ECHO_REPLY, s, seq, ts)) = decode_header(&buf[..n]) {
                let i = seq as usize;
                if s == ctl.session && i < samples.len() && samples[i].is_none() {
                    samples[i] =
                        Some((start.elapsed().as_nanos() as u64).saturating_sub(ts) as f64 / 1e6);
                }
            }
        }
        if now >= next_iv && next_iv <= deadline {
            let window: Vec<f64> = samples[iv_from..sent.min(samples.len())]
                .iter()
                .flatten()
                .copied()
                .collect();
            let lost = (sent - iv_from).saturating_sub(window.len()) as u64;
            on_progress(PerfProgress {
                dir: "latency",
                index,
                t: next_iv.duration_since(start).as_secs_f64(),
                mbps: 0.0,
                bytes: 0,
                retransmits: None,
                rtt_ms: avg(window.iter().copied()),
                lost: Some(lost),
                jitter_ms: None,
            });
            iv_from = sent;
            index += 1;
            next_iv += iv;
        }
    }
    let mut late = Vec::new();
    let _ = finish(ctl, Some(sent as u64), None, &mut late);
    samples.truncate(sent);
    let mut rtts: Vec<f64> = samples.iter().flatten().copied().collect();
    let ordered = rtts.clone();
    rtts.sort_by(|a, b| a.total_cmp(b));
    let n = rtts.len();
    let mean = if n > 0 {
        rtts.iter().sum::<f64>() / n as f64
    } else {
        0.0
    };
    let sd = if n > 1 {
        (rtts.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt()
    } else {
        0.0
    };
    let jitter = if ordered.len() > 1 {
        ordered.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (ordered.len() - 1) as f64
    } else {
        0.0
    };
    out.latency = Some(LatencyResult {
        sent: sent as u64,
        received: n as u64,
        loss_percent: if sent > 0 {
            (sent - n) as f64 * 100.0 / sent as f64
        } else {
            0.0
        },
        min_ms: rtts.first().copied().unwrap_or(0.0),
        avg_ms: mean,
        max_ms: rtts.last().copied().unwrap_or(0.0),
        p50_ms: percentile(&rtts, 50.0),
        p90_ms: percentile(&rtts, 90.0),
        p99_ms: percentile(&rtts, 99.0),
        stddev_ms: sd,
        jitter_ms: jitter,
        samples,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: TestKind, direction: Direction) -> TestSpec {
        TestSpec {
            kind,
            direction,
            streams: 2,
            duration_secs: 2,
            bitrate_mbps: 50.0,
            packet_size: 1200,
            interval_ms: 500,
            latency_pps: 50,
        }
    }

    /// End-to-end over loopback: server + client in one process.
    #[test]
    fn loopback_suite() {
        let port = 46_000 + (std::process::id() % 1000) as u16;
        let events: server::EventSink = Arc::new(|_| {});
        let srv = server::start(port, Some("1234".into()), events).unwrap();
        let cancel = AtomicBool::new(false);

        let wrong = run_test(
            "127.0.0.1",
            port,
            Some("0000".into()),
            &spec(TestKind::Tcp, Direction::Upload),
            &cancel,
            |_| {},
        );
        assert!(wrong.is_err(), "wrong access code must be rejected");

        let code = Some("1234".to_string());
        let r = run_test(
            "127.0.0.1",
            port,
            code.clone(),
            &spec(TestKind::Tcp, Direction::Bidir),
            &cancel,
            |_| {},
        )
        .unwrap();
        let (u, d) = (r.upload.unwrap(), r.download.unwrap());
        assert!(
            u.mbps_avg > 50.0 && d.mbps_avg > 50.0,
            "loopback TCP should be fast: {} / {}",
            u.mbps_avg,
            d.mbps_avg
        );
        assert!(u.intervals.len() >= 3);

        let r = run_test(
            "127.0.0.1",
            port,
            code.clone(),
            &spec(TestKind::Udp, Direction::Upload),
            &cancel,
            |_| {},
        )
        .unwrap();
        let u = r.upload.unwrap();
        assert!(
            (u.mbps_avg - 50.0).abs() < 15.0,
            "UDP rate should track the target: {}",
            u.mbps_avg
        );
        assert!(u.loss_percent.unwrap() < 5.0);

        let r = run_test(
            "127.0.0.1",
            port,
            code.clone(),
            &spec(TestKind::Udp, Direction::Download),
            &cancel,
            |_| {},
        )
        .unwrap();
        assert!(r.download.unwrap().packets_received.unwrap() > 1000);

        let r = run_test(
            "127.0.0.1",
            port,
            code,
            &spec(TestKind::Latency, Direction::Upload),
            &cancel,
            |_| {},
        )
        .unwrap();
        let l = r.latency.unwrap();
        assert!(
            l.received >= 90 && l.avg_ms < 5.0,
            "loopback latency: {l:?}"
        );

        assert!(discover(port, Duration::from_millis(500))
            .iter()
            .any(|s| s.port == port && s.code_required));
        srv.stop();
    }
}
