// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/dissect/net.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Frame, link and network/transport layer dissectors: Ethernet,
//           802.1Q VLAN, Linux cooked capture (SLL/SLL2), BSD loopback, raw
//           IP, ARP, IPv4, IPv6 (+ extension headers), ICMP, ICMPv6, IGMP,
//           TCP (options, relative sequence numbers, analysis flags) and UDP,
//           then hands the payload to the application-layer dissectors.
// =============================================================================

use std::net::IpAddr;

use super::{dns, linktype, mac_str, media, tcpflag, web, Cx, Proto, Rd, TcpFacts, Val};

/// TCP analysis flags computed by the capture analysis (stored per frame).
pub mod an {
    pub const RETRANSMISSION: u16 = 0x0001;
    pub const OUT_OF_ORDER: u16 = 0x0002;
    pub const LOST_SEGMENT: u16 = 0x0004;
    pub const DUP_ACK: u16 = 0x0008;
    pub const ZERO_WINDOW: u16 = 0x0010;
    pub const KEEP_ALIVE: u16 = 0x0020;
    pub const PORT_REUSE: u16 = 0x0040;
    /// Problems worth highlighting (keep-alives are normal).
    pub const PROBLEMS: u16 =
        RETRANSMISSION | OUT_OF_ORDER | LOST_SEGMENT | DUP_ACK | ZERO_WINDOW;
}

// ------------------------------------------------------------- frame ---

pub fn frame(cx: &mut Cx, lt: u16) {
    let total = cx.data.len();
    let m = cx.meta;
    let (num, orig) = (m.number, m.orig_len.max(total as u32));
    let iface = m.interface.clone();
    cx.layer("frame", None, 0, total, || {
        let on = if iface.is_empty() { String::new() } else { format!(", on interface {iface}") };
        format!(
            "Frame {num}: {orig} bytes on wire ({} bits), {total} bytes captured{on}",
            orig as u64 * 8
        )
    });
    if !m.ts_abs.is_empty() {
        let t = m.ts_abs.clone();
        cx.text(0, 0, || format!("Arrival time: {t}"));
    }
    cx.add("frame.time_relative", Val::F(m.ts_rel), 0, 0, |_| {
        format!("Time since first frame: {:.6} s", m.ts_rel)
    });
    cx.add("frame.time_delta", Val::F(m.ts_delta), 0, 0, |_| {
        format!("Time since previous frame: {:.6} s", m.ts_delta)
    });
    cx.kv("frame.number", "Frame number", Val::U(num), 0, 0);
    cx.add("frame.len", Val::U(orig as u64), 0, 0, |_| format!("Frame length: {orig} bytes"));
    cx.add("frame.cap_len", Val::U(total as u64), 0, 0, |_| {
        format!("Capture length: {total} bytes")
    });
    if !iface.is_empty() {
        cx.kv("frame.interface_name", "Interface", Val::S(iface), 0, 0);
    }
    if let Some(out) = m.outgoing {
        let d = if out { "out" } else { "in" };
        cx.add("frame.direction", Val::S(d.into()), 0, 0, |_| {
            format!("Direction: {}", if out { "sent by this PC" } else { "received by this PC" })
        });
    }
    if let Some((name, pid)) = m.app.clone() {
        cx.add("app.name", Val::S(name.clone()), 0, 0, |_| {
            format!("Application: {name} (PID {pid})")
        });
        cx.hidden("app.pid", Val::U(pid as u64), 0, 0);
    }
    cx.close();

    match lt {
        linktype::ETHERNET => ethernet(cx, 0),
        linktype::LINUX_SLL => sll(cx, false),
        linktype::LINUX_SLL2 => sll(cx, true),
        linktype::NULL => null_loop(cx),
        linktype::RAW | linktype::IPV4 | linktype::IPV6 => raw_ip(cx, 0),
        other => {
            cx.text(0, total, || format!("Unsupported link type {other}"));
            cx.info(|| format!("Link type {other}"));
        }
    }
}

fn raw_ip(cx: &mut Cx, off: usize) {
    match Rd::new(cx.data).u8(off).map(|b| b >> 4) {
        Some(4) => ipv4(cx, off),
        Some(6) => ipv6(cx, off),
        _ => data(cx, off),
    }
}

fn ethertype_name(t: u16) -> &'static str {
    match t {
        0x0800 => "IPv4",
        0x86DD => "IPv6",
        0x0806 => "ARP",
        0x8100 => "802.1Q VLAN",
        0x88A8 => "802.1ad",
        0x88CC => "LLDP",
        0x888E => "802.1X (EAPOL)",
        0x8863 | 0x8864 => "PPPoE",
        0x8847 => "MPLS",
        0x88F7 => "PTP",
        0x22F0 => "AVTP",
        _ => "Unknown",
    }
}

fn ethertype_dispatch(cx: &mut Cx, t: u16, off: usize) {
    match t {
        0x0800 => ipv4(cx, off),
        0x86DD => ipv6(cx, off),
        0x0806 => arp(cx, off),
        0x8100 | 0x88A8 => vlan(cx, off),
        _ => {
            cx.info(|| format!("Ethernet type 0x{t:04x} ({})", ethertype_name(t)));
            data(cx, off);
        }
    }
}

