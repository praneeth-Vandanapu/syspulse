// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/dissect/media.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Streaming media dissectors (IP cameras, NVRs, media servers):
//             RTSP 1.0 requests/responses incl. Transport negotiation and
//             interleaved ($) binary frames over TCP;
//             SDP session descriptions (codecs via a=rtpmap);
//             RTP (with H.264 / H.265 NAL unit types, so key frames are
//             visible) and RTCP (SR, RR with loss/jitter, SDES, BYE,
//             feedback such as PLI/FIR key-frame requests).
//           UDP RTP flows are recognised from the ports negotiated in RTSP
//           SETUP (see analysis), like Wireshark's conversation tracking.
// =============================================================================

use std::net::IpAddr;

use super::{printable, split_lines, Cx, MsgFacts, Proto, Rd, RtpFacts, RtspTransport, Val};

const METHODS: [&str; 11] = [
    "OPTIONS",
    "DESCRIBE",
    "SETUP",
    "PLAY",
    "PAUSE",
    "TEARDOWN",
    "GET_PARAMETER",
    "SET_PARAMETER",
    "ANNOUNCE",
    "RECORD",
    "REDIRECT",
];

pub fn looks_like_rtsp(p: &[u8]) -> bool {
    if p.starts_with(b"RTSP/1.") {
        return true;
    }
    let line_end = p.iter().position(|&b| b == b'\r' || b == b'\n').unwrap_or(p.len().min(512));
    let line = &p[..line_end];
    METHODS.iter().any(|m| line.starts_with(m.as_bytes()) && line.get(m.len()) == Some(&b' '))
        && line.windows(7).any(|w| w == b"RTSP/1.")
}

fn rtsp_status_hint(code: u16) -> Option<&'static str> {
    Some(match code {
        401 => "Authentication required: the camera/server needs a user name and password",
        403 => "Forbidden: the credentials are not allowed to open this stream",
        404 => "Stream not found: check the URL path",
        453 => "Not enough bandwidth",
        454 => "Session not found (expired or wrong Session header)",
        455 => "Method not valid in this state",
        461 => "Unsupported transport: try RTP over TCP (interleaved) or another port",
        500..=599 => "Server error",
        _ => return None,
    })
}

