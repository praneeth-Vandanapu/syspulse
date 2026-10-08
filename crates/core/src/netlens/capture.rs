// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/capture.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Live packet capture.
//
//           Privilege model (same idea as Wireshark's dumpcap): the GUI never
//           captures itself. A tiny helper, `syspulse-capture`, gets the file
//           capabilities cap_net_raw,cap_net_admin once (via the polkit
//           helper, "Grant capture permission"). It opens one AF_PACKET socket
//           per interface, attaches the compiled capture filter, enables
//           promiscuous mode if asked, then DROPS all capabilities and only
//           streams packets to stdout. It accepts no file paths and runs no
//           other programs.
//
//           Capture filters (BPF syntax, "tcp port 554") are compiled by the
//           system libpcap, loaded at run time (dlopen), so no -dev package or
//           link-time dependency is needed; without libpcap capture still
//           works, only capture filters are unavailable.
//
//           Helper -> app stream (little endian records):
//             1 packet : iface u8, pkttype u8, pad u8, ts_ns u64,
//                        orig_len u32, cap_len u32, bytes[cap_len]
//             2 stats  : iface u8, pad u16, received u32, dropped u32
//             3 message: len u16, utf-8 text
//             4 ready  : count u8
// =============================================================================

use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use serde::Serialize;

use super::dissect::linktype;
use crate::error::{CoreError, Result};

pub const CAPTURE_HELPER: &str = "syspulse-capture";
pub const MAX_SNAPLEN: u32 = 262_144;

// ------------------------------------------------------------ records ---

#[derive(Debug)]
pub enum Record {
    Packet {
        iface: u8,
        outgoing: bool,
        ts_ns: u64,
        orig_len: u32,
        data: Vec<u8>,
    },
    Stats {
        iface: u8,
        received: u32,
        dropped: u32,
    },
    Message(String),
    Ready(u8),
}

pub fn encode_packet(out: &mut Vec<u8>, iface: u8, pkttype: u8, ts_ns: u64, orig_len: u32, data: &[u8]) {
    out.push(1);
    out.push(iface);
    out.push(pkttype);
    out.push(0);
    out.extend_from_slice(&ts_ns.to_le_bytes());
    out.extend_from_slice(&orig_len.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

pub fn encode_stats(out: &mut Vec<u8>, iface: u8, received: u32, dropped: u32) {
    out.push(2);
    out.push(iface);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&received.to_le_bytes());
    out.extend_from_slice(&dropped.to_le_bytes());
}

pub fn encode_message(out: &mut Vec<u8>, msg: &str) {
    let b = &msg.as_bytes()[..msg.len().min(4000)];
    out.push(3);
    out.extend_from_slice(&(b.len() as u16).to_le_bytes());
    out.extend_from_slice(b);
}

pub fn encode_ready(out: &mut Vec<u8>, count: u8) {
    out.push(4);
    out.push(count);
}

/// Linux PACKET_OUTGOING.
const PACKET_OUTGOING: u8 = 4;

pub fn read_record(r: &mut impl Read) -> std::io::Result<Option<Record>> {
    let mut tag = [0u8; 1];
    if r.read(&mut tag)? == 0 {
        return Ok(None);
    }
    Ok(Some(match tag[0] {
        1 => {
            let mut h = [0u8; 19];
            r.read_exact(&mut h)?;
            let cap = u32::from_le_bytes(h[15..19].try_into().unwrap_or_default()) as usize;
            if cap > MAX_SNAPLEN as usize * 2 {
                return Err(std::io::Error::other("corrupt capture stream"));
            }
            let mut data = vec![0u8; cap];
            r.read_exact(&mut data)?;
            Record::Packet {
                iface: h[0],
                outgoing: h[1] == PACKET_OUTGOING,
                ts_ns: u64::from_le_bytes(h[3..11].try_into().unwrap_or_default()),
                orig_len: u32::from_le_bytes(h[11..15].try_into().unwrap_or_default()),
                data,
            }
        }
        2 => {
            let mut h = [0u8; 11];
            r.read_exact(&mut h)?;
            Record::Stats {
                iface: h[0],
                received: u32::from_le_bytes(h[3..7].try_into().unwrap_or_default()),
                dropped: u32::from_le_bytes(h[7..11].try_into().unwrap_or_default()),
            }
        }
        3 => {
            let mut l = [0u8; 2];
            r.read_exact(&mut l)?;
            let mut b = vec![0u8; u16::from_le_bytes(l) as usize];
            r.read_exact(&mut b)?;
            Record::Message(String::from_utf8_lossy(&b).to_string())
        }
        4 => {
            let mut c = [0u8; 1];
            r.read_exact(&mut c)?;
            Record::Ready(c[0])
        }
        _ => return Err(std::io::Error::other("corrupt capture stream")),
    }))
}

// --------------------------------------------------------- interfaces ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceInfo {
    pub name: String,
    /// ethernet, wifi, loopback, bridge, virtual, tunnel, other
    pub kind: String,
    pub up: bool,
    pub mac: String,
    pub addresses: Vec<String>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// pcap link type, None when NetLens cannot capture this interface.
    pub linktype: Option<u16>,
}

