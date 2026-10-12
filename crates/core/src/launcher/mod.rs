// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/launcher/mod.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: App Launchers - turn an application folder (C++ build, PyInstaller
//           bundle, Python project, AppImage, Java jar, script) into a proper
//           desktop application for one or more user accounts: start-menu
//           entry, trusted desktop icon and dock favourite.
//
//             scan      find the executable, icons, bundled library folders,
//                       Qt plugins and Python virtual environments
//             users     local login accounts
//             check     what would stop a user from running it (folder
//                       permissions, missing libraries, interpreter)
//             entry     freedesktop .desktop file with correct Exec quoting
//             install   (install.rs) per-user installation; runs as the
//                       target user wherever files are written, so nothing
//                       in a user's home is ever written by root
//             icon      (icon.rs) icon previews and PNG conversion
// =============================================================================

pub mod icon;
pub mod install;

use std::collections::{HashMap, HashSet};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

pub const ID_PREFIX: &str = "syspulse-";
const MAX_SCAN_ENTRIES: usize = 50_000;
const SKIP_DIRS: [&str; 9] = [
    "venv", ".venv", "env", "site-packages", "__pycache__", "node_modules", ".git", "dist-info", "include",
];

// ------------------------------------------------------------- types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppKind {
    /// Native ELF executable (C/C++, Rust, Go, PyInstaller, Nuitka ...).
    Native,
    AppImage,
    /// Shell script or any file with a #! line.
    Script,
    /// Python source run by an interpreter.
    Python,
    /// Java archive run with `java -jar`.
    Jar,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    /// Path relative to the folder, for display.
    pub rel: String,
    pub kind: AppKind,
    pub score: i32,
    pub reason: String,
    pub executable: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconFile {
    pub path: String,
    pub rel: String,
    /// data: URL for the UI preview (None for formats we cannot render).
    pub preview: Option<String>,
    pub width: u32,
    pub height: u32,
    pub score: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub folder: String,
    pub name: String,
    pub candidates: Vec<Candidate>,
    pub icons: Vec<IconFile>,
    /// Folders that contain bundled shared libraries (for LD_LIBRARY_PATH).
    pub library_dirs: Vec<String>,
    /// Qt plugin folder (contains platforms/libqxcb.so), if bundled.
    pub qt_plugin_dir: Option<String>,
    /// Python interpreter of a bundled virtual environment.
    pub venv_python: Option<String>,
    pub files: usize,
    pub bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// Everything needed to create a launcher.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSpec {
    /// Short id: [a-z0-9-], becomes "syspulse-<id>.desktop".
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub comment: String,
    pub folder: String,
    /// The program: an executable, script, .py or .jar inside `folder`.
    pub program: String,
    pub kind: AppKind,
    /// Interpreter for Python / Java (absolute path), if any.
    #[serde(default)]
    pub interpreter: Option<String>,
    /// Extra command-line arguments (shell-like quoting allowed).
    #[serde(default)]
    pub args: String,
    /// Working directory (defaults to the program's folder).
    #[serde(default)]
    pub workdir: String,
    #[serde(default)]
    pub env: Vec<EnvVar>,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default = "default_category")]
    pub category: String,
    /// StartupWMClass, so the dock groups the window with the launcher.
    #[serde(default)]
    pub wm_class: String,
    /// Source icon file (inside or outside the folder); converted to PNG.
    #[serde(default)]
    pub icon: String,
    #[serde(default = "yes")]
    pub menu: bool,
    #[serde(default = "yes")]
    pub desktop: bool,
    #[serde(default = "yes")]
    pub dock: bool,
    /// Give target users read access to the folder (ACL) when they lack it.
    #[serde(default = "yes")]
    pub grant_access: bool,
    /// Also let them write inside the folder (logs, settings next to the exe).
    #[serde(default)]
    pub grant_write: bool,
}

fn default_category() -> String {
    "Utility".into()
}
fn yes() -> bool {
    true
}

