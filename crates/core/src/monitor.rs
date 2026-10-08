// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/monitor.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Periodic sampler for CPU, memory, filesystems, disk I/O, network,
//           temperature sensors and battery. One instance lives on the
//           telemetry thread; call `sample()` once per tick.
// =============================================================================

use std::collections::HashMap;
use std::time::Instant;

use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

use crate::models::*;

/// Interfaces that only mirror traffic of real NICs (would double count).
fn is_virtual_iface(name: &str) -> bool {
    name == "lo"
        || ["veth", "docker", "br-", "virbr", "vnet", "tun", "tap"]
            .iter()
            .any(|p| name.starts_with(p))
}

/// Pseudo / read-only filesystems hidden from the storage view.
fn is_pseudo_fs(fs: &str, mount: &str) -> bool {
    matches!(
        fs,
        "squashfs"
            | "tmpfs"
            | "devtmpfs"
            | "overlay"
            | "proc"
            | "sysfs"
            | "efivarfs"
            | "fuse.portal"
            | "nsfs"
    ) || mount.starts_with("/snap/")
        || mount.starts_with("/var/snap/")
        || mount.starts_with("/run/")
        || mount.starts_with("/sys/")
}

/// `VERSION` from /etc/os-release, e.g. "22.04.5 LTS (Jammy Jellyfish)".
#[cfg(target_os = "linux")]
fn os_release_version() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("VERSION="))
        .map(|v| v.trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(not(target_os = "linux"))]
fn os_release_version() -> Option<String> {
    System::long_os_version()
}

pub fn system_summary() -> SystemSummary {
    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    SystemSummary {
        hostname: System::host_name().unwrap_or_default(),
        user: std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_default(),
        os_name: System::name().unwrap_or_else(|| "Unknown".into()),
        os_version: os_release_version()
            .or_else(System::os_version)
            .unwrap_or_default(),
        kernel: System::kernel_version().unwrap_or_default(),
        arch: System::cpu_arch(),
        desktop: std::env::var("XDG_CURRENT_DESKTOP").ok(),
        session_type: std::env::var("XDG_SESSION_TYPE").ok(),
        cpu_model,
        physical_cores: System::physical_core_count(),
        logical_cores: sys.cpus().len(),
        total_memory: sys.total_memory(),
        boot_time: System::boot_time(),
    }
}

pub struct SystemSampler {
    sys: System,
    networks: Networks,
    disks: Disks,
    last: Instant,
    #[cfg(target_os = "linux")]
    disk_stats: HashMap<String, linux::DiskStat>,
    #[cfg(not(target_os = "linux"))]
    _unused: HashMap<(), ()>,
}

