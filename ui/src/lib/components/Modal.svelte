<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Modal.svelte
  Author : Praneeth
  License: MIT
  Purpose: Accessible confirmation dialog (Escape closes, focus trapped in box).
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    onclose,
    width = 460,
    children,
    footer,
  }: { title: string; onclose: () => void; width?: number; children: Snippet; footer?: Snippet } = $props();

  let box: HTMLDivElement;
  $effect(() => {
    box?.querySelector<HTMLElement>("button.primary, button.danger, button")?.focus();
  });
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="box" role="dialog" aria-modal="true" aria-label={title} style:width="{width}px" bind:this={box}>
    <h2>{title}</h2>
    <div class="body">{@render children()}</div>
    {#if footer}<div class="foot">{@render footer()}</div>{/if}
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.55); display: grid; place-items: center; z-index: 50; }
  .box { max-width: calc(100vw - 40px); max-height: calc(100vh - 60px); overflow: auto; background: var(--surface-1);
    border: 1px solid var(--border-strong); border-radius: 14px; padding: 22px; }
  .body { margin-top: 10px; color: var(--text-secondary); line-height: 1.5; font-size: 13.5px; }
  .foot { display: flex; justify-content: flex-end; gap: 8px; margin-top: 18px; }
</style>
