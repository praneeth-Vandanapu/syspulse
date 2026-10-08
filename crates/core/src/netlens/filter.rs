// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/filter.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: NetLens display filters - a Wireshark-compatible subset:
//
//             tcp                         protocol present
//             ip.addr == 10.0.0.0/8       address / subnet (either side)
//             tcp.port in {80 443 8080}   set membership
//             http.host contains "api"    substring (case-sensitive)
//             dns.qry.name matches "goo"  regular expression (case-insensitive)
//             frame.len > 1000 && !arp    comparisons, && || ! and/or/not, ()
//             app.name == "firefox"       NetLens extra: owning application
//
//           Fields that occur several times (ip.addr, tcp.port) match when
//           ANY occurrence matches; `!=` means "no occurrence equals".
//           Every field is listed in FIELDS (type + description), which also
//           drives auto-completion in the UI.
// =============================================================================

use std::net::IpAddr;

use regex::Regex;
use serde::Serialize;

use super::dissect::{Field, Val};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Proto,
    Uint,
    Float,
    Bool,
    Str,
    Ip,
    Mac,
    Bytes,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    pub name: &'static str,
    pub kind: Kind,
    pub description: &'static str,
}

macro_rules! fields {
    ($( $name:literal $kind:ident $desc:literal ),* $(,)?) => {
        pub const FIELDS: &[(&str, Kind, &str)] = &[ $( ($name, Kind::$kind, $desc) ),* ];
    };
}

