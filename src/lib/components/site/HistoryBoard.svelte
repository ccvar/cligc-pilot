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
  const count = (k: (typeof KINDS)[number]['k']) => (items ?? []).filter((x) => { const s = x.item.status; return k === 'all' || (k === 'live' ? s === 'deployed' || s === 'done' : k === 'bad' ? s === 'failed' || s === 'rolled_back' : s === 'dismissed'); }).length;
</script>

<div class="hb">
  <div class="bar">
    <div class="seg">{#each KINDS as x (x.k)}<button class:on={kind === x.k} onclick={() => (kind = x.k)}>{x.l}{#if items && count(x.k)}<span class="n">{count(x.k)}</span>{/if}</button>{/each}</div>
    <span class="grow"></span>
    <div class="seg">{#each [7, 30] as d (d)}<button class:on={days === d} onclick={() => (days = d)}>最近 {d} 天</button>{/each}</div>
  </div>
  {#if !items}
    <div class="card"><p class="hint">正在读取…</p></div>
  {:else if !groups.length}
    <div class="card"><p class="hint">最近 {days} 天{kind === 'all' ? '还没有交给 AI 处理完的' : `没有「${KINDS.find((x) => x.k === kind)?.l}」的`}。</p></div>
  {:else}
    {#each groups as g (g.key)}
      <section class="day">
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
  .day { display: flex; flex-direction: column; gap: 8px; }
  .dl { display: flex; align-items: baseline; gap: 8px; margin: 4px 2px 0; font-size: 12.5px; font-weight: 600; color: var(--dim); }
  .dl .faint { font-size: 11.5px; font-weight: 400; }
  /* 卡片只留左右 18px（列表行自己会顶到两边），上下不留空，第一行上面不画分隔线 */
  .list { padding: 0 18px; overflow: hidden; }
  .list :global(.rows) { margin-bottom: 0 !important; }
  .list :global(.rows > li:first-child) { border-top: none; }
</style>
