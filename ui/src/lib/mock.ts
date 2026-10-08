// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/mock.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: Synthetic backend used only when the UI runs outside Tauri
//           (browser development). Never bundled into real data paths.
// =============================================================================
import type { HwSection, ProcessInfo, Telemetry } from "./types";

const GiB = 1024 ** 3;
let t = 0;
const wave = (base: number, amp: number, speed: number, phase = 0) =>
  Math.max(0, Math.min(100, base + amp * Math.sin(t * speed + phase) + (Math.random() - 0.5) * amp * 0.6));

function telemetry(): Telemetry {
  t += 1;
  const cores = Array.from({ length: 16 }, (_, i) => wave(25 + (i % 4) * 8, 18, 0.15 + i * 0.01, i));
  const total = cores.reduce((a, b) => a + b, 0) / cores.length;
  return {
    timestamp: Date.now(),
    cpu: {
      total,
      perCore: cores,
      frequencyMhz: cores.map((c) => 1200 + c * 30),
      loadAverage: [2.1, 1.8, 1.5],
      temperature: 48 + total * 0.3,
      processCount: 412,
      uptimeSecs: 3600 * 5 + t,
    },
    memory: {
      total: 16 * GiB,
      used: (5.4 + Math.sin(t * 0.05) * 0.4) * GiB,
      available: 9 * GiB,
      cached: 5.8 * GiB,
      swapTotal: 2 * GiB,
      swapUsed: 0.1 * GiB,
    },
    filesystems: [
      { device: "/dev/nvme0n1p2", mountPoint: "/", fsType: "ext4", total: 512 * GiB, available: 188 * GiB, removable: false },
      { device: "/dev/nvme0n1p1", mountPoint: "/boot/efi", fsType: "vfat", total: 0.5 * GiB, available: 0.49 * GiB, removable: false },
    ],
    diskIo: [{ device: "nvme0n1", readBps: wave(10, 10, 0.3) * 1e5, writeBps: wave(8, 8, 0.2) * 1e5, busyPercent: wave(5, 5, 0.3) }],
    network: [{ interface: "wlp0s20f3", rxBps: wave(30, 30, 0.25) * 2e4, txBps: wave(10, 10, 0.2) * 1e4, rxTotal: 3e9, txTotal: 6e8 }],
    sensors: [
      { chip: "coretemp", label: "Package id 0", temperature: 48 + total * 0.3, critical: 100 },
      { chip: "nvme", label: "Composite", temperature: 41, critical: 84 },
      { chip: "acpitz", label: "temp1", temperature: 44, critical: 105 },
    ],
    battery: { percent: 87, status: "Discharging", powerWatts: 18.4 },
    gpus: [
      {
        id: "0000:01:00.0", vendor: "nvidia", name: "NVIDIA GeForce RTX 3060 Laptop GPU", driver: "nvidia",
        driverVersion: "595.91.07", utilization: wave(20, 20, 0.2), memoryUtilization: wave(10, 8, 0.2),
        vramUsed: 1.2 * GiB, vramTotal: 6 * GiB, temperature: 52, powerWatts: 24 + Math.random() * 6, powerLimitWatts: 80,
        coreClockMhz: 1350, maxCoreClockMhz: 2100, memoryClockMhz: 6001, fanPercent: 34, encoderUtilization: 0,
        decoderUtilization: 3, pcieGeneration: 4, pcieWidth: 16, powerState: "active",
        processes: [{ pid: 2211, name: "Xorg", usedMemory: 280 * 1024 ** 2, kind: "graphics" }],
      },
      {
        id: "0000:00:02.0", vendor: "intel", name: "Intel TigerLake-H GT1 [UHD Graphics]", driver: "i915",
        driverVersion: null, utilization: null, memoryUtilization: null, vramUsed: null, vramTotal: null,
        temperature: null, powerWatts: null, powerLimitWatts: null, coreClockMhz: 350, maxCoreClockMhz: 1450,
        memoryClockMhz: null, fanPercent: null, encoderUtilization: null, decoderUtilization: null,
        pcieGeneration: null, pcieWidth: null, powerState: "active", processes: [],
      },
    ],
  };
}

