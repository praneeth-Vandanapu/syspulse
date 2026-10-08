// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/software/apt.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: APT / dpkg packages. Reads /var/lib/dpkg/status directly (fast,
//           no subprocess), maps packages to desktop launchers through the
//           dpkg file lists, and plans removals with `apt-get -s`.
// =============================================================================

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use super::{desktop, run, Package, PackageSource, RemovalPlan};
use crate::error::Result;

const STATUS: &str = "/var/lib/dpkg/status";
const INFO_DIR: &str = "/var/lib/dpkg/info";
const EXTENDED: &str = "/var/lib/apt/extended_states";

/// Packages whose removal would cripple the desktop session.
const CRITICAL: [&str; 9] = [
    "ubuntu-desktop",
    "ubuntu-desktop-minimal",
    "ubuntu-minimal",
    "ubuntu-standard",
    "gnome-shell",
    "gdm3",
    "systemd",
    "network-manager",
    "apt",
];

#[derive(Debug, Default, Clone)]
pub struct DpkgEntry {
    pub name: String,
    pub arch: String,
    pub version: String,
    pub installed_kib: Option<u64>,
    pub section: Option<String>,
    pub priority: Option<String>,
    pub essential: bool,
    pub summary: String,
    pub homepage: Option<String>,
    pub maintainer: Option<String>,
}

/// Parse installed packages from the dpkg status database.
pub fn read_status() -> Vec<DpkgEntry> {
    let Ok(text) = fs::read_to_string(STATUS) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for para in text.split("\n\n") {
        let mut e = DpkgEntry::default();
        let mut installed = false;
        for line in para.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                continue; // continuation lines (long description, conffiles)
            }
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let v = v.trim();
            match k {
                "Package" => e.name = v.to_string(),
                "Status" => installed = v.ends_with(" installed"),
                "Version" => e.version = v.to_string(),
                "Architecture" => e.arch = v.to_string(),
                "Installed-Size" => e.installed_kib = v.parse().ok(),
                "Section" => e.section = Some(v.to_string()),
                "Priority" => e.priority = Some(v.to_string()),
                "Essential" => e.essential = v == "yes",
                "Description" => e.summary = v.to_string(),
                "Homepage" => e.homepage = Some(v.to_string()),
                "Maintainer" => {
                    e.maintainer = Some(v.split('<').next().unwrap_or(v).trim().to_string())
                }
                _ => {}
            }
        }
        if installed && !e.name.is_empty() {
            out.push(e);
        }
    }
    out
}

fn auto_installed() -> HashSet<String> {
    let Ok(text) = fs::read_to_string(EXTENDED) else {
        return HashSet::new();
    };
    text.split("\n\n")
        .filter(|p| p.lines().any(|l| l.trim() == "Auto-Installed: 1"))
        .filter_map(|p| {
            p.lines()
                .find_map(|l| l.strip_prefix("Package: "))
                .map(|s| s.trim().to_string())
        })
        .collect()
}

struct ListInfo {
    desktop_files: Vec<String>,
    mtime: Option<u64>,
}

