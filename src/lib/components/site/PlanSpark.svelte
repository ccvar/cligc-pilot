<script lang="ts">
  // 方案卡片标题上的小柱子：这个方案的站点最近 14 天每天做了多少（上线绿、其余灰、有失败顶上一截红），
  // 跑了没改动的一个小点，没跑的留空。悬停看合计。
  import type { DayCount } from '$lib/types';

  let { days }: { days: DayCount[] } = $props();
  const N = 14;
  const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const by = $derived(new Map(days.map((d) => [d.date, d])));
  const cols = $derived.by(() => {
    const t = new Date(); t.setHours(0, 0, 0, 0);
    return Array.from({ length: N }, (_, i) => { const x = new Date(t); x.setDate(t.getDate() - (N - 1 - i)); return by.get(iso(x)); });
  });
  const work = (d?: DayCount) => (d ? d.shipped + d.planned + d.proposed + d.broken : 0);
  const top = $derived(Math.max(3, ...cols.map(work)));
  const H = 14;
  const sum = $derived(cols.reduce((a, d) => ({ s: a.s + (d?.shipped ?? 0), p: a.p + (d?.planned ?? 0), b: a.b + (d?.broken ?? 0), idle: a.idle + (!d?.ran && !work(d) ? 1 : 0) }), { s: 0, p: 0, b: 0, idle: 0 }));
  const tip = $derived(`近 14 天：上线 ${sum.s} · 出计划 ${sum.p}${sum.b ? ` · 回滚或失败 ${sum.b}` : ''}${sum.idle ? ` · ${sum.idle} 天没跑` : ''}`);
</script>

<span class="spark" data-tip={tip} aria-label={tip} style:height="{H}px">
  {#each cols as d, i (i)}
    {@const w = work(d)}
    <span class="col">
      {#if w}
        {#if d?.broken}<i class="err" style:height="{Math.max(2, (d.broken / top) * H)}px"></i>{/if}
        {#if w - (d?.shipped ?? 0) - (d?.broken ?? 0) > 0}<i class="gray" style:height="{Math.max(2, ((w - (d?.shipped ?? 0) - (d?.broken ?? 0)) / top) * H)}px"></i>{/if}
        {#if d?.shipped}<i class="ok" style:height="{Math.max(2, (d.shipped / top) * H)}px"></i>{/if}
      {:else if d?.ran}<i class="idle"></i>{/if}
    </span>
  {/each}
</span>

<style>
  .spark { display: inline-flex; align-items: flex-end; gap: 2px; flex: none; margin-left: 4px; cursor: default; }
  .col { width: 4px; height: 100%; display: flex; flex-direction: column; justify-content: flex-end; border-radius: 1px; overflow: hidden; background: #f3f2ee; }
  i { display: block; width: 100%; }
  .ok { background: var(--ok); }
  .gray { background: #cfccc4; }
  .err { background: var(--err); }
  .idle { height: 2px; background: #dcd9d2; }
</style>
