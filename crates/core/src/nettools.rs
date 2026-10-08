// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/nettools.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Network diagnostics: ping (with live per-reply output), route
//           tracing (tracepath, no root needed), DNS lookup and TCP port
//           checks. Host names are validated before reaching any tool.
// =============================================================================

use std::io::{BufRead, BufReader};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{CoreError, Result};

/// Hostname or IP literal: letters, digits, '.', '-', ':' (IPv6), '%' (zone).
pub fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && !h.starts_with('-')
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-:%_".contains(c))
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PingSummary {
    pub host: String,
    pub address: Option<String>,
    pub sent: u32,
    pub received: u32,
    pub loss_percent: f64,
    pub min_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Mean deviation reported by ping (jitter-like).
    pub mdev_ms: Option<f64>,
    pub rtts: Vec<Option<f64>>,
}

/// Run ping; `on_line` receives every output line as it arrives.
pub fn ping(
    host: &str,
    count: u32,
    interval_ms: u32,
    cancel: &AtomicBool,
    on_line: impl Fn(&str),
) -> Result<PingSummary> {
    if !valid_host(host) {
        return Err(CoreError::Refused("invalid host name".into()));
    }
    let count = count.clamp(1, 1000).to_string();
    // Unprivileged users may not ping faster than every 200 ms.
    let interval = format!("{:.1}", (interval_ms.max(200) as f64) / 1000.0);
    let mut child = Command::new("ping")
        .args(["-c", &count, "-i", &interval, "-W", "2", "-O", host])
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::Other(format!("ping: {e}")))?;
    let mut s = PingSummary {
        host: host.to_string(),
        ..Default::default()
    };
    let stdout = child.stdout.take().unwrap();
    for line in BufReader::new(stdout)
        .lines()
        .map_while(std::result::Result::ok)
    {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            break;
        }
        on_line(&line);
        if line.starts_with("PING ") {
            s.address = line.split(['(', ')']).nth(1).map(str::to_string);
        } else if let Some(t) = line.split("time=").nth(1) {
            s.rtts
                .push(t.split_whitespace().next().and_then(|v| v.parse().ok()));
        } else if line.starts_with("no answer yet") {
            s.rtts.push(None);
        } else if line.contains("packets transmitted") {
            let nums: Vec<f64> = line
                .split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .filter_map(|x| x.parse().ok())
                .collect();
            if nums.len() >= 3 {
                s.sent = nums[0] as u32;
                s.received = nums[1] as u32;
                s.loss_percent = nums[2];
            }
        } else if line.starts_with("rtt ") || line.starts_with("round-trip") {
            if let Some(v) = line.split('=').nth(1) {
                let p: Vec<f64> = v
                    .trim()
                    .trim_end_matches(" ms")
                    .split('/')
                    .filter_map(|x| x.trim().parse().ok())
                    .collect();
                if p.len() == 4 {
                    (s.min_ms, s.avg_ms, s.max_ms, s.mdev_ms) =
                        (Some(p[0]), Some(p[1]), Some(p[2]), Some(p[3]));
                }
            }
        }
    }
    let status = child.wait();
    if s.sent == 0 && s.rtts.is_empty() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut err);
        }
        if !cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Other(if err.trim().is_empty() {
                format!("ping failed ({:?})", status.ok())
            } else {
                err.trim().to_string()
            }));
        }
    }
    Ok(s)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hop {
    pub ttl: u32,
    pub host: Option<String>,
    pub rtt_ms: Option<f64>,
    pub note: String,
}

