// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/views.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Read models for the NetLens UI, built from the store on request:
//           packet list rows (only the visible window), packet details,
//           conversations, applications, protocol hierarchy, traffic series,
//           RTP streams, insights, and Follow Stream reassembly.
// =============================================================================

use std::collections::HashMap;
use std::net::IpAddr;

use serde::Serialize;

use super::dissect::{Node, Proto, Want};
use super::explain::{explain, Names};
use super::store::{pf, Store, NONE};

fn b64<S: serde::Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&crate::util::base64(v))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// Frame number (1-based).
    pub no: u64,
    /// Seconds since the first frame.
    pub time: f64,
    /// Seconds since the previous frame.
    pub delta: f64,
    /// Local wall-clock time "YYYY-MM-DD HH:MM:SS.nnnnnnnnn".
    pub clock: String,
    pub src: String,
    pub dst: String,
    pub proto: String,
    pub len: u32,
    pub info: String,
    pub color: &'static str,
    /// 0 none, 1 warning, 2 error
    pub problem: u8,
    pub app: String,
    pub dir: u8,
}

impl Store {
    fn label(&self, d: &super::dissect::Dissection) -> String {
        d.label.map(|l| l.to_string()).unwrap_or_else(|| d.proto.name().to_string())
    }

    fn addr(&self, s: &str, resolve: bool) -> String {
        if resolve {
            if let Ok(ip) = s.parse::<IpAddr>() {
                if let Some(h) = self.hostnames.get(&ip) {
                    return h.clone();
                }
            }
        }
        s.to_string()
    }

    pub fn rows(&self, start: usize, count: usize, resolve: bool) -> Vec<Row> {
        let end = (start + count.min(500)).min(self.shown());
        (start..end)
            .filter_map(|pos| self.shown_index(pos))
            .map(|i| {
                let p = &self.packets[i];
                let d = self.dissect(i, Want { info: true, ..Default::default() });
                let m = self.meta(i);
                Row {
                    no: m.number,
                    time: m.ts_rel,
                    delta: m.ts_delta,
                    clock: m.ts_abs.clone(),
                    src: self.addr(&d.src, resolve),
                    dst: self.addr(&d.dst, resolve),
                    proto: self.label(&d),
                    len: p.orig_len,
                    info: d.info.clone(),
                    color: d.proto.color(),
                    problem: if p.flags & pf::ERROR != 0 { 2 } else if p.flags & pf::WARN != 0 { 1 } else { 0 },
                    app: m.app.map(|a| a.0).unwrap_or_default(),
                    dir: p.dir,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketDetail {
    pub no: u64,
    pub summary: String,
    pub proto: String,
    pub tree: Vec<Node>,
    #[serde(serialize_with = "b64")]
    pub bytes: Vec<u8>,
    pub stream: Option<u32>,
    pub tcp: bool,
    pub time: String,
}

impl Store {
    pub fn detail(&self, index: usize) -> Option<PacketDetail> {
        let p = self.packets.get(index)?;
        let d = self.dissect(index, Want { tree: true, info: true, fields: false });
        let m = self.meta(index);
        let names = Names {
            hostnames: &self.hostnames,
            local: &self.local_ips,
        };
        Some(PacketDetail {
            no: m.number,
            summary: explain(&d, &m, &names),
            proto: self.label(&d),
            tree: d.tree.clone(),
            bytes: self.data(p).to_vec(),
            stream: m.stream,
            tcp: d.facts.tcp.is_some(),
            time: m.ts_abs.clone(),
        })
    }
}

// --------------------------------------------------------- conversations ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: u32,
    pub transport: &'static str,
    pub protocol: String,
    pub a: String,
    pub a_port: u16,
    pub b: String,
    pub b_port: u16,
    pub host: String,
    pub app: String,
    pub packets_ab: u64,
    pub packets_ba: u64,
    pub bytes_ab: u64,
    pub bytes_ba: u64,
    pub start: f64,
    pub duration: f64,
    pub state: &'static str,
    pub problems: u32,
    pub service: String,
}

impl Store {
    pub fn conversations(&self) -> Vec<Conversation> {
        let first = self.first_ns.unwrap_or(0);
        self.streams
            .iter()
            .map(|s| {
                let state = if !s.tcp {
                    "flow"
                } else if s.rst {
                    "reset"
                } else if s.closed() {
                    "closed"
                } else if s.syn {
                    "open"
                } else {
                    "mid-stream"
                };
                let svc = super::explain::service(s.b.1).or(super::explain::service(s.a.1)).unwrap_or("");
                Conversation {
                    id: s.id,
                    transport: if s.tcp { "TCP" } else { "UDP" },
                    protocol: s.proto.name().to_string(),
                    a: s.a.0.to_string(),
                    a_port: s.a.1,
                    b: s.b.0.to_string(),
                    b_port: s.b.1,
                    host: s.host.clone().unwrap_or_default(),
                    app: s.app.as_ref().map(|a| a.0.clone()).unwrap_or_default(),
                    packets_ab: s.packets[0],
                    packets_ba: s.packets[1],
                    bytes_ab: s.bytes[0],
                    bytes_ba: s.bytes[1],
                    start: s.first_ns.saturating_sub(first) as f64 / 1e9,
                    duration: s.last_ns.saturating_sub(s.first_ns) as f64 / 1e9,
                    state,
                    problems: s.problems,
                    service: svc.to_string(),
                }
            })
            .collect()
    }
}

// -------------------------------------------------------------- apps ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    pub name: String,
    pub pids: Vec<u32>,
    pub connections: u32,
    pub packets: u64,
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub hosts: Vec<String>,
    pub problems: u32,
}

impl Store {
    pub fn apps(&self) -> Vec<AppUsage> {
        let mut map: HashMap<String, AppUsage> = HashMap::new();
        for s in &self.streams {
            let name = s.app.as_ref().map(|a| a.0.clone()).unwrap_or_else(|| {
                if self.local_ips.is_empty() {
                    "(not available for files)".into()
                } else if self.local_ips.contains(&s.a.0) || self.local_ips.contains(&s.b.0) {
                    "(unknown / system)".into()
                } else {
                    "(other devices)".into()
                }
            });
            let e = map.entry(name.clone()).or_insert_with(|| AppUsage {
                name,
                pids: Vec::new(),
                connections: 0,
                packets: 0,
                bytes_out: 0,
                bytes_in: 0,
                hosts: Vec::new(),
                problems: 0,
            });
            if let Some((_, pid)) = &s.app {
                if !e.pids.contains(pid) {
                    e.pids.push(*pid);
                }
            }
            e.connections += 1;
            e.packets += s.packets[0] + s.packets[1];
            // A = initiator; for local apps that is usually this PC.
            let a_local = self.local_ips.contains(&s.a.0);
            let (out, inn) = if a_local { (s.bytes[0], s.bytes[1]) } else { (s.bytes[1], s.bytes[0]) };
            e.bytes_out += out;
            e.bytes_in += inn;
            e.problems += s.problems;
            if let Some(h) = &s.host {
                if e.hosts.len() < 12 && !e.hosts.contains(h) {
                    e.hosts.push(h.clone());
                }
            }
        }
        let mut v: Vec<AppUsage> = map.into_values().collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.bytes_in + a.bytes_out));
        v
    }
}

// ------------------------------------------------------------- stats ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyRow {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub packets: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub hierarchy: Vec<HierarchyRow>,
    /// Per second: packets, bits.
    pub io_packets: Vec<u32>,
    pub io_bits: Vec<u64>,
    pub total_packets: u64,
    pub total_bytes: u64,
    pub duration: f64,
    pub protocols: Vec<(String, u64, u64)>,
}

