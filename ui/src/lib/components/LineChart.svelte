<!--
  SysPulse - Multipurpose System Utility
  File   : ui/src/lib/components/LineChart.svelte
  Author : Praneeth
  License: MIT
  Purpose: Live time-series chart (uPlot) with gradient area fill, crosshair
           tooltip and a legend for multi-series charts. Re-themes itself
           when the app theme changes.
-->
<script lang="ts">
  import uPlot from "uplot";
  import { onDestroy, onMount } from "svelte";

  export interface Series {
    label: string;
    /** CSS custom property name, e.g. "--series-1". */
    color: string;
    values: (number | null)[];
  }

  let {
    time,
    series,
    height = 160,
    yMax = null,
    fill = true,
    format = (v: number) => v.toFixed(0),
    showLegend = true,
    elapsed = false,
  }: {
    time: number[];
    series: Series[];
    height?: number;
    yMax?: number | null;
    fill?: boolean;
    format?: (v: number) => string;
    showLegend?: boolean;
    /** X values are seconds since start instead of Unix time. */
    elapsed?: boolean;
  } = $props();

  let el: HTMLDivElement = $state()!;
  let plot: uPlot | null = null;
  let tip = $state<{ x: number; y: number; time: number; rows: { label: string; color: string; value: string }[] } | null>(null);
  let ro: ResizeObserver | null = null;
  let mo: MutationObserver | null = null;

  const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888";

  function hexA(hex: string, a: number) {
    const h = hex.replace("#", "");
    if (h.length !== 6) return hex;
    const n = parseInt(h, 16);
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
  }

  function build() {
    plot?.destroy();
    const grid = css("--grid");
    const muted = css("--text-muted");
    const surface = css("--surface-1");
    const opts: uPlot.Options = {
      width: Math.max(100, el.clientWidth),
      height,
      padding: [8, 4, 0, 0],
      legend: { show: false },
      cursor: {
        y: false,
        points: { size: 8, width: 2, stroke: () => surface },
        drag: { x: false, y: false },
      },
      scales: {
        x: { time: !elapsed },
        y: yMax != null ? { range: [0, yMax] } : { range: (_u, _min, max) => [0, max > 0 ? max * 1.15 : 1] },
      },
      axes: [
        { stroke: muted, grid: { show: false }, ticks: { show: false }, size: 24, font: "11px sans-serif", values: (_u, v) => v.map(() => "") },
        {
          stroke: muted,
          grid: { stroke: grid, width: 1 },
          ticks: { show: false },
          // Fit the widest tick label so values like "10.7 MiB/s" are not clipped.
          size: (_u, values) => (values?.length ? Math.ceil(Math.max(...values.map((v) => String(v).length)) * 6.4) + 16 : 40),
          font: "11px sans-serif",
          values: (_u, v) => v.map((x) => format(x)),
          splits: yMax != null ? () => [0, yMax / 2, yMax] : undefined,
        },
      ],
      series: [
        {},
        ...series.map((s) => {
          const c = css(s.color);
          return {
            label: s.label,
            stroke: c,
            width: 2,
            points: { show: false },
            fill: fill
              ? (u: uPlot) => {
                  const g = u.ctx.createLinearGradient(0, u.bbox.top, 0, u.bbox.top + u.bbox.height);
                  g.addColorStop(0, hexA(c, series.length > 1 ? 0.18 : 0.32));
                  g.addColorStop(1, hexA(c, 0));
                  return g;
                }
              : undefined,
          } as uPlot.Series;
        }),
      ],
      hooks: {
        setCursor: [
          (u) => {
            const i = u.cursor.idx;
            if (i == null || u.cursor.left == null || u.cursor.left < 0) {
              tip = null;
              return;
            }
            tip = {
              x: u.cursor.left + u.over.offsetLeft,
              y: u.cursor.top ?? 0,
              time: u.data[0][i] as number,
              rows: series.map((s, k) => {
                const v = u.data[k + 1][i];
                return { label: s.label, color: s.color, value: v == null ? "—" : format(v as number) };
              }),
            };
          },
        ],
      },
    };
    plot = new uPlot(opts, data(), el);
  }

  function data(): uPlot.AlignedData {
    return [time, ...series.map((s) => s.values)] as uPlot.AlignedData;
  }

  onMount(() => {
    build();
    ro = new ResizeObserver(() => plot?.setSize({ width: Math.max(100, el.clientWidth), height }));
    ro.observe(el);
    mo = new MutationObserver(() => build());
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  });

  onDestroy(() => {
    ro?.disconnect();
    mo?.disconnect();
    plot?.destroy();
  });

  // Push new data on every telemetry tick; rebuild if the series set changes.
  let seriesKey = $derived(series.map((s) => s.label + s.color).join("|"));
  let builtKey = "";
  $effect(() => {
    const d = data();
    const key = seriesKey;
    if (!plot) return;
    if (key !== builtKey) {
      builtKey = key;
      if (plot.series.length !== series.length + 1) {
        build();
        return;
      }
    }
    plot.setData(d);
  });
</script>

{#if showLegend && series.length > 1}
  <div class="legend">
    {#each series as s (s.label)}
      {@const last = s.values[s.values.length - 1]}
      <span class="item">
        <span class="sw" style:background="var({s.color})"></span>
        <span class="secondary">{s.label}</span>
        <span class="num">{last == null ? "—" : format(last)}</span>
      </span>
    {/each}
  </div>
{/if}
<div class="wrap" style:height="{height}px">
  <div class="plot" bind:this={el} onmouseleave={() => (tip = null)} role="img" aria-label={series.map((s) => s.label).join(", ") + " chart"}></div>
  {#if tip}
    <div class="tip" style:left="{tip.x}px" class:flip={tip.x > (el?.clientWidth ?? 0) * 0.6}>
      <div class="t muted">{elapsed ? `${tip.time.toFixed(1)} s` : new Date(tip.time * 1000).toLocaleTimeString()}</div>
      {#each tip.rows as r (r.label)}
        <div class="r"><span class="sw" style:background="var({r.color})"></span><span class="secondary">{r.label}</span><b class="num">{r.value}</b></div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .wrap { position: relative; width: 100%; }
  .plot { width: 100%; }
  .legend { display: flex; flex-wrap: wrap; gap: 14px; font-size: 12px; margin-bottom: 6px; }
  .item { display: inline-flex; align-items: center; gap: 6px; }
  .sw { width: 10px; height: 3px; border-radius: 2px; display: inline-block; }
  .tip {
    position: absolute; top: 4px; transform: translateX(10px);
    background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: 8px;
    padding: 6px 9px; font-size: 12px; pointer-events: none; box-shadow: var(--shadow); z-index: 5;
    display: flex; flex-direction: column; gap: 3px; min-width: 130px;
  }
  .tip.flip { transform: translateX(calc(-100% - 10px)); }
  .t { font-size: 11px; }
  .r { display: flex; align-items: center; gap: 6px; }
  .r b { margin-left: auto; font-weight: 600; }
</style>
