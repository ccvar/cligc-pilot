<script lang="ts">
  // 托管控制台「需要你」页：左边「全部」+ 有事的站点（带条数），右边按「待你确认 / 失败了 / 上线后待核对」分组，
  // 每条一行，点开看原因并直接处理（看改动、上线、驳回、重试、回滚…，动作和任务队列里的是同一套）。
  import { escClose } from '$lib/escClose';
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import WhyChips from './WhyChips.svelte';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import { sites, openSite } from '$lib/state/sites.svelte.js';
  import { ops, acceptItem, dismissItem, workItem, recheckItem, ackItem, rollbackItem, showDiff, sourceOf } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';
  import type { PendingItem, OpsItem } from '$lib/types';

  type Stuck = { id: string; name: string; queued: number; why: string; fix: string };
  let { needs, stuck }: { needs: PendingItem[]; stuck: Stuck[] } = $props();

  const kindOf = (i: OpsItem) => (i.status === 'proposed' ? 'proposed' : i.status === 'failed' ? 'failed' : 'attention');
  const GROUPS = [
    { k: 'proposed', l: '待你确认', c: 'warn', tip: '判定不能自动上线的改动：新页面待审、改到推荐 / 邀请内容、超出每日上限等。看过改动后上线或驳回。' },
    { k: 'failed', l: '失败了', c: 'err', tip: 'AI 没改成或检查没通过。可以重新检查、让 AI 重新改，或驳回。' },
    { k: 'attention', l: '上线后待核对', c: 'warn', tip: '已经上线、网站正常，但线上验收有几项要你看一眼。没问题就点「没问题」，有问题可以回滚。' },
  ];
  const why = (i: OpsItem) => (i.status === 'deployed' ? i.attention : i.verdict && i.verdict !== 'auto' ? i.verdict : i.note) || '';

  const bySite = $derived.by(() => {
    const m = new Map<string, { id: string; name: string; n: number; bad: boolean }>();
    for (const x of needs) {
      const e = m.get(x.site_id) ?? { id: x.site_id, name: x.site_name, n: 0, bad: false };
      e.n++; if (x.item.status === 'failed') e.bad = true;
      m.set(x.site_id, e);
    }
    return [...m.values()];
  });
  let picked = $state('');
  const sel = $derived(bySite.some((s) => s.id === picked) ? picked : '');
  const shown = $derived(sel ? needs.filter((x) => x.site_id === sel) : needs);
  const groups = $derived(GROUPS.map((g) => ({ ...g, items: shown.filter((x) => kindOf(x.item) === g.k) })).filter((g) => g.items.length));

  let open = $state<Set<string>>(new Set());
  function toggle(id: string) { const n = new Set(open); if (n.has(id)) n.delete(id); else n.add(id); open = n; }
  const siteOf = (id: string) => sites.list.find((s) => s.id === id);
</script>

