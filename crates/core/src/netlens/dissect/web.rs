// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/dissect/web.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Web protocol dissectors:
//             HTTP/1.x (and SSDP, which uses HTTP syntax over UDP);
//             TLS record layer + handshake (ClientHello SNI/ALPN/versions,
//             ServerHello negotiated version and cipher, certificate names,
//             alerts) - the visible part of HTTPS;
//             QUIC long/short headers (HTTP/3 transport).
//           Encrypted payloads are identified, never decrypted.
// =============================================================================

use super::{printable, split_lines, Cx, MsgFacts, Proto, Rd, Val};

const METHODS: [&str; 10] = [
    "GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH", "CONNECT", "TRACE", "PROPFIND",
];
const SSDP_METHODS: [&str; 3] = ["NOTIFY", "M-SEARCH", "SUBSCRIBE"];

pub fn looks_like_http(p: &[u8]) -> bool {
    if p.starts_with(b"HTTP/1.") {
        return true;
    }
    METHODS.iter().chain(SSDP_METHODS.iter()).any(|m| {
        p.len() > m.len() && p.starts_with(m.as_bytes()) && p[m.len()] == b' '
    })
}

pub fn looks_like_tls(p: &[u8]) -> bool {
    p.len() >= 5 && (20..=24).contains(&p[0]) && p[1] == 3 && p[2] <= 4
}

fn find_header_end(b: &[u8]) -> Option<usize> {
    b.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .or_else(|| b.windows(2).position(|w| w == b"\n\n").map(|p| p + 2))
}

fn header_field(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "host" => "http.host",
        "user-agent" => "http.user_agent",
        "content-type" => "http.content_type",
        "content-length" => "http.content_length",
        "server" => "http.server",
        "location" => "http.location",
        "referer" => "http.referer",
        "cookie" => "http.cookie",
        "set-cookie" => "http.set_cookie",
        "authorization" => "http.authorization",
        "www-authenticate" => "http.www_authenticate",
        "connection" => "http.connection",
        "transfer-encoding" => "http.transfer_encoding",
        "content-encoding" => "http.content_encoding",
        "accept" => "http.accept",
        "accept-encoding" => "http.accept_encoding",
        "cache-control" => "http.cache_control",
        "upgrade" => "http.upgrade",
        "date" => "http.date",
        "last-modified" => "http.last_modified",
        "x-forwarded-for" => "http.x_forwarded_for",
        _ => return None,
    })
}

