// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/bin/syspulse-helper.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Privileged helper. Launched by the SysPulse app through
//               pkexec /usr/bin/syspulse-helper '<json array of requests>'
//           after the user authenticates in the polkit dialog. It executes
//           only the fixed request types in syspulse_core::privileged (all
//           input validated there) and prints a JSON array of responses.
// =============================================================================

use std::io::Write;

use syspulse_core::privileged::{execute_all, HelperRequest, HelperResponse};

// Launcher requests carry a prepared icon (up to ~2 MB as base64).
const MAX_PAYLOAD: usize = 4 * 1024 * 1024;

fn fail(msg: &str) -> ! {
    let resp = vec![HelperResponse {
        ok: false,
        message: msg.to_string(),
        ..Default::default()
    }];
    println!("{}", serde_json::to_string(&resp).unwrap_or_default());
    std::process::exit(2);
}

fn main() {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        fail("syspulse-helper must be started through pkexec");
    }
    // Known-good environment for the package tools we invoke.
    std::env::set_var(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    std::env::set_var("LC_ALL", "C");

    let payload = std::env::args().nth(1).unwrap_or_default();
    if payload.is_empty() || payload.len() > MAX_PAYLOAD {
        fail("missing or oversized request");
    }
    let requests: Vec<HelperRequest> = match serde_json::from_str(&payload) {
        Ok(r) => r,
        Err(e) => fail(&format!("invalid request: {e}")),
    };
    if requests.is_empty() || requests.len() > 32 {
        fail("expected 1-32 requests");
    }

    let caller = std::env::var("PKEXEC_UID").unwrap_or_else(|_| "?".into());
    eprintln!(
        "syspulse-helper: uid {caller} requested {} operation(s)",
        requests.len()
    );

    let responses = execute_all(requests);
    let out = serde_json::to_string(&responses).unwrap_or_else(|_| "[]".into());
    let _ = writeln!(std::io::stdout(), "{out}");
}