fn ethernet(cx: &mut Cx, off: usize) {
    let r = Rd::new(cx.data);
    let (Some(dst), Some(src), Some(t)) = (r.mac(off), r.mac(off + 6), r.u16(off + 12)) else {
        cx.text(off, cx.data.len() - off, || "Ethernet header truncated".into());
        return;
    };
    cx.layer("eth", Some(Proto::Eth), off, 14, || {
        format!("Ethernet II, Src: {}, Dst: {}", mac_str(&src), mac_str(&dst))
    });
    cx.add("eth.dst", Val::Mac(dst), off, 6, |v| format!("Destination: {}{}", v.display(), mac_note(&dst)));
    cx.add("eth.src", Val::Mac(src), off + 6, 6, |v| format!("Source: {}", v.display()));
    cx.hidden("eth.addr", Val::Mac(dst), off, 6);
    cx.hidden("eth.addr", Val::Mac(src), off + 6, 6);
    cx.out.src = mac_str(&src);
    cx.out.dst = mac_str(&dst);
    if t <= 1500 {
        cx.add("eth.len", Val::U(t as u64), off + 12, 2, |v| format!("Length: {}", v.display()));
        cx.close();
        cx.layer("llc", Some(Proto::Llc), off + 14, 3, || "Logical-Link Control".into());
        let (dsap, ssap) = (r.u8(off + 14).unwrap_or(0), r.u8(off + 15).unwrap_or(0));
        cx.text(off + 14, 3, || format!("DSAP 0x{dsap:02x}, SSAP 0x{ssap:02x}"));
        cx.close();
        cx.info(|| {
            if dsap == 0x42 {
                "Spanning Tree Protocol".into()
            } else {
                format!("LLC DSAP 0x{dsap:02x}")
            }
        });
        return;
    }
    cx.add("eth.type", Val::U(t as u64), off + 12, 2, |_| {
        format!("Type: {} (0x{t:04x})", ethertype_name(t))
    });
    cx.close();
    ethertype_dispatch(cx, t, off + 14);
}

fn mac_note(m: &[u8; 6]) -> &'static str {
    if *m == [0xff; 6] {
        " (Broadcast)"
    } else if m[0] & 1 == 1 {
        " (Multicast)"
    } else {
        ""
    }
}

fn vlan(cx: &mut Cx, off: usize) {
    let r = Rd::new(cx.data);
    let (Some(tci), Some(t)) = (r.u16(off), r.u16(off + 2)) else {
        return;
    };
    let id = tci & 0x0fff;
    cx.layer("vlan", None, off, 4, || format!("802.1Q Virtual LAN, PRI: {}, ID: {id}", tci >> 13));
    cx.kv("vlan.priority", "Priority", Val::U((tci >> 13) as u64), off, 2);
    cx.kv("vlan.id", "ID", Val::U(id as u64), off, 2);
    cx.add("vlan.etype", Val::U(t as u64), off + 2, 2, |_| {
        format!("Type: {} (0x{t:04x})", ethertype_name(t))
    });
    cx.close();
    ethertype_dispatch(cx, t, off + 4);
}

fn sll(cx: &mut Cx, v2: bool) {
    let r = Rd::new(cx.data);
    let (len, proto, pkttype, addr_off, halen) = if v2 {
        (20, r.u16(0), r.u8(10), 12, r.u8(11))
    } else {
        (16, r.u16(14), r.u16(0).map(|v| v as u8), 6, r.u16(4).map(|v| v as u8))
    };
    let (Some(proto), Some(pkttype)) = (proto, pkttype) else {
        return;
    };
    let kind = match pkttype {
        0 => "Unicast to us",
        1 => "Broadcast",
        2 => "Multicast",
        3 => "Unicast to another host",
        4 => "Sent by us",
        _ => "Unknown",
    };
    cx.layer("sll", None, 0, len, || {
        format!("Linux cooked capture v{}", if v2 { 2 } else { 1 })
    });
    cx.text(0, len, || format!("Packet type: {kind} ({pkttype})"));
    if let Some(addr) = r.bytes(addr_off, halen.unwrap_or(0).min(8) as usize) {
        if addr.len() == 6 {
            let m: [u8; 6] = addr.try_into().unwrap_or_default();
            cx.add("sll.src.eth", Val::Mac(m), addr_off, 6, |v| format!("Source: {}", v.display()));
            cx.out.src = mac_str(&m);
        }
    }
    cx.add("sll.etype", Val::U(proto as u64), len - 2, 2, |_| {
        format!("Protocol: {} (0x{proto:04x})", ethertype_name(proto))
    });
    cx.close();
    ethertype_dispatch(cx, proto, len);
}

fn null_loop(cx: &mut Cx) {
    let Some(b) = Rd::new(cx.data).bytes(0, 4) else {
        return;
    };
    // Host byte order of the capturing machine: accept either.
    let le = u32::from_le_bytes(b.try_into().unwrap_or_default());
    let fam = if le > 0xffff { u32::from_be_bytes(b.try_into().unwrap_or_default()) } else { le };
    cx.layer("null", None, 0, 4, || format!("Null/Loopback, family {fam}"));
    cx.close();
    match fam {
        2 => ipv4(cx, 4),
        24 | 28 | 30 => ipv6(cx, 4),
        _ => data(cx, 4),
    }
}

// --------------------------------------------------------------- ARP ---