pub const CATEGORIES: [(&str, &str); 11] = [
    ("Utility", "Accessories"),
    ("Development", "Programming"),
    ("Science", "Science & Engineering"),
    ("Network", "Internet"),
    ("Graphics", "Graphics"),
    ("AudioVideo", "Sound & Video"),
    ("Office", "Office"),
    ("Education", "Education"),
    ("Game", "Games"),
    ("System", "System Tools"),
    ("Settings", "Settings"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserAccount {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: String,
    pub full_name: String,
    pub logged_in: bool,
    pub is_admin: bool,
}

// ------------------------------------------------------------- users ---

fn groups_by_member() -> (HashMap<String, Vec<u32>>, HashMap<u32, String>) {
    let mut member: HashMap<String, Vec<u32>> = HashMap::new();
    let mut names = HashMap::new();
    for line in std::fs::read_to_string("/etc/group").unwrap_or_default().lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() < 4 {
            continue;
        }
        let Ok(gid) = f[2].parse::<u32>() else { continue };
        names.insert(gid, f[0].to_string());
        for m in f[3].split(',').filter(|m| !m.is_empty()) {
            member.entry(m.to_string()).or_default().push(gid);
        }
    }
    (member, names)
}

/// Human login accounts (uid 1000-59999 with a real shell).
pub fn users() -> Vec<UserAccount> {
    let (member, names) = groups_by_member();
    let admin_gids: Vec<u32> = names
        .iter()
        .filter(|(_, n)| *n == "sudo" || *n == "admin")
        .map(|(g, _)| *g)
        .collect();
    let mut out: Vec<UserAccount> = std::fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            if f.len() < 7 {
                return None;
            }
            let uid: u32 = f[2].parse().ok()?;
            let gid: u32 = f[3].parse().ok()?;
            if !(1000..60000).contains(&uid) || f[6].ends_with("nologin") || f[6].ends_with("false") {
                return None;
            }
            let groups = member.get(f[0]).cloned().unwrap_or_default();
            Some(UserAccount {
                name: f[0].to_string(),
                uid,
                gid,
                home: f[5].to_string(),
                full_name: f[4].split(',').next().unwrap_or("").to_string(),
                logged_in: Path::new(&format!("/run/user/{uid}")).exists(),
                is_admin: groups.iter().any(|g| admin_gids.contains(g)) || admin_gids.contains(&gid),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn find_user(name: &str) -> Option<UserAccount> {
    users().into_iter().find(|u| u.name == name)
}

/// All group ids of a user (primary + supplementary).
pub fn user_gids(u: &UserAccount) -> HashSet<u32> {
    let (member, _) = groups_by_member();
    let mut s: HashSet<u32> = member.get(&u.name).cloned().unwrap_or_default().into_iter().collect();
    s.insert(u.gid);
    s
}

pub fn current_user() -> Option<String> {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    std::fs::read_to_string("/etc/passwd").ok()?.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.get(2) == Some(&uid.to_string().as_str())).then(|| f[0].to_string())
    })
}

// -------------------------------------------------------------- scan ---

/// Machine architecture of an ELF file and whether it is a runnable program
/// (has a program interpreter or is statically linked ET_EXEC).
pub fn elf_info(path: &Path) -> Option<(u16, bool)> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = vec![0u8; 64];
    let n = f.read(&mut h).ok()?;
    if n < 52 || &h[0..4] != b"\x7fELF" {
        return None;
    }
    let is64 = h[4] == 2;
    let le = h[5] == 1;
    let rd16 = |o: usize| if le { u16::from_le_bytes([h[o], h[o + 1]]) } else { u16::from_be_bytes([h[o], h[o + 1]]) };
    let rd32 = |b: &[u8]| {
        let a: [u8; 4] = b[..4].try_into().unwrap_or_default();
        if le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) }
    };
    let e_type = rd16(16);
    let machine = rd16(18);
    let (phoff, phentsize, phnum) = if is64 {
        let a: [u8; 8] = h[32..40].try_into().ok()?;
        (if le { u64::from_le_bytes(a) } else { u64::from_be_bytes(a) }, rd16(54) as u64, rd16(56) as u64)
    } else {
        (rd32(&h[28..32]) as u64, rd16(42) as u64, rd16(44) as u64)
    };
    if e_type == 2 {
        return Some((machine, true));
    }
    if e_type != 3 || phnum == 0 || phnum > 128 || phentsize < 4 {
        return Some((machine, false));
    }
    // ET_DYN: a PIE executable has PT_INTERP, a shared library does not.
    use std::io::{Seek, SeekFrom};
    let mut ph = vec![0u8; (phentsize * phnum) as usize];
    f.seek(SeekFrom::Start(phoff)).ok()?;
    f.read_exact(&mut ph).ok()?;
    let has_interp = (0..phnum as usize).any(|i| rd32(&ph[i * phentsize as usize..]) == 3);
    Some((machine, has_interp))
}

fn host_machine() -> u16 {
    match std::env::consts::ARCH {
        "x86_64" => 0x3E,
        "aarch64" => 0xB7,
        "x86" => 3,
        "arm" => 0x28,
        _ => 0,
    }
}

fn machine_name(m: u16) -> &'static str {
    match m {
        0x3E => "x86-64",
        0xB7 => "ARM64",
        3 => "x86 (32-bit)",
        0x28 => "ARM (32-bit)",
        _ => "another architecture",
    }
}

fn similar(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && !b.is_empty() && (a.contains(&b) || b.contains(&a))
}

