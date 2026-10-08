// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/forge/links.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: PacketForge transports: TCP client (optional auto-reconnect),
//           TCP server (many clients, send to one or all), UDP (broadcast,
//           multicast, reply-to-last) and serial (modem lines, break).
// =============================================================================

use std::io::{Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Ctx, Dir, ForgeEvent, Link, SessionConfig};
use crate::error::{CoreError, Result};

const READ_BUF: usize = 64 * 1024;

pub(crate) fn open(config: &SessionConfig, ctx: Arc<Ctx>) -> Result<Box<dyn Link>> {
    Ok(match config {
        SessionConfig::TcpClient {
            host,
            port,
            auto_reconnect,
        } => Box::new(TcpClient::connect(host, *port, *auto_reconnect, ctx)?),
        SessionConfig::TcpServer {
            bind,
            port,
            max_clients,
        } => Box::new(TcpServer::start(bind, *port, *max_clients, ctx)?),
        SessionConfig::Udp {
            bind,
            local_port,
            remote_host,
            remote_port,
            broadcast,
            multicast_group,
        } => Box::new(Udp::start(
            bind,
            *local_port,
            remote_host.as_deref().filter(|h| !h.trim().is_empty()),
            *remote_port,
            *broadcast,
            multicast_group.as_deref().filter(|g| !g.trim().is_empty()),
            ctx,
        )?),
        SessionConfig::Serial { .. } => Box::new(super::serial::SerialLink::open(config, ctx)?),
    })
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr> {
    (host.trim(), port)
        .to_socket_addrs()
        .map_err(|e| CoreError::Other(format!("{host}: {e}")))?
        .next()
        .ok_or_else(|| CoreError::Other(format!("{host}: no address")))
}

/// Read until EOF/error, reporting every chunk. Returns when the stream ends.
fn pump(mut s: TcpStream, ctx: &Ctx, peer: Option<String>, stop: &AtomicBool) {
    let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
    let mut buf = vec![0u8; READ_BUF];
    while !stop.load(Ordering::Relaxed) {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => ctx.data(Dir::Rx, peer.clone(), &buf[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
}

// ------------------------------------------------------------ TCP client ---

struct TcpClient {
    addr: SocketAddr,
    label: String,
    stream: Option<TcpStream>,
    alive: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    auto_reconnect: bool,
    next_retry: Instant,
    ctx: Arc<Ctx>,
}

impl TcpClient {
    fn connect(host: &str, port: u16, auto_reconnect: bool, ctx: Arc<Ctx>) -> Result<Self> {
        let addr = resolve(host, port)?;
        let mut c = TcpClient {
            addr,
            label: format!("{host}:{port}"),
            stream: None,
            alive: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            auto_reconnect,
            next_retry: Instant::now(),
            ctx,
        };
        c.dial()?;
        Ok(c)
    }

    fn dial(&mut self) -> Result<()> {
        self.ctx
            .state("connecting", format!("connecting to {}", self.label));
        let s = TcpStream::connect_timeout(&self.addr, Duration::from_secs(5))
            .map_err(|e| CoreError::Other(format!("{}: {e}", self.label)))?;
        let _ = s.set_nodelay(true);
        let reader = s.try_clone()?;
        self.stream = Some(s);
        self.alive.store(true, Ordering::SeqCst);
        let (ctx, alive, stop, peer) = (
            self.ctx.clone(),
            self.alive.clone(),
            self.stop.clone(),
            self.addr.to_string(),
        );
        std::thread::spawn(move || {
            pump(reader, &ctx, Some(peer.clone()), &stop);
            if !stop.load(Ordering::Relaxed) {
                alive.store(false, Ordering::SeqCst);
                ctx.state("closed", format!("{peer} closed the connection"));
            }
        });
        self.ctx
            .state("open", format!("connected to {}", self.addr));
        Ok(())
    }
}

impl Link for TcpClient {
    fn send(&mut self, data: &[u8], _target: Option<&str>) -> Result<()> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(CoreError::Other("not connected".into()));
        }
        let s = self
            .stream
            .as_mut()
            .ok_or_else(|| CoreError::Other("not connected".into()))?;
        s.write_all(data)
            .map_err(|e| CoreError::Other(e.to_string()))
    }

    fn tick(&mut self) {
        if self.auto_reconnect
            && !self.alive.load(Ordering::SeqCst)
            && Instant::now() >= self.next_retry
        {
            self.next_retry = Instant::now() + Duration::from_secs(3);
            if let Err(e) = self.dial() {
                self.ctx.state("error", format!("{e} — retrying in 3 s"));
            }
        }
    }

    fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(s) = self.stream.take() {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

// ------------------------------------------------------------ TCP server ---

type Clients = Arc<Mutex<Vec<(String, TcpStream)>>>;

struct TcpServer {
    clients: Clients,
    stop: Arc<AtomicBool>,
}

fn publish_clients(ctx: &Ctx, clients: &Clients) {
    let list = clients
        .lock()
        .map(|c| c.iter().map(|(a, _)| a.clone()).collect())
        .unwrap_or_default();
    ctx.emit(ForgeEvent::Clients {
        session: ctx.id,
        clients: list,
    });
}

impl TcpServer {
    fn start(bind: &str, port: u16, max_clients: u16, ctx: Arc<Ctx>) -> Result<Self> {
        let ip: IpAddr = bind.trim().parse().map_err(|_| {
            CoreError::Refused(format!("bind address \"{bind}\" is not an IP address"))
        })?;
        let listener = TcpListener::bind((ip, port))
            .map_err(|e| CoreError::Other(format!("cannot listen on {ip}:{port}: {e}")))?;
        listener.set_nonblocking(true)?;
        let clients: Clients = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        ctx.state(
            "listening",
            format!("listening on {}", listener.local_addr()?),
        );
        let (c, s) = (clients.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        let full = c
                            .lock()
                            .map(|v| v.len() >= usize::from(max_clients))
                            .unwrap_or(true);
                        if full {
                            ctx.state(
                                "error",
                                format!("rejected {peer}: client limit ({max_clients}) reached"),
                            );
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_nodelay(true);
                        let peer_s = peer.to_string();
                        if let (Ok(reader), Ok(mut list)) = (stream.try_clone(), c.lock()) {
                            list.push((peer_s.clone(), stream));
                            drop(list);
                            ctx.state("open", format!("{peer_s} connected"));
                            publish_clients(&ctx, &c);
                            let (ctx2, c2, s2) = (ctx.clone(), c.clone(), s.clone());
                            std::thread::spawn(move || {
                                pump(reader, &ctx2, Some(peer_s.clone()), &s2);
                                if let Ok(mut l) = c2.lock() {
                                    l.retain(|(a, _)| *a != peer_s);
                                }
                                if !s2.load(Ordering::Relaxed) {
                                    ctx2.state("closed", format!("{peer_s} disconnected"));
                                    publish_clients(&ctx2, &c2);
                                }
                            });
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50))
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(200)),
                }
            }
        });
        Ok(TcpServer { clients, stop })
    }
}

