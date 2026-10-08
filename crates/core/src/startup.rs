// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/startup.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Startup applications (XDG autostart). Merges system entries in
//           /etc/xdg/autostart with the user's ~/.config/autostart overrides,
//           and enables / disables / adds / edits / removes entries the same
//           way GNOME "Startup Applications" does - only ever writing to the
//           user's own autostart folder.
// =============================================================================

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

const SYSTEM_DIR: &str = "/etc/xdg/autostart";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupEntry {
    /// File name, e.g. `org.gnome.Evolution-alarm-notify.desktop`.
    pub id: String,
    pub name: String,
    pub comment: String,
    pub exec: String,
    pub icon: Option<String>,
    pub enabled: bool,
    /// Provided by an installed package (/etc/xdg/autostart).
    pub system: bool,
    /// The user has a copy in ~/.config/autostart.
    pub user_file: bool,
    /// Hidden from GNOME's own tool (NoDisplay=true): usually desktop plumbing.
    pub no_display: bool,
    /// Runs in this desktop (OnlyShowIn / NotShowIn).
    pub applies_here: bool,
    pub delay: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryInput {
    /// Existing id to edit; `None` creates a new entry.
    pub id: Option<String>,
    pub name: String,
    pub exec: String,
    #[serde(default)]
    pub comment: String,
    pub delay: Option<u32>,
}

/// Ordered key/value view of the [Desktop Entry] group (other groups kept verbatim).
struct DesktopFile {
    lines: Vec<String>,
}

impl DesktopFile {
    fn parse(text: &str) -> Self {
        DesktopFile {
            lines: text.lines().map(str::to_string).collect(),
        }
    }

    /// Range of lines belonging to [Desktop Entry].
    fn main_range(&self) -> (usize, usize) {
        let start = self
            .lines
            .iter()
            .position(|l| l.trim() == "[Desktop Entry]")
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = self.lines[start..]
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .map(|i| start + i)
            .unwrap_or(self.lines.len());
        (start, end)
    }

    fn get(&self, key: &str) -> Option<String> {
        let (a, b) = self.main_range();
        self.lines[a..b].iter().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    }

    fn set(&mut self, key: &str, value: &str) {
        let value = value.replace(['\n', '\r'], " ");
        let (a, b) = self.main_range();
        if let Some(i) = (a..b).find(|&i| {
            self.lines[i]
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
        }) {
            self.lines[i] = format!("{key}={value}");
        } else {
            if a == 0 && !self.lines.iter().any(|l| l.trim() == "[Desktop Entry]") {
                self.lines.insert(0, "[Desktop Entry]".into());
                return self.set(key, &value);
            }
            self.lines.insert(b, format!("{key}={value}"));
        }
    }

    fn render(&self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }
}

fn is_true(v: Option<String>) -> bool {
    v.is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn entry_from(id: &str, f: &DesktopFile, system: bool, user_file: bool) -> StartupEntry {
    let desktops = current_desktops();
    let list = |k: &str| {
        f.get(k).map(|v| {
            v.split(';')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
    };
    let applies_here = match (list("OnlyShowIn"), list("NotShowIn")) {
        (Some(only), _) => desktops.is_empty() || only.iter().any(|d| desktops.contains(d)),
        (None, Some(not)) => !not.iter().any(|d| desktops.contains(d)),
        _ => true,
    };
    let enabled = !is_true(f.get("Hidden"))
        && !f
            .get("X-GNOME-Autostart-enabled")
            .is_some_and(|v| v.eq_ignore_ascii_case("false"));
    StartupEntry {
        id: id.to_string(),
        name: f
            .get("Name")
            .unwrap_or_else(|| id.trim_end_matches(".desktop").to_string()),
        comment: f.get("Comment").unwrap_or_default(),
        exec: f.get("Exec").unwrap_or_default(),
        icon: f.get("Icon"),
        enabled,
        system,
        user_file,
        no_display: is_true(f.get("NoDisplay")),
        applies_here,
        delay: f
            .get("X-GNOME-Autostart-Delay")
            .and_then(|d| d.parse().ok()),
    }
}

fn user_dir(home: &Path) -> PathBuf {
    home.join(".config/autostart")
}

fn valid_id(id: &str) -> bool {
    id.ends_with(".desktop")
        && id.len() <= 200
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-@+".contains(c))
}

fn read(path: &Path) -> Option<DesktopFile> {
    fs::read_to_string(path)
        .ok()
        .map(|t| DesktopFile::parse(&t))
}

pub fn list(home: &Path) -> Vec<StartupEntry> {
    let names = |dir: &Path| -> Vec<String> {
        fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(std::result::Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.ends_with(".desktop"))
                    .collect()
            })
            .unwrap_or_default()
    };
    let sys_dir = Path::new(SYSTEM_DIR);
    let udir = user_dir(home);
    let mut ids = names(sys_dir);
    ids.extend(names(&udir));
    ids.sort();
    ids.dedup();
    let mut out: Vec<StartupEntry> = ids
        .iter()
        .filter_map(|id| {
            let system = sys_dir.join(id).is_file();
            let user_file = udir.join(id).is_file();
            // The user's copy overrides the system one.
            let f = if user_file {
                read(&udir.join(id))?
            } else {
                read(&sys_dir.join(id))?
            };
            Some(entry_from(id, &f, system, user_file))
        })
        .collect();
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// Load the user's copy, creating it from the system entry when needed.
fn user_copy(home: &Path, id: &str) -> Result<DesktopFile> {
    let u = user_dir(home).join(id);
    if let Some(f) = read(&u) {
        return Ok(f);
    }
    read(&Path::new(SYSTEM_DIR).join(id)).ok_or_else(|| CoreError::Other(format!("{id} not found")))
}

fn write_user(home: &Path, id: &str, f: &DesktopFile) -> Result<()> {
    let dir = user_dir(home);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(id), f.render())?;
    Ok(())
}

pub fn set_enabled(home: &Path, id: &str, enabled: bool) -> Result<()> {
    if !valid_id(id) {
        return Err(CoreError::Refused(format!("invalid entry {id}")));
    }
    let mut f = user_copy(home, id)?;
    f.set(
        "X-GNOME-Autostart-enabled",
        if enabled { "true" } else { "false" },
    );
    if enabled && is_true(f.get("Hidden")) {
        f.set("Hidden", "false");
    }
    write_user(home, id, &f)
}

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "startup-entry".into()
    } else {
        s.chars().take(60).collect()
    }
}

