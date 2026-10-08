<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/Processes.svelte
  Author : Praneeth
  License: MIT
  Purpose: Process manager: searchable, sortable process table (flat or tree)
           with filters, a details panel (open files, network connections),
           priority control and End / Kill / Stop / Continue actions.
-->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/stores.svelte";
  import { bytes, dateTime, duration, pct, rate } from "../lib/format";
  import type { ProcessDetails, ProcessInfo, ProcessResources, Signal } from "../lib/types";

  const PRIORITIES = [
    { nice: -20, label: "Very high" },
    { nice: -5, label: "High" },
    { nice: 0, label: "Normal" },
    { nice: 5, label: "Low" },
    { nice: 19, label: "Very low" },
  ];

  type SortKey = "name" | "pid" | "user" | "cpu" | "memory" | "disk" | "threads" | "startTime";
  const COLS: { key: SortKey; label: string; right?: boolean }[] = [
    { key: "name", label: "Name" },
    { key: "pid", label: "PID", right: true },
    { key: "user", label: "User" },
    { key: "cpu", label: "CPU", right: true },
    { key: "memory", label: "Memory", right: true },
    { key: "disk", label: "Disk R/W", right: true },
    { key: "threads", label: "Threads", right: true },
    { key: "startTime", label: "Started", right: true },
  ];

  let procs = $state<ProcessInfo[]>([]);
  let query = $state("");
  let filter = $state<"all" | "mine" | "active">("all");
  let sortKey = $state<SortKey>("cpu");
  let desc = $state(true);
  let paused = $state(false);
  let selected = $state<number | null>(null);
  let details = $state<ProcessDetails | null>(null);
  let confirm = $state<{ pid: number; name: string; signal: Signal } | null>(null);
  let tree = $state(false);
  let collapsed = $state<Record<number, boolean>>({});
  let resources = $state<ProcessResources | null>(null);
  let resTab = $state<"connections" | "files">("connections");
  let timer: ReturnType<typeof setInterval>;

  let cores = $derived(app.summary?.logicalCores ?? 1);
  let me = $derived(app.summary?.user ?? "");

  async function load() {
    if (paused) return;
    try {
      procs = await api.listProcesses();
      if (selected != null) details = await api.processDetails(selected).catch(() => null);
    } catch (e) {
      show(String(e), true);
    }
  }

  onMount(() => {
    load();
    timer = setInterval(load, 2000);
  });
  onDestroy(() => clearInterval(timer));

  function cpuOf(p: ProcessInfo) {
    return app.settings.normalizeProcessCpu ? p.cpu / cores : p.cpu;
  }

  function value(p: ProcessInfo, k: SortKey): number | string {
    switch (k) {
      case "cpu": return p.cpu;
      case "disk": return p.diskReadBps + p.diskWriteBps;
      case "threads": return p.threads ?? 0;
      case "name": return p.name.toLowerCase();
      case "user": return p.user;
      default: return p[k];
    }
  }

  let rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = procs.filter((p) => {
      if (filter === "mine" && p.user !== me) return false;
      if (filter === "active" && p.cpu < 0.5 && p.diskReadBps + p.diskWriteBps < 1) return false;
      if (!q) return true;
      return p.name.toLowerCase().includes(q) || String(p.pid) === q || p.command.toLowerCase().includes(q) || p.user.toLowerCase() === q;
    });
    const dir = desc ? -1 : 1;
    const cmp = (a: ProcessInfo, b: ProcessInfo) => {
      const va = value(a, sortKey);
      const vb = value(b, sortKey);
      return va < vb ? -dir : va > vb ? dir : a.pid - b.pid;
    };
    if (!tree || q || filter !== "all") return list.sort(cmp).map((p) => ({ p, depth: 0, kids: 0 }));

    // Tree view: depth-first over parent links, siblings sorted by the active column.
    const byPid = new Map(list.map((p) => [p.pid, p]));
    const children = new Map<number, ProcessInfo[]>();
    const roots: ProcessInfo[] = [];
    for (const p of list) {
      if (p.ppid != null && byPid.has(p.ppid)) {
        if (!children.has(p.ppid)) children.set(p.ppid, []);
        children.get(p.ppid)!.push(p);
      } else roots.push(p);
    }
    const out: { p: ProcessInfo; depth: number; kids: number }[] = [];
    const walk = (nodes: ProcessInfo[], depth: number) => {
      for (const p of nodes.sort(cmp)) {
        const kids = children.get(p.pid) ?? [];
        out.push({ p, depth, kids: kids.length });
        if (!collapsed[p.pid]) walk(kids, depth + 1);
      }
    };
    walk(roots, 0);
    return out;
  });

  let totals = $derived({
    cpu: procs.reduce((a, p) => a + p.cpu, 0) / cores,
    mem: procs.reduce((a, p) => a + p.memory, 0),
  });

  function sortBy(k: SortKey) {
    if (sortKey === k) desc = !desc;
    else {
      sortKey = k;
      desc = !["name", "user", "pid"].includes(k);
    }
  }

  async function select(pid: number) {
    selected = pid;
    resources = null;
    details = await api.processDetails(pid).catch((e) => {
      show(String(e), true);
      return null;
    });
    loadResources();
  }

  async function loadResources() {
    if (selected == null) return;
    resources = await api.processResources(selected).catch(() => null);
  }

  async function setPriority(nice: number) {
    if (!details) return;
    try {
      const elevated = await api.setProcessPriority(details.info.pid, nice);
      show(`Priority of ${details.info.name} set to ${PRIORITIES.find((x) => x.nice === nice)?.label ?? nice} (nice ${nice})${elevated ? " as administrator" : ""}`, false);
      load();
    } catch (e) {
      show(String(e), true);
    }
  }

  function ask(signal: Signal) {
    if (!details) return;
    if (signal === "term" || signal === "cont") return send(details.info.pid, signal);
    confirm = { pid: details.info.pid, name: details.info.name, signal };
  }

  async function send(pid: number, signal: Signal) {
    confirm = null;
    try {
      const r = await api.signalProcess(pid, signal);
      const verb = { term: "End request sent to", kill: "Killed", stop: "Stopped", cont: "Resumed", hup: "Hang-up sent to", int: "Interrupted" }[signal];
      show(`${verb} PID ${pid}${r.elevated ? " (as administrator)" : ""}`, false);
      if (signal === "kill" || signal === "term") {
        setTimeout(load, 400);
      } else load();
    } catch (e) {
      show(String(e), true);
    }
  }

  function show(text: string, error: boolean) {
    app.notify(text, error ? "error" : "ok");
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Delete" && details && !confirm && document.activeElement?.tagName !== "INPUT") ask("term");
    if (e.key === "Escape") {
      if (confirm) confirm = null;
      else selected = null, (details = null);
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<PageHeader title="Processes" subtitle="{procs.length} processes · CPU {pct(totals.cpu, 0)} · Memory {bytes(totals.mem)}">
  {#snippet actions()}
    <div class="search">
      <Icon name="search" size={15} />
      <input placeholder="Search name, PID, command…" bind:value={query} aria-label="Search processes" />
    </div>
    <div class="seg" role="radiogroup" aria-label="Filter">
      <button class:on={filter === "all"} onclick={() => (filter = "all")}>All</button>
      <button class:on={filter === "mine"} onclick={() => (filter = "mine")}>My processes</button>
      <button class:on={filter === "active"} onclick={() => (filter = "active")}>Active</button>
    </div>
    <button class:on={tree} onclick={() => (tree = !tree)} title="Show parent/child hierarchy" aria-pressed={tree}>
      <Icon name="process" size={14} /> Tree
    </button>
    <button onclick={() => (paused = !paused)} title={paused ? "Resume updates" : "Pause updates"}>
      <Icon name={paused ? "play" : "stop"} size={14} /> {paused ? "Resume" : "Pause"}
    </button>
  {/snippet}
</PageHeader>

<div class="layout" class:withside={details}>
  <div class="tablewrap">
    <table>
      <thead>
        <tr>
          {#each COLS as c (c.key)}
            <th class:r={c.right} class:sorted={sortKey === c.key}>
              <button class="th" onclick={() => sortBy(c.key)}>
                {c.label}{#if sortKey === c.key}<span class="arrow">{desc ? "▾" : "▴"}</span>{/if}
              </button>
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each rows as { p, depth, kids } (p.pid)}
          {@const c = cpuOf(p)}
          <tr class:sel={selected === p.pid} onclick={() => select(p.pid)}>
            <td class="name" title={p.command} style:padding-left="{12 + depth * 16}px">
              {#if kids}
                <button class="twisty" onclick={(e) => { e.stopPropagation(); collapsed[p.pid] = !collapsed[p.pid]; }} aria-label={collapsed[p.pid] ? "Expand" : "Collapse"}>{collapsed[p.pid] ? "▸" : "▾"}</button>
              {:else if tree}<span class="twisty-sp"></span>{/if}
              <span class="dot" class:run={p.state === "Runnable"} class:zombie={p.state === "Zombie"}></span>{p.name}
            </td>
            <td class="r num muted">{p.pid}</td>
            <td class="muted">{p.user}</td>
            <td class="r num">
              <span class="cpu" style:--w="{Math.min(100, app.settings.normalizeProcessCpu ? c : c / cores)}%">{c.toFixed(1)}%</span>
            </td>
            <td class="r num">{bytes(p.memory)}</td>
            <td class="r num muted">{p.diskReadBps + p.diskWriteBps > 0 ? `${rate(p.diskReadBps)} / ${rate(p.diskWriteBps)}` : "—"}</td>
            <td class="r num muted">{p.threads ?? "—"}</td>
            <td class="r num muted">{new Date(p.startTime * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if !rows.length}<div class="empty muted">No processes match.</div>{/if}
  </div>

  {#if details}
    {@const d = details}
    <aside class="side">
      <div class="sh">
        <div>
          <h2>{d.info.name}</h2>
          <div class="muted">PID {d.info.pid} · {d.info.user} · {d.info.state}</div>
        </div>
        <button class="ghost" onclick={() => ((selected = null), (details = null))} aria-label="Close details"><Icon name="close" size={16} /></button>
      </div>

      <div class="actions">
        <button onclick={() => ask("term")} title="Ask the process to exit (SIGTERM) — Delete key"><Icon name="close" size={14} /> End</button>
        <button class="danger" onclick={() => ask("kill")} title="Force kill (SIGKILL)"><Icon name="skull" size={14} /> Kill</button>
        {#if d.info.state === "Stopped"}
          <button onclick={() => ask("cont")} title="Continue (SIGCONT)"><Icon name="play" size={14} /> Continue</button>
        {:else}
          <button onclick={() => ask("stop")} title="Pause (SIGSTOP)"><Icon name="stop" size={14} /> Stop</button>
        {/if}
      </div>

      <label class="prio">
        <span class="muted">Priority</span>
        <select value={PRIORITIES.reduce((best, x) => (Math.abs(x.nice - (d.info.nice ?? 0)) < Math.abs(best.nice - (d.info.nice ?? 0)) ? x : best)).nice}
          onchange={(e) => setPriority(Number((e.currentTarget as HTMLSelectElement).value))}>
          {#each PRIORITIES as pr (pr.nice)}<option value={pr.nice}>{pr.label} (nice {pr.nice})</option>{/each}
        </select>
      </label>

      <dl>
        <dt>CPU</dt><dd class="num">{pct(cpuOf(d.info), 1)}</dd>
        <dt>Memory (RSS)</dt><dd class="num">{bytes(d.info.memory)}</dd>
        <dt>Virtual memory</dt><dd class="num">{bytes(d.info.virtualMemory)}</dd>
        <dt>Disk read / write</dt><dd class="num">{rate(d.info.diskReadBps)} / {rate(d.info.diskWriteBps)}</dd>
        <dt>Threads</dt><dd>{d.info.threads ?? "—"}</dd>
        <dt>Nice</dt><dd>{d.info.nice ?? "—"}</dd>
        <dt>Parent PID</dt><dd>
          {#if d.info.ppid}<button class="link" onclick={() => select(d.info.ppid!)}>{d.info.ppid}</button>{:else}—{/if}
        </dd>
        <dt>Children</dt><dd>{d.children.length ? d.children.slice(0, 12).join(", ") + (d.children.length > 12 ? " …" : "") : "—"}</dd>
        <dt>Started</dt><dd>{dateTime(d.info.startTime)} ({duration(Date.now() / 1000 - d.info.startTime)} ago)</dd>
        <dt>Open files</dt><dd>{d.openFiles ?? "—"}</dd>
        <dt>UID / GID</dt><dd>{d.uid ?? "—"} / {d.gid ?? "—"}</dd>
        <dt>Executable</dt><dd class="mono wrap">{d.info.exe ?? "—"}</dd>
        <dt>Working dir</dt><dd class="mono wrap">{d.cwd ?? "—"}</dd>
        <dt>cgroup</dt><dd class="mono wrap">{d.cgroup ?? "—"}</dd>
        <dt>Environment</dt><dd>{d.environmentCount != null ? `${d.environmentCount} variables` : "—"}</dd>
      </dl>
      <h3 class="cap">Command line</h3>
      <pre class="mono">{d.cmdline.join(" ") || d.info.name}</pre>
      <div class="restabs">
        <button class:on={resTab === "connections"} onclick={() => (resTab = "connections")}>Connections {resources ? `(${resources.connections.length})` : ""}</button>
        <button class:on={resTab === "files"} onclick={() => (resTab = "files")}>Open files {resources ? `(${resources.totalFds})` : ""}</button>
        <button class="ghost" onclick={loadResources} aria-label="Refresh"><Icon name="refresh" size={13} /></button>
      </div>
      {#if !resources}
        <div class="muted small">Loading…</div>
      {:else if resources.permissionDenied}
        <div class="muted small">Belongs to another user — open files and connections are not readable without admin rights.</div>
      {:else if resTab === "connections"}
        {#if resources.connections.length}
          <table class="res">
            <thead><tr><th>Proto</th><th>Local</th><th>Remote</th><th>State</th></tr></thead>
            <tbody>
              {#each resources.connections as cn, i (i)}
                <tr><td>{cn.protocol}</td><td class="mono">{cn.local}</td><td class="mono">{cn.remote}</td><td class:listen={cn.state === "LISTEN"}>{cn.state}</td></tr>
              {/each}
            </tbody>
          </table>
        {:else}<div class="muted small">No network sockets.</div>{/if}
      {:else}
        <ul class="fds">
          {#each resources.files as f (f.fd)}
            <li><span class="fd num">{f.fd}</span><span class="kind {f.kind}">{f.kind}</span><span class="mono t">{f.target}</span></li>
          {/each}
        </ul>
      {/if}

      {#if d.status.length}
        <h3 class="cap">Kernel status</h3>
        <dl>
          {#each d.status as [k, v] (k)}<dt>{k}</dt><dd class="mono">{v}</dd>{/each}
        </dl>
      {/if}
    </aside>
  {/if}
</div>

{#if confirm}
  {@const cf = confirm}
  <Modal title="{cf.signal === 'kill' ? 'Kill' : 'Stop'} “{cf.name}”?" onclose={() => (confirm = null)} width={420}>
    {#if cf.signal === "kill"}
      The process (PID {cf.pid}) will be terminated immediately. Unsaved data will be lost.
    {:else}
      The process (PID {cf.pid}) will be paused until you continue it.
    {/if}
    {#snippet footer()}
      <button onclick={() => (confirm = null)}>Cancel</button>
      <button class={cf.signal === "kill" ? "danger" : "primary"} onclick={() => send(cf.pid, cf.signal)}>
        {cf.signal === "kill" ? "Kill process" : "Stop process"}
      </button>
    {/snippet}
  </Modal>
{/if}

<style>
  .search { display: flex; align-items: center; gap: 6px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: 8px; padding: 0 10px; color: var(--text-muted); }
  .search input { border: none; background: transparent; width: 220px; padding: 7px 0; }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 4px 12px; border-radius: 7px; color: var(--text-secondary); font-size: 13px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  button :global(svg) { vertical-align: -2px; }

  .layout { display: grid; grid-template-columns: 1fr; gap: 14px; height: calc(100vh - 130px); }
  .layout.withside { grid-template-columns: minmax(0, 1fr) 380px; }
  .tablewrap { overflow: auto; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  thead th { position: sticky; top: 0; background: var(--surface-2); z-index: 1; text-align: left; font-weight: 500; border-bottom: 1px solid var(--border); }
  .th { all: unset; cursor: pointer; display: block; padding: 9px 12px; color: var(--text-secondary); font-size: 12px; }
  th.r .th { text-align: right; }
  th.sorted .th { color: var(--text-primary); }
  .arrow { margin-left: 4px; color: var(--accent); }
  td { padding: 6px 12px; border-bottom: 1px solid var(--border); white-space: nowrap; }
  tbody tr { cursor: default; }
  tbody tr:hover { background: var(--surface-2); }
  tbody tr.sel { background: color-mix(in srgb, var(--accent) 16%, transparent); }
  .r { text-align: right; }
  .name { max-width: 280px; overflow: hidden; text-overflow: ellipsis; font-weight: 500; }
  .dot { display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: var(--surface-3); margin-right: 8px; vertical-align: 1px; }
  .dot.run { background: var(--good); }
  .dot.zombie { background: var(--critical); }
  .cpu { position: relative; display: inline-block; min-width: 56px; padding: 1px 6px; border-radius: 4px; z-index: 0; }
  .cpu::before { content: ""; position: absolute; inset: 0; right: auto; width: var(--w); background: color-mix(in srgb, var(--series-1) 30%, transparent); border-radius: 4px; z-index: -1; }
  .empty { padding: 30px; text-align: center; }

  .side { overflow-y: auto; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--radius); padding: 16px 18px; }
  .sh { display: flex; justify-content: space-between; align-items: flex-start; gap: 8px; }
  .sh h2 { word-break: break-all; }
  .sh .muted { font-size: 12px; margin-top: 4px; }
  .actions { display: flex; gap: 8px; margin: 14px 0; }
  dl { display: grid; grid-template-columns: 120px 1fr; gap: 7px 10px; margin: 0; font-size: 12.5px; }
  dt { color: var(--text-muted); }
  dd { margin: 0; min-width: 0; }
  .wrap { word-break: break-all; }
  .cap { margin: 16px 0 8px; font-size: 11.5px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  pre { margin: 0; white-space: pre-wrap; word-break: break-all; background: var(--surface-2); padding: 10px; border-radius: 8px; font-size: 12px; }
  .link { all: unset; color: var(--accent); cursor: pointer; }

  .twisty { all: unset; cursor: pointer; width: 14px; display: inline-block; color: var(--text-muted); font-size: 11px; }
  .twisty-sp { display: inline-block; width: 14px; }
  button.on { border-color: var(--accent); color: var(--text-primary); background: color-mix(in srgb, var(--accent) 16%, transparent); }
  .prio { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 14px; font-size: 12.5px; }
  .prio select { font-size: 12.5px; }
  .restabs { display: flex; gap: 6px; margin: 18px 0 8px; align-items: center; }
  .restabs button { font-size: 12px; padding: 3px 10px; border-radius: 99px; }
  .restabs button.on { background: var(--surface-3); }
  .restabs .ghost { margin-left: auto; }
  .small { font-size: 12px; }
  table.res { font-size: 11.5px; }
  table.res th { padding: 4px 6px 4px 0; position: static; background: none; color: var(--text-muted); font-weight: 500; }
  table.res td { padding: 4px 6px 4px 0; white-space: nowrap; }
  .listen { color: var(--good); }
  .fds { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 3px; font-size: 11.5px; max-height: 260px; overflow-y: auto; }
  .fds li { display: grid; grid-template-columns: 28px 52px 1fr; gap: 6px; align-items: baseline; }
  .fd { color: var(--text-muted); text-align: right; }
  .kind { font-size: 10px; text-transform: uppercase; color: var(--text-muted); }
  .kind.file { color: var(--series-3); }
  .kind.socket { color: var(--series-1); }
  .t { word-break: break-all; }
</style>
