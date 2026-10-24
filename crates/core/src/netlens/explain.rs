// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/explain.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Plain-English one-paragraph explanation of a frame, shown above
//           the detail tree ("firefox on this PC asks the DNS server what the
//           IPv4 address of github.com is"). Uses learned host names and the
//           owning application, so the sentence names real things.
// =============================================================================

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use super::dissect::net::an;
use super::dissect::{tcpflag, Dissection, Meta, Proto};

pub fn service(port: u16) -> Option<&'static str> {
    Some(match port {
        20 | 21 => "FTP",
        22 => "SSH",
        23 => "Telnet",
        25 | 465 | 587 => "SMTP (mail)",
        53 => "DNS",
        67 | 68 => "DHCP",
        80 | 8080 | 8000 | 8008 => "HTTP (web)",
        110 | 995 => "POP3 (mail)",
        123 => "NTP (time)",
        137..=139 | 445 => "Windows file sharing",
        143 | 993 => "IMAP (mail)",
        161 | 162 => "SNMP",
        443 | 8443 => "HTTPS (secure web)",
        502 => "Modbus/TCP",
        554 | 8554 => "RTSP (video streaming)",
        631 => "IPP (printing)",
        853 => "DNS over TLS",
        1883 | 8883 => "MQTT",
        1900 => "SSDP (UPnP)",
        3306 => "MySQL",
        3389 => "Remote Desktop",
        5201 => "SysPulse speed test",
        5353 => "mDNS",
        5432 => "PostgreSQL",
        5900..=5903 => "VNC",
        6379 => "Redis",
        27017 => "MongoDB",
        _ => return None,
    })
}

pub struct Names<'a> {
    pub hostnames: &'a HashMap<IpAddr, String>,
    pub local: &'a HashSet<IpAddr>,
}

impl Names<'_> {
    fn who(&self, ip: Option<IpAddr>) -> String {
        let Some(ip) = ip else { return "a device".into() };
        if self.local.contains(&ip) || ip.is_loopback() {
            return "this PC".into();
        }
        if ip.is_multicast() || ip == IpAddr::from([255, 255, 255, 255]) {
            return format!("all devices on the local network ({ip})");
        }
        match self.hostnames.get(&ip) {
            Some(h) => format!("{h} ({ip})"),
            None => ip.to_string(),
        }
    }
}

fn http_meaning(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "created",
        204 => "no content",
        206 => "partial content",
        301 | 308 => "moved permanently",
        302 | 303 | 307 => "redirect",
        304 => "not modified, use the cached copy",
        400 => "bad request",
        401 => "login required",
        403 => "forbidden",
        404 => "not found",
        405 => "method not allowed",
        408 => "request timeout",
        429 => "too many requests",
        500 => "server error",
        502 => "bad gateway",
        503 => "service unavailable",
        504 => "gateway timeout",
        c if c < 300 => "success",
        c if c < 400 => "redirect",
        c if c < 500 => "client error",
        _ => "server error",
    }
}

fn rtsp_method(m: &str) -> &'static str {
    match m {
        "OPTIONS" => "asks which commands the streaming server supports",
        "DESCRIBE" => "asks for the stream description (codecs and tracks)",
        "SETUP" => "sets up how the media will be delivered (ports or TCP)",
        "PLAY" => "tells the server to start streaming",
        "PAUSE" => "pauses the stream",
        "TEARDOWN" => "stops the stream and ends the session",
        "GET_PARAMETER" => "sends a keep-alive so the session does not expire",
        "SET_PARAMETER" => "changes a stream parameter",
        "ANNOUNCE" => "announces a stream it will publish",
        "RECORD" => "starts publishing (recording) a stream",
        _ => "sends a request",
    }
}

fn ms(t: Option<f64>) -> String {
    match t {
        Some(t) => format!(" after {:.1} ms", t * 1000.0),
        None => String::new(),
    }
}

