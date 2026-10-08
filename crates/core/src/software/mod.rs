// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/software/mod.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Installed software inventory across package systems (APT/dpkg,
//           Snap, Flatpak), removal planning and removal.
// =============================================================================

#[cfg(target_os = "linux")]
mod apt;
#[cfg(target_os = "linux")]
mod desktop;
#[cfg(target_os = "linux")]
mod flatpak;
pub mod icons;
#[cfg(target_os = "linux")]
pub mod snap;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PackageSource {
    Apt,
    Snap,
    Flatpak,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    /// Unique key: "<source>:<name>[:<installation>]".
    pub id: String,
    pub source: PackageSource,
    /// Package / snap / application id used for removal.
    pub name: String,
    /// Human friendly name (from the .desktop file when available).
    pub display_name: String,
    pub version: String,
    pub summary: String,
    pub installed_size: Option<u64>,
    /// Unix seconds.
    pub install_date: Option<u64>,
    pub section: Option<String>,
    pub arch: Option<String>,
    pub publisher: Option<String>,
    pub homepage: Option<String>,
    /// Has a visible desktop launcher.
    pub is_app: bool,
    /// Icon name or absolute path (resolve with `icons::icon_data_url`).
    pub icon: Option<String>,
    /// Essential / required system component: uninstall is blocked.
    pub protected: bool,
    /// Installed automatically as a dependency (APT).
    pub auto_installed: bool,
    /// Flatpak installation ("system" or "user").
    pub installation: Option<String>,
}

/// What an uninstall will actually do, shown to the user before confirming.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalPlan {
    pub source: PackageSource,
    pub requested: Vec<String>,
    /// Every package that will be removed (requested + dependents).
    pub removes: Vec<String>,
    pub freed_bytes: u64,
    pub needs_admin: bool,
    /// Blocking problems: if non-empty the removal must not proceed.
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

/// Collect packages from every available package system.
pub fn list_packages() -> Vec<Package> {
    #[cfg(target_os = "linux")]
    {
        let (a, (s, f)) = std::thread::scope(|sc| {
            let a = sc.spawn(apt::list);
            let s = sc.spawn(snap::list);
            let f = sc.spawn(flatpak::list);
            (
                a.join().unwrap_or_default(),
                (s.join().unwrap_or_default(), f.join().unwrap_or_default()),
            )
        });
        let mut all = a;
        all.extend(s);
        all.extend(f);
        all
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// Validate a package name for the given source before it reaches a shell tool.
pub fn valid_name(source: PackageSource, name: &str) -> bool {
    let ok_chars = |extra: &str| {
        !name.is_empty()
            && name.len() <= 200
            && !name.starts_with('-')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
    };
    match source {
        PackageSource::Apt => {
            ok_chars("+-.:")
                && name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric())
        }
        PackageSource::Snap => ok_chars("-") && name.chars().all(|c| !c.is_ascii_uppercase()),
        PackageSource::Flatpak => ok_chars("._-") && name.contains('.'),
    }
}

pub fn plan_removal(source: PackageSource, names: &[String]) -> Result<RemovalPlan> {
    if let Some(bad) = names.iter().find(|n| !valid_name(source, n)) {
        return Err(CoreError::Refused(format!("invalid package name: {bad}")));
    }
    #[cfg(target_os = "linux")]
    {
        match source {
            PackageSource::Apt => apt::plan_removal(names),
            PackageSource::Snap => snap::plan_removal(names),
            PackageSource::Flatpak => flatpak::plan_removal(names),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(CoreError::Unsupported("package removal"))
    }
}

/// Remove packages. Must run as root for APT, Snap and system Flatpaks
/// (called from the privileged helper); user Flatpaks are removed directly.
pub fn remove(source: PackageSource, names: &[String], purge: bool) -> Result<String> {
    if names.is_empty() {
        return Err(CoreError::Refused("nothing to remove".into()));
    }
    if let Some(bad) = names.iter().find(|n| !valid_name(source, n)) {
        return Err(CoreError::Refused(format!("invalid package name: {bad}")));
    }
    #[cfg(target_os = "linux")]
    {
        // Re-check the plan as root: never remove protected packages.
        let plan = plan_removal(source, names)?;
        if !plan.blockers.is_empty() {
            return Err(CoreError::Refused(plan.blockers.join("; ")));
        }
        match source {
            PackageSource::Apt => apt::remove(names, purge),
            PackageSource::Snap => snap::remove(names, purge),
            PackageSource::Flatpak => flatpak::remove(names, true),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = purge;
        Err(CoreError::Unsupported("package removal"))
    }
}

/// Remove user-installation Flatpaks without admin rights.
pub fn remove_user_flatpaks(names: &[String]) -> Result<String> {
    if let Some(bad) = names
        .iter()
        .find(|n| !valid_name(PackageSource::Flatpak, n))
    {
        return Err(CoreError::Refused(format!("invalid application id: {bad}")));
    }
    #[cfg(target_os = "linux")]
    {
        flatpak::remove(names, false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(CoreError::Unsupported("flatpak"))
    }
}

/// Packages `apt-get autoremove` would remove, and the space they use.
#[cfg(target_os = "linux")]
pub fn apt_autoremovable() -> (Vec<String>, u64) {
    apt::autoremovable()
}

#[cfg(target_os = "linux")]
pub(crate) use run as run_tool;

/// Run a command and return combined output, mapping failures to errors.
#[cfg(target_os = "linux")]
pub(crate) fn run(cmd: &str, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new(cmd)
        .args(args)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| CoreError::Other(format!("{cmd}: {e}")))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(text)
    } else {
        let tail: Vec<&str> = text.lines().rev().take(6).collect();
        Err(CoreError::Other(format!(
            "{cmd} failed: {}",
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_validation() {
        assert!(valid_name(PackageSource::Apt, "libc6:amd64"));
        assert!(valid_name(PackageSource::Apt, "g++-12"));
        assert!(!valid_name(PackageSource::Apt, "-oDebug::x"));
        assert!(!valid_name(PackageSource::Apt, "a b"));
        assert!(!valid_name(PackageSource::Apt, "a;rm"));
        assert!(valid_name(PackageSource::Snap, "code-insiders"));
        assert!(!valid_name(PackageSource::Snap, "Code"));
        assert!(valid_name(PackageSource::Flatpak, "org.gimp.GIMP"));
        assert!(!valid_name(PackageSource::Flatpak, "gimp"));
    }

    #[test]
    fn lists_packages() {
        let p = list_packages();
        assert!(p.iter().any(|x| x.source == PackageSource::Apt));
    }
}
