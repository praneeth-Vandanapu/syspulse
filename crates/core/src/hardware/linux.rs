// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/hardware/linux.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Linux hardware inventory from /sys and /proc. Everything here is
//           readable by an unprivileged user; root-only data (DIMM details,
//           serial numbers) is left for the privileged helper (Phase 2).
// =============================================================================

use std::fs;
use std::path::{Path, PathBuf};

use crate::gpu::normalize_bus_id;
use crate::models::{GpuSample, HwDevice, HwSection};
use crate::monitor::system_summary;
use crate::util::{
    human_bytes, link_name, marketing_name, pci_ids, read_hex, read_num, read_trim, short_vendor,
    usb_ids,
};

pub fn collect(gpus: &[GpuSample]) -> Vec<HwSection> {
    let sections = vec![
        section("system", "System", system()),
        section("cpu", "Processor", cpu()),
        section("memory", "Memory", memory()),
        section("graphics", "Graphics & Displays", graphics(gpus)),
        section("storage", "Storage", storage()),
        section("network", "Network", network()),
        section("audio", "Audio", audio()),
        section("battery", "Power", battery()),
        section("usb", "USB Devices", usb()),
        section("pci", "PCI Devices", pci()),
    ];
    sections
        .into_iter()
        .filter(|s| !s.devices.is_empty())
        .collect()
}

fn section(id: &str, title: &str, devices: Vec<HwDevice>) -> HwSection {
    HwSection {
        id: id.into(),
        title: title.into(),
        devices,
    }
}

fn sorted_dir(path: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(path)
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ----------------------------------------------------------------- system ---

fn chassis_name(code: Option<u32>) -> Option<String> {
    let name = match code? {
        3 => "Desktop",
        4 => "Low-profile desktop",
        6 => "Mini tower",
        7 => "Tower",
        8 => "Portable",
        9 => "Laptop",
        10 => "Notebook",
        13 => "All-in-one",
        14 => "Sub-notebook",
        17 => "Main server chassis",
        23 => "Rack mount",
        30 => "Tablet",
        31 => "Convertible",
        32 => "Detachable",
        35 => "Mini PC",
        36 => "Stick PC",
        _ => "Other",
    };
    Some(name.into())
}

fn system() -> Vec<HwDevice> {
    let dmi = Path::new("/sys/class/dmi/id");
    let r = |f: &str| {
        read_trim(dmi.join(f))
            .filter(|v| !v.eq_ignore_ascii_case("to be filled by o.e.m.") && v != "Default string")
    };
    let s = system_summary();
    let uefi = Path::new("/sys/firmware/efi").exists();

    let mut out = vec![
        HwDevice::new(r("product_name").unwrap_or_else(|| "Computer".into()))
            .subtitle(r("sys_vendor").unwrap_or_default())
            .prop("Vendor", r("sys_vendor"))
            .prop("Product", r("product_name"))
            .prop("Version", r("product_version"))
            .prop("Family", r("product_family"))
            .prop("SKU", r("product_sku"))
            .prop("Chassis", chassis_name(read_num(dmi.join("chassis_type"))))
            .prop("Hostname", Some(s.hostname.clone())),
        HwDevice::new(r("board_name").unwrap_or_else(|| "Motherboard".into()))
            .subtitle("Motherboard")
            .prop("Vendor", r("board_vendor"))
            .prop("Model", r("board_name"))
            .prop("Version", r("board_version")),
        HwDevice::new(r("bios_vendor").unwrap_or_else(|| "Firmware".into()))
            .subtitle("BIOS / UEFI firmware")
            .prop("Version", r("bios_version"))
            .prop("Release date", r("bios_date"))
            .prop("Revision", r("bios_release"))
            .prop("EC firmware", r("ec_firmware_release"))
            .prop("Boot mode", Some(if uefi { "UEFI" } else { "Legacy BIOS" }))
            .prop("Secure Boot", uefi.then(secure_boot).flatten()),
        HwDevice::new(format!("{} {}", s.os_name, s.os_version))
            .subtitle("Operating system")
            .prop("Kernel", Some(s.kernel.clone()))
            .prop("Architecture", Some(s.arch.clone()))
            .prop("Desktop", s.desktop.clone())
            .prop("Session", s.session_type.clone())
            .prop("glibc", glibc_version()),
    ];
    out.retain(|d| !d.properties.is_empty());
    out
}

fn secure_boot() -> Option<String> {
    let dir = sorted_dir("/sys/firmware/efi/efivars");
    let var = dir
        .iter()
        .find(|p| file_name(p).starts_with("SecureBoot-"))?;
    let bytes = fs::read(var).ok()?;
    // 4 bytes of attributes followed by a 1-byte value.
    Some(
        if bytes.get(4) == Some(&1) {
            "Enabled"
        } else {
            "Disabled"
        }
        .into(),
    )
}

fn glibc_version() -> Option<String> {
    // SAFETY: gnu_get_libc_version returns a static NUL-terminated string.
    #[cfg(target_env = "gnu")]
    unsafe {
        extern "C" {
            fn gnu_get_libc_version() -> *const std::os::raw::c_char;
        }
        let p = gnu_get_libc_version();
        if !p.is_null() {
            return Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned());
        }
    }
    None
}

