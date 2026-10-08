// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netperf/server.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Speed-test server. Listens on one TCP port (control + data) and
//           the same UDP port (UDP tests, latency echo, LAN discovery).
//           One test runs at a time; data connections and UDP packets are
//           only accepted from the peer that started the session. An
//           optional access code keeps other machines out.
// =============================================================================

use std::io::{BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub port: u16,
    pub host: String,
    pub addresses: Vec<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServerEvent {
    #[serde(rename_all = "camelCase")]
    TestStarted {
        peer: String,
        host: String,
        spec: TestSpec,
    },
    #[serde(rename_all = "camelCase")]
    Interval { dir: Direction, t: f64, mbps: f64 },
    #[serde(rename_all = "camelCase")]
    TestFinished { peer: String, summary: String },
    #[serde(rename_all = "camelCase")]
    Rejected { peer: String, reason: String },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
}

pub type EventSink = Arc<dyn Fn(ServerEvent) + Send + Sync>;

struct Session {
    id: u64,
    spec: TestSpec,
    peer: IpAddr,
    start: Instant,
    deadline: Instant,
    done: AtomicBool,
    /// TCP upload: bytes received per stream.
    up_counters: Mutex<Vec<Arc<AtomicU64>>>,
    up_last: Mutex<Option<Instant>>,
    /// UDP upload accounting.
    udp: Mutex<UdpRecv>,
    /// UDP download destination (learnt from the client's hello packet).
    dl_target: Mutex<Option<SocketAddr>>,
    dl_bytes: AtomicU64,
    dl_packets: AtomicU64,
    /// TCP download sender sockets for TCP_INFO sampling.
    dl_sockets: Mutex<Vec<TcpStream>>,
}

type Current = Arc<Mutex<Option<Arc<Session>>>>;

struct Ctx {
    port: u16,
    code: Option<String>,
    host: String,
    current: Current,
    udp: UdpSocket,
    stop: Arc<AtomicBool>,
    events: EventSink,
    epoch: Instant,
}

pub struct PerfServer {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    pub info: ServerInfo,
}

impl PerfServer {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for PerfServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// IPv4 addresses of this machine's interfaces (to show to the user).
pub fn local_addresses() -> Vec<String> {
    let nets = sysinfo::Networks::new_with_refreshed_list();
    let mut v: Vec<String> = nets
        .list()
        .iter()
        .filter(|(name, _)| {
            *name != "lo"
                && !name.starts_with("docker")
                && !name.starts_with("veth")
                && !name.starts_with("br-")
        })
        .flat_map(|(_, d)| d.ip_networks().iter().map(|n| n.addr))
        .filter(|a| a.is_ipv4() && !a.is_loopback())
        .map(|a| a.to_string())
        .collect();
    v.sort();
    v.dedup();
    v
}

pub fn start(port: u16, code: Option<String>, events: EventSink) -> Result<PerfServer> {
    let listener = TcpListener::bind(("0.0.0.0", port))
        .map_err(|e| CoreError::Other(format!("cannot listen on TCP port {port}: {e}")))?;
    listener.set_nonblocking(true)?;
    let udp = UdpSocket::bind(("0.0.0.0", port))
        .map_err(|e| CoreError::Other(format!("cannot listen on UDP port {port}: {e}")))?;
    udp.set_read_timeout(Some(Duration::from_millis(200)))?;
    let stop = Arc::new(AtomicBool::new(false));
    let code = code.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
    let ctx = Arc::new(Ctx {
        port,
        code: code.clone(),
        host: crate::netconfig::hostname(),
        current: Arc::new(Mutex::new(None)),
        udp,
        stop: stop.clone(),
        events,
        epoch: Instant::now(),
    });

    let c1 = ctx.clone();
    let accept = std::thread::Builder::new()
        .name("perf-accept".into())
        .spawn(move || {
            while !c1.stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((s, peer)) => {
                        let c = c1.clone();
                        let _ = s.set_nonblocking(false);
                        std::thread::spawn(move || handle_conn(c, s, peer));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50))
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(200)),
                }
            }
        })?;
    let c2 = ctx.clone();
    let udp_thread = std::thread::Builder::new()
        .name("perf-udp".into())
        .spawn(move || udp_loop(c2))?;

    Ok(PerfServer {
        stop,
        threads: vec![accept, udp_thread],
        info: ServerInfo {
            port,
            host: ctx.host.clone(),
            addresses: local_addresses(),
            code,
        },
    })
}