const NAMES = ["firefox", "code", "gnome-shell", "Xorg", "syspulse", "pipewire", "nautilus", "slack", "python3", "dockerd", "systemd", "bash"];
function processes(): ProcessInfo[] {
  return Array.from({ length: 140 }, (_, i) => ({
    pid: 1000 + i * 7,
    ppid: 1,
    name: NAMES[i % NAMES.length] + (i >= NAMES.length ? `-${i}` : ""),
    user: i % 5 === 0 ? "root" : "praneeth",
    state: i % 9 === 0 ? "Runnable" : "Sleeping",
    cpu: Math.random() * (i < 6 ? 40 : 3),
    memory: Math.random() * 800 * 1024 ** 2,
    virtualMemory: Math.random() * 4 * GiB,
    startTime: Date.now() / 1000 - i * 300,
    diskReadBps: Math.random() * 1e4,
    diskWriteBps: Math.random() * 1e4,
    threads: 1 + (i % 30),
    nice: 0,
    command: `/usr/bin/${NAMES[i % NAMES.length]} --flag`,
    exe: `/usr/bin/${NAMES[i % NAMES.length]}`,
  }));
}

const HW: HwSection[] = [
  { id: "system", title: "System", devices: [{ title: "Mock Laptop", subtitle: "Vendor Inc.", properties: [{ key: "BIOS", value: "1.2.3" }] }] },
  { id: "cpu", title: "Processor", devices: [{ title: "Intel Core i7-11800H", subtitle: "8 cores / 16 threads", properties: [{ key: "Max frequency", value: "4600 MHz" }] }] },
];

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  switch (cmd) {
    case "system_summary":
      return {
        hostname: "dev-browser", user: "praneeth", osName: "Ubuntu", osVersion: "22.04.5 LTS", kernel: "6.8.0", arch: "x86_64",
        desktop: "GNOME", sessionType: "x11", cpuModel: "Intel Core i7-11800H @ 2.30GHz", physicalCores: 8,
        logicalCores: 16, totalMemory: 16 * GiB, bootTime: Date.now() / 1000 - 18000,
      } as T;
    case "hardware_inventory":
      return HW as T;
    case "list_processes":
      return processes() as T;
    case "process_details": {
      const info = processes().find((p) => p.pid === args?.pid) ?? processes()[0];
      return { info, cmdline: info.command.split(" "), cwd: "/home/praneeth", root: "/", uid: 1000, gid: 1000, openFiles: 42, cgroup: "/user.slice", children: [], environmentCount: 51, status: [["State", "S (sleeping)"]] } as T;
    }
    case "list_packages":
      return [
        { id: "apt:gimp:amd64", source: "apt", name: "gimp", displayName: "GNU Image Manipulation Program", version: "2.10.30", summary: "Create images and edit photographs", installedSize: 21e6, installDate: 1.7e9, section: "graphics", arch: "amd64", publisher: "Ubuntu Developers", homepage: null, isApp: true, icon: null, protected: false, autoInstalled: false, installation: null },
        { id: "snap:firefox", source: "snap", name: "firefox", displayName: "Firefox", version: "143.0 (rev 6966)", summary: "Mozilla Firefox web browser", installedSize: 259e6, installDate: 1.76e9, section: "snap/app", arch: null, publisher: "Mozilla", homepage: null, isApp: true, icon: null, protected: false, autoInstalled: false, installation: null },
        { id: "apt:dpkg:amd64", source: "apt", name: "dpkg", displayName: "dpkg", version: "1.21.1", summary: "Debian package management system", installedSize: 6.7e6, installDate: 1.6e9, section: "admin", arch: "amd64", publisher: "Ubuntu Developers", homepage: null, isApp: false, icon: null, protected: true, autoInstalled: false, installation: null },
      ] as T;
    case "package_icon":
      return null as T;
    case "plan_package_removal":
      return { source: args?.source, requested: args?.names, removes: args?.names, freedBytes: 21e6, needsAdmin: true, blockers: [], warnings: [] } as T;
    case "cleaner_scan":
      return [
        { id: "apt-cache", title: "APT package cache", description: "Downloaded .deb installers", category: "system", paths: ["/var/cache/apt/archives"], size: 385e6, files: 120, needsAdmin: true, caution: null, selectedByDefault: true, details: [] },
        { id: "journal", title: "System journal logs", description: "Keeps the newest 100 MB", category: "system", paths: ["/var/log/journal"], size: 1.9e9, files: 0, needsAdmin: true, caution: null, selectedByDefault: true, details: [] },
        { id: "thumbnails", title: "Thumbnails", description: "Image previews", category: "user", paths: ["~/.cache/thumbnails"], size: 14e6, files: 900, needsAdmin: false, caution: null, selectedByDefault: true, details: [] },
        { id: "firefox", title: "Firefox cache", description: "Cached web pages", category: "browser", paths: ["~/.cache/mozilla/firefox"], size: 331e6, files: 4000, needsAdmin: false, caution: "Close the browser first", selectedByDefault: true, details: [] },
        { id: "pip", title: "pip cache", description: "Downloaded wheels", category: "developer", paths: ["~/.cache/pip"], size: 43e6, files: 200, needsAdmin: false, caution: null, selectedByDefault: true, details: [] },
      ] as T;
    case "cleaner_clean":
      return { freedBytes: 400e6, cleaned: args?.ids, errors: [] } as T;
    case "large_scan":
      await new Promise((r) => setTimeout(r, 1200));
      return {
        root: (args?.options as { root: string }).root, totalFiles: 152000, totalBytes: 210e9, skippedDirs: 0, elapsedMs: 1200, cancelled: false,
        files: [
          { path: "/home/praneeth/Videos/trip.mkv", name: "trip.mkv", size: 4.2e9, modified: 1.7e9, category: "video", owned: true },
          { path: "/home/praneeth/Downloads/ubuntu-24.04.iso", name: "ubuntu-24.04.iso", size: 6.1e9, modified: 1.71e9, category: "diskimage", owned: true },
          { path: "/home/praneeth/backup.zip", name: "backup.zip", size: 2.2e9, modified: 1.69e9, category: "archive", owned: true },
        ],
        folders: [{ path: "/home/praneeth/Videos", size: 80e9, files: 300 }, { path: "/home/praneeth/Downloads", size: 40e9, files: 900 }, { path: "/home/praneeth/.cache", size: 20e9, files: 90000 }],
        subfolders: [{ path: "/home/praneeth/Videos/2024", size: 50e9, files: 120 }],
        categories: [{ category: "video", bytes: 90e9, files: 400 }, { category: "archive", bytes: 30e9, files: 50 }, { category: "other", bytes: 60e9, files: 140000 }],
        tree: {
          name: "/home/praneeth", path: "/home/praneeth", size: 210e9, kind: "dir", children: [
            { name: "Videos", path: "/home/praneeth/Videos", size: 80e9, kind: "dir", children: [
              { name: "2024", path: "/home/praneeth/Videos/2024", size: 50e9, kind: "dir", children: [] },
              { name: "2023", path: "/home/praneeth/Videos/2023", size: 25e9, kind: "dir", children: [] },
              { name: "Files here", path: "/home/praneeth/Videos", size: 5e9, kind: "files", children: [] }] },
            { name: "Downloads", path: "/home/praneeth/Downloads", size: 40e9, kind: "dir", children: [] },
            { name: ".cache", path: "/home/praneeth/.cache", size: 30e9, kind: "dir", children: [
              { name: "tracker3", path: "/home/praneeth/.cache/tracker3", size: 25e9, kind: "dir", children: [] },
              { name: "pip", path: "/home/praneeth/.cache/pip", size: 5e9, kind: "dir", children: [] }] },
            { name: "Projects", path: "/home/praneeth/Projects", size: 25e9, kind: "dir", children: [] },
            { name: "Pictures", path: "/home/praneeth/Pictures", size: 15e9, kind: "dir", children: [] },
            { name: "12 smaller folders", path: "/home/praneeth", size: 12e9, kind: "other", children: [] },
            { name: "Files here", path: "/home/praneeth", size: 8e9, kind: "files", children: [] },
          ],
        },
      } as T;
    case "process_resources":
      return { files: [{ fd: 3, target: "/home/praneeth/.config/app.db", kind: "file" }], connections: [{ protocol: "tcp", local: "127.0.0.1:5432", remote: "0.0.0.0:0", state: "LISTEN" }], totalFds: 24, permissionDenied: false } as T;
    case "search_files":
      await new Promise((r) => setTimeout(r, 600));
      return {
        root: "/home/praneeth", scanned: 152000, truncated: false, cancelled: false, elapsedMs: 600,
        hits: [
          { path: "/home/praneeth/Documents/report-2024.pdf", name: "report-2024.pdf", isDir: false, size: 2.4e6, modified: 1.72e9, category: "document", owner: "praneeth", content: null },
          { path: "/home/praneeth/Projects/notes.md", name: "notes.md", isDir: false, size: 3200, modified: 1.75e9, category: "document", owner: "praneeth", content: { line: 12, text: "TODO: send the report to the team", matches: 3 } },
          { path: "/home/praneeth/Documents/reports", name: "reports", isDir: true, size: null, modified: 1.7e9, category: null, owner: "praneeth", content: null },
        ],
      } as T;
    case "list_services":
      return [
        { unit: "cron.service", description: "Regular background program processing daemon", load: "loaded", active: "active", sub: "running", fileState: "enabled", preset: "enabled", user: false, protected: false },
        { unit: "cups.service", description: "CUPS Scheduler", load: "loaded", active: "active", sub: "running", fileState: "enabled", preset: "enabled", user: false, protected: false },
        { unit: "dbus.service", description: "D-Bus System Message Bus", load: "loaded", active: "active", sub: "running", fileState: "static", preset: null, user: false, protected: true },
        { unit: "nut-monitor.service", description: "Network UPS Tools - power device monitor", load: "loaded", active: "failed", sub: "failed", fileState: "enabled", preset: "enabled", user: false, protected: false },
        { unit: "bluetooth.service", description: "Bluetooth service", load: "loaded", active: "inactive", sub: "dead", fileState: "disabled", preset: "enabled", user: false, protected: false },
      ] as T;
    case "service_details":
      return { unit: args?.unit, mainPid: 1487, memory: 737280, cpuSeconds: 1.2, tasks: 1, restarts: 0, since: "Tue 2026-10-07 11:02:11 IST", fragmentPath: "/lib/systemd/system/cron.service", properties: [["Type", "simple"], ["Restart", "on-failure"]], logs: ["2026-10-07T17:17:01+0530 ad-camera CRON[5521]: (root) CMD (cd / && run-parts --report /etc/cron.hourly)"] } as T;
    case "list_startup":
      return [
        { id: "anydesk_global_tray.desktop", name: "AnyDesk Tray", comment: "", exec: "/usr/bin/anydesk --tray", icon: "anydesk", enabled: true, system: true, userFile: false, noDisplay: false, appliesHere: true, delay: null },
        { id: "my-backup.desktop", name: "My Backup", comment: "Nightly sync", exec: "/home/praneeth/bin/backup.sh", icon: null, enabled: false, system: false, userFile: true, noDisplay: false, appliesHere: true, delay: 30 },
        { id: "gnome-keyring-ssh.desktop", name: "SSH Key Agent", comment: "GNOME Keyring: SSH Agent", exec: "/usr/bin/gnome-keyring-daemon --start --components=ssh", icon: null, enabled: true, system: true, userFile: false, noDisplay: false, appliesHere: true, delay: null },
      ] as T;
    case "list_connections":
      return [
        { protocol: "tcp", localAddress: "127.0.0.53", localPort: 53, remoteAddress: "0.0.0.0", remotePort: 0, state: "LISTEN", pid: null, process: null, user: "systemd-resolve", scope: "listening" },
        { protocol: "tcp", localAddress: "192.168.1.20", localPort: 51544, remoteAddress: "142.250.183.14", remotePort: 443, state: "ESTABLISHED", pid: 2211, process: "firefox", user: "praneeth", scope: "remote" },
        { protocol: "udp", localAddress: "0.0.0.0", localPort: 5353, remoteAddress: "0.0.0.0", remotePort: 0, state: "UNCONN", pid: 900, process: "avahi-daemon", user: "avahi", scope: "listening" },
      ] as T;
    case "get_alert_config":
      return { enabled: true, cpuTemp: 90, gpuTemp: 85, memoryPercent: 95, diskPercent: 95, batteryPercent: 15, sustainSecs: 30, cooldownSecs: 900 } as T;
    case "net_devices":
      return [
        { device: "enp3s0", kind: "ethernet", state: "connected", connection: "Wired connection 1", connectionUuid: "265d463f-07cd-417f-b101-0499a31790b0", mac: "D8:BB:C1:AC:EC:12", mtu: 1500,
          ipv4: ["10.1.0.181/24", "192.168.0.181/24"], ipv4Gateway: "10.1.0.1", ipv4Dns: ["10.1.0.1"], ipv6: ["fe80::1ce:2283:d910:84c1/64"], ipv6Gateway: null, ipv6Dns: [],
          routes: ["dst = 0.0.0.0/0, nh = 10.1.0.1, mt = 100"], carrier: true, speedMbps: 1000, driver: "r8169", wifi: null },
        { device: "wlp5s0", kind: "wifi", state: "connected", connection: "BRIT_Cnt3", connectionUuid: "68f45d5d-fefc-402e-8c3f-89b0db5cf3c1", mac: "A0:E7:0B:CD:BA:86", mtu: 1500,
          ipv4: ["192.168.29.14/24"], ipv4Gateway: "192.168.29.1", ipv4Dns: ["192.168.29.1"], ipv6: [], ipv6Gateway: null, ipv6Dns: [], routes: [], carrier: null, speedMbps: null, driver: "iwlwifi",
          wifi: { ssid: "BRIT_Cnt3", signal: 85, bitrate: "270 Mbit/s", frequencyMhz: 2472, security: "WPA2" } },
      ] as T;
    case "net_connections":
      return [] as T;
    case "net_connection_settings":
      return { uuid: args?.uuid, name: "Wired connection 1", kind: "802-3-ethernet", interfaceName: "enp3s0", autoconnect: true, mtu: null, ssid: null,
        ipv4: { method: "manual", addresses: ["10.1.0.181/24", "192.168.0.181/24"], gateway: "10.1.0.1", dns: [], ignoreAutoDns: false },
        ipv6: { method: "auto", addresses: [], gateway: null, dns: [], ignoreAutoDns: false } } as T;
    case "wifi_scan":
      return [
        { inUse: true, ssid: "BRIT_Cnt3", bssid: "34:0A:33:6C:67:DE", signal: 85, channel: 13, frequencyMhz: 2472, band: "2.4 GHz", rate: "270 Mbit/s", security: "WPA2", knownUuid: "68f45d5d-fefc-402e-8c3f-89b0db5cf3c1" },
        { inUse: false, ssid: "AM3-5G", bssid: "11:22:33:44:55:66", signal: 62, channel: 44, frequencyMhz: 5220, band: "5 GHz", rate: "540 Mbit/s", security: "WPA2 WPA3", knownUuid: "3b347daf-538f-4f22-abbe-4b6a3fd621ca" },
        { inUse: false, ssid: "Office-Guest", bssid: "aa:bb:cc:dd:ee:ff", signal: 40, channel: 6, frequencyMhz: 2437, band: "2.4 GHz", rate: "130 Mbit/s", security: "Open", knownUuid: null },
      ] as T;
    case "wifi_radio":
      return true as T;
    case "perf_server_status":
      return null as T;
    case "perf_discover":
      return [{ address: "192.168.0.50", host: "lab-pc", port: 5201, codeRequired: false }] as T;
    case "net_lookup":
      return { host: args?.host, addresses: ["142.250.183.14", "2404:6800:4007:81a::200e"], elapsedMs: 12.4, servers: ["10.1.0.1"] } as T;
    case "net_ports":
      return (args?.ports as number[]).map((p, i) => ({ port: p, state: i % 3 === 0 ? "open" : i % 3 === 1 ? "closed" : "filtered", elapsedMs: 3 + i })) as T;
    case "signal_process":
      return { pid: args?.pid, signal: args?.signal, elevated: false } as T;
    // ---- NetLens (browser preview only)
    case "netlens_setup":
      return {
        access: { helperFound: true, helperPath: "/usr/bin/syspulse-capture", canCapture: true, filtersAvailable: true, message: "ready" },
        tools: { tshark: null, wireshark: "/usr/bin/wireshark" },
        interfaces: [
          { name: "enp3s0", kind: "ethernet", up: true, mac: "", addresses: ["192.168.1.20"], rxBytes: 0, txBytes: 0, linktype: 1 },
          { name: "wlp5s0", kind: "wifi", up: true, mac: "", addresses: ["192.168.1.31"], rxBytes: 0, txBytes: 0, linktype: 1 },
          { name: "lo", kind: "loopback", up: true, mac: "", addresses: ["127.0.0.1"], rxBytes: 0, txBytes: 0, linktype: 1 },
        ],
      } as T;
    case "netlens_status":
      return {
        state: "stopped", source: "demo.pcapng", interfaces: ["enp3s0"], captureFilter: "", packets: 5000, shown: 5000, bytes: 4_200_000,
        memory: 0, dropped: 0, duration: 42.5, packetsPerSec: 0, bitsPerSec: 0, messages: [], displayFilter: "", unsaved: false, progress: 1, stopReason: "",
      } as T;
    case "netlens_rows": {
      const a = args as { start: number; count: number };
      const protos = [["TCP", "tcp"], ["TLSv1.3", "tls"], ["HTTP", "http"], ["DNS", "dns"], ["RTSP", "media"], ["RTP", "media"], ["ARP", "arp"]];
      return Array.from({ length: Math.max(0, Math.min(a.count, 5000 - a.start)) }, (_, k) => {
        const i = a.start + k;
        const [proto, color] = protos[i % protos.length];
        return { no: i + 1, time: i * 0.0085, delta: 0.0085, clock: `2026-10-08 14:03:${String(12 + Math.floor(i * 0.0085) % 47).padStart(2, "0")}.123456789`, src: "192.168.1.20", dst: i % 3 ? "140.82.112.3" : "192.168.1.64", proto, len: 60 + ((i * 37) % 1400),
          info: `${proto} demo packet ${i + 1}`, color, problem: i % 97 === 0 ? 2 : i % 41 === 0 ? 1 : 0, app: i % 2 ? "firefox" : "vlc", dir: (i % 2) + 1 };
      }) as T;
    }
    case "netlens_detail":
      return {
        no: (args as { no: number }).no, summary: "firefox on this PC sends 517 bytes to github.com (140.82.112.3) over TCP (HTTPS (secure web)).",
        proto: "TLSv1.3", stream: 3, tcp: true, time: "2026-10-08 14:03:12.123456789", bytes: btoa("\x00\x11\x22\x33\x44\x55 demo packet bytes ".repeat(8)),
        tree: [
          { label: "Frame 1: 74 bytes on wire", field: "frame", filter: "frame", start: 0, len: 74, level: 0, children: [{ label: "Interface: enp3s0", field: "frame.interface_name", filter: 'frame.interface_name == "enp3s0"', start: 0, len: 0, level: 0 }] },
          { label: "Ethernet II", field: "eth", filter: "eth", start: 0, len: 14, level: 0 },
          { label: "Transport Layer Security", field: "tls", filter: "tls", start: 54, len: 20, level: 0, children: [{ label: "Server name: github.com", field: "tls.handshake.extensions_server_name", filter: 'tls.handshake.extensions_server_name == "github.com"', start: 60, len: 10, level: 0 }] },
        ],
      } as T;
    case "netlens_fields":
      return [{ name: "tcp.port", kind: "uint", description: "Source or destination TCP port" }, { name: "tls.handshake.extensions_server_name", kind: "str", description: "Server name (SNI)" }] as T;
    case "netlens_insights":
      return [{ id: "retrans", severity: "warning", title: "12 TCP retransmissions", detail: "Data had to be sent again.", count: 12, filter: "tcp.analysis.retransmission" }] as T;
    case "netlens_conversations":
    case "netlens_apps":
    case "netlens_rtp":
      return [] as T;
    case "netlens_check_filter":
    case "netlens_check_capture_filter":
      return null as T;
    default:
      return undefined as T;
  }
}

export function subscribe(handler: (t: Telemetry) => void): () => void {
  handler(telemetry());
  const id = setInterval(() => handler(telemetry()), 1000);
  return () => clearInterval(id);
}
