<script lang="ts">
  // 全部站点 →「待处理」→「运营机会」：自己运营的站点里还没处理的机会，按优先级分三组——
  // 先处理（正在伤害站点）、收益大、顺手做。每条可以让 AI 改、驳回、打开站点；每组可以一键全交给 AI（同时最多 3 个站点在跑）。
  // 托管站点的机会由 AI 按托管规则自动处理，这里只给个数。
  import Icon from './Icon.svelte';
  import SiteIcon from './SiteIcon.svelte';
  import { fleet, loadOpps, workOpps, dismissOpps } from '$lib/state/fleet.svelte.js';
  import { KIND_LABEL } from '$lib/state/ops.svelte.js';
  import { openSite } from '$lib/state/sites.svelte.js';
  import { num } from './fmt';
  import type { OppItem } from '$lib/types';

  $effect(() => { void loadOpps(); });
  const o = $derived(fleet.opps);
  const TIERS = [
    { k: 'urgent', l: '先处理', d: '正在伤害站点：抓取出错、页面打不开或太慢、点击掉得厉害' },
    { k: 'value', l: '收益大', d: '按数据估算多拿点击最多的：点击率低、排名 8–20、内容缺口、对比竞品' },
    { k: 'easy', l: '顺手做', d: '小改动：标题年份、久未更新、结构化数据、图片替代文字' },
  ] as const;
  const groups = $derived(TIERS.map((t) => ({ ...t, items: (o?.items ?? []).filter((i) => i.tier === t.k) })).filter((g) => g.items.length));
  let expanded = $state<string[]>([]);
  const SHOW = 8;
  const busy = (i: OppItem) => !!o?.running.includes(i.site_id);
  const handed = (i: OppItem) => fleet.handed.includes(i.item.id);
  // 这一组里还能交给 AI 的（排着的、站点正在跑作业的不算）
  const todo = (items: OppItem[]) => items.filter((i) => !handed(i));
</script>

{#if !o}
  <div class="card"><p class="hint">正在读取…</p></div>
{:else}
  <p class="sum faint">
    {#if o.items.length}自己运营的站点有 {o.items.length} 个机会没处理{:else}自己运营的站点暂时没有新机会{/if}{#if o.managed_open} · 托管站点的 {o.managed_open} 个由 AI 按托管规则自动处理{/if}{#if o.waiting} · <span class="run">还有 {o.waiting} 个站点排着等 AI</span>{/if}
  </p>
  {#each groups as g (g.k)}
    {@const open = expanded.includes(g.k)}
    {@const left = todo(g.items)}
    <div class="card tbl-card grp">
      <div class="gh">
        <span class="dot {g.k}"></span><b>{g.l}</b><span class="faint n">{g.items.length}</span>
        <small class="faint d">{g.d}</small>
        <span class="grow"></span>
        {#if left.length}<button class="btn tiny" onclick={() => workOpps(left)} data-tip="这一组都交给 AI：每个站点一个作业，同时最多 3 个，改好留给你确认"><Icon name="wand" size={11} />这一组都让 AI 改</button>{/if}
      </div>
      <ul class="rows">
        {#each open ? g.items : g.items.slice(0, SHOW) as i (i.item.id)}
          <li class="row">
            <button class="main" onclick={() => openSite(i.site_id, 'queue')} data-tip={`打开 ${i.domain || i.site_name} 的待处理\n\n${i.item.opp.evidence}`}>
              <span class="site"><SiteIcon id={i.site_id} size={13} /><span class="sn">{i.domain || i.site_name}</span></span>
              <span class="tt">{i.item.opp.title}<small class="kind">{KIND_LABEL[i.item.opp.kind] ?? ''}</small></span>
            </button>
            <span class="gain faint">{i.gain >= 1 ? `约 +${num(Math.round(i.gain))} 次点击/月` : ''}</span>
            <span class="acts">
              {#if handed(i)}<span class="pill"><span class="spin"></span>排队等 AI</span>
              {:else if busy(i)}<span class="pill" data-tip="这个站点正在跑别的作业，等它跑完再交给 AI"><span class="spin"></span>站点忙</span>
              {:else}
                <button class="ic-btn ic-sm" onclick={() => workOpps([i], false)} aria-label="让 AI 改" data-tip="让 AI 改：改好、检查通过后留给你确认"><Icon name="wand" size={13} /></button>
                <button class="ic-btn ic-sm" onclick={() => dismissOpps([i])} aria-label="驳回" data-tip="驳回：之后不再提这个机会"><Icon name="x" size={13} /></button>
              {/if}
            </span>
          </li>
        {/each}
      </ul>
      {#if g.items.length > SHOW}
        <button class="more link-btn" onclick={() => (expanded = open ? expanded.filter((k) => k !== g.k) : [...expanded, g.k])}>{open ? '收起' : `展开其余 ${g.items.length - SHOW} 条`}</button>
      {/if}
    </div>
  {/each}
{/if}

<style>
  .sum { margin: 0 2px 10px; font-size: 12.5px; }
  .sum .run { color: var(--su-run, var(--warn)); }
  .grp + .grp { margin-top: 12px; }
  .gh { display: flex; align-items: center; gap: 8px; padding: 11px 16px; border-bottom: 1px solid var(--border); font-size: 13px; }
  .gh b { font-weight: 600; }
  .gh .n { font-size: 12px; font-variant-numeric: tabular-nums; }
  .gh .d { min-width: 0; font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { flex: none; width: 7px; height: 7px; border-radius: 50%; }
  .dot.urgent { background: var(--err); }
  .dot.value { background: var(--ok); }
  .dot.easy { background: #b9a36a; }
  .grow { flex: 1; }
  .rows { list-style: none; margin: 0; padding: 0; }
  .row { display: flex; align-items: center; gap: 12px; padding: 0 12px 0 16px; min-height: 42px; border-top: 1px solid var(--border); }
  .row:first-child { border-top: none; }
  .main { flex: 1; min-width: 0; display: flex; align-items: center; gap: 14px; border: 0; background: none; padding: 9px 0; font: inherit; font-size: 12.5px; color: var(--text); text-align: left; cursor: pointer; }
  .main:hover .tt { text-decoration: underline; text-underline-offset: 3px; text-decoration-color: var(--border2); }
  .site { flex: none; width: 168px; display: inline-flex; align-items: center; gap: 7px; font-weight: 600; min-width: 0; }
  .sn { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tt { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .kind { margin-left: 8px; color: var(--faint); font-size: 11.5px; }
  .gain { flex: none; font-size: 11.5px; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .acts { flex: none; display: inline-flex; align-items: center; gap: 2px; min-width: 60px; justify-content: flex-end; }
  .acts .pill { gap: 5px; }
  .more { display: block; width: 100%; padding: 8px 0 10px; border-top: 1px solid var(--border); font-size: 12px; text-align: center; }
  @container page (max-width: 760px) {
    .site { width: 120px; }
    .gain, .gh .d { display: none; }
  }
</style>
