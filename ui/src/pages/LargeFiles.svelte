<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/LargeFiles.svelte
  Author : Praneeth
  License: MIT
  Purpose: Large-file finder: scan a folder or disk with filters, see the
           biggest files, folders and file types, and open / trash / delete.
-->
<script lang="ts">
  import { onDestroy } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import Treemap from "../lib/components/Treemap.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api, isTauri, onScanProgress, pickFolder } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import { bytes, duration } from "../lib/format";
  import type { FileCategory, LargeFile, ScanProgress, ScanResult } from "../lib/types";

  const SIZES = [
    { v: 10 * 2 ** 20, l: "10 MB" },
    { v: 50 * 2 ** 20, l: "50 MB" },
    { v: 100 * 2 ** 20, l: "100 MB" },
    { v: 250 * 2 ** 20, l: "250 MB" },
    { v: 500 * 2 ** 20, l: "500 MB" },
    { v: 2 ** 30, l: "1 GB" },
    { v: 5 * 2 ** 30, l: "5 GB" },
  ];
  const CATS: { id: FileCategory; label: string; color: string }[] = [
    { id: "video", label: "Video", color: "var(--series-1)" },
    { id: "archive", label: "Archives", color: "var(--series-2)" },
    { id: "diskimage", label: "Disk images", color: "var(--series-7)" },
    { id: "image", label: "Images", color: "var(--series-3)" },
    { id: "audio", label: "Audio", color: "var(--series-5)" },
    { id: "document", label: "Documents", color: "var(--series-4)" },
    { id: "package", label: "Installers", color: "var(--series-6)" },
    { id: "code", label: "Code & data", color: "var(--series-8)" },
    { id: "other", label: "Other", color: "var(--text-muted)" },
  ];
  const catInfo = (c: FileCategory) => CATS.find((x) => x.id === c)!;

  let root = $state("~");
  let minSize = $state(100 * 2 ** 20);
  let cats = $state<Record<string, boolean>>({});
  let includeHidden = $state(true);
  let sameFs = $state(true);
  let scanning = $state(false);
  let progress = $state<ScanProgress | null>(null);
  let result = $state<ScanResult | null>(null);
  let selected = $state<Record<string, boolean>>({});
  let confirm = $state<null | "trash" | "delete">(null);
  let working = $state(false);
  let unlisten: (() => void) | null = null;
  let view = $state<"files" | "map">("files");

  onDestroy(() => {
    unlisten?.();
    if (scanning) api.largeCancel();
  });

  async function scan(path = root) {
    root = path;
    scanning = true;
    progress = null;
    selected = {};
    unlisten?.();
    unlisten = await onScanProgress((p) => (progress = p));
    try {
      result = await api.largeScan({
        root: path,
        minSize,
        includeHidden,
        sameFilesystem: sameFs,
        categories: Object.entries(cats).filter(([, v]) => v).map(([k]) => k as FileCategory),
        maxResults: 1000,
      });
      if (result.cancelled) app.notify("Scan cancelled — showing partial results", "info");
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      scanning = false;
      unlisten?.();
      unlisten = null;
    }
  }

  async function browse() {
    const p = await pickFolder(root === "~" ? undefined : root);
    if (p) root = p;
  }

  let files = $derived(result?.files ?? []);
  let maxSize = $derived(Math.max(1, ...files.map((f) => f.size)));
  let chosen = $derived(files.filter((f) => selected[f.path]));
  let chosenBytes = $derived(chosen.reduce((a, f) => a + f.size, 0));
  let allChecked = $derived(files.length > 0 && chosen.length === files.length);
  let catTotal = $derived(result?.categories.reduce((a, c) => a + c.bytes, 0) ?? 1);

  const folderOf = (p: string) => p.slice(0, p.lastIndexOf("/")) || "/";
  const short = (p: string) => (result && p.startsWith(result.root) && result.root !== "/" ? "…" + p.slice(result.root.length) : p);

  async function act(kind: "trash" | "delete") {
    working = true;
    const paths = chosen.map((f) => f.path);
    try {
      let r = kind === "trash" ? await api.filesTrash(paths) : await api.filesDelete(paths, false);
      if (r.denied.length) {
        // Files owned by root/another user: retry with admin rights (delete only).
        const admin = await api.filesDelete(r.denied, true).catch((e) => {
          app.notify(`Admin delete: ${e}`, "error", 8000);
          return null;
        });
        if (admin) r = { ...r, done: [...r.done, ...admin.done], freedBytes: r.freedBytes + admin.freedBytes, denied: [] };
      }
      if (r.done.length) {
        app.notify(`${kind === "trash" ? "Moved to Trash" : "Deleted"} ${r.done.length} file(s) · ${bytes(r.freedBytes)}${kind === "trash" ? " (empty Trash to free the space)" : " freed"}`);
        const gone = new Set(r.done);
        if (result) result = { ...result, files: result.files.filter((f) => !gone.has(f.path)) };
        selected = {};
      }
      for (const e of r.errors) app.notify(e, "error", 8000);
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      working = false;
      confirm = null;
    }
  }

  function reveal(f: LargeFile) {
    api.revealPath(f.path).catch((e) => app.notify(String(e), "error"));
  }

  function toggleAll() {
    const v = !allChecked;
    selected = Object.fromEntries(files.map((f) => [f.path, v]));
  }
