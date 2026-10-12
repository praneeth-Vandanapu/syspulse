<!--
  SysPulse - Multipurpose System Utility
  File   : docs/ARCHITECTURE.md
  Author : Praneeth
  License: MIT
-->
# SysPulse — Architecture, Concepts & Technologies

**Author:** Praneeth · **Version:** 0.7.0 · **License:** MIT

SysPulse is an open-source, all-in-one system utility for Ubuntu 22.04+. It is a Linux-only project. It combines the following:

- System monitor and GPU monitor
- Hardware inventory and process manager
- Software manager, cache cleaner, large-file finder with disk map, and file search
- systemd services and startup apps
- Network settings, iperf-style speed test, network tools and connections view
- PacketForge (TCP / UDP / serial terminal)
- Screen recorder and desktop alerts

This document explains **how it is built and why**. Installation is in [INSTALL.md](INSTALL.md), end-user usage in [USER_MANUAL.md](USER_MANUAL.md), and the feature history in [ROADMAP.md](ROADMAP.md).

---

## Contents

1. [Design goals](#1-design-goals)
2. [Technology stack](#2-technology-stack)
3. [Process model](#3-process-model)
4. [Repository layout](#4-repository-layout)
5. [Core library modules (`crates/core`)](#5-core-library-modules-cratescore)
6. [Application layer (`src-tauri`)](#6-application-layer-src-tauri)
7. [Frontend (`ui`)](#7-frontend-ui)
8. [IPC reference — commands and events](#8-ipc-reference--commands-and-events)
9. [Data flows](#9-data-flows)
10. [Security model](#10-security-model)
11. [Protocols and formats](#11-protocols-and-formats)
12. [Performance engineering](#12-performance-engineering)
13. [Persistence and configuration](#13-persistence-and-configuration)
14. [Testing strategy](#14-testing-strategy)
15. [Packaging](#15-packaging)
16. [Platform scope](#16-platform-scope)
17. [Known limitations](#17-known-limitations)
18. [Coding conventions](#18-coding-conventions)

---

## 1. Design goals

| Goal | How it is met |
|---|---|
| **Light** — a monitor must not be a load itself | Native Rust backend, OS webview (no bundled browser), push-based telemetry; the UI idles at ~3% of one core |
| **Safe** — the app can delete files, kill processes and change networks | GUI never runs as root; a tiny helper runs allow-listed operations behind the polkit password dialog; destructive actions show a preview and ask for confirmation |
| **Honest** — no fake data | Every reading comes from the kernel or system tools; missing data is shown as "—" or "not available" |
| **Maintainable** — Linux first | OS logic lives in one Rust library behind small interfaces; the UI and command layer only call that library |
| **Maintainable** | One crate per concern, unit-tested, zero clippy warnings, every file carries an author header |

---

## 2. Technology stack

| Layer | Technology | Version | Why this choice |
|---|---|---|---|
| Core logic | **Rust** (`syspulse-core` crate) | 1.99 | C-level speed, memory safety for privileged code, excellent system crates |
| Desktop shell | **Tauri 2** | 2.12 | Uses the OS webview (WebKitGTK): ~40 MB RAM vs 300 MB+ for Electron |
| UI framework | **Svelte 5** (runes) + TypeScript | 5.57 / 5.6 | No virtual DOM, fine-grained reactivity for 1 Hz updates, small bundles |
| Build | **Vite** | 6.4 | Instant dev server and HMR |
| Charts | **uPlot** + hand-written SVG / HTML | 1.6.32 | Canvas time series that draws thousands of points in milliseconds; gauges, sparklines and treemap are custom |
| System metrics | `sysinfo` | 0.39 | CPU, memory, disks, networks, processes (cross-platform) |
| NVIDIA | `nvml-wrapper` (NVML, loaded at runtime) | 0.13 | Same source as `nvidia-smi`; no build-time CUDA dependency |
| AMD / Intel GPU | Linux DRM / hwmon sysfs | — | No vendor SDK required; never wakes a sleeping GPU |
| Hardware | sysfs, procfs, `pci.ids`, `usb.ids`, EDID | — | Works without root |
| Packages | dpkg database, snapd REST socket, `flatpak` CLI | — | Fast (2,500 packages in < 0.1 s) |
| Filesystem walking | `jwalk` (parallel) | 0.9 | ~1.7 M files in ~3 s; per-entry work runs on worker threads |
| Trash | `trash` | 5.2 | freedesktop Trash spec (files can be restored from the Trash) |
| Search patterns | `regex` | 1.13 | Regex and wildcard file-name matching |
| systemd | `systemctl --output=json`, `journalctl` | systemd 249 | Stable, scriptable interfaces |
| Networking | NetworkManager `nmcli`, `/proc/net/*`, `ping`, `tracepath` | NM 1.36 | Same backend as Ubuntu Settings |
| Speed test | Own TCP/UDP protocol in Rust (`std::net`) | — | iperf-style tests between two SysPulse instances |
| Serial | `serialport` (libudev) | 4.10 | USB adapter names and serial numbers, modem lines |
| Packet capture | Linux `AF_PACKET` sockets in a capability-limited helper (`syspulse-capture`); BPF filters compiled by the system **libpcap** (loaded with `dlopen`) | libpcap 1.10 | Same kernel interface as tcpdump/Wireshark; no build-time libpcap needed |
| Protocol analysis | Own Rust dissectors (NetLens) + optional **tshark** | — | Fast, safe on hostile input; Wireshark's 3000+ decoders on demand |
| App Launchers | freedesktop Desktop Entry spec, `runuser`, `setfacl`, `gio` metadata, `gsettings`, `image` crate | — | Standard Ubuntu mechanisms only: menu entries, trusted desktop icons (DING), dock favourites, POSIX ACLs |
| Parallel filtering | `rayon` | 1 | Display filters over millions of packets use every core |
| Recorder | **GStreamer** via `gstreamer` crate | 0.25 / GStreamer 1.20 | `ximagesrc` + `pulsesrc` → encoders → muxers; pause without gaps |
| Privileges | `pkexec` + polkit policy + `syspulse-helper` | — | Standard Ubuntu password dialog |
| Tauri plugins | dialog, opener, notification, global-shortcut | 2.x | File dialogs, open/reveal, desktop notifications, recorder hotkeys |
| Tray | Tauri `tray-icon` (libayatana-appindicator) | — | Top-bar recorder menu |
| Packaging | `tauri build` → `.deb`, AppImage | CLI 2.12 | One command for both formats |

---

## 3. Process model

```
┌───────────────────────────── syspulse (normal user) ─────────────────────────────┐
│                                                                                  │
│  WebKitGTK webview  ◄──── events (push) ─────┐                                   │
│  Svelte UI (ui/)    ───── invoke (request) ──┼──►  Tauri command layer           │
│                                              │     (src-tauri/src/*.rs)          │
│                                              │            │                      │
│  Background threads (owned by src-tauri):    │            ▼                      │
│   • telemetry 1 Hz  ─────────────────────────┤     syspulse-core library         │
│   • alert engine                             │     (crates/core) — all OS logic  │
│   • PacketForge sessions + 40 ms batcher ────┤            │                      │
│   • speed-test server / client               │            ├─► /proc, /sys, NVML  │
│   • recorder pipeline, ticker, tray, hotkeys │            ├─► dpkg db, snapd sock │
│   • NetLens reader + app-attribution threads ┘            │                      │
│                                                           ├─► nmcli, systemctl…   │
│                                                           └─► GStreamer           │
└──────────────────────────────────────────┬───────────────────────────────────────┘
                                           │ pkexec (password dialog, only when needed)
                                           ▼
                    ┌──────────── syspulse-helper (root, short-lived) ────────────┐
                    │ JSON request in argv → validate → execute → JSON response   │
                    │ signal · renice · removePackages · cleanSystem · deleteFiles │
                    │ service · grantSerialAccess · setCaptureAccess               │
                    └──────────────────────────────────────────────────────────────┘

        syspulse ──spawn──► syspulse-capture (user, file caps cap_net_raw+cap_net_admin)
                   ◄─stdout─ open AF_PACKET sockets → attach BPF → DROP all caps → stream packets
```

* **Pull** (`invoke`): data the user asks for, such as the process table, hardware inventory, packages and searches.
* **Push** (events): continuous data, such as telemetry, progress, PacketForge traffic and recorder status.
* Long or blocking work always runs on worker threads (`spawn_blocking`), so the UI never freezes.

---

## 4. Repository layout

```
UBUNTU_SYSTEM_Monitor/
├── Cargo.toml                    Rust workspace (crates/core, src-tauri)
├── package.json                  Tauri CLI scripts (dev, build)
├── crates/core/                  syspulse-core — all OS logic, no UI code
│   ├── src/
│   │   ├── lib.rs                module list
│   │   ├── error.rs              CoreError (serialises to a message)
│   │   ├── models.rs             telemetry / process / hardware data types
│   │   ├── util.rs               sysfs readers, PCI/USB ID database, base64, human sizes
│   │   ├── fsutil.rs             disk usage, safe deletion rules, home dir
│   │   ├── monitor.rs            CPU / memory / disks / network / sensors / battery sampler
│   │   ├── gpu/                  mod.rs (manager), nvidia.rs (NVML), drm.rs (AMD/Intel sysfs)
│   │   ├── hardware/             mod.rs, linux.rs (inventory)
│   │   ├── process.rs            process table, details, signals, renice, open files/sockets
│   │   ├── software/             mod.rs, apt.rs, snap.rs, flatpak.rs, desktop.rs, icons.rs
│   │   ├── cleaner.rs            cache targets (user + system)
│   │   ├── largefiles.rs         parallel scanner, folder tree, trash/delete
│   │   ├── search.rs             file search engine
│   │   ├── services.rs           systemd services
│   │   ├── startup.rs            XDG autostart entries
│   │   ├── netconn.rs            system-wide sockets with owners
│   │   ├── alerts.rs             threshold alert engine
│   │   ├── netconfig.rs          NetworkManager: adapters, IP editing, Wi-Fi
│   │   ├── nettools.rs           ping, tracepath, DNS lookup, port check
│   │   ├── netperf/              speed test: mod.rs (protocol), server.rs, client.rs
│   │   ├── forge/                PacketForge: mod.rs (sessions), links.rs, serial.rs, encode.rs
│   │   ├── recorder/             screen recorder: mod.rs (pipeline), sources.rs
│   │   ├── launcher/             App Launchers: mod.rs (scan, checks, entry), install.rs, icon.rs
│   │   ├── netlens/              packet analyzer: mod.rs (engine), capture.rs, sniffer.rs,
│   │   │                         pcapfile.rs, filter.rs, store.rs, views.rs, explain.rs,
│   │   │                         procmap.rs, wireshark.rs, dissect/{mod,net,web,media,dns}.rs
│   │   └── privileged.rs         helper protocol, pkexec launcher, executor
│   └── examples/                 dump, phase2, phase3, netperf, netcfg_selftest, rec_check
├── src-tauri/                    desktop application
│   ├── tauri.conf.json           window, bundle, deb dependencies, polkit file
│   ├── capabilities/             default.json (main window), region.json (region picker)
│   └── src/
│       ├── main.rs               builder, plugins, setup, command registry, exit hook
│       ├── state.rs              shared state, telemetry thread, alert config
│       ├── commands.rs           monitor / process commands, blocking() helper
│       ├── maintenance.rs        software, cleaner, large files
│       ├── tools.rs              search, services, startup, connections, alerts, export
│       ├── network.rs            network settings, tools, speed test
│       ├── forge.rs              PacketForge commands + event batcher
│       ├── recorder.rs           recorder commands, tray, hotkeys, region picker
│       ├── netlens.rs            NetLens commands
│       ├── launchers.rs          App Launchers commands + registry
│       ├── bin/syspulse-helper.rs privileged helper binary
│       └── bin/syspulse-capture.rs packet-capture helper binary
├── ui/                           Svelte frontend
│   └── src/
│       ├── main.ts               mounts App (or RegionPicker for #region)
│       ├── App.svelte            shell: sidebar, router, toasts
│       ├── app.css               design tokens (dark/light), base styles
│       ├── lib/                  api.ts, types.ts, stores.svelte.ts, format.ts, mock.ts,
│       │                         forge.svelte.ts, forgefmt.ts, recorder.ts, netlens.ts
│       ├── lib/components/       Card, Gauge, LineChart, Sparkline, Meter, Stat, Status,
│       │                         Icon, Modal, Toasts, PageHeader, AppIcon, Treemap,
│       │                         ForgeLog, ComingSoon, netlens/{PacketList, DetailTree,
│       │                         HexView, FollowStream}
│       └── pages/                one Svelte file per page (21)
├── packaging/linux/              polkit policy
├── scripts/setup-dev-ubuntu.sh   developer environment setup
└── docs/                         INSTALL, ARCHITECTURE, USER_MANUAL, ROADMAP
```

Size: ~33,000 lines of Rust, Svelte and TypeScript.

---

## 5. Core library modules (`crates/core`)

`syspulse-core` contains all operating-system logic and does not depend on Tauri. It could equally back a CLI or a headless agent; the `examples/` folder shows this.

### 5.1 Monitoring

| Module | Responsibility | Data sources | Key types |
|---|---|---|---|
| `monitor` | One `SystemSampler` instance samples everything each tick; rates are deltas between ticks | `sysinfo`, `/proc/meminfo`, `/proc/diskstats` (sectors, io_ticks → busy %), `/sys/class/hwmon/*` (temps), `/sys/class/power_supply` | `Telemetry`, `CpuSample`, `MemorySample`, `DiskIoSample`, `NetworkSample`, `SensorSample`, `BatterySample` |
| `gpu` | `GpuManager` merges providers by PCI bus id; trait `GpuProvider` | NVML (`nvml-wrapper`, tries `libnvidia-ml.so` then `.so.1`); DRM sysfs `gpu_busy_percent`, `mem_info_vram_*`, `pp_dpm_sclk`, hwmon power/energy, i915/xe frequency files | `GpuSample` (util, VRAM, temp, power/limit, clocks, fan, enc/dec, PCIe, processes, power state) |
| `hardware` | Builds sections of devices with key/value properties | DMI (`/sys/class/dmi/id`), `/proc/cpuinfo`, CPU cache/cpufreq, EFI vars (Secure Boot), DRM connectors + EDID, `/sys/block`, `/sys/class/net`, `/proc/asound`, power_supply, `/sys/bus/usb`, `/sys/bus/pci`, `pci.ids`/`usb.ids` | `HwSection`, `HwDevice`, `HwProperty` |
| `process` | Process table (CPU %, memory, disk rates, threads, nice), details, signals, renice, open files and sockets | `sysinfo`, `/proc/<pid>/{stat,status,cgroup,fd,net/*}` | `ProcessInfo`, `ProcessDetails`, `ProcessResources`, `Signal` |
| `alerts` | `AlertEngine` evaluates each telemetry tick: sustain time, cooldown, hysteresis (3 units) | `Telemetry` | `AlertConfig`, `Alert` |

**Laptop GPU concept:** when a hybrid laptop's dGPU is runtime-suspended (`power/runtime_status = suspended`), NVML is **not queried**, because querying would wake the GPU and drain the battery. The UI shows "Sleeping".

### 5.2 Maintenance

| Module | Responsibility | Notes |
|---|---|---|
| `software::apt` | Parses `/var/lib/dpkg/status`; maps packages to launchers via `/var/lib/dpkg/info/*.list`; install date = `.list` mtime; auto-installed from `extended_states` | Removal plan via `apt-get -s remove` (dry run). Blocks essential, required, desktop-critical packages and the running kernel |
| `software::snap` | `GET /v2/snaps` over `/run/snapd.socket` (HTTP/1.0 by hand) | Old revisions = `/var/lib/snapd/snaps/*.snap` not pointed to by `/snap/<name>/current` |
| `software::flatpak` | `flatpak list --app --columns=…` | User installs removed without root |
| `software::icons` | Resolves icon names in hicolor / Yaru / pixmaps / snap / flatpak dirs → data URL | Absolute paths only from icon directories (prevents arbitrary file reads) |
| `cleaner` | User targets (thumbnails, Trash, GNOME Tracker index, browser / VS Code / pip / npm / Go / Cargo / Gradle / Conda / ML / JetBrains caches, other `~/.cache`) and system targets (APT cache, journal → 100 MB, rotated logs, crash reports, core dumps, old snaps, autoremove) | System targets are **fixed IDs** only the helper can run |
| `largefiles` | `jwalk` with stat on worker threads, top-N heap, folder totals, category totals, 3-level folder tree for the disk map | Progress every 200 ms, cancellable |
| `search` | Name matcher (contains / glob→regex / regex / exact), type, extension, size, date, owner, hidden, noise-dir skipping, content search (binary files skipped, 64 MB cap) — all evaluated inside `jwalk`'s parallel callbacks | Streams hit batches every 150 ms |
| `fsutil` | Allocated size, `clear_dir`, `remove_path`, `is_user_deletable` (protected prefixes, depth rules) | Shared by cleaner, large files and helper |

### 5.3 System

| Module | Responsibility |
|---|---|
| `services` | `systemctl list-units/list-unit-files --output=json` (system or `--user`), `systemctl show` properties, `journalctl` tail, actions; protected units (dbus, logind, journald, udevd, resolved, gdm/display-manager, polkit, accounts-daemon, NetworkManager, `systemd-*`, `user@*`, `getty@*`) cannot be stopped or disabled |
| `startup` | Merges `/etc/xdg/autostart` with `~/.config/autostart`; edits only the user copy (`X-GNOME-Autostart-enabled`), creates and removes user entries |
| `netconn` | Parses `/proc/net/{tcp,tcp6,udp,udp6}`, maps socket inodes → PIDs through `/proc/*/fd` |

### 5.4 Networking

| Module | Responsibility |
|---|---|
| `netconfig` | `nmcli -t` parsing (escaped `\:` aware); devices with live addressing, profiles, settings; validated `connection modify` + optional `connection up`; Wi-Fi scan and radio |
| `nettools` | `ping` (live lines, summary parse), `tracepath -n` hops, DNS via system resolver + `resolvectl dns`, parallel TCP connect port check (≤ 64 ports) |
| `netperf` | iperf-style speed test — see [11.1](#111-speed-test-protocol-netperf) |

### 5.5 PacketForge (`forge`)

| File | Responsibility |
|---|---|
| `forge/mod.rs` | `Forge` session manager (≤ 32 sessions); one control thread per session runs commands, repeat and file-send jobs, modem-line polling and logging; shared `Ctx` emits events and writes the log so the UI and log always agree |
| `forge/links.rs` | `Link` trait implementations: TCP client (auto-reconnect), TCP server (client list, send to one/all, max clients), UDP (bind, broadcast, multicast join, reply-to-last) |
| `forge/serial.rs` | Port discovery (USB VID/PID/serial/manufacturer; phantom `ttyS*` filtered by sysfs `type`), `dialout` check, serial link (data bits, parity, stop bits, flow control, DTR/RTS, CTS/DSR/DCD/RI, break) |
| `forge/encode.rs` | Payload encoding (text with C escapes or Hercules `$HH`, hex in any common notation), line endings, checksums — see [11.2](#112-packetforge-notation) |

### 5.6 Screen recorder (`recorder`)

| File | Responsibility |
|---|---|
| `recorder/sources.rs` | Monitors (`xrandr`), windows (`xprop` EWMH + `xwininfo`), audio devices (`pactl`, with plug-in detection and Bluetooth flag), "follow current output/input" resolution, **encoder probing** (each candidate encodes 2 test frames; cached), snapshot (JPEG) for the region picker |
| `recorder/mod.rs` | `plan()` builds a validated gst-launch description; `Recorder` start / pause / resume / stop (EOS + wait) / cancel; bus thread for errors and audio levels; `AudioMeter` preview pipeline; recordings list |

Pipeline shape (system sound + mic):

```
ximagesrc (region | xid) ! video/x-raw,framerate=F ! videoconvert ! format ! queue ! ENCODER ! parse ! queue ! mux.
audiomixer name=amix ! audioconvert ! AAC|Opus ! queue ! mux.
pulsesrc(system monitor) ! convert ! resample ! 48 kHz stereo ! volume ! level ! queue ! amix.
pulsesrc(microphone)     ! convert ! resample ! 48 kHz stereo ! volume ! level ! queue ! amix.
mp4mux|matroskamux|webmmux name=mux ! filesink
```

* **Encoder choice:** NVENC and VA-API first, *if a probe succeeds*; otherwise x264 / x265 / VP8. GStreamer 1.20's NVENC plugin is incompatible with NVIDIA ≥ 5xx drivers, and the probe detects this automatically.
* **Bitrate:** width × height × fps × bits-per-pixel. The quality preset sets the bits per pixel: Standard 0.06, High 0.10, Max 0.20, and ×0.7 for H.265. The result is clamped to 1–80 Mbit/s.
* **Pause:** the live pipeline goes to PAUSED, which stops running time, so the file has **no gap** (verified: 4.0 s recorded with a 2 s pause → 3.8 s file).

### 5.7 Privilege separation (`privileged`)

`HelperRequest` (JSON, tagged by `op`): `signal`, `renice`, `removePackages`, `cleanSystem`, `deleteFiles`, `service`, `grantSerialAccess`, `setCaptureAccess`, `installLauncher`, `removeLauncher`.

* **App side:** `run_elevated()` runs `pkexec <helper> '<json array>'`. Exit code 126 means the user cancelled; 127 means not authorised.
* **Helper side:** `execute()` validates every argument again, as root. See [10](#10-security-model).

### 5.8 NetLens packet analyzer (`netlens`)

| File | Responsibility |
|---|---|
| `capture.rs` | Interface discovery (`/sys/class/net` + `getifaddrs`, ARPHRD → pcap link type), capture-filter compilation via `dlopen("libpcap.so.0.8")` → `pcap_open_dead` + `pcap_compile`, the helper's binary record stream, starting/stopping the helper, permission check, and the root-side `setxattr(security.capability)` used by `setCaptureAccess` |
| `sniffer.rs` | Runs inside `syspulse-capture`: one `AF_PACKET/SOCK_RAW` socket per interface, BPF attached **before** `bind()`, `SO_TIMESTAMPNS` kernel timestamps, 16 MB receive buffer, `PACKET_STATISTICS` drop counters, loopback de-duplication, then `capset(0)` + `PR_SET_NO_NEW_PRIVS` |
| `pcapfile.rs` | pcap (µs/ns, both endians) and pcapng (SHB/IDB/EPB/SPB, `if_name`, `if_tsresol`, `epb_flags`) reader; pcapng writer with ns timestamps and direction |
| `dissect/mod.rs` | Dissection framework: one pass yields the UI tree (labels + byte ranges + ready-made filters), the flat field list for filters, and typed *facts* for analysis — each only when requested |
| `dissect/net.rs` | Frame, Ethernet, VLAN, Linux cooked v1/v2, loopback, raw IP, ARP, IPv4, IPv6 + extension headers, ICMP/ICMPv6 (quoted datagrams), IGMP, TCP (options, relative sequence numbers, analysis notes), UDP, port/heuristic dispatch |
| `dissect/web.rs` | HTTP/1.x and SSDP; TLS records and handshakes (SNI, ALPN, versions, cipher names, certificate CNs, alerts, ECH); QUIC long/short headers |
| `dissect/media.rs` | RTSP (incl. Transport parsing and `$` interleaved frames), SDP (`a=rtpmap`), RTP with H.264/H.265 NAL types (key frames), RTCP SR/RR/SDES/BYE/NACK/PLI/FIR |
| `dissect/dns.rs` | DNS/mDNS/LLMNR with compression-loop protection, DHCP, NTP |
| `filter.rs` | Display-filter tokenizer, recursive-descent parser and evaluator; registry of ~330 fields (type + description) that also drives UI auto-completion |
| `store.rs` | Chunked byte arena (16 MB chunks) + 48-byte frame records; single-pass analysis: conversations, TCP analysis, request/response timing (DNS, HTTP, RTSP, ping), RTSP→RTP port registration, SDP codecs, RTP loss/jitter (RFC 3550), host names, protocol hierarchy, per-second I/O, insight counters |
| `views.rs` | Visible-window rows, packet detail, conversations, per-app usage, statistics, insights, Follow Stream (TCP reassembly skipping retransmitted bytes, gaps marked) |
| `explain.rs` | Plain-English sentence for the selected frame (uses host names and owning app) |
| `procmap.rs` | Local port → process via `/proc/net/*` inodes and `/proc/<pid>/fd` |
| `wireshark.rs` | Optional: `tshark -V` decode of one frame (indent-parsed into a tree) and "open in Wireshark" |
| `mod.rs` | `NetLens` engine: `RwLock<Store>`, reader thread (batched ingest), attribution/time-limit thread, file loading with progress, save, filter application (parallel pass under a read lock, then catch-up under the write lock) |

**Two-pass economy.** At capture time each frame is dissected once *without* labels to update the analysis; the Info column and detail tree are produced later, only for frames on screen. Filters dissect with field collection only.

### 5.9 App Launchers (`launcher`)

| File | Responsibility |
|---|---|
| `mod.rs` | **Scan** an application folder (≤ 50,000 entries): ELF parsing (machine type; PIE executable vs shared library via `PT_INTERP`), AppImages, scripts (`#!`), Python entry points (`__main__`), jars; ranking by depth, name similarity, execute bit and penalties (install/uninstall/test…); icons with previews; bundled library folders (`LD_LIBRARY_PATH`), Qt plugin folder (`QT_PLUGIN_PATH`), Python venv. **Users** from `/etc/passwd` (uid 1000–59999, real shell; logged in = `/run/user/<uid>` exists; admin = sudo/admin group). **Check**: per-user path traversal with mode bits, groups and ACL entries (`getfacl`), missing libraries (`ldd` with the launcher's library path), architecture, interpreters, desktop-file validation. **Entry**: Desktop Entry rendering with spec-correct `Exec` quoting (reserved characters, `\\` rule, `%%`) |
| `install.rs` | Per-user install/remove. Direct mode for the current account; root mode inside the helper, where **every file in a user's home is written by a `runuser -u <user>` child** (no root writes into homes, no symlink redirection). D-Bus work (`gio set metadata::trusted`, `gsettings` favourites) uses the user's session bus when logged in, otherwise `dbus-run-session`. Root itself only adds the program's execute bit and ACLs (`setfacl -m u:<user>:--x` on blocking parents, `-R -P` and default `rX`/`rwX` on the folder) |
| `icon.rs` | Previews (data URLs) and conversion of PNG/ICO/JPEG/BMP to a centred 256×256 PNG; SVG/XPM installed as-is after a format check |

The app keeps a registry (`launchers.json`) with each spec, its users and the ACL parents added per user, so **Remove** can revoke exactly what was granted.

---

## 6. Application layer (`src-tauri`)

A thin layer: commands translate IPC to `syspulse-core` calls, and background threads push events.

| File | Contents |
|---|---|
| `main.rs` | Plugins (global-shortcut, dialog, opener, notification), managed state (`AppState`, `RecorderState`, `ForgeState`, `NetLensState`), setup (alerts config, PacketForge, telemetry thread, recorder tray and hotkeys), command registry; `RunEvent::Exit` finalises an active recording and stops a running capture |
| `state.rs` | `AppState` (refresh interval, process manager, latest GPUs, hardware cache, cancel flags, alert config, speed-test server); telemetry thread (sample → GPU → emit → alert engine → notification) |
| `commands.rs` | Monitor and process commands; `blocking()` helper (`spawn_blocking`) |
| `maintenance.rs` | Software, cleaner, large files; splits cleaner IDs into user (direct) and system (helper) |
| `tools.rs` | Search, services, startup, connections, alert config, CSV/TXT export |
| `network.rs` | Network settings, tools (with live line events), speed-test server lifecycle and client runs |
| `forge.rs` | PacketForge commands; **event batcher** collects session events and emits one `forge-events` array every ~40 ms |
| `recorder.rs` | Countdown → hide window → start; stop/save/cancel; tray menu (status line, Start, Pause/Resume, Stop and save, Cancel, Open, Quit) with a red/amber icon drawn in code and a live "REC mm:ss" label; global hotkeys `Ctrl+Alt+R` / `Ctrl+Alt+P`; 1 s ticker; region-picker window |
| `netlens.rs` | NetLens commands; the UI pulls a status poll (2 Hz) and only the visible rows, so capture rate never floods the webview; capture is stopped on exit |
| `launchers.rs` | App Launchers commands; installs for the current user directly, for others via the helper; maintains `launchers.json`; test run with captured output |
| `bin/syspulse-capture.rs` | Capture helper entry point (`--check`, `capture ...`) |
| `bin/syspulse-helper.rs` | Root helper: requires euid 0, fixed `PATH`, payload ≤ 256 KB and ≤ 32 requests, logs the requesting `PKEXEC_UID` |

---

## 7. Frontend (`ui`)

| Part | Description |
|---|---|
| `App.svelte` | Sidebar groups (Monitor, Network, System, Maintenance, Media), page router (`app.page`), global toasts, listens to the `navigate` event |
| `lib/api.ts` | Typed `invoke` wrappers and event subscriptions. In a plain browser it falls back to `lib/mock.ts`, so the UI can be developed without Tauri |
| `lib/stores.svelte.ts` | `AppState` class with runes: latest telemetry, ring-buffer history (default 120 samples) for charts, settings (persisted in `localStorage`), toasts |
| `lib/forge.svelte.ts` | PacketForge store: sessions, records with **framing** (pause-gap default, newline/delimiter with 500 ms idle close, fixed length, chunk), macros and profiles, throttled re-render (~10 fps) |
| `lib/forgefmt.ts` | Byte renderers: text with C escapes, hex, `hexdump -C`, dec, bin, UTF-8 validation |
| `lib/recorder.ts` | Recorder types, API wrappers, "follow" constants, browser demo data |
| `lib/netlens.ts` + `components/netlens/` | NetLens types, filter presets; `PacketList` (virtualised: fetches only visible rows, scales past WebKit's height limit), `DetailTree` (remembers expansion per field), `HexView` (two-way linking with the tree), `FollowStream` |
| Components | `LineChart` (uPlot; gradient fill, crosshair tooltip, theme-aware, elapsed mode), `Gauge` (SVG arc), `Sparkline`, `Meter`, `Treemap` (squarified, zoomable), `ForgeLog` (windowed log view), `Modal`, `Toasts`, `AppIcon`, … |
| Pages | Dashboard, Performance, GPU, Processes, Network Settings, Speed Test, Net Tools, PacketForge, NetLens, Connections, Hardware, Services, Startup Apps, Software, Cache Cleaner, Large Files, File Search, Screen Recorder, Settings, RegionPicker (separate window) |
| Theme | Design tokens in `app.css` (dark default + light); colour-blind-validated categorical series colours; status colours always paired with an icon or text |

---

## 8. IPC reference — commands and events

### 8.1 Commands (`invoke`)

| Group (file) | Commands |
|---|---|
| Monitor (`commands.rs`) | `system_summary`, `hardware_inventory`, `list_processes`, `process_details`, `process_resources`, `signal_process`, `set_process_priority`, `set_refresh_interval` |
| Maintenance (`maintenance.rs`) | `list_packages`, `package_icon`, `plan_package_removal`, `remove_packages`, `cleaner_scan`, `cleaner_clean`, `large_scan`, `large_cancel`, `files_trash`, `files_delete`, `reveal_path` |
| Tools (`tools.rs`) | `search_files`, `search_cancel`, `open_path`, `list_services`, `service_details`, `service_action`, `list_startup`, `set_startup_enabled`, `save_startup`, `remove_startup`, `list_connections`, `get_alert_config`, `set_alert_config`, `export_text` |
| Network (`network.rs`) | `net_devices`, `net_connections`, `net_connection_settings`, `net_update_connection`, `net_activate`, `wifi_scan`, `wifi_radio`, `net_ping`, `net_trace`, `net_tool_cancel`, `net_lookup`, `net_ports`, `perf_server_start`, `perf_server_stop`, `perf_server_status`, `perf_discover`, `perf_run`, `perf_cancel` |
| PacketForge (`forge.rs`) | `forge_ports`, `forge_serial_access`, `forge_grant_serial`, `forge_open`, `forge_close`, `forge_sessions`, `forge_encode`, `forge_send`, `forge_repeat`, `forge_stop_jobs`, `forge_send_file`, `forge_lines`, `forge_break`, `forge_log`, `forge_store_load`, `forge_store_save` |
| App Launchers (`launchers.rs`) | `launcher_users`, `launcher_scan`, `launcher_check`, `launcher_icon_preview`, `launcher_create`, `launcher_list`, `launcher_remove`, `launcher_test_run` |
| NetLens (`netlens.rs`) | `netlens_setup`, `netlens_interfaces`, `netlens_grant`, `netlens_start`, `netlens_stop`, `netlens_clear`, `netlens_status`, `netlens_rows`, `netlens_detail`, `netlens_set_filter`, `netlens_check_filter`, `netlens_check_capture_filter`, `netlens_fields`, `netlens_conversations`, `netlens_apps`, `netlens_stats`, `netlens_insights`, `netlens_rtp`, `netlens_follow`, `netlens_locate`, `netlens_open`, `netlens_save`, `netlens_wireshark_decode`, `netlens_open_wireshark` |
| Recorder (`recorder.rs`) | `rec_sources`, `rec_audio`, `rec_defaults`, `rec_save_defaults`, `rec_start`, `rec_toggle_pause`, `rec_stop`, `rec_status`, `rec_list`, `rec_meter`, `rec_pick_region`, `rec_region_snapshot`, `rec_region_done` |

### 8.2 Events (`listen`)

| Event | Payload | Rate |
|---|---|---|
| `telemetry` | `Telemetry` (CPU, memory, filesystems, disk I/O, network, sensors, battery, GPUs) | every refresh interval (default 1 s) |
| `alert` | `Alert` | when a threshold fires |
| `largefiles-progress` / `search-progress` | progress (+ hit batches for search) | 5–7 per second |
| `nettool-line` | one output line of ping / tracepath | as produced |
| `perf-progress` | `PerfProgress` per interval | per test interval |
| `perf-server-event` | test started / interval / finished / rejected | as they happen |
| `forge-events` | array of `ForgeEvent` (data base64, state, clients, lines, job) | ≤ 25 per second (batched) |
| `recorder-status`, `recorder-countdown`, `recorder-event` (levels, errors), `recorder-finished`, `recorder-error`, `recorder-cancelled` | recorder state | 1 s ticker, levels ~10/s |
| `region-selected` | rectangle or null | once |
| `navigate` | page id | e.g. return to the recorder after stopping |

NetLens sends no events: its UI pulls status and visible rows (see [9](#9-data-flows)). The TypeScript mirrors of all payloads are in `ui/src/lib/types.ts`, `forge.svelte.ts`, `recorder.ts` and `netlens.ts`. Serde uses camelCase. The PacketForge contract is locked by the `forge::tests::ui_contract` test.

---

## 9. Data flows

**Telemetry:** the telemetry thread calls `SystemSampler::sample()` and then `GpuManager::sample()`. It stores the latest GPUs (used by the hardware inventory), emits `telemetry`, runs `AlertEngine::check()` and raises a notification if needed. In the UI, `stores.ingest()` pushes into the ring buffers and the charts update.

**Uninstall:** `plan_package_removal` runs a dry run and the UI shows exactly what will be removed and the space freed. On confirmation, `remove_packages` calls the helper through `pkexec`. The helper **re-plans as root** and refuses any blocker, then `apt-get remove` runs and the list reloads.

**Large-file scan / search:** the command spawns a blocking task. jwalk worker threads do the stat (plus content matching for search). The main loop aggregates and emits progress or batches, and the final result is returned to the `invoke` call. A cancel flag in `AppState` stops the walk.

**PacketForge:** each link's reader thread calls `Ctx::data` (log line plus event). Events go through the channel to the batcher, which emits `forge-events`. The store decodes base64 and frames the bytes into records, and `ForgeLog` renders the newest window of them.

**NetLens capture:** `netlens_start` compiles the capture filter for each interface's link type (libpcap via `dlopen`), creates a fresh `Store` and spawns `syspulse-capture`. The helper streams binary records over its stdout. The reader thread drains everything already buffered (up to 1,024 records), takes the store's write lock once and calls `Store::add` for each frame, which dissects it without labels and updates the analysis. The attribution thread maps new connections to processes every 0.4 s. The UI polls `netlens_status` twice a second, and `PacketList` asks `netlens_rows` only for the rows on screen; selecting a row calls `netlens_detail`, which re-dissects that single frame with labels. NetLens uses no push events.

**Recorder start:** the user (or a hotkey or the tray) triggers `start_flow`. It releases the audio meter, emits countdown events, hides the main window, then calls `Recorder::start`. That resolves "follow" devices, plans and launches the pipeline. A 1 s ticker updates the UI and tray. Stop sends EOS, waits for the file to finalise, emits `recorder-finished` and shows the window again.

---

## 10. Security model

1. **Least privilege:** the GUI is never root. Only `syspulse-helper` runs as root, only via `pkexec`, only for one batch of requests, then exits.
2. **Allow-listed operations only.** The helper takes no shell commands and no arbitrary paths for cleaning:
   * `cleanSystem` accepts only the 7 fixed target IDs.
   * `deleteFiles` accepts only absolute, normalised paths under `/home/`, `/tmp/`, `/var/tmp/`, `/opt/`, `/srv/`, `/mnt/`, `/media/`, `/data/`. Paths must be at least 4 components deep, and system prefixes (`/etc`, `/usr`, `/boot`, `/var/lib`, `/root`, …) are always refused.
   * `removePackages` validates name characters per package system and re-runs the safety plan as root.
   * `service` validates unit names, and core units cannot be stopped or disabled.
   * `grantSerialAccess` derives the user from `PKEXEC_UID`, never from the caller.
   * `setCaptureAccess` sets/removes file capabilities only on the `syspulse-capture` binary next to the helper (or `/usr/bin`), checked to be a regular file — the caller cannot name a path.
   * `signal` and `renice` refuse PID 0/1 and the helper itself.
   * `installLauncher` / `removeLauncher`: target accounts must be real login users (not root); the spec is re-validated (id charset, no control characters, absolute normalised paths, program canonically inside the folder, system folders refused, interpreter allow-list); the icon arrives as validated bytes (PNG/SVG/XPM signature, ≤ 2 MB) — the helper never opens the icon source. Files in homes are written as the user via `runuser`; nothing from the application folder is executed as root; ACL removal is limited to the folder and its recorded ancestors.
3. **Polkit:** action `io.github.praneeth.syspulse.helper` with `auth_admin_keep` for active sessions (one password covers a few minutes).
4. **Input validation everywhere:**
   * Host names for ping and trace.
   * IP / CIDR / DNS / MTU and method whitelists for NetworkManager.
   * Unit names and autostart IDs.
   * Recorder device names and window IDs (`[A-Za-z0-9._:@-]`), and gst-launch values quoted.
   * PacketForge payload size (≤ 1 MiB) and UDP datagram size.
5. **Packet capture is isolated:**
   * Only `syspulse-capture` holds `cap_net_raw,cap_net_admin` (never root, never the GUI — GTK refuses to run with elevated rights anyway).
   * It accepts only interface names (validated) and pre-compiled BPF bytecode, opens its sockets, then drops **all** capabilities and sets `no_new_privs` before reading a single packet.
   * Any modification of the binary clears its capabilities (kernel rule), so a replaced helper is never privileged.
   * Dissectors use bounds-checked reads only and are fuzz-tested with random frames on every link type.
6. **Network listeners are opt-in:**
   * The speed-test server runs only while Server mode is on. It runs one test at a time, accepts data only from the session's peer, and can require a 6-digit access code.
   * The PacketForge TCP server binds to the address you choose (127.0.0.1 is offered).
7. **Webview hardening:** strict CSP (`default-src 'self'`). Capabilities grant only `core:default` and the dialog permissions; the region-picker window has its own minimal capability.
8. **Privacy:** no telemetry or uploads. NetLens keeps captures in memory and writes files only when you save. Settings stay in `~/.config/io.github.praneeth.syspulse`.

---

## 11. Protocols and formats

### 11.1 Speed-test protocol (`netperf`)

* **Port:** one TCP port (default **5201**) and the same **UDP** port.
* **Control connection:** one JSON object per line.
  `hello{protocol:"syspulse-perf/1", role:"control", code?, host}` → `start{spec}` → `ready{session, udpPort, host}` → `interval{dir, stat}`… → `done{sentPackets?, sentBytes?}` → `result{upload?: ReceiveStats, downloadSent?: SendStats}`.
* **Data connections (TCP):** `hello{role:"data", session, stream, dir}`, then raw bytes in 128 KB writes. The receiver counts bytes per interval, and the sender adds kernel `TCP_INFO` (`tcpi_total_retrans`, `tcpi_rtt`).
* **UDP packets:** 28-byte header `[type u8][3 reserved][session u64][seq u64][send-time ns u64]`. Types: 1 data, 2 echo request, 3 echo reply, 4 hello (download target), 5 discover, 6 fin, 7 discover reply.
* **UDP metrics:** loss = expected − received (from the sender's final count); out-of-order = seq < max seen; jitter = RFC 3550 (`J += (|ΔD| − J)/16`).
* **Latency:** UDP echo at N probes/s. Reported as min, avg, p50, p90, p99, max, standard deviation, jitter (mean |ΔRTT|) and loss.
* **Discovery:** a UDP broadcast of `0x05 "SYSPULSE-DISCOVER"` to 255.255.255.255 and each interface's directed broadcast address. Servers reply with JSON `{host, port, codeRequired}`.
* **Throughput** is always the receiver's measurement, like iperf.

### 11.2 PacketForge notation

| Purpose | Notation |
|---|---|
| Text input | C escapes `\r \n \t \0 \e \\ \xHH` (opt-in Hercules: `$HH`, `$$` = `$`) |
| Hex input | `48 65 6C`, `0x48,0x65`, `48-65`, `48:65`, `48656C` |
| Text display | printable characters; others as `\r`, `\n`, `\xHH` (Hercules mode: `{0D}`) |
| Hex display | `48 65 6C` (uppercase) |
| Dump | `hexdump -C` layout |
| Checksums | CRC-16/MODBUS (LE), CRC-16/CCITT-FALSE (BE), CRC-32/IEEE (BE), XOR-8, SUM-8, LRC — verified against the standard `"123456789"` check values |
| Log file line | `ISO-8601Z RX|TX [peer] [len] HEX  |escaped|` |

### 11.3 NetLens capture stream and filters

* **Helper → app records** (little endian): `1` packet `{iface u8, pkttype u8, pad, ts_ns u64, orig_len u32, cap_len u32, bytes}`, `2` stats `{iface, received u32, dropped u32}`, `3` message `{len u16, utf-8}`, `4` ready `{count u8}`.
* **Helper arguments:** `capture [--promisc] [--snaplen N] --iface NAME[=BPF-hex]…` — BPF as 16 hex digits per instruction (`code jt jf k`), ≤ 4096 instructions.
* **Display filter grammar:** `expr := or; or := and ('||'|'or' and)*; and := unary ('&&'|'and' unary)*; unary := ('!'|'not') unary | '(' or ')' | test; test := field [op value | 'in' '{' values '}']`. Operators `== != > >= < <= contains matches ~`. Values: numbers (dec/hex), IPv4/IPv6 with optional `/prefix`, MAC, quoted strings, byte strings `aa:bb:cc`. Multi-occurrence fields match if **any** occurrence matches; `!=` means "none equals".
* **TCP analysis** (per direction): retransmission = data starting below the next expected sequence (keep-alive when 0–1 byte at `next-1`; out-of-order when < 3 ms after the latest data); lost segment = data above the next expected sequence; duplicate ACK = same ACK and window without data while data is outstanding; zero window; port reuse = SYN on a closed connection's tuple.

---

## 12. Performance engineering

| Concern | Technique |
|---|---|
| WebKitGTK paints in software | **No** `box-shadow`, full-window gradients, `backdrop-filter`, SVG blur filters or continuous CSS animations. Each cost 7–15% CPU at 1 Hz; without them the UI idles at ~3% of one core |
| Charts | uPlot canvas; history arrays replaced (not mutated) once per tick; `$state.raw` |
| Big lists | Paging ("Show more") in Software, Search, Large Files; windowed rendering in PacketForge (400 rows, older on demand); fully virtualised NetLens packet list (only visible rows exist in the DOM, height scaled past 30 M px) |
| Packet capture | Helper writes 512 KB batches; ingest takes the write lock once per batch; rows dissected only for the visible window; list virtualised; filters run in parallel (`rayon`) under a read lock so capture continues |
| Fast streams | PacketForge events batched every 40 ms; UI re-renders at ≤ 10 fps; records capped at 50,000 per session |
| Filesystem walks | `jwalk` parallel readdir with per-entry work in worker callbacks; same-filesystem and pseudo-FS pruning |
| Process table | Polled only while the page is visible (2 s) |
| Recorder | x264 `veryfast` ≈ 0.5 core at 1080p30; queues sized by time, not buffers |
| GPU | No polling of suspended GPUs |

---

## 13. Persistence and configuration

| Data | Where | Owner |
|---|---|---|
| UI preferences (theme, refresh rate, history length, view options, recent searches, speed-test form) | Webview `localStorage` | UI |
| Alert thresholds | `~/.config/io.github.praneeth.syspulse/alerts.json` | backend (alerts run without the UI) |
| Recorder defaults (for hotkeys / tray) | `…/recorder.json` | backend |
| PacketForge macros, profiles, history | `…/packetforge.json` | UI via backend |
| Recordings | `~/Videos/SysPulse` (configurable) | backend |
| NetLens options (interfaces, capture filter, limits, split position) | Webview `localStorage` | UI |
| NetLens captures | memory; `.pcapng` where you save; temp copies in `~/.cache/syspulse/netlens/` | backend |

---

## 14. Testing strategy

* **105 Rust tests** (`cargo test --workspace`), including:
  * parsers: dpkg, nmcli terse output, `/proc/net` addresses, EDID, pci.ids, RFC 3339, `apt-get -s` output;
  * safety rules: helper refuses bad input, protected paths, units and packages, gst-launch injection;
  * engines on temporary trees: cleaner, large files, search including content search;
  * loopback end-to-end: speed test (TCP bidirectional, UDP both ways, latency, discovery, access code) and PacketForge (TCP server↔client, UDP reply-to-last, repeat, logging);
  * serial over a **pseudo-terminal pair** (no hardware);
  * a **real recording** of a screen region with pause/resume;
  * the UI↔Rust JSON contract;
  * NetLens: synthetic HTTP session (retransmission, 404, response time, follow without duplicate bytes), RTSP→SDP→RTP (port registration, H.264 key frames, loss), DNS names and timing, TLS SNI, filter grammar and errors, pcap/pcapng round-trips, helper record stream, BPF compilation, and a **fuzz test** pushing 3,000 random frames through every link type;
  * App Launchers: folder scan of a synthetic bundle (ELF vs library detection, scripts, libs, Qt plugins, icons), Exec quoting rules, desktop-file validation, spec rejection, icon conversion, dock favourites parsing. `examples/launcher_selftest.rs` installs a real launcher for the current user (menu, trusted desktop icon, dock), verifies it and removes it again.
* `examples/` are manual checks against the real system. For instance, `netcfg_selftest` edits a throw-away inactive NetworkManager profile and deletes it.
* **UI:** `svelte-check` (zero errors and warnings), plus visual checks in a browser with the mock backend.

---

## 15. Packaging

* `tauri build` produces a `.deb` (package `sys-pulse`) and an AppImage.
* The `.deb` includes `/usr/bin/syspulse`, `/usr/bin/syspulse-helper`, `/usr/bin/syspulse-capture`, the polkit policy, the desktop file and icons. It depends on WebKitGTK, AppIndicator, polkit, pciutils, usbutils, apt, x11-utils, x11-xserver-utils, pulseaudio-utils, libpcap0.8, acl, desktop-file-utils and the GStreamer plugin packages (see [INSTALL.md](INSTALL.md#3-runtime-dependencies-explained)).
* Flatpak and Snap are intentionally **not** used: their sandboxes block `/proc`, other processes, dpkg and system files.

---

## 16. Platform scope

SysPulse targets **Linux only** (Ubuntu 22.04 and newer; other Debian-based distributions are likely to work). A Windows port was evaluated and dropped, so the code is free to use Linux interfaces directly: procfs and sysfs, NetworkManager, systemd, dpkg/Snap/Flatpak, polkit, `AF_PACKET` with file capabilities, X11 and PulseAudio/PipeWire.

Planned Linux coverage: Wayland screen recording through PipeWire, and packages for other distributions (see the roadmap).

---|---|---|
| Metrics | `sysinfo` + procfs | `sysinfo` + PDH (shared code) |
| GPU | NVML + DRM sysfs | NVML + DXGI / D3DKMT |
| Hardware | sysfs | WMI |
| Processes | POSIX signals | `TerminateProcess` (already behind `#[cfg]`) |
| Software | dpkg / snap / flatpak | Registry uninstall keys, winget |
| Privileges | pkexec helper | Elevated helper via UAC |
| Network settings | NetworkManager | `netsh` / WMI |
| Speed test, PacketForge | `std::net`, `serialport` | same code |
| NetLens capture | `AF_PACKET` helper with file capabilities | Npcap driver + elevated helper; dissectors, filters, store and UI unchanged |
| Recorder | `ximagesrc` + `pulsesrc` | `d3d11screencapturesrc` + `wasapisrc` (GStreamer) |

The UI and the Tauri command layer are shared across platforms.

---

## 17. Known limitations

* The screen recorder needs an X11 session; Wayland (PipeWire portal) is planned.
* NVENC is unusable with GStreamer 1.20 and NVIDIA ≥ 5xx drivers, so CPU encoding is used.
* Memory-module (DIMM) details and NIC firmware versions need root and are not shown yet.
* Connections owned by other users show the user but not the process name.
* NetLens does not decrypt TLS/QUIC (no key-log support yet), does not reassemble HTTP bodies across segments in the tree (Follow Stream does), and cannot use Wi-Fi monitor mode.
* PacketForge cannot yet connect to new Wi-Fi networks or provide auto-reply, plotter or decoders (see the roadmap).

---

## 18. Coding conventions

* Every source file starts with a header naming the author (**Praneeth**) and the MIT licence. JSON files carry it in metadata fields instead.
* Rust: `cargo fmt`, `cargo clippy --workspace --all-targets` with zero warnings, errors via `CoreError`, no panics on missing hardware.
* TypeScript: strict mode, and `svelte-check` must be clean.
* UI copy is plain language; destructive actions always confirm and say what will happen.