/// HTTP/1.x request or response (or SSDP when `ssdp`).
pub fn http(cx: &mut Cx, off: usize, end: usize, ssdp: bool) {
    let payload = &cx.d()[off..end];
    let (field, proto, name) = if ssdp {
        ("ssdp", Proto::Ssdp, "Simple Service Discovery Protocol")
    } else {
        ("http", Proto::Http, "Hypertext Transfer Protocol")
    };
    if !looks_like_http(payload) {
        cx.layer(field, Some(proto), off, end - off, || name.to_string());
        cx.add("http.file_data", Val::Raw, off, end - off, |_| {
            format!("Continuation data ({} bytes)", end - off)
        });
        cx.close();
        cx.info(|| "Continuation".into());
        return;
    }
    let hdr_end = find_header_end(payload).unwrap_or(payload.len());
    let lines = split_lines(&payload[..hdr_end]);
    let Some(&(_, first)) = lines.first() else { return };
    let first_s = String::from_utf8_lossy(first).to_string();
    cx.layer(field, Some(proto), off, end - off, || name.to_string());

    let mut facts = MsgFacts::default();
    let mut parts = first_s.splitn(3, ' ');
    let (a, b, c) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    );
    let fl = first.len();
    cx.open(off, fl + 2, || format!("{}\\r\\n", printable(first, 300)));
    if a.starts_with("HTTP/") {
        let code: u16 = b.parse().unwrap_or(0);
        facts.code = code;
        cx.hidden("http.response", Val::None, off, fl);
        cx.kv("http.response.version", "Response version", Val::S(a.clone()), off, a.len());
        cx.add("http.response.code", Val::U(code as u64), off + a.len() + 1, b.len(), |_| {
            format!("Status code: {code}")
        });
        cx.kv("http.response.phrase", "Response phrase", Val::S(c.clone()), off + a.len() + b.len() + 2, c.len());
        if code >= 400 {
            cx.expert(2, "", off, fl, || format!("HTTP error {code} {c}"));
        }
    } else {
        facts.request = true;
        facts.method = a.clone();
        cx.hidden("http.request", Val::None, off, fl);
        cx.kv("http.request.method", "Request method", Val::S(a.clone()), off, a.len());
        cx.kv("http.request.uri", "Request URI", Val::S(b.clone()), off + a.len() + 1, b.len());
        cx.kv("http.request.version", "Request version", Val::S(c.clone()), off + a.len() + b.len() + 2, c.len());
    }
    cx.close();

    let mut content_type = String::new();
    let mut auth = false;
    for &(lo, line) in lines.iter().skip(1) {
        if line.is_empty() {
            continue;
        }
        let s = String::from_utf8_lossy(line);
        let start = off + lo;
        if let Some((k, v)) = s.split_once(':') {
            let v = v.trim().to_string();
            match header_field(k) {
                Some("http.content_length") => {
                    let n: u64 = v.parse().unwrap_or(0);
                    cx.add("http.content_length", Val::U(n), start, line.len(), |_| format!("{k}: {v}"));
                }
                Some(f) => {
                    if f == "http.host" {
                        facts.host = v.clone();
                    }
                    if f == "http.content_type" {
                        content_type = v.clone();
                    }
                    if f == "http.authorization" {
                        auth = true;
                    }
                    let label = format!("{k}: {}", printable(v.as_bytes(), 300));
                    cx.add(f, Val::S(v), start, line.len(), |_| label);
                }
                None => {
                    let label = printable(line, 300);
                    cx.text(start, line.len(), || label);
                }
            }
        } else {
            let label = printable(line, 300);
            cx.text(start, line.len(), || label);
        }
    }
    if auth {
        cx.expert(1, "", off, fl, || "Credentials are sent in clear text (no HTTPS)".into());
    }
    if !facts.request {
        if let Some(t) = cx.meta.response_time {
            cx.add("http.time", Val::F(t), off, 0, |_| format!("[Time since request: {:.3} ms]", t * 1000.0));
        }
    }
    if hdr_end < payload.len() {
        let blen = payload.len() - hdr_end;
        let ct = content_type.clone();
        cx.add("http.file_data", Val::Raw, off + hdr_end, blen, |_| {
            format!("File data: {blen} bytes{}", if ct.is_empty() { String::new() } else { format!(" ({ct})") })
        });
        let texty = ct.contains("text") || ct.contains("json") || ct.contains("xml") || ct.contains("javascript");
        if texty {
            let preview = printable(&payload[hdr_end..], 160);
            cx.text(off + hdr_end, blen, || format!("Preview: {preview}"));
        }
        if ct.contains("json") {
            cx.out.label = Some("HTTP/JSON");
        } else if ct.contains("xml") {
            cx.out.label = Some("HTTP/XML");
        }
    }
    cx.close();

    let line = printable(first, 200);
    let ct_short = content_type.split(';').next().unwrap_or("").to_string();
    cx.info(|| {
        if !facts_is_request(&first_s) && !ct_short.is_empty() {
            format!("{line}  ({ct_short})")
        } else {
            line
        }
    });
    cx.out.facts.http = Some(facts);
}

fn facts_is_request(first: &str) -> bool {
    !first.starts_with("HTTP/")
}

// --------------------------------------------------------------- TLS ---

pub fn tls_version_name(v: u16) -> &'static str {
    match v {
        0x0300 => "SSLv3",
        0x0301 => "TLSv1.0",
        0x0302 => "TLSv1.1",
        0x0303 => "TLSv1.2",
        0x0304 => "TLSv1.3",
        v if v & 0x0f0f == 0x0a0a => "GREASE",
        _ => "Unknown",
    }
}

fn content_type_name(t: u8) -> &'static str {
    match t {
        20 => "Change Cipher Spec",
        21 => "Alert",
        22 => "Handshake",
        23 => "Application Data",
        24 => "Heartbeat",
        _ => "Unknown",
    }
}

