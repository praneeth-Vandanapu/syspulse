// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/software/icons.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Resolve freedesktop icon names to files and return them as data
//           URLs, so the UI can show application icons without filesystem
//           access from the webview.
// =============================================================================

use std::path::{Path, PathBuf};

const SIZES: [&str; 9] = [
    "64x64", "48x48", "128x128", "96x96", "256x256", "scalable", "512x512", "32x32", "symbolic",
];
const MAX_ICON_BYTES: u64 = 512 * 1024;

fn theme_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/usr/share/icons/hicolor"),
        PathBuf::from("/usr/share/icons/Yaru"),
        PathBuf::from("/usr/share/icons/Adwaita"),
        PathBuf::from("/var/lib/flatpak/exports/share/icons/hicolor"),
        PathBuf::from("/var/lib/snapd/desktop/icons"),
    ];
    if let Some(home) = crate::fsutil::home_dir() {
        roots.push(home.join(".local/share/icons/hicolor"));
        roots.push(home.join(".local/share/flatpak/exports/share/icons/hicolor"));
    }
    roots
}

/// Find the best file for an icon name (or accept an absolute path).
pub fn resolve(icon: &str) -> Option<PathBuf> {
    let p = Path::new(icon);
    if p.is_absolute() {
        // Only image files from icon locations: the UI must not be able to
        // read arbitrary files through this command.
        let allowed_dir = [
            "/usr/share/",
            "/snap/",
            "/var/lib/snapd/",
            "/var/lib/flatpak/",
            "/opt/",
        ]
        .iter()
        .any(|d| icon.starts_with(d))
            || crate::fsutil::home_dir().is_some_and(|h| p.starts_with(h.join(".local/share")));
        let image = matches!(p.extension().and_then(|e| e.to_str()), Some("png" | "svg"));
        return (allowed_dir && image && !icon.contains("..") && p.is_file())
            .then(|| p.to_path_buf());
    }
    if icon.contains('/') || icon.contains("..") {
        return None;
    }
    for root in theme_roots() {
        // Flat directory (snapd desktop icons).
        for ext in ["png", "svg"] {
            let f = root.join(format!("{icon}.{ext}"));
            if f.is_file() {
                return Some(f);
            }
        }
        for size in SIZES {
            for ext in ["png", "svg"] {
                let f = root.join(size).join("apps").join(format!("{icon}.{ext}"));
                if f.is_file() {
                    return Some(f);
                }
            }
        }
    }
    for ext in ["png", "svg"] {
        let f = PathBuf::from(format!("/usr/share/pixmaps/{icon}.{ext}"));
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

/// Icon as `data:image/...;base64,...`, or `None` when not found.
pub fn icon_data_url(icon: &str) -> Option<String> {
    let path = resolve(icon)?;
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > MAX_ICON_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("xpm") => return None,
        _ => "image/png",
    };
    Some(format!(
        "data:{mime};base64,{}",
        crate::util::base64(&bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        assert!(resolve("../../etc/passwd").is_none());
    }
}