fn read_sys(iface: &str, f: &str) -> String {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/{f}"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// pcap link type for a Linux ARPHRD_* device type.
pub fn linktype_for(iface: &str) -> Option<u16> {
    match read_sys(iface, "type").parse::<u32>().ok()? {
        1 | 772 => Some(linktype::ETHERNET),
        65534 | 512 | 768 | 769 | 776 | 778 | 823 => Some(linktype::RAW),
        _ => None,
    }
}

pub fn valid_iface_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 15
        && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-@:".contains(c))
        && n != "."
        && n != ".."
}

fn addresses() -> std::collections::HashMap<String, Vec<String>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = Default::default();
    #[cfg(unix)]
    // SAFETY: getifaddrs allocates a list we walk read-only and free once.
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return map;
        }
        let mut cur = ifap;
        while !cur.is_null() {
            let ifa = &*cur;
            if !ifa.ifa_addr.is_null() && !ifa.ifa_name.is_null() {
                let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy().to_string();
                let fam = (*ifa.ifa_addr).sa_family as i32;
                let addr = if fam == libc::AF_INET {
                    let a = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    Some(std::net::Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr)).to_string())
                } else if fam == libc::AF_INET6 {
                    let a = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                    Some(std::net::Ipv6Addr::from(a.sin6_addr.s6_addr).to_string())
                } else {
                    None
                };
                if let Some(a) = addr {
                    map.entry(name).or_default().push(a);
                }
            }
            cur = ifa.ifa_next;
        }
        libc::freeifaddrs(ifap);
    }
    map
}