fn handshake_name(t: u8) -> &'static str {
    match t {
        0 => "Hello Request",
        1 => "Client Hello",
        2 => "Server Hello",
        4 => "New Session Ticket",
        8 => "Encrypted Extensions",
        11 => "Certificate",
        12 => "Server Key Exchange",
        13 => "Certificate Request",
        14 => "Server Hello Done",
        15 => "Certificate Verify",
        16 => "Client Key Exchange",
        20 => "Finished",
        _ => "",
    }
}

pub fn cipher_name(c: u16) -> String {
    let n = match c {
        0x1301 => "TLS_AES_128_GCM_SHA256",
        0x1302 => "TLS_AES_256_GCM_SHA384",
        0x1303 => "TLS_CHACHA20_POLY1305_SHA256",
        0xc02b => "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
        0xc02c => "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
        0xc02f => "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
        0xc030 => "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
        0xcca8 => "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
        0xcca9 => "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256",
        0xc013 => "TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA",
        0xc014 => "TLS_ECDHE_RSA_WITH_AES_256_CBC_SHA",
        0xc009 => "TLS_ECDHE_ECDSA_WITH_AES_128_CBC_SHA",
        0xc00a => "TLS_ECDHE_ECDSA_WITH_AES_256_CBC_SHA",
        0xc023 => "TLS_ECDHE_ECDSA_WITH_AES_128_CBC_SHA256",
        0xc027 => "TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA256",
        0x009c => "TLS_RSA_WITH_AES_128_GCM_SHA256",
        0x009d => "TLS_RSA_WITH_AES_256_GCM_SHA384",
        0x002f => "TLS_RSA_WITH_AES_128_CBC_SHA",
        0x0035 => "TLS_RSA_WITH_AES_256_CBC_SHA",
        0x003c => "TLS_RSA_WITH_AES_128_CBC_SHA256",
        0x000a => "TLS_RSA_WITH_3DES_EDE_CBC_SHA",
        0x00ff => "TLS_EMPTY_RENEGOTIATION_INFO_SCSV",
        c if c & 0x0f0f == 0x0a0a => "Reserved (GREASE)",
        _ => "",
    };
    if n.is_empty() {
        format!("Unknown (0x{c:04x})")
    } else {
        format!("{n} (0x{c:04x})")
    }
}

fn ext_name(t: u16) -> &'static str {
    match t {
        0 => "server_name",
        5 => "status_request",
        10 => "supported_groups",
        11 => "ec_point_formats",
        13 => "signature_algorithms",
        16 => "application_layer_protocol_negotiation",
        18 => "signed_certificate_timestamp",
        21 => "padding",
        23 => "extended_master_secret",
        27 => "compress_certificate",
        35 => "session_ticket",
        41 => "pre_shared_key",
        43 => "supported_versions",
        45 => "psk_key_exchange_modes",
        51 => "key_share",
        57 => "quic_transport_parameters",
        17513 | 17613 => "application_settings",
        0xfe0d => "encrypted_client_hello",
        0xff01 => "renegotiation_info",
        t if t & 0x0f0f == 0x0a0a => "Reserved (GREASE)",
        _ => "unknown",
    }
}

fn alert_name(d: u8) -> &'static str {
    match d {
        0 => "Close Notify",
        10 => "Unexpected Message",
        20 => "Bad Record MAC",
        40 => "Handshake Failure",
        42 => "Bad Certificate",
        43 => "Unsupported Certificate",
        44 => "Certificate Revoked",
        45 => "Certificate Expired",
        46 => "Certificate Unknown",
        47 => "Illegal Parameter",
        48 => "Unknown CA",
        50 => "Decode Error",
        51 => "Decrypt Error",
        70 => "Protocol Version",
        71 => "Insufficient Security",
        80 => "Internal Error",
        86 => "Inappropriate Fallback",
        90 => "User Canceled",
        109 => "Missing Extension",
        110 => "Unsupported Extension",
        112 => "Unrecognized Name",
        116 => "Certificate Required",
        120 => "No Application Protocol",
        _ => "Unknown",
    }
}

