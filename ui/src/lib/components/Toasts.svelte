<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Toasts.svelte
  Author : Praneeth
  License: MIT
  Purpose: Renders app-wide notifications from app.notify().
-->
<script lang="ts">
  import Icon from "./Icon.svelte";
  import { app } from "../stores.svelte";
</script>

<div class="stack" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="toast {t.kind}" role="status">
      <Icon name={t.kind === "error" ? "alert" : t.kind === "info" ? "info" : "check"} size={15} />
      <span>{t.text}</span>
    </div>
  {/each}
</div>

<style>
  .stack { position: fixed; bottom: 20px; right: 20px; display: flex; flex-direction: column; gap: 8px; z-index: 70; max-width: 460px; }
  .toast { display: flex; gap: 8px; align-items: flex-start; background: var(--surface-2); border: 1px solid var(--border-strong);
    border-left: 3px solid var(--good); padding: 10px 14px; border-radius: 10px; font-size: 13px; line-height: 1.4; }
  .toast :global(svg) { flex: none; margin-top: 1px; }
  .toast.ok :global(svg) { color: var(--good); }
  .toast.error { border-left-color: var(--critical); }
  .toast.error :global(svg) { color: var(--critical); }
  .toast.info { border-left-color: var(--accent); }
  .toast.info :global(svg) { color: var(--accent); }
</style>
