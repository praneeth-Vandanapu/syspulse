<!--
  SysPulse - Multipurpose System Utility
  File   : docs/INSTALL.md
  Author : Praneeth
  License: MIT
-->
# SysPulse — Installation & Required Modules

**Author:** Praneeth · **Version:** 0.6.0 · **License:** MIT

This guide covers:

1. [System requirements](#1-system-requirements)
2. [Installing SysPulse (end users)](#2-installing-syspulse-end-users)
3. [Runtime dependencies explained](#3-runtime-dependencies-explained)
4. [Developer setup (build from source)](#4-developer-setup-build-from-source)
5. [Build-time dependencies (system, Rust, JavaScript)](#5-build-time-dependencies)
6. [Building packages](#6-building-packages)
7. [Post-install configuration](#7-post-install-configuration)
8. [Files SysPulse creates](#8-files-syspulse-creates)
9. [Uninstalling](#9-uninstalling)
10. [Troubleshooting](#10-troubleshooting)

---

## 1. System requirements

| Item | Requirement |
|---|---|
| OS | Ubuntu **22.04 LTS** or newer (also other Debian-based distributions with the same packages) |
| Architecture | x86_64 (amd64) |
| Desktop session | Any; the **screen recorder needs an X11 session** ("Ubuntu on Xorg"). Wayland recording is planned |
| Network stack | NetworkManager (default on Ubuntu Desktop) for *Network Settings* |
| Audio | PulseAudio or PipeWire-Pulse (default on Ubuntu) for the recorder |
| RAM / disk | ~150 MB RAM while running; ~15 MB installed |
| GPU (optional) | NVIDIA (driver with NVML), AMD (amdgpu) or Intel (i915/xe) for the GPU page |

---

## 2. Installing SysPulse (end users)

### Option A — `.deb` package (recommended)

```bash
sudo apt install ./SysPulse_0.6.0_amd64.deb
```

`apt` installs every runtime dependency listed in [section 3](#3-runtime-dependencies-explained) automatically. The package installs:

| Path | Purpose |
|---|---|
| `/usr/bin/syspulse` | The application |
| `/usr/bin/syspulse-helper` | Privileged helper (only ever started through `pkexec`) |
| `/usr/bin/syspulse-capture` | Packet-capture helper for NetLens (gets capture rights only after you grant them) |
| `/usr/share/polkit-1/actions/io.github.praneeth.syspulse.policy` | Password-prompt policy for the helper |
| `/usr/share/applications/SysPulse.desktop` and icons | Menu entry |

Start it from the application menu (**SysPulse**) or run `syspulse`.

### Option B — AppImage (portable)

```bash
chmod +x SysPulse_0.6.0_amd64.AppImage
./SysPulse_0.6.0_amd64.AppImage
```

**Limitation:** inside an AppImage the admin helper lives on a FUSE mount that `root` cannot read. Features that need your password (uninstalling software, cleaning system caches, managing system services, ending other users' processes, granting serial-port access, granting NetLens capture permission, creating launchers for other users) therefore only work with the `.deb`. NetLens can still open capture files in the AppImage. Install the runtime packages from [section 3](#3-runtime-dependencies-explained) yourself.

---

## 3. Runtime dependencies explained

Installed automatically by the `.deb`:

| Package | Used by | Why |
|---|---|---|
| `libwebkit2gtk-4.1-0`, `libgtk-3-0` | Whole app | Window and web view (Tauri) |
| `libayatana-appindicator3-1` | Recorder | Top-bar (tray) menu |
| `policykit-1` | Admin actions | `pkexec` password dialog |
| `pciutils`, `usbutils` | Hardware | PCI / USB ID databases (device names) |
| `apt` | Software, Cleaner | Package removal and APT cache cleaning |
| `x11-utils` | Recorder | `xprop`, `xwininfo` (window list) |
| `x11-xserver-utils` | Recorder | `xrandr` (monitor list) |
| `pulseaudio-utils` | Recorder | `pactl` (audio devices) |
| `gstreamer1.0-x` | Recorder | `ximagesrc` (screen capture) |
| `gstreamer1.0-pulseaudio` | Recorder | `pulsesrc` (audio capture) |
| `gstreamer1.0-plugins-good` | Recorder | MP4 / MKV / WebM muxers, VP8, level meter, JPEG |
| `gstreamer1.0-plugins-ugly` | Recorder | `x264enc` (H.264 on the CPU) |
| `gstreamer1.0-libav` | Recorder | `avenc_aac` (AAC audio) |
| `acl` | App Launchers | `setfacl` / `getfacl`: give another user access to an application folder |
| `desktop-file-utils` | App Launchers | Validates launcher files and refreshes the application menu |
| `libpcap0.8` | NetLens | Compiles capture filters (`tcp port 554`); loaded at run time |

Optional, used when present:

| Package / component | Enables |
|---|---|
| NVIDIA proprietary driver (`libnvidia-ml.so.1`) | Detailed NVIDIA GPU metrics (NVML) |
| `gstreamer1.0-plugins-bad` | H.265 (`x265enc`), NVENC (`nvh264enc`) when compatible with the driver |
| `gstreamer1.0-vaapi` | Intel/AMD hardware video encoding |
| `snapd`, `flatpak` | Snap / Flatpak listings in *Software* |
| `tracker3` | Resetting the GNOME search index in *Cache Cleaner* |
| `tracepath` (iputils-tracepath) | Traceroute in *Net Tools* (installed by default) |
| `systemd`, `journalctl` | *Services* page (always present on Ubuntu) |
| `nmcli` (NetworkManager) | *Network Settings* (default on Ubuntu Desktop) |
| `tshark` | NetLens **Wireshark decode** (Wireshark's 3000+ protocol decoders for one packet) |
| `wireshark` | NetLens **Wireshark ↗** button (open the capture in Wireshark) |

---

## 4. Developer setup (build from source)

### 4.1 One-command setup

```bash
git clone https://github.com/praneeth/syspulse.git
cd syspulse
./scripts/setup-dev-ubuntu.sh        # asks for your sudo password once
source ~/.cargo/env                  # or open a new terminal
npm install && npm --prefix ui install
npm run dev                          # starts the app with hot reload
```

`scripts/setup-dev-ubuntu.sh` installs:

1. **System libraries** with `apt` (see [5.1](#51-system-packages-apt)).
2. **Rust** (stable) with `rustup`, into `~/.cargo`.
3. **Node.js 22 LTS**, into `~/.local/opt/node`, with links in `~/.local/bin`.

The script is safe to run again.

### 4.2 Manual setup

```bash
sudo apt-get update
sudo apt-get install -y build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-pipewire \
  gstreamer1.0-x gstreamer1.0-pulseaudio gstreamer1.0-plugins-ugly gstreamer1.0-libav \
  pciutils usbutils lshw policykit-1 libudev-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# Node.js 20+ (nvm, NodeSource or the official tarball)
```

### 4.3 Anaconda / Conda users

If Conda is on your `PATH`, its own `gst-*` and `pkg-config` tools can shadow the system ones. SysPulse links against the **system** GStreamer. Diagnose with `/usr/bin/gst-inspect-1.0` (not the Conda copy), and make sure `which pkg-config` prints `/usr/bin/pkg-config` when building.

---

## 5. Build-time dependencies

### 5.1 System packages (apt)

| Package | Needed for |
|---|---|
| `build-essential`, `pkg-config` | C toolchain for native crates |
| `libwebkit2gtk-4.1-dev`, `libssl-dev`, `librsvg2-dev`, `libxdo-dev` | Tauri / WebKitGTK |
| `libayatana-appindicator3-dev` | Tray icon |
| `libgstreamer1.0-dev`, `libgstreamer-plugins-base1.0-dev` | `gstreamer` Rust bindings |
| `libudev-dev` | `serialport` crate (USB serial port details) |
| *(no `libpcap-dev`)* | NetLens loads the runtime `libpcap0.8` with `dlopen`, so no development package is needed |

### 5.2 Toolchain versions (tested)

| Tool | Version |
|---|---|
| Rust | 1.99 (minimum 1.77) |
| Node.js / npm | 22.11 / 10.9 |
| Tauri CLI | 2.12.1 |
| GStreamer (system) | 1.20.3 (Ubuntu 22.04) |

### 5.3 Rust crates

Workspace `Cargo.toml` → `crates/core` (library) and `src-tauri` (application + helper):

| Crate | Version | Used for |
|---|---|---|
| `tauri` (+ `tray-icon`, `image-png`) | 2.12 | Desktop shell, IPC, tray |
| `tauri-build` | 2.7 | Build script |
| `tauri-plugin-dialog` | 2.8 | Open / save / folder dialogs |
| `tauri-plugin-opener` | 2.7 | Open files, show in folder |
| `tauri-plugin-notification` | 2.5 | Desktop notifications (alerts, recorder) |
| `tauri-plugin-global-shortcut` | 2.4 | Recorder hotkeys |
| `sysinfo` | 0.39 | CPU, memory, disks, network, processes |
| `nvml-wrapper` | 0.13 | NVIDIA GPU metrics (loads NVML at runtime) |
| `nix` | 0.31 | Signals, user/group info |
| `libc` | 0.2 | `setpriority`, `getsockopt(TCP_INFO)`, `access`, `openpty` |
| `jwalk` | 0.9 | Parallel directory walking (large files, search, cleaner) |
| `trash` | 5.2 | Move to Trash (freedesktop spec) |
| `regex` | 1.13 | File-search patterns |
| `serialport` | 4.10 | Serial ports for PacketForge |
| `rayon` | 1 | Parallel display-filter evaluation in NetLens |
| `image` | 0.25 (png, ico, jpeg, bmp) | App Launchers icon conversion to 256×256 PNG |
| `gstreamer` | 0.25 (feature `v1_20`) | Screen recorder pipelines |
| `serde`, `serde_json` | 1 | Data model / IPC serialisation |
| `thiserror` | 2 | Error type |
| `log`, `env_logger` | 0.4 / 0.11 | Logging (`RUST_LOG=debug syspulse`) |

### 5.4 JavaScript packages (`ui/package.json`)

| Package | Version | Used for |
|---|---|---|
| `svelte` | 5.57 | UI framework (runes) |
| `vite`, `@sveltejs/vite-plugin-svelte` | 6.4 / 5.1 | Dev server and bundler |
| `typescript`, `svelte-check`, `@tsconfig/svelte` | 5.6 / 4.7 / 5.0 | Type checking |
| `@tauri-apps/api` | 2.12 | `invoke` / `listen` bridge |
| `@tauri-apps/plugin-dialog`, `@tauri-apps/plugin-opener` | 2.8 / 2.7 | Dialogs from the UI |
| `uplot` | 1.6.32 | Fast time-series charts |

Root `package.json` only contains `@tauri-apps/cli` (2.12).

---

## 6. Building packages

```bash
npm run build
```

This builds the UI, compiles Rust in release mode, and produces:

```
target/release/bundle/deb/SysPulse_<version>_amd64.deb
target/release/bundle/appimage/SysPulse_<version>_amd64.AppImage
```

Other useful commands:

| Command | What it does |
|---|---|
| `npm run dev` | Run the desktop app with hot reload |
| `npm run ui:dev` | UI only in a browser at http://localhost:1420 (demo data) |
| `npm run ui:check` | Svelte + TypeScript type check |
| `cargo test --workspace` | Rust unit and integration tests (69) |
| `cargo clippy --workspace --all-targets` | Lints (kept at zero warnings) |
| `cargo fmt --all` | Formatting |
| `cargo run -p syspulse-core --example dump [hw]` | Print telemetry / hardware inventory as JSON |
| `cargo run -p syspulse-core --example phase2 -- software\|cleaner\|large` | Software, cleaner and large-file engines from the command line |
| `cargo run --release -p syspulse-core --example netperf -- server` | Command-line speed-test server |
| `cargo run --release -p syspulse-core --example netperf -- <ip> tcp bidir` | Command-line speed-test client |
| `cargo run -p syspulse-core --example rec_check -- sources` | List monitors, windows, audio devices and working encoders |

---

## 7. Post-install configuration

| Feature | One-time step |
|---|---|
| **Serial ports** (PacketForge) | Click **Grant serial access** in PacketForge, or run `sudo usermod -aG dialout $USER`, then **log out and back in** |
| **Packet capture** (NetLens) | Click **Grant capture permission** in NetLens (one password prompt). This gives only `syspulse-capture` the `cap_net_raw,cap_net_admin` capabilities. To remove: Capture options → **Revoke**, or `sudo setcap -r /usr/bin/syspulse-capture` |
| **Speed-test server** | Allow the port if a firewall is on: `sudo ufw allow 5201` (TCP and UDP) |
| **Screen recorder** | Use an X11 session ("Ubuntu on Xorg" on the login screen) |
| **Tray menu** | Ubuntu shows it by default. On other GNOME setups, enable the *AppIndicator and KStatusNotifierItem Support* extension |
| **NVIDIA metrics** | Use the proprietary driver (`nvidia-smi` must work) |

---

## 8. Files SysPulse creates

| Location | Contents |
|---|---|
| `~/.config/io.github.praneeth.syspulse/alerts.json` | Alert thresholds |
| `~/.config/io.github.praneeth.syspulse/recorder.json` | Recorder settings used by the hotkeys and tray |
| `~/.config/io.github.praneeth.syspulse/packetforge.json` | PacketForge macros, profiles and send history |
| `~/.local/share/io.github.praneeth.syspulse/` | Web view storage (UI preferences) |
| `~/Videos/SysPulse/` | Screen recordings (default, changeable) |
| `~/.config/io.github.praneeth.syspulse/launchers.json` | App Launchers registry (what was created, for whom, folder access granted) |
| Target users: `~/.local/share/applications/syspulse-*.desktop`, `~/.local/share/icons/…/syspulse-*`, desktop icon | Launchers you create for them (removed with **Remove…**) |
| `~/.cache/syspulse/netlens/` | Temporary copies for *Open in Wireshark* / *Wireshark decode* (replaced each time) |
| `~/.config/autostart/*.desktop` | Only when you add or disable startup apps |

Nothing is sent over the internet. The only network traffic is what you start yourself: speed tests, ping, DNS lookups and PacketForge sessions. NetLens only listens; captured packets stay in memory until you save them.

---

## 9. Uninstalling

```bash
sudo apt remove sys-pulse           # package name is "sys-pulse"; keeps settings
rm -rf ~/.config/io.github.praneeth.syspulse ~/.local/share/io.github.praneeth.syspulse   # remove settings
```

Recordings in `~/Videos/SysPulse` are not removed.

---

## 10. Troubleshooting

| Symptom | Cause / fix |
|---|---|
| Layout looks broken in `npm run dev`, terminal shows `[postcss] … Unknown word SysPulse` | Fixed in `ui/vite.config.ts` (development builds inject component styles instead of serving them separately). If you still see it with an older copy, restart `npm run dev`. Packaged builds were never affected |
| "Port 1420 is already in use" | Another `npm run dev` is still running; stop it first |
| GPU page shows no NVIDIA data | NVML missing (no proprietary driver) or the dGPU is asleep. SysPulse never wakes a suspended GPU |
| Recorder: "No working GPU encoder" | GStreamer 1.20's NVENC plugin does not support NVIDIA drivers ≥ 5xx. The CPU encoder (x264) is used automatically. Install `gstreamer1.0-vaapi` for Intel/AMD hardware encoding |
| Recording has no sound, or only hiss | Choose **System sound → Follow current output**. A "not plugged in" microphone (e.g. headset jack) records only hiss; untick Microphone or pick a connected one |
| Recorder refuses to start on Wayland | Log in with "Ubuntu on Xorg"; Wayland support is planned |
| PacketForge: "permission denied" on `/dev/ttyUSB0` | Not in `dialout`; use **Grant serial access**, then log out and in |
| NetLens: **Start capture** is greyed out | Capture permission not granted yet: click **Grant capture permission**. In a development build, rebuilding `syspulse-capture` removes the permission; grant it again |
| NetLens: capture filter field disabled | `libpcap0.8` missing: `sudo apt install libpcap0.8` (display filters still work) |
| NetLens: "Dropped" count in the status bar | The PC could not keep up; use a capture filter or a smaller snap length (Capture options) |
| NetLens: no RTP streams for a camera | Start capturing **before** the player connects, so NetLens sees the RTSP SETUP that announces the media ports |
| App Launchers: desktop icon shows "Allow Launching" | The trust flag could not be set (for example the GNOME desktop-icons extension is disabled). Right-click → Allow Launching once, or recreate the launcher while the user is logged in |
| App Launchers: app does not start for the other user | Open **Edit** and check the Checks panel: folder access, missing libraries, or the program needs write access (enable "let them write in it") |
| Speed test cannot connect | Server mode not started on the other PC, wrong IP, or firewall — `sudo ufw allow 5201` |
| Admin actions fail in the AppImage | Use the `.deb` (see [Option B](#option-b--appimage-portable)) |
| Hotkeys do nothing | Another app owns Ctrl+Alt+R / Ctrl+Alt+P; the tray menu still works. Logs: run `syspulse` from a terminal |
| Network changes ask for a password | NetworkManager's policy for your account; enter it in the system dialog |
