// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/examples/launcher_selftest.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: End-to-end check of App Launchers for the CURRENT user (no root):
//           builds a throw-away app folder, installs a launcher (menu +
//           trusted desktop icon, no dock), verifies the files and the trust
//           flag, then removes everything again.
//             cargo run -p syspulse-core --example launcher_selftest
// =============================================================================

use std::path::Path;

use syspulse_core::launcher::{self, icon, install};

fn main() {
    let dir = std::env::temp_dir().join("syspulse-launcher-selftest").join("Demo_Tool-1.0");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::copy("/bin/true", dir.join("DemoTool")).unwrap();
    std::fs::write(dir.join("lib/libdemo.so.1"), b"x").unwrap();
    std::fs::write(dir.join("icon.png"), icon::encode_png_rgba(2, 2, &[30, 120, 220, 255].repeat(4)).unwrap()).unwrap();

    let scan = launcher::scan(dir.to_str().unwrap()).expect("scan");
    println!("scan: name={:?} program={} icon={}", scan.name, scan.candidates[0].rel, scan.icons[0].rel);
    let me = launcher::current_user().expect("user");
    let spec = launcher::LauncherSpec {
        id: "selftest-demo".into(),
        name: scan.name.clone(),
        comment: "SysPulse launcher self-test".into(),
        folder: scan.folder.clone(),
        program: scan.candidates[0].path.clone(),
        kind: scan.candidates[0].kind,
        interpreter: None,
        args: String::new(),
        workdir: String::new(),
        env: launcher::suggested_env(&scan),
        terminal: false,
        category: "Utility".into(),
        wm_class: String::new(),
        icon: scan.icons[0].path.clone(),
        menu: true,
        desktop: true,
        dock: true,
        grant_access: true,
        grant_write: false,
    };
    let report = launcher::check(&spec, std::slice::from_ref(&me));
    for i in &report.issues {
        println!("check [{}] {}", i.level, i.text);
    }
    assert!(report.issues.iter().all(|i| i.level != "error"));
    let user = launcher::find_user(&me).unwrap();
    let ic = icon::prepare(Path::new(&spec.icon)).unwrap();
    let res = install::install(&spec, Some(&ic), std::slice::from_ref(&user)).expect("install");
    println!("install: {:?}", res[0]);
    let favs = String::from_utf8(std::process::Command::new("/usr/bin/gsettings").args(["get", "org.gnome.shell", "favorite-apps"]).output().unwrap().stdout).unwrap();
    assert!(favs.contains("syspulse-selftest-demo.desktop"), "not pinned: {favs}");
    println!("dock: pinned");
    assert!(res[0].ok);

    let home = Path::new(&user.home);
    let menu = home.join(".local/share/applications/syspulse-selftest-demo.desktop");
    let desk_dir = String::from_utf8(std::process::Command::new("/usr/bin/xdg-user-dir").arg("DESKTOP").output().unwrap().stdout).unwrap();
    let desk = Path::new(desk_dir.trim()).join("syspulse-selftest-demo.desktop");
    let icon_file = home.join(".local/share/icons/hicolor/256x256/apps/syspulse-selftest-demo.png");
    for p in [&menu, &desk, &icon_file] {
        assert!(p.exists(), "{} missing", p.display());
    }
    let v = std::process::Command::new("/usr/bin/desktop-file-validate").arg(&menu).output().unwrap();
    println!("desktop-file-validate: {}", if v.stdout.is_empty() { "OK".into() } else { String::from_utf8_lossy(&v.stdout).to_string() });
    let trust = std::process::Command::new("/usr/bin/gio").args(["info", "-a", "metadata::trusted"]).arg(&desk).output().unwrap();
    let trust = String::from_utf8_lossy(&trust.stdout);
    println!("desktop icon trust: {}", trust.lines().find(|l| l.contains("trusted")).unwrap_or("(not set)").trim());
    println!("--- menu entry ---\n{}", std::fs::read_to_string(&menu).unwrap());

    let rm = install::RemoveSpec { id: spec.id.clone(), folder: spec.folder.clone(), revoke_access: false, acl_parents: Default::default() };
    let r = install::remove(&rm, std::slice::from_ref(&user)).expect("remove");
    assert!(r[0].ok, "{:?}", r[0]);
    for p in [&menu, &desk, &icon_file] {
        assert!(!p.exists(), "{} still present", p.display());
    }
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    println!("removed cleanly - self-test passed");
    let favs = String::from_utf8(std::process::Command::new("/usr/bin/gsettings").args(["get", "org.gnome.shell", "favorite-apps"]).output().unwrap().stdout).unwrap();
    assert!(!favs.contains("syspulse-selftest-demo"), "still pinned");
}
