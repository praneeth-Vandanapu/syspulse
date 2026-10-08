// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/pcapfile.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Capture file formats shared with Wireshark/tcpdump:
//             * reads classic pcap (µs and ns, both byte orders) and pcapng
//               (SHB/IDB/EPB/SPB/legacy PB, if_name, if_tsresol, if_tsoffset,
//               epb_flags direction);
//             * writes pcapng with nanosecond timestamps, interface names and
//               packet direction, so saved captures open in Wireshark.
// =============================================================================

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct IfaceDesc {
    pub name: String,
    pub linktype: u16,
    pub snaplen: u32,
}

#[derive(Debug, Clone)]
pub struct RawPacket {
    pub ts_ns: u64,
    pub iface: u32,
    pub orig_len: u32,
    /// Some(true) = sent by the capturing host.
    pub outgoing: Option<bool>,
    pub data: Vec<u8>,
}

const MAX_BLOCK: usize = 64 * 1024 * 1024;

fn bad(msg: &str) -> CoreError {
    CoreError::Other(format!("not a valid capture file: {msg}"))
}

enum Format {
    Pcap { swapped: bool, nanos: bool },
    PcapNg { swapped: bool, tsres: Vec<(u64, i64)> },
}

/// Streaming reader for pcap and pcapng files.
pub struct CaptureReader {
    r: BufReader<File>,
    fmt: Format,
    pub interfaces: Vec<IfaceDesc>,
    /// Total file size, for progress.
    pub size: u64,
    pub read: u64,
}

