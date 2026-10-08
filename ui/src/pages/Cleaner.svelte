<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Cleaner.svelte
  Author : Praneeth
  License: MIT
  Purpose: Cache cleaner: shows reclaimable space per location (system, user,
           browsers, developer tools), lets the user pick items and cleans
           them (system items via one admin authentication).
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import { bytes } from "../lib/format";
  import type { CleanCategory, CleanTarget } from "../lib/types";

  const GROUPS: { id: CleanCategory; title: string; icon: string; color: string }[] = [
    { id: "system", title: "System", icon: "chip", color: "var(--series-1)" },
    { id: "user", title: "Your files", icon: "files", color: "var(--series-3)" },
    { id: "browser", title: "Browsers", icon: "network", color: "var(--series-2)" },
    { id: "developer", title: "Developer tools", icon: "cpu", color: "var(--series-7)" },
  ];

  let targets = $state<CleanTarget[]>([]);
  let picked = $state<Record<string, boolean>>({});
  let open = $state<Record<string, boolean>>({});
  let scanning = $state(true);
  let confirming = $state(false);
  let cleaning = $state(false);

  async function scan() {
    scanning = true;
    try {
      targets = await api.cleanerScan();
      picked = Object.fromEntries(targets.map((t) => [t.id, t.selectedByDefault && t.size > 0]));
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      scanning = false;
    }
  }
  onMount(scan);

  let chosen = $derived(targets.filter((t) => picked[t.id] && t.size > 0));
  let chosenBytes = $derived(chosen.reduce((a, t) => a + t.size, 0));
  let totalBytes = $derived(targets.reduce((a, t) => a + t.size, 0));
  let needsAdmin = $derived(chosen.some((t) => t.needsAdmin));
  let byGroup = $derived(
    GROUPS.map((g) => {
      const items = targets.filter((t) => t.category === g.id).sort((a, b) => b.size - a.size);
      return { ...g, items, total: items.reduce((a, t) => a + t.size, 0), selected: items.filter((t) => picked[t.id]).reduce((a, t) => a + t.size, 0) };
    }),
  );

  function toggleGroup(id: CleanCategory, value: boolean) {
    for (const t of targets) if (t.category === id && t.size > 0) picked[t.id] = value;
  }

  async function clean() {
    cleaning = true;
    try {
      const r = await api.cleanerClean(chosen.map((t) => t.id));
      if (r.freedBytes > 0 || r.cleaned.length) app.notify(`Freed ${bytes(r.freedBytes)} from ${r.cleaned.length} location(s)`);
      for (const e of r.errors) app.notify(e, "error", 8000);
      confirming = false;
      await scan();
    } catch (e) {
      app.notify(String(e), "error", 8000);
    } finally {
      cleaning = false;
    }
  }
</script>

