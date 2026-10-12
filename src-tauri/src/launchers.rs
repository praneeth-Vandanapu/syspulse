// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/launchers.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Tauri commands for App Launchers. Launchers for the account
//           running SysPulse are installed directly; launchers for other
//           accounts go through the root helper (one password prompt).
//           A registry (launchers.json) remembers what was created, for
//           whom, and which folder permissions were added, so launchers can
//           be repaired or removed cleanly later.
// =============================================================================

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use syspulse_core::launcher::install::{self, RemoveSpec, UserResult};
use syspulse_core::launcher::{self, icon, CheckReport, EnvVar, LauncherSpec, ScanResult, UserAccount};
use syspulse_core::privileged::{run_one, HelperRequest, IconPayload};
use tauri::{AppHandle, Manager};

use crate::commands::{blocking, CmdResult};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherRecord {
    pub spec: LauncherSpec,
    pub users: Vec<String>,
    pub results: Vec<UserResult>,
    pub created: u64,
    pub updated: u64,
    #[serde(default)]
    pub icon_preview: Option<String>,
}

fn registry_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("launchers.json"))
}

fn load(app: &AppHandle) -> Vec<LauncherRecord> {
    registry_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, list: &[LauncherRecord]) -> Result<(), String> {
    let p = registry_path(app).ok_or("no config directory")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(list).map_err(err)?).map_err(err)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsersInfo {
    pub users: Vec<UserAccount>,
    pub me: String,
    pub acl_available: bool,
}

