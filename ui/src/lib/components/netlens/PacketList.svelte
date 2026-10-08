<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/netlens/PacketList.svelte
  Author : Praneeth
  License: MIT
  Purpose: Virtualised packet list. Only the rows in the viewport are fetched
           from the backend and rendered, so millions of frames scroll as
           smoothly as ten. Keyboard: Up/Down, PageUp/PageDown, Home/End.
-->
<script lang="ts">
  import { nl, type Row } from "../../netlens";

  let {
    total,
    gen,
    resolve,
    showApp,
    filtered,
    timeFormat = "clock",
    selectedNo,
    follow = $bindable(true),
    onselect,
    onmenu,
    onactivate,
  }: {
    total: number;
    /** Bumped by the parent whenever the view changes (filter, new file). */
    gen: number;
    resolve: boolean;
    showApp: boolean;
    filtered: boolean;
    /** clock = time of day, date = date + time, relative = since first packet, delta = since previous. */
    timeFormat?: "clock" | "date" | "relative" | "delta";
    selectedNo: number | null;
    follow?: boolean;
    onselect: (no: number, pos: number) => void;
    onmenu: (row: Row, x: number, y: number) => void;
    onactivate: (row: Row) => void;
  } = $props();

  const RH = 22;
  const MAXH = 30_000_000; // WebKit limits element height
  let box: HTMLDivElement = $state()!;
  let viewH = $state(400);
  let scrollTop = $state(0);
  let rows = $state.raw<Row[]>([]);
  let rowsFrom = $state(0);
  let selectedPos = $state(-1);
  let pendingPos: number | null = null;

  let virtH = $derived(Math.max(Math.min(total * RH, MAXH), 0));
  let scaled = $derived(total * RH > MAXH);
  let visible = $derived(Math.ceil(viewH / RH) + 1);
  let first = $derived.by(() => {
    if (!scaled) return Math.floor(scrollTop / RH);
    const maxScroll = Math.max(virtH - viewH, 1);
    return Math.round((scrollTop / maxScroll) * Math.max(total - visible + 1, 0));
  });
  let windowTop = $derived(scaled ? scrollTop : first * RH);

  // ---- fetching (latest request wins)
  let token = 0;
  let inflight = false;
  let again = false;
  async function fetchRows() {
    if (inflight) {
      again = true;
      return;
    }
    inflight = true;
    const t = ++token;
    const from = Math.max(0, first - 5);
    const count = visible + 15;
    try {
      const r = await nl.rows(from, count, resolve);
      if (t === token) {
        rows = r;
        rowsFrom = from;
        if (pendingPos !== null) {
          const row = r[pendingPos - from];
          if (row) {
            onselect(row.no, pendingPos);
            pendingPos = null;
          }
        }
      }
    } catch {
      /* transient (e.g. capture restarting) */
    } finally {
      inflight = false;
      if (again) {
        again = false;
        fetchRows();
      }
    }
  }

  $effect(() => {
    // dependencies
    void first;
    void visible;
    void gen;
    void total;
    void resolve;
    fetchRows();
  });

  // ---- follow newest
  $effect(() => {
    void total;
    if (follow && box) {
      queueMicrotask(() => {
        if (box) box.scrollTop = box.scrollHeight;
      });
    }
  });

  // Keep the selected position in sync when the selection comes from outside.
  $effect(() => {
    void gen;
    selectedPos = -1;
  });

  function onScroll() {
    scrollTop = box.scrollTop;
    const atBottom = box.scrollTop + box.clientHeight >= box.scrollHeight - RH;
    if (follow !== atBottom) follow = atBottom;
  }

  $effect(() => {
    if (!box) return;
    const ro = new ResizeObserver(() => (viewH = box.clientHeight));
    ro.observe(box);
    return () => ro.disconnect();
  });

  function scrollToPos(p: number) {
    if (!box) return;
    if (!scaled) {
      const top = p * RH;
      if (top < box.scrollTop) box.scrollTop = top;
      else if (top + RH > box.scrollTop + viewH) box.scrollTop = top + RH - viewH;
    } else {
      const maxScroll = Math.max(virtH - viewH, 1);
      box.scrollTop = (p / Math.max(total - visible + 1, 1)) * maxScroll;
    }
  }

  export function selectPos(p: number) {
    if (total === 0) return;
    p = Math.max(0, Math.min(total - 1, p));
    selectedPos = p;
    follow = false;
    scrollToPos(p);
    const row = rows[p - rowsFrom];
    if (row) onselect(row.no, p);
    else pendingPos = p;
  }

  export function reveal(pos: number) {
    selectPos(pos);
  }

  function key(e: KeyboardEvent) {
    const cur = selectedPos >= 0 ? selectedPos : rows.findIndex((r) => r.no === selectedNo) + rowsFrom;
    const page = Math.max(1, visible - 2);
    let next: number | null = null;
    switch (e.key) {
      case "ArrowDown": next = cur + 1; break;
      case "ArrowUp": next = cur - 1; break;
      case "PageDown": next = cur + page; break;
      case "PageUp": next = cur - page; break;
      case "Home": next = 0; break;
      case "End": next = total - 1; break;
      case "Enter": {
        const row = rows[cur - rowsFrom];
        if (row) onactivate(row);
        return;
      }
      default: return;
    }
    e.preventDefault();
    selectPos(next);
  }

  function fmtTime(r: Row) {
    switch (timeFormat) {
      case "clock": return r.clock.slice(11, 26); // HH:MM:SS.µµµµµµ
      case "date": return r.clock.slice(0, 26);
      case "delta": return r.delta.toFixed(6);
      default: return r.time.toFixed(6);
    }
  }
