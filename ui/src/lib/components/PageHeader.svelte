<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/PageHeader.svelte
  Author : Praneeth
  License: MIT
  Purpose: Page title row with optional subtitle and right-aligned actions.
-->
<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, subtitle = "", actions }: { title: string; subtitle?: string; actions?: Snippet } = $props();
</script>

<header>
  <div>
    <h1>{title}</h1>
    {#if subtitle}<p>{subtitle}</p>{/if}
  </div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  header { display: flex; align-items: flex-end; justify-content: space-between; gap: 16px; margin-bottom: 18px; flex-wrap: wrap; }
  p { margin: 4px 0 0; color: var(--text-secondary); font-size: 13px; }
  .actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
</style>
