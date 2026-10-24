<!--
  SysPulse - Multipurpose System Utility
  File   : docs/ROADMAP.md
  Author : Praneeth
  License: MIT
-->
# SysPulse — Roadmap

**Author:** Praneeth

## Phase 1 — Monitoring foundation  ✅
- [x] Workspace, Tauri 2 shell, Svelte UI, theme system
- [x] **Dashboard**: live CPU / memory / GPU / disk / network gauges and charts
- [x] **Performance**: per-core CPU, memory and swap, disk I/O, network throughput, temperatures
- [x] **GPU monitor**: NVIDIA (NVML), AMD (amdgpu sysfs), Intel (i915/xe sysfs) — utilisation, VRAM, temperature, power, clocks, fan, encoder/decoder, per-process VRAM
- [x] **Hardware inventory**: system/board/BIOS, CPU, memory, GPUs, storage, PCI, USB, network, audio, battery — with versions, drivers and firmware
- [x] **Processes**: sortable and searchable table, filters (all / mine / active), details panel, End / Kill / Stop / Continue

## Phase 2 — System maintenance  ✅
- [x] `syspulse-helper` + polkit policy (privileged operations, one password prompt per action, allow-listed requests)
- [x] **Software**: APT/dpkg (direct database read), Snap (snapd socket API), Flatpak; icons, size, version, install date; uninstall with a dry-run preview of every package removed; essential/desktop-critical packages blocked
- [x] **Cache cleaner**: APT cache, journal (keep 100 MB), rotated logs, crash reports, core dumps, old Snap revisions, autoremove; thumbnails, Trash, GNOME Tracker index, browser caches, VS Code / pip / npm / Go / Cargo / Gradle / Conda / ML-model / JetBrains caches, other `~/.cache`
- [x] **Large files**: parallel scan (≈1.7 M files in 3 s), size threshold, type filters, hidden files, single filesystem; biggest folders, hot spots, space by type; open folder / Trash / delete (admin for other users' files)
- [x] Process tree view, priority (renice), open files and network connections per process

## Phase 3 — Search and insight  ✅
- [x] **File search**: contains / wildcard / regex / exact, whole-path matching, type and extension filters, size range, modified date, owner, hidden files, noise-folder skipping, content search (parallel, binary files skipped); live streaming results; open / reveal / copy / trash
- [x] **Startup apps**: XDG autostart list (system + user), enable / disable, add / edit / remove (user directory only)
- [x] **Services**: systemd system and user units, status filters, details (PID, memory, CPU, restarts, unit file), recent journal, start / stop / restart / enable / disable (protected core units)
- [x] **Connections**: system-wide TCP/UDP sockets with owning process, listening vs established, well-known port names, network-exposed listeners highlighted
- [x] **Disk map**: zoomable squarified treemap (3 levels) in Large Files
- [x] **Alerts**: CPU/GPU temperature, memory, disk, battery thresholds with sustain time, cooldown and hysteresis; desktop notifications + in-app toasts; settings persisted
- [x] **CSV export** of the performance history

## Phase 3.5 — Network utilities  ✅
- [x] **Network Settings** (NetworkManager): adapters with live IPv4/IPv6, gateway, DNS, MAC, link speed, driver; edit IPv4/IPv6 (DHCP / static, multiple addresses, gateway, DNS, ignore DHCP DNS), MTU, autoconnect; save or save & apply; saved profiles; connect / disconnect
- [x] **Wi-Fi**: radio on/off, scan with signal, channel, band, rate, security; channel-congestion chart; connect to saved networks
- [x] **Speed Test** (iperf-style, SysPulse ↔ SysPulse): Server mode with optional access code and LAN discovery; TCP upload / download / bidirectional with 1–16 streams, retransmits and RTT (TCP_INFO); UDP at a target bitrate with loss, jitter (RFC 3550) and reordering; latency (RTT min/avg/p50/p90/p99/max, jitter, loss); full suite with exportable report; command-line client/server (`--example netperf`)
- [x] **Net Tools**: ping with live RTT chart, traceroute (tracepath), DNS lookup, TCP port check

## Phase 3.6 — PacketForge (TCP / UDP / serial terminal)  ✅
- [x] Sessions in tabs: TCP client (auto-reconnect), TCP server (many clients, send to one/all), UDP (broadcast, multicast, reply-to-last), serial (USB adapter names and serial numbers, any baud, data/parity/stop bits, RTS/CTS and XON/XOFF, DTR/RTS control, CTS/DSR/DCD/RI indicators, break)
- [x] Traffic view in standard notations: text with C escapes, hex, text+hex, `hexdump -C`, decimal, binary; UTF-8 / Latin-1 / ASCII; clock / relative / delta timestamps; optional Hercules notation (`{0D}`, `$0D`)
- [x] Framing of received data: newline, delimiter byte, idle gap, fixed length, as received
- [x] Highlight and filter by text or hex pattern; export view
- [x] Send: text (C escapes) or hex, line endings, checksums (CRC-16/MODBUS, CRC-16/CCITT, CRC-32, XOR-8, SUM-8, LRC), live byte preview, repeat at an interval, send file in chunks, history
- [x] Macros with F1–F12 shortcuts, saved session profiles, logging every byte to a file
- [x] One-click serial permission fix (adds you to `dialout` via the admin helper)
- [ ] Later: auto-reply rules, serial plotter, protocol decoders (Modbus, NMEA), serial↔TCP bridge, scripting

## Phase 3.7 — NetLens (packet capture and analysis)  ✅
- [x] Capture helper `syspulse-capture` (AF_PACKET, BPF capture filters via libpcap, drop counters) with one-click permission grant/revoke; all capabilities dropped after setup
- [x] pcap / pcapng open and save (Wireshark compatible)
- [x] Dissectors: Ethernet, VLAN, Linux cooked, ARP, IPv4/IPv6, ICMP/ICMPv6, IGMP, TCP, UDP, HTTP/1.x, SSDP, TLS (SNI, ALPN, versions, certificates, alerts), QUIC headers, RTSP + SDP + interleaved, RTP (H.264/H.265 key frames), RTCP, DNS/mDNS, DHCP, NTP
- [x] Wireshark-style display filters (≈330 fields, subnets, sets, regex) with auto-completion; capture-filter presets
- [x] TCP analysis (retransmission, out-of-order, lost segment, dup ACK, zero window, keep-alive, port reuse); DNS/HTTP/RTSP/ping response times
- [x] Owning application per connection; host names from DNS / SNI / Host
- [x] Views: packets (virtual list, tree ↔ hex, plain-English summary), conversations, applications, RTP streams, statistics, insights; Follow TCP/UDP stream
- [x] Optional tshark decode and "open in Wireshark"
- [ ] Later: TLS key-log decryption (SSLKEYLOGFILE), QUIC Initial SNI, HTTP object export, ring-buffer capture to disk, Modbus/MQTT decoders

## Phase 3.8 — App Launchers  ✅
- [x] Scan an application folder: native programs (ELF, architecture check), AppImages, scripts, Python (venv aware), Java jars; icons; bundled libraries and Qt plugins
- [x] Launchers for other user accounts: start menu, trusted desktop icon (no "Allow Launching"), dock pin — nothing for the user to do
- [x] Folder access for other users via ACLs (read or read/write), only where needed
- [x] Live checks (permissions per user, missing libraries, desktop-file validation), test run, edit, add users, clean removal
- [ ] Later: system-wide launcher for all current and future users (`/usr/share/applications` + `/etc/skel`), unattended mode from a JSON file, copy-to-/opt option

## Phase 3b — Later
- [ ] Long-term history (SQLite) with day/week charts
- [ ] Optional background file index for instant search
- [ ] Software: available updates, install from search, Flatpak unused runtimes
- [ ] Hardware: memory-module details via helper (dmidecode)
- [ ] Per-process network bandwidth
- [ ] Speed test: iperf3 protocol compatibility, IPv6 server, connecting to new Wi-Fi networks

## Phase 4 — Screen recorder  ✅ (X11) — hidden in public releases (`recorder::ENABLED = false`) until Wayland support and polish are done
- [x] Capture full screen (any monitor), a single window, or a region selected on a frozen snapshot
- [x] Audio per recording: system sound, microphone, both mixed or none; device pickers, volume, live level meters
- [x] MP4 / MKV / WebM; H.264, H.265, VP8; 15–60 fps; quality presets; encoder auto-selection with runtime probing (NVENC / VA-API when usable, x264 / x265 otherwise)
- [x] Pause / resume without gaps; stop finalises the file; cancel discards it
- [x] Countdown, hide SysPulse while recording, global hotkeys (Ctrl+Alt+R start/stop, Ctrl+Alt+P pause)
- [x] Top-bar (tray / AppIndicator) menu with live REC timer: start, pause/resume, stop & save, cancel, open, quit
- [x] Recordings list: play, show in folder, move to Trash
- [ ] Wayland capture via the xdg-desktop-portal ScreenCast + PipeWire
- [ ] NVENC on GStreamer 1.20 + new NVIDIA drivers (needs GStreamer ≥ 1.22 or an ffmpeg backend)
- [ ] Webcam overlay, click highlighting, trimming, configurable hotkeys

## Phase 5 — Linux platform coverage
- [ ] Wayland screen recording (PipeWire / xdg-desktop-portal)
- [ ] Other distributions: Debian, Linux Mint, Fedora (RPM package)
- [ ] AppImage with working admin actions (helper installed outside the read-only image)

> SysPulse is a **Linux-only** project. A Windows port was considered and dropped (decision of 8 October 2026).
