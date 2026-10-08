// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/process.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Process table, per-process details and signal delivery
//           (end / kill / stop / continue). Signals to processes owned by
//           another user are retried through pkexec (admin password prompt).
// =============================================================================

use std::collections::HashMap;
use std::time::Instant;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};

use crate::error::{CoreError, Result};
use crate::models::{KillOutcome, ProcessDetails, ProcessInfo, Signal};

pub struct ProcessManager {
    sys: System,
    users: Users,
    last: Instant,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

fn refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_disk_usage()
        .with_tasks()
        .with_user(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_exe(UpdateKind::OnlyIfNotSet)
}

impl ProcessManager {
    pub fn new() -> Self {
        let mut sys = System::new();
        // Prime CPU counters so the first list has meaningful percentages.
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
        ProcessManager {
            sys,
            users: Users::new_with_refreshed_list(),
            last: Instant::now(),
        }
    }

    fn refresh(&mut self) -> f64 {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64().max(0.001);
        self.last = now;
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
        elapsed
    }

    fn user_name(&self, p: &sysinfo::Process) -> String {
        p.user_id()
            .and_then(|uid| self.users.get_user_by_id(uid))
            .map(|u| u.name().to_string())
            .or_else(|| p.user_id().map(|u| u.to_string()))
            .unwrap_or_default()
    }

    fn info(&self, p: &sysinfo::Process, elapsed: f64) -> ProcessInfo {
        let disk = p.disk_usage();
        let cmd: Vec<String> = p
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        ProcessInfo {
            pid: p.pid().as_u32(),
            ppid: p.parent().map(|pp| pp.as_u32()),
            name: p.name().to_string_lossy().into_owned(),
            user: self.user_name(p),
            state: p.status().to_string(),
            cpu: p.cpu_usage(),
            memory: p.memory(),
            virtual_memory: p.virtual_memory(),
            start_time: p.start_time(),
            disk_read_bps: disk.read_bytes as f64 / elapsed,
            disk_write_bps: disk.written_bytes as f64 / elapsed,
            threads: p.tasks().map(|t| t.len().max(1)),
            nice: nice_value(p.pid().as_u32()),
            command: cmd.join(" "),
            exe: p.exe().map(|e| e.to_string_lossy().into_owned()),
        }
    }

    /// Full process table. Kernel threads (no command line, child of kthreadd) are skipped.
    pub fn list(&mut self) -> Vec<ProcessInfo> {
        let elapsed = self.refresh();
        // Thread entries (tasks) are reported separately by sysinfo; keep only real processes.
        self.sys
            .processes()
            .values()
            .filter(|p| p.thread_kind().is_none())
            .filter(|p| !(p.cmd().is_empty() && p.parent().map(|pp| pp.as_u32()) == Some(2)))
            .map(|p| self.info(p, elapsed))
            .collect()
    }

    pub fn details(&mut self, pid: u32) -> Result<ProcessDetails> {
        let spid = Pid::from_u32(pid);
        let elapsed = self.last.elapsed().as_secs_f64().max(0.001);
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[spid]),
            true,
            refresh_kind()
                .with_cwd(UpdateKind::Always)
                .with_root(UpdateKind::Always)
                .with_environ(UpdateKind::Always),
        );
        let p = self
            .sys
            .process(spid)
            .ok_or(CoreError::NoSuchProcess(pid))?;
        let mut children: Vec<u32> = self
            .sys
            .processes()
            .values()
            .filter(|c| c.parent() == Some(spid) && c.thread_kind().is_none())
            .map(|c| c.pid().as_u32())
            .collect();
        children.sort_unstable();
        Ok(ProcessDetails {
            info: self.info(p, elapsed),
            cmdline: p
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect(),
            cwd: p.cwd().map(|c| c.to_string_lossy().into_owned()),
            root: p.root().map(|c| c.to_string_lossy().into_owned()),
            uid: p.user_id().map(|u| **u),
            gid: p.group_id().map(|g| *g),
            open_files: p.open_files(),
            cgroup: cgroup(pid),
            children,
            environment_count: (!p.environ().is_empty()).then(|| p.environ().len()),
            status: status_fields(pid),
        })
    }
}

/// Deliver a signal. On EPERM it is retried through the privileged helper.
pub fn send_signal(pid: u32, signal: Signal) -> Result<KillOutcome> {
    guard(pid)?;
    match signal_direct(pid, signal) {
        Ok(()) => Ok(KillOutcome {
            pid,
            signal,
            elevated: false,
        }),
        Err(CoreError::PermissionDenied) => {
            crate::privileged::run_one(crate::privileged::HelperRequest::Signal { pid, signal })?;
            Ok(KillOutcome {
                pid,
                signal,
                elevated: true,
            })
        }
        Err(e) => Err(e),
    }
}

