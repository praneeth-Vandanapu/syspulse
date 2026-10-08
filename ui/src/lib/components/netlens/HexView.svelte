<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/netlens/HexView.svelte
  Author : Praneeth
  License: MIT
  Purpose: Packet bytes: offset, 16 hex bytes and ASCII per row. The bytes of
           the selected tree node are highlighted; clicking a byte selects
           the field it belongs to. Very large frames show the first 16 KiB
           until "Show all" is pressed.
-->
<script lang="ts">
  let {
    bytes,
    start = 0,
    len = 0,
    onpick,
  }: { bytes: Uint8Array; start?: number; len?: number; onpick: (offset: number) => void } = $props();

  const HEX = Array.from({ length: 256 }, (_, i) => i.toString(16).padStart(2, "0"));
  const LIMIT = 16384;
  let all = $state(false);
  let shown = $derived(all ? bytes.length : Math.min(bytes.length, LIMIT));
  let rows = $derived(Array.from({ length: Math.ceil(shown / 16) }, (_, r) => r * 16));
  const inRange = (i: number) => len > 0 && i >= start && i < start + len;
  const ch = (b: number) => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : "·");

  let box: HTMLDivElement = $state()!;
  // Scroll the highlight into view when the selection changes.
  $effect(() => {
    if (!box || len === 0) return;
    const row = box.querySelector<HTMLElement>(`[data-row="${Math.floor(start / 16) * 16}"]`);
    row?.scrollIntoView({ block: "nearest" });
  });
</script>

<div class="hex mono" bind:this={box}>
  {#each rows as off (off)}
    <div class="r" data-row={off}>
      <span class="off">{off.toString(16).padStart(4, "0")}</span>
      <span class="bytes">
        {#each { length: 16 } as _, k}
          {@const i = off + k}
          {#if i < shown}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <span class="b" class:hl={inRange(i)} class:gap={k === 8} onclick={() => onpick(i)}>{HEX[bytes[i]]}</span>
          {:else}
            <span class="b empty" class:gap={k === 8}>&nbsp;&nbsp;</span>
          {/if}
        {/each}
      </span>
      <span class="asc">
        {#each { length: Math.min(16, shown - off) } as _, k}
          {@const i = off + k}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <span class:hl={inRange(i)} onclick={() => onpick(i)}>{ch(bytes[i])}</span>
        {/each}
      </span>
    </div>
  {/each}
  {#if bytes.length > shown}
    <button class="more" onclick={() => (all = true)}>Show all {bytes.length} bytes</button>
  {/if}
</div>

<style>
  .hex { font-size: 12px; padding: 6px 10px; line-height: 19px; }
  .r { display: flex; gap: 14px; white-space: pre; }
  .off { color: var(--text-muted); }
  .bytes { display: inline-flex; gap: 0; }
  .b { padding: 0 3px; border-radius: 3px; cursor: pointer; }
  .b.gap { margin-left: 8px; }
  .b.empty { cursor: default; }
  .asc span { cursor: pointer; }
  .hl { background: color-mix(in srgb, var(--accent) 40%, transparent); color: var(--text-primary); }
  .b:hover:not(.empty), .asc span:hover { outline: 1px solid var(--accent); }
  .asc { color: var(--text-secondary); }
  .more { margin-top: 6px; font-size: 12px; }
</style>