// -------------------------------------------------------------------- cpu ---

fn cpu() -> Vec<HwDevice> {
    let Ok(info) = fs::read_to_string("/proc/cpuinfo") else {
        return Vec::new();
    };
    let first: Vec<(&str, &str)> = info
        .split("\n\n")
        .next()
        .unwrap_or("")
        .lines()
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    let get = |k: &str| {
        first
            .iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| v.to_string())
    };
    let threads = info.matches("processor\t:").count();
    let sockets = {
        let mut ids: Vec<&str> = info
            .lines()
            .filter(|l| l.starts_with("physical id"))
            .collect();
        ids.sort();
        ids.dedup();
        ids.len().max(1)
    };
    let cores = sysinfo::System::physical_core_count();
    let flags = get("flags").unwrap_or_default();
    let has = |f: &str| flags.split_whitespace().any(|x| x == f);
    let features: Vec<&str> = [
        "avx",
        "avx2",
        "avx512f",
        "fma",
        "aes",
        "sha_ni",
        "sse4_2",
        "vmx",
        "svm",
        "hypervisor",
    ]
    .into_iter()
    .filter(|f| has(f))
    .collect();
    let virt = if has("vmx") {
        Some("Intel VT-x")
    } else if has("svm") {
        Some("AMD-V")
    } else {
        None
    };

    let freq = Path::new("/sys/devices/system/cpu/cpu0/cpufreq");
    let khz = |f: &str| read_num::<u64>(freq.join(f)).map(|k| format!("{} MHz", k / 1000));

    let mut caches: Vec<String> = Vec::new();
    for idx in sorted_dir("/sys/devices/system/cpu/cpu0/cache")
        .iter()
        .filter(|p| file_name(p).starts_with("index"))
    {
        let level = read_trim(idx.join("level")).unwrap_or_default();
        let ty = read_trim(idx.join("type")).unwrap_or_default();
        let size = read_trim(idx.join("size")).unwrap_or_default();
        let tag = match ty.as_str() {
            "Data" => "d",
            "Instruction" => "i",
            _ => "",
        };
        caches.push(format!("L{level}{tag} {size}"));
    }

    let model = get("model name").unwrap_or_else(|| "CPU".into());
    vec![HwDevice::new(model.clone())
        .subtitle(format!(
            "{} cores / {} threads",
            cores.map(|c| c.to_string()).unwrap_or("?".into()),
            threads
        ))
        .prop("Vendor", get("vendor_id"))
        .prop("Sockets", Some(sockets.to_string()))
        .prop("Physical cores", cores.map(|c| c.to_string()))
        .prop("Logical processors", Some(threads.to_string()))
        .prop(
            "Family / Model / Stepping",
            Some(format!(
                "{} / {} / {}",
                get("cpu family").unwrap_or_default(),
                get("model").unwrap_or_default(),
                get("stepping").unwrap_or_default()
            )),
        )
        .prop("Microcode", get("microcode"))
        .prop("Min frequency", khz("cpuinfo_min_freq"))
        .prop("Max frequency", khz("cpuinfo_max_freq"))
        .prop(
            "Base frequency",
            khz("base_frequency").or_else(|| {
                read_num::<u64>(freq.join("base_frequency")).map(|k| format!("{} MHz", k / 1000))
            }),
        )
        .prop("Scaling driver", read_trim(freq.join("scaling_driver")))
        .prop("Governor", read_trim(freq.join("scaling_governor")))
        .prop(
            "Energy preference",
            read_trim(freq.join("energy_performance_preference")),
        )
        .prop("Cache", (!caches.is_empty()).then(|| caches.join(", ")))
        .prop("Virtualization", virt)
        .prop("Address sizes", get("address sizes"))
        .prop(
            "Notable features",
            (!features.is_empty()).then(|| features.join(" ")),
        )
        .prop("Vulnerability mitigations", mitigations_summary())]
}

