// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/gpu/mod.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: GPU monitoring front door. Combines vendor providers:
//             * NVIDIA  -> NVML (nvidia.rs)
//             * AMD/Intel/others -> Linux DRM sysfs (drm.rs)
//           Every GPU appears once, keyed by its PCI bus id.
// =============================================================================

#[cfg(target_os = "linux")]
mod drm;
mod nvidia;

use crate::models::GpuSample;

/// A source of GPU samples. Implementations must be cheap to call at 1 Hz
/// and must never panic when hardware or drivers are missing.
pub trait GpuProvider: Send {
    fn name(&self) -> &'static str;
    /// Sample every GPU this provider can see.
    fn sample(&mut self) -> Vec<GpuSample>;
}

/// Normalise a PCI bus id to `dddd:bb:dd.f` in lowercase.
/// NVML reports `00000000:01:00.0`; sysfs reports `0000:01:00.0`.
pub fn normalize_bus_id(id: &str) -> String {
    let id = id.to_ascii_lowercase();
    match id.split_once(':') {
        Some((domain, rest)) if domain.len() > 4 => {
            format!("{}:{}", &domain[domain.len() - 4..], rest)
        }
        _ => id,
    }
}

pub struct GpuManager {
    nvidia: Option<nvidia::NvidiaProvider>,
    #[cfg(target_os = "linux")]
    drm: drm::DrmProvider,
}

impl Default for GpuManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuManager {
    pub fn new() -> Self {
        let nvidia = nvidia::NvidiaProvider::new();
        if nvidia.is_none() {
            log::info!("NVML not available; NVIDIA GPUs (if any) use the basic sysfs view");
        }
        GpuManager {
            nvidia,
            #[cfg(target_os = "linux")]
            drm: drm::DrmProvider::new(),
        }
    }

    /// Driver version reported by NVML (used by the hardware inventory).
    pub fn nvidia_driver_version(&self) -> Option<String> {
        self.nvidia.as_ref().and_then(|n| n.driver_version())
    }

    pub fn sample(&mut self) -> Vec<GpuSample> {
        let mut out: Vec<GpuSample> = Vec::new();
        if let Some(nv) = self.nvidia.as_mut() {
            out.extend(nv.sample());
        }
        #[cfg(target_os = "linux")]
        {
            for g in self.drm.sample() {
                match out.iter_mut().find(|o| o.id == g.id) {
                    // NVML already covers it: borrow the runtime PM state from sysfs.
                    Some(existing) => {
                        if existing.power_state.is_none() {
                            existing.power_state = g.power_state;
                        }
                    }
                    None => out.push(g),
                }
            }
        }
        // Discrete GPUs first, then by bus id for a stable order.
        out.sort_by(|a, b| {
            let rank = |g: &GpuSample| matches!(g.vendor, crate::models::GpuVendor::Intel) as u8;
            rank(a).cmp(&rank(b)).then_with(|| a.id.cmp(&b.id))
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_ids_normalise() {
        assert_eq!(normalize_bus_id("00000000:01:00.0"), "0000:01:00.0");
        assert_eq!(normalize_bus_id("0000:00:02.0"), "0000:00:02.0");
        assert_eq!(normalize_bus_id("00000000:0A:00.0"), "0000:0a:00.0");
    }

    #[test]
    fn manager_never_panics() {
        let mut m = GpuManager::new();
        let _ = m.sample();
    }
}
