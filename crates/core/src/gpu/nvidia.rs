// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/gpu/nvidia.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: NVIDIA GPU metrics through NVML (libnvidia-ml, shipped with the
//           proprietary driver). The library is loaded at runtime, so SysPulse
//           runs on machines without NVIDIA hardware.
//
//           Hybrid laptops: if the dGPU is runtime-suspended (D3cold), NVML
//           queries would wake it and drain the battery, so suspended GPUs
//           are reported as sleeping and not queried.
// =============================================================================

use std::ffi::OsStr;

use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::enums::device::UsedGpuMemory;
use nvml_wrapper::Nvml;

use super::{normalize_bus_id, GpuProvider};
use crate::models::{GpuProcess, GpuSample, GpuVendor};

struct DeviceInfo {
    index: u32,
    bus_id: String,
    name: String,
    max_core_clock: Option<u32>,
    power_limit: Option<f32>,
    fans: u32,
}

pub struct NvidiaProvider {
    nvml: Nvml,
    devices: Vec<DeviceInfo>,
    driver_version: Option<String>,
}

impl NvidiaProvider {
    /// Returns `None` when the NVIDIA driver / NVML is not installed.
    pub fn new() -> Option<Self> {
        let nvml = Nvml::init()
            .or_else(|_| {
                Nvml::builder()
                    .lib_path(OsStr::new("libnvidia-ml.so.1"))
                    .init()
            })
            .ok()?;
        let count = nvml.device_count().ok()?;
        let devices = (0..count)
            .filter_map(|index| {
                let d = nvml.device_by_index(index).ok()?;
                Some(DeviceInfo {
                    index,
                    bus_id: normalize_bus_id(&d.pci_info().ok()?.bus_id),
                    name: d.name().unwrap_or_else(|_| "NVIDIA GPU".into()),
                    max_core_clock: d.max_clock_info(Clock::Graphics).ok(),
                    power_limit: d.enforced_power_limit().ok().map(|mw| mw as f32 / 1000.0),
                    fans: d.num_fans().unwrap_or(0),
                })
            })
            .collect();
        let driver_version = nvml.sys_driver_version().ok();
        Some(NvidiaProvider {
            nvml,
            devices,
            driver_version,
        })
    }

    pub fn driver_version(&self) -> Option<String> {
        self.driver_version.clone()
    }

    fn sample_device(&self, info: &DeviceInfo) -> GpuSample {
        let mut s = GpuSample::empty(info.bus_id.clone(), GpuVendor::Nvidia, info.name.clone());
        s.driver = Some("nvidia".into());
        s.driver_version = self.driver_version.clone();
        s.max_core_clock_mhz = info.max_core_clock;
        s.power_limit_watts = info.power_limit;

        let state = runtime_status(&info.bus_id);
        if state.as_deref() == Some("suspended") {
            s.power_state = state;
            return s;
        }
        s.power_state = state.or_else(|| Some("active".into()));

        let Ok(d) = self.nvml.device_by_index(info.index) else {
            return s;
        };
        if let Ok(u) = d.utilization_rates() {
            s.utilization = Some(u.gpu as f32);
            s.memory_utilization = Some(u.memory as f32);
        }
        if let Ok(m) = d.memory_info() {
            s.vram_used = Some(m.used);
            s.vram_total = Some(m.total);
        }
        s.temperature = d.temperature(TemperatureSensor::Gpu).ok().map(|t| t as f32);
        s.power_watts = d.power_usage().ok().map(|mw| mw as f32 / 1000.0);
        s.core_clock_mhz = d.clock_info(Clock::Graphics).ok();
        s.memory_clock_mhz = d.clock_info(Clock::Memory).ok();
        if info.fans > 0 {
            s.fan_percent = d.fan_speed(0).ok();
        }
        s.encoder_utilization = d.encoder_utilization().ok().map(|u| u.utilization);
        s.decoder_utilization = d.decoder_utilization().ok().map(|u| u.utilization);
        s.pcie_generation = d.current_pcie_link_gen().ok();
        s.pcie_width = d.current_pcie_link_width().ok();

        let mut procs: Vec<GpuProcess> = Vec::new();
        for (list, kind) in [
            (d.running_graphics_processes(), "graphics"),
            (d.running_compute_processes(), "compute"),
        ] {
            for p in list.unwrap_or_default() {
                if procs.iter().any(|x| x.pid == p.pid) {
                    continue;
                }
                procs.push(GpuProcess {
                    pid: p.pid,
                    name: process_name(p.pid),
                    used_memory: match p.used_gpu_memory {
                        UsedGpuMemory::Used(b) => Some(b),
                        UsedGpuMemory::Unavailable => None,
                    },
                    kind,
                });
            }
        }
        procs.sort_by_key(|p| std::cmp::Reverse(p.used_memory));
        s.processes = procs;
        s
    }
}

impl GpuProvider for NvidiaProvider {
    fn name(&self) -> &'static str {
        "nvml"
    }

    fn sample(&mut self) -> Vec<GpuSample> {
        self.devices.iter().map(|d| self.sample_device(d)).collect()
    }
}

#[cfg(target_os = "linux")]
fn runtime_status(bus_id: &str) -> Option<String> {
    crate::util::read_trim(format!(
        "/sys/bus/pci/devices/{bus_id}/power/runtime_status"
    ))
}

#[cfg(not(target_os = "linux"))]
fn runtime_status(_bus_id: &str) -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn process_name(pid: u32) -> String {
    crate::util::read_trim(format!("/proc/{pid}/comm")).unwrap_or_else(|| pid.to_string())
}

#[cfg(not(target_os = "linux"))]
fn process_name(pid: u32) -> String {
    pid.to_string()
}
