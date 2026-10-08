// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/largefiles.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Large-file finder. Parallel directory walk (jwalk) with stat calls
//           done on the worker threads, top-N selection, folder totals,
//           category breakdown, progress reporting and cancellation.
//           Also provides Trash / delete actions for the results.
// =============================================================================

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::fsutil;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum FileCategory {
    Video,
    Audio,
    Image,
    Archive,
    DiskImage,
    Document,
    Package,
    Code,
    Other,
}

pub fn categorize(path: &Path) -> FileCategory {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    use FileCategory::*;
    match ext.as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "m4v" | "mpg" | "mpeg" | "ts"
        | "m2ts" | "3gp" => Video,
        "mp3" | "flac" | "wav" | "ogg" | "opus" | "m4a" | "aac" | "wma" | "aiff" => Audio,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "tif" | "tiff" | "bmp" | "raw" | "cr2"
        | "nef" | "arw" | "heic" | "psd" | "xcf" | "svg" => Image,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "lz4" | "lzma" => {
            Archive
        }
        "iso" | "img" | "qcow2" | "vdi" | "vmdk" | "vhd" | "vhdx" | "raw-img" => DiskImage,
        "pdf" | "doc" | "docx" | "odt" | "xls" | "xlsx" | "ods" | "ppt" | "pptx" | "odp"
        | "epub" | "txt" | "csv" | "md" => Document,
        "deb" | "rpm" | "appimage" | "snap" | "flatpak" | "exe" | "msi" | "run" | "whl"
        | "conda" | "jar" => Package,
        "rs" | "c" | "h" | "cpp" | "py" | "js" | "ts_" | "java" | "go" | "o" | "so" | "a"
        | "rlib" | "pt" | "pth" | "onnx" | "safetensors" | "bin" | "ckpt" | "h5" | "npy"
        | "parquet" | "db" | "sqlite" => Code,
        _ => Other,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOptions {
    pub root: String,
    pub min_size: u64,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default = "yes")]
    pub same_filesystem: bool,
    /// Empty = every category.
    #[serde(default)]
    pub categories: Vec<FileCategory>,
    #[serde(default = "default_max")]
    pub max_results: usize,
}

fn yes() -> bool {
    true
}
fn default_max() -> usize {
    500
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: Option<u64>,
    pub category: FileCategory,
    /// Owned by the current user (can be removed without admin rights).
    pub owned: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSize {
    pub path: String,
    pub size: u64,
    pub files: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryTotal {
    pub category: FileCategory,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub current: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub root: String,
    pub files: Vec<LargeFile>,
    /// Immediate children of the root, largest first.
    pub folders: Vec<FolderSize>,
    /// Second-level folders, largest first ("hot spots").
    pub subfolders: Vec<FolderSize>,
    /// All scanned files grouped by type.
    pub categories: Vec<CategoryTotal>,
    pub total_files: u64,
    pub total_bytes: u64,
    pub skipped_dirs: u64,
    pub elapsed_ms: u64,
    pub cancelled: bool,
    /// Folder hierarchy (3 levels) for the disk-usage map.
    pub tree: DirNode,
}

/// A node of the disk-usage map. `kind`: dir | files (loose files in the
/// parent) | other (small folders merged together).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub kind: &'static str,
    pub children: Vec<DirNode>,
}

const TREE_DEPTH: usize = 3;
const TREE_FANOUT: usize = 14;

/// Build the map from per-directory totals (relative paths, depth <= TREE_DEPTH).
fn build_tree(
    root: &Path,
    totals: &HashMap<PathBuf, u64>,
    rel: &Path,
    size: u64,
    depth: usize,
) -> DirNode {
    let name = if rel.as_os_str().is_empty() {
        root.display().to_string()
    } else {
        rel.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut node = DirNode {
        name,
        path: root.join(rel).display().to_string(),
        size,
        kind: "dir",
        children: Vec::new(),
    };
    if depth >= TREE_DEPTH {
        return node;
    }
    let mut kids: Vec<(&PathBuf, u64)> = totals
        .iter()
        .filter(|(p, _)| p.parent() == Some(rel) && !p.as_os_str().is_empty())
        .map(|(p, s)| (p, *s))
        .collect();
    kids.sort_by_key(|k| Reverse(k.1));
    let in_dirs: u64 = kids.iter().map(|k| k.1).sum();
    let rest: Vec<(&PathBuf, u64)> = if kids.len() > TREE_FANOUT {
        kids.split_off(TREE_FANOUT)
    } else {
        Vec::new()
    };
    for (p, s) in kids {
        node.children
            .push(build_tree(root, totals, p, s, depth + 1));
    }
    if !rest.is_empty() {
        node.children.push(DirNode {
            name: format!("{} smaller folders", rest.len()),
            path: node.path.clone(),
            size: rest.iter().map(|r| r.1).sum(),
            kind: "other",
            children: Vec::new(),
        });
    }
    let loose = size.saturating_sub(in_dirs);
    if loose > 0 {
        node.children.push(DirNode {
            name: "Files here".into(),
            path: node.path.clone(),
            size: loose,
            kind: "files",
            children: Vec::new(),
        });
    }
    node.children.sort_by_key(|c| Reverse(c.size));
    node
}

/// Top-N heap entry: (size, path, mtime, uid).
type Ranked = Reverse<(u64, PathBuf, Option<u64>, u32)>;

/// Per-entry metadata gathered on jwalk worker threads.
#[derive(Debug, Clone, Default)]
struct Meta {
    size: u64,
    mtime: Option<u64>,
    uid: u32,
}

const PSEUDO_ROOTS: [&str; 6] = ["/proc", "/sys", "/dev", "/run", "/snap", "/tmp/.X11-unix"];

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions.
    unsafe { libc::getuid() }
}
#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

/// Expand a leading `~` to the user's home directory.
fn expand_home(p: &str) -> PathBuf {
    match (p.strip_prefix('~'), fsutil::home_dir()) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(p),
    }
}

