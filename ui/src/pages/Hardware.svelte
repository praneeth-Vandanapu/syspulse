<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Hardware.svelte
  Author : Praneeth
  License: MIT
  Purpose: Hardware inventory browser: every onboard device with its
           configuration, driver and version numbers. Exportable as text.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api } from "../lib/api";
  import type { HwSection } from "../lib/types";

  const ICONS: Record<string, string> = {
    system: "board", cpu: "cpu", memory: "memory", graphics: "gpu", storage: "disk", network: "network",
    audio: "audio", battery: "battery", usb: "usb", pci: "chip",
  };

  let sections = $state<HwSection[]>([]);
  let active = $state("system");
  let query = $state("");
  let loading = $state(true);
  let error = $state("");
  let copied = $state(false);

  async function load(refresh = false) {
    loading = true;
    error = "";
    try {
      sections = await api.hardwareInventory(refresh);
      if (!sections.some((s) => s.id === active) && sections.length) active = sections[0].id;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  onMount(() => load());

  let q = $derived(query.trim().toLowerCase());
  let visible = $derived.by(() => {
    if (!q) return sections.filter((s) => s.id === active);
    // Searching spans every section.
    return sections
      .map((s) => ({
        ...s,
        devices: s.devices.filter((d) =>
          [d.title, d.subtitle ?? "", ...d.properties.flatMap((p) => [p.key, p.value])].some((x) => x.toLowerCase().includes(q)),
        ),
      }))
      .filter((s) => s.devices.length);
  });

  function report(): string {
    const lines = [`SysPulse hardware report — ${new Date().toLocaleString()}`, ""];
    for (const s of sections) {
      lines.push(`== ${s.title} ==`);
      for (const d of s.devices) {
        lines.push(`* ${d.title}${d.subtitle ? ` (${d.subtitle})` : ""}`);
        for (const p of d.properties) lines.push(`    ${p.key}: ${p.value}`);
      }
      lines.push("");
    }
    return lines.join("\n");
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(report());
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch (e) {
      error = `Clipboard unavailable: ${e}`;
    }
  }
</script>

<PageHeader title="Hardware" subtitle="Onboard devices, configuration, drivers and firmware versions">
  {#snippet actions()}
    <div class="search">
      <Icon name="search" size={15} />
      <input placeholder="Search all hardware…" bind:value={query} aria-label="Search hardware" />
    </div>
    <button onclick={copy}><Icon name={copied ? "check" : "files"} size={14} /> {copied ? "Copied" : "Copy report"}</button>
    <button onclick={() => load(true)} disabled={loading}><Icon name="refresh" size={14} /> Rescan</button>
  {/snippet}
</PageHeader>

{#if error}<div class="err">{error}</div>{/if}

<div class="layout">
  <nav aria-label="Hardware sections">
    {#each sections as s (s.id)}
      <button class:on={!q && active === s.id} onclick={() => ((active = s.id), (query = ""))}>
        <Icon name={ICONS[s.id] ?? "chip"} size={16} />
        <span>{s.title}</span>
        <span class="count">{s.devices.length}</span>
      </button>
    {/each}
  </nav>

  <div class="content">
    {#if loading && !sections.length}
      <div class="muted">Scanning hardware…</div>
    {/if}
    {#each visible as s (s.id)}
      {#if q}<h2 class="sec">{s.title}</h2>{/if}
      <div class="devices">
        {#each s.devices as d, i (i)}
          <article class="dev">
            <header>
              <div class="ic"><Icon name={ICONS[s.id] ?? "chip"} size={18} /></div>
              <div class="tt">
                <h3>{d.title}</h3>
                {#if d.subtitle}<div class="muted">{d.subtitle}</div>{/if}
              </div>
            </header>
            <dl>
              {#each d.properties as p (p.key)}
                <dt>{p.key}</dt>
                <dd class:mono={/version|firmware|id|address|microcode|revision|bios/i.test(p.key)}>{p.value}</dd>
              {/each}
            </dl>
          </article>
        {/each}
      </div>
    {:else}
      {#if q}<div class="muted">Nothing matches “{query}”.</div>{/if}
    {/each}
  </div>
</div>

<style>
  .search { display: flex; align-items: center; gap: 6px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: 8px; padding: 0 10px; color: var(--text-muted); }
  .search input { border: none; background: transparent; width: 220px; padding: 7px 0; }
  button :global(svg) { vertical-align: -2px; }
  .err { color: var(--critical); margin-bottom: 12px; }
  .layout { display: grid; grid-template-columns: 210px 1fr; gap: 18px; align-items: start; }
  nav { display: flex; flex-direction: column; gap: 2px; position: sticky; top: 0; }
  nav button { display: flex; align-items: center; gap: 10px; background: transparent; border: 1px solid transparent; color: var(--text-secondary); padding: 8px 10px; border-radius: 9px; text-align: left; }
  nav button:hover { background: var(--surface-2); }
  nav button.on { background: var(--surface-2); border-color: var(--border-strong); color: var(--text-primary); }
  .count { margin-left: auto; font-size: 11px; color: var(--text-muted); }
  .sec { margin: 6px 0 10px; font-size: 13px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  .devices { display: grid; grid-template-columns: repeat(auto-fill, minmax(400px, 1fr)); gap: 14px; margin-bottom: 18px; }
  .dev { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 16px 18px; box-shadow: var(--shadow); min-width: 0; }
  .dev header { display: flex; gap: 12px; align-items: flex-start; margin-bottom: 12px; }
  .ic { width: 34px; height: 34px; border-radius: 9px; display: grid; place-items: center; flex: none; color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent); }
  .tt { min-width: 0; }
  .tt h3 { font-size: 14.5px; word-break: break-word; }
  .tt .muted { font-size: 12px; margin-top: 2px; }
  dl { display: grid; grid-template-columns: minmax(110px, 38%) 1fr; gap: 6px 12px; margin: 0; font-size: 12.5px; }
  dt { color: var(--text-muted); }
  dd { margin: 0; word-break: break-word; }
  dd.mono { font-size: 12px; }
</style>