fn pretty_name(folder: &Path) -> String {
    let base = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    // "my_app-1.2.3" -> "My App"
    let cleaned: String = base
        .split(['-', '_', ' ', '.'])
        .filter(|w| !w.is_empty() && !w.chars().next().is_some_and(|c| c.is_ascii_digit()) && !w.eq_ignore_ascii_case("linux") && !w.eq_ignore_ascii_case("x64") && !w.eq_ignore_ascii_case("amd64"))
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() { base } else { cleaned }
}

pub fn suggest_id(name: &str) -> String {
    let mut s: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').chars().take(40).collect::<String>();
    if s.is_empty() { "app".into() } else { s }
}

/// Inspect an application folder.
pub fn scan(folder: &str) -> Result<ScanResult> {
    let root = std::fs::canonicalize(folder)?;
    if !root.is_dir() {
        return Err(CoreError::Refused(format!("{folder} is not a folder")));
    }
    let name = pretty_name(&root);
    let host = host_machine();
    let mut candidates = Vec::new();
    let mut icons = Vec::new();
    let mut lib_dirs: Vec<(String, usize)> = Vec::new();
    let mut qt_plugin_dir = None;
    let mut venv_python = None;
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut truncated = false;

    for py in ["venv/bin/python3", ".venv/bin/python3", "env/bin/python3", "venv/bin/python", ".venv/bin/python"] {
        let p = root.join(py);
        if p.exists() {
            venv_python = Some(p.to_string_lossy().to_string());
            break;
        }
    }

    let mut stack: Vec<(PathBuf, usize)> = vec![(root.clone(), 0)];
    let mut so_count: HashMap<PathBuf, usize> = HashMap::new();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            files += 1;
            if files > MAX_SCAN_ENTRIES {
                truncated = true;
                break;
            }
            let path = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            let fname = e.file_name().to_string_lossy().to_string();
            if meta.is_dir() {
                if fname == "platforms" && path.join("libqxcb.so").exists() {
                    qt_plugin_dir = path.parent().map(|p| p.to_string_lossy().to_string());
                }
                if depth < 6 && !SKIP_DIRS.contains(&fname.as_str()) && !fname.ends_with(".dist-info") {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            // Follow symlinks for the target type (bundles often link libs).
            let meta = std::fs::metadata(&path).unwrap_or(meta);
            if !meta.is_file() {
                continue;
            }
            bytes += meta.len();
            let lower = fname.to_ascii_lowercase();
            let rel = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().to_string();
            let exec_bit = meta.permissions().mode() & 0o111 != 0;

            if lower.contains(".so") && (lower.ends_with(".so") || lower.contains(".so.")) {
                *so_count.entry(dir.clone()).or_default() += 1;
                continue;
            }
            // Icons
            if [".png", ".svg", ".ico", ".xpm", ".jpg", ".jpeg", ".bmp"].iter().any(|x| lower.ends_with(x)) && depth <= 4 && meta.len() < 4 * 1024 * 1024 {
                let mut score = 0;
                if lower.contains("icon") || lower.contains("logo") {
                    score += 30;
                }
                if similar(&fname, &name) {
                    score += 25;
                }
                if lower.ends_with(".svg") || lower.ends_with(".png") {
                    score += 10;
                }
                if lower.ends_with(".ico") {
                    score += 5;
                }
                score -= depth as i32 * 3;
                icons.push((path.clone(), rel.clone(), score));
                continue;
            }
            // Program candidates (only near the top of the folder).
            if depth > 3 {
                continue;
            }
            let mut cand: Option<(AppKind, i32, String)> = None;
            if lower.ends_with(".appimage") {
                cand = Some((AppKind::AppImage, 60, "AppImage".into()));
            } else if lower.ends_with(".jar") {
                cand = Some((AppKind::Jar, 25, "Java archive".into()));
            } else if lower.ends_with(".py") {
                if depth <= 1 {
                    let main_like = ["main.py", "app.py", "run.py", "start.py", "__main__.py", "gui.py"].contains(&lower.as_str()) || similar(&fname, &name);
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    let has_main = text.contains("__main__");
                    if main_like || has_main {
                        cand = Some((AppKind::Python, if main_like { 35 } else { 15 } + if has_main { 15 } else { 0 }, "Python program".into()));
                    }
                }
            } else if let Some((machine, runnable)) = elf_info(&path) {
                if runnable {
                    let mut s = 50;
                    let mut why = "Native program".to_string();
                    if machine != host && host != 0 {
                        s -= 60;
                        why = format!("Native program for {} (this PC cannot run it)", machine_name(machine));
                    }
                    cand = Some((AppKind::Native, s, why));
                }
            } else if exec_bit || lower.ends_with(".sh") {
                let head = std::fs::read(&path).map(|b| b.into_iter().take(2).collect::<Vec<u8>>()).unwrap_or_default();
                if head == b"#!" || lower.ends_with(".sh") {
                    let s = if ["run", "start", "launch"].iter().any(|k| lower.starts_with(k)) || similar(&fname, &name) { 40 } else { 15 };
                    cand = Some((AppKind::Script, s, "Script".into()));
                }
            }
            if let Some((kind, mut score, reason)) = cand {
                score += match depth {
                    0 => 25,
                    1 => 10,
                    _ => 0,
                };
                if similar(&fname, &name) {
                    score += 35;
                }
                if exec_bit {
                    score += 10;
                }
                if ["test", "uninstall", "update", "crash", "helper", "debug", "setup", "install", "config", "check"]
                    .iter()
                    .any(|k| lower.contains(k))
                {
                    score -= 25;
                }
                if meta.len() > 200_000 {
                    score += 5;
                }
                candidates.push(Candidate {
                    path: path.to_string_lossy().to_string(),
                    rel,
                    kind,
                    score,
                    reason,
                    executable: exec_bit,
                    size: meta.len(),
                });
            }
        }
        if truncated {
            break;
        }
    }
    for (dir, n) in so_count {
        let s = dir.to_string_lossy().to_string();
        if !s.contains("site-packages") && !s.contains("/venv/") && !s.contains("/.venv/") && !s.contains("/platforms") && !s.contains("/plugins/") {
            lib_dirs.push((s, n));
        }
    }
    lib_dirs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then(a.rel.len().cmp(&b.rel.len())));
    candidates.truncate(25);

    icons.sort_by_key(|i| std::cmp::Reverse(i.2));
    let icons = icons
        .into_iter()
        .take(16)
        .map(|(p, rel, score)| {
            let (preview, w, h) = icon::preview(&p).unwrap_or((None, 0, 0));
            // Prefer square, reasonably large icons.
            let size_bonus = if w >= 64 && w == h { 10 } else { 0 };
            IconFile {
                path: p.to_string_lossy().to_string(),
                rel,
                preview,
                width: w,
                height: h,
                score: score + size_bonus,
            }
        })
        .collect::<Vec<_>>();
    let mut icons = icons;
    icons.sort_by_key(|i| std::cmp::Reverse(i.score));

    Ok(ScanResult {
        folder: root.to_string_lossy().to_string(),
        name,
        candidates,
        icons,
        library_dirs: lib_dirs.into_iter().map(|x| x.0).take(8).collect(),
        qt_plugin_dir,
        venv_python,
        files,
        bytes,
        truncated,
    })
}