pub fn scan(
    opts: &ScanOptions,
    cancel: &AtomicBool,
    progress: impl Fn(&ScanProgress),
) -> Result<ScanResult> {
    let root = expand_home(opts.root.trim());
    let root_meta =
        std::fs::metadata(&root).map_err(|e| CoreError::Other(format!("{}: {e}", opts.root)))?;
    if !root_meta.is_dir() {
        return Err(CoreError::Other(format!("{} is not a folder", opts.root)));
    }
    let root_dev = fsutil::device_of(&root_meta);
    let same_fs = opts.same_filesystem;
    let include_hidden = opts.include_hidden;
    let started = Instant::now();
    let me = current_uid();
    let skipped = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let skipped_w = skipped.clone();

    let walker = jwalk::WalkDirGeneric::<((), Option<Meta>)>::new(&root)
        .skip_hidden(!include_hidden)
        .follow_links(false)
        .process_read_dir(move |_, dir, _, children| {
            if PSEUDO_ROOTS.iter().any(|p| dir == Path::new(p)) {
                children.clear();
                return;
            }
            for child in children.iter_mut().flatten() {
                let Ok(m) = child.metadata() else { continue };
                if child.file_type().is_dir() {
                    let skip = (same_fs && fsutil::device_of(&m) != root_dev)
                        || PSEUDO_ROOTS.iter().any(|p| child.path() == Path::new(p));
                    if skip {
                        child.read_children = None;
                        skipped_w.fetch_add(1, Ordering::Relaxed);
                    }
                } else if child.file_type().is_file() {
                    #[cfg(unix)]
                    let uid = std::os::unix::fs::MetadataExt::uid(&m);
                    #[cfg(not(unix))]
                    let uid = 0;
                    child.client_state = Some(Meta {
                        size: m.len(),
                        mtime: m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_secs()),
                        uid,
                    });
                }
            }
        });

    let mut heap: BinaryHeap<Ranked> = BinaryHeap::new();
    let mut folders: HashMap<PathBuf, (u64, u64)> = HashMap::new();
    let mut subfolders: HashMap<PathBuf, (u64, u64)> = HashMap::new();
    // Relative directory -> total size, for directories up to TREE_DEPTH deep.
    let mut dir_totals: HashMap<PathBuf, u64> = HashMap::new();
    let mut cats: HashMap<FileCategory, (u64, u64)> = HashMap::new();
    let (mut total_files, mut total_bytes) = (0u64, 0u64);
    let mut last_report = Instant::now();
    let mut cancelled = false;

    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let Ok(e) = entry else { continue };
        let Some(meta) = e.client_state.clone() else {
            continue;
        };
        let path = e.path();
        total_files += 1;
        total_bytes += meta.size;

        let cat = categorize(&path);
        let c = cats.entry(cat).or_default();
        c.0 += meta.size;
        c.1 += 1;

        if let Ok(rel) = path.strip_prefix(&root) {
            if let Some(parent) = rel.parent() {
                let mut acc = PathBuf::new();
                for c in parent.components().take(TREE_DEPTH) {
                    acc.push(c);
                    *dir_totals.entry(acc.clone()).or_default() += meta.size;
                }
            }
            let mut comps = rel.components();
            if let (Some(first), Some(_)) = (
                comps.next(),
                rel.parent().filter(|p| !p.as_os_str().is_empty()),
            ) {
                let f = folders.entry(root.join(first)).or_default();
                f.0 += meta.size;
                f.1 += 1;
                if let (Some(second), Some(_)) = (comps.next(), comps.next()) {
                    let s = subfolders.entry(root.join(first).join(second)).or_default();
                    s.0 += meta.size;
                    s.1 += 1;
                }
            }
        }

        if meta.size >= opts.min_size
            && (opts.categories.is_empty() || opts.categories.contains(&cat))
        {
            heap.push(Reverse((meta.size, path.clone(), meta.mtime, meta.uid)));
            if heap.len() > opts.max_results {
                heap.pop();
            }
        }

        if last_report.elapsed().as_millis() >= 200 {
            last_report = Instant::now();
            progress(&ScanProgress {
                files_scanned: total_files,
                bytes_scanned: total_bytes,
                current: path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
    }

    let mut files: Vec<LargeFile> = heap
        .into_iter()
        .map(|Reverse((size, path, mtime, uid))| LargeFile {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            category: categorize(&path),
            path: path.display().to_string(),
            size,
            modified: mtime,
            owned: uid == me,
        })
        .collect();
    files.sort_by_key(|f| Reverse(f.size));

    let top = |m: HashMap<PathBuf, (u64, u64)>, n: usize| {
        let mut v: Vec<FolderSize> = m
            .into_iter()
            .map(|(p, (size, files))| FolderSize {
                path: p.display().to_string(),
                size,
                files,
            })
            .collect();
        v.sort_by_key(|f| Reverse(f.size));
        v.truncate(n);
        v
    };
    let mut categories: Vec<CategoryTotal> = cats
        .into_iter()
        .map(|(category, (bytes, files))| CategoryTotal {
            category,
            bytes,
            files,
        })
        .collect();
    categories.sort_by_key(|c| Reverse(c.bytes));

    Ok(ScanResult {
        root: root.display().to_string(),
        files,
        folders: top(folders, 20),
        subfolders: top(subfolders, 25),
        categories,
        total_files,
        total_bytes,
        skipped_dirs: skipped.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_millis() as u64,
        cancelled,
        tree: build_tree(&root, &dir_totals, Path::new(""), total_bytes, 0),
    })
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileActionReport {
    pub done: Vec<String>,
    pub freed_bytes: u64,
    /// Paths that failed with "permission denied" (can be retried as admin).
    pub denied: Vec<String>,
    pub errors: Vec<String>,
}

fn file_size(p: &Path) -> u64 {
    std::fs::symlink_metadata(p)
        .map(|m| fsutil::allocated(&m))
        .unwrap_or(0)
}

/// Move files to the desktop Trash (freedesktop spec / Windows Recycle Bin).
pub fn trash_files(paths: &[String]) -> FileActionReport {
    let mut r = FileActionReport::default();
    for p in paths {
        let size = file_size(Path::new(p));
        match trash::delete(p) {
            Ok(()) => {
                r.done.push(p.clone());
                r.freed_bytes += size;
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.to_lowercase().contains("permission") {
                    r.denied.push(p.clone());
                } else {
                    r.errors.push(format!("{p}: {msg}"));
                }
            }
        }
    }
    r
}

/// Permanently delete regular files (no directories).
pub fn delete_files(paths: &[String]) -> FileActionReport {
    let mut r = FileActionReport::default();
    for p in paths {
        let path = Path::new(p);
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.is_file() || m.file_type().is_symlink() => {}
            Ok(_) => {
                r.errors.push(format!("{p}: not a regular file"));
                continue;
            }
            Err(e) => {
                r.errors.push(format!("{p}: {e}"));
                continue;
            }
        }
        let size = file_size(path);
        match std::fs::remove_file(path) {
            Ok(()) => {
                r.done.push(p.clone());
                r.freed_bytes += size;
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => r.denied.push(p.clone()),
            Err(e) => r.errors.push(format!("{p}: {e}")),
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories() {
        assert_eq!(categorize(Path::new("/a/b.MKV")), FileCategory::Video);
        assert_eq!(
            categorize(Path::new("/a/ubuntu.iso")),
            FileCategory::DiskImage
        );
        assert_eq!(categorize(Path::new("/a/noext")), FileCategory::Other);
    }

    #[test]
    fn finds_large_files() {
        let dir = std::env::temp_dir().join(format!("syspulse-lf-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/big.mp4"), vec![1u8; 300_000]).unwrap();
        std::fs::write(dir.join("small.txt"), b"hi").unwrap();
        let opts = ScanOptions {
            root: dir.display().to_string(),
            min_size: 100_000,
            include_hidden: false,
            same_filesystem: true,
            categories: vec![],
            max_results: 10,
        };
        let r = scan(&opts, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(r.files.len(), 1);
        assert_eq!(r.files[0].name, "big.mp4");
        assert!(r.files[0].owned);
        assert_eq!(r.total_files, 2);
        assert_eq!(r.folders[0].path, dir.join("a").display().to_string());
        assert_eq!(r.tree.size, r.total_bytes);
        let a = r.tree.children.iter().find(|c| c.name == "a").unwrap();
        assert_eq!(a.children[0].name, "b");
        assert!(
            r.tree.children.iter().any(|c| c.kind == "files"),
            "small.txt is a loose file at the root"
        );
        let d = delete_files(&[r.files[0].path.clone()]);
        assert_eq!(d.done.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
