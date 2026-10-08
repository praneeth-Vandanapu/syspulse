// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/dissect/mod.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: NetLens protocol dissection framework. One pass over a frame
//           produces up to three things, each only when asked for:
//             * `fields`  - flat (name, value, byte range) list used by the
//                           display-filter engine;
//             * `tree`    - the labelled detail tree shown in the UI, with the
//                           byte range of every node (links tree <-> hex view);
//             * `facts`   - typed values the capture analysis needs (ports,
//                           TCP sequence numbers, DNS answers, RTSP transport).
//           Dissectors are plain functions over a byte slice, so they are easy
//           to test and safe on truncated or hostile input (every read is
//           bounds-checked through `Rd`).
// =============================================================================

pub mod dns;
pub mod media;
pub mod net;
pub mod web;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::net::IpAddr;

use serde::Serialize;

// ------------------------------------------------------------ values ---

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    /// Presence only (protocol names, flags without value).
    None,
    U(u64),
    F(f64),
    B(bool),
    S(String),
    Ip(IpAddr),
    Mac([u8; 6]),
    /// Raw bytes of the node's own range (payloads); read from the frame.
    Raw,
}

impl Val {
    pub fn display(&self) -> String {
        match self {
            Val::None | Val::Raw => String::new(),
            Val::U(v) => v.to_string(),
            Val::F(v) => format!("{v:.6}"),
            Val::B(b) => if *b { "True" } else { "False" }.into(),
            Val::S(s) => s.clone(),
            Val::Ip(ip) => ip.to_string(),
            Val::Mac(m) => mac_str(m),
        }
    }
}

pub fn mac_str(m: &[u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        m[0], m[1], m[2], m[3], m[4], m[5]
    )
}

/// Byte-safe printable rendering used in labels (C escapes for control bytes).
pub fn printable(b: &[u8], max: usize) -> String {
    let mut s = String::new();
    for &c in b.iter().take(max) {
        match c {
            b'\r' => s.push_str("\\r"),
            b'\n' => s.push_str("\\n"),
            b'\t' => s.push_str("\\t"),
            0x20..=0x7e => s.push(c as char),
            _ => {
                let _ = write!(s, "\\x{c:02X}");
            }
        }
    }
    if b.len() > max {
        s.push('…');
    }
    s
}

// --------------------------------------------------------- protocols ---

/// Top-most protocol of a frame (Protocol column, colouring, statistics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[repr(u8)]
pub enum Proto {
    #[default]
    Unknown = 0,
    Eth,
    Arp,
    Ipv4,
    Ipv6,
    Icmp,
    Icmpv6,
    Igmp,
    Tcp,
    Udp,
    Http,
    Tls,
    Quic,
    Rtsp,
    Rtp,
    Rtcp,
    Dns,
    Mdns,
    Dhcp,
    Ntp,
    Ssdp,
    Llc,
}

impl Proto {
    pub const ALL: [Proto; 22] = [
        Proto::Unknown,
        Proto::Eth,
        Proto::Arp,
        Proto::Ipv4,
        Proto::Ipv6,
        Proto::Icmp,
        Proto::Icmpv6,
        Proto::Igmp,
        Proto::Tcp,
        Proto::Udp,
        Proto::Http,
        Proto::Tls,
        Proto::Quic,
        Proto::Rtsp,
        Proto::Rtp,
        Proto::Rtcp,
        Proto::Dns,
        Proto::Mdns,
        Proto::Dhcp,
        Proto::Ntp,
        Proto::Ssdp,
        Proto::Llc,
    ];

