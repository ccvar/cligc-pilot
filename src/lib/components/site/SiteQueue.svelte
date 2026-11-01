<script lang="ts">
  import { escClose } from '$lib/escClose';
  import './site.css';
  import { untrack, tick } from 'svelte';
  import Icon from './Icon.svelte';
  import type { Site, OpsItem } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import ModelMenu from './ModelMenu.svelte';
  import OpsErrorHint from './OpsErrorHint.svelte';
  import { openConnectionSetup } from '$lib/state/settings.svelte.js';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import { clock, full as fullTime, pageUrl } from './fmt';
  import { managed, AUTONOMY_LABEL, budgetFull, editGroup, openManaged, setBlockKind, BLOCK_KIND_LABEL } from '$lib/state/managed.svelte.js';
  import WhyChips from './WhyChips.svelte';
  import { sites } from '$lib/state/sites.svelte.js';
  import { synd } from '$lib/state/syndication.svelte.js';
  import { ops, loadQueue, runTask, workItem, redoItem, recheckItem, reshipItem, acceptItem, acceptItems, blockedCode, setContentCode, workItems, dismissItems, ackItem, dismissItem, restoreItem, markDone, rollbackItem, showDiff, sourceOf, KIND_LABEL, CHANGE_LABEL, STATUS_LABEL } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';
  import { mdRender, mdClick } from '$lib/state/messages.svelte.js';
  import { invoke } from '@tauri-apps/api/core';
  import { say } from '$lib/state/app.svelte.js';
  import { modelName, modelTarget } from '$lib/state/fleet.svelte.js';

  let { site }: { site: Site } = $props();
  $effect(() => { void loadQueue(site.id); });
  const all = $derived(ops.queue[site.id] ?? []);
  const job = $derived(sites.jobs[site.id]);
  const busy = $derived(job?.status === 'running');
  // 「待处理」一栏里分三段：需要你处理 / AI 处理中（含这一批里排着的）/ 未处理的机会；条目只在段之间挪，不会跳到别的栏
  let filter = $state<'todo' | 'live' | 'closed'>('todo');
  const groups = {
    live: ['merged', 'deployed', 'done'],
    closed: ['dismissed', 'rolled_back'],
  } as const;
  // 合并了但上线没完成的：还没上线，要你改好问题后「重新上线」
  const unshipped = (i: OpsItem) => i.status === 'merged' && !i.deployed_at && site.deploy.mode !== 'none';
  // 要你动手的：改好等你确认、失败、上线没完成、上线了但验收有待核对
  const needOf = (i: OpsItem) => i.status === 'proposed' || i.status === 'failed' || unshipped(i) || (i.status === 'deployed' && !!i.attention);
  const inGroup = (i: OpsItem, k: 'todo' | 'live' | 'closed') =>
    k === 'todo' ? needOf(i) || i.status === 'working' || i.status === 'open' : !needOf(i) && (groups[k] as readonly string[]).includes(i.status);
  // 这一批交给 AI 的（按顺序）：还没轮到的放在「AI 处理中」里写排第几，不混在机会里
  const batchIds = $derived(job?.status === 'running' && job.kind === 'ops:batch' ? (ops.batch[site.id] ?? []) : []);
  const queued = (i: OpsItem) => i.status === 'open' && batchIds.includes(i.id);
  const needItems = $derived(all.filter(needOf));
  const runningItems = $derived([...all.filter((i) => i.status === 'working'), ...batchIds.map((id) => all.find((i) => i.id === id)).filter((i): i is OpsItem => !!i && queued(i))]);
  const openItems = $derived(all.filter((i) => i.status === 'open' && !queued(i)));
  const shown = $derived(filter === 'todo' ? [] : all.filter((i) => inGroup(i, filter)));
  // 这一栏空了，但刚处理的条目只是挪到了别的栏（已经好了自动关掉去了「已上线」…）：
  // 空白处列出最近 15 分钟动过的，写明现在在哪、点一下过去
  const TAB_LABEL: Record<'todo' | 'live' | 'closed', string> = { todo: '待处理', live: '已上线', closed: '已驳回 / 回滚' };
  const tabOf = (i: OpsItem) => (['todo', 'live', 'closed'] as const).find((k) => inGroup(i, k)) ?? 'todo';
  const moved = $derived(all.filter((i) => !inGroup(i, filter) && Date.now() / 1000 - i.updated_at < 900).sort((a, b) => b.updated_at - a.updated_at).slice(0, 3));
  // 栏上的数字：待处理只算要你动手的（和侧栏角标一致），机会再多也不让数字一直很大
  const counts = $derived({ todo: needItems.length, live: all.filter((i) => inGroup(i, 'live')).length, closed: all.filter((i) => inGroup(i, 'closed')).length });
  // 从别处跳过来指定了栏目（页面已经开着也生效）；老的「未处理的机会」「需要你处理」都在「待处理」里
  $effect(() => {
    const want = sites.queueFilter;
    if (want) untrack(() => { filter = want === 'open' ? 'todo' : want; sites.queueFilter = ''; });
  });
  // 从别处点「看详情」跳过来定位某一条：切到它所在的栏、展开、滚过去闪一下。
  // 队列是旧缓存、里面还没有这一条（刚上线的）就重读一次，还没有就算了
  let listEl = $state<HTMLElement>();
  let flash = $state('');
  let reread = '';
  $effect(() => {
    const want = sites.queueItem;
    const q = ops.queue[site.id];
    if (!want || !q) return;
    const it = q.find((x) => x.id === want);
    untrack(() => {
      if (!it) {
        if (reread !== want) { reread = want; void loadQueue(site.id); } else sites.queueItem = '';
        return;
      }
      sites.queueItem = '';
      filter = tabOf(it);
      unfolded = new Set([...unfolded, it.id]);
      flash = it.id;
      void tick().then(() => listEl?.querySelector(`[data-qid="${CSS.escape(it.id)}"]`)?.scrollIntoView({ block: 'center', behavior: 'smooth' }));
      setTimeout(() => { if (flash === it.id) flash = ''; }, 2400);
    });
  });
  let openChecks = $state<Set<string>>(new Set());
  function toggleChecks(id: string) { const n = new Set(openChecks); if (n.has(id)) n.delete(id); else n.add(id); openChecks = n; }
  // 上面那块作业记录：跑着的时候一直显示；跑完以后，针对某一条的作业（改、重新检查、批量改）结果已经写在那一条上了，
  // 不再重复挂一块（同一句报错上下各一遍很乱）；扫描、制定计划这类整站作业跑完 5 分钟内还留着
  const ITEM_JOBS = ['ops:work', 'ops:recheck', 'ops:batch'];
  const showJob = $derived(!!job && job.kind.startsWith('ops:') && (job.status === 'running' || (!ITEM_JOBS.includes(job.kind) && job.finished_at > Date.now() / 1000 - 300)));
  // 待你确认的条目里合不进主分支的（本地来源才查；队列一变就重查，不动站点文件夹）
  let conflicts = $state<Set<string>>(new Set());
  $effect(() => {
    const id = site.id;
    const n = all.filter((i) => i.status === 'proposed' && i.branch).map((i) => i.id + i.head_sha).join();
    if (site.source !== 'local' || !n) { conflicts = new Set(); return; }
    invoke<string[]>('queue_conflicts', { id }).then((v) => { if (id === site.id) conflicts = new Set(v); }).catch(() => (conflicts = new Set()));
  });
  const when = clock;
  const tone = (s: string) => (s === 'proposed' ? 'warn' : s === 'deployed' || s === 'done' ? 'ok' : s === 'failed' ? 'err' : s === 'working' ? 'run' : '');
  const myPlan = $derived(managed.overview?.groups.find((g) => g.sites.includes(site.id)));
  // 托管停着（方案暂停或这个站点单独停止）：扫描只找机会，不交给 AI
  const planOff = $derived(!!myPlan && (myPlan.paused || myPlan.stopped.includes(site.id)));
  const full = $derived(myPlan ? budgetFull(managed.overview?.sites.find((x) => x.id === site.id)?.budget) : '');
  const canWork = (i: OpsItem) => i.status === 'open' || i.status === 'failed';
  // 「用 XX 改」：按钮上写这个站点默认用的模型（托管站点是方案里的主模型）；▾ 换个模型只用这一次
  let siteModel = $state('');
  // 主模型这会儿用不了（余额不足这类）时，后端给的是备用模型，modelNote 写明原因
  let modelNote = $state('');
  $effect(() => {
    const id = site.id;
    void job?.status; // 作业跑完再取一次：中途可能刚发现主模型用不了
    invoke<[string, string, string]>('site_ops_model', { id }).then(([b, m, n]) => { if (id === site.id) { siteModel = b ? `${b}::${m}` : ''; modelNote = n; } }).catch(() => { siteModel = ''; modelNote = ''; });
  });
  const mt = $derived(modelTarget(siteModel));
  let mMenu = $state<{ at: { left: number; top: number; bottom: number; right: number }; item: OpsItem } | null>(null);
  function openModelMenu(e: MouseEvent, item: OpsItem) {
    e.preventDefault();
    const r = e.type === 'contextmenu' ? { left: e.clientX, right: e.clientX + 260, top: e.clientY, bottom: e.clientY } : (e.currentTarget as HTMLElement).getBoundingClientRect();
    mMenu = { at: { left: r.left, right: r.right, top: r.top, bottom: r.bottom }, item };
  }
  let openWhy = $state<Set<string>>(new Set());
  // 已上线、已驳回 / 回滚：每条默认只留标题一行，点标题或左边的箭头展开看详情；上线后有待核对的默认展开
  const foldable = $derived(filter === 'live' || filter === 'closed');
  let unfolded = $state<Set<string>>(new Set());
  const folded = (i: OpsItem) => foldable && !unfolded.has(i.id) && !(i.status === 'deployed' && i.attention);
  const flipFold = (id: string) => { const n = new Set(unfolded); if (n.has(id)) n.delete(id); else n.add(id); unfolded = n; };
  // 机会的说明常常是「数据依据 + 做法」写成一大段：按「做法 / 改法 / 怎么改」拆成两行，各自默认最多两行，点「展开全文」看完整的
  function splitEvidence(t: string): { k: string; v: string }[] {
    const m = t.match(/^(.*?)[。；;\s]*(?:做法|改法|怎么改|建议做法)[:：]\s*(.+)$/s);
    if (!m || !m[1].trim()) return [{ k: '依据', v: t.trim() }];
    return [{ k: '依据', v: m[1].trim().replace(/[。；;]?$/, '。') }, { k: '做法', v: m[2].trim() }];
  }
  let openEv = $state<Set<string>>(new Set());
  const flipEv = (id: string) => { const n = new Set(openEv); if (n.has(id)) n.delete(id); else n.add(id); openEv = n; };
  // 标题后面的目标页面：改已有页面就是那个页面，新写的页面用 AI 报上来的新网址；都没有就不显示
  // 页面只记了路径的（页面体检那几类）补上站点域名，才能在浏览器打开
  const targetOf = (i: OpsItem) => pageUrl(i.opp.page, site.domain) || ((i.new_pages ?? [])[0] ?? '');
  // 网址不在站点主域名下（www、别的子域名）就带上域名，免得只看到 /sitemap.xml 分不清是哪个
  const shortPath = (u: string) => { try { const x = new URL(u); const p = x.pathname + x.search || '/'; return x.host.toLowerCase() === site.domain.trim().toLowerCase() ? p : x.host + p; } catch { return u; } };
  // 待处理的条目写明是按哪天的数据发现的（数据会变，动手前还会再确认一次）；其它状态照旧写最后更新的时间
  const DAY = 86_400;
  const foundTip = (i: OpsItem) => `${fullTime(i.created_at)} 按当时的数据发现。数据会变：拉数据时问题已经没有了会自动关掉，交给 AI 之前也会再按最新数据确认一次`;
  // 「AI 怎么改的」不是中文（英文站点容易这样）：可以翻成中文，译文存在条目上；翻过的默认看译文，能切回原文
  const foreign = (t: string) => !/[\u4e00-\u9fff]/.test(t.slice(0, 300));
  let translating = $state('');
  let showOrig = $state<Set<string>>(new Set());
  async function translate(i: OpsItem) {
    if (translating) return;
    translating = i.id;
    try {
      const zh = await invoke<string>('queue_translate_summary', { id: site.id, itemId: i.id });
      i.ai_summary_zh = zh;
    } catch (e) { say(String(e), 'err'); }
    finally { translating = ''; }
  }
  const flipOrig = (id: string) => { const n = new Set(showOrig); if (n.has(id)) n.delete(id); else n.add(id); showOrig = n; };
  // 批量：只在「未处理的机会」那一段里勾选
  let picked = $state<Set<string>>(new Set());
  const pickable = $derived(filter === 'todo' ? openItems : []);
  const pickedItems = $derived(pickable.filter((i) => picked.has(i.id)));
  const allPicked = $derived(pickable.length > 0 && pickedItems.length === pickable.length);
  $effect(() => { void filter; picked = new Set(); });
  function togglePick(id: string) { const n = new Set(picked); if (n.has(id)) n.delete(id); else n.add(id); picked = n; }
  function pickAll() { picked = allPicked ? new Set() : new Set(pickable.map((i) => i.id)); }
  async function batchWork(model = '', effort = '') {
    const ids = pickedItems.map((i) => i.id);
    picked = new Set();
    await workItems(site.id, ids, model, effort);
  }
  // 批量的 ▾：换个模型改这几条（只用这一次）
  let bMenu = $state<{ left: number; top: number; bottom: number; right: number } | null>(null);
  async function batchDismiss() {
    const items = pickedItems;
    await dismissItems(site, items);
    picked = new Set();
  }
  // 「需要你处理」里改好、能合进主分支的待确认改动：两条以上时可以一起合并、只上线一次
  const mergeable = $derived(needItems.filter((i) => i.status === 'proposed' && i.branch && !conflicts.has(i.id)));
  function toggleWhy(id: string) { const n = new Set(openWhy); if (n.has(id)) n.delete(id); else n.add(id); openWhy = n; }
