<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Treemap.svelte
  Author : Praneeth
  License: MIT
  Purpose: Zoomable disk-usage map (squarified treemap). Each tile is a
           folder sized by its total; the next level is drawn inside the
           larger tiles. Click to zoom in, use the breadcrumb to zoom out.
-->
<script lang="ts">
  import { bytes } from "../format";
  import type { DirNode } from "../types";

  let { root, onscan }: { root: DirNode; onscan?: (path: string) => void } = $props();

  interface Rect { x: number; y: number; w: number; h: number }

  const COLORS = ["--series-1", "--series-2", "--series-3", "--series-7", "--series-5", "--series-4", "--series-6", "--series-8"];

  let stack = $state<DirNode[]>([]);
  let width = $state(800);
  let height = $state(460);
  let hover = $state<DirNode | null>(null);

  $effect(() => {
    // New scan result: reset zoom.
    stack = [root];
  });
  let node = $derived(stack[stack.length - 1] ?? root);

  /** Squarified layout (Bruls et al.) of values sorted descending. */
  function squarify(values: number[], x: number, y: number, w: number, h: number): Rect[] {
    const total = values.reduce((a, b) => a + b, 0);
    if (!total || w <= 0 || h <= 0) return values.map(() => ({ x, y, w: 0, h: 0 }));
    const areas = values.map((v) => (v / total) * w * h);
    const out: Rect[] = new Array(values.length);
    const worst = (row: number[], side: number) => {
      const s = row.reduce((a, b) => a + b, 0);
      return Math.max(...row.map((r) => Math.max((side * side * r) / (s * s), (s * s) / (side * side * r))));
    };
    let i = 0;
    while (i < areas.length) {
      const side = Math.min(w, h);
      let row = [i];
      let best = worst([areas[i]], side);
      let j = i + 1;
      while (j < areas.length) {
        const cand = [...row, j];
        const wr = worst(cand.map((k) => areas[k]), side);
        if (wr > best) break;
        row = cand;
        best = wr;
        j++;
      }
      const rowArea = row.reduce((a, k) => a + areas[k], 0);
      if (w >= h) {
        const cw = rowArea / h;
        let yy = y;
        for (const k of row) {
          const hh = areas[k] / cw;
          out[k] = { x, y: yy, w: cw, h: hh };
          yy += hh;
        }
        x += cw;
        w -= cw;
      } else {
        const rh = rowArea / w;
        let xx = x;
        for (const k of row) {
          const ww = areas[k] / rh;
          out[k] = { x: xx, y, w: ww, h: rh };
          xx += ww;
        }
        y += rh;
        h -= rh;
      }
      i = j;
    }
    return out;
  }

  let kids = $derived(node.children.filter((c) => c.size > 0));
  let rects = $derived(squarify(kids.map((c) => c.size), 0, 0, width, height));

  function inner(c: DirNode, r: Rect) {
    const sub = c.children.filter((x) => x.size > 0);
    if (!sub.length || r.w < 90 || r.h < 60) return [];
    const pad = 4;
    const top = 20;
    const rs = squarify(sub.map((x) => x.size), pad, top, r.w - pad * 2, r.h - top - pad);
    return sub.map((x, i) => ({ node: x, rect: rs[i] })).filter((x) => x.rect.w > 3 && x.rect.h > 3);
  }

  function zoom(c: DirNode) {
    if (c.kind === "dir" && c.children.length) stack = [...stack, c];
    else if (c.kind === "dir" && onscan) onscan(c.path);
  }
  const color = (i: number) => `var(${COLORS[i % COLORS.length]})`;
  const pct = (c: DirNode) => ((c.size / (node.size || 1)) * 100).toFixed(1);
</script>

<div class="tm">
  <nav class="crumbs" aria-label="Map location">
    {#each stack as s, i (s.path + i)}
      {#if i > 0}<span class="sep">›</span>{/if}
      <button class="crumb" disabled={i === stack.length - 1} onclick={() => (stack = stack.slice(0, i + 1))}>{i === 0 ? s.name : s.name}</button>
    {/each}
    <span class="grow"></span>
    <span class="muted num">{bytes(node.size)}</span>
    {#if hover}<span class="hov">{hover.name} · <b class="num">{bytes(hover.size)}</b> · {pct(hover)}%</span>{/if}
  </nav>
  <div class="area" bind:clientWidth={width} bind:clientHeight={height} role="tree">
    {#each kids as c, i (c.path + c.name)}
      {@const r = rects[i]}
      {#if r && r.w > 1 && r.h > 1}
        <button
          class="tile {c.kind}"
          style:left="{r.x}px" style:top="{r.y}px" style:width="{r.w}px" style:height="{r.h}px" style:--c={color(i)}
          onclick={() => zoom(c)}
          onmouseenter={() => (hover = c)}
          onmouseleave={() => (hover = null)}
          title="{c.path} — {bytes(c.size)}{c.kind === 'dir' ? (c.children.length ? ' (click to zoom)' : ' (click to scan)') : ''}"
          role="treeitem"
          aria-selected="false"
        >
          {#if r.w > 60 && r.h > 28}
            <span class="lbl"><b>{c.name}</b> <span class="num">{bytes(c.size)}</span></span>
          {/if}
          {#each inner(c, r) as s (s.node.path + s.node.name)}
            <span class="sub" style:left="{s.rect.x}px" style:top="{s.rect.y}px" style:width="{s.rect.w}px" style:height="{s.rect.h}px">
              {#if s.rect.w > 70 && s.rect.h > 22}<span class="slbl">{s.node.name}</span>{/if}
            </span>
          {/each}
        </button>
      {/if}
    {/each}
  </div>
</div>

<style>
  .tm { display: flex; flex-direction: column; gap: 8px; height: 100%; }
  .crumbs { display: flex; align-items: center; gap: 4px; font-size: 12.5px; flex-wrap: wrap; min-height: 26px; }
  .crumb { all: unset; cursor: pointer; color: var(--accent); max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .crumb:disabled { color: var(--text-primary); cursor: default; font-weight: 600; }
  .sep { color: var(--text-muted); }
  .grow { flex: 1; }
  .hov { margin-left: 12px; color: var(--text-secondary); }
  .area { position: relative; flex: 1; min-height: 420px; border-radius: 10px; overflow: hidden; background: var(--surface-2); }
  .tile { position: absolute; padding: 0; margin: 0; border: 2px solid var(--bg); border-radius: 6px; overflow: hidden; cursor: pointer; text-align: left;
    background: color-mix(in srgb, var(--c) 72%, var(--surface-1)); color: #fff; box-sizing: border-box; }
  .tile:hover { background: color-mix(in srgb, var(--c) 88%, white 6%); }
  .tile.files { background: color-mix(in srgb, var(--c) 30%, var(--surface-2)); }
  .tile.other { background: repeating-linear-gradient(45deg, color-mix(in srgb, var(--c) 28%, var(--surface-2)) 0 6px, color-mix(in srgb, var(--c) 18%, var(--surface-2)) 6px 12px); }
  .lbl { position: absolute; top: 3px; left: 6px; right: 6px; font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5); z-index: 1; }
  .lbl .num { opacity: 0.85; margin-left: 4px; }
  .sub { position: absolute; border: 1px solid color-mix(in srgb, var(--c) 40%, black 30%); border-radius: 3px; background: rgba(255, 255, 255, 0.07); box-sizing: border-box; overflow: hidden; pointer-events: none; }
  .slbl { position: absolute; left: 4px; top: 2px; right: 4px; font-size: 10.5px; opacity: 0.8; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