/// Change scheduling priority (nice -20..19). Raising priority (lower nice)
/// or touching another user's process goes through the helper.
/// Returns true when admin rights were used.
pub fn set_priority(pid: u32, nice: i32) -> Result<bool> {
    guard(pid)?;
    if !(-20..=19).contains(&nice) {
        return Err(CoreError::Refused("nice must be between -20 and 19".into()));
    }
    match renice_direct(pid, nice) {
        Ok(()) => Ok(false),
        Err(CoreError::PermissionDenied) => {
            crate::privileged::run_one(crate::privileged::HelperRequest::Renice { pid, nice })?;
            Ok(true)
        }
        Err(e) => Err(e),
    }
}

fn guard(pid: u32) -> Result<()> {
    if pid <= 1 {
        return Err(CoreError::Refused(
            "refusing to touch PID 0/1 (init)".into(),
        ));
    }
    if pid == std::process::id() {
        return Err(CoreError::Refused(
            "refusing to touch SysPulse itself".into(),
        ));
    }
    Ok(())
}

pub use platform::{renice_direct, signal_direct};

#[cfg(unix)]
mod platform {
    use super::*;
    use nix::errno::Errno;
    use nix::sys::signal::{kill, Signal as NixSignal};
    use nix::unistd::Pid as NixPid;

    /// Send a signal with the caller's own privileges.
    pub fn signal_direct(pid: u32, signal: Signal) -> Result<()> {
        let sig = match signal {
            Signal::Term => NixSignal::SIGTERM,
            Signal::Kill => NixSignal::SIGKILL,
            Signal::Stop => NixSignal::SIGSTOP,
            Signal::Cont => NixSignal::SIGCONT,
            Signal::Hup => NixSignal::SIGHUP,
            Signal::Int => NixSignal::SIGINT,
        };
        match kill(NixPid::from_raw(pid as i32), sig) {
            Ok(()) => Ok(()),
            Err(Errno::ESRCH) => Err(CoreError::NoSuchProcess(pid)),
            Err(Errno::EPERM) => Err(CoreError::PermissionDenied),
            Err(e) => Err(CoreError::Other(e.to_string())),
        }
    }

    /// setpriority(PRIO_PROCESS) with the caller's own privileges.
    pub fn renice_direct(pid: u32, nice: i32) -> Result<()> {
        // SAFETY: plain syscall wrapper, arguments are validated integers.
        let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) };
        if rc == 0 {
            return Ok(());
        }
        match Errno::last() {
            Errno::ESRCH => Err(CoreError::NoSuchProcess(pid)),
            Errno::EPERM | Errno::EACCES => Err(CoreError::PermissionDenied),
            e => Err(CoreError::Other(e.to_string())),
        }
    }
}

#[cfg(not(unix))]
mod platform {
    use super::*;

    pub fn renice_direct(_pid: u32, _nice: i32) -> Result<()> {
        Err(CoreError::Unsupported("renice"))
    }

    pub fn signal_direct(pid: u32, signal: Signal) -> Result<()> {
        let mut sys = System::new();
        let spid = Pid::from_u32(pid);
        sys.refresh_processes(ProcessesToUpdate::Some(&[spid]), true);
        let p = sys.process(spid).ok_or(CoreError::NoSuchProcess(pid))?;
        match signal {
            Signal::Term | Signal::Kill | Signal::Int | Signal::Hup => {
                if p.kill() {
                    Ok(())
                } else {
                    Err(CoreError::PermissionDenied)
                }
            }
            _ => Err(CoreError::Unsupported("stop/continue")),
        }
    }
}

#[cfg(target_os = "linux")]
fn nice_value(pid: u32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Fields after the parenthesised comm: state(3) ... nice is field 19 overall.
    let rest = &stat[stat.rfind(')')? + 2..];
    rest.split_whitespace().nth(16)?.parse().ok()
}

#[cfg(not(target_os = "linux"))]
fn nice_value(_pid: u32) -> Option<i32> {
    None
}

#[cfg(target_os = "linux")]
fn cgroup(pid: u32) -> Option<String> {
    let t = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    t.lines()
        .next()
        .and_then(|l| l.splitn(3, ':').nth(2))
        .map(str::to_string)
}

