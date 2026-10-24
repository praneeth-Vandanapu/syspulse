<!--
  SysPulse - Multipurpose System Utility
  File   : README.md
  Author : Praneeth
  License: MIT
-->
# SysPulse

**A fast, good-looking system monitor and utility suite for Ubuntu (22.04+).**
Author: **Praneeth** · License: **MIT**

SysPulse combines what you normally need several tools for: a resource monitor,
a GPU monitor, a hardware inventory, a process manager, a software manager, a
cache cleaner, a large-file finder with a disk map, file search, a startup-app
and systemd service manager, network settings (IP editing, Wi-Fi), an
iperf-style speed test between two PCs, network tools, **PacketForge** (a
TCP / UDP / serial terminal in the spirit of Hercules and Packet Sender),
**NetLens** (a Wireshark-style packet analyzer that also shows which app sent
each packet), a connections view, **App Launchers** (desktop/menu/dock entries for any user
from an application folder) and desktop alerts. It is built with Rust + Tauri, so it stays light: the UI process idles
at about 3% of one CPU core.

## Features (v0.7)

| Module | What it does |
|---|---|
| **Dashboard** | Live gauges for CPU, memory, GPU and disk; history charts; temperatures; storage; battery; busiest processes |
| **Performance** | Per-core CPU with clocks, memory and swap composition, disk throughput and active time, per-interface network, every hwmon sensor; CSV export |
| **GPU** | NVIDIA (NVML), AMD (amdgpu) and Intel (i915/xe): load, VRAM, temperature, power and limit, clocks, fan, NVENC/NVDEC, PCIe link, processes using the GPU. Sleeping laptop dGPUs are **not woken up** for monitoring |
| **Processes** | Search, filters (all / mine / active), sortable flat or tree view, details panel (command line, cwd, cgroup, open files, network connections, children…), priority, End / Kill / Stop / Continue. Processes owned by other users are handled through the standard admin password prompt |
| **Software** | APT, Snap and Flatpak in one list with icons, sizes and install dates. Uninstall shows exactly which packages will go and how much space is freed; essential and desktop-critical packages are blocked |
| **Cache Cleaner** | Sizes and locations of system caches (APT, journal, old logs, crash reports, old Snap revisions, unused packages), your caches (thumbnails, Trash, search index), browsers and developer tools. System items need one password |
| **Large Files** | Fast parallel scan of any folder or the whole disk with size and type filters; zoomable disk map; biggest folders, hot spots and space by file type; open folder, move to Trash or delete |
| **File Search** | Name search (contains, wildcard, regex, exact), type / extension / size / date / owner filters, search inside text files; results stream in live; open, show in folder, copy path, move to Trash |
| **Services** | systemd system and session services: status, details, recent logs, start / stop / restart / enable / disable (core services protected) |
| **Startup Apps** | Programs started at login: enable / disable, add, edit, remove |
| **Network Settings** | Wired and Wi-Fi adapters with live addressing; switch between DHCP and static IPs (several addresses, gateway, DNS, MTU) and apply instantly; Wi-Fi scanner with channel congestion; saved connections |
| **Speed Test** | Run SysPulse in **Server mode** on one PC and test from another: TCP upload/download/both with parallel streams, retransmits and RTT; UDP at a set bitrate with loss and jitter; latency percentiles; one-click full suite with an exportable report |
| **App Launchers** | Turn an application folder (C++, Python, AppImage, script, jar) into a start-menu entry, trusted desktop icon and dock pin for any user account; detects the program, icon, bundled libraries and Qt plugins; grants folder access to other users only where needed; live checks, test run, edit and clean removal |
| **NetLens** | Live capture on any network card (one-time permission) or open pcap/pcapng; virtual packet list, detail tree linked to hex bytes, plain-English summary per packet; Wireshark-style display filters with auto-completion; TCP, UDP, HTTP, HTTPS/TLS (site names), QUIC, RTSP/SDP, RTP/RTCP (camera stream loss, jitter, key frames), DNS, DHCP, ARP, ICMP; conversations, per-application traffic, statistics, insights; Follow Stream; save as pcapng; optional Wireshark/tshark integration |
| **PacketForge** | TCP client / server, UDP and serial sessions in tabs; traffic as text (C escapes), hex, text+hex, hexdump, decimal or binary with timestamps; framing, highlighting; send text or hex with line endings and checksums (CRC-16, CRC-32, XOR, SUM, LRC), repeat, file send; F-key macros, saved profiles, logging; DTR/RTS and modem-status lines |
| **Net Tools** | Ping with a live chart, traceroute, DNS lookup, port check |
| **Connections** | Every listening port and network connection with its process; ports reachable from the network are highlighted |
| **Alerts** | Desktop notifications for high CPU/GPU temperature, memory pressure, full disks and low battery (thresholds in Settings) |
| **Hardware** | System, motherboard, BIOS/UEFI (Secure Boot), CPU (caches, microcode, features), memory, GPUs and connected displays (EDID), storage (model, firmware), network adapters (driver versions), audio codecs, battery health, USB and PCI devices. Searchable, exportable as a text report |

