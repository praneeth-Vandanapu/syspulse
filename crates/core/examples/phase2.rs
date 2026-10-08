// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/examples/phase2.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Debug tool for Phase 2 modules.
//           cargo run -p syspulse-core --example phase2 -- software|cleaner|large <dir>
// =============================================================================

use std::sync::atomic::AtomicBool;

use syspulse_core::{cleaner, largefiles, software, util::human_bytes};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    match args.get(1).map(String::as_str) {
        Some("software") => {
            let p = software::list_packages();
            let apps = p.iter().filter(|x| x.is_app).count();
            println!("{} packages ({apps} apps) in {:?}", p.len(), t.elapsed());
            for x in p.iter().filter(|x| x.is_app).take(12) {
                println!(
                    "  {:?} {:40} {:>10} icon={:?}",
                    x.source,
                    x.display_name,
                    x.installed_size.map(human_bytes).unwrap_or_default(),
                    x.icon
                );
            }
            if let Some(n) = args.get(2) {
                println!(
                    "{:#?}",
                    software::plan_removal(software::PackageSource::Apt, std::slice::from_ref(n))
                );
            }
        }
        Some("cleaner") => {
            let home = std::env::var("HOME").unwrap();
            for c in cleaner::scan(std::path::Path::new(&home)) {
                println!(
                    "{:10?} {:28} {:>10} admin={} {:?}",
                    c.category,
                    c.title,
                    human_bytes(c.size),
                    c.needs_admin,
                    c.paths
                );
            }
            println!("scan took {:?}", t.elapsed());
        }
        Some("large") => {
            let opts = largefiles::ScanOptions {
                root: args
                    .get(2)
                    .cloned()
                    .unwrap_or_else(|| std::env::var("HOME").unwrap()),
                min_size: 100 * 1024 * 1024,
                include_hidden: true,
                same_filesystem: true,
                categories: vec![],
                max_results: 15,
            };
            let r = largefiles::scan(&opts, &AtomicBool::new(false), |_| {}).unwrap();
            println!(
                "{} files, {} in {} ms",
                r.total_files,
                human_bytes(r.total_bytes),
                r.elapsed_ms
            );
            for f in &r.files {
                println!("  {:>10} {:?} {}", human_bytes(f.size), f.category, f.path);
            }
            for f in r.folders.iter().take(8) {
                println!("  DIR {:>10} {}", human_bytes(f.size), f.path);
            }
        }
        _ => eprintln!("usage: phase2 software|cleaner|large [dir]"),
    }
}