fn mitigations_summary() -> Option<String> {
    let dir = sorted_dir("/sys/devices/system/cpu/vulnerabilities");
    if dir.is_empty() {
        return None;
    }
    let vulnerable = dir
        .iter()
        .filter(|p| {
            read_trim(p)
                .map(|v| v.starts_with("Vulnerable"))
                .unwrap_or(false)
        })
        .count();
    Some(format!("{} checked, {} vulnerable", dir.len(), vulnerable))
}

// ----------------------------------------------------------------- memory ---

fn memory() -> Vec<HwDevice> {
    let Ok(t) = fs::read_to_string("/proc/meminfo") else {
        return Vec::new();
    };
    let kb = |k: &str| {
        t.lines()
            .find(|l| l.starts_with(&format!("{k}:")))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|v| human_bytes(v * 1024))
    };
    let swaps: Vec<String> = fs::read_to_string("/proc/swaps")
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            Some(format!(
                "{} ({}, {})",
                f.first()?,
                f.get(1)?,
                human_bytes(f.get(2)?.parse::<u64>().ok()? * 1024)
            ))
        })
        .collect();
    let zram = Path::new("/sys/block/zram0").exists();
    let total_mem = kb("MemTotal").unwrap_or_default();
    vec![HwDevice::new(format!("{total_mem} RAM"))
        .subtitle("System memory")
        .prop("Total", kb("MemTotal"))
        .prop("Huge page size", kb("Hugepagesize"))
        .prop("Swap total", kb("SwapTotal"))
        .prop(
            "Swap devices",
            (!swaps.is_empty()).then(|| swaps.join("; ")),
        )
        .prop("zram", zram.then_some("enabled"))
        .prop(
            "Module details",
            Some("Speed, type and slot layout need admin access (coming in Phase 2)"),
        )]
}

// --------------------------------------------------------------- graphics ---

