<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/Status.svelte
  Author : Praneeth
  License: MIT
  Purpose: Status pill (icon + label + colour, never colour alone).
-->
<script lang="ts">
  import Icon from "./Icon.svelte";
  let { level = "good", text }: { level?: "good" | "warning" | "critical"; text: string } = $props();
  const ICON = { good: "check", warning: "alert", critical: "alert" } as const;
</script>

<span class="pill {level}"><Icon name={ICON[level]} size={12} stroke={2.4} />{text}</span>

<style>
  .pill { display: inline-flex; align-items: center; gap: 4px; font-size: 11.5px; padding: 2px 8px; border-radius: 99px; border: 1px solid; white-space: nowrap; }
  .good { color: var(--good); border-color: color-mix(in srgb, var(--good) 40%, transparent); background: color-mix(in srgb, var(--good) 10%, transparent); }
  .warning { color: var(--warning); border-color: color-mix(in srgb, var(--warning) 40%, transparent); background: color-mix(in srgb, var(--warning) 10%, transparent); }
  .critical { color: var(--critical); border-color: color-mix(in srgb, var(--critical) 45%, transparent); background: color-mix(in srgb, var(--critical) 12%, transparent); }
</style>
