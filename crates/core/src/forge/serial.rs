// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/forge/serial.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Serial ports for PacketForge: discovery with USB details,
//           permission check (Linux `dialout` group), and the serial link
//           (8N1-style framing options, flow control, DTR/RTS control,
//           CTS/DSR/DCD/RI monitoring, break).
// =============================================================================

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serialport::{DataBits, FlowControl, Parity, SerialPort, SerialPortType, StopBits};

use super::{Ctx, Dir, ForgeEvent, Link, SessionConfig};
use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortInfo {
    pub path: String,
    /// usb | pci | bluetooth | builtin
    pub kind: &'static str,
    pub description: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    /// The current user can open it (read + write).
    pub accessible: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialAccess {
    pub user: String,
    /// Group that owns serial devices (usually `dialout`).
    pub group: String,
    pub in_group: bool,
    /// In the group in /etc/group but this login session predates it.
    pub needs_relogin: bool,
}

pub const BAUD_RATES: [u32; 18] = [
    300, 1200, 2400, 4800, 9600, 14_400, 19_200, 38_400, 57_600, 115_200, 230_400, 250_000,
    460_800, 500_000, 921_600, 1_000_000, 2_000_000, 3_000_000,
];

#[cfg(unix)]
fn can_open(path: &str) -> bool {
    let Ok(c) = std::ffi::CString::new(path) else {
        return false;
    };
    // SAFETY: access() only reads the NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::R_OK | libc::W_OK) == 0 }
}

#[cfg(not(unix))]
fn can_open(_path: &str) -> bool {
    true
}

/// Built-in ttyS ports exist even without hardware; sysfs `type` 0 means no UART.
#[cfg(target_os = "linux")]
fn phantom(path: &str) -> bool {
    path.strip_prefix("/dev/")
        .filter(|n| n.starts_with("ttyS"))
        .map(|n| crate::util::read_trim(format!("/sys/class/tty/{n}/type")).as_deref() == Some("0"))
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn phantom(_path: &str) -> bool {
    false
}

pub fn list_ports() -> Vec<SerialPortInfo> {
    let mut v: Vec<SerialPortInfo> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !phantom(&p.port_name))
        .map(|p| {
            let accessible = can_open(&p.port_name);
            match p.port_type {
                SerialPortType::UsbPort(u) => {
                    let name = [u.manufacturer.clone(), u.product.clone()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" ");
                    SerialPortInfo {
                        description: if name.is_empty() {
                            format!("USB serial {:04x}:{:04x}", u.vid, u.pid)
                        } else {
                            name
                        },
                        path: p.port_name,
                        kind: "usb",
                        vid: Some(u.vid),
                        pid: Some(u.pid),
                        serial_number: u.serial_number,
                        manufacturer: u.manufacturer,
                        product: u.product,
                        accessible,
                    }
                }
                other => SerialPortInfo {
                    kind: match other {
                        SerialPortType::PciPort => "pci",
                        SerialPortType::BluetoothPort => "bluetooth",
                        _ => "builtin",
                    },
                    description: match other {
                        SerialPortType::PciPort => "PCI serial port".into(),
                        SerialPortType::BluetoothPort => "Bluetooth serial".into(),
                        _ => "Built-in serial port".into(),
                    },
                    path: p.port_name,
                    vid: None,
                    pid: None,
                    serial_number: None,
                    manufacturer: None,
                    product: None,
                    accessible,
                },
            }
        })
        .collect();
    // USB adapters first: they are what people plug in.
    v.sort_by_key(|p| (p.kind != "usb", p.path.clone()));
    v
}

/// Is the current user allowed to use serial ports?
#[cfg(target_os = "linux")]
pub fn access() -> SerialAccess {
    let user = std::env::var("USER").unwrap_or_default();
    let group = "dialout".to_string();
    let group_line = std::fs::read_to_string("/etc/group")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("dialout:"))
        .map(str::to_string)
        .unwrap_or_default();
    let gid: Option<u32> = group_line.split(':').nth(2).and_then(|g| g.parse().ok());
    let listed = group_line
        .split(':')
        .nth(3)
        .unwrap_or("")
        .split(',')
        .any(|u| u == user);
    let in_session = gid.is_some_and(|g| {
        nix::unistd::getgroups()
            .map(|gs| gs.iter().any(|x| x.as_raw() == g))
            .unwrap_or(false)
    });
    SerialAccess {
        user,
        group,
        in_group: in_session,
        needs_relogin: listed && !in_session,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn access() -> SerialAccess {
    SerialAccess {
        user: String::new(),
        group: String::new(),
        in_group: true,
        needs_relogin: false,
    }
}

pub struct SerialLink {
    port: Box<dyn SerialPort>,
    stop: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
    ctx: Arc<Ctx>,
    dtr: bool,
    rts: bool,
    last_lines: Option<(bool, bool, bool, bool)>,
    next_poll: Instant,
}

impl SerialLink {
    pub fn open(config: &SessionConfig, ctx: Arc<Ctx>) -> Result<Self> {
        let SessionConfig::Serial {
            path,
            baud,
            data_bits,
            parity,
            stop_bits,
            flow,
            dtr,
            rts,
        } = config
        else {
            return Err(CoreError::Other("not a serial config".into()));
        };
        if !(50..=12_000_000).contains(baud) {
            return Err(CoreError::Refused(
                "baud rate must be between 50 and 12,000,000".into(),
            ));
        }
        let db = match data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            _ => DataBits::Eight,
        };
        let par = match parity.as_str() {
            "odd" => Parity::Odd,
            "even" => Parity::Even,
            _ => Parity::None,
        };
        let sb = if *stop_bits == 2 {
            StopBits::Two
        } else {
            StopBits::One
        };
        let fc = match flow.as_str() {
            "hardware" => FlowControl::Hardware,
            "software" => FlowControl::Software,
            _ => FlowControl::None,
        };
        let mut port = serialport::new(path, *baud)
            .data_bits(db)
            .parity(par)
            .stop_bits(sb)
            .flow_control(fc)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("Permission denied") {
                    CoreError::Other(format!("{path}: permission denied — add your user to the 'dialout' group (PacketForge can do this for you)"))
                } else if msg.contains("Device or resource busy") {
                    CoreError::Other(format!("{path} is already in use by another program"))
                } else {
                    CoreError::Other(format!("{path}: {msg}"))
                }
            })?;
        if fc != FlowControl::Hardware {
            let _ = port.write_request_to_send(*rts);
        }
        let _ = port.write_data_terminal_ready(*dtr);
        let mut reader = port
            .try_clone()
            .map_err(|e| CoreError::Other(e.to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let dead = Arc::new(AtomicBool::new(false));
        let (s, d, c, label) = (stop.clone(), dead.clone(), ctx.clone(), path.clone());
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 16 * 1024];
            while !s.load(Ordering::Relaxed) {
                match reader.read(&mut buf) {
                    Ok(0) => {}
                    Ok(n) => c.data(Dir::Rx, None, &buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => {
                        d.store(true, Ordering::SeqCst);
                        c.state("closed", format!("{label}: {e} (device unplugged?)"));
                        break;
                    }
                }
            }
        });
        ctx.state("open", format!("{} opened", config.title()));
        Ok(SerialLink {
            port,
            stop,
            dead,
            ctx,
            dtr: *dtr,
            rts: *rts,
            last_lines: None,
            next_poll: Instant::now(),
        })
    }
}

