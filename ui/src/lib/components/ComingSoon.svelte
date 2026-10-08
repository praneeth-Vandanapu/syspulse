<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/ComingSoon.svelte
  Author : Praneeth
  License: MIT
  Purpose: Placeholder for modules scheduled in later roadmap phases.
-->
<script lang="ts">
  import Icon from "./Icon.svelte";
  let { icon, title, phase, features }: { icon: string; title: string; phase: string; features: string[] } = $props();
</script>

<div class="soon">
  <div class="badge"><Icon name={icon} size={30} /></div>
  <h1>{title}</h1>
  <p class="secondary">Planned for <b>{phase}</b> of the roadmap.</p>
  <ul>
    {#each features as f (f)}<li><Icon name="check" size={14} />{f}</li>{/each}
  </ul>
</div>

<style>
  .soon { max-width: 560px; margin: 8vh auto; text-align: center; display: flex; flex-direction: column; align-items: center; gap: 10px; }
  .badge { width: 64px; height: 64px; border-radius: 18px; display: grid; place-items: center; color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent); border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent); }
  ul { list-style: none; padding: 0; margin: 10px 0 0; text-align: left; display: flex; flex-direction: column; gap: 8px; }
  li { display: flex; gap: 8px; align-items: center; color: var(--text-secondary); }
  li :global(svg) { color: var(--good); flex: none; }
</style>