impl Link for TcpServer {
    /// `target` = a client address, or None for every connected client.
    fn send(&mut self, data: &[u8], target: Option<&str>) -> Result<()> {
        let mut list = self
            .clients
            .lock()
            .map_err(|_| CoreError::Other("client list poisoned".into()))?;
        if list.is_empty() {
            return Err(CoreError::Other("no clients connected".into()));
        }
        let mut sent = 0;
        for (addr, s) in list.iter_mut() {
            if target.is_none_or(|t| t == addr) && s.write_all(data).is_ok() {
                sent += 1;
            }
        }
        if sent == 0 {
            return Err(CoreError::Other(match target {
                Some(t) => format!("client {t} is not connected"),
                None => "send failed".into(),
            }));
        }
        Ok(())
    }

    fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(list) = self.clients.lock() {
            for (_, s) in list.iter() {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
    }
}

// -------------------------------------------------------------------- UDP ---

struct Udp {
    sock: UdpSocket,
    remote: Option<SocketAddr>,
    last_peer: Arc<Mutex<Option<SocketAddr>>>,
    stop: Arc<AtomicBool>,
}

impl Udp {
    fn start(
        bind: &str,
        port: u16,
        remote_host: Option<&str>,
        remote_port: Option<u16>,
        broadcast: bool,
        group: Option<&str>,
        ctx: Arc<Ctx>,
    ) -> Result<Self> {
        let ip: IpAddr = bind.trim().parse().map_err(|_| {
            CoreError::Refused(format!("bind address \"{bind}\" is not an IP address"))
        })?;
        let sock = UdpSocket::bind((ip, port))
            .map_err(|e| CoreError::Other(format!("cannot bind UDP {ip}:{port}: {e}")))?;
        sock.set_broadcast(broadcast)?;
        if let Some(g) = group {
            let g: Ipv4Addr = g.trim().parse().map_err(|_| {
                CoreError::Refused(format!("\"{g}\" is not an IPv4 multicast group"))
            })?;
            if !g.is_multicast() {
                return Err(CoreError::Refused(format!(
                    "{g} is not a multicast address (224.0.0.0/4)"
                )));
            }
            sock.join_multicast_v4(&g, &Ipv4Addr::UNSPECIFIED)
                .map_err(|e| CoreError::Other(format!("join {g}: {e}")))?;
        }
        let remote = match (remote_host, remote_port) {
            (Some(h), Some(p)) => Some(resolve(h, p)?),
            _ => None,
        };
        sock.set_read_timeout(Some(Duration::from_millis(200)))?;
        let reader = sock.try_clone()?;
        let last_peer = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        ctx.state("listening", format!("UDP bound to {}", sock.local_addr()?));
        let (lp, s) = (last_peer.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 65_536];
            while !s.load(Ordering::Relaxed) {
                if let Ok((n, src)) = reader.recv_from(&mut buf) {
                    if let Ok(mut l) = lp.lock() {
                        *l = Some(src);
                    }
                    ctx.data(Dir::Rx, Some(src.to_string()), &buf[..n]);
                }
            }
        });
        Ok(Udp {
            sock,
            remote,
            last_peer,
            stop,
        })
    }
}

