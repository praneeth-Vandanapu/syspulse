// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/software/desktop.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Minimal freedesktop .desktop file reader (name, icon, visibility).
// =============================================================================

use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct DesktopEntry {
    pub name: Option<String>,
    pub comment: Option<String>,
    pub icon: Option<String>,
    pub visible: bool,
}

pub fn read(path: &Path) -> Option<DesktopEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_main = false;
    let mut e = DesktopEntry {
        visible: true,
        ..Default::default()
    };
    let mut is_app = true;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().to_string();
        match k.trim() {
            "Name" => e.name = Some(v),
            "Comment" => e.comment = Some(v),
            "Icon" => e.icon = Some(v),
            "NoDisplay" | "Hidden" if v.eq_ignore_ascii_case("true") => e.visible = false,
            "Type" => is_app = v == "Application",
            _ => {}
        }
    }
    e.visible &= is_app;
    Some(e)
}