fn arp(cx: &mut Cx, off: usize) {
    let r = Rd::new(cx.data);
    let (Some(op), Some(hlen), Some(plen)) = (r.u16(off + 6), r.u8(off + 4), r.u8(off + 5)) else {
        cx.info(|| "ARP (truncated)".into());
        return;
    };
    if hlen != 6 || plen != 4 {
        cx.layer("arp", Some(Proto::Arp), off, 8, || "Address Resolution Protocol".into());
        cx.close();
        cx.info(|| format!("ARP opcode {op}"));
        return;
    }
    let (sha, spa, tha, tpa) = (
        r.mac(off + 8).unwrap_or_default(),
        r.ip4(off + 14),
        r.mac(off + 18).unwrap_or_default(),
        r.ip4(off + 24),
    );
    let (Some(spa), Some(tpa)) = (spa, tpa) else {
        return;
    };
    let gratuitous = spa == tpa;
    let opname = match op {
        1 => "request",
        2 => "reply",
        3 => "reverse request",
        4 => "reverse reply",
        _ => "unknown",
    };
    cx.layer("arp", Some(Proto::Arp), off, 28, || {
        format!(
            "Address Resolution Protocol ({opname}{})",
            if gratuitous { ", gratuitous" } else { "" }
        )
    });
    cx.kv("arp.hw.type", "Hardware type", Val::U(r.u16(off).unwrap_or(0) as u64), off, 2);
    cx.add("arp.proto.type", Val::U(r.u16(off + 2).unwrap_or(0) as u64), off + 2, 2, |v| {
        format!("Protocol type: IPv4 (0x{:04x})", match v { Val::U(x) => *x, _ => 0 })
    });
    cx.add("arp.opcode", Val::U(op as u64), off + 6, 2, |_| format!("Opcode: {opname} ({op})"));
    if gratuitous {
        cx.hidden("arp.isgratuitous", Val::None, off, 28);
    }
    cx.kv("arp.src.hw_mac", "Sender MAC address", Val::Mac(sha), off + 8, 6);
    cx.kv("arp.src.proto_ipv4", "Sender IP address", Val::Ip(spa), off + 14, 4);
    cx.kv("arp.dst.hw_mac", "Target MAC address", Val::Mac(tha), off + 18, 6);
    cx.kv("arp.dst.proto_ipv4", "Target IP address", Val::Ip(tpa), off + 24, 4);
    cx.close();
    cx.info(|| match op {
        1 if gratuitous => format!("Gratuitous ARP for {spa} (Request)"),
        1 => format!("Who has {tpa}? Tell {spa}"),
        2 => format!("{spa} is at {}", mac_str(&sha)),
        _ => format!("ARP {opname}"),
    });
}

// -------------------------------------------------------------- IPv4 ---

fn ip_proto_name(p: u8) -> &'static str {
    match p {
        0 => "IPv6 Hop-by-Hop",
        1 => "ICMP",
        2 => "IGMP",
        6 => "TCP",
        17 => "UDP",
        41 => "IPv6",
        43 => "IPv6 Routing",
        44 => "IPv6 Fragment",
        47 => "GRE",
        50 => "ESP",
        51 => "AH",
        58 => "ICMPv6",
        59 => "No Next Header",
        60 => "IPv6 Destination Options",
        89 => "OSPF",
        103 => "PIM",
        112 => "VRRP",
        132 => "SCTP",
        _ => "Unknown",
    }
}

fn dscp_name(d: u8) -> &'static str {
    match d {
        0 => "Default",
        46 => "Expedited Forwarding",
        8 => "CS1",
        16 => "CS2",
        24 => "CS3",
        32 => "CS4",
        40 => "CS5",
        48 => "CS6",
        56 => "CS7",
        10 | 12 | 14 => "AF1x",
        18 | 20 | 22 => "AF2x",
        26 | 28 | 30 => "AF3x",
        34 | 36 | 38 => "AF4x",
        _ => "Unknown",
    }
}

fn ipv4(cx: &mut Cx, off: usize) {
    let r = Rd::new(cx.data);
    let Some(vihl) = r.u8(off) else { return };
    let ihl = ((vihl & 0x0f) as usize) * 4;
    let (Some(tos), Some(tlen), Some(id), Some(ff), Some(ttl), Some(p), Some(ck), Some(src), Some(dst)) = (
        r.u8(off + 1),
        r.u16(off + 2),
        r.u16(off + 4),
        r.u16(off + 6),
        r.u8(off + 8),
        r.u8(off + 9),
        r.u16(off + 10),
        r.ip4(off + 12),
        r.ip4(off + 16),
    ) else {
        cx.text(off, cx.data.len().saturating_sub(off), || "IPv4 header truncated".into());
        cx.info(|| "IPv4 (truncated)".into());
        return;
    };
    if ihl < 20 {
        cx.text(off, 1, || format!("Bogus IPv4 header length {ihl}"));
        return;
    }
    let df = ff & 0x4000 != 0;
    let mf = ff & 0x2000 != 0;
    let frag = (ff & 0x1fff) as usize * 8;
    cx.out.facts.src = Some(src);
    cx.out.facts.dst = Some(dst);
    cx.out.src = src.to_string();
    cx.out.dst = dst.to_string();
    cx.layer("ip", Some(Proto::Ipv4), off, ihl, || {
        format!("Internet Protocol Version 4, Src: {src}, Dst: {dst}")
    });
    cx.kv("ip.version", "Version", Val::U(4), off, 1);
    cx.add("ip.hdr_len", Val::U(ihl as u64), off, 1, |_| format!("Header length: {ihl} bytes"));
    cx.add("ip.dsfield.dscp", Val::U((tos >> 2) as u64), off + 1, 1, |_| {
        format!("Differentiated services: DSCP {} ({}), ECN {}", tos >> 2, dscp_name(tos >> 2), tos & 3)
    });
    cx.kv("ip.len", "Total length", Val::U(tlen as u64), off + 2, 2);
    cx.add("ip.id", Val::U(id as u64), off + 4, 2, |_| format!("Identification: 0x{id:04x} ({id})"));
    cx.open(off + 6, 2, || {
        format!(
            "Flags: 0x{:x}{}{}",
            ff >> 13,
            if df { ", Don't fragment" } else { "" },
            if mf { ", More fragments" } else { "" }
        )
    });
    cx.add("ip.flags.df", Val::B(df), off + 6, 1, |_| {
        format!("Don't fragment: {}", if df { "Set" } else { "Not set" })
    });
    cx.add("ip.flags.mf", Val::B(mf), off + 6, 1, |_| {
        format!("More fragments: {}", if mf { "Set" } else { "Not set" })
    });
    cx.close();
    cx.add("ip.frag_offset", Val::U(frag as u64), off + 6, 2, |_| format!("Fragment offset: {frag}"));
    cx.kv("ip.ttl", "Time to live", Val::U(ttl as u64), off + 8, 1);
    cx.add("ip.proto", Val::U(p as u64), off + 9, 1, |_| {
        format!("Protocol: {} ({p})", ip_proto_name(p))
    });
    cx.add("ip.checksum", Val::U(ck as u64), off + 10, 2, |_| format!("Header checksum: 0x{ck:04x}"));
    cx.kv("ip.src", "Source address", Val::Ip(src), off + 12, 4);
    cx.kv("ip.dst", "Destination address", Val::Ip(dst), off + 16, 4);
    cx.hidden("ip.addr", Val::Ip(src), off + 12, 4);
    cx.hidden("ip.addr", Val::Ip(dst), off + 16, 4);
    if ttl <= 1 && !src.is_loopback() {
        cx.expert(1, "", off + 8, 1, || "Note: time to live is 1 (local network only)".into());
    }
    cx.close();

    let end = (off + tlen as usize).min(cx.data.len()).max(off + ihl);
    if frag > 0 {
        cx.info(|| format!("Fragmented IP protocol ({}, offset {frag})", ip_proto_name(p)));
        data_range(cx, off + ihl, end);
        return;
    }
    if mf {
        cx.hidden("ip.fragment", Val::None, off, ihl);
    }
    transport(cx, p, off + ihl, end);
}

