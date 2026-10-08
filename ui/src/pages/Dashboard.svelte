<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Dashboard.svelte
  Author : Praneeth
  License: MIT
  Purpose: At-a-glance overview: headline gauges, live history charts,
           temperatures, storage and the busiest processes.
-->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Card from "../lib/components/Card.svelte";
  import Gauge from "../lib/components/Gauge.svelte";
  import LineChart from "../lib/components/LineChart.svelte";
  import Meter from "../lib/components/Meter.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Stat from "../lib/components/Stat.svelte";
  import Status from "../lib/components/Status.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import { bytes, duration, level, mhz, pct, rate, temp, watts } from "../lib/format";
  import type { ProcessInfo } from "../lib/types";

  let t = $derived(app.latest);
  let h = $derived(app.history);
  let s = $derived(app.summary);

  let gpu = $derived(t?.gpus.find((g) => g.utilization != null || g.powerState === "suspended") ?? t?.gpus[0] ?? null);
  let root = $derived(t?.filesystems.find((f) => f.mountPoint === "/") ?? t?.filesystems[0] ?? null);
  let rootUsed = $derived(root ? ((root.total - root.available) / root.total) * 100 : null);
  let memPct = $derived(t ? (t.memory.used / t.memory.total) * 100 : null);
  let avgFreq = $derived(t && t.cpu.frequencyMhz.length ? t.cpu.frequencyMhz.reduce((a, b) => a + b, 0) / t.cpu.frequencyMhz.length : null);
  let netRx = $derived(h.netRx[h.netRx.length - 1] ?? 0);
  let netTx = $derived(h.netTx[h.netTx.length - 1] ?? 0);
  let hottest = $derived(t ? [...t.sensors].sort((a, b) => b.temperature - a.temperature).slice(0, 6) : []);

  let top = $state<ProcessInfo[]>([]);
  let timer: ReturnType<typeof setInterval>;
  async function loadTop() {
    try {
      const list = await api.listProcesses();
      top = list.sort((a, b) => b.cpu - a.cpu).slice(0, 6);
    } catch (e) {
      console.error(e);
    }
  }
  onMount(() => {
    loadTop();
    timer = setInterval(loadTop, 3000);
  });
  onDestroy(() => clearInterval(timer));
</script>

<PageHeader title="Dashboard" subtitle={s ? `${s.cpuModel} · ${bytes(s.totalMemory, 0)} RAM · Kernel ${s.kernel}` : ""} />