fields! {
    "frame" Proto "Any frame",
    "frame.number" Uint "Frame number",
    "frame.len" Uint "Bytes on the wire",
    "frame.cap_len" Uint "Bytes captured",
    "frame.time_relative" Float "Seconds since the first frame",
    "frame.time_delta" Float "Seconds since the previous frame",
    "frame.interface_name" Str "Capture interface",
    "frame.direction" Str "\"in\" (received) or \"out\" (sent by this PC)",
    "app.name" Str "Application that owns the connection (NetLens)",
    "app.pid" Uint "Process ID of the owning application (NetLens)",
    "eth" Proto "Ethernet",
    "eth.addr" Mac "Source or destination MAC address",
    "eth.src" Mac "Source MAC address",
    "eth.dst" Mac "Destination MAC address",
    "eth.type" Uint "EtherType",
    "eth.len" Uint "802.3 length",
    "vlan" Proto "802.1Q VLAN tag",
    "vlan.id" Uint "VLAN ID",
    "vlan.priority" Uint "VLAN priority",
    "vlan.etype" Uint "Inner EtherType",
    "sll" Proto "Linux cooked capture",
    "sll.etype" Uint "Protocol",
    "sll.src.eth" Mac "Source MAC",
    "llc" Proto "802.2 Logical-Link Control",
    "null" Proto "BSD loopback",
    "arp" Proto "Address Resolution Protocol",
    "arp.opcode" Uint "1 request, 2 reply",
    "arp.hw.type" Uint "Hardware type",
    "arp.proto.type" Uint "Protocol type",
    "arp.isgratuitous" Proto "Gratuitous ARP",
    "arp.src.hw_mac" Mac "Sender MAC address",
    "arp.src.proto_ipv4" Ip "Sender IP address",
    "arp.dst.hw_mac" Mac "Target MAC address",
    "arp.dst.proto_ipv4" Ip "Target IP address",
    "ip" Proto "IPv4",
    "ip.addr" Ip "Source or destination IPv4 address",
    "ip.src" Ip "Source IPv4 address",
    "ip.dst" Ip "Destination IPv4 address",
    "ip.version" Uint "IP version",
    "ip.hdr_len" Uint "Header length",
    "ip.dsfield.dscp" Uint "DSCP value",
    "ip.len" Uint "Total length",
    "ip.id" Uint "Identification",
    "ip.flags.df" Bool "Don't fragment",
    "ip.flags.mf" Bool "More fragments",
    "ip.fragment" Proto "Fragmented datagram",
    "ip.frag_offset" Uint "Fragment offset",
    "ip.ttl" Uint "Time to live",
    "ip.proto" Uint "Protocol number (6 TCP, 17 UDP, 1 ICMP)",
    "ip.checksum" Uint "Header checksum",
    "ipv6" Proto "IPv6",
    "ipv6.addr" Ip "Source or destination IPv6 address",
    "ipv6.src" Ip "Source IPv6 address",
    "ipv6.dst" Ip "Destination IPv6 address",
    "ipv6.version" Uint "IP version",
    "ipv6.tclass" Uint "Traffic class",
    "ipv6.flow" Uint "Flow label",
    "ipv6.plen" Uint "Payload length",
    "ipv6.nxt" Uint "Next header",
    "ipv6.hlim" Uint "Hop limit",
    "icmp" Proto "ICMP",
    "icmp.type" Uint "ICMP type (8 echo request, 0 reply, 3 unreachable)",
    "icmp.code" Uint "ICMP code",
    "icmp.checksum" Uint "Checksum",
    "icmp.ident" Uint "Echo identifier",
    "icmp.seq" Uint "Echo sequence number",
    "icmp.resptime" Float "Ping response time (s)",
    "icmpv6" Proto "ICMPv6",
    "icmpv6.type" Uint "ICMPv6 type",
    "icmpv6.code" Uint "ICMPv6 code",
    "icmpv6.echo.identifier" Uint "Echo identifier",
    "icmpv6.echo.sequence_number" Uint "Echo sequence number",
    "icmpv6.nd.ns.target_address" Ip "Neighbor discovery target",
    "igmp" Proto "IGMP",
    "igmp.type" Uint "IGMP type",
    "tcp" Proto "TCP",
    "tcp.port" Uint "Source or destination TCP port",
    "tcp.srcport" Uint "Source port",
    "tcp.dstport" Uint "Destination port",
    "tcp.stream" Uint "Connection index",
    "tcp.len" Uint "Payload length",
    "tcp.seq" Uint "Relative sequence number",
    "tcp.seq_raw" Uint "Raw sequence number",
    "tcp.nxtseq" Uint "Next sequence number",
    "tcp.ack" Uint "Relative acknowledgment number",
    "tcp.ack_raw" Uint "Raw acknowledgment number",
    "tcp.hdr_len" Uint "Header length",
    "tcp.flags" Uint "Flags (0x02 SYN, 0x10 ACK, 0x04 RST, 0x01 FIN)",
    "tcp.flags.syn" Bool "SYN",
    "tcp.flags.ack" Bool "ACK",
    "tcp.flags.fin" Bool "FIN",
    "tcp.flags.reset" Bool "RST",
    "tcp.flags.push" Bool "PSH",
    "tcp.flags.urg" Bool "URG",
    "tcp.window_size_value" Uint "Advertised window",
    "tcp.checksum" Uint "Checksum",
    "tcp.urgent_pointer" Uint "Urgent pointer",
    "tcp.options.mss_val" Uint "Maximum segment size",
    "tcp.options.wscale.shift" Uint "Window scale",
    "tcp.options.sack_perm" Proto "SACK permitted",
    "tcp.options.sack" Proto "SACK blocks",
    "tcp.options.timestamp.tsval" Uint "Timestamp value",
    "tcp.payload" Bytes "TCP payload",
    "tcp.analysis.flags" Proto "Any TCP problem (retransmission, lost segment, dup ACK, zero window)",
    "tcp.analysis.retransmission" Proto "Retransmission",
    "tcp.analysis.out_of_order" Proto "Out-of-order segment",
    "tcp.analysis.lost_segment" Proto "Previous segment not captured",
    "tcp.analysis.duplicate_ack" Proto "Duplicate ACK",
    "tcp.analysis.zero_window" Proto "Zero window",
    "tcp.analysis.keep_alive" Proto "Keep-alive",
    "tcp.analysis.reused_ports" Proto "Port reuse",
    "udp" Proto "UDP",
    "udp.port" Uint "Source or destination UDP port",
    "udp.srcport" Uint "Source port",
    "udp.dstport" Uint "Destination port",
    "udp.length" Uint "Length",
    "udp.checksum" Uint "Checksum",
    "udp.stream" Uint "Conversation index",
    "udp.payload" Bytes "UDP payload",
    "http" Proto "HTTP",
    "http.request" Proto "HTTP request",
    "http.response" Proto "HTTP response",
    "http.request.method" Str "Request method (GET, POST...)",
    "http.request.uri" Str "Request URI",
    "http.request.version" Str "Request version",
    "http.response.code" Uint "Status code",
    "http.response.phrase" Str "Status phrase",
    "http.response.version" Str "Response version",
    "http.host" Str "Host header",
    "http.user_agent" Str "User-Agent header",
    "http.content_type" Str "Content-Type header",
    "http.content_length" Uint "Content-Length header",
    "http.server" Str "Server header",
    "http.location" Str "Location header",
    "http.referer" Str "Referer header",
    "http.cookie" Str "Cookie header",
    "http.set_cookie" Str "Set-Cookie header",
    "http.authorization" Str "Authorization header",
    "http.www_authenticate" Str "WWW-Authenticate header",
    "http.connection" Str "Connection header",
    "http.transfer_encoding" Str "Transfer-Encoding header",
    "http.content_encoding" Str "Content-Encoding header",
    "http.accept" Str "Accept header",
    "http.accept_encoding" Str "Accept-Encoding header",
    "http.cache_control" Str "Cache-Control header",
    "http.upgrade" Str "Upgrade header",
    "http.date" Str "Date header",
    "http.last_modified" Str "Last-Modified header",
    "http.x_forwarded_for" Str "X-Forwarded-For header",
    "http.file_data" Bytes "Body",
    "http.time" Float "Seconds from request to response",
    "ssdp" Proto "SSDP (UPnP discovery)",
    "tls" Proto "TLS (HTTPS and other encrypted TCP)",
    "tls.record.content_type" Uint "22 handshake, 23 application data, 21 alert",
    "tls.record.version" Uint "Record version",
    "tls.record.length" Uint "Record length",
    "tls.handshake.type" Uint "1 Client Hello, 2 Server Hello, 11 Certificate",
    "tls.handshake.length" Uint "Handshake length",
    "tls.handshake.version" Uint "Hello version",
    "tls.handshake.random" Bytes "Hello random",
    "tls.handshake.session_id_length" Uint "Session ID length",
    "tls.handshake.ciphersuite" Uint "Cipher suite",
    "tls.handshake.extension.type" Uint "Extension type",
    "tls.handshake.extensions_server_name" Str "Server name (SNI) - the website",
    "tls.handshake.extensions_alpn_str" Str "ALPN protocol (h2, http/1.1)",
    "tls.handshake.extensions.supported_version" Uint "Negotiated version (0x0304 = TLS 1.3)",
    "tls.handshake.ech" Proto "Encrypted Client Hello",
    "tls.handshake.encrypted" Proto "Encrypted handshake message",
    "tls.cert.subject_cn" Str "Certificate subject common name",
    "tls.cert.issuer_cn" Str "Certificate issuer common name",
    "tls.alert_message" Proto "Alert",
    "tls.alert_message.level" Uint "1 warning, 2 fatal",
    "tls.alert_message.desc" Uint "Alert description",
    "tls.alert_message.encrypted" Proto "Encrypted alert",
    "tls.app_data" Bytes "Encrypted application data",
    "tls.continuation" Bytes "Continuation data",
    "quic" Proto "QUIC (HTTP/3)",
    "quic.header_form" Uint "1 long header, 0 short header",
    "quic.long.packet_type" Str "Initial, Handshake, 0-RTT, Retry",
    "quic.version" Uint "QUIC version",
    "quic.dcid" Str "Destination connection ID (hex)",
    "quic.scid" Str "Source connection ID (hex)",
    "quic.remaining_payload" Bytes "Protected payload",
    "quic.protected_payload" Bytes "Protected payload",
    "rtsp" Proto "RTSP (camera / media streaming control)",
    "rtsp.request" Proto "RTSP request",
    "rtsp.response" Proto "RTSP response",
    "rtsp.method" Str "Method (DESCRIBE, SETUP, PLAY...)",
    "rtsp.url" Str "Request URL",
    "rtsp.status" Uint "Status code",
    "rtsp.cseq" Uint "CSeq",
    "rtsp.session" Str "Session ID",
    "rtsp.transport" Str "Transport header",
    "rtsp.content_type" Str "Content-Type",
    "rtsp.content_length" Uint "Content-Length",
    "rtsp.content_base" Str "Content-Base",
    "rtsp.public" Str "Supported methods",
    "rtsp.range" Str "Range",
    "rtsp.rtp_info" Str "RTP-Info",
    "rtsp.www_authenticate" Str "Authentication challenge",
    "rtsp.authorization" Str "Credentials",
    "rtsp.user_agent" Str "User-Agent",
    "rtsp.server" Str "Server",
    "rtsp.channel" Uint "Interleaved channel",
    "rtsp.length" Uint "Interleaved frame length",
    "rtsp.data" Bytes "Body / continuation",
    "rtsp.time" Float "Seconds from request to response",
    "sdp" Proto "Session Description Protocol",
    "sdp.version" Str "v=",
    "sdp.owner" Str "o=",
    "sdp.session_name" Str "s=",
    "sdp.session_info" Str "i=",
    "sdp.uri" Str "u=",
    "sdp.connection_info" Str "c=",
    "sdp.bandwidth" Str "b=",
    "sdp.time" Str "t=",
    "sdp.media" Str "m= media line",
    "sdp.media_attr" Str "a= attribute",
    "sdp.rtpmap" Str "a=rtpmap",
    "sdp.control" Str "a=control",
    "sdp.fmtp" Str "a=fmtp",
    "rtp" Proto "RTP (audio/video packets)",
    "rtp.version" Uint "Version",
    "rtp.padding" Bool "Padding",
    "rtp.ext" Bool "Header extension",
    "rtp.cc" Uint "CSRC count",
    "rtp.marker" Bool "Marker (end of a video frame)",
    "rtp.p_type" Uint "Payload type",
    "rtp.codec" Str "Codec (H264, PCMA...)",
    "rtp.seq" Uint "Sequence number",
    "rtp.timestamp" Uint "Timestamp",
    "rtp.ssrc" Uint "Stream identifier (SSRC)",
    "rtp.nal_type" Uint "H.264/H.265 NAL unit type",
    "rtp.keyframe" Proto "Key frame (IDR) data",
    "rtp.payload" Bytes "Payload",
    "rtcp" Proto "RTCP (stream statistics)",
    "rtcp.pt" Uint "200 SR, 201 RR, 202 SDES, 203 BYE, 206 feedback",
    "rtcp.senderssrc" Uint "Sender SSRC",
    "rtcp.timestamp.rtp" Uint "RTP timestamp",
    "rtcp.sender.packetcount" Uint "Packets sent",
    "rtcp.sender.octetcount" Uint "Bytes sent",
    "rtcp.ssrc.fraction" Uint "Fraction lost (/256)",
    "rtcp.ssrc.cum_nr" Uint "Cumulative packets lost",
    "rtcp.ssrc.ext_high_seq" Uint "Highest sequence number",
    "rtcp.ssrc.jitter" Uint "Interarrival jitter",
    "rtcp.sdes.text" Str "SDES item (CNAME)",
    "rtcp.psfb.fmt" Uint "Feedback type (1 PLI, 4 FIR)",
    "dns" Proto "DNS (includes mDNS)",
    "mdns" Proto "Multicast DNS",
    "dns.id" Uint "Transaction ID",
    "dns.flags.response" Bool "Is a response",
    "dns.flags.opcode" Uint "Opcode",
    "dns.flags.recdesired" Bool "Recursion desired",
    "dns.flags.rcode" Uint "Reply code (0 ok, 3 no such name)",
    "dns.count.queries" Uint "Questions",
    "dns.count.answers" Uint "Answers",
    "dns.count.auth_rr" Uint "Authority records",
    "dns.count.add_rr" Uint "Additional records",
    "dns.qry.name" Str "Queried name",
    "dns.qry.type" Uint "Query type (1 A, 28 AAAA, 5 CNAME)",
    "dns.resp.name" Str "Answer name",
    "dns.resp.type" Uint "Answer type",
    "dns.resp.ttl" Uint "Answer TTL",
    "dns.a" Ip "IPv4 address answer",
    "dns.aaaa" Ip "IPv6 address answer",
    "dns.cname" Str "Canonical name answer",
    "dns.time" Float "Seconds from query to response",
    "dhcp" Proto "DHCP",
    "dhcp.type" Uint "1 request, 2 reply",
    "dhcp.id" Uint "Transaction ID",
    "dhcp.ip.your" Ip "Offered address",
    "dhcp.hw.mac_addr" Mac "Client MAC",
    "dhcp.option.dhcp" Uint "Message type (1 Discover, 2 Offer, 3 Request, 5 ACK)",
    "dhcp.option.hostname" Str "Client host name",
    "dhcp.option.requested_ip_address" Ip "Requested address",
    "dhcp.option.dhcp_server_id" Ip "DHCP server",
    "dhcp.option.router" Ip "Router",
    "ntp" Proto "NTP",
    "ntp.flags.vn" Uint "Version",
    "ntp.flags.mode" Uint "Mode",
    "ntp.stratum" Uint "Stratum",
    "data" Bytes "Undecoded data",
}

