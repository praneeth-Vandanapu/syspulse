// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/dissect/dns.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Name and address service dissectors: DNS / mDNS / LLMNR (with
//           name compression), DHCP (message type, client, offered address,
//           host name) and NTP. DNS answers feed NetLens' address -> host
//           name table so conversations show readable names.
// =============================================================================

use std::net::IpAddr;

use super::{Cx, DnsFacts, Proto, Rd, Val};

fn qtype_name(t: u16) -> String {
    match t {
        1 => "A".into(),
        2 => "NS".into(),
        5 => "CNAME".into(),
        6 => "SOA".into(),
        12 => "PTR".into(),
        15 => "MX".into(),
        16 => "TXT".into(),
        28 => "AAAA".into(),
        33 => "SRV".into(),
        41 => "OPT".into(),
        47 => "NSEC".into(),
        64 => "SVCB".into(),
        65 => "HTTPS".into(),
        255 => "ANY".into(),
        t => format!("TYPE{t}"),
    }
}

fn rcode_name(c: u8) -> &'static str {
    match c {
        0 => "No error",
        1 => "Format error",
        2 => "Server failure",
        3 => "No such name",
        4 => "Not implemented",
        5 => "Refused",
        _ => "Error",
    }
}

/// Read a (possibly compressed) domain name. Returns (name, bytes consumed).
pub fn read_name(msg: &[u8], start: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut p = start;
    let mut consumed = None;
    let mut jumps = 0;
    loop {
        let len = *msg.get(p)? as usize;
        if len == 0 {
            p += 1;
            break;
        }
        if len & 0xc0 == 0xc0 {
            let ptr = ((len & 0x3f) << 8) | *msg.get(p + 1)? as usize;
            if consumed.is_none() {
                consumed = Some(p + 2 - start);
            }
            jumps += 1;
            if jumps > 32 || ptr >= msg.len() {
                return None;
            }
            p = ptr;
            continue;
        }
        let label = msg.get(p + 1..p + 1 + len)?;
        labels.push(String::from_utf8_lossy(label).to_string());
        p += 1 + len;
        if labels.len() > 128 {
            return None;
        }
    }
    let name = if labels.is_empty() { "<Root>".to_string() } else { labels.join(".") };
    Some((name, consumed.unwrap_or_else(|| p - start)))
}

pub fn dns_tcp(cx: &mut Cx, off: usize, end: usize) {
    if end - off > 2 {
        dns(cx, off + 2, end, false);
    }
}

