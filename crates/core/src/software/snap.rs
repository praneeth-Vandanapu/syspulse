// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/software/snap.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Snap packages via the snapd REST API on /run/snapd.socket (read
//           access needs no privileges), plus old disabled revisions for the
//           cache cleaner.
// =============================================================================

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::{desktop, run, Package, PackageSource, RemovalPlan};
use crate::error::{CoreError, Result};

const SOCKET: &str = "/run/snapd.socket";

/// Minimal HTTP/1.0 GET over the snapd unix socket.
fn get(path: &str) -> Option<Value> {
    let mut s = UnixStream::connect(SOCKET).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    write!(s, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").ok()?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let body = text.split_once("\r\n\r\n")?.1;
    serde_json::from_str(body).ok()
}

/// Parse an RFC 3339 timestamp ("2025-10-14T12:39:52.64+05:30") to Unix seconds.
pub(crate) fn parse_rfc3339(s: &str) -> Option<u64> {
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let hms: Vec<i64> = rest
        .get(..8)?
        .split(':')
        .filter_map(|x| x.parse().ok())
        .collect();
    if hms.len() != 3 {
        return None;
    }
    // Timezone offset
    let tz = rest[8..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match tz.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let mut p = tz[1..].split(':').filter_map(|x| x.parse::<i64>().ok());
            let secs = p.next().unwrap_or(0) * 3600 + p.next().unwrap_or(0) * 60;
            if sign == '+' {
                secs
            } else {
                -secs
            }
        }
        _ => 0,
    };
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let t = days * 86_400 + hms[0] * 3600 + hms[1] * 60 + hms[2] - offset;
    u64::try_from(t).ok()
}

pub fn list() -> Vec<Package> {
    let Some(v) = get("/v2/snaps") else {
        return Vec::new();
    };
    let Some(items) = v.get("result").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|s| {
            let name = s.get("name")?.as_str()?.to_string();
            let kind = s.get("type").and_then(Value::as_str).unwrap_or("app");
            let entry = s
                .get("apps")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|a| a.get("desktop-file").and_then(Value::as_str))
                .filter_map(|f| desktop::read(Path::new(f)))
                .find(|d| d.visible);
            let str_of = |k: &str| s.get(k).and_then(Value::as_str).map(str::to_string);
            Some(Package {
                id: format!("snap:{name}"),
                source: PackageSource::Snap,
                display_name: entry
                    .as_ref()
                    .and_then(|d| d.name.clone())
                    .or_else(|| str_of("title"))
                    .unwrap_or_else(|| name.clone()),
                summary: str_of("summary").unwrap_or_default(),
                version: format!(
                    "{} (rev {})",
                    str_of("version").unwrap_or_default(),
                    str_of("revision").unwrap_or_default()
                ),
                installed_size: s.get("installed-size").and_then(Value::as_u64),
                install_date: str_of("install-date").and_then(|d| parse_rfc3339(&d)),
                section: Some(format!("snap/{kind}")),
                arch: None,
                publisher: s
                    .get("publisher")
                    .and_then(|p| p.get("display-name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                homepage: str_of("website"),
                is_app: entry.is_some(),
                icon: entry.as_ref().and_then(|d| d.icon.clone()),
                // Bases, kernels, gadgets and snapd itself keep the system running.
                protected: kind != "app",
                auto_installed: false,
                installation: None,
                name,
            })
        })
        .collect()
}

pub fn plan_removal(names: &[String]) -> Result<RemovalPlan> {
    let installed = list();
    let mut blockers = Vec::new();
    let mut freed = 0;
    for n in names {
        match installed.iter().find(|p| &p.name == n) {
            Some(p) if p.protected => {
                blockers.push(format!("{n} is a core snap used by the system"))
            }
            Some(p) => {
                freed += p.installed_size.unwrap_or(0)
                    + old_revisions_of(n).iter().map(|r| r.size).sum::<u64>()
            }
            None => blockers.push(format!("{n} is not installed")),
        }
    }
    Ok(RemovalPlan {
        source: PackageSource::Snap,
        requested: names.to_vec(),
        removes: names.to_vec(),
        freed_bytes: freed,
        needs_admin: true,
        blockers,
        warnings: vec!["A snapshot of the snap's data is kept for 31 days unless purged".into()],
    })
}

pub fn remove(names: &[String], purge: bool) -> Result<String> {
    let mut args = vec!["remove"];
    if purge {
        args.push("--purge");
    }
    args.extend(names.iter().map(String::as_str));
    run("snap", &args)
}

#[derive(Debug, Clone)]
pub struct OldRevision {
    pub name: String,
    pub revision: String,
    pub size: u64,
}

fn current_revision(name: &str) -> Option<String> {
    fs::read_link(format!("/snap/{name}/current"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

fn old_revisions_of(name: &str) -> Vec<OldRevision> {
    old_revisions()
        .into_iter()
        .filter(|r| r.name == name)
        .collect()
}

/// Disabled (non-current) revisions kept on disk by snapd.
pub fn old_revisions() -> Vec<OldRevision> {
    let Ok(rd) = fs::read_dir("/var/lib/snapd/snaps") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.filter_map(std::result::Result::ok) {
        let f = e.file_name().to_string_lossy().into_owned();
        let Some(stem) = f.strip_suffix(".snap") else {
            continue;
        };
        let Some((name, rev)) = stem.rsplit_once('_') else {
            continue;
        };
        if current_revision(name).as_deref() == Some(rev) {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(OldRevision {
            name: name.to_string(),
            revision: rev.to_string(),
            size,
        });
    }
    out
}

/// Remove all disabled revisions (root only). Returns bytes freed.
pub fn remove_old_revisions() -> Result<u64> {
    let mut freed = 0;
    for r in old_revisions() {
        if !super::valid_name(PackageSource::Snap, &r.name)
            || !r.revision.chars().all(|c| c.is_ascii_alphanumeric())
        {
            continue;
        }
        if run(
            "snap",
            &["remove", &r.name, &format!("--revision={}", r.revision)],
        )
        .is_ok()
        {
            freed += r.size;
        }
    }
    if freed == 0 && !old_revisions().is_empty() {
        return Err(CoreError::Other(
            "snapd refused to remove old revisions".into(),
        ));
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339("2025-10-14T12:39:52.64006808+05:30"),
            Some(1_760_425_792)
        );
    }
}
