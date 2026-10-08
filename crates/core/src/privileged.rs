// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/privileged.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Privilege separation. The GUI never runs as root; operations that
//           need root are serialised as a small, fixed set of requests and
//           executed by `syspulse-helper`, launched through `pkexec` (polkit
//           password dialog). Several requests can be batched so the user
//           authenticates once.
//
//           Security rules enforced in `execute` (helper side, as root):
//             * only the request variants below exist - no shell, no paths
//               for cleaning (fixed target IDs), package names validated;
//             * file deletion limited to user data locations (fsutil);
//             * PID 0/1 and the helper itself are never signalled;
//             * capture capabilities only ever go to the syspulse-capture
//               binary installed next to the helper.
// =============================================================================

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::models::Signal;
use crate::software::PackageSource;

/// Polkit action id (see packaging/linux/*.policy).
pub const POLKIT_ACTION: &str = "io.github.praneeth.syspulse.helper";
pub const HELPER_NAME: &str = "syspulse-helper";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum HelperRequest {
    #[serde(rename_all = "camelCase")]
    Signal { pid: u32, signal: Signal },
    #[serde(rename_all = "camelCase")]
    Renice { pid: u32, nice: i32 },
    #[serde(rename_all = "camelCase")]
    RemovePackages {
        source: PackageSource,
        names: Vec<String>,
        purge: bool,
    },
    #[serde(rename_all = "camelCase")]
    CleanSystem { targets: Vec<String> },
    #[serde(rename_all = "camelCase")]
    DeleteFiles { paths: Vec<String> },
    #[serde(rename_all = "camelCase")]
    Service {
        unit: String,
        action: crate::services::ServiceAction,
    },
    /// Add the authenticating user (from PKEXEC_UID) to the serial-port group.
    GrantSerialAccess,
    /// Give (or remove) the packet-capture capabilities of `syspulse-capture`.
    /// The binary path is resolved by the helper, never sent by the caller.
    #[serde(rename_all = "camelCase")]
    SetCaptureAccess { grant: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HelperResponse {
    pub ok: bool,
    pub message: String,
    pub freed_bytes: u64,
    /// Tool output (apt/snap) for the details view.
    pub output: String,
}

impl HelperResponse {
    fn ok(message: impl Into<String>) -> Self {
        HelperResponse {
            ok: true,
            message: message.into(),
            ..Default::default()
        }
    }
    fn err(message: impl Into<String>) -> Self {
        HelperResponse {
            ok: false,
            message: message.into(),
            ..Default::default()
        }
    }
}

// ----------------------------------------------------------- app side ---

/// Find the helper binary: installed location first, then next to the
/// running executable (development builds: target/debug/syspulse-helper).
pub fn helper_path() -> Option<std::path::PathBuf> {
    let installed = [
        "/usr/bin/syspulse-helper",
        "/usr/lib/syspulse/syspulse-helper",
    ];
    if let Some(p) = installed
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
    {
        return Some(p);
    }
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join(HELPER_NAME);
    p.is_file().then_some(p)
}

/// Run requests as root through pkexec. Blocks while the password dialog is
/// open. Returns one response per request.
#[cfg(unix)]
pub fn run_elevated(requests: &[HelperRequest]) -> Result<Vec<HelperResponse>> {
    let helper =
        helper_path().ok_or_else(|| CoreError::Other("syspulse-helper is not installed".into()))?;
    let payload = serde_json::to_string(requests).map_err(|e| CoreError::Other(e.to_string()))?;
    let out = std::process::Command::new("pkexec")
        .arg(&helper)
        .arg(payload)
        .output()
        .map_err(|e| CoreError::Other(format!("pkexec unavailable: {e}")))?;
    match out.status.code() {
        Some(126) => return Err(CoreError::Cancelled),
        Some(127) => return Err(CoreError::PermissionDenied),
        _ => {}
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).map_err(|_| {
        CoreError::Other(format!(
            "helper failed: {}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("no output")
        ))
    })
}

#[cfg(not(unix))]
pub fn run_elevated(_requests: &[HelperRequest]) -> Result<Vec<HelperResponse>> {
    Err(CoreError::Unsupported("elevation"))
}

/// Convenience: run a single request and turn a failed response into an error.
pub fn run_one(req: HelperRequest) -> Result<HelperResponse> {
    let mut r = run_elevated(std::slice::from_ref(&req))?;
    let resp = r
        .pop()
        .ok_or_else(|| CoreError::Other("empty helper response".into()))?;
    if resp.ok {
        Ok(resp)
    } else {
        Err(CoreError::Other(resp.message))
    }
}

// -------------------------------------------------------- helper side ---

/// Execute requests. Called by `syspulse-helper` after verifying it runs as root.
pub fn execute_all(requests: Vec<HelperRequest>) -> Vec<HelperResponse> {
    requests.into_iter().map(execute).collect()
}

pub fn execute(req: HelperRequest) -> HelperResponse {
    match req {
        HelperRequest::Signal { pid, signal } => {
            if pid <= 1 || pid == std::process::id() {
                return HelperResponse::err("refusing to signal this process");
            }
            match crate::process::signal_direct(pid, signal) {
                Ok(()) => HelperResponse::ok(format!("sent SIG{} to {pid}", signal.name())),
                Err(e) => HelperResponse::err(e.to_string()),
            }
        }
        HelperRequest::Renice { pid, nice } => {
            if pid <= 1 || !(-20..=19).contains(&nice) {
                return HelperResponse::err("invalid renice request");
            }
            match crate::process::renice_direct(pid, nice) {
                Ok(()) => HelperResponse::ok(format!("priority of {pid} set to {nice}")),
                Err(e) => HelperResponse::err(e.to_string()),
            }
        }
        HelperRequest::RemovePackages {
            source,
            names,
            purge,
        } => match crate::software::remove(source, &names, purge) {
            Ok(output) => HelperResponse {
                ok: true,
                message: format!("removed {}", names.join(", ")),
                output,
                freed_bytes: 0,
            },
            Err(e) => HelperResponse::err(e.to_string()),
        },
        HelperRequest::CleanSystem { targets } => {
            let mut freed = 0;
            let mut errors = Vec::new();
            for t in &targets {
                if !crate::cleaner::SYSTEM_TARGET_IDS.contains(&t.as_str()) {
                    errors.push(format!("{t}: unknown target"));
                    continue;
                }
                match crate::cleaner::clean_system(t) {
                    Ok(b) => freed += b,
                    Err(e) => errors.push(format!("{t}: {e}")),
                }
            }
            HelperResponse {
                ok: errors.is_empty(),
                message: if errors.is_empty() {
                    "cleaned".into()
                } else {
                    errors.join("; ")
                },
                freed_bytes: freed,
                output: String::new(),
            }
        }
        HelperRequest::Service { unit, action } => {
            match crate::services::apply(&unit, action, false) {
                Ok(msg) => HelperResponse::ok(msg),
                Err(e) => HelperResponse::err(e.to_string()),
            }
        }
        HelperRequest::GrantSerialAccess => grant_serial_access(),
        HelperRequest::SetCaptureAccess { grant } => match crate::netlens::capture::set_capture_caps(grant) {
            Ok(m) => HelperResponse::ok(m),
            Err(e) => HelperResponse::err(e),
        },
        HelperRequest::DeleteFiles { paths } => {
            if let Some(bad) = paths
                .iter()
                .find(|p| !crate::fsutil::is_user_deletable(std::path::Path::new(p)))
            {
                return HelperResponse::err(format!("refusing to delete protected path {bad}"));
            }
            let r = crate::largefiles::delete_files(&paths);
            HelperResponse {
                ok: r.errors.is_empty() && r.denied.is_empty(),
                message: if r.errors.is_empty() {
                    format!("deleted {} file(s)", r.done.len())
                } else {
                    r.errors.join("; ")
                },
                freed_bytes: r.freed_bytes,
                output: String::new(),
            }
        }
    }
}

/// usermod -aG dialout <user>, where the user is the one pkexec authenticated
/// (never a name supplied by the caller).
fn grant_serial_access() -> HelperResponse {
    let Some(uid) = std::env::var("PKEXEC_UID")
        .ok()
        .and_then(|u| u.parse::<u32>().ok())
    else {
        return HelperResponse::err("PKEXEC_UID not set (helper must run via pkexec)");
    };
    if uid == 0 {
        return HelperResponse::err("refusing to modify root");
    }
    let name = std::fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            (f.get(2) == Some(&uid.to_string().as_str())).then(|| f[0].to_string())
        });
    let Some(name) = name.filter(|n| {
        !n.is_empty()
            && n.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    }) else {
        return HelperResponse::err("cannot resolve the user name");
    };
    match std::process::Command::new("usermod")
        .args(["-aG", "dialout", &name])
        .output()
    {
        Ok(o) if o.status.success() => HelperResponse::ok(format!(
            "{name} added to dialout — log out and back in to use serial ports"
        )),
        Ok(o) => HelperResponse::err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => HelperResponse::err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_json_roundtrip() {
        let reqs = vec![
            HelperRequest::CleanSystem {
                targets: vec!["apt-cache".into()],
            },
            HelperRequest::RemovePackages {
                source: PackageSource::Snap,
                names: vec!["hello".into()],
                purge: false,
            },
        ];
        let json = serde_json::to_string(&reqs).unwrap();
        assert!(json.contains(r#""op":"cleanSystem""#));
        let back: Vec<HelperRequest> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn executor_rejects_bad_input() {
        assert!(
            !execute(HelperRequest::Signal {
                pid: 1,
                signal: Signal::Kill
            })
            .ok
        );
        assert!(
            !execute(HelperRequest::Renice {
                pid: 1234,
                nice: -50
            })
            .ok
        );
        assert!(
            !execute(HelperRequest::DeleteFiles {
                paths: vec!["/etc/passwd".into()]
            })
            .ok
        );
        let r = execute(HelperRequest::CleanSystem {
            targets: vec!["../../etc".into()],
        });
        assert!(!r.ok);
        let r = execute(HelperRequest::RemovePackages {
            source: PackageSource::Apt,
            names: vec!["-oAPT::x=1".into()],
            purge: false,
        });
        assert!(!r.ok);
    }
}