pub fn interfaces() -> Vec<InterfaceInfo> {
    let addrs = addresses();
    let mut out: Vec<InterfaceInfo> = std::fs::read_dir("/sys/class/net")
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| valid_iface_name(n))
                .map(|name| {
                    let base = format!("/sys/class/net/{name}");
                    let ty = read_sys(&name, "type");
                    let kind = if ty == "772" {
                        "loopback"
                    } else if std::path::Path::new(&format!("{base}/wireless")).exists()
                        || std::path::Path::new(&format!("{base}/phy80211")).exists()
                    {
                        "wifi"
                    } else if std::path::Path::new(&format!("{base}/bridge")).exists() {
                        "bridge"
                    } else if ty == "65534" || ty == "768" || ty == "776" {
                        "tunnel"
                    } else if std::path::Path::new(&format!("{base}/device")).exists() {
                        "ethernet"
                    } else {
                        "virtual"
                    };
                    let oper = read_sys(&name, "operstate");
                    InterfaceInfo {
                        kind: kind.into(),
                        up: oper == "up" || (kind == "loopback" && read_sys(&name, "flags").ends_with('9'))
                            || oper == "unknown",
                        mac: read_sys(&name, "address"),
                        addresses: addrs.get(&name).cloned().unwrap_or_default(),
                        rx_bytes: read_sys(&name, "statistics/rx_bytes").parse().unwrap_or(0),
                        tx_bytes: read_sys(&name, "statistics/tx_bytes").parse().unwrap_or(0),
                        linktype: linktype_for(&name),
                        name,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let rank = |k: &str| match k {
        "ethernet" => 0,
        "wifi" => 1,
        "loopback" => 3,
        _ => 2,
    };
    out.sort_by(|a, b| (!a.up, rank(&a.kind), &a.name).cmp(&(!b.up, rank(&b.kind), &b.name)));
    out
}

// ---------------------------------------------------------------- BPF ---

/// Compiled classic BPF program (struct sock_filter array).
pub type Bpf = Vec<(u16, u8, u8, u32)>;

#[repr(C)]
struct BpfInsn {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

#[repr(C)]
struct BpfProgram {
    len: u32,
    insns: *mut BpfInsn,
}

/// Compile a capture filter for a link type with the system libpcap.
#[cfg(target_os = "linux")]
pub fn compile_bpf(expr: &str, lt: u16) -> Result<Bpf> {
    use std::ffi::{CStr, CString};
    type OpenDead = unsafe extern "C" fn(i32, i32) -> *mut libc::c_void;
    type Compile = unsafe extern "C" fn(*mut libc::c_void, *mut BpfProgram, *const libc::c_char, i32, u32) -> i32;
    type GetErr = unsafe extern "C" fn(*mut libc::c_void) -> *const libc::c_char;
    type FreeCode = unsafe extern "C" fn(*mut BpfProgram);
    type Close = unsafe extern "C" fn(*mut libc::c_void);

    if expr.len() > 4096 {
        return Err(CoreError::Refused("capture filter too long".into()));
    }
    let dlt = match lt {
        linktype::RAW => 12, // DLT_RAW on Linux
        other => other as i32,
    };
    let cexpr = CString::new(expr).map_err(|_| CoreError::Refused("invalid filter text".into()))?;
    // SAFETY: plain dlopen/dlsym of libpcap's documented C API; every symbol
    // is checked for null before use and handles are released on all paths.
    unsafe {
        let mut lib = std::ptr::null_mut();
        for name in ["libpcap.so.0.8\0", "libpcap.so.1\0", "libpcap.so\0"] {
            lib = libc::dlopen(name.as_ptr() as *const libc::c_char, libc::RTLD_NOW | libc::RTLD_LOCAL);
            if !lib.is_null() {
                break;
            }
        }
        if lib.is_null() {
            return Err(CoreError::Unsupported("capture filters (libpcap is not installed)"));
        }
        let sym = |n: &str| libc::dlsym(lib, n.as_ptr() as *const libc::c_char);
        let (od, cp, ge, fc, cl) = (
            sym("pcap_open_dead\0"),
            sym("pcap_compile\0"),
            sym("pcap_geterr\0"),
            sym("pcap_freecode\0"),
            sym("pcap_close\0"),
        );
        if [od, cp, ge, fc, cl].iter().any(|p| p.is_null()) {
            libc::dlclose(lib);
            return Err(CoreError::Unsupported("capture filters (libpcap symbols missing)"));
        }
        let open_dead: OpenDead = std::mem::transmute(od);
        let compile: Compile = std::mem::transmute(cp);
        let geterr: GetErr = std::mem::transmute(ge);
        let freecode: FreeCode = std::mem::transmute(fc);
        let close: Close = std::mem::transmute(cl);
        let p = open_dead(dlt, MAX_SNAPLEN as i32);
        if p.is_null() {
            libc::dlclose(lib);
            return Err(CoreError::Other("libpcap: cannot create compiler handle".into()));
        }
        let mut prog = BpfProgram { len: 0, insns: std::ptr::null_mut() };
        let rc = compile(p, &mut prog, cexpr.as_ptr(), 1, 0xffff_ffff);
        let result = if rc != 0 {
            let msg = CStr::from_ptr(geterr(p)).to_string_lossy().to_string();
            Err(CoreError::Refused(format!("capture filter: {msg}")))
        } else {
            let insns = std::slice::from_raw_parts(prog.insns, prog.len as usize);
            let v: Bpf = insns.iter().map(|i| (i.code, i.jt, i.jf, i.k)).collect();
            freecode(&mut prog);
            Ok(v)
        };
        close(p);
        libc::dlclose(lib);
        result
    }
}

#[cfg(not(target_os = "linux"))]
pub fn compile_bpf(_expr: &str, _lt: u16) -> Result<Bpf> {
    Err(CoreError::Unsupported("capture filters"))
}

pub fn bpf_to_hex(b: &Bpf) -> String {
    let mut s = String::with_capacity(b.len() * 16);
    for (code, jt, jf, k) in b {
        s.push_str(&format!("{code:04x}{jt:02x}{jf:02x}{k:08x}"));
    }
    s
}

pub fn bpf_from_hex(s: &str) -> Option<Bpf> {
    if !s.len().is_multiple_of(16) || s.len() / 16 > 4096 || s.is_empty() {
        return None;
    }
    (0..s.len() / 16)
        .map(|i| {
            let c = &s[i * 16..i * 16 + 16];
            Some((
                u16::from_str_radix(&c[0..4], 16).ok()?,
                u8::from_str_radix(&c[4..6], 16).ok()?,
                u8::from_str_radix(&c[6..8], 16).ok()?,
                u32::from_str_radix(&c[8..16], 16).ok()?,
            ))
        })
        .collect()
}

// ------------------------------------------------------- permissions ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureAccess {
    pub helper_found: bool,
    pub helper_path: String,
    /// The helper can open capture sockets.
    pub can_capture: bool,
    pub filters_available: bool,
    pub message: String,
}

/// Locate syspulse-capture: installed copy first, then next to `exe_dir`.
pub fn capture_helper_path(exe_dir: Option<PathBuf>) -> Option<PathBuf> {
    let installed = PathBuf::from("/usr/bin").join(CAPTURE_HELPER);
    if installed.is_file() {
        return Some(installed);
    }
    let dir = exe_dir.or_else(|| std::env::current_exe().ok()?.parent().map(|p| p.to_path_buf()))?;
    let p = dir.join(CAPTURE_HELPER);
    p.is_file().then_some(p)
}

pub fn access() -> CaptureAccess {
    let filters_available = compile_bpf("tcp", linktype::ETHERNET).is_ok();
    let Some(path) = capture_helper_path(None) else {
        return CaptureAccess {
            helper_found: false,
            helper_path: String::new(),
            can_capture: false,
            filters_available,
            message: format!("{CAPTURE_HELPER} is not installed"),
        };
    };
    let out = Command::new(&path).arg("--check").stdin(Stdio::null()).output();
    let (can, msg) = match out {
        Ok(o) if o.status.success() => (true, "ready".to_string()),
        Ok(o) => (false, String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Err(e) => (false, e.to_string()),
    };
    CaptureAccess {
        helper_found: true,
        helper_path: path.to_string_lossy().to_string(),
        can_capture: can,
        filters_available,
        message: msg,
    }
}

/// Root side (syspulse-helper): give the capture helper its capabilities.
/// The path is never taken from the caller.
#[cfg(target_os = "linux")]
pub fn set_capture_caps(grant: bool) -> std::result::Result<String, String> {
    let helper_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let path = capture_helper_path(helper_dir).ok_or("syspulse-capture not found")?;
    let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() || path.file_name().and_then(|n| n.to_str()) != Some(CAPTURE_HELPER) {
        return Err("refusing: capture helper is not a regular file".into());
    }
    let cpath = std::ffi::CString::new(path.as_os_str().to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let name = b"security.capability\0";
    // SAFETY: setxattr/removexattr on a NUL-terminated path with a local buffer.
    let rc = unsafe {
        if grant {
            // VFS_CAP_REVISION_2 | VFS_CAP_FLAGS_EFFECTIVE; permitted = NET_ADMIN(12) | NET_RAW(13).
            let mut v = Vec::with_capacity(20);
            v.extend_from_slice(&0x0200_0001u32.to_le_bytes());
            v.extend_from_slice(&((1u32 << 12) | (1u32 << 13)).to_le_bytes());
            v.extend_from_slice(&0u32.to_le_bytes());
            v.extend_from_slice(&0u32.to_le_bytes());
            v.extend_from_slice(&0u32.to_le_bytes());
            libc::setxattr(cpath.as_ptr(), name.as_ptr() as *const libc::c_char, v.as_ptr() as *const libc::c_void, v.len(), 0)
        } else {
            libc::removexattr(cpath.as_ptr(), name.as_ptr() as *const libc::c_char)
        }
    };
    if rc != 0 {
        let e = std::io::Error::last_os_error();
        if !grant && e.raw_os_error() == Some(libc::ENODATA) {
            return Ok("capture permission was not granted".into());
        }
        return Err(format!("cannot update {}: {e}", path.display()));
    }
    Ok(if grant {
        "Capture permission granted".into()
    } else {
        "Capture permission removed".into()
    })
}

#[cfg(not(target_os = "linux"))]
pub fn set_capture_caps(_grant: bool) -> std::result::Result<String, String> {
    Err("not supported on this platform".into())
}

// ------------------------------------------------------- app: spawn ---

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSpec {
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub filter: String,
    #[serde(default = "default_true")]
    pub promiscuous: bool,
    #[serde(default)]
    pub snaplen: u32,
}

fn default_true() -> bool {
    true
}

/// Running helper, its packet stream, and (name, linktype) per interface.
pub type Started = (CaptureProcess, BufReader<std::process::ChildStdout>, Vec<(String, u16)>);

pub struct CaptureProcess {
    child: Child,
}

impl CaptureProcess {
    /// Start the helper. Returns the process and a reader for its stream.
    pub fn start(spec: &CaptureSpec) -> Result<Started> {
        let path = capture_helper_path(None)
            .ok_or_else(|| CoreError::Other(format!("{CAPTURE_HELPER} is not installed")))?;
        if spec.interfaces.is_empty() || spec.interfaces.len() > 32 {
            return Err(CoreError::Refused("choose 1-32 interfaces".into()));
        }
        let mut ifaces = Vec::new();
        let mut cmd = Command::new(path);
        cmd.arg("capture");
        let snap = if spec.snaplen == 0 { MAX_SNAPLEN } else { spec.snaplen.clamp(64, MAX_SNAPLEN) };
        cmd.arg("--snaplen").arg(snap.to_string());
        if spec.promiscuous {
            cmd.arg("--promisc");
        }
        for name in &spec.interfaces {
            if !valid_iface_name(name) {
                return Err(CoreError::Refused(format!("invalid interface name {name}")));
            }
            let lt = linktype_for(name)
                .ok_or_else(|| CoreError::Refused(format!("{name}: this interface type cannot be captured")))?;
            let mut arg = name.clone();
            if !spec.filter.trim().is_empty() {
                let prog = compile_bpf(spec.filter.trim(), lt)?;
                arg = format!("{name}={}", bpf_to_hex(&prog));
            }
            cmd.arg("--iface").arg(arg);
            ifaces.push((name.clone(), lt));
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CoreError::Other(format!("cannot start {CAPTURE_HELPER}: {e}")))?;
        let out = child.stdout.take().ok_or_else(|| CoreError::Other("no capture stream".into()))?;
        Ok((CaptureProcess { child }, BufReader::with_capacity(1 << 20, out), ifaces))
    }

    pub fn stop(&mut self) {
        #[cfg(unix)]
        // SAFETY: signalling our own child process by PID.
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        #[cfg(not(unix))]
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for CaptureProcess {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_roundtrip() {
        let mut b = Vec::new();
        encode_ready(&mut b, 2);
        encode_packet(&mut b, 1, PACKET_OUTGOING, 123, 99, &[1, 2, 3]);
        encode_stats(&mut b, 0, 10, 2);
        encode_message(&mut b, "hello");
        let mut r = std::io::Cursor::new(b);
        assert!(matches!(read_record(&mut r).unwrap(), Some(Record::Ready(2))));
        match read_record(&mut r).unwrap() {
            Some(Record::Packet { iface, outgoing, ts_ns, orig_len, data }) => {
                assert_eq!((iface, outgoing, ts_ns, orig_len, data), (1, true, 123, 99, vec![1, 2, 3]));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(read_record(&mut r).unwrap(), Some(Record::Stats { iface: 0, received: 10, dropped: 2 })));
        assert!(matches!(read_record(&mut r).unwrap(), Some(Record::Message(m)) if m == "hello"));
        assert!(read_record(&mut r).unwrap().is_none());
    }

    #[test]
    fn bpf_hex_roundtrip() {
        let p: Bpf = vec![(0x28, 0, 0, 12), (0x15, 0, 1, 0x86dd), (6, 0, 0, 262144)];
        assert_eq!(bpf_from_hex(&bpf_to_hex(&p)), Some(p));
        assert!(bpf_from_hex("zz").is_none());
    }

    #[test]
    fn compiles_filters_when_libpcap_present() {
        match compile_bpf("tcp port 554", linktype::ETHERNET) {
            Ok(p) => assert!(!p.is_empty()),
            Err(CoreError::Unsupported(_)) => {} // libpcap not installed
            Err(e) => panic!("{e}"),
        }
        if compile_bpf("tcp", linktype::ETHERNET).is_ok() {
            assert!(compile_bpf("tcp port", linktype::ETHERNET).is_err());
            assert!(compile_bpf("ip", linktype::RAW).is_ok());
        }
    }

    #[test]
    fn iface_names() {
        assert!(valid_iface_name("enp3s0"));
        assert!(valid_iface_name("wlp5s0"));
        assert!(!valid_iface_name("../etc"));
        assert!(!valid_iface_name("a very long interface"));
        assert!(!valid_iface_name(""));
    }
}