pub fn lookup(name: &str) -> Option<(&'static str, Kind)> {
    let name = match name {
        "ssl" => "tls",
        "app" => "app.name",
        "bootp" => "dhcp",
        n => n,
    };
    FIELDS.iter().find(|(n, _, _)| *n == name).map(|(n, k, _)| (*n, *k))
}

pub fn field_list() -> Vec<FieldInfo> {
    FIELDS
        .iter()
        .map(|(name, kind, description)| FieldInfo {
            name,
            kind: *kind,
            description,
        })
        .collect()
}

// ------------------------------------------------------------- AST ---

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
    Matches,
}

#[derive(Debug, Clone)]
enum Lit {
    Num(u64),
    Float(f64),
    Str(String),
    Net(IpAddr, u8),
    Mac([u8; 6]),
    Bytes(Vec<u8>),
    Re(Regex),
}

#[derive(Debug, Clone)]
enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Present(&'static str),
    Cmp(&'static str, Op, Lit),
    In(&'static str, Vec<Lit>),
}

/// A compiled display filter.
#[derive(Debug, Clone)]
pub struct Filter {
    expr: Expr,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    And,
    Or,
    Not,
    Op(Op),
    In,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        let two: String = c[i..(i + 2).min(c.len())].iter().collect();
        match two.as_str() {
            "&&" => { out.push(Tok::And); i += 2; continue; }
            "||" => { out.push(Tok::Or); i += 2; continue; }
            "==" => { out.push(Tok::Op(Op::Eq)); i += 2; continue; }
            "!=" => { out.push(Tok::Op(Op::Ne)); i += 2; continue; }
            ">=" => { out.push(Tok::Op(Op::Ge)); i += 2; continue; }
            "<=" => { out.push(Tok::Op(Op::Le)); i += 2; continue; }
            _ => {}
        }
        match ch {
            '(' => out.push(Tok::LParen),
            ')' => out.push(Tok::RParen),
            '{' => out.push(Tok::LBrace),
            '}' => out.push(Tok::RBrace),
            '!' => out.push(Tok::Not),
            '>' => out.push(Tok::Op(Op::Gt)),
            '<' => out.push(Tok::Op(Op::Lt)),
            '~' => out.push(Tok::Op(Op::Matches)),
            ',' => {}
            '"' => {
                let mut v = String::new();
                i += 1;
                while i < c.len() && c[i] != '"' {
                    if c[i] == '\\' && i + 1 < c.len() {
                        i += 1;
                        v.push(match c[i] {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            o => o,
                        });
                    } else {
                        v.push(c[i]);
                    }
                    i += 1;
                }
                if i >= c.len() {
                    return Err("missing closing quote".into());
                }
                out.push(Tok::Str(v));
            }
            '=' => out.push(Tok::Op(Op::Eq)),
            _ => {
                let start = i;
                while i < c.len()
                    && (c[i].is_alphanumeric() || matches!(c[i], '.' | '_' | '-' | ':' | '/'))
                {
                    i += 1;
                }
                if start == i {
                    return Err(format!("unexpected character '{ch}'"));
                }
                let w: String = c[start..i].iter().collect();
                out.push(match w.to_ascii_lowercase().as_str() {
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    "eq" => Tok::Op(Op::Eq),
                    "ne" => Tok::Op(Op::Ne),
                    "gt" => Tok::Op(Op::Gt),
                    "ge" => Tok::Op(Op::Ge),
                    "lt" => Tok::Op(Op::Lt),
                    "le" => Tok::Op(Op::Le),
                    "contains" => Tok::Op(Op::Contains),
                    "matches" => Tok::Op(Op::Matches),
                    "in" => Tok::In,
                    _ => Tok::Ident(w),
                });
                continue;
            }
        }
        i += 1;
    }
    Ok(out)
}

struct Parser {
    t: Vec<Tok>,
    p: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.p)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.t.get(self.p).cloned();
        self.p += 1;
        t
    }
    fn or(&mut self) -> Result<Expr, String> {
        let mut l = self.and()?;
        while self.peek() == Some(&Tok::Or) {
            self.p += 1;
            let r = self.and()?;
            l = Expr::Or(Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut l = self.unary()?;
        while self.peek() == Some(&Tok::And) {
            self.p += 1;
            let r = self.unary()?;
            l = Expr::And(Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Not) => {
                self.p += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Some(Tok::LParen) => {
                self.p += 1;
                let e = self.or()?;
                if self.next() != Some(Tok::RParen) {
                    return Err("missing ')'".into());
                }
                Ok(e)
            }
            _ => self.test(),
        }
    }
    fn test(&mut self) -> Result<Expr, String> {
        let name = match self.next() {
            Some(Tok::Ident(n)) => n,
            Some(t) => return Err(format!("expected a field name, found {t:?}")),
            None => return Err("unexpected end of filter".into()),
        };
        let (field, kind) = lookup(&name.to_ascii_lowercase()).ok_or_else(|| format!("unknown field \"{name}\""))?;
        match self.peek().cloned() {
            Some(Tok::Op(op)) => {
                self.p += 1;
                let raw = match self.next() {
                    Some(Tok::Ident(v)) => (v, false),
                    Some(Tok::Str(v)) => (v, true),
                    _ => return Err(format!("missing value after {field}")),
                };
                let lit = literal(field, kind, op, &raw.0, raw.1)?;
                Ok(Expr::Cmp(field, op, lit))
            }
            Some(Tok::In) => {
                self.p += 1;
                if self.next() != Some(Tok::LBrace) {
                    return Err("expected '{' after in".into());
                }
                let mut vals = Vec::new();
                loop {
                    match self.next() {
                        Some(Tok::RBrace) => break,
                        Some(Tok::Ident(v)) => vals.push(literal(field, kind, Op::Eq, &v, false)?),
                        Some(Tok::Str(v)) => vals.push(literal(field, kind, Op::Eq, &v, true)?),
                        _ => return Err("missing '}'".into()),
                    }
                }
                Ok(Expr::In(field, vals))
            }
            _ => Ok(Expr::Present(field)),
        }
    }
}

fn parse_num(s: &str) -> Option<u64> {
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split([':', '-', '.']).collect();
    if parts.len() != 6 {
        return None;
    }
    let mut m = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        m[i] = u8::from_str_radix(p, 16).ok()?;
    }
    Some(m)
}

fn parse_bytes(s: &str) -> Option<Vec<u8>> {
    let parts: Vec<&str> = s.split([':', '-', ' ', '.']).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts.iter().any(|p| p.len() != 2) {
        return None;
    }
    parts.iter().map(|p| u8::from_str_radix(p, 16).ok()).collect()
}

fn literal(field: &str, kind: Kind, op: Op, v: &str, quoted: bool) -> Result<Lit, String> {
    if op == Op::Matches {
        return Regex::new(&format!("(?i){v}"))
            .map(Lit::Re)
            .map_err(|e| format!("bad regular expression: {e}"));
    }
    let bad = || format!("\"{v}\" is not a valid value for {field}");
    Ok(match kind {
        Kind::Uint => match v.to_ascii_lowercase().as_str() {
            "true" => Lit::Num(1),
            "false" => Lit::Num(0),
            _ => Lit::Num(parse_num(v).ok_or_else(bad)?),
        },
        Kind::Bool => match v.to_ascii_lowercase().as_str() {
            "1" | "true" => Lit::Num(1),
            "0" | "false" => Lit::Num(0),
            _ => return Err(bad()),
        },
        Kind::Float => Lit::Float(v.parse().map_err(|_| bad())?),
        Kind::Str => Lit::Str(v.to_string()),
        Kind::Ip => {
            let (a, p) = v.split_once('/').unwrap_or((v, ""));
            let ip: IpAddr = a.parse().map_err(|_| bad())?;
            let max = if ip.is_ipv4() { 32 } else { 128 };
            let prefix = if p.is_empty() { max } else { p.parse::<u8>().map_err(|_| bad())?.min(max) };
            Lit::Net(ip, prefix)
        }
        Kind::Mac => Lit::Mac(parse_mac(v).ok_or_else(bad)?),
        Kind::Bytes | Kind::Proto => {
            if quoted {
                Lit::Bytes(v.as_bytes().to_vec())
            } else {
                Lit::Bytes(parse_bytes(v).ok_or_else(bad)?)
            }
        }
    })
}

impl Filter {
    pub fn parse(text: &str) -> Result<Filter, String> {
        let toks = tokenize(text)?;
        if toks.is_empty() {
            return Err("empty filter".into());
        }
        let mut p = Parser { t: toks, p: 0 };
        let expr = p.or()?;
        if p.p < p.t.len() {
            return Err(format!("unexpected {:?}", p.t[p.p]));
        }
        Ok(Filter {
            expr,
            text: text.to_string(),
        })
    }

    /// Evaluate against the fields of one frame.
    pub fn matches(&self, fields: &[Field], data: &[u8]) -> bool {
        eval(&self.expr, fields, data)
    }
}

fn eval(e: &Expr, f: &[Field], d: &[u8]) -> bool {
    match e {
        Expr::And(a, b) => eval(a, f, d) && eval(b, f, d),
        Expr::Or(a, b) => eval(a, f, d) || eval(b, f, d),
        Expr::Not(a) => !eval(a, f, d),
        Expr::Present(name) => f.iter().any(|x| x.name == *name),
        Expr::Cmp(name, Op::Ne, lit) => !f.iter().filter(|x| x.name == *name).any(|x| cmp(x, Op::Eq, lit, d)),
        Expr::Cmp(name, op, lit) => f.iter().filter(|x| x.name == *name).any(|x| cmp(x, *op, lit, d)),
        Expr::In(name, lits) => f
            .iter()
            .filter(|x| x.name == *name)
            .any(|x| lits.iter().any(|l| cmp(x, Op::Eq, l, d))),
    }
}

fn ord<T: PartialOrd>(a: T, b: T, op: Op) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        Op::Gt => a > b,
        Op::Ge => a >= b,
        Op::Lt => a < b,
        Op::Le => a <= b,
        _ => false,
    }
}

