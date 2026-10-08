// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/gpu/drm.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Vendor-neutral GPU metrics from the Linux DRM / hwmon sysfs tree.
//             * AMD (amdgpu): busy %, VRAM, temperature, power, clocks, fan
//             * Intel (i915/xe): current / max GT frequency, power (dGPU)
//             * Others: identification only
//           Reading sysfs never wakes a runtime-suspended GPU.
// =============================================================================

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::GpuProvider;
use crate::models::{GpuSample, GpuVendor};
use crate::util::{
    link_name, marketing_name, pci_ids, read_hex, read_num, read_trim, short_vendor,
};

struct Card {
    /// `/sys/class/drm/cardN`
    path: PathBuf,
    bus_id: String,
    vendor: GpuVendor,
    name: String,
    driver: Option<String>,
    hwmon: Option<PathBuf>,
}

pub struct DrmProvider {
    cards: Vec<Card>,
    /// Previous (energy µJ, time) per card for power derived from energy counters.
    energy: HashMap<String, (u64, Instant)>,
}

impl DrmProvider {
    pub fn new() -> Self {
        DrmProvider {
            cards: enumerate(),
            energy: HashMap::new(),
        }
    }

    fn sample_card(&mut self, c: &Card) -> GpuSample {
        let dev = c.path.join("device");
        let mut s = GpuSample::empty(c.bus_id.clone(), c.vendor, c.name.clone());
        s.driver = c.driver.clone();
        s.driver_version = c.driver.as_deref().and_then(driver_version);
        s.power_state = read_trim(dev.join("power/runtime_status"));
        if s.power_state.as_deref() == Some("suspended") {
            return s;
        }

        match c.driver.as_deref() {
            Some("amdgpu") => {
                s.utilization = read_num(dev.join("gpu_busy_percent"));
                s.memory_utilization = read_num(dev.join("mem_busy_percent"));
                s.vram_used = read_num(dev.join("mem_info_vram_used"));
                s.vram_total = read_num(dev.join("mem_info_vram_total"));
                s.core_clock_mhz = active_dpm_mhz(&dev.join("pp_dpm_sclk"));
                s.max_core_clock_mhz = max_dpm_mhz(&dev.join("pp_dpm_sclk"));
                s.memory_clock_mhz = active_dpm_mhz(&dev.join("pp_dpm_mclk"));
                s.pcie_generation =
                    read_trim(dev.join("current_link_speed")).and_then(|v| pcie_gen(&v));
                s.pcie_width = read_num(dev.join("current_link_width"));
            }
            Some("i915") => {
                s.core_clock_mhz = read_num(c.path.join("gt_cur_freq_mhz"))
                    .or_else(|| read_num(c.path.join("gt_act_freq_mhz")));
                s.max_core_clock_mhz = read_num(c.path.join("gt_max_freq_mhz"));
            }
            Some("xe") => {
                let f = dev.join("tile0/gt0/freq0");
                s.core_clock_mhz =
                    read_num(f.join("act_freq")).or_else(|| read_num(f.join("cur_freq")));
                s.max_core_clock_mhz = read_num(f.join("max_freq"));
            }
            _ => {}
        }

        if let Some(h) = &c.hwmon {
            s.temperature = read_num::<f32>(h.join("temp1_input")).map(|m| m / 1000.0);
            s.power_watts = read_num::<f64>(h.join("power1_average"))
                .or_else(|| read_num::<f64>(h.join("power1_input")))
                .map(|uw| (uw / 1e6) as f32)
                .or_else(|| self.power_from_energy(&c.bus_id, h));
            s.power_limit_watts = read_num::<f64>(h.join("power1_cap"))
                .or_else(|| read_num::<f64>(h.join("power1_max")))
                .map(|uw| (uw / 1e6) as f32)
                .filter(|w| *w > 0.0);
            if let (Some(pwm), Some(max)) = (
                read_num::<u32>(h.join("pwm1")),
                read_num::<u32>(h.join("pwm1_max")).or(Some(255)),
            ) {
                s.fan_percent = (pwm * 100).checked_div(max);
            }
        }
        s
    }