fn handle_conn(ctx: Arc<Ctx>, s: TcpStream, peer: SocketAddr) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = s.set_nodelay(true);
    let Ok(rs) = s.try_clone() else { return };
    let mut reader = BufReader::new(rs);
    let Ok(Msg::Hello {
        protocol,
        role,
        session,
        stream,
        dir,
        code,
        host,
    }) = read_msg(&mut reader)
    else {
        return;
    };
    let mut w = s;
    if !protocol.starts_with("syspulse-perf/") {
        return;
    }
    if ctx.code.is_some() && code.as_deref().map(str::trim) != ctx.code.as_deref() {
        let _ = write_msg(
            &mut w,
            &Msg::Error {
                message: "wrong or missing access code".into(),
            },
        );
        (ctx.events)(ServerEvent::Rejected {
            peer: peer.ip().to_string(),
            reason: "wrong access code".into(),
        });
        return;
    }
    match role {
        Role::Control => control(ctx, w, reader, peer, host.unwrap_or_default()),
        Role::Data => {
            let sess = ctx.current.lock().ok().and_then(|c| c.clone());
            match (sess, session, dir) {
                (Some(sess), Some(id), Some(dir)) if sess.id == id && sess.peer == peer.ip() => {
                    data(sess, w, reader, dir, stream.unwrap_or(0));
                }
                _ => {}
            }
        }
    }
}

