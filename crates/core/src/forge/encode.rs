// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/forge/encode.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: PacketForge payload encoding. Turns what the user typed into the
//           exact bytes to send:
//             * Text  with standard C escapes: \r \n \t \0 \\ \xHH (and $HH in
//               Hercules-compatible mode)
//             * Hex   "48 65 6C", "0x48,0x65", "48-65", "48656C"
//           then appends a line ending and an optional checksum
//           (CRC-16/MODBUS, CRC-16/CCITT-FALSE, CRC-32, XOR-8, SUM-8, LRC).
// =============================================================================

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum InputMode {
    #[default]
    Text,
    Hex,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    #[default]
    None,
    Cr,
    Lf,
    Crlf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Checksum {
    #[default]
    None,
    /// CRC-16/MODBUS, appended little-endian (as Modbus RTU sends it).
    Crc16Modbus,
    /// CRC-16/CCITT-FALSE, appended big-endian.
    Crc16Ccitt,
    /// CRC-32 (IEEE 802.3), appended big-endian.
    Crc32,
    Xor8,
    Sum8,
    /// Modbus-ASCII style LRC (two's complement of the byte sum).
    Lrc,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SendSpec {
    pub mode: InputMode,
    pub data: String,
    #[serde(default)]
    pub line_ending: LineEnding,
    #[serde(default)]
    pub checksum: Checksum,
    /// Accept Hercules-style `$HH` escapes in text mode.
    #[serde(default)]
    pub hercules: bool,
}

pub const MAX_PAYLOAD: usize = 1 << 20;

fn hex_val(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// Parse hex input. Accepts separators (space, comma, colon, dash), optional
/// 0x prefixes and continuous runs ("48656C"). Errors point at the bad token.
pub fn parse_hex(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for raw in s.split(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '-' | ';')) {
        if raw.is_empty() {
            continue;
        }
        let tok = raw
            .strip_prefix("0x")
            .or_else(|| raw.strip_prefix("0X"))
            .unwrap_or(raw);
        if tok.is_empty() || tok.len() % 2 != 0 || !tok.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CoreError::Refused(format!(
                "\"{raw}\" is not valid hex (use pairs like 0D 0A or 0x0D)"
            )));
        }
        out.extend(
            tok.as_bytes()
                .chunks(2)
                .map(|p| hex_val(p[0]).unwrap() << 4 | hex_val(p[1]).unwrap()),
        );
    }
    Ok(out)
}

/// Text with C escapes (and `$HH` when `hercules`).
pub fn parse_text(s: &str, hercules: bool) -> Result<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() => {
                i += 1;
                match b[i] {
                    b'r' => out.push(b'\r'),
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'0' => out.push(0),
                    b'e' => out.push(0x1b),
                    b'\\' => out.push(b'\\'),
                    b'x' | b'X' => {
                        let (h, l) = (
                            b.get(i + 1).copied().and_then(hex_val),
                            b.get(i + 2).copied().and_then(hex_val),
                        );
                        match (h, l) {
                            (Some(h), Some(l)) => {
                                out.push(h << 4 | l);
                                i += 2;
                            }
                            _ => {
                                return Err(CoreError::Refused(
                                    "\\x must be followed by two hex digits, e.g. \\x1B".into(),
                                ))
                            }
                        }
                    }
                    other => {
                        return Err(CoreError::Refused(format!(
                            "unknown escape \\{} (use \\r \\n \\t \\0 \\e \\\\ or \\xHH)",
                            other as char
                        )))
                    }
                }
            }
            b'$' if hercules => {
                let (h, l) = (
                    b.get(i + 1).copied().and_then(hex_val),
                    b.get(i + 2).copied().and_then(hex_val),
                );
                match (h, l) {
                    (Some(h), Some(l)) => {
                        out.push(h << 4 | l);
                        i += 2;
                    }
                    // "$$" is a literal dollar in Hercules syntax.
                    _ if b.get(i + 1) == Some(&b'$') => {
                        out.push(b'$');
                        i += 1;
                    }
                    _ => out.push(b'$'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    Ok(out)
}

pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Bytes to append for the given checksum.
pub fn checksum_bytes(kind: Checksum, data: &[u8]) -> Vec<u8> {
    match kind {
        Checksum::None => Vec::new(),
        Checksum::Crc16Modbus => crc16_modbus(data).to_le_bytes().to_vec(),
        Checksum::Crc16Ccitt => crc16_ccitt(data).to_be_bytes().to_vec(),
        Checksum::Crc32 => crc32(data).to_be_bytes().to_vec(),
        Checksum::Xor8 => vec![data.iter().fold(0u8, |a, b| a ^ b)],
        Checksum::Sum8 => vec![data.iter().fold(0u8, |a, b| a.wrapping_add(*b))],
        Checksum::Lrc => vec![data
            .iter()
            .fold(0u8, |a, b| a.wrapping_add(*b))
            .wrapping_neg()],
    }
}

/// Full pipeline: parse, then line ending, then checksum (over everything before it).
pub fn encode(spec: &SendSpec) -> Result<Vec<u8>> {
    let mut out = match spec.mode {
        InputMode::Text => parse_text(&spec.data, spec.hercules)?,
        InputMode::Hex => parse_hex(&spec.data)?,
    };
    match spec.line_ending {
        LineEnding::None => {}
        LineEnding::Cr => out.push(b'\r'),
        LineEnding::Lf => out.push(b'\n'),
        LineEnding::Crlf => out.extend_from_slice(b"\r\n"),
    }
    let cs = checksum_bytes(spec.checksum, &out);
    out.extend(cs);
    if out.len() > MAX_PAYLOAD {
        return Err(CoreError::Refused("payload is larger than 1 MiB".into()));
    }
    Ok(out)
}

/// "48 65 6C" representation used in previews and logs.
pub fn to_hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 3);
    for (i, b) in data.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02X}"));
    }
    s
}