</script>

<PageHeader title="Large Files" subtitle="Find what is using your disk space">
  {#snippet actions()}
    {#if scanning}
      <button onclick={() => api.largeCancel()}><Icon name="stop" size={14} /> Cancel</button>
    {:else}
      <button class="primary" onclick={() => scan()}><Icon name="search" size={14} /> Scan</button>
    {/if}
  {/snippet}
</PageHeader>

<section class="controls">
  <div class="row1">
    <div class="pathbox">
      <Icon name="files" size={15} />
      <input bind:value={root} aria-label="Folder to scan" spellcheck="false" onkeydown={(e) => e.key === "Enter" && !scanning && scan()} />
    </div>
    {#if isTauri}<button onclick={browse}>Browse…</button>{/if}
    <button class="ghost" onclick={() => (root = "~")}>Home</button>
    <button class="ghost" onclick={() => (root = "/")}>Whole disk</button>
    <label class="sel">Larger than
      <select bind:value={minSize}>{#each SIZES as s (s.v)}<option value={s.v}>{s.l}</option>{/each}</select>
    </label>
  </div>
  <div class="row2">
    {#each CATS as c (c.id)}
      <button class="chip" class:on={cats[c.id]} onclick={() => (cats[c.id] = !cats[c.id])} aria-pressed={!!cats[c.id]}>
        <i style:background={c.color}></i>{c.label}
      </button>
    {/each}
    <span class="grow"></span>
    <label class="chk"><input type="checkbox" bind:checked={includeHidden} /> Include hidden</label>
    <label class="chk"><input type="checkbox" bind:checked={sameFs} /> Stay on this disk</label>
  </div>
</section>

{#if scanning}
  <section class="progress">
    <div class="pbar"><span></span></div>
    <div class="pstats">
      <span><b class="num">{(progress?.filesScanned ?? 0).toLocaleString()}</b> files</span>
      <span><b class="num">{bytes(progress?.bytesScanned ?? 0)}</b> scanned</span>
      <span class="muted">{duration((progress?.elapsedMs ?? 0) / 1000)}</span>
    </div>
    <div class="cur mono muted">{progress?.current ?? "Starting…"}</div>
  </section>
{/if}

{#if result && !scanning}
  <div class="summary muted">
    Scanned <b>{result.totalFiles.toLocaleString()}</b> files ({bytes(result.totalBytes)}) in {(result.elapsedMs / 1000).toFixed(1)} s ·
    {files.length} file{files.length === 1 ? "" : "s"} over {SIZES.find((s) => s.v === minSize)?.l}
  </div>

  <div class="layout">
    <section class="files" class:mapview={view === "map"}>
      <div class="viewtabs">
        <button class:on={view === "files"} onclick={() => (view = "files")}>Largest files</button>
        <button class:on={view === "map"} onclick={() => (view = "map")}>Disk map</button>
      </div>
      {#if view === "map"}
        <div class="map"><Treemap root={result.tree} onscan={(p) => scan(p)} /></div>
      {:else}
      <div class="bar">
        <label class="chk"><input type="checkbox" checked={allChecked} onchange={toggleAll} /> {chosen.length ? `${chosen.length} selected · ${bytes(chosenBytes)}` : "Select all"}</label>
        <span class="grow"></span>
        <button disabled={chosen.length !== 1} onclick={() => reveal(chosen[0])}><Icon name="files" size={14} /> Open folder</button>
        <button disabled={!chosen.length || working} onclick={() => (confirm = "trash")}><Icon name="close" size={14} /> Move to Trash</button>
        <button class="danger" disabled={!chosen.length || working} onclick={() => (confirm = "delete")}><Icon name="skull" size={14} /> Delete</button>
      </div>
      <div class="list">
        {#each files as f (f.path)}
          {@const c = catInfo(f.category)}
          <div class="frow" class:sel={selected[f.path]}>
            <input type="checkbox" bind:checked={selected[f.path]} aria-label="Select {f.name}" />
            <span class="dot" style:background={c.color} title={c.label}></span>
            <div class="fmeta" role="button" tabindex="0" ondblclick={() => reveal(f)} onkeydown={(e) => e.key === "Enter" && reveal(f)} title="Double-click to open the folder">
              <div class="fname">{f.name}{#if !f.owned}<span class="badge" title="Owned by another user — deleting needs your password">admin</span>{/if}</div>
              <div class="fpath mono muted">{short(folderOf(f.path))}</div>
            </div>
            <div class="sizebar"><span style:width="{(f.size / maxSize) * 100}%" style:background={c.color}></span></div>
            <span class="fsize num">{bytes(f.size)}</span>
            <span class="fdate muted num">{f.modified ? new Date(f.modified * 1000).toLocaleDateString() : "—"}</span>
          </div>
        {:else}
          <div class="muted pad">No files above the size limit. Try a smaller threshold.</div>
        {/each}
      </div>
      {/if}
    </section>

    <aside class="side">
      <section class="panel">
        <h3>Biggest folders</h3>
        {#each result.folders.slice(0, 10) as d (d.path)}
          <button class="folder" onclick={() => scan(d.path)} title="Scan {d.path}">
            <span class="fl">{d.path.split("/").pop() || d.path}</span>
            <span class="num">{bytes(d.size)}</span>
            <span class="fb"><span style:width="{(d.size / (result.folders[0]?.size || 1)) * 100}%"></span></span>
          </button>
        {/each}
      </section>
      <section class="panel">
        <h3>Space by file type</h3>
        <div class="stack">
          {#each result.categories as c (c.category)}
            {#if c.bytes > 0}<span style:flex-grow={c.bytes} style:background={catInfo(c.category).color} title="{catInfo(c.category).label}: {bytes(c.bytes)}"></span>{/if}
          {/each}
        </div>
        <ul class="cats">
          {#each result.categories.slice(0, 8) as c (c.category)}
            <li><i style:background={catInfo(c.category).color}></i>{catInfo(c.category).label}<span class="grow"></span><span class="num">{bytes(c.bytes)}</span><span class="muted pct num">{((c.bytes / catTotal) * 100).toFixed(0)}%</span></li>
          {/each}
        </ul>
      </section>
      {#if result.subfolders.length}
        <section class="panel">
          <h3>Hot spots</h3>
          {#each result.subfolders.slice(0, 8) as d (d.path)}
            <button class="folder" onclick={() => scan(d.path)} title="Scan {d.path}">
              <span class="fl mono">{short(d.path)}</span><span class="num">{bytes(d.size)}</span>
            </button>
          {/each}
        </section>
      {/if}
    </aside>
  </div>
{:else if !scanning}
  <div class="empty">
    <Icon name="disk" size={36} />
    <p>Choose a folder and press <b>Scan</b>. Scanning your home folder usually takes a few seconds.</p>
  </div>
{/if}

{#if confirm}
  <Modal title={confirm === "trash" ? `Move ${chosen.length} file(s) to Trash?` : `Delete ${chosen.length} file(s) permanently?`} onclose={() => !working && (confirm = null)} width={540}>
    <p>
      {#if confirm === "trash"}
        The files go to the Trash and can be restored from there. Space is freed when the Trash is emptied.
      {:else}
        <b>This cannot be undone.</b> {bytes(chosenBytes)} will be freed immediately.
      {/if}
      {#if chosen.some((f) => !f.owned)}Some files belong to another user; you will be asked for your password.{/if}
    </p>
    <ul class="confirm mono">{#each chosen.slice(0, 12) as f (f.path)}<li>{f.path}</li>{/each}{#if chosen.length > 12}<li>… and {chosen.length - 12} more</li>{/if}</ul>
    {#snippet footer()}
      <button onclick={() => (confirm = null)} disabled={working}>Cancel</button>
      <button class="danger" onclick={() => act(confirm!)} disabled={working}>{working ? "Working…" : confirm === "trash" ? "Move to Trash" : "Delete permanently"}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .controls { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 14px 16px; display: flex; flex-direction: column; gap: 12px; margin-bottom: 14px; }
  .row1, .row2 { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .pathbox { flex: 1; min-width: 260px; display: flex; align-items: center; gap: 8px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: 8px; padding: 0 10px; color: var(--text-muted); }
  .pathbox input { flex: 1; border: none; background: transparent; padding: 7px 0; font-family: var(--font-mono); font-size: 13px; }
  .sel, .chk { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--text-secondary); }
  .chk input { accent-color: var(--accent); }
  .chip { display: inline-flex; align-items: center; gap: 6px; font-size: 12.5px; padding: 4px 10px; border-radius: 99px; background: transparent; color: var(--text-secondary); }
  .chip.on { background: var(--surface-3); color: var(--text-primary); border-color: var(--accent); }
  .chip i, .cats i { width: 8px; height: 8px; border-radius: 50%; display: inline-block; }
  .grow { flex: 1; }
  .progress { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 14px 16px; margin-bottom: 14px; }
  .pbar { height: 4px; background: var(--surface-3); border-radius: 4px; overflow: hidden; position: relative; }
  .pbar span { position: absolute; inset: 0; width: 30%; background: var(--accent); border-radius: 4px; animation: slide 1.4s ease-in-out infinite; }
  @keyframes slide { from { left: -30%; } to { left: 100%; } }
  .pstats { display: flex; gap: 22px; margin-top: 10px; font-size: 13px; }
  .cur { font-size: 11.5px; margin-top: 6px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .summary { font-size: 12.5px; margin-bottom: 10px; }
  .layout { display: grid; grid-template-columns: minmax(0, 1fr) 320px; gap: 14px; align-items: start; }
  .viewtabs { display: flex; gap: 2px; padding: 6px 8px 0; background: var(--surface-2); }
  .viewtabs button { border: none; border-radius: 8px 8px 0 0; background: transparent; color: var(--text-secondary); font-size: 13px; padding: 6px 14px; }
  .viewtabs button.on { background: var(--surface-1); color: var(--text-primary); }
  .map { padding: 12px; height: calc(100vh - 330px); min-height: 460px; }
  .files { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .bar { display: flex; align-items: center; gap: 8px; padding: 10px 14px; border-bottom: 1px solid var(--border); background: var(--surface-2); flex-wrap: wrap; }
  .list { max-height: calc(100vh - 330px); overflow-y: auto; }
  .frow { display: grid; grid-template-columns: 18px 8px minmax(0, 1fr) 140px 84px 84px; gap: 10px; align-items: center; padding: 8px 14px; border-bottom: 1px solid var(--border); }
  .frow:hover { background: var(--surface-2); }
  .frow.sel { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .frow input { accent-color: var(--accent); }
  .dot { width: 8px; height: 8px; border-radius: 50%; }
  .fmeta { min-width: 0; cursor: default; }
  .fname { font-weight: 550; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; display: flex; gap: 6px; align-items: center; }
  .fpath { font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .badge { font-size: 10px; font-weight: 600; color: var(--warning); border: 1px solid currentColor; border-radius: 99px; padding: 0 5px; }
  .sizebar { height: 6px; background: var(--surface-3); border-radius: 6px; overflow: hidden; }
  .sizebar span { display: block; height: 100%; border-radius: 6px; }
  .fsize { text-align: right; font-weight: 600; font-size: 13px; }
  .fdate { text-align: right; font-size: 12px; }
  .pad { padding: 24px; }
  .side { display: flex; flex-direction: column; gap: 14px; }
  .panel { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 14px 16px; }
  .panel h3 { font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); margin-bottom: 10px; }
  .folder { all: unset; cursor: pointer; display: grid; grid-template-columns: 1fr auto; gap: 2px 8px; width: 100%; padding: 5px 0; font-size: 12.5px; box-sizing: border-box; }
  .folder:hover .fl { color: var(--accent); }
  .fl { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fl.mono { font-size: 11.5px; }
  .fb { grid-column: 1 / -1; height: 4px; background: var(--surface-3); border-radius: 4px; overflow: hidden; }
  .fb span { display: block; height: 100%; background: var(--series-4); }
  .stack { display: flex; gap: 2px; height: 12px; border-radius: 6px; overflow: hidden; background: var(--surface-3); }
  .stack span { min-width: 2px; }
  .cats { list-style: none; padding: 0; margin: 10px 0 0; display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; }
  .cats li { display: flex; align-items: center; gap: 8px; }
  .pct { width: 36px; text-align: right; }
  .empty { text-align: center; color: var(--text-secondary); margin-top: 10vh; display: flex; flex-direction: column; align-items: center; gap: 8px; }
  .confirm { font-size: 11.5px; background: var(--surface-2); border-radius: 8px; padding: 8px 10px 8px 26px; max-height: 200px; overflow: auto; color: var(--text-primary); word-break: break-all; }
</style>
