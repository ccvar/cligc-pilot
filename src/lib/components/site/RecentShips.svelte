<script lang="ts">
  // 托管页记录卡片里的「最近上线」tab：所有托管站点最近 30 天上线的改动，新的在前（卡片和 tab 在 ManagedConsole）。
  // 每条：站点、标题、动作、页面链接（在浏览器打开线上页面）、现在的状态（验收通过 / 待核对 / 已回滚）、自动上线还是人工确认；
  // 点开看 AI 的说明和线上验收；悬停可以看改动、回滚、去任务队列。
  // 全部站点 →「处理记录」也用它（每天一组，limit 不限）：那里还有没成功的、你驳回的，时间按处理完的时候算。
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import DiffModal from './DiffModal.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { sites, openSite } from '$lib/state/sites.svelte.js';
  import { showDiff, rollbackItem, CHANGE_LABEL } from '$lib/state/ops.svelte.js';
  import { ago, full } from './fmt';
  import { modelName } from '$lib/state/fleet.svelte.js';
  import { kTok } from '$lib/state/managed.svelte.js';
  import type { ShipItem, OpsItem, GscRow } from '$lib/types';

  let { items, limit = 5, empty = '最近 30 天托管站点没有上线改动。' }: { items: ShipItem[]; limit?: number; empty?: string } = $props();
  const LIMIT = $derived(limit);
  let all = $state(false);
  let open = $state<Set<string>>(new Set());
  function toggle(id: string) { const n = new Set(open); if (n.has(id)) n.delete(id); else n.add(id); open = n; }
  const shown = $derived(all ? items : items.slice(0, LIMIT));
  const siteOf = (id: string) => sites.list.find((s) => s.id === id);

  function shipState(i: OpsItem) {
    if (i.status === 'failed') return { l: '没成功', c: 'err' };
    if (i.status === 'dismissed') return { l: '你驳回了', c: 'line' };
    if (i.status === 'rolled_back') return { l: '已回滚', c: 'line' };
    if (i.status === 'done') return { l: '已处理', c: '' };
    if (i.attention) return { l: '上线待核对', c: 'warn' };
    const bad = (i.checks ?? []).filter((c) => !c.ok).length;
    if (i.checks?.length && !bad) return { l: '验收通过', c: 'ok' };
    return { l: '已上线', c: 'ok' };
  }
  // 效果：复盘出结论（上线满 31 天）前，拿上线前 28 天和现在近 28 天的点击比，标明还是早期数据
  const DAY = 86_400;
  type Effect = { t: string; c: string; tip: string };
  function effect(x: ShipItem): Effect {
    const i = x.item;
    // 没上线过的（已处理、没成功、你驳回了）不谈效果
    if (!i.deployed_at) return { t: '', c: '', tip: '' };
    const days = Math.floor((Date.now() / 1000 - i.deployed_at) / DAY);
    if (i.review) {
      const l = i.review === 'positive' ? '有效' : i.review === 'negative' ? '负面' : '持平';
      return { t: l, c: i.review === 'positive' ? 'ok' : i.review === 'negative' ? 'err' : 'line', tip: `上线满 4 周的复盘：${i.review_note}` };
    }
    if (i.status === 'rolled_back') return { t: '', c: '', tip: '' };
    const now = x.page_now, b = i.before;
    const wait = Math.max(0, 31 - days);
    const tail = `\n上线 ${days} 天${wait ? `，再过 ${wait} 天出复盘结论（搜索数据有 2–3 天延迟，前几周的数字只是苗头）` : '，复盘结论稍后出'}`;
    if (!b) {
      // 新页面：先看有没有被 Google 收录（上线 3 天起用 URL 检查确认），收录了再看展示
      const idx = i.index_state === 'indexed' ? '已被 Google 收录' : i.index_state === 'not_indexed' ? `还没被 Google 收录（${i.index_note}）` : i.index_state === 'error' ? `收录情况查不了：${i.index_note}` : days < 3 ? '上线 3 天后开始确认有没有被收录' : '等确认收录';
      if (i.index_state === 'not_indexed') return { t: '未收录', c: 'down', tip: `新页面${idx}${tail}` };
      if (!now) return { t: i.index_state === 'indexed' ? '已收录' : days < 4 ? '等数据' : '还没展示', c: i.index_state === 'indexed' ? '' : 'line', tip: `新页面，${idx}；Google 还没有展示记录${tail}` };
      return { t: `展示 ${now.impressions}`, c: '', tip: `新页面，${idx}；近 28 天：展示 ${now.impressions}、点击 ${now.clicks}${now.position ? `、平均排名 ${now.position.toFixed(1)}` : ''}${tail}` };
    }
    if (days < 4) return { t: '等数据', c: 'line', tip: `上线前 28 天：${rowText(b)}${tail}` };
    const after = now ?? { clicks: 0, impressions: 0, ctr: 0, position: 0, key: '' };
    // 前后都没点击（很多站还在起步）就看展示的变化
    const byClicks = b.clicks > 0 || after.clicks > 0;
    const d = byClicks ? after.clicks - b.clicks : after.impressions - b.impressions;
    const what = byClicks ? '点击' : '展示';
    return {
      t: d === 0 ? `${what}持平` : `${what} ${d > 0 ? '+' : ''}${d}`,
      c: d > 0 ? 'up' : d < 0 ? 'down' : '',
      tip: `上线前 28 天：${rowText(b)}\n现在近 28 天：${rowText(after)}${x.data_end ? `（数据到 ${x.data_end}）` : ''}${tail}`,
    };
  }
  const rowText = (r: GscRow) => `点击 ${r.clicks} · 展示 ${r.impressions}${r.position ? ` · 排名 ${r.position.toFixed(1)}` : ''}`;

  /** 页面链接显示成短路径：https://iosur.com/zh/posts/x/ → /zh/posts/x/ */
  const shortPath = (u: string) => { try { const x = new URL(u); return x.pathname + x.search || '/'; } catch { return u; } };