impl Default for SystemSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemSampler {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        SystemSampler {
            sys,
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            last: Instant::now(),
            #[cfg(target_os = "linux")]
            disk_stats: linux::read_diskstats(),
            #[cfg(not(target_os = "linux"))]
            _unused: HashMap::new(),
        }
    }

    /// Collect one sample (GPU list left empty for the GPU manager to fill).
    /// Rates are computed against the previous call.
    pub fn sample(&mut self) -> Telemetry {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64().max(0.001);
        self.last = now;

        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.networks.refresh(true);
        self.disks.refresh(true);

        let sensors = read_sensors();
        Telemetry {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            cpu: self.cpu_sample(&sensors),
            memory: self.memory_sample(),
            filesystems: self.filesystems(),
            disk_io: self.disk_io(elapsed),
            network: self.network(elapsed),
            sensors,
            battery: read_battery(),
            gpus: Vec::new(),
        }
    }

    fn cpu_sample(&self, sensors: &[SensorSample]) -> CpuSample {
        let la = System::load_average();
        CpuSample {
            total: self.sys.global_cpu_usage(),
            per_core: self.sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
            frequency_mhz: self.sys.cpus().iter().map(|c| c.frequency()).collect(),
            load_average: [la.one, la.five, la.fifteen],
            temperature: cpu_temperature(sensors),
            process_count: process_count(),
            uptime_secs: System::uptime(),
        }
    }

    fn memory_sample(&self) -> MemorySample {
        let total = self.sys.total_memory();
        let available = self.sys.available_memory();
        MemorySample {
            total,
            used: total.saturating_sub(available),
            available,
            cached: cached_memory(),
            swap_total: self.sys.total_swap(),
            swap_used: self.sys.used_swap(),
        }
    }

    fn filesystems(&self) -> Vec<FilesystemSample> {
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<FilesystemSample> = self
            .disks
            .list()
            .iter()
            .filter_map(|d| {
                let fs = d.file_system().to_string_lossy().to_string();
                let mount = d.mount_point().to_string_lossy().to_string();
                let device = d.name().to_string_lossy().to_string();
                if is_pseudo_fs(&fs, &mount) || d.total_space() == 0 || !seen.insert(device.clone())
                {
                    return None;
                }
                Some(FilesystemSample {
                    device,
                    mount_point: mount,
                    fs_type: fs,
                    total: d.total_space(),
                    available: d.available_space(),
                    removable: d.is_removable(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
        out
    }

    #[cfg(target_os = "linux")]
    fn disk_io(&mut self, elapsed: f64) -> Vec<DiskIoSample> {
        let current = linux::read_diskstats();
        let mut out: Vec<DiskIoSample> = current
            .iter()
            .filter_map(|(name, cur)| {
                let prev = self.disk_stats.get(name)?;
                Some(DiskIoSample {
                    device: name.clone(),
                    read_bps: cur.sectors_read.saturating_sub(prev.sectors_read) as f64 * 512.0
                        / elapsed,
                    write_bps: cur.sectors_written.saturating_sub(prev.sectors_written) as f64
                        * 512.0
                        / elapsed,
                    busy_percent: (cur.io_ticks_ms.saturating_sub(prev.io_ticks_ms) as f64
                        / (elapsed * 10.0))
                        .min(100.0),
                })
            })
            .collect();
        out.sort_by(|a, b| a.device.cmp(&b.device));
        self.disk_stats = current;
        out
    }

    #[cfg(not(target_os = "linux"))]
    fn disk_io(&mut self, elapsed: f64) -> Vec<DiskIoSample> {
        self.disks
            .list()
            .iter()
            .map(|d| {
                let u = d.usage();
                DiskIoSample {
                    device: d.name().to_string_lossy().to_string(),
                    read_bps: u.read_bytes as f64 / elapsed,
                    write_bps: u.written_bytes as f64 / elapsed,
                    busy_percent: 0.0,
                }
            })
            .collect()
    }

    fn network(&self, elapsed: f64) -> Vec<NetworkSample> {
        let mut out: Vec<NetworkSample> = self
            .networks
            .list()
            .iter()
            .filter(|(name, _)| !is_virtual_iface(name))
            .map(|(name, d)| NetworkSample {
                interface: name.clone(),
                rx_bps: d.received() as f64 / elapsed,
                tx_bps: d.transmitted() as f64 / elapsed,
                rx_total: d.total_received(),
                tx_total: d.total_transmitted(),
            })
            .collect();
        out.sort_by(|a, b| a.interface.cmp(&b.interface));
        out
    }
}

/// Pick the most representative CPU temperature from the sensor list.
fn cpu_temperature(sensors: &[SensorSample]) -> Option<f32> {
    const PREFERRED: [(&str, &str); 5] = [
        ("coretemp", "Package id 0"),
        ("k10temp", "Tctl"),
        ("k10temp", "Tdie"),
        ("zenpower", "Tdie"),
        ("cpu_thermal", ""),
    ];
    PREFERRED
        .iter()
        .find_map(|(chip, label)| {
            sensors
                .iter()
                .find(|s| s.chip == *chip && (label.is_empty() || s.label == *label))
        })
        .or_else(|| {
            sensors
                .iter()
                .find(|s| s.chip == "coretemp" || s.chip == "k10temp")
        })
        .map(|s| s.temperature)
}

#[cfg(target_os = "linux")]
fn process_count() -> usize {
    std::fs::read_dir("/proc")
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                })
                .count()
        })
        .unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
fn process_count() -> usize {
    0
}

#[cfg(target_os = "linux")]
fn cached_memory() -> u64 {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            matches!(k, "Cached" | "Buffers" | "SReclaimable")
                .then(|| v.trim().trim_end_matches(" kB").trim().parse::<u64>().ok())
                .flatten()
        })
        .sum::<u64>()
        * 1024
}

#[cfg(not(target_os = "linux"))]
fn cached_memory() -> u64 {
    0
}

#[cfg(target_os = "linux")]
pub fn read_sensors() -> Vec<SensorSample> {
    linux::read_hwmon()
}

