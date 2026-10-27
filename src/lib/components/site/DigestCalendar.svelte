<script lang="ts">
  // 日报日历：一列一周、一行一天（周一到周日），最近 26 周。
  // 颜色深浅＝那天 AI 做了多少事（上线、出计划、待你确认）；红点＝那天有回滚或失败。
  // 有日报的日子能点开，带细边框；正在看的那天描深边。
  import type { DayCount } from '$lib/types';

  let { digests, daily, selected, onpick }: { digests: string[]; daily: DayCount[]; selected: string | null; onpick: (name: string) => void } = $props();

  const WEEKS = 26;
  const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const byDate = $derived(new Map(daily.map((d) => [d.date, d])));
  const hasReport = $derived(new Set(digests));
  const work = (d?: DayCount) => (d ? d.shipped + d.planned + d.proposed : 0);
  const top = $derived(Math.max(1, ...daily.map((d) => work(d))));
  const level = (n: number) => (n <= 0 ? 0 : n / top <= 0.25 ? 1 : n / top <= 0.5 ? 2 : n / top <= 0.75 ? 3 : 4);

  type Cell = { date: string; future: boolean; month: number; day: number };
  const weeks = $derived.by(() => {
    const today = new Date(); today.setHours(0, 0, 0, 0);
    const start = new Date(today);
    start.setDate(today.getDate() - ((today.getDay() + 6) % 7) - (WEEKS - 1) * 7);
    const out: Cell[][] = [];
    for (let w = 0; w < WEEKS; w++) {
      const col: Cell[] = [];
      for (let i = 0; i < 7; i++) {
        const d = new Date(start); d.setDate(start.getDate() + w * 7 + i);
        col.push({ date: iso(d), future: d > today, month: d.getMonth() + 1, day: d.getDate() });
      }
      out.push(col);
    }
    return out;
  });
  // 月份标在这个月第一天所在的那一列（第一列总是标上）
  const monthOf = (col: Cell[], w: number) => { const first = col.find((c) => c.day === 1); return first ? `${first.month}月` : w === 0 ? `${col[0].month}月` : ''; };

  function tip(c: Cell) {
    const d = byDate.get(c.date);
    const parts = d ? [d.shipped && `上线 ${d.shipped}`, d.planned && `出计划 ${d.planned}`, d.proposed && `待你确认 ${d.proposed}`, d.broken && `回滚或失败 ${d.broken}`].filter(Boolean) : [];
    return `${c.month}月${c.day}日 · ${parts.length ? parts.join(' · ') : '没有动静'}\n${hasReport.has(c.date) ? '点开看日报' : '没有日报（那天客户端可能没开）'}`;
  }
</script>

<div class="cal" style:--weeks={WEEKS}>
  <div class="months" aria-hidden="true">{#each weeks as col, w (w)}<span>{monthOf(col, w)}</span>{/each}</div>
  <div class="body">
    <div class="wd" aria-hidden="true"><span>一</span><span></span><span>三</span><span></span><span>五</span><span></span><span></span></div>
    <div class="grid" role="grid" aria-label="日报日历">
      {#each weeks as col, w (w)}
        <div class="col" role="row">
          {#each col as c (c.date)}
            {#if c.future}<span class="cell none" role="gridcell"></span>
            {:else}
              {@const d = byDate.get(c.date)}
              {@const rep = hasReport.has(c.date)}
              <button type="button" role="gridcell" class="cell lv{level(work(d))}" class:rep class:on={selected === c.date} disabled={!rep} onclick={() => onpick(c.date)} data-tip={tip(c)} aria-label={tip(c)}>{#if d?.broken}<i class="bad"></i>{/if}</button>
            {/if}
          {/each}
        </div>
      {/each}
    </div>
  </div>
  <div class="legend" aria-hidden="true"><span class="cell rep" style="display:inline-block"></span>有日报<span class="gap"></span>少{#each [0, 1, 2, 3, 4] as l (l)}<span class="cell lv{l}"></span>{/each}多<span class="gap"></span><i class="bad static"></i>有回滚或失败</div>
</div>

<style>
  .cal { --c: 12px; --g: 3px; display: flex; flex-direction: column; gap: 4px; margin: 2px 0 6px; overflow-x: auto; }
  .months { display: grid; grid-template-columns: repeat(var(--weeks), var(--c)); gap: var(--g); margin-left: calc(14px + var(--g) * 2); font-size: 10px; color: var(--faint); line-height: 1; height: 11px; }
  .months span { white-space: nowrap; overflow: visible; }
  .body { display: flex; gap: calc(var(--g) * 2); }
  .wd { display: grid; grid-template-rows: repeat(7, var(--c)); gap: var(--g); width: 14px; font-size: 9.5px; color: var(--faint); line-height: var(--c); }
  .grid { display: flex; gap: var(--g); }
  .col { display: grid; grid-template-rows: repeat(7, var(--c)); gap: var(--g); }
  .cell { position: relative; width: var(--c); height: var(--c); padding: 0; border: 0; border-radius: 3px; background: #f0efeb; cursor: default; }
  .cell.none { background: none; }
  .cell.lv1 { background: color-mix(in srgb, var(--ok) 28%, #fff); }
  .cell.lv2 { background: color-mix(in srgb, var(--ok) 50%, #fff); }
  .cell.lv3 { background: color-mix(in srgb, var(--ok) 75%, #fff); }
  .cell.lv4 { background: var(--ok); }
  .cell.rep { box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--text) 22%, transparent); cursor: pointer; }
  button.cell.rep:hover { box-shadow: inset 0 0 0 1.5px color-mix(in srgb, var(--text) 55%, transparent); }
  .cell.on, .cell.on:hover { box-shadow: 0 0 0 1.5px var(--text); }
  .cell:disabled { cursor: default; }
  .bad { position: absolute; top: 1px; right: 1px; width: 4px; height: 4px; border-radius: 50%; background: var(--err); }
  .bad.static { position: static; display: inline-block; margin-right: 1px; }
  .legend { display: flex; align-items: center; justify-content: flex-end; gap: 4px; font-size: 10.5px; color: var(--faint); margin-top: 2px; }
  .legend .cell { width: 10px; height: 10px; border-radius: 2.5px; }
  .legend .gap { width: 10px; }
</style>
