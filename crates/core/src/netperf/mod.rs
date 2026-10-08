// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netperf/mod.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Network performance testing between two SysPulse instances
//           (iperf-style). One machine runs the server, the other runs tests:
//             * TCP throughput: upload / download / bidirectional, 1-16 streams,
//               per-interval rates, retransmits and RTT from TCP_INFO
//             * UDP at a target bitrate: loss, out-of-order, RFC 3550 jitter
//             * Latency: UDP echo RTT distribution, jitter and loss
//           Wire protocol: a TCP control connection carrying one JSON message
//           per line, separate TCP data connections, and UDP packets with a
//           fixed 28-byte header. Throughput is always measured by the
//           receiving side, like iperf.
// =============================================================================

pub mod client;
pub mod server;

use std::net::TcpStream;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

pub const DEFAULT_PORT: u16 = 5201;
pub const PROTOCOL: &str = "syspulse-perf/1";
pub const TCP_BUF: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TestKind {
    Tcp,
    Udp,
    Latency,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Upload,
    Download,
    Bidir,
}

impl Direction {
    pub fn up(self) -> bool {
        matches!(self, Direction::Upload | Direction::Bidir)
    }
    pub fn down(self) -> bool {
        matches!(self, Direction::Download | Direction::Bidir)
    }
}

fn one() -> u8 {
    1
}
fn default_packet() -> u16 {
    1400
}
fn default_interval() -> u32 {
    1000
}
fn default_pps() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSpec {
    pub kind: TestKind,
    pub direction: Direction,
    #[serde(default = "one")]
    pub streams: u8,
    pub duration_secs: u32,
    /// UDP target rate.
    #[serde(default)]
    pub bitrate_mbps: f64,
    /// UDP datagram size in bytes (incl. our 28-byte header).
    #[serde(default = "default_packet")]
    pub packet_size: u16,
    #[serde(default = "default_interval")]
    pub interval_ms: u32,
    /// Latency probes per second.
    #[serde(default = "default_pps")]
    pub latency_pps: u32,
}

impl TestSpec {
    pub fn validate(&self) -> Result<()> {
        let bad = |m: &str| Err(CoreError::Refused(m.to_string()));
        if !(1..=16).contains(&self.streams) {
            return bad("streams must be 1-16");
        }
        if !(1..=120).contains(&self.duration_secs) {
            return bad("duration must be 1-120 s");
        }
        if !(250..=5000).contains(&self.interval_ms) {
            return bad("interval must be 250-5000 ms");
        }
        if self.kind == TestKind::Udp {
            if !(0.1..=10_000.0).contains(&self.bitrate_mbps) {
                return bad("UDP bitrate must be 0.1-10000 Mbit/s");
            }
            if !(64..=9000).contains(&self.packet_size) {
                return bad("UDP packet size must be 64-9000 bytes");
            }
        }
        if self.kind == TestKind::Latency && !(1..=1000).contains(&self.latency_pps) {
            return bad("latency rate must be 1-1000 probes/s");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Control,
    Data,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IntervalStat {
    pub index: u32,
    /// End of the interval, seconds since test start.
    pub t: f64,
    pub bytes: u64,
    pub mbps: f64,
    pub packets: Option<u64>,
    pub lost: Option<u64>,
    pub jitter_ms: Option<f64>,
    pub retransmits: Option<u64>,
    pub rtt_ms: Option<f64>,
}

/// Receiver-side statistics computed by the server for uploads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveStats {
    pub bytes: u64,
    pub seconds: f64,
    pub packets: Option<u64>,
    pub lost: Option<u64>,
    pub out_of_order: Option<u64>,
    pub jitter_ms: Option<f64>,
    pub intervals: Vec<IntervalStat>,
}

/// Sender-side statistics reported by the server for downloads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SendStats {
    pub bytes: u64,
    pub packets: Option<u64>,
    pub retransmits: Option<u64>,
    pub intervals: Vec<IntervalStat>,
}

/// Control-channel messages (one JSON object per line).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Msg {
    #[serde(rename_all = "camelCase")]
    Hello {
        protocol: String,
        role: Role,
        session: Option<u64>,
        stream: Option<u8>,
        dir: Option<Direction>,
        code: Option<String>,
        host: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Start { spec: TestSpec },
    #[serde(rename_all = "camelCase")]
    Ready {
        session: u64,
        udp_port: u16,
        host: String,
    },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
    #[serde(rename_all = "camelCase")]
    Interval { dir: Direction, stat: IntervalStat },
    #[serde(rename_all = "camelCase")]
    Done {
        sent_packets: Option<u64>,
        sent_bytes: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Result {
        upload: Option<ReceiveStats>,
        download_sent: Option<SendStats>,
    },
}

pub fn write_msg(w: &mut impl std::io::Write, m: &Msg) -> std::io::Result<()> {
    let mut s = serde_json::to_string(m).map_err(std::io::Error::other)?;
    s.push('\n');
    w.write_all(s.as_bytes())?;
    w.flush()
}

pub fn read_msg(r: &mut impl std::io::BufRead) -> std::io::Result<Msg> {
    let mut line = String::new();
    let n = r.read_line(&mut line)?;
    if n == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "connection closed",
        ));
    }
    if line.len() > 1 << 20 {
        return Err(std::io::Error::other("message too large"));
    }
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