#[cfg(not(target_os = "linux"))]
pub fn read_sensors() -> Vec<SensorSample> {
    let comps = sysinfo::Components::new_with_refreshed_list();
    comps
        .list()
        .iter()
        .filter_map(|c| {
            Some(SensorSample {
                chip: String::new(),
                label: c.label().to_string(),
                temperature: c.temperature()?,
                critical: c.critical(),
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn read_battery() -> Option<BatterySample> {
    linux::read_battery()
}

#[cfg(not(target_os = "linux"))]
fn read_battery() -> Option<BatterySample> {
    None
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::util::{read_num, read_trim};
    use std::fs;
    use std::path::Path;

    #[derive(Debug, Clone, Copy)]
    pub struct DiskStat {
        pub sectors_read: u64,
        pub sectors_written: u64,
        pub io_ticks_ms: u64,
    }

    /// Whole block devices from /proc/diskstats (partitions and loop/ram excluded).
    pub fn read_diskstats() -> HashMap<String, DiskStat> {
        let Ok(text) = fs::read_to_string("/proc/diskstats") else {
            return HashMap::new();
        };
        text.lines()
            .filter_map(|line| {
                let f: Vec<&str> = line.split_whitespace().collect();
                if f.len() < 14 {
                    return None;
                }
                let name = f[2];
                if ["loop", "ram", "zram", "fd", "sr"]
                    .iter()
                    .any(|p| name.starts_with(p))
                {
                    return None;
                }
                // Only whole disks have a /sys/block entry.
                if !Path::new("/sys/block").join(name).exists() {
                    return None;
                }
                Some((
                    name.to_string(),
                    DiskStat {
                        sectors_read: f[5].parse().ok()?,
                        sectors_written: f[9].parse().ok()?,
                        io_ticks_ms: f[12].parse().ok()?,
                    },
                ))
            })
            .collect()
    }

    pub fn read_hwmon() -> Vec<SensorSample> {
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir("/sys/class/hwmon") else {
            return out;
        };
        for entry in rd.filter_map(Result::ok) {
            let dir = entry.path();
            let chip = read_trim(dir.join("name")).unwrap_or_else(|| "hwmon".into());
            // GPU chips are reported by the GPU module.
            if matches!(chip.as_str(), "amdgpu" | "nouveau" | "i915" | "xe") {
                continue;
            }
            let Ok(files) = fs::read_dir(&dir) else {
                continue;
            };
            let mut inputs: Vec<String> = files
                .filter_map(Result::ok)
                .map(|f| f.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("temp") && n.ends_with("_input"))
                .collect();
            inputs.sort();
            for input in inputs {
                let base = input.trim_end_matches("_input");
                let Some(milli) = read_num::<f32>(dir.join(&input)) else {
                    continue;
                };
                let temperature = milli / 1000.0;
                if !(-40.0..=150.0).contains(&temperature) {
                    continue;
                }
                let label = read_trim(dir.join(format!("{base}_label")))
                    .unwrap_or_else(|| base.to_string());
                let critical = read_num::<f32>(dir.join(format!("{base}_crit")))
                    .map(|c| c / 1000.0)
                    .filter(|c| *c > 0.0 && *c < 200.0);
                out.push(SensorSample {
                    chip: chip.clone(),
                    label,
                    temperature,
                    critical,
                });
            }
        }
        out
    }

    pub fn read_battery() -> Option<BatterySample> {
        let rd = fs::read_dir("/sys/class/power_supply").ok()?;
        for entry in rd.filter_map(Result::ok) {
            let dir = entry.path();
            if read_trim(dir.join("type")).as_deref() != Some("Battery") {
                continue;
            }
            if read_trim(dir.join("scope")).as_deref() == Some("Device") {
                continue; // wireless mouse/keyboard batteries
            }
            let percent = read_num::<f32>(dir.join("capacity"))?;
            let status = read_trim(dir.join("status")).unwrap_or_else(|| "Unknown".into());
            let power_watts = read_num::<f64>(dir.join("power_now"))
                .map(|uw| uw / 1e6)
                .or_else(|| {
                    let ua = read_num::<f64>(dir.join("current_now"))?;
                    let uv = read_num::<f64>(dir.join("voltage_now"))?;
                    Some(ua * uv / 1e12)
                })
                .filter(|w| *w > 0.0)
                .map(|w| w as f32);
            return Some(BatterySample {
                percent,
                status,
                power_watts,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_interfaces_are_filtered() {
        assert!(is_virtual_iface("lo"));
        assert!(is_virtual_iface("veth12ab"));
        assert!(!is_virtual_iface("wlp0s20f3"));
    }

    #[test]
    fn sampler_produces_data() {
        let mut s = SystemSampler::new();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let t = s.sample();
        assert!(!t.cpu.per_core.is_empty());
        assert!(t.memory.total > 0);
    }
}