impl Link for Udp {
    /// `target`: "ip:port", "last" (reply to the last sender) or None (configured remote).
    fn send(&mut self, data: &[u8], target: Option<&str>) -> Result<()> {
        let dest = match target {
            Some("last") => self
                .last_peer
                .lock()
                .ok()
                .and_then(|l| *l)
                .ok_or_else(|| CoreError::Other("nothing received yet to reply to".into()))?,
            Some(t) => t.trim().parse::<SocketAddr>().or_else(|_| {
                let (h, p) = t.rsplit_once(':').ok_or_else(|| {
                    CoreError::Refused(format!("target \"{t}\" must be host:port"))
                })?;
                resolve(
                    h,
                    p.parse()
                        .map_err(|_| CoreError::Refused(format!("bad port in \"{t}\"")))?,
                )
            })?,
            None => self.remote.ok_or_else(|| {
                CoreError::Other("no remote address configured — set one or choose a target".into())
            })?,
        };
        if data.len() > 65_507 {
            return Err(CoreError::Refused(
                "UDP payload is limited to 65507 bytes".into(),
            ));
        }
        self.sock
            .send_to(data, dest)
            .map(|_| ())
            .map_err(|e| CoreError::Other(format!("{dest}: {e}")))
    }

    fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use std::sync::mpsc;

    fn forge() -> (Forge, mpsc::Receiver<ForgeEvent>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let sink: EventSink = Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        (Forge::new(sink), rx)
    }