fn layer_name(f: &str) -> String {
    match f {
        "eth" => "Ethernet".into(),
        "ip" => "IPv4".into(),
        "ipv6" => "IPv6".into(),
        "tcp" => "TCP".into(),
        "udp" => "UDP".into(),
        "tls" => "TLS".into(),
        "http" => "HTTP".into(),
        "rtsp" => "RTSP".into(),
        "rtp" => "RTP".into(),
        "rtcp" => "RTCP".into(),
        "sdp" => "SDP".into(),
        "dns" => "DNS".into(),
        "mdns" => "mDNS".into(),
        "arp" => "ARP".into(),
        "icmp" => "ICMP".into(),
        "icmpv6" => "ICMPv6".into(),
        "quic" => "QUIC".into(),
        "dhcp" => "DHCP".into(),
        "ssdp" => "SSDP".into(),
        "ntp" => "NTP".into(),
        "igmp" => "IGMP".into(),
        "vlan" => "802.1Q VLAN".into(),
        "sll" => "Linux cooked".into(),
        "llc" => "LLC".into(),
        "null" => "Loopback".into(),
        "data" => "Data".into(),
        o => o.to_uppercase(),
    }
}

impl Store {
    pub fn stats(&self) -> Stats {
        // Aggregate every prefix of every path.
        let mut agg: HashMap<String, (u64, u64)> = HashMap::new();
        for (path, (p, b)) in &self.hierarchy {
            let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            for i in 1..=parts.len() {
                let e = agg.entry(parts[..i].join("/")).or_default();
                e.0 += p;
                e.1 += b;
            }
        }
        let mut keys: Vec<&String> = agg.keys().collect();
        keys.sort();
        let hierarchy = keys
            .into_iter()
            .map(|k| {
                let (p, b) = agg[k];
                HierarchyRow {
                    name: layer_name(k.rsplit('/').next().unwrap_or(k)),
                    depth: k.matches('/').count(),
                    path: k.clone(),
                    packets: p,
                    bytes: b,
                }
            })
            .collect();
        let mut protos: HashMap<u8, (u64, u64)> = HashMap::new();
        for p in &self.packets {
            let e = protos.entry(p.proto).or_default();
            e.0 += 1;
            e.1 += p.orig_len as u64;
        }
        let mut protocols: Vec<(String, u64, u64)> = protos
            .into_iter()
            .map(|(k, (p, b))| (Proto::from_u8(k).name().to_string(), p, b))
            .collect();
        protocols.sort_by_key(|p| std::cmp::Reverse(p.2));
        let duration = match (self.packets.first(), self.packets.last()) {
            (Some(a), Some(b)) => b.ts_ns.saturating_sub(a.ts_ns) as f64 / 1e9,
            _ => 0.0,
        };
        Stats {
            hierarchy,
            io_packets: self.io.iter().map(|x| x.0).collect(),
            io_bits: self.io.iter().map(|x| x.1 * 8).collect(),
            total_packets: self.packets.len() as u64,
            total_bytes: self.total_bytes,
            duration,
            protocols,
        }
    }
}