pub fn dns(cx: &mut Cx, off: usize, end: usize, mdns: bool) {
    let msg = &cx.d()[off..end];
    let r = Rd::new(msg);
    let (Some(id), Some(flags), Some(qd), Some(an), Some(ns), Some(ar)) =
        (r.u16(0), r.u16(2), r.u16(4), r.u16(6), r.u16(8), r.u16(10))
    else {
        return;
    };
    let response = flags & 0x8000 != 0;
    let opcode = ((flags >> 11) & 0xf) as u8;
    let rcode = (flags & 0xf) as u8;
    let (field, proto, title) = if mdns {
        ("mdns", Proto::Mdns, "Multicast Domain Name System")
    } else {
        ("dns", Proto::Dns, "Domain Name System")
    };
    cx.layer(field, Some(proto), off, end - off, || {
        format!("{title} ({})", if response { "response" } else { "query" })
    });
    if mdns {
        cx.hidden("dns", Val::None, off, end - off);
    }
    cx.add("dns.id", Val::U(id as u64), off, 2, |_| format!("Transaction ID: 0x{id:04x}"));
    cx.open(off + 2, 2, || {
        format!(
            "Flags: 0x{flags:04x} {}{}",
            if response { "Standard query response" } else { "Standard query" },
            if response && rcode != 0 { format!(", {}", rcode_name(rcode)) } else { String::new() }
        )
    });
    cx.add("dns.flags.response", Val::B(response), off + 2, 1, |_| {
        format!("Response: {}", if response { "Message is a response" } else { "Message is a query" })
    });
    cx.kv("dns.flags.opcode", "Opcode", Val::U(opcode as u64), off + 2, 1);
    cx.add("dns.flags.recdesired", Val::B(flags & 0x0100 != 0), off + 2, 2, |v| format!("Recursion desired: {}", v.display()));
    if response {
        cx.add("dns.flags.rcode", Val::U(rcode as u64), off + 3, 1, |_| format!("Reply code: {} ({rcode})", rcode_name(rcode)));
    }
    cx.close();
    cx.kv("dns.count.queries", "Questions", Val::U(qd as u64), off + 4, 2);
    cx.kv("dns.count.answers", "Answer RRs", Val::U(an as u64), off + 6, 2);
    cx.kv("dns.count.auth_rr", "Authority RRs", Val::U(ns as u64), off + 8, 2);
    cx.kv("dns.count.add_rr", "Additional RRs", Val::U(ar as u64), off + 10, 2);

    let mut facts = DnsFacts {
        id,
        response,
        rcode,
        ..Default::default()
    };
    let mut p = 12;
    let mut first_q = String::new();
    if qd > 0 {
        cx.open(off + 12, 0, || "Queries".into());
        for _ in 0..qd.min(16) {
            let Some((name, n)) = read_name(msg, p) else { break };
            let (Some(qt), Some(_qc)) = (r.u16(p + n), r.u16(p + n + 2)) else { break };
            let tn = qtype_name(qt);
            if first_q.is_empty() {
                first_q = format!("{tn} {name}");
                facts.qname = name.clone();
            }
            let label = format!("{name}: type {tn}");
            cx.add("dns.qry.name", Val::S(name), off + p, n + 4, |_| label);
            cx.hidden("dns.qry.type", Val::U(qt as u64), off + p + n, 2);
            p += n + 4;
        }
        cx.close();
    }
    let mut answers_txt = Vec::new();
    let sections = [(an, "Answers"), (ns, "Authoritative nameservers"), (ar, "Additional records")];
    for (si, (count, title)) in sections.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        cx.open(off + p, 0, || title.to_string());
        for _ in 0..(*count).min(64) {
            let Some((name, n)) = read_name(msg, p) else { break };
            let (Some(t), Some(ttl), Some(rdlen)) = (r.u16(p + n), r.u32(p + n + 4), r.u16(p + n + 8)) else {
                break;
            };
            let rd = p + n + 10;
            let rdlen = rdlen as usize;
            if rd + rdlen > msg.len() {
                break;
            }
            let tn = qtype_name(t);
            let data_str = match t {
                1 if rdlen == 4 => r.ip4(rd).map(|ip| {
                    if si == 0 {
                        cx.hidden("dns.a", Val::Ip(ip), off + rd, 4);
                        facts.answers.push((name.clone(), ip));
                    }
                    ip.to_string()
                }),
                28 if rdlen == 16 => r.ip6(rd).map(|ip| {
                    if si == 0 {
                        cx.hidden("dns.aaaa", Val::Ip(ip), off + rd, 16);
                        facts.answers.push((name.clone(), ip));
                    }
                    ip.to_string()
                }),
                5 | 2 | 12 => read_name(msg, rd).map(|(n2, _)| {
                    if t == 5 && si == 0 {
                        cx.hidden("dns.cname", Val::S(n2.clone()), off + rd, rdlen);
                    }
                    n2
                }),
                15 => read_name(msg, rd + 2).map(|(n2, _)| format!("{} {n2}", r.u16(rd).unwrap_or(0))),
                16 => Some(
                    String::from_utf8_lossy(r.bytes(rd + 1, (r.u8(rd).unwrap_or(0) as usize).min(rdlen.saturating_sub(1))).unwrap_or_default())
                        .to_string(),
                ),
                41 => Some(format!("UDP payload size {}", r.u16(p + n + 2).unwrap_or(0))),
                _ => Some(format!("{rdlen} bytes")),
            }
            .unwrap_or_default();
            if si == 0 && answers_txt.len() < 4 {
                answers_txt.push(format!("{tn} {data_str}"));
            }
            let label = if t == 41 {
                format!("<Root>: type OPT, {data_str}")
            } else {
                format!("{name}: type {tn}, ttl {ttl}, {data_str}")
            };
            if si == 0 {
                cx.add("dns.resp.name", Val::S(name), off + p, n + 10 + rdlen, |_| label);
                cx.hidden("dns.resp.type", Val::U(t as u64), off + p + n, 2);
                cx.hidden("dns.resp.ttl", Val::U(ttl as u64), off + p + n + 4, 4);
            } else {
                cx.text(off + p, n + 10 + rdlen, || label);
            }
            p = rd + rdlen;
        }
        cx.close();
    }
    if response {
        if let Some(t) = cx.meta.response_time {
            cx.add("dns.time", Val::F(t), off, 0, |_| format!("[Time: {:.3} ms]", t * 1000.0));
        }
        if rcode != 0 {
            cx.expert(2, "", off + 2, 2, || format!("DNS error: {}", rcode_name(rcode)));
        }
    }
    cx.close();
    let kind = if response { "Standard query response" } else { "Standard query" };
    cx.info(|| {
        let mut s = format!("{kind} 0x{id:04x} {first_q}");
        if response && rcode != 0 {
            s.push_str(&format!(" {}", rcode_name(rcode)));
        }
        for a in &answers_txt {
            s.push(' ');
            s.push_str(a);
        }
        s
    });
    cx.out.facts.dns = Some(facts);
}

// -------------------------------------------------------------- DHCP ---