{#if !t}
  <div class="muted">Collecting first sample…</div>
{:else}
  <div class="gauges">
    <Card>
      <div class="g">
        <Gauge value={t.cpu.total} label="CPU" color="var(--series-1)" detail={mhz(avgFreq)} />
        <div class="facts">
          <Stat label="Temperature" value={temp(t.cpu.temperature)} />
          <Stat label="Load (1/5/15)" value={t.cpu.loadAverage.map((l) => l.toFixed(2)).join(" · ")} />
          <Stat label="Processes" value={String(t.cpu.processCount)} />
        </div>
      </div>
    </Card>
    <Card>
      <div class="g">
        <Gauge value={memPct} label="Memory" color="var(--series-2)" detail={`${bytes(t.memory.used)} / ${bytes(t.memory.total)}`} />
        <div class="facts">
          <Stat label="Available" value={bytes(t.memory.available)} />
          <Stat label="Cache" value={bytes(t.memory.cached)} />
          <Stat label="Swap" value={`${bytes(t.memory.swapUsed)} / ${bytes(t.memory.swapTotal)}`} />
        </div>
      </div>
    </Card>
    <Card>
      <div class="g">
        <Gauge value={gpu?.utilization ?? null} label="GPU" color="var(--series-7)" detail={gpu ? gpu.name.replace(/^NVIDIA (GeForce )?/, "") : "No GPU"} />
        <div class="facts">
          {#if gpu?.powerState === "suspended"}
            <Stat label="State" value="Sleeping" hint="dGPU powered down to save energy" />
          {:else}
            <Stat label="VRAM" value={gpu?.vramTotal ? `${bytes(gpu.vramUsed)} / ${bytes(gpu.vramTotal, 0)}` : "—"} />
            <Stat label="Temperature" value={temp(gpu?.temperature)} />
            <Stat label="Power" value={watts(gpu?.powerWatts)} />
          {/if}
        </div>
      </div>
    </Card>
    <Card>
      <div class="g">
        <Gauge value={rootUsed} label="Disk /" color="var(--series-4)" detail={root ? `${bytes(root.available)} free` : ""} />
        <div class="facts">
          <Stat label="Read" value={rate(h.diskRead[h.diskRead.length - 1])} />
          <Stat label="Write" value={rate(h.diskWrite[h.diskWrite.length - 1])} />
          <Stat label="Network ↓ / ↑" value={`${rate(netRx)} · ${rate(netTx)}`} />
        </div>
      </div>
    </Card>
  </div>

  <div class="grid2">
    <Card title="CPU usage" icon="cpu" subtitle="last {h.time.length} s">
      <LineChart time={h.time} series={[{ label: "CPU", color: "--series-1", values: h.cpu }]} yMax={100} format={(v) => `${v.toFixed(0)}%`} />
    </Card>
    <Card title="Memory" icon="memory">
      <LineChart
        time={h.time}
        series={[
          { label: "RAM", color: "--series-2", values: h.mem },
          { label: "Swap", color: "--series-5", values: h.swap },
        ]}
        yMax={100}
        format={(v) => `${v.toFixed(0)}%`}
      />
    </Card>
    <Card title="Network" icon="network">
      <LineChart
        time={h.time}
        series={[
          { label: "Download", color: "--series-3", values: h.netRx },
          { label: "Upload", color: "--series-5", values: h.netTx },
        ]}
        format={(v) => rate(v)}
      />
    </Card>
    <Card title="Disk I/O" icon="disk">
      <LineChart
        time={h.time}
        series={[
          { label: "Read", color: "--series-4", values: h.diskRead },
          { label: "Write", color: "--series-1", values: h.diskWrite },
        ]}
        format={(v) => rate(v)}
      />
    </Card>
  </div>

  <div class="grid3">
    <Card title="Top processes" icon="process">
      {#snippet actions()}<button class="ghost small" onclick={() => (app.page = "processes")}>View all →</button>{/snippet}
      <table>
        <thead><tr><th>Name</th><th class="r">CPU</th><th class="r">Memory</th></tr></thead>
        <tbody>
          {#each top as p (p.pid)}
            <tr>
              <td><span class="pname">{p.name}</span> <span class="muted">{p.pid}</span></td>
              <td class="r num">{pct(p.cpu, 1)}</td>
              <td class="r num">{bytes(p.memory)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </Card>

    <Card title="Temperatures" icon="thermo">
      <div class="list">
        {#each hottest as sn (sn.chip + sn.label)}
          {@const crit = sn.critical ?? 100}
          {@const lv = level((sn.temperature / crit) * 100, 75, 90)}
          <div class="trow">
            <Meter value={(sn.temperature / crit) * 100} label={`${sn.chip} · ${sn.label}`} right={temp(sn.temperature)} color={lv === "good" ? "var(--series-3)" : lv === "warning" ? "var(--warning)" : "var(--critical)"} height={6} />
            {#if lv !== "good"}<Status level={lv} text={lv === "warning" ? "Hot" : "Critical"} />{/if}
          </div>
        {:else}
          <div class="muted">No temperature sensors found.</div>
        {/each}
      </div>
    </Card>

    <Card title="Storage & power" icon="disk">
      <div class="list">
        {#each t.filesystems as f (f.device)}
          {@const used = ((f.total - f.available) / f.total) * 100}
          <Meter value={used} label={f.mountPoint} right={`${bytes(f.total - f.available)} / ${bytes(f.total, 0)}`} color={used > 90 ? "var(--critical)" : "var(--series-4)"} height={6} />
        {/each}
        {#if t.battery}
          <Meter value={t.battery.percent} label={`Battery · ${t.battery.status}`} right={`${t.battery.percent.toFixed(0)}%${t.battery.powerWatts ? ` · ${watts(t.battery.powerWatts)}` : ""}`} color="var(--series-6)" height={6} />
        {/if}
        <div class="muted small">Uptime {duration(t.cpu.uptimeSecs)}</div>
      </div>
    </Card>
  </div>
{/if}

<style>
  .gauges { display: grid; grid-template-columns: repeat(auto-fit, minmax(310px, 1fr)); gap: 14px; margin-bottom: 14px; }
  .g { display: flex; align-items: center; gap: 14px; }
  .facts { display: flex; flex-direction: column; gap: 10px; min-width: 0; flex: 1; }
  .grid2 { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; margin-bottom: 14px; }
  .grid3 { display: grid; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); gap: 14px; }
  .list { display: flex; flex-direction: column; gap: 12px; }
  .trow { display: flex; align-items: flex-end; gap: 8px; }
  .trow :global(.meter) { flex: 1; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; font-weight: 500; color: var(--text-muted); font-size: 11.5px; padding: 0 0 6px; }
  td { padding: 6px 0; border-top: 1px solid var(--border); }
  .r { text-align: right; }
  .pname { font-weight: 500; }
  .small { font-size: 12px; }
  button.small { font-size: 12px; padding: 2px 8px; color: var(--accent); }
</style>
