// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/launcher/install.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Install / remove launchers for user accounts.
//
//           Runs in two modes:
//             * directly, for the account running SysPulse (no password);
//             * inside the root helper (pkexec), for other accounts.
//
//           Safety rule in root mode: root never writes into a user's home.
//           Every file there is created by a `runuser -u <user>` child, so
//           ownership is correct and a symlink planted in that home cannot
//           redirect a root write. Root itself only (a) adds the execute bit
//           to the chosen program and (b) adds ACL entries that let the user
//           read the application folder. Nothing from the application folder
//           is ever executed as root.
//
//           Per user it writes:
//             ~/.local/share/applications/syspulse-<id>.desktop    start menu
//             ~/.local/share/icons/hicolor/.../syspulse-<id>.png   icon
//             <XDG desktop dir>/syspulse-<id>.desktop              desktop icon
//           marks the desktop icon trusted (gio metadata::trusted, used by
//           Ubuntu's desktop-icons extension) so it starts without "Allow
//           Launching", and adds it to the dock favourites (gsettings).
// =============================================================================

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::icon::IconData;
use super::{blocked_parents, desktop_entry, desktop_file_name, has_access, user_gids, validate, AppKind, LauncherSpec, UserAccount, ID_PREFIX};
use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserResult {
    pub user: String,
    pub ok: bool,
    pub done: Vec<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    /// Parent folders that got a traverse ACL for this user (for removal).
    pub acl_parents: Vec<String>,
    pub acl_folder: bool,
}

/// How commands for a user are run.
enum Runner<'a> {
    /// We are that user (SysPulse running as them).
    Direct,
    /// We are root: run through runuser with a clean environment.
    AsUser(&'a UserAccount),
}

const TIMEOUT: Duration = Duration::from_secs(25);

impl Runner<'_> {
    fn command(&self, program: &str, args: &[&str], bus: bool) -> Command {
        match self {
            Runner::Direct => {
                if bus && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
                    let mut c = Command::new("/usr/bin/dbus-run-session");
                    c.arg("--").arg(program).args(args);
                    c
                } else {
                    let mut c = Command::new(program);
                    c.args(args);
                    c
                }
            }
            Runner::AsUser(u) => {
                let runtime = format!("/run/user/{}", u.uid);
                let bus_sock = format!("{runtime}/bus");
                let mut c = Command::new("/usr/sbin/runuser");
                c.args(["-u", &u.name, "--", "/usr/bin/env", "-i"]);
                c.arg(format!("HOME={}", u.home));
                c.arg(format!("USER={}", u.name));
                c.arg(format!("LOGNAME={}", u.name));
                c.arg("PATH=/usr/local/bin:/usr/bin:/bin");
                c.arg("LANG=C.UTF-8");
                if Path::new(&runtime).is_dir() {
                    c.arg(format!("XDG_RUNTIME_DIR={runtime}"));
                }
                if bus && Path::new(&bus_sock).exists() {
                    // The user is logged in: talk to their real session.
                    c.arg(format!("DBUS_SESSION_BUS_ADDRESS=unix:path={bus_sock}"));
                    c.arg(program).args(args);
                } else if bus {
                    // Not logged in: a private bus; settings land in their files.
                    c.arg("/usr/bin/dbus-run-session").arg("--").arg(program).args(args);
                } else {
                    c.arg(program).args(args);
                }
                c
            }
        }
    }

    /// Run with optional stdin; returns stdout.
    fn run(&self, program: &str, args: &[&str], stdin: Option<&[u8]>, bus: bool) -> Result<String> {
        let mut c = self.command(program, args, bus);
        c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| CoreError::Other(format!("{program}: {e}")))?;
        if let (Some(data), Some(mut si)) = (stdin, child.stdin.take()) {
            si.write_all(data)?;
        }
        let start = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if start.elapsed() > TIMEOUT {
                let _ = child.kill();
                return Err(CoreError::Other(format!("{program} did not finish")));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let out = child.wait_with_output()?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(CoreError::Other(format!(
                "{} failed: {}",
                Path::new(program).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
                err.lines().last().unwrap_or("error")
            )))
        }
    }

    /// Write a file (creating its folder) as the user.
    fn write(&self, path: &Path, data: &[u8], mode: &str) -> Result<()> {
        let dir = path.parent().ok_or_else(|| CoreError::Other("bad path".into()))?;
        let (d, f) = (dir.to_string_lossy().to_string(), path.to_string_lossy().to_string());
        self.run(
            "/bin/sh",
            &["-c", "umask 022 && mkdir -p -- \"$1\" && cat > \"$2\" && chmod \"$3\" -- \"$2\"", "sh", &d, &f, mode],
            Some(data),
            false,
        )
        .map(|_| ())
    }

    fn remove(&self, paths: &[String]) -> Result<()> {
        let mut args: Vec<&str> = vec!["-f", "--"];
        args.extend(paths.iter().map(|s| s.as_str()));
        self.run("/bin/rm", &args, None, false).map(|_| ())
    }
}

fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

fn desktop_dir(r: &Runner, u: &UserAccount) -> PathBuf {
    let home = PathBuf::from(&u.home);
    let d = r
        .run("/usr/bin/xdg-user-dir", &["DESKTOP"], None, false)
        .map(|s| PathBuf::from(s.trim()))
        .unwrap_or_default();
    if d.starts_with(&home) && d != home && !d.as_os_str().is_empty() {
        d
    } else {
        home.join("Desktop")
    }
}

fn icon_target(u: &UserAccount, id: &str, icon: &IconData) -> PathBuf {
    let base = PathBuf::from(&u.home).join(".local/share/icons");
    let file = format!("{ID_PREFIX}{id}.{}", icon.extension());
    match icon {
        IconData::Png(_) => base.join("hicolor/256x256/apps").join(file),
        IconData::Svg(_) => base.join("hicolor/scalable/apps").join(file),
        IconData::Xpm(_) => base.join(file),
    }
}

fn parse_favorites(s: &str) -> Vec<String> {
    let t = s.trim().trim_start_matches("@as").trim();
    t.trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|x| x.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn format_favorites(v: &[String]) -> String {
    format!("[{}]", v.iter().map(|x| format!("'{}'", x.replace('\'', ""))).collect::<Vec<_>>().join(", "))
}

fn set_dock(r: &Runner, file: &str, pin: bool) -> Result<bool> {
    let cur = r.run("/usr/bin/gsettings", &["get", "org.gnome.shell", "favorite-apps"], None, true)?;
    let mut favs = parse_favorites(&cur);
    let present = favs.iter().any(|f| f == file);
    if pin == present {
        return Ok(false);
    }
    if pin {
        favs.push(file.to_string());
    } else {
        favs.retain(|f| f != file);
    }
    r.run("/usr/bin/gsettings", &["set", "org.gnome.shell", "favorite-apps", &format_favorites(&favs)], None, true)?;
    Ok(true)
}

/// Root only: make the program runnable and the folder readable for `u`.
fn prepare_access(spec: &LauncherSpec, u: &UserAccount, res: &mut UserResult) -> Result<()> {
    let prog = Path::new(&spec.program);
    let needs_x = matches!(spec.kind, AppKind::Native | AppKind::AppImage) || (spec.kind == AppKind::Script && spec.interpreter.is_none());
    if needs_x {
        let meta = std::fs::symlink_metadata(prog)?;
        if !meta.file_type().is_file() {
            return Err(CoreError::Refused("the program must be a regular file (not a link)".into()));
        }
        let mode = meta.permissions().mode();
        if mode & 0o111 != 0o111 {
            std::fs::set_permissions(prog, std::fs::Permissions::from_mode(mode | 0o111))?;
            res.done.push("Made the program executable".into());
        }
    }
    if !spec.grant_access {
        return Ok(());
    }
    let gids = user_gids(u);
    let folder = Path::new(&spec.folder);
    let blocked = blocked_parents(u, &gids, folder);
    let folder_ok = has_access(u, &gids, folder, 5) && has_access(u, &gids, prog, 4);
    let entry = format!("u:{}:", u.name);
    for p in &blocked {
        if p == folder {
            continue;
        }
        acl(&["-m", &format!("{entry}--x"), "--", &p.to_string_lossy()])?;
        res.acl_parents.push(p.to_string_lossy().to_string());
    }
    let perms = if spec.grant_write { "rwX" } else { "rX" };
    if !folder_ok || spec.grant_write || blocked.iter().any(|p| p == folder) {
        let f = folder.to_string_lossy().to_string();
        acl(&["-R", "-P", "-m", &format!("{entry}{perms}"), "--", &f])?;
        // Default entries so files added later stay readable.
        acl(&["-R", "-P", "-d", "-m", &format!("{entry}{perms}"), "--", &f])?;
        res.acl_folder = true;
        res.done.push(format!(
            "Gave {} {} access to {} (this folder only{})",
            u.name,
            if spec.grant_write { "read/write" } else { "read" },
            spec.folder,
            if res.acl_parents.is_empty() { String::new() } else { format!("; may pass through {}", res.acl_parents.join(", ")) }
        ));
    }
    Ok(())
}

fn acl(args: &[&str]) -> Result<()> {
    let out = Command::new("/usr/bin/setfacl").args(args).output().map_err(|e| CoreError::Other(format!("setfacl: {e} (install the acl package)")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(CoreError::Other(format!("setfacl: {}", String::from_utf8_lossy(&out.stderr).trim())))
    }
}

/// Install for one user. `as_root` = running in the helper for another account.
fn install_one(spec: &LauncherSpec, icon: Option<&IconData>, u: &UserAccount, as_root: bool) -> UserResult {
    let mut res = UserResult {
        user: u.name.clone(),
        ..Default::default()
    };
    let r = if as_root { Runner::AsUser(u) } else { Runner::Direct };
    let step = |res: &mut UserResult| -> Result<()> {
        if as_root {
            prepare_access(spec, u, res)?;
        } else if !Path::new(&spec.program).metadata().map(|m| m.permissions().mode() & 0o100 != 0).unwrap_or(true)
            && matches!(spec.kind, AppKind::Native | AppKind::AppImage)
        {
            let m = std::fs::metadata(&spec.program)?.permissions().mode();
            std::fs::set_permissions(&spec.program, std::fs::Permissions::from_mode(m | 0o111))?;
            res.done.push("Made the program executable".into());
        }
        let file = desktop_file_name(&spec.id);
        let mut icon_path = None;
        if let Some(i) = icon {
            let p = icon_target(u, &spec.id, i);
            r.write(&p, i.bytes(), "644")?;
            icon_path = Some(p.to_string_lossy().to_string());
            res.done.push("Installed the icon".into());
        }
        let entry = desktop_entry(spec, icon_path.as_deref())?;
        let apps = PathBuf::from(&u.home).join(".local/share/applications");
        // The menu file is also the dock's reference, so it is written when
        // either the menu or the dock is wanted.
        if spec.menu || spec.dock {
            r.write(&apps.join(&file), entry.as_bytes(), "644")?;
            let _ = r.run("/usr/bin/update-desktop-database", &["-q", &apps.to_string_lossy()], None, false);
            res.done.push("Added to the applications menu".into());
        }
        if spec.desktop {
            let desk = desktop_dir(&r, u);
            let target = desk.join(&file);
            r.write(&target, entry.as_bytes(), "755")?;
            let t = target.to_string_lossy().to_string();
            match r.run("/usr/bin/gio", &["set", "-t", "string", &t, "metadata::trusted", "true"], None, true) {
                Ok(_) => res.done.push(format!("Placed a trusted icon on the desktop ({})", desk.display())),
                Err(e) => {
                    res.done.push(format!("Placed the icon on the desktop ({})", desk.display()));
                    res.warnings.push(format!(
                        "Could not mark the desktop icon as trusted ({e}). The user may need to right-click it once and choose \"Allow Launching\"."
                    ));
                }
            }
        }
        if spec.dock {
            match set_dock(&r, &file, true) {
                Ok(true) => res.done.push("Pinned to the dock".into()),
                Ok(false) => res.done.push("Already pinned to the dock".into()),
                Err(e) => res.warnings.push(format!("Could not pin to the dock: {e}")),
            }
        }
        Ok(())
    };
    match step(&mut res) {
        Ok(()) => res.ok = true,
        Err(e) => res.error = Some(e.to_string()),
    }
    res
}

/// Install a launcher for each user. Must be root unless the only user is
/// the account running SysPulse.
pub fn install(spec: &LauncherSpec, icon: Option<&IconData>, users: &[UserAccount]) -> Result<Vec<UserResult>> {
    validate(spec)?;
    // Resolve symlinks: the program must really live inside the folder.
    let folder = std::fs::canonicalize(&spec.folder)?;
    let program = std::fs::canonicalize(&spec.program)?;
    if !program.starts_with(&folder) || folder.to_string_lossy() != spec.folder.trim_end_matches('/') {
        return Err(CoreError::Refused("the program must be inside the application folder (links are not followed)".into()));
    }
    let root = is_root();
    let me = super::current_user();
    let mut out = Vec::new();
    for u in users {
        if u.uid == 0 {
            return Err(CoreError::Refused("launchers are not created for root".into()));
        }
        let direct = me.as_deref() == Some(u.name.as_str()) && !root;
        if !root && !direct {
            return Err(CoreError::PermissionDenied);
        }
        out.push(install_one(spec, icon, u, root));
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveSpec {
    pub id: String,
    pub folder: String,
    /// Also remove the ACL entries SysPulse added for these users.
    pub revoke_access: bool,
    /// user -> parent folders that received a traverse ACL.
    #[serde(default)]
    pub acl_parents: std::collections::HashMap<String, Vec<String>>,
}

pub fn remove(spec: &RemoveSpec, users: &[UserAccount]) -> Result<Vec<UserResult>> {
    if !super::valid_id(&spec.id) {
        return Err(CoreError::Refused("invalid launcher id".into()));
    }
    let root = is_root();
    let me = super::current_user();
    let file = desktop_file_name(&spec.id);
    let mut out = Vec::new();
    for u in users {
        let direct = me.as_deref() == Some(u.name.as_str()) && !root;
        if !root && !direct {
            return Err(CoreError::PermissionDenied);
        }
        let r = if root { Runner::AsUser(u) } else { Runner::Direct };
        let mut res = UserResult {
            user: u.name.clone(),
            ..Default::default()
        };
        let home = PathBuf::from(&u.home);
        let desk = desktop_dir(&r, u);
        let mut files = vec![
            home.join(".local/share/applications").join(&file),
            desk.join(&file),
        ];
        for ext in ["png", "svg", "xpm"] {
            let n = format!("{ID_PREFIX}{}.{ext}", spec.id);
            files.push(home.join(".local/share/icons/hicolor/256x256/apps").join(&n));
            files.push(home.join(".local/share/icons/hicolor/scalable/apps").join(&n));
            files.push(home.join(".local/share/icons").join(&n));
        }
        let files: Vec<String> = files.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let result = (|| -> Result<()> {
            if let Err(e) = set_dock(&r, &file, false) {
                res.warnings.push(format!("Could not update the dock: {e}"));
            }
            r.remove(&files)?;
            let apps = home.join(".local/share/applications");
            let _ = r.run("/usr/bin/update-desktop-database", &["-q", &apps.to_string_lossy()], None, false);
            res.done.push("Removed the menu entry, desktop icon and icon file".into());
            if root && spec.revoke_access && super::normal_abs(&spec.folder) && Path::new(&spec.folder).is_dir() {
                let entry = format!("u:{0},d:u:{0}", u.name);
                if acl(&["-R", "-P", "-x", &entry, "--", &spec.folder]).is_ok() {
                    res.done.push(format!("Removed {}'s access to {}", u.name, spec.folder));
                }
                for p in spec.acl_parents.get(&u.name).into_iter().flatten() {
                    // Only ancestors of the application folder.
                    if super::normal_abs(p) && Path::new(&spec.folder).starts_with(p) {
                        let _ = acl(&["-x", &format!("u:{}", u.name), "--", p]);
                    }
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => res.ok = true,
            Err(e) => res.error = Some(e.to_string()),
        }
        out.push(res);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_roundtrip() {
        let v = parse_favorites("['firefox_firefox.desktop', 'org.gnome.Nautilus.desktop']");
        assert_eq!(v, vec!["firefox_firefox.desktop", "org.gnome.Nautilus.desktop"]);
        assert!(parse_favorites("@as []").is_empty());
        assert_eq!(format_favorites(&v), "['firefox_firefox.desktop', 'org.gnome.Nautilus.desktop']");
    }

    #[test]
    fn icon_paths() {
        let u = UserAccount {
            name: "op".into(),
            uid: 1001,
            gid: 1001,
            home: "/home/op".into(),
            full_name: String::new(),
            logged_in: false,
            is_admin: false,
        };
        assert_eq!(
            icon_target(&u, "viewer", &IconData::Png(vec![])),
            PathBuf::from("/home/op/.local/share/icons/hicolor/256x256/apps/syspulse-viewer.png")
        );
        assert_eq!(
            icon_target(&u, "viewer", &IconData::Svg(vec![])),
            PathBuf::from("/home/op/.local/share/icons/hicolor/scalable/apps/syspulse-viewer.svg")
        );
    }
}