pub fn dhcp(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let (Some(op), Some(xid)) = (r.u8(off), r.u32(off + 4)) else { return };
    let yi: Option<IpAddr> = r.ip4(off + 16);
    let mac = r.mac(off + 28).unwrap_or_default();
    cx.layer("dhcp", Some(Proto::Dhcp), off, end - off, || "Dynamic Host Configuration Protocol".into());
    cx.add("dhcp.type", Val::U(op as u64), off, 1, |_| {
        format!("Message type: {}", if op == 1 { "Boot Request (1)" } else { "Boot Reply (2)" })
    });
    cx.add("dhcp.id", Val::U(xid as u64), off + 4, 4, |_| format!("Transaction ID: 0x{xid:08x}"));
    if let Some(ip) = yi {
        cx.kv("dhcp.ip.your", "Your (client) IP address", Val::Ip(ip), off + 16, 4);
    }
    cx.kv("dhcp.hw.mac_addr", "Client MAC address", Val::Mac(mac), off + 28, 6);
    let mut msg_type = 0u8;
    let mut host = String::new();
    // Options after the magic cookie.
    let mut p = off + 240;
    if r.u32(off + 236) == Some(0x6382_5363) {
        while p + 2 <= end {
            let Some(code) = r.u8(p) else { break };
            if code == 255 {
                break;
            }
            if code == 0 {
                p += 1;
                continue;
            }
            let Some(len) = r.u8(p + 1).map(|v| v as usize) else { break };
            match code {
                53 => {
                    msg_type = r.u8(p + 2).unwrap_or(0);
                    let n = dhcp_msg_name(msg_type);
                    cx.add("dhcp.option.dhcp", Val::U(msg_type as u64), p, len + 2, |_| format!("Option (53) DHCP Message Type: {n}"));
                }
                12 => {
                    host = String::from_utf8_lossy(r.bytes(p + 2, len).unwrap_or_default()).to_string();
                    let h = host.clone();
                    cx.add("dhcp.option.hostname", Val::S(host.clone()), p, len + 2, |_| format!("Option (12) Host Name: {h}"));
                }
                50 => {
                    if let Some(ip) = r.ip4(p + 2) {
                        cx.add("dhcp.option.requested_ip_address", Val::Ip(ip), p, len + 2, |_| format!("Option (50) Requested IP Address: {ip}"));
                    }
                }
                54 => {
                    if let Some(ip) = r.ip4(p + 2) {
                        cx.add("dhcp.option.dhcp_server_id", Val::Ip(ip), p, len + 2, |_| format!("Option (54) DHCP Server Identifier: {ip}"));
                    }
                }
                3 => {
                    if let Some(ip) = r.ip4(p + 2) {
                        cx.add("dhcp.option.router", Val::Ip(ip), p, len + 2, |_| format!("Option (3) Router: {ip}"));
                    }
                }
                6 => {
                    let ips: Vec<String> = (0..len / 4).filter_map(|i| r.ip4(p + 2 + i * 4)).map(|i| i.to_string()).collect();
                    let j = ips.join(", ");
                    cx.text(p, len + 2, || format!("Option (6) Domain Name Server: {j}"));
                }
                51 => {
                    let s = r.u32(p + 2).unwrap_or(0);
                    cx.text(p, len + 2, || format!("Option (51) IP Address Lease Time: {s} s"));
                }
                _ => cx.text(p, len + 2, || format!("Option ({code}) {len} bytes")),
            }
            p += 2 + len;
        }
    }
    cx.close();
    let n = dhcp_msg_name(msg_type);
    cx.info(|| {
        let mut s = format!("DHCP {n} - Transaction ID 0x{xid:08x}");
        if !host.is_empty() {
            s.push_str(&format!(" ({host})"));
        }
        s
    });
}

fn dhcp_msg_name(t: u8) -> &'static str {
    match t {
        1 => "Discover",
        2 => "Offer",
        3 => "Request",
        4 => "Decline",
        5 => "ACK",
        6 => "NAK",
        7 => "Release",
        8 => "Inform",
        _ => "message",
    }
}

// --------------------------------------------------------------- NTP ---

pub fn ntp(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let Some(b0) = r.u8(off) else { return };
    let mode = b0 & 7;
    let stratum = r.u8(off + 1).unwrap_or(0);
    let mname = match mode {
        1 => "symmetric active",
        2 => "symmetric passive",
        3 => "client",
        4 => "server",
        5 => "broadcast",
        6 => "control",
        _ => "reserved",
    };
    cx.layer("ntp", Some(Proto::Ntp), off, end - off, || "Network Time Protocol".into());
    cx.kv("ntp.flags.vn", "Version", Val::U(((b0 >> 3) & 7) as u64), off, 1);
    cx.add("ntp.flags.mode", Val::U(mode as u64), off, 1, |_| format!("Mode: {mname} ({mode})"));
    cx.kv("ntp.stratum", "Stratum", Val::U(stratum as u64), off + 1, 1);
    cx.close();
    cx.info(|| format!("NTP Version {}, {mname}", (b0 >> 3) & 7));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_names() {
        // header(12) + "a.example.com" + pointer to offset 14 ("example.com")
        let mut m = vec![0u8; 12];
        m.extend_from_slice(b"\x01a\x07example\x03com\x00");
        let ptr_at = m.len();
        m.extend_from_slice(&[0xc0, 14]);
        assert_eq!(read_name(&m, 12), Some(("a.example.com".into(), 15)));
        assert_eq!(read_name(&m, ptr_at), Some(("example.com".into(), 2)));
        // loops are rejected
        let looped = vec![0xc0, 0x00];
        assert_eq!(read_name(&looped, 0), None);
    }
}