impl Link for SerialLink {
    fn send(&mut self, data: &[u8], _target: Option<&str>) -> Result<()> {
        if self.dead.load(Ordering::SeqCst) {
            return Err(CoreError::Other("port is closed".into()));
        }
        self.port
            .write_all(data)
            .map_err(|e| CoreError::Other(e.to_string()))?;
        self.port
            .flush()
            .map_err(|e| CoreError::Other(e.to_string()))
    }

    /// Poll modem-status lines and report changes.
    fn tick(&mut self) {
        if self.dead.load(Ordering::Relaxed) || Instant::now() < self.next_poll {
            return;
        }
        self.next_poll = Instant::now() + Duration::from_millis(200);
        let lines = (
            self.port.read_clear_to_send().unwrap_or(false),
            self.port.read_data_set_ready().unwrap_or(false),
            self.port.read_carrier_detect().unwrap_or(false),
            self.port.read_ring_indicator().unwrap_or(false),
        );
        if self.last_lines != Some(lines) {
            self.last_lines = Some(lines);
            self.ctx.emit(ForgeEvent::Lines {
                session: self.ctx.id,
                cts: lines.0,
                dsr: lines.1,
                dcd: lines.2,
                ri: lines.3,
                dtr: self.dtr,
                rts: self.rts,
            });
        }
    }

    fn set_lines(&mut self, dtr: Option<bool>, rts: Option<bool>) -> Result<()> {
        if let Some(d) = dtr {
            self.port
                .write_data_terminal_ready(d)
                .map_err(|e| CoreError::Other(e.to_string()))?;
            self.dtr = d;
        }
        if let Some(r) = rts {
            self.port
                .write_request_to_send(r)
                .map_err(|e| CoreError::Other(e.to_string()))?;
            self.rts = r;
        }
        self.last_lines = None; // force a Lines event with the new outputs
        self.next_poll = Instant::now();
        Ok(())
    }

    fn send_break(&mut self, ms: u64) -> Result<()> {
        self.port
            .set_break()
            .map_err(|e| CoreError::Other(e.to_string()))?;
        std::thread::sleep(Duration::from_millis(ms.clamp(1, 5000)));
        self.port
            .clear_break()
            .map_err(|e| CoreError::Other(e.to_string()))?;
        self.ctx.state("open", format!("break sent ({ms} ms)"));
        Ok(())
    }

    fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::super::*;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;

    /// Serial path end-to-end over a pseudo-terminal pair (no hardware needed).
    #[test]
    fn serial_over_pty() {
        let (mut master, mut slave) = (0, 0);
        let mut name = [0 as libc::c_char; 128];
        // SAFETY: openpty fills the two fds and the NUL-terminated name buffer.
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                name.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(rc, 0);
        let path = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: we own the master fd from openpty.
        let mut m = unsafe { std::fs::File::from_raw_fd(master) };

        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let f = Forge::new(Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        }));
        let cfg = SessionConfig::Serial {
            path,
            baud: 115_200,
            data_bits: 8,
            parity: "none".into(),
            stop_bits: 1,
            flow: "none".into(),
            dtr: true,
            rts: false,
        };
        let s = f.open(cfg).unwrap();

        m.write_all(b"OK\r\n").unwrap();
        let want = crate::util::base64(b"OK\r\n");
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut got = false;
        while Instant::now() < deadline && !got {
            if let Ok(ForgeEvent::Data {
                dir: Dir::Rx, data, ..
            }) = rx.recv_timeout(Duration::from_millis(100))
            {
                got = data == want;
            }
        }
        assert!(
            got,
            "serial session received data written to the pty master"
        );

        f.command(
            s.id,
            Command::Send {
                bytes: b"AT\r".to_vec(),
                target: None,
            },
        )
        .unwrap();
        let mut buf = [0u8; 16];
        std::thread::sleep(Duration::from_millis(200));
        let n = m.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"AT\r");
        f.close_all();
        // SAFETY: slave fd from openpty, closed once.
        unsafe { libc::close(slave) };
    }
}
