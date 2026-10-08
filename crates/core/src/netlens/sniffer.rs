// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/sniffer.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: The capture loop that runs inside `syspulse-capture` (Linux).
//           One AF_PACKET/SOCK_RAW socket per interface; the BPF filter is
//           attached BEFORE the socket is bound so no unfiltered packet slips
//           through; kernel timestamps via SO_TIMESTAMPNS; drop counters via
//           PACKET_STATISTICS. After setup every capability is dropped and
//           PR_SET_NO_NEW_PRIVS is set, so the running process is no more
//           privileged than the user.
// =============================================================================

#![cfg(target_os = "linux")]

use std::io::Write;
use std::os::fd::RawFd;
use std::time::{Duration, Instant};

use super::capture::{bpf_from_hex, encode_message, encode_packet, encode_ready, encode_stats, valid_iface_name, Bpf, MAX_SNAPLEN};

const ETH_P_ALL: u16 = 0x0003;
const PACKET_OUTGOING: u8 = 4;

struct Sock {
    fd: RawFd,
    loopback: bool,
}

fn errno() -> std::io::Error {
    std::io::Error::last_os_error()
}

fn setsockopt<T>(fd: RawFd, level: i32, name: i32, val: &T) -> bool {
    // SAFETY: passes a pointer to a properly sized value owned by the caller.
    unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            val as *const T as *const libc::c_void,
            std::mem::size_of::<T>() as libc::socklen_t,
        ) == 0
    }
}

/// `--check`: can we open a packet socket?
pub fn check() -> i32 {
    // SAFETY: plain socket()/close() calls.
    let fd = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW, 0) };
    if fd < 0 {
        let e = errno();
        println!("{}", if e.raw_os_error() == Some(libc::EPERM) { "no capture permission".to_string() } else { e.to_string() });
        return 3;
    }
    // SAFETY: fd is a valid descriptor we own.
    unsafe { libc::close(fd) };
    println!("ok");
    0
}

fn open(name: &str, bpf: Option<&Bpf>, promisc: bool) -> Result<Sock, String> {
    let cname = std::ffi::CString::new(name).map_err(|_| "bad name".to_string())?;
    // SAFETY: if_nametoindex reads a NUL-terminated string.
    let index = unsafe { libc::if_nametoindex(cname.as_ptr()) };
    if index == 0 {
        return Err(format!("{name}: no such interface"));
    }
    // Protocol 0: receives nothing until bound, so the filter is in place first.
    // SAFETY: socket() with constant arguments.
    let fd = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0) };
    if fd < 0 {
        return Err(format!("{name}: {}", errno()));
    }
    let fail = |msg: String| {
        // SAFETY: closing the descriptor we just opened.
        unsafe { libc::close(fd) };
        Err(msg)
    };
    if let Some(prog) = bpf {
        let mut insns: Vec<libc::sock_filter> = prog
            .iter()
            .map(|&(code, jt, jf, k)| libc::sock_filter { code, jt, jf, k })
            .collect();
        let fprog = libc::sock_fprog {
            len: insns.len() as u16,
            filter: insns.as_mut_ptr(),
        };
        if !setsockopt(fd, libc::SOL_SOCKET, libc::SO_ATTACH_FILTER, &fprog) {
            return fail(format!("{name}: cannot attach capture filter: {}", errno()));
        }
    }
    let on: libc::c_int = 1;
    setsockopt(fd, libc::SOL_SOCKET, libc::SO_TIMESTAMPNS, &on);
    let buf: libc::c_int = 16 * 1024 * 1024;
    if !setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVBUFFORCE, &buf) {
        setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVBUF, &buf);
    }
    // SAFETY: zeroed sockaddr_ll is a valid starting value.
    let mut sll: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
    sll.sll_family = libc::AF_PACKET as u16;
    sll.sll_protocol = ETH_P_ALL.to_be();
    sll.sll_ifindex = index as i32;
    // SAFETY: bind with a correctly sized sockaddr_ll.
    let rc = unsafe {
        libc::bind(
            fd,
            &sll as *const libc::sockaddr_ll as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return fail(format!("{name}: bind failed: {}", errno()));
    }
    if promisc {
        // SAFETY: zeroed packet_mreq is valid.
        let mut mr: libc::packet_mreq = unsafe { std::mem::zeroed() };
        mr.mr_ifindex = index as i32;
        mr.mr_type = libc::PACKET_MR_PROMISC as u16;
        setsockopt(fd, libc::SOL_PACKET, libc::PACKET_ADD_MEMBERSHIP, &mr);
    }
    let loopback = std::fs::read_to_string(format!("/sys/class/net/{name}/type")).map(|t| t.trim() == "772").unwrap_or(false);
    Ok(Sock { fd, loopback })
}

/// Drop every capability and forbid regaining privileges.
fn drop_privileges() {
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    let mut h = Header { version: 0x2008_0522, pid: 0 };
    let d = [Data::default(); 2];
    // SAFETY: capset with the v3 header and two zeroed data structs.
    unsafe {
        libc::syscall(libc::SYS_capset, &mut h as *mut Header, d.as_ptr());
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
    }
}

