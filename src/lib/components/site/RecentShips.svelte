<script lang="ts">
  // 托管页记录卡片里的「最近上线」tab：所有托管站点最近 30 天上线的改动，新的在前（卡片和 tab 在 ManagedConsole）。
  // 每条：站点、标题、动作、页面链接（在浏览器打开线上页面）、现在的状态（验收通过 / 待核对 / 已回滚）、自动上线还是人工确认；
  // 点开看 AI 的说明和线上验收；悬停可以看改动、回滚、去任务队列。
  // 全部站点 →「近期动态」也用它（每天一组，limit 不限）：那里还有没成功的、你驳回的，时间按处理完的时候算。
  // external：不经 Pilot 的提交（手改、CI 机器人），按提交时间和上线条目混排，一眼看出线上还有谁在改；
  // 上线之后碰到同一批文件 / 页面的，那条上线的效果栏标「有外部改动」（效果和复查都看不准了）。
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import DiffModal from './DiffModal.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { sites, openSite } from '$lib/state/sites.svelte.js';
  import { showDiff, rollbackItem, CHANGE_LABEL } from '$lib/state/ops.svelte.js';
  import { ago, full, pageUrl, dayOf, dayLabel, hhmm } from './fmt';
  import { modelName } from '$lib/state/fleet.svelte.js';
  import { kTok } from '$lib/state/managed.svelte.js';
  import { mdRender, mdClick } from '$lib/state/messages.svelte.js';
  import type { ShipItem, OpsItem, GscRow, ExternalItem } from '$lib/types';

  // byDay：列表里按天插小标题（今天 · 23 条），行尾时间今天的写「36 分钟前」、更早的写时刻（日期已经在标题里）
  let { items, external = [], limit = 5, empty = '最近 30 天托管站点没有上线改动。', byDay = false }: { items: ShipItem[]; external?: ExternalItem[]; limit?: number; empty?: string; byDay?: boolean } = $props();
  const LIMIT = $derived(limit);
  let all = $state(false);
  let open = $state<Set<string>>(new Set());
  function toggle(id: string) { const n = new Set(open); if (n.has(id)) n.delete(id); else n.add(id); open = n; }
  const at = (x: ShipItem) => x.item.deployed_at || x.item.updated_at;
  type Row = { kind: 'ship'; key: string; ts: number; x: ShipItem } | { kind: 'ext'; key: string; ts: number; e: ExternalItem };
  const rows = $derived.by(() => {
    const r: Row[] = [...items.map((x) => ({ kind: 'ship' as const, key: x.item.id, ts: at(x), x })), ...external.map((e) => ({ kind: 'ext' as const, key: `x${e.change.sha}`, ts: e.change.at, e }))];
    r.sort((a, b) => b.ts - a.ts);
    return r;
  });
  const shown = $derived(all ? rows : rows.slice(0, LIMIT));
  // 小标题上的条数按这一天全部的算（收起时只露前几条也照样是当天总数）；有回滚 / 待核对 / 没成功的才标出来；外部改动另计
  const isBad = (i: OpsItem) => i.status === 'rolled_back' || i.status === 'failed' || !!i.attention;
  const days = $derived.by(() => {
    if (!byDay) return [{ key: 'all', label: '', n: 0, ext: 0, sites: 0, bad: 0, rows: shown }];
    const out: { key: string; label: string; n: number; ext: number; sites: number; bad: number; rows: Row[] }[] = [];
    for (const r of shown) {
      const k = dayOf(r.ts);
      const g = out.find((o) => o.key === k);
      if (g) g.rows.push(r);
      else {
        const same = rows.filter((y) => dayOf(y.ts) === k);
        const ships = same.filter((y) => y.kind === 'ship') as Extract<Row, { kind: 'ship' }>[];
        out.push({ key: k, label: dayLabel(r.ts), n: ships.length, ext: same.length - ships.length, sites: new Set(ships.map((y) => y.x.site_id)).size, bad: ships.filter((y) => isBad(y.x.item)).length, rows: [r] });
      }
    }
    return out;
  });
  const today = $derived(dayOf(Date.now() / 1000));
  const when = (ts: number) => (byDay && dayOf(ts) !== today ? hhmm(ts) : ago(ts));
  // 「AI 怎么改的」是 Markdown（加粗、列表、代码）：渲染出来看；长的先收成几行，点「展开全文」看完整的
  let fullSum = $state<Set<string>>(new Set());
  function flipSum(id: string) { const n = new Set(fullSum); if (n.has(id)) n.delete(id); else n.add(id); fullSum = n; }
  const LONG = 220;
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
    // 上线之后有不经 Pilot 的提交改到同一批文件 / 页面：涨跌分不清是谁的，复盘结论也只能存疑
    const ext = x.external_after ?? [];
    const extNote = ext.length ? `
上线后有 ${ext.length} 次外部改动碰到同一批文件 / 页面，效果分不清是谁的：
${ext.map((c) => `${full(c.at)} ${c.author}：${c.message}`).join('\n')}` : '';
    if (i.review) {
      const l = i.review === 'positive' ? '有效' : i.review === 'negative' ? '负面' : '持平';
      return { t: l, c: i.review === 'positive' ? 'ok' : i.review === 'negative' ? 'err' : 'line', tip: `上线满 4 周的复盘：${i.review_note}${extNote}` };
    }
    if (ext.length && i.status !== 'rolled_back') return { t: '有外部改动', c: 'warn', tip: extNote.trim() };
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
  {#if !rows.length}<p class="hint">{empty}</p>{/if}
  {#if rows.length}
    <ol class="rows">
      {#each days as d (d.key)}
      {#if d.label}<li class="dh"><b>{d.label}</b><span>{d.n} 条{#if d.sites > 1} · {d.sites} 个站点{/if}{#if d.ext} · {d.ext} 条外部改动{/if}</span>{#if d.bad}<span class="dh-bad">{d.bad} 条回滚 / 待核对 / 没成功</span>{/if}</li>{/if}
      {#each d.rows as r (r.key)}
      {#if r.kind === 'ext'}
        {@const e = r.e}
        {@const c = e.change}
        {@const isOpen = open.has(r.key)}
        {@const page = c.pages[0] || ''}
        <!-- 外部改动：不经 Pilot 的提交。标题是提交说明，效果栏放文件数，「自动上线 / 人工确认」那格放提交人 -->
        <li class="ext" class:open={isOpen}>
          <div class="row">
            <button class="main" onclick={() => toggle(r.key)} aria-expanded={isOpen}>
              <span class="sn"><SiteIcon id={e.site_id} size={13} />{e.site_name}</span>
              <span class="tt" data-tip={c.message}>{c.message}</span>
            </button>
            {#if page}<button class="url mono" onclick={() => openUrl(page)} data-tip={`在浏览器打开：${page}`}><span class="up">{shortPath(page)}</span><Icon name="external" size={10} /></button>{/if}
            <span class="eff line" data-tip={c.files.slice(0, 12).join('\n') + (c.files.length > 12 ? `\n… 共 ${c.files.length} 个` : '')}>{c.files.length} 个文件</span>
            <span class="how faint who" data-tip={`提交人：${c.author}（在 ${c.on} 上）`}>{c.author}</span>
            <span class="pill line">外部改动</span>
            <span class="t faint" data-tip={full(c.at)}>{when(c.at)}</span>
          </div>
          {#if isOpen}
            <div class="more">
              <div class="meta"><span>提交 {c.sha.slice(0, 7)}</span><span>{c.author}</span><span>在 {c.on} 上</span><span>{full(c.at)}</span></div>
              <div class="blk"><span class="lb">改了的文件</span><p class="files mono">{c.files.join('　')}</p></div>
              {#if c.pages.length}<div class="blk"><span class="lb">可能影响</span><p>{c.pages.map(shortPath).join('、')}</p></div>{/if}
              <p class="hint">不经 Pilot 运营流程进到仓库的提交（手改、CI 机器人、别的工具）。它碰到 Pilot 最近改过的页面时，那条上线的效果栏会标「有外部改动」。</p>
            </div>
          {/if}
        </li>
      {:else}
        {@const x = r.x}
        {@const i = x.item}
        {@const st = shipState(i)}
        {@const site = siteOf(x.site_id)}
        {@const isOpen = open.has(i.id)}
        {@const ef = effect(x)}
        <!-- 写新页面的条目计划里没有 page，用 AI 报告的新页面；再没有就用改到的第一个页面 -->
        {@const url = pageUrl(i.opp.page, site?.domain) || (i.new_pages ?? [])[0] || (i.pages ?? [])[0] || ''}
        <li class:open={isOpen}>
          <div class="row">
            <button class="main" onclick={() => toggle(i.id)} aria-expanded={isOpen}>
              <span class="sn"><SiteIcon id={x.site_id} size={13} />{x.site_name}</span>
              <span class="tt" class:gone={i.status === 'rolled_back' || i.status === 'dismissed'} data-tip={i.opp.title}>{i.opp.title}</span>
            </button>
            {#if url}<button class="url mono" onclick={() => openUrl(url)} data-tip={`在浏览器打开：${url}`}><span class="up">{shortPath(url)}</span><Icon name="external" size={10} /></button>{/if}
            <span class="acts">
              {#if site && i.head_sha}<button class="ic-btn ic-sm" onclick={() => showDiff(site.id, i)} aria-label="看改动" data-tip="看改动"><Icon name="diff" size={13} /></button>{/if}
              {#if site && i.status === 'deployed' && i.merge_sha}<button class="ic-btn ic-sm" onclick={() => rollbackItem(site, i)} aria-label="回滚" data-tip="回滚这次改动（先确认）"><Icon name="undo" size={13} /></button>{/if}
              <button class="ic-btn ic-sm" onclick={() => openSite(x.site_id, 'queue', '', i.id)} aria-label="在任务队列里看" data-tip="在任务队列里看"><Icon name="inbox" size={13} /></button>
            </span>
            <span class="eff {ef.c}" data-tip={ef.tip || null}>{#if ef.c === 'ok' || ef.c === 'err' || ef.c === 'warn' || (ef.c === 'line' && i.review)}<span class="pill {ef.c}">{ef.t}</span>{:else}{ef.t}{/if}</span>
            <span class="how faint" data-tip={!i.deployed_at ? null : i.auto ? '检查都通过后自动上线' : '人工确认后才上线的'}>{!i.deployed_at ? '' : i.auto ? '自动上线' : '人工确认'}</span>
            <span class="pill {st.c}">{st.l}</span>
            <span class="t faint" data-tip={full(at(x))}>{when(at(x))}</span>
          </div>
          {#if isOpen}
            {@const sum = i.ai_summary_zh || i.ai_summary}
            <div class="more">
              <!-- 一行小字：改动类型、模型和用量、线上验收 -->
              <div class="meta">
                {#if CHANGE_LABEL[i.opp.change]}<span class="tag">{CHANGE_LABEL[i.opp.change]}</span>{/if}
                {#if i.model_used || x.cost?.turns}<span>{#if i.model_used}{modelName(i.model_used) || i.model_used}{/if}{#if x.cost?.turns}{i.model_used ? ' · ' : ''}{kTok(x.cost.tokens)} token{#if x.cost.usd >= 0.01}（API 约 ${x.cost.usd.toFixed(2)}）{/if}{#if x.cost.sub_tokens}{` · 订阅额度内 ${kTok(x.cost.sub_tokens)}`}{/if}{/if}</span>{/if}
                {#if i.checks?.length}
                  {@const bad = i.checks.filter((c) => !c.ok)}
                  <span>线上验收 {i.checks.length - bad.length} 项通过{#if bad.length}，<span class="warn-text">{bad.length} 项没过（{bad.map((c) => c.label).join('、')}）</span>{/if}</span>
                {/if}
              </div>
              {#if i.attention}<p class="warn-text">要你核对：{i.attention}</p>{/if}
              {#if i.status === 'failed' && i.note}<p class="err-t">没成功：{i.note}</p>{/if}
              {#if ef.tip}<div class="blk"><span class="lb">效果</span><p class="eff-d">{ef.tip}</p></div>{/if}
              {#if sum}
                <div class="blk">
                  <span class="lb">AI 怎么改的</span>
                  <div>
                    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                    <div class="sum" class:clamp={sum.length > LONG && !fullSum.has(i.id)} onclick={(e) => mdClick(e, null)}>{@html mdRender(sum)}</div>
                    {#if sum.length > LONG}<button class="link-btn more-t" onclick={() => flipSum(i.id)}>{fullSum.has(i.id) ? '收起' : '展开全文'}</button>{/if}
                  </div>
                </div>
              {/if}
            </div>
          {/if}
        </li>
      {/if}
      {/each}
      {/each}
    </ol>
    {#if rows.length > LIMIT}<button class="all" onclick={() => (all = !all)}>{all ? '收起' : `显示全部 ${rows.length} 条`}<svg class="chev" class:open={all} width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5 6 7.5 9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>{/if}
  {/if}
</div>
<DiffModal />

<style>
  .rows { list-style: none; margin: 0 -18px; padding: 0; }
  .rows > li { border-top: 1px solid var(--border); }
  /* 每天一个小标题：翻长列表时贴在顶上，随时知道看到哪天了 */
  .rows > li.dh { position: sticky; top: 0; z-index: 1; display: flex; align-items: baseline; gap: 8px; padding: 7px 18px 6px; background: #faf9f6; font-size: 11.5px; color: var(--faint); }
  .dh b { font-size: 12px; font-weight: 600; color: var(--dim); }
  .dh-bad { color: var(--err); }
  .rows > li.dh:hover { background: #faf9f6; }
  .row { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 4px 18px; }
  .rows > li:hover .row { background: #fcfbf9; }
  .main { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; border: 0; background: none; padding: 6px 0; font: inherit; color: inherit; text-align: left; cursor: pointer; }
  .sn { flex: none; display: inline-flex; align-items: center; gap: 5px; max-width: 150px; font-size: 12px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tt { min-width: 0; font-size: 13px; font-weight: 550; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tt.gone { color: var(--faint); text-decoration: line-through; }
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
  /* 外部改动那行：标题用提交说明、不加粗，提交人名字截短 */
  .ext .tt { font-weight: 450; color: var(--dim); }
  .who { overflow: hidden; text-overflow: ellipsis; }
  .files { overflow-wrap: anywhere; font-size: 11.5px !important; }
  .eff.up { color: var(--ok); }
  .eff.down { color: var(--err); }
  .eff-d { white-space: pre-line; }
  .err-t { color: var(--err) !important; }
  .how { flex: none; width: 52px; text-align: right; font-size: 11px; white-space: nowrap; }
  .row > .pill { flex: none; min-width: 74px; justify-content: center; }
  .t { flex: none; width: 64px; text-align: right; font-size: 11.5px; white-space: nowrap; }
  .more { padding: 2px 18px 14px; display: flex; flex-direction: column; align-items: stretch; gap: 10px; }
  .more p { margin: 0; font-size: 12.5px; line-height: 1.65; color: var(--dim); }
  .meta { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 14px; font-size: 11.5px; color: var(--faint); }
  /* 左边一列小标签、右边内容，几块上下对齐 */
  .blk { display: grid; grid-template-columns: 76px minmax(0, 1fr); gap: 12px; align-items: baseline; }
  .lb { font-size: 11.5px; color: var(--faint); white-space: nowrap; }
  .sum { font-size: 12.5px; line-height: 1.7; color: var(--text); max-width: 760px; }
  .sum.clamp { max-height: 8.5em; overflow: hidden; -webkit-mask-image: linear-gradient(#000 60%, transparent); mask-image: linear-gradient(#000 60%, transparent); }
  .sum :global(p) { margin: 0 0 6px; color: inherit; }
  .sum :global(p:last-child), .sum :global(ul:last-child), .sum :global(ol:last-child) { margin-bottom: 0; }
  .sum :global(ul), .sum :global(ol) { margin: 2px 0 6px; padding-left: 18px; }
  .sum :global(li) { margin: 3px 0; }
  .sum :global(strong) { font-weight: 600; }
  .sum :global(code) { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11.5px; padding: 1px 5px; border-radius: 4px; background: var(--accent-soft); overflow-wrap: anywhere; }
  .sum :global(a) { color: inherit; text-decoration: underline; text-underline-offset: 3px; }
  .more-t { margin-top: 4px; font-size: 12px; }
  .tag { display: inline-flex; align-items: center; height: 20px; padding: 0 8px; border-radius: 999px; background: var(--accent-soft); font-size: 11px; color: var(--dim); }
  .all { display: flex; align-items: center; justify-content: center; gap: 5px; width: calc(100% + 36px); margin: 0 -18px -16px; padding: 9px 18px; border: 0; border-top: 1px solid var(--border); border-radius: 0 0 12px 12px; background: none; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; }
  .all .chev { transition: transform .15s; }
  .all .chev.open { transform: rotate(180deg); }
  .all:hover { color: var(--text); background: #fcfbf9; }
  .rows:last-child { margin-bottom: -16px; }
  .rows:last-child > li:last-child .row { border-radius: 0 0 12px 12px; }
</style>
