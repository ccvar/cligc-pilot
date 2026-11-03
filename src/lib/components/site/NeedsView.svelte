<script lang="ts">
  // 托管控制台「需要你」页：左边「全部」+ 有事的站点（带条数），右边按「待你确认 / 失败了 / 上线后待核对」分组，
  // 每条一行，点开看原因并直接处理（看改动、上线、驳回、重试、回滚…，动作和任务队列里的是同一套）。
  import { untrack } from 'svelte';
  import DiffModal from './DiffModal.svelte';
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import WhyChips from './WhyChips.svelte';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import ModelMenu from './ModelMenu.svelte';
  import OpsErrorHint from './OpsErrorHint.svelte';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import { modelTarget } from '$lib/state/fleet.svelte.js';
  import { sites, openSite } from '$lib/state/sites.svelte.js';
  import { acceptItem, acceptItems, reshipItem, blockedCode, setContentCode, dismissItem, workItem, redoItem, recheckItem, ackItem, rollbackItem, showDiff, sourceOf } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';
  import { invoke } from '@tauri-apps/api/core';
  import { confirmDialog, say } from '$lib/state/app.svelte.js';
  import { loadManaged, managed, setBlockKind, BLOCK_KIND_LABEL } from '$lib/state/managed.svelte.js';
  import type { PendingItem, OpsItem } from '$lib/types';

  type Stuck = { id: string; name: string; queued: number; why: string; fix: string; kind: string; group: string; hasPlan: boolean };
  let { needs, stuck, onturnon, onbudget }: { needs: PendingItem[]; stuck: Stuck[]; onturnon: (id: string) => void; onbudget: (groupId: string) => void } = $props();

  /** 排着没动的任务：直接清掉这个站点计划里还没开始的那些 */
  async function clearStuck(x: Stuck) {
    if (!(await confirmDialog(`清除 ${x.name} 的计划？还没开始的任务会从任务队列里拿掉，已经在处理或上线的保留。\n\n站点的托管打开后，下次自动运行会重新制定一份。`, { title: '清除计划', kind: 'danger', confirmText: '清除计划' }))) return;
    try { const r = await invoke<{ removed: number; kept: number }>('plan_clear', { id: x.id }); say(`已清除计划，拿掉 ${r.removed} 条任务`); await loadManaged(); }
    catch (e) { say(String(e), 'err'); }
  }

  // 排着没动的那一块可以收起（多半是你自己停着的站点，天天看着占地方）：默认收起，记在本机
  const STUCK_KEY = 'cligc.pilot.stuckOpen';
  let stuckOpen = $state((() => { try { return localStorage.getItem(STUCK_KEY) === '1'; } catch { return false; } })());
  function toggleStuck() { stuckOpen = !stuckOpen; try { localStorage.setItem(STUCK_KEY, stuckOpen ? '1' : '0'); } catch { /* */ } }
  const stuckTotal = $derived(stuck.reduce((n, x) => n + x.queued, 0));
  /** 收起时的一句话：按原因数站点，比如「12 个托管暂停着 · 1 个本周额度用完」 */
  const stuckWhy = $derived.by(() => {
    const m = new Map<string, number>();
    for (const x of stuck) m.set(x.why, (m.get(x.why) ?? 0) + 1);
    return [...m].sort((a, b) => b[1] - a[1]).map(([w, n]) => `${n} 个${w.replace(/^这个站点的?/, '')}`).join(' · ');
  });
  // 站点作业正在处理这一条（点了上线、重新检查，或调度自动接着跑）：按钮换成「处理中」，免得以为没反应再点一次
  const BUSY_LABEL: Record<string, string> = { 'ops:accept': '上线中', 'ops:accept_many': '合并上线中', 'ops:recheck': '重新检查中', 'ops:work': 'AI 改动中', 'ops:batch': 'AI 改动中', 'ops:rollback': '回滚中', 'ops:resolve': '处理中' };
  function busyOf(siteId: string, itemId: string) {
    const j = sites.jobs[siteId];
    if (j?.status !== 'running' || !j.items?.includes(itemId)) return null;
    const step = j.steps.find((s) => s.status === 'running');
    return { label: BUSY_LABEL[j.kind] ?? '处理中', step: step?.label ?? '' };
  }
  // 合并了、上线失败的（发布命令不对之类）也算失败：改动在主分支里却没上线
  const kindOf = (i: OpsItem) => (i.status === 'proposed' ? 'proposed' : i.status === 'failed' || i.status === 'merged' ? 'failed' : 'attention');
  const GROUPS = [
    { k: 'proposed', l: '待你确认', c: 'warn', tip: '判定不能自动上线的改动：新页面待审、改到推荐 / 邀请内容、超出每日上限等。看过改动后上线或驳回。' },
    { k: 'failed', l: '失败了', c: 'err', tip: 'AI 没改成或检查没通过。可以重新检查、让 AI 重新改，或驳回。' },
    { k: 'attention', l: '上线后待核对', c: 'warn', tip: '已经上线、网站正常，但线上验收有几项要你看一眼。没问题就点「没问题」，有问题可以回滚。' },
  ];
  // 失败的显示失败原因（note）；verdict 是上线前判定留下的，失败后还在，不能拿来当原因
  const why = (i: OpsItem) => (i.status === 'deployed' ? i.attention : i.status === 'failed' || i.status === 'merged' ? i.note : i.verdict && i.verdict !== 'auto' ? i.verdict : i.note) || '';

  const bySite = $derived.by(() => {
    const m = new Map<string, { id: string; name: string; n: number; bad: boolean }>();
    for (const x of needs) {
      const e = m.get(x.site_id) ?? { id: x.site_id, name: x.site_name, n: 0, bad: false };
      e.n++; if (x.item.status === 'failed' || x.item.status === 'merged') e.bad = true;
      m.set(x.site_id, e);
    }
    return [...m.values()];
  });
  let picked = $state('');
  const sel = $derived(bySite.some((s) => s.id === picked) ? picked : '');
  const shown = $derived(sel ? needs.filter((x) => x.site_id === sel) : needs);
  const groups = $derived(GROUPS.map((g) => ({ ...g, items: shown.filter((x) => kindOf(x.item) === g.k) })).filter((g) => g.items.length));

  // 待你确认的条目里合不进主分支的（和站点任务队列同一个判断：本地来源的站点在内存里试合并，不动文件夹）
  let conflicts = $state<Set<string>>(new Set());
  $effect(() => {
    const ps = needs.filter((x) => x.item.status === 'proposed' && x.item.branch && siteOf(x.site_id)?.source === 'local');
    const key = ps.map((x) => x.item.id + x.item.head_sha).join();
    if (!key) { conflicts = new Set(); return; }
    const ids = [...new Set(ps.map((x) => x.site_id))];
    Promise.all(ids.map((id) => invoke<string[]>('queue_conflicts', { id }).catch(() => [] as string[])))
      .then((r) => { if (key === needs.filter((x) => x.item.status === 'proposed' && x.item.branch && siteOf(x.site_id)?.source === 'local').map((x) => x.item.id + x.item.head_sha).join()) conflicts = new Set(r.flat()); });
  });
  // 一起合并上线：显示的待确认改动里能合进主分支的都是同一个站点、两条以上时，「等你确认」标题旁给一个按钮
  const mergeable = $derived.by(() => {
    const ps = shown.filter((x) => x.item.status === 'proposed' && x.item.branch && !conflicts.has(x.item.id));
    const ids = new Set(ps.map((x) => x.site_id));
    const site = ids.size === 1 ? siteOf(ps[0].site_id) : undefined;
    return site && ps.length > 1 ? { site, items: ps.map((x) => x.item) } : null;
  });
  // 「用 XX 重新改」：有失败条目的站点各自默认用的模型（托管站点是方案里的主模型）；▾ 换个模型只用这一次
  let siteModels = $state<Record<string, string>>({});
  $effect(() => {
    const ids = [...new Set(needs.filter((x) => x.item.status === 'failed').map((x) => x.site_id))];
    for (const id of ids) {
      if (untrack(() => id in siteModels)) continue;
      invoke<[string, string, string]>('site_ops_model', { id }).then(([b, m]) => (siteModels = { ...siteModels, [id]: b ? `${b}::${m}` : '' })).catch(() => {});
    }
  });
  let mMenu = $state<{ at: { left: number; top: number; bottom: number; right: number }; siteId: string; item: OpsItem } | null>(null);
  function openModelMenu(e: MouseEvent, siteId: string, item: OpsItem) {
    e.preventDefault();
    const r = e.type === 'contextmenu' ? { left: e.clientX, right: e.clientX + 260, top: e.clientY, bottom: e.clientY } : (e.currentTarget as HTMLElement).getBoundingClientRect();
    mMenu = { at: { left: r.left, right: r.right, top: r.top, bottom: r.bottom }, siteId, item };
  }
  let open = $state<Set<string>>(new Set());
  function toggle(id: string) { const n = new Set(open); if (n.has(id)) n.delete(id); else n.add(id); open = n; }
  const siteOf = (id: string) => sites.list.find((s) => s.id === id);