    /// Intel discrete GPUs only expose a cumulative energy counter.
    fn power_from_energy(&mut self, id: &str, hwmon: &Path) -> Option<f32> {
        let now_uj: u64 = read_num(hwmon.join("energy1_input"))?;
        let now = Instant::now();
        let prev = self.energy.insert(id.to_string(), (now_uj, now));
        let (prev_uj, prev_t) = prev?;
        let dt = now.duration_since(prev_t).as_secs_f64();
        (dt > 0.0 && now_uj >= prev_uj).then(|| ((now_uj - prev_uj) as f64 / 1e6 / dt) as f32)
    }
}

impl GpuProvider for DrmProvider {
    fn name(&self) -> &'static str {
        "drm-sysfs"
    }

    fn sample(&mut self) -> Vec<GpuSample> {
        let cards = std::mem::take(&mut self.cards);
        let out = cards.iter().map(|c| self.sample_card(c)).collect();
        self.cards = cards;
        out
    }
}

/// Find DRM primary nodes (`card0`, `card1`, ...) backed by a PCI device.
fn enumerate() -> Vec<Card> {
    let Ok(rd) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut cards: Vec<Card> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n = name.strip_prefix("card")?;
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let path = e.path();
            let dev = path.join("device");
            let vendor_id = read_hex(dev.join("vendor"))?;
            let device_id = read_hex(dev.join("device")).unwrap_or(0);
            let bus_id = fs::canonicalize(&dev)
                .ok()?
                .file_name()?
                .to_string_lossy()
                .into_owned();
            let name = read_trim(dev.join("product_name"))
                .unwrap_or_else(|| gpu_name(vendor_id, device_id));
            let hwmon = fs::read_dir(dev.join("hwmon"))
                .ok()
                .and_then(|mut r| r.find_map(Result::ok))
                .map(|e| e.path());
            Some(Card {
                path,
                bus_id: bus_id.to_ascii_lowercase(),
                vendor: GpuVendor::from_pci(vendor_id),
                name,
                driver: link_name(dev.join("driver")),
                hwmon,
            })
        })
        .collect();
    cards.sort_by(|a, b| a.bus_id.cmp(&b.bus_id));
    cards.dedup_by(|a, b| a.bus_id == b.bus_id);
    cards
}

pub fn gpu_name(vendor: u16, device: u16) -> String {
    let db = pci_ids();
    let v = db
        .vendor(vendor)
        .map(short_vendor)
        .unwrap_or_else(|| format!("{vendor:04x}"));
    match db.device(vendor, device) {
        Some(d) => format!("{v} {}", marketing_name(d)),
        None => format!("{v} GPU {device:04x}"),
    }
}

/// Version of a loadable kernel module, or the kernel version for in-tree drivers.
pub fn driver_version(driver: &str) -> Option<String> {
    read_trim(format!("/sys/module/{driver}/version"))
        .or_else(|| read_trim("/proc/sys/kernel/osrelease").map(|k| format!("in-kernel ({k})")))
}

/// amdgpu DPM tables look like "0: 500Mhz\n1: 1800Mhz *".
fn active_dpm_mhz(path: &Path) -> Option<u32> {
    let t = fs::read_to_string(path).ok()?;
    t.lines()
        .find(|l| l.trim_end().ends_with('*'))
        .and_then(parse_mhz)
}

fn max_dpm_mhz(path: &Path) -> Option<u32> {
    let t = fs::read_to_string(path).ok()?;
    t.lines().filter_map(parse_mhz).max()
}

fn parse_mhz(line: &str) -> Option<u32> {
    let v = line.split_whitespace().nth(1)?;
    v.to_ascii_lowercase().trim_end_matches("mhz").parse().ok()
}

/// "16.0 GT/s PCIe" -> 4
fn pcie_gen(speed: &str) -> Option<u32> {
    let gts: f32 = speed.split_whitespace().next()?.parse().ok()?;
    Some(match gts {
        x if x >= 64.0 => 6,
        x if x >= 32.0 => 5,
        x if x >= 16.0 => 4,
        x if x >= 8.0 => 3,
        x if x >= 5.0 => 2,
        _ => 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dpm_lines() {
        assert_eq!(parse_mhz("1: 1800Mhz *"), Some(1800));
        assert_eq!(parse_mhz("0: 500Mhz"), Some(500));
    }

    #[test]
    fn pcie_generation_from_speed() {
        assert_eq!(pcie_gen("16.0 GT/s PCIe"), Some(4));
        assert_eq!(pcie_gen("8.0 GT/s PCIe"), Some(3));
    }
}
