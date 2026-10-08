// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/software/flatpak.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Flatpak applications via the `flatpak` CLI (system and user
//           installations). Silently empty when Flatpak is not installed.
// =============================================================================

use super::{run, Package, PackageSource, RemovalPlan};
use crate::error::Result;

/// "1.4 GB", "560.2 MB", "12 kB" (decimal units, as printed by flatpak).
fn parse_size(s: &str) -> Option<u64> {
    let s = s.replace('\u{a0}', " ");
    let mut it = s.split_whitespace();
    let n: f64 = it.next()?.replace(',', ".").parse().ok()?;
    let mult = match it.next().unwrap_or("B").to_ascii_lowercase().as_str() {
        "b" | "bytes" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        _ => return None,
    };
    Some((n * mult) as u64)
}

fn installed() -> Vec<Package> {
    let cols = "application,name,version,branch,installation,size,origin,description";
    let Ok(out) = run("flatpak", &["list", "--app", &format!("--columns={cols}")]) else {
        return Vec::new();
    };
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 8 {
                return None;
            }
            let app = f[0].to_string();
            Some(Package {
                id: format!("flatpak:{app}:{}", f[4]),
                source: PackageSource::Flatpak,
                display_name: f[1].to_string(),
                version: if f[2].is_empty() {
                    f[3].to_string()
                } else {
                    f[2].to_string()
                },
                summary: f[7].to_string(),
                installed_size: parse_size(f[5]),
                install_date: None,
                section: None,
                arch: None,
                publisher: Some(f[6].to_string()),
                homepage: None,
                is_app: true,
                icon: Some(app.clone()),
                protected: false,
                auto_installed: false,
                installation: Some(f[4].to_string()),
                name: app,
            })
        })
        .collect()
}

pub fn list() -> Vec<Package> {
    installed()
}

pub fn plan_removal(names: &[String]) -> Result<RemovalPlan> {
    let all = installed();
    let mut freed = 0;
    let mut needs_admin = false;
    let mut blockers = Vec::new();
    for n in names {
        match all.iter().find(|p| &p.name == n) {
            Some(p) => {
                freed += p.installed_size.unwrap_or(0);
                needs_admin |= p.installation.as_deref() != Some("user");
            }
            None => blockers.push(format!("{n} is not installed")),
        }
    }
    Ok(RemovalPlan {
        source: PackageSource::Flatpak,
        requested: names.to_vec(),
        removes: names.to_vec(),
        freed_bytes: freed,
        needs_admin,
        blockers,
        warnings: vec![
            "Shared runtimes stay installed; remove unused ones in Cache Cleaner".into(),
        ],
    })
}

pub fn remove(names: &[String], system: bool) -> Result<String> {
    let mut args = vec![
        "uninstall",
        "-y",
        "--noninteractive",
        if system { "--system" } else { "--user" },
    ];
    args.extend(names.iter().map(String::as_str));
    run("flatpak", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("1.5 GB"), Some(1_500_000_000));
        assert_eq!(parse_size("12 kB"), Some(12_000));
        assert_eq!(parse_size("junk"), None);
    }
}