/// RTSP message or interleaved frame. Returns false when it is not RTSP.
pub fn rtsp(cx: &mut Cx, off: usize, end: usize, known_port: bool) -> bool {
    let payload = &cx.d()[off..end];
    if payload[0] == b'$' && known_port {
        interleaved(cx, off, end);
        return true;
    }
    if !looks_like_rtsp(payload) {
        if !known_port {
            return false;
        }
        cx.layer("rtsp", Some(Proto::Rtsp), off, end - off, || "Real Time Streaming Protocol".into());
        cx.add("rtsp.data", Val::Raw, off, end - off, |_| format!("Continuation ({} bytes)", end - off));
        cx.close();
        cx.info(|| "Continuation".into());
        return true;
    }

    let hdr_end = payload
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(payload.len());
    let lines = split_lines(&payload[..hdr_end]);
    let first = lines.first().map(|l| l.1).unwrap_or_default();
    let first_s = String::from_utf8_lossy(first).to_string();
    cx.layer("rtsp", Some(Proto::Rtsp), off, end - off, || "Real Time Streaming Protocol".into());
    let mut facts = MsgFacts::default();
    let mut parts = first_s.splitn(3, ' ');
    let (a, b, c) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    );
    if a.starts_with("RTSP/") {
        let code: u16 = b.parse().unwrap_or(0);
        facts.code = code;
        cx.hidden("rtsp.response", Val::None, off, first.len());
        cx.add("rtsp.status", Val::U(code as u64), off, first.len(), |_| {
            format!("Response: {first_s}")
        });
        if let Some(h) = rtsp_status_hint(code) {
            cx.expert(2, "", off, first.len(), || format!("{code} {c}: {h}"));
        }
        if let Some(t) = cx.meta.response_time {
            cx.add("rtsp.time", Val::F(t), off, 0, |_| format!("[Time since request: {:.3} ms]", t * 1000.0));
        }
    } else {
        facts.request = true;
        facts.method = a.clone();
        cx.hidden("rtsp.request", Val::None, off, first.len());
        cx.open(off, first.len(), || format!("Request: {first_s}"));
        cx.kv("rtsp.method", "Method", Val::S(a.clone()), off, a.len());
        cx.kv("rtsp.url", "URL", Val::S(b.clone()), off + a.len() + 1, b.len());
        cx.close();
        if b.contains('@') && b.contains(':') && b.split('@').next().is_some_and(|x| x.matches(':').count() >= 2) {
            cx.expert(1, "", off, first.len(), || "The URL contains a user name and password in clear text".into());
        }
    }

    let mut content_type = String::new();
    for &(lo, line) in lines.iter().skip(1) {
        if line.is_empty() {
            continue;
        }
        let s = String::from_utf8_lossy(line).to_string();
        let start = off + lo;
        let Some((k, v)) = s.split_once(':') else {
            cx.text(start, line.len(), || printable(line, 300));
            continue;
        };
        let v = v.trim().to_string();
        let label = format!("{k}: {}", printable(v.as_bytes(), 300));
        match k.to_ascii_lowercase().as_str() {
            "cseq" => {
                let n: u32 = v.parse().unwrap_or(0);
                facts.cseq = Some(n);
                cx.add("rtsp.cseq", Val::U(n as u64), start, line.len(), |_| label);
            }
            "session" => cx.add("rtsp.session", Val::S(v.split(';').next().unwrap_or("").to_string()), start, line.len(), |_| label),
            "transport" => {
                let t = parse_transport(&v);
                cx.add("rtsp.transport", Val::S(v.clone()), start, line.len(), |_| label);
                let summary = transport_summary(&t);
                cx.text(start, line.len(), || format!("    → {summary}"));
                cx.out.facts.rtsp_transport = Some(t);
            }
            "content-type" => {
                content_type = v.clone();
                cx.add("rtsp.content_type", Val::S(v), start, line.len(), |_| label);
            }
            "content-length" => cx.add("rtsp.content_length", Val::U(v.parse().unwrap_or(0)), start, line.len(), |_| label),
            "content-base" => cx.add("rtsp.content_base", Val::S(v), start, line.len(), |_| label),
            "public" => cx.add("rtsp.public", Val::S(v), start, line.len(), |_| label),
            "range" => cx.add("rtsp.range", Val::S(v), start, line.len(), |_| label),
            "rtp-info" => cx.add("rtsp.rtp_info", Val::S(v), start, line.len(), |_| label),
            "www-authenticate" => cx.add("rtsp.www_authenticate", Val::S(v), start, line.len(), |_| label),
            "authorization" => cx.add("rtsp.authorization", Val::S(v), start, line.len(), |_| label),
            "user-agent" => cx.add("rtsp.user_agent", Val::S(v), start, line.len(), |_| label),
            "server" => cx.add("rtsp.server", Val::S(v), start, line.len(), |_| label),
            _ => cx.text(start, line.len(), || label),
        }
    }
    cx.close();

    let mut sdp_info = false;
    if hdr_end < payload.len() {
        if content_type.contains("sdp") {
            sdp(cx, off + hdr_end, end);
            sdp_info = true;
            cx.out.label = Some("RTSP/SDP");
        } else {
            let n = payload.len() - hdr_end;
            cx.add("rtsp.data", Val::Raw, off + hdr_end, n, |_| format!("Body ({n} bytes)"));
        }
    }
    cx.out.proto = Proto::Rtsp;
    let code = facts.code;
    cx.info(|| {
        let mut s = if facts_is_reply(&first_s) {
            format!("Reply: {first_s}")
        } else {
            first_s.clone()
        };
        if sdp_info {
            s.push_str(" (application/sdp)");
        }
        if code == 401 {
            s.push_str(" — authentication required");
        }
        s
    });
    cx.out.facts.rtsp = Some(facts);
    true
}