// -------------------------------------------------------------- IPv6 ---

fn ipv6(cx: &mut Cx, off: usize) {
    let r = Rd::new(cx.data);
    let (Some(w0), Some(plen), Some(mut nxt), Some(hlim), Some(src), Some(dst)) = (
        r.u32(off),
        r.u16(off + 4),
        r.u8(off + 6),
        r.u8(off + 7),
        r.ip6(off + 8),
        r.ip6(off + 24),
    ) else {
        cx.info(|| "IPv6 (truncated)".into());
        return;
    };
    cx.out.facts.src = Some(src);
    cx.out.facts.dst = Some(dst);
    cx.out.src = src.to_string();
    cx.out.dst = dst.to_string();
    cx.layer("ipv6", Some(Proto::Ipv6), off, 40, || {
        format!("Internet Protocol Version 6, Src: {src}, Dst: {dst}")
    });
    cx.kv("ipv6.version", "Version", Val::U(6), off, 1);
    cx.add("ipv6.tclass", Val::U(((w0 >> 20) & 0xff) as u64), off, 2, |v| {
        format!("Traffic class: 0x{:02x}", match v { Val::U(x) => *x, _ => 0 })
    });
    cx.add("ipv6.flow", Val::U((w0 & 0xfffff) as u64), off + 1, 3, |v| {
        format!("Flow label: 0x{:05x}", match v { Val::U(x) => *x, _ => 0 })
    });
    cx.kv("ipv6.plen", "Payload length", Val::U(plen as u64), off + 4, 2);
    cx.add("ipv6.nxt", Val::U(nxt as u64), off + 6, 1, |_| {
        format!("Next header: {} ({nxt})", ip_proto_name(nxt))
    });
    cx.kv("ipv6.hlim", "Hop limit", Val::U(hlim as u64), off + 7, 1);
    cx.kv("ipv6.src", "Source address", Val::Ip(src), off + 8, 16);
    cx.kv("ipv6.dst", "Destination address", Val::Ip(dst), off + 24, 16);
    cx.hidden("ipv6.addr", Val::Ip(src), off + 8, 16);
    cx.hidden("ipv6.addr", Val::Ip(dst), off + 24, 16);

    let end = (off + 40 + plen as usize).min(cx.data.len());
    let mut p = off + 40;
    let mut fragmented = false;
    // Extension headers.
    for _ in 0..8 {
        match nxt {
            0 | 43 | 60 => {
                let (Some(n), Some(l)) = (r.u8(p), r.u8(p + 1)) else { break };
                let len = (l as usize + 1) * 8;
                cx.text(p, len, || format!("{} header ({len} bytes)", ip_proto_name(nxt)));
                nxt = n;
                p += len;
            }
            44 => {
                let (Some(n), Some(fo)) = (r.u8(p), r.u16(p + 2)) else { break };
                cx.text(p, 8, || format!("Fragment header, offset {}", (fo >> 3) * 8));
                if fo >> 3 != 0 {
                    fragmented = true;
                }
                nxt = n;
                p += 8;
            }
            _ => break,
        }
    }
    cx.close();
    if fragmented {
        cx.info(|| "IPv6 fragment".into());
        data_range(cx, p, end);
        return;
    }
    transport(cx, nxt, p, end.max(p));
}

fn transport(cx: &mut Cx, proto: u8, off: usize, end: usize) {
    match proto {
        6 => tcp(cx, off, end),
        17 => udp(cx, off, end),
        1 => icmp(cx, off, end),
        58 => icmpv6(cx, off, end),
        2 => {
            cx.layer("igmp", Some(Proto::Igmp), off, end - off, || "Internet Group Management Protocol".into());
            let t = Rd::new(cx.data).u8(off).unwrap_or(0);
            let group = Rd::new(cx.data).ip4(off + 4);
            cx.kv("igmp.type", "Type", Val::U(t as u64), off, 1);
            cx.close();
            cx.info(|| match (t, group) {
                (0x11, _) => "Membership Query".into(),
                (0x16, Some(g)) => format!("Membership Report group {g}"),
                (0x17, Some(g)) => format!("Leave Group {g}"),
                (0x22, _) => "Membership Report (v3)".into(),
                _ => format!("IGMP type 0x{t:02x}"),
            });
        }
        p => {
            cx.info(|| format!("{} (IP protocol {p})", ip_proto_name(p)));
            data_range(cx, off, end);
        }
    }
}

// -------------------------------------------------------------- ICMP ---