// ----------------------------------------------------------- validate ---

pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 48
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn no_control(s: &str) -> bool {
    !s.chars().any(|c| c.is_control())
}

fn normal_abs(p: &str) -> bool {
    let path = Path::new(p);
    path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}

const FORBIDDEN_ROOTS: [&str; 8] = ["/proc", "/sys", "/dev", "/run", "/boot", "/etc", "/root", "/var/lib"];

/// Structural validation (both in the app and, again, in the root helper).
pub fn validate(spec: &LauncherSpec) -> Result<()> {
    let bad = |m: &str| Err(CoreError::Refused(m.to_string()));
    if !valid_id(&spec.id) {
        return bad("the launcher id may only contain a-z, 0-9 and '-'");
    }
    for (label, v) in [
        ("name", &spec.name),
        ("description", &spec.comment),
        ("arguments", &spec.args),
        ("window class", &spec.wm_class),
        ("category", &spec.category),
    ] {
        if !no_control(v) {
            return Err(CoreError::Refused(format!("{label} contains control characters")));
        }
    }
    if spec.name.trim().is_empty() || spec.name.len() > 120 {
        return bad("give the launcher a name (up to 120 characters)");
    }
    if !CATEGORIES.iter().any(|(c, _)| *c == spec.category) {
        return bad("unknown category");
    }
    for p in [&spec.folder, &spec.program] {
        if !normal_abs(p) || !no_control(p) {
            return Err(CoreError::Refused(format!("{p} must be an absolute path")));
        }
    }
    if FORBIDDEN_ROOTS.iter().any(|r| spec.folder == *r || spec.folder.starts_with(&format!("{r}/"))) || Path::new(&spec.folder).components().count() < 3 {
        return bad("choose the application's own folder (system folders are not allowed)");
    }
    if !Path::new(&spec.program).starts_with(&spec.folder) {
        return bad("the program must be inside the application folder");
    }
    if let Some(i) = &spec.interpreter {
        let allowed = ["/usr/bin/python3", "/usr/bin/python", "/usr/bin/java", "/usr/bin/bash", "/bin/bash", "/usr/bin/sh", "/bin/sh"];
        if !(allowed.contains(&i.as_str()) || (normal_abs(i) && Path::new(i).starts_with(&spec.folder))) {
            return bad("interpreter must be a system interpreter or one inside the application folder");
        }
    }
    if !spec.workdir.is_empty() && (!normal_abs(&spec.workdir) || !no_control(&spec.workdir)) {
        return bad("working folder must be an absolute path");
    }
    if !spec.icon.is_empty() && (!normal_abs(&spec.icon) || !no_control(&spec.icon)) {
        return bad("icon must be an absolute path");
    }
    for e in &spec.env {
        if e.name.is_empty()
            || !e.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || e.name.chars().next().is_some_and(|c| c.is_ascii_digit())
            || !no_control(&e.value)
        {
            return Err(CoreError::Refused(format!("invalid environment variable {}", e.name)));
        }
    }
    if !spec.menu && !spec.desktop {
        return bad("choose at least the start menu or the desktop");
    }
    split_args(&spec.args).map(|_| ())
}