</script>

<div class="rs">
  {#if !items.length}<p class="hint">{empty}</p>{/if}
  {#if items.length}
    <ol class="rows">
      {#each shown as x (x.item.id)}
        {@const i = x.item}
        {@const st = shipState(i)}
        {@const site = siteOf(x.site_id)}
        {@const isOpen = open.has(i.id)}
        {@const ef = effect(x)}
        <li class:open={isOpen}>
          <div class="row">
            <button class="main" onclick={() => toggle(i.id)} aria-expanded={isOpen}>
              <span class="sn"><SiteIcon id={x.site_id} size={13} />{x.site_name}</span>
              <span class="tt" class:gone={i.status === 'rolled_back' || i.status === 'dismissed'}>{i.opp.title}</span>
            </button>
            {#if i.opp.page}<button class="url mono" onclick={() => openUrl(i.opp.page)} data-tip={`在浏览器打开：${i.opp.page}`}><span class="up">{shortPath(i.opp.page)}</span><Icon name="external" size={10} /></button>{/if}
            <span class="acts">
              {#if site && i.head_sha}<button class="ic-btn ic-sm" onclick={() => showDiff(site.id, i)} aria-label="看改动" data-tip="看改动"><Icon name="diff" size={13} /></button>{/if}
              {#if site && i.status === 'deployed' && i.merge_sha}<button class="ic-btn ic-sm" onclick={() => rollbackItem(site, i)} aria-label="回滚" data-tip="回滚这次改动（先确认）"><Icon name="undo" size={13} /></button>{/if}
              <button class="ic-btn ic-sm" onclick={() => openSite(x.site_id, 'queue', i.status === 'failed' ? 'todo' : i.status === 'dismissed' || i.status === 'rolled_back' ? 'closed' : 'live')} aria-label="在任务队列里看" data-tip="在任务队列里看"><Icon name="inbox" size={13} /></button>
            </span>
            <span class="eff {ef.c}" data-tip={ef.tip || null}>{#if ef.c === 'ok' || ef.c === 'err' || (ef.c === 'line' && i.review)}<span class="pill {ef.c}">{ef.t}</span>{:else}{ef.t}{/if}</span>
            <span class="how faint" data-tip={!i.deployed_at ? null : i.auto ? '检查都通过后自动上线' : '人工确认后才上线的'}>{!i.deployed_at ? '' : i.auto ? '自动上线' : '人工确认'}</span>
            <span class="pill {st.c}">{st.l}</span>
            <span class="t faint" data-tip={full(i.deployed_at || i.updated_at)}>{ago(i.deployed_at || i.updated_at)}</span>
          </div>
          {#if isOpen}
            <div class="more">
              {#if CHANGE_LABEL[i.opp.change]}<span class="tag">{CHANGE_LABEL[i.opp.change]}</span>{/if}
              {#if i.attention}<p class="warn-text">要你核对：{i.attention}</p>{/if}
              {#if ef.tip}<p class="eff-d">{ef.tip}</p>{/if}
              {#if i.status === 'failed' && i.note}<p class="err-t">没成功：{i.note}</p>{/if}
              {#if i.ai_summary}<p>{i.ai_summary_zh || i.ai_summary}</p>{/if}
              {#if i.model_used || x.cost?.turns}<p class="faint">{#if i.model_used}模型：{modelName(i.model_used) || i.model_used}{/if}{#if x.cost?.turns}{i.model_used ? ' · ' : ''}这一条用了 {kTok(x.cost.tokens)} token{#if x.cost.usd >= 0.01}，API 约 ${x.cost.usd.toFixed(2)}{/if}{#if x.cost.sub_tokens} · 订阅额度内 {kTok(x.cost.sub_tokens)}{/if}{/if}</p>{/if}
              {#if i.checks?.length}
                {@const bad = i.checks.filter((c) => !c.ok)}
                <p class="faint">线上验收：{i.checks.length - bad.length} 项通过{#if bad.length}，<span class="warn-text">{bad.length} 项没过（{bad.map((c) => c.label).join('、')}）</span>{/if}</p>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ol>
    {#if items.length > LIMIT}<button class="all" onclick={() => (all = !all)}>{all ? '收起' : `显示全部 ${items.length} 条`}</button>{/if}
  {/if}
</div>
<DiffModal />

<style>
  .rows { list-style: none; margin: 0 -18px; padding: 0; }
  .rows > li { border-top: 1px solid var(--border); }
  .row { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 4px 18px; }
  .rows > li:hover .row { background: #fcfbf9; }
  .main { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; border: 0; background: none; padding: 6px 0; font: inherit; color: inherit; text-align: left; cursor: pointer; }
  .sn { flex: none; display: inline-flex; align-items: center; gap: 5px; max-width: 150px; font-size: 12px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tt { min-width: 0; font-size: 13px; font-weight: 550; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tt.gone { color: var(--faint); text-decoration: line-through; }
  li.open .tt { white-space: normal; }
  /* 路径太长时只截路径文字，后面的 ↗ 图标一直露着 */
  .url { flex: 0 1 auto; min-width: 0; max-width: 260px; display: inline-flex; align-items: center; gap: 3px; border: 0; background: none; padding: 0; font-size: 11px; color: var(--faint); cursor: pointer; white-space: nowrap; }
  .url .up { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .url :global(svg) { flex: none; }
  .url:hover { color: var(--text); text-decoration: underline; text-underline-offset: 3px; }
  .acts { flex: none; display: inline-flex; gap: 2px; opacity: 0; transition: opacity .12s; }
  .rows > li:hover .acts, .acts:focus-within { opacity: 1; }
  .acts .ic-btn { margin: 0; }
  /* 「自动上线 / 人工确认」和状态胶囊各占固定宽度，几行上下对齐 */
  /* 效果：复盘前是点击变化（涨绿跌红）或「等数据」，复盘后是 有效 / 持平 / 负面 */
  .eff { flex: none; width: 76px; text-align: right; font-size: 11.5px; color: var(--dim); white-space: nowrap; font-variant-numeric: tabular-nums; }
  .eff.line { color: var(--faint); }
  .eff.up { color: var(--ok); }
  .eff.down { color: var(--err); }
  .eff-d { white-space: pre-line; }
  .err-t { color: var(--err) !important; }
  .how { flex: none; width: 52px; text-align: right; font-size: 11px; white-space: nowrap; }
  .row > .pill { flex: none; min-width: 74px; justify-content: center; }
  .t { flex: none; width: 64px; text-align: right; font-size: 11.5px; white-space: nowrap; }
  .more { padding: 0 18px 12px; display: flex; flex-direction: column; align-items: flex-start; gap: 6px; }
  .more p { margin: 0; font-size: 12.5px; line-height: 1.65; color: var(--dim); }
  .tag { display: inline-flex; align-items: center; height: 20px; padding: 0 8px; border-radius: 999px; background: var(--accent-soft); font-size: 11px; color: var(--dim); }
  .all { display: block; width: calc(100% + 36px); margin: 0 -18px -16px; padding: 9px 18px; border: 0; border-top: 1px solid var(--border); border-radius: 0 0 12px 12px; background: none; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; }
  .all:hover { color: var(--text); background: #fcfbf9; }
  .rows:last-child { margin-bottom: -16px; }
  .rows:last-child > li:last-child .row { border-radius: 0 0 12px 12px; }
</style>
