// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/examples/dump.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Debug tool - print one telemetry sample and the hardware inventory
//           as JSON.   cargo run -p syspulse-core --example dump [hw]
// =============================================================================

use syspulse_core::{gpu::GpuManager, hardware, monitor::SystemSampler};

fn main() {
    let mut sampler = SystemSampler::new();
    let mut gpus = GpuManager::new();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let mut t = sampler.sample();
    t.gpus = gpus.sample();
    if std::env::args().nth(1).as_deref() == Some("hw") {
        println!(
            "{}",
            serde_json::to_string_pretty(&hardware::collect(&t.gpus)).unwrap()
        );
    } else {
        println!("{}", serde_json::to_string_pretty(&t).unwrap());
    }
}