// -------------------------------------------------------------- entry ---

/// Split an argument string with simple shell-like quoting.
pub fn split_args(s: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, ' ' | '\t') => {
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                have = true;
            }
            (Some(q), c) if c == q => quote = None,
            (Some('"') | None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    have = true;
                }
            }
            (_, c) => {
                cur.push(c);
                have = true;
            }
        }
    }
    if quote.is_some() {
        return Err(CoreError::Refused("unbalanced quotes in the arguments".into()));
    }
    if have {
        out.push(cur);
    }
    Ok(out)
}

/// Quote one Exec argument (freedesktop Desktop Entry spec, "Exec key").
fn exec_quote(arg: &str) -> String {
    let reserved = |c: char| " \t\n\"'\\><~|&;$*?#()`".contains(c);
    let mut s = if arg.is_empty() || arg.chars().any(reserved) {
        let mut q = String::from("\"");
        for c in arg.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                q.push('\\');
            }
            q.push(c);
        }
        q.push('"');
        q
    } else {
        arg.to_string()
    };
    // Field codes and the string-level backslash escape.
    s = s.replace('%', "%%");
    s.replace('\\', "\\\\")
}

fn escape_value(v: &str) -> String {
    v.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "\\r")
}

/// The command line as a list (interpreter, program, args).
pub fn command(spec: &LauncherSpec) -> Result<Vec<String>> {
    let mut cmd: Vec<String> = Vec::new();
    if !spec.env.is_empty() {
        cmd.push("env".into());
        for e in &spec.env {
            cmd.push(format!("{}={}", e.name, e.value));
        }
    }
    match spec.kind {
        AppKind::Python => {
            cmd.push(spec.interpreter.clone().unwrap_or_else(|| "/usr/bin/python3".into()));
            cmd.push(spec.program.clone());
        }
        AppKind::Jar => {
            cmd.push(spec.interpreter.clone().unwrap_or_else(|| "/usr/bin/java".into()));
            cmd.push("-jar".into());
            cmd.push(spec.program.clone());
        }
        AppKind::Script if spec.interpreter.is_some() => {
            cmd.push(spec.interpreter.clone().unwrap_or_default());
            cmd.push(spec.program.clone());
        }
        _ => cmd.push(spec.program.clone()),
    }
    cmd.extend(split_args(&spec.args)?);
    Ok(cmd)
}

pub fn desktop_file_name(id: &str) -> String {
    format!("{ID_PREFIX}{id}.desktop")
}

pub fn workdir(spec: &LauncherSpec) -> String {
    if !spec.workdir.is_empty() {
        return spec.workdir.clone();
    }
    Path::new(&spec.program)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| spec.folder.clone())
}

/// Render the .desktop file. `icon` is the installed icon path.
pub fn desktop_entry(spec: &LauncherSpec, icon: Option<&str>) -> Result<String> {
    let exec = command(spec)?.iter().map(|a| exec_quote(a)).collect::<Vec<_>>().join(" ");
    let mut s = String::from("[Desktop Entry]\n");
    s.push_str("Type=Application\nVersion=1.0\n");
    s.push_str(&format!("Name={}\n", escape_value(spec.name.trim())));
    if !spec.comment.trim().is_empty() {
        s.push_str(&format!("Comment={}\n", escape_value(spec.comment.trim())));
    }
    s.push_str(&format!("Exec={exec}\n"));
    s.push_str(&format!("Path={}\n", escape_value(&workdir(spec))));
    if let Some(i) = icon {
        s.push_str(&format!("Icon={}\n", escape_value(i)));
    }
    s.push_str(&format!("Terminal={}\n", spec.terminal));
    s.push_str(&format!("Categories={};\n", spec.category));
    if !spec.wm_class.trim().is_empty() {
        s.push_str(&format!("StartupWMClass={}\n", escape_value(spec.wm_class.trim())));
    }
    s.push_str("StartupNotify=true\n");
    s.push_str(&format!("X-SysPulse-Launcher={}\n", spec.id));
    s.push_str(&format!("X-SysPulse-Folder={}\n", escape_value(&spec.folder)));
    Ok(s)
}