fn graphics(gpus: &[GpuSample]) -> Vec<HwDevice> {
    let mut out: Vec<HwDevice> = Vec::new();
    for card in sorted_dir("/sys/class/drm") {
        let name = file_name(&card);
        let Some(n) = name.strip_prefix("card") else {
            continue;
        };
        if !n.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dev = card.join("device");
        let (Some(v), Some(d)) = (read_hex(dev.join("vendor")), read_hex(dev.join("device")))
        else {
            continue;
        };
        let bus = fs::canonicalize(&dev)
            .ok()
            .map(|p| file_name(&p))
            .unwrap_or_default();
        let sample = gpus.iter().find(|g| g.id == normalize_bus_id(&bus));
        let title = sample.map(|g| g.name.clone()).unwrap_or_else(|| {
            crate::util::pci_ids()
                .device(v, d)
                .map(marketing_name)
                .unwrap_or_else(|| "GPU".into())
        });
        let driver = link_name(dev.join("driver"));
        out.push(
            HwDevice::new(title)
                .subtitle(pci_ids().vendor(v).map(short_vendor).unwrap_or_default())
                .prop("PCI address", Some(bus.clone()))
                .prop("PCI ID", Some(format!("{v:04x}:{d:04x}")))
                .prop("Driver", driver.clone())
                .prop(
                    "Driver version",
                    sample.and_then(|g| g.driver_version.clone()),
                )
                .prop("VRAM", sample.and_then(|g| g.vram_total).map(human_bytes))
                .prop(
                    "Max core clock",
                    sample
                        .and_then(|g| g.max_core_clock_mhz)
                        .map(|m| format!("{m} MHz")),
                )
                .prop(
                    "Power limit",
                    sample
                        .and_then(|g| g.power_limit_watts)
                        .map(|w| format!("{w:.0} W")),
                )
                .prop(
                    "PCIe link",
                    read_trim(dev.join("current_link_speed"))
                        .filter(|s| !s.starts_with("Unknown"))
                        .map(|s| {
                            format!(
                                "{s} x{}",
                                read_trim(dev.join("current_link_width")).unwrap_or("?".into())
                            )
                        }),
                )
                .prop("VBIOS", read_trim(dev.join("vbios_version")))
                .prop("Power state", read_trim(dev.join("power/runtime_status")))
                .prop(
                    "Boot VGA",
                    read_trim(dev.join("boot_vga"))
                        .map(|b| if b == "1" { "Yes" } else { "No" }.to_string()),
                ),
        );
        // Connectors (displays) of this card: cardN-HDMI-A-1 ...
        for conn in sorted_dir("/sys/class/drm") {
            let cn = file_name(&conn);
            let Some(port) = cn.strip_prefix(&format!("{name}-")) else {
                continue;
            };
            if read_trim(conn.join("status")).as_deref() != Some("connected") {
                continue;
            }
            let edid = fs::read(conn.join("edid")).unwrap_or_default();
            let (maker, model) = parse_edid(&edid);
            let mode =
                read_trim(conn.join("modes")).and_then(|m| m.lines().next().map(str::to_string));
            out.push(
                HwDevice::new(
                    model
                        .clone()
                        .unwrap_or_else(|| format!("Display on {port}")),
                )
                .subtitle(format!("Display · {port}"))
                .prop("Manufacturer", maker)
                .prop("Model", model)
                .prop("Connector", Some(port.to_string()))
                .prop("Preferred mode", mode)
                .prop("Physical size", edid_size(&edid)),
            );
        }
    }
    out
}

/// Extract the 3-letter PNP manufacturer ID and the monitor name descriptor.
fn parse_edid(e: &[u8]) -> (Option<String>, Option<String>) {
    if e.len() < 128 || e[0..8] != [0, 255, 255, 255, 255, 255, 255, 0] {
        return (None, None);
    }
    let id = u16::from_be_bytes([e[8], e[9]]);
    let ch = |s: u16| (((id >> s) & 0x1f) as u8 + b'A' - 1) as char;
    let maker = format!("{}{}{}", ch(10), ch(5), ch(0));
    let mut name = None;
    for i in 0..4 {
        let o = 54 + i * 18;
        if e[o] == 0 && e[o + 1] == 0 && e[o + 3] == 0xfc {
            let raw = &e[o + 5..o + 18];
            let s: String = raw
                .iter()
                .take_while(|&&b| b != 0x0a)
                .map(|&b| b as char)
                .collect();
            name = Some(s.trim().to_string()).filter(|s| !s.is_empty());
        }
    }
    (Some(maker), name)
}

fn edid_size(e: &[u8]) -> Option<String> {
    if e.len() < 128 || e[21] == 0 || e[22] == 0 {
        return None;
    }
    let (w, h) = (e[21] as f32, e[22] as f32);
    let inches = (w * w + h * h).sqrt() / 2.54;
    Some(format!("{w:.0} × {h:.0} cm ({inches:.1}\")"))
}

// ---------------------------------------------------------------- storage ---

