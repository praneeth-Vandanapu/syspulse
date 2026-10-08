<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/NetTools.svelte
  Author : Praneeth
  License: MIT
  Purpose: Network diagnostics: ping (live RTT chart), traceroute, DNS lookup
           and TCP port check.
-->
<script lang="ts">
  import { onDestroy } from "svelte";
  import Card from "../lib/components/Card.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import LineChart from "../lib/components/LineChart.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Stat from "../lib/components/Stat.svelte";
  import { api, onEvent } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import type { DnsResult, Hop, PingSummary, PortResult } from "../lib/types";

  type Tool = "ping" | "trace" | "dns" | "ports";
  const PRESETS: Record<string, string> = {
    Common: "21,22,23,25,53,80,110,143,443,445,3306,3389,5432,5900,8080",
    Web: "80,443,8000,8080,8443,3000,5000",
    Remote: "22,23,3389,5900,7070",
    "Speed test": "5201",
  };

  let tool = $state<Tool>("ping");
  let host = $state("8.8.8.8");
  let count = $state(10);
  let interval = $state(500);
  let ports = $state(PRESETS.Common);
  let running = $state(false);
  let lines = $state<string[]>([]);
  let rtts = $state<(number | null)[]>([]);
  let ping = $state<PingSummary | null>(null);
  let hops = $state<Hop[] | null>(null);
  let dns = $state<DnsResult | null>(null);
  let portRes = $state<PortResult[] | null>(null);
  let unlisten: (() => void) | null = null;
  onDestroy(() => unlisten?.());

  async function run() {
    const h = host.trim();
    if (!h) return;
    running = true;
    lines = [];
    unlisten?.();
    unlisten = await onEvent<string>("nettool-line", (l) => {
      lines = [...lines.slice(-400), l];
      const m = /time=([\d.]+)/.exec(l);
      if (m) rtts = [...rtts, Number(m[1])];
      else if (l.startsWith("no answer yet")) rtts = [...rtts, null];
    });
    try {
      if (tool === "ping") {
        rtts = [];
        ping = null;
        ping = await api.netPing(h, count, interval);
        rtts = ping.rtts;
      } else if (tool === "trace") {
        hops = null;
        hops = await api.netTrace(h);
      } else if (tool === "dns") {
        dns = await api.netLookup(h);
      } else {
        const list = [...new Set(ports.split(/[,\s]+/).map(Number).filter((p) => p > 0 && p < 65536))].slice(0, 64);
        portRes = await api.netPorts(h, list, 1500);
        portRes.sort((a, b) => a.port - b.port);
      }
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      running = false;
      unlisten?.();
      unlisten = null;
    }
  }

  let maxHop = $derived(Math.max(1, ...(hops ?? []).map((h) => h.rttMs ?? 0)));
  const fmt = (v: number | null | undefined, d = 1) => (v == null ? "—" : `${v.toFixed(d)} ms`);
</script>