    pub fn from_u8(v: u8) -> Proto {
        Proto::ALL.get(v as usize).copied().unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            Proto::Unknown => "Data",
            Proto::Eth => "Ethernet",
            Proto::Arp => "ARP",
            Proto::Ipv4 => "IPv4",
            Proto::Ipv6 => "IPv6",
            Proto::Icmp => "ICMP",
            Proto::Icmpv6 => "ICMPv6",
            Proto::Igmp => "IGMP",
            Proto::Tcp => "TCP",
            Proto::Udp => "UDP",
            Proto::Http => "HTTP",
            Proto::Tls => "TLS",
            Proto::Quic => "QUIC",
            Proto::Rtsp => "RTSP",
            Proto::Rtp => "RTP",
            Proto::Rtcp => "RTCP",
            Proto::Dns => "DNS",
            Proto::Mdns => "MDNS",
            Proto::Dhcp => "DHCP",
            Proto::Ntp => "NTP",
            Proto::Ssdp => "SSDP",
            Proto::Llc => "LLC",
        }
    }

    /// Colour class for the packet list (see NetLens.svelte).
    pub fn color(self) -> &'static str {
        match self {
            Proto::Http | Proto::Ssdp => "http",
            Proto::Tls | Proto::Quic => "tls",
            Proto::Dns | Proto::Mdns | Proto::Dhcp | Proto::Ntp => "dns",
            Proto::Rtsp | Proto::Rtp | Proto::Rtcp => "media",
            Proto::Arp | Proto::Llc | Proto::Eth => "arp",
            Proto::Icmp | Proto::Icmpv6 | Proto::Igmp => "icmp",
            Proto::Udp => "udp",
            _ => "tcp",
        }
    }
}

// ------------------------------------------------------------- input ---

/// Link-layer header types (pcap LINKTYPE_* values).
pub mod linktype {
    pub const NULL: u16 = 0;
    pub const ETHERNET: u16 = 1;
    pub const RAW: u16 = 101;
    pub const LINUX_SLL: u16 = 113;
    pub const LINUX_SLL2: u16 = 276;
    pub const IPV4: u16 = 228;
    pub const IPV6: u16 = 229;
}

/// What dissection should produce.
#[derive(Debug, Clone, Copy, Default)]
pub struct Want {
    pub tree: bool,
    pub fields: bool,
    pub info: bool,
}

/// Per-frame metadata known to the store (from capture analysis).
#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub number: u64,
    pub orig_len: u32,
    pub ts_rel: f64,
    pub ts_delta: f64,
    pub ts_abs: String,
    pub interface: String,
    /// Outgoing from this PC.
    pub outgoing: Option<bool>,
    pub app: Option<(String, u32)>,
    pub stream: Option<u32>,
    pub analysis: u16,
    /// Request/response time for DNS, HTTP and RTSP responses (seconds).
    pub response_time: Option<f64>,
    /// TCP initial sequence numbers (this direction, reverse) for relative seq.
    pub isn: Option<(u32, u32)>,
    /// Negotiated TLS version of the stream (0x0304 for TLS 1.3).
    pub tls_version: u16,
}

/// Capture-wide state learned from earlier packets (RTSP SETUP, SDP).
#[derive(Debug, Clone, Default)]
pub struct Shared {
    /// UDP endpoint -> RTP (false) or RTCP (true), from RTSP Transport headers.
    pub rtp_ports: HashMap<(IpAddr, u16), bool>,
    /// RTP payload type -> (encoding name, clock rate), from SDP a=rtpmap.
    pub rtpmap: HashMap<u8, (String, u32)>,
    /// TCP stream endpoints that speak RTSP on a non-standard port.
    pub rtsp_ports: Vec<(IpAddr, u16)>,
}

// ------------------------------------------------------------ output ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub label: String,
    /// Filter field name, empty for text-only nodes.
    pub field: &'static str,
    /// Ready-made display filter selecting this value (UI "Apply as filter").
    #[serde(skip_serializing_if = "String::is_empty")]
    pub filter: String,
    pub start: u32,
    pub len: u32,
    /// 0 normal, 1 note, 2 warning, 3 error.
    pub level: u8,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: &'static str,
    pub val: Val,
    pub start: u32,
    pub len: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpFacts {
    pub sport: u16,
    pub dport: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u16,
    pub window: u16,
    pub payload_off: usize,
    pub payload_len: usize,
}