fn storage() -> Vec<HwDevice> {
    let mut out = Vec::new();
    for b in sorted_dir("/sys/block") {
        let name = file_name(&b);
        if ["loop", "ram", "zram", "dm-", "md", "sr", "fd"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            continue;
        }
        let dev = b.join("device");
        let sectors: u64 = read_num(b.join("size")).unwrap_or(0);
        if sectors == 0 {
            continue;
        }
        let real = fs::canonicalize(&b)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let transport = if name.starts_with("nvme") {
            "NVMe"
        } else if real.contains("/usb") {
            "USB"
        } else if real.contains("/mmc") || name.starts_with("mmcblk") {
            "SD/MMC"
        } else if real.contains("/ata") {
            "SATA"
        } else if real.contains("/virtio") {
            "VirtIO"
        } else {
            "Other"
        };
        let rotational = read_trim(b.join("queue/rotational")).as_deref() == Some("1");
        let kind = if name.starts_with("nvme") || !rotational {
            "SSD"
        } else {
            "HDD"
        };
        let model = read_trim(dev.join("model")).or_else(|| read_trim(dev.join("name")));
        out.push(
            HwDevice::new(model.clone().unwrap_or_else(|| name.clone()))
                .subtitle(format!(
                    "{} · {} {}",
                    human_bytes(sectors * 512),
                    transport,
                    kind
                ))
                .prop("Device", Some(format!("/dev/{name}")))
                .prop("Vendor", read_trim(dev.join("vendor")))
                .prop("Model", model)
                .prop("Capacity", Some(human_bytes(sectors * 512)))
                .prop(
                    "Firmware",
                    read_trim(dev.join("firmware_rev")).or_else(|| read_trim(dev.join("rev"))),
                )
                .prop("Interface", Some(transport))
                .prop("Type", Some(kind))
                .prop(
                    "Removable",
                    read_trim(b.join("removable"))
                        .map(|r| if r == "1" { "Yes" } else { "No" }.to_string()),
                )
                .prop(
                    "Scheduler",
                    read_trim(b.join("queue/scheduler")).and_then(|s| {
                        s.split_whitespace()
                            .find(|w| w.starts_with('['))
                            .map(|w| w.trim_matches(['[', ']']).to_string())
                    }),
                )
                .prop(
                    "Logical block",
                    read_trim(b.join("queue/logical_block_size")).map(|v| format!("{v} B")),
                )
                .prop(
                    "Partitions",
                    Some(
                        sorted_dir(&b)
                            .iter()
                            .filter(|p| file_name(p).starts_with(&name))
                            .count()
                            .to_string(),
                    ),
                ),
        );
    }
    out
}

// ---------------------------------------------------------------- network ---

fn network() -> Vec<HwDevice> {
    let mut out = Vec::new();
    for n in sorted_dir("/sys/class/net") {
        let name = file_name(&n);
        if name == "lo" {
            continue;
        }
        let dev = n.join("device");
        let physical = dev.exists();
        let wireless = n.join("wireless").exists() || n.join("phy80211").exists();
        let kind = match (physical, wireless) {
            (_, true) => "Wi-Fi",
            (true, false) => "Ethernet",
            _ => "Virtual",
        };
        let driver = link_name(dev.join("driver"));
        let model = match (read_hex(dev.join("vendor")), read_hex(dev.join("device"))) {
            (Some(v), Some(d)) if real_bus(&dev) == "pci" => {
                pci_ids().device(v, d).map(marketing_name)
            }
            (Some(v), Some(d)) => usb_ids().device(v, d).map(str::to_string),
            _ => None,
        };
        let speed = read_num::<i64>(n.join("speed"))
            .filter(|s| *s > 0)
            .map(|s| format!("{s} Mb/s"));
        out.push(
            HwDevice::new(model.clone().unwrap_or_else(|| name.clone()))
                .subtitle(format!("{kind} · {name}"))
                .prop("Interface", Some(name.clone()))
                .prop("Type", Some(kind))
                .prop("MAC address", read_trim(n.join("address")))
                .prop("State", read_trim(n.join("operstate")))
                .prop("Link speed", speed)
                .prop("MTU", read_trim(n.join("mtu")))
                .prop("Driver", driver.clone())
                .prop(
                    "Driver version",
                    driver
                        .as_deref()
                        .and_then(|d| read_trim(format!("/sys/module/{d}/version"))),
                )
                .prop("Firmware", wifi_firmware(&n)),
        );
    }
    // Virtual interfaces last.
    out.sort_by_key(|d| {
        d.subtitle
            .as_deref()
            .map(|s| s.starts_with("Virtual"))
            .unwrap_or(false)
    });
    out
}