pub fn explain(d: &Dissection, m: &Meta, n: &Names) -> String {
    let f = &d.facts;
    let mut src = n.who(f.src);
    let dst = n.who(f.dst);
    if let (Some(true), Some((app, _))) = (m.outgoing, &m.app) {
        src = format!("{app} on this PC");
    }
    let mut s = match d.proto {
        Proto::Dns | Proto::Mdns => match &f.dns {
            Some(q) if !q.response => {
                if d.proto == Proto::Mdns {
                    format!("{src} looks for \"{}\" on the local network (multicast DNS / Bonjour).", q.qname)
                } else {
                    format!("{src} asks the DNS server {dst} for the address of {}.", q.qname)
                }
            }
            Some(q) if q.rcode == 3 => format!("DNS answer: {} does not exist (no such name){}.", q.qname, ms(m.response_time)),
            Some(q) if q.rcode != 0 => format!("DNS lookup of {} failed (server error){}.", q.qname, ms(m.response_time)),
            Some(q) if !q.answers.is_empty() => {
                let ips: Vec<String> = q.answers.iter().take(3).map(|a| a.1.to_string()).collect();
                format!("DNS answer: {} is {}{}.", q.qname, ips.join(", "), ms(m.response_time))
            }
            Some(q) => format!("DNS answer for {} (no addresses){}.", q.qname, ms(m.response_time)),
            None => format!("DNS message from {src} to {dst}."),
        },
        Proto::Http | Proto::Ssdp => match &f.http {
            Some(h) if h.request => {
                let what = d.info.split_whitespace().nth(1).unwrap_or("/");
                if d.proto == Proto::Ssdp {
                    format!("UPnP discovery: {src} {} devices or services on the network.", if h.method == "NOTIFY" { "announces itself to" } else { "searches for" })
                } else {
                    format!(
                        "{src} asks {dst} for {what} ({} request, host {}). Plain HTTP is not encrypted: anyone on the path can read it.",
                        h.method,
                        if h.host.is_empty() { "?" } else { &h.host }
                    )
                }
            }
            Some(h) => format!("{src} answers with HTTP {} ({}){}.", h.code, http_meaning(h.code), ms(m.response_time)),
            None => format!("More HTTP data from {src} to {dst}."),
        },
        Proto::Tls => {
            if let Some(sni) = &f.tls_sni {
                format!("{src} starts an encrypted HTTPS/TLS session with {sni}. Only the site name is visible; the content will be encrypted.")
            } else if let Some(v) = f.tls_server_version {
                format!(
                    "{src} accepts the encrypted session using {}.",
                    super::dissect::web::tls_version_name(v)
                )
            } else if let Some((lvl, _)) = f.tls_alert {
                if lvl == 2 {
                    format!("The encrypted session between {src} and {dst} failed: {}.", d.info)
                } else {
                    format!("{src} sends a TLS notice: {}.", d.info)
                }
            } else {
                let n = f.tcp.map(|t| t.payload_len).unwrap_or(0);
                format!("{n} bytes of encrypted data from {src} to {dst}. The content cannot be read without the session keys.")
            }
        }
        Proto::Quic => format!("Encrypted QUIC (HTTP/3) traffic from {src} to {dst}. QUIC runs over UDP and encrypts almost everything."),
        Proto::Rtsp => match &f.rtsp {
            Some(r) if r.request => format!("{src} {} ({} to {dst}).", rtsp_method(&r.method), r.method),
            Some(r) => {
                let hint = match r.code {
                    200 => "OK".to_string(),
                    401 => "the camera needs a user name and password".into(),
                    404 => "the stream path does not exist".into(),
                    454 => "the session expired or is unknown".into(),
                    461 => "the requested transport is not supported".into(),
                    c => format!("status {c}"),
                };
                format!("{src} replies: {hint}{}.", ms(m.response_time))
            }
            None => {
                if f.rtp.is_some() {
                    format!("Video/audio data sent inside the RTSP TCP connection from {src} to {dst}.")
                } else {
                    format!("RTSP data from {src} to {dst}.")
                }
            }
        },
        Proto::Rtp => match f.rtp {
            Some(r) => format!(
                "Media packet #{} of stream 0x{:08X} from {src} to {dst}{}{}.",
                r.seq,
                r.ssrc,
                if r.keyframe { " — part of a key frame (a full picture)" } else { "" },
                if r.marker { ", last packet of a video frame" } else { "" }
            ),
            None => format!("RTP media from {src} to {dst}."),
        },
        Proto::Rtcp => {
            if f.keyframe_request {
                format!("{src} asks {dst} for a new key frame (picture loss): the video was probably corrupted by lost packets.")
            } else {
                format!("Stream quality report (RTCP) from {src} to {dst}: packet counts, loss and jitter.")
            }
        }
        Proto::Arp => {
            if d.info.starts_with("Who has") {
                format!("ARP: a device asks the local network \"{}\" — it needs the MAC address before it can send.", d.info)
            } else {
                format!("ARP reply: {}.", d.info)
            }
        }
        Proto::Icmp | Proto::Icmpv6 => match f.echo {
            Some((true, _, _)) => format!("Ping request from {src} to {dst}."),
            Some((false, _, _)) => format!("Ping reply from {src}{} — {dst} can reach it.", ms(m.response_time)),
            None if f.icmp_error => format!("{src} reports a problem: {}. The original packet could not be delivered.", d.info),
            None => format!("Network control message: {}.", d.info),
        },
        Proto::Dhcp => format!("Automatic IP configuration (DHCP): {}.", d.info),
        Proto::Ntp => format!("Clock synchronisation (NTP) between {src} and {dst}."),
        Proto::Igmp => "Multicast group membership (IGMP): a device joins or leaves a multicast group.".into(),
        Proto::Tcp => tcp_sentence(d, &src, &dst),
        Proto::Udp => {
            let (sp, dp, l, _) = f.udp.unwrap_or_default();
            let svc = service(dp).or(service(sp)).map(|s| format!(" ({s})")).unwrap_or_default();
            format!("{src} sends {l} bytes over UDP to {dst} port {dp}{svc}.")
        }
        _ => {
            if d.src.is_empty() {
                format!("{} frame.", d.proto.name())
            } else {
                format!("{} frame from {} to {}.", d.proto.name(), d.src, d.dst)
            }
        }
    };
    let a = m.analysis;
    if a & an::RETRANSMISSION != 0 {
        s.push_str(" ⚠ This is a retransmission: the data was sent again because the first copy was lost or not acknowledged in time (often a busy or lossy network).");
    }
    if a & an::OUT_OF_ORDER != 0 {
        s.push_str(" ⚠ This segment arrived out of order.");
    }
    if a & an::LOST_SEGMENT != 0 {
        s.push_str(" ⚠ Some earlier data of this connection is missing (lost on the network or not captured).");
    }
    if a & an::DUP_ACK != 0 {
        s.push_str(" ⚠ Duplicate acknowledgment: the receiver is signalling that a packet is missing.");
    }
    if a & an::ZERO_WINDOW != 0 {
        s.push_str(" ⚠ The receiver's buffer is full (zero window): the sender must pause. The receiving application is too slow.");
    }
    capitalize(s)
}