fn facts_is_reply(first: &str) -> bool {
    first.starts_with("RTSP/")
}

pub fn parse_transport(v: &str) -> RtspTransport {
    let mut t = RtspTransport::default();
    // Several alternatives may be offered, separated by commas: use the first.
    let first = v.split(',').next().unwrap_or(v);
    let range = |s: &str| -> Option<(u16, u16)> {
        let (a, b) = s.split_once('-').unwrap_or((s, ""));
        let a: u16 = a.trim().parse().ok()?;
        let b: u16 = b.trim().parse().unwrap_or(a.saturating_add(1));
        Some((a, b))
    };
    for part in first.split(';') {
        let (k, val) = part.split_once('=').unwrap_or((part, ""));
        match k.trim().to_ascii_lowercase().as_str() {
            "client_port" => t.client_ports = range(val),
            "server_port" => t.server_ports = range(val),
            "port" => t.client_ports = range(val),
            "interleaved" => t.interleaved = true,
            "destination" => t.destination = val.trim().parse::<IpAddr>().ok(),
            "rtp/avp/tcp" => t.interleaved = true,
            _ => {}
        }
    }
    t
}

fn transport_summary(t: &RtspTransport) -> String {
    if t.interleaved {
        return "media will be sent inside this TCP connection (interleaved)".into();
    }
    let mut s = String::from("media over UDP");
    if let Some((a, b)) = t.client_ports {
        s.push_str(&format!(", client ports {a}-{b}"));
    }
    if let Some((a, b)) = t.server_ports {
        s.push_str(&format!(", server ports {a}-{b}"));
    }
    if let Some(d) = t.destination {
        s.push_str(&format!(", destination {d}"));
    }
    s
}

fn interleaved(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let mut p = off;
    let mut n = 0;
    while p + 4 <= end && r.u8(p) == Some(b'$') && n < 16 {
        let ch = r.u8(p + 1).unwrap_or(0);
        let len = r.u16(p + 2).unwrap_or(0) as usize;
        let fend = (p + 4 + len).min(end);
        cx.layer("rtsp", Some(Proto::Rtsp), p, fend - p, || {
            format!("RTSP Interleaved Frame, Channel: 0x{ch:02x}, {len} bytes")
        });
        cx.kv("rtsp.channel", "Channel", Val::U(ch as u64), p + 1, 1);
        cx.kv("rtsp.length", "Length", Val::U(len as u64), p + 2, 2);
        cx.close();
        if ch.is_multiple_of(2) {
            rtp(cx, p + 4, fend);
        } else {
            rtcp(cx, p + 4, fend);
        }
        n += 1;
        p = p + 4 + len;
    }
    if p < end && n > 0 {
        let rest = end - p;
        cx.text(p, rest, || format!("Remaining {rest} bytes (frame continues in next segment)"));
    }
}

// --------------------------------------------------------------- SDP ---

