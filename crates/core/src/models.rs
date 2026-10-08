// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/models.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Serializable data model shared with the UI. The TypeScript mirror
//           lives in ui/src/lib/types.ts; keep both in sync.
// =============================================================================

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSummary {
    pub hostname: String,
    /// Login name of the user running SysPulse.
    pub user: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel: String,
    pub arch: String,
    pub desktop: Option<String>,
    pub session_type: Option<String>,
    pub cpu_model: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub total_memory: u64,
    pub boot_time: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuSample {
    pub total: f32,
    pub per_core: Vec<f32>,
    pub frequency_mhz: Vec<u64>,
    pub load_average: [f64; 3],
    pub temperature: Option<f32>,
    pub process_count: usize,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemSample {
    pub device: String,
    pub mount_point: String,
    pub fs_type: String,
    pub total: u64,
    pub available: u64,
    pub removable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskIoSample {
    pub device: String,
    pub read_bps: f64,
    pub write_bps: f64,
    pub busy_percent: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSample {
    pub interface: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub rx_total: u64,
    pub tx_total: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SensorSample {
    pub chip: String,
    pub label: String,
    pub temperature: f32,
    pub critical: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatterySample {
    pub percent: f32,
    pub status: String,
    pub power_watts: Option<f32>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl GpuVendor {
    pub fn from_pci(vendor: u16) -> Self {
        match vendor {
            0x10de => GpuVendor::Nvidia,
            0x1002 | 0x1022 => GpuVendor::Amd,
            0x8086 => GpuVendor::Intel,
            _ => GpuVendor::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuProcess {
    pub pid: u32,
    pub name: String,
    pub used_memory: Option<u64>,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuSample {
    /// PCI bus id, e.g. `0000:01:00.0` (stable across samples).
    pub id: String,
    pub vendor: GpuVendor,
    pub name: String,
    pub driver: Option<String>,
    pub driver_version: Option<String>,
    pub utilization: Option<f32>,
    pub memory_utilization: Option<f32>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub temperature: Option<f32>,
    pub power_watts: Option<f32>,
    pub power_limit_watts: Option<f32>,
    pub core_clock_mhz: Option<u32>,
    pub max_core_clock_mhz: Option<u32>,
    pub memory_clock_mhz: Option<u32>,
    pub fan_percent: Option<u32>,
    pub encoder_utilization: Option<u32>,
    pub decoder_utilization: Option<u32>,
    pub pcie_generation: Option<u32>,
    pub pcie_width: Option<u32>,
    /// Runtime PM state from sysfs: `active`, `suspended` (sleeping dGPU), ...
    pub power_state: Option<String>,
    pub processes: Vec<GpuProcess>,
}

impl GpuSample {
    pub fn empty(id: String, vendor: GpuVendor, name: String) -> Self {
        GpuSample {
            id,
            vendor,
            name,
            driver: None,
            driver_version: None,
            utilization: None,
            memory_utilization: None,
            vram_used: None,
            vram_total: None,
            temperature: None,
            power_watts: None,
            power_limit_watts: None,
            core_clock_mhz: None,
            max_core_clock_mhz: None,
            memory_clock_mhz: None,
            fan_percent: None,
            encoder_utilization: None,
            decoder_utilization: None,
            pcie_generation: None,
            pcie_width: None,
            power_state: None,
            processes: Vec::new(),
        }
    }
}

/// One telemetry tick, pushed to the UI as the `telemetry` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Telemetry {
    /// Unix time in milliseconds.
    pub timestamp: u64,
    pub cpu: CpuSample,
    pub memory: MemorySample,
    pub filesystems: Vec<FilesystemSample>,
    pub disk_io: Vec<DiskIoSample>,
    pub network: Vec<NetworkSample>,
    pub sensors: Vec<SensorSample>,
    pub battery: Option<BatterySample>,
    pub gpus: Vec<GpuSample>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: Option<u32>,
    pub name: String,
    pub user: String,
    pub state: String,
    /// CPU usage in percent of one core (can exceed 100 on multi-core).
    pub cpu: f32,
    pub memory: u64,
    pub virtual_memory: u64,
    pub start_time: u64,
    pub disk_read_bps: f64,
    pub disk_write_bps: f64,
    pub threads: Option<usize>,
    pub nice: Option<i32>,
    pub command: String,
    pub exe: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessDetails {
    pub info: ProcessInfo,
    pub cmdline: Vec<String>,
    pub cwd: Option<String>,
    pub root: Option<String>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub open_files: Option<usize>,
    pub cgroup: Option<String>,
    pub children: Vec<u32>,
    pub environment_count: Option<usize>,
    /// Selected `/proc/<pid>/status` fields as (key, value).
    pub status: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Signal {
    Term,
    Kill,
    Stop,
    Cont,
    Hup,
    Int,
}

impl Signal {
    /// Name accepted by `kill -<NAME>`.
    pub fn name(self) -> &'static str {
        match self {
            Signal::Term => "TERM",
            Signal::Kill => "KILL",
            Signal::Stop => "STOP",
            Signal::Cont => "CONT",
            Signal::Hup => "HUP",
            Signal::Int => "INT",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KillOutcome {
    pub pid: u32,
    pub signal: Signal,
    /// True when the signal had to be sent through pkexec (admin password).
    pub elevated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HwProperty {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HwDevice {
    pub title: String,
    pub subtitle: Option<String>,
    pub properties: Vec<HwProperty>,
}

impl HwDevice {
    pub fn new(title: impl Into<String>) -> Self {
        HwDevice {
            title: title.into(),
            subtitle: None,
            properties: Vec::new(),
        }
    }

    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.subtitle = Some(s);
        }
        self
    }

    /// Add a property; `None` or empty values are skipped.
    pub fn prop(mut self, key: &str, value: Option<impl Into<String>>) -> Self {
        if let Some(v) = value {
            let v = v.into();
            if !v.trim().is_empty() {
                self.properties.push(HwProperty {
                    key: key.to_string(),
                    value: v,
                });
            }
        }
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HwSection {
    pub id: String,
    pub title: String,
    pub devices: Vec<HwDevice>,
}
