<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/NetLens.svelte
  Author : Praneeth
  License: MIT
  Purpose: NetLens - packet capture and analysis (Wireshark-like).
           Capture bar (interfaces, capture filter, start/stop, files),
           display filter with auto-completion, and tabs:
             Packets       virtual list + plain-English summary, detail tree
                           and linked hex view
             Conversations TCP connections / UDP flows with host and app
             Applications  traffic per program on this PC
             RTP streams   media quality (loss, jitter, key frames)
             Statistics    traffic over time, protocol hierarchy
             Insights      detected problems explained, one click to filter
-->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Modal from "../lib/components/Modal.svelte";
  import LineChart from "../lib/components/LineChart.svelte";
  import PacketList from "../lib/components/netlens/PacketList.svelte";
  import DetailTree from "../lib/components/netlens/DetailTree.svelte";
  import HexView from "../lib/components/netlens/HexView.svelte";
  import FollowStream from "../lib/components/netlens/FollowStream.svelte";
  import { app } from "../lib/stores.svelte";
  import { bytes as fmtBytes } from "../lib/format";
  import {
    nl, pickCapture, pickCaptureSave, b64bytes, bits, secs, DISPLAY_PRESETS, CAPTURE_PRESETS,
    type Setup, type Status, type PacketDetail, type Node, type Row, type Conversation, type AppUsage,
    type RtpStat, type Stats, type Insight, type FollowResult, type FieldInfo, type Limits,
  } from "../lib/netlens";

  // ------------------------------------------------------------ settings
  const LS = "syspulse.netlens";
  interface Saved { timeFormat: "clock" | "date" | "relative" | "delta"; ifaces: string[]; captureFilter: string; promisc: boolean; snaplen: number; limits: Limits; resolve: boolean; split: number }
  function loadSaved(): Saved {
    const d: Saved = { timeFormat: "clock", ifaces: [], captureFilter: "", promisc: true, snaplen: 0, limits: { maxPackets: 0, maxMb: 1024, maxSeconds: 0 }, resolve: true, split: 52 };
    try {
      return { ...d, ...JSON.parse(localStorage.getItem(LS) ?? "{}") };
    } catch {
      return d;
    }
  }
  let cfg = $state(loadSaved());
  $effect(() => {
    const snapshot = JSON.stringify(cfg);
    try { localStorage.setItem(LS, snapshot); } catch { /* private mode */ }
  });

  // --------------------------------------------------------------- state
  let setup = $state<Setup | null>(null);
  let status = $state<Status | null>(null);
  let gen = $state(0);
  let tab = $state<"packets" | "conversations" | "apps" | "rtp" | "stats" | "insights">("packets");
  let follow = $state(true);
  let list: PacketList | undefined = $state();
  let tree: DetailTree | undefined = $state();

  let selectedNo = $state<number | null>(null);
  let detail = $state<PacketDetail | null>(null);
  let detailBytes = $derived(detail ? b64bytes(detail.bytes) : new Uint8Array());
  let selNode = $state<Node | null>(null);
  let detailMode = $state<"netlens" | "wireshark">("netlens");
  let wsTree = $state<Node[] | null>(null);
  let wsBusy = $state(false);

  let filterText = $state("");
  let filterError = $state<string | null>(null);
  let appliedFilter = $state("");
  let filtering = $state(false);
  let fields: FieldInfo[] = [];
  let suggest = $state<FieldInfo[]>([]);
  let suggestIdx = $state(-1);
  let filterInput: HTMLInputElement | undefined = $state();

  let capErr = $state<string | null>(null);
  let showOptions = $state(false);
  let confirm = $state<{ text: string; run: () => void } | null>(null);
  let menu = $state<{ row: Row; x: number; y: number } | null>(null);
  let followRes = $state<FollowResult | null>(null);
  let granting = $state(false);

  let convs = $state<Conversation[]>([]);
  let convSort = $state<{ key: keyof Conversation | "bytes" | "packets"; desc: boolean }>({ key: "bytes", desc: true });
  let convSearch = $state("");
  let apps = $state<AppUsage[]>([]);
  let rtp = $state<RtpStat[]>([]);
  let stats = $state<Stats | null>(null);
  let insights = $state<Insight[]>([]);

  let busy = $derived(status?.state === "capturing" || status?.state === "loading");
  let canCapture = $derived(!!setup?.access.canCapture);
  let live = $derived(status?.state === "capturing" || (status?.source ?? "").startsWith("Live"));
  let problemCount = $derived(insights.filter((i) => i.severity !== "info").reduce((a, i) => a + i.count, 0));

  // ------------------------------------------------------------- startup
  async function loadSetup() {
    try {
      setup = await nl.setup();
      if (cfg.ifaces.length === 0 || !cfg.ifaces.every((n) => setup!.interfaces.some((i) => i.name === n))) {
        const best = setup.interfaces.find((i) => i.up && i.linktype !== null && (i.kind === "ethernet" || i.kind === "wifi") && i.addresses.length);
        cfg.ifaces = best ? [best.name] : setup.interfaces.filter((i) => i.up && i.linktype !== null).slice(0, 1).map((i) => i.name);
      }
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  let timer: ReturnType<typeof setInterval> | undefined;
  let lastState = "";
  let tick = 0;
  async function poll() {
    try {
      const s = await nl.status();
      const prevShown = status?.shown ?? -1;
      status = s;
      if (lastState === "capturing" && s.state === "stopped") {
        if (s.stopReason) app.notify(s.stopReason, "info");
        refreshTab();
      }
      if (lastState === "loading" && s.state === "stopped") {
        app.notify(`Loaded ${s.packets.toLocaleString()} packets from ${s.source}`, "ok");
        refreshTab();
      }
      if (s.messages.length && s.state !== lastState) {
        for (const m of s.messages) app.notify(m, "error", 7000);
      }
      lastState = s.state;
      if (prevShown !== s.shown && s.state !== "capturing") gen++;
      tick++;
      if (busy && tick % 4 === 0) refreshTab();
    } catch {
      /* backend not ready */
    }
  }

  onMount(async () => {
    await loadSetup();
    try { fields = await nl.fields(); } catch { fields = []; }
    await poll();
    if (status?.displayFilter) {
      filterText = appliedFilter = status.displayFilter;
    }
    timer = setInterval(poll, 500);
    refreshTab();
    window.addEventListener("click", closeMenu);
  });
  onDestroy(() => {
    clearInterval(timer);
    window.removeEventListener("click", closeMenu);
  });
  function closeMenu() {
    menu = null;
  }

  // ------------------------------------------------------------ actions
  function guard(text: string, run: () => void) {
    if (status && status.unsaved && status.packets > 0) confirm = { text, run };
    else run();
  }

  async function start() {
    capErr = null;
    if (cfg.captureFilter.trim()) {
      const e = await nl.checkCaptureFilter(cfg.captureFilter);
      if (e) {
        capErr = e;
        return;
      }
    }
    guard("Start a new capture? The current packets have not been saved.", async () => {
      try {
        await nl.start({ interfaces: cfg.ifaces, filter: cfg.captureFilter.trim(), promiscuous: cfg.promisc, snaplen: cfg.snaplen }, cfg.limits);
        resetSelection();
        follow = true;
        gen++;
        await poll();
      } catch (e) {
        app.notify(String(e), "error", 7000);
      }
    });
  }

  async function stop() {
    await nl.stop();
    await poll();
    gen++;
    refreshTab();
  }

  function openFile() {
    guard("Open a file? The current packets have not been saved.", async () => {
      const p = await pickCapture();
      if (!p) return;
      try {
        await nl.open(p);
        resetSelection();
        follow = false;
        gen++;
        await poll();
      } catch (e) {
        app.notify(String(e), "error", 7000);
      }
    });
  }

  async function save(displayedOnly: boolean): Promise<boolean> {
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    const p = await pickCaptureSave(`netlens-${stamp}.pcapng`);
    if (!p) return false;
    try {
      const n = await nl.save(p, displayedOnly);
      app.notify(`Saved ${n.toLocaleString()} packets`, "ok");
      await poll();
      return true;
    } catch (e) {
      app.notify(String(e), "error");
      return false;
    }
  }

  function clearAll() {
    guard("Discard all captured packets?", async () => {
      await nl.clear();
      resetSelection();
      gen++;
      await poll();
      refreshTab();
    });
  }

  async function openWireshark() {
    try {
      await nl.openWireshark(!!appliedFilter);
      app.notify("Opened in Wireshark", "ok");
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function grant() {
    granting = true;
    try {
      app.notify(await nl.grant(true), "ok");
      await loadSetup();
    } catch (e) {
      app.notify(String(e), "error", 7000);
    } finally {
      granting = false;
    }
  }

  function resetSelection() {
    selectedNo = null;
    detail = null;
    selNode = null;
    wsTree = null;
    detailMode = "netlens";
  }

  // ---------------------------------------------------------- selection
  let detailToken = 0;
  async function select(no: number) {
    selectedNo = no;
    const t = ++detailToken;
    const d = await nl.detail(no).catch(() => null);
    if (t !== detailToken) return;
    detail = d;
    selNode = null;
    wsTree = null;
    if (detailMode === "wireshark") decodeWireshark();
  }

  async function decodeWireshark() {
    if (selectedNo == null) return;
    detailMode = "wireshark";
    wsBusy = true;
    try {
      wsTree = await nl.wiresharkDecode(selectedNo);
    } catch (e) {
      app.notify(String(e), "error");
      detailMode = "netlens";
    } finally {
      wsBusy = false;
    }
  }

  function pickByte(off: number) {
    if (!detail) return;
    let best: Node | null = null;
    const walk = (list: Node[]) => {
      for (const n of list) {
        if (n.len > 0 && off >= n.start && off < n.start + n.len) {
          if (!best || n.len <= best.len) best = n;
        }
        if (n.children) walk(n.children);
      }
    };
    walk(detail.tree);
    if (best) {
      detailMode = "netlens";
      selNode = best;
      tree?.reveal(best);
    }
  }

  async function showFollow(stream: number | null) {
    if (stream == null) return;
    try {
      followRes = await nl.follow(stream);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  // ------------------------------------------------------------- filter
  let checkTimer: ReturnType<typeof setTimeout> | undefined;
  function onFilterInput() {
    clearTimeout(checkTimer);
    checkTimer = setTimeout(async () => {
      filterError = filterText.trim() ? await nl.checkFilter(filterText) : null;
    }, 150);
    const m = filterText.slice(0, filterInput?.selectionStart ?? filterText.length).match(/[a-z][\w.]*$/i);
    if (m && m[0].length >= 1) {
      const w = m[0].toLowerCase();
      suggest = fields.filter((f) => f.name.startsWith(w) && f.name !== w).slice(0, 9);
    } else {
      suggest = [];
    }
    suggestIdx = -1;
  }

  function acceptSuggestion(f: FieldInfo) {
    const pos = filterInput?.selectionStart ?? filterText.length;
    const before = filterText.slice(0, pos).replace(/[a-z][\w.]*$/i, f.name);
    filterText = before + filterText.slice(pos);
    suggest = [];
    filterError = null;
    queueMicrotask(() => {
      filterInput?.focus();
      filterInput?.setSelectionRange(before.length, before.length);
    });
  }

  function filterKey(e: KeyboardEvent) {
    if (suggest.length) {
      if (e.key === "ArrowDown") { suggestIdx = (suggestIdx + 1) % suggest.length; e.preventDefault(); return; }
      if (e.key === "ArrowUp") { suggestIdx = (suggestIdx - 1 + suggest.length) % suggest.length; e.preventDefault(); return; }
      if ((e.key === "Tab" || e.key === "Enter") && suggestIdx >= 0) { acceptSuggestion(suggest[suggestIdx]); e.preventDefault(); return; }
      if (e.key === "Escape") { suggest = []; return; }
    }
    if (e.key === "Enter") applyFilter();
  }

  async function applyFilter(text = filterText) {
    filterText = text;
    suggest = [];
    const err = text.trim() ? await nl.checkFilter(text) : null;
    filterError = err;
    if (err) return;
    filtering = true;
    try {
      await nl.setFilter(text);
      appliedFilter = text.trim();
      gen++;
      await poll();
      if (selectedNo != null) {
        const pos = await nl.locate(selectedNo);
        if (pos != null) setTimeout(() => list?.reveal(pos), 50);
      }
    } catch (e) {
      filterError = String(e);
    } finally {
      filtering = false;
    }
  }

  function filterFrom(f: string, mode: "apply" | "and" | "not") {
    const cur = appliedFilter.trim();
    let next = f;
    if (mode === "and" && cur) next = `(${cur}) && ${f}`;
    if (mode === "not") next = cur ? `(${cur}) && !(${f})` : `!(${f})`;
    tab = "packets";
    applyFilter(next);
  }

  // --------------------------------------------------------------- tabs
  async function refreshTab() {
    try {
      insights = await nl.insights();
      if (tab === "conversations") convs = await nl.conversations();
      else if (tab === "apps") apps = await nl.apps();
      else if (tab === "rtp") rtp = await nl.rtp();
      else if (tab === "stats") stats = await nl.stats();
    } catch {
      /* ignore */
    }
  }
  $effect(() => {
    void tab;
    refreshTab();
  });

  let sortedConvs = $derived.by(() => {
    const q = convSearch.trim().toLowerCase();
    const v = convs.filter((c) => !q || `${c.a}:${c.aPort} ${c.b}:${c.bPort} ${c.host} ${c.app} ${c.protocol} ${c.service}`.toLowerCase().includes(q));
    const k = convSort.key;
    const val = (c: Conversation): number | string =>
      k === "bytes" ? c.bytesAb + c.bytesBa : k === "packets" ? c.packetsAb + c.packetsBa : (c[k] as number | string);
    v.sort((x, y) => {
      const a = val(x), b = val(y);
      const r = typeof a === "number" && typeof b === "number" ? a - b : String(a).localeCompare(String(b));
      return convSort.desc ? -r : r;
    });
    return v.slice(0, 2000);
  });
  function sortBy(key: typeof convSort.key) {
    convSort = { key, desc: convSort.key === key ? !convSort.desc : true };
  }

  let ioTime = $derived(stats ? stats.ioBits.map((_, i) => i) : []);
  let maxHier = $derived(stats?.hierarchy.reduce((m, h) => Math.max(m, h.bytes), 1) ?? 1);

  // -------------------------------------------------------------- split
  let mainEl: HTMLDivElement | undefined = $state();
  function dragSplit(e: MouseEvent) {
    e.preventDefault();
    const move = (ev: MouseEvent) => {
      if (!mainEl) return;
      const r = mainEl.getBoundingClientRect();
      cfg.split = Math.min(85, Math.max(18, ((ev.clientY - r.top) / r.height) * 100));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  function ifaceLabel(kind: string) {
    return { ethernet: "Ethernet", wifi: "Wi-Fi", loopback: "Loopback", bridge: "Bridge", virtual: "Virtual", tunnel: "Tunnel/VPN" }[kind] ?? kind;
  }
  function toggleIface(name: string) {
    cfg.ifaces = cfg.ifaces.includes(name) ? cfg.ifaces.filter((n) => n !== name) : [...cfg.ifaces, name];
  }
  const sevIcon = (s: string) => (s === "error" ? "alert" : s === "warning" ? "alert" : "info");
</script>

<div class="nl">
  <div class="top">
    <div class="brand">
      <h1>NetLens</h1>
      <span class="muted">Capture and understand the traffic on your network cards</span>
    </div>
    <div class="grow"></div>
    <button onclick={openFile} disabled={busy} title="Open a .pcap / .pcapng file (also from Wireshark or tcpdump)"><Icon name="files" size={15} /> Open</button>
    <button onclick={() => save(false)} disabled={busy || !status?.packets} title="Save all packets as pcapng"><Icon name="disk" size={15} /> Save</button>
    {#if appliedFilter}<button onclick={() => save(true)} disabled={busy || !status?.shown} title="Save only the packets matching the display filter">Save displayed</button>{/if}
    <button onclick={clearAll} disabled={busy || !status?.packets}><Icon name="close" size={15} /> Clear</button>
    {#if setup?.tools.wireshark}
      <button onclick={openWireshark} disabled={busy || !status?.packets} title="Open these packets in Wireshark">Wireshark ↗</button>
    {/if}
  </div>

  {#if setup && !setup.access.canCapture}
    <div class="banner">
      <Icon name="info" size={16} />
      {#if setup.access.helperFound}
        <span>NetLens needs a one-time permission to capture network traffic. Only the small <b>syspulse-capture</b> helper gets it, not SysPulse itself. You can still open saved capture files.</span>
        <button class="primary small" onclick={grant} disabled={granting}>{granting ? "Waiting for password…" : "Grant capture permission"}</button>
      {:else}
        <span>The capture helper ({setup.access.message}) is missing — reinstall SysPulse. You can still open capture files.</span>
      {/if}
    </div>
  {/if}

  <!-- capture bar -->
  <div class="capbar">
    {#if status?.state === "capturing"}
      <button class="danger" onclick={stop}><Icon name="stop" size={15} /> Stop</button>
    {:else if status?.state === "loading"}
      <button class="danger" onclick={stop}><Icon name="stop" size={15} /> Cancel</button>
    {:else}
      <button class="primary" onclick={start} disabled={!canCapture || cfg.ifaces.length === 0} title={canCapture ? "Start capturing" : "Grant capture permission first"}><Icon name="play" size={15} /> Start capture</button>
    {/if}
    <div class="ifaces">
      {#each setup?.interfaces ?? [] as i (i.name)}
        <button
          class="chip"
          class:on={cfg.ifaces.includes(i.name)}
          class:down={!i.up}
          disabled={busy || i.linktype === null}
          title="{ifaceLabel(i.kind)}{i.up ? '' : ' (down)'}{i.addresses.length ? ' · ' + i.addresses.join(', ') : ''}{i.linktype === null ? ' · cannot be captured' : ''}"
          onclick={() => toggleIface(i.name)}
        >
          <Icon name={i.kind === "wifi" ? "network" : i.kind === "loopback" ? "refresh" : "usb"} size={13} />
          {i.name}
          {#if i.addresses[0]}<span class="addr">{i.addresses[0]}</span>{/if}
        </button>
      {/each}
    </div>
    <div class="capfilter" class:bad={!!capErr}>
      <span class="muted">Capture filter</span>
      <input bind:value={cfg.captureFilter} placeholder="e.g. host 192.168.1.64 or tcp port 554" disabled={busy || !setup?.access.filtersAvailable} oninput={() => (capErr = null)} spellcheck="false" />
      <select disabled={busy} onchange={(e) => { cfg.captureFilter = (e.currentTarget as HTMLSelectElement).value; (e.currentTarget as HTMLSelectElement).selectedIndex = 0; capErr = null; }}>
        <option value="" selected disabled>Presets</option>
        {#each CAPTURE_PRESETS as p (p.label)}<option value={p.filter}>{p.label}</option>{/each}
      </select>
    </div>
    <button class="ghost" onclick={() => (showOptions = true)} disabled={busy}><Icon name="settings" size={15} /> Options</button>
  </div>
  {#if capErr}<div class="err small-line">{capErr}</div>{/if}

  <!-- display filter -->
  <div class="fbar">
    <div class="fwrap">
      <span class="fl" class:ok={filterText && !filterError} class:bad={!!filterError}>
        <Icon name="search" size={14} />
        <input
          bind:this={filterInput}
          bind:value={filterText}
          oninput={onFilterInput}
          onkeydown={filterKey}
          onblur={() => setTimeout(() => (suggest = []), 150)}
          placeholder="Display filter — e.g. http.host contains &quot;api&quot; && ip.addr == 10.0.0.0/8   (Enter to apply)"
          spellcheck="false"
          class="mono"
        />
        {#if filterText}<button class="x" onclick={() => applyFilter("")} title="Clear filter">✕</button>{/if}
        <button class="small" onclick={() => applyFilter()} disabled={filtering}>{filtering ? "…" : "Apply"}</button>
      </span>
      {#if suggest.length}
        <div class="sugg">
          {#each suggest as f, i (f.name)}
            <button class:on={i === suggestIdx} onmousedown={(e) => { e.preventDefault(); acceptSuggestion(f); }}>
              <span class="mono">{f.name}</span><span class="muted">{f.description}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
    <div class="presets">
      {#each DISPLAY_PRESETS as p (p.label)}
        <button class="chip" class:on={appliedFilter === p.filter} title="{p.hint}: {p.filter}" onclick={() => applyFilter(appliedFilter === p.filter ? "" : p.filter)}>{p.label}</button>
      {/each}
    </div>
    {#if filterError}<div class="err small-line">{filterError}</div>{/if}
  </div>

  <!-- tabs -->
  <div class="tabs">
    {#each [["packets", "Packets"], ["conversations", "Conversations"], ["apps", "Applications"], ["rtp", "RTP streams"], ["stats", "Statistics"], ["insights", "Insights"]] as [id, label] (id)}
      <button class="tab" class:on={tab === id} onclick={() => (tab = id as typeof tab)}>
        {label}
        {#if id === "insights" && problemCount > 0}<span class="badge">{problemCount > 999 ? "999+" : problemCount}</span>{/if}
      </button>
    {/each}
    <span class="grow"></span>
    {#if tab === "packets"}
      <label class="chk">Time
        <select class="tsel" bind:value={cfg.timeFormat} title="Time column format">
          <option value="clock">Time of day</option>
          <option value="date">Date and time</option>
          <option value="relative">Seconds since start</option>
          <option value="delta">Since previous packet</option>
        </select>
      </label>
      <label class="chk"><input type="checkbox" bind:checked={cfg.resolve} /> Show host names</label>
      <label class="chk"><input type="checkbox" bind:checked={follow} /> Auto-scroll</label>
    {/if}
  </div>

  <div class="main" bind:this={mainEl}>
    {#if tab === "packets"}
      <div class="pane listpane" style:height="{cfg.split}%">
        <PacketList
          bind:this={list}
          bind:follow
          total={status?.shown ?? 0}
          {gen}
          resolve={cfg.resolve}
          showApp={live}
          filtered={!!appliedFilter}
          timeFormat={cfg.timeFormat}
          {selectedNo}
          onselect={(no) => select(no)}
          onmenu={(row, x, y) => (menu = { row, x, y })}
          onactivate={() => detail?.stream != null && showFollow(detail.stream)}
        />
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="splitter" onmousedown={dragSplit} title="Drag to resize"></div>
      <div class="bottom">
        {#if detail}
          <div class="pane detailpane">
            <div class="summary">
              <Icon name="info" size={15} />
              <div>
                <p>{detail.summary}</p>
                <div class="sumacts">
                  <span class="muted mono">Frame {detail.no} · {detail.time}</span>
                  {#if detail.stream != null}
                    <button class="small" onclick={() => showFollow(detail!.stream)}>Follow {detail.tcp ? "TCP" : "UDP"} stream</button>
                    <button class="small" onclick={() => filterFrom(`${detail!.tcp ? "tcp" : "udp"}.stream == ${detail!.stream}`, "apply")}>Only this conversation</button>
                  {/if}
                </div>
              </div>
            </div>
            <div class="treebar">
              <div class="seg">
                <button class:on={detailMode === "netlens"} onclick={() => (detailMode = "netlens")}>NetLens</button>
                <button class:on={detailMode === "wireshark"} disabled={!setup?.tools.tshark} title={setup?.tools.tshark ? "Decode this packet with Wireshark's dissectors (tshark)" : "Install the tshark package to decode with Wireshark's 3000+ protocols"} onclick={decodeWireshark}>Wireshark decode</button>
              </div>
              <span class="grow"></span>
              {#if detailMode === "netlens"}
                <button class="small ghost" onclick={() => tree?.expandAll(true)}>Expand all</button>
                <button class="small ghost" onclick={() => tree?.expandAll(false)}>Collapse all</button>
              {/if}
            </div>
            <div class="treescroll">
              {#if detailMode === "netlens"}
                <DetailTree bind:this={tree} nodes={detail.tree} selected={selNode} onselect={(n) => (selNode = n)} onfilter={filterFrom} />
              {:else if wsBusy}
                <p class="muted pad">Decoding with tshark…</p>
              {:else if wsTree}
                <DetailTree nodes={wsTree} selected={null} onselect={() => {}} onfilter={() => {}} />
              {/if}
            </div>
          </div>
          <div class="pane hexpane">
            <HexView bytes={detailBytes} start={selNode?.start ?? 0} len={selNode?.len ?? 0} onpick={pickByte} />
          </div>
        {:else}
          <div class="pane placeholder muted">
            {#if (status?.packets ?? 0) === 0}
              {#if canCapture}Choose an interface above and press <b>Start capture</b>, or open a capture file.{:else}Open a capture file, or grant capture permission to capture live.{/if}
            {:else}
              Select a packet to see what it is, its protocol layers and its bytes.
            {/if}
          </div>
        {/if}
      </div>
    {:else if tab === "conversations"}
      <div class="pane tablepane">
        <div class="tbar">
          <input placeholder="Search address, host, app, protocol…" bind:value={convSearch} />
          <span class="muted">{convs.length.toLocaleString()} conversations</span>
          <span class="grow"></span>
          <button class="small" onclick={refreshTab}><Icon name="refresh" size={13} /> Refresh</button>
        </div>
        <div class="tscroll">
          <table>
            <thead>
              <tr>
                <th onclick={() => sortBy("protocol")}>Protocol</th>
                <th onclick={() => sortBy("a")}>Address A</th>
                <th onclick={() => sortBy("b")}>Address B</th>
                <th onclick={() => sortBy("host")}>Host</th>
                <th onclick={() => sortBy("app")}>App</th>
                <th class="r" onclick={() => sortBy("packets")}>Packets</th>
                <th class="r" onclick={() => sortBy("bytesAb")}>A → B</th>
                <th class="r" onclick={() => sortBy("bytesBa")}>B → A</th>
                <th class="r" onclick={() => sortBy("duration")}>Duration</th>
                <th onclick={() => sortBy("state")}>State</th>
                <th class="r" onclick={() => sortBy("problems")}>Issues</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each sortedConvs as c (c.id)}
                <tr class:issue={c.problems > 0}>
                  <td><b>{c.protocol}</b> <span class="muted">{c.transport}</span></td>
                  <td class="mono">{c.a}:{c.aPort}</td>
                  <td class="mono">{c.b}:{c.bPort}{#if c.service}<span class="muted"> {c.service}</span>{/if}</td>
                  <td class="ell" title={c.host}>{c.host}</td>
                  <td>{c.app}</td>
                  <td class="r num">{(c.packetsAb + c.packetsBa).toLocaleString()}</td>
                  <td class="r num">{fmtBytes(c.bytesAb)}</td>
                  <td class="r num">{fmtBytes(c.bytesBa)}</td>
                  <td class="r num">{secs(c.duration)}</td>
                  <td>{c.state}</td>
                  <td class="r num">{c.problems || ""}</td>
                  <td class="acts">
                    <button class="small" onclick={() => filterFrom(`${c.transport.toLowerCase()}.stream == ${c.id}`, "apply")}>Packets</button>
                    <button class="small" onclick={() => showFollow(c.id)}>Follow</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if convs.length === 0}<p class="muted pad">No TCP or UDP conversations yet.</p>{/if}
        </div>
      </div>
    {:else if tab === "apps"}
      <div class="pane tablepane">
        <div class="tbar">
          <span class="muted">Traffic per program on this PC (live captures). Programs of other users show as "unknown / system".</span>
          <span class="grow"></span>
          <button class="small" onclick={refreshTab}><Icon name="refresh" size={13} /> Refresh</button>
        </div>
        <div class="tscroll cards">
          {#each apps as a (a.name)}
            <div class="appcard">
              <div class="apphead">
                <b>{a.name}</b>
                {#if a.pids.length}<span class="muted mono">PID {a.pids.slice(0, 4).join(", ")}</span>{/if}
                <span class="grow"></span>
                {#if a.pids.length}<button class="small" onclick={() => filterFrom(`app.name == "${a.name}"`, "apply")}>Show packets</button>{/if}
              </div>
              <div class="kvs">
                <span>Downloaded</span><b class="num">{fmtBytes(a.bytesIn)}</b>
                <span>Uploaded</span><b class="num">{fmtBytes(a.bytesOut)}</b>
                <span>Connections</span><b class="num">{a.connections}</b>
                <span>Issues</span><b class="num" class:warn={a.problems > 0}>{a.problems}</b>
              </div>
              {#if a.hosts.length}<div class="hosts muted">{a.hosts.join(" · ")}</div>{/if}
            </div>
          {/each}
          {#if apps.length === 0}<p class="muted pad">No connections yet.</p>{/if}
        </div>
      </div>
    {:else if tab === "rtp"}
      <div class="pane tablepane">
        <div class="tbar">
          <span class="muted">Audio/video streams (RTP), found through RTSP SETUP or RTSP-interleaved TCP. Loss and jitter tell you why video stutters.</span>
          <span class="grow"></span>
          <button class="small" onclick={refreshTab}><Icon name="refresh" size={13} /> Refresh</button>
        </div>
        <div class="tscroll">
          <table>
            <thead>
              <tr><th>SSRC</th><th>From</th><th>To</th><th>Codec</th><th class="r">Packets</th><th class="r">Lost</th><th class="r">Seq. errors</th><th class="r">Max jitter</th><th class="r">Mean jitter</th><th class="r">Max gap</th><th class="r">Frames</th><th class="r">Key frames</th><th class="r">Bitrate</th><th></th></tr>
            </thead>
            <tbody>
              {#each rtp as s (s.ssrc + s.src)}
                {@const dur = Math.max((s.lastNs - s.firstNs) / 1e9, 0.001)}
                {@const lossPct = (s.lost / Math.max(s.packets + s.lost, 1)) * 100}
                <tr class:issue={s.lost > 0}>
                  <td class="mono">0x{s.ssrc.toString(16).toUpperCase().padStart(8, "0")}</td>
                  <td class="mono">{s.src}</td>
                  <td class="mono">{s.dst}</td>
                  <td>{s.codec} <span class="muted">({s.payloadType})</span></td>
                  <td class="r num">{s.packets.toLocaleString()}</td>
                  <td class="r num" class:warn={s.lost > 0}>{s.lost} ({lossPct.toFixed(2)}%)</td>
                  <td class="r num">{s.sequenceErrors}</td>
                  <td class="r num">{s.maxJitterMs.toFixed(2)} ms</td>
                  <td class="r num">{s.meanJitterMs.toFixed(2)} ms</td>
                  <td class="r num">{s.maxDeltaMs.toFixed(1)} ms</td>
                  <td class="r num">{s.frames}</td>
                  <td class="r num">{s.keyFrames}</td>
                  <td class="r num">{bits((s.bytes * 8) / dur)}</td>
                  <td><button class="small" onclick={() => filterFrom(`rtp.ssrc == ${s.ssrc}`, "apply")}>Packets</button></td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if rtp.length === 0}<p class="muted pad">No RTP streams. Start the capture <b>before</b> the player connects so NetLens sees the RTSP SETUP that announces the media ports.</p>{/if}
        </div>
      </div>
    {:else if tab === "stats"}
      <div class="pane tablepane">
        {#if stats}
          <div class="tscroll statgrid">
            <div class="statcards">
              <div><span class="muted">Packets</span><b class="num">{stats.totalPackets.toLocaleString()}</b></div>
              <div><span class="muted">Data</span><b class="num">{fmtBytes(stats.totalBytes)}</b></div>
              <div><span class="muted">Duration</span><b class="num">{secs(stats.duration)}</b></div>
              <div><span class="muted">Average</span><b class="num">{bits(stats.duration > 0 ? (stats.totalBytes * 8) / stats.duration : 0)}</b></div>
            </div>
            <h3>Traffic over time</h3>
            {#if ioTime.length > 1}
              <LineChart time={ioTime} elapsed height={170} series={[{ label: "Throughput", color: "--series-1", values: stats.ioBits }]} format={(v) => bits(v)} />
              <LineChart time={ioTime} elapsed height={120} series={[{ label: "Packets/s", color: "--series-3", values: stats.ioPackets }]} />
            {:else}
              <p class="muted">Needs at least two seconds of traffic.</p>
            {/if}
            <h3>Protocol hierarchy</h3>
            <table>
              <thead><tr><th>Protocol</th><th class="r">Packets</th><th class="r">Bytes</th><th style="width:40%">Share of bytes</th></tr></thead>
              <tbody>
                {#each stats.hierarchy as h (h.path)}
                  <tr>
                    <td style:padding-left="{10 + h.depth * 18}px">{h.name}</td>
                    <td class="r num">{h.packets.toLocaleString()}</td>
                    <td class="r num">{fmtBytes(h.bytes)}</td>
                    <td><div class="bar"><div style:width="{(h.bytes / maxHier) * 100}%"></div></div></td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </div>
    {:else if tab === "insights"}
      <div class="pane tablepane">
        <div class="tscroll cards">
          {#each insights as i (i.id)}
            <div class="insight {i.severity}">
              <Icon name={sevIcon(i.severity)} size={18} />
              <div class="ibody">
                <b>{i.title}</b>
                <p class="muted">{i.detail}</p>
              </div>
              {#if i.filter}<button class="small" onclick={() => filterFrom(i.filter, "apply")}>Show packets</button>{/if}
            </div>
          {/each}
          {#if insights.length === 0}
            <div class="insight good"><Icon name="check" size={18} /><div class="ibody"><b>No problems detected</b><p class="muted">No retransmissions, resets, failed lookups, HTTP/RTSP errors, TLS failures or media loss in this capture.</p></div></div>
          {/if}
        </div>
      </div>
    {/if}
  </div>

  <!-- status bar -->
  <div class="statusbar">
    <span class="dot" class:rec={status?.state === "capturing"} class:load={status?.state === "loading"}></span>
    <span>
      {#if status?.state === "capturing"}Capturing on {status.interfaces.join(", ")}{#if status.captureFilter} · filter “{status.captureFilter}”{/if}
      {:else if status?.state === "loading"}Loading {status.source} — {Math.round((status.progress ?? 0) * 100)}%
      {:else if status?.packets}{status.source}
      {:else}Ready{/if}
    </span>
    <span class="grow"></span>
    {#if status}
      <span>Packets <b class="num">{status.packets.toLocaleString()}</b></span>
      {#if appliedFilter}<span>Displayed <b class="num">{status.shown.toLocaleString()}</b> ({status.packets ? ((status.shown / status.packets) * 100).toFixed(1) : 0}%)</span>{/if}
      {#if status.dropped > 0}<span class="warn">Dropped <b class="num">{status.dropped.toLocaleString()}</b></span>{/if}
      <span>{fmtBytes(status.bytes)}</span>
      {#if status.state === "capturing"}<span class="num">{status.packetsPerSec.toLocaleString()} pkt/s · {bits(status.bitsPerSec)}</span>{/if}
      <span class="num">{secs(status.duration)}</span>
      {#if status.unsaved && status.packets}<span class="muted">unsaved</span>{/if}
    {/if}
  </div>
</div>

{#if menu}
  {@const m = menu}
  <div class="menu" style:left="{m.x}px" style:top="{m.y}px" role="menu">
    {#if detail?.stream != null}
      <button onclick={() => showFollow(detail!.stream)}>Follow {detail.tcp ? "TCP" : "UDP"} stream</button>
      <button onclick={() => filterFrom(`${detail!.tcp ? "tcp" : "udp"}.stream == ${detail!.stream}`, "apply")}>Only this conversation</button>
    {/if}
    {#if /^[\d.:a-f]+$/i.test(m.row.src) && (m.row.src.includes(".") || m.row.src.includes(":")) && !/^([0-9a-f]{2}:){5}/i.test(m.row.src)}
      <button onclick={() => filterFrom(`${m.row.src.includes(":") ? "ipv6" : "ip"}.addr == ${m.row.src}`, "apply")}>Only {m.row.src}</button>
      <button onclick={() => filterFrom(`${m.row.src.includes(":") ? "ipv6" : "ip"}.addr == ${m.row.src}`, "not")}>Hide {m.row.src}</button>
    {/if}
    {#if m.row.app}<button onclick={() => filterFrom(`app.name == "${m.row.app}"`, "apply")}>Only {m.row.app}</button>{/if}
    {#if setup?.tools.tshark}<button onclick={decodeWireshark}>Decode with Wireshark</button>{/if}
    <button onclick={() => navigator.clipboard.writeText(`${m.row.no}\t${m.row.time}\t${m.row.src}\t${m.row.dst}\t${m.row.proto}\t${m.row.len}\t${m.row.info}`)}>Copy row</button>
  </div>
{/if}

{#if followRes}
  <FollowStream result={followRes} onclose={() => (followRes = null)} onfilter={(f) => { followRes = null; filterFrom(f, "apply"); }} />
{/if}

{#if confirm}
  {@const c = confirm}
  <!-- handlers copy what they need before clearing `confirm` (c becomes null afterwards) -->
  <Modal title="Unsaved capture" onclose={() => (confirm = null)}>
    <p>{c.text}</p>
    {#snippet footer()}
      <button onclick={() => (confirm = null)}>Cancel</button>
      <button onclick={async () => { const run = c.run; confirm = null; if (await save(false)) run(); }}>Save first…</button>
      <button class="danger" onclick={() => { const run = c.run; confirm = null; run(); }}>Discard</button>
    {/snippet}
  </Modal>
{/if}

{#if showOptions}
  <Modal title="Capture options" onclose={() => (showOptions = false)} width={560}>
    <div class="opts">
      <label class="chk"><input type="checkbox" bind:checked={cfg.promisc} /> Promiscuous mode <span class="muted">— also see traffic addressed to other devices (on hubs, mirror ports or bridges)</span></label>
      <label>Bytes kept per packet
        <select bind:value={cfg.snaplen}>
          <option value={0}>Whole packet</option>
          <option value={128}>128 (headers only, smallest files)</option>
          <option value={1514}>1514 (one Ethernet frame)</option>
          <option value={9000}>9000</option>
        </select>
      </label>
      <h4>Stop automatically</h4>
      <label>After packets <input type="number" min="0" bind:value={cfg.limits.maxPackets} /> <span class="muted">0 = no limit</span></label>
      <label>Memory limit (MB) <input type="number" min="16" max="16384" bind:value={cfg.limits.maxMb} /></label>
      <label>After seconds <input type="number" min="0" bind:value={cfg.limits.maxSeconds} /> <span class="muted">0 = no limit</span></label>
      {#if setup && !setup.access.filtersAvailable}<p class="warn">Capture filters need libpcap (sudo apt install libpcap0.8). Display filters always work.</p>{/if}
      {#if setup?.access.canCapture}
        <p class="muted small-line">Capture permission is granted to {setup.access.helperPath}. <button class="ghost small" onclick={async () => { try { app.notify(await nl.grant(false), "ok"); await loadSetup(); } catch (e) { app.notify(String(e), "error"); } }}>Revoke</button></p>
      {/if}
    </div>
    {#snippet footer()}<button class="primary" onclick={() => (showOptions = false)}>Done</button>{/snippet}
  </Modal>
{/if}

<style>
  button :global(svg) { vertical-align: -2px; }
  .nl { display: flex; flex-direction: column; height: calc(100vh - 54px); gap: 8px; }
  .top { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .brand { display: flex; flex-direction: column; }
  .brand .muted { font-size: 12.5px; }
  .grow { flex: 1; }
  .banner { display: flex; gap: 10px; align-items: center; font-size: 13px; padding: 9px 12px; border-radius: 10px;
    background: color-mix(in srgb, var(--accent) 12%, transparent); border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent); }
  .banner span { flex: 1; }
  .small { font-size: 12px; padding: 3px 10px; }
  .capbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .ifaces { display: flex; gap: 5px; flex-wrap: wrap; }
  .chip { font-size: 12px; padding: 3px 10px; border-radius: 999px; display: inline-flex; align-items: center; gap: 5px; }
  .chip.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 18%, transparent); }
  .chip.down { opacity: 0.6; }
  .chip .addr { color: var(--text-muted); font-size: 11px; }
  .capfilter { display: flex; gap: 6px; align-items: center; flex: 1; min-width: 320px; font-size: 12.5px; }
  .capfilter input { flex: 1; font-family: var(--font-mono); font-size: 12.5px; padding: 4px 8px; }
  .capfilter select { font-size: 12px; padding: 4px 6px; }
  .capfilter.bad input { border-color: var(--critical); }
  .err { color: var(--critical); }
  .warn { color: var(--warning); }
  .small-line { font-size: 12.5px; }
  .fbar { display: flex; flex-direction: column; gap: 6px; }
  .fwrap { position: relative; }
  .fl { display: flex; align-items: center; gap: 6px; padding: 0 4px 0 10px; border: 1px solid var(--border-strong); border-radius: 9px; background: var(--surface-1); color: var(--text-muted); }
  .fl.ok { border-color: var(--good); }
  .fl.bad { border-color: var(--critical); }
  .fl input { flex: 1; border: none; background: transparent; padding: 6px 0; font-size: 13px; }
  .fl .x { border: none; background: transparent; color: var(--text-muted); padding: 2px 6px; }
  .fl .small { margin: 3px 0; }
  .sugg { position: absolute; z-index: 30; top: 100%; left: 28px; margin-top: 2px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: 8px; padding: 4px; min-width: 460px; }
  .sugg button { display: flex; gap: 12px; width: 100%; border: none; background: transparent; text-align: left; padding: 4px 8px; font-size: 12.5px; }
  .sugg button.on, .sugg button:hover { background: var(--surface-3); }
  .presets { display: flex; gap: 5px; flex-wrap: wrap; }
  .tabs { display: flex; gap: 4px; align-items: center; border-bottom: 1px solid var(--border); padding-bottom: 6px; }
  .tab { white-space: nowrap; border: 1px solid transparent; background: transparent; padding: 4px 12px; font-size: 13px; color: var(--text-secondary); display: inline-flex; gap: 6px; align-items: center; }
  .tab.on { background: var(--surface-2); border-color: var(--border-strong); color: var(--text-primary); }
  .badge { background: var(--warning); color: #000; border-radius: 999px; font-size: 10.5px; font-weight: 700; padding: 0 6px; }
  .chk { display: inline-flex; gap: 5px; align-items: center; font-size: 12.5px; color: var(--text-secondary); }
  .chk input { accent-color: var(--accent); }
  .tsel { font-size: 12px; padding: 2px 6px; }
  .main { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .pane { background: var(--surface-1); border: 1px solid var(--border); border-radius: 10px; overflow: hidden; min-height: 0; }
  .listpane { flex: none; }
  .splitter { height: 7px; cursor: row-resize; flex: none; }
  .splitter:hover { background: color-mix(in srgb, var(--accent) 25%, transparent); border-radius: 4px; }
  .bottom { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr); gap: 8px; }
  .bottom:has(.placeholder) { grid-template-columns: 1fr; }
  .detailpane { display: flex; flex-direction: column; }
  .summary { display: flex; gap: 8px; padding: 9px 12px; background: color-mix(in srgb, var(--accent) 8%, var(--surface-2)); border-bottom: 1px solid var(--border); color: var(--accent); }
  .summary p { margin: 0 0 4px; color: var(--text-primary); font-size: 13px; line-height: 1.45; }
  .sumacts { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; font-size: 11.5px; }
  .sumacts .muted { margin-right: 6px; }
  .treebar { display: flex; align-items: center; gap: 6px; padding: 4px 8px; border-bottom: 1px solid var(--border); }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 8px; padding: 2px; gap: 2px; }
  .seg button { border: none; background: transparent; padding: 2px 9px; border-radius: 6px; color: var(--text-secondary); font-size: 12px; }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .treescroll { flex: 1; overflow: auto; min-height: 0; }
  .hexpane { overflow: auto; }
  .placeholder { display: grid; place-items: center; text-align: center; padding: 20px; font-size: 13.5px; }
  .pad { padding: 14px; }
  .tablepane { flex: 1; display: flex; flex-direction: column; }
  .tbar { display: flex; gap: 10px; align-items: center; padding: 8px 10px; border-bottom: 1px solid var(--border); font-size: 12.5px; }
  .tbar input { width: 320px; font-size: 12.5px; padding: 4px 8px; }
  .tscroll { flex: 1; overflow: auto; min-height: 0; }
  table { width: 100%; border-collapse: collapse; font-size: 12.5px; }
  th { position: sticky; top: 0; background: var(--surface-2); text-align: left; font-weight: 600; color: var(--text-secondary); padding: 6px 10px; cursor: pointer; white-space: nowrap; z-index: 1; }
  td { padding: 5px 10px; border-bottom: 1px solid var(--border); white-space: nowrap; }
  tr:hover td { background: var(--surface-2); }
  tr.issue td:first-child { box-shadow: inset 3px 0 0 var(--warning); }
  .r { text-align: right; }
  .ell { max-width: 220px; overflow: hidden; text-overflow: ellipsis; }
  td.acts { display: flex; gap: 4px; }
  .cards { padding: 10px; display: flex; flex-direction: column; gap: 8px; }
  .appcard { border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; background: var(--surface-2); }
  .apphead { display: flex; gap: 10px; align-items: center; }
  .kvs { display: grid; grid-template-columns: repeat(4, auto 1fr); gap: 4px 10px; font-size: 12.5px; margin-top: 6px; }
  .kvs span { color: var(--text-muted); }
  .hosts { font-size: 12px; margin-top: 6px; }
  .statgrid { padding: 12px 14px; display: flex; flex-direction: column; gap: 10px; }
  .statcards { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }
  .statcards div { background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 8px 12px; display: flex; flex-direction: column; }
  .statcards b { font-size: 18px; }
  .bar { height: 8px; background: var(--surface-3); border-radius: 4px; overflow: hidden; }
  .bar div { height: 100%; background: var(--series-1); }
  .insight { display: flex; gap: 12px; align-items: flex-start; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--border); background: var(--surface-2); }
  .insight.error { border-color: color-mix(in srgb, var(--critical) 45%, transparent); color: var(--critical); }
  .insight.warning { border-color: color-mix(in srgb, var(--warning) 45%, transparent); color: var(--warning); }
  .insight.info { color: var(--series-1); }
  .insight.good { color: var(--good); }
  .ibody { flex: 1; color: var(--text-primary); }
  .ibody p { margin: 2px 0 0; font-size: 12.5px; }
  .statusbar { display: flex; gap: 14px; align-items: center; font-size: 12px; color: var(--text-secondary); padding: 4px 4px 0; flex-wrap: wrap; }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--text-muted); }
  .dot.rec { background: var(--critical); }
  .dot.load { background: var(--warning); }
  .menu { position: fixed; z-index: 50; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: 9px; padding: 4px; display: flex; flex-direction: column; min-width: 220px; }
  .menu button { border: none; background: transparent; text-align: left; padding: 6px 10px; font-size: 12.5px; }
  .menu button:hover { background: var(--surface-3); }
  .opts { display: flex; flex-direction: column; gap: 10px; font-size: 13px; }
  .opts label { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .opts input[type="number"] { width: 110px; }
  .opts h4 { margin: 6px 0 0; font-size: 12px; text-transform: uppercase; letter-spacing: 0.05em; color: var(--text-secondary); }
</style>