fn sdp(cx: &mut Cx, off: usize, end: usize) {
    let body = &cx.d()[off..end];
    cx.layer("sdp", None, off, end - off, || "Session Description Protocol".into());
    for (lo, line) in split_lines(body) {
        if line.len() < 2 || line[1] != b'=' {
            continue;
        }
        let start = off + lo;
        let val = String::from_utf8_lossy(&line[2..]).to_string();
        let (field, name): (&'static str, &str) = match line[0] {
            b'v' => ("sdp.version", "Session Description Protocol Version (v)"),
            b'o' => ("sdp.owner", "Owner/Creator, Session Id (o)"),
            b's' => ("sdp.session_name", "Session Name (s)"),
            b'i' => ("sdp.session_info", "Session Information (i)"),
            b'u' => ("sdp.uri", "URI of Description (u)"),
            b'c' => ("sdp.connection_info", "Connection Information (c)"),
            b'b' => ("sdp.bandwidth", "Bandwidth Information (b)"),
            b't' => ("sdp.time", "Time Description, active time (t)"),
            b'm' => ("sdp.media", "Media Description, name and address (m)"),
            b'a' => ("sdp.media_attr", "Media Attribute (a)"),
            _ => ("", "Line"),
        };
        if line[0] == b'a' {
            if let Some(rest) = val.strip_prefix("rtpmap:") {
                // rtpmap:96 H264/90000
                let mut it = rest.splitn(2, ' ');
                let pt: Option<u8> = it.next().and_then(|x| x.parse().ok());
                let enc = it.next().unwrap_or("");
                let mut e = enc.split('/');
                let (codec, clock) = (e.next().unwrap_or("").to_string(), e.next().and_then(|c| c.parse().ok()).unwrap_or(90000));
                if let Some(pt) = pt {
                    cx.out.facts.rtpmap.push((pt, codec.clone(), clock));
                }
                cx.add("sdp.rtpmap", Val::S(rest.to_string()), start, line.len(), |_| {
                    format!("Media Attribute (a): rtpmap: payload type {} = {codec}, clock {clock} Hz", pt.unwrap_or(0))
                });
                continue;
            }
            if let Some(rest) = val.strip_prefix("control:") {
                cx.add("sdp.control", Val::S(rest.to_string()), start, line.len(), |_| format!("Media Attribute (a): control: {rest}"));
                continue;
            }
            if let Some(rest) = val.strip_prefix("fmtp:") {
                cx.add("sdp.fmtp", Val::S(rest.to_string()), start, line.len(), |_| format!("Media Attribute (a): fmtp: {}", printable(rest.as_bytes(), 200)));
                continue;
            }
        }
        if field.is_empty() {
            cx.text(start, line.len(), || printable(line, 300));
        } else {
            cx.add(field, Val::S(val.clone()), start, line.len(), |_| format!("{name}: {}", printable(val.as_bytes(), 300)));
        }
    }
    cx.close();
}

// --------------------------------------------------------------- RTP ---

pub fn static_pt(pt: u8) -> Option<(&'static str, u32)> {
    Some(match pt {
        0 => ("PCMU", 8000),
        3 => ("GSM", 8000),
        4 => ("G723", 8000),
        8 => ("PCMA", 8000),
        9 => ("G722", 8000),
        10 | 11 => ("L16", 44100),
        13 => ("CN", 8000),
        14 => ("MPA", 90000),
        18 => ("G729", 8000),
        26 => ("JPEG", 90000),
        31 => ("H261", 90000),
        32 => ("MPV", 90000),
        33 => ("MP2T", 90000),
        34 => ("H263", 90000),
        _ => return None,
    })
}

/// Codec name and clock rate for a payload type (SDP first, then static).
pub fn codec_for(cx_shared: &super::Shared, pt: u8) -> (String, u32) {
    if let Some((n, c)) = cx_shared.rtpmap.get(&pt) {
        return (n.clone(), *c);
    }
    match static_pt(pt) {
        Some((n, c)) => (n.to_string(), c),
        None => (format!("dynamic-{pt}"), 90000),
    }
}

fn h264_nal(t: u8) -> &'static str {
    match t {
        1 => "non-IDR slice",
        5 => "IDR slice (key frame)",
        6 => "SEI",
        7 => "SPS",
        8 => "PPS",
        9 => "Access unit delimiter",
        24 => "STAP-A",
        28 => "FU-A",
        29 => "FU-B",
        _ => "NAL unit",
    }
}

fn h265_nal(t: u8) -> &'static str {
    match t {
        0..=9 => "slice",
        16..=21 => "IRAP slice (key frame)",
        32 => "VPS",
        33 => "SPS",
        34 => "PPS",
        35 => "Access unit delimiter",
        39 | 40 => "SEI",
        48 => "Aggregation packet",
        49 => "Fragmentation unit",
        _ => "NAL unit",
    }
}

