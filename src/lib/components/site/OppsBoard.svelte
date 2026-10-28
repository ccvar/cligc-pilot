<script lang="ts">
  // 全部站点 →「待处理」→「运营机会」：没人自动处理的站点（自己运营的、托管停着的）里还没处理的机会。
  // 最上面一块是 AI 能直接处理、不用你拿主意的（改标题描述、sitemap、图片…），一键全交给 AI，默认收起明细；
  // 交给 AI 以后从列表挪到「处理进度」：排队、处理中（走到哪一步）、等你确认、已处理、没成功，一天内的都在。
  // 剩下要你判断的按优先级分三组——先处理（正在伤害站点）、收益大、顺手做。每条可以让 AI 改、驳回、打开站点。
  // 交给 AI 用默认模型（设置里改）；按钮右边的 ▾、魔杖上右键可以换个模型处理这一次。
  // 托管且在运行的站点由 AI 按托管规则自动处理，这里只给个数。
  import Icon from './Icon.svelte';
  import SiteIcon from './SiteIcon.svelte';
  import { fleet, loadOpps, workOpps, dismissOpps, stopAcross, batchPick, setBatchModel, modelTarget, modelName } from '$lib/state/fleet.svelte.js';
  import ModelMenu from './ModelMenu.svelte';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import { confirmDialog } from '$lib/state/app.svelte.js';
  import { KIND_LABEL } from '$lib/state/ops.svelte.js';
  import { openSite, cancelSiteJob } from '$lib/state/sites.svelte.js';
  import { num } from './fmt';
  import type { OppItem } from '$lib/types';

  $effect(() => { void loadOpps(); });
  const o = $derived(fleet.opps);
  const sitesN = $derived(new Set((o?.items ?? []).map((i) => i.site_id)).size);
  const TIERS = [
    { k: 'urgent', l: '先处理', d: '正在伤害站点：页面太慢、点击掉得厉害' },
    { k: 'value', l: '收益大', d: '按数据估算多拿点击最多的：排名 8–20 要扩写、内容缺口要新写、对比竞品' },
    { k: 'easy', l: '顺手做', d: '改动不大，但要你看一眼：久未更新的页面、内链' },
  ] as const;
  const isQueued = (i: OppItem) => !!o?.queued.includes(i.item.id);
  const open_ = $derived((o?.items ?? []).filter((i) => !isQueued(i)));
  const ready = $derived(open_.filter((i) => i.ai));
  const groups = $derived(TIERS.map((t) => ({ ...t, items: open_.filter((i) => !i.ai && i.tier === t.k) })).filter((g) => g.items.length));
  // 处理进度：处理中 → 排队 → 没成功 → 等你确认 → 已处理
  const stage = (i: OppItem) => (isQueued(i) && i.item.status === 'open' ? 'queued' : i.item.status === 'working' ? 'working' : i.item.status === 'failed' ? 'failed' : i.item.status === 'proposed' ? 'proposed' : 'done');
  const ORDER = { working: 0, queued: 1, failed: 2, proposed: 3, done: 4 } as const;
  const allRecent = $derived([...(o?.items ?? []).filter(isQueued), ...(o?.recent ?? [])].sort((a, b) => ORDER[stage(a)] - ORDER[stage(b)]));
  // 还有排队 / 处理中的：整批都列出来看进度；都跑完了就只留没成功的（要你重试），等你确认的在上面「等你确认」里，已处理的收起来
  const active = $derived(allRecent.some((i) => ['queued', 'working'].includes(stage(i))));
  const progress = $derived(active ? allRecent : allRecent.filter((i) => stage(i) === 'failed'));
  const doneToday = $derived(allRecent.filter((i) => stage(i) === 'done').length);
  const counts = $derived(progress.reduce((m, i) => ({ ...m, [stage(i)]: (m[stage(i)] ?? 0) + 1 }), {} as Record<string, number>));
  const finished = $derived((counts.done ?? 0) + (counts.proposed ?? 0) + (counts.failed ?? 0));
  let progOpen = $state(false);
  const STAGE: Record<string, string> = { queued: '排队中', working: 'AI 处理中', failed: '没成功', proposed: '等你确认', done: '已处理' };
  // 默认交给哪个模型（设置里改；没设过用这些站点用得最多的），按钮上写它的名字
  const pick = $derived(batchPick());
  const target = $derived(modelTarget(pick));
  // 换个模型处理这一次的菜单：锚在 ▾ 按钮下面，或右键的位置
  let menu = $state<{ at: { left: number; top: number; bottom: number; right: number }; items: OppItem[]; ask: boolean } | null>(null);
  function openMenu(e: MouseEvent, items: OppItem[], ask: boolean) {
    e.preventDefault();
    const r = e.type === 'contextmenu' ? { left: e.clientX, right: e.clientX + 260, top: e.clientY, bottom: e.clientY } : (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { at: { left: r.left, right: r.right, top: r.top, bottom: r.bottom }, items, ask };
  }
  function pickFromMenu(model: string, makeDefault: boolean) {
    const m = menu;
    menu = null;
    if (!m) return;
    if (makeDefault) setBatchModel(model);
    void workOpps(m.items, m.ask, model);
  }
  // 处理进度里每条用的模型：排着的看这批选的，处理中 / 处理过的看实际用的
  const usedModel = (i: OppItem) => {
    const q = o?.queued_model.find(([id]) => id === i.item.id)?.[1];
    if (q) return modelName(q);
    if (i.item.status === 'working') return modelName(`${i.brain}::${i.model}`);
    return i.item.model_used ? modelName(i.item.model_used) : '';
  };
  async function stopJob(i: OppItem) {
    if (await confirmDialog(`停止 ${i.domain || i.site_name} 正在跑的作业？\n正在改的这一条会标记为没成功（改了一半的不会上线），可以再交给 AI；这个站点后面没开始的几条留在列表里。`, { title: '停止作业', kind: 'warning', confirmText: '停止' })) void cancelSiteJob(i.site_id);
  }
  // AI 能直接处理的按类型数一下：「改标题和描述 14 · sitemap 报错 3」
  const readyKinds = $derived.by(() => {
    const m = new Map<string, number>();
    for (const i of ready) m.set(i.item.opp.kind, (m.get(i.item.opp.kind) ?? 0) + 1);
    return [...m.entries()].sort((a, b) => b[1] - a[1]);
  });
  let readyOpen = $state(false);
  let expanded = $state<string[]>([]);
  const SHOW = 8;
  // 这块里用更直白的说法（说清 AI 要做什么）
  const READY_LABEL: Record<string, string> = { low_ctr: '改标题和描述', stale_year: '更新标题年份', sitemap_error: 'sitemap 报错', crawl_issue: '抓取出错', img_audit: '图片问题', schema: '结构化数据' };
  const busy = (i: OppItem) => !!o?.running.includes(i.site_id);
</script>

{#snippet row(i: OppItem)}
  <li class="row">
    <button class="main" onclick={() => openSite(i.site_id, 'queue')} data-tip={`打开 ${i.domain || i.site_name} 的待处理\n\n${i.item.opp.evidence}`}>
      <span class="site"><SiteIcon id={i.site_id} size={13} /><span class="sn">{i.domain || i.site_name}</span></span>
      <span class="tt">{i.item.opp.title}<small class="kind">{KIND_LABEL[i.item.opp.kind] ?? ''}</small></span>
    </button>
    <span class="gain faint">{i.gain >= 1 ? `约 +${num(Math.round(i.gain))} 次点击/月` : ''}</span>
    <span class="acts">
      {#if busy(i)}<span class="pill" data-tip="这个站点正在跑别的作业，等它跑完再交给 AI"><span class="spin"></span>站点忙</span>
      {:else}
        <button class="ic-btn ic-sm" onclick={() => workOpps([i], false)} oncontextmenu={(e) => openMenu(e, [i], false)} aria-label={`交给 ${target.name}`} data-tip={`交给 ${target.name} 改：改好、检查通过后留给你确认\n右键换个模型`}><Icon name="wand" size={13} /></button>
        <button class="ic-btn ic-sm" onclick={() => dismissOpps([i])} aria-label="驳回" data-tip="驳回：之后不再提这个机会"><Icon name="x" size={13} /></button>
      {/if}
    </span>
  </li>
{/snippet}

{#snippet hand(items: OppItem[], verb: string, primary: boolean)}
  <span class="split" class:primary>
    <button class="btn tiny" class:primary onclick={() => workOpps(items)} data-tip={`${verb} ${target.name}：每个站点一个作业，同时最多 3 个；改好、检查通过后留给你确认`}>{@render who(target)}{verb} {target.name}</button><button class="btn tiny caret" class:primary onclick={(e) => openMenu(e, items, true)} aria-label="换个模型处理" data-tip="换个模型处理这一批"><svg width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
  </span>
{/snippet}

{#snippet who(t: { icon: string; name: string })}{#if t.icon}<span class="bi-w"><BrainIcon brain={t.icon} size={12} /></span>{:else}<Icon name="wand" size={11} />{/if}{/snippet}

{#snippet prow(i: OppItem)}
  {@const st = stage(i)}
  <li class="row">
    <button class="main" onclick={() => openSite(i.site_id, 'queue')} data-tip={`打开 ${i.domain || i.site_name} 的待处理${i.item.note ? `\n\n${i.item.note}` : ''}`}>
      <span class="site"><SiteIcon id={i.site_id} size={13} /><span class="sn">{i.domain || i.site_name}</span></span>
      <span class="tt">{i.item.opp.title}<small class="kind">{KIND_LABEL[i.item.opp.kind] ?? ''}</small></span>
    </button>
    <span class="st {st}" data-tip={i.item.note || null}>{#if st === 'working' || st === 'queued'}<span class="spin"></span>{/if}{STAGE[st]}{#if st === 'working' && i.step} · {i.step}{/if}{#if usedModel(i)}<span class="um"> · {usedModel(i)}</span>{/if}</span>
    <span class="acts">
      {#if st === 'working'}<button class="ic-btn ic-sm" onclick={() => stopJob(i)} aria-label="停止" data-tip="停止这个站点正在跑的作业"><Icon name="stop" size={13} /></button>
      {:else if st === 'failed'}<button class="ic-btn ic-sm" onclick={() => workOpps([i], false)} disabled={busy(i)} aria-label="再交给 AI" data-tip="再交给 AI 改一次"><Icon name="refresh" size={13} /></button>
      {:else if st === 'proposed'}<button class="ic-btn ic-sm" onclick={() => openSite(i.site_id, 'queue')} aria-label="去确认" data-tip="去看改动、确认或驳回"><Icon name="arrow" size={13} /></button>{/if}
    </span>
  </li>
{/snippet}

<div class="ob">
{#if !o}
  <div class="card"><p class="hint">正在读取…</p></div>
{:else}
  <div class="top">
  <p class="sum faint">
    {#if o.items.length}{sitesN} 个站点有 {o.items.length} 个机会没处理{:else}暂时没有新机会{/if}{#if o.managed_open} · 托管中站点的 {o.managed_open} 个由 AI 自动处理{/if}{#if o.waiting} · <span class="run">还有 {o.waiting} 个站点排着等 AI</span>{/if}{#if !active && doneToday} · <button class="okt link-btn" onclick={() => (fleet.tab = 'history')} data-tip="去「处理记录」看">今天 AI 处理完 {doneToday} 个</button>{/if}
  </p>
  </div>
  {#if progress.length}
    <div class="card tbl-card grp">
      <div class="gh">
        <Icon name="wand" size={13} /><b>{active ? 'AI 处理进度' : 'AI 没处理成功的'}</b><span class="faint n">{active ? `${finished}/${progress.length}` : progress.length}</span>
        <small class="faint d">{#each Object.entries(STAGE) as [k, l] (k)}{#if counts[k]}<span class="cnt {k}">{l} {counts[k]}</span>{/if}{/each}</small>
        <span class="grow"></span>
        {#if counts.queued}<button class="btn tiny ghost" onclick={stopAcross} data-tip="还没开始的都不做了（回到列表），正在跑的不受影响">停止排队</button>{/if}
      </div>
      {#if active}<div class="bar"><i style="width:{(finished / progress.length) * 100}%"></i></div>{/if}
      <ul class="rows">{#each progOpen ? progress : progress.slice(0, SHOW) as i (i.site_id + i.item.id)}{@render prow(i)}{/each}</ul>
      {#if progress.length > SHOW}<button class="more link-btn" onclick={() => (progOpen = !progOpen)}>{progOpen ? '收起' : `展开其余 ${progress.length - SHOW} 条`}</button>{/if}
    </div>
  {/if}
  {#if ready.length}
    <div class="card tbl-card grp">
      <div class="gh">
        <Icon name="wand" size={13} /><b>AI 能直接处理</b><span class="faint n">{ready.length}</span>
        <small class="faint d">改标题和描述、修 sitemap 和图片这类，不用你拿主意；sitemap 报错先由程序检查，多数不用改代码</small>
        <span class="grow"></span>
        {@render hand(ready, '全部交给', true)}
      </div>
      <div class="kinds">
        {#each readyKinds as [k, n] (k)}<span>{READY_LABEL[k] ?? KIND_LABEL[k] ?? k}<b>{n}</b></span>{/each}
        <span class="grow"></span>
        <button class="link-btn" onclick={() => (readyOpen = !readyOpen)}>{readyOpen ? '收起' : '看看是哪些'}</button>
      </div>
      {#if readyOpen}<ul class="rows sep">{#each ready as i (i.item.id)}{@render row(i)}{/each}</ul>{/if}
    </div>
    {#if groups.length}<h4 class="sec">要你拿主意的</h4>{/if}
  {/if}
  {#each groups as g (g.k)}
    {@const open = expanded.includes(g.k)}
    <div class="card tbl-card grp">
      <div class="gh">
        <span class="dot {g.k}"></span><b>{g.l}</b><span class="faint n">{g.items.length}</span>
        <small class="faint d">{g.d}</small>
        <span class="grow"></span>
        {@render hand(g.items, '这一组都交给', false)}
      </div>
      <ul class="rows">
        {#each open ? g.items : g.items.slice(0, SHOW) as i (i.item.id)}{@render row(i)}
        {/each}
      </ul>
      {#if g.items.length > SHOW}
        <button class="more link-btn" onclick={() => (expanded = open ? expanded.filter((k) => k !== g.k) : [...expanded, g.k])}>{open ? '收起' : `展开其余 ${g.items.length - SHOW} 条`}</button>
      {/if}
    </div>
  {/each}
{/if}
{#if menu}<ModelMenu at={menu.at} current={pick} onpick={pickFromMenu} onclose={() => (menu = null)} />{/if}
</div>

<style>
  /* 说明一行、各组卡片之间统一 12px（不再叠加外层间距） */
  .ob { display: flex; flex-direction: column; gap: 12px; }
  /* 卡片自己不留内边距：分隔线顶到卡片两边，标题和每行各自留 16px */
  .grp { padding: 0; overflow: hidden; }
  .sum { margin: 0 2px; font-size: 12.5px; }
  .sum .run { color: var(--su-run, var(--warn)); }
  .sum .okt { color: var(--ok); font-size: inherit; }
  .gh { display: flex; align-items: center; gap: 8px; padding: 11px 16px; border-bottom: 1px solid var(--border); font-size: 13px; }
  .gh b { font-weight: 600; }
  .gh .n { font-size: 12px; font-variant-numeric: tabular-nums; }
  .gh .d { min-width: 0; font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { flex: none; width: 7px; height: 7px; border-radius: 50%; }
  .dot.urgent { background: var(--err); }
  .dot.value { background: var(--ok); }
  .dot.easy { background: #b9a36a; }
  .grow { flex: 1; }
  .sec { margin: 4px 2px 0; font-size: 12.5px; font-weight: 600; color: var(--dim); }
  .kinds { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 16px; padding: 10px 16px; font-size: 12.5px; color: var(--dim); }
  .kinds b { margin-left: 5px; font-weight: 600; color: var(--text); font-variant-numeric: tabular-nums; }
  .kinds .link-btn { font-size: 12px; }
  .rows.sep { border-top: 1px solid var(--border); }
  /* 深色按钮上的模型图标垫一个小白底，单色图标（OpenAI、Grok）也看得清 */
  .bi-w { display: inline-flex; align-items: center; justify-content: center; width: 15px; height: 15px; border-radius: 4px; background: #fff; color: #26241f; }
  .top { display: flex; align-items: center; gap: 12px; min-height: 28px; }
  .top .sum { flex: 1; min-width: 0; }
  /* 拆分按钮：左边用默认模型交出去，右边 ▾ 换个模型 */
  .split { display: inline-flex; align-items: stretch; }
  .split .btn:first-child { border-top-right-radius: 0; border-bottom-right-radius: 0; }
  .split .caret { padding: 0 6px; margin-left: -1px; border-top-left-radius: 0; border-bottom-left-radius: 0; }
  .split.primary .caret { margin-left: 0; border-left: 1px solid rgba(255, 255, 255, 0.22); }
  .um { color: var(--faint); }
  .cnt { margin-right: 10px; }
  .cnt.working, .cnt.queued { color: var(--su-run, var(--warn)); }
  .cnt.proposed { color: var(--warn); }
  .cnt.done { color: var(--ok); }
  .cnt.failed { color: var(--err); }
  .bar { height: 2px; background: var(--border); }
  .bar i { display: block; height: 100%; background: var(--ok); transition: width 0.4s; }
  .st { flex: none; display: inline-flex; align-items: center; gap: 5px; font-size: 11.5px; color: var(--dim); white-space: nowrap; }
  .st.working, .st.queued { color: var(--su-run, var(--warn)); }
  .st.proposed { color: var(--warn); }
  .st.done { color: var(--ok); }
  .st.failed { color: var(--err); }
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
