<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Software.svelte
  Author : Praneeth
  License: MIT
  Purpose: Installed software manager for APT, Snap and Flatpak: browse,
           search, sort, inspect and uninstall (with a dry-run preview of
           everything that will be removed).
-->
<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "../lib/components/AppIcon.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import { bytes } from "../lib/format";
  import type { Package, PackageSource, RemovalPlan } from "../lib/types";

  const SOURCES: { id: PackageSource; label: string }[] = [
    { id: "apt", label: "APT" },
    { id: "snap", label: "Snap" },
    { id: "flatpak", label: "Flatpak" },
  ];
  const PAGE = 120;

  let packages = $state<Package[]>([]);
  let loading = $state(true);
  let view = $state<"apps" | "all">("apps");
  let sources = $state<Record<PackageSource, boolean>>({ apt: true, snap: true, flatpak: true });
  let query = $state("");
  let sort = $state<"name" | "size" | "date">("name");
  let hideAuto = $state(true);
  let limit = $state(PAGE);
  let selected = $state<Package | null>(null);

  // Uninstall dialog state
  let target = $state<Package | null>(null);
  let plan = $state<RemovalPlan | null>(null);
  let planError = $state("");
  let purge = $state(false);
  let removing = $state(false);

  async function load() {
    loading = true;
    try {
      packages = await api.listPackages();
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      loading = false;
    }
  }
  onMount(load);

  let stats = $derived(
    SOURCES.map((s) => {
      const list = packages.filter((p) => p.source === s.id);
      return { ...s, count: list.length, apps: list.filter((p) => p.isApp).length, size: list.reduce((a, p) => a + (p.installedSize ?? 0), 0) };
    }),
  );

  let rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = packages.filter((p) => {
      if (!sources[p.source]) return false;
      if (view === "apps" && !p.isApp) return false;
      if (view === "all" && hideAuto && p.autoInstalled && !q) return false;
      if (!q) return true;
      return p.displayName.toLowerCase().includes(q) || p.name.toLowerCase().includes(q) || p.summary.toLowerCase().includes(q);
    });
    const by = {
      name: (a: Package, b: Package) => a.displayName.localeCompare(b.displayName, undefined, { sensitivity: "base" }),
      size: (a: Package, b: Package) => (b.installedSize ?? 0) - (a.installedSize ?? 0),
      date: (a: Package, b: Package) => (b.installDate ?? 0) - (a.installDate ?? 0),
    }[sort];
    return list.sort(by);
  });

  $effect(() => {
    // Reset paging when the filter changes.
    void [query, view, sort, hideAuto, sources.apt, sources.snap, sources.flatpak];
    limit = PAGE;
  });

  async function askUninstall(p: Package) {
    target = p;
    plan = null;
    planError = "";
    purge = false;
    try {
      plan = await api.planPackageRemoval(p.source, [p.name]);
    } catch (e) {
      planError = String(e);
    }
  }

  async function uninstall() {
    if (!target || !plan) return;
    removing = true;
    try {
      const r = await api.removePackages(target.source, [target.name], purge);
      app.notify(`${r.message} · freed about ${bytes(plan.freedBytes)}`);
      target = null;
      selected = null;
      await load();
    } catch (e) {
      app.notify(String(e), "error", 8000);
    } finally {
      removing = false;
    }
  }

  const fmtDate = (s: number | null) => (s ? new Date(s * 1000).toLocaleDateString() : "—");
</script>

<PageHeader
  title="Software"
  subtitle={loading ? "Reading package databases…" : `${packages.length} packages · ${packages.filter((p) => p.isApp).length} applications`}