pub fn rtp(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let (Some(b0), Some(b1), Some(seq), Some(ts), Some(ssrc)) =
        (r.u8(off), r.u8(off + 1), r.u16(off + 2), r.u32(off + 4), r.u32(off + 8))
    else {
        return;
    };
    if b0 >> 6 != 2 {
        cx.add("data", Val::Raw, off, end - off, |_| format!("Data ({} bytes, not RTP version 2)", end - off));
        return;
    }
    let padding = b0 & 0x20 != 0;
    let ext = b0 & 0x10 != 0;
    let cc = (b0 & 0x0f) as usize;
    let marker = b1 & 0x80 != 0;
    let pt = b1 & 0x7f;
    let (codec, clock) = codec_for(cx.shared, pt);
    let mut p = off + 12 + cc * 4;
    cx.layer("rtp", Some(Proto::Rtp), off, end - off, || "Real-Time Transport Protocol".into());
    cx.kv("rtp.version", "Version", Val::U(2), off, 1);
    cx.kv("rtp.padding", "Padding", Val::B(padding), off, 1);
    cx.kv("rtp.ext", "Extension", Val::B(ext), off, 1);
    cx.kv("rtp.cc", "Contributing source count", Val::U(cc as u64), off, 1);
    cx.add("rtp.marker", Val::B(marker), off + 1, 1, |_| {
        format!("Marker: {}", if marker { "True (end of frame)" } else { "False" })
    });
    let codec_l = codec.clone();
    cx.add("rtp.p_type", Val::U(pt as u64), off + 1, 1, |_| format!("Payload type: {codec_l} ({pt}), clock {clock} Hz"));
    cx.hidden("rtp.codec", Val::S(codec.clone()), off + 1, 1);
    cx.kv("rtp.seq", "Sequence number", Val::U(seq as u64), off + 2, 2);
    cx.kv("rtp.timestamp", "Timestamp", Val::U(ts as u64), off + 4, 4);
    cx.add("rtp.ssrc", Val::U(ssrc as u64), off + 8, 4, |_| format!("Synchronization source identifier: 0x{ssrc:08x} ({ssrc})"));
    if ext {
        let words = r.u16(p + 2).unwrap_or(0) as usize;
        let profile = r.u16(p).unwrap_or(0);
        cx.text(p, 4 + words * 4, || format!("Header extension: profile 0x{profile:04x}, {} bytes", words * 4));
        p += 4 + words * 4;
    }
    let mut pend = end;
    if padding {
        if let Some(n) = end.checked_sub(1).and_then(|i| r.u8(i)) {
            pend = end.saturating_sub(n as usize).max(p);
        }
    }
    let plen = pend.saturating_sub(p);
    let mut nal_desc = String::new();
    let mut keyframe = false;
    let upper = codec.to_ascii_uppercase();
    if plen > 0 && (upper == "H264" || upper == "H265" || upper == "HEVC") {
        let b = r.u8(p).unwrap_or(0);
        if upper == "H264" {
            let t = b & 0x1f;
            let mut inner = t;
            let mut frag = "";
            if t == 28 {
                let fu = r.u8(p + 1).unwrap_or(0);
                inner = fu & 0x1f;
                frag = if fu & 0x80 != 0 { " start" } else if fu & 0x40 != 0 { " end" } else { "" };
            }
            nal_desc = if t == 28 {
                format!("FU-A{frag}: {}", h264_nal(inner))
            } else {
                h264_nal(t).to_string()
            };
            cx.add("rtp.nal_type", Val::U(inner as u64), p, 1, |_| format!("H.264 NAL unit: {nal_desc} (type {t})"));
            if inner == 5 || inner == 7 {
                keyframe = true;
                cx.hidden("rtp.keyframe", Val::None, p, 1);
            }
        } else {
            let t = (b >> 1) & 0x3f;
            let mut inner = t;
            if t == 49 {
                inner = r.u8(p + 2).unwrap_or(0) & 0x3f;
            }
            nal_desc = if t == 49 { format!("FU: {}", h265_nal(inner)) } else { h265_nal(t).to_string() };
            cx.add("rtp.nal_type", Val::U(inner as u64), p, 2, |_| format!("H.265 NAL unit: {nal_desc} (type {t})"));
            if (16..=21).contains(&inner) || inner == 32 {
                keyframe = true;
                cx.hidden("rtp.keyframe", Val::None, p, 1);
            }
        }
    }
    cx.add("rtp.payload", Val::Raw, p, plen, |_| format!("Payload: {plen} bytes"));
    cx.close();
    cx.out.facts.rtp = Some(RtpFacts {
        ssrc,
        seq,
        timestamp: ts,
        pt,
        marker,
        payload_len: plen,
        keyframe,
    });
    cx.info(|| {
        let mut s = format!("PT={codec}, SSRC=0x{ssrc:08X}, Seq={seq}, Time={ts}");
        if marker {
            s.push_str(", Mark");
        }
        if !nal_desc.is_empty() {
            s.push_str(&format!(", {nal_desc}"));
        }
        s
    });
}