</script>

<div class="plist" class:noapp={!showApp} style:--tw={timeFormat === "date" ? "200px" : "112px"}>
  <div class="head">
    <span>No.</span><span>Time</span><span>Source</span><span>Destination</span><span>Protocol</span><span class="r">Length</span>
    {#if showApp}<span>App</span>{/if}<span>Info</span>
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="scroll" bind:this={box} onscroll={onScroll} tabindex="0" onkeydown={key} role="grid" aria-rowcount={total}>
    <div class="spacer" style:height="{virtH}px">
      <div class="win" style:transform="translateY({windowTop}px)">
        {#each rows.slice(Math.max(0, first - rowsFrom), Math.max(0, first - rowsFrom) + visible) as r (r.no)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="row c-{r.color}"
            class:sel={r.no === selectedNo}
            class:warn={r.problem === 1}
            class:err={r.problem === 2}
            role="row"
            tabindex="-1"
            onclick={() => {
              const pos = rows.indexOf(r) + rowsFrom;
              selectedPos = pos;
              onselect(r.no, pos);
              box.focus();
            }}
            ondblclick={() => onactivate(r)}
            oncontextmenu={(e) => {
              e.preventDefault();
              const pos = rows.indexOf(r) + rowsFrom;
              selectedPos = pos;
              onselect(r.no, pos);
              onmenu(r, e.clientX, e.clientY);
            }}
          >
            <span class="mono n">{r.no}</span>
            <span class="mono ell" title="{r.clock} · +{r.time.toFixed(6)} s since first packet">{fmtTime(r)}</span>
            <span class="mono ell" title={r.src}>{r.src}</span>
            <span class="mono ell" title={r.dst}>{r.dst}</span>
            <span class="proto">{r.proto}</span>
            <span class="mono r">{r.len}</span>
            {#if showApp}<span class="ell app" title={r.app}>{r.app}</span>{/if}
            <span class="ell info" title={r.info}>{#if r.dir === 2}<i class="out" title="Sent by this PC">↑</i>{:else if r.dir === 1}<i class="in" title="Received by this PC">↓</i>{/if}{r.info}</span>
          </div>
        {/each}
      </div>
    </div>
    {#if total === 0}
      <div class="empty muted">No packets{filtered ? " match the display filter" : " yet"}.</div>
    {/if}
  </div>
</div>

<style>
  .plist { display: flex; flex-direction: column; min-height: 0; height: 100%; font-size: 12.5px; --tw: 112px; --cols: 64px var(--tw) minmax(110px, 1.1fr) minmax(110px, 1.1fr) 82px 60px 96px minmax(200px, 3fr); }
  .plist.noapp { --cols: 64px var(--tw) minmax(110px, 1.1fr) minmax(110px, 1.1fr) 82px 60px minmax(200px, 3fr); }
  .head, .row { display: grid; grid-template-columns: var(--cols); gap: 10px; align-items: center; padding: 0 10px; }
  .head { height: 26px; background: var(--surface-2); border-bottom: 1px solid var(--border); color: var(--text-secondary); font-weight: 600; font-size: 12px; flex: none; }
  .scroll { flex: 1; min-height: 0; overflow-y: auto; overflow-x: hidden; position: relative; outline: none; }
  .spacer { position: relative; }
  .win { position: absolute; left: 0; right: 0; top: 0; will-change: transform; }
  .row { height: 22px; cursor: default; white-space: nowrap; border-bottom: 1px solid color-mix(in srgb, var(--border) 50%, transparent); }
  .row:hover { background: var(--surface-2); }
  .ell { overflow: hidden; text-overflow: ellipsis; }
  .r { text-align: right; }
  .n { color: var(--text-muted); }
  .proto { font-weight: 600; font-size: 11.5px; }
  .app { color: var(--text-secondary); }
  .info i { font-style: normal; font-weight: 700; margin-right: 5px; }
  .info i.out { color: var(--series-5); }
  .info i.in { color: var(--series-3); }
  /* Protocol tints (subtle; colour is never the only signal - see Protocol column) */
  .c-http { background: color-mix(in srgb, var(--series-6) 9%, transparent); }
  .c-tls { background: color-mix(in srgb, var(--series-7) 9%, transparent); }
  .c-dns { background: color-mix(in srgb, var(--series-1) 9%, transparent); }
  .c-media { background: color-mix(in srgb, var(--series-2) 9%, transparent); }
  .c-arp { background: color-mix(in srgb, var(--series-4) 10%, transparent); }
  .c-icmp { background: color-mix(in srgb, var(--series-5) 9%, transparent); }
  .c-udp { background: color-mix(in srgb, var(--series-3) 6%, transparent); }
  .row.warn .info { color: var(--warning); }
  .row.err { background: color-mix(in srgb, var(--critical) 16%, transparent); }
  .row.err .info { color: color-mix(in srgb, var(--critical) 70%, var(--text-primary)); }
  .row.sel, .row.sel:hover { background: var(--accent); color: #fff; }
  .row.sel .n, .row.sel .app, .row.sel .info, .row.sel i { color: #fff; }
  .empty { position: absolute; top: 40px; left: 0; right: 0; text-align: center; }
</style>