/// Run desktop-file-validate on a rendered entry; returns its complaints.
pub fn validate_entry(text: &str) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("syspulse-launcher-{}", std::process::id()));
    if std::fs::create_dir_all(&dir).is_err() {
        return Vec::new();
    }
    let f = dir.join("check.desktop");
    if std::fs::write(&f, text).is_err() {
        return Vec::new();
    }
    let out = std::process::Command::new("/usr/bin/desktop-file-validate").arg(&f).output();
    let _ = std::fs::remove_dir_all(&dir);
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| l.contains("error:"))
            .map(|l| l.split("error:").nth(1).unwrap_or(l).trim().to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

// -------------------------------------------------------------- check ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// "error" blocks creation, "warning" is shown, "fix" will be repaired.
    pub level: &'static str,
    pub user: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub issues: Vec<Issue>,
    pub preview: String,
    pub command_line: String,
    /// Users that need folder access granted (ACL).
    pub needs_access: Vec<String>,
    pub missing_libraries: Vec<String>,
}

/// Does `user` have `want` (bits r=4 w=2 x=1) on `path`? Mode bits plus
/// named-user / group ACL entries (via getfacl).
pub fn has_access(user: &UserAccount, gids: &HashSet<u32>, path: &Path, want: u32) -> bool {
    let Ok(m) = std::fs::metadata(path) else { return false };
    let mode = m.mode();
    let bits = if m.uid() == user.uid {
        (mode >> 6) & 7
    } else if gids.contains(&m.gid()) {
        (mode >> 3) & 7
    } else {
        mode & 7
    };
    if bits & want == want {
        return true;
    }
    // ACL entries only matter when the file has an ACL ("+" in ls).
    if let Ok(out) = std::process::Command::new("/usr/bin/getfacl").args(["--omit-header", "--absolute-names", "--"]).arg(path).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        let mut mask = 7u32;
        let mut entry = None;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let f: Vec<&str> = line.split(':').collect();
            if f.len() != 3 {
                continue;
            }
            let p = f[2];
            let v = (p.contains('r') as u32) * 4 + (p.contains('w') as u32) * 2 + (p.contains('x') as u32);
            match (f[0], f[1]) {
                ("mask", _) => mask = v,
                ("user", n) if n == user.name => entry = Some(v),
                _ => {}
            }
        }
        if let Some(v) = entry {
            return (v & mask) & want == want;
        }
    }
    false
}

/// Directories from / down to `path` that `user` cannot traverse.
pub fn blocked_parents(user: &UserAccount, gids: &HashSet<u32>, path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut cur = PathBuf::from("/");
    for c in path.components().skip(1) {
        cur.push(c);
        if cur.is_dir() && !has_access(user, gids, &cur, 1) {
            out.push(cur.clone());
        }
    }
    out
}

