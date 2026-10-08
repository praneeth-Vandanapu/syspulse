// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/fsutil.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Filesystem helpers shared by the cleaner, large-file finder and
//           privileged helper: on-disk usage of a tree, safe content removal,
//           home-directory resolution and path safety checks.
// =============================================================================

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Bytes actually allocated on disk (what deleting would free).
pub fn allocated(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.blocks() * 512
    }
    #[cfg(not(unix))]
    {
        meta.len()
    }
}

#[cfg(unix)]
pub fn device_of(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}

#[cfg(not(unix))]
pub fn device_of(_meta: &fs::Metadata) -> u64 {
    0
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub bytes: u64,
    pub files: u64,
}

/// Total allocated size of everything below `path` (not following symlinks,
/// not crossing into other filesystems). Unreadable entries are skipped.
pub fn usage(path: &Path, cancel: Option<&AtomicBool>) -> Usage {
    let Ok(root_meta) = fs::symlink_metadata(path) else {
        return Usage::default();
    };
    if !root_meta.is_dir() {
        return Usage {
            bytes: allocated(&root_meta),
            files: 1,
        };
    }
    let root_dev = device_of(&root_meta);
    let mut u = Usage::default();
    for entry in jwalk::WalkDir::new(path)
        .skip_hidden(false)
        .follow_links(false)
    {
        if cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false) {
            break;
        }
        let Ok(e) = entry else { continue };
        if e.depth == 0 {
            continue;
        }
        let Ok(m) = e.metadata() else { continue };
        if device_of(&m) != root_dev {
            continue;
        }
        if m.is_file() {
            u.files += 1;
        }
        u.bytes += allocated(&m);
    }
    u
}

/// Remove everything inside `dir` but keep `dir` itself. Returns bytes freed.
/// Entries that cannot be removed (in use, permissions) are skipped.
pub fn clear_dir(dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(dir) else { return 0 };
    let mut freed = 0;
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        let size = usage(&p, None).bytes;
        let ok = match e.file_type() {
            Ok(t) if t.is_dir() => fs::remove_dir_all(&p).is_ok(),
            _ => fs::remove_file(&p).is_ok(),
        };
        if ok {
            freed += size;
        }
    }
    freed
}

/// Remove a single file or directory tree. Returns bytes freed.
pub fn remove_path(p: &Path) -> std::io::Result<u64> {
    let meta = fs::symlink_metadata(p)?;
    let size = usage(p, None).bytes;
    if meta.is_dir() {
        fs::remove_dir_all(p)?;
    } else {
        fs::remove_file(p)?;
    }
    Ok(size)
}

/// The invoking user's home directory. Under pkexec, `$HOME` is root's, so
/// the helper passes an explicit home instead of calling this.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// Locations that must never be deleted from, even by the privileged helper.
const PROTECTED_PREFIXES: [&str; 17] = [
    "/bin", "/boot", "/dev", "/etc", "/lib", "/lib32", "/lib64", "/libx32", "/proc", "/run",
    "/sbin", "/snap", "/sys", "/usr", "/var/lib", "/efi", "/root",
];

/// Paths the helper may delete on the user's behalf (large-file finder).
const DELETABLE_PREFIXES: [&str; 8] = [
    "/home/",
    "/tmp/",
    "/var/tmp/",
    "/opt/",
    "/srv/",
    "/mnt/",
    "/media/",
    "/data/",
];

/// True when `p` is an absolute, normalised path outside protected system areas.
pub fn is_user_deletable(p: &Path) -> bool {
    let Some(s) = p.to_str() else { return false };
    if !p.is_absolute() || s.contains("/../") || s.ends_with("/..") || s.contains("/./") {
        return false;
    }
    if PROTECTED_PREFIXES
        .iter()
        .any(|pre| s == *pre || s.starts_with(&format!("{pre}/")))
    {
        return false;
    }
    // Never a mount root or a home directory itself.
    let depth = p.components().count();
    depth > 3 && DELETABLE_PREFIXES.iter().any(|pre| s.starts_with(pre))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletable_rules() {
        assert!(is_user_deletable(Path::new("/home/a/Videos/big.mkv")));
        assert!(!is_user_deletable(Path::new("/home/a")));
        assert!(!is_user_deletable(Path::new("/usr/lib/x.so")));
        assert!(!is_user_deletable(Path::new("/etc/passwd")));
        assert!(!is_user_deletable(Path::new("/home/a/../../etc/passwd")));
        assert!(!is_user_deletable(Path::new("relative/file")));
    }

    #[test]
    fn usage_counts_files() {
        let dir = std::env::temp_dir().join(format!("syspulse-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/a"), vec![1u8; 10_000]).unwrap();
        let u = usage(&dir, None);
        assert_eq!(u.files, 1);
        assert!(u.bytes >= 10_000);
        assert!(clear_dir(&dir) > 0);
        assert!(dir.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