/// Printable text with C escapes for everything else (`\r`, `\n`, `\x1B`).
pub fn to_escaped(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len());
    for &b in data {
        match b {
            b'\r' => s.push_str("\\r"),
            b'\n' => s.push_str("\\n"),
            b'\t' => s.push_str("\\t"),
            b'\\' => s.push_str("\\\\"),
            0x20..=0x7e => s.push(b as char),
            _ => s.push_str(&format!("\\x{b:02X}")),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_formats() {
        assert_eq!(parse_hex("48 65 6c").unwrap(), b"Hel");
        assert_eq!(parse_hex("0x48,0x65").unwrap(), b"He");
        assert_eq!(parse_hex("48-65:6C;6C").unwrap(), b"Hell");
        assert_eq!(parse_hex("48656C6C6F").unwrap(), b"Hello");
        assert!(parse_hex("4").is_err());
        assert!(parse_hex("GG").is_err());
        assert_eq!(parse_hex("").unwrap(), b"");
    }

    #[test]
    fn text_escapes() {
        assert_eq!(parse_text(r"AT\r\n", false).unwrap(), b"AT\r\n");
        assert_eq!(parse_text(r"\x1B[0m\0", false).unwrap(), b"\x1b[0m\x00");
        assert_eq!(parse_text(r"a\\b", false).unwrap(), b"a\\b");
        assert!(parse_text(r"\q", false).is_err());
        assert!(parse_text(r"\x1", false).is_err());
        // Hercules mode
        assert_eq!(parse_text("AT$0D$0A", true).unwrap(), b"AT\r\n");
        assert_eq!(parse_text("cost $$5", true).unwrap(), b"cost $5");
        assert_eq!(
            parse_text("AT$0D", false).unwrap(),
            b"AT$0D",
            "plain mode keeps $"
        );
    }

    #[test]
    fn checksums_match_reference_values() {
        // Standard "123456789" check values.
        let d = b"123456789";
        assert_eq!(crc16_modbus(d), 0x4B37);
        assert_eq!(crc16_ccitt(d), 0x29B1);
        assert_eq!(crc32(d), 0xCBF4_3926);
        // Modbus RTU frame: read holding registers 01 03 00 00 00 0A -> CRC C5 CD
        assert_eq!(
            checksum_bytes(Checksum::Crc16Modbus, &[1, 3, 0, 0, 0, 10]),
            vec![0xC5, 0xCD]
        );
        assert_eq!(
            checksum_bytes(Checksum::Lrc, &[1, 3, 0, 0, 0, 10]),
            vec![0xF2]
        );
        assert_eq!(checksum_bytes(Checksum::Xor8, &[0x0F, 0xF0]), vec![0xFF]);
        assert_eq!(checksum_bytes(Checksum::Sum8, &[0xFF, 0x02]), vec![0x01]);
    }

    #[test]
    fn pipeline_and_rendering() {
        let s = SendSpec {
            mode: InputMode::Hex,
            data: "01 03 00 00 00 0A".into(),
            line_ending: LineEnding::None,
            checksum: Checksum::Crc16Modbus,
            hercules: false,
        };
        assert_eq!(to_hex(&encode(&s).unwrap()), "01 03 00 00 00 0A C5 CD");
        let t = SendSpec {
            mode: InputMode::Text,
            data: "AT".into(),
            line_ending: LineEnding::Crlf,
            ..Default::default()
        };
        assert_eq!(encode(&t).unwrap(), b"AT\r\n");
        assert_eq!(to_escaped(b"OK\r\n\x1b\\"), r"OK\r\n\x1B\\");
    }
}