fn missing_libs(spec: &LauncherSpec) -> Vec<String> {
    if spec.kind != AppKind::Native {
        return Vec::new();
    }
    let mut cmd = std::process::Command::new("/usr/bin/ldd");
    cmd.arg(&spec.program).current_dir(workdir(spec));
    for e in &spec.env {
        if e.name == "LD_LIBRARY_PATH" {
            cmd.env("LD_LIBRARY_PATH", &e.value);
        }
    }
    match cmd.output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| l.contains("not found"))
            .filter_map(|l| l.split("=>").next())
            .map(|s| s.trim().to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Everything that would stop the listed users from running the launcher.
pub fn check(spec: &LauncherSpec, user_names: &[String]) -> CheckReport {
    let mut issues = Vec::new();
    let mut needs_access = Vec::new();
    let mut missing = Vec::new();
    let err = |t: String| Issue { level: "error", user: None, text: t };
    if let Err(e) = validate(spec) {
        issues.push(err(e.to_string()));
    }
    let prog = Path::new(&spec.program);
    match std::fs::metadata(prog) {
        Err(_) => issues.push(err(format!("{} does not exist", spec.program))),
        Ok(m) => {
            let x = m.permissions().mode() & 0o111 != 0;
            if !x && matches!(spec.kind, AppKind::Native | AppKind::AppImage) {
                issues.push(Issue { level: "fix", user: None, text: "The program is not marked executable; it will be made executable.".into() });
            }
            if spec.kind == AppKind::Script && !x && spec.interpreter.is_none() {
                issues.push(Issue { level: "fix", user: None, text: "The script is not executable; it will be made executable.".into() });
            }
        }
    }
    if spec.kind == AppKind::Native {
        if let Some((machine, runnable)) = elf_info(prog) {
            if machine != host_machine() {
                issues.push(err(format!("The program is built for {} and cannot run on this PC.", machine_name(machine))));
            } else if !runnable {
                issues.push(err("This file is a library, not a program.".into()));
            }
        }
        missing = missing_libs(spec);
        if !missing.is_empty() {
            issues.push(Issue {
                level: "warning",
                user: None,
                text: format!(
                    "Libraries not found: {}. Install them with apt, or add the folder that contains them to LD_LIBRARY_PATH.",
                    missing.join(", ")
                ),
            });
        }
    }
    if let Some(i) = &spec.interpreter {
        if !Path::new(i).exists() {
            issues.push(err(format!("Interpreter {i} is not installed")));
        }
    } else if spec.kind == AppKind::Python && !Path::new("/usr/bin/python3").exists() {
        issues.push(err("python3 is not installed".into()));
    } else if spec.kind == AppKind::Jar && !Path::new("/usr/bin/java").exists() {
        issues.push(err("Java is not installed (sudo apt install default-jre)".into()));
    }
    if !spec.icon.is_empty() && !Path::new(&spec.icon).exists() {
        issues.push(err(format!("Icon {} does not exist", spec.icon)));
    }
    if spec.icon.is_empty() {
        issues.push(Issue { level: "warning", user: None, text: "No icon selected: a generic icon will be shown.".into() });
    }
    let all = users();
    for name in user_names {
        let Some(u) = all.iter().find(|u| &u.name == name) else {
            issues.push(err(format!("User {name} does not exist")));
            continue;
        };
        let gids = user_gids(u);
        let blocked = blocked_parents(u, &gids, Path::new(&spec.folder));
        let folder_ok = has_access(u, &gids, Path::new(&spec.folder), 5);
        let prog_ok = has_access(u, &gids, prog, if matches!(spec.kind, AppKind::Native | AppKind::AppImage | AppKind::Script) && spec.interpreter.is_none() { 5 } else { 4 });
        if !blocked.is_empty() || !folder_ok || !prog_ok {
            needs_access.push(name.clone());
            let where_ = blocked.first().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| spec.folder.clone());
            issues.push(Issue {
                level: if spec.grant_access { "fix" } else { "error" },
                user: Some(name.clone()),
                text: if spec.grant_access {
                    format!("{name} cannot open {where_}; read access to the application folder will be granted to {name} only.")
                } else {
                    format!("{name} cannot open {where_}, so the app would not start. Enable \"Give access to the folder\" or move the folder.")
                },
            });
        }
        if !Path::new(&u.home).is_dir() {
            issues.push(err(format!("{name} has no home folder ({})", u.home)));
        }
        if !u.logged_in && spec.dock {
            issues.push(Issue {
                level: "warning",
                user: Some(name.clone()),
                text: format!("{name} is not logged in; the dock pin and desktop trust are applied to their saved settings and take effect at next login."),
            });
        }
    }
    let icon_path = if spec.icon.is_empty() { None } else { Some("<installed icon>") };
    let preview = desktop_entry(spec, icon_path).unwrap_or_default();
    if issues.iter().all(|i| i.level != "error") {
        for e in validate_entry(&preview) {
            issues.push(err(format!("Desktop entry: {e}")));
        }
    }
    CheckReport {
        command_line: command(spec).map(|c| c.join(" ")).unwrap_or_default(),
        issues,
        preview,
        needs_access,
        missing_libraries: missing,
    }
}