/// Trace the route with `tracepath` (UDP, no root needed).
pub fn trace(host: &str, cancel: &AtomicBool, on_line: impl Fn(&str)) -> Result<Vec<Hop>> {
    if !valid_host(host) {
        return Err(CoreError::Refused("invalid host name".into()));
    }
    let mut child = Command::new("tracepath")
        .args(["-n", "-m", "30", host])
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| CoreError::Other(format!("tracepath: {e}")))?;
    let mut hops: Vec<Hop> = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map_while(std::result::Result::ok)
    {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            break;
        }
        on_line(&line);
        // " 3:  10.1.0.1     4.123ms" | " 4:  no reply"
        let t = line.trim_start();
        let Some((ttl, rest)) = t.split_once(':') else {
            continue;
        };
        let Ok(ttl) = ttl.trim_end_matches('?').parse::<u32>() else {
            continue;
        };
        let parts: Vec<&str> = rest.split_whitespace().collect();
        let (host, rtt) = if rest.contains("no reply") {
            (None, None)
        } else {
            (
                parts.first().map(|s| s.to_string()),
                parts
                    .iter()
                    .find(|p| p.ends_with("ms"))
                    .and_then(|p| p.trim_end_matches("ms").parse().ok()),
            )
        };
        if host.as_deref() == Some("[LOCALHOST]") {
            continue;
        }
        let note = parts
            .iter()
            .skip(1)
            .filter(|p| !p.ends_with("ms"))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        match hops.iter_mut().find(|h| h.ttl == ttl) {
            // tracepath may print a hop twice (MTU probing); keep the answered one.
            Some(h) if h.host.is_none() => {
                *h = Hop {
                    ttl,
                    host,
                    rtt_ms: rtt,
                    note,
                }
            }
            Some(_) => {}
            None => hops.push(Hop {
                ttl,
                host,
                rtt_ms: rtt,
                note,
            }),
        }
    }
    let _ = child.wait();
    Ok(hops)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsResult {
    pub host: String,
    pub addresses: Vec<String>,
    pub elapsed_ms: f64,
    pub servers: Vec<String>,
}

/// Resolve a name with the system resolver; also report the DNS servers in use.
pub fn lookup(host: &str) -> Result<DnsResult> {
    if !valid_host(host) {
        return Err(CoreError::Refused("invalid host name".into()));
    }
    let t = Instant::now();
    let mut addrs: Vec<String> = (host, 0)
        .to_socket_addrs()
        .map_err(|e| CoreError::Other(format!("{host}: {e}")))?
        .map(|a| a.ip().to_string())
        .collect();
    addrs.dedup();
    let servers = Command::new("resolvectl")
        .arg("dns")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
                .flat_map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(DnsResult {
        host: host.to_string(),
        addresses: addrs,
        elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
        servers,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortResult {
    pub port: u16,
    /// open | closed | filtered
    pub state: &'static str,
    pub elapsed_ms: f64,
}

/// TCP connect check for up to 64 ports.
pub fn check_ports(host: &str, ports: &[u16], timeout_ms: u64) -> Result<Vec<PortResult>> {
    if !valid_host(host) {
        return Err(CoreError::Refused("invalid host name".into()));
    }
    if ports.is_empty() || ports.len() > 64 {
        return Err(CoreError::Refused("check between 1 and 64 ports".into()));
    }
    let ip: IpAddr = (host, 0)
        .to_socket_addrs()
        .map_err(|e| CoreError::Other(format!("{host}: {e}")))?
        .next()
        .ok_or_else(|| CoreError::Other(format!("{host}: no address")))?
        .ip();
    let timeout = Duration::from_millis(timeout_ms.clamp(100, 10_000));
    let results = std::thread::scope(|s| {
        let handles: Vec<_> = ports
            .iter()
            .map(|&port| {
                s.spawn(move || {
                    let t = Instant::now();
                    let state =
                        match TcpStream::connect_timeout(&SocketAddr::new(ip, port), timeout) {
                            Ok(_) => "open",
                            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => "closed",
                            Err(_) => "filtered",
                        };
                    PortResult {
                        port,
                        state,
                        elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
                    }
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_validation() {
        assert!(valid_host("example.com"));
        assert!(valid_host("192.168.1.1"));
        assert!(valid_host("fe80::1%enp3s0"));
        assert!(!valid_host("-c 100 x"));
        assert!(!valid_host("a;reboot"));
        assert!(!valid_host(""));
    }

    #[test]
    fn localhost_tools() {
        let p = ping("127.0.0.1", 2, 200, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(p.received, 2);
        assert!(p.avg_ms.is_some());
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let r = check_ports("127.0.0.1", &[port], 500).unwrap();
        assert_eq!(r[0].state, "open");
        assert!(!lookup("localhost").unwrap().addresses.is_empty());
    }
}
