<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/pages/RegionPicker.svelte
  Author : Praneeth
  License: MIT
  Purpose: Full-screen region selector for the screen recorder. Shows a
           frozen snapshot of the monitor; drag to select, adjust, then
           Enter / double-click to confirm or Esc to cancel.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { rec, type RegionSnapshot } from "../lib/recorder";

  let snap = $state<RegionSnapshot | null>(null);
  let start = $state<{ x: number; y: number } | null>(null);
  let sel = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  let dragging = $state(false);
  let vw = $state(window.innerWidth);
  let vh = $state(window.innerHeight);

  onMount(async () => {
    try {
      snap = await rec.regionSnapshot();
    } catch {
      rec.regionDone(null);
    }
  });

  // CSS pixels -> screen pixels of the monitor.
  let sx = $derived(snap ? snap.width / vw : 1);
  let sy = $derived(snap ? snap.height / vh : 1);
  let real = $derived(sel && snap ? { x: Math.round(snap.x + sel.x * sx), y: Math.round(snap.y + sel.y * sy), width: Math.round(sel.w * sx) & ~1, height: Math.round(sel.h * sy) & ~1 } : null);

  function down(e: PointerEvent) {
    start = { x: e.clientX, y: e.clientY };
    sel = { x: e.clientX, y: e.clientY, w: 0, h: 0 };
    dragging = true;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (!dragging || !start) return;
    sel = { x: Math.min(start.x, e.clientX), y: Math.min(start.y, e.clientY), w: Math.abs(e.clientX - start.x), h: Math.abs(e.clientY - start.y) };
  }
  function up() {
    dragging = false;
    if (sel && (sel.w < 8 || sel.h < 8)) sel = null;
  }
  function confirm() {
    if (real && real.width >= 16 && real.height >= 16) rec.regionDone(real);
  }
  function whole() {
    sel = { x: 0, y: 0, w: vw, h: vh };
  }
  function key(e: KeyboardEvent) {
    if (e.key === "Escape") rec.regionDone(null);
    else if (e.key === "Enter") confirm();
    else if (e.key.toLowerCase() === "a") whole();
  }
</script>

<svelte:window onkeydown={key} onresize={() => ((vw = window.innerWidth), (vh = window.innerHeight))} />

<div class="picker" role="application" aria-label="Select a screen region" style:background-image={snap ? `url(${snap.image})` : "none"}
  onpointerdown={down} onpointermove={move} onpointerup={up} ondblclick={confirm}>
  {#if sel}
    <div class="shade" style:clip-path="polygon(evenodd, 0 0, 100% 0, 100% 100%, 0 100%, 0 0, {sel.x}px {sel.y}px, {sel.x}px {sel.y + sel.h}px, {sel.x + sel.w}px {sel.y + sel.h}px, {sel.x + sel.w}px {sel.y}px, {sel.x}px {sel.y}px)"></div>
    <div class="box" style:left="{sel.x}px" style:top="{sel.y}px" style:width="{sel.w}px" style:height="{sel.h}px">
      {#if real}<span class="dim">{real.width} × {real.height}</span>{/if}
    </div>
    {#if !dragging && real}
      <div class="bar" style:left="{Math.min(sel.x, vw - 330)}px" style:top="{sel.y + sel.h + 10 > vh - 50 ? sel.y - 50 : sel.y + sel.h + 10}px" role="toolbar" tabindex="-1" onpointerdown={(e) => e.stopPropagation()}>
        <button class="ok" onclick={confirm}>Record this area</button>
        <button onclick={() => (sel = null)}>Redraw</button>
        <button onclick={() => rec.regionDone(null)}>Cancel</button>
      </div>
    {/if}
  {:else}
    <div class="shade full"></div>
    <div class="help">Drag to select the area to record · <b>A</b> whole screen · <b>Enter</b> confirm · <b>Esc</b> cancel</div>
  {/if}
</div>

<style>
  :global(html), :global(body) { background: #000 !important; overflow: hidden; }
  .picker { position: fixed; inset: 0; background-size: 100% 100%; cursor: crosshair; user-select: none; }
  .shade { position: absolute; inset: 0; background: rgba(0, 0, 0, 0.45); pointer-events: none; }
  .box { position: absolute; border: 2px solid #3987e5; pointer-events: none; box-sizing: border-box; }
  .dim { position: absolute; top: -26px; left: 0; background: #3987e5; color: #fff; font: 600 12px var(--font-sans); padding: 3px 8px; border-radius: 6px; white-space: nowrap; }
  .help { position: absolute; top: 24px; left: 50%; transform: translateX(-50%); background: rgba(20, 20, 24, 0.9); color: #fff; padding: 10px 16px; border-radius: 10px; font: 14px var(--font-sans); }
  .bar { position: absolute; display: flex; gap: 6px; background: rgba(20, 20, 24, 0.95); padding: 6px; border-radius: 10px; cursor: default; }
  .bar button { font: 13px var(--font-sans); padding: 6px 12px; border-radius: 7px; border: 1px solid rgba(255, 255, 255, 0.2); background: rgba(255, 255, 255, 0.08); color: #fff; cursor: pointer; }
  .bar .ok { background: #d03b3b; border-color: transparent; }
</style>
