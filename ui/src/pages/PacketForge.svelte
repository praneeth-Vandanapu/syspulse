<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/PacketForge.svelte
  Author : Praneeth
  License: MIT
  Purpose: PacketForge - TCP client / TCP server / UDP / serial terminal.
           Session tabs, live traffic view in standard notations, framing,
           highlighting, send panel (text with C escapes or hex, line endings,
           checksums, repeat, file), macros with F-key shortcuts, saved
           profiles, serial modem lines and logging to file.
-->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import ForgeLog, { type LogOptions } from "../lib/components/ForgeLog.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import { api, isTauri, pickFile, pickSaveFile } from "../lib/api";
  import { forge, type Checksum, type LineEnding, type Macro, type SendSpec, type SerialAccess, type SerialPortInfo, type SessionConfig, type SessionView } from "../lib/forge.svelte";
  import { parseHex, toDump, toHex, toText } from "../lib/forgefmt";
  import { app } from "../lib/stores.svelte";
  import { bytes, rate } from "../lib/format";

  const BAUDS = [300, 1200, 2400, 4800, 9600, 14400, 19200, 38400, 57600, 115200, 230400, 250000, 460800, 500000, 921600, 1000000, 2000000, 3000000];
  const CHECKSUMS: { id: Checksum; label: string }[] = [
    { id: "none", label: "No checksum" },
    { id: "crc16-modbus", label: "CRC-16/MODBUS (LE)" },
    { id: "crc16-ccitt", label: "CRC-16/CCITT (BE)" },
    { id: "crc32", label: "CRC-32 (BE)" },
    { id: "xor8", label: "XOR-8" },
    { id: "sum8", label: "SUM-8" },
    { id: "lrc", label: "LRC" },
  ];
  const OPTS_KEY = "syspulse.forge.view";

  // ---------------------------------------------------------- view options ---
  function loadOpts(): LogOptions {
    const d: LogOptions = { format: "text", encoding: "utf8", timestamps: "absolute", hercules: false, showRx: true, showTx: true, showInfo: true, wrap: true, highlight: null, filterToHighlight: false, follow: true };
    try {
      return { ...d, ...JSON.parse(localStorage.getItem(OPTS_KEY) ?? "{}"), highlight: null, filterToHighlight: false };
    } catch {
      return d;
    }
  }
  let opts = $state<LogOptions>(loadOpts());
  $effect(() => {
    const { highlight: _h, filterToHighlight: _f, ...persist } = opts;
    try {
      localStorage.setItem(OPTS_KEY, JSON.stringify(persist));
    } catch {
      /* ignore */
    }
  });
  let hlText = $state("");
  let hlHex = $state(false);
  $effect(() => {
    const t = hlText.trim();
    opts.highlight = !t ? null : hlHex ? parseHex(t) : new TextEncoder().encode(t);
  });

  // ------------------------------------------------------------- sessions ---
  let s = $derived(forge.current);
  let access = $state<SerialAccess | null>(null);
  let creating = $state(false);

  onMount(async () => {
    await forge.start();
    access = await api.forgeSerialAccess().catch(() => null);
    if (!forge.sessions.length) creating = true;
  });

  // Rates (bytes/s) per session, sampled every second.
  let rates = $state<Record<number, { rx: number; tx: number }>>({});
  let prev: Record<number, { rx: number; tx: number }> = {};
  const rateTimer = setInterval(() => {
    const next: Record<number, { rx: number; tx: number }> = {};
    for (const v of forge.sessions) {
      const p = prev[v.id] ?? { rx: v.rx, tx: v.tx };
      next[v.id] = { rx: v.rx - p.rx, tx: v.tx - p.tx };
      prev[v.id] = { rx: v.rx, tx: v.tx };
    }
    rates = next;
  }, 1000);
  onDestroy(() => clearInterval(rateTimer));

  async function closeSession(v: SessionView) {
    await api.forgeClose(v.id).catch(() => {});
    forge.remove(v.id);
  }

  async function reopen(v: SessionView) {
    try {
      const info = await api.forgeOpen($state.snapshot(v.config) as SessionConfig);
      await api.forgeClose(v.id).catch(() => {});
      forge.remove(v.id);
      forge.add(info);
    } catch (e) {
      app.notify(String(e), "error", 8000);
    }
  }

  // ------------------------------------------------------------ new session ---
  type Kind = SessionConfig["kind"];
  let kind = $state<Kind>("tcpClient");
  let f = $state({
    host: "127.0.0.1", port: 5000, autoReconnect: false,
    bind: "0.0.0.0", sport: 5000, maxClients: 32,
    ubind: "0.0.0.0", localPort: 5000, remoteHost: "", remotePort: 5001 as number | null, broadcast: false, multicast: "",
    path: "", baud: 115200, customBaud: false, dataBits: 8, parity: "none" as "none" | "odd" | "even", stopBits: 1, flow: "none" as "none" | "hardware" | "software", dtr: true, rts: false,
  });
  let ports = $state<SerialPortInfo[]>([]);
  let saveAs = $state("");
  let opening = $state(false);

  async function refreshPorts() {
    ports = await api.forgePorts().catch(() => []);
    if (!f.path && ports.length) f.path = ports[0].path;
  }
  $effect(() => {
    if (creating && kind === "serial") refreshPorts();
  });

  function buildConfig(): SessionConfig {
    switch (kind) {
      case "tcpClient": return { kind, host: f.host.trim(), port: Number(f.port), autoReconnect: f.autoReconnect };
      case "tcpServer": return { kind, bind: f.bind.trim(), port: Number(f.sport), maxClients: Number(f.maxClients) };
      case "udp": return { kind, bind: f.ubind.trim(), localPort: Number(f.localPort), remoteHost: f.remoteHost.trim() || null, remotePort: f.remoteHost.trim() ? Number(f.remotePort) : null, broadcast: f.broadcast, multicastGroup: f.multicast.trim() || null };
      default: return { kind: "serial", path: f.path, baud: Number(f.baud), dataBits: Number(f.dataBits), parity: f.parity, stopBits: Number(f.stopBits), flow: f.flow, dtr: f.dtr, rts: f.rts };
    }
  }

  async function openSession(config: SessionConfig = buildConfig()) {
    opening = true;
    try {
      const info = await api.forgeOpen(config);
      forge.add(info);
      if (saveAs.trim()) forge.saveProfile(saveAs.trim(), config);
      saveAs = "";
      creating = false;
    } catch (e) {
      app.notify(String(e), "error", 9000);
    } finally {
      opening = false;
    }
  }

  async function grantSerial() {
    try {
      const msg = await api.forgeGrantSerial();
      app.notify(msg, "ok", 10000);
      access = await api.forgeSerialAccess();
      if (access) access.needsRelogin = true;
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  // ------------------------------------------------------------------ send ---
  let spec = $state<SendSpec>({ mode: "text", data: "", lineEnding: "none", checksum: "none", hercules: false });
  $effect(() => {
    spec.hercules = opts.hercules;
  });
  let target = $state<string>(""); // "" = default
  let customTarget = $state("");
  let preview = $state<{ hex: string; length: number; error: string | null } | null>(null);
  let repeatMs = $state(1000);
  let repeatCount = $state<number | null>(null);
  let histIdx = -1;
  let previewTimer: ReturnType<typeof setTimeout>;
  $effect(() => {
    const snap = $state.snapshot(spec) as SendSpec;
    clearTimeout(previewTimer);
    if (!snap.data && snap.lineEnding === "none") {
      preview = null;
      return;
    }
    previewTimer = setTimeout(async () => (preview = await api.forgeEncode(snap).catch(() => null)), 120);
  });

  function resolvedTarget(): string | null {
    if (!s) return null;
    if (s.config.kind === "udp") return target === "custom" ? customTarget.trim() || null : target || null;
    if (s.config.kind === "tcpServer") return target || null;
    return null;
  }

  async function send(sp: SendSpec = $state.snapshot(spec) as SendSpec) {
    if (!s) return;
    try {
      await api.forgeSend(s.id, sp, resolvedTarget());
      if (sp === spec || sp.data === spec.data) forge.remember(sp);
      histIdx = -1;
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function startRepeat() {
    if (!s) return;
    try {
      await api.forgeRepeat(s.id, $state.snapshot(spec) as SendSpec, resolvedTarget(), Number(repeatMs), repeatCount ? Number(repeatCount) : null);
      s.repeat = { active: true, sent: 0 };
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function sendFile() {
    if (!s) return;
    const path = await pickFile();
    if (!path) return;
    try {
      const n = await api.forgeSendFile(s.id, path, resolvedTarget(), 1024, 5);
      app.notify(`Sending ${bytes(n)} from ${path.split("/").pop()}`, "info");
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  function onSendKey(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      send();
    } else if (e.key === "ArrowUp" && e.altKey && forge.history.length) {
      histIdx = Math.min(histIdx + 1, forge.history.length - 1);
      spec = { ...forge.history[histIdx], hercules: opts.hercules };
    } else if (e.key === "ArrowDown" && e.altKey && histIdx > 0) {
      histIdx--;
      spec = { ...forge.history[histIdx], hercules: opts.hercules };
    }
  }

  // --------------------------------------------------------------- macros ---
  let macroEdit = $state<{ id?: string; name: string; key: string | null; spec: SendSpec } | null>(null);
  function onGlobalKey(e: KeyboardEvent) {
    if (/^F([1-9]|1[0-2])$/.test(e.key) && s) {
      const m = forge.macros.find((x) => x.key === e.key);
      if (m) {
        e.preventDefault();
        send({ ...m.spec, hercules: opts.hercules && m.spec.hercules });
      }
    }
  }
  function saveMacro() {
    if (!macroEdit || !macroEdit.name.trim()) return;
    forge.saveMacro(macroEdit.name.trim(), macroEdit.spec, macroEdit.key, macroEdit.id);
    macroEdit = null;
  }

  // -------------------------------------------------------------- logging ---
  async function toggleLog() {
    if (!s) return;
    if (s.logPath) {
      await api.forgeLog(s.id, null);
      app.notify(`Stopped logging to ${s.logPath}`, "info");
      s.logPath = null;
      return;
    }
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    const path = await pickSaveFile(`packetforge-${stamp}.log`, "log");
    if (!path) return;
    try {
      await api.forgeLog(s.id, path);
      s.logPath = path;
      app.notify(`Logging all traffic to ${path}`);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function exportView() {
    if (!s) return;
    const lines = s.records.map((r) => {
      const ts = new Date(r.t).toISOString();
      if (r.dir === "info") return `${ts} -- ${r.note}`;
      const body = opts.format === "hex" ? toHex(r.bytes) : opts.format === "dump" ? "\n" + toDump(r.bytes).join("\n") : toText(r.bytes, opts.encoding, opts.hercules).map((x) => x.t).join("");
      return `${ts} ${r.dir.toUpperCase()}${r.peer ? " " + r.peer : ""} [${r.bytes.length}] ${body}`;
    });
    const path = await pickSaveFile(`packetforge-view.txt`, "txt");
    if (!path) return;
    try {
      await api.exportText(path, lines.join("\n") + "\n");
      app.notify(`Exported ${lines.length} records`);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  const stateColor = (st: string) => (st === "open" ? "var(--good)" : st === "listening" ? "var(--accent)" : st === "connecting" ? "var(--warning)" : "var(--text-muted)");
  const kindLabel: Record<Kind, string> = { tcpClient: "TCP client", tcpServer: "TCP server", udp: "UDP", serial: "Serial" };
  const usbLabel = (p: SerialPortInfo) => `${p.path} — ${p.description}${p.serialNumber ? ` (SN ${p.serialNumber})` : ""}${p.vid != null ? ` [${p.vid.toString(16).padStart(4, "0")}:${p.pid!.toString(16).padStart(4, "0")}]` : ""}`;
</script>

<svelte:window onkeydown={onGlobalKey} />

<div class="pf">
  <header class="top">
    <div class="brand"><h1>PacketForge</h1><span class="muted">TCP · UDP · Serial terminal</span></div>
    <nav class="tabs" aria-label="Sessions">
      {#each forge.sessions as v (v.id)}
        <div class="tab" class:on={forge.active === v.id}>
          <button class="tabbtn" onclick={() => (forge.active = v.id)} title={v.message}>
            <span class="dot" style:background={stateColor(v.state)}></span>{v.title}
          </button>
          <button class="x" onclick={() => closeSession(v)} aria-label="Close session"><Icon name="close" size={12} /></button>
        </div>
      {/each}
      <button class="tab new" onclick={() => (creating = true)}><Icon name="play" size={12} /> New session</button>
    </nav>
  </header>

  {#if access && (!access.inGroup || access.needsRelogin)}
    <div class="banner">
      <Icon name="alert" size={15} />
      {#if access.needsRelogin}
        Serial access was granted to <b>{access.user}</b>. Log out and back in to start using serial ports.
      {:else}
        Your account (<b>{access.user}</b>) is not in the <b>dialout</b> group, so serial ports cannot be opened.
        <button class="small primary" onclick={grantSerial}>Grant serial access</button>
        <span class="muted">(asks for your password once; then log out and back in)</span>
      {/if}
    </div>
  {/if}

  {#if s}
    {@const v = s}
    <div class="main">
      <section class="logcol">
        <div class="viewbar">
          <div class="seg">
            {#each [["text", "Text"], ["hex", "Hex"], ["mixed", "Text+Hex"], ["dump", "Dump"], ["dec", "Dec"], ["bin", "Bin"]] as [id, l] (id)}
              <button class:on={opts.format === id} onclick={() => (opts.format = id as LogOptions["format"])}>{l}</button>
            {/each}
          </div>
          <select bind:value={opts.encoding} title="Text encoding"><option value="utf8">UTF-8</option><option value="latin1">Latin-1</option><option value="ascii">ASCII</option></select>
          <select bind:value={opts.timestamps} title="Timestamps"><option value="absolute">Clock time</option><option value="relative">Since start</option><option value="delta">Δ between lines</option><option value="none">No time</option></select>
          <label class="chk"><input type="checkbox" bind:checked={opts.showRx} /> RX</label>
          <label class="chk"><input type="checkbox" bind:checked={opts.showTx} /> TX</label>
          <label class="chk"><input type="checkbox" bind:checked={opts.showInfo} /> Events</label>
          <label class="chk"><input type="checkbox" bind:checked={opts.wrap} /> Wrap</label>
          <label class="chk" title="Hercules-compatible notation: {'{0D}'} display and $0D input"><input type="checkbox" bind:checked={opts.hercules} /> Hercules</label>
          <span class="grow"></span>
          <div class="hlbox" title="Highlight records containing this text (or hex bytes)">
            <Icon name="search" size={13} />
            <input placeholder={hlHex ? "0D 0A" : "highlight…"} bind:value={hlText} class="mono" />
            <label class="mini"><input type="checkbox" bind:checked={hlHex} />hex</label>
            <label class="mini"><input type="checkbox" bind:checked={opts.filterToHighlight} />only</label>
          </div>
          <button class="small" class:on={opts.follow} onclick={() => (opts.follow = !opts.follow)} title="Auto-scroll to new data">Follow</button>
          <button class="small" onclick={() => v.clear()}>Clear</button>
          <button class="small" onclick={exportView}>Export</button>
        </div>
        <div class="logwrap"><ForgeLog session={v} {opts} /></div>

        <!-- Send panel -->
        <div class="send">
          <div class="sendrow">
            <div class="seg">
              <button class:on={spec.mode === "text"} onclick={() => (spec.mode = "text")}>Text</button>
              <button class:on={spec.mode === "hex"} onclick={() => (spec.mode = "hex")}>Hex</button>
            </div>
            <select bind:value={spec.lineEnding} title="Append line ending">
              {#each [["none", "No line ending"], ["cr", "+ CR (\\r)"], ["lf", "+ LF (\\n)"], ["crlf", "+ CR LF (\\r\\n)"]] as [id, l] (id)}<option value={id}>{l}</option>{/each}
            </select>
            <select bind:value={spec.checksum} title="Append checksum">{#each CHECKSUMS as c (c.id)}<option value={c.id}>{c.label}</option>{/each}</select>
            {#if v.config.kind === "tcpServer"}
              <select bind:value={target} title="Send to"><option value="">All clients ({v.clients.length})</option>{#each v.clients as c (c)}<option value={c}>{c}</option>{/each}</select>
            {:else if v.config.kind === "udp"}
              <select bind:value={target} title="Send to">
                <option value="">{v.config.remoteHost ? `${v.config.remoteHost}:${v.config.remotePort}` : "Remote (not set)"}</option>
                <option value="last">Reply to last sender</option>
                <option value="custom">Other address…</option>
              </select>
              {#if target === "custom"}<input class="mono w160" placeholder="host:port" bind:value={customTarget} />{/if}
            {/if}
            <span class="grow"></span>
            {#if preview}
              <span class="preview mono" class:err={preview.error} title={preview.hex}>{preview.error ?? `${preview.length} B: ${preview.hex}`}</span>
            {/if}
          </div>
          <div class="sendrow">
            <textarea
              class="mono"
              rows="2"
              bind:value={spec.data}
              onkeydown={onSendKey}
              placeholder={spec.mode === "hex" ? "01 03 00 00 00 0A   (also 0x01,0x03 or 0103)" : opts.hercules ? "AT$0D$0A  ($HH = hex byte)" : "AT\\r\\n   (escapes: \\r \\n \\t \\0 \\e \\\\ \\xHH)"}
              spellcheck="false"
              aria-label="Data to send"
            ></textarea>
            <div class="sendbtns">
              <button class="primary" onclick={() => send()} disabled={!!preview?.error} title="Ctrl+Enter"><Icon name="play" size={13} /> Send</button>
              <button class="small" onclick={() => (macroEdit = { name: "", key: null, spec: $state.snapshot(spec) as SendSpec })} title="Save as macro">+ Macro</button>
            </div>
          </div>
          <div class="sendrow small-row">
            <span class="muted">Repeat every</span><input type="number" min="1" class="w80" bind:value={repeatMs} /><span class="muted">ms ×</span>
            <input type="number" min="1" class="w80" placeholder="∞" bind:value={repeatCount} />
            {#if v.repeat?.active}
              <button class="small danger" onclick={() => api.forgeStopJobs(v.id)}>Stop ({v.repeat.sent})</button>
            {:else}
              <button class="small" onclick={startRepeat} disabled={!!preview?.error}>Start repeat</button>
            {/if}
            <span class="sep"></span>
            {#if v.file?.active}
              <span class="muted">Sending file {Math.round((v.file.sent / Math.max(1, v.file.total)) * 100)}%</span>
              <button class="small danger" onclick={() => api.forgeStopJobs(v.id)}>Stop</button>
            {:else}
              <button class="small" onclick={sendFile}>Send file…</button>
            {/if}
            <span class="grow"></span>
            <span class="muted">Alt+↑/↓ history · Ctrl+Enter send · F1–F12 macros</span>
          </div>
          {#if forge.macros.length}
            <div class="macros">
              {#each forge.macros as m (m.id)}
                <span class="macro">
                  <button class="small" onclick={() => send({ ...m.spec, hercules: m.spec.hercules })} title="{m.spec.mode}: {m.spec.data}">{#if m.key}<b>{m.key}</b> {/if}{m.name}</button>
                  <button class="ghost tiny" onclick={() => (macroEdit = { id: m.id, name: m.name, key: m.key, spec: { ...m.spec } })} aria-label="Edit macro"><Icon name="settings" size={11} /></button>
                </span>
              {/each}
            </div>
          {/if}
        </div>
      </section>

      <aside class="side">
        <section class="panel">
          <div class="st"><span class="dot" style:background={stateColor(v.state)}></span><b>{v.state}</b></div>
          <div class="muted small msg">{v.message}</div>
          <div class="kv">
            <span>Type</span><b>{kindLabel[v.config.kind]}</b>
            <span>Received</span><b class="num">{bytes(v.rx)} · {rate(rates[v.id]?.rx ?? 0)}</b>
            <span>Sent</span><b class="num">{bytes(v.tx)} · {rate(rates[v.id]?.tx ?? 0)}</b>
            <span>Records</span><b class="num">{v.rxRecs} RX · {v.txRecs} TX</b>
          </div>
          <div class="btnrow">
            {#if v.state === "closed"}<button class="small primary" onclick={() => reopen(v)}>Reconnect</button>{/if}
            <button class="small" onclick={toggleLog} class:on={!!v.logPath}>{v.logPath ? "Stop logging" : "Log to file…"}</button>
            <button class="small" onclick={() => forge.saveProfile(v.title, $state.snapshot(v.config) as SessionConfig)}>Save profile</button>
          </div>
          {#if v.logPath}<div class="muted small mono wrapall">{v.logPath}</div>{/if}
        </section>

        <section class="panel">
          <h4>Framing (received data)</h4>
          <select bind:value={v.framing.mode}>
            <option value="gap">After a pause (default)</option>
            <option value="newline">Split at newline (\n)</option>
            <option value="delimiter">Split at delimiter byte</option>
            <option value="length">Fixed length</option>
            <option value="chunk">Every read separately</option>
          </select>
          {#if v.framing.mode === "newline"}
            <p class="muted small hint">Lines without a newline are closed after 0.5 s of silence.</p>
          {/if}
          {#if v.framing.mode === "delimiter"}
            <label class="lbl">Delimiter (hex) <input class="mono w80" value={v.framing.delimiter.toString(16).padStart(2, "0").toUpperCase()}
              onchange={(e) => { const b = parseInt((e.currentTarget as HTMLInputElement).value, 16); if (b >= 0 && b < 256) v.framing.delimiter = b; }} /></label>
          {:else if v.framing.mode === "gap"}
            <label class="lbl">Pause <input type="number" min="1" class="w80" bind:value={v.framing.gapMs} /> ms</label>
            <p class="muted small hint">Data arriving within this pause belongs to the same message.</p>
          {:else if v.framing.mode === "length"}
            <label class="lbl">Length <input type="number" min="1" max="4096" class="w80" bind:value={v.framing.length} /> bytes</label>
          {/if}
        </section>

        {#if v.config.kind === "tcpServer"}
          <section class="panel">
            <h4>Clients ({v.clients.length})</h4>
            {#each v.clients as c (c)}<div class="mono small client">{c}</div>{:else}<div class="muted small">Waiting for connections…</div>{/each}
          </section>
        {/if}

        {#if v.config.kind === "serial"}
          <section class="panel">
            <h4>Modem lines</h4>
            <div class="lines">
              <button class="line" class:on={v.lines?.dtr} onclick={() => api.forgeLines(v.id, !v.lines?.dtr, null)}>DTR</button>
              <button class="line" class:on={v.lines?.rts} onclick={() => api.forgeLines(v.id, null, !v.lines?.rts)} disabled={v.config.flow === "hardware"}>RTS</button>
              <button class="small" onclick={() => api.forgeBreak(v.id, 250)}>Break</button>
            </div>
            <div class="leds">
              {#each [["CTS", v.lines?.cts], ["DSR", v.lines?.dsr], ["DCD", v.lines?.dcd], ["RI", v.lines?.ri]] as [n, on] (n)}
                <span class="led" class:on={!!on}><i></i>{n}</span>
              {/each}
            </div>
          </section>
        {/if}
      </aside>
    </div>
  {:else if !creating}
    <div class="empty"><p>No sessions open.</p><button class="primary" onclick={() => (creating = true)}>New session</button></div>
  {/if}
</div>

{#if creating}
  <Modal title="New PacketForge session" onclose={() => forge.sessions.length && (creating = false)} width={620}>
    {#if forge.profiles.length}
      <div class="profiles">
        <span class="muted small">Saved profiles</span>
        {#each forge.profiles as p (p.id)}
          <span class="prof"><button class="small" onclick={() => openSession(p.config)}>{p.name}</button><button class="ghost tiny" onclick={() => forge.deleteProfile(p.id)} aria-label="Delete profile"><Icon name="close" size={11} /></button></span>
        {/each}
      </div>
    {/if}
    <div class="seg kinds">
      {#each [["tcpClient", "TCP client"], ["tcpServer", "TCP server"], ["udp", "UDP"], ["serial", "Serial"]] as [id, l] (id)}
        <button class:on={kind === id} onclick={() => (kind = id as Kind)}>{l}</button>
      {/each}
    </div>
    <div class="form">
      {#if kind === "tcpClient"}
        <label>Host <input class="mono" bind:value={f.host} placeholder="192.168.1.10 or device.local" /></label>
        <label>Port <input type="number" min="1" max="65535" bind:value={f.port} /></label>
        <label class="chk"><input type="checkbox" bind:checked={f.autoReconnect} /> Reconnect automatically if the connection drops</label>
      {:else if kind === "tcpServer"}
        <label>Listen on
          <select bind:value={f.bind}><option value="0.0.0.0">All interfaces (0.0.0.0)</option><option value="127.0.0.1">This computer only (127.0.0.1)</option></select>
        </label>
        <label>Port <input type="number" min="1" max="65535" bind:value={f.sport} /></label>
        <label>Max clients <input type="number" min="1" max="256" bind:value={f.maxClients} /></label>
        {#if f.bind === "0.0.0.0"}<p class="muted small"><Icon name="info" size={12} /> Other computers on your network can connect while this session is open.</p>{/if}
      {:else if kind === "udp"}
        <label>Bind <select bind:value={f.ubind}><option value="0.0.0.0">All interfaces</option><option value="127.0.0.1">Local only</option></select></label>
        <label>Local port <input type="number" min="0" max="65535" bind:value={f.localPort} /> <span class="muted small">0 = any</span></label>
        <label>Remote host <input class="mono" bind:value={f.remoteHost} placeholder="optional, e.g. 192.168.1.255" /></label>
        <label>Remote port <input type="number" min="1" max="65535" bind:value={f.remotePort} /></label>
        <label class="chk"><input type="checkbox" bind:checked={f.broadcast} /> Allow broadcast</label>
        <label>Multicast group <input class="mono" bind:value={f.multicast} placeholder="optional, e.g. 239.1.2.3" /></label>
      {:else}
        <label>Port
          <select bind:value={f.path}>
            {#each ports as p (p.path)}<option value={p.path}>{usbLabel(p)}{p.accessible ? "" : " — no access"}</option>{/each}
            {#if !ports.length}<option value="">No serial ports found</option>{/if}
          </select>
          <button class="ghost small" onclick={refreshPorts} aria-label="Refresh ports"><Icon name="refresh" size={13} /></button>
        </label>
        <label>Baud rate
          {#if f.customBaud}<input type="number" min="50" bind:value={f.baud} />{:else}<select bind:value={f.baud}>{#each BAUDS as b (b)}<option value={b}>{b}</option>{/each}</select>{/if}
          <label class="chk inline"><input type="checkbox" bind:checked={f.customBaud} /> custom</label>
        </label>
        <div class="grid4">
          <label>Data bits <select bind:value={f.dataBits}>{#each [8, 7, 6, 5] as n (n)}<option value={n}>{n}</option>{/each}</select></label>
          <label>Parity <select bind:value={f.parity}><option value="none">None</option><option value="even">Even</option><option value="odd">Odd</option></select></label>
          <label>Stop bits <select bind:value={f.stopBits}><option value={1}>1</option><option value={2}>2</option></select></label>
          <label>Flow control <select bind:value={f.flow}><option value="none">None</option><option value="hardware">RTS/CTS</option><option value="software">XON/XOFF</option></select></label>
        </div>
        <div class="row2"><label class="chk"><input type="checkbox" bind:checked={f.dtr} /> DTR on at open</label><label class="chk"><input type="checkbox" bind:checked={f.rts} /> RTS on at open</label></div>
      {/if}
      <label>Save as profile <input bind:value={saveAs} placeholder="optional name, e.g. ESP32 console" /></label>
    </div>
    {#snippet footer()}
      {#if forge.sessions.length}<button onclick={() => (creating = false)}>Cancel</button>{/if}
      <button class="primary" onclick={() => openSession()} disabled={opening || !isTauri || (kind === "serial" && !f.path)}>{opening ? "Opening…" : kind === "tcpServer" || kind === "udp" ? "Start" : "Connect"}</button>
    {/snippet}
  </Modal>
{/if}

{#if macroEdit}
  {@const me = macroEdit}
  <Modal title={me.id ? "Edit macro" : "New macro"} onclose={() => (macroEdit = null)} width={520}>
    <div class="form">
      <label>Name <input bind:value={me.name} placeholder="Read registers" /></label>
      <label>Shortcut <select bind:value={me.key}><option value={null}>None</option>{#each Array.from({ length: 12 }, (_, i) => `F${i + 1}`) as k (k)}<option value={k}>{k}</option>{/each}</select></label>
      <label>Mode <select bind:value={me.spec.mode}><option value="text">Text</option><option value="hex">Hex</option></select></label>
      <label>Data <input class="mono" bind:value={me.spec.data} /></label>
      <label>Line ending <select bind:value={me.spec.lineEnding}>{#each ["none", "cr", "lf", "crlf"] as l (l)}<option value={l as LineEnding}>{l.toUpperCase()}</option>{/each}</select></label>
      <label>Checksum <select bind:value={me.spec.checksum}>{#each CHECKSUMS as c (c.id)}<option value={c.id}>{c.label}</option>{/each}</select></label>
    </div>
    {#snippet footer()}
      {#if me.id}<button class="danger" onclick={() => { forge.deleteMacro(me.id!); macroEdit = null; }}>Delete</button>{/if}
      <span style:flex="1"></span>
      <button onclick={() => (macroEdit = null)}>Cancel</button>
      <button class="primary" onclick={saveMacro} disabled={!me.name.trim()}>Save</button>
    {/snippet}
  </Modal>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .pf { display: flex; flex-direction: column; height: calc(100vh - 54px); gap: 10px; }
  .top { display: flex; align-items: flex-end; gap: 18px; flex-wrap: wrap; }
  .brand { display: flex; flex-direction: column; }
  .brand .muted { font-size: 12.5px; }
  .tabs { display: flex; gap: 4px; flex-wrap: wrap; flex: 1; }
  .tab { display: inline-flex; align-items: center; background: var(--surface-1); border: 1px solid var(--border); border-radius: 9px; }
  .tab.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, var(--surface-1)); }
  .tabbtn { all: unset; cursor: pointer; display: inline-flex; gap: 7px; align-items: center; padding: 5px 4px 5px 10px; font-size: 12.5px; max-width: 260px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .x { border: none; background: transparent; padding: 4px 6px; color: var(--text-muted); }
  .tab.new { padding: 5px 10px; font-size: 12.5px; color: var(--accent); cursor: pointer; }
  .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; display: inline-block; }
  .banner { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; font-size: 13px; padding: 9px 12px; border-radius: 10px;
    background: color-mix(in srgb, var(--warning) 12%, transparent); border: 1px solid color-mix(in srgb, var(--warning) 35%, transparent); }
  .main { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) 270px; gap: 12px; }
  .logcol { display: flex; flex-direction: column; min-height: 0; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .viewbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; padding: 7px 10px; background: var(--surface-2); border-bottom: 1px solid var(--border); font-size: 12.5px; }
  .viewbar select { font-size: 12px; padding: 3px 6px; }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 8px; padding: 2px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 3px 9px; border-radius: 6px; color: var(--text-secondary); font-size: 12px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .chk { display: inline-flex; align-items: center; gap: 4px; font-size: 12.5px; color: var(--text-secondary); }
  .chk input, .mini input { accent-color: var(--accent); }
  .grow { flex: 1; }
  .hlbox { display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--border-strong); border-radius: 7px; padding: 0 6px; color: var(--text-muted); }
  .hlbox input { border: none; background: transparent; width: 110px; padding: 3px 0; font-size: 12px; }
  .mini { display: inline-flex; gap: 2px; align-items: center; font-size: 11px; }
  .small { font-size: 12px; padding: 3px 9px; }
  .tiny { padding: 2px 4px; }
  button.on { border-color: var(--accent); color: var(--text-primary); background: color-mix(in srgb, var(--accent) 16%, transparent); }
  .logwrap { flex: 1; min-height: 120px; }
  .send { border-top: 1px solid var(--border); padding: 8px 10px; display: flex; flex-direction: column; gap: 6px; background: var(--surface-2); }
  .sendrow { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .sendrow select { font-size: 12px; padding: 3px 6px; }
  .small-row { font-size: 12px; }
  textarea { flex: 1; min-width: 300px; resize: vertical; font-size: 13px; padding: 7px 9px; background: var(--surface-1); color: var(--text-primary); border: 1px solid var(--border-strong); border-radius: 8px; }
  textarea:focus { outline: none; border-color: var(--accent); }
  .sendbtns { display: flex; flex-direction: column; gap: 5px; }
  .preview { font-size: 11.5px; color: var(--text-secondary); max-width: 46%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .preview.err { color: var(--critical); }
  .w80 { width: 80px; }
  .w160 { width: 160px; }
  .sep { width: 1px; height: 18px; background: var(--border-strong); }
  .macros { display: flex; gap: 6px; flex-wrap: wrap; }
  .macro { display: inline-flex; align-items: center; }
  .macro b { color: var(--accent); font-size: 10.5px; }
  .side { display: flex; flex-direction: column; gap: 10px; overflow-y: auto; }
  .panel { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; }
  .panel h4 { margin: 0; font-size: 11.5px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  .st { display: flex; align-items: center; gap: 8px; text-transform: capitalize; }
  .msg { word-break: break-word; }
  .kv { display: grid; grid-template-columns: 70px 1fr; gap: 4px 8px; font-size: 12.5px; }
  .kv span { color: var(--text-muted); }
  .btnrow { display: flex; gap: 6px; flex-wrap: wrap; }
  .wrapall { word-break: break-all; }
  .hint { margin: 0; }
  .lbl { display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--text-secondary); }
  .client { padding: 2px 0; }
  .lines { display: flex; gap: 6px; }
  .line { font-size: 12px; padding: 3px 10px; font-family: var(--font-mono); }
  .leds { display: flex; gap: 12px; font-size: 11.5px; font-family: var(--font-mono); }
  .led { display: inline-flex; align-items: center; gap: 4px; color: var(--text-muted); }
  .led i { width: 9px; height: 9px; border-radius: 50%; background: var(--surface-3); display: inline-block; }
  .led.on { color: var(--text-primary); }
  .led.on i { background: var(--good); }
  .empty { margin: 12vh auto; text-align: center; color: var(--text-secondary); }
  .profiles { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-bottom: 12px; }
  .prof { display: inline-flex; align-items: center; }
  .kinds { margin-bottom: 14px; }
  .kinds button { padding: 5px 14px; font-size: 13px; }
  .form { display: flex; flex-direction: column; gap: 10px; color: var(--text-primary); }
  .form > label, .grid4 label { display: flex; align-items: center; gap: 10px; font-size: 13px; color: var(--text-secondary); }
  .form > label > input:not([type="checkbox"]), .form > label > select { flex: 1; }
  .grid4 { display: grid; grid-template-columns: 1fr 1fr; gap: 8px 14px; }
  .grid4 select { flex: 1; }
  .row2 { display: flex; gap: 16px; }
  .inline { flex: none !important; }
</style>
