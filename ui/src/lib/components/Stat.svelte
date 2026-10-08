<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Stat.svelte
  Author : Praneeth
  License: MIT
  Purpose: Small key/value statistic tile.
-->
<script lang="ts">
  let { label, value, hint = "" }: { label: string; value: string; hint?: string } = $props();
</script>

<div class="stat">
  <div class="label">{label}</div>
  <div class="value num">{value}</div>
  {#if hint}<div class="hint">{hint}</div>{/if}
</div>

<style>
  .stat { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .label { font-size: 11.5px; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.06em; }
  .value { font-size: 17px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .hint { font-size: 11.5px; color: var(--text-secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
