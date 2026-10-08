<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Sparkline.svelte
  Author : Praneeth
  License: MIT
  Purpose: Tiny SVG area sparkline (0-100 scale) for dense grids like per-core CPU.
-->
<script lang="ts">
  let {
    values,
    color = "var(--series-1)",
    height = 36,
    max = 100,
    points = 60,
  }: { values: (number | null)[]; color?: string; height?: number; max?: number; points?: number } = $props();

  const W = 100;
  let path = $derived.by(() => {
    const v = values.slice(-points);
    if (v.length < 2) return { line: "", area: "" };
    const step = W / (points - 1);
    const off = (points - v.length) * step;
    const pts = v.map((y, i) => [off + i * step, height - (Math.min(max, y ?? 0) / max) * (height - 2) - 1]);
    const line = pts.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(2)},${y.toFixed(2)}`).join("");
    const area = `${line}L${pts[pts.length - 1][0].toFixed(2)},${height}L${pts[0][0].toFixed(2)},${height}Z`;
    return { line, area };
  });
</script>

<svg viewBox="0 0 {W} {height}" preserveAspectRatio="none" style:height="{height}px" aria-hidden="true">
  <path d={path.area} fill={color} fill-opacity="0.18" />
  <path d={path.line} fill="none" stroke={color} stroke-width="1.5" vector-effect="non-scaling-stroke" />
</svg>

<style>
  svg { width: 100%; display: block; }
</style>
