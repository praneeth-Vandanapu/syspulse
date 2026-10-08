<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Startup.svelte
  Author : Praneeth
  License: MIT
  Purpose: Startup applications (XDG autostart): enable / disable, add, edit
           and remove programs that start when you log in.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "../lib/components/AppIcon.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import type { StartupEntry, StartupInput } from "../lib/types";

  let entries = $state<StartupEntry[]>([]);
  let loading = $state(true);
  let showHidden = $state(false);
  let editing = $state<StartupInput | null>(null);
  let removing = $state<StartupEntry | null>(null);

  async function load() {
    loading = true;
    try {
      entries = await api.listStartup();
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      loading = false;
    }
  }
  onMount(load);

  let visible = $derived(entries.filter((e) => showHidden || (!e.noDisplay && e.appliesHere)));
  let hiddenCount = $derived(entries.length - entries.filter((e) => !e.noDisplay && e.appliesHere).length);
  let enabledCount = $derived(visible.filter((e) => e.enabled).length);

  async function toggle(e: StartupEntry) {
    const next = !e.enabled;
    try {
      await api.setStartupEnabled(e.id, next);
      e.enabled = next;
      app.notify(`${e.name} will ${next ? "" : "no longer "}start when you log in`);
    } catch (err) {
      app.notify(String(err), "error");
    }
  }

  async function save() {
    if (!editing) return;
    try {
      await api.saveStartup({ ...editing, delay: editing.delay || null });
      app.notify(editing.id ? "Startup entry updated" : `${editing.name} added to startup`);
      editing = null;
      await load();
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function remove() {
    if (!removing) return;
    try {
      await api.removeStartup(removing.id);
      app.notify(`${removing.name} removed`);
      removing = null;
      await load();
    } catch (e) {
      app.notify(String(e), "error");
    }
  }
</script>

<PageHeader title="Startup Apps" subtitle="{enabledCount} of {visible.length} programs start automatically when you log in">
  {#snippet actions()}
    <label class="chk"><input type="checkbox" bind:checked={showHidden} /> Show system entries ({hiddenCount})</label>
    <button class="primary" onclick={() => (editing = { id: null, name: "", exec: "", comment: "", delay: null })}><Icon name="play" size={13} /> Add program</button>
  {/snippet}
</PageHeader>

{#if loading && !entries.length}<div class="muted">Loading…</div>{/if}

<div class="grid">
  {#each visible as e (e.id)}
    <article class="card" class:off={!e.enabled}>
      <AppIcon icon={e.icon} name={e.name} size={40} />
      <div class="meta">
        <div class="nm">
          {e.name}
          {#if e.system}<span class="tag">system</span>{:else}<span class="tag mine">added by you</span>{/if}
          {#if e.noDisplay}<span class="tag">hidden</span>{/if}
          {#if !e.appliesHere}<span class="tag">other desktops</span>{/if}
        </div>
        {#if e.comment}<div class="muted cm">{e.comment}</div>{/if}
        <div class="mono ex" title={e.exec}>{e.exec}</div>
        <div class="row">
          {#if e.delay}<span class="muted small">Delay {e.delay}s</span>{/if}
          <span class="grow"></span>
          <button class="ghost tiny" onclick={() => (editing = { id: e.id, name: e.name, exec: e.exec, comment: e.comment, delay: e.delay })}>Edit</button>
          {#if !e.system}<button class="ghost tiny danger-t" onclick={() => (removing = e)}>Remove</button>{/if}
        </div>
      </div>
      <button class="switch" class:on={e.enabled} role="switch" aria-checked={e.enabled} aria-label="Start {e.name} at login" onclick={() => toggle(e)}><span></span></button>
    </article>
  {/each}
</div>

{#if editing}
  <Modal title={editing.id ? "Edit startup program" : "Add startup program"} onclose={() => (editing = null)} width={520}>
    <div class="form">
      <label>Name <input bind:value={editing.name} placeholder="My sync script" /></label>
      <label>Command <input class="mono" bind:value={editing.exec} placeholder="/home/you/bin/sync.sh --quiet" /></label>
      <label>Comment <input bind:value={editing.comment} placeholder="Optional description" /></label>
      <label>Delay after login (seconds) <input type="number" min="0" max="600" bind:value={editing.delay} placeholder="0" /></label>
      <p class="muted small">Saved to ~/.config/autostart. Only your account is affected.</p>
    </div>
    {#snippet footer()}
      <button onclick={() => (editing = null)}>Cancel</button>
      <button class="primary" disabled={!editing?.name.trim() || !editing?.exec.trim()} onclick={save}>Save</button>
    {/snippet}
  </Modal>
{/if}

{#if removing}
  <Modal title="Remove {removing.name}?" onclose={() => (removing = null)}>
    The program stays installed; it just won't start at login anymore.
    {#snippet footer()}
      <button onclick={() => (removing = null)}>Cancel</button>
      <button class="danger" onclick={remove}>Remove</button>
    {/snippet}
  </Modal>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .chk { display: inline-flex; gap: 6px; align-items: center; font-size: 13px; color: var(--text-secondary); }
  .chk input { accent-color: var(--accent); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(440px, 1fr)); gap: 12px; }
  .card { display: grid; grid-template-columns: 40px 1fr auto; gap: 14px; align-items: start; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 14px 16px; }
  .card.off .meta, .card.off :global(img), .card.off :global(.mono-ic) { opacity: 0.55; }
  .meta { min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .nm { font-weight: 600; display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .tag { font-size: 10.5px; font-weight: 600; color: var(--text-muted); border: 1px solid var(--border-strong); border-radius: 99px; padding: 0 7px; }
  .tag.mine { color: var(--accent); border-color: currentColor; }
  .cm { font-size: 12.5px; }
  .ex { font-size: 11.5px; color: var(--text-secondary); background: var(--surface-2); padding: 4px 8px; border-radius: 6px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .row { display: flex; align-items: center; gap: 4px; }
  .grow { flex: 1; }
  .small { font-size: 12px; }
  .tiny { font-size: 12px; padding: 2px 8px; }
  .danger-t { color: var(--critical); }
  .switch { width: 42px; height: 24px; border-radius: 99px; padding: 0; position: relative; background: var(--surface-3); border: 1px solid var(--border-strong); flex: none; }
  .switch span { position: absolute; top: 2px; left: 2px; width: 18px; height: 18px; border-radius: 50%; background: #fff; transition: left 0.15s; }
  .switch.on { background: var(--accent); border-color: transparent; }
  .switch.on span { left: 20px; }
  .form { display: flex; flex-direction: column; gap: 12px; }
  .form label { display: flex; flex-direction: column; gap: 5px; font-size: 12.5px; color: var(--text-secondary); }
  .form input { font-size: 13.5px; }
</style>