// -------------------------------------------------------------- RTCP ---

fn rtcp_pt_name(pt: u8) -> &'static str {
    match pt {
        200 => "Sender Report",
        201 => "Receiver Report",
        202 => "Source description",
        203 => "Goodbye",
        204 => "Application specific",
        205 => "Generic RTP Feedback",
        206 => "Payload-specific Feedback",
        207 => "Extended Report",
        _ => "Unknown",
    }
}

pub fn rtcp(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let mut p = off;
    let mut infos = Vec::new();
    cx.layer("rtcp", Some(Proto::Rtcp), off, end - off, || "Real-time Transport Control Protocol".into());
    while p + 4 <= end && infos.len() < 16 {
        let (Some(b0), Some(pt), Some(lw)) = (r.u8(p), r.u8(p + 1), r.u16(p + 2)) else { break };
        if b0 >> 6 != 2 {
            break;
        }
        let rc = (b0 & 0x1f) as usize;
        let len = (lw as usize + 1) * 4;
        let pend = (p + len).min(end);
        let name = rtcp_pt_name(pt);
        cx.open(p, pend - p, || format!("{name} ({len} bytes)"));
        cx.add("rtcp.pt", Val::U(pt as u64), p + 1, 1, |_| format!("Packet type: {name} ({pt})"));
        let mut info = name.to_string();
        match pt {
            200 | 201 => {
                let ssrc = r.u32(p + 4).unwrap_or(0);
                cx.add("rtcp.senderssrc", Val::U(ssrc as u64), p + 4, 4, |_| format!("Sender SSRC: 0x{ssrc:08x}"));
                let mut q = p + 8;
                if pt == 200 {
                    let (pc, oc) = (r.u32(p + 20).unwrap_or(0), r.u32(p + 24).unwrap_or(0));
                    cx.text(p + 8, 8, || format!("NTP timestamp: 0x{:016x}", r.u64(p + 8).unwrap_or(0)));
                    cx.kv("rtcp.timestamp.rtp", "RTP timestamp", Val::U(r.u32(p + 16).unwrap_or(0) as u64), p + 16, 4);
                    cx.kv("rtcp.sender.packetcount", "Sender's packet count", Val::U(pc as u64), p + 20, 4);
                    cx.kv("rtcp.sender.octetcount", "Sender's octet count", Val::U(oc as u64), p + 24, 4);
                    q = p + 28;
                }
                for i in 0..rc {
                    let b = q + i * 24;
                    if b + 24 > pend {
                        break;
                    }
                    let src = r.u32(b).unwrap_or(0);
                    let frac = r.u8(b + 4).unwrap_or(0);
                    let cum = r.u24(b + 5).unwrap_or(0);
                    let cum = if cum & 0x80_0000 != 0 { cum as i64 - 0x100_0000 } else { cum as i64 };
                    let jitter = r.u32(b + 12).unwrap_or(0);
                    let pct = frac as f64 * 100.0 / 256.0;
                    cx.open(b, 24, || format!("Report for SSRC 0x{src:08x}: {pct:.1}% lost, {cum} lost in total, jitter {jitter}"));
                    cx.add("rtcp.ssrc.fraction", Val::U(frac as u64), b + 4, 1, |_| format!("Fraction lost: {frac} / 256 ({pct:.1}%)"));
                    cx.add("rtcp.ssrc.cum_nr", Val::U(cum.max(0) as u64), b + 5, 3, |_| format!("Cumulative packets lost: {cum}"));
                    cx.kv("rtcp.ssrc.ext_high_seq", "Extended highest sequence number", Val::U(r.u32(b + 8).unwrap_or(0) as u64), b + 8, 4);
                    cx.kv("rtcp.ssrc.jitter", "Interarrival jitter", Val::U(jitter as u64), b + 12, 4);
                    if frac > 0 {
                        cx.expert(2, "", b + 4, 1, || format!("Receiver reports {pct:.1}% packet loss"));
                    }
                    cx.close();
                }
            }
            202 => {
                // First chunk: SSRC then items.
                let mut q = p + 8;
                while q + 2 <= pend {
                    let t = r.u8(q).unwrap_or(0);
                    if t == 0 {
                        break;
                    }
                    let l = r.u8(q + 1).unwrap_or(0) as usize;
                    let text = String::from_utf8_lossy(r.bytes(q + 2, l).unwrap_or_default()).to_string();
                    let iname = match t {
                        1 => "CNAME",
                        2 => "NAME",
                        3 => "EMAIL",
                        4 => "PHONE",
                        5 => "LOC",
                        6 => "TOOL",
                        7 => "NOTE",
                        _ => "PRIV",
                    };
                    let label = format!("{iname}: {text}");
                    cx.add("rtcp.sdes.text", Val::S(text), q, 2 + l, |_| label);
                    q += 2 + l;
                }
            }
            206 => {
                let fmt = rc as u8;
                let what = match fmt {
                    1 => "Picture Loss Indication (PLI): receiver asks for a new key frame",
                    4 => "Full Intra Request (FIR): receiver asks for a new key frame",
                    15 => "Application layer feedback (REMB bandwidth estimate)",
                    _ => "Payload-specific feedback",
                };
                cx.add("rtcp.psfb.fmt", Val::U(fmt as u64), p, 1, |_| what.to_string());
                if fmt == 1 || fmt == 4 {
                    cx.out.facts.keyframe_request = true;
                }
                info = match fmt {
                    1 => "PLI (key frame request)".into(),
                    4 => "FIR (key frame request)".into(),
                    _ => info,
                };
            }
            205 if rc == 1 => {
                cx.text(p, pend - p, || "Generic NACK: receiver asks to resend lost packets".into());
                info = "NACK (retransmission request)".into();
            }
            _ => {}
        }
        cx.close();
        infos.push(info);
        p += len;
    }
    cx.close();
    cx.info(|| infos.join("   "));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtsp_detection() {
        assert!(looks_like_rtsp(b"DESCRIBE rtsp://cam/live RTSP/1.0\r\nCSeq: 2\r\n"));
        assert!(looks_like_rtsp(b"RTSP/1.0 200 OK\r\n"));
        assert!(!looks_like_rtsp(b"OPTIONS / HTTP/1.1\r\n"));
    }

    #[test]
    fn transport_header() {
        let t = parse_transport("RTP/AVP;unicast;client_port=5000-5001;server_port=6970-6971");
        assert_eq!(t.client_ports, Some((5000, 5001)));
        assert_eq!(t.server_ports, Some((6970, 6971)));
        assert!(!t.interleaved);
        let t = parse_transport("RTP/AVP/TCP;unicast;interleaved=0-1");
        assert!(t.interleaved);
        let t = parse_transport("RTP/AVP;multicast;destination=239.1.1.1;port=5004-5005;ttl=16");
        assert_eq!(t.destination, Some("239.1.1.1".parse().unwrap()));
        assert_eq!(t.client_ports, Some((5004, 5005)));
    }
}