fn icmp(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let (Some(t), Some(code)) = (r.u8(off), r.u8(off + 1)) else { return };
    let (tname, cname) = icmp_names(t, code);
    cx.layer("icmp", Some(Proto::Icmp), off, end.saturating_sub(off), || {
        "Internet Control Message Protocol".into()
    });
    cx.add("icmp.type", Val::U(t as u64), off, 1, |_| format!("Type: {t} ({tname})"));
    cx.add("icmp.code", Val::U(code as u64), off + 1, 1, |_| {
        if cname.is_empty() { format!("Code: {code}") } else { format!("Code: {code} ({cname})") }
    });
    cx.add("icmp.checksum", Val::U(r.u16(off + 2).unwrap_or(0) as u64), off + 2, 2, |v| {
        format!("Checksum: 0x{:04x}", match v { Val::U(x) => *x, _ => 0 })
    });
    match t {
        0 | 8 => {
            let (id, seq) = (r.u16(off + 4).unwrap_or(0), r.u16(off + 6).unwrap_or(0));
            cx.out.facts.echo = Some((t == 8, id, seq));
            cx.add("icmp.ident", Val::U(id as u64), off + 4, 2, |_| format!("Identifier: 0x{id:04x} ({id})"));
            cx.kv("icmp.seq", "Sequence number", Val::U(seq as u64), off + 6, 2);
            if let Some(rt) = cx.meta.response_time {
                cx.add("icmp.resptime", Val::F(rt), off, 0, |_| format!("[Response time: {:.3} ms]", rt * 1000.0));
            }
            let ttl = r.u8(off.wrapping_sub(12)).unwrap_or(0);
            cx.info(|| {
                format!(
                    "Echo (ping) {} id=0x{id:04x}, seq={seq}, ttl={ttl}",
                    if t == 8 { "request" } else { "reply" }
                )
            });
            if end > off + 8 {
                cx.add("data", Val::Raw, off + 8, end - off - 8, |_| format!("Data ({} bytes)", end - off - 8));
            }
            cx.close();
        }
        3 | 11 | 12 | 5 => {
            cx.out.facts.icmp_error = true;
            cx.expert(2, "", off, 2, || format!("{tname}: {cname}"));
            cx.info(|| if cname.is_empty() { tname.to_string() } else { format!("{tname} ({cname})") });
            cx.close();
            // The quoted original datagram.
            let src = cx.out.src.clone();
            let dst = cx.out.dst.clone();
            let proto = cx.out.proto;
            let facts = cx.out.facts.clone();
            let saved_info = cx.out.info.clone();
            if end > off + 8 {
                cx.open(off + 8, end - off - 8, || "Original datagram (as quoted by the router/host)".into());
                quoted_ip(cx, off + 8, end);
                cx.close();
            }
            cx.out.src = src;
            cx.out.dst = dst;
            cx.out.proto = proto;
            cx.out.facts = facts;
            cx.out.info = saved_info;
        }
        _ => {
            cx.info(|| tname.to_string());
            cx.close();
        }
    }
}

/// Header-only dissection of a quoted IPv4 datagram inside an ICMP error.
fn quoted_ip(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let Some(vihl) = r.u8(off) else { return };
    let ihl = ((vihl & 0x0f) as usize) * 4;
    let (Some(p), Some(src), Some(dst)) = (r.u8(off + 9), r.ip4(off + 12), r.ip4(off + 16)) else {
        return;
    };
    cx.text(off, ihl, || format!("IPv4 {src} → {dst}, protocol {}", ip_proto_name(p)));
    if matches!(p, 6 | 17) {
        if let (Some(sp), Some(dp)) = (r.u16(off + ihl), r.u16(off + ihl + 2)) {
            cx.text(off + ihl, (end - off - ihl).min(8), || {
                format!("{} port {sp} → {dp}", ip_proto_name(p))
            });
        }
    }
}

fn icmp_names(t: u8, c: u8) -> (&'static str, &'static str) {
    match t {
        0 => ("Echo (ping) reply", ""),
        3 => (
            "Destination unreachable",
            match c {
                0 => "Network unreachable",
                1 => "Host unreachable",
                2 => "Protocol unreachable",
                3 => "Port unreachable",
                4 => "Fragmentation needed",
                5 => "Source route failed",
                9 | 10 | 13 => "Administratively prohibited",
                _ => "Unreachable",
            },
        ),
        4 => ("Source quench", ""),
        5 => ("Redirect", ""),
        8 => ("Echo (ping) request", ""),
        9 => ("Router advertisement", ""),
        10 => ("Router solicitation", ""),
        11 => (
            "Time exceeded",
            if c == 0 { "Time to live exceeded in transit" } else { "Fragment reassembly time exceeded" },
        ),
        12 => ("Parameter problem", ""),
        13 => ("Timestamp request", ""),
        14 => ("Timestamp reply", ""),
        _ => ("Unknown ICMP type", ""),
    }
}

