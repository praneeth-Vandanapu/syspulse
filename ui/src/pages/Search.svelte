<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Search.svelte
  Author : Praneeth
  License: MIT
  Purpose: File search with name modes (contains / wildcard / regex / exact),
           filters (type, extension, size, date, owner, hidden) and content
           search. Results stream in live and can be opened, revealed,
           copied or moved to the Trash.
-->
<script lang="ts">
  import { onDestroy } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api, isTauri, onSearchProgress, pickFolder } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import { bytes } from "../lib/format";
  import type { FileCategory, MatchMode, SearchHit, SearchOptions, SearchSummary } from "../lib/types";

  const MODES: { id: MatchMode; label: string; hint: string }[] = [
    { id: "contains", label: "Contains", hint: "report" },
    { id: "glob", label: "Wildcard", hint: "*.pdf   IMG_????.jpg" },
    { id: "regex", label: "Regex", hint: "^invoice_\\d{4}" },
    { id: "exact", label: "Exact", hint: "README.md" },
  ];
  const CATS: { id: FileCategory; label: string; color: string }[] = [
    { id: "document", label: "Documents", color: "var(--series-4)" },
    { id: "image", label: "Images", color: "var(--series-3)" },
    { id: "video", label: "Video", color: "var(--series-1)" },
    { id: "audio", label: "Audio", color: "var(--series-5)" },
    { id: "archive", label: "Archives", color: "var(--series-2)" },
    { id: "code", label: "Code & data", color: "var(--series-8)" },
    { id: "package", label: "Installers", color: "var(--series-6)" },
    { id: "diskimage", label: "Disk images", color: "var(--series-7)" },
  ];
  const DATES = [
    { id: "any", label: "Any time", days: 0 },
    { id: "1", label: "Today", days: 1 },
    { id: "7", label: "Past 7 days", days: 7 },
    { id: "30", label: "Past 30 days", days: 30 },
    { id: "365", label: "Past year", days: 365 },
    { id: "older", label: "Older than a year", days: -365 },
  ];
  const RECENT_KEY = "syspulse.search.recent";
  const PAGE = 200;

  let query = $state("");
  let mode = $state<MatchMode>("contains");
  let caseSensitive = $state(false);
  let matchPath = $state(false);
  let root = $state("~");
  let kind = $state<"any" | "files" | "folders">("any");
  let cats = $state<Record<string, boolean>>({});
  let exts = $state("");
  let minMb = $state<number | null>(null);
  let maxMb = $state<number | null>(null);
  let date = $state("any");
  let owner = $state<"any" | "me" | "others">("any");
  let includeHidden = $state(false);
  let skipNoise = $state(true);
  let sameFs = $state(true);
  let content = $state("");
  let showFilters = $state(false);

  let running = $state(false);
  let hits = $state<SearchHit[]>([]);
  let summary = $state<SearchSummary | null>(null);
  let scanned = $state(0);
  let current = $state("");
  let selected = $state<Record<string, boolean>>({});
  let sort = $state<"relevance" | "name" | "size" | "date">("relevance");
  let limit = $state(PAGE);
  let confirmTrash = $state(false);
  let unlisten: (() => void) | null = null;

  let recent = $state<string[]>(loadRecent());
  function loadRecent(): string[] {
    try {
      return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    } catch {
      return [];
    }
  }
  function remember(q: string) {
    if (!q) return;
    recent = [q, ...recent.filter((r) => r !== q)].slice(0, 8);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(recent));
    } catch {
      /* storage unavailable */
    }
  }

  onDestroy(() => {
    unlisten?.();
    if (running) api.searchCancel();
  });

  let activeFilters = $derived(
    [kind !== "any", Object.values(cats).some(Boolean), !!exts.trim(), minMb != null, maxMb != null, date !== "any", owner !== "any", includeHidden, !skipNoise, !!content.trim()].filter(Boolean).length,
  );

  function options(): SearchOptions {
    const now = Date.now() / 1000;
    const d = DATES.find((x) => x.id === date)!;
    return {
      root,
      query: query.trim(),
      mode,
      caseSensitive,
      matchPath,
      kind,
      categories: Object.entries(cats).filter(([, v]) => v).map(([k]) => k as FileCategory),
      extensions: exts.split(/[,\s]+/).map((e) => e.replace(/^\./, "")).filter(Boolean),
      minSize: minMb != null ? Math.round(minMb * 2 ** 20) : null,
      maxSize: maxMb != null ? Math.round(maxMb * 2 ** 20) : null,
      modifiedAfter: d.days > 0 ? Math.floor(now - d.days * 86400) : null,
      modifiedBefore: d.days < 0 ? Math.floor(now + d.days * 86400) : null,
      owner,
      includeHidden,
      skipNoise,
      sameFilesystem: sameFs,
      content: content.trim() || null,
      maxResults: 5000,
    };
  }

  async function run() {
    if (running) return;
    const o = options();
    if (!o.query && !o.content && activeFilters === 0) {
      app.notify("Type a name, some text to find inside files, or choose a filter", "info");
      return;
    }
    running = true;
    hits = [];
    summary = null;
    selected = {};
    limit = PAGE;
    scanned = 0;
    remember(o.query);
    unlisten?.();
    unlisten = await onSearchProgress((p) => {
      if (p.batch.length) hits = [...hits, ...p.batch];
      scanned = p.scanned;
      current = p.current;
    });
    try {
      summary = await api.searchFiles(o);
      hits = summary.hits;
      scanned = summary.scanned;
      if (summary.truncated) app.notify(`Showing the first ${summary.hits.length} matches — refine the search for more`, "info");
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      running = false;
      unlisten?.();
      unlisten = null;
    }
  }

  let sorted = $derived.by(() => {
    const list = [...hits];
    if (sort === "name") list.sort((a, b) => a.name.localeCompare(b.name));
    if (sort === "size") list.sort((a, b) => (b.size ?? -1) - (a.size ?? -1));
    if (sort === "date") list.sort((a, b) => (b.modified ?? 0) - (a.modified ?? 0));
    return list;
  });
  let chosen = $derived(hits.filter((h) => selected[h.path]));

  /** Split a name around the first case-insensitive occurrence of the query (contains mode). */
  function parts(name: string): [string, string, string] {
    if (mode !== "contains" || !query.trim() || matchPath) return [name, "", ""];
    const q = query.trim();
    const i = caseSensitive ? name.indexOf(q) : name.toLowerCase().indexOf(q.toLowerCase());
    return i < 0 ? [name, "", ""] : [name.slice(0, i), name.slice(i, i + q.length), name.slice(i + q.length)];
  }
  const folderOf = (p: string) => p.slice(0, p.lastIndexOf("/")) || "/";
  const catColor = (h: SearchHit) => (h.isDir ? "var(--accent)" : (CATS.find((c) => c.id === h.category)?.color ?? "var(--text-muted)"));

  function open(h: SearchHit) {
    api.openPath(h.path).catch((e) => app.notify(String(e), "error"));
  }
  function reveal(h: SearchHit) {
    api.revealPath(h.path).catch((e) => app.notify(String(e), "error"));
  }
  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      app.notify("Copied to clipboard", "info", 2000);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }
  async function trash() {
    const r = await api.filesTrash(chosen.map((h) => h.path)).catch((e) => {
      app.notify(String(e), "error");
      return null;
    });
    confirmTrash = false;
    if (!r) return;
    if (r.done.length) {
      app.notify(`Moved ${r.done.length} item(s) to Trash`);
      const gone = new Set(r.done);
      hits = hits.filter((h) => !gone.has(h.path));
      selected = {};
    }
    for (const e of [...r.errors, ...r.denied.map((d) => `${d}: permission denied`)]) app.notify(e, "error", 8000);
  }
  async function browse() {
    const p = await pickFolder(root === "~" ? undefined : root);
    if (p) root = p;
  }
  function resetFilters() {
    kind = "any";
    cats = {};
    exts = "";
    minMb = maxMb = null;
    date = "any";
    owner = "any";
    includeHidden = false;
    skipNoise = true;
    content = "";
  }