fn real_bus(dev: &Path) -> String {
    link_name(dev.join("subsystem")).unwrap_or_default()
}

fn wifi_firmware(_n: &Path) -> Option<String> {
    // Firmware versions need ethtool ioctls; added with the privileged helper.
    None
}

// ------------------------------------------------------------------ audio ---

fn audio() -> Vec<HwDevice> {
    let Ok(t) = fs::read_to_string("/proc/asound/cards") else {
        return Vec::new();
    };
    let lines: Vec<&str> = t.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i].trim();
        // " 0 [PCH            ]: HDA-Intel - HDA Intel PCH"
        if let Some((head, desc)) = l.split_once("]: ") {
            let idx = head.split_whitespace().next().unwrap_or("");
            let (driver, short) = desc.split_once(" - ").unwrap_or((desc, desc));
            let long = lines.get(i + 1).map(|s| s.trim().to_string());
            let codec = read_trim(format!("/proc/asound/card{idx}/codec#0")).and_then(|c| {
                c.lines()
                    .next()
                    .and_then(|l| l.strip_prefix("Codec: "))
                    .map(str::to_string)
            });
            out.push(
                HwDevice::new(short.trim())
                    .subtitle(format!("Sound card {idx}"))
                    .prop("Driver", Some(driver.trim()))
                    .prop("Codec", codec)
                    .prop("Description", long),
            );
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------- battery ---

fn battery() -> Vec<HwDevice> {
    let mut out = Vec::new();
    for ps in sorted_dir("/sys/class/power_supply") {
        let ty = read_trim(ps.join("type")).unwrap_or_default();
        let name = file_name(&ps);
        match ty.as_str() {
            "Battery" => {
                let full: Option<f64> =
                    read_num(ps.join("energy_full")).or_else(|| read_num(ps.join("charge_full")));
                let design: Option<f64> = read_num(ps.join("energy_full_design"))
                    .or_else(|| read_num(ps.join("charge_full_design")));
                let unit_is_energy = ps.join("energy_full").exists();
                let fmt = |v: f64| {
                    if unit_is_energy {
                        format!("{:.1} Wh", v / 1e6)
                    } else {
                        format!("{:.0} mAh", v / 1e3)
                    }
                };
                let health = match (full, design) {
                    (Some(f), Some(d)) if d > 0.0 => Some(format!("{:.0}%", f / d * 100.0)),
                    _ => None,
                };
                out.push(
                    HwDevice::new(read_trim(ps.join("model_name")).unwrap_or_else(|| name.clone()))
                        .subtitle(format!("Battery · {name}"))
                        .prop("Manufacturer", read_trim(ps.join("manufacturer")))
                        .prop("Technology", read_trim(ps.join("technology")))
                        .prop("Status", read_trim(ps.join("status")))
                        .prop(
                            "Charge",
                            read_trim(ps.join("capacity")).map(|c| format!("{c}%")),
                        )
                        .prop("Design capacity", design.map(fmt))
                        .prop("Full capacity", full.map(fmt))
                        .prop("Health", health)
                        .prop(
                            "Cycle count",
                            read_trim(ps.join("cycle_count")).filter(|c| c != "0"),
                        )
                        .prop(
                            "Charge limit",
                            read_trim(ps.join("charge_control_end_threshold"))
                                .map(|c| format!("{c}%")),
                        ),
                );
            }
            "Mains" | "USB" => {
                out.push(
                    HwDevice::new(format!("AC adapter ({name})"))
                        .subtitle("Power supply")
                        .prop(
                            "Online",
                            read_trim(ps.join("online"))
                                .map(|o| if o == "1" { "Yes" } else { "No" }.to_string()),
                        ),
                );
            }
            _ => {}
        }
    }
    out
}

// -------------------------------------------------------------------- usb ---

fn usb() -> Vec<HwDevice> {
    let mut out = Vec::new();
    for d in sorted_dir("/sys/bus/usb/devices") {
        let name = file_name(&d);
        // Skip interfaces ("1-2:1.0") and root hubs ("usb1").
        if name.contains(':') || name.starts_with("usb") {
            continue;
        }
        let (Some(v), Some(p)) = (read_hex(d.join("idVendor")), read_hex(d.join("idProduct")))
        else {
            continue;
        };
        let db = usb_ids();
        let product = read_trim(d.join("product")).or_else(|| db.device(v, p).map(str::to_string));
        let maker = read_trim(d.join("manufacturer")).or_else(|| db.vendor(v).map(str::to_string));
        let class = read_trim(d.join("bDeviceClass"))
            .as_deref()
            .map(usb_class)
            .map(str::to_string);
        let speed = read_trim(d.join("speed")).map(|s| format!("{s} Mb/s"));
        out.push(
            HwDevice::new(
                product
                    .clone()
                    .unwrap_or_else(|| format!("USB device {v:04x}:{p:04x}")),
            )
            .subtitle(maker.clone().unwrap_or_default())
            .prop("Manufacturer", maker)
            .prop("ID", Some(format!("{v:04x}:{p:04x}")))
            .prop("Bus / port", Some(name.clone()))
            .prop("USB version", read_trim(d.join("version")))
            .prop("Device revision", read_trim(d.join("bcdDevice")))
            .prop("Speed", speed)
            .prop("Class", class)
            .prop("Max power", read_trim(d.join("bMaxPower"))),
        );
    }
    out
}

fn usb_class(code: &str) -> &'static str {
    match code {
        "00" => "Per-interface",
        "01" => "Audio",
        "02" => "Communications",
        "03" => "HID",
        "06" => "Imaging",
        "07" => "Printer",
        "08" => "Mass storage",
        "09" => "Hub",
        "0a" => "CDC data",
        "0b" => "Smart card",
        "0e" => "Video",
        "e0" => "Wireless (Bluetooth)",
        "ef" => "Miscellaneous",
        "ff" => "Vendor specific",
        _ => "Other",
    }
}