/// Create or edit an entry. Returns its id.
pub fn save(home: &Path, input: &EntryInput) -> Result<String> {
    let name = input.name.trim();
    let exec = input.exec.trim();
    if name.is_empty() || exec.is_empty() {
        return Err(CoreError::Refused("name and command are required".into()));
    }
    let (id, mut f) = match &input.id {
        Some(id) => {
            if !valid_id(id) {
                return Err(CoreError::Refused(format!("invalid entry {id}")));
            }
            (id.clone(), user_copy(home, id)?)
        }
        None => {
            let base = slug(name);
            let dir = user_dir(home);
            let mut id = format!("{base}.desktop");
            let mut n = 2;
            while dir.join(&id).exists() || Path::new(SYSTEM_DIR).join(&id).exists() {
                id = format!("{base}-{n}.desktop");
                n += 1;
            }
            let mut f = DesktopFile::parse("[Desktop Entry]\nType=Application\nX-GNOME-Autostart-enabled=true\nX-SysPulse-Created=true");
            f.set("Terminal", "false");
            (id, f)
        }
    };
    f.set("Name", name);
    f.set("Exec", exec);
    f.set("Comment", input.comment.trim());
    match input.delay {
        Some(d) if d > 0 => f.set("X-GNOME-Autostart-Delay", &d.to_string()),
        _ => {}
    }
    write_user(home, &id, &f)?;
    Ok(id)
}

/// Remove a user-created entry. System entries can only be disabled.
pub fn remove(home: &Path, id: &str) -> Result<()> {
    if !valid_id(id) {
        return Err(CoreError::Refused(format!("invalid entry {id}")));
    }
    if Path::new(SYSTEM_DIR).join(id).is_file() {
        return Err(CoreError::Refused(
            "this entry comes from an installed package; disable it instead".into(),
        ));
    }
    fs::remove_file(user_dir(home).join(id))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        let h = std::env::temp_dir().join(format!(
            "syspulse-startup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&h).unwrap();
        h
    }

    #[test]
    fn desktop_file_set_and_get() {
        let mut f = DesktopFile::parse("[Desktop Entry]\nName=A\n\n[Desktop Action x]\nName=B");
        f.set("Name", "C");
        f.set("Exec", "foo --bar");
        assert_eq!(f.get("Name").as_deref(), Some("C"));
        assert_eq!(f.get("Exec").as_deref(), Some("foo --bar"));
        assert!(f.render().contains("[Desktop Action x]\nName=B"));
    }

    #[test]
    fn create_disable_edit_remove() {
        let h = home();
        let id = save(
            &h,
            &EntryInput {
                id: None,
                name: "My Backup!".into(),
                exec: "backup.sh\n--now".into(),
                comment: "x".into(),
                delay: Some(10),
            },
        )
        .unwrap();
        assert_eq!(id, "my-backup.desktop");
        let e = list(&h).into_iter().find(|e| e.id == id).unwrap();
        assert!(e.enabled && !e.system && e.user_file);
        assert_eq!(e.exec, "backup.sh --now", "newlines are neutralised");
        assert_eq!(e.delay, Some(10));
        set_enabled(&h, &id, false).unwrap();
        assert!(!list(&h).into_iter().find(|e| e.id == id).unwrap().enabled);
        remove(&h, &id).unwrap();
        assert!(!list(&h).iter().any(|e| e.id == id));
        assert!(set_enabled(&h, "../evil.desktop", true).is_err());
        fs::remove_dir_all(&h).unwrap();
    }
}