#[tauri::command]
pub async fn launcher_users() -> CmdResult<UsersInfo> {
    blocking(|| {
        Ok(UsersInfo {
            users: launcher::users(),
            me: launcher::current_user().unwrap_or_default(),
            acl_available: Path::new("/usr/bin/setfacl").exists(),
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReply {
    pub scan: ScanResult,
    pub suggested_id: String,
    pub suggested_env: Vec<EnvVar>,
    pub taken_ids: Vec<String>,
}

#[tauri::command]
pub async fn launcher_scan(app: AppHandle, folder: String) -> CmdResult<ScanReply> {
    let taken: Vec<String> = load(&app).into_iter().map(|r| r.spec.id).collect();
    blocking(move || {
        let scan = launcher::scan(&folder).map_err(err)?;
        let mut id = launcher::suggest_id(&scan.name);
        let base = id.clone();
        let mut n = 2;
        while taken.contains(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        Ok(ScanReply {
            suggested_env: launcher::suggested_env(&scan),
            suggested_id: id,
            scan,
            taken_ids: taken,
        })
    })
    .await
}

#[tauri::command]
pub async fn launcher_check(spec: LauncherSpec, users: Vec<String>) -> CmdResult<CheckReport> {
    blocking(move || Ok(launcher::check(&spec, &users))).await
}

#[tauri::command]
pub async fn launcher_icon_preview(path: String) -> CmdResult<Option<String>> {
    blocking(move || Ok(icon::preview(Path::new(&path)).and_then(|p| p.0))).await
}

/// Create (or update) a launcher for the given users.
#[tauri::command]
pub async fn launcher_create(app: AppHandle, spec: LauncherSpec, users: Vec<String>) -> CmdResult<Vec<UserResult>> {
    let app2 = app.clone();
    blocking(move || {
        launcher::validate(&spec).map_err(err)?;
        if users.is_empty() {
            return Err("choose at least one user".into());
        }
        let icon_data = if spec.icon.is_empty() {
            None
        } else {
            Some(icon::prepare(Path::new(&spec.icon)).map_err(err)?)
        };
        let me = launcher::current_user().unwrap_or_default();
        let results: Vec<UserResult> = if users.len() == 1 && users[0] == me {
            let u = launcher::find_user(&me).ok_or("cannot resolve the current user")?;
            install::install(&spec, icon_data.as_ref(), &[u]).map_err(err)?
        } else {
            let resp = run_one(HelperRequest::InstallLauncher {
                spec: Box::new(spec.clone()),
                icon: icon_data.as_ref().map(IconPayload::from_icon),
                users: users.clone(),
            });
            match resp {
                Ok(r) => serde_json::from_str(&r.output).map_err(err)?,
                Err(e) => {
                    // A failed batch still reports per-user results in `output`
                    // when the helper ran; otherwise surface the error.
                    return Err(e.to_string());
                }
            }
        };
        // Registry: merge with an existing record of the same id.
        let mut list = load(&app2);
        let preview = if spec.icon.is_empty() { None } else { icon::preview(Path::new(&spec.icon)).and_then(|p| p.0) };
        let ok_users: Vec<String> = results.iter().filter(|r| r.ok).map(|r| r.user.clone()).collect();
        match list.iter_mut().find(|r| r.spec.id == spec.id) {
            Some(rec) => {
                rec.spec = spec.clone();
                for u in &ok_users {
                    if !rec.users.contains(u) {
                        rec.users.push(u.clone());
                    }
                }
                for r in &results {
                    rec.results.retain(|x| x.user != r.user);
                    rec.results.push(r.clone());
                }
                rec.updated = now();
                rec.icon_preview = preview;
            }
            None if !ok_users.is_empty() => list.push(LauncherRecord {
                spec: spec.clone(),
                users: ok_users,
                results: results.clone(),
                created: now(),
                updated: now(),
                icon_preview: preview,
            }),
            None => {}
        }
        save(&app2, &list)?;
        Ok(results)
    })
    .await
}

#[tauri::command]
pub fn launcher_list(app: AppHandle) -> Vec<LauncherRecord> {
    load(&app)
}

/// Remove a launcher from some or all of its users.
#[tauri::command]
pub async fn launcher_remove(app: AppHandle, id: String, users: Vec<String>, revoke_access: bool) -> CmdResult<Vec<UserResult>> {
    blocking(move || {
        let mut list = load(&app);
        let rec = list.iter().find(|r| r.spec.id == id).cloned().ok_or("unknown launcher")?;
        let users: Vec<String> = if users.is_empty() { rec.users.clone() } else { users };
        let acl_parents: HashMap<String, Vec<String>> = rec
            .results
            .iter()
            .filter(|r| users.contains(&r.user))
            .map(|r| (r.user.clone(), r.acl_parents.clone()))
            .collect();
        let spec = RemoveSpec {
            id: id.clone(),
            folder: rec.spec.folder.clone(),
            revoke_access,
            acl_parents,
        };
        let me = launcher::current_user().unwrap_or_default();
        let results: Vec<UserResult> = if users.len() == 1 && users[0] == me {
            let u = launcher::find_user(&me).ok_or("cannot resolve the current user")?;
            install::remove(&spec, &[u]).map_err(err)?
        } else {
            let r = run_one(HelperRequest::RemoveLauncher { remove: spec, users: users.clone() }).map_err(err)?;
            serde_json::from_str(&r.output).map_err(err)?
        };
        let removed: Vec<String> = results.iter().filter(|r| r.ok).map(|r| r.user.clone()).collect();
        if let Some(rec) = list.iter_mut().find(|r| r.spec.id == id) {
            rec.users.retain(|u| !removed.contains(u));
            rec.results.retain(|r| !removed.contains(&r.user));
        }
        list.retain(|r| !r.users.is_empty());
        save(&app, &list)?;
        Ok(results)
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRun {
    /// Still running after the grace period (usually means it started fine).
    pub running: bool,
    pub exit_code: Option<i32>,
    pub output: String,
}

/// Start the app as the current user to see whether it launches.
#[tauri::command]
pub async fn launcher_test_run(spec: LauncherSpec) -> CmdResult<TestRun> {
    blocking(move || {
        launcher::validate(&spec).map_err(err)?;
        let cmd = launcher::command(&spec).map_err(err)?;
        // `env A=B prog args` -> set the variables on the Command instead.
        let mut it = cmd.into_iter().peekable();
        let mut envs = Vec::new();
        if it.peek().map(|s| s == "env").unwrap_or(false) {
            it.next();
            while let Some(a) = it.peek() {
                if let Some((k, v)) = a.split_once('=') {
                    if !k.contains('/') {
                        envs.push((k.to_string(), v.to_string()));
                        it.next();
                        continue;
                    }
                }
                break;
            }
        }
        let prog = it.next().ok_or("empty command")?;
        let log = std::env::temp_dir().join(format!("syspulse-launch-{}.log", spec.id));
        let file = std::fs::File::create(&log).map_err(err)?;
        let file2 = file.try_clone().map_err(err)?;
        let mut child = std::process::Command::new(prog)
            .args(it)
            .envs(envs)
            .current_dir(launcher::workdir(&spec))
            .stdin(std::process::Stdio::null())
            .stdout(file)
            .stderr(file2)
            .spawn()
            .map_err(|e| format!("could not start: {e}"))?;
        let mut exit = None;
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(100));
            if let Ok(Some(st)) = child.try_wait() {
                exit = Some(st.code().unwrap_or(-1));
                break;
            }
        }
        if exit.is_none() {
            // Leave it running for the user; reap it in the background.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        let output = std::fs::read_to_string(&log).unwrap_or_default();
        let output: String = output.chars().rev().take(4000).collect::<Vec<_>>().into_iter().rev().collect();
        Ok(TestRun {
            running: exit.is_none(),
            exit_code: exit,
            output,
        })
    })
    .await
}
