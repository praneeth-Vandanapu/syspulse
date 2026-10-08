// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/store.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: In-memory capture store and single-pass analysis.
//
//           Frames are kept in a chunked byte arena (no large reallocations)
//           with a compact 48-byte record each. While frames are added the
//           store also maintains, incrementally:
//             * conversations (TCP connections / UDP flows) with per-direction
//               packet and byte counts, owning application and host name;
//             * TCP analysis: retransmissions, out-of-order, lost segments,
//               duplicate ACKs, zero windows, keep-alives, port reuse;
//             * request -> response times for DNS, HTTP, RTSP and ping;
//             * RTSP SETUP -> RTP/RTCP port registration, SDP codecs;
//             * RTP stream quality (loss, sequence errors, RFC 3550 jitter);
//             * host names learned from DNS answers, TLS SNI and HTTP Host;
//             * protocol hierarchy and a per-second traffic series;
//             * counters behind the "Insights" panel.
//           Everything else (detail tree, Info column) is produced on demand
//           by re-dissecting only the frames that are on screen.
// =============================================================================

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;

use serde::Serialize;

use super::dissect::net::an;
use super::dissect::{self, media, tcpflag, Dissection, Facts, Meta, Proto, Shared, Want};
use super::filter::Filter;
use super::pcapfile::IfaceDesc;

const CHUNK: usize = 16 * 1024 * 1024;
pub const NONE: u32 = u32::MAX;

/// Per-frame flags.
pub mod pf {
    pub const ERROR: u8 = 0x01;
    pub const WARN: u8 = 0x02;
    /// Frame travels B -> A in its conversation.
    pub const REVERSE: u8 = 0x04;
}

#[derive(Debug, Clone, Copy)]
pub struct PacketRec {
    pub ts_ns: u64,
    pub off: u64,
    pub cap_len: u32,
    pub orig_len: u32,
    pub stream: u32,
    pub resp_us: u32,
    pub analysis: u16,
    pub iface: u8,
    /// 0 unknown, 1 received, 2 sent
    pub dir: u8,
    pub proto: u8,
    pub flags: u8,
}

#[derive(Debug, Clone, Copy, Default)]
struct TcpDir {
    isn: Option<u32>,
    next_seq: Option<u32>,
    last_data_ns: u64,
    last_ack: Option<u32>,
    last_win: u16,
    fin: bool,
}

#[derive(Debug, Clone)]
pub struct Stream {
    pub id: u32,
    pub tcp: bool,
    pub a: (IpAddr, u16),
    pub b: (IpAddr, u16),
    pub packets: [u64; 2],
    pub bytes: [u64; 2],
    pub first_ns: u64,
    pub last_ns: u64,
    pub first_frame: u32,
    pub app: Option<(String, u32)>,
    /// Local endpoint port to resolve the owning app (live captures).
    pub app_pending: Option<u16>,
    pub proto: Proto,
    pub host: Option<String>,
    pub syn: bool,
    pub rst: bool,
    pub problems: u32,
    pub tls_version: u16,
    tcpd: [TcpDir; 2],
}

impl Stream {
    pub fn closed(&self) -> bool {
        self.rst || (self.tcpd[0].fin && self.tcpd[1].fin)
    }
}

type StreamKey = (bool, (IpAddr, u16), (IpAddr, u16));