// ---------------------------------------------------------- insights ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub id: &'static str,
    /// "error", "warning", "info"
    pub severity: &'static str,
    pub title: String,
    pub detail: &'static str,
    pub count: u64,
    pub filter: &'static str,
}

impl Store {
    pub fn insights(&self) -> Vec<Insight> {
        let c = &self.counters;
        let mut v = Vec::new();
        let mut add = |id, severity, count: u64, title: String, detail, filter| {
            if count > 0 {
                v.push(Insight { id, severity, title, detail, count, filter });
            }
        };
        add("retrans", "warning", c.retrans, format!("{} TCP retransmissions", c.retrans),
            "Data had to be sent again because it was lost or acknowledged too late. A few are normal; many mean a congested, lossy (Wi-Fi) or overloaded path.",
            "tcp.analysis.retransmission");
        add("lost", "warning", c.lost, format!("{} gaps in TCP data", c.lost),
            "Earlier segments were never seen: lost on the network, or the capture missed them (see dropped packets).",
            "tcp.analysis.lost_segment");
        add("dupack", "info", c.dup_ack, format!("{} duplicate ACKs", c.dup_ack),
            "Receivers repeated an acknowledgment to signal a missing segment. Usually followed by a retransmission.",
            "tcp.analysis.duplicate_ack");
        add("zerowin", "warning", c.zero_window, format!("{} zero-window events", c.zero_window),
            "A receiver's buffer was full, so the sender had to stop. The receiving application (or disk) is too slow.",
            "tcp.analysis.zero_window");
        add("ooo", "info", c.out_of_order, format!("{} out-of-order segments", c.out_of_order),
            "Packets arrived in a different order than sent, typical for multi-path or busy networks.",
            "tcp.analysis.out_of_order");
        add("resets", "warning", c.resets, format!("{} connection resets", c.resets),
            "Connections were refused (nothing listening on that port, or a firewall) or aborted.",
            "tcp.flags.reset == 1");
        add("dns", "error", c.dns_errors, format!("{} failed DNS lookups", c.dns_errors),
            "Names that could not be resolved (no such name or server failure). Check spelling or the DNS server.",
            "dns.flags.rcode != 0");
        add("dnsslow", "warning", c.dns_slow, format!("{} slow DNS answers (> 500 ms)", c.dns_slow),
            "Name lookups are slow: every new website or connection waits for them.",
            "dns.time > 0.5");
        add("http4", "warning", c.http_client_err, format!("{} HTTP client errors (4xx)", c.http_client_err),
            "Requests for missing pages, without permission, or malformed.",
            "http.response.code >= 400 && http.response.code < 500");
        add("http5", "error", c.http_server_err, format!("{} HTTP server errors (5xx)", c.http_server_err),
            "The web server failed to handle requests.",
            "http.response.code >= 500");
        add("rtspauth", "info", c.rtsp_auth, format!("{} RTSP authentication challenges (401)", c.rtsp_auth),
            "The camera/stream server asked for credentials. Normal once per session; repeated 401s mean wrong user name or password.",
            "rtsp.status == 401");
        add("rtsp", "error", c.rtsp_errors, format!("{} RTSP errors", c.rtsp_errors),
            "The streaming server rejected requests (wrong path, unsupported transport, expired session).",
            "rtsp.status >= 400 && rtsp.status != 401");
        add("tls", "error", c.tls_alerts, format!("{} failed TLS handshakes / fatal alerts", c.tls_alerts),
            "Encrypted connections were aborted: certificate problems, protocol mismatch or interception.",
            "tls.alert_message.level == 2");
        add("icmp", "warning", c.icmp_errors, format!("{} ICMP error messages", c.icmp_errors),
            "Routers or hosts reported that packets could not be delivered (unreachable, TTL exceeded).",
            "icmp.type in {3 11} || icmpv6.type in {1 2 3}");
        add("pli", "warning", c.keyframe_requests, format!("{} key-frame requests (PLI/FIR)", c.keyframe_requests),
            "Video receivers asked for a new full picture, usually after packet loss corrupted the image.",
            "rtcp.psfb.fmt in {1 4}");
        let rtp_lost: i64 = self.rtp_streams().iter().map(|s| s.lost).sum();
        add("rtploss", "warning", rtp_lost.max(0) as u64, format!("{rtp_lost} lost RTP media packets"),
            "Audio/video packets that never arrived: expect glitches, frozen or smeared video. See the RTP streams tab.",
            "rtp");
        add("drops", "error", self.dropped, format!("{} packets dropped by the capture", self.dropped),
            "The kernel could not hand packets to NetLens fast enough, so this capture is incomplete. Use a capture filter to reduce load.",
            "");
        v
    }
}