pub fn tls(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let stream_ver = cx.meta.tls_version;
    cx.out.label = Some(if stream_ver == 0x0304 { "TLSv1.3" } else { "TLS" });
    cx.layer("tls", Some(Proto::Tls), off, end - off, || "Transport Layer Security".into());
    let mut p = off;
    let mut infos: Vec<String> = Vec::new();
    let mut first = true;
    while p + 5 <= end {
        let (Some(ct), Some(ver), Some(len)) = (r.u8(p), r.u16(p + 1), r.u16(p + 3)) else { break };
        if !(20..=24).contains(&ct) || ver >> 8 != 3 {
            if first {
                cx.add("tls.continuation", Val::Raw, p, end - p, |_| {
                    format!("Continuation data ({} bytes, part of an earlier record)", end - p)
                });
                infos.push("Continuation Data".into());
            }
            break;
        }
        first = false;
        let len = len as usize;
        let body = p + 5;
        let rend = (body + len).min(end);
        let partial = body + len > end;
        let shown_ver = if stream_ver == 0x0304 { "TLSv1.3" } else { tls_version_name(ver) };
        if cx.out.label == Some("TLS") && ver >= 0x0301 {
            cx.out.label = Some(match tls_version_name(ver) {
                "TLSv1.0" => "TLSv1",
                "TLSv1.1" => "TLSv1.1",
                "TLSv1.2" => "TLSv1.2",
                _ => "TLS",
            });
        }
        cx.open(p, rend - p, || {
            format!("{shown_ver} Record Layer: {} ({len} bytes{})", content_type_name(ct), if partial { ", continues in next segment" } else { "" })
        });
        cx.add("tls.record.content_type", Val::U(ct as u64), p, 1, |_| {
            format!("Content type: {} ({ct})", content_type_name(ct))
        });
        cx.add("tls.record.version", Val::U(ver as u64), p + 1, 2, |_| {
            format!("Version: {} (0x{ver:04x})", tls_version_name(ver))
        });
        cx.kv("tls.record.length", "Length", Val::U(len as u64), p + 3, 2);
        match ct {
            22 => handshake(cx, body, rend, partial, &mut infos),
            21 => {
                if len == 2 && !partial {
                    let (lvl, desc) = (r.u8(body).unwrap_or(0), r.u8(body + 1).unwrap_or(0));
                    let lname = if lvl == 2 { "Fatal" } else { "Warning" };
                    cx.kv("tls.alert_message.level", "Level", Val::U(lvl as u64), body, 1);
                    cx.add("tls.alert_message.desc", Val::U(desc as u64), body + 1, 1, |_| {
                        format!("Description: {} ({desc})", alert_name(desc))
                    });
                    cx.out.facts.tls_alert = Some((lvl, desc));
                    if desc != 0 {
                        cx.expert(if lvl == 2 { 3 } else { 2 }, "", body, 2, || format!("TLS alert: {lname}, {}", alert_name(desc)));
                    }
                    infos.push(format!("Alert ({lname}, {})", alert_name(desc)));
                } else {
                    cx.add("tls.alert_message", Val::Raw, body, rend - body, |_| "Encrypted Alert".into());
                    cx.hidden("tls.alert_message.encrypted", Val::None, body, rend - body);
                    infos.push("Encrypted Alert".into());
                }
            }
            20 => {
                cx.text(body, rend - body, || "Change Cipher Spec Message".into());
                infos.push("Change Cipher Spec".into());
            }
            23 => {
                cx.add("tls.app_data", Val::Raw, body, rend - body, |_| {
                    format!("Encrypted Application Data ({len} bytes)")
                });
                infos.push("Application Data".into());
            }
            _ => {
                infos.push(content_type_name(ct).to_string());
            }
        }
        cx.close();
        p = body + len;
    }
    cx.close();
    // Collapse "Application Data, Application Data, ..." runs.
    infos.dedup();
    cx.info(|| infos.join(", "));
}

fn handshake(cx: &mut Cx, mut p: usize, end: usize, partial: bool, infos: &mut Vec<String>) {
    let r = Rd::new(cx.data);
    while p + 4 <= end {
        let (Some(t), Some(len)) = (r.u8(p), r.u24(p + 1)) else { break };
        let len = len as usize;
        let name = handshake_name(t);
        // Encrypted handshake messages (TLS 1.2 Finished after CCS) look random.
        if name.is_empty() || (!partial && p + 4 + len > end) {
            cx.add("tls.handshake.encrypted", Val::Raw, p, end - p, |_| "Encrypted Handshake Message".into());
            infos.push("Encrypted Handshake Message".into());
            return;
        }
        let mend = (p + 4 + len).min(end);
        cx.open(p, mend - p, || format!("Handshake Protocol: {name}"));
        cx.add("tls.handshake.type", Val::U(t as u64), p, 1, |_| format!("Handshake type: {name} ({t})"));
        cx.kv("tls.handshake.length", "Length", Val::U(len as u64), p + 1, 3);
        let body = p + 4;
        let mut info = name.to_string();
        match t {
            1 | 2 => hello(cx, body, mend, t == 1, &mut info),
            11 => certificate(cx, body, mend, &mut info),
            _ => {}
        }
        cx.close();
        infos.push(info);
        p = body + len;
    }
}

