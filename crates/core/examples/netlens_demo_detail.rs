// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/examples/netlens_demo_detail.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Builds a realistic HTTPS Client Hello frame, runs the real NetLens
//           dissector on it and prints the packet detail as JSON. The output is
//           the browser demo's NetLens detail (ui/src/lib/demo/netlens-detail.json).
//             cargo run -p syspulse-core --example netlens_demo_detail > ...json
// =============================================================================

use syspulse_core::netlens::pcapfile::IfaceDesc;
use syspulse_core::netlens::store::Store;

fn ext(t: u16, body: &[u8]) -> Vec<u8> {
    let mut v = t.to_be_bytes().to_vec();
    v.extend_from_slice(&(body.len() as u16).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn client_hello(sni: &str) -> Vec<u8> {
    let name = sni.as_bytes();
    let mut sn = ((name.len() + 3) as u16).to_be_bytes().to_vec();
    sn.push(0);
    sn.extend_from_slice(&(name.len() as u16).to_be_bytes());
    sn.extend_from_slice(name);
    let alpn_list: Vec<u8> = [&b"h2"[..], b"http/1.1"].iter().flat_map(|p| {
        let mut x = vec![p.len() as u8];
        x.extend_from_slice(p);
        x
    }).collect();
    let mut alpn = (alpn_list.len() as u16).to_be_bytes().to_vec();
    alpn.extend_from_slice(&alpn_list);
    let mut exts = Vec::new();
    exts.extend(ext(0, &sn));
    exts.extend(ext(23, &[]));
    exts.extend(ext(0xff01, &[0]));
    exts.extend(ext(10, &[0, 6, 0x00, 0x1d, 0x00, 0x17, 0x00, 0x18]));
    exts.extend(ext(16, &alpn));
    exts.extend(ext(13, &[0, 8, 0x04, 0x03, 0x08, 0x04, 0x04, 0x01, 0x05, 0x03]));
    exts.extend(ext(43, &[4, 0x03, 0x04, 0x03, 0x03]));
    exts.extend(ext(45, &[1, 1]));
    let mut ks = vec![0, 0x26, 0x00, 0x1d, 0x00, 0x20];
    ks.extend((0..32u8).map(|i| i.wrapping_mul(37).wrapping_add(11)));
    exts.extend(ext(51, &ks));
    let mut body = vec![3, 3];
    body.extend((0..32u8).map(|i| i.wrapping_mul(73).wrapping_add(5)));
    body.push(32);
    body.extend((0..32u8).map(|i| i.wrapping_mul(29).wrapping_add(3)));
    let suites: [u16; 8] = [0x1301, 0x1303, 0x1302, 0xc02b, 0xc02f, 0xcca9, 0xcca8, 0xc02c];
    body.extend_from_slice(&((suites.len() * 2) as u16).to_be_bytes());
    for s in suites {
        body.extend_from_slice(&s.to_be_bytes());
    }
    body.extend_from_slice(&[1, 0]);
    body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
    body.extend_from_slice(&exts);
    let mut hs = vec![1];
    hs.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    hs.extend_from_slice(&body);
    let mut rec = vec![22, 3, 1];
    rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
    rec.extend_from_slice(&hs);
    rec
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0x3c, 0x84, 0x6a, 0xaa, 0x10, 0x22, 0xa4, 0x83, 0xe7, 0x12, 0x9c, 0x01, 0x08, 0x00];
    let tot = 20 + 32 + payload.len();
    f.extend_from_slice(&[0x45, 0, (tot >> 8) as u8, tot as u8, 0x5d, 0x1a, 0x40, 0, 64, 6, 0x7b, 0x3e]);
    f.extend_from_slice(&[192, 168, 1, 20]);
    f.extend_from_slice(&[140, 82, 112, 3]);
    f.extend_from_slice(&51844u16.to_be_bytes());
    f.extend_from_slice(&443u16.to_be_bytes());
    f.extend_from_slice(&0x6b8d1f02u32.to_be_bytes());
    f.extend_from_slice(&0x0f3a9c55u32.to_be_bytes());
    f.extend_from_slice(&[0x80, 0x18, 0x01, 0xf6, 0x9a, 0x4c, 0, 0]);
    // NOP NOP Timestamps
    f.extend_from_slice(&[1, 1, 8, 10, 0x2b, 0x3c, 0x91, 0x07, 0x8e, 0x12, 0x44, 0x5a]);
    f.extend_from_slice(payload);
    f
}

fn main() {
    let mut st = Store::new(vec![IfaceDesc { name: "enp3s0".into(), linktype: 1, snaplen: 262144 }], "demo");
    st.local_ips.insert("192.168.1.20".parse().unwrap());
    st.hostnames.insert("140.82.112.3".parse().unwrap(), "github.com".into());
    let t0 = 1_791_455_034_000_000_000u64;
    let syn = |flags: u8, seq: u32, ack: u32| {
        let mut f = frame(&[]);
        f[47] = flags;
        f[38..42].copy_from_slice(&seq.to_be_bytes());
        f[42..46].copy_from_slice(&ack.to_be_bytes());
        f
    };
    st.add(0, t0, 0, Some(true), &syn(0x02, 0x6b8d1f01, 0));
    // Server SYN/ACK: same frame with addresses and ports swapped.
    let mut sa = syn(0x12, 0x0f3a9c54, 0x6b8d1f02);
    let (a, b) = (sa[26..30].to_vec(), sa[30..34].to_vec());
    sa[26..30].copy_from_slice(&b);
    sa[30..34].copy_from_slice(&a);
    let (p1, p2) = (sa[34..36].to_vec(), sa[36..38].to_vec());
    sa[34..36].copy_from_slice(&p2);
    sa[36..38].copy_from_slice(&p1);
    st.add(0, t0 + 11_000_000, 0, Some(false), &sa);
    st.add(0, t0 + 13_000_000, 0, Some(true), &syn(0x10, 0x6b8d1f02, 0x0f3a9c55));
    st.add(0, t0 + 68_500_000, 0, Some(true), &frame(&client_hello("github.com")));
    let d = st.detail(3).expect("detail");
    let mut v = serde_json::to_value(&d).unwrap();
    v["no"] = serde_json::json!(6);
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
}