pub mod tcpflag {
    pub const FIN: u16 = 0x01;
    pub const SYN: u16 = 0x02;
    pub const RST: u16 = 0x04;
    pub const PSH: u16 = 0x08;
    pub const ACK: u16 = 0x10;
    pub const URG: u16 = 0x20;
}

#[derive(Debug, Clone, Default)]
pub struct DnsFacts {
    pub id: u16,
    pub response: bool,
    pub rcode: u8,
    pub qname: String,
    pub answers: Vec<(String, IpAddr)>,
}

#[derive(Debug, Clone, Default)]
pub struct MsgFacts {
    /// HTTP / RTSP: request (true) or response (false).
    pub request: bool,
    pub method: String,
    pub code: u16,
    pub host: String,
    pub cseq: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct RtspTransport {
    pub client_ports: Option<(u16, u16)>,
    pub server_ports: Option<(u16, u16)>,
    pub interleaved: bool,
    /// Multicast destination group.
    pub destination: Option<IpAddr>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RtpFacts {
    pub ssrc: u32,
    pub seq: u16,
    pub timestamp: u32,
    pub pt: u8,
    pub marker: bool,
    pub payload_len: usize,
    pub keyframe: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub src: Option<IpAddr>,
    pub dst: Option<IpAddr>,
    pub tcp: Option<TcpFacts>,
    /// (source port, destination port, payload length, payload offset)
    pub udp: Option<(u16, u16, usize, usize)>,
    pub dns: Option<DnsFacts>,
    pub http: Option<MsgFacts>,
    pub rtsp: Option<MsgFacts>,
    pub rtsp_transport: Option<RtspTransport>,
    /// SDP a=rtpmap entries found in this frame.
    pub rtpmap: Vec<(u8, String, u32)>,
    pub rtp: Option<RtpFacts>,
    pub tls_sni: Option<String>,
    /// TLS version chosen by a ServerHello (supported_versions aware).
    pub tls_server_version: Option<u16>,
    pub tls_alert: Option<(u8, u8)>,
    pub icmp_error: bool,
    /// ICMP/ICMPv6 echo: (is_request, identifier, sequence).
    pub echo: Option<(bool, u16, u16)>,
    /// RTCP PLI/FIR: a receiver asked for a new key frame.
    pub keyframe_request: bool,
}

/// Result of dissecting one frame.
#[derive(Debug, Default)]
pub struct Dissection {
    pub proto: Proto,
    /// Protocol column override ("TLSv1.3", "HTTP/JSON").
    pub label: Option<&'static str>,
    pub src: String,
    pub dst: String,
    pub info: String,
    pub tree: Vec<Node>,
    pub fields: Vec<Field>,
    pub facts: Facts,
    /// Layer path ("eth/ipv4/tcp/tls") for the protocol hierarchy.
    pub path: Vec<&'static str>,
}

/// Dissection context: input bytes, options and the output under construction.
pub struct Cx<'a> {
    pub data: &'a [u8],
    pub want: Want,
    pub meta: &'a Meta,
    pub shared: &'a Shared,
    pub out: Dissection,
    stack: Vec<Node>,
}

impl<'a> Cx<'a> {
    pub fn new(data: &'a [u8], want: Want, meta: &'a Meta, shared: &'a Shared) -> Self {
        Cx {
            data,
            want,
            meta,
            shared,
            out: Dissection::default(),
            stack: Vec::new(),
        }
    }

    /// The frame bytes, with the input lifetime (not tied to the `Cx` borrow).
    pub fn d(&self) -> &'a [u8] {
        self.data
    }

    pub fn finish(mut self) -> Dissection {
        while !self.stack.is_empty() {
            self.close();
        }
        self.out
    }

    fn push_node(&mut self, node: Node) {
        match self.stack.last_mut() {
            Some(parent) => parent.children.push(node),
            None => self.out.tree.push(node),
        }
    }

