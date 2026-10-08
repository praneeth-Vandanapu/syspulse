// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/hardware/mod.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Hardware inventory: every onboard device with its configuration,
//           driver and firmware/version numbers, grouped into sections.
//           Linux reads sysfs/procfs (no root needed); Windows will use WMI.
// =============================================================================

#[cfg(target_os = "linux")]
mod linux;

use crate::models::{GpuSample, HwSection};

/// Collect the full inventory. `gpus` is the latest GPU sample, used to enrich
/// the graphics section with VRAM size and driver versions.
pub fn collect(gpus: &[GpuSample]) -> Vec<HwSection> {
    #[cfg(target_os = "linux")]
    {
        linux::collect(gpus)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = gpus;
        Vec::new()
    }
}