fn control(
    ctx: Arc<Ctx>,
    mut w: TcpStream,
    mut reader: BufReader<TcpStream>,
    peer: SocketAddr,
    host: String,
) {
    let spec = match read_msg(&mut reader) {
        Ok(Msg::Start { spec }) => spec,
        _ => return,
    };
    if let Err(e) = spec.validate() {
        let _ = write_msg(
            &mut w,
            &Msg::Error {
                message: e.to_string(),
            },
        );
        return;
    }
    // One test at a time.
    {
        let mut cur = match ctx.current.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        if let Some(s) = cur.as_ref() {
            if !s.done.load(Ordering::Relaxed)
                && Instant::now() < s.deadline + Duration::from_secs(20)
            {
                let _ = write_msg(
                    &mut w,
                    &Msg::Error {
                        message: "server is busy with another test".into(),
                    },
                );
                (ctx.events)(ServerEvent::Rejected {
                    peer: peer.ip().to_string(),
                    reason: "busy".into(),
                });
                return;
            }
        }
        let now = Instant::now();
        *cur = Some(Arc::new(Session {
            id: new_session_id(),
            spec: spec.clone(),
            peer: peer.ip(),
            start: now,
            deadline: now + Duration::from_secs(u64::from(spec.duration_secs)),
            done: AtomicBool::new(false),
            up_counters: Mutex::new(Vec::new()),
            up_last: Mutex::new(None),
            udp: Mutex::new(UdpRecv::default()),
            dl_target: Mutex::new(None),
            dl_bytes: AtomicU64::new(0),
            dl_packets: AtomicU64::new(0),
            dl_sockets: Mutex::new(Vec::new()),
        }));
    }
    let Some(sess) = ctx.current.lock().ok().and_then(|c| c.clone()) else {
        return;
    };
    if write_msg(
        &mut w,
        &Msg::Ready {
            session: sess.id,
            udp_port: ctx.port,
            host: ctx.host.clone(),
        },
    )
    .is_err()
    {
        return;
    }
    (ctx.events)(ServerEvent::TestStarted {
        peer: peer.ip().to_string(),
        host: host.clone(),
        spec: spec.clone(),
    });

    let writer = Arc::new(Mutex::new(w));
    // Interval reporter (server-side receive stats / sender TCP_INFO).
    let rep = {
        let (sess, writer, events) = (sess.clone(), writer.clone(), ctx.events.clone());
        std::thread::spawn(move || reporter(sess, writer, events))
    };
    // UDP download sender.
    let udp_tx = (spec.kind == TestKind::Udp && spec.direction.down()).then(|| {
        let (sess, sock) = (sess.clone(), ctx.udp.try_clone());
        let epoch = ctx.epoch;
        std::thread::spawn(move || {
            if let Ok(sock) = sock {
                udp_sender(sess, sock, epoch)
            }
        })
    });

    // Wait for the client's Done.
    let _ = reader.get_ref().set_read_timeout(Some(Duration::from_secs(
        u64::from(spec.duration_secs) + 30,
    )));
    let mut client_sent: Option<u64> = None;
    if let Ok(Msg::Done { sent_packets, .. }) = read_msg(&mut reader) {
        client_sent = sent_packets;
    }
    std::thread::sleep(Duration::from_millis(300)); // trailing packets / bytes
    sess.done.store(true, Ordering::SeqCst);
    let intervals = rep.join().unwrap_or_default();
    if let Some(t) = udp_tx {
        let _ = t.join();
    }

    // Final statistics.
    let upload = (spec.direction.up() && spec.kind != TestKind::Latency).then(|| {
        let mut stats = ReceiveStats {
            intervals: intervals.0,
            ..Default::default()
        };
        let last = sess.up_last.lock().ok().and_then(|l| *l);
        stats.seconds = last
            .map(|l| l.duration_since(sess.start).as_secs_f64())
            .unwrap_or(0.0)
            .max(0.001);
        if spec.kind == TestKind::Tcp {
            stats.bytes = sess
                .up_counters
                .lock()
                .map(|c| c.iter().map(|x| x.load(Ordering::Relaxed)).sum())
                .unwrap_or(0);
        } else if let Ok(u) = sess.udp.lock() {
            stats.bytes = u.bytes;
            stats.packets = Some(u.packets);
            let expected = client_sent.unwrap_or(u.expected()).max(u.expected());
            stats.lost = Some(expected.saturating_sub(u.packets));
            stats.out_of_order = Some(u.out_of_order);
            stats.jitter_ms = Some(u.jitter_ms());
        }
        stats
    });
    let download_sent =
        (spec.direction.down() && spec.kind != TestKind::Latency).then(|| SendStats {
            bytes: sess.dl_bytes.load(Ordering::Relaxed),
            packets: (spec.kind == TestKind::Udp).then(|| sess.dl_packets.load(Ordering::Relaxed)),
            retransmits: intervals
                .1
                .iter()
                .filter_map(|i| i.retransmits)
                .reduce(|a, b| a + b),
            intervals: intervals.1,
        });
    let summary = match (&upload, &download_sent) {
        (Some(u), _) => format!("received {:.1} Mbit/s", mbps(u.bytes, u.seconds)),
        (None, Some(d)) => format!("sent {}", crate::util::human_bytes(d.bytes)),
        _ => "latency test".into(),
    };
    if let Ok(mut w) = writer.lock() {
        let _ = write_msg(
            &mut *w,
            &Msg::Result {
                upload,
                download_sent,
            },
        );
    }
    (ctx.events)(ServerEvent::TestFinished {
        peer: peer.ip().to_string(),
        summary,
    });
    if let Ok(mut c) = ctx.current.lock() {
        if c.as_ref().map(|s| s.id) == Some(sess.id) {
            *c = None;
        }
    }
}