>
  {#snippet actions()}
    <div class="search">
      <Icon name="search" size={15} />
      <input placeholder="Search name or description…" bind:value={query} aria-label="Search software" />
    </div>
    <button onclick={load} disabled={loading}><Icon name="refresh" size={14} /> Refresh</button>
  {/snippet}
</PageHeader>

<div class="tiles">
  {#each stats as s (s.id)}
    <button class="tile" class:off={!sources[s.id]} onclick={() => (sources[s.id] = !sources[s.id])} aria-pressed={sources[s.id]}>
      <span class="src {s.id}">{s.label}</span>
      <span class="big num">{s.count}</span>
      <span class="muted">{s.apps} apps · {bytes(s.size)}</span>
    </button>
  {/each}
</div>

<div class="toolbar">
  <div class="seg">
    <button class:on={view === "apps"} onclick={() => (view = "apps")}>Applications</button>
    <button class:on={view === "all"} onclick={() => (view = "all")}>All packages</button>
  </div>
  {#if view === "all"}
    <label class="chk"><input type="checkbox" bind:checked={hideAuto} /> Hide automatically installed dependencies</label>
  {/if}
  <span class="grow"></span>
  <label class="sort">Sort
    <select bind:value={sort}>
      <option value="name">Name</option>
      <option value="size">Size</option>
      <option value="date">Install date</option>
    </select>
  </label>
</div>

<div class="layout" class:withside={selected}>
  <div class="list">
    {#if loading}
      <div class="muted pad">Loading…</div>
    {:else if !rows.length}
      <div class="muted pad">No software matches.</div>
    {/if}
    {#each rows.slice(0, limit) as p (p.id)}
      <div class="row" class:sel={selected?.id === p.id} role="button" tabindex="0"
        onclick={() => (selected = p)} onkeydown={(e) => e.key === "Enter" && (selected = p)}>
        <AppIcon icon={p.icon} name={p.displayName} size={34} />
        <div class="meta">
          <div class="nm">{p.displayName} {#if p.displayName !== p.name}<span class="muted pkg">{p.name}</span>{/if}</div>
          <div class="sum muted">{p.summary}</div>
        </div>
        <span class="src {p.source}">{p.source}</span>
        <span class="ver muted mono" title={p.version}>{p.version}</span>
        <span class="size num">{bytes(p.installedSize)}</span>
        <span class="date muted num">{fmtDate(p.installDate)}</span>
        {#if p.protected}
          <span class="lock muted" title="System component — cannot be uninstalled here"><Icon name="alert" size={13} /> System</span>
        {:else}
          <button class="small" onclick={(e) => { e.stopPropagation(); askUninstall(p); }}>Uninstall</button>
        {/if}
      </div>
    {/each}
    {#if rows.length > limit}
      <button class="more" onclick={() => (limit += PAGE)}>Show more ({rows.length - limit} remaining)</button>
    {/if}
  </div>

  {#if selected}
    {@const p = selected}
    <aside class="side">
      <div class="sh">
        <AppIcon icon={p.icon} name={p.displayName} size={48} />
        <div>
          <h2>{p.displayName}</h2>
          <div class="muted">{p.name}</div>
        </div>
        <button class="ghost close" onclick={() => (selected = null)} aria-label="Close"><Icon name="close" size={16} /></button>
      </div>
      <p class="secondary">{p.summary}</p>
      <dl>
        <dt>Source</dt><dd><span class="src {p.source}">{p.source}</span>{p.installation ? ` (${p.installation})` : ""}</dd>
        <dt>Version</dt><dd class="mono">{p.version}</dd>
        <dt>Installed size</dt><dd>{bytes(p.installedSize)}</dd>
        <dt>Installed</dt><dd>{fmtDate(p.installDate)}</dd>
        {#if p.section}<dt>Section</dt><dd>{p.section}</dd>{/if}
        {#if p.arch}<dt>Architecture</dt><dd>{p.arch}</dd>{/if}
        {#if p.publisher}<dt>Publisher</dt><dd>{p.publisher}</dd>{/if}
        {#if p.homepage}<dt>Homepage</dt><dd class="wrap">{p.homepage}</dd>{/if}
        <dt>Dependency</dt><dd>{p.autoInstalled ? "Installed automatically" : "Installed by you"}</dd>
      </dl>
      {#if p.protected}
        <div class="note"><Icon name="alert" size={14} /> Essential system component. Removing it could break Ubuntu, so SysPulse does not offer it.</div>
      {:else}
        <button class="danger wide" onclick={() => askUninstall(p)}>Uninstall…</button>
      {/if}
    </aside>
  {/if}
</div>

{#if target}
  <Modal title="Uninstall {target.displayName}?" onclose={() => !removing && (target = null)} width={520}>
    {#if planError}
      <p class="err">{planError}</p>
    {:else if !plan}
      <p>Checking what will be removed…</p>
    {:else}
      {#if plan.blockers.length}
        <div class="blk">
          <b><Icon name="alert" size={14} /> This cannot be uninstalled safely:</b>
          <ul>{#each plan.blockers as b (b)}<li>{b}</li>{/each}</ul>
        </div>
      {:else}
        <p>
          {plan.removes.length === 1 ? "This package" : `${plan.removes.length} packages`} will be removed, freeing about
          <b class="num">{bytes(plan.freedBytes)}</b>.
          {#if plan.needsAdmin}You will be asked for your password.{/if}
        </p>
        {#if plan.removes.length > 1}
          <div class="pkgs mono">{plan.removes.join("  ·  ")}</div>
        {/if}
        {#each plan.warnings as w (w)}<p class="warn"><Icon name="info" size={13} /> {w}</p>{/each}
        {#if target.source !== "flatpak"}
          <label class="chk">
            <input type="checkbox" bind:checked={purge} />
            {target.source === "apt" ? "Also remove system-wide configuration files (purge)" : "Delete the app's saved data too (no snapshot)"}
          </label>
        {/if}
      {/if}
    {/if}
    {#snippet footer()}
      <button onclick={() => (target = null)} disabled={removing}>Cancel</button>
      <button class="danger" onclick={uninstall} disabled={!plan || plan.blockers.length > 0 || removing}>
        {removing ? "Removing…" : "Uninstall"}
      </button>
    {/snippet}
  </Modal>
{/if}

<style>
  .search { display: flex; align-items: center; gap: 6px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: 8px; padding: 0 10px; color: var(--text-muted); }
  .search input { border: none; background: transparent; width: 240px; padding: 7px 0; }
  button :global(svg) { vertical-align: -2px; }
  .tiles { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; margin-bottom: 14px; }
  .tile { display: flex; flex-direction: column; align-items: flex-start; gap: 4px; text-align: left; padding: 12px 16px; border-radius: var(--radius);
    background: var(--surface-1); border: 1px solid var(--border); }
  .tile.off { opacity: 0.45; }
  .tile .big { font-size: 24px; font-weight: 650; }
  .tile .muted { font-size: 12px; }
  .src { font-size: 10.5px; font-weight: 700; letter-spacing: 0.05em; text-transform: uppercase; padding: 2px 7px; border-radius: 5px; color: #fff; }
  .src.apt { background: #c2410c; }
  .src.snap { background: #15803d; }
  .src.flatpak { background: #1d4ed8; }
  .toolbar { display: flex; align-items: center; gap: 14px; margin-bottom: 10px; flex-wrap: wrap; }
  .grow { flex: 1; }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 4px 12px; border-radius: 7px; color: var(--text-secondary); font-size: 13px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .chk { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--text-secondary); }
  .chk input { accent-color: var(--accent); }
  .sort { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--text-secondary); }

  .layout { display: grid; grid-template-columns: 1fr; gap: 14px; }
  .layout.withside { grid-template-columns: minmax(0, 1fr) 340px; }
  .list { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .pad { padding: 24px; }
  .row { display: grid; grid-template-columns: 34px minmax(0, 1fr) 70px 150px 80px 90px 96px; align-items: center; gap: 12px;
    padding: 9px 14px; border-bottom: 1px solid var(--border); cursor: default; }
  .row:hover { background: var(--surface-2); }
  .row.sel { background: color-mix(in srgb, var(--accent) 14%, transparent); }
  .meta { min-width: 0; }
  .nm { font-weight: 550; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pkg { font-weight: 400; font-size: 12px; margin-left: 4px; }
  .sum { font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; margin-top: 2px; }
  .row .src { justify-self: start; }
  .ver { font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .size { text-align: right; font-size: 13px; }
  .date { font-size: 12px; text-align: right; }
  .lock { font-size: 11.5px; display: inline-flex; align-items: center; gap: 4px; justify-self: center; }
  button.small { font-size: 12px; padding: 4px 10px; }
  .more { width: 100%; border: none; border-radius: 0; padding: 12px; background: transparent; color: var(--accent); }

  .side { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 16px 18px; align-self: start; position: sticky; top: 0; }
  .sh { display: flex; gap: 12px; align-items: center; }
  .sh h2 { font-size: 16px; }
  .sh .muted { font-size: 12px; }
  .close { margin-left: auto; align-self: flex-start; }
  dl { display: grid; grid-template-columns: 110px 1fr; gap: 7px 10px; font-size: 12.5px; margin: 12px 0 16px; }
  dt { color: var(--text-muted); }
  dd { margin: 0; min-width: 0; }
  .wrap { word-break: break-all; }
  .note { font-size: 12.5px; color: var(--text-secondary); background: var(--surface-2); border-radius: 8px; padding: 10px; display: flex; gap: 6px; }
  .wide { width: 100%; }
  .blk { background: color-mix(in srgb, var(--critical) 12%, transparent); border: 1px solid color-mix(in srgb, var(--critical) 40%, transparent); border-radius: 8px; padding: 10px 12px; color: var(--text-primary); }
  .blk ul { margin: 6px 0 0; padding-left: 18px; }
  .pkgs { font-size: 11.5px; background: var(--surface-2); padding: 8px 10px; border-radius: 8px; max-height: 140px; overflow: auto; margin: 8px 0; color: var(--text-primary); }
  .warn { font-size: 12.5px; margin: 6px 0; }
  .err { color: var(--critical); }
</style>