/// Suggested environment for a scanned bundle.
pub fn suggested_env(scan: &ScanResult) -> Vec<EnvVar> {
    let mut env = Vec::new();
    if !scan.library_dirs.is_empty() {
        env.push(EnvVar {
            name: "LD_LIBRARY_PATH".into(),
            value: scan.library_dirs.join(":"),
        });
    }
    if let Some(q) = &scan.qt_plugin_dir {
        env.push(EnvVar {
            name: "QT_PLUGIN_PATH".into(),
            value: q.clone(),
        });
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(dir: &str) -> LauncherSpec {
        LauncherSpec {
            id: "my-app".into(),
            name: "My App".into(),
            comment: "Test \"tool\"".into(),
            folder: dir.into(),
            program: format!("{dir}/bin/My App"),
            kind: AppKind::Native,
            interpreter: None,
            args: "--mode \"fast run\" 100%".into(),
            workdir: String::new(),
            env: vec![EnvVar { name: "LD_LIBRARY_PATH".into(), value: format!("{dir}/lib") }],
            terminal: false,
            category: "Science".into(),
            wm_class: "myapp".into(),
            icon: String::new(),
            menu: true,
            desktop: true,
            dock: true,
            grant_access: true,
            grant_write: false,
        }
    }

    #[test]
    fn args_split() {
        assert_eq!(split_args("a \"b c\" 'd e' f\\ g").unwrap(), vec!["a", "b c", "d e", "f g"]);
        assert!(split_args("\"open").is_err());
        assert!(split_args("").unwrap().is_empty());
    }

    #[test]
    fn exec_quoting_follows_spec() {
        assert_eq!(exec_quote("/opt/app/run"), "/opt/app/run");
        assert_eq!(exec_quote("/opt/My App/run"), "\"/opt/My App/run\"");
        // literal backslash -> 4 backslashes, $ escaped, % doubled
        assert_eq!(exec_quote("a\\b"), "\"a\\\\\\\\b\"");
        assert_eq!(exec_quote("$HOME"), "\"\\\\$HOME\"");
        assert_eq!(exec_quote("50%"), "50%%");
    }

    #[test]
    fn entry_renders_and_validates() {
        let s = spec("/opt/myapp");
        validate(&s).unwrap();
        let e = desktop_entry(&s, Some("/home/u/.local/share/icons/hicolor/256x256/apps/syspulse-my-app.png")).unwrap();
        assert!(e.contains("Exec=env LD_LIBRARY_PATH=/opt/myapp/lib \"/opt/myapp/bin/My App\" --mode \"fast run\" 100%%\n"), "{e}");
        assert!(e.contains("Path=/opt/myapp/bin\n"));
        assert!(e.contains("Categories=Science;\n"));
        assert!(e.contains("StartupWMClass=myapp\n"));
        if Path::new("/usr/bin/desktop-file-validate").exists() {
            assert!(validate_entry(&e).is_empty(), "{:?}", validate_entry(&e));
        }
    }

    #[test]
    fn rejects_bad_specs() {
        let mut s = spec("/opt/myapp");
        s.id = "../x".into();
        assert!(validate(&s).is_err());
        let mut s = spec("/opt/myapp");
        s.name = "x\nExec=evil".into();
        assert!(validate(&s).is_err());
        let mut s = spec("/opt/myapp");
        s.program = "/usr/bin/bash".into();
        assert!(validate(&s).is_err());
        let mut s = spec("/etc/foo");
        s.program = "/etc/foo/x".into();
        assert!(validate(&s).is_err());
        let mut s = spec("/opt/myapp");
        s.interpreter = Some("/tmp/evil".into());
        s.kind = AppKind::Python;
        assert!(validate(&s).is_err());
        let mut s = spec("/opt/myapp");
        s.env.push(EnvVar { name: "A B".into(), value: "x".into() });
        assert!(validate(&s).is_err());
    }

    #[test]
    fn scans_a_bundle() {
        let dir = std::env::temp_dir().join(format!("syspulse-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = dir.join("Cool_Viewer-2.1-linux");
        std::fs::create_dir_all(app.join("lib")).unwrap();
        std::fs::create_dir_all(app.join("plugins/platforms")).unwrap();
        std::fs::create_dir_all(app.join("res")).unwrap();
        // a real ELF executable: copy /bin/true
        std::fs::copy("/bin/true", app.join("CoolViewer")).unwrap();
        std::fs::copy("/bin/true", app.join("uninstall")).unwrap();
        std::fs::write(app.join("lib/libfoo.so.1"), b"x").unwrap();
        std::fs::write(app.join("plugins/platforms/libqxcb.so"), b"x").unwrap();
        std::fs::write(app.join("run.sh"), b"#!/bin/sh\nexec ./CoolViewer\n").unwrap();
        // 1x1 PNG
        let png = icon::encode_png_rgba(1, 1, &[255, 0, 0, 255]).unwrap();
        std::fs::write(app.join("res/app_icon.png"), png).unwrap();
        let r = scan(app.to_str().unwrap()).unwrap();
        assert_eq!(r.name, "Cool Viewer");
        assert_eq!(r.candidates[0].rel, "CoolViewer", "{:?}", r.candidates);
        assert!(r.candidates.iter().any(|c| c.rel == "run.sh" && c.kind == AppKind::Script));
        assert!(r.library_dirs.iter().any(|d| d.ends_with("/lib")));
        assert!(r.qt_plugin_dir.as_deref().is_some_and(|q| q.ends_with("/plugins")));
        assert_eq!(r.icons[0].rel, "res/app_icon.png");
        assert!(r.icons[0].preview.as_deref().is_some_and(|p| p.starts_with("data:image/png;base64,")));
        let env = suggested_env(&r);
        assert_eq!(env.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn elf_detection() {
        let (m, runnable) = elf_info(Path::new("/bin/true")).unwrap();
        assert_eq!(m, host_machine());
        assert!(runnable);
        // a shared library is not runnable
        let lib = ["/lib/x86_64-linux-gnu/libz.so.1", "/usr/lib/x86_64-linux-gnu/libz.so.1"]
            .into_iter()
            .find(|p| Path::new(p).exists());
        if let Some(l) = lib {
            assert!(!elf_info(Path::new(l)).unwrap().1);
        }
        assert!(elf_info(Path::new("/etc/hostname")).is_none());
    }

    #[test]
    fn ids_and_names() {
        assert_eq!(suggest_id("My App 2!"), "my-app-2");
        assert!(valid_id("my-app-2"));
        assert!(!valid_id("My App"));
        assert_eq!(pretty_name(Path::new("/opt/signal_analyzer-1.2-linux-x64")), "Signal Analyzer");
    }
}
