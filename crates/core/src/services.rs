// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/services.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: systemd service manager. Lists system and user services (state,
//           enablement, preset), shows details and recent journal lines, and
//           starts / stops / restarts / enables / disables them. System-level
//           changes run in the privileged helper; user services run directly.
//           Services the desktop session depends on are protected.
// =============================================================================

use std::collections::HashMap;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub unit: String,
    pub description: String,
    /// loaded | not-found | masked | (absent when only the unit file exists)
    pub load: String,
    /// active | inactive | failed | activating | deactivating
    pub active: String,
    /// running | exited | dead | failed | ...
    pub sub: String,
    /// enabled | disabled | static | masked | indirect | generated | alias
    pub file_state: Option<String>,
    pub preset: Option<String>,
    pub user: bool,
    /// Stopping or disabling would break the session or boot.
    pub protected: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
}

impl ServiceAction {
    fn verb(self) -> &'static str {
        match self {
            ServiceAction::Start => "start",
            ServiceAction::Stop => "stop",
            ServiceAction::Restart => "restart",
            ServiceAction::Enable => "enable",
            ServiceAction::Disable => "disable",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDetails {
    pub unit: String,
    pub main_pid: Option<u32>,
    pub memory: Option<u64>,
    pub cpu_seconds: Option<f64>,
    pub tasks: Option<u64>,
    pub restarts: Option<u64>,
    pub since: Option<String>,
    pub fragment_path: Option<String>,
    pub properties: Vec<(String, String)>,
    pub logs: Vec<String>,
}

/// Units whose stop / restart / disable would end the desktop session or
/// break core system functions.
const PROTECTED: [&str; 13] = [
    "dbus.service",
    "dbus-broker.service",
    "systemd-journald.service",
    "systemd-logind.service",
    "systemd-udevd.service",
    "systemd-resolved.service",
    "systemd-timesyncd.service",
    "gdm.service",
    "gdm3.service",
    "display-manager.service",
    "polkit.service",
    "accounts-daemon.service",
    "NetworkManager.service",
];

pub fn is_protected(unit: &str) -> bool {
    PROTECTED.contains(&unit)
        || unit.starts_with("user@")
        || unit.starts_with("getty@")
        || unit.starts_with("systemd-")
}

/// `name.service`, `name@instance.service`; no paths, no options.
pub fn valid_unit(unit: &str) -> bool {
    unit.len() <= 256
        && unit.ends_with(".service")
        && !unit.starts_with('-')
        && unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._:-\\".contains(c))
}

fn systemctl(user: bool, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    let out = cmd
        .args(args)
        .args(["--no-pager", "--plain"])
        .env("LC_ALL", "C")
        .env("SYSTEMD_COLORS", "0")
        .output()
        .map_err(|e| CoreError::Other(format!("systemctl: {e}")))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(text)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(CoreError::Other(
            err.lines()
                .last()
                .unwrap_or("systemctl failed")
                .trim()
                .to_string(),
        ))
    }
}

fn json_list(user: bool, args: &[&str]) -> Vec<Value> {
    systemctl(user, args)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default()
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

/// List services. `user` = the per-user systemd instance.
pub fn list(user: bool) -> Vec<Service> {
    let units = json_list(
        user,
        &["list-units", "--type=service", "--all", "--output=json"],
    );
    let files = json_list(
        user,
        &["list-unit-files", "--type=service", "--output=json"],
    );
    let file_info: HashMap<String, (Option<String>, Option<String>)> = files
        .iter()
        .filter_map(|f| Some((s(f, "unit_file")?, (s(f, "state"), s(f, "vendor_preset")))))
        .collect();

    let mut out: HashMap<String, Service> = HashMap::new();
    for u in &units {
        let Some(unit) = s(u, "unit") else { continue };
        let (file_state, preset) = file_info.get(&unit).cloned().unwrap_or((None, None));
        out.insert(
            unit.clone(),
            Service {
                description: s(u, "description").unwrap_or_default(),
                load: s(u, "load").unwrap_or_default(),
                active: s(u, "active").unwrap_or_default(),
                sub: s(u, "sub").unwrap_or_default(),
                file_state,
                preset,
                user,
                protected: !user && is_protected(&unit),
                unit,
            },
        );
    }
    // Installed but never loaded (typically disabled) services.
    for (unit, (state, preset)) in file_info {
        if unit.ends_with("@.service") || out.contains_key(&unit) {
            continue;
        }
        out.insert(
            unit.clone(),
            Service {
                description: String::new(),
                load: "unloaded".into(),
                active: "inactive".into(),
                sub: "dead".into(),
                file_state: state,
                preset,
                user,
                protected: !user && is_protected(&unit),
                unit,
            },
        );
    }
    let mut v: Vec<Service> = out.into_values().collect();
    v.sort_by_key(|s| s.unit.to_lowercase());
    v
}

const PROPS: [&str; 16] = [
    "Description",
    "LoadState",
    "ActiveState",
    "SubState",
    "UnitFileState",
    "MainPID",
    "MemoryCurrent",
    "CPUUsageNSec",
    "TasksCurrent",
    "NRestarts",
    "ActiveEnterTimestamp",
    "FragmentPath",
    "Type",
    "Restart",
    "User",
    "Documentation",
];

pub fn details(unit: &str, user: bool) -> Result<ServiceDetails> {
    if !valid_unit(unit) {
        return Err(CoreError::Refused(format!("invalid unit name {unit}")));
    }
    let mut args = vec!["show", unit];
    let prop_args: Vec<String> = PROPS.iter().map(|p| format!("--property={p}")).collect();
    args.extend(prop_args.iter().map(String::as_str));
    let text = systemctl(user, &args)?;
    let map: HashMap<String, String> = text
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let num = |k: &str| {
        map.get(k)
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v != u64::MAX)
    };
    let logs = journal(unit, user, 80);
    Ok(ServiceDetails {
        unit: unit.to_string(),
        main_pid: num("MainPID").filter(|p| *p > 0).map(|p| p as u32),
        memory: num("MemoryCurrent"),
        cpu_seconds: num("CPUUsageNSec").map(|n| n as f64 / 1e9),
        tasks: num("TasksCurrent"),
        restarts: num("NRestarts"),
        since: map
            .get("ActiveEnterTimestamp")
            .filter(|v| !v.is_empty())
            .cloned(),
        fragment_path: map.get("FragmentPath").filter(|v| !v.is_empty()).cloned(),
        properties: PROPS
            .iter()
            .filter_map(|p| {
                map.get(*p)
                    .filter(|v| !v.is_empty())
                    .map(|v| (p.to_string(), v.clone()))
            })
            .collect(),
        logs,
    })
}

/// Recent journal lines (readable for members of `adm` / `systemd-journal`).
fn journal(unit: &str, user: bool, lines: u32) -> Vec<String> {
    let n = lines.to_string();
    let mut args = vec![
        if user { "--user-unit" } else { "-u" },
        unit,
        "-n",
        &n,
        "--no-pager",
        "-o",
        "short-iso",
    ];
    if user {
        args.insert(0, "--user");
    }
    Command::new("journalctl")
        .args(&args)
        .env("SYSTEMD_COLORS", "0")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Apply an action. System services must be changed as root (helper);
/// user services are changed directly.
pub fn apply(unit: &str, action: ServiceAction, user: bool) -> Result<String> {
    if !valid_unit(unit) {
        return Err(CoreError::Refused(format!("invalid unit name {unit}")));
    }
    if !user
        && is_protected(unit)
        && action != ServiceAction::Start
        && action != ServiceAction::Enable
    {
        return Err(CoreError::Refused(format!(
            "{unit} is required by the system and cannot be {}ped/disabled here",
            action.verb()
        )));
    }
    systemctl(user, &[action.verb(), unit])?;
    Ok(format!("{} {unit}", action.verb()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_validation() {
        assert!(valid_unit("ssh.service"));
        assert!(valid_unit("getty@tty1.service"));
        assert!(!valid_unit("ssh"));
        assert!(!valid_unit("--now.service"));
        assert!(!valid_unit("a b.service"));
        assert!(!valid_unit("../x.service"));
    }

    #[test]
    fn protected_units_refused() {
        assert!(matches!(
            apply("dbus.service", ServiceAction::Stop, false),
            Err(CoreError::Refused(_))
        ));
        assert!(is_protected("systemd-logind.service"));
        assert!(!is_protected("cups.service"));
    }

    #[test]
    fn lists_system_services() {
        let v = list(false);
        assert!(v.iter().any(|s| s.unit == "dbus.service" && s.protected));
    }
}