// ------------------------------------------------------------ follow ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    /// true when sent by endpoint A (the client).
    pub from_a: bool,
    pub frame: u64,
    #[serde(serialize_with = "b64")]
    pub data: Vec<u8>,
    /// Bytes missing before this chunk (not captured).
    pub gap: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowResult {
    pub stream: u32,
    pub tcp: bool,
    pub a: String,
    pub b: String,
    pub bytes_a: u64,
    pub bytes_b: u64,
    pub chunks: Vec<Chunk>,
    pub truncated: bool,
    pub filter: String,
}

const FOLLOW_MAX: u64 = 8 * 1024 * 1024;

impl Store {
    pub fn follow(&self, stream: u32) -> Option<FollowResult> {
        let s = self.streams.get(stream as usize)?;
        let mut next: [Option<u32>; 2] = [None, None];
        let mut chunks: Vec<Chunk> = Vec::new();
        let mut totals = [0u64; 2];
        let mut truncated = false;
        let start = s.first_frame as usize;
        for i in start..self.packets.len() {
            let p = &self.packets[i];
            if p.stream != stream {
                continue;
            }
            let dir = if p.flags & pf::REVERSE != 0 { 1 } else { 0 };
            let f = self.facts(i);
            let data = self.data(p);
            let (payload, gap) = if let Some(t) = f.tcp {
                if t.payload_len == 0 {
                    if t.flags & super::dissect::tcpflag::SYN != 0 {
                        next[dir] = Some(t.seq.wrapping_add(1));
                    }
                    continue;
                }
                let end = t.seq.wrapping_add(t.payload_len as u32);
                let mut skip = 0usize;
                let mut gap = 0u32;
                if let Some(n) = next[dir] {
                    let ahead = t.seq.wrapping_sub(n) as i32;
                    if ahead < 0 {
                        // retransmitted / overlapping bytes
                        let overlap = (-ahead) as usize;
                        if overlap >= t.payload_len {
                            continue;
                        }
                        skip = overlap;
                    } else if ahead > 0 {
                        gap = ahead as u32;
                    }
                }
                next[dir] = Some(end);
                let lo = (t.payload_off + skip).min(data.len());
                let hi = (t.payload_off + t.payload_len).min(data.len());
                (&data[lo..hi], gap)
            } else if let Some((_, _, l, off)) = f.udp {
                let lo = off.min(data.len());
                let hi = (off + l).min(data.len());
                (&data[lo..hi], 0)
            } else {
                continue;
            };
            if payload.is_empty() {
                continue;
            }
            totals[dir] += payload.len() as u64;
            if totals[0] + totals[1] > FOLLOW_MAX {
                truncated = true;
                break;
            }
            match chunks.last_mut() {
                Some(c) if c.from_a == (dir == 0) && gap == 0 && s.tcp => c.data.extend_from_slice(payload),
                _ => chunks.push(Chunk {
                    from_a: dir == 0,
                    frame: i as u64 + 1,
                    data: payload.to_vec(),
                    gap,
                }),
            }
        }
        let proto = if s.tcp { "tcp" } else { "udp" };
        Some(FollowResult {
            stream,
            tcp: s.tcp,
            a: format!("{}:{}", s.a.0, s.a.1),
            b: format!("{}:{}", s.b.0, s.b.1),
            bytes_a: totals[0],
            bytes_b: totals[1],
            chunks,
            truncated,
            filter: format!("{proto}.stream == {stream}"),
        })
    }

    /// Stream index of a frame, if it belongs to one.
    pub fn stream_of(&self, index: usize) -> Option<u32> {
        self.packets.get(index).map(|p| p.stream).filter(|s| *s != NONE)
    }
}
