// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netconn.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: System-wide network sockets (like `ss -tunap`): TCP/UDP over IPv4
//           and IPv6 with state and owning process. Sockets are read from
//           /proc/net/*, owners resolved by matching socket inodes in
//           /proc/<pid>/fd (processes of other users need root to resolve).
// =============================================================================

use std::collections::HashMap;
use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Socket {
    pub protocol: &'static str,
    pub local_address: String,
    pub local_port: u16,
    pub remote_address: String,
    pub remote_port: u16,
    /// TCP state; "UNCONN" for unconnected UDP sockets.
    pub state: String,
    pub pid: Option<u32>,
    pub process: Option<String>,
    pub user: String,
    /// Loopback / unspecified (0.0.0.0, ::) / remote classification for the UI.
    pub scope: &'static str,
}

/// Decode a /proc/net endpoint ("0100007F:0035" or 32 hex digits for IPv6).
pub fn decode_endpoint(s: &str) -> Option<(String, u16)> {
    let (ip, port) = s.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let addr = match ip.len() {
        8 => Ipv4Addr::from(u32::from_str_radix(ip, 16).ok()?.swap_bytes()).to_string(),
        32 => {
            let mut bytes = [0u8; 16];
            for w in 0..4 {
                let word = u32::from_str_radix(&ip[w * 8..w * 8 + 8], 16)
                    .ok()?
                    .swap_bytes();
                bytes[w * 4..w * 4 + 4].copy_from_slice(&word.to_be_bytes());
            }
            let a = Ipv6Addr::from(bytes);
            match a.to_ipv4_mapped() {
                Some(v4) => v4.to_string(),
                None => a.to_string(),
            }
        }
        _ => return None,
    };
    Some((addr, port))
}

pub fn tcp_state(hex: &str) -> &'static str {
    match hex {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        _ => "UNKNOWN",
    }
}

fn scope(remote: &str, local: &str) -> &'static str {
    let is_loop = |a: &str| a.starts_with("127.") || a == "::1";
    let is_any = |a: &str| a == "0.0.0.0" || a == "::";
    if is_loop(local) && (is_loop(remote) || is_any(remote)) {
        "local"
    } else if is_any(remote) {
        "listening"
    } else {
        "remote"
    }
}

/// socket inode -> pid, for every process whose fds we can read.
pub(crate) fn inode_owners() -> HashMap<String, u32> {
    let mut map = HashMap::new();
    let Ok(rd) = fs::read_dir("/proc") else {
        return map;
    };
    for e in rd.filter_map(Result::ok) {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(fds) = fs::read_dir(e.path().join("fd")) else {
            continue;
        };
        for fd in fds.filter_map(Result::ok) {
            if let Ok(t) = fs::read_link(fd.path()) {
                let t = t.to_string_lossy();
                if let Some(inode) = t.strip_prefix("socket:[").and_then(|x| x.strip_suffix(']')) {
                    map.insert(inode.to_string(), pid);
                }
            }
        }
    }
    map
}

fn user_names() -> HashMap<u32, String> {
    fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            Some((f.get(2)?.parse().ok()?, f.first()?.to_string()))
        })
        .collect()
}

pub fn list() -> Vec<Socket> {
    let owners = inode_owners();
    let users = user_names();
    let mut names: HashMap<u32, String> = HashMap::new();
    let mut out = Vec::new();
    for (proto, file) in [
        ("tcp", "/proc/net/tcp"),
        ("tcp6", "/proc/net/tcp6"),
        ("udp", "/proc/net/udp"),
        ("udp6", "/proc/net/udp6"),
    ] {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 {
                continue;
            }
            let (Some((la, lp)), Some((ra, rp))) = (decode_endpoint(f[1]), decode_endpoint(f[2]))
            else {
                continue;
            };
            let state = if proto.starts_with("tcp") {
                tcp_state(f[3]).to_string()
            } else if f[3] == "01" {
                "ESTABLISHED".into()
            } else {
                "UNCONN".into()
            };
            let uid: u32 = f[7].parse().unwrap_or(0);
            let pid = owners.get(f[9]).copied();
            let process = pid.map(|p| {
                names
                    .entry(p)
                    .or_insert_with(|| {
                        crate::util::read_trim(format!("/proc/{p}/comm")).unwrap_or_default()
                    })
                    .clone()
            });
            out.push(Socket {
                protocol: proto,
                scope: scope(&ra, &la),
                local_address: la,
                local_port: lp,
                remote_address: ra,
                remote_port: rp,
                state,
                pid,
                process,
                user: users.get(&uid).cloned().unwrap_or_else(|| uid.to_string()),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes() {
        assert_eq!(
            decode_endpoint("0100007F:0035"),
            Some(("127.0.0.1".into(), 53))
        );
        assert_eq!(
            decode_endpoint("00000000000000000000000001000000:1F90"),
            Some(("::1".into(), 8080))
        );
        assert_eq!(
            decode_endpoint("0000000000000000FFFF00000100007F:0050"),
            Some(("127.0.0.1".into(), 80))
        );
        assert_eq!(scope("0.0.0.0", "0.0.0.0"), "listening");
    }

    #[test]
    fn finds_own_listener() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let me = std::process::id();
        let s = list();
        assert!(s
            .iter()
            .any(|x| x.local_port == port && x.state == "LISTEN" && x.pid == Some(me)));
    }
}