## Documentation

| Document | For |
|---|---|
| [docs/INSTALL.md](docs/INSTALL.md) | Requirements, required modules and packages, installation, building from source, troubleshooting |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Concepts, every module, technologies and versions, IPC reference, security model, protocols |
| [docs/USER_MANUAL.md](docs/USER_MANUAL.md) | Page-by-page guide, shortcuts, common tasks, FAQ |
| [docs/ROADMAP.md](docs/ROADMAP.md) | What is done and what is planned |

## Administrator actions

SysPulse itself never runs as root. Uninstalling, cleaning system caches,
signalling other users' processes and deleting their files go through a small
helper (`syspulse-helper`) started by `pkexec`. It shows the standard Ubuntu
password dialog and accepts only a fixed list of validated operations. Install
the `.deb` for these features; the AppImage cannot start the helper.

Packet capture (NetLens) uses a second, even smaller helper, `syspulse-capture`.
After a one-time **Grant capture permission** it holds only the
`cap_net_raw,cap_net_admin` capabilities, opens the capture sockets, then drops
every capability before reading packets. Revoke it any time from NetLens'
Capture options.

## Security notes

SysPulse is a system administration tool, so some features need elevated rights.
These are the known limitations of version 0.7, planned to be fixed in a later release:

| Area | Limitation | Until it is fixed |
|---|---|---|
| NetLens capture helper | After **Grant capture permission**, `/usr/bin/syspulse-capture` can be run by every account on the PC, so other local users could capture network traffic | On shared PCs, do not grant capture permission (or revoke it after use in NetLens → Capture options), or restrict it: `sudo chmod 750 /usr/bin/syspulse-capture` |
| Admin password | Polkit remembers the password for a few minutes after an admin action (`auth_admin_keep`), like `sudo` | Lock the screen when leaving the PC |
| Web interface hardening | Commands are not yet restricted per window, and some file-saving commands accept a path from the interface. No exploit is known (the interface never renders untrusted text as HTML); this is defence in depth | — |

Please report security problems privately, as described in [SECURITY.md](SECURITY.md), rather than in a public issue.

## Speed test between two PCs

1. On PC **A**: open *Speed Test* → *Server mode* → *Start server* (default port 5201).
2. On PC **B**: open *Speed Test*, press *Find servers* (or type A's IP) and start a test.
3. If B cannot connect, allow the port on A: `sudo ufw allow 5201` (TCP and UDP).

Headless machines can use the command-line version of the same protocol:

```bash
cargo run --release -p syspulse-core --example netperf -- server
cargo run --release -p syspulse-core --example netperf -- 192.168.0.50 tcp bidir
```

## PacketForge notation

| What | Notation | Example |
|---|---|---|
| Text input / display | C escapes | `AT\r\n`, `\x1B[0m`, `\0`, `\e`, `\\` |
| Hex input | byte pairs, any separator | `01 03 00 00 00 0A`, `0x01,0x03`, `0103` |
| Hex display | uppercase pairs | `41 54 0D 0A` |
| Dump | `hexdump -C` | `00000000  41 54 0d 0a  │AT..│` |
| Hercules mode (optional) | `$HH` input, `{HH}` display | `AT$0D$0A` → `AT{0D}{0A}` |

Serial ports need membership of the `dialout` group; PacketForge offers to add
you (one password prompt, then log out and back in).

## Development setup

```bash
./scripts/setup-dev-ubuntu.sh     # apt libraries + Rust + Node.js (asks for sudo once)
source ~/.cargo/env
npm install && npm --prefix ui install
npm run dev                       # launches the desktop app with hot reload
```

UI-only development in a normal browser (with synthetic data):

```bash
npm run ui:dev                    # http://localhost:1420
```

Useful commands:

```bash
cargo test --workspace                              # Rust unit tests
cargo run -p syspulse-core --example dump           # print one telemetry sample as JSON
cargo run -p syspulse-core --example dump hw        # print the hardware inventory as JSON
npm run ui:check                                    # Svelte/TypeScript type check
npm run build                                       # release .deb and AppImage in target/release/bundle
```

## Project layout

```
crates/core/   Rust library with all OS logic (no UI): monitor, GPU, hardware, processes,
               maintenance, networking, PacketForge, NetLens, launchers, privileged helper
src-tauri/     Desktop shell: Tauri commands, event loops, syspulse-helper and
               syspulse-capture binaries
ui/            Svelte 5 + TypeScript frontend (one page per tool)
packaging/     polkit policy
docs/          Installation, architecture, user manual, roadmap (+ Word versions in docs/word)
scripts/       Developer setup and md2docx.py (rebuilds the Word documents)
```

## Contributing

Every source file starts with a header naming the author and license. Please
keep that convention. Run `cargo fmt`, `cargo clippy --workspace` and
`npm run ui:check` before opening a pull request.

## License

MIT © 2026 Praneeth
