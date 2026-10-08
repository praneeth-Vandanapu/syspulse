<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Performance.svelte
  Author : Praneeth
  License: MIT
  Purpose: Detailed performance views: CPU (per core), memory, disks,
           network and temperature sensors.
-->
<script lang="ts">
  import Card from "../lib/components/Card.svelte";
  import LineChart from "../lib/components/LineChart.svelte";
  import Meter from "../lib/components/Meter.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Sparkline from "../lib/components/Sparkline.svelte";
  import Stat from "../lib/components/Stat.svelte";
  import { app } from "../lib/stores.svelte";
  import { api, isTauri, pickSaveFile } from "../lib/api";
  import { bytes, duration, mhz, pct, rate, temp } from "../lib/format";

  const TABS = [
    { id: "cpu", label: "CPU" },
    { id: "memory", label: "Memory" },
    { id: "disks", label: "Disks" },
    { id: "network", label: "Network" },
    { id: "sensors", label: "Sensors" },
  ];
  let tab = $state("cpu");

  /** Export the in-memory history (one row per sample) as CSV. */
  async function exportCsv() {
    const hh = app.history;
    const gpuIds = Object.keys(hh.gpus);
    const head = ["time", "cpu_percent", "cpu_temp_c", "memory_percent", "swap_percent", "net_rx_bps", "net_tx_bps", "disk_read_bps", "disk_write_bps",
      ...hh.perCore.map((_, i) => `cpu${i}_percent`), ...gpuIds.flatMap((g) => [`gpu_${g}_util`, `gpu_${g}_vram_percent`, `gpu_${g}_temp_c`, `gpu_${g}_power_w`])];
    const v = (x: number | null | undefined) => (x == null ? "" : String(Math.round(x * 100) / 100));
    const lines = hh.time.map((t, i) => [new Date(t * 1000).toISOString(), v(hh.cpu[i]), v(hh.cpuTemp[i]), v(hh.mem[i]), v(hh.swap[i]), v(hh.netRx[i]), v(hh.netTx[i]),
      v(hh.diskRead[i]), v(hh.diskWrite[i]), ...hh.perCore.map((c) => v(c[i])),
      ...gpuIds.flatMap((g) => [v(hh.gpus[g].util[i]), v(hh.gpus[g].vram[i]), v(hh.gpus[g].temp[i]), v(hh.gpus[g].power[i])])].join(","));
    const csv = [head.join(","), ...lines].join("\n") + "\n";
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    const path = await pickSaveFile(`syspulse-history-${stamp}.csv`, "csv");
    if (!path) return;
    try {
      await api.exportText(path, csv);
      app.notify(`Exported ${hh.time.length} samples to ${path}`);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  let t = $derived(app.latest);
  let h = $derived(app.history);
  let s = $derived(app.summary);
  let maxFreq = $derived(t ? Math.max(...t.cpu.frequencyMhz) : null);
  let avgFreq = $derived(t && t.cpu.frequencyMhz.length ? t.cpu.frequencyMhz.reduce((a, b) => a + b, 0) / t.cpu.frequencyMhz.length : null);
  let sensorsByChip = $derived.by(() => {
    const m = new Map<string, NonNullable<typeof t>["sensors"]>();
    for (const x of t?.sensors ?? []) {
      if (!m.has(x.chip)) m.set(x.chip, []);
      m.get(x.chip)!.push(x);
    }
    return [...m.entries()];
  });
</script>

<PageHeader title="Performance" subtitle="Live resource usage, refreshed every {app.settings.refreshMs / 1000} s">
  {#snippet actions()}
    {#if isTauri}<button onclick={exportCsv} title="Save the chart history as a spreadsheet">Export CSV</button>{/if}
    <div class="seg" role="tablist">
      {#each TABS as x (x.id)}
        <button role="tab" aria-selected={tab === x.id} class:on={tab === x.id} onclick={() => (tab = x.id)}>{x.label}</button>
      {/each}
    </div>
  {/snippet}
</PageHeader>

<div class="perf">
{#if !t}
  <div class="muted">Collecting first sample…</div>
{:else if tab === "cpu"}
  <div class="stats card-row">
    <Stat label="Utilisation" value={pct(t.cpu.total, 1)} />
    <Stat label="Avg / max clock" value={`${mhz(avgFreq)} · ${mhz(maxFreq)}`} />
    <Stat label="Temperature" value={temp(t.cpu.temperature)} />
    <Stat label="Load average" value={t.cpu.loadAverage.map((l) => l.toFixed(2)).join(" / ")} />
    <Stat label="Cores / threads" value={`${s?.physicalCores ?? "?"} / ${s?.logicalCores ?? t.cpu.perCore.length}`} />
    <Stat label="Uptime" value={duration(t.cpu.uptimeSecs)} />
  </div>
  <Card title="Total CPU" icon="cpu" subtitle={s?.cpuModel}>
    <LineChart time={h.time} series={[{ label: "CPU", color: "--series-1", values: h.cpu }]} yMax={100} height={200} format={(v) => `${v.toFixed(0)}%`} />
  </Card>
  <h2 class="sec">Logical processors</h2>
  <div class="cores">
    {#each t.cpu.perCore as v, i (i)}
      <div class="core">
        <div class="ch"><span class="muted">CPU {i}</span><b class="num">{v.toFixed(0)}%</b></div>
        <Sparkline values={h.perCore[i] ?? []} color="var(--series-1)" />
        <div class="muted small num">{mhz(t.cpu.frequencyMhz[i])}</div>
      </div>
    {/each}
  </div>
  {#if h.cpuTemp.some((x) => x != null)}
    <Card title="CPU temperature" icon="thermo">
      <LineChart time={h.time} series={[{ label: "Package", color: "--series-2", values: h.cpuTemp }]} format={(v) => `${v.toFixed(0)}°`} />
    </Card>
  {/if}
{:else if tab === "memory"}
  {@const m = t.memory}
  {@const appsMem = Math.max(0, m.used)}
  <div class="stats card-row">
    <Stat label="In use" value={bytes(m.used)} hint={pct((m.used / m.total) * 100, 1)} />
    <Stat label="Available" value={bytes(m.available)} />
    <Stat label="Cache / buffers" value={bytes(m.cached)} />
    <Stat label="Total" value={bytes(m.total)} />
    <Stat label="Swap used" value={`${bytes(m.swapUsed)} / ${bytes(m.swapTotal)}`} />
  </div>
  <Card title="Memory & swap" icon="memory">
    <LineChart
      time={h.time}
      height={220}
      series={[
        { label: "RAM", color: "--series-2", values: h.mem },
        { label: "Swap", color: "--series-5", values: h.swap },
      ]}
      yMax={100}
      format={(v) => `${v.toFixed(0)}%`}
    />
  </Card>
  <Card title="Composition" icon="memory">
    <div class="stack" role="img" aria-label="Memory composition">
      <span style:width="{(appsMem / m.total) * 100}%" style:background="var(--series-2)"></span>
      <span style:width="{(Math.min(m.cached, m.available) / m.total) * 100}%" style:background="var(--series-1)"></span>
    </div>
    <div class="legend">
      <span><i style:background="var(--series-2)"></i>Used {bytes(appsMem)}</span>
      <span><i style:background="var(--series-1)"></i>Cache (reclaimable) {bytes(m.cached)}</span>
      <span><i style:background="var(--surface-3)"></i>Free {bytes(Math.max(0, m.available - m.cached))}</span>
    </div>
  </Card>
{:else if tab === "disks"}
  <Card title="Disk throughput" icon="disk">
    <LineChart
      time={h.time}
      height={200}
      series={[
        { label: "Read", color: "--series-4", values: h.diskRead },
        { label: "Write", color: "--series-1", values: h.diskWrite },
      ]}
      format={(v) => rate(v)}
    />
  </Card>
  <div class="grid2">
    <Card title="Devices" icon="disk">
      <div class="list">
        {#each t.diskIo as d (d.device)}
          <div class="dev">
            <div class="dh"><b>{d.device}</b><span class="muted num">R {rate(d.readBps)} · W {rate(d.writeBps)}</span></div>
            <Meter value={d.busyPercent} label="Active time" right={pct(d.busyPercent)} color="var(--series-4)" height={6} />
          </div>
        {/each}
      </div>
    </Card>
    <Card title="Filesystems" icon="files">
      <div class="list">
        {#each t.filesystems as f (f.device)}
          {@const used = ((f.total - f.available) / f.total) * 100}
          <div class="dev">
            <Meter value={used} label={`${f.mountPoint}  ·  ${f.device} (${f.fsType})`} right={`${bytes(f.total - f.available)} / ${bytes(f.total)}`} color={used > 90 ? "var(--critical)" : "var(--series-4)"} />
          </div>
        {/each}
      </div>
    </Card>
  </div>
{:else if tab === "network"}
  <Card title="Network throughput" icon="network">
    <LineChart
      time={h.time}
      height={220}
      series={[
        { label: "Download", color: "--series-3", values: h.netRx },
        { label: "Upload", color: "--series-5", values: h.netTx },
      ]}
      format={(v) => rate(v)}
    />
  </Card>
  <Card title="Interfaces" icon="network">
    <table>
      <thead><tr><th>Interface</th><th class="r">Download</th><th class="r">Upload</th><th class="r">Received total</th><th class="r">Sent total</th></tr></thead>
      <tbody>
        {#each t.network as n (n.interface)}
          <tr><td>{n.interface}</td><td class="r num">{rate(n.rxBps)}</td><td class="r num">{rate(n.txBps)}</td><td class="r num">{bytes(n.rxTotal)}</td><td class="r num">{bytes(n.txTotal)}</td></tr>
        {/each}
      </tbody>
    </table>
  </Card>
{:else if tab === "sensors"}
  <div class="grid2">
    {#each sensorsByChip as [chip, list] (chip)}
      <Card title={chip} icon="thermo">
        <div class="list">
          {#each list as x (x.label)}
            {@const crit = x.critical ?? 100}
            {@const ratio = (x.temperature / crit) * 100}
            <Meter value={ratio} label={x.label} right={`${temp(x.temperature)}${x.critical ? ` / ${temp(x.critical)}` : ""}`} color={ratio > 90 ? "var(--critical)" : ratio > 75 ? "var(--warning)" : "var(--series-3)"} height={6} />
          {/each}
        </div>
      </Card>
    {:else}
      <div class="muted">No temperature sensors exposed by the kernel.</div>
    {/each}
  </div>
{/if}
</div>

<style>
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 5px 14px; border-radius: 7px; color: var(--text-secondary); font-size: 13px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2); }
  .card-row {
    display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 16px;
    background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 14px 18px; margin-bottom: 14px;
  }
  .perf > :global(.card + .card) { margin-top: 14px; }
  .sec { margin: 22px 0 10px; font-size: 13px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  .cores { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 10px; margin-bottom: 14px; }
  .core { background: var(--surface-1); border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; display: flex; flex-direction: column; gap: 6px; }
  .ch { display: flex; justify-content: space-between; font-size: 12.5px; }
  .small { font-size: 11px; }
  .grid2 { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; margin-top: 14px; }
  .list { display: flex; flex-direction: column; gap: 14px; }
  .dev { display: flex; flex-direction: column; gap: 6px; }
  .dh { display: flex; justify-content: space-between; font-size: 13px; }
  .stack { display: flex; height: 18px; border-radius: 6px; overflow: hidden; background: var(--surface-3); gap: 2px; }
  .stack span { height: 100%; }
  .legend { display: flex; gap: 18px; margin-top: 10px; font-size: 12.5px; color: var(--text-secondary); flex-wrap: wrap; }
  .legend i { display: inline-block; width: 10px; height: 10px; border-radius: 3px; margin-right: 6px; vertical-align: -1px; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; font-weight: 500; color: var(--text-muted); font-size: 11.5px; padding-bottom: 6px; }
  td { padding: 7px 0; border-top: 1px solid var(--border); }
  .r { text-align: right; }
</style>
