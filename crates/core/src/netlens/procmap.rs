// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/procmap.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Which application owns a connection. Local TCP/UDP ports are
//           mapped to processes through /proc/net/{tcp,udp}{,6} socket inodes
//           and /proc/<pid>/fd (same source as the Connections page).
//           Sockets of other users' processes need root and stay unknown.
// =============================================================================

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// (is_tcp, local port) -> (process name, pid)
#[derive(Default)]
pub struct ProcMap {
    map: HashMap<(bool, u16), (String, u32)>,
    refreshed: Option<Instant>,
}


impl ProcMap {
    /// Rebuild the table (at most every `min_gap`). Returns true if rebuilt.
    pub fn refresh(&mut self, min_gap: Duration) -> bool {
        if self.refreshed.is_some_and(|t| t.elapsed() < min_gap) {
            return false;
        }
        self.refreshed = Some(Instant::now());
        let owners = crate::netconn::inode_owners();
        let mut names: HashMap<u32, String> = HashMap::new();
        let mut map = HashMap::new();
        for (tcp, file) in [
            (true, "/proc/net/tcp"),
            (true, "/proc/net/tcp6"),
            (false, "/proc/net/udp"),
            (false, "/proc/net/udp6"),
        ] {
            let Ok(text) = std::fs::read_to_string(file) else { continue };
            for line in text.lines().skip(1) {
                let f: Vec<&str> = line.split_whitespace().collect();
                if f.len() < 10 {
                    continue;
                }
                let Some((_, port)) = crate::netconn::decode_endpoint(f[1]) else { continue };
                let Some(&pid) = owners.get(f[9]) else { continue };
                let name = names
                    .entry(pid)
                    .or_insert_with(|| crate::util::read_trim(format!("/proc/{pid}/comm")).unwrap_or_default())
                    .clone();
                // Listening sockets lose to connected ones on the same port.
                let listening = f[3] == "0A";
                if listening && map.contains_key(&(tcp, port)) {
                    continue;
                }
                map.insert((tcp, port), (name, pid));
            }
        }
        self.map = map;
        true
    }

    pub fn lookup(&self, tcp: bool, port: u16) -> Option<(String, u32)> {
        self.map.get(&(tcp, port)).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_own_socket() {
        let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = s.local_addr().unwrap().port();
        let mut m = ProcMap::default();
        assert!(m.refresh(Duration::ZERO));
        let (_, pid) = m.lookup(false, port).expect("own socket");
        assert_eq!(pid, std::process::id());
    }
}
