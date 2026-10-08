<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Card.svelte
  Author : Praneeth
  License: MIT
  Purpose: Glass panel container with optional title, icon and header actions.
-->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let {
    title = "",
    icon = "",
    subtitle = "",
    pad = true,
    actions,
    children,
  }: { title?: string; icon?: string; subtitle?: string; pad?: boolean; actions?: Snippet; children: Snippet } = $props();
</script>

<section class="card" class:pad>
  {#if title}
    <header>
      <div class="ttl">
        {#if icon}<span class="ic"><Icon name={icon} size={16} /></span>{/if}
        <h3>{title}</h3>
        {#if subtitle}<span class="sub">{subtitle}</span>{/if}
      </div>
      {#if actions}<div class="actions">{@render actions()}</div>{/if}
    </header>
  {/if}
  {@render children()}
</section>

<style>
  .card {
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .pad { padding: 16px 18px; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 12px; }
  .ttl { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .ic { color: var(--text-secondary); display: inline-flex; }
  h3 { font-size: 13px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  .sub { color: var(--text-muted); font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .actions { display: flex; gap: 6px; align-items: center; }
</style>