    /// Open a protocol layer: records the presence field, the hierarchy path
    /// and the top protocol, and starts a subtree.
    pub fn layer(
        &mut self,
        field: &'static str,
        proto: Option<Proto>,
        start: usize,
        len: usize,
        label: impl FnOnce() -> String,
    ) {
        if let Some(p) = proto {
            self.out.proto = p;
        }
        self.out.path.push(field);
        if self.want.fields {
            self.out.fields.push(Field {
                name: field,
                val: Val::None,
                start: start as u32,
                len: len as u32,
            });
        }
        if self.want.tree {
            self.stack.push(Node {
                label: label(),
                field,
                filter: field.to_string(),
                start: start as u32,
                len: len as u32,
                level: 0,
                children: Vec::new(),
            });
        }
    }

    /// Open a plain subtree (e.g. "Flags") without a field.
    pub fn open(&mut self, start: usize, len: usize, label: impl FnOnce() -> String) {
        if self.want.tree {
            self.stack.push(Node {
                label: label(),
                field: "",
                filter: String::new(),
                start: start as u32,
                len: len as u32,
                level: 0,
                children: Vec::new(),
            });
        }
    }

    /// Close the innermost open layer/subtree.
    pub fn close(&mut self) {
        if !self.want.tree {
            return;
        }
        if let Some(n) = self.stack.pop() {
            self.push_node(n);
        }
    }

    /// Add a field. The label closure only runs when a tree is wanted.
    pub fn add(
        &mut self,
        field: &'static str,
        val: Val,
        start: usize,
        len: usize,
        label: impl FnOnce(&Val) -> String,
    ) {
        if self.want.tree {
            let filter = filter_for(field, &val, &self.data[clamp(start, len, self.data.len())]);
            let node = Node {
                label: label(&val),
                field,
                filter,
                start: start as u32,
                len: len as u32,
                level: 0,
                children: Vec::new(),
            };
            self.push_node(node);
        }
        if self.want.fields {
            self.out.fields.push(Field {
                name: field,
                val,
                start: start as u32,
                len: len as u32,
            });
        }
    }

    /// "Name: value" field.
    pub fn kv(&mut self, field: &'static str, name: &str, val: Val, start: usize, len: usize) {
        self.add(field, val, start, len, |v| format!("{name}: {}", v.display()));
    }

    /// Field-only value (filterable, not shown in the tree).
    pub fn hidden(&mut self, field: &'static str, val: Val, start: usize, len: usize) {
        if self.want.fields {
            self.out.fields.push(Field {
                name: field,
                val,
                start: start as u32,
                len: len as u32,
            });
        }
    }

    /// Text-only tree line.
    pub fn text(&mut self, start: usize, len: usize, label: impl FnOnce() -> String) {
        if self.want.tree {
            let node = Node {
                label: label(),
                field: "",
                filter: String::new(),
                start: start as u32,
                len: len as u32,
                level: 0,
                children: Vec::new(),
            };
            self.push_node(node);
        }
    }

    /// Expert note / warning / error line (also bumps the parent's level).
    pub fn expert(&mut self, level: u8, field: &'static str, start: usize, len: usize, msg: impl FnOnce() -> String) {
        if self.want.fields && !field.is_empty() {
            self.out.fields.push(Field {
                name: field,
                val: Val::None,
                start: start as u32,
                len: len as u32,
            });
        }
        if self.want.tree {
            for n in self.stack.iter_mut() {
                n.level = n.level.max(level);
            }
            let node = Node {
                label: msg(),
                field,
                filter: field.to_string(),
                start: start as u32,
                len: len as u32,
                level,
                children: Vec::new(),
            };
            self.push_node(node);
        }
    }

    pub fn info(&mut self, f: impl FnOnce() -> String) {
        if self.want.info {
            self.out.info = f();
        }
    }

    pub fn info_append(&mut self, f: impl FnOnce() -> String) {
        if self.want.info {
            let s = f();
            if !self.out.info.is_empty() {
                self.out.info.push_str(", ");
            }
            self.out.info.push_str(&s);
        }
    }
}

fn clamp(start: usize, len: usize, total: usize) -> std::ops::Range<usize> {
    let s = start.min(total);
    s..(s + len).min(total)
}

