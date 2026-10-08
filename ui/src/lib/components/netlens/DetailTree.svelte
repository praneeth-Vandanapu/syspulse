<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/netlens/DetailTree.svelte
  Author : Praneeth
  License: MIT
  Purpose: Packet detail tree (protocol layers and fields). Selecting a node
           highlights its bytes in the hex view; every field can become a
           display filter. Expanded/collapsed state is remembered by layer
           and field name, so it survives moving between packets.
-->
<script lang="ts">
  import type { Node } from "../../netlens";

  let {
    nodes,
    selected,
    onselect,
    onfilter,
  }: {
    nodes: Node[];
    selected: Node | null;
    onselect: (n: Node) => void;
    onfilter: (filter: string, mode: "apply" | "and" | "not") => void;
  } = $props();

  // key -> expanded (user choice); default: last protocol layer open.
  let toggled = $state<Record<string, boolean>>({});

  function keyOf(path: string, n: Node) {
    const name = n.field || n.label.split(":")[0].replace(/\d+/g, "#");
    return `${path}/${name}`;
  }

  function isOpen(key: string, depth: number, idx: number) {
    if (key in toggled) return toggled[key];
    return depth === 0 && idx === nodes.length - 1;
  }

  function toggle(key: string, depth: number, idx: number) {
    toggled[key] = !isOpen(key, depth, idx);
  }

  /** Expand every ancestor of `target` (used when a hex byte is clicked). */
  export function reveal(target: Node) {
    const walk = (list: Node[], path: string, depth: number): boolean => {
      for (const n of list) {
        const k = keyOf(path, n);
        if (n === target) return true;
        if (n.children && walk(n.children, k, depth + 1)) {
          toggled[k] = true;
          return true;
        }
      }
      return false;
    };
    walk(nodes, "", 0);
  }

  export function expandAll(open: boolean) {
    const next: Record<string, boolean> = {};
    const walk = (list: Node[], path: string) => {
      for (const n of list) {
        const k = keyOf(path, n);
        if (n.children?.length) {
          next[k] = open;
          walk(n.children, k);
        }
      }
    };
    walk(nodes, "");
    toggled = next;
  }
</script>

{#snippet branch(list: Node[], path: string, depth: number)}
  {#each list as n, i (i)}
    {@const k = keyOf(path, n)}
    {@const kids = !!n.children?.length}
    {@const open = kids && isOpen(k, depth, i)}
    <div class="item" style:padding-left="{depth * 16 + 4}px">
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="line"
        class:sel={selected === n}
        class:top={depth === 0}
        class:l1={n.level === 1}
        class:l2={n.level === 2}
        class:l3={n.level === 3}
        role="treeitem"
        aria-selected={selected === n}
        aria-expanded={kids ? open : undefined}
        tabindex="-1"
        onclick={() => onselect(n)}
        ondblclick={() => kids && toggle(k, depth, i)}
      >
        {#if kids}
          <button class="tw" onclick={(e) => { e.stopPropagation(); toggle(k, depth, i); }} aria-label={open ? "Collapse" : "Expand"}>{open ? "▾" : "▸"}</button>
        {:else}
          <span class="tw"></span>
        {/if}
        {#if n.level >= 2}<span class="badge" aria-hidden="true">!</span>{/if}
        <span class="lbl">{n.label}</span>
        {#if selected === n && n.filter}
          <span class="acts">
            <button title="Show only packets with this value: {n.filter}" onclick={(e) => { e.stopPropagation(); onfilter(n.filter!, "apply"); }}>Filter</button>
            <button title="Add to the current filter with &&" onclick={(e) => { e.stopPropagation(); onfilter(n.filter!, "and"); }}>+ and</button>
            <button title="Exclude packets with this value" onclick={(e) => { e.stopPropagation(); onfilter(n.filter!, "not"); }}>Exclude</button>
          </span>
        {/if}
      </div>
    </div>
    {#if open}{@render branch(n.children!, k, depth + 1)}{/if}
  {/each}
{/snippet}

<div class="tree mono" role="tree">
  {@render branch(nodes, "", 0)}
</div>

<style>
  .tree { font-size: 12px; padding: 4px 0; }
  .line { display: flex; align-items: center; gap: 4px; min-height: 21px; padding-right: 8px; border-radius: 5px; cursor: default; white-space: nowrap; }
  .line:hover { background: var(--surface-2); }
  .line.top .lbl { font-weight: 600; }
  .line.sel { background: color-mix(in srgb, var(--accent) 22%, transparent); }
  .line.l1 .lbl { color: var(--series-1); }
  .line.l2 .lbl { color: var(--warning); }
  .line.l3 .lbl { color: var(--critical); }
  .tw { all: unset; width: 14px; text-align: center; color: var(--text-muted); cursor: pointer; flex: none; }
  .lbl { overflow: hidden; text-overflow: ellipsis; }
  .badge { flex: none; width: 14px; height: 14px; border-radius: 50%; background: var(--warning); color: #000; font-size: 10px; font-weight: 700; display: inline-grid; place-items: center; }
  .l3 .badge { background: var(--critical); color: #fff; }
  .acts { display: inline-flex; gap: 4px; margin-left: 8px; font-family: var(--font-sans); }
  .acts button { font-size: 11px; padding: 1px 7px; }
</style>