    fn wait_rx(rx: &mpsc::Receiver<ForgeEvent>, session: u32, want: &[u8]) -> Option<String> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let b64 = crate::util::base64(want);
        while Instant::now() < deadline {
            if let Ok(ForgeEvent::Data {
                session: s,
                dir: Dir::Rx,
                peer,
                data,
                ..
            }) = rx.recv_timeout(Duration::from_millis(100))
            {
                if s == session && data == b64 {
                    return peer;
                }
            }
        }
        None
    }

    #[test]
    fn tcp_server_and_client_talk() {
        let (f, rx) = forge();
        let port = 47_100 + (std::process::id() % 500) as u16;
        let srv = f
            .open(SessionConfig::TcpServer {
                bind: "127.0.0.1".into(),
                port,
                max_clients: 4,
            })
            .unwrap();
        let cli = f
            .open(SessionConfig::TcpClient {
                host: "127.0.0.1".into(),
                port,
                auto_reconnect: false,
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        f.command(
            cli.id,
            Command::Send {
                bytes: b"PING\r\n".to_vec(),
                target: None,
            },
        )
        .unwrap();
        let peer = wait_rx(&rx, srv.id, b"PING\r\n").expect("server got data");
        f.command(
            srv.id,
            Command::Send {
                bytes: vec![0x01, 0xFF],
                target: Some(peer),
            },
        )
        .unwrap();
        assert!(
            wait_rx(&rx, cli.id, &[0x01, 0xFF]).is_some(),
            "client got the reply"
        );
        assert_eq!(f.list().len(), 2);
        f.close_all();
        assert!(f.list().is_empty());
    }

    #[test]
    fn udp_echo_and_reply_to_last() {
        let (f, rx) = forge();
        let pa = 47_700 + (std::process::id() % 500) as u16;
        let a = f
            .open(SessionConfig::Udp {
                bind: "127.0.0.1".into(),
                local_port: pa,
                remote_host: None,
                remote_port: None,
                broadcast: false,
                multicast_group: None,
            })
            .unwrap();
        let b = f
            .open(SessionConfig::Udp {
                bind: "127.0.0.1".into(),
                local_port: 0,
                remote_host: Some("127.0.0.1".into()),
                remote_port: Some(pa),
                broadcast: false,
                multicast_group: None,
            })
            .unwrap();
        f.command(
            b.id,
            Command::Send {
                bytes: b"hello".to_vec(),
                target: None,
            },
        )
        .unwrap();
        assert!(wait_rx(&rx, a.id, b"hello").is_some());
        f.command(
            a.id,
            Command::Send {
                bytes: b"back".to_vec(),
                target: Some("last".into()),
            },
        )
        .unwrap();
        assert!(wait_rx(&rx, b.id, b"back").is_some());
        f.command(
            a.id,
            Command::Repeat {
                bytes: b"r".to_vec(),
                target: Some("last".into()),
                interval_ms: 20,
                count: Some(3),
            },
        )
        .unwrap();
        for _ in 0..3 {
            assert!(
                wait_rx(&rx, b.id, b"r").is_some(),
                "repeat delivers 3 packets"
            );
        }
        f.close_all();
    }

    #[test]
    fn log_file_records_traffic() {
        let (f, rx) = forge();
        let pa = 48_300 + (std::process::id() % 500) as u16;
        let a = f
            .open(SessionConfig::Udp {
                bind: "127.0.0.1".into(),
                local_port: pa,
                remote_host: Some("127.0.0.1".into()),
                remote_port: Some(pa),
                broadcast: false,
                multicast_group: None,
            })
            .unwrap();
        let log = std::env::temp_dir().join(format!("forge-{}.log", std::process::id()));
        f.command(
            a.id,
            Command::Log {
                path: Some(log.display().to_string()),
            },
        )
        .unwrap();
        f.command(
            a.id,
            Command::Send {
                bytes: b"AT\r\n".to_vec(),
                target: None,
            },
        )
        .unwrap();
        assert!(wait_rx(&rx, a.id, b"AT\r\n").is_some());
        f.close(a.id);
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            text.contains("TX") && text.contains("41 54 0D 0A") && text.contains(r"|AT\r\n|"),
            "{text}"
        );
        std::fs::remove_file(log).unwrap();
    }
}