fn in_net(ip: &IpAddr, net: &IpAddr, prefix: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => {
            let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix as u32) };
            u32::from(*a) & mask == u32::from(*n) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(n)) => {
            let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix as u32) };
            u128::from(*a) & mask == u128::from(*n) & mask
        }
        _ => false,
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

fn raw<'a>(x: &Field, d: &'a [u8]) -> &'a [u8] {
    let s = (x.start as usize).min(d.len());
    let e = (s + x.len as usize).min(d.len());
    &d[s..e]
}

fn cmp(x: &Field, op: Op, lit: &Lit, d: &[u8]) -> bool {
    match (&x.val, lit) {
        (Val::U(v), Lit::Num(n)) => ord(*v, *n, op),
        (Val::U(v), Lit::Float(n)) => ord(*v as f64, *n, op),
        (Val::B(b), Lit::Num(n)) => ord(*b as u64, *n, op),
        (Val::F(v), Lit::Float(n)) => ord(*v, *n, op),
        (Val::F(v), Lit::Num(n)) => ord(*v, *n as f64, op),
        (Val::S(s), Lit::Str(t)) => match op {
            Op::Contains => s.contains(t.as_str()),
            _ => ord(s.as_str(), t.as_str(), op),
        },
        (Val::S(s), Lit::Re(re)) => re.is_match(s),
        (Val::U(v), Lit::Re(re)) => re.is_match(&v.to_string()),
        (Val::Ip(ip), Lit::Net(n, p)) => match op {
            Op::Eq => in_net(ip, n, *p),
            _ => false,
        },
        (Val::Ip(ip), Lit::Re(re)) => re.is_match(&ip.to_string()),
        (Val::Mac(m), Lit::Mac(n)) => ord(m, n, op),
        (Val::Raw | Val::None, Lit::Bytes(b)) => match op {
            Op::Contains => contains(raw(x, d), b),
            Op::Eq => raw(x, d) == b.as_slice(),
            _ => false,
        },
        (Val::Raw | Val::None, Lit::Re(re)) => re.is_match(&String::from_utf8_lossy(raw(x, d))),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(name: &'static str, val: Val) -> Field {
        Field { name, val, start: 0, len: 0 }
    }

    #[test]
    fn parses_and_evaluates() {
        let fields = vec![
            f("ip", Val::None),
            f("ip.addr", Val::Ip("10.1.2.3".parse().unwrap())),
            f("ip.addr", Val::Ip("8.8.8.8".parse().unwrap())),
            f("tcp", Val::None),
            f("tcp.port", Val::U(51000)),
            f("tcp.port", Val::U(443)),
            f("tls.handshake.extensions_server_name", Val::S("www.github.com".into())),
            f("tcp.flags.syn", Val::B(true)),
        ];
        let t = |s: &str| Filter::parse(s).unwrap().matches(&fields, &[]);
        assert!(t("tcp"));
        assert!(!t("udp"));
        assert!(t("ip.addr == 10.0.0.0/8"));
        assert!(t("ip.addr == 8.8.8.8 && tcp.port == 443"));
        assert!(!t("ip.addr != 8.8.8.8"));
        assert!(t("tcp.port in {80 443}"));
        assert!(t("tls.handshake.extensions_server_name contains \"github\""));
        assert!(t("tls.handshake.extensions_server_name matches \"GITHUB\\\\.com$\""));
        assert!(t("not udp and (tcp.port > 50000 or arp)"));
        assert!(t("tcp.flags.syn == 1"));
    }

    #[test]
    fn reports_errors() {
        assert!(Filter::parse("tcp.prot == 1").unwrap_err().contains("unknown field"));
        assert!(Filter::parse("ip.addr == 300.1.1.1").is_err());
        assert!(Filter::parse("(tcp").is_err());
        assert!(Filter::parse("tcp.port ==").is_err());
        assert!(Filter::parse("http.host == \"x").is_err());
    }

    #[test]
    fn bytes_contains() {
        let fields = vec![Field { name: "tcp.payload", val: Val::Raw, start: 2, len: 4 }];
        let data = b"xxGET yy";
        assert!(Filter::parse("tcp.payload contains \"GET\"").unwrap().matches(&fields, data));
        assert!(Filter::parse("tcp.payload contains 47:45").unwrap().matches(&fields, data));
        assert!(!Filter::parse("tcp.payload contains \"yy\"").unwrap().matches(&fields, data));
    }

    #[test]
    fn registry_has_no_duplicates() {
        let mut names: Vec<&str> = FIELDS.iter().map(|f| f.0).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len());
    }
}