fn icmpv6(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let (Some(t), Some(code)) = (r.u8(off), r.u8(off + 1)) else { return };
    let name = match t {
        1 => "Destination unreachable",
        2 => "Packet too big",
        3 => "Time exceeded",
        4 => "Parameter problem",
        128 => "Echo (ping) request",
        129 => "Echo (ping) reply",
        130 => "Multicast listener query",
        131 => "Multicast listener report",
        133 => "Router solicitation",
        134 => "Router advertisement",
        135 => "Neighbor solicitation",
        136 => "Neighbor advertisement",
        137 => "Redirect",
        143 => "Multicast listener report v2",
        _ => "Unknown ICMPv6 type",
    };
    cx.layer("icmpv6", Some(Proto::Icmpv6), off, end.saturating_sub(off), || {
        "Internet Control Message Protocol v6".into()
    });
    cx.add("icmpv6.type", Val::U(t as u64), off, 1, |_| format!("Type: {name} ({t})"));
    cx.kv("icmpv6.code", "Code", Val::U(code as u64), off + 1, 1);
    let mut info = name.to_string();
    match t {
        128 | 129 => {
            let (id, seq) = (r.u16(off + 4).unwrap_or(0), r.u16(off + 6).unwrap_or(0));
            cx.out.facts.echo = Some((t == 128, id, seq));
            cx.add("icmpv6.echo.identifier", Val::U(id as u64), off + 4, 2, |_| format!("Identifier: 0x{id:04x}"));
            cx.kv("icmpv6.echo.sequence_number", "Sequence", Val::U(seq as u64), off + 6, 2);
            info = format!("{name} id=0x{id:04x}, seq={seq}");
        }
        135 | 136 => {
            if let Some(target) = r.ip6(off + 8) {
                cx.kv("icmpv6.nd.ns.target_address", "Target address", Val::Ip(target), off + 8, 16);
                info = if t == 135 {
                    format!("Neighbor Solicitation for {target}")
                } else {
                    format!("Neighbor Advertisement {target}")
                };
            }
        }
        1..=4 => {
            cx.out.facts.icmp_error = true;
            cx.expert(2, "", off, 2, || name.to_string());
        }
        _ => {}
    }
    cx.close();
    cx.info(|| info);
}

// --------------------------------------------------------------- TCP ---

fn flag_names(f: u16) -> String {
    let mut v = Vec::new();
    for (bit, n) in [
        (tcpflag::SYN, "SYN"),
        (tcpflag::FIN, "FIN"),
        (tcpflag::RST, "RST"),
        (tcpflag::PSH, "PSH"),
        (tcpflag::ACK, "ACK"),
        (tcpflag::URG, "URG"),
        (0x40, "ECE"),
        (0x80, "CWR"),
    ] {
        if f & bit != 0 {
            v.push(n);
        }
    }
    if v.is_empty() {
        "<none>".into()
    } else {
        v.join(", ")
    }
}