<PageHeader title="Net Tools" subtitle="Ping, trace the route, look up names and check open ports">
  {#snippet actions()}
    <div class="seg">
      {#each [["ping", "Ping"], ["trace", "Traceroute"], ["dns", "DNS lookup"], ["ports", "Port check"]] as [id, l] (id)}
        <button class:on={tool === id} onclick={() => (tool = id as Tool)}>{l}</button>
      {/each}
    </div>
  {/snippet}
</PageHeader>

<section class="bar">
  <div class="hbox">
    <Icon name="network" size={16} />
    <input class="mono" bind:value={host} placeholder="Host name or IP address" spellcheck="false" onkeydown={(e) => e.key === "Enter" && !running && run()} aria-label="Host" />
  </div>
  {#if tool === "ping"}
    <label>Count <select bind:value={count}>{#each [4, 10, 20, 50, 100] as n (n)}<option value={n}>{n}</option>{/each}</select></label>
    <label>Every <select bind:value={interval}><option value={200}>0.2 s</option><option value={500}>0.5 s</option><option value={1000}>1 s</option></select></label>
  {/if}
  {#if running}
    <button class="danger" onclick={() => api.netToolCancel()}><Icon name="stop" size={14} /> Stop</button>
  {:else}
    <button class="primary" onclick={run}><Icon name="play" size={14} /> Run</button>
  {/if}
</section>
{#if tool === "ports"}
  <section class="bar">
    <label class="grow">Ports <input class="mono wide" bind:value={ports} /></label>
    {#each Object.entries(PRESETS) as [name, list] (name)}<button class="chip" onclick={() => (ports = list)}>{name}</button>{/each}
  </section>
{/if}

<div class="col">
  {#if tool === "ping" && (rtts.length || ping)}
    <Card title="Ping {ping?.address ? `${host} (${ping.address})` : host}" icon="network">
      {#if ping}
        <div class="stats">
          <Stat label="Average" value={fmt(ping.avgMs, 2)} />
          <Stat label="Min / max" value="{fmt(ping.minMs, 2)} / {fmt(ping.maxMs, 2)}" />
          <Stat label="Jitter (mdev)" value={fmt(ping.mdevMs, 2)} />
          <Stat label="Packet loss" value="{ping.lossPercent}%" hint="{ping.received} of {ping.sent} replies" />
        </div>
      {/if}
      <LineChart elapsed time={rtts.map((_, i) => (i * interval) / 1000)} height={160} format={(v) => `${v.toFixed(1)} ms`}
        series={[{ label: "Round trip", color: "--series-7", values: rtts }]} />
    </Card>
  {/if}

  {#if tool === "trace" && hops}
    <Card title="Route to {host}" icon="network">
      <table>
        <thead><tr><th>Hop</th><th>Address</th><th>Round trip</th><th></th></tr></thead>
        <tbody>
          {#each hops as h (h.ttl)}
            <tr>
              <td class="num">{h.ttl}</td>
              <td class="mono">{h.host ?? "* (no reply)"}</td>
              <td class="num">{fmt(h.rttMs, 2)}</td>
              <td class="barcell">{#if h.rttMs != null}<span style:width="{(h.rttMs / maxHop) * 100}%"></span>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </Card>
  {/if}

  {#if tool === "dns" && dns}
    <Card title="DNS — {dns.host}" icon="search">
      <div class="stats">
        <Stat label="Answered in" value={fmt(dns.elapsedMs, 1)} />
        <Stat label="DNS servers" value={dns.servers.join(", ") || "—"} />
      </div>
      <ul class="addrs mono">{#each dns.addresses as a (a)}<li>{a} <span class="muted">{a.includes(":") ? "IPv6" : "IPv4"}</span></li>{/each}</ul>
    </Card>
  {/if}

  {#if tool === "ports" && portRes}
    <Card title="Ports on {host}" icon="network">
      <div class="ports">
        {#each portRes as p (p.port)}
          <div class="port {p.state}"><b class="num">{p.port}</b><span>{p.state}</span><span class="muted num">{p.elapsedMs.toFixed(0)} ms</span></div>
        {/each}
      </div>
      <p class="muted small">Open = accepting connections · Closed = host answered but nothing listens · Filtered = no answer (firewall)</p>
    </Card>
  {/if}

  {#if (tool === "ping" || tool === "trace") && lines.length}
    <Card title="Output" icon="process"><pre class="out mono">{lines.join("\n")}</pre></Card>
  {/if}
</div>

<style>
  button :global(svg) { vertical-align: -2px; }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 5px 12px; border-radius: 7px; color: var(--text-secondary); font-size: 13px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .bar { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
  .hbox { flex: 1; min-width: 280px; display: flex; align-items: center; gap: 8px; background: var(--surface-1); border: 1px solid var(--border-strong); border-radius: 10px; padding: 0 12px; color: var(--text-muted); }
  .hbox:focus-within { border-color: var(--accent); }
  .hbox input { flex: 1; border: none; background: transparent; padding: 9px 0; font-size: 15px; }
  label { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--text-secondary); }
  .grow { flex: 1; }
  .wide { flex: 1; }
  .chip { font-size: 12.5px; padding: 3px 10px; border-radius: 99px; }
  .col { display: flex; flex-direction: column; gap: 14px; }
  .stats { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 14px; margin-bottom: 12px; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; font-weight: 500; color: var(--text-muted); font-size: 11.5px; padding-bottom: 6px; }
  td { padding: 6px 10px 6px 0; border-top: 1px solid var(--border); }
  .barcell { width: 40%; }
  .barcell span { display: block; height: 6px; border-radius: 6px; background: var(--series-7); }
  .addrs { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 6px; font-size: 14px; }
  .ports { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 8px; margin-bottom: 10px; }
  .port { display: flex; flex-direction: column; gap: 2px; padding: 8px 10px; border-radius: 8px; border: 1px solid var(--border); font-size: 12px; }
  .port b { font-size: 16px; }
  .port.open { border-color: var(--good); color: var(--good); }
  .port.open .muted { color: var(--text-muted); }
  .port.closed span:nth-child(2) { color: var(--text-secondary); }
  .port.filtered span:nth-child(2) { color: var(--warning); }
  .small { font-size: 12px; margin: 0; }
  .out { margin: 0; font-size: 11.5px; line-height: 1.5; max-height: 280px; overflow: auto; white-space: pre-wrap; color: var(--text-secondary); }
</style>