/// Sends interval reports until the session ends.
/// Returns (upload receive intervals, download sender intervals).
fn reporter(
    sess: Arc<Session>,
    writer: Arc<Mutex<TcpStream>>,
    events: EventSink,
) -> (Vec<IntervalStat>, Vec<IntervalStat>) {
    let iv = Duration::from_millis(u64::from(sess.spec.interval_ms));
    let (mut up, mut down) = (Vec::new(), Vec::new());
    let (mut prev_up, mut prev_dl, mut prev_retrans) = (0u64, 0u64, 0u64);
    let mut index = 0u32;
    let mut next = sess.start + iv;
    let hard_stop = sess.deadline + Duration::from_secs(25);
    while !sess.done.load(Ordering::Relaxed) && Instant::now() < hard_stop {
        let now = Instant::now();
        if now < next {
            std::thread::sleep((next - now).min(Duration::from_millis(50)));
            continue;
        }
        // Stop reporting a while after the deadline (only trailing data left).
        if now > sess.deadline + iv {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let t = next.duration_since(sess.start).as_secs_f64();
        let secs = iv.as_secs_f64();
        let mut msgs = Vec::new();
        if sess.spec.direction.up() && sess.spec.kind != TestKind::Latency {
            let mut st = IntervalStat {
                index,
                t,
                ..Default::default()
            };
            if sess.spec.kind == TestKind::Tcp {
                let total: u64 = sess
                    .up_counters
                    .lock()
                    .map(|c| c.iter().map(|x| x.load(Ordering::Relaxed)).sum())
                    .unwrap_or(0);
                st.bytes = total - prev_up.min(total);
                prev_up = total;
            } else if let Ok(mut u) = sess.udp.lock() {
                let (b, p, lost) = u.take_interval();
                st.bytes = b;
                st.packets = Some(p);
                st.lost = Some(lost);
                st.jitter_ms = Some(u.jitter_ms());
            }
            st.mbps = mbps(st.bytes, secs);
            events(ServerEvent::Interval {
                dir: Direction::Upload,
                t,
                mbps: st.mbps,
            });
            msgs.push(Msg::Interval {
                dir: Direction::Upload,
                stat: st.clone(),
            });
            up.push(st);
        }
        if sess.spec.direction.down() && sess.spec.kind != TestKind::Latency {
            let total = sess.dl_bytes.load(Ordering::Relaxed);
            let mut st = IntervalStat {
                index,
                t,
                bytes: total - prev_dl.min(total),
                ..Default::default()
            };
            prev_dl = total;
            st.mbps = mbps(st.bytes, secs);
            if sess.spec.kind == TestKind::Tcp {
                if let Ok(socks) = sess.dl_sockets.lock() {
                    let infos: Vec<(u64, f64)> = socks.iter().filter_map(tcp_info).collect();
                    if !infos.is_empty() {
                        let retrans: u64 = infos.iter().map(|i| i.0).sum();
                        st.retransmits = Some(retrans - prev_retrans.min(retrans));
                        prev_retrans = retrans;
                        st.rtt_ms =
                            Some(infos.iter().map(|i| i.1).sum::<f64>() / infos.len() as f64);
                    }
                }
            } else {
                st.packets = Some(sess.dl_packets.load(Ordering::Relaxed));
            }
            events(ServerEvent::Interval {
                dir: Direction::Download,
                t,
                mbps: st.mbps,
            });
            msgs.push(Msg::Interval {
                dir: Direction::Download,
                stat: st.clone(),
            });
            down.push(st);
        }
        if let Ok(mut w) = writer.lock() {
            for m in &msgs {
                let _ = write_msg(&mut *w, m);
            }
        }
        index += 1;
        next += iv;
    }
    (up, down)
}

fn data(
    sess: Arc<Session>,
    mut w: TcpStream,
    mut reader: BufReader<TcpStream>,
    dir: Direction,
    _stream: u8,
) {
    match dir {
        // Client uploads: we receive.
        Direction::Upload => {
            let counter = Arc::new(AtomicU64::new(0));
            if let Ok(mut c) = sess.up_counters.lock() {
                c.push(counter.clone());
            }
            let _ = reader
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = vec![0u8; TCP_BUF];
            let hard_stop = sess.deadline + Duration::from_secs(10);
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        counter.fetch_add(n as u64, Ordering::Relaxed);
                        if let Ok(mut l) = sess.up_last.lock() {
                            *l = Some(Instant::now());
                        }
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        if Instant::now() > hard_stop || sess.done.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        // Client downloads: we send until the deadline.
        Direction::Download => {
            if let Ok(clone) = w.try_clone() {
                if let Ok(mut s) = sess.dl_sockets.lock() {
                    s.push(clone);
                }
            }
            let buf: Vec<u8> = (0..TCP_BUF).map(|i| (i % 251) as u8).collect();
            let _ = w.set_write_timeout(Some(Duration::from_secs(5)));
            while Instant::now() < sess.deadline && !sess.done.load(Ordering::Relaxed) {
                match w.write(&buf) {
                    Ok(n) => {
                        sess.dl_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    }
                    Err(_) => break,
                }
            }
            let _ = w.shutdown(std::net::Shutdown::Write);
        }
        Direction::Bidir => {}
    }
}

fn udp_sender(sess: Arc<Session>, sock: UdpSocket, epoch: Instant) {
    // Wait for the client's hello so we know where to send.
    let wait_until = Instant::now() + Duration::from_secs(5);
    let target = loop {
        if let Some(t) = sess.dl_target.lock().ok().and_then(|t| *t) {
            break Some(t);
        }
        if Instant::now() > wait_until || sess.done.load(Ordering::Relaxed) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let Some(target) = target else { return };
    let size = usize::from(sess.spec.packet_size).max(UDP_HEADER);
    let pps = sess.spec.bitrate_mbps * 1e6 / (size as f64 * 8.0);
    let mut buf = vec![0u8; size];
    let start = Instant::now();
    let mut seq = 0u64;
    while Instant::now() < sess.deadline && !sess.done.load(Ordering::Relaxed) {
        let due = (start.elapsed().as_secs_f64() * pps) as u64;
        let mut burst = 0;
        while seq < due && burst < 2000 {
            encode_header(
                &mut buf,
                PKT_DATA,
                sess.id,
                seq,
                epoch.elapsed().as_nanos() as u64,
            );
            if sock.send_to(&buf, target).is_ok() {
                sess.dl_bytes.fetch_add(size as u64, Ordering::Relaxed);
                sess.dl_packets.fetch_add(1, Ordering::Relaxed);
            }
            seq += 1;
            burst += 1;
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    for _ in 0..3 {
        encode_header(&mut buf, PKT_FIN, sess.id, seq, 0);
        let _ = sock.send_to(&buf[..UDP_HEADER], target);
    }
}

fn udp_loop(ctx: Arc<Ctx>) {
    let mut buf = vec![0u8; 65536];
    while !ctx.stop.load(Ordering::Relaxed) {
        let Ok((n, src)) = ctx.udp.recv_from(&mut buf) else {
            continue;
        };
        let pkt = &buf[..n];
        if pkt.first() == Some(&PKT_DISCOVER)
            && pkt.get(1..1 + DISCOVER_MAGIC.len()) == Some(DISCOVER_MAGIC)
        {
            let reply = serde_json::json!({ "host": ctx.host, "port": ctx.port, "codeRequired": ctx.code.is_some(), "protocol": PROTOCOL });
            let mut out = vec![PKT_DISCOVER_REPLY];
            out.extend_from_slice(reply.to_string().as_bytes());
            let _ = ctx.udp.send_to(&out, src);
            continue;
        }
        let Some((kind, session, seq, ts)) = decode_header(pkt) else {
            continue;
        };
        let Some(sess) = ctx.current.lock().ok().and_then(|c| c.clone()) else {
            continue;
        };
        if sess.id != session || sess.peer != src.ip() {
            continue;
        }
        match kind {
            PKT_DATA if sess.spec.kind == TestKind::Udp => {
                if let Ok(mut u) = sess.udp.lock() {
                    u.on_packet(seq, ts, ctx.epoch.elapsed().as_nanos() as u64, n);
                }
                if let Ok(mut l) = sess.up_last.lock() {
                    *l = Some(Instant::now());
                }
            }
            PKT_ECHO_REQ if sess.spec.kind == TestKind::Latency => {
                let mut out = pkt.to_vec();
                out[0] = PKT_ECHO_REPLY;
                let _ = ctx.udp.send_to(&out, src);
            }
            PKT_HELLO => {
                if let Ok(mut t) = sess.dl_target.lock() {
                    *t = Some(src);
                }
            }
            _ => {}
        }
    }
}