fn tcp(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let (Some(sp), Some(dp), Some(seq), Some(ack), Some(hf), Some(win), Some(ck), Some(urg)) = (
        r.u16(off),
        r.u16(off + 2),
        r.u32(off + 4),
        r.u32(off + 8),
        r.u16(off + 12),
        r.u16(off + 14),
        r.u16(off + 16),
        r.u16(off + 18),
    ) else {
        cx.layer("tcp", Some(Proto::Tcp), off, end.saturating_sub(off), || "Transmission Control Protocol (truncated)".into());
        cx.close();
        cx.info(|| "TCP (truncated)".into());
        return;
    };
    let hlen = ((hf >> 12) as usize) * 4;
    let flags = hf & 0x01ff;
    let pstart = (off + hlen).min(end);
    let plen = end.saturating_sub(pstart);
    cx.out.facts.tcp = Some(TcpFacts {
        sport: sp,
        dport: dp,
        seq,
        ack,
        flags,
        window: win,
        payload_off: pstart,
        payload_len: plen,
    });
    let (rseq, rack) = match cx.meta.isn {
        Some((a, b)) => (seq.wrapping_sub(a), ack.wrapping_sub(b)),
        None => (seq, ack),
    };
    let rel = cx.meta.isn.is_some();
    let analysis = cx.meta.analysis;
    cx.layer("tcp", Some(Proto::Tcp), off, hlen.min(end - off), || {
        format!(
            "Transmission Control Protocol, Src Port: {sp}, Dst Port: {dp}, Seq: {rseq}{}, Len: {plen}",
            if flags & tcpflag::ACK != 0 { format!(", Ack: {rack}") } else { String::new() }
        )
    });
    cx.kv("tcp.srcport", "Source port", Val::U(sp as u64), off, 2);
    cx.kv("tcp.dstport", "Destination port", Val::U(dp as u64), off + 2, 2);
    cx.hidden("tcp.port", Val::U(sp as u64), off, 2);
    cx.hidden("tcp.port", Val::U(dp as u64), off + 2, 2);
    if let Some(s) = cx.meta.stream {
        cx.add("tcp.stream", Val::U(s as u64), off, 0, |_| format!("[Stream index: {s}]"));
    }
    cx.add("tcp.len", Val::U(plen as u64), off, 0, |_| format!("[TCP segment length: {plen}]"));
    cx.add("tcp.seq", Val::U(rseq as u64), off + 4, 4, |_| {
        format!("Sequence number: {rseq}{}", if rel { "    (relative)" } else { "" })
    });
    cx.add("tcp.seq_raw", Val::U(seq as u64), off + 4, 4, |_| format!("Sequence number (raw): {seq}"));
    let syn_fin = (flags & (tcpflag::SYN | tcpflag::FIN) != 0) as u32;
    let nxt = rseq.wrapping_add(plen as u32).wrapping_add(syn_fin);
    if plen > 0 || syn_fin > 0 {
        cx.add("tcp.nxtseq", Val::U(nxt as u64), off, 0, |_| format!("[Next sequence number: {nxt}]"));
    }
    if flags & tcpflag::ACK != 0 {
        cx.add("tcp.ack", Val::U(rack as u64), off + 8, 4, |_| {
            format!("Acknowledgment number: {rack}{}", if rel { "    (relative)" } else { "" })
        });
        cx.add("tcp.ack_raw", Val::U(ack as u64), off + 8, 4, |_| format!("Acknowledgment number (raw): {ack}"));
    }
    cx.add("tcp.hdr_len", Val::U(hlen as u64), off + 12, 1, |_| format!("Header length: {hlen} bytes"));
    cx.add("tcp.flags", Val::U(flags as u64), off + 12, 2, |_| {
        format!("Flags: 0x{flags:03x} ({})", flag_names(flags))
    });
    cx.open(off + 12, 2, || "Flag bits".into());
    for (bit, field, name) in [
        (tcpflag::URG, "tcp.flags.urg", "Urgent"),
        (tcpflag::ACK, "tcp.flags.ack", "Acknowledgment"),
        (tcpflag::PSH, "tcp.flags.push", "Push"),
        (tcpflag::RST, "tcp.flags.reset", "Reset"),
        (tcpflag::SYN, "tcp.flags.syn", "Syn"),
        (tcpflag::FIN, "tcp.flags.fin", "Fin"),
    ] {
        let set = flags & bit != 0;
        cx.add(field, Val::B(set), off + 13, 1, |_| {
            format!("{name}: {}", if set { "Set" } else { "Not set" })
        });
    }
    cx.close();
    cx.kv("tcp.window_size_value", "Window", Val::U(win as u64), off + 14, 2);
    cx.add("tcp.checksum", Val::U(ck as u64), off + 16, 2, |_| format!("Checksum: 0x{ck:04x}"));
    cx.kv("tcp.urgent_pointer", "Urgent pointer", Val::U(urg as u64), off + 18, 2);

    // Options.
    let mut mss = None;
    let mut ws = None;
    if hlen > 20 {
        let oend = (off + hlen).min(end);
        cx.open(off + 20, oend - off - 20, || format!("Options ({} bytes)", hlen - 20));
        let mut p = off + 20;
        while p < oend {
            let Some(kind) = r.u8(p) else { break };
            match kind {
                0 => {
                    cx.text(p, oend - p, || "End of option list".into());
                    break;
                }
                1 => {
                    p += 1;
                    continue;
                }
                _ => {}
            }
            let Some(len) = r.u8(p + 1).map(|l| l as usize) else { break };
            if len < 2 || p + len > oend {
                break;
            }
            match kind {
                2 if len == 4 => {
                    let v = r.u16(p + 2).unwrap_or(0);
                    mss = Some(v);
                    cx.kv("tcp.options.mss_val", "Maximum segment size", Val::U(v as u64), p, len);
                }
                3 if len == 3 => {
                    let v = r.u8(p + 2).unwrap_or(0);
                    ws = Some(v);
                    cx.add("tcp.options.wscale.shift", Val::U(v as u64), p, len, |_| {
                        format!("Window scale: {v} (multiply by {})", 1u32 << v.min(14))
                    });
                }
                4 => cx.add("tcp.options.sack_perm", Val::None, p, len, |_| "SACK permitted".into()),
                5 => {
                    let blocks = (len - 2) / 8;
                    cx.add("tcp.options.sack", Val::None, p, len, |_| format!("SACK: {blocks} block(s)"));
                }
                8 if len == 10 => {
                    let (tv, te) = (r.u32(p + 2).unwrap_or(0), r.u32(p + 6).unwrap_or(0));
                    cx.add("tcp.options.timestamp.tsval", Val::U(tv as u64), p, len, |_| {
                        format!("Timestamps: TSval {tv}, TSecr {te}")
                    });
                }
                30 => cx.text(p, len, || "Multipath TCP".into()),
                34 => cx.text(p, len, || "TCP Fast Open cookie".into()),
                _ => cx.text(p, len, || format!("Option kind {kind} ({len} bytes)")),
            }
            p += len;
        }
        cx.close();
    }

    // Analysis.
    if analysis != 0 {
        cx.open(off, 0, || "[SEQ/ACK analysis]".into());
        if analysis & an::PROBLEMS != 0 {
            cx.hidden("tcp.analysis.flags", Val::None, off, 0);
        }
        let notes: [(u16, &'static str, u8, &str); 7] = [
            (an::RETRANSMISSION, "tcp.analysis.retransmission", 2, "This frame is a (suspected) retransmission"),
            (an::OUT_OF_ORDER, "tcp.analysis.out_of_order", 2, "This frame is a (suspected) out-of-order segment"),
            (an::LOST_SEGMENT, "tcp.analysis.lost_segment", 2, "Previous segment(s) not captured"),
            (an::DUP_ACK, "tcp.analysis.duplicate_ack", 1, "Duplicate ACK"),
            (an::ZERO_WINDOW, "tcp.analysis.zero_window", 2, "Zero window: the receiver cannot take more data"),
            (an::KEEP_ALIVE, "tcp.analysis.keep_alive", 1, "Keep-alive"),
            (an::PORT_REUSE, "tcp.analysis.reused_ports", 1, "A new connection reuses the ports of an old one"),
        ];
        for (bit, field, lvl, msg) in notes {
            if analysis & bit != 0 {
                cx.expert(lvl, field, off, 0, || msg.to_string());
            }
        }
        cx.close();
    }
    if flags & tcpflag::RST != 0 {
        cx.expert(2, "", off + 13, 1, || "Connection reset (RST)".into());
    }
    cx.close();

    // Info column.
    let tags = analysis_tags(analysis);
    cx.info(|| {
        let mut s = format!("{tags}{sp} → {dp} [{}] Seq={rseq}", flag_names(flags));
        if flags & tcpflag::ACK != 0 {
            s.push_str(&format!(" Ack={rack}"));
        }
        s.push_str(&format!(" Win={win} Len={plen}"));
        if let Some(m) = mss {
            s.push_str(&format!(" MSS={m}"));
        }
        if let Some(w) = ws {
            s.push_str(&format!(" WS={}", 1u32 << w.min(14)));
        }
        s
    });

    if plen > 0 {
        // Filterable (tcp.payload contains ...) but not a separate tree line:
        // the bytes belong to the application layer shown below.
        cx.hidden("tcp.payload", Val::Raw, pstart, plen);
        let src = cx.out.facts.src;
        let dst = cx.out.facts.dst;
        tcp_app(cx, sp, dp, src, dst, pstart, end, &tags);
    }
}

fn analysis_tags(a: u16) -> String {
    let mut s = String::new();
    if a & an::RETRANSMISSION != 0 {
        s.push_str("[Retransmission] ");
    }
    if a & an::OUT_OF_ORDER != 0 {
        s.push_str("[Out-Of-Order] ");
    }
    if a & an::LOST_SEGMENT != 0 {
        s.push_str("[Previous segment not captured] ");
    }
    if a & an::DUP_ACK != 0 {
        s.push_str("[Dup ACK] ");
    }
    if a & an::ZERO_WINDOW != 0 {
        s.push_str("[Zero Window] ");
    }
    if a & an::KEEP_ALIVE != 0 {
        s.push_str("[Keep-Alive] ");
    }
    s
}

const HTTP_PORTS: [u16; 9] = [80, 8080, 8000, 8008, 8081, 8888, 3000, 5000, 591];
const TLS_PORTS: [u16; 10] = [443, 8443, 993, 995, 465, 853, 636, 5061, 990, 4433];
const RTSP_PORTS: [u16; 3] = [554, 8554, 7070];

#[allow(clippy::too_many_arguments)]
fn tcp_app(
    cx: &mut Cx,
    sp: u16,
    dp: u16,
    src: Option<IpAddr>,
    dst: Option<IpAddr>,
    off: usize,
    end: usize,
    tags: &str,
) {
    let payload = &cx.d()[off..end];
    let on = |ports: &[u16]| ports.contains(&sp) || ports.contains(&dp);
    let rtsp_known = on(&RTSP_PORTS)
        || src.is_some_and(|a| cx.shared.rtsp_ports.contains(&(a, sp)))
        || dst.is_some_and(|a| cx.shared.rtsp_ports.contains(&(a, dp)));

    if (media::looks_like_rtsp(payload) || rtsp_known) && media::rtsp(cx, off, end, rtsp_known) {
        prefix_info(cx, tags);
        return;
    }
    if web::looks_like_http(payload) || (on(&HTTP_PORTS) && !web::looks_like_tls(payload)) {
        web::http(cx, off, end, false);
        prefix_info(cx, tags);
        return;
    }
    if web::looks_like_tls(payload) || on(&TLS_PORTS) {
        web::tls(cx, off, end);
        prefix_info(cx, tags);
        return;
    }
    if on(&[53, 853]) && sp != 853 && dp != 853 {
        dns::dns_tcp(cx, off, end);
        prefix_info(cx, tags);
    }
}

fn prefix_info(cx: &mut Cx, tags: &str) {
    if cx.want.info && !tags.is_empty() {
        cx.out.info = format!("{tags}{}", cx.out.info);
    }
}

// --------------------------------------------------------------- UDP ---

fn udp(cx: &mut Cx, off: usize, end: usize) {
    let r = Rd::new(cx.data);
    let (Some(sp), Some(dp), Some(len), Some(ck)) = (r.u16(off), r.u16(off + 2), r.u16(off + 4), r.u16(off + 6))
    else {
        cx.info(|| "UDP (truncated)".into());
        return;
    };
    let pstart = (off + 8).min(end);
    let pend = (off + len as usize).clamp(pstart, end);
    let plen = pend - pstart;
    cx.out.facts.udp = Some((sp, dp, plen, pstart));
    cx.layer("udp", Some(Proto::Udp), off, 8, || {
        format!("User Datagram Protocol, Src Port: {sp}, Dst Port: {dp}")
    });
    cx.kv("udp.srcport", "Source port", Val::U(sp as u64), off, 2);
    cx.kv("udp.dstport", "Destination port", Val::U(dp as u64), off + 2, 2);
    cx.hidden("udp.port", Val::U(sp as u64), off, 2);
    cx.hidden("udp.port", Val::U(dp as u64), off + 2, 2);
    cx.kv("udp.length", "Length", Val::U(len as u64), off + 4, 2);
    cx.add("udp.checksum", Val::U(ck as u64), off + 6, 2, |_| format!("Checksum: 0x{ck:04x}"));
    if let Some(s) = cx.meta.stream {
        cx.add("udp.stream", Val::U(s as u64), off, 0, |_| format!("[Stream index: {s}]"));
    }
    if plen > 0 {
        cx.add("udp.payload", Val::Raw, pstart, plen, |_| format!("UDP payload ({plen} bytes)"));
    }
    cx.close();
    cx.info(|| format!("{sp} → {dp} Len={plen}"));
    if plen == 0 {
        return;
    }

    let on = |p: u16| sp == p || dp == p;
    let (src, dst) = (cx.out.facts.src, cx.out.facts.dst);
    let registered = |ip: Option<IpAddr>, port: u16| ip.and_then(|a| cx.shared.rtp_ports.get(&(a, port)).copied());
    let rtp_kind = registered(dst, dp).or_else(|| registered(src, sp));

    if let Some(is_rtcp) = rtp_kind {
        if is_rtcp {
            media::rtcp(cx, pstart, pend);
        } else {
            media::rtp(cx, pstart, pend);
        }
    } else if on(53) {
        dns::dns(cx, pstart, pend, false);
    } else if on(5353) {
        dns::dns(cx, pstart, pend, true);
    } else if on(5355) {
        dns::dns(cx, pstart, pend, false);
    } else if on(67) || on(68) {
        dns::dhcp(cx, pstart, pend);
    } else if on(123) {
        dns::ntp(cx, pstart, pend);
    } else if on(1900) {
        web::http(cx, pstart, pend, true);
    } else if (on(443) || on(8443) || on(80)) && web::quic(cx, pstart, pend) {
        // QUIC (HTTP/3)
    }
}

// -------------------------------------------------------------- data ---

fn data(cx: &mut Cx, off: usize) {
    let end = cx.data.len();
    data_range(cx, off, end);
}

fn data_range(cx: &mut Cx, off: usize, end: usize) {
    if end > off {
        cx.add("data", Val::Raw, off, end - off, |_| format!("Data ({} bytes)", end - off));
    }
}
