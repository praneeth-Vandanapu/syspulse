// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/examples/launcher_scan.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Print what App Launchers detects in a folder (diagnostics).
//             cargo run -p syspulse-core --example launcher_scan -- <folder>
// =============================================================================

fn main() {
    let folder = std::env::args().nth(1).expect("usage: launcher_scan <folder>");
    let s = syspulse_core::launcher::scan(&folder).expect("scan");
    println!("name: {}   files: {}   size: {} bytes", s.name, s.files, s.bytes);
    println!("programs:");
    for c in s.candidates.iter().take(5) {
        println!("  {:>4}  {:<10?} {:<28} {}", c.score, c.kind, c.rel, c.reason);
    }
    println!("icons:");
    for i in s.icons.iter().take(5) {
        println!("  {:>4}  {} ({}x{})", i.score, i.rel, i.width, i.height);
    }
    println!("library folders: {:?}", s.library_dirs);
    println!("qt plugins: {:?}   python venv: {:?}", s.qt_plugin_dir, s.venv_python);
    println!("suggested env: {:?}", syspulse_core::launcher::suggested_env(&s));
}
