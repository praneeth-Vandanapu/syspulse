<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/netlens/FollowStream.svelte
  Author : Praneeth
  License: MIT
  Purpose: Follow TCP/UDP Stream: the reassembled conversation as text or hex
           dump, client data and server data in two colours, gaps marked.
-->
<script lang="ts">
  import Modal from "../Modal.svelte";
  import { toDump, toText } from "../../forgefmt";
  import { b64bytes, type FollowResult } from "../../netlens";
  import { bytes as fmtBytes } from "../../format";

  let {
    result,
    onclose,
    onfilter,
  }: { result: FollowResult; onclose: () => void; onfilter: (f: string) => void } = $props();

  let mode = $state<"text" | "hex">("text");
  let show = $state<"both" | "a" | "b">("both");
  const MAX_CHUNKS = 3000;

  let chunks = $derived(
    result.chunks
      .filter((c) => show === "both" || (show === "a") === c.fromA)
      .slice(0, MAX_CHUNKS)
      .map((c) => ({ ...c, bytes: b64bytes(c.data) })),
  );

  async function copy() {
    const text = chunks.map((c) => new TextDecoder().decode(c.bytes)).join("");
    await navigator.clipboard.writeText(text);
  }
</script>

<Modal title="Follow {result.tcp ? 'TCP' : 'UDP'} stream {result.stream}" {onclose} width={940}>
  <div class="hdr">
    <span class="a">● {result.a}</span><span class="muted">→ sent {fmtBytes(result.bytesA)}</span>
    <span class="b">● {result.b}</span><span class="muted">→ sent {fmtBytes(result.bytesB)}</span>
  </div>
  <div class="bar">
    <div class="seg">
      <button class:on={mode === "text"} onclick={() => (mode = "text")}>Text</button>
      <button class:on={mode === "hex"} onclick={() => (mode = "hex")}>Hex dump</button>
    </div>
    <div class="seg">
      <button class:on={show === "both"} onclick={() => (show = "both")}>Whole conversation</button>
      <button class:on={show === "a"} onclick={() => (show = "a")}>{result.a} only</button>
      <button class:on={show === "b"} onclick={() => (show = "b")}>{result.b} only</button>
    </div>
    <span class="grow"></span>
    <button onclick={copy}>Copy text</button>
    <button onclick={() => onfilter(result.filter)}>Show these packets</button>
  </div>
  <div class="body mono">
    {#if chunks.length === 0}
      <p class="muted">This stream carries no payload (only handshakes / acknowledgements).</p>
    {/if}
    {#each chunks as c, i (i)}
      {#if c.gap > 0}<div class="gapline">[{c.gap} bytes missing — not captured]</div>{/if}
      <div class="chunk" class:fa={c.fromA} class:fb={!c.fromA} title="Starts in frame {c.frame}">
        {#if mode === "text"}
          {#each toText(c.bytes, "utf8", false, true) as s, j (j)}<span class:ctrl={s.ctrl}>{s.t}</span>{/each}
        {:else}
          {#each toDump(c.bytes) as line, j (j)}<div>{line}</div>{/each}
        {/if}
      </div>
    {/each}
    {#if result.truncated || result.chunks.length > MAX_CHUNKS}
      <p class="muted">Showing the first part of this stream only.</p>
    {/if}
  </div>
</Modal>

<style>
  .hdr { display: flex; gap: 10px; flex-wrap: wrap; font-size: 13px; margin-bottom: 8px; }
  .a { color: var(--series-8); font-weight: 600; }
  .b { color: var(--series-1); font-weight: 600; margin-left: 12px; }
  .bar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 8px; }
  .bar button { font-size: 12px; padding: 3px 10px; }
  .seg { display: inline-flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 8px; padding: 2px; gap: 2px; }
  .seg button { border: none; background: transparent; color: var(--text-secondary); }
  .seg button.on { background: var(--surface-3); color: var(--text-primary); }
  .grow { flex: 1; }
  .body { height: 58vh; overflow: auto; background: var(--surface-1); border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; font-size: 12px; }
  .chunk { white-space: pre-wrap; word-break: break-all; padding: 2px 6px; border-left: 3px solid; margin-bottom: 2px; }
  .chunk.fa { color: color-mix(in srgb, var(--series-8) 75%, var(--text-primary)); border-color: var(--series-8); background: color-mix(in srgb, var(--series-8) 7%, transparent); }
  .chunk.fb { color: color-mix(in srgb, var(--series-1) 75%, var(--text-primary)); border-color: var(--series-1); background: color-mix(in srgb, var(--series-1) 7%, transparent); }
  .ctrl { opacity: 0.55; }
  .gapline { color: var(--warning); font-style: italic; margin: 4px 0; }
</style>