fn hello(cx: &mut Cx, b: usize, end: usize, client: bool, info: &mut String) {
    let r = Rd::new(cx.data);
    let Some(ver) = r.u16(b) else { return };
    // Hellos carry the real (legacy) protocol version; the record layer of a
    // Client Hello says TLS 1.0 for compatibility, so prefer the hello's.
    if matches!(cx.out.label, Some("TLS") | Some("TLSv1") | Some("TLSv1.1")) && ver >= 0x0302 {
        cx.out.label = Some(if ver >= 0x0303 { "TLSv1.2" } else { "TLSv1.1" });
    }
    cx.add("tls.handshake.version", Val::U(ver as u64), b, 2, |_| {
        format!("Version: {} (0x{ver:04x})", tls_version_name(ver))
    });
    cx.add("tls.handshake.random", Val::Raw, b + 2, 32, |_| "Random: (32 bytes)".into());
    let Some(sid_len) = r.u8(b + 34).map(|v| v as usize) else { return };
    cx.kv("tls.handshake.session_id_length", "Session ID length", Val::U(sid_len as u64), b + 34, 1);
    let mut p = b + 35 + sid_len;
    let mut chosen_ver = ver;
    if client {
        let Some(cs_len) = r.u16(p).map(|v| v as usize) else { return };
        let n = cs_len / 2;
        cx.open(p, 2 + cs_len, || format!("Cipher suites ({n} suites)"));
        for i in 0..n.min(64) {
            let c = r.u16(p + 2 + i * 2).unwrap_or(0);
            cx.add("tls.handshake.ciphersuite", Val::U(c as u64), p + 2 + i * 2, 2, |_| {
                format!("Cipher suite: {}", cipher_name(c))
            });
        }
        cx.close();
        p += 2 + cs_len;
        let Some(comp) = r.u8(p).map(|v| v as usize) else { return };
        p += 1 + comp;
    } else {
        let Some(c) = r.u16(p) else { return };
        cx.add("tls.handshake.ciphersuite", Val::U(c as u64), p, 2, |_| {
            format!("Cipher suite: {}", cipher_name(c))
        });
        p += 3;
    }
    let Some(ext_len) = r.u16(p).map(|v| v as usize) else { return };
    let eend = (p + 2 + ext_len).min(end);
    let count = {
        let mut q = p + 2;
        let mut n = 0;
        while q + 4 <= eend {
            n += 1;
            q += 4 + r.u16(q + 2).unwrap_or(0) as usize;
        }
        n
    };
    cx.open(p, eend - p, || format!("Extensions ({count})"));
    let mut q = p + 2;
    while q + 4 <= eend {
        let (Some(et), Some(el)) = (r.u16(q), r.u16(q + 2)) else { break };
        let el = el as usize;
        let d = q + 4;
        let dend = (d + el).min(eend);
        match et {
            0 => {
                // server_name list: len(2) type(1) name_len(2) name
                if let (Some(nl), Some(name)) = (r.u16(d + 3), r.bytes(d + 5, r.u16(d + 3).unwrap_or(0) as usize)) {
                    let sni = String::from_utf8_lossy(name).to_string();
                    let label = format!("Extension: server_name: {sni}");
                    cx.add("tls.handshake.extensions_server_name", Val::S(sni.clone()), d + 5, nl as usize, |_| label);
                    cx.out.facts.tls_sni = Some(sni.clone());
                    *info = format!("{info} (SNI={sni})");
                }
            }
            16 => {
                let mut a = d + 2;
                let mut protos = Vec::new();
                while a < dend {
                    let Some(l) = r.u8(a).map(|v| v as usize) else { break };
                    if let Some(s) = r.bytes(a + 1, l) {
                        let s = String::from_utf8_lossy(s).to_string();
                        cx.hidden("tls.handshake.extensions_alpn_str", Val::S(s.clone()), a + 1, l);
                        protos.push(s);
                    }
                    a += 1 + l;
                }
                let joined = protos.join(", ");
                cx.text(q, dend - q, || format!("Extension: ALPN: {joined}"));
            }
            43 => {
                if client {
                    let n = r.u8(d).unwrap_or(0) as usize / 2;
                    let vs: Vec<&str> = (0..n)
                        .filter_map(|i| r.u16(d + 1 + i * 2))
                        .map(tls_version_name)
                        .filter(|v| *v != "GREASE")
                        .collect();
                    let joined = vs.join(", ");
                    cx.text(q, dend - q, || format!("Extension: supported_versions: {joined}"));
                } else if let Some(v) = r.u16(d) {
                    chosen_ver = v;
                    cx.add("tls.handshake.extensions.supported_version", Val::U(v as u64), d, 2, |_| {
                        format!("Extension: supported_versions: {} (negotiated)", tls_version_name(v))
                    });
                }
            }
            0xfe0d => {
                cx.expert(1, "tls.handshake.ech", q, dend - q, || {
                    "Extension: encrypted_client_hello (the real server name is hidden)".into()
                });
            }
            _ => {
                cx.add("tls.handshake.extension.type", Val::U(et as u64), q, dend - q, |_| {
                    format!("Extension: {} (len={el})", ext_name(et))
                });
            }
        }
        q = d + el;
    }
    cx.close();
    if !client {
        cx.out.facts.tls_server_version = Some(chosen_ver);
        if chosen_ver == 0x0304 {
            cx.out.label = Some("TLSv1.3");
        }
        *info = format!("{info} ({})", tls_version_name(chosen_ver));
    }
}

