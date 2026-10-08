<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Services.svelte
  Author : Praneeth
  License: MIT
  Purpose: systemd services: system and user units with status filters,
           details (resources, unit file, recent logs) and start / stop /
           restart / enable / disable. System changes ask for the admin
           password; services the session depends on are protected.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import { bytes } from "../lib/format";
  import type { Service, ServiceAction, ServiceDetails } from "../lib/types";

  type Filter = "all" | "running" | "failed" | "enabled" | "disabled";

  let scope = $state<"system" | "user">("system");
  let services = $state<Service[]>([]);
  let loading = $state(true);
  let query = $state("");
  let filter = $state<Filter>("all");
  let selected = $state<Service | null>(null);
  let details = $state<ServiceDetails | null>(null);
  let busy = $state(false);
  let confirm = $state<ServiceAction | null>(null);

  async function load() {
    loading = true;
    try {
      services = await api.listServices(scope === "user");
      if (selected) selected = services.find((s) => s.unit === selected!.unit) ?? null;
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      loading = false;
    }
  }
  onMount(load);

  async function select(s: Service) {
    selected = s;
    details = null;
    details = await api.serviceDetails(s.unit, s.user).catch((e) => {
      app.notify(String(e), "error");
      return null;
    });
  }

  const running = (s: Service) => s.active === "active" && s.sub === "running";
  const failed = (s: Service) => s.active === "failed";
  const enabled = (s: Service) => s.fileState === "enabled" || s.fileState === "enabled-runtime";

  let counts = $derived({
    all: services.length,
    running: services.filter(running).length,
    failed: services.filter(failed).length,
    enabled: services.filter(enabled).length,
    disabled: services.filter((s) => s.fileState === "disabled").length,
  });

  let rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return services.filter((s) => {
      if (filter === "running" && !running(s)) return false;
      if (filter === "failed" && !failed(s)) return false;
      if (filter === "enabled" && !enabled(s)) return false;
      if (filter === "disabled" && s.fileState !== "disabled") return false;
      return !q || s.unit.toLowerCase().includes(q) || s.description.toLowerCase().includes(q);
    });
  });

  function ask(action: ServiceAction) {
    if (action === "start" || action === "enable") return run(action);
    confirm = action;
  }

  async function run(action: ServiceAction) {
    if (!selected) return;
    confirm = null;
    busy = true;
    const unit = selected.unit;
    try {
      await api.serviceAction(unit, action, selected.user);
      const done = { start: "Started", stop: "Stopped", restart: "Restarted", enable: "Enabled", disable: "Disabled" }[action];
      app.notify(`${done} ${unit}`);
      await load();
      if (selected) await select(selected);
    } catch (e) {
      app.notify(String(e), "error", 8000);
    } finally {
      busy = false;
    }
  }

  const stateLabel = (s: Service) => (s.active === "active" ? s.sub : s.active);
</script>

