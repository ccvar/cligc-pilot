<script lang="ts">
  import './site.css';
  import { untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import type { Site, OpsItem } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import { clock } from './fmt';
  import { managed, AUTONOMY_LABEL, budgetFull, editGroup, openManaged } from '$lib/state/managed.svelte.js';
  import WhyChips from './WhyChips.svelte';
  import { sites } from '$lib/state/sites.svelte.js';
  import { ops, loadQueue, runTask, workItem, recheckItem, reshipItem, acceptItem, workItems, dismissItems, ackItem, dismissItem, restoreItem, markDone, rollbackItem, showDiff, sourceOf, KIND_LABEL, CHANGE_LABEL, STATUS_LABEL } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';

  let { site }: { site: Site } = $props();
  $effect(() => { void loadQueue(site.id); });
  const all = $derived(ops.queue[site.id] ?? []);
  let filter = $state<'todo' | 'open' | 'live' | 'closed'>('todo');
  const groups = {
    todo: ['proposed', 'working', 'failed'],
    open: ['open'],
    live: ['merged', 'deployed', 'done'],
    closed: ['dismissed', 'rolled_back'],
  } as const;
  // 上线了但验收有待核对问题的，也放进「需要你处理」
  const inGroup = (i: OpsItem, k: keyof typeof groups) => (groups[k] as readonly string[]).includes(i.status) || (k === 'todo' && i.status === 'deployed' && !!i.attention);
  const shown = $derived(all.filter((i) => inGroup(i, filter)));
  const counts = $derived(Object.fromEntries((Object.keys(groups) as (keyof typeof groups)[]).map((k) => [k, all.filter((i) => inGroup(i, k)).length])) as Record<string, number>);
  // 进来时选哪一栏：别处指定了就用它；否则「需要你处理」是空的、有未处理的机会，就直接看机会
  let picked0 = '';
  let explicit = false;
  // 从别处跳过来指定了栏目（页面已经开着也生效），之后不再自动改
  $effect(() => {
    const want = sites.queueFilter;
    if (want) untrack(() => { filter = want; explicit = true; sites.queueFilter = ''; });
  });
  $effect(() => {
    const id = site.id;
    if (picked0 === id || !ops.queue[id]) return;
    picked0 = id;
    untrack(() => {
      if (explicit) { explicit = false; return; }
      filter = !counts.todo && counts.open ? 'open' : 'todo';
    });
  });
  let openChecks = $state<Set<string>>(new Set());
  function toggleChecks(id: string) { const n = new Set(openChecks); if (n.has(id)) n.delete(id); else n.add(id); openChecks = n; }
  const job = $derived(sites.jobs[site.id]);
  const busy = $derived(job?.status === 'running');
  const when = clock;
  const tone = (s: string) => (s === 'proposed' ? 'warn' : s === 'deployed' || s === 'done' ? 'ok' : s === 'failed' ? 'err' : s === 'working' ? 'run' : '');
  const myPlan = $derived(managed.overview?.groups.find((g) => g.sites.includes(site.id)));
  const full = $derived(myPlan ? budgetFull(managed.overview?.sites.find((x) => x.id === site.id)?.budget) : '');
  const canWork = (i: OpsItem) => i.status === 'open' || i.status === 'failed';
  let openWhy = $state<Set<string>>(new Set());
  // 批量：只在「未处理的机会」里勾选
  let picked = $state<Set<string>>(new Set());
  const pickable = $derived(filter === 'open' ? shown : []);
  const pickedItems = $derived(pickable.filter((i) => picked.has(i.id)));
  const allPicked = $derived(pickable.length > 0 && pickedItems.length === pickable.length);
  $effect(() => { void filter; picked = new Set(); });
  function togglePick(id: string) { const n = new Set(picked); if (n.has(id)) n.delete(id); else n.add(id); picked = n; }
  function pickAll() { picked = allPicked ? new Set() : new Set(pickable.map((i) => i.id)); }
  async function batchWork() {
    const ids = pickedItems.map((i) => i.id);
    picked = new Set();
    await workItems(site.id, ids);
  }
  async function batchDismiss() {
    const items = pickedItems;
    await dismissItems(site, items);
    picked = new Set();
  }
  function toggleWhy(id: string) { const n = new Set(openWhy); if (n.has(id)) n.delete(id); else n.add(id); openWhy = n; }
</script>

<div class="qq">
  <div class="qbar">
    <div class="seg">
      <button class:on={filter === 'todo'} onclick={() => (filter = 'todo')}>需要你处理{#if counts.todo}<span class="n">{counts.todo}</span>{/if}</button>
      <button class:on={filter === 'open'} onclick={() => (filter = 'open')}>未处理的机会{#if counts.open}<span class="n">{counts.open}</span>{/if}</button>
      <button class:on={filter === 'live'} onclick={() => (filter = 'live')}>已上线{#if counts.live}<span class="n">{counts.live}</span>{/if}</button>
      <button class:on={filter === 'closed'} onclick={() => (filter = 'closed')}>已驳回 / 回滚{#if counts.closed}<span class="n">{counts.closed}</span>{/if}</button>
    </div>
    <span class="grow"></span>
    <button class="link-btn lvl" onclick={() => (sites.tab = 'overview')}>{myPlan ? `托管：${myPlan.name} · ${AUTONOMY_LABEL[myPlan.autonomy] ?? ''}` : '自己运营'}</button>
    <button class="btn small" onclick={() => runTask(site.id, 'scan')} disabled={busy}>{#if busy && job?.kind === 'ops:scan'}<span class="spin"></span>扫描中{:else}<Icon name="scan" size={12} />立即扫描{/if}</button>
  </div>

  {#if job && job.kind.startsWith('ops:') && (job.status === 'running' || job.finished_at > Date.now() / 1000 - 300)}<div class="card"><SiteJobPanel {job} compact /></div>{/if}

  {#if full && counts.open && (filter === 'open' || filter === 'todo')}
    <div class="qnote warn"><Icon name="gauge" size={13} /><span>{full}，托管暂停处理这个站点的任务，下面的会一直排着。调高方案里的「每站每周 token」，或等用量滚出最近 7 天后，AI 会接着做；也可以点「让 AI 改」单独做某一条。</span>{#if myPlan}<button class="link-btn" onclick={() => { openManaged('groups'); editGroup(myPlan); }}>调整方案</button>{/if}</div>
  {/if}

  {#if !shown.length}
    <div class="card empty">
      <p>{filter === 'todo' ? '没有需要你确认的事项' : filter === 'open' ? '队列里没有未处理的机会' : '暂无记录'}</p>
      <p class="hint">{filter === 'open' ? '可以「立即扫描」，或在「数据」页把机会加进来。' : filter === 'todo' ? 'AI 改好的提案、失败的任务会出现在这里。' : ''}</p>
    </div>
  {:else}
    <div class="card items">
      {#if filter === 'open'}
        <div class="list-head" class:on={pickedItems.length > 0}>
          {#if shown.length > 1}<label class="ck"><input type="checkbox" checked={allPicked} indeterminate={pickedItems.length > 0 && !allPicked} onchange={pickAll} />{pickedItems.length ? `已选 ${pickedItems.length} 条` : '全选'}</label>{/if}
          {#if pickedItems.length}
            <span class="grow"></span>
            <button class="btn tiny primary" onclick={batchWork} disabled={busy} data-tip="一条一条按顺序处理：改、检查，再按方案规则上线或留给你确认；本周额度用完会停下"><Icon name="sparkles" size={11} />让 AI 改 {pickedItems.length} 条</button>
            <button class="btn tiny quiet" onclick={batchDismiss} disabled={busy}><Icon name="x" size={11} />驳回 {pickedItems.length} 条</button>
          {:else}
            <span class="faint lh-hint">{myPlan && myPlan.autonomy !== 'suggest' ? `托管每轮按优先级处理排在前面的几条，其余的排着等下一轮；想现在做，勾选后点「让 AI 改」或用单条的按钮。` : '勾选几条，可以一起让 AI 改或驳回。'}</span>
          {/if}
        </div>
      {/if}
      <div class="list">
        {#each shown as i (i.id)}
          <div class="li item" class:picked={picked.has(i.id)}>
            <div class="row it-head">
              {#if filter === 'open' && shown.length > 1}<input class="pick" type="checkbox" checked={picked.has(i.id)} onchange={() => togglePick(i.id)} aria-label="选中这一条" />{/if}
              {#if i.status === 'deployed' && i.attention}<span class="pill warn">已上线 · 待核对</span>{:else}<span class="pill {tone(i.status)}">{STATUS_LABEL[i.status] ?? i.status}</span>{/if}
              <b class="it-title" title={i.opp.title}>{i.opp.title}</b>
              <span class="it-meta">{KIND_LABEL[i.opp.kind] ?? i.opp.kind} · {when(i.updated_at)}</span>
            </div>
            <div class="why-row"><WhyChips change={i.opp.change} source={sourceOf(i.opp)} kind={i.opp.kind} target={i.opp.target} /></div>
            <p class="it-ev">{i.opp.evidence}</p>
            {#if i.ai_summary || i.files?.length}
              <button class="how-t" class:open={openWhy.has(i.id)} onclick={() => toggleWhy(i.id)} aria-expanded={openWhy.has(i.id)}>
                <svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                AI 怎么改的{#if i.files?.length}<span class="faint"> · {i.files.length} 个文件</span>{/if}{#if i.model_used}<span class="faint"> · {i.model_used}</span>{/if}
              </button>
              {#if openWhy.has(i.id)}
                <div class="how">
                  {#if i.ai_summary}<p>{i.ai_summary}</p>{/if}
                  {#if i.files?.length}<ul class="files">{#each i.files as f (f)}<li class="mono">{f}</li>{/each}</ul>{/if}
                </div>
              {/if}
            {/if}
            {#if i.branch || i.change_kind}
              <div class="row wrap meta">
                {#if i.change_kind}<span class="pill line" data-tip="Pilot 按实际文件差异判定的改动类型">实际改动：{i.change_kind.split(',').map((k) => CHANGE_LABEL[k] ?? k).join(' + ')}</span>{/if}
                {#if i.auto}<span class="pill ok">自动上线</span>{/if}
                {#if i.branch}<span class="mono faint br">{i.branch}</span>{/if}
                {#if i.pr_url}<button class="link-btn" onclick={() => openUrl(i.pr_url)}>PR #{i.pr_number} ↗</button>{/if}
              </div>
            {/if}
            {#if i.verdict && i.status === 'proposed'}
              <p class="verdict" class:ok={i.verdict === 'auto'}>{i.verdict === 'auto' ? '整份改动都在白名单内，检查已通过。' : `不能自动上线：${i.verdict}`}</p>
            {/if}
            {#if i.note}<p class="verdict">{i.note}</p>{/if}
            {#if i.status === 'deployed' && i.attention}<p class="verdict">上线了，网站正常；线上验收有几项要你核对：{i.attention}</p>{/if}
            {#if i.checks?.length}
              {@const bad = i.checks.filter((c) => !c.ok).length}
              <button class="how-t" class:open={openChecks.has(i.id)} onclick={() => toggleChecks(i.id)} aria-expanded={openChecks.has(i.id)}>
                <svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                线上验收<span class="faint"> · {i.checks.length - bad} 项通过</span>{#if bad}<span class="warn-text"> · {bad} 项没过</span>{/if}{#if (i.watch_stage ?? 0) === 1}<span class="faint"> · 24 小时后再复查一次</span>{/if}
              </button>
              {#if openChecks.has(i.id)}
                <ul class="checks">
                  {#each i.checks as c, k (k)}<li class:bad={!c.ok}><span class="ck-i">{c.ok ? '✓' : '✗'}</span>{c.label}{#if c.detail}<span class="faint">：{c.detail}</span>{/if}</li>{/each}
                </ul>
              {/if}
            {/if}
            {#if i.review}<p class="hint">效果复盘：<b class={i.review === 'positive' ? 'up' : i.review === 'negative' ? 'down' : ''}>{i.review === 'positive' ? '正面' : i.review === 'negative' ? '负面' : '持平'}</b> · {i.review_note}</p>{/if}
            <div class="row wrap acts">
              {#if i.status === 'proposed'}<button class="btn tiny primary" onclick={() => acceptItem(site, i)} disabled={busy}><Icon name={i.branch ? 'merge' : 'check'} size={11} />{i.branch ? (site.source === 'github' ? '合并并上线' : '合并到主分支并上线') : '标记已处理'}</button>{/if}
              {#if (i.status === 'failed' || (i.status === 'proposed' && i.verdict !== 'auto')) && i.branch && i.head_sha}<button class="btn tiny" class:primary={i.status === 'failed'} onclick={() => recheckItem(site.id, i)} disabled={busy} data-tip="AI 的改动还在分支上：重新构建、检查，通过后按方案规则上线或留给你确认，不再让 AI 改一遍"><Icon name="refresh" size={11} />重新检查</button>{/if}
              {#if canWork(i)}<button class="btn tiny" onclick={() => workItem(site.id, i)} disabled={busy}><Icon name="sparkles" size={11} />{i.status === 'failed' && i.head_sha ? '让 AI 重新改' : '让 AI 改'}</button>{/if}
              {#if i.head_sha}<button class="btn tiny" onclick={() => showDiff(site.id, i)}><Icon name="diff" size={11} />查看差异</button>{/if}
              {#if i.conv_id}<button class="btn tiny" onclick={() => openConv(i.conv_id)}><Icon name="chat" size={11} />打开会话</button>{/if}
              {#if i.status === 'failed' && i.conv_id}<button class="btn tiny" onclick={() => markDone(site.id, i)}><Icon name="check" size={11} />我已手动处理</button>{/if}
              {#if ['dismissed', 'failed'].includes(i.status)}<button class="btn tiny" onclick={() => restoreItem(site.id, i)}><Icon name="inbox" size={11} />放回待处理</button>{/if}
              {#if i.status === 'deployed' && i.attention}<button class="btn tiny" onclick={() => ackItem(site.id, i)}><Icon name="check" size={11} />已核对</button>{/if}
              {#if i.status === 'merged' && !i.deployed_at && site.deploy.mode !== 'none'}<button class="btn tiny primary" onclick={() => reshipItem(site, i)} disabled={busy} data-tip="已经合并，上次上线没完成：重新构建并上线"><Icon name="rocket" size={11} />重新上线</button>{/if}
              {#if ['merged', 'deployed'].includes(i.status) && i.merge_sha}<button class="btn tiny danger" onclick={() => rollbackItem(site, i)} disabled={busy}><Icon name="undo" size={11} />回滚</button>{/if}
              {#if ['open', 'proposed', 'failed'].includes(i.status)}<span class="grow"></span><button class="btn tiny quiet" onclick={() => dismissItem(site, i)} disabled={busy}><Icon name="x" size={11} />驳回</button>{/if}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
  <p class="hint foot">驳回过的机会不会再提；每条改动在默认分支上是独立提交，回滚即回退那一次提交并重新上线。</p>
</div>

{#if ops.diff}
  <div class="su-mask" role="presentation" onclick={() => (ops.diff = null)}></div>
  <div class="su modal diff-modal" role="dialog" aria-modal="true" aria-label="改动差异">
    <header class="sheet-head"><b>{ops.diff.item.opp.title}</b><button class="x" onclick={() => (ops.diff = null)}>×</button></header>
    <div class="sheet-body"><pre class="diff">{#each ops.diff.text.split('\n') as l, n (n)}<span class={l.startsWith('+') && !l.startsWith('+++') ? 'add' : l.startsWith('-') && !l.startsWith('---') ? 'del' : l.startsWith('@@') ? 'hunk' : ''}>{l}{'\n'}</span>{/each}</pre></div>
  </div>
{/if}

<style>
  .checks { list-style: none; margin: 2px 0 4px; padding: 0 0 0 16px; display: flex; flex-direction: column; gap: 3px; font-size: 12.5px; }
  .checks li { line-height: 1.55; }
  .checks .ck-i { display: inline-block; width: 16px; color: var(--ok); }
  .checks li.bad .ck-i { color: var(--warn); }
  .list-head { display: flex; align-items: center; gap: 12px; margin: 0 -18px; padding: 9px 18px; min-height: 42px; border-bottom: 1px solid var(--border); border-radius: 12px 12px 0 0; font-size: 12.5px; }
  .list-head.on { background: var(--su-run-soft); }
  .list-head .ck { display: inline-flex; align-items: center; gap: 8px; cursor: pointer; flex: none; font-weight: 500; }
  .list-head .ck input { margin: 0 2px 0 0; }
  .list-head .grow { flex: 1; }
  .lh-hint { font-size: 12px; line-height: 1.5; min-width: 0; }
  .pick { margin: 0 2px 0 0; flex: none; cursor: pointer; }
  .item.picked { background: var(--su-hover); }
  .qnote { display: flex; align-items: flex-start; gap: 8px; font-size: 12.5px; line-height: 1.6; color: var(--dim); padding: 9px 14px; border: 1px solid var(--border); border-radius: 10px; margin-bottom: 12px; }
  .qnote :global(svg) { flex: none; margin-top: 4px; }
  .qnote span { flex: 1; }
  .qnote.warn { color: var(--su-warn-ink); background: var(--su-warn-soft); border-color: transparent; }
  .qnote .link-btn { flex: none; font-size: 12.5px; }
  .qq { display: flex; flex-direction: column; gap: 12px; }
  .qbar { display: flex; align-items: center; gap: 8px 12px; flex-wrap: wrap; }
  .qbar .grow, .acts .grow { flex: 1; }
  .seg .n { margin-left: 6px; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .seg button.on .n { color: var(--dim); }
  .lvl { font-size: 12px; color: var(--dim); text-decoration: none; }
  .lvl:hover { color: var(--text); }
  .empty { text-align: center; padding: 28px 18px; }
  .empty p:first-child { margin: 0 0 2px; font-size: 13px; font-weight: 550; }
  .items { padding-top: 0; padding-bottom: 0; }
  .items .list { margin-bottom: 0; }
  .items .list > .li:first-child { border-top: none; }
  .item { display: flex; flex-direction: column; gap: 6px; padding-top: 14px; padding-bottom: 14px; }
  .it-head { min-width: 0; gap: 10px; }
  .it-head .pill { flex: none; }
  .it-title { flex: 1; min-width: 0; font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .it-meta { flex: none; font-size: 11.5px; color: var(--faint); white-space: nowrap; }
  .why-row { margin: -1px 0 0; }
  .how-t { align-self: flex-start; display: inline-flex; align-items: center; gap: 5px; border: none; background: none; padding: 0; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; }
  .how-t:hover { color: var(--text); }
  .how-t svg { transition: transform .16s var(--su-ease); }
  .how-t.open svg { transform: rotate(90deg); }
  .how { padding: 8px 12px; border-left: 2px solid var(--border2); margin-left: 4px; font-size: 12.5px; color: var(--dim); }
  .how p { margin: 0 0 6px; line-height: 1.65; white-space: pre-wrap; }
  .files { margin: 0; padding: 0; list-style: none; display: flex; flex-wrap: wrap; gap: 4px 12px; font-size: 11.5px; color: var(--faint); }
  .it-ev { margin: 0; font-size: 12.5px; color: var(--dim); line-height: 1.65; max-width: 80ch; }
  .meta { gap: 8px; }
  .br { font-size: 11.5px; }
  .verdict { margin: 0; font-size: 12px; line-height: 1.55; color: var(--su-warn-ink); background: var(--su-warn-soft); border-radius: 7px; padding: 5px 9px; align-self: flex-start; max-width: 100%; overflow-wrap: anywhere; }
  .verdict.ok { color: var(--ok); background: var(--su-ok-soft); }
  .acts { gap: 6px; margin-top: 4px; }
  .foot { font-size: 11.5px; color: var(--faint); }
  .diff-modal { width: min(900px, calc(100vw - 32px)); }
  pre.diff { margin: 0; font-size: 11.5px; line-height: 1.5; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
  pre.diff .add { background: #e8f5ec; color: #135c34; }
  pre.diff .del { background: #fdecec; color: #8f1d1d; }
  pre.diff .hunk { color: #2f5fb8; }
  @media (max-width: 640px) { .it-meta { display: none; } }
</style>