<PageHeader title="Cache Cleaner" subtitle={scanning ? "Measuring cache locations…" : `${bytes(totalBytes)} found across ${targets.filter((t) => t.size > 0).length} locations`}>
  {#snippet actions()}
    <button onclick={scan} disabled={scanning || cleaning}><Icon name="refresh" size={14} /> Rescan</button>
  {/snippet}
</PageHeader>

<section class="hero">
  <div class="sum">
    <div class="muted lbl">Selected to clean</div>
    <div class="big num">{bytes(chosenBytes)}</div>
    <div class="muted">{chosen.length} item{chosen.length === 1 ? "" : "s"}{needsAdmin ? " · system items need your password" : ""}</div>
  </div>
  <div class="bars">
    <div class="stack" role="img" aria-label="Reclaimable space by group">
      {#each byGroup as g (g.id)}
        {#if g.total > 0}<span style:flex-grow={g.total} style:background={g.color} title="{g.title}: {bytes(g.total)}"></span>{/if}
      {/each}
    </div>
    <div class="legend">
      {#each byGroup as g (g.id)}
        <span><i style:background={g.color}></i>{g.title} <b class="num">{bytes(g.total)}</b></span>
      {/each}
    </div>
  </div>
  <button class="primary go" disabled={!chosen.length || scanning || cleaning} onclick={() => (confirming = true)}>
    <Icon name="broom" size={15} /> Clean selected
  </button>
</section>

{#if scanning && !targets.length}
  <div class="muted">Scanning…</div>
{/if}

<div class="groups">
  {#each byGroup as g (g.id)}
    {#if g.items.length}
      <section class="group">
        <header>
          <span class="gi" style:color={g.color}><Icon name={g.icon} size={16} /></span>
          <h3>{g.title}</h3>
          <span class="muted num">{bytes(g.total)}</span>
          <span class="grow"></span>
          <button class="ghost tiny" onclick={() => toggleGroup(g.id, true)}>Select all</button>
          <button class="ghost tiny" onclick={() => toggleGroup(g.id, false)}>None</button>
        </header>
        {#each g.items as t (t.id)}
          <div class="item" class:empty={t.size === 0}>
            <input type="checkbox" id="c-{t.id}" bind:checked={picked[t.id]} disabled={t.size === 0} />
            <label for="c-{t.id}" class="txt">
              <span class="tt">
                {t.title}
                {#if t.needsAdmin}<span class="badge" title="Requires administrator password"><Icon name="settings" size={11} /> admin</span>{/if}
              </span>
              <span class="desc muted">{t.description}</span>
              {#if t.paths.length}<span class="paths mono">{t.paths.join("  ·  ")}</span>{/if}
              {#if t.caution && t.size > 0}<span class="caution"><Icon name="alert" size={12} /> {t.caution}</span>{/if}
            </label>
            <div class="right">
              <span class="size num">{t.size > 0 ? bytes(t.size) : "Clean"}</span>
              {#if t.files > 0}<span class="muted files num">{t.files.toLocaleString()} files</span>{/if}
              {#if t.details.length}
                <button class="ghost tiny" onclick={() => (open[t.id] = !open[t.id])}>{open[t.id] ? "Hide" : "Details"}</button>
              {/if}
            </div>
            {#if open[t.id]}
              <ul class="details mono">{#each t.details as d (d)}<li>{d}</li>{/each}</ul>
            {/if}
          </div>
        {/each}
      </section>
    {/if}
  {/each}
</div>

{#if confirming}
  <Modal title="Clean {bytes(chosenBytes)}?" onclose={() => !cleaning && (confirming = false)} width={520}>
    <p>The following will be <b>permanently deleted</b>. Caches are rebuilt automatically by the applications that use them.</p>
    <ul class="confirm">
      {#each chosen as t (t.id)}<li><span>{t.title}</span><span class="num">{bytes(t.size)}</span></li>{/each}
    </ul>
    {#if needsAdmin}<p class="muted"><Icon name="info" size={13} /> System items will ask for your password once.</p>{/if}
    {#snippet footer()}
      <button onclick={() => (confirming = false)} disabled={cleaning}>Cancel</button>
      <button class="danger" onclick={clean} disabled={cleaning}>{cleaning ? "Cleaning…" : "Clean now"}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .hero { display: grid; grid-template-columns: 220px 1fr auto; gap: 24px; align-items: center; background: var(--surface-1);
    border: 1px solid var(--border); border-radius: var(--radius); padding: 18px 22px; margin-bottom: 16px; }
  .lbl { font-size: 11.5px; text-transform: uppercase; letter-spacing: 0.06em; }
  .big { font-size: 30px; font-weight: 700; letter-spacing: -0.02em; margin: 2px 0; }
  .stack { display: flex; gap: 2px; height: 14px; border-radius: 6px; overflow: hidden; background: var(--surface-3); }
  .stack span { min-width: 3px; }
  .legend { display: flex; flex-wrap: wrap; gap: 16px; margin-top: 10px; font-size: 12.5px; color: var(--text-secondary); }
  .legend i { display: inline-block; width: 10px; height: 10px; border-radius: 3px; margin-right: 6px; vertical-align: -1px; }
  .legend b { color: var(--text-primary); font-weight: 600; margin-left: 4px; }
  .go { padding: 10px 18px; font-size: 14px; }
  .groups { display: grid; grid-template-columns: repeat(auto-fill, minmax(520px, 1fr)); gap: 14px; align-items: start; }
  .group { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 6px 0; }
  .group header { display: flex; align-items: center; gap: 8px; padding: 10px 16px; }
  .group h3 { font-size: 13px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  .gi { display: inline-flex; }
  .grow { flex: 1; }
  .tiny { font-size: 12px; padding: 2px 8px; color: var(--accent); }
  .item { display: grid; grid-template-columns: 20px 1fr auto; gap: 10px; padding: 10px 16px; border-top: 1px solid var(--border); align-items: start; }
  .item.empty { opacity: 0.5; }
  .item input { margin-top: 3px; width: 16px; height: 16px; accent-color: var(--accent); }
  .txt { display: flex; flex-direction: column; gap: 3px; min-width: 0; cursor: pointer; }
  .tt { font-weight: 550; display: inline-flex; gap: 8px; align-items: center; }
  .badge { font-size: 10.5px; font-weight: 600; color: var(--text-secondary); border: 1px solid var(--border-strong); border-radius: 99px; padding: 0 6px; display: inline-flex; gap: 3px; align-items: center; }
  .desc { font-size: 12.5px; }
  .paths { font-size: 11px; color: var(--text-muted); word-break: break-all; }
  .caution { font-size: 12px; color: var(--warning); display: inline-flex; gap: 4px; align-items: center; }
  .right { display: flex; flex-direction: column; align-items: flex-end; gap: 2px; }
  .size { font-weight: 600; }
  .files { font-size: 11.5px; }
  .details { grid-column: 2 / -1; margin: 0; padding: 8px 10px 8px 26px; background: var(--surface-2); border-radius: 8px; font-size: 11.5px; max-height: 180px; overflow: auto; color: var(--text-secondary); }
  .confirm { list-style: none; padding: 0; margin: 10px 0; display: flex; flex-direction: column; gap: 6px; color: var(--text-primary); }
  .confirm li { display: flex; justify-content: space-between; border-bottom: 1px solid var(--border); padding-bottom: 6px; }
</style>
