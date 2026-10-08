<!--
  SysPulse - Multipurpose System Utility
  File   : docs/USER_MANUAL.md
  Author : Praneeth
  License: MIT
-->
# SysPulse — User Manual

**Author:** Praneeth · **Version:** 0.6.0

SysPulse shows what your computer is doing and helps you keep it healthy, from one window. This manual walks through every page. For installation see [INSTALL.md](INSTALL.md).

---

## Contents

- [1. Getting started](#1-getting-started)
- [2. Monitor](#2-monitor) — Dashboard · Performance · GPU · Processes
- [3. Network](#3-network) — Network Settings · Speed Test · Net Tools · PacketForge · NetLens · Connections
- [4. System](#4-system) — Hardware · Services · Startup Apps
- [5. Maintenance](#5-maintenance) — Software · Cache Cleaner · Large Files · File Search
- [6. Media](#6-media) — Screen Recorder
- [7. Settings and alerts](#7-settings-and-alerts)
- [8. Keyboard shortcuts](#8-keyboard-shortcuts)
- [9. Common tasks (quick recipes)](#9-common-tasks-quick-recipes)
- [10. FAQ](#10-faq)

---

## 1. Getting started

**Open SysPulse** from the application menu, or run `syspulse` in a terminal.

The window has two parts:

* **Sidebar (left):** pages grouped into *Monitor*, *Network*, *System*, *Maintenance* and *Media*, with *Settings* at the bottom. The bottom also shows your computer name, Ubuntu version and uptime.
* **Page (right):** the selected tool.

**Conventions used throughout the app:**

| You see | Meaning |
|---|---|
| A password dialog | The action changes the system (for example uninstalling software). SysPulse never runs as administrator by itself; Ubuntu asks for your password and remembers it for a few minutes |
| A confirmation dialog | The action deletes something or stops something. It always says exactly what will happen |
| ⚠ coloured labels | Warnings (hot, almost full, not plugged in). Colours are always paired with text |
| "—" | The value is not available on this machine |
| Toast messages (bottom right) | Results of actions, and alerts |

---

## 2. Monitor

### 2.1 Dashboard

Your computer at a glance, updated every second.

* **Four gauges:**
  * **CPU** — usage, clock speed, temperature, load average, number of processes.
  * **Memory** — used and available RAM, cache, swap.
  * **GPU** — load, video memory, temperature, power. Shows *Sleeping* when a laptop's graphics card is powered down.
  * **Disk /** — space used, read and write speed, network speed.
* **Charts:** CPU, memory and swap, network download and upload, disk read and write. Hover over a chart to see exact values at that moment.
* **Top processes:** the six busiest programs. *View all →* opens Processes.
* **Temperatures:** the hottest sensors. A bar turns orange (**Hot**) above 75% of the sensor's critical value and red (**Critical**) above 90%.
* **Storage & power:** fill level of each disk and battery charge with power draw.

### 2.2 Performance

Detailed views, chosen with the tabs at the top right:

| Tab | Shows |
|---|---|
| **CPU** | Total usage chart, every logical processor with a mini chart and its clock speed, CPU temperature history |
| **Memory** | RAM and swap chart; composition bar (used / reclaimable cache / free) |
| **Disks** | Read/write speed chart, active time per drive, fill level of each filesystem |
| **Network** | Download/upload chart, per-interface speed and totals |
| **Sensors** | Every temperature sensor, grouped by chip, with its critical limit |

**Export CSV** saves the chart history as a spreadsheet. History length is set in *Settings*.

### 2.3 GPU

One section per graphics adapter (NVIDIA, AMD, Intel):

* Load and video-memory gauges.
* Details: temperature, power and power limit, core and memory clocks, fan speed, PCIe link, video encode and decode load.
* Charts of load, VRAM and power.
* **Processes using this GPU**, with the video memory each one uses (NVIDIA).

On laptops with two GPUs, the NVIDIA card is often **Sleeping**. That's normal and saves battery; SysPulse deliberately doesn't wake it.

### 2.4 Processes

Every running program.

* **Search** by name, PID, command or user.
* **Filters:** *All*, *My processes*, *Active* (using CPU or disk right now).
* **Tree** shows parent/child relationships; click ▸/▾ to fold.
* Click a **column header** to sort; click again to reverse.
* **Pause** freezes the list so you can read it.

Click a process to open its **details panel**:

| Item | Description |
|---|---|
| **End** (or press `Delete`) | Asks the program to close normally |
| **Kill** | Stops it immediately (after confirmation). Unsaved work is lost |
| **Stop / Continue** | Pauses and resumes the program |
| **Priority** | Very high … Very low. Raising priority needs your password |
| **Connections** | Network connections and listening ports of the program |
| **Open files** | Files and devices the program has open |
| Other details | Memory, threads, start time, executable, working folder, command line, kernel status |

Programs owned by another user (for example `root`) ask for your password to end or kill.

---

## 3. Network

### 3.1 Network Settings

**Adapters tab:** one card per wired or Wi-Fi adapter, showing:
* Connection name and status.
* IPv4 and IPv6 addresses, gateway, DNS servers.
* MAC address, link speed, MTU, driver.
* For Wi-Fi: network name, signal, speed and frequency.

**Change an IP address:**

1. Click **Edit IP settings** on the adapter.
2. Under **IPv4** choose:
   * **Automatic (DHCP)** — the router assigns the address.
   * **Manual (static)** — type the address and pick the prefix; the netmask is shown next to it, e.g. `24 (255.255.255.0)`. Use **+ Add address** for more than one IP on the same adapter. Enter the **Gateway**.
3. **DNS servers:** comma separated, e.g. `1.1.1.1, 8.8.8.8`. With DHCP, tick *Use only these DNS servers* to ignore the router's DNS.
4. Optional: IPv6 settings, MTU, *Connect automatically*.
5. Click **Save** (applies the next time the connection starts) or **Save & apply** (reconnects now).

Mistakes such as an invalid address or a missing gateway are shown in red before anything is saved. *Save & apply* briefly disconnects the adapter, which can drop remote sessions over it.

**Wi-Fi networks tab:**
* Turn the Wi-Fi radio on or off, and **Rescan**.
* Each network shows signal bars, channel, band (2.4 / 5 / 6 GHz), speed and security. **Open** networks are highlighted.
* The **channel chart** shows how crowded each channel is; your network is green. Use it to choose a quieter router channel.
* **Connect** works for networks you've used before. New networks are added in Ubuntu Settings.

**Saved connections tab:** every connection profile, with Edit and Connect/Disconnect.

### 3.2 Speed Test (between two PCs)

Measures the real network speed between two computers that both run SysPulse, like `iperf`.

**On the PC that receives the tests (PC A):**
1. Open **Speed Test → Server mode**.
2. Optionally tick **Require an access code**.
3. Click **Start server**. SysPulse shows this PC's IP addresses and the access code.

**On the PC that runs the tests (PC B):**
1. Open **Speed Test → Run test**.
2. Click **Find servers** (same network) or type PC A's IP address. Enter the code if one is required.
3. Choose a test:

| Test | Measures | Options |
|---|---|---|
| **TCP throughput** | Maximum speed, retransmits, round-trip time | Upload ↑ / Download ↓ / Both ⇅; parallel streams 1–16; duration |
| **UDP** | Packet loss, jitter, out-of-order packets at a fixed speed | Bitrate (Mbit/s), packet size |
| **Latency** | Round-trip time distribution | Probes per second |
| **Full suite** | Everything above in sequence, with one summary table | Duration per test |

4. Click **Start test** and watch the live chart.

**Results** show the average, minimum, maximum and variation of the speed, the data transferred, and depending on the test:
* retransmits and round-trip time (TCP);
* loss %, jitter and out-of-order packets (UDP);
* min / median / 90th / 99th percentile / max round-trip time (latency).

**Show intervals** lists every second of the test. **Export** saves a text report. Earlier results of the session are listed below.

If PC B cannot connect, allow the port on PC A with `sudo ufw allow 5201`, and check the IP address.

### 3.3 Net Tools

| Tool | How to use |
|---|---|
| **Ping** | Enter a host or IP, choose count and interval, **Run**. Shows a live round-trip chart, average, min/max, jitter and packet loss |
| **Traceroute** | Shows every router on the path with its response time |
| **DNS lookup** | Shows the IPv4/IPv6 addresses of a name, how long the lookup took, and which DNS servers answered |
| **Port check** | Checks whether TCP ports are **open**, **closed** or **filtered** (firewall). Presets: Common, Web, Remote, Speed test |

### 3.4 PacketForge (TCP / UDP / serial terminal)

A tool for talking to devices and network services, in the spirit of Hercules and Packet Sender.

**Open a session:** click **New session** and choose a type.

| Type | Settings |
|---|---|
| **TCP client** | Host, port, *Reconnect automatically* |
| **TCP server** | Listen on all interfaces or this computer only, port, max clients |
| **UDP** | Bind address, local port, optional remote host/port, broadcast, multicast group |
| **Serial** | Port (USB adapters show their name and serial number), baud rate (or custom), data bits, parity, stop bits, flow control, DTR/RTS at open |

Optionally save the settings as a **profile**; saved profiles appear at the top of the dialog for one-click reopening. Several sessions can be open at once, as tabs.

**Reading data (the log):**
* **Format:** Text, Hex, Text+Hex, Dump (`hexdump -C`), Dec, Bin.
* **Encoding:** UTF-8, Latin-1, ASCII.
* **Timestamps:** clock time, since start, or time between lines.
* **RX / TX / Events** check boxes show or hide received data, sent data and connection events.
* **Highlight** marks lines containing text, or bytes when *hex* is ticked; *only* hides everything else.
* **Follow** keeps the newest data in view. **Clear** and **Export** are next to it.
* Non-printable bytes in text views are shown in standard C notation and coloured: `\r`, `\n`, `\x1B`. Tick **Hercules** if you prefer `{0D}` style.

**Framing** (right panel) decides where a received message starts and ends:

| Mode | Use when |
|---|---|
| **After a pause** (default, 20 ms) | Each device message is a burst of bytes — works with or without line endings |
| **Split at newline** | Text protocols that end lines with `\n` |
| **Split at delimiter byte** | Binary protocols with an end byte, e.g. `03` |
| **Fixed length** | Fixed-size frames |
| **Every read separately** | Raw view |

**Sending data:**
1. Choose **Text** or **Hex**.
2. Type the data:
   * Text accepts escapes: `\r \n \t \0 \e \\ \xHH`, e.g. `AT\r\n`.
   * Hex accepts `01 03 00 0A`, `0x01,0x03` or `0103`.
3. Optional: append a **line ending** (CR, LF, CR LF) and a **checksum** (CRC-16/MODBUS, CRC-16/CCITT, CRC-32, XOR-8, SUM-8, LRC). The preview shows the exact bytes before sending.
4. **Send** (or `Ctrl+Enter`). For a TCP server choose *All clients* or one client; for UDP choose the remote, *Reply to last sender*, or another address.

More sending options:
* **Repeat every N ms** (optionally N times); stop at any time.
* **Send file…** sends a file in chunks.
* `Alt+↑ / Alt+↓` recalls earlier sends.
* **+ Macro** saves the current message as a button. Give it an **F1–F12** shortcut to send with one key.

**Session panel (right):**
* Status, bytes and speed received and sent.
* **Log to file…** records every byte with timestamps.
* **Save profile**, and **Reconnect** after a disconnect.
* For servers, the list of connected clients.
* For serial: **DTR / RTS** buttons, **CTS / DSR / DCD / RI** indicators and **Break**.

**Serial permission:** Ubuntu allows serial ports only for members of the `dialout` group. If you're not a member, PacketForge shows a banner. Click **Grant serial access**, enter your password, then **log out and back in**.

### 3.5 NetLens (packet capture and analysis)

NetLens records the network traffic going through your network cards and explains it, like Wireshark — with a few extras: it shows **which application** sent each packet, explains each packet in plain English, and lists the problems it found.

**First time only — allow capturing.** Capturing network traffic needs special rights. Click **Grant capture permission** and enter your password. Only the small `syspulse-capture` helper receives this right (not SysPulse itself). Without it you can still open saved capture files.

**Capture traffic:**
1. Click the network cards to capture on (for example `enp3s0` for wired, `wlp5s0` for Wi-Fi). Their IP addresses are shown on the chips.
2. Optional — **Capture filter**: record only what you need, in the standard tcpdump syntax. Use **Presets** or type, for example:
   * `host 192.168.1.64` — only one device (a camera)
   * `tcp port 554 or tcp port 8554` — RTSP control
   * `tcp port 80 or tcp port 443 or udp port 443` — web traffic
   * `not port 22` — everything except SSH
3. Press **Start capture**. Packets appear live; the status bar shows packets, data, packets per second and speed. **Stop** ends it.
4. **Options**: promiscuous mode, bytes kept per packet, and automatic stop after N packets / MB / seconds.

**The Packets tab:**

| Area | What it shows |
|---|---|
| **Packet list** | No., time (choose **Time**: time of day, date and time, seconds since start, or since previous packet), source, destination, protocol, length, **App** (live captures) and a summary. ↑ = sent by this PC, ↓ = received. Rows are tinted by protocol; problems are shown in orange/red text. Arrow keys, Page Up/Down, Home/End move the selection; **Auto-scroll** follows new packets |
| **Summary** (blue box) | A plain-English explanation of the selected packet, e.g. *"firefox on this PC asks the DNS server 8.8.8.8 for the address of github.com"* |
| **Detail tree** | Every protocol layer and field. Select a field to highlight its bytes; then **Filter**, **+ and** or **Exclude** turn it into a display filter |
| **Bytes** | Hex and text of the packet; click a byte to find the field it belongs to |

Drag the bar between the list and the details to resize. Right-click a packet for: **Follow stream**, **Only this conversation**, **Only / Hide** an address, **Only** an app, **Decode with Wireshark**, **Copy row**.

**Display filters** (the search bar) hide packets without deleting them. Type and press **Enter**; the bar turns green when the filter is valid and red with an explanation when not. Field names are suggested while typing (↑/↓ and Tab to pick). Quick buttons: HTTP, HTTPS, TLS handshakes, RTSP, Media (RTP), DNS, TCP, UDP, Connections opened, **Problems**. Examples:

| Filter | Shows |
|---|---|
| `ip.addr == 192.168.1.64` | Everything to or from one device |
| `ip.addr == 10.0.0.0/8` | A whole subnet |
| `tcp.port in {80 443 8080}` | Several ports |
| `http.request.method == "POST"` | Web form submissions |
| `http.response.code >= 400` | Web errors |
| `tls.handshake.extensions_server_name contains "google"` | HTTPS connections to Google sites |
| `rtsp.method == "SETUP"` | How a camera stream is being set up |
| `rtsp.status == 401` | Camera asked for a password |
| `rtp.keyframe` | Video key frames |
| `dns.flags.rcode != 0` | Failed name lookups |
| `dns.time > 0.2` | Slow DNS answers (over 200 ms) |
| `tcp.analysis.retransmission` | Re-sent data |
| `app.name == "firefox"` | One application (live captures) |
| `frame.len > 1000 && !tls` | Large packets that are not encrypted web traffic |

Words work too: `and`, `or`, `not`. Text in quotes; `matches` takes a regular expression (case-insensitive).

**Follow stream** shows the whole conversation of a connection as text (or hex): what the client sent in red, what the server answered in blue — for example a complete HTTP or RTSP dialogue. **Show these packets** filters the list to that conversation.

**Other tabs:**

| Tab | Shows |
|---|---|
| **Conversations** | Every TCP connection / UDP flow: both ends, website or host name, application, packets, data each way, duration, state (open, closed, reset) and issues. Sort by clicking a column; **Packets** / **Follow** buttons |
| **Applications** | Download/upload per program on this PC, connection count, the sites it talked to, and issues |
| **RTP streams** | Audio/video streams (IP cameras, VoIP): codec, packets, **lost packets**, sequence errors, jitter, largest gap, frames, key frames, bitrate |
| **Statistics** | Totals, traffic over time (bits/s and packets/s), and the protocol hierarchy (how much of the traffic is TLS, HTTP, RTP…) |
| **Insights** | Problems found, each with an explanation and **Show packets**: retransmissions, lost segments, zero windows, resets, failed or slow DNS, HTTP 4xx/5xx, RTSP errors and authentication, TLS failures, ICMP errors, key-frame requests, lost RTP packets, and packets the capture itself dropped |

**Files:** **Open** reads `.pcap` / `.pcapng` files (from Wireshark, tcpdump, or NetLens). **Save** writes `.pcapng` (opens in Wireshark); **Save displayed** writes only the filtered packets. NetLens asks before discarding an unsaved capture.

**Wireshark integration (optional):** if Wireshark is installed, **Wireshark ↗** opens the current capture in it. If `tshark` is installed (`sudo apt install tshark`), **Wireshark decode** shows the selected packet decoded by Wireshark's 3,000+ protocol decoders — useful for rare protocols.

**Tips for IP cameras (RTSP):**
* Start the capture **before** opening the stream in the player (VLC, NVR), so NetLens sees the RTSP SETUP that announces the video ports. Then the RTP streams tab shows loss and jitter for each stream.
* A `401` reply is normal once (the camera asks for the password); repeated 401s mean wrong credentials.
* Many *PLI/FIR key-frame requests* or lost RTP packets explain smeared or frozen video.

**Limits:** HTTPS and QUIC content is encrypted and cannot be read — NetLens shows the website name, TLS version and certificate instead. On switched and Wi-Fi networks you see your own PC's traffic (plus broadcasts), the same as with Wireshark.

### 3.6 Connections

Every open port and network connection on the computer, refreshed every 3 seconds.

* Tiles filter **All**, **Listening**, **Established** and **UDP**.
* Each row: protocol, local and remote address with well-known service names (ssh, https, dns…), state, process and user.
* Ports reachable from other computers are marked **network**; check these if you're worried about security.
* Sockets of system services show *system* (their process names need administrator rights).

---

## 4. System

### 4.1 Hardware

A full inventory of your computer, from the sections list on the left:
* System, motherboard, BIOS/UEFI (version, date, Secure Boot) and operating system.
* Processor: cores, caches, microcode, features, frequencies, governor.
* Memory and swap.
* Graphics adapters and **connected displays** (manufacturer, model, size).
* Storage: model, firmware, capacity, interface, SSD/HDD.
* Network adapters: driver and version.
* Audio cards and codecs.
* Battery: health = full capacity vs design capacity, cycle count.
* USB and PCI devices.

**Search all hardware** finds any value (for example a firmware version). **Copy report** copies the whole inventory as text, which is useful for support requests. **Rescan** refreshes after plugging in devices.

### 4.2 Services

Background services managed by systemd.

* **System** or **My session** services.
* Tiles filter **Total**, **Running**, **Failed** (red), **Start at boot** and **Disabled**.
* Click a service to see its main process, memory, CPU time, restarts, unit file and **recent log** lines.
* Buttons: **Start**, **Stop**, **Restart**, **Enable at boot**, **Disable at boot**. System services ask for your password. Services Ubuntu needs to work (marked ⚠) cannot be stopped or disabled here.

### 4.3 Startup Apps

Programs that start when you log in.

* The switch on each card turns autostart on or off.
* **Add program** takes a name, command, comment and optional delay in seconds. **Edit** and **Remove** are on each card (system entries can only be turned off).
* **Show system entries** reveals background components that Ubuntu normally hides.

Changes affect only your account (`~/.config/autostart`).

---

## 5. Maintenance

### 5.1 Software

All installed software from **APT**, **Snap** and **Flatpak**, with icons.

* Source tiles (click to include or exclude), *Applications* or *All packages*, search and sort (name, size, install date).
* Click an item for details: version, size, install date, publisher, homepage, whether it was installed automatically.

**Uninstall:**
1. Click **Uninstall**. SysPulse first works out **exactly** what will be removed, including dependent packages, and how much space is freed.
2. Optionally tick *Also remove system-wide configuration files (purge)* (APT) or *Delete the app's saved data too* (Snap).
3. Confirm and enter your password.

Essential system packages, and removals that would break the desktop (for example `gnome-shell`), are blocked and labelled **System**.

### 5.2 Cache Cleaner

Shows how much space each cache uses and where it is.

| Group | Items |
|---|---|
| **System** (password) | APT package cache, journal logs (keeps the newest 100 MB), old rotated logs, crash reports, core dumps, old Snap revisions, unused packages and old kernels |
| **Your files** | Thumbnails, Trash, GNOME search index (Tracker), Flatpak app caches, other application caches |
| **Browsers** | Firefox, Chrome, Chromium, Brave, Edge, Vivaldi/Opera caches (close the browser first) |
| **Developer tools** | VS Code, pip, npm/yarn, Go, Cargo, Gradle, Conda archives, Hugging Face/PyTorch models, JetBrains |

1. Tick the items to clean. Safe items are pre-selected; items marked ⚠ (Trash, unused packages, model caches…) are not.
2. **Details** lists exactly what an item contains (for example which packages autoremove would delete).
3. Click **Clean selected** and confirm. Caches are rebuilt automatically by the programs that use them.

### 5.3 Large Files

Finds what is filling your disk.

1. Choose a folder (**Home**, **Whole disk**, or **Browse…**) and a minimum size.
2. Optionally filter by type (Video, Archives, Disk images…), include hidden files, and stay on one disk.
3. Click **Scan**. Progress is live and **Cancel** stops it. Your home folder usually takes a few seconds.

Results:

| View | Shows |
|---|---|
| **Largest files** | Tick files, then **Open folder**, **Move to Trash** (can be restored) or **Delete** (permanent). Files owned by other users ask for your password |
| **Disk map** | Each block is a folder sized by the space it uses, with its sub-folders inside. Click to zoom; use the breadcrumb to go back |
| **Biggest folders / Hot spots** | Click to rescan that folder |
| **Space by file type** | Which kinds of files use the most space |

### 5.4 File Search

1. Type a name and choose the match mode:

| Mode | Example |
|---|---|
| **Contains** | `report` finds `Report_2024.pdf` |
| **Wildcard** | `*.pdf`, `IMG_????.jpg` |
| **Regex** | `^invoice_\d{4}` |
| **Exact** | `README.md` |

   **Aa** matches upper/lower case exactly; **/path** matches the whole path instead of just the name.
2. Choose where to search: Home, Whole computer, or Browse.
3. Optional **Filters**:
   * files or folders;
   * extensions (`pdf, docx`);
   * size range in MB;
   * modified (today, last 7/30 days, last year, older);
   * owner;
   * type chips;
   * **Containing text** (searches inside text files);
   * hidden files, skipping `.git` / `node_modules`, staying on one disk.
4. Press **Search** (or `Enter`). Results appear while searching; **Stop** ends early.

Double-click a result to open it, or a folder to search inside it. Tick results to **Open**, **Show in folder**, **Copy path** or **Move to Trash**. Recent searches are offered as one-click chips.

---

## 6. Media

### 6.1 Screen Recorder

Records the screen with sound. Requires an X11 session ("Ubuntu on Xorg").

**1 — What to record**

| Option | Notes |
|---|---|
| **Full screen** | Pick the monitor |
| **Window** | Pick from the list; records that window even if others cover it. Keep its size constant while recording |
| **Region** | **Select region on screen…** shows a frozen picture of the screen. Drag a rectangle, then click **Record this area** (or press `Enter`). Press `A` for the whole screen, `Esc` to cancel. Fine-tune with the X/Y/W/H fields |

*Show mouse pointer* includes the cursor.

**2 — Audio**

| Setting | Meaning |
|---|---|
| **System sound → Follow current output** (recommended) | Records whatever you're listening on: speakers, Bluetooth earbuds or HDMI, decided when recording starts |
| **Microphone → Follow current input** | Records the current microphone |
| A specific device | Always that device |

* Tick one, both (mixed into one track), or neither (video only).
* Volume sliders and live **level meters** show the sound is arriving before you start.
* Warnings appear when:
  * sound is playing through a different device than the one selected;
  * a microphone has **nothing plugged in** (it would record only hiss);
  * you pick Bluetooth earbuds' microphone (it lowers their sound quality while recording).

**3 — Format & quality**

* **Container:** MP4 (most compatible), MKV (survives crashes), WebM.
* **Codec:** H.264 or H.265 (smaller files). **Frame rate:** 15/24/30/60. **Quality:** Standard / High / Maximum.
* **Encoder:** *Automatic* picks the best one that works. On systems where the GPU encoder isn't usable, the CPU is used (about half a core at 1080p30).

**4 — Options**

* **Countdown** (none, 3, 5, 10 s).
* **Hide SysPulse while recording** keeps the window out of the video; control the recording from the top bar instead.
* **Save to** folder (default `~/Videos/SysPulse`).

**Recording:**
1. Click **Start recording** (or press `Ctrl+Alt+R` anywhere). The countdown runs, SysPulse hides, recording starts.
2. The **top bar** (next to Wi-Fi and battery) shows a red record icon and **REC 00:12**. Its menu has Pause / Resume, Stop and save, Cancel recording, Open SysPulse and Quit.
3. **Pause** with `Ctrl+Alt+P` or the menu; the icon turns amber. Pauses leave no gap in the video.
4. **Stop** with `Ctrl+Alt+R` or *Stop and save*. A notification confirms the file and SysPulse returns to the recorder page. **Cancel** discards the recording.

The **Recordings** list shows your videos with **Play**, **Show in folder** and **Move to Trash**.

---

## 7. Settings and alerts

| Setting | Description |
|---|---|
| **Refresh interval** | How often metrics update (0.5–5 s) |
| **Chart history** | Number of samples kept in charts (60–600) |
| **Process CPU as share of whole machine** | Off: 100% = one core (like `top`). On: 100% = all cores |
| **Desktop notifications (alerts)** | Notify when: CPU or GPU temperature exceeds a limit, memory or a disk is almost full, or the battery is low. *Must last for* avoids false alarms from short spikes; *Repeat at most every* limits repeats |
| **Theme** | Dark, Light or System |

Alerts work in the background as long as SysPulse is running.

---

## 8. Keyboard shortcuts

| Where | Keys | Action |
|---|---|---|
| Anywhere (desktop) | `Ctrl+Alt+R` | Start / stop screen recording |
| Anywhere (desktop) | `Ctrl+Alt+P` | Pause / resume recording |
| Processes | `Delete` | End the selected process |
| Processes | `Esc` | Close the details panel |
| Dialogs | `Esc` | Cancel |
| PacketForge | `Ctrl+Enter` | Send |
| PacketForge | `Alt+↑ / Alt+↓` | Previous / next sent message |
| PacketForge | `F1`–`F12` | Send the macro bound to that key |
| NetLens | `Enter` (filter bar) | Apply the display filter |
| NetLens | `↑ / ↓`, `Page Up / Down`, `Home / End` | Move through the packet list |
| NetLens | `Enter` / double-click (packet) | Follow the stream of the selected packet |
| NetLens | `Tab` (filter bar) | Accept a field-name suggestion |
| Region picker | `Enter` / `A` / `Esc` | Confirm / whole screen / cancel |
| File Search, Net Tools | `Enter` | Run |

---

## 9. Common tasks (quick recipes)

| I want to… | Do this |
|---|---|
| Find out why the computer is slow | Dashboard → *Top processes*; Processes sorted by CPU or Memory; Performance → CPU per core |
| Stop a frozen program | Processes → search its name → **Kill** |
| Free disk space | Cache Cleaner → **Clean selected**; then Large Files → scan Home → Disk map |
| Uninstall an app | Software → search → **Uninstall** → review → confirm |
| Set a static IP | Network Settings → **Edit IP settings** → Manual → address, gateway, DNS → **Save & apply** |
| Test the network between two PCs | PC A: Speed Test → Server mode → Start. PC B: Find servers → **Full suite** |
| Talk to a USB serial device | PacketForge → New session → Serial → choose port and baud → Connect |
| Record a tutorial with my voice | Screen Recorder → Full screen → System sound + Microphone (Follow) → Start (`Ctrl+Alt+R`) |
| See what a program sends over the network | NetLens → Start capture → **Applications** tab, or filter `app.name == "name"` |
| Check why camera video stutters | NetLens → capture filter `host <camera IP>` → Start → open the stream → **RTP streams** and **Insights** |
| Find which websites a PC contacts | NetLens → quick button **TLS handshakes** (website names in the Info column) |
| See which program uses a port | Connections → search the port number |
| Stop an app from starting at login | Startup Apps → switch it off |
| Check battery health | Hardware → Power → *Health* |

---

## 10. FAQ

**Is my data sent anywhere?**
No. SysPulse works entirely on your computer. Network traffic happens only when you start a speed test, ping, DNS lookup or PacketForge session. NetLens only listens: captured packets stay in memory until you save them to a file.

**Why does it ask for my password?**
Only for actions that change the system: uninstalling, cleaning system caches, system services, other users' processes, raising priority, serial access, and the one-time NetLens capture permission. SysPulse itself never runs as administrator.

**The recording has no sound or only noise.**
Set *System sound* to **Follow current output** and untick a microphone that shows **not plugged in**. Check the level meters before recording.

**Recording doesn't start on my laptop.**
You may be in a Wayland session. Log out, click the gear icon on the login screen and choose **Ubuntu on Xorg**.

**PacketForge shows all received data on one line.**
Choose **Framing → After a pause** (the default), or a delimiter that matches your device.

**The speed test cannot find or reach the other PC.**
Both PCs must be on the same network for *Find servers*. Otherwise type the IP shown in Server mode, and allow port 5201 in the firewall.

**The GPU shows "Sleeping".**
Your laptop has powered down the dedicated GPU to save energy. It wakes when an application uses it.

**NetLens shows "Grant capture permission" again.**
The permission belongs to the `syspulse-capture` file. Reinstalling or updating SysPulse replaces that file, so grant it once more.

**Can NetLens read my HTTPS traffic?**
No. It shows which site was contacted (the name in the TLS handshake), the TLS version and the certificate, but the content stays encrypted.

**Where are my settings?**
In `~/.config/io.github.praneeth.syspulse/`. Recordings are in `~/Videos/SysPulse/`.
