<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/SpeedTest.svelte
  Author : Praneeth
  License: MIT
  Purpose: iperf-style network speed test between two PCs running SysPulse.
           One PC runs Server mode; the other runs TCP / UDP / latency tests
           (or the full suite) against it and gets live charts plus detailed
           results that can be exported.
-->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Card from "../lib/components/Card.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import LineChart from "../lib/components/LineChart.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Stat from "../lib/components/Stat.svelte";
  import { api, isTauri, onEvent, pickSaveFile } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import { bytes } from "../lib/format";
  import type { DirectionResult, DiscoveredServer, PerfDirection, PerfKind, PerfProgress, PerfResult, PerfServerEvent, PerfServerInfo, TestSpec } from "../lib/types";

  const PORT_DEFAULT = 5201;
  const SETTINGS_KEY = "syspulse.speedtest";

  // ------------------------------------------------------------ settings ---
  interface Saved { target: string; port: number; code: string; kind: PerfKind | "suite"; direction: PerfDirection; streams: number; duration: number; bitrate: number; packet: number; interval: number; pps: number }
  const defaults: Saved = { target: "", port: PORT_DEFAULT, code: "", kind: "tcp", direction: "upload", streams: 4, duration: 10, bitrate: 100, packet: 1400, interval: 1000, pps: 20 };
  function loadSaved(): Saved {
    try {
      return { ...defaults, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "{}") };
    } catch {
      return { ...defaults };
    }
  }
  let cfg = $state<Saved>(loadSaved());
  $effect(() => {
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(cfg));
    } catch {
      /* ignore */
    }
  });

  let mode = $state<"client" | "server">("client");

  // -------------------------------------------------------------- server ---
  let server = $state<PerfServerInfo | null>(null);
  let serverPort = $state(PORT_DEFAULT);
  let requireCode = $state(false);
  let serverLog = $state<{ at: string; text: string; kind: string }[]>([]);
  let serverLive = $state<{ t: number[]; up: number[]; down: number[] }>({ t: [], up: [], down: [] });
  let unlistenServer: (() => void) | null = null;

  function log(text: string, kind = "info") {
    serverLog = [{ at: new Date().toLocaleTimeString(), text, kind }, ...serverLog].slice(0, 100);
  }

  async function startServer() {
    const code = requireCode ? String(Math.floor(100000 + Math.random() * 900000)) : null;
    try {
      server = await api.perfServerStart(serverPort, code);
      log(`Listening on port ${server.port}${code ? " (access code required)" : ""}`);
    } catch (e) {
      app.notify(String(e), "error", 8000);
    }
  }
  async function stopServer() {
    await api.perfServerStop().catch(() => {});
    server = null;
    log("Server stopped");
  }

  // -------------------------------------------------------------- client ---
  let found = $state<DiscoveredServer[]>([]);
  let discovering = $state(false);
  let running = $state(false);
  let stage = $state("");
  let live = $state<{ t: number[]; up: number[]; down: number[]; rtt: (number | null)[] }>({ t: [], up: [], down: [], rtt: [] });
  let now = $state<{ up: number | null; down: number | null; rtt: number | null }>({ up: null, down: null, rtt: null });
  let results = $state<PerfResult[]>([]);
  let suite = $state<PerfResult[] | null>(null);
  let showIntervals = $state(false);
  let unlistenProgress: (() => void) | null = null;

  async function discover() {
    discovering = true;
    try {
      found = await api.perfDiscover(cfg.port);
      if (!found.length) app.notify("No SysPulse servers answered. Start Server mode on the other PC (same network), or type its IP.", "info", 7000);
    } finally {
      discovering = false;
    }
  }

  function spec(kind: PerfKind, direction: PerfDirection, overrides: Partial<TestSpec> = {}): TestSpec {
    return {
      kind,
      direction,
      streams: cfg.streams,
      durationSecs: cfg.duration,
      bitrateMbps: cfg.bitrate,
      packetSize: cfg.packet,
      intervalMs: cfg.interval,
      latencyPps: cfg.pps,
      ...overrides,
    };
  }

  function resetLive() {
    live = { t: [], up: [], down: [], rtt: [] };
    now = { up: null, down: null, rtt: null };
  }

  function onProgress(p: PerfProgress) {
    // Merge per-interval points (upload and download arrive separately).
    let i = live.t.findIndex((t) => Math.abs(t - p.t) < 0.01);
    const next = { t: [...live.t], up: [...live.up], down: [...live.down], rtt: [...live.rtt] };
    if (i < 0) {
      next.t.push(p.t);
      next.up.push(NaN);
      next.down.push(NaN);
      next.rtt.push(null);
      i = next.t.length - 1;
    }
    if (p.dir === "upload") {
      next.up[i] = p.mbps;
      now.up = p.mbps;
    } else if (p.dir === "download") {
      next.down[i] = p.mbps;
      now.down = p.mbps;
    } else {
      next.rtt[i] = p.rttMs;
      now.rtt = p.rttMs;
    }
    live = next;
  }

  async function runOne(s: TestSpec): Promise<PerfResult | null> {
    resetLive();
    try {
      const r = await api.perfRun(cfg.target.trim(), cfg.port, cfg.code.trim() || null, s);
      results = [r, ...results].slice(0, 20);
      return r;
    } catch (e) {
      app.notify(String(e), "error", 9000);
      return null;
    }
  }

  async function start() {
    if (!cfg.target.trim()) {
      app.notify("Enter the IP address of the PC running Server mode", "info");
      return;
    }
    running = true;
    suite = null;
    unlistenProgress?.();
    unlistenProgress = await onEvent<PerfProgress>("perf-progress", onProgress);
    try {
      if (cfg.kind === "suite") {
        await runSuite();
      } else {
        stage = cfg.kind.toUpperCase();
        await runOne(spec(cfg.kind, cfg.kind === "latency" ? "upload" : cfg.direction));
      }
    } finally {
      running = false;
      stage = "";
      unlistenProgress?.();
      unlistenProgress = null;
    }
  }

  let stopRequested = false;
  async function runSuite() {
    stopRequested = false;
    const out: PerfResult[] = [];
    const steps: [string, TestSpec | (() => TestSpec)][] = [
      ["Latency", spec("latency", "upload", { durationSecs: 10, latencyPps: 20 })],
      ["TCP upload", spec("tcp", "upload")],
      ["TCP download", spec("tcp", "download")],
      ["TCP both directions", spec("tcp", "bidir")],
      // UDP at ~90% of the measured TCP rate shows loss/jitter near capacity.
      ["UDP upload", () => spec("udp", "upload", { streams: 1, bitrateMbps: udpRate(out, "upload") })],
      ["UDP download", () => spec("udp", "download", { streams: 1, bitrateMbps: udpRate(out, "download") })],
    ];
    for (const [name, s] of steps) {
      if (stopRequested) break;
      stage = name;
      const r = await runOne(typeof s === "function" ? s() : s);
      if (!r) break;
      out.push(r);
      suite = [...out];
      await new Promise((res) => setTimeout(res, 800));
    }
  }
  function udpRate(done: PerfResult[], dir: "upload" | "download") {
    const tcp = done.find((r) => r.spec.kind === "tcp" && r.spec.direction === dir)?.[dir]?.mbpsAvg ?? 100;
    return Math.max(1, Math.min(10000, Math.round(tcp * 0.9)));
  }

  async function stop() {
    stopRequested = true;
    await api.perfCancel();
  }

  onMount(async () => {
    server = await api.perfServerStatus().catch(() => null);
    if (server) mode = "server";
    unlistenServer = await onEvent<PerfServerEvent>("perf-server-event", (e) => {
      if (e.kind === "testStarted") {
        serverLive = { t: [], up: [], down: [] };
        log(`${e.host || e.peer} (${e.peer}) started ${e.spec.kind.toUpperCase()} ${e.spec.kind === "latency" ? "" : e.spec.direction} · ${e.spec.durationSecs}s${e.spec.kind === "tcp" ? ` · ${e.spec.streams} streams` : ""}`, "start");
      } else if (e.kind === "interval") {
        const i = serverLive.t.findIndex((t) => Math.abs(t - e.t) < 0.01);
        const n = { t: [...serverLive.t], up: [...serverLive.up], down: [...serverLive.down] };
        const k = i < 0 ? n.t.push(e.t) - 1 : i;
        if (i < 0) { n.up.push(NaN); n.down.push(NaN); }
        if (e.dir === "upload") n.up[k] = e.mbps; else n.down[k] = e.mbps;
        serverLive = n;
      } else if (e.kind === "testFinished") log(`Test from ${e.peer} finished: ${e.summary}`, "done");
      else if (e.kind === "rejected") log(`Rejected ${e.peer}: ${e.reason}`, "warn");
      else log(e.message, "warn");
    });
  });
  onDestroy(() => {
    unlistenServer?.();
    unlistenProgress?.();
  });

  // ------------------------------------------------------------- display ---
  const fmtRate = (m: number | null | undefined) =>
    m == null || !isFinite(m) ? "—" : m >= 1000 ? `${(m / 1000).toFixed(2)} Gbit/s` : `${m.toFixed(m >= 100 ? 0 : 1)} Mbit/s`;
  const fmtMs = (v: number | null | undefined, d = 2) => (v == null ? "—" : `${v.toFixed(d)} ms`);
  let latest = $derived(results[0] ?? null);
  let chartSeries = $derived.by(() => {
    const s = [];
    if (live.up.some((x) => isFinite(x))) s.push({ label: "Upload", color: "--series-1", values: live.up.map((x) => (isFinite(x) ? x : null)) });
    if (live.down.some((x) => isFinite(x))) s.push({ label: "Download", color: "--series-3", values: live.down.map((x) => (isFinite(x) ? x : null)) });
    return s;
  });
  let rttSeries = $derived(live.rtt.some((x) => x != null) ? [{ label: "Round trip", color: "--series-7", values: live.rtt }] : []);

  function describe(r: PerfResult) {
    const k = r.spec.kind;
    return k === "latency" ? "Latency" : `${k.toUpperCase()} ${r.spec.direction === "bidir" ? "both directions" : r.spec.direction}`;
  }

  function reportText(list: PerfResult[]): string {
    const lines = [`SysPulse network test report`, `Date: ${new Date().toLocaleString()}`, `From: ${app.summary?.hostname ?? "this PC"}  To: ${list[0]?.serverHost} (${list[0]?.server})`, ""];
    for (const r of list) {
      lines.push(`== ${describe(r)} (${r.spec.durationSecs}s${r.spec.kind === "tcp" ? `, ${r.spec.streams} streams` : ""}${r.spec.kind === "udp" ? `, target ${r.spec.bitrateMbps} Mbit/s, ${r.spec.packetSize} B packets` : ""}) ==`);
      for (const d of [r.upload, r.download].filter(Boolean) as DirectionResult[]) {
        lines.push(`  ${d.direction}: avg ${fmtRate(d.mbpsAvg)}  min ${fmtRate(d.mbpsMin)}  max ${fmtRate(d.mbpsMax)}  stddev ${d.mbpsStddev.toFixed(1)}  data ${bytes(d.bytes)} in ${d.seconds.toFixed(2)}s`);
        if (d.retransmits != null || d.rttMs != null) lines.push(`    TCP retransmits ${d.retransmits ?? "—"}  RTT ${fmtMs(d.rttMs)}`);
        if (d.lossPercent != null) lines.push(`    UDP sent ${d.packetsSent}  received ${d.packetsReceived}  lost ${d.lost} (${d.lossPercent.toFixed(2)}%)  jitter ${fmtMs(d.jitterMs, 3)}  out-of-order ${d.outOfOrder}`);
        lines.push(`    per interval (Mbit/s): ${d.intervals.map((i) => i.mbps.toFixed(1)).join(", ")}`);
      }
      if (r.latency) {
        const l = r.latency;
        lines.push(`  RTT min ${fmtMs(l.minMs, 3)}  avg ${fmtMs(l.avgMs, 3)}  p50 ${fmtMs(l.p50Ms, 3)}  p90 ${fmtMs(l.p90Ms, 3)}  p99 ${fmtMs(l.p99Ms, 3)}  max ${fmtMs(l.maxMs, 3)}`);
        lines.push(`  jitter ${fmtMs(l.jitterMs, 3)}  stddev ${fmtMs(l.stddevMs, 3)}  loss ${l.lossPercent.toFixed(2)}% (${l.received}/${l.sent})`);
      }
      lines.push("");
    }
    return lines.join("\n");
  }

  async function exportReport(list: PerfResult[]) {
    const stamp = new Date().toISOString().slice(0, 16).replace(/[:T]/g, "-");
    const path = await pickSaveFile(`syspulse-speedtest-${stamp}.txt`, "txt");
    if (!path) return;
    try {
      await api.exportText(path, reportText(list));
      app.notify(`Report saved to ${path}`);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function copy(text: string) {
    await navigator.clipboard.writeText(text).catch(() => {});
    app.notify(`Copied ${text}`, "info", 2000);
  }
</script>

<PageHeader title="Speed Test" subtitle="Measure throughput, packet loss, jitter and latency between two PCs running SysPulse">
  {#snippet actions()}
    <div class="seg">
      <button class:on={mode === "client"} onclick={() => (mode = "client")}><Icon name="play" size={13} /> Run test</button>
      <button class:on={mode === "server"} onclick={() => (mode = "server")}><Icon name="network" size={13} /> Server mode{server ? " ●" : ""}</button>
    </div>
  {/snippet}
</PageHeader>

{#if mode === "server"}
  <div class="srv">
    <Card title="Server mode" icon="network">
      {#if !server}
        <p class="secondary lead">Run this on the <b>other</b> PC. It waits for tests from SysPulse on this network.</p>
        <div class="formrow">
          <label>Port <input type="number" min="1024" max="65535" bind:value={serverPort} class="w90" /></label>
          <label class="chk"><input type="checkbox" bind:checked={requireCode} /> Require an access code</label>
          <button class="primary" onclick={startServer} disabled={!isTauri}><Icon name="play" size={14} /> Start server</button>
        </div>
        <p class="muted small"><Icon name="info" size={12} /> If tests cannot connect, allow the port in the firewall: <span class="mono">sudo ufw allow {serverPort}</span> (TCP and UDP).</p>
      {:else}
        <div class="ready">
          <span class="pulse"></span>
          <div>
            <div class="big">Ready — waiting for tests</div>
            <div class="muted">{server.host} · port {server.port}</div>
          </div>
          <span class="grow"></span>
          <button class="danger" onclick={stopServer}><Icon name="stop" size={14} /> Stop server</button>
        </div>
        <div class="addrs">
          <span class="muted small">Enter one of these on the other PC:</span>
          {#each server.addresses as a (a)}<button class="addr mono" onclick={() => copy(a)} title="Copy">{a}</button>{/each}
          {#if server.code}<span class="code">Access code <b class="mono">{server.code}</b></span>{/if}
        </div>
      {/if}
    </Card>
    {#if server}
      <Card title="Incoming test" icon="gpu">
        {#if serverLive.t.length}
          <LineChart elapsed time={serverLive.t} height={180} format={(v) => fmtRate(v)}
            series={[
              ...(serverLive.up.some((x) => isFinite(x)) ? [{ label: "Received (client upload)", color: "--series-1", values: serverLive.up.map((x) => (isFinite(x) ? x : null)) }] : []),
              ...(serverLive.down.some((x) => isFinite(x)) ? [{ label: "Sent (client download)", color: "--series-3", values: serverLive.down.map((x) => (isFinite(x) ? x : null)) }] : []),
            ]} />
        {:else}<p class="muted">No test running.</p>{/if}
      </Card>
    {/if}
    <Card title="Activity" icon="process">
      <ul class="log">
        {#each serverLog as l, i (i)}<li class={l.kind}><span class="muted mono">{l.at}</span> {l.text}</li>{:else}<li class="muted">Nothing yet.</li>{/each}
      </ul>
    </Card>
  </div>
{:else}
  <section class="target">
    <div class="tbox">
      <Icon name="network" size={16} />
      <input placeholder="Server IP address, e.g. 192.168.0.50" bind:value={cfg.target} class="mono" spellcheck="false" aria-label="Server address" />
    </div>
    <label>Port <input type="number" class="w90" bind:value={cfg.port} /></label>
    <label>Code <input class="w90 mono" placeholder="none" bind:value={cfg.code} /></label>
    <button onclick={discover} disabled={discovering}><Icon name="search" size={14} /> {discovering ? "Searching…" : "Find servers"}</button>
    {#each found as f (f.address)}
      <button class="chip" class:on={cfg.target === f.address} onclick={() => { cfg.target = f.address; cfg.port = f.port; }}>
        {f.host || f.address} <span class="muted mono">{f.address}</span>{#if f.codeRequired}<Icon name="settings" size={11} />{/if}
      </button>
    {/each}
  </section>

  <section class="kinds">
    {#each [["tcp", "TCP throughput", "Maximum speed with parallel streams, retransmits and RTT"], ["udp", "UDP", "Fixed bitrate: packet loss, jitter, reordering"], ["latency", "Latency", "Round-trip time distribution and jitter"], ["suite", "Full suite", "Latency + TCP up/down/both + UDP up/down, one report"]] as [id, title, desc] (id)}
      <button class="kind" class:on={cfg.kind === id} onclick={() => (cfg.kind = id as PerfKind | "suite")}>
        <b>{title}</b><span class="muted">{desc}</span>
      </button>
    {/each}
  </section>

  <section class="opts">
    {#if cfg.kind === "tcp" || cfg.kind === "udp"}
      <div class="seg small-seg">
        {#each [["upload", "Upload ↑"], ["download", "Download ↓"], ["bidir", "Both ⇅"]] as [v, l] (v)}
          <button class:on={cfg.direction === v} onclick={() => (cfg.direction = v as PerfDirection)}>{l}</button>
        {/each}
      </div>
    {/if}
    {#if cfg.kind === "tcp" || cfg.kind === "suite"}
      <label>Streams <select bind:value={cfg.streams}>{#each [1, 2, 4, 8, 16] as n (n)}<option value={n}>{n}</option>{/each}</select></label>
    {/if}
    {#if cfg.kind !== "suite"}
      <label>Duration <select bind:value={cfg.duration}>{#each [5, 10, 20, 30, 60, 120] as n (n)}<option value={n}>{n} s</option>{/each}</select></label>
    {:else}
      <label>Each test <select bind:value={cfg.duration}>{#each [5, 10, 20] as n (n)}<option value={n}>{n} s</option>{/each}</select></label>
    {/if}
    {#if cfg.kind === "udp"}
      <label>Bitrate <input type="number" min="0.1" max="10000" step="any" class="w90" bind:value={cfg.bitrate} /> Mbit/s</label>
      <label>Packet <input type="number" min="64" max="9000" class="w90" bind:value={cfg.packet} /> bytes</label>
    {/if}
    {#if cfg.kind === "latency"}
      <label>Probes <select bind:value={cfg.pps}>{#each [5, 10, 20, 50, 100] as n (n)}<option value={n}>{n}/s</option>{/each}</select></label>
    {/if}
    <label>Report every <select bind:value={cfg.interval}><option value={500}>0.5 s</option><option value={1000}>1 s</option><option value={2000}>2 s</option></select></label>
    <span class="grow"></span>
    {#if running}
      <span class="stage"><span class="spin"></span>{stage}</span>
      <button class="danger" onclick={stop}><Icon name="stop" size={14} /> Stop</button>
    {:else}
      <button class="primary go" onclick={start} disabled={!isTauri}><Icon name="play" size={14} /> Start test</button>
    {/if}
  </section>

  {#if running || live.t.length}
    <div class="livegrid">
      <Card>
        <div class="nums">
          {#if now.up != null}<Stat label="Upload" value={fmtRate(now.up)} />{/if}
          {#if now.down != null}<Stat label="Download" value={fmtRate(now.down)} />{/if}
          {#if now.rtt != null}<Stat label="Round trip" value={fmtMs(now.rtt)} />{/if}
          {#if now.up == null && now.down == null && now.rtt == null}<Stat label="Status" value="Connecting…" />{/if}
        </div>
      </Card>
      <Card title="Live" icon="network">
        {#if chartSeries.length}
          <LineChart elapsed time={live.t} series={chartSeries} height={170} format={(v) => fmtRate(v)} />
        {:else if rttSeries.length}
          <LineChart elapsed time={live.t} series={rttSeries} height={170} format={(v) => `${v.toFixed(1)} ms`} />
        {:else}<p class="muted">Waiting for the first interval…</p>{/if}
      </Card>
    </div>
  {/if}

  <div class="col">
  {#if suite && suite.length}
    <Card title="Full suite — {suite[0].serverHost} ({suite[0].server})" icon="dashboard">
      {#snippet actions()}{#if !running}<button class="small" onclick={() => exportReport(suite!)}>Export report</button>{/if}{/snippet}
      <table class="sum">
        <thead><tr><th>Test</th><th class="r">Upload</th><th class="r">Download</th><th class="r">Retransmits</th><th class="r">Loss</th><th class="r">Jitter</th><th class="r">Latency</th></tr></thead>
        <tbody>
          {#each suite as r (r.startedAt + describe(r))}
            <tr>
              <td>{describe(r)}</td>
              <td class="r num">{r.upload ? fmtRate(r.upload.mbpsAvg) : "—"}</td>
              <td class="r num">{r.download ? fmtRate(r.download.mbpsAvg) : "—"}</td>
              <td class="r num">{r.spec.kind === "tcp" ? (r.upload?.retransmits ?? 0) + (r.download?.retransmits ?? 0) : "—"}</td>
              <td class="r num">{r.spec.kind === "udp" ? `${Math.max(r.upload?.lossPercent ?? 0, r.download?.lossPercent ?? 0).toFixed(2)}%` : r.latency ? `${r.latency.lossPercent.toFixed(1)}%` : "—"}</td>
              <td class="r num">{r.spec.kind === "udp" ? fmtMs(r.upload?.jitterMs ?? r.download?.jitterMs, 3) : r.latency ? fmtMs(r.latency.jitterMs, 3) : "—"}</td>
              <td class="r num">{r.latency ? fmtMs(r.latency.avgMs) : r.upload?.rttMs != null ? fmtMs(r.upload.rttMs) : "—"}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </Card>
  {/if}

  {#if latest && !running}
    {@const r = latest}
    <Card title="Result — {describe(r)} · {r.serverHost} ({r.server}){r.cancelled ? ' · stopped early' : ''}" icon="check">
      {#snippet actions()}
        <button class="small" onclick={() => (showIntervals = !showIntervals)}>{showIntervals ? "Hide" : "Show"} intervals</button>
        <button class="small" onclick={() => exportReport([r])}>Export</button>
      {/snippet}
      <div class="dirs">
        {#each [r.upload, r.download].filter(Boolean) as d (d!.direction)}
          {@const x = d!}
          <section class="dir">
            <h4>{x.direction === "upload" ? "↑ Upload" : "↓ Download"} <span class="big num">{fmtRate(x.mbpsAvg)}</span></h4>
            <div class="grid">
              <Stat label="Min / max" value="{fmtRate(x.mbpsMin)} / {fmtRate(x.mbpsMax)}" />
              <Stat label="Std deviation" value="{x.mbpsStddev.toFixed(1)} Mbit/s" />
              <Stat label="Data" value="{bytes(x.bytes)} in {x.seconds.toFixed(1)} s" />
              {#if x.retransmits != null}<Stat label="TCP retransmits" value={String(x.retransmits)} />{/if}
              {#if x.rttMs != null}<Stat label="TCP round trip" value={fmtMs(x.rttMs)} />{/if}
              {#if x.lossPercent != null}<Stat label="Packet loss" value="{x.lossPercent.toFixed(2)}%" hint="{x.lost} of {x.packetsSent} packets" />{/if}
              {#if x.jitterMs != null}<Stat label="Jitter" value={fmtMs(x.jitterMs, 3)} />{/if}
              {#if x.outOfOrder != null}<Stat label="Out of order" value={String(x.outOfOrder)} />{/if}
            </div>
            {#if showIntervals}
              <table class="ivs">
                <thead><tr><th>Interval</th><th class="r">Rate</th><th class="r">Data</th>{#if x.lossPercent == null}<th class="r">Retr</th><th class="r">RTT</th>{:else}<th class="r">Packets</th><th class="r">Lost</th><th class="r">Jitter</th>{/if}</tr></thead>
                <tbody>
                  {#each x.intervals as iv (iv.index)}
                    <tr>
                      <td class="num">{(iv.t - r.spec.intervalMs / 1000).toFixed(1)}–{iv.t.toFixed(1)} s</td>
                      <td class="r num">{fmtRate(iv.mbps)}</td>
                      <td class="r num">{bytes(iv.bytes)}</td>
                      {#if x.lossPercent == null}<td class="r num">{iv.retransmits ?? "—"}</td><td class="r num">{fmtMs(iv.rttMs)}</td>
                      {:else}<td class="r num">{iv.packets ?? "—"}</td><td class="r num">{iv.lost ?? "—"}</td><td class="r num">{fmtMs(iv.jitterMs, 3)}</td>{/if}
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </section>
        {/each}
        {#if r.latency}
          {@const l = r.latency}
          <section class="dir">
            <h4>Round trip <span class="big num">{fmtMs(l.avgMs)}</span></h4>
            <div class="grid">
              <Stat label="Min / max" value="{fmtMs(l.minMs, 3)} / {fmtMs(l.maxMs, 3)}" />
              <Stat label="Median (p50)" value={fmtMs(l.p50Ms, 3)} />
              <Stat label="p90 / p99" value="{fmtMs(l.p90Ms, 2)} / {fmtMs(l.p99Ms, 2)}" />
              <Stat label="Jitter" value={fmtMs(l.jitterMs, 3)} />
              <Stat label="Std deviation" value={fmtMs(l.stddevMs, 3)} />
              <Stat label="Loss" value="{l.lossPercent.toFixed(2)}%" hint="{l.received} of {l.sent} replies" />
            </div>
            <LineChart elapsed time={l.samples.map((_, i) => i / r.spec.latencyPps)} height={140} format={(v) => `${v.toFixed(2)} ms`}
              series={[{ label: "RTT", color: "--series-7", values: l.samples }]} />
          </section>
        {/if}
      </div>
    </Card>
  {/if}

  {#if results.length > 1}
    <Card title="Earlier results (this session)" icon="process">
      <table class="sum">
        <tbody>
          {#each results.slice(1) as r (r.startedAt + describe(r))}
            <tr>
              <td class="muted">{new Date(r.startedAt * 1000).toLocaleTimeString()}</td>
              <td>{describe(r)}</td>
              <td class="muted">{r.serverHost}</td>
              <td class="r num">{r.upload ? `↑ ${fmtRate(r.upload.mbpsAvg)}` : ""}</td>
              <td class="r num">{r.download ? `↓ ${fmtRate(r.download.mbpsAvg)}` : ""}</td>
              <td class="r num">{r.latency ? fmtMs(r.latency.avgMs) : ""}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </Card>
  {/if}

  </div>
  {#if !isTauri}<p class="muted">Speed tests run in the desktop app.</p>{/if}
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 5px 12px; border-radius: 7px; color: var(--text-secondary); font-size: 13px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .small-seg button { padding: 4px 10px; }
  .grow { flex: 1; }
  .small { font-size: 12px; padding: 3px 9px; }
  .w90 { width: 90px; }
  label { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--text-secondary); }
  .chk input { accent-color: var(--accent); }
  .srv { display: flex; flex-direction: column; gap: 14px; }
  .lead { margin: 0 0 12px; }
  .formrow { display: flex; gap: 16px; align-items: center; flex-wrap: wrap; margin-bottom: 10px; }
  .ready { display: flex; align-items: center; gap: 14px; }
  .big { font-size: 18px; font-weight: 650; }
  .pulse { width: 14px; height: 14px; border-radius: 50%; background: var(--good); box-shadow: 0 0 0 4px color-mix(in srgb, var(--good) 25%, transparent); }
  .addrs { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin-top: 14px; }
  .addr { font-size: 15px; padding: 6px 12px; }
  .code { margin-left: 10px; font-size: 13px; }
  .code b { font-size: 18px; letter-spacing: 0.15em; color: var(--accent); }
  .log { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 5px; font-size: 12.5px; max-height: 260px; overflow: auto; }
  .log .start { color: var(--accent); }
  .log .done { color: var(--good); }
  .log .warn { color: var(--warning); }
  .target { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
  .tbox { flex: 1; min-width: 300px; display: flex; align-items: center; gap: 8px; background: var(--surface-1); border: 1px solid var(--border-strong); border-radius: 10px; padding: 0 12px; color: var(--text-muted); }
  .tbox:focus-within { border-color: var(--accent); }
  .tbox input { flex: 1; border: none; background: transparent; padding: 9px 0; font-size: 15px; }
  .chip { display: inline-flex; gap: 6px; align-items: center; font-size: 12.5px; padding: 4px 10px; border-radius: 99px; }
  .chip.on { border-color: var(--accent); }
  .kinds { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; margin-bottom: 12px; }
  .kind { display: flex; flex-direction: column; align-items: flex-start; gap: 4px; text-align: left; padding: 12px 14px; background: var(--surface-1); border: 1px solid var(--border); border-radius: 12px; }
  .kind.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, var(--surface-1)); }
  .kind .muted { font-size: 12px; line-height: 1.35; }
  .opts { display: flex; gap: 16px; align-items: center; flex-wrap: wrap; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 10px 14px; margin-bottom: 14px; }
  .go { padding: 8px 18px; font-size: 14px; }
  .stage { display: inline-flex; gap: 8px; align-items: center; font-size: 13px; color: var(--text-secondary); }
  .spin { width: 12px; height: 12px; border: 2px solid var(--surface-3); border-top-color: var(--accent); border-radius: 50%; animation: sp 0.8s linear infinite; }
  @keyframes sp { to { transform: rotate(360deg); } }
  .livegrid { display: grid; grid-template-columns: 240px 1fr; gap: 14px; margin-bottom: 14px; }
  .nums { display: flex; flex-direction: column; gap: 18px; }
  .nums :global(.value) { font-size: 26px; }
  .dirs { display: grid; grid-template-columns: repeat(auto-fit, minmax(420px, 1fr)); gap: 18px; }
  .dir h4 { display: flex; align-items: baseline; gap: 12px; margin: 0 0 12px; font-size: 13px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  .dir h4 .big { font-size: 26px; color: var(--text-primary); text-transform: none; letter-spacing: -0.01em; }
  .grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 14px 18px; margin-bottom: 12px; }
  table { width: 100%; border-collapse: collapse; font-size: 12.5px; }
  th { text-align: left; font-weight: 500; color: var(--text-muted); font-size: 11.5px; padding: 0 8px 6px 0; }
  td { padding: 5px 8px 5px 0; border-top: 1px solid var(--border); }
  .r { text-align: right; }
  .ivs { margin-top: 8px; }
  .col { display: flex; flex-direction: column; gap: 14px; }
</style>
