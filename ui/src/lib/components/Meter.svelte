<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Meter.svelte
  Author : Praneeth
  License: MIT
  Purpose: Labelled horizontal usage bar (0-100).
-->
<script lang="ts">
  let {
    value = 0,
    label = "",
    right = "",
    color = "var(--series-1)",
    height = 8,
  }: { value?: number | null; label?: string; right?: string; color?: string; height?: number } = $props();
  let w = $derived(Math.max(0, Math.min(100, value ?? 0)));
</script>

<div class="meter">
  {#if label || right}
    <div class="row">
      <span class="l">{label}</span>
      <span class="r num">{right}</span>
    </div>
  {/if}
  <div class="track" style:height="{height}px">
    <div class="fill" style:width="{w}%" style:background={color}></div>
  </div>
</div>

<style>
  .meter { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .row { display: flex; justify-content: space-between; gap: 8px; font-size: 12.5px; }
  .l { color: var(--text-secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .r { color: var(--text-primary); white-space: nowrap; }
  .track { background: var(--surface-3); border-radius: 99px; overflow: hidden; }
  .fill { height: 100%; border-radius: 99px; }
</style>
