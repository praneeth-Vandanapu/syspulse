<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/AppIcon.svelte
  Author : Praneeth
  License: MIT
  Purpose: Application icon resolved by the backend (cached per icon name),
           with a coloured monogram fallback.
-->
<script lang="ts" module>
  import { api } from "../api";
  const cache = new Map<string, Promise<string | null>>();
  function load(icon: string) {
    if (!cache.has(icon)) cache.set(icon, api.packageIcon(icon).catch(() => null));
    return cache.get(icon)!;
  }
</script>

<script lang="ts">
  let { icon, name, size = 32 }: { icon: string | null; name: string; size?: number } = $props();

  let src = $state<string | null>(null);
  $effect(() => {
    src = null;
    if (icon) {
      const want = icon;
      load(want).then((s) => {
        if (icon === want) src = s;
      });
    }
  });

  const HUES = [210, 25, 160, 45, 330, 120, 260, 0];
  let hue = $derived(HUES[[...name].reduce((a, c) => a + c.charCodeAt(0), 0) % HUES.length]);
</script>

{#if src}
  <img {src} alt="" width={size} height={size} loading="lazy" />
{:else}
  <span class="mono-ic" style:width="{size}px" style:height="{size}px" style:--h={hue} style:font-size="{size * 0.45}px" aria-hidden="true">
    {name.trim().charAt(0).toUpperCase() || "?"}
  </span>
{/if}

<style>
  img { object-fit: contain; flex: none; }
  .mono-ic { flex: none; display: grid; place-items: center; border-radius: 9px; font-weight: 700; color: #fff;
    background: hsl(var(--h) 55% 45%); }
</style>