impl CaptureReader {
    pub fn open(path: &Path) -> Result<CaptureReader> {
        let f = File::open(path)?;
        let size = f.metadata().map(|m| m.len()).unwrap_or(0);
        let mut r = BufReader::with_capacity(1 << 20, f);
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic).map_err(|_| bad("file too short"))?;
        let m = u32::from_le_bytes(magic);
        let mut me = CaptureReader {
            r,
            fmt: Format::Pcap { swapped: false, nanos: false },
            interfaces: Vec::new(),
            size,
            read: 4,
        };
        match m {
            0xa1b2c3d4 | 0xd4c3b2a1 | 0xa1b23c4d | 0x4d3cb2a1 => {
                let swapped = m == 0xd4c3b2a1 || m == 0x4d3cb2a1;
                let nanos = m == 0xa1b23c4d || m == 0x4d3cb2a1;
                let mut h = [0u8; 20];
                me.r.read_exact(&mut h).map_err(|_| bad("short header"))?;
                me.read += 20;
                let u32_at = |o: usize| {
                    let b: [u8; 4] = h[o..o + 4].try_into().unwrap_or_default();
                    if swapped { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }
                };
                me.interfaces.push(IfaceDesc {
                    name: String::new(),
                    linktype: (u32_at(16) & 0xffff) as u16,
                    snaplen: u32_at(12),
                });
                me.fmt = Format::Pcap { swapped, nanos };
            }
            0x0A0D0D0A => {
                me.fmt = Format::PcapNg { swapped: false, tsres: Vec::new() };
                // Re-read the SHB from the start of the block.
                me.read_shb_rest()?;
            }
            _ => return Err(bad("unknown format (expected pcap or pcapng)")),
        }
        Ok(me)
    }

    fn read_shb_rest(&mut self) -> Result<()> {
        let mut lenb = [0u8; 4];
        let mut bom = [0u8; 4];
        self.r.read_exact(&mut lenb).map_err(|_| bad("short section header"))?;
        self.r.read_exact(&mut bom).map_err(|_| bad("short section header"))?;
        let swapped = match u32::from_le_bytes(bom) {
            0x1A2B3C4D => false,
            0x4D3C2B1A => true,
            _ => return Err(bad("bad byte-order magic")),
        };
        let len = if swapped { u32::from_be_bytes(lenb) } else { u32::from_le_bytes(lenb) } as usize;
        if !(28..=MAX_BLOCK).contains(&len) {
            return Err(bad("bad section header length"));
        }
        let mut rest = vec![0u8; len - 12];
        self.r.read_exact(&mut rest).map_err(|_| bad("short section header"))?;
        self.read += len as u64 - 4;
        self.interfaces.clear();
        self.fmt = Format::PcapNg { swapped, tsres: Vec::new() };
        Ok(())
    }

    /// Next packet, or None at the end of the file.
    pub fn next_packet(&mut self) -> Result<Option<RawPacket>> {
        match self.fmt {
            Format::Pcap { swapped, nanos } => {
                let mut h = [0u8; 16];
                match self.r.read_exact(&mut h) {
                    Ok(()) => {}
                    Err(_) => return Ok(None),
                }
                let g = |o: usize| {
                    let b: [u8; 4] = h[o..o + 4].try_into().unwrap_or_default();
                    if swapped { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }
                };
                let (sec, frac, incl, orig) = (g(0) as u64, g(4) as u64, g(8) as usize, g(12));
                if incl > MAX_BLOCK {
                    return Err(bad("record too large"));
                }
                let mut data = vec![0u8; incl];
                if self.r.read_exact(&mut data).is_err() {
                    return Ok(None); // truncated last record
                }
                self.read += 16 + incl as u64;
                Ok(Some(RawPacket {
                    ts_ns: sec * 1_000_000_000 + if nanos { frac } else { frac * 1000 },
                    iface: 0,
                    orig_len: orig,
                    outgoing: None,
                    data,
                }))
            }
            Format::PcapNg { .. } => self.next_ng(),
        }
    }

    fn next_ng(&mut self) -> Result<Option<RawPacket>> {
        loop {
            let mut h = [0u8; 8];
            if self.r.read_exact(&mut h).is_err() {
                return Ok(None);
            }
            let btype = u32::from_le_bytes(h[0..4].try_into().unwrap_or_default());
            if btype == 0x0A0D0D0A {
                // New section: the length/BOM follow; reuse the SHB parser.
                let mut bom = [0u8; 4];
                self.r.read_exact(&mut bom).map_err(|_| bad("short section header"))?;
                let swapped = u32::from_le_bytes(bom) == 0x4D3C2B1A;
                let lenb: [u8; 4] = h[4..8].try_into().unwrap_or_default();
                let len = if swapped { u32::from_be_bytes(lenb) } else { u32::from_le_bytes(lenb) } as usize;
                if !(28..=MAX_BLOCK).contains(&len) {
                    return Err(bad("bad section header length"));
                }
                let mut rest = vec![0u8; len - 12];
                self.r.read_exact(&mut rest).map_err(|_| bad("short section header"))?;
                self.read += len as u64;
                self.interfaces.clear();
                self.fmt = Format::PcapNg { swapped, tsres: Vec::new() };
                continue;
            }
            let Format::PcapNg { swapped, .. } = self.fmt else { return Ok(None) };
            let rd32 = |b: &[u8]| {
                let a: [u8; 4] = b[..4].try_into().unwrap_or_default();
                if swapped { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) }
            };
            let rd16 = |b: &[u8]| {
                let a: [u8; 2] = b[..2].try_into().unwrap_or_default();
                if swapped { u16::from_be_bytes(a) } else { u16::from_le_bytes(a) }
            };
            let btype = rd32(&h[0..4]);
            let len = rd32(&h[4..8]) as usize;
            if !(12..=MAX_BLOCK).contains(&len) || !len.is_multiple_of(4) {
                return Err(bad("bad block length"));
            }
            let mut body = vec![0u8; len - 8];
            if self.r.read_exact(&mut body).is_err() {
                return Ok(None);
            }
            self.read += len as u64;
            let body = &body[..len - 12]; // strip trailing length
            match btype {
                1 if body.len() >= 8 => {
                    let linktype = rd16(&body[0..2]);
                    let snaplen = rd32(&body[4..8]);
                    let mut name = String::new();
                    let mut res: u64 = 1_000_000; // ticks per second (default µs)
                    let mut offset: i64 = 0;
                    let mut p = 8;
                    while p + 4 <= body.len() {
                        let code = rd16(&body[p..]);
                        let olen = rd16(&body[p + 2..]) as usize;
                        let v = body.get(p + 4..p + 4 + olen).unwrap_or_default();
                        match code {
                            0 => break,
                            2 => name = String::from_utf8_lossy(v).trim_end_matches('\0').to_string(),
                            9 if !v.is_empty() => {
                                // Ticks per second: 10^e (MSB clear) or 2^e (MSB set).
                                let e = (v[0] & 0x7f) as u32;
                                let tps: u64 = if v[0] & 0x80 == 0 {
                                    10u64.checked_pow(e).unwrap_or(1_000_000_000)
                                } else {
                                    1u64.checked_shl(e).unwrap_or(1_000_000_000)
                                };
                                res = tps;
                            }
                            14 if v.len() >= 8 => {
                                offset = rd32(&v[0..4]) as i64; // low part is enough here
                            }
                            _ => {}
                        }
                        p += 4 + olen.div_ceil(4) * 4;
                    }
                    self.interfaces.push(IfaceDesc { name, linktype, snaplen });
                    if let Format::PcapNg { tsres, .. } = &mut self.fmt {
                        tsres.push((res, offset));
                    }
                }
                6 if body.len() >= 20 => {
                    let iface = rd32(&body[0..4]);
                    let ts = ((rd32(&body[4..8]) as u64) << 32) | rd32(&body[8..12]) as u64;
                    let cap = rd32(&body[12..16]) as usize;
                    let orig = rd32(&body[16..20]);
                    let data = body.get(20..20 + cap).ok_or_else(|| bad("packet beyond block"))?.to_vec();
                    let mut outgoing = None;
                    let mut p = 20 + cap.div_ceil(4) * 4;
                    while p + 4 <= body.len() {
                        let code = rd16(&body[p..]);
                        let olen = rd16(&body[p + 2..]) as usize;
                        if code == 0 {
                            break;
                        }
                        if code == 2 && olen == 4 {
                            match rd32(&body[p + 4..]) & 3 {
                                1 => outgoing = Some(false),
                                2 => outgoing = Some(true),
                                _ => {}
                            }
                        }
                        p += 4 + olen.div_ceil(4) * 4;
                    }
                    let (tps, off) = match &self.fmt {
                        Format::PcapNg { tsres, .. } => tsres.get(iface as usize).copied().unwrap_or((1_000_000, 0)),
                        _ => (1_000_000, 0),
                    };
                    let ts_ns = ticks_to_ns(ts, tps).saturating_add_signed(off * 1_000_000_000);
                    return Ok(Some(RawPacket { ts_ns, iface, orig_len: orig, outgoing, data }));
                }
                3 if body.len() >= 4 => {
                    let orig = rd32(&body[0..4]);
                    let snap = self.interfaces.first().map(|i| i.snaplen).unwrap_or(u32::MAX) as usize;
                    let cap = (orig as usize).min(snap).min(body.len() - 4);
                    return Ok(Some(RawPacket {
                        ts_ns: 0,
                        iface: 0,
                        orig_len: orig,
                        outgoing: None,
                        data: body[4..4 + cap].to_vec(),
                    }));
                }
                2 if body.len() >= 20 => {
                    let iface = rd16(&body[0..2]) as u32;
                    let ts = ((rd32(&body[4..8]) as u64) << 32) | rd32(&body[8..12]) as u64;
                    let cap = rd32(&body[12..16]) as usize;
                    let orig = rd32(&body[16..20]);
                    let data = body.get(20..20 + cap).ok_or_else(|| bad("packet beyond block"))?.to_vec();
                    return Ok(Some(RawPacket { ts_ns: ts * 1000, iface, orig_len: orig, outgoing: None, data }));
                }
                _ => {} // name resolution, statistics, custom blocks
            }
        }
    }
}