// -------------------------------------------------------------------- pci ---

fn pci_class(class: u32) -> &'static str {
    match (class >> 16) & 0xff {
        0x01 => "Storage controller",
        0x02 => "Network controller",
        0x03 => "Display controller",
        0x04 => "Multimedia controller",
        0x05 => "Memory controller",
        0x06 => "Bridge",
        0x07 => "Communication controller",
        0x08 => "System peripheral",
        0x09 => "Input device",
        0x0c => "Serial bus controller",
        0x0d => "Wireless controller",
        0x10 => "Encryption controller",
        0x11 => "Signal processing",
        0x12 => "Processing accelerator",
        0x13 => "Instrumentation",
        _ => "Other",
    }
}

fn pci() -> Vec<HwDevice> {
    let mut out = Vec::new();
    for d in sorted_dir("/sys/bus/pci/devices") {
        let addr = file_name(&d);
        let (Some(v), Some(p)) = (read_hex(d.join("vendor")), read_hex(d.join("device"))) else {
            continue;
        };
        let class = read_trim(d.join("class"))
            .and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);
        let db = pci_ids();
        let vendor = db.vendor(v).map(short_vendor);
        let device = db.device(v, p).map(str::to_string);
        let driver = link_name(d.join("driver"));
        out.push(
            HwDevice::new(device.clone().unwrap_or_else(|| format!("Device {p:04x}")))
                .subtitle(format!(
                    "{} · {}",
                    pci_class(class),
                    vendor.clone().unwrap_or_default()
                ))
                .prop("Address", Some(addr))
                .prop("Vendor", vendor)
                .prop("ID", Some(format!("{v:04x}:{p:04x}")))
                .prop("Class", Some(pci_class(class)))
                .prop("Revision", read_trim(d.join("revision")))
                .prop("Driver", driver.clone())
                .prop(
                    "Driver version",
                    driver
                        .as_deref()
                        .and_then(|m| read_trim(format!("/sys/module/{m}/version"))),
                )
                .prop("IOMMU group", link_name(d.join("iommu_group"))),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_has_core_sections() {
        let s = collect(&[]);
        assert!(s.iter().any(|x| x.id == "cpu"));
        assert!(s.iter().any(|x| x.id == "memory"));
    }

    #[test]
    fn edid_rejects_garbage() {
        assert_eq!(parse_edid(&[1, 2, 3]), (None, None));
    }
}