#[cfg(not(target_os = "linux"))]
fn cgroup(_pid: u32) -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn status_fields(pid: u32) -> Vec<(String, String)> {
    const KEEP: [&str; 10] = [
        "State",
        "Tgid",
        "PPid",
        "Threads",
        "VmPeak",
        "VmRSS",
        "VmSwap",
        "RssAnon",
        "RssFile",
        "voluntary_ctxt_switches",
    ];
    let Ok(t) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
        return Vec::new();
    };
    let map: HashMap<&str, &str> = t.lines().filter_map(|l| l.split_once(':')).collect();
    KEEP.iter()
        .filter_map(|k| map.get(k).map(|v| (k.to_string(), v.trim().to_string())))
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn status_fields(_pid: u32) -> Vec<(String, String)> {
    Vec::new()
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFile {
    pub fd: u32,
    pub target: String,
    /// file | socket | pipe | device | anon | other
    pub kind: &'static str,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub protocol: &'static str,
    pub local: String,
    pub remote: String,
    pub state: String,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcessResources {
    pub files: Vec<OpenFile>,
    pub connections: Vec<Connection>,
    pub total_fds: usize,
    /// Another user's process: file descriptors are not readable.
    pub permission_denied: bool,
}

/// Open files and network connections of a process (Linux /proc).
#[cfg(target_os = "linux")]
pub fn resources(pid: u32) -> Result<ProcessResources> {
    const MAX_FILES: usize = 300;
    let fd_dir = format!("/proc/{pid}/fd");
    let rd = match std::fs::read_dir(&fd_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Ok(ProcessResources {
                permission_denied: true,
                ..Default::default()
            })
        }
        Err(_) => return Err(CoreError::NoSuchProcess(pid)),
    };
    let mut files = Vec::new();
    let mut socket_inodes = std::collections::HashSet::new();
    for e in rd.filter_map(std::result::Result::ok) {
        let Ok(fd) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(target) = std::fs::read_link(e.path()) else {
            continue;
        };
        let target = target.to_string_lossy().into_owned();
        let kind = if let Some(inode) = target
            .strip_prefix("socket:[")
            .and_then(|t| t.strip_suffix(']'))
        {
            socket_inodes.insert(inode.to_string());
            "socket"
        } else if target.starts_with("pipe:") {
            "pipe"
        } else if target.starts_with("anon_inode:") {
            "anon"
        } else if target.starts_with("/dev/") {
            "device"
        } else if target.starts_with('/') {
            "file"
        } else {
            "other"
        };
        files.push(OpenFile { fd, target, kind });
    }
    files.sort_by_key(|f| f.fd);
    let total_fds = files.len();
    // Regular files first: they are what users look for.
    files.sort_by_key(|f| (f.kind != "file", f.fd));
    files.truncate(MAX_FILES);

    let mut connections = Vec::new();
    for (proto, file) in [
        ("tcp", "tcp"),
        ("tcp6", "tcp6"),
        ("udp", "udp"),
        ("udp6", "udp6"),
    ] {
        let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/net/{file}")) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 || !socket_inodes.contains(f[9]) {
                continue;
            }
            let state = if proto.starts_with("tcp") {
                tcp_state(f[3])
            } else {
                "—"
            };
            connections.push(Connection {
                protocol: proto,
                local: decode_addr(f[1]).unwrap_or_else(|| f[1].to_string()),
                remote: decode_addr(f[2]).unwrap_or_else(|| f[2].to_string()),
                state: state.to_string(),
            });
        }
    }
    Ok(ProcessResources {
        files,
        connections,
        total_fds,
        permission_denied: false,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn resources(_pid: u32) -> Result<ProcessResources> {
    Ok(ProcessResources::default())
}

fn tcp_state(hex: &str) -> &'static str {
    crate::netconn::tcp_state(hex)
}

/// "ip:port" (IPv6 in brackets) from a /proc/net endpoint.
fn decode_addr(s: &str) -> Option<String> {
    let (ip, port) = crate::netconn::decode_endpoint(s)?;
    Some(if ip.contains(':') {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_self() {
        let mut pm = ProcessManager::new();
        let me = std::process::id();
        assert!(pm.list().iter().any(|p| p.pid == me));
        assert!(pm.details(me).is_ok());
    }

    #[test]
    fn decodes_proc_net_addresses() {
        assert_eq!(
            decode_addr("0100007F:0035").as_deref(),
            Some("127.0.0.1:53")
        );
        assert_eq!(
            decode_addr("00000000000000000000000001000000:1F90").as_deref(),
            Some("[::1]:8080")
        );
        assert_eq!(tcp_state("0A"), "LISTEN");
    }

    #[test]
    fn own_resources_readable() {
        let r = resources(std::process::id()).unwrap();
        assert!(!r.permission_denied);
        assert!(r.total_fds > 0);
    }

    #[test]
    fn refuses_init() {
        assert!(matches!(
            send_signal(1, Signal::Term),
            Err(CoreError::Refused(_))
        ));
    }
}
