<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/ForgeLog.svelte
  Author : Praneeth
  License: MIT
  Purpose: PacketForge traffic view. Renders the newest records of a session
           in the chosen notation (text with escapes, hex, text+hex, hexdump,
           decimal, binary) with timestamps, direction, peers, highlighting
           and filtering. Only a window of rows is in the DOM; older ones are
           revealed on demand to keep WebKitGTK fast.
-->
<script lang="ts">
  import { contains, toBin, toDec, toDump, toHex, toText, timeLabel, type Encoding, type Format } from "../forgefmt";
  import type { Rec, SessionView } from "../forge.svelte";

  export interface LogOptions {
    format: Format;
    encoding: Encoding;
    timestamps: "none" | "absolute" | "relative" | "delta";
    hercules: boolean;
    showRx: boolean;
    showTx: boolean;
    showInfo: boolean;
    wrap: boolean;
    highlight: Uint8Array | null;
    filterToHighlight: boolean;
    follow: boolean;
  }

  let { session, opts }: { session: SessionView; opts: LogOptions } = $props();

  const PAGE = 400;
  let limit = $state(PAGE);
  let box: HTMLDivElement;
  let atBottom = true;

  let all = $derived.by(() => {
    void session.version; // re-run when records change
    const hl = opts.highlight;
    return session.records.filter((r) => {
      if (r.dir === "rx" && !opts.showRx) return false;
      if (r.dir === "tx" && !opts.showTx) return false;
      if (r.dir === "info" && !opts.showInfo) return false;
      if (opts.filterToHighlight && hl && hl.length && (r.dir === "info" || !contains(r.bytes, hl, true))) return false;
      return true;
    });
  });
  let visible = $derived(all.slice(Math.max(0, all.length - limit)));
  let hidden = $derived(all.length - visible.length);

  $effect(() => {
    void visible;
    if (opts.follow && atBottom && box) {
      queueMicrotask(() => (box.scrollTop = box.scrollHeight));
    }
  });

  function onScroll() {
    atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  }

  const prevT = (i: number) => (i > 0 ? visible[i - 1].t : null);
  const isHl = (r: Rec) => !!opts.highlight?.length && r.dir !== "info" && contains(r.bytes, opts.highlight, true);
  const peerOf = (r: Rec) => (session.config.kind === "tcpServer" || session.config.kind === "udp" ? r.peer : null);
</script>

<div class="log" class:nowrap={!opts.wrap} bind:this={box} onscroll={onScroll} role="log" aria-live="off">
  {#if hidden > 0}
    <button class="earlier" onclick={() => (limit += PAGE)}>Show {Math.min(PAGE, hidden)} earlier of {hidden.toLocaleString()} hidden</button>
  {/if}
  {#each visible as r, i (r.seq + ":" + r.bytes.length)}
    {#if r.dir === "info"}
      <div class="row info {r.level}">
        {#if opts.timestamps !== "none"}<span class="ts">{timeLabel(opts.timestamps, r.t, session.startT, prevT(i))}</span>{/if}
        <span class="note">— {r.note}</span>
      </div>
    {:else}
      <div class="row {r.dir}" class:hl={isHl(r)} class:open={r.open}>
        {#if opts.timestamps !== "none"}<span class="ts">{timeLabel(opts.timestamps, r.t, session.startT, prevT(i))}</span>{/if}
        <span class="dir">{r.dir === "rx" ? "RX" : "TX"}</span>
        {#if peerOf(r)}<span class="peer">{r.dir === "rx" ? "←" : "→"} {r.peer}</span>{/if}
        <span class="len">{r.bytes.length}</span>
        <span class="data">
          {#if opts.format === "text" || opts.format === "mixed"}
            <span class="txt">{#each toText(r.bytes, opts.encoding, opts.hercules) as seg, k (k)}{#if seg.ctrl}<span class="ctrl">{seg.t}</span>{:else}{seg.t}{/if}{/each}</span>
            {#if opts.format === "mixed"}<span class="sub">{toHex(r.bytes, 512)}</span>{/if}
          {:else if opts.format === "hex"}
            {toHex(r.bytes)}
          {:else if opts.format === "dump"}
            <pre class="dump">{toDump(r.bytes).join("\n")}</pre>
          {:else if opts.format === "dec"}
            {toDec(r.bytes)}
          {:else}
            {toBin(r.bytes)}
          {/if}
        </span>
      </div>
    {/if}
  {:else}
    <div class="empty">No traffic yet.</div>
  {/each}
</div>

<style>
  .log { font-family: var(--font-mono); font-size: 12.5px; line-height: 1.55; overflow: auto; height: 100%; background: var(--surface-1); padding: 6px 0; }
  .row { display: flex; gap: 10px; padding: 1px 12px; align-items: baseline; border-left: 3px solid transparent; }
  .row:hover { background: var(--surface-2); }
  .ts { color: var(--text-muted); flex: none; min-width: 92px; font-size: 11.5px; }
  .dir { flex: none; font-size: 10.5px; font-weight: 700; padding: 0 5px; border-radius: 4px; font-family: var(--font-sans); }
  .rx .dir { color: var(--series-3); background: color-mix(in srgb, var(--series-3) 14%, transparent); }
  .tx .dir { color: var(--series-1); background: color-mix(in srgb, var(--series-1) 14%, transparent); }
  .tx { color: color-mix(in srgb, var(--series-1) 55%, var(--text-primary)); }
  .peer { flex: none; color: var(--text-muted); font-size: 11.5px; }
  .len { flex: none; color: var(--text-muted); font-size: 11px; min-width: 32px; text-align: right; }
  .len::after { content: " B"; }
  .data { min-width: 0; white-space: pre-wrap; word-break: break-all; }
  .nowrap .data { white-space: pre; word-break: normal; }
  .ctrl { color: var(--warning); opacity: 0.85; }
  .sub { display: block; color: var(--text-muted); font-size: 11.5px; }
  .dump { margin: 0; font: inherit; }
  /* Frame still receiving (static marker: animations cost CPU in WebKitGTK). */
  .open .data::after { content: "▍"; color: var(--accent); }
  .hl { border-left-color: var(--warning); background: color-mix(in srgb, var(--warning) 10%, transparent); }
  .info { color: var(--text-muted); font-family: var(--font-sans); font-size: 12px; }
  .info.error { color: var(--critical); }
  .info.open, .info.listening { color: var(--good); }
  .note { font-style: italic; }
  .earlier { margin: 4px 12px 8px; font-size: 12px; padding: 3px 10px; }
  .empty { padding: 30px; text-align: center; color: var(--text-muted); font-family: var(--font-sans); }
</style>