fn ticks_to_ns(ticks: u64, tps: u64) -> u64 {
    let tps = tps.max(1);
    let secs = ticks / tps;
    let rem = ticks % tps;
    secs * 1_000_000_000 + (rem as u128 * 1_000_000_000 / tps as u128) as u64
}

// ------------------------------------------------------------ writer ---

fn opt(out: &mut Vec<u8>, code: u16, val: &[u8]) {
    out.extend_from_slice(&code.to_le_bytes());
    out.extend_from_slice(&(val.len() as u16).to_le_bytes());
    out.extend_from_slice(val);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn block(w: &mut impl Write, btype: u32, body: &[u8]) -> std::io::Result<()> {
    let len = (body.len() + 12) as u32;
    w.write_all(&btype.to_le_bytes())?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(body)?;
    w.write_all(&len.to_le_bytes())
}

/// pcapng writer (little endian, nanosecond timestamps).
pub struct PcapNgWriter {
    w: BufWriter<File>,
}

impl PcapNgWriter {
    pub fn create(path: &Path, interfaces: &[IfaceDesc]) -> Result<PcapNgWriter> {
        let mut w = BufWriter::with_capacity(1 << 20, File::create(path)?);
        let mut shb = Vec::new();
        shb.extend_from_slice(&0x1A2B3C4Du32.to_le_bytes());
        shb.extend_from_slice(&1u16.to_le_bytes());
        shb.extend_from_slice(&0u16.to_le_bytes());
        shb.extend_from_slice(&(-1i64).to_le_bytes());
        opt(&mut shb, 4, b"SysPulse NetLens");
        opt(&mut shb, 0, &[]);
        block(&mut w, 0x0A0D0D0A, &shb)?;
        for i in interfaces {
            let mut idb = Vec::new();
            idb.extend_from_slice(&i.linktype.to_le_bytes());
            idb.extend_from_slice(&0u16.to_le_bytes());
            idb.extend_from_slice(&i.snaplen.to_le_bytes());
            if !i.name.is_empty() {
                opt(&mut idb, 2, i.name.as_bytes());
            }
            opt(&mut idb, 9, &[9]); // nanoseconds
            opt(&mut idb, 0, &[]);
            block(&mut w, 1, &idb)?;
        }
        Ok(PcapNgWriter { w })
    }

    pub fn write(&mut self, iface: u32, ts_ns: u64, orig_len: u32, outgoing: Option<bool>, data: &[u8]) -> Result<()> {
        let mut b = Vec::with_capacity(data.len() + 40);
        b.extend_from_slice(&iface.to_le_bytes());
        b.extend_from_slice(&((ts_ns >> 32) as u32).to_le_bytes());
        b.extend_from_slice(&(ts_ns as u32).to_le_bytes());
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&orig_len.to_le_bytes());
        b.extend_from_slice(data);
        while b.len() % 4 != 0 {
            b.push(0);
        }
        if let Some(out) = outgoing {
            opt(&mut b, 2, &(if out { 2u32 } else { 1u32 }).to_le_bytes());
            opt(&mut b, 0, &[]);
        }
        block(&mut self.w, 6, &b)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.w.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcapng_roundtrip() {
        let dir = std::env::temp_dir().join(format!("netlens-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.pcapng");
        let ifs = vec![
            IfaceDesc { name: "eth0".into(), linktype: 1, snaplen: 65535 },
            IfaceDesc { name: "lo".into(), linktype: 1, snaplen: 65535 },
        ];
        let mut w = PcapNgWriter::create(&path, &ifs).unwrap();
        w.write(0, 1_700_000_000_123_456_789, 60, Some(true), &[1, 2, 3]).unwrap();
        w.write(1, 1_700_000_001_000_000_000, 5, None, &[9; 5]).unwrap();
        w.finish().unwrap();

        let mut r = CaptureReader::open(&path).unwrap();
        let a = r.next_packet().unwrap().unwrap();
        assert_eq!(r.interfaces, ifs);
        assert_eq!(a.ts_ns, 1_700_000_000_123_456_789);
        assert_eq!(a.data, vec![1, 2, 3]);
        assert_eq!(a.orig_len, 60);
        assert_eq!(a.outgoing, Some(true));
        let b = r.next_packet().unwrap().unwrap();
        assert_eq!(b.iface, 1);
        assert_eq!(b.data.len(), 5);
        assert!(r.next_packet().unwrap().is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn classic_pcap() {
        let dir = std::env::temp_dir().join(format!("netlens-test2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.pcap");
        let mut b = Vec::new();
        {
            let v = 0xa1b2c3d4u32;
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(&65535u32.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&10u32.to_le_bytes());
        b.extend_from_slice(&5u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&[7, 8]);
        std::fs::write(&path, b).unwrap();
        let mut r = CaptureReader::open(&path).unwrap();
        let p = r.next_packet().unwrap().unwrap();
        assert_eq!(p.ts_ns, 10_000_005_000);
        assert_eq!(p.data, vec![7, 8]);
        assert_eq!(r.interfaces[0].linktype, 1);
        std::fs::remove_dir_all(dir).ok();
    }
}