<PageHeader title="Services" subtitle="systemd units — background services that start with the system or your session">
  {#snippet actions()}
    <div class="search">
      <Icon name="search" size={15} />
      <input placeholder="Search services…" bind:value={query} aria-label="Search services" />
    </div>
    <div class="seg">
      <button class:on={scope === "system"} onclick={() => { scope = "system"; selected = null; load(); }}>System</button>
      <button class:on={scope === "user"} onclick={() => { scope = "user"; selected = null; load(); }}>My session</button>
    </div>
    <button onclick={load} disabled={loading}><Icon name="refresh" size={14} /> Refresh</button>
  {/snippet}
</PageHeader>

<div class="tiles">
  {#each [["all", "Total", "var(--text-primary)"], ["running", "Running", "var(--good)"], ["failed", "Failed", "var(--critical)"], ["enabled", "Start at boot", "var(--accent)"], ["disabled", "Disabled", "var(--text-muted)"]] as [id, label, color] (id)}
    <button class="tile" class:on={filter === id} onclick={() => (filter = id as Filter)} aria-pressed={filter === id}>
      <span class="big num" style:color={color}>{counts[id as Filter]}</span>
      <span class="muted">{label}</span>
    </button>
  {/each}
</div>

<div class="layout" class:withside={selected}>
  <div class="list">
    {#if loading && !services.length}<div class="muted pad">Loading services…</div>{/if}
    {#each rows as s (s.unit)}
      <div class="row" class:sel={selected?.unit === s.unit} role="button" tabindex="0" onclick={() => select(s)} onkeydown={(e) => e.key === "Enter" && select(s)}>
        <span class="dot" class:run={running(s)} class:fail={failed(s)} class:act={s.active === "active" && !running(s)}></span>
        <div class="meta">
          <div class="nm">{s.unit.replace(/\.service$/, "")}{#if s.protected}<span class="lock" title="Required by the system"><Icon name="alert" size={11} /></span>{/if}</div>
          <div class="muted desc">{s.description}</div>
        </div>
        <span class="st" class:fail={failed(s)}>{stateLabel(s)}</span>
        <span class="fs {s.fileState ?? ''}">{s.fileState ?? "—"}</span>
      </div>
    {:else}
      {#if !loading}<div class="muted pad">No services match.</div>{/if}
    {/each}
  </div>

  {#if selected}
    {@const s = selected}
    <aside class="side">
      <div class="sh">
        <div>
          <h2>{s.unit}</h2>
          <div class="muted">{s.description}</div>
        </div>
        <button class="ghost" onclick={() => (selected = null)} aria-label="Close"><Icon name="close" size={16} /></button>
      </div>
      <div class="badges">
        <span class="pill" class:good={running(s)} class:bad={failed(s)}>{s.active} · {s.sub}</span>
        <span class="pill">{s.fileState ?? "no unit file"}</span>
        {#if s.preset}<span class="pill muted">preset: {s.preset}</span>{/if}
      </div>
      <div class="actions">
        <button disabled={busy || s.active === "active"} onclick={() => ask("start")}><Icon name="play" size={13} /> Start</button>
        <button disabled={busy || s.active !== "active" || s.protected} onclick={() => ask("stop")} title={s.protected ? "Required by the system" : ""}><Icon name="stop" size={13} /> Stop</button>
        <button disabled={busy || s.protected} onclick={() => ask("restart")}><Icon name="refresh" size={13} /> Restart</button>
        {#if enabled(s)}
          <button disabled={busy || s.protected || s.fileState === "static"} onclick={() => ask("disable")}>Disable at boot</button>
        {:else}
          <button disabled={busy || s.fileState === "static" || s.fileState === "masked"} onclick={() => ask("enable")}>Enable at boot</button>
        {/if}
      </div>
      {#if !s.user}<p class="muted note"><Icon name="info" size={12} /> Changes to system services ask for your password.</p>{/if}
      {#if s.fileState === "static"}<p class="muted note">Static units are started by other units and cannot be enabled directly.</p>{/if}

      {#if details}
        {@const d = details}
        <dl>
          <dt>Main PID</dt><dd>{#if d.mainPid}<button class="link" onclick={() => (app.page = "processes")}>{d.mainPid}</button>{:else}—{/if}</dd>
          <dt>Memory</dt><dd>{bytes(d.memory)}</dd>
          <dt>CPU time</dt><dd>{d.cpuSeconds != null ? `${d.cpuSeconds.toFixed(1)} s` : "—"}</dd>
          <dt>Tasks</dt><dd>{d.tasks ?? "—"}</dd>
          <dt>Restarts</dt><dd>{d.restarts ?? "—"}</dd>
          <dt>Active since</dt><dd>{d.since ?? "—"}</dd>
          <dt>Unit file</dt><dd class="mono wrap">{d.fragmentPath ?? "—"}</dd>
          {#each d.properties.filter(([k]) => ["Type", "Restart", "User", "Documentation"].includes(k)) as [k, v] (k)}
            <dt>{k}</dt><dd class="wrap">{v}</dd>
          {/each}
        </dl>
        <h3 class="cap">Recent log</h3>
        <pre class="logs mono">{d.logs.length ? d.logs.join("\n") : "No journal entries (or no permission to read them)."}</pre>
      {:else}
        <div class="muted">Loading details…</div>
      {/if}
    </aside>
  {/if}
</div>

{#if confirm && selected}
  {@const c = confirm}
  <Modal title="{c[0].toUpperCase() + c.slice(1)} {selected.unit}?" onclose={() => (confirm = null)}>
    {#if c === "stop"}Features provided by this service will stop working until it is started again.
    {:else if c === "restart"}The service will be stopped and started again.
    {:else}The service will no longer start automatically at boot.{/if}
    {#snippet footer()}
      <button onclick={() => (confirm = null)}>Cancel</button>
      <button class="danger" onclick={() => run(c)}>{c[0].toUpperCase() + c.slice(1)}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .search { display: flex; align-items: center; gap: 6px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: 8px; padding: 0 10px; color: var(--text-muted); }
  .search input { border: none; background: transparent; width: 200px; padding: 7px 0; }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 4px 12px; border-radius: 7px; color: var(--text-secondary); font-size: 13px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .tiles { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 10px; margin-bottom: 14px; }
  .tile { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; padding: 10px 14px; background: var(--surface-1); border: 1px solid var(--border); border-radius: 12px; text-align: left; }
  .tile.on { border-color: var(--accent); }
  .tile .big { font-size: 22px; font-weight: 650; }
  .tile .muted { font-size: 12px; }
  .layout { display: grid; grid-template-columns: 1fr; gap: 14px; }
  .layout.withside { grid-template-columns: minmax(0, 1fr) 420px; }
  .list { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); max-height: calc(100vh - 250px); overflow-y: auto; }
  .pad { padding: 20px; }
  .row { display: grid; grid-template-columns: 10px minmax(0, 1fr) 90px 90px; gap: 12px; align-items: center; padding: 8px 14px; border-bottom: 1px solid var(--border); }
  .row:hover { background: var(--surface-2); }
  .row.sel { background: color-mix(in srgb, var(--accent) 14%, transparent); }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--surface-3); }
  .dot.run { background: var(--good); }
  .dot.act { background: color-mix(in srgb, var(--good) 45%, var(--surface-3)); }
  .dot.fail { background: var(--critical); }
  .meta { min-width: 0; }
  .nm { font-weight: 550; display: flex; align-items: center; gap: 6px; }
  .lock { color: var(--warning); display: inline-flex; }
  .desc { font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .st { font-size: 12px; color: var(--text-secondary); }
  .st.fail { color: var(--critical); font-weight: 600; }
  .fs { font-size: 11px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--text-muted); }
  .fs.enabled { color: var(--accent); }
  .side { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 16px 18px; max-height: calc(100vh - 250px); overflow-y: auto; }
  .sh { display: flex; justify-content: space-between; gap: 8px; }
  .sh h2 { font-size: 15px; word-break: break-all; }
  .sh .muted { font-size: 12.5px; margin-top: 3px; }
  .badges { display: flex; gap: 6px; flex-wrap: wrap; margin: 12px 0; }
  .pill { font-size: 11.5px; border: 1px solid var(--border-strong); border-radius: 99px; padding: 2px 9px; }
  .pill.good { color: var(--good); border-color: currentColor; }
  .pill.bad { color: var(--critical); border-color: currentColor; }
  .actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .actions button { font-size: 12.5px; padding: 5px 10px; }
  .note { font-size: 12px; margin: 8px 0 0; display: flex; gap: 4px; align-items: center; }
  dl { display: grid; grid-template-columns: 110px 1fr; gap: 6px 10px; margin: 14px 0; font-size: 12.5px; }
  dt { color: var(--text-muted); }
  dd { margin: 0; min-width: 0; }
  .wrap { word-break: break-all; }
  .link { all: unset; color: var(--accent); cursor: pointer; }
  .cap { margin: 14px 0 8px; font-size: 11.5px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  .logs { margin: 0; font-size: 11px; line-height: 1.5; background: var(--surface-2); padding: 10px; border-radius: 8px; max-height: 320px; overflow: auto; white-space: pre-wrap; word-break: break-all; color: var(--text-secondary); }
</style>