fn key(tcp: bool, x: (IpAddr, u16), y: (IpAddr, u16)) -> StreamKey {
    if x <= y {
        (tcp, x, y)
    } else {
        (tcp, y, x)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RtpStat {
    pub ssrc: u32,
    pub src: String,
    pub dst: String,
    pub payload_type: u8,
    pub codec: String,
    pub clock: u32,
    pub packets: u64,
    pub bytes: u64,
    pub lost: i64,
    pub sequence_errors: u64,
    pub max_jitter_ms: f64,
    pub mean_jitter_ms: f64,
    pub max_delta_ms: f64,
    pub frames: u64,
    pub key_frames: u64,
    pub first_ns: u64,
    pub last_ns: u64,
    pub first_frame: u32,
    #[serde(skip)]
    base_seq: u32,
    #[serde(skip)]
    max_seq: u32,
    #[serde(skip)]
    cycles: u32,
    #[serde(skip)]
    jitter: f64,
    #[serde(skip)]
    jitter_sum: f64,
    #[serde(skip)]
    prev_transit: Option<f64>,
    #[serde(skip)]
    prev_arrival: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Counters {
    pub retrans: u64,
    pub out_of_order: u64,
    pub lost: u64,
    pub dup_ack: u64,
    pub zero_window: u64,
    pub resets: u64,
    pub dns_errors: u64,
    pub dns_slow: u64,
    pub http_client_err: u64,
    pub http_server_err: u64,
    pub rtsp_errors: u64,
    pub rtsp_auth: u64,
    pub tls_alerts: u64,
    pub icmp_errors: u64,
    pub cleartext_auth: u64,
    pub keyframe_requests: u64,
}

#[derive(Default)]
pub struct Store {
    pub interfaces: Vec<IfaceDesc>,
    pub packets: Vec<PacketRec>,
    chunks: Vec<Vec<u8>>,
    pub bytes_stored: u64,
    /// Sum of on-the-wire lengths.
    pub total_bytes: u64,
    pub shared: Shared,
    pub streams: Vec<Stream>,
    index: HashMap<StreamKey, u32>,
    pub hostnames: HashMap<IpAddr, String>,
    pub local_ips: HashSet<IpAddr>,
    pending_dns: HashMap<(u32, u16), u64>,
    pending_icmp: HashMap<(IpAddr, IpAddr, u16, u16), u64>,
    pending_req: HashMap<u32, VecDeque<u64>>,
    rtp: HashMap<(u32, IpAddr, u16), RtpStat>,
    pub hierarchy: HashMap<String, (u64, u64)>,
    /// Per-second (packets, bytes) since the first frame.
    pub io: Vec<(u32, u64)>,
    pub counters: Counters,
    pub first_ns: Option<u64>,
    pub filter: Option<Filter>,
    /// Indices of frames matching the filter (None = all frames).
    pub view: Option<Vec<u32>>,
    pub received: u64,
    pub dropped: u64,
    pub unsaved: bool,
    pub source: String,
}


fn seq_lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

impl Store {
    pub fn new(interfaces: Vec<IfaceDesc>, source: &str) -> Store {
        Store {
            interfaces,
            source: source.to_string(),
            ..Default::default()
        }
    }

    pub fn data(&self, p: &PacketRec) -> &[u8] {
        let c = (p.off / CHUNK as u64) as usize;
        let o = (p.off % CHUNK as u64) as usize;
        &self.chunks[c][o..o + p.cap_len as usize]
    }

    fn push_bytes(&mut self, d: &[u8]) -> u64 {
        let need_new = self.chunks.last().is_none_or(|c| c.len() + d.len() > CHUNK);
        if need_new {
            self.chunks.push(Vec::with_capacity(CHUNK));
        }
        let ci = self.chunks.len() - 1;
        let c = &mut self.chunks[ci];
        let off = (ci * CHUNK + c.len()) as u64;
        c.extend_from_slice(d);
        self.bytes_stored += d.len() as u64;
        off
    }

    pub fn memory(&self) -> u64 {
        (self.chunks.len() * CHUNK) as u64 + (self.packets.len() * std::mem::size_of::<PacketRec>()) as u64
    }

    pub fn linktype(&self, p: &PacketRec) -> u16 {
        self.interfaces.get(p.iface as usize).map(|i| i.linktype).unwrap_or(1)
    }

    // ------------------------------------------------------- ingest ---

    /// Add one frame. Returns its index.
    pub fn add(&mut self, iface: u8, ts_ns: u64, orig_len: u32, outgoing: Option<bool>, data: &[u8]) -> u32 {
        let idx = self.packets.len() as u32;
        let first = *self.first_ns.get_or_insert(ts_ns);
        let lt = self.interfaces.get(iface as usize).map(|i| i.linktype).unwrap_or(1);
        let meta = Meta::default();
        let d = dissect::dissect(data, lt, Want::default(), &meta, &self.shared);
        let off = self.push_bytes(data);
        let mut rec = PacketRec {
            ts_ns,
            off,
            cap_len: data.len() as u32,
            orig_len: orig_len.max(data.len() as u32),
            stream: NONE,
            resp_us: NONE,
            analysis: 0,
            iface,
            dir: match outgoing {
                Some(true) => 2,
                Some(false) => 1,
                None => 0,
            },
            proto: d.proto as u8,
            flags: 0,
        };
        if rec.dir == 0 && !self.local_ips.is_empty() {
            if let Some(s) = d.facts.src {
                rec.dir = if self.local_ips.contains(&s) { 2 } else { 1 };
            }
        }
        self.analyse(&mut rec, idx, &d);
        // statistics
        let path = d.path.iter().filter(|p| **p != "frame").copied().collect::<Vec<_>>().join("/");
        let h = self.hierarchy.entry(path).or_default();
        h.0 += 1;
        h.1 += rec.orig_len as u64;
        let sec = ((ts_ns.saturating_sub(first)) / 1_000_000_000) as usize;
        if sec < 7 * 24 * 3600 {
            if self.io.len() <= sec {
                self.io.resize(sec + 1, (0, 0));
            }
            self.io[sec].0 += 1;
            self.io[sec].1 += rec.orig_len as u64;
        }
        self.total_bytes += rec.orig_len as u64;
        self.packets.push(rec);
        self.unsaved = true;
        // live filter
        if let Some(f) = &self.filter {
            let matched = self.matches(f, idx);
            if matched {
                self.view.get_or_insert_with(Vec::new).push(idx);
            }
        }
        idx
    }

    fn analyse(&mut self, rec: &mut PacketRec, idx: u32, d: &Dissection) {
        let f = &d.facts;
        let ts = rec.ts_ns;
        if f.icmp_error {
            self.counters.icmp_errors += 1;
            rec.flags |= pf::ERROR;
        }
        // Ping response time.
        if f.echo.is_some() {
            self.icmp_pair(rec, d);
        }
        if f.keyframe_request {
            self.counters.keyframe_requests += 1;
        }
        let (Some(src), Some(dst)) = (f.src, f.dst) else { return };
        let (tcp, sp, dp) = match (f.tcp, f.udp) {
            (Some(t), _) => (true, t.sport, t.dport),
            (None, Some((s, dd, _, _))) => (false, s, dd),
            _ => return,
        };
        let a = (src, sp);
        let b = (dst, dp);
        let k = key(tcp, a, b);
        let syn_only = f.tcp.is_some_and(|t| t.flags & (tcpflag::SYN | tcpflag::ACK) == tcpflag::SYN);
        let mut reuse = false;
        let sid = match self.index.get(&k).copied() {
            Some(id) if syn_only && self.streams[id as usize].closed() => {
                reuse = true;
                None
            }
            Some(id) => Some(id),
            None => None,
        };
        let sid = sid.unwrap_or_else(|| {
            let id = self.streams.len() as u32;
            let local_port = if self.local_ips.contains(&src) {
                Some(sp)
            } else if self.local_ips.contains(&dst) {
                Some(dp)
            } else {
                None
            };
            self.streams.push(Stream {
                id,
                tcp,
                a,
                b,
                packets: [0; 2],
                bytes: [0; 2],
                first_ns: ts,
                last_ns: ts,
                first_frame: idx,
                app: None,
                app_pending: local_port,
                proto: if tcp { Proto::Tcp } else { Proto::Udp },
                host: None,
                syn: false,
                rst: false,
                problems: 0,
                tls_version: 0,
                tcpd: [TcpDir::default(); 2],
            });
            self.index.insert(k, id);
            id
        });
        rec.stream = sid;
        let s = &mut self.streams[sid as usize];
        let dir = if s.a == a { 0 } else { 1 };
        if dir == 1 {
            rec.flags |= pf::REVERSE;
        }
        s.packets[dir] += 1;
        s.bytes[dir] += rec.orig_len as u64;
        s.last_ns = ts;
        if matches!(s.proto, Proto::Tcp | Proto::Udp) && !matches!(d.proto, Proto::Tcp | Proto::Udp) {
            s.proto = d.proto;
        }
        if reuse {
            rec.analysis |= an::PORT_REUSE;
        }

        // ---- TCP analysis
        if let Some(t) = f.tcp {
            if t.flags & tcpflag::SYN != 0 {
                s.syn = true;
            }
            if t.flags & tcpflag::RST != 0 {
                s.rst = true;
                self.counters.resets += 1;
                rec.flags |= pf::ERROR;
            }
            let seg_len = t.payload_len as u32
                + ((t.flags & tcpflag::SYN != 0) as u32)
                + ((t.flags & tcpflag::FIN != 0) as u32);
            let (me, other) = if dir == 0 { (0, 1) } else { (1, 0) };
            let st = &mut s.tcpd[me];
            if st.isn.is_none() {
                st.isn = Some(t.seq.wrapping_sub(if t.flags & tcpflag::SYN != 0 { 0 } else { 1 }));
            }
            if t.flags & tcpflag::FIN != 0 {
                st.fin = true;
            }
            let mut a_flags = 0u16;
            if t.flags & tcpflag::RST == 0 {
                if t.window == 0 && t.flags & (tcpflag::SYN | tcpflag::FIN) == 0 {
                    a_flags |= an::ZERO_WINDOW;
                }
                if seg_len > 0 {
                    match st.next_seq {
                        Some(next) if seq_lt(t.seq, next) => {
                            if t.payload_len <= 1 && t.seq == next.wrapping_sub(1) && t.flags & (tcpflag::SYN | tcpflag::FIN) == 0 {
                                a_flags |= an::KEEP_ALIVE;
                            } else if ts.saturating_sub(st.last_data_ns) < 3_000_000 && t.flags & tcpflag::SYN == 0 {
                                a_flags |= an::OUT_OF_ORDER;
                            } else {
                                a_flags |= an::RETRANSMISSION;
                            }
                        }
                        Some(next) if seq_lt(next, t.seq) => a_flags |= an::LOST_SEGMENT,
                        _ => {}
                    }
                    let end = t.seq.wrapping_add(seg_len);
                    if st.next_seq.is_none_or(|n| seq_lt(n, end)) {
                        st.next_seq = Some(end);
                    }
                    st.last_data_ns = ts;
                } else if t.flags & tcpflag::ACK != 0 && t.flags & (tcpflag::SYN | tcpflag::FIN) == 0
                    && st.last_ack == Some(t.ack) && st.last_win == t.window && s.tcpd[other].next_seq.is_some_and(|n| seq_lt(t.ack, n)) {
                        a_flags |= an::DUP_ACK;
                    }
                let st = &mut s.tcpd[me];
                if t.flags & tcpflag::ACK != 0 {
                    st.last_ack = Some(t.ack);
                    st.last_win = t.window;
                }
            }
            rec.analysis |= a_flags;
            if a_flags & an::PROBLEMS != 0 {
                s.problems += 1;
                rec.flags |= pf::WARN;
            }
            let c = &mut self.counters;
            if a_flags & an::RETRANSMISSION != 0 {
                c.retrans += 1;
            }
            if a_flags & an::OUT_OF_ORDER != 0 {
                c.out_of_order += 1;
            }
            if a_flags & an::LOST_SEGMENT != 0 {
                c.lost += 1;
            }
            if a_flags & an::DUP_ACK != 0 {
                c.dup_ack += 1;
            }
            if a_flags & an::ZERO_WINDOW != 0 {
                c.zero_window += 1;
            }
        }

        // ---- application layer
        if let Some(sni) = &f.tls_sni {
            s.host = Some(sni.clone());
            self.hostnames.entry(dst).or_insert_with(|| sni.clone());
        }
        if let Some(v) = f.tls_server_version {
            s.tls_version = v;
        }
        if let Some((lvl, desc)) = f.tls_alert {
            if lvl == 2 && desc != 0 {
                self.counters.tls_alerts += 1;
                rec.flags |= pf::ERROR;
                s.problems += 1;
            }
        }
        if let Some(h) = &f.http {
            if h.request {
                if !h.host.is_empty() {
                    s.host = Some(h.host.clone());
                    self.hostnames.entry(dst).or_insert_with(|| h.host.split(':').next().unwrap_or("").to_string());
                }
                let q = self.pending_req.entry(sid).or_default();
                if q.len() < 256 {
                    q.push_back(ts);
                }
            } else {
                if let Some(t0) = self.pending_req.get_mut(&sid).and_then(|q| q.pop_front()) {
                    rec.resp_us = (ts.saturating_sub(t0) / 1000).min(u32::MAX as u64 - 1) as u32;
                }
                if h.code >= 500 {
                    self.counters.http_server_err += 1;
                    rec.flags |= pf::ERROR;
                } else if h.code >= 400 {
                    self.counters.http_client_err += 1;
                    rec.flags |= pf::ERROR;
                }
            }
        }
        if let Some(r) = &f.rtsp {
            let s_rtsp_server = if r.request { (dst, dp) } else { (src, sp) };
            if !self.shared.rtsp_ports.contains(&s_rtsp_server) && self.shared.rtsp_ports.len() < 256 {
                self.shared.rtsp_ports.push(s_rtsp_server);
            }
            if r.request {
                let q = self.pending_req.entry(sid).or_default();
                if q.len() < 256 {
                    q.push_back(ts);
                }
            } else {
                if let Some(t0) = self.pending_req.get_mut(&sid).and_then(|q| q.pop_front()) {
                    rec.resp_us = (ts.saturating_sub(t0) / 1000).min(u32::MAX as u64 - 1) as u32;
                }
                if r.code == 401 {
                    self.counters.rtsp_auth += 1;
                } else if r.code >= 400 {
                    self.counters.rtsp_errors += 1;
                    rec.flags |= pf::ERROR;
                }
            }
        }
        if let Some(t) = &f.rtsp_transport {
            // Request: src = client; response: dst = client.
            let request = f.rtsp.as_ref().is_some_and(|r| r.request);
            let (client, server) = if request { (src, dst) } else { (dst, src) };
            let mut reg = |ip: IpAddr, ports: Option<(u16, u16)>| {
                if let Some((rtp_p, rtcp_p)) = ports {
                    self.shared.rtp_ports.insert((ip, rtp_p), false);
                    self.shared.rtp_ports.insert((ip, rtcp_p), true);
                }
            };
            if !t.interleaved {
                reg(t.destination.unwrap_or(client), t.client_ports);
                reg(server, t.server_ports);
            }
        }
        for (pt, codec, clock) in &f.rtpmap {
            self.shared.rtpmap.insert(*pt, (codec.clone(), *clock));
        }
        if let Some(dns) = &f.dns {
            let k = (sid, dns.id);
            if !dns.response {
                if self.pending_dns.len() < 100_000 {
                    self.pending_dns.insert(k, ts);
                }
            } else {
                if let Some(t0) = self.pending_dns.remove(&k) {
                    let us = ts.saturating_sub(t0) / 1000;
                    rec.resp_us = us.min(u32::MAX as u64 - 1) as u32;
                    if us > 500_000 {
                        self.counters.dns_slow += 1;
                        rec.flags |= pf::WARN;
                    }
                }
                if dns.rcode != 0 && d.proto == Proto::Dns {
                    self.counters.dns_errors += 1;
                    rec.flags |= pf::ERROR;
                }
                for (name, ip) in &dns.answers {
                    if self.hostnames.len() < 200_000 {
                        self.hostnames.insert(*ip, name.clone());
                    }
                }
                if !dns.qname.is_empty() {
                    if let Some((_, ip)) = dns.answers.first() {
                        self.hostnames.insert(*ip, dns.qname.clone());
                    }
                }
            }
        }
        if let Some(r) = f.rtp {
            self.rtp_update(rec, idx, src, sp, dst, dp, r);
        }
        if self.streams[sid as usize].host.is_none() {
            let host = self.hostnames.get(&dst).or_else(|| self.hostnames.get(&src)).cloned();
            self.streams[sid as usize].host = host;
        }
    }

    fn icmp_pair(&mut self, rec: &mut PacketRec, d: &Dissection) {
        let (Some(src), Some(dst), Some((request, id, seq))) = (d.facts.src, d.facts.dst, d.facts.echo) else {
            return;
        };
        if request {
            if self.pending_icmp.len() < 100_000 {
                self.pending_icmp.insert((src, dst, id, seq), rec.ts_ns);
            }
        } else if let Some(t0) = self.pending_icmp.remove(&(dst, src, id, seq)) {
            rec.resp_us = (rec.ts_ns.saturating_sub(t0) / 1000).min(u32::MAX as u64 - 1) as u32;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rtp_update(&mut self, rec: &mut PacketRec, idx: u32, src: IpAddr, sp: u16, dst: IpAddr, dp: u16, r: dissect::RtpFacts) {
        let (codec, clock) = media::codec_for(&self.shared, r.pt);
        let st = self.rtp.entry((r.ssrc, src, sp)).or_insert_with(|| RtpStat {
            ssrc: r.ssrc,
            src: format!("{src}:{sp}"),
            dst: format!("{dst}:{dp}"),
            payload_type: r.pt,
            codec: codec.clone(),
            clock,
            first_ns: rec.ts_ns,
            first_frame: idx,
            base_seq: r.seq as u32,
            max_seq: r.seq as u32,
            ..Default::default()
        });
        st.codec = codec;
        st.clock = clock;
        st.packets += 1;
        st.bytes += r.payload_len as u64;
        if r.marker {
            st.frames += 1;
        }
        if r.keyframe {
            st.key_frames += 1;
        }
        // Sequence tracking (RFC 3550 A.1, simplified).
        if st.packets > 1 {
            let max16 = (st.max_seq & 0xffff) as u16;
            let delta = r.seq.wrapping_sub(max16);
            if delta > 0 && delta < 3000 {
                if r.seq < max16 {
                    st.cycles += 1;
                }
                st.max_seq = (st.cycles << 16) | r.seq as u32;
                if delta > 1 {
                    st.sequence_errors += 1;
                    rec.flags |= pf::WARN;
                }
            } else if delta == 0 {
                st.sequence_errors += 1; // duplicate
            } else {
                st.sequence_errors += 1; // late / out of order
            }
        }
        let expected = st.max_seq.wrapping_sub(st.base_seq) as i64 + 1;
        st.lost = (expected - st.packets as i64).max(0);
        // Jitter.
        let arrival = rec.ts_ns as f64 * clock as f64 / 1e9;
        let transit = arrival - r.timestamp as f64;
        if let Some(prev) = st.prev_transit {
            let mut dd = (transit - prev).abs();
            if dd > clock as f64 * 10.0 {
                dd = 0.0; // timestamp wrap or stream reset
            }
            st.jitter += (dd - st.jitter) / 16.0;
            let ms = st.jitter * 1000.0 / clock.max(1) as f64;
            st.jitter_sum += ms;
            st.max_jitter_ms = st.max_jitter_ms.max(ms);
            st.mean_jitter_ms = st.jitter_sum / (st.packets - 1).max(1) as f64;
            let gap = (rec.ts_ns.saturating_sub(st.prev_arrival)) as f64 / 1e6;
            st.max_delta_ms = st.max_delta_ms.max(gap);
        }
        st.prev_transit = Some(transit);
        st.prev_arrival = rec.ts_ns;
        st.last_ns = rec.ts_ns;
    }

    pub fn rtp_streams(&self) -> Vec<RtpStat> {
        let mut v: Vec<RtpStat> = self.rtp.values().cloned().collect();
        v.sort_by_key(|s| s.first_frame);
        v
    }

    // -------------------------------------------------------- views ---

    pub fn meta(&self, i: usize) -> Meta {
        let p = &self.packets[i];
        let first = self.first_ns.unwrap_or(p.ts_ns);
        let prev = if i > 0 { self.packets[i - 1].ts_ns } else { p.ts_ns };
        let stream = (p.stream != NONE).then(|| &self.streams[p.stream as usize]);
        let rev = p.flags & pf::REVERSE != 0;
        Meta {
            number: i as u64 + 1,
            orig_len: p.orig_len,
            ts_rel: p.ts_ns.saturating_sub(first) as f64 / 1e9,
            ts_delta: p.ts_ns.saturating_sub(prev) as f64 / 1e9,
            ts_abs: super::fmt_time(p.ts_ns),
            interface: self.interfaces.get(p.iface as usize).map(|i| i.name.clone()).unwrap_or_default(),
            outgoing: match p.dir {
                1 => Some(false),
                2 => Some(true),
                _ => None,
            },
            app: stream.and_then(|s| s.app.clone()),
            stream: (p.stream != NONE).then_some(p.stream),
            analysis: p.analysis,
            response_time: (p.resp_us != NONE).then(|| p.resp_us as f64 / 1e6),
            isn: stream.filter(|s| s.tcp).and_then(|s| {
                let (me, other) = if rev { (1, 0) } else { (0, 1) };
                Some((s.tcpd[me].isn?, s.tcpd[other].isn.unwrap_or(0)))
            }),
            tls_version: stream.map(|s| s.tls_version).unwrap_or(0),
        }
    }

    pub fn dissect(&self, i: usize, want: Want) -> Dissection {
        let p = &self.packets[i];
        let meta = self.meta(i);
        dissect::dissect(self.data(p), self.linktype(p), want, &meta, &self.shared)
    }

    pub fn matches(&self, f: &Filter, i: u32) -> bool {
        let d = self.dissect(i as usize, Want { fields: true, ..Default::default() });
        f.matches(&d.fields, self.data(&self.packets[i as usize]))
    }

    /// Apply (or clear) the display filter over all frames.
    pub fn set_filter(&mut self, f: Option<Filter>) {
        match f {
            None => {
                self.filter = None;
                self.view = None;
            }
            Some(f) => {
                let n = self.packets.len();
                let view: Vec<u32> = {
                    use rayon::prelude::*;
                    (0..n as u32).into_par_iter().filter(|&i| self.matches(&f, i)).collect()
                };
                self.view = Some(view);
                self.filter = Some(f);
            }
        }
    }

    pub fn shown(&self) -> usize {
        self.view.as_ref().map(|v| v.len()).unwrap_or(self.packets.len())
    }

    pub fn shown_index(&self, pos: usize) -> Option<usize> {
        match &self.view {
            Some(v) => v.get(pos).map(|&i| i as usize),
            None => (pos < self.packets.len()).then_some(pos),
        }
    }

    /// Position of frame `index` in the current view (for "go to packet").
    pub fn position_of(&self, index: usize) -> Option<usize> {
        match &self.view {
            Some(v) => v.binary_search(&(index as u32)).ok(),
            None => (index < self.packets.len()).then_some(index),
        }
    }

    pub fn pending_apps(&self) -> Vec<(u32, bool, u16)> {
        self.streams
            .iter()
            .filter_map(|s| s.app_pending.map(|p| (s.id, s.tcp, p)))
            .collect()
    }

    pub fn resolve_app(&mut self, id: u32, app: Option<(String, u32)>, give_up: bool) {
        if let Some(s) = self.streams.get_mut(id as usize) {
            if app.is_some() {
                s.app = app;
                s.app_pending = None;
            } else if give_up {
                s.app_pending = None;
            }
        }
    }

    pub fn facts(&self, i: usize) -> Facts {
        self.dissect(i, Want::default()).facts
    }
}