// ------------------------------------------------------------ UDP wire ---

pub const UDP_HEADER: usize = 28;
pub const PKT_DATA: u8 = 1;
pub const PKT_ECHO_REQ: u8 = 2;
pub const PKT_ECHO_REPLY: u8 = 3;
pub const PKT_HELLO: u8 = 4;
pub const PKT_DISCOVER: u8 = 5;
pub const PKT_FIN: u8 = 6;
pub const PKT_DISCOVER_REPLY: u8 = 7;
pub const DISCOVER_MAGIC: &[u8] = b"SYSPULSE-DISCOVER";

pub fn encode_header(buf: &mut [u8], kind: u8, session: u64, seq: u64, ts_ns: u64) {
    buf[0] = kind;
    buf[1..4].fill(0);
    buf[4..12].copy_from_slice(&session.to_le_bytes());
    buf[12..20].copy_from_slice(&seq.to_le_bytes());
    buf[20..28].copy_from_slice(&ts_ns.to_le_bytes());
}

/// (kind, session, seq, ts_ns)
pub fn decode_header(buf: &[u8]) -> Option<(u8, u64, u64, u64)> {
    if buf.len() < UDP_HEADER {
        return None;
    }
    let u = |r: std::ops::Range<usize>| u64::from_le_bytes(buf[r].try_into().unwrap());
    Some((buf[0], u(4..12), u(12..20), u(20..28)))
}

/// UDP receive accounting: loss by sequence gaps, reordering and RFC 3550 jitter.
#[derive(Debug, Default)]
pub struct UdpRecv {
    pub packets: u64,
    pub bytes: u64,
    pub max_seq: Option<u64>,
    pub out_of_order: u64,
    jitter_ns: f64,
    last_transit: Option<i128>,
    // interval accounting
    iv_packets: u64,
    iv_bytes: u64,
    iv_seq_base: u64,
}

impl UdpRecv {
    pub fn on_packet(&mut self, seq: u64, ts_ns: u64, now_ns: u64, len: usize) {
        self.packets += 1;
        self.bytes += len as u64;
        self.iv_packets += 1;
        self.iv_bytes += len as u64;
        match self.max_seq {
            Some(m) if seq < m => self.out_of_order += 1,
            _ => self.max_seq = Some(seq),
        }
        let transit = now_ns as i128 - ts_ns as i128;
        if let Some(prev) = self.last_transit {
            let d = (transit - prev).abs() as f64;
            self.jitter_ns += (d - self.jitter_ns) / 16.0;
        }
        self.last_transit = Some(transit);
    }

