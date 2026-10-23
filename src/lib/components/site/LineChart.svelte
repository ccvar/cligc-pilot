<script lang="ts">
  // 极简趋势图：每条线按自己的最大值归一（不同量级的指标共用一张图），悬停显示当天数值。
  // 第一条线是主指标：加一层很淡的面积；其余只画线。
  type Series = { label: string; color: string; values: number[]; fmt?: (v: number) => string };
  let { labels, series, height = 150 }: { labels: string[]; series: Series[]; height?: number } = $props();
  const W = 600;
  const H = $derived(height);
  const TOP = 8;
  const BOT = 6;
  let hover = $state(-1);
  let box: SVGSVGElement | undefined = $state();
  const uid = `lc${Math.random().toString(36).slice(2, 8)}`;
  const n = $derived(labels.length);
  const x = (i: number) => (n <= 1 ? W / 2 : (i / (n - 1)) * W);
  const y = (v: number, max: number) => H - BOT - (v / max) * (H - BOT - TOP);
  const maxOf = (vals: number[]) => Math.max(1, ...vals);
  function line(vals: number[]) {
    const max = maxOf(vals);
    return vals.map((v, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)},${y(v, max).toFixed(1)}`).join('');
  }
  const area = (vals: number[]) => (vals.length ? `${line(vals)}L${x(vals.length - 1).toFixed(1)},${H - BOT}L0,${H - BOT}Z` : '');
  function onMove(e: MouseEvent) {
    if (!box || n === 0) return;
    const r = box.getBoundingClientRect();
    hover = Math.max(0, Math.min(n - 1, Math.round(((e.clientX - r.left) / r.width) * (n - 1))));
  }
  const fmt = (s: Series, v: number) => (s.fmt ? s.fmt(v) : Math.round(v).toLocaleString('en-US'));
  const md = (k: string) => { const m = /^\d{4}-(\d{2})-(\d{2})$/.exec(k); return m ? `${+m[1]}月${+m[2]}日` : k; };
  const ticks = $derived(n > 2 ? [0, Math.floor((n - 1) / 2), n - 1] : n ? [0, n - 1] : []);
</script>

<div class="lc">
  <div class="legend">
    {#each series as s (s.label)}<span><i style="background:{s.color}"></i>{s.label}{#if hover >= 0} <b>{fmt(s, s.values[hover] ?? 0)}</b>{/if}</span>{/each}
    {#if hover >= 0}<span class="when">{md(labels[hover])}</span>{/if}
  </div>
  {#if n}
    <div class="plot">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <svg bind:this={box} viewBox="0 0 {W} {H}" preserveAspectRatio="none" style="height:{H}px" onmousemove={onMove} onmouseleave={() => (hover = -1)}>
        <defs>
          <linearGradient id={uid} x1="0" x2="0" y1="0" y2="1">
            <stop offset="0" stop-color={series[0]?.color} stop-opacity=".14" />
            <stop offset="1" stop-color={series[0]?.color} stop-opacity="0" />
          </linearGradient>
        </defs>
        {#each [0.25, 0.5, 0.75] as g (g)}<line x1="0" x2={W} y1={TOP + (H - BOT - TOP) * g} y2={TOP + (H - BOT - TOP) * g} class="grid" />{/each}
        <line x1="0" x2={W} y1={H - BOT} y2={H - BOT} class="axis" />
        {#if series[0]}<path d={area(series[0].values)} fill="url(#{uid})" />{/if}
        {#each series as s, si (s.label)}<path d={line(s.values)} stroke={s.color} fill="none" stroke-width={si === 0 ? 1.8 : 1.4} stroke-opacity={si === 0 ? 1 : 0.75} stroke-linejoin="round" vector-effect="non-scaling-stroke" />{/each}
        {#if hover >= 0}<line x1={x(hover)} x2={x(hover)} y1={TOP - 4} y2={H - BOT} class="cursor" />{/if}
      </svg>
      {#if hover >= 0}
        {#each series as s (s.label)}
          <span class="pt" style="left:{(x(hover) / W) * 100}%;top:{y(s.values[hover] ?? 0, maxOf(s.values))}px;border-color:{s.color}"></span>
        {/each}
      {/if}
    </div>
    <div class="ticks">{#each ticks as t (t)}<span>{md(labels[t])}</span>{/each}</div>
  {:else}
    <p class="empty">还没有数据</p>
  {/if}
</div>

<style>
  .lc { display: flex; flex-direction: column; gap: 8px; }
  .legend { display: flex; flex-wrap: wrap; gap: 4px 16px; font-size: 11.5px; color: var(--dim); min-height: 18px; font-variant-numeric: tabular-nums; }
  .legend i { display: inline-block; width: 10px; height: 3px; border-radius: 2px; margin-right: 6px; vertical-align: middle; }
  .legend b { color: var(--text); font-weight: 600; margin-left: 2px; }
  .legend .when { margin-left: auto; color: var(--dim); }
  .plot { position: relative; }
  svg { width: 100%; display: block; cursor: crosshair; overflow: visible; }
  .grid { stroke: var(--border); stroke-width: 1; stroke-dasharray: 2 4; vector-effect: non-scaling-stroke; opacity: .8; }
  .axis { stroke: var(--border2); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .cursor { stroke: var(--faint); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .pt { position: absolute; width: 8px; height: 8px; margin: -4px 0 0 -4px; border-radius: 50%; background: #fff; border: 2px solid; box-sizing: border-box; pointer-events: none; }
  .ticks { display: flex; justify-content: space-between; font-size: 10.5px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .empty { color: var(--faint); font-size: 12px; margin: 20px 0; text-align: center; }
</style>