{#if stuck.length}
  <div class="stucks">
    {#each stuck as x (x.id)}
      <div class="stuck"><Icon name="pause" size={12} /><span><b>{x.name}</b> 有 {x.queued} 条任务排着等 AI 处理，{x.why}，所以一直没动。{x.fix}</span><button class="link-btn" onclick={() => openSite(x.id, 'queue', 'open')}>去看看</button></div>
    {/each}
  </div>
{/if}

<div class="card nvc">
  {#if !needs.length}
    <div class="empty"><Icon name="check" size={14} />没有需要你处理的事。<InfoTip text={'这里只放要你动手的：判定不能自动上线的改动、失败的任务，以及上线后线上验收有问题要你核对的。排着等 AI 处理的不在这里。'} label="需要你说明" /></div>
  {:else}
    <div class="nv">
      <nav class="slist" aria-label="站点"><div class="sl-in">
        <button class="site" class:on={!sel} onclick={() => (picked = '')}><Icon name="inbox" size={14} /><span class="nm">全部</span><span class="pg">{needs.length}</span></button>
        {#each bySite as s (s.id)}
          <button class="site" class:on={sel === s.id} onclick={() => (picked = s.id)}>
            <SiteIcon id={s.id} size={15} /><span class="nm">{s.name}</span>{#if s.bad}<i class="dot" aria-label="有失败"></i>{/if}<span class="pg">{s.n}</span>
          </button>
        {/each}
      </div></nav>

      <section class="detail">
        <header class="hd"><b class="title">{sel ? bySite.find((s) => s.id === sel)?.name : '全部'}</b><span class="sub">{shown.length} 件事要你处理</span><InfoTip text={'这里只放要你动手的：判定不能自动上线的改动、失败的任务，以及上线后线上验收有问题要你核对的。排着等 AI 处理的不在这里。'} label="需要你说明" />
          {#if sel}<span class="grow"></span><button class="btn tiny" onclick={() => openSite(sel, 'queue')}><Icon name="inbox" size={11} />打开任务队列</button>{/if}</header>

        {#each groups as g (g.k)}
          <div class="grp">
            <h4><span class="gdot {g.c}"></span>{g.l}<span class="cnt">{g.items.length}</span><InfoTip text={g.tip} label="{g.l}说明" /></h4>
            <ol class="items">
              {#each g.items as x (x.item.id)}
                {@const i = x.item}
                {@const site = siteOf(x.site_id)}
                {@const isOpen = open.has(i.id)}
                {@const w = why(i)}
                <li class:open={isOpen}>
                  <button class="row" onclick={() => toggle(i.id)} aria-expanded={isOpen}>
                    {#if !sel}<span class="sn"><SiteIcon id={x.site_id} size={13} />{x.site_name}</span>{/if}
                    <span class="tt">{i.opp.title}</span>
                    <svg class="chev" class:up={isOpen} width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  </button>
                  {#if isOpen}
                    <div class="more">
                      {#if w}<p class="why {g.k === 'failed' ? 'err' : ''}">{g.k === 'proposed' ? '没自动上线：' : g.k === 'failed' ? '失败原因：' : '要你核对：'}{w}</p>{/if}
                      <WhyChips change={i.opp.change} source={sourceOf(i.opp)} kind={i.opp.kind} target={i.opp.target} />
                      {#if i.opp.page}<span class="mono url">{i.opp.page}</span>{/if}
                      <div class="acts">
                        {#if site}
                          {#if i.status === 'proposed'}
                            {#if i.head_sha}<button class="btn tiny" onclick={() => showDiff(site.id, i)}><Icon name="diff" size={11} />看改动</button>{/if}
                            <button class="btn tiny primary" onclick={() => acceptItem(site, i)}><Icon name={i.branch ? 'rocket' : 'check'} size={11} />{i.branch ? '上线' : '标记已处理'}</button>
                          {:else if i.status === 'failed'}
                            {#if i.branch && i.head_sha}<button class="btn tiny primary" onclick={() => recheckItem(site.id, i)} data-tip="改动还在分支上：重新构建、检查，不再让 AI 改一遍"><Icon name="refresh" size={11} />重新检查</button>{/if}
                            <button class="btn tiny" onclick={() => workItem(site.id, i)}><Icon name="sparkles" size={11} />让 AI 重新改</button>
                            {#if i.head_sha}<button class="btn tiny" onclick={() => showDiff(site.id, i)}><Icon name="diff" size={11} />看改动</button>{/if}
                          {:else}
                            <button class="btn tiny primary" onclick={() => ackItem(site.id, i)}><Icon name="check" size={11} />没问题</button>
                            {#if i.merge_sha}<button class="btn tiny danger" onclick={() => rollbackItem(site, i)}><Icon name="undo" size={11} />回滚</button>{/if}
                          {/if}
                          {#if i.conv_id}<button class="btn tiny" onclick={() => openConv(i.conv_id)}><Icon name="chat" size={11} />打开会话</button>{/if}
                          <span class="grow"></span>
                          {#if i.status !== 'deployed'}<button class="btn tiny quiet" onclick={() => dismissItem(site, i)}><Icon name="x" size={11} />驳回</button>{/if}
                        {/if}
                        <button class="link-btn" onclick={() => openSite(x.site_id, 'queue')}>在任务队列里看</button>
                      </div>
                    </div>
                  {/if}
                </li>
              {/each}
            </ol>
          </div>
        {/each}
      </section>
    </div>
  {/if}
</div>

{#if ops.diff}
  <div class="su-mask" role="presentation" onclick={() => (ops.diff = null)}></div>
  <div class="su modal diff-modal" use:escClose={() => (ops.diff = null)} role="dialog" aria-modal="true" aria-label="改动差异">
    <header class="sheet-head"><b>{ops.diff.item.opp.title}</b><button class="x" aria-label="关闭" data-tip="关闭" onclick={() => (ops.diff = null)}>×</button></header>
    <div class="sheet-body"><pre class="diff">{#each ops.diff.text.split('\n') as l, n (n)}<span class={l.startsWith('+') && !l.startsWith('+++') ? 'add' : l.startsWith('-') && !l.startsWith('---') ? 'del' : l.startsWith('@@') ? 'hunk' : ''}>{l}{'\n'}</span>{/each}</pre></div>
  </div>
{/if}

<style>
  .stucks { display: flex; flex-direction: column; gap: 6px; margin-bottom: 12px; }
  .stuck { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-radius: 9px; background: var(--su-warn-soft); color: var(--su-warn-ink); font-size: 12.5px; line-height: 1.55; }
  .stuck > span { flex: 1; min-width: 0; }
  .stuck .link-btn { flex: none; color: inherit; }

  .nvc { padding: 0; overflow: clip; container-type: inline-size; }
  .empty { display: flex; align-items: center; gap: 8px; padding: 16px 18px; font-size: 13px; color: var(--dim); }
  .nv { display: grid; grid-template-columns: 220px minmax(0, 1fr); }
  /* 分隔线画在整列上（跟右边一样高）；列表本身滚动时贴在顶上 */
  .slist { border-right: 1px solid var(--border); min-width: 0; }
  .sl-in { position: sticky; top: 0; display: flex; flex-direction: column; gap: 2px; padding: 10px 8px; max-height: 80vh; overflow-y: auto; overflow-x: hidden; }
  .site { display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 8px; border: 0; border-radius: 7px; background: none; font: inherit; font-size: 12.5px; color: var(--dim); cursor: pointer; text-align: left; min-width: 0; }
  .site:hover { background: var(--su-hover); color: var(--text); }
  .site.on { background: var(--accent-soft); color: var(--text); font-weight: 550; }
  .site .nm { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .site .pg { flex: none; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; font-weight: 400; }
  .dot { flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--err); }

  .detail { min-width: 0; padding: 16px 18px; display: flex; flex-direction: column; gap: 14px; }
  .hd { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .hd .title { font-size: 14px; font-weight: 600; }
  .hd .sub { font-size: 12px; color: var(--faint); }
  .grow { flex: 1; }

  .grp h4 { margin: 0 0 6px; display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: var(--text); }
  .gdot { width: 7px; height: 7px; border-radius: 50%; }
  .gdot.warn { background: var(--warn); }
  .gdot.err { background: var(--err); }
  .cnt { font-weight: 400; color: var(--faint); font-variant-numeric: tabular-nums; }
  .items { list-style: none; margin: 0; padding: 0; border: 1px solid var(--border); border-radius: 10px; }
  .items > li + li { border-top: 1px solid var(--border); }
  .row { width: 100%; display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 6px 12px; border: 0; background: none; font: inherit; color: inherit; text-align: left; cursor: pointer; }
  .row:hover { background: var(--su-hover); }
  .items > li:first-child .row { border-radius: 10px 10px 0 0; }
  .items > li:last-child:not(.open) .row { border-radius: 0 0 10px 10px; }
  .sn { flex: none; display: inline-flex; align-items: center; gap: 5px; max-width: 160px; font-size: 12px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tt { flex: 1; min-width: 0; font-size: 13px; font-weight: 550; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  li.open .tt { white-space: normal; }
  .chev { flex: none; color: var(--faint); transition: transform .16s; }
  .chev.up { transform: rotate(180deg); }
  .row:hover .chev { color: var(--dim); }
  .more { padding: 0 12px 12px; display: flex; flex-direction: column; gap: 8px; }
  .why { margin: 0; font-size: 12.5px; color: var(--dim); line-height: 1.65; }
  .why.err { color: var(--err); }
  .url { font-size: 11px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .acts { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .acts .link-btn { font-size: 11.5px; margin-left: 4px; }

  .diff-modal { width: min(900px, calc(100vw - 32px)); }
  pre.diff { margin: 0; font-size: 11.5px; line-height: 1.5; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
  pre.diff .add { background: #e8f5ec; color: #135c34; }
  pre.diff .del { background: #fdecec; color: #8f1d1d; }
  pre.diff .hunk { color: #2f5fb8; }

  @container (max-width: 640px) {
    .nv { grid-template-columns: minmax(0, 1fr); }
    .slist { border-right: 0; border-bottom: 1px solid var(--border); }
    .sl-in { position: static; flex-direction: row; flex-wrap: wrap; max-height: none; }
    .site { flex: none; }
    .site .nm { flex: none; }
  }
</style>
