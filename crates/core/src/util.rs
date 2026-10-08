// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/util.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Small helpers for reading sysfs/procfs and resolving PCI / USB
//           vendor and device names from the system ID databases.
// =============================================================================

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

/// Read a sysfs/procfs attribute as a trimmed string. Empty values become `None`.
pub fn read_trim(path: impl AsRef<Path>) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    let s = s.trim().trim_matches(char::from(0)).trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Read a sysfs attribute and parse it as a number.
pub fn read_num<T: std::str::FromStr>(path: impl AsRef<Path>) -> Option<T> {
    read_trim(path)?.parse().ok()
}

/// Parse a hexadecimal sysfs ID such as `0x10de` or `10de`.
pub fn read_hex(path: impl AsRef<Path>) -> Option<u16> {
    let s = read_trim(path)?;
    u16::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

/// Final path component of a symlink target (e.g. a device's `driver` link).
pub fn link_name(path: impl AsRef<Path>) -> Option<String> {
    fs::read_link(path)
        .ok()?
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// Vendor/device name database parsed from `pci.ids` or `usb.ids`.
#[derive(Default)]
pub struct IdDatabase {
    vendors: HashMap<u16, String>,
    devices: HashMap<(u16, u16), String>,
}

impl IdDatabase {
    fn load(candidates: &[&str]) -> Self {
        // The ID files are not guaranteed to be valid UTF-8 (usb.ids has Latin-1 bytes).
        let text = candidates
            .iter()
            .find_map(|p| fs::read(p).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let mut db = IdDatabase::default();
        let Some(text) = text else { return db };
        let mut vendor: Option<u16> = None;
        for line in text.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            // Class sections ("C 03 ...") follow the vendor list; stop there.
            if line.starts_with("C ") {
                break;
            }
            if let Some(rest) = line.strip_prefix("\t\t") {
                let _ = rest; // sub-system entries are not needed
                continue;
            }
            if let Some(rest) = line.strip_prefix('\t') {
                if let (Some(v), Some((id, name))) = (vendor, rest.split_once("  ")) {
                    if let Ok(d) = u16::from_str_radix(id.trim(), 16) {
                        db.devices.insert((v, d), name.trim().to_string());
                    }
                }
                continue;
            }
            if let Some((id, name)) = line.split_once("  ") {
                vendor = u16::from_str_radix(id.trim(), 16).ok();
                if let Some(v) = vendor {
                    db.vendors.insert(v, name.trim().to_string());
                }
            } else {
                vendor = None;
            }
        }
        db
    }

    pub fn vendor(&self, v: u16) -> Option<&str> {
        self.vendors.get(&v).map(String::as_str)
    }

    pub fn device(&self, v: u16, d: u16) -> Option<&str> {
        self.devices.get(&(v, d)).map(String::as_str)
    }
}

pub fn pci_ids() -> &'static IdDatabase {
    static DB: OnceLock<IdDatabase> = OnceLock::new();
    DB.get_or_init(|| {
        IdDatabase::load(&[
            "/usr/share/misc/pci.ids",
            "/usr/share/hwdata/pci.ids",
            "/usr/share/pci.ids",
        ])
    })
}

pub fn usb_ids() -> &'static IdDatabase {
    static DB: OnceLock<IdDatabase> = OnceLock::new();
    DB.get_or_init(|| {
        IdDatabase::load(&[
            "/var/lib/usbutils/usb.ids",
            "/usr/share/misc/usb.ids",
            "/usr/share/hwdata/usb.ids",
        ])
    })
}

/// Shorten verbose vendor names ("NVIDIA Corporation" -> "NVIDIA").
pub fn short_vendor(name: &str) -> String {
    let mut s = name.to_string();
    for suffix in [
        " Corporation",
        " Corp.",
        " Inc.",
        " Co., Ltd.",
        ", Inc.",
        " Limited",
        " Ltd.",
    ] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
        }
    }
    s.replace("Advanced Micro Devices, Inc. [AMD/ATI]", "AMD")
        .replace("Advanced Micro Devices, Inc. [AMD]", "AMD")
}

/// Pick the bracketed marketing name if present: "GA106M [GeForce RTX 3060 Mobile]" -> "GeForce RTX 3060 Mobile".
pub fn marketing_name(device: &str) -> String {
    match (device.find('['), device.rfind(']')) {
        (Some(a), Some(b)) if b > a => device[a + 1..b].to_string(),
        _ => device.to_string(),
    }
}

/// Format bytes for inventory text (1024-based).
pub fn human_bytes(n: u64) -> String {
    const U: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

/// Standard base64 (RFC 4648) with padding.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marketing_name_extracts_brackets() {
        assert_eq!(
            marketing_name("GA106M [GeForce RTX 3060 Mobile / Max-Q]"),
            "GeForce RTX 3060 Mobile / Max-Q"
        );
        assert_eq!(marketing_name("Plain"), "Plain");
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn human_bytes_formats() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
    }
}