/// Entry point for `syspulse-capture capture ...`. Returns the exit code.
pub fn run(args: &[String]) -> i32 {
    // SAFETY: prctl/signal with constant arguments.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let mut snaplen = MAX_SNAPLEN;
    let mut promisc = false;
    let mut specs: Vec<(String, Option<Bpf>)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--snaplen" => {
                snaplen = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(MAX_SNAPLEN).clamp(64, MAX_SNAPLEN);
                i += 1;
            }
            "--promisc" => promisc = true,
            "--iface" => {
                let Some(v) = args.get(i + 1) else { return 2 };
                let (name, hex) = v.split_once('=').unwrap_or((v, ""));
                if !valid_iface_name(name) {
                    return 2;
                }
                let bpf = if hex.is_empty() {
                    None
                } else {
                    match bpf_from_hex(hex) {
                        Some(b) => Some(b),
                        None => return 2,
                    }
                };
                specs.push((name.to_string(), bpf));
                i += 1;
            }
            _ => return 2,
        }
        i += 1;
    }
    if specs.is_empty() || specs.len() > 32 {
        return 2;
    }

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::with_capacity(1 << 20, stdout.lock());
    let mut buf = Vec::with_capacity(1 << 16);
    let mut socks = Vec::new();
    for (idx, (name, bpf)) in specs.iter().enumerate() {
        match open(name, bpf.as_ref(), promisc) {
            Ok(s) => socks.push((idx as u8, s)),
            Err(e) => {
                encode_message(&mut buf, &e);
            }
        }
    }
    drop_privileges();
    encode_ready(&mut buf, socks.len() as u8);
    if out.write_all(&buf).and_then(|_| out.flush()).is_err() || socks.is_empty() {
        return if socks.is_empty() { 4 } else { 0 };
    }
    buf.clear();

    let mut pkt = vec![0u8; MAX_SNAPLEN as usize + 64];
    let mut cbuf = [0u8; 128];
    let mut pfds: Vec<libc::pollfd> = socks
        .iter()
        .map(|(_, s)| libc::pollfd { fd: s.fd, events: libc::POLLIN, revents: 0 })
        .collect();
    let mut last_stats = Instant::now();
    loop {
        // SAFETY: pfds is a valid array of pollfd.
        let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, 100) };
        if n < 0 && errno().raw_os_error() != Some(libc::EINTR) {
            return 5;
        }
        for (k, pfd) in pfds.iter().enumerate() {
            if pfd.revents & libc::POLLIN == 0 {
                continue;
            }
            let (idx, sock) = &socks[k];
            for _ in 0..512 {
                // SAFETY: zeroed sockaddr_ll / msghdr are valid; buffers outlive the call.
                let mut from: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
                let mut iov = libc::iovec { iov_base: pkt.as_mut_ptr() as *mut libc::c_void, iov_len: pkt.len() };
                let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
                msg.msg_name = &mut from as *mut libc::sockaddr_ll as *mut libc::c_void;
                msg.msg_namelen = std::mem::size_of::<libc::sockaddr_ll>() as u32;
                msg.msg_iov = &mut iov;
                msg.msg_iovlen = 1;
                msg.msg_control = cbuf.as_mut_ptr() as *mut libc::c_void;
                msg.msg_controllen = cbuf.len();
                // SAFETY: recvmsg into the buffers set up above.
                let r = unsafe { libc::recvmsg(sock.fd, &mut msg, libc::MSG_TRUNC | libc::MSG_DONTWAIT) };
                if r < 0 {
                    break;
                }
                let orig = r as usize;
                if sock.loopback && from.sll_pkttype == PACKET_OUTGOING {
                    continue; // loopback frames are seen twice
                }
                let mut ts_ns = 0u64;
                // SAFETY: walking the control messages the kernel filled in.
                unsafe {
                    let mut c = libc::CMSG_FIRSTHDR(&msg);
                    while !c.is_null() {
                        if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_TIMESTAMPNS {
                            let ts = std::ptr::read_unaligned(libc::CMSG_DATA(c) as *const libc::timespec);
                            ts_ns = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
                        }
                        c = libc::CMSG_NXTHDR(&msg, c);
                    }
                }
                if ts_ns == 0 {
                    ts_ns = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(0);
                }
                let cap = orig.min(pkt.len()).min(snaplen as usize);
                encode_packet(&mut buf, *idx, from.sll_pkttype, ts_ns, orig as u32, &pkt[..cap]);
                if buf.len() > 512 * 1024 {
                    if out.write_all(&buf).is_err() {
                        return 0;
                    }
                    buf.clear();
                }
            }
        }
        if last_stats.elapsed() >= Duration::from_secs(1) {
            last_stats = Instant::now();
            for (idx, s) in &socks {
                // SAFETY: tpacket_stats is plain data sized for PACKET_STATISTICS.
                let mut st: libc::tpacket_stats = unsafe { std::mem::zeroed() };
                let mut len = std::mem::size_of::<libc::tpacket_stats>() as libc::socklen_t;
                let ok = unsafe {
                    libc::getsockopt(s.fd, libc::SOL_PACKET, libc::PACKET_STATISTICS, &mut st as *mut _ as *mut libc::c_void, &mut len)
                } == 0;
                if ok {
                    encode_stats(&mut buf, *idx, st.tp_packets, st.tp_drops);
                }
            }
        }
        if !buf.is_empty() {
            if out.write_all(&buf).and_then(|_| out.flush()).is_err() {
                return 0;
            }
            buf.clear();
        }
    }
}