/// Display filter that selects `val` for `field`.
fn filter_for(field: &str, val: &Val, raw: &[u8]) -> String {
    match val {
        Val::None => field.to_string(),
        Val::S(s) => format!("{field} == \"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Val::B(b) => format!("{field} == {}", if *b { 1 } else { 0 }),
        Val::F(f) => format!("{field} >= {f:.6}"),
        Val::Raw => {
            if raw.is_empty() || raw.len() > 64 {
                field.to_string()
            } else {
                let hex: Vec<String> = raw.iter().map(|b| format!("{b:02x}")).collect();
                format!("{field} contains {}", hex.join(":"))
            }
        }
        v => format!("{field} == {}", v.display()),
    }
}

// ------------------------------------------------------------ reader ---

/// Bounds-checked big-endian reader over a frame.
#[derive(Clone, Copy)]
pub struct Rd<'a> {
    pub d: &'a [u8],
}

impl<'a> Rd<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Rd { d }
    }
    pub fn u8(&self, o: usize) -> Option<u8> {
        self.d.get(o).copied()
    }
    pub fn u16(&self, o: usize) -> Option<u16> {
        Some(u16::from_be_bytes(self.d.get(o..o + 2)?.try_into().ok()?))
    }
    pub fn u24(&self, o: usize) -> Option<u32> {
        let b = self.d.get(o..o + 3)?;
        Some(((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32)
    }
    pub fn u32(&self, o: usize) -> Option<u32> {
        Some(u32::from_be_bytes(self.d.get(o..o + 4)?.try_into().ok()?))
    }
    pub fn u64(&self, o: usize) -> Option<u64> {
        Some(u64::from_be_bytes(self.d.get(o..o + 8)?.try_into().ok()?))
    }
    pub fn bytes(&self, o: usize, n: usize) -> Option<&'a [u8]> {
        self.d.get(o..o.checked_add(n)?)
    }
    pub fn mac(&self, o: usize) -> Option<[u8; 6]> {
        self.d.get(o..o + 6)?.try_into().ok()
    }
    pub fn ip4(&self, o: usize) -> Option<IpAddr> {
        let b: [u8; 4] = self.d.get(o..o + 4)?.try_into().ok()?;
        Some(IpAddr::from(b))
    }
    pub fn ip6(&self, o: usize) -> Option<IpAddr> {
        let b: [u8; 16] = self.d.get(o..o + 16)?.try_into().ok()?;
        let a = std::net::Ipv6Addr::from(b);
        Some(IpAddr::V6(a))
    }
}

// ------------------------------------------------------------- entry ---

/// Dissect one frame.
pub fn dissect(data: &[u8], linktype: u16, want: Want, meta: &Meta, shared: &Shared) -> Dissection {
    let mut cx = Cx::new(data, want, meta, shared);
    net::frame(&mut cx, linktype);
    let mut out = cx.finish();
    if out.info.is_empty() && want.info {
        out.info = format!("{} bytes", data.len());
    }
    out
}

/// Text-based protocols: split a header block into lines.
pub(crate) fn split_lines(b: &[u8]) -> Vec<(usize, &[u8])> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\n' {
            let mut end = i;
            if end > start && b[end - 1] == b'\r' {
                end -= 1;
            }
            out.push((start, &b[start..end]));
            start = i + 1;
        }
        i += 1;
    }
    if start < b.len() {
        out.push((start, &b[start..]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_escapes() {
        assert_eq!(printable(b"AT\r\n\x01", 10), "AT\\r\\n\\x01");
        assert_eq!(printable(b"abcdef", 3), "abc…");
    }

    #[test]
    fn filters_for_values() {
        assert_eq!(filter_for("tcp.port", &Val::U(80), &[]), "tcp.port == 80");
        assert_eq!(
            filter_for("http.host", &Val::S("a\"b".into()), &[]),
            "http.host == \"a\\\"b\""
        );
        assert_eq!(filter_for("tcp.payload", &Val::Raw, &[1, 0xab]), "tcp.payload contains 01:ab");
    }
}
