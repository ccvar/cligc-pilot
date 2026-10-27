<script lang="ts">
  // 托管方案页顶上的执行记录：最近 30 天，每天一根柱子（上线 / 出计划 / 待你确认 / 回滚或失败叠起来）。
  // 跑了作业但没产出的日子画一截浅灰，完全没跑的日子留空——一眼看出客户端哪几天没开、哪几天停了。
  import type { DayCount } from '$lib/types';

  let { daily, digests, onpick }: { daily: DayCount[]; digests: string[]; onpick: (date: string, hasReport: boolean) => void } = $props();

  const DAYS = 30;
  const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const byDate = $derived(new Map(daily.map((d) => [d.date, d])));
  const hasReport = $derived(new Set(digests));
  const days = $derived.by(() => {
    const out: { date: string; label: string; d?: DayCount }[] = [];
    const t = new Date(); t.setHours(0, 0, 0, 0);
    for (let i = DAYS - 1; i >= 0; i--) {
      const x = new Date(t); x.setDate(t.getDate() - i);
      const date = iso(x);
      out.push({ date, label: `${x.getMonth() + 1}月${x.getDate()}日`, d: byDate.get(date) });
    }
    return out;
  });
  const total = (d?: DayCount) => (d ? d.shipped + d.planned + d.proposed + d.broken : 0);
  const top = $derived(Math.max(4, ...days.map((x) => total(x.d))));
  const sumRange = (from: number, to: number, k: 'shipped' | 'broken') => days.slice(from, to).reduce((n, x) => n + (x.d?.[k] ?? 0), 0);
  const week = $derived(sumRange(DAYS - 7, DAYS, 'shipped'));
  const lastWeek = $derived(sumRange(DAYS - 14, DAYS - 7, 'shipped'));
  const broken = $derived(sumRange(DAYS - 7, DAYS, 'broken'));
  const idle = $derived(days.slice(DAYS - 7).filter((x) => !x.d?.ran && !total(x.d)).length);

  const H = 44;
  const seg = (n: number) => (n / top) * H;
  function tip(x: (typeof days)[number]) {
    const d = x.d;
    const parts = d ? [d.shipped && `上线 ${d.shipped}`, d.planned && `出计划 ${d.planned}`, d.proposed && `待你确认 ${d.proposed}`, d.broken && `回滚或失败 ${d.broken}`].filter(Boolean) : [];
    const ran = d?.ran ? `跑了 ${d.ran} 次作业` : '没有跑（客户端可能没开）';
    return `${x.label}\n${parts.length ? parts.join(' · ') : '没有改动'} · ${ran}\n${hasReport.has(x.date) ? '点一下看这天的日报' : '点一下看这天的操作日志'}`;
  }
</script>

<div class="card rc">
  <div class="stats">
    <div class="st"><b>{week}</b><span>近 7 天上线</span></div>
    <div class="st"><b class:up={week > lastWeek} class:down={week < lastWeek}>{week === lastWeek ? '持平' : `${week > lastWeek ? '+' : ''}${week - lastWeek}`}</b><span>比前 7 天</span></div>
    <div class="st"><b class:err={broken > 0}>{broken}</b><span>回滚或失败</span></div>
    {#if idle}<div class="st"><b class="warn">{idle} 天</b><span>没有跑</span></div>{/if}
  </div>
  <div class="chart">
    <div class="bars" style:height="{H}px">
      {#each days as x (x.date)}
        {@const d = x.d}
        <button class="day" onclick={() => onpick(x.date, hasReport.has(x.date))} data-tip={tip(x)} aria-label={tip(x)}>
          {#if total(d)}
            <span class="stack">
              {#if d?.broken}<i class="b err" style:height="{seg(d.broken)}px"></i>{/if}
              {#if d?.proposed}<i class="b warn" style:height="{seg(d.proposed)}px"></i>{/if}
              {#if d?.planned}<i class="b plan" style:height="{seg(d.planned)}px"></i>{/if}
              {#if d?.shipped}<i class="b ok" style:height="{seg(d.shipped)}px"></i>{/if}
            </span>
          {:else if d?.ran}<span class="stack"><i class="b idle"></i></span>
          {:else}<span class="stack"><i class="b none"></i></span>{/if}
        </button>
      {/each}
    </div>
    <div class="axis"><span>{days[0].label}</span><span>今天</span></div>
  </div>
  <div class="legend" aria-hidden="true">
    <span><i class="ok"></i>上线</span><span><i class="plan"></i>出计划</span><span><i class="warn"></i>待你确认</span><span><i class="err"></i>回滚或失败</span><span><i class="idle"></i>跑了没改动</span>
  </div>
</div>

<style>
  .rc { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 22px; padding: 12px 18px; margin-bottom: 14px; }
  .stats { display: flex; gap: 18px; }
  .st { display: flex; flex-direction: column; gap: 1px; white-space: nowrap; }
  .st b { font-size: 16px; font-weight: 650; line-height: 1.25; color: var(--text); font-variant-numeric: tabular-nums; }
  .st b.up { color: var(--ok); }
  .st b.down, .st b.err { color: var(--err); }
  .st b.warn { color: var(--warn); }
  .st span { font-size: 11px; color: var(--faint); }
  .chart { min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .bars { display: flex; align-items: flex-end; gap: 3px; }
  .day { flex: 1; min-width: 0; height: 100%; display: flex; align-items: flex-end; padding: 0; border: 0; border-radius: 3px; background: none; cursor: pointer; }
  .day:hover { background: var(--su-hover); }
  .stack { width: 100%; display: flex; flex-direction: column; border-radius: 2px; overflow: hidden; }
  .b { display: block; width: 100%; min-height: 2px; }
  .b.ok { background: var(--ok); }
  .b.plan { background: #cfccc4; }
  .b.warn { background: var(--warn); }
  .b.err { background: var(--err); }
  .b.idle { height: 3px; background: #e4e2dc; }
  .b.none { height: 1px; background: transparent; border-bottom: 1px dashed #e4e2dc; }
  .axis { display: flex; justify-content: space-between; font-size: 10px; color: var(--faint); }
  .legend { display: grid; grid-template-columns: auto auto; gap: 3px 12px; font-size: 10.5px; color: var(--faint); white-space: nowrap; }
  .legend span { display: inline-flex; align-items: center; gap: 5px; }
  .legend i { width: 8px; height: 8px; border-radius: 2px; }
  .legend i.ok { background: var(--ok); }
  .legend i.plan { background: #cfccc4; }
  .legend i.warn { background: var(--warn); }
  .legend i.err { background: var(--err); }
  .legend i.idle { background: #e4e2dc; }
  @media (max-width: 900px) { .rc { grid-template-columns: minmax(0, 1fr); gap: 10px; } .legend { display: flex; flex-wrap: wrap; gap: 10px; } }
</style>