    pub fn jitter_ms(&self) -> f64 {
        self.jitter_ns / 1e6
    }

    /// Packets expected so far (sequence numbers start at 0).
    pub fn expected(&self) -> u64 {
        self.max_seq.map(|m| m + 1).unwrap_or(0)
    }

    /// Close an interval: (bytes, packets, lost-in-interval).
    pub fn take_interval(&mut self) -> (u64, u64, u64) {
        let expected = self.expected();
        let exp_iv = expected.saturating_sub(self.iv_seq_base);
        let lost = exp_iv.saturating_sub(self.iv_packets);
        self.iv_seq_base = expected;
        let r = (self.iv_bytes, self.iv_packets, lost);
        self.iv_bytes = 0;
        self.iv_packets = 0;
        r
    }
}

// ---------------------------------------------------------------- misc ---

/// (total retransmits, smoothed RTT ms) from the kernel's TCP_INFO.
#[cfg(target_os = "linux")]
pub fn tcp_info(s: &TcpStream) -> Option<(u64, f64)> {
    use std::os::fd::AsRawFd;
    // SAFETY: tcp_info is plain old data; getsockopt writes at most `len` bytes.
    unsafe {
        let mut info: libc::tcp_info = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::tcp_info>() as libc::socklen_t;
        let rc = libc::getsockopt(
            s.as_raw_fd(),
            libc::IPPROTO_TCP,
            libc::TCP_INFO,
            &mut info as *mut _ as *mut libc::c_void,
            &mut len,
        );
        (rc == 0).then(|| {
            (
                info.tcpi_total_retrans as u64,
                info.tcpi_rtt as f64 / 1000.0,
            )
        })
    }
}

#[cfg(not(target_os = "linux"))]
pub fn tcp_info(_s: &TcpStream) -> Option<(u64, f64)> {
    None
}

pub fn mbps(bytes: u64, secs: f64) -> f64 {
    if secs <= 0.0 {
        0.0
    } else {
        bytes as f64 * 8.0 / secs / 1e6
    }
}

/// Session id that is hard to guess (time, pid and address entropy mixed).
pub fn new_session_id() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let stack = &t as *const u128 as u64;
    let mut x = (t as u64) ^ ((t >> 64) as u64) ^ (u64::from(std::process::id()) << 32) ^ stack;
    // splitmix64 finaliser
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let mut b = [0u8; 64];
        encode_header(&mut b, PKT_DATA, 42, 7, 123_456);
        assert_eq!(decode_header(&b), Some((PKT_DATA, 42, 7, 123_456)));
        assert_eq!(decode_header(&b[..10]), None);
    }

    #[test]
    fn udp_loss_and_reorder() {
        let mut r = UdpRecv::default();
        for (i, seq) in [0u64, 1, 2, 4, 3, 6].iter().enumerate() {
            r.on_packet(*seq, i as u64 * 1000, i as u64 * 1000 + 50, 100);
        }
        assert_eq!(r.packets, 6);
        assert_eq!(r.out_of_order, 1, "3 arrived after 4");
        assert_eq!(r.expected(), 7);
        let (_, p, lost) = r.take_interval();
        assert_eq!((p, lost), (6, 1), "seq 5 missing");
        assert!(r.jitter_ms() < 0.001, "constant transit time -> no jitter");
    }

    #[test]
    fn spec_validation() {
        let mut s = TestSpec {
            kind: TestKind::Udp,
            direction: Direction::Upload,
            streams: 1,
            duration_secs: 10,
            bitrate_mbps: 100.0,
            packet_size: 1400,
            interval_ms: 1000,
            latency_pps: 20,
        };
        assert!(s.validate().is_ok());
        s.streams = 64;
        assert!(s.validate().is_err());
        s.streams = 1;
        s.bitrate_mbps = 0.0;
        assert!(s.validate().is_err());
    }
}
