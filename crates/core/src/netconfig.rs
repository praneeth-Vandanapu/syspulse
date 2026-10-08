// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netconfig.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Network configuration through NetworkManager (`nmcli`, the same
//           backend Ubuntu Settings uses): adapters with live addressing,
//           saved connection profiles, IPv4/IPv6 editing (DHCP or static,
//           multiple addresses, gateway, DNS, MTU), activation and Wi-Fi
//           scanning. NetworkManager's own polkit rules decide whether a
//           password is needed (Ubuntu allows admins in an active session).
// =============================================================================

use std::collections::HashMap;
use std::net::IpAddr;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::util::{link_name, read_num, read_trim};

// ------------------------------------------------------------ nmcli io ---

/// Split one line of `nmcli -t` output on unescaped ':' and unescape fields.
pub fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.last_mut().unwrap().push(n);
                }
            }
            ':' => out.push(String::new()),
            c => out.last_mut().unwrap().push(c),
        }
    }
    out
}

/// "key:value" lines (value may contain escaped colons) -> ordered pairs.
fn key_values(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.to_string(), v.replace("\\:", ":").replace("\\\\", "\\")))
        })
        .collect()
}

fn nmcli(args: &[&str]) -> Result<String> {
    let out = Command::new("nmcli")
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| CoreError::Other(format!("nmcli not available: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.trim().trim_start_matches("Error: ").to_string();
        if msg.contains("Insufficient privileges") || msg.contains("not authorized") {
            Err(CoreError::PermissionDenied)
        } else {
            Err(CoreError::Other(if msg.is_empty() {
                "nmcli failed".into()
            } else {
                msg
            }))
        }
    }
}

// --------------------------------------------------------------- models ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiLink {
    pub ssid: String,
    pub signal: Option<u8>,
    pub bitrate: Option<String>,
    pub frequency_mhz: Option<u32>,
    pub security: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetDevice {
    pub device: String,
    /// ethernet | wifi | bridge | loopback | ...
    pub kind: String,
    pub state: String,
    pub connection: Option<String>,
    pub connection_uuid: Option<String>,
    pub mac: Option<String>,
    pub mtu: Option<u32>,
    pub ipv4: Vec<String>,
    pub ipv4_gateway: Option<String>,
    pub ipv4_dns: Vec<String>,
    pub ipv6: Vec<String>,
    pub ipv6_gateway: Option<String>,
    pub ipv6_dns: Vec<String>,
    pub routes: Vec<String>,
    pub carrier: Option<bool>,
    pub speed_mbps: Option<u32>,
    pub driver: Option<String>,
    pub wifi: Option<WifiLink>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetConnection {
    pub name: String,
    pub uuid: String,
    /// 802-3-ethernet | 802-11-wireless | bridge | vpn | ...
    pub kind: String,
    pub device: Option<String>,
    pub active: bool,
    pub autoconnect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpSettings {
    /// v4: auto | manual | link-local | shared | disabled
    /// v6: auto | dhcp | manual | link-local | ignore | disabled
    pub method: String,
    /// CIDR notation, e.g. "192.168.1.50/24".
    pub addresses: Vec<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    /// Use only the DNS servers above, ignoring the ones from DHCP.
    pub ignore_auto_dns: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSettings {
    pub uuid: String,
    pub name: String,
    pub kind: String,
    pub interface_name: Option<String>,
    pub autoconnect: bool,
    /// None = automatic.
    pub mtu: Option<u32>,
    pub ssid: Option<String>,
    pub ipv4: IpSettings,
    pub ipv6: IpSettings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    pub in_use: bool,
    pub ssid: String,
    pub bssid: String,
    pub signal: u8,
    pub channel: u32,
    pub frequency_mhz: u32,
    /// "2.4 GHz" | "5 GHz" | "6 GHz"
    pub band: String,
    pub rate: String,
    pub security: String,
    /// A saved profile exists for this SSID.
    pub known_uuid: Option<String>,
}

// ---------------------------------------------------------------- read ---

pub fn devices() -> Result<Vec<NetDevice>> {
    let list = nmcli(&[
        "-t",
        "-f",
        "DEVICE,TYPE,STATE,CONNECTION",
        "device",
        "status",
    ])?;
    let uuids: HashMap<String, String> =
        nmcli(&["-t", "-f", "DEVICE,UUID", "connection", "show", "--active"])
            .unwrap_or_default()
            .lines()
            .map(split_terse)
            .filter(|f| f.len() >= 2 && !f[0].is_empty())
            .map(|f| (f[0].clone(), f[1].clone()))
            .collect();
    let wifi_active: HashMap<String, WifiNetwork> = wifi_list(false)
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.in_use)
        .map(|w| (w.ssid.clone(), w))
        .collect();

    let mut out = Vec::new();
    for f in list.lines().map(split_terse) {
        if f.len() < 4 || matches!(f[1].as_str(), "loopback" | "wifi-p2p") {
            continue;
        }
        let dev = f[0].clone();
        let detail = nmcli(&["-t", "device", "show", &dev]).unwrap_or_default();
        let kv = key_values(&detail);
        let get = |k: &str| {
            kv.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_empty() && v != "--")
        };
        let all = |prefix: &str| -> Vec<String> {
            kv.iter()
                .filter(|(k, v)| k.starts_with(prefix) && !v.is_empty() && v != "--")
                .map(|(_, v)| v.clone())
                .collect()
        };
        let sys = format!("/sys/class/net/{dev}");
        let connection = (!f[3].is_empty()).then(|| f[3].clone());
        let wifi = if f[1] == "wifi" {
            connection
                .as_ref()
                .and_then(|c| wifi_active.get(c))
                .map(|w| WifiLink {
                    ssid: w.ssid.clone(),
                    signal: Some(w.signal),
                    bitrate: Some(w.rate.clone()),
                    frequency_mhz: Some(w.frequency_mhz),
                    security: Some(w.security.clone()),
                })
        } else {
            None
        };
        out.push(NetDevice {
            connection_uuid: uuids.get(&dev).cloned(),
            kind: f[1].clone(),
            state: f[2].clone(),
            connection,
            mac: get("GENERAL.HWADDR"),
            mtu: get("GENERAL.MTU").and_then(|m| m.parse().ok()),
            ipv4: all("IP4.ADDRESS"),
            ipv4_gateway: get("IP4.GATEWAY"),
            ipv4_dns: all("IP4.DNS"),
            ipv6: all("IP6.ADDRESS"),
            ipv6_gateway: get("IP6.GATEWAY"),
            ipv6_dns: all("IP6.DNS"),
            routes: all("IP4.ROUTE"),
            carrier: get("WIRED-PROPERTIES.CARRIER").map(|c| c == "on"),
            speed_mbps: read_num::<i64>(format!("{sys}/speed"))
                .filter(|s| *s > 0)
                .map(|s| s as u32),
            driver: link_name(format!("{sys}/device/driver")),
            wifi,
            device: dev,
        });
    }
    Ok(out)
}

pub fn connections() -> Result<Vec<NetConnection>> {
    let text = nmcli(&[
        "-t",
        "-f",
        "NAME,UUID,TYPE,DEVICE,ACTIVE,AUTOCONNECT",
        "connection",
        "show",
    ])?;
    Ok(text
        .lines()
        .map(split_terse)
        .filter(|f| f.len() >= 6)
        .map(|f| NetConnection {
            name: f[0].clone(),
            uuid: f[1].clone(),
            kind: f[2].clone(),
            device: (!f[3].is_empty()).then(|| f[3].clone()),
            active: f[4] == "yes",
            autoconnect: f[5] == "yes",
        })
        .collect())
}

fn valid_uuid(u: &str) -> bool {
    u.len() == 36 && u.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn list_field(v: Option<&String>) -> Vec<String> {
    v.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

pub fn connection_settings(uuid: &str) -> Result<ConnectionSettings> {
    if !valid_uuid(uuid) {
        return Err(CoreError::Refused("invalid connection id".into()));
    }
    let fields = "connection.id,connection.uuid,connection.type,connection.interface-name,connection.autoconnect,\
ipv4.method,ipv4.addresses,ipv4.gateway,ipv4.dns,ipv4.ignore-auto-dns,\
ipv6.method,ipv6.addresses,ipv6.gateway,ipv6.dns,ipv6.ignore-auto-dns,\
802-3-ethernet.mtu,802-11-wireless.mtu,802-11-wireless.ssid";
    let text = nmcli(&["-t", "-f", fields, "connection", "show", uuid])?;
    let m: HashMap<String, String> = key_values(&text).into_iter().collect();
    let opt = |k: &str| m.get(k).filter(|v| !v.is_empty() && *v != "--").cloned();
    let mtu = opt("802-3-ethernet.mtu")
        .or_else(|| opt("802-11-wireless.mtu"))
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0);
    let ip = |p: &str| IpSettings {
        method: opt(&format!("{p}.method")).unwrap_or_else(|| "auto".into()),
        addresses: list_field(m.get(&format!("{p}.addresses"))),
        gateway: opt(&format!("{p}.gateway")),
        dns: list_field(m.get(&format!("{p}.dns"))),
        ignore_auto_dns: opt(&format!("{p}.ignore-auto-dns")).as_deref() == Some("yes"),
    };
    Ok(ConnectionSettings {
        uuid: uuid.to_string(),
        name: opt("connection.id").unwrap_or_default(),
        kind: opt("connection.type").unwrap_or_default(),
        interface_name: opt("connection.interface-name"),
        autoconnect: opt("connection.autoconnect").as_deref() != Some("no"),
        mtu,
        ssid: opt("802-11-wireless.ssid"),
        ipv4: ip("ipv4"),
        ipv6: ip("ipv6"),
    })
}

/// Nearby Wi-Fi networks (strongest first). `rescan` triggers a fresh scan.
pub fn wifi_list(rescan: bool) -> Result<Vec<WifiNetwork>> {
    let known: HashMap<String, String> =
        nmcli(&["-t", "-f", "NAME,UUID,TYPE", "connection", "show"])
            .unwrap_or_default()
            .lines()
            .map(split_terse)
            .filter(|f| f.len() >= 3 && f[2] == "802-11-wireless")
            .map(|f| (f[0].clone(), f[1].clone()))
            .collect();
    let text = nmcli(&[
        "-t",
        "-f",
        "IN-USE,SSID,BSSID,SIGNAL,CHAN,FREQ,RATE,SECURITY",
        "device",
        "wifi",
        "list",
        "--rescan",
        if rescan { "yes" } else { "no" },
    ])?;
    let mut v: Vec<WifiNetwork> = text
        .lines()
        .map(split_terse)
        .filter(|f| f.len() >= 8)
        .map(|f| {
            let freq: u32 = f[5]
                .split_whitespace()
                .next()
                .and_then(|x| x.parse().ok())
                .unwrap_or(0);
            WifiNetwork {
                in_use: f[0] == "*",
                known_uuid: known.get(&f[1]).cloned(),
                ssid: f[1].clone(),
                bssid: f[2].clone(),
                signal: f[3].parse().unwrap_or(0),
                channel: f[4].parse().unwrap_or(0),
                frequency_mhz: freq,
                band: match freq {
                    f if f >= 5925 => "6 GHz",
                    f if f >= 4900 => "5 GHz",
                    _ => "2.4 GHz",
                }
                .into(),
                rate: f[6].clone(),
                security: if f[7].is_empty() {
                    "Open".into()
                } else {
                    f[7].clone()
                },
            }
        })
        .collect();
    v.sort_by_key(|w| std::cmp::Reverse((w.in_use, w.signal)));
    Ok(v)
}

// --------------------------------------------------------------- write ---

/// Validate "address/prefix" for the given family.
pub fn valid_cidr(s: &str, v6: bool) -> bool {
    let Some((ip, prefix)) = s.split_once('/') else {
        return false;
    };
    let Ok(ip) = ip.parse::<IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    match ip {
        IpAddr::V4(a) => {
            !v6 && (1..=32).contains(&prefix)
                && !a.is_unspecified()
                && !a.is_multicast()
                && !a.is_broadcast()
        }
        IpAddr::V6(a) => {
            v6 && (1..=128).contains(&prefix) && !a.is_unspecified() && !a.is_multicast()
        }
    }
}

fn valid_ip(s: &str, v6: bool) -> bool {
    matches!(s.parse::<IpAddr>(), Ok(IpAddr::V4(_)) if !v6)
        || matches!(s.parse::<IpAddr>(), Ok(IpAddr::V6(_)) if v6)
}

fn check_ip(settings: &IpSettings, v6: bool) -> Result<()> {
    let fam = if v6 { "IPv6" } else { "IPv4" };
    let methods: &[&str] = if v6 {
        &["auto", "dhcp", "manual", "link-local", "ignore", "disabled"]
    } else {
        &["auto", "manual", "link-local", "shared", "disabled"]
    };
    if !methods.contains(&settings.method.as_str()) {
        return Err(CoreError::Refused(format!(
            "{fam}: unknown method {}",
            settings.method
        )));
    }
    if let Some(bad) = settings.addresses.iter().find(|a| !valid_cidr(a, v6)) {
        return Err(CoreError::Refused(format!(
            "{fam}: \"{bad}\" is not a valid address/prefix"
        )));
    }
    if settings.method == "manual" && settings.addresses.is_empty() {
        return Err(CoreError::Refused(format!(
            "{fam}: a static configuration needs at least one address"
        )));
    }
    if let Some(g) = settings.gateway.as_deref().filter(|g| !g.is_empty()) {
        if !valid_ip(g, v6) {
            return Err(CoreError::Refused(format!(
                "{fam}: gateway \"{g}\" is not a valid address"
            )));
        }
    }
    if let Some(bad) = settings.dns.iter().find(|d| !valid_ip(d, v6)) {
        return Err(CoreError::Refused(format!(
            "{fam}: DNS server \"{bad}\" is not a valid address"
        )));
    }
    Ok(())
}

/// Save edited settings to the profile; `apply_now` re-activates it so the
/// change takes effect immediately (the link drops for a moment).
pub fn update_connection(s: &ConnectionSettings, apply_now: bool) -> Result<String> {
    if !valid_uuid(&s.uuid) {
        return Err(CoreError::Refused("invalid connection id".into()));
    }
    check_ip(&s.ipv4, false)?;
    check_ip(&s.ipv6, true)?;
    if let Some(m) = s.mtu {
        if !(576..=9216).contains(&m) {
            return Err(CoreError::Refused(
                "MTU must be between 576 and 9216".into(),
            ));
        }
    }
    let current = connection_settings(&s.uuid)?;
    let mut args: Vec<String> = vec!["connection".into(), "modify".into(), s.uuid.clone()];
    let mut push = |k: &str, v: String| {
        args.push(k.to_string());
        args.push(v);
    };
    for (p, ip) in [("ipv4", &s.ipv4), ("ipv6", &s.ipv6)] {
        // Order matters: addresses must be set before switching to manual.
        let static_like = ip.method == "manual";
        push(
            &format!("{p}.addresses"),
            if static_like || !ip.addresses.is_empty() {
                ip.addresses.join(",")
            } else {
                String::new()
            },
        );
        push(
            &format!("{p}.gateway"),
            ip.gateway
                .clone()
                .filter(|_| static_like)
                .unwrap_or_default(),
        );
        push(&format!("{p}.method"), ip.method.clone());
        push(&format!("{p}.dns"), ip.dns.join(","));
        push(
            &format!("{p}.ignore-auto-dns"),
            if ip.ignore_auto_dns {
                "yes".into()
            } else {
                "no".into()
            },
        );
    }
    push(
        "connection.autoconnect",
        if s.autoconnect {
            "yes".into()
        } else {
            "no".into()
        },
    );
    let mtu_key = match current.kind.as_str() {
        "802-3-ethernet" => Some("802-3-ethernet.mtu"),
        "802-11-wireless" => Some("802-11-wireless.mtu"),
        _ => None,
    };
    if let Some(k) = mtu_key {
        push(
            k,
            s.mtu
                .map(|m| m.to_string())
                .unwrap_or_else(|| "auto".into()),
        );
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    nmcli(&refs)?;
    if apply_now {
        nmcli(&["connection", "up", &s.uuid])?;
        Ok(format!("Saved and applied \"{}\"", current.name))
    } else {
        Ok(format!(
            "Saved \"{}\" (applies on next connect)",
            current.name
        ))
    }
}

pub fn activate(uuid: &str) -> Result<()> {
    if !valid_uuid(uuid) {
        return Err(CoreError::Refused("invalid connection id".into()));
    }
    nmcli(&["connection", "up", uuid]).map(|_| ())
}

pub fn deactivate(uuid: &str) -> Result<()> {
    if !valid_uuid(uuid) {
        return Err(CoreError::Refused("invalid connection id".into()));
    }
    nmcli(&["connection", "down", uuid]).map(|_| ())
}

pub fn wifi_radio(on: bool) -> Result<()> {
    nmcli(&["radio", "wifi", if on { "on" } else { "off" }]).map(|_| ())
}

pub fn wifi_radio_enabled() -> bool {
    nmcli(&["radio", "wifi"])
        .map(|s| s.trim() == "enabled")
        .unwrap_or(false)
}

/// This machine's hostname (shown to speed-test peers).
pub fn hostname() -> String {
    read_trim("/proc/sys/kernel/hostname").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terse_split() {
        assert_eq!(
            split_terse(r"*:BRIT:34\:0A\:33:85"),
            vec!["*", "BRIT", "34:0A:33", "85"]
        );
        assert_eq!(split_terse("a::b"), vec!["a", "", "b"]);
        assert_eq!(split_terse(r"x\\y:z"), vec![r"x\y", "z"]);
    }

    #[test]
    fn cidr_validation() {
        assert!(valid_cidr("192.168.1.50/24", false));
        assert!(!valid_cidr("192.168.1.50", false));
        assert!(!valid_cidr("192.168.1.300/24", false));
        assert!(!valid_cidr("10.0.0.1/33", false));
        assert!(!valid_cidr("0.0.0.0/8", false));
        assert!(valid_cidr("fd00::5/64", true));
        assert!(!valid_cidr("fd00::5/64", false));
    }

    #[test]
    fn settings_validation() {
        let mut ip = IpSettings {
            method: "manual".into(),
            addresses: vec![],
            gateway: None,
            dns: vec![],
            ignore_auto_dns: false,
        };
        assert!(check_ip(&ip, false).is_err(), "manual needs an address");
        ip.addresses = vec!["10.1.0.50/24".into()];
        ip.gateway = Some("10.1.0.1".into());
        ip.dns = vec!["1.1.1.1".into()];
        assert!(check_ip(&ip, false).is_ok());
        ip.dns = vec!["1.1.1.1; rm -rf /".into()];
        assert!(check_ip(&ip, false).is_err());
        ip.method = "evil".into();
        assert!(check_ip(&ip, false).is_err());
    }

    #[test]
    fn reads_devices() {
        if let Ok(d) = devices() {
            assert!(d.iter().all(|x| x.kind != "loopback"));
        }
    }
}
