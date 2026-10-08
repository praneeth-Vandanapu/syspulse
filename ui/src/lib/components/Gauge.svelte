<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Gauge.svelte
  Author : Praneeth
  License: MIT
  Purpose: Animated 270-degree radial gauge for a single 0-100 value.
-->
<script lang="ts">
  let {
    value = null,
    label = "",
    detail = "",
    color = "var(--series-1)",
    size = 150,
  }: { value?: number | null; label?: string; detail?: string; color?: string; size?: number } = $props();

  const R = 52;
  const C = 2 * Math.PI * R;
  const ARC = C * 0.75; // 270 degrees
  let shown = $derived(value == null ? 0 : Math.max(0, Math.min(100, value)));
  let dash = $derived(`${(ARC * shown) / 100} ${C}`);
  const uid = Math.random().toString(36).slice(2, 8);
</script>

<div class="gauge" style:width="{size}px" style:height="{size}px" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={value ?? undefined}>
  <svg viewBox="0 0 120 120">
    <defs>
      <linearGradient id="g-{uid}" x1="0" y1="1" x2="1" y2="0">
        <stop offset="0%" stop-color={color} stop-opacity="0.55" />
        <stop offset="100%" stop-color={color} />
      </linearGradient>
    </defs>
    <circle cx="60" cy="60" r={R} class="track" stroke-dasharray="{ARC} {C}" transform="rotate(135 60 60)" />
    {#if shown >= 0.5}
      <!-- Cheap glow: a wider translucent stroke instead of an SVG blur filter (WebKitGTK renders filters in software). -->
      <circle cx="60" cy="60" r={R} class="halo" stroke={color} stroke-dasharray={dash} transform="rotate(135 60 60)" />
      <circle cx="60" cy="60" r={R} class="bar" stroke="url(#g-{uid})" stroke-dasharray={dash} transform="rotate(135 60 60)" />
    {/if}
  </svg>
  <div class="center">
    <div class="val num">{value == null ? "—" : Math.round(value)}<span class="unit">{value == null ? "" : "%"}</span></div>
    <div class="lbl">{label}</div>
    {#if detail}<div class="det">{detail}</div>{/if}
  </div>
</div>

<style>
  .gauge { position: relative; flex: none; }
  svg { width: 100%; height: 100%; }
  circle { fill: none; stroke-width: 9; stroke-linecap: round; }
  .track { stroke: var(--surface-3); }
  .halo { stroke-width: 15; opacity: 0.12; }
  .center { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; padding-top: 6px; }
  .val { font-size: 30px; font-weight: 650; letter-spacing: -0.02em; line-height: 1; }
  .unit { font-size: 14px; color: var(--text-secondary); margin-left: 1px; }
  .lbl { margin-top: 6px; font-size: 12px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.08em; }
  .det { font-size: 11px; color: var(--text-muted); margin-top: 2px; max-width: 80%; text-align: center; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