</script>

{#if stuck.length}
  <div class="stucks">
    <button class="stuck st-head" onclick={toggleStuck} aria-expanded={stuckOpen} data-tip={stuckOpen ? '收起' : '展开看是哪些站点'}>
      <Icon name="pause" size={12} />
      <span class="st-t"><b>{stuck.length} 个站点的任务排着没动</b><span>共 {stuckTotal} 条</span>{#if stuckWhy}<span class="st-why">{stuckWhy}</span>{/if}</span>
      <svg class="st-chev" class:open={stuckOpen} width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5 6 7.5 9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </button>
    {#if stuckOpen}{#each stuck as x (x.id)}
      <!-- 一行：站点 · 几条排着 · 为什么（悬停看怎么恢复），右边查看、一个主要动作、清除计划 -->
      <div class="stuck">
        <Icon name="pause" size={12} />
        <span class="st-t">
          <button class="st-site" onclick={() => openSite(x.id, 'queue', 'open')} data-tip="打开任务队列看这些任务"><SiteIcon id={x.id} size={13} />{x.name}</button>
          <span>{x.queued} 条任务排着没动</span>
          <span class="st-why" data-tip={x.fix}>{x.why}</span>
        </span>
        <button class="btn tiny quiet st-see" onclick={() => openSite(x.id, 'queue', 'open')} data-tip="打开任务队列看这些任务">查看</button>
        {#if x.kind === 'site'}<button class="btn tiny" onclick={() => onturnon(x.id)} data-tip="只打开这个站点的托管，排着的任务会接着做"><Icon name="play" size={10} />打开托管</button>{/if}
        {#if x.kind === 'budget' && x.group}<button class="btn tiny" onclick={() => onbudget(x.group)}><Icon name="gauge" size={11} />调整额度</button>{/if}
        {#if x.hasPlan}<button class="ic-btn ic-sm st-x" onclick={() => clearStuck(x)} aria-label="清除计划" data-tip="清除计划：拿掉这些还没开始的任务"><Icon name="x" size={13} /></button>{/if}
      </div>
    {/each}{/if}
  </div>
{/if}

<div class="card nvc">
  {#if !needs.length}
    <div class="empty"><Icon name="check" size={14} />{stuck.length ? '没有要你确认的改动。' : '没有需要你处理的事。'}<InfoTip text={'这里只放要你动手的：判定不能自动上线的改动、失败的任务，以及上线后线上验收有问题要你核对的。排着等 AI 处理的不在这里。'} label="需要你说明" /></div>
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
            <h4><span class="gdot {g.c}"></span>{g.l}<span class="cnt">{g.items.length}</span><InfoTip text={g.tip} label="{g.l}说明" />
              {#if g.k === 'proposed' && mergeable}{@const m = mergeable}<span class="grow"></span><button class="btn tiny primary" onclick={() => acceptItems(m.site, m.items)} data-tip={`按顺序把这 ${m.items.length} 条合并进 ${m.site.name} 的主分支，合完只构建上线一次\n合不进去的会跳过、留在这里；上线后验收不通过会把这次合并的全部回退`}><Icon name="merge" size={11} />一起合并上线 {m.items.length} 条</button>{/if}</h4>
            <ol class="items">
              {#each g.items as x (x.item.id)}
                {@const i = x.item}
                {@const site = siteOf(x.site_id)}
                {@const isOpen = open.has(i.id)}
                {@const w = why(i)}
                {@const busy = busyOf(x.site_id, i.id)}
                <li class:open={isOpen}>
                  <button class="row" onclick={() => toggle(i.id)} aria-expanded={isOpen}>
                    {#if !sel}<span class="sn"><SiteIcon id={x.site_id} size={13} />{x.site_name}</span>{/if}
                    <span class="tt">{i.opp.title}</span>
                    {#if busy}<span class="pill run cf"><span class="spin"></span>{busy.label}</span>{:else if conflicts.has(i.id)}<span class="pill warn cf" data-tip="和主分支有冲突，要让 AI 在最新代码上重改">有冲突</span>{/if}
                    <svg class="chev" class:up={isOpen} width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  </button>
                  {#if isOpen}
                    <div class="more">
                      {#if conflicts.has(i.id)}<p class="why">和主分支有冲突：AI 这版是在旧代码上改的，主分支后来也改了同几处，合不进去。点「让 AI 在最新代码上重改」：先试着直接接上，接不上再让 AI 重做。合并上线以后有这种情况会自动处理。</p>
                      {:else if w}<p class="why {g.k === 'failed' ? 'err' : ''}">{g.k === 'proposed' ? '没自动上线：' : g.k === 'failed' ? '失败原因：' : '要你核对：'}{w}</p>{/if}
                      {#if g.k === 'failed' && w}<OpsErrorHint text={w} />{/if}
                      {#if g.k === 'proposed' && site && !conflicts.has(i.id)}{@const plan = managed.overview?.groups.find((p) => p.sites.includes(site.id))}{#if plan?.autonomy === 'auto'}{#each (i.block_kinds ?? []).filter((k) => !plan.allow_kinds?.includes(k)) as k (k)}<button class="link-btn allow" onclick={() => setBlockKind(site.id, k, true, plan.name)} data-tip={`托管方案「${plan.name}」以后遇到这类原因不再留给你确认；别的原因照样拦`}><Icon name="check" size={11} />「{BLOCK_KIND_LABEL[k] ?? k}」以后不拦</button>{/each}{/if}{#if plan}{#each blockedCode(i.verdict, site.ops.content_code) as f (f)}<button class="link-btn allow" onclick={() => setContentCode(site, f, true, plan)} data-tip="这个站点的页面内容（标题、描述、文章）写在这个代码文件里的话：放行后按托管方案的「拦截等级」判断，不再一律留给你确认"><Icon name="check" size={11} />{f} 是页面内容，以后按方案规则判断</button>{/each}{/if}{/if}
                      <WhyChips change={i.opp.change} source={sourceOf(i.opp)} kind={i.opp.kind} target={i.opp.target} />
                      {#if i.opp.page || i.new_pages?.length}<span class="mono url">{i.opp.page || i.new_pages?.[0]}</span>{/if}
                      <div class="acts">
                        {#if site && busy}
                          <button class="btn tiny primary" disabled><span class="spin"></span>{busy.label}{busy.step ? ` · ${busy.step}` : ''}</button>
                        {:else if site}
                          {#if i.status === 'proposed'}
                            {#if i.head_sha}<button class="btn tiny" onclick={() => showDiff(site.id, i)}><Icon name="diff" size={11} />看改动</button>{/if}
                            {#if conflicts.has(i.id)}<button class="btn tiny primary" onclick={() => redoItem(site.id, i)} data-tip="先试着用 git 把这一版直接接到最新的主分支上（不花钱）；接不上再让 AI 照着这一版在最新代码上重做。好了照常检查，再按方案规则上线或留给你确认"><Icon name="sparkles" size={11} />让 AI 在最新代码上重改</button>
                            {:else}<button class="btn tiny primary" onclick={() => acceptItem(site, i)}><Icon name={i.branch ? 'rocket' : 'check'} size={11} />{i.branch ? '上线' : '标记已处理'}</button>{/if}
                          {:else if i.status === 'merged'}
                            <button class="btn tiny primary" onclick={() => reshipItem(site, i)} data-tip="改动已经在主分支里，不再合并，直接用主分支最新的代码重新构建上线"><Icon name="rocket" size={11} />重新上线</button>
                          {:else if i.status === 'failed'}
                            {#if i.branch && i.head_sha && !i.merge_sha}<button class="btn tiny primary" onclick={() => recheckItem(site.id, i)} data-tip="改动还在分支上：重新构建、检查，不再让 AI 改一遍"><Icon name="refresh" size={11} />重新检查</button>{/if}
                            {@const mt = modelTarget(siteModels[site.id] ?? '')}
                            <span class="split"><button class="btn tiny" onclick={() => workItem(site.id, i)} oncontextmenu={(e) => openModelMenu(e, site.id, i)} data-tip={`用 ${mt.name} 重新改一遍（这个站点默认的模型）\n右边 ▾ 可以换个模型，只用这一次`}>{#if mt.icon}<span class="bi-w"><BrainIcon brain={mt.icon} size={12} /></span>{:else}<Icon name="sparkles" size={11} />{/if}用 {mt.name} 重新改</button><button class="btn tiny caret" onclick={(e) => openModelMenu(e, site.id, i)} aria-label="换个模型重新改" data-tip="换个模型重新改（只用这一次）"><svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button></span>
                            {#if i.head_sha}<button class="btn tiny" onclick={() => showDiff(site.id, i)}><Icon name="diff" size={11} />看改动</button>{/if}
                          {:else}
                            <button class="btn tiny primary" onclick={() => ackItem(site.id, i)}><Icon name="check" size={11} />没问题</button>
                            {#if i.merge_sha}<button class="btn tiny danger" onclick={() => rollbackItem(site, i)}><Icon name="undo" size={11} />回滚</button>{/if}
                          {/if}
                          {#if i.conv_id}<button class="btn tiny" onclick={() => openConv(i.conv_id)}><Icon name="chat" size={11} />打开会话</button>{/if}
                          <span class="grow"></span>
                          {#if i.status !== 'deployed'}<button class="btn tiny quiet" onclick={() => dismissItem(site, i)}><Icon name="x" size={11} />驳回</button>{/if}
                        {/if}
                        <button class="link-btn" onclick={() => openSite(x.site_id, 'queue', '', i.id)}>在任务队列里看</button>
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

<DiffModal />

{#if mMenu}<ModelMenu at={mMenu.at} current={siteModels[mMenu.siteId] ?? ''} note="只用这一次，站点和托管方案的模型不变" onpick={(m, _d, e) => { const x = mMenu; mMenu = null; if (x) void workItem(x.siteId, x.item, m, e); }} onclose={() => (mMenu = null)} />{/if}

<style>
  /* 「用 XX 重新改」+ ▾：两个按钮拼成一个 */
  .split { display: inline-flex; }
  .split .btn:first-child { border-top-right-radius: 0; border-bottom-right-radius: 0; }
  .split .caret { padding: 0 6px; margin-left: -1px; border-top-left-radius: 0; border-bottom-left-radius: 0; }
  .bi-w { display: inline-flex; }
  .row .cf { flex: none; font-size: 11px; }
  .stucks { display: flex; flex-direction: column; gap: 6px; margin-bottom: 12px; }
  .stuck { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 6px 8px 6px 12px; border-radius: 9px; background: var(--su-warn-soft); color: var(--su-warn-ink); font-size: 12.5px; }
  .stuck > :global(svg) { flex: none; }
  .st-t { flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; white-space: nowrap; overflow: hidden; }
  .st-site { flex: none; display: inline-flex; align-items: center; gap: 5px; border: 0; background: none; padding: 0; font: inherit; font-weight: 600; color: inherit; cursor: pointer; }
  .st-site:hover { text-decoration: underline; text-underline-offset: 3px; }
  .st-why { min-width: 0; overflow: hidden; text-overflow: ellipsis; opacity: .75; }
  .st-why::before { content: '·'; margin-right: 8px; }
  .stuck .st-see { color: var(--su-warn-ink); }
  .stuck .st-see:hover { background: rgba(0, 0, 0, .05); }
  .stuck .st-x { color: var(--su-warn-ink); opacity: .6; margin: 0; }
  .stuck .st-x:hover { opacity: 1; background: rgba(0, 0, 0, .05); }
  /* 汇总那一行：整行可点，展开后下面是每个站点一行 */
  .st-head { width: 100%; border: 0; font: inherit; text-align: left; cursor: pointer; padding-right: 14px; }
  .st-head:hover { filter: brightness(.98); }
  .st-head b { font-weight: 600; }
  .st-chev { flex: none; transition: transform .15s; }
  .st-chev.open { transform: rotate(180deg); }

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
  .link-btn.allow { display: inline-flex; align-items: center; gap: 5px; align-self: flex-start; font-size: 12px; }

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


  @container (max-width: 640px) {
    .nv { grid-template-columns: minmax(0, 1fr); }
    .slist { border-right: 0; border-bottom: 1px solid var(--border); }
    .sl-in { position: static; flex-direction: row; flex-wrap: wrap; max-height: none; }
    .site { flex: none; }
    .site .nm { flex: none; }
  }
</style>
