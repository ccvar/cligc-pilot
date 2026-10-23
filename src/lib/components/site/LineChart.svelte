<script lang="ts">
  // 极简趋势图：每条线按自己的最大值归一（不同量级的指标共用一张图），悬停显示当天数值。
  type Series = { label: string; color: string; values: number[]; fmt?: (v: number) => string };
  let { labels, series, height = 150 }: { labels: string[]; series: Series[]; height?: number } = $props();
  const W = 600;
  const H = $derived(height);
  let hover = $state(-1);
  let box: SVGSVGElement | undefined = $state();
  const n = $derived(labels.length);
  const x = (i: number) => (n <= 1 ? W / 2 : (i / (n - 1)) * W);
  function path(vals: number[]) {
    const max = Math.max(1, ...vals);
    return vals.map((v, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)},${(H - 6 - (v / max) * (H - 16)).toFixed(1)}`).join('');
  }
  function onMove(e: MouseEvent) {
    if (!box || n === 0) return;
    const r = box.getBoundingClientRect();
    hover = Math.max(0, Math.min(n - 1, Math.round(((e.clientX - r.left) / r.width) * (n - 1))));
  }
  const fmt = (s: Series, v: number) => (s.fmt ? s.fmt(v) : Math.round(v).toLocaleString());
</script>

<div class="lc">
  <div class="legend">
    {#each series as s (s.label)}<span><i style="background:{s.color}"></i>{s.label}{#if hover >= 0} <b>{fmt(s, s.values[hover] ?? 0)}</b>{/if}</span>{/each}
    {#if hover >= 0}<span class="when">{labels[hover]}</span>{/if}
  </div>
  {#if n}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <svg bind:this={box} viewBox="0 0 {W} {H}" preserveAspectRatio="none" style="height:{H}px" onmousemove={onMove} onmouseleave={() => (hover = -1)}>
      <line x1="0" x2={W} y1={H - 6} y2={H - 6} class="axis" />
      {#each series as s (s.label)}<path d={path(s.values)} stroke={s.color} fill="none" stroke-width="1.6" vector-effect="non-scaling-stroke" />{/each}
      {#if hover >= 0}<line x1={x(hover)} x2={x(hover)} y1="0" y2={H} class="cursor" />{/if}
    </svg>
    <div class="ticks"><span>{labels[0]}</span><span>{labels[n - 1]}</span></div>
  {:else}
    <p class="empty">还没有数据</p>
  {/if}
</div>

<style>
  .lc { display: flex; flex-direction: column; gap: 6px; }
  .legend { display: flex; flex-wrap: wrap; gap: 4px 14px; font-size: 11.5px; color: var(--dim); min-height: 18px; }
  .legend i { display: inline-block; width: 10px; height: 3px; border-radius: 2px; margin-right: 5px; vertical-align: middle; }
  .legend b { color: var(--text); font-weight: 600; }
  .legend .when { margin-left: auto; color: var(--faint); }
  svg { width: 100%; display: block; cursor: crosshair; }
  .axis { stroke: var(--border); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .cursor { stroke: var(--faint); stroke-width: 1; stroke-dasharray: 3 3; vector-effect: non-scaling-stroke; }
  .ticks { display: flex; justify-content: space-between; font-size: 10.5px; color: var(--faint); }
  .empty { color: var(--faint); font-size: 12px; margin: 20px 0; text-align: center; }
</style>