/// Sentences may start with "this PC" or a host name: capitalise the first letter.
fn capitalize(s: String) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) if f.is_lowercase() => f.to_uppercase().collect::<String>() + c.as_str(),
        _ => s,
    }
}

fn tcp_sentence(d: &Dissection, src: &str, dst: &str) -> String {
    let Some(t) = d.facts.tcp else { return format!("TCP segment from {src} to {dst}.") };
    let svc = service(t.dport).map(|s| format!(" ({s})")).unwrap_or_default();
    let fl = t.flags;
    if fl & tcpflag::RST != 0 {
        format!("{src} resets the connection with {dst}: it was refused (nothing listening on that port) or aborted.")
    } else if fl & tcpflag::SYN != 0 && fl & tcpflag::ACK == 0 {
        format!("{src} opens a TCP connection to {dst} port {}{svc} (step 1 of the handshake).", t.dport)
    } else if fl & tcpflag::SYN != 0 {
        format!("{src} accepts the connection from {dst} (step 2 of the handshake).")
    } else if fl & tcpflag::FIN != 0 {
        format!("{src} has finished sending and closes its side of the connection.")
    } else if t.payload_len > 0 {
        format!("{src} sends {} bytes to {dst} over TCP{svc}.", t.payload_len)
    } else if d.facts.tcp.is_some() && d.info.contains("Keep-Alive") {
        format!("Keep-alive from {src}: checks that the idle connection is still open.")
    } else {
        format!("{src} acknowledges data received from {dst} (no payload).")
    }
}