/// Scan dpkg file lists once: install time (list mtime) and launchers per package.
fn list_info() -> HashMap<String, ListInfo> {
    let mut map = HashMap::new();
    let Ok(rd) = fs::read_dir(INFO_DIR) else {
        return map;
    };
    for e in rd.filter_map(std::result::Result::ok) {
        let fname = e.file_name().to_string_lossy().into_owned();
        let Some(stem) = fname.strip_suffix(".list") else {
            continue;
        };
        let pkg = stem.split(':').next().unwrap_or(stem).to_string();
        let mtime = e
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let desktop_files = fs::read_to_string(e.path())
            .map(|t| {
                t.lines()
                    .filter(|l| {
                        l.starts_with("/usr/share/applications/") && l.ends_with(".desktop")
                    })
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        map.insert(
            pkg,
            ListInfo {
                desktop_files,
                mtime,
            },
        );
    }
    map
}

pub fn list() -> Vec<Package> {
    let status = read_status();
    let auto = auto_installed();
    let lists = list_info();
    status
        .into_iter()
        .map(|e| {
            let info = lists.get(&e.name);
            let entry = info
                .into_iter()
                .flat_map(|i| i.desktop_files.iter())
                .filter_map(|f| desktop::read(Path::new(f)))
                .find(|d| d.visible);
            let protected = e.essential
                || matches!(e.priority.as_deref(), Some("required") | Some("important"))
                || CRITICAL.contains(&e.name.as_str());
            Package {
                id: format!("apt:{}:{}", e.name, e.arch),
                source: PackageSource::Apt,
                display_name: entry
                    .as_ref()
                    .and_then(|d| d.name.clone())
                    .unwrap_or_else(|| e.name.clone()),
                summary: entry
                    .as_ref()
                    .and_then(|d| d.comment.clone())
                    .unwrap_or_else(|| e.summary.clone()),
                icon: entry.as_ref().and_then(|d| d.icon.clone()),
                is_app: entry.is_some(),
                name: e.name.clone(),
                version: e.version,
                installed_size: e.installed_kib.map(|k| k * 1024),
                install_date: info.and_then(|i| i.mtime),
                section: e.section,
                arch: Some(e.arch),
                publisher: e.maintainer,
                homepage: e.homepage,
                protected,
                auto_installed: auto.contains(&e.name),
                installation: None,
            }
        })
        .collect()
}

/// Parse `apt-get -s` output: "Remv pkg [version]" / "Purg pkg [...]".
fn simulated_removals(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|l| l.strip_prefix("Remv ").or_else(|| l.strip_prefix("Purg ")))
        .filter_map(|r| r.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

pub fn plan_removal(names: &[String]) -> Result<RemovalPlan> {
    let mut args = vec!["-s", "remove"];
    args.extend(names.iter().map(String::as_str));
    let out = run("apt-get", &args)?;
    let removes = simulated_removals(&out);
    let status: HashMap<String, DpkgEntry> = read_status()
        .into_iter()
        .map(|e| (e.name.clone(), e))
        .collect();
    let base = |n: &str| n.split(':').next().unwrap_or(n).to_string();

    let freed_bytes = removes
        .iter()
        .filter_map(|n| status.get(&base(n)).and_then(|e| e.installed_kib))
        .sum::<u64>()
        * 1024;

    let running_kernel = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let running_kernel = running_kernel.trim();
    let mut blockers = Vec::new();
    for n in &removes {
        let b = base(n);
        let e = status.get(&b);
        if e.map(|e| e.essential).unwrap_or(false) {
            blockers.push(format!("{b} is an essential system package"));
        } else if e
            .and_then(|e| e.priority.as_deref())
            .is_some_and(|p| p == "required")
        {
            blockers.push(format!("{b} is a required system package"));
        } else if CRITICAL.contains(&b.as_str()) {
            blockers.push(format!("{b} is needed by the desktop session"));
        } else if !running_kernel.is_empty() && b.contains(running_kernel) {
            blockers.push(format!("{b} belongs to the running kernel"));
        }
    }
    let mut warnings = Vec::new();
    let extra: Vec<&String> = removes
        .iter()
        .filter(|r| !names.iter().any(|n| base(n) == base(r)))
        .collect();
    if !extra.is_empty() {
        warnings.push(format!(
            "{} dependent package(s) will also be removed",
            extra.len()
        ));
    }
    Ok(RemovalPlan {
        source: PackageSource::Apt,
        requested: names.to_vec(),
        removes,
        freed_bytes,
        needs_admin: true,
        blockers,
        warnings,
    })
}

pub fn remove(names: &[String], purge: bool) -> Result<String> {
    let mut args = vec!["-y", if purge { "purge" } else { "remove" }];
    args.extend(names.iter().map(String::as_str));
    run("apt-get", &args)
}

/// Packages `apt-get autoremove` would remove and the space they use.
pub fn autoremovable() -> (Vec<String>, u64) {
    let Ok(out) = run("apt-get", &["-s", "autoremove"]) else {
        return (Vec::new(), 0);
    };
    let names = simulated_removals(&out);
    let status: HashMap<String, u64> = read_status()
        .into_iter()
        .filter_map(|e| Some((e.name, e.installed_kib?)))
        .collect();
    let bytes = names
        .iter()
        .filter_map(|n| status.get(n.split(':').next().unwrap_or(n)))
        .sum::<u64>()
        * 1024;
    (names, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simulation() {
        let out = "NOTE: x\nRemv foo [1.0]\nRemv libbar:amd64 [2]\nPurg baz [3]\n";
        assert_eq!(simulated_removals(out), vec!["foo", "libbar:amd64", "baz"]);
    }

    #[test]
    fn reads_dpkg_status() {
        let s = read_status();
        assert!(s.iter().any(|e| e.name == "dpkg" && e.essential));
    }
}