/// Common Name values from a DER certificate: (subject, issuer).
fn cert_names(der: &[u8]) -> (Option<String>, Option<String>) {
    const OID_CN: [u8; 5] = [0x06, 0x03, 0x55, 0x04, 0x03];
    let mut found = Vec::new();
    let mut i = 0;
    while i + OID_CN.len() + 2 < der.len() {
        if der[i..i + 5] == OID_CN {
            let tag = der[i + 5];
            let len = der[i + 6] as usize;
            if matches!(tag, 0x0c | 0x13 | 0x16 | 0x14) && len < 128 {
                if let Some(v) = der.get(i + 7..i + 7 + len) {
                    found.push(String::from_utf8_lossy(v).to_string());
                }
            }
            i += 5;
        } else {
            i += 1;
        }
    }
    // Issuer appears before subject in TBSCertificate.
    match found.len() {
        0 => (None, None),
        1 => (found.pop(), None),
        _ => (found.get(1).cloned(), found.first().cloned()),
    }
}

fn certificate(cx: &mut Cx, b: usize, end: usize, info: &mut String) {
    let r = Rd::new(cx.data);
    let Some(total) = r.u24(b).map(|v| v as usize) else { return };
    let mut p = b + 3;
    let lend = (b + 3 + total).min(end);
    let mut n = 0;
    while p + 3 <= lend {
        let Some(cl) = r.u24(p).map(|v| v as usize) else { break };
        let cend = (p + 3 + cl).min(lend);
        let (subject, issuer) = cert_names(&cx.d()[p + 3..cend]);
        let s = subject.clone().unwrap_or_else(|| "?".into());
        let iss = issuer.clone().unwrap_or_else(|| "?".into());
        cx.open(p, cend - p, || format!("Certificate {} ({cl} bytes): CN={s}, issued by CN={iss}", n + 1));
        if let Some(s) = subject {
            if n == 0 {
                *info = format!("{info} (CN={s})");
            }
            cx.kv("tls.cert.subject_cn", "Subject common name", Val::S(s), p + 3, cend - p - 3);
        }
        if let Some(i) = issuer {
            cx.kv("tls.cert.issuer_cn", "Issuer common name", Val::S(i), p + 3, cend - p - 3);
        }
        cx.close();
        n += 1;
        p = p + 3 + cl;
    }
}

// -------------------------------------------------------------- QUIC ---