</script>

<PageHeader title="File Search" subtitle="Find files and folders by name, type, size, date or content" />

<section class="bar">
  <div class="q">
    <Icon name="search" size={18} />
    <input
      class="qi"
      placeholder={MODES.find((m) => m.id === mode)?.hint}
      bind:value={query}
      onkeydown={(e) => e.key === "Enter" && run()}
      aria-label="File name to search for"
      spellcheck="false"
    />
    <button class="tog" class:on={caseSensitive} onclick={() => (caseSensitive = !caseSensitive)} title="Match case" aria-pressed={caseSensitive}>Aa</button>
    <button class="tog" class:on={matchPath} onclick={() => (matchPath = !matchPath)} title="Match the whole path, not just the name" aria-pressed={matchPath}>/path</button>
  </div>
  <div class="seg" role="radiogroup" aria-label="Match mode">
    {#each MODES as m (m.id)}<button class:on={mode === m.id} onclick={() => (mode = m.id)}>{m.label}</button>{/each}
  </div>
  {#if running}
    <button onclick={() => api.searchCancel()}><Icon name="stop" size={14} /> Stop</button>
  {:else}
    <button class="primary" onclick={run}><Icon name="search" size={14} /> Search</button>
  {/if}
</section>

<section class="where">
  <span class="muted">In</span>
  <div class="pathbox"><input bind:value={root} aria-label="Folder to search" spellcheck="false" /></div>
  {#if isTauri}<button class="small" onclick={browse}>Browse…</button>{/if}
  <button class="ghost small" onclick={() => (root = "~")}>Home</button>
  <button class="ghost small" onclick={() => (root = "/")}>Whole computer</button>
  <span class="grow"></span>
  <button class="small" class:on={showFilters} onclick={() => (showFilters = !showFilters)}>
    <Icon name="settings" size={13} /> Filters{activeFilters ? ` (${activeFilters})` : ""}
  </button>
</section>

{#if showFilters}
  <section class="filters">
    <div class="frow">
      <label>Type
        <select bind:value={kind}><option value="any">Files and folders</option><option value="files">Files only</option><option value="folders">Folders only</option></select>
      </label>
      <label>Extensions <input placeholder="pdf, docx, mp4" bind:value={exts} /></label>
      <label>Size (MB) <input type="number" min="0" placeholder="min" bind:value={minMb} class="num-in" /> – <input type="number" min="0" placeholder="max" bind:value={maxMb} class="num-in" /></label>
      <label>Modified <select bind:value={date}>{#each DATES as d (d.id)}<option value={d.id}>{d.label}</option>{/each}</select></label>
      <label>Owner <select bind:value={owner}><option value="any">Anyone</option><option value="me">Me</option><option value="others">Others</option></select></label>
    </div>
    <div class="frow">
      {#each CATS as c (c.id)}
        <button class="chip" class:on={cats[c.id]} onclick={() => (cats[c.id] = !cats[c.id])} aria-pressed={!!cats[c.id]}><i style:background={c.color}></i>{c.label}</button>
      {/each}
    </div>
    <div class="frow">
      <label class="grow">Containing text <input class="wide" placeholder="Search inside text files (slower)" bind:value={content} onkeydown={(e) => e.key === "Enter" && run()} /></label>
      <label class="chk"><input type="checkbox" bind:checked={includeHidden} /> Hidden files</label>
      <label class="chk"><input type="checkbox" bind:checked={skipNoise} /> Skip .git / node_modules</label>
      <label class="chk"><input type="checkbox" bind:checked={sameFs} /> Stay on this disk</label>
      <button class="ghost small" onclick={resetFilters}>Reset</button>
    </div>
  </section>
{/if}

{#if running}
  <div class="status"><span class="spin"></span> {hits.length.toLocaleString()} found · {scanned.toLocaleString()} scanned <span class="mono muted cur">{current}</span></div>
{:else if summary}
  <div class="status muted">
    {hits.length.toLocaleString()} result{hits.length === 1 ? "" : "s"} · {summary.scanned.toLocaleString()} items scanned in {(summary.elapsedMs / 1000).toFixed(1)} s{summary.cancelled ? " · stopped" : ""}
  </div>
{/if}

{#if hits.length}
  <section class="results">
    <div class="rbar">
      <span class="muted">{chosen.length ? `${chosen.length} selected` : ""}</span>
      <span class="grow"></span>
      <button class="small" disabled={chosen.length !== 1} onclick={() => open(chosen[0])}>Open</button>
      <button class="small" disabled={chosen.length !== 1} onclick={() => reveal(chosen[0])}>Show in folder</button>
      <button class="small" disabled={!chosen.length} onclick={() => copy(chosen.map((h) => h.path).join("\n"))}>Copy path</button>
      <button class="small danger" disabled={!chosen.length} onclick={() => (confirmTrash = true)}>Move to Trash</button>
      <label class="sort">Sort
        <select bind:value={sort}><option value="relevance">Found order</option><option value="name">Name</option><option value="size">Size</option><option value="date">Modified</option></select>
      </label>
    </div>
    <div class="list">
      {#each sorted.slice(0, limit) as h (h.path)}
        {@const [a, m, b] = parts(h.name)}
        <div class="hit" class:sel={selected[h.path]} role="button" tabindex="0"
          ondblclick={() => (h.isDir ? (root = h.path) : open(h))}
          onkeydown={(e) => e.key === "Enter" && open(h)}>
          <input type="checkbox" bind:checked={selected[h.path]} aria-label="Select {h.name}" />
          <span class="ic" style:color={catColor(h)}><Icon name={h.isDir ? "files" : "chip"} size={15} /></span>
          <div class="meta">
            <div class="nm">{a}<mark>{m}</mark>{b}{#if h.isDir}<span class="muted">/</span>{/if}</div>
            <div class="path mono muted">{folderOf(h.path)}</div>
            {#if h.content}
              <div class="snip mono"><span class="ln">{h.content.line}:</span> {h.content.text}{#if h.content.matches > 1}<span class="muted"> (+{h.content.matches - 1} more lines)</span>{/if}</div>
            {/if}
          </div>
          <span class="sz num">{h.isDir ? "Folder" : bytes(h.size)}</span>
          <span class="dt num muted">{h.modified ? new Date(h.modified * 1000).toLocaleDateString() : "—"}</span>
          <span class="ow muted">{h.owner}</span>
          <div class="acts">
            <button class="ghost tiny" onclick={() => open(h)} title="Open">Open</button>
            <button class="ghost tiny" onclick={() => reveal(h)} title="Show in folder"><Icon name="files" size={13} /></button>
          </div>
        </div>
      {/each}
      {#if sorted.length > limit}
        <button class="more" onclick={() => (limit += PAGE)}>Show more ({sorted.length - limit} remaining)</button>
      {/if}
    </div>
  </section>
{:else if !running}
  <div class="empty">
    <Icon name="search" size={34} />
    {#if summary}
      <p>No matches. Try another mode, fewer filters, or include hidden files.</p>
    {:else}
      <p>Search by name with <b>Contains</b>, <b>Wildcard</b> (<span class="mono">*.mp4</span>) or <b>Regex</b>, and narrow down with <b>Filters</b>.</p>
      {#if recent.length}
        <div class="recent">
          <span class="muted">Recent:</span>
          {#each recent as r (r)}<button class="chip" onclick={() => { query = r; run(); }}>{r}</button>{/each}
        </div>
      {/if}
    {/if}
  </div>
{/if}

{#if confirmTrash}
  <Modal title="Move {chosen.length} item(s) to Trash?" onclose={() => (confirmTrash = false)}>
    They can be restored from the Trash.
    <ul class="confirm mono">{#each chosen.slice(0, 10) as h (h.path)}<li>{h.path}</li>{/each}{#if chosen.length > 10}<li>… and {chosen.length - 10} more</li>{/if}</ul>
    {#snippet footer()}
      <button onclick={() => (confirmTrash = false)}>Cancel</button>
      <button class="danger" onclick={trash}>Move to Trash</button>
    {/snippet}
  </Modal>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .bar { display: flex; gap: 10px; align-items: center; margin-bottom: 10px; flex-wrap: wrap; }
  .q { flex: 1; min-width: 320px; display: flex; align-items: center; gap: 8px; background: var(--surface-1); border: 1px solid var(--border-strong);
    border-radius: 12px; padding: 4px 6px 4px 14px; color: var(--text-muted); }
  .q:focus-within { border-color: var(--accent); }
  .qi { flex: 1; border: none; background: transparent; font-size: 16px; padding: 8px 0; }
  .tog { font-size: 12px; padding: 4px 8px; font-family: var(--font-mono); background: transparent; border-color: transparent; color: var(--text-muted); }
  .tog.on, button.on { color: var(--text-primary); border-color: var(--accent); background: color-mix(in srgb, var(--accent) 16%, transparent); }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 5px 11px; border-radius: 7px; color: var(--text-secondary); font-size: 13px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .where { display: flex; gap: 8px; align-items: center; margin-bottom: 12px; font-size: 13px; flex-wrap: wrap; }
  .pathbox { min-width: 260px; flex: 0 1 420px; }
  .pathbox input { width: 100%; font-family: var(--font-mono); font-size: 12.5px; }
  .grow { flex: 1; }
  .small { font-size: 12.5px; padding: 4px 10px; }
  .tiny { font-size: 11.5px; padding: 2px 6px; }
  .filters { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 12px 14px; display: flex; flex-direction: column; gap: 10px; margin-bottom: 12px; }
  .frow { display: flex; gap: 14px; align-items: center; flex-wrap: wrap; font-size: 13px; color: var(--text-secondary); }
  .frow label { display: inline-flex; gap: 6px; align-items: center; }
  .num-in { width: 80px; }
  .wide { flex: 1; min-width: 240px; }
  label.grow { display: flex; }
  .chk input { accent-color: var(--accent); }
  .chip { display: inline-flex; align-items: center; gap: 6px; font-size: 12.5px; padding: 3px 10px; border-radius: 99px; background: transparent; color: var(--text-secondary); }
  .chip i { width: 8px; height: 8px; border-radius: 50%; }
  .status { font-size: 12.5px; margin-bottom: 8px; display: flex; gap: 8px; align-items: center; white-space: nowrap; overflow: hidden; }
  .cur { overflow: hidden; text-overflow: ellipsis; font-size: 11.5px; }
  .spin { width: 12px; height: 12px; border: 2px solid var(--surface-3); border-top-color: var(--accent); border-radius: 50%; animation: sp 0.8s linear infinite; flex: none; }
  @keyframes sp { to { transform: rotate(360deg); } }
  .results { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .rbar { display: flex; gap: 8px; align-items: center; padding: 8px 12px; background: var(--surface-2); border-bottom: 1px solid var(--border); flex-wrap: wrap; font-size: 12.5px; }
  .sort { display: inline-flex; gap: 6px; align-items: center; color: var(--text-secondary); }
  .list { max-height: calc(100vh - 300px); overflow-y: auto; }
  .hit { display: grid; grid-template-columns: 18px 18px minmax(0, 1fr) 84px 90px 80px 90px; gap: 10px; align-items: center; padding: 7px 12px; border-bottom: 1px solid var(--border); }
  .hit:hover { background: var(--surface-2); }
  .hit.sel { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .hit input { accent-color: var(--accent); }
  .ic { display: inline-flex; }
  .meta { min-width: 0; }
  .nm { font-weight: 550; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  mark { background: color-mix(in srgb, var(--warning) 35%, transparent); color: inherit; border-radius: 3px; padding: 0 1px; }
  .path { font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .snip { font-size: 11.5px; margin-top: 3px; padding: 3px 6px; background: var(--surface-2); border-radius: 5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--text-secondary); }
  .ln { color: var(--accent); }
  .sz { text-align: right; font-size: 12.5px; }
  .dt, .ow { font-size: 12px; text-align: right; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .acts { display: flex; gap: 2px; justify-content: flex-end; opacity: 0; }
  .hit:hover .acts { opacity: 1; }
  .more { width: 100%; border: none; border-radius: 0; padding: 12px; background: transparent; color: var(--accent); }
  .empty { text-align: center; color: var(--text-secondary); margin-top: 8vh; display: flex; flex-direction: column; align-items: center; gap: 8px; }
  .recent { display: flex; gap: 6px; flex-wrap: wrap; justify-content: center; align-items: center; font-size: 12.5px; }
  .confirm { font-size: 11.5px; background: var(--surface-2); border-radius: 8px; padding: 8px 10px 8px 26px; max-height: 200px; overflow: auto; color: var(--text-primary); word-break: break-all; }
</style>
