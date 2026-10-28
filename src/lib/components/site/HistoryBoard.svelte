<script lang="ts">
  // 全部站点 →「处理记录」：所有站点交给 AI 处理过、已经有结果的条目（已上线、已处理、已回滚、没成功、你驳回了），按天分组。
  // 能按时间（7 / 30 天）和结果筛；已上线的带上线前后这个页面的点击对比，满 4 周后是复盘结论。每条的列表沿用托管页「最近上线」的样子。
  import RecentShips from './RecentShips.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { say } from '$lib/state/app.svelte.js';
  import type { ShipItem } from '$lib/types';

  let days = $state(7);
  let kind = $state<'all' | 'live' | 'bad' | 'dismissed'>('all');
  let items = $state<ShipItem[] | null>(null);
  $effect(() => {
    const d = days;
    invoke<ShipItem[]>('history_across_sites', { days: d }).then((v) => (items = v)).catch((e) => { items = []; say(String(e), 'err'); });
  });
  const KINDS = [
    { k: 'all', l: '全部' },
    { k: 'live', l: '已上线' },
    { k: 'bad', l: '没成功 / 回滚' },
    { k: 'dismissed', l: '你驳回的' },
  ] as const;
  const match = (x: ShipItem) => {
    const s = x.item.status;
    return kind === 'all' || (kind === 'live' ? s === 'deployed' || s === 'done' : kind === 'bad' ? s === 'failed' || s === 'rolled_back' : s === 'dismissed');
  };
  const at = (x: ShipItem) => x.item.deployed_at || x.item.updated_at;
  const dayKey = (t: number) => { const d = new Date(t * 1000); return `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`; };
  function dayLabel(t: number) {
    const d = new Date(t * 1000), today = new Date();
    const y = new Date(); y.setDate(today.getDate() - 1);
    if (dayKey(t) === dayKey(today.getTime() / 1000)) return '今天';
    if (dayKey(t) === dayKey(y.getTime() / 1000)) return '昨天';
    return `${d.getMonth() + 1}月${d.getDate()}日 · 周${'日一二三四五六'[d.getDay()]}`;
  }
  const shown = $derived((items ?? []).filter(match));
  const groups = $derived.by(() => {
    const out: { key: string; label: string; items: ShipItem[] }[] = [];
    for (const x of shown) {
      const k = dayKey(at(x));
      const g = out.find((o) => o.key === k);
      if (g) g.items.push(x); else out.push({ key: k, label: dayLabel(at(x)), items: [x] });
    }
    return out;
  });
  // 上面的小方块图：选的时间范围里每天一列，按结果叠起来（上线 / 已处理 / 没成功或回滚；你驳回的不画），点一列跳到那天
  const bars = $derived.by(() => {
    const out: { key: string; label: string; live: number; done: number; bad: number }[] = [];
    const today = new Date(); today.setHours(12, 0, 0, 0);
    for (let n = days - 1; n >= 0; n--) {
      const d = new Date(today); d.setDate(today.getDate() - n);
      const t = d.getTime() / 1000;
      out.push({ key: dayKey(t), label: `${d.getMonth() + 1}月${d.getDate()}日 周${'日一二三四五六'[d.getDay()]}`, live: 0, done: 0, bad: 0 });
    }
    for (const x of items ?? []) {
      const b = out.find((o) => o.key === dayKey(at(x)));
      if (!b) continue;
      const s = x.item.status;
      if (s === 'deployed') b.live++; else if (s === 'done') b.done++; else if (s === 'failed' || s === 'rolled_back') b.bad++;
    }
    return out;
  });
  // 一个方块＝一条，每天一列从下往上叠：上线、已处理、没成功 / 回滚；一天最多画 8 格，多的在顶上写「+N」
  const CAP = 8;
  // 超过 8 条时先保证没成功 / 回滚的露出来（出问题的不能被「+N」藏住），再补已处理、上线；精确数字看悬停
  function cells(b: { live: number; done: number; bad: number }): string[] {
    const bad = Math.min(b.bad, CAP), done = Math.min(b.done, CAP - bad), live = Math.min(b.live, CAP - bad - done);
    return [...Array(live).fill('live'), ...Array(done).fill('done'), ...Array(bad).fill('bad')];
  }
  const rows = $derived(Math.min(CAP, Math.max(1, ...bars.map((b) => b.live + b.done + b.bad))));
  function jump(key: string) {
    document.getElementById(`hd-${key}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
  const count = (k: (typeof KINDS)[number]['k']) => (items ?? []).filter((x) => { const s = x.item.status; return k === 'all' || (k === 'live' ? s === 'deployed' || s === 'done' : k === 'bad' ? s === 'failed' || s === 'rolled_back' : s === 'dismissed'); }).length;
</script>

<div class="hb">
  <div class="bar">
    <div class="seg">{#each KINDS as x (x.k)}<button class:on={kind === x.k} onclick={() => (kind = x.k)}>{x.l}{#if items && count(x.k)}<span class="n">{count(x.k)}</span>{/if}</button>{/each}</div>
    <span class="grow"></span>
    <div class="seg">{#each [7, 30] as d (d)}<button class:on={days === d} onclick={() => (days = d)}>最近 {d} 天</button>{/each}</div>
  </div>
  {#if items && items.length}
    <div class="card chart">
      <div class="lg"><span><i class="live"></i>上线</span><span><i class="done"></i>已处理</span><span><i class="bad"></i>没成功 / 回滚</span></div>
      <div class="cols" role="group" aria-label="最近 {days} 天每天处理完的条数">
        {#each bars as b (b.key)}
          {@const total = b.live + b.done + b.bad}
          {@const cs = cells(b)}
          <button class="col" style="height:{rows * 12 + (total > CAP ? 14 : 0)}px" disabled={!total} onclick={() => jump(b.key)} data-tip={`${b.label}：${total ? `上线 ${b.live} · 已处理 ${b.done} · 没成功 / 回滚 ${b.bad}` : '没有'}`} aria-label={`${b.label}：上线 ${b.live}，已处理 ${b.done}，没成功或回滚 ${b.bad}`}>
            {#each cs as c, n (n)}<i class="sq {c}"></i>{:else}<i class="sq empty"></i>{/each}
            {#if total > CAP}<span class="more">+{total - CAP}</span>{/if}
          </button>
        {/each}
      </div>
      <div class="ax"><span>{bars[0]?.label.split(' ')[0]}</span><span>今天</span></div>
    </div>
  {/if}
  {#if !items}
    <div class="card"><p class="hint">正在读取…</p></div>
  {:else if !groups.length}
    <div class="card"><p class="hint">最近 {days} 天{kind === 'all' ? '还没有交给 AI 处理完的' : `没有「${KINDS.find((x) => x.k === kind)?.l}」的`}。</p></div>
  {:else}
    {#each groups as g (g.key)}
      <section class="day" id="hd-{g.key}">
        <h4 class="dl">{g.label}<span class="faint">{g.items.length} 条</span></h4>
        <div class="card list"><RecentShips items={g.items} limit={1000} /></div>
      </section>
    {/each}
  {/if}
</div>

<style>
  .hb { display: flex; flex-direction: column; gap: 12px; }
  .bar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .grow { flex: 1; }
  .seg .n { margin-left: 5px; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .day { display: flex; flex-direction: column; gap: 8px; scroll-margin-top: 12px; }
  .dl { display: flex; align-items: baseline; gap: 8px; margin: 4px 2px 0; font-size: 12.5px; font-weight: 600; color: var(--dim); }
  .dl .faint { font-size: 11.5px; font-weight: 400; }
  /* 小方块图：每天一列，一个方块一条，方块之间留 2px 白缝；空的日子画一个淡色方块 */
  .chart { padding: 12px 18px 10px; }
  .lg { display: flex; gap: 14px; justify-content: flex-end; font-size: 11.5px; color: var(--dim); margin-bottom: 8px; }
  .lg span { display: inline-flex; align-items: center; gap: 5px; }
  .lg i { width: 10px; height: 10px; border-radius: 2.5px; }
  .cols { display: flex; align-items: flex-end; gap: 4px; }
  .col { flex: 1; min-width: 0; display: flex; flex-direction: column-reverse; align-items: center; gap: 2px; border: 0; padding: 0; background: none; cursor: pointer; border-radius: 4px; }
  .col:disabled { cursor: default; }
  .col:not(:disabled):hover .sq { filter: brightness(1.08); outline: 1px solid color-mix(in srgb, var(--text) 25%, transparent); outline-offset: 0; }
  .sq { display: block; width: 10px; height: 10px; border-radius: 2.5px; flex: none; }
  .sq.empty { background: var(--border); opacity: .6; }
  .live { background: var(--ok); }
  .done { background: #9a958a; }
  .bad { background: var(--err); }
  .more { font-size: 10px; line-height: 12px; color: var(--dim); font-variant-numeric: tabular-nums; }
  .ax { display: flex; justify-content: space-between; margin-top: 6px; font-size: 11px; color: var(--faint); }
  /* 卡片只留左右 18px（列表行自己会顶到两边），上下不留空，第一行上面不画分隔线 */
  .list { padding: 0 18px; overflow: hidden; }
  .list :global(.rows) { margin-bottom: 0 !important; }
  .list :global(.rows > li:first-child) { border-top: none; }
</style>