</script>

<div class="qq">
  <div class="qbar">
    <div class="seg">
      <button class:on={filter === 'todo'} onclick={() => (filter = 'todo')} data-tip="要你处理的、AI 正在处理的、还没处理的机会都在这里；数字是要你处理的条数">待处理{#if counts.todo}<span class="n">{counts.todo}</span>{/if}</button>
      <button class:on={filter === 'live'} onclick={() => (filter = 'live')}>已上线{#if counts.live}<span class="n">{counts.live}</span>{/if}</button>
      <button class:on={filter === 'closed'} onclick={() => (filter = 'closed')}>已驳回 / 回滚{#if counts.closed}<span class="n">{counts.closed}</span>{/if}</button>
    </div>
    <span class="grow"></span>
    <button class="link-btn lvl" class:off={planOff} onclick={() => (sites.tab = 'overview')}>{myPlan ? `托管：${myPlan.name} · ${myPlan.paused ? '方案已暂停' : planOff ? '已停止' : AUTONOMY_LABEL[myPlan.autonomy] ?? ''}` : '自己运营'}</button>
    <button class="btn small" onclick={() => runTask(site.id, 'scan')} disabled={busy} data-tip={planOff ? '托管停着：拉最新数据、找出新的机会放进「未处理的机会」，不交给 AI 改；想改哪条再手动点' : myPlan || site.ops.level >= 2 ? '拉最新数据、找出新的机会，再按优先级挑前几条交给 AI，按方案规则改好上线或等你确认' : '拉最新数据、找出新的机会，放进「未处理的机会」'}>{#if busy && job?.kind === 'ops:scan'}<span class="spin"></span>扫描中{:else}<Icon name="scan" size={12} />立即扫描{/if}</button>
  </div>

  {#if showJob && job}<div class="card"><SiteJobPanel {job} compact /></div>{/if}

  {#if synd.pending[site.id]}
    <div class="qnote"><Icon name="share" size={13} /><span>分发：{synd.pending[site.id]} 篇文章的摘要已经写好，等你确认发布。</span><button class="link-btn" onclick={() => (sites.tab = 'distribute')}>去分发</button></div>
  {/if}

  {#if modelNote}
    <div class="qnote warn"><Icon name="info" size={13} /><span>{modelNote}。</span>{#if !modelNote.includes('没登录')}<button class="link-btn" onclick={(e) => openConnectionSetup(e)}>打开 API 设置</button>{/if}{#if myPlan}<button class="link-btn" onclick={() => { openManaged('groups'); editGroup(myPlan); }}>换主模型</button>{/if}</div>
  {/if}

  {#if full && openItems.length && filter === 'todo'}
    <div class="qnote warn"><Icon name="gauge" size={13} /><span>{full}，托管暂停处理这个站点的任务，下面的会一直排着。调高方案里的「每站每周 token」，或等用量滚出最近 7 天后，AI 会接着做；也可以点「让 AI 改」单独做某一条。</span>{#if myPlan}<button class="link-btn" onclick={() => { openManaged('groups'); editGroup(myPlan); }}>调整方案</button>{/if}</div>
  {/if}

  <div class="secs" bind:this={listEl}>
  {#if filter === 'todo'}
    {#if !needItems.length && !runningItems.length && !openItems.length}
      <div class="card empty">
        <p>没有要处理的事</p>
        {#if moved.length}
          <p class="hint">刚才处理的在这里：</p>
          <ul class="moved">
            {#each moved as i (i.id)}
              <li>
                <span class="pill {tone(i.status)}">{STATUS_LABEL[i.status] ?? i.status}</span>
                <span class="mv-t" data-tip={i.note || i.opp.title}>{i.opp.title}</span>
                <button class="link-btn" onclick={() => (sites.queueItem = i.id)}>去「{TAB_LABEL[tabOf(i)]}」看 →</button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="hint">AI 改好的提案、失败的任务、新发现的机会都会出现在这里；可以「立即扫描」，或在「数据」页把机会加进来。</p>
        {/if}
      </div>
    {/if}
    {#if needItems.length}
      <div class="card items">
        <div class="list-head sec-h">
          <b class="sec-t need">需要你处理<span class="n">{needItems.length}</span></b>
          {#if mergeable.length > 1}
            <span class="faint lh-hint">{mergeable.length} 条改好的可以一起合并，合完只{site.deploy.mode === 'none' ? '提交一次' : '上线一次'}</span>
            <span class="grow"></span>
            <button class="btn tiny primary" onclick={() => acceptItems(site, mergeable)} disabled={busy} data-tip={`按顺序把这 ${mergeable.length} 条合并进主分支，合完只构建上线一次\n后面的和前面的改到同一处、合不进去的会跳过，之后自动接到最新代码上\n上线后验收不通过会把这次合并的全部回退`}><Icon name="merge" size={11} />一起合并上线 {mergeable.length} 条</button>
          {/if}
        </div>
        <div class="list">{#each needItems as i (i.id)}{@render row(i, false)}{/each}</div>
      </div>
    {/if}
    {#if runningItems.length}
      <div class="card items">
        <div class="list-head sec-h"><b class="sec-t">AI 处理中<span class="n">{runningItems.length}</span></b><span class="faint lh-hint">改好以后按方案规则上线，或挪到上面「需要你处理」等你确认</span></div>
        <div class="list">{#each runningItems as i (i.id)}{@render row(i, false)}{/each}</div>
      </div>
    {/if}
    {#if openItems.length}
      <div class="card items">
        <div class="list-head sec-h" class:on={pickedItems.length > 0}>
          {#if openItems.length > 1}<label class="ck" data-tip={allPicked ? '全不选' : '全选'}><input type="checkbox" checked={allPicked} indeterminate={pickedItems.length > 0 && !allPicked} onchange={pickAll} aria-label="全选" />{#if pickedItems.length}已选 {pickedItems.length} 条{/if}</label>{/if}
          {#if !pickedItems.length}<b class="sec-t">未处理的机会<span class="n">{openItems.length}</span></b>{/if}
          {#if pickedItems.length}
            <span class="grow"></span>
            <span class="split"><button class="btn tiny primary" onclick={() => batchWork()} disabled={busy} data-tip={`用 ${mt.name} 一条一条按顺序处理：改、检查，再按方案规则上线或留给你确认；本周额度用完会停下${modelNote ? `\n${modelNote}` : ''}\n右边 ▾ 可以换个模型，只用这一次`}>{#if mt.icon}<span class="bi-w"><BrainIcon brain={mt.icon} size={12} /></span>{:else}<Icon name="sparkles" size={11} />{/if}用 {mt.name} 改 {pickedItems.length} 条</button><button class="btn tiny primary caret" onclick={(e) => { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); bMenu = { left: r.left, right: r.right, top: r.top, bottom: r.bottom }; }} disabled={busy} aria-label="换个模型改这几条" data-tip="换个模型改这几条（只用这一次）"><svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button></span>
            <button class="btn tiny quiet" onclick={batchDismiss} disabled={busy}><Icon name="x" size={11} />驳回 {pickedItems.length} 条</button>
          {:else}
            <span class="faint lh-hint">{myPlan && myPlan.autonomy !== 'suggest' ? `托管每轮按优先级处理排在前面的几条，其余的排着等下一轮；想现在做，勾选后点「让 AI 改」或用单条的按钮。` : '勾选几条，可以一起让 AI 改或驳回。'}</span>
          {/if}
        </div>
        <div class="list">{#each openItems as i (i.id)}{@render row(i, openItems.length > 1)}{/each}</div>
      </div>
    {/if}
  {:else if !shown.length}
    <div class="card empty">
      <p>暂无记录</p>
      {#if moved.length}
        <p class="hint">刚才处理的在这里：</p>
        <ul class="moved">
          {#each moved as i (i.id)}
            <li>
              <span class="pill {tone(i.status)}">{#if i.status === 'working'}<span class="spin"></span>{/if}{STATUS_LABEL[i.status] ?? i.status}</span>
              <span class="mv-t" data-tip={i.note || i.opp.title}>{i.opp.title}</span>
              <button class="link-btn" onclick={() => (sites.queueItem = i.id)}>去「{TAB_LABEL[tabOf(i)]}」看 →</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {:else}
    <div class="card items">
      <div class="list">{#each shown as i (i.id)}{@render row(i, false)}{/each}</div>
    </div>
  {/if}
  </div>
  <p class="hint foot">驳回过的机会不会再提；每条改动在默认分支上是独立提交，回滚即回退那一次提交并重新上线。</p>
</div>

{#snippet row(i: OpsItem, pick: boolean)}
    <div class="li item" class:picked={picked.has(i.id)} class:flash={flash === i.id} data-qid={i.id}>
      <div class="row it-head">
        {#if pick}<input class="pick" type="checkbox" checked={picked.has(i.id)} onchange={() => togglePick(i.id)} aria-label="选中这一条" />{/if}
        {#if foldable}<button class="fold" class:open={!folded(i)} onclick={() => flipFold(i.id)} aria-label={folded(i) ? '展开' : '收起'} aria-expanded={!folded(i)}><svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg></button>{/if}
        {#if queued(i)}<span class="pill run" data-tip="这一批交给 AI 的，前面的改完就轮到它">排队中 · 第 {batchIds.indexOf(i.id) + 1}/{batchIds.length} 条</span>
        {:else if i.status === 'deployed' && i.attention}<span class="pill warn">已上线 · 待核对</span>{:else if unshipped(i)}<span class="pill warn">已合并 · 没上线</span>{:else}<span class="pill {tone(i.status)}">{#if i.status === 'working'}<span class="spin"></span>{/if}{STATUS_LABEL[i.status] ?? i.status}</span>{/if}
        {#if foldable}<button class="it-title as-btn" title={i.opp.title} onclick={() => flipFold(i.id)}>{i.opp.title}</button>{:else}<b class="it-title" title={i.opp.title}>{i.opp.title}</b>{/if}
        {#if targetOf(i)}<button class="it-url mono" onclick={() => openUrl(targetOf(i))} data-tip={`在浏览器打开：${targetOf(i)}`}><span>{shortPath(targetOf(i))}</span><Icon name="external" size={11} /></button>{/if}
        <span class="grow"></span>
 {#if i.also?.length}<span class="pill line also" data-tip={`同一个页面还有：${i.also.map((o) => o.title).join('；')}\n交给 AI 时一起改`}>+ {i.also.map((o) => KIND_LABEL[o.kind] ?? o.kind).join('、')}</span>{/if}
        {#if i.status === 'open'}<span class="it-meta" data-tip={foundTip(i)}>{KIND_LABEL[i.opp.kind] ?? i.opp.kind} · {Date.now() / 1000 - i.created_at > DAY ? `${when(i.created_at)} 的数据` : when(i.created_at)}</span>
        {:else}<span class="it-meta">{KIND_LABEL[i.opp.kind] ?? i.opp.kind} · {when(i.updated_at)}</span>{/if}
      </div>
      {#if !folded(i)}
      <div class="why-row"><WhyChips change={i.opp.change} source={sourceOf(i.opp)} kind={i.opp.kind} target={i.opp.target} /></div>
      <dl class="it-ev" class:open={openEv.has(i.id)}>
        {#each splitEvidence(i.opp.evidence) as part, n (n)}<dt>{part.k}</dt><dd>{part.v}</dd>{/each}
      </dl>
      {#if i.opp.evidence.length > 90}<button class="link-btn ev-more" onclick={() => flipEv(i.id)}>{openEv.has(i.id) ? '收起' : '展开全文'}</button>{/if}
      {#if i.opp.expect_note}<p class="it-expect"><Icon name="target" size={11} />预期：{i.opp.expect_note}<span class="faint">（上线 4 周后按数据对比）</span></p>{/if}
      {#if i.ai_summary || i.files?.length}
        <button class="how-t" class:open={openWhy.has(i.id)} onclick={() => toggleWhy(i.id)} aria-expanded={openWhy.has(i.id)}>
          <svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
          AI 怎么改的{#if i.files?.length}<span class="faint"> · {i.files.length} 个文件</span>{/if}{#if i.model_used}<span class="faint" data-tip={i.model_used}> · {modelName(i.model_used) || i.model_used}</span>{/if}
        </button>
        {#if openWhy.has(i.id)}
          <div class="how">
            {#if i.ai_summary}
              {@const zh = i.ai_summary_zh && !showOrig.has(i.id)}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <div class="sum" onclick={(e) => mdClick(e, null)}>{@html mdRender((zh ? i.ai_summary_zh : i.ai_summary) ?? '')}</div>
              {#if i.ai_summary_zh}<button class="link-btn tr" onclick={() => flipOrig(i.id)}>{zh ? '看原文' : '看中文'}</button>
              {:else if foreign(i.ai_summary)}<button class="link-btn tr" onclick={() => translate(i)} disabled={translating === i.id}>{#if translating === i.id}<span class="spin"></span>翻译中…{:else}翻成中文{/if}</button>{/if}
            {/if}
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
      {#if conflicts.has(i.id) && i.status === 'proposed'}
        <p class="verdict">和主分支有冲突：AI 这版是在旧代码上改的，主分支后来也改了同几处，合不进去。点「让 AI 在最新代码上重改」：先试着直接接上，接不上再让 AI 重做。合并上线以后有这种情况会自动处理。</p>
      {:else if i.verdict && i.status === 'proposed'}
        <p class="verdict" class:ok={i.verdict === 'auto'}>{i.verdict === 'auto' ? '整份改动都在白名单内，检查已通过。' : `不能自动上线：${i.verdict}`}</p>
        {#if myPlan?.autonomy === 'auto'}{#each (i.block_kinds ?? []).filter((k) => !myPlan.allow_kinds?.includes(k)) as k (k)}<button class="link-btn allow" onclick={() => setBlockKind(site.id, k, true, myPlan.name)} data-tip={`托管方案「${myPlan.name}」以后遇到这类原因不再留给你确认；别的原因照样拦`}><Icon name="check" size={11} />「{BLOCK_KIND_LABEL[k] ?? k}」以后不拦</button>{/each}{/if}
        {#if myPlan}{#each blockedCode(i.verdict, site.ops.content_code) as f (f)}<button class="link-btn allow" onclick={() => setContentCode(site, f, true, myPlan)} data-tip="这个站点的页面内容（标题、描述、文章）写在这个代码文件里的话：放行后按托管方案的「拦截等级」判断，不再一律留给你确认"><Icon name="check" size={11} />{f} 是页面内容，以后按方案规则判断</button>{/each}{/if}
      {/if}
      {#if i.quality?.length}
        <!-- 判断模型（Jev）的内容质检：每页一行分数，没过的标出原因 -->
        <div class="quality">
          {#each i.quality as q, qi (`${q.page}:${qi}`)}
            <p class:bad={q.reasons.length > 0}><span class="qp">{q.page.split('/').pop()}</span>{#if q.answers != null}<span>回答问题 {q.answers.toFixed(1)}/3</span>{/if}<span>信息量 {q.substance.toFixed(1)}/3</span><span>相关 {q.on_topic.toFixed(1)}/3</span>{#if q.unsourced >= 0.5}<span>数据来源存疑 {Math.round(q.unsourced * 100)}%</span>{/if}{#if q.leftover >= 0.5}<span>遗留说明 {Math.round(q.leftover * 100)}%</span>{/if}{#if q.inherited?.length}<span class="inh" data-tip={`这些问题改动前的页面就有，这次没有变差，所以不拦：${q.inherited.join('；')}`}>原来就有的问题不算</span>{/if}</p>
          {/each}
        </div>
      {/if}
      {#if i.note && !(conflicts.has(i.id) && i.status === 'proposed' && i.note.includes('冲突'))}<p class="verdict">{i.note}</p>{#if i.status === 'failed' || i.status === 'merged'}<OpsErrorHint text={i.note} siteId={site.id} />{/if}{/if}
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
        {#if i.status === 'proposed' && conflicts.has(i.id)}<button class="btn tiny primary" onclick={() => redoItem(site.id, i)} disabled={busy} data-tip="先试着用 git 把这一版直接接到最新的主分支上（不花钱）；接不上再让 AI 照着这一版在最新代码上重做。好了照常检查，再按方案规则上线或留给你确认"><Icon name="sparkles" size={11} />让 AI 在最新代码上重改</button>
        {:else if i.status === 'proposed'}<button class="btn tiny primary" onclick={() => acceptItem(site, i)} disabled={busy}><Icon name={i.branch ? 'merge' : 'check'} size={11} />{i.branch ? (site.source === 'github' ? '合并并上线' : '合并到主分支并上线') : '标记已处理'}</button>{/if}
        {#if (i.status === 'failed' || (i.status === 'proposed' && i.verdict !== 'auto' && !conflicts.has(i.id))) && i.branch && i.head_sha}<button class="btn tiny" class:primary={i.status === 'failed'} onclick={() => recheckItem(site.id, i)} disabled={busy} data-tip="AI 的改动还在分支上：重新构建、检查，通过后按方案规则上线或留给你确认，不再让 AI 改一遍"><Icon name="refresh" size={11} />重新检查</button>{/if}
        {#if canWork(i)}<span class="split"><button class="btn tiny" onclick={() => workItem(site.id, i)} oncontextmenu={(e) => openModelMenu(e, i)} disabled={busy} data-tip={`用 ${mt.name} ${i.status === 'failed' && i.head_sha ? '重新改一遍' : '改'}：${modelNote || `这个站点默认的模型${siteModel ? '' : '（还没设好，会提示你去设）'}`}\n右边 ▾ 可以换个模型，只用这一次`}>{#if mt.icon}<span class="bi-w"><BrainIcon brain={mt.icon} size={12} /></span>{:else}<Icon name="sparkles" size={11} />{/if}用 {mt.name} {i.status === 'failed' && i.head_sha ? '重新改' : '改'}</button><button class="btn tiny caret" onclick={(e) => openModelMenu(e, i)} disabled={busy} aria-label="换个模型改" data-tip="换个模型改（只用这一次）"><svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button></span>{/if}
        {#if i.head_sha}<button class="btn tiny" onclick={() => showDiff(site.id, i)}><Icon name="diff" size={11} />查看差异</button>{/if}
        {#if i.conv_id}<button class="btn tiny" onclick={() => openConv(i.conv_id)}><Icon name="chat" size={11} />打开会话</button>{/if}
        {#if i.status === 'failed' && i.conv_id}<button class="btn tiny" onclick={() => markDone(site.id, i)}><Icon name="check" size={11} />我已手动处理</button>{/if}
        {#if ['dismissed', 'failed'].includes(i.status)}<button class="btn tiny" onclick={() => restoreItem(site.id, i)}><Icon name="inbox" size={11} />放回待处理</button>{/if}
        {#if i.status === 'deployed' && i.attention}<button class="btn tiny" onclick={() => ackItem(site.id, i)}><Icon name="check" size={11} />已核对</button>{/if}
        {#if i.status === 'merged' && !i.deployed_at && site.deploy.mode !== 'none'}<button class="btn tiny primary" onclick={() => reshipItem(site, i)} disabled={busy} data-tip="已经合并，上次上线没完成：重新构建并上线"><Icon name="rocket" size={11} />重新上线</button>{/if}
        {#if ['merged', 'deployed'].includes(i.status) && i.merge_sha}<button class="btn tiny danger" onclick={() => rollbackItem(site, i)} disabled={busy}><Icon name="undo" size={11} />回滚</button>{/if}
        {#if ['open', 'proposed', 'failed'].includes(i.status)}<span class="grow"></span><button class="btn tiny quiet" onclick={() => dismissItem(site, i)} disabled={busy}><Icon name="x" size={11} />驳回</button>{/if}
      </div>
      {/if}
    </div>
{/snippet}

{#if ops.diff}
  <div class="su-mask" role="presentation" onclick={() => (ops.diff = null)}></div>
  <div class="su modal diff-modal" use:escClose={() => (ops.diff = null)} role="dialog" aria-modal="true" aria-label="改动差异">
    <header class="sheet-head"><b>{ops.diff.item.opp.title}</b><button class="x" aria-label="关闭" data-tip="关闭" onclick={() => (ops.diff = null)}>×</button></header>
    <div class="sheet-body"><pre class="diff">{#each ops.diff.text.split('\n') as l, n (n)}<span class={l.startsWith('+') && !l.startsWith('+++') ? 'add' : l.startsWith('-') && !l.startsWith('---') ? 'del' : l.startsWith('@@') ? 'hunk' : ''}>{l}{'\n'}</span>{/each}</pre></div>
  </div>
{/if}

{#if bMenu}<ModelMenu at={bMenu} current={siteModel} note="只用这一次，站点和托管方案的模型不变" onpick={(m, _d, e) => { bMenu = null; void batchWork(m, e); }} onclose={() => (bMenu = null)} />{/if}
{#if mMenu}<ModelMenu at={mMenu.at} current={siteModel} note="只用这一次，站点和托管方案的模型不变" onpick={(m, _d, e) => { const it = mMenu?.item; mMenu = null; if (it) void workItem(site.id, it, m, e); }} onclose={() => (mMenu = null)} />{/if}

<style>
  .checks { list-style: none; margin: 2px 0 4px; padding: 0 0 0 16px; display: flex; flex-direction: column; gap: 3px; font-size: 12.5px; }
  .checks li { line-height: 1.55; }
  .checks .ck-i { display: inline-block; width: 16px; color: var(--ok); }
  .checks li.bad .ck-i { color: var(--warn); }
  .link-btn.lvl.off { color: var(--warn); }
  .link-btn.allow { display: inline-flex; align-items: center; gap: 5px; font-size: 12px; margin-top: -2px; }
  .list-head { display: flex; align-items: center; gap: 12px; margin: 0 -18px; padding: 9px 18px; min-height: 42px; border-bottom: 1px solid var(--border); border-radius: 12px 12px 0 0; font-size: 12.5px; }
  .list-head.on { background: var(--su-run-soft); }
  .list-head .ck { display: inline-flex; align-items: center; gap: 8px; cursor: pointer; flex: none; font-weight: 500; }
  .list-head .ck input { margin: 0 2px 0 0; }
  .list-head .grow { flex: 1; }
  .lh-hint { font-size: 12px; line-height: 1.5; min-width: 0; }
  .secs { display: flex; flex-direction: column; gap: 12px; }
  .sec-t { flex: none; font-size: 12.5px; font-weight: 600; }
  .sec-t .n { margin-left: 6px; font-size: 11.5px; font-weight: 500; color: var(--faint); font-variant-numeric: tabular-nums; }
  .sec-t.need .n { color: var(--su-warn-ink); }
  .pick { margin: 0 2px 0 0; flex: none; cursor: pointer; }
  .item.picked { background: var(--su-hover); }
  .item.flash { animation: qflash 2.4s ease-out; }
  @keyframes qflash { 0%, 40% { background: color-mix(in srgb, var(--accent, var(--ok)) 12%, transparent); } 100% { background: transparent; } }
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
  /* 卡片里直接一行一条（不再套一层框）：状态 · 标题 · 去哪一栏看，整行居中 */
  .moved { list-style: none; margin: 12px auto 0; padding: 0; display: flex; flex-direction: column; align-items: center; gap: 8px; }
  .moved li { display: flex; align-items: center; justify-content: center; gap: 10px; max-width: 100%; font-size: 12.5px; }
  .moved .pill { flex: none; }
  .mv-t { min-width: 0; max-width: 420px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .moved .link-btn { flex: none; font-size: 12px; }
  /* 「用 XX 改」+ ▾：两个按钮拼成一个 */
  .split { display: inline-flex; }
  .split .btn:first-child { border-top-right-radius: 0; border-bottom-right-radius: 0; }
  .split .caret { padding: 0 6px; margin-left: -1px; border-top-left-radius: 0; border-bottom-left-radius: 0; }
  .split .primary.caret { margin-left: 0; border-left: 1px solid rgba(255, 255, 255, .22); }
  .bi-w { display: inline-flex; }
  .items { padding-top: 0; padding-bottom: 0; }
  /* 全局 .su .card > .list:last-child 会把列表往下拉 8px（压出卡片底边）：这张卡片的列表贴着卡片底部，不往外伸 */
  .card.items > .list:last-child { margin-bottom: 0; }
  /* 悬停 / 选中时整条变底色：贴着卡片上下边的那一条跟卡片一样圆角（卡片 12px 圆角减 1px 边框），不露出直角 */
  .card.items > .list:first-child > .li:first-child { border-radius: 11px 11px 0 0; }
  .card.items > .list:last-child > .li:last-child { border-bottom-left-radius: 11px; border-bottom-right-radius: 11px; }
  .items .list > .li:first-child { border-top: none; }
  .item { display: flex; flex-direction: column; gap: 6px; padding-top: 14px; padding-bottom: 14px; }
  .it-head { min-width: 0; gap: 10px; }
  .it-head .pill { flex: none; }
  .it-url { flex: 0 1 auto; min-width: 0; max-width: 320px; display: inline-flex; align-items: center; gap: 3px; border: 0; background: none; padding: 0; font-size: 11.5px; color: var(--faint); cursor: pointer; white-space: nowrap; }
  .it-url span { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .it-url :global(svg) { flex: none; }
  .it-url:hover { color: var(--text); text-decoration: underline; text-underline-offset: 3px; }
  .it-head .grow { flex: 1; }
  .fold { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 18px; height: 18px; margin-right: -4px; border: 0; padding: 0; background: none; color: var(--faint); cursor: pointer; border-radius: 4px; }
  .fold svg { transition: transform .16s var(--su-ease); }
  .fold.open svg { transform: rotate(90deg); }
  .fold:hover { color: var(--text); }
  .it-title.as-btn { border: 0; padding: 0; background: none; font: inherit; font-size: 13px; font-weight: 600; color: var(--text); text-align: left; cursor: pointer; }
  .it-title.as-btn:hover { text-decoration: underline; text-underline-offset: 3px; text-decoration-color: var(--border2); }
  .it-title { flex: 0 1 auto; min-width: 0; font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .it-meta { flex: none; font-size: 11.5px; color: var(--faint); white-space: nowrap; }
  .why-row { margin: -1px 0 0; }
  .how-t { align-self: flex-start; display: inline-flex; align-items: center; gap: 5px; border: none; background: none; padding: 0; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; }
  .how-t:hover { color: var(--text); }
  .how .tr { align-self: flex-start; display: inline-flex; align-items: center; gap: 5px; font-size: 12px; }
  .how-t svg { transition: transform .16s var(--su-ease); }
  .how-t.open svg { transform: rotate(90deg); }
  .how { padding: 8px 12px; border-left: 2px solid var(--border2); margin-left: 4px; font-size: 12.5px; color: var(--dim); }
  /* AI 的说明是 Markdown：渲染出来（加粗、列表、代码），和「近期动态」里一样 */
  .how .sum { line-height: 1.7; color: var(--text); margin-bottom: 6px; }
  .how .sum :global(p) { margin: 0 0 6px; }
  .how .sum :global(p:last-child), .how .sum :global(ul:last-child), .how .sum :global(ol:last-child) { margin-bottom: 0; }
  .how .sum :global(ul), .how .sum :global(ol) { margin: 2px 0 6px; padding-left: 18px; }
  .how .sum :global(li) { margin: 3px 0; }
  .how .sum :global(strong) { font-weight: 600; }
  .how .sum :global(code) { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11.5px; padding: 1px 5px; border-radius: 4px; background: var(--accent-soft); overflow-wrap: anywhere; }
  .how .sum :global(a) { color: inherit; text-decoration: underline; text-underline-offset: 3px; }
  .files { margin: 0; padding: 0; list-style: none; display: flex; flex-wrap: wrap; gap: 4px 12px; font-size: 11.5px; color: var(--faint); }
  .it-expect { margin: 0; display: flex; align-items: center; gap: 5px; font-size: 12.5px; color: var(--dim); }
  /* 依据 / 做法两行：左边小标签，右边正文默认最多两行，展开看全文 */
  .it-ev { margin: 0; display: grid; grid-template-columns: auto 1fr; gap: 4px 10px; font-size: 12.5px; line-height: 1.65; max-width: 86ch; }
  .it-ev dt { padding-top: 1px; font-size: 11.5px; color: var(--faint); white-space: nowrap; }
  .it-ev dd { margin: 0; color: var(--dim); display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .it-ev.open dd { display: block; }
  .ev-more { align-self: flex-start; margin: -2px 0 0; font-size: 12px; }
  .meta { gap: 8px; }
  .br { font-size: 11.5px; }
  .verdict { margin: 0; font-size: 12px; line-height: 1.55; color: var(--su-warn-ink); background: var(--su-warn-soft); border-radius: 7px; padding: 5px 9px; align-self: flex-start; max-width: 100%; overflow-wrap: anywhere; }
  .verdict.ok { color: var(--ok); background: var(--su-ok-soft); }
  .quality { display: flex; flex-direction: column; gap: 2px; }
  .quality p { margin: 0; display: flex; flex-wrap: wrap; gap: 2px 10px; font-size: 11.5px; color: var(--dim); }
  .quality p.bad { color: var(--su-warn-ink); }
  .quality .qp { font-weight: 600; color: var(--text); max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .quality .inh { color: var(--faint); text-decoration: underline dotted; text-underline-offset: 2px; cursor: help; }
  .acts { gap: 6px; margin-top: 4px; }
  .foot { font-size: 11.5px; color: var(--faint); }
  .diff-modal { width: min(900px, calc(100vw - 32px)); }
  pre.diff { margin: 0; font-size: 11.5px; line-height: 1.5; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
  pre.diff .add { background: #e8f5ec; color: #135c34; }
  pre.diff .del { background: #fdecec; color: #8f1d1d; }
  pre.diff .hunk { color: #2f5fb8; }
  @media (max-width: 640px) { .it-meta { display: none; } }
</style>