/// QUIC packet header. Returns false when the datagram does not look like QUIC.
pub fn quic(cx: &mut Cx, off: usize, end: usize) -> bool {
    let r = Rd::new(cx.data);
    let Some(b0) = r.u8(off) else { return false };
    if b0 & 0x40 == 0 && b0 & 0x80 == 0 {
        return false;
    }
    if b0 & 0x80 != 0 {
        let (Some(ver), Some(dl)) = (r.u32(off + 1), r.u8(off + 5)) else { return false };
        let known = matches!(ver, 0 | 1 | 0x6b33_43cf) || ver & 0xffff_ff00 == 0xff00_0000;
        if !known || dl > 20 {
            return false;
        }
        let dcid = r.bytes(off + 6, dl as usize).unwrap_or_default();
        let sl_off = off + 6 + dl as usize;
        let sl = r.u8(sl_off).unwrap_or(0).min(20);
        let scid = r.bytes(sl_off + 1, sl as usize).unwrap_or_default();
        let raw_type = (b0 >> 4) & 3;
        let ptype = match (ver, raw_type) {
            (0, _) => "Version Negotiation",
            (0x6b33_43cf, 0) => "Retry",
            (0x6b33_43cf, 1) => "Initial",
            (0x6b33_43cf, 2) => "0-RTT",
            (0x6b33_43cf, _) => "Handshake",
            (_, 0) => "Initial",
            (_, 1) => "0-RTT",
            (_, 2) => "Handshake",
            _ => "Retry",
        };
        let vname = match ver {
            0 => "negotiation".to_string(),
            1 => "1".into(),
            0x6b33_43cf => "2".into(),
            v => format!("draft-{}", v & 0xff),
        };
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let (dh, sh) = (hex(dcid), hex(scid));
        cx.layer("quic", Some(Proto::Quic), off, end - off, || {
            format!("QUIC IETF, {ptype}, version {vname}")
        });
        cx.add("quic.header_form", Val::U(1), off, 1, |_| "Header form: Long header".into());
        cx.kv("quic.long.packet_type", "Packet type", Val::S(ptype.into()), off, 1);
        cx.add("quic.version", Val::U(ver as u64), off + 1, 4, |_| format!("Version: {vname} (0x{ver:08x})"));
        cx.kv("quic.dcid", "Destination connection ID", Val::S(dh.clone()), off + 6, dl as usize);
        cx.kv("quic.scid", "Source connection ID", Val::S(sh.clone()), sl_off + 1, sl as usize);
        cx.add("quic.remaining_payload", Val::Raw, sl_off + 1 + sl as usize, end.saturating_sub(sl_off + 1 + sl as usize), |_| {
            "Protected payload (encrypted)".into()
        });
        cx.close();
        cx.info(|| {
            let short = |s: &str| if s.len() > 16 { format!("{}…", &s[..16]) } else { s.to_string() };
            format!("{ptype}, DCID={}, SCID={}", short(&dh), short(&sh))
        });
        true
    } else {
        cx.layer("quic", Some(Proto::Quic), off, end - off, || "QUIC IETF, short header (1-RTT)".into());
        cx.add("quic.header_form", Val::U(0), off, 1, |_| "Header form: Short header".into());
        let kp = (b0 >> 2) & 1;
        cx.text(off, 1, || format!("Key phase: {kp}"));
        cx.add("quic.protected_payload", Val::Raw, off + 1, end - off - 1, |_| "Protected payload (encrypted)".into());
        cx.close();
        cx.info(|| format!("Protected Payload (KP{kp})"));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects() {
        assert!(looks_like_http(b"GET / HTTP/1.1\r\n"));
        assert!(looks_like_http(b"HTTP/1.1 200 OK\r\n"));
        assert!(looks_like_http(b"M-SEARCH * HTTP/1.1\r\n"));
        assert!(!looks_like_http(b"GETX"));
        assert!(looks_like_tls(&[22, 3, 1, 0, 5]));
        assert!(!looks_like_tls(&[22, 4, 1, 0, 5]));
    }

    #[test]
    fn cert_cn() {
        // issuer CN=CA, subject CN=example.com
        let mut der = vec![0x30, 0x00];
        der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 0x02, b'C', b'A']);
        der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 0x0b]);
        der.extend_from_slice(b"example.com");
        der.extend_from_slice(&[0, 0, 0]);
        assert_eq!(cert_names(&der), (Some("example.com".into()), Some("CA".into())));
    }
}
