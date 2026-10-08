<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Gpu.svelte
  Author : Praneeth
  License: MIT
  Purpose: Per-GPU monitor (NVIDIA / AMD / Intel): utilisation, VRAM,
           temperature, power, clocks, video engines and GPU processes.
-->
<script lang="ts">
  import Card from "../lib/components/Card.svelte";
  import Gauge from "../lib/components/Gauge.svelte";
  import LineChart from "../lib/components/LineChart.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Stat from "../lib/components/Stat.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import { app } from "../lib/stores.svelte";
  import { bytes, mhz, pct, temp, watts } from "../lib/format";

  let gpus = $derived(app.latest?.gpus ?? []);
  let h = $derived(app.history);
  const VENDOR: Record<string, string> = { nvidia: "NVIDIA", amd: "AMD", intel: "Intel", other: "GPU" };
</script>

<PageHeader title="GPU" subtitle="{gpus.length} graphics adapter{gpus.length === 1 ? '' : 's'} detected" />

{#each gpus as g (g.id)}
  {@const gh = h.gpus[g.id]}
  {@const vramPct = g.vramUsed != null && g.vramTotal ? (g.vramUsed / g.vramTotal) * 100 : null}
  {@const sleeping = g.powerState === "suspended"}
  <section class="gpu">
    <div class="head">
      <div class="badge {g.vendor}">{VENDOR[g.vendor]}</div>
      <div class="title">
        <h2>{g.name}</h2>
        <div class="muted">{g.id} · {g.driver ?? "no driver"}{g.driverVersion ? ` ${g.driverVersion}` : ""}</div>
      </div>
      {#if sleeping}<span class="sleep"><Icon name="moon" size={14} /> Sleeping (runtime suspended)</span>{/if}
    </div>

    {#if sleeping}
      <Card>
        <p class="secondary note">
          This GPU is powered down by the kernel to save energy. SysPulse does not wake it for monitoring;
          metrics appear automatically when an application starts using it.
        </p>
      </Card>
    {:else}
      {@const hasGauges = g.utilization != null || vramPct != null}
      <div class="row" class:single={!hasGauges}>
        {#if hasGauges}
          <Card>
            <div class="gauges">
              <Gauge value={g.utilization} label="Load" color="var(--series-7)" size={140} />
              <Gauge value={vramPct} label="VRAM" color="var(--series-2)" size={140} detail={g.vramTotal ? `${bytes(g.vramUsed)} / ${bytes(g.vramTotal, 0)}` : "shared"} />
            </div>
          </Card>
        {/if}
        <Card title="Details" icon="info">
          <div class="stats">
            <Stat label="Temperature" value={temp(g.temperature)} />
            <Stat label="Power" value={watts(g.powerWatts)} hint={g.powerLimitWatts ? `limit ${watts(g.powerLimitWatts)}` : ""} />
            <Stat label="Core clock" value={mhz(g.coreClockMhz)} hint={g.maxCoreClockMhz ? `max ${mhz(g.maxCoreClockMhz)}` : ""} />
            <Stat label="Memory clock" value={mhz(g.memoryClockMhz)} />
            <Stat label="Fan" value={g.fanPercent != null ? `${g.fanPercent}%` : "—"} />
            <Stat label="PCIe link" value={g.pcieGeneration ? `Gen ${g.pcieGeneration} x${g.pcieWidth}` : "—"} />
            <Stat label="Video encode" value={pct(g.encoderUtilization)} />
            <Stat label="Video decode" value={pct(g.decoderUtilization)} />
            <Stat label="Memory controller" value={pct(g.memoryUtilization)} />
          </div>
        </Card>
      </div>

      {#if gh && (g.utilization != null || g.vramTotal != null)}
        <div class="row2">
          <Card title="Utilisation" icon="gpu">
            <LineChart
              time={h.time}
              series={[
                { label: "GPU load", color: "--series-7", values: gh.util },
                { label: "VRAM", color: "--series-2", values: gh.vram },
              ]}
              yMax={100}
              format={(v) => `${v.toFixed(0)}%`}
            />
          </Card>
          {#if g.powerWatts != null}
            <Card title="Power draw" icon="bolt">
              <LineChart time={h.time} series={[{ label: "Power", color: "--series-4", values: gh.power }]} format={(v) => `${v.toFixed(0)} W`} />
            </Card>
          {:else if g.temperature != null}
            <Card title="Temperature" icon="thermo">
              <LineChart time={h.time} series={[{ label: "Temperature", color: "--series-8", values: gh.temp }]} format={(v) => `${v.toFixed(0)}°`} />
            </Card>
          {/if}
        </div>
      {:else}
        <Card>
          <p class="secondary note">
            {#if g.vendor === "intel"}
              Intel integrated graphics only expose clock speeds without elevated permissions.
              Engine utilisation (via the i915 PMU) is planned with the privileged helper.
            {:else}
              Detailed metrics are not exposed for this adapter by its driver.
            {/if}
          </p>
        </Card>
      {/if}

      {#if g.processes.length}
        <Card title="Processes using this GPU" icon="process">
          <table>
            <thead><tr><th>Process</th><th>PID</th><th>Type</th><th class="r">GPU memory</th></tr></thead>
            <tbody>
              {#each g.processes as p (p.pid)}
                <tr><td>{p.name}</td><td class="num muted">{p.pid}</td><td class="muted">{p.kind}</td><td class="r num">{bytes(p.usedMemory)}</td></tr>
              {/each}
            </tbody>
          </table>
        </Card>
      {/if}
    {/if}
  </section>
{:else}
  <div class="muted">No GPUs detected.</div>
{/each}

<style>
  .gpu { margin-bottom: 28px; display: flex; flex-direction: column; gap: 14px; }
  .head { display: flex; align-items: center; gap: 12px; }
  .title { min-width: 0; }
  .title .muted { font-size: 12px; margin-top: 2px; }
  .badge { font-size: 11px; font-weight: 700; letter-spacing: 0.06em; padding: 6px 10px; border-radius: 8px; color: #fff; }
  .badge.nvidia { background: #5a8f00; }
  .badge.amd { background: #c4262e; }
  .badge.intel { background: #0068b5; }
  .badge.other { background: var(--surface-3); }
  .sleep { margin-left: auto; display: inline-flex; gap: 6px; align-items: center; font-size: 12px; color: var(--text-secondary);
    border: 1px solid var(--border-strong); border-radius: 99px; padding: 4px 10px; }
  .row { display: grid; grid-template-columns: auto 1fr; gap: 14px; }
  .row.single { grid-template-columns: 1fr; }
  .row2 { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; }
  .gauges { display: flex; gap: 16px; align-items: center; height: 100%; }
  .stats { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 16px 20px; }
  .note { margin: 0; font-size: 13px; line-height: 1.5; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; font-weight: 500; color: var(--text-muted); font-size: 11.5px; padding-bottom: 6px; }
  td { padding: 7px 0; border-top: 1px solid var(--border); }
  .r { text-align: right; }
</style>
