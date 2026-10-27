<script lang="ts">
  // 托管控制台「计划」页：左边站点列表（带进度），右边一次只看一个站点的计划。
  // 每条计划一行（标题 + 动作 + 在队列里走到哪了），理由、预期、页面点开再看；AI 的说明和站点定位默认只露两行。
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import WhyChips from './WhyChips.svelte';
  import { runTask, STAGE_LABEL } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';
  import { openSite, sites as siteState } from '$lib/state/sites.svelte.js';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import { modelIcon } from '$lib/state/models';
  import { launcher } from '$lib/state/launcher.svelte.js';
  import { invoke } from '@tauri-apps/api/core';
  import { confirmDialog, say } from '$lib/state/app.svelte.js';
  import { loadPlans } from '$lib/state/managed.svelte.js';
  import type { ManagedSite, SitePlan } from '$lib/types';

  let { sites, plans }: { sites: ManagedSite[]; plans: SitePlan[] } = $props();

  const ST: Record<string, { l: string; c: string }> = {
    '': { l: '没进队列', c: 'line' }, open: { l: '排队中', c: '' }, working: { l: '处理中', c: 'run' }, proposed: { l: '待你确认', c: 'warn' },
    merged: { l: '已合并', c: 'run' }, deployed: { l: '已上线', c: 'ok' }, attention: { l: '上线待核对', c: 'warn' },
    dismissed: { l: '已驳回', c: 'line' }, rolled_back: { l: '已回滚', c: 'err' }, failed: { l: '失败', c: 'err' },
  };
  const planOf = (id: string) => plans.find((p) => p.site_id === id);
  function progress(p?: SitePlan) {
    const st = p?.task_status ?? [];
    return {
      total: p?.plan.tasks.length ?? 0,
      done: st.filter((x) => x === 'deployed' || x === 'attention').length,
      tone: st.some((x) => x === 'failed' || x === 'rolled_back') ? 'err' : st.some((x) => x === 'proposed' || x === 'attention') ? 'warn' : '',
    };
  }

  let picked = $state('');
  let q = $state('');
  const shownSites = $derived.by(() => { const k = q.trim().toLowerCase(); return k ? sites.filter((s) => s.name.toLowerCase().includes(k) || s.domain?.toLowerCase().includes(k)) : sites; });
  const cur = $derived(sites.find((s) => s.id === picked) ?? sites.find((s) => progress(planOf(s.id)).tone) ?? sites[0]);
  const p = $derived(cur ? planOf(cur.id) : undefined);
  const prog = $derived(progress(p));

  // 正在跑的作业（制定计划、立即执行…）：按钮显示进行中，下面直接看进度；点过「制定」的站点跑完后结果留着
  const jobOf = (id: string) => siteState.jobs[id];
  const busy = (s: ManagedSite) => !!s.job || jobOf(s.id)?.status === 'running';
  const job = $derived(cur ? jobOf(cur.id) : undefined);
  const running = $derived(!!cur && busy(cur));
  let watched = $state<Set<string>>(new Set());
  async function makePlan() {
    if (!cur || running) return;
    const id = cur.id;
    watched = new Set([...watched, id]);
    await runTask(id, 'plan');
  }
  const showJob = $derived(!!job && (running || (!!cur && watched.has(cur.id) && job.kind.startsWith('ops:'))));

  // 谁出的计划（主模型失败换了备用模型的，记的是实际出计划的那个）
  const modelLabel = (b: string, m: string) => launcher.comboOpts.find((x) => x.value === `${b}::${m}`)?.label ?? (m || (b === 'codex' ? 'Codex 默认模型' : b));

  // 清除整份计划：还没开始的任务拿掉，已经在处理 / 上线的留着
  async function clearPlan() {
    if (!cur || !p || running) return;
    const st = p.task_status ?? [];
    const removable = st.filter((x) => x === 'open').length;
    const kept = st.filter((x) => x && x !== 'open').length;
    const msg = `清除 ${cur.name} ${md(p.plan.date)}的计划？\n· ${removable} 条还没开始的任务会从任务队列里拿掉${kept ? `\n· ${kept} 条已经在处理、等你确认或已上线的保留，已经做的改动不撤回` : ''}\n\n站点的托管还开着的话，下次自动运行会重新制定一份；不想要可以先暂停这个站点。`;
    if (!(await confirmDialog(msg, { title: '清除计划', kind: 'danger', confirmText: '清除计划' }))) return;
    try { const r = await invoke<{ removed: number; kept: number }>('plan_clear', { id: cur.id }); say(`已清除计划，拿掉 ${r.removed} 条任务${r.kept ? `，保留 ${r.kept} 条` : ''}`); await loadPlans(); }
    catch (e) { say(String(e), 'err'); }
  }
  // 单条：移出计划（不拦着以后再提）/ 驳回（以后不再提）
  async function removeTask(i: number) {
    if (!cur || !p) return;
    const t = p.plan.tasks[i];
    if (!(await confirmDialog(`把「${t.title}」移出计划？${p.task_status?.[i] === 'open' ? '任务队列里排着的这条也一起拿掉。' : ''}以后 AI 还可能再提类似的。`, { title: '移出计划', kind: 'warning', confirmText: '移出计划' }))) return;
    try { await invoke('plan_remove_task', { id: cur.id, index: i }); openTask = new Set(); await loadPlans(); }
    catch (e) { say(String(e), 'err'); }
  }
  async function rejectTask(i: number) {
    if (!cur || !p) return;
    const t = p.plan.tasks[i];
    const item = p.task_item?.[i];
    if (!item) return;
    if (!(await confirmDialog(`驳回「${t.title}」？之后不会再提这个机会。`, { title: '驳回', kind: 'warning', confirmText: '驳回' }))) return;
    try { await invoke('queue_dismiss', { id: cur.id, itemId: item }); await loadPlans(); }
    catch (e) { say(String(e), 'err'); }
  }

  let openTask = $state<Set<number>>(new Set());
  let noteOpen = $state(false);
  let posOpen = $state(false);
  $effect(() => { void cur?.id; openTask = new Set(); noteOpen = false; posOpen = false; });
  function toggle(i: number) { const n = new Set(openTask); if (n.has(i)) n.delete(i); else n.add(i); openTask = n; }

  // AI 的说明常写成「1) … 2) …」：展开时拆成清单
  const noteParts = $derived((p?.plan.notes ?? '').split(/\s*(?=(?:^|\s)\d+\)\s)/).map((x) => x.replace(/^\s*\d+\)\s*/, '').trim()).filter(Boolean));
  const md = (iso: string) => { const m = /^\d{4}-(\d{2})-(\d{2})$/.exec(iso); return m ? `${+m[1]}月${+m[2]}日` : iso; };
  const sourceFor = (t: SitePlan['plan']['tasks'][number]) => t.source || (t.change === 'new_content' ? 'positioning' : t.change === 'technical' ? 'health' : 'data');
</script>

<div class="card pvc"><div class="pv">
  <nav class="slist" aria-label="站点"><div class="sl-in">
    <!-- 搜索固定在列表顶上：列表滚动时一直在 -->
    <label class="sq"><Icon name="search" size={13} /><input type="search" bind:value={q} placeholder="搜索站点" spellcheck="false" autocomplete="off" onkeydown={(e) => { if (e.key === 'Escape') q = ''; }} /></label>
    {#each shownSites as s (s.id)}
      {@const sp = planOf(s.id)}
      {@const g = progress(sp)}
      <button class="site" class:on={cur?.id === s.id} onclick={() => (picked = s.id)} aria-current={cur?.id === s.id}>
        <SiteIcon id={s.id} size={15} />
        <span class="nm">{s.name}</span>
        {#if busy(s)}<span class="spin" aria-label="正在跑任务"></span>{/if}
        {#if g.tone}<i class="dot {g.tone}" aria-label={g.tone === 'err' ? '有失败' : '有要你处理的'}></i>{/if}
        <span class="pg">{sp ? `${g.done}/${g.total}` : '—'}</span>
      </button>
    {:else}<p class="none">没有匹配「{q.trim()}」的站点</p>{/each}
  </div></nav>

  {#if cur}
    <section class="detail">
      <header class="hd">
        <SiteIcon id={cur.id} size={16} />
        <b class="title">{cur.name}</b>
        <span class="stg {cur.stage}">{STAGE_LABEL[cur.stage] ?? cur.stage}</span>
        <span class="sub">{p ? `${md(p.plan.date)}制定 · ${prog.done}/${prog.total} 已上线` : '还没有计划'}</span>
        {#if p?.plan.brain}<span class="by" data-tip="出这份计划的模型"><BrainIcon brain={modelIcon(p.plan.brain)} size={12} />{modelLabel(p.plan.brain, p.plan.model ?? '')}</span>{/if}
        <span class="grow"></span>
        {#if p?.plan.conv_id}<button class="btn tiny" onclick={() => openConv(p.plan.conv_id)}><Icon name="sparkles" size={11} />看 AI 的分析</button>{/if}
        {#if p && !running}<button class="btn tiny quiet danger-q" onclick={clearPlan} data-tip="拿掉还没开始的任务，计划回到「还没有计划」"><Icon name="x" size={11} />清除计划</button>{/if}
        <button class="btn tiny" onclick={makePlan} disabled={running} data-tip={running ? '这个站点正在跑任务，跑完才能再制定' : null}>{#if running}<span class="spin"></span>{job?.title || cur.job}中…{:else}<Icon name="refresh" size={11} />{p ? '重新制定' : '现在制定'}{/if}</button>
      </header>
      {#if showJob && job}<div class="job"><SiteJobPanel {job} logOpen={running} /></div>{/if}
      {#if p && prog.total}<div class="bar" aria-hidden="true"><i style:width="{(prog.done / prog.total) * 100}%"></i></div>{/if}

      {#if p?.plan.notes}
        <div class="box">
          <div class="box-t"><Icon name="info" size={12} />AI 给你的说明<span class="grow"></span><button class="ic-btn ic-sm fold" class:up={noteOpen} onclick={() => (noteOpen = !noteOpen)} aria-expanded={noteOpen} aria-label={noteOpen ? '收起' : '展开全文'} data-tip={noteOpen ? '收起' : '展开全文'}><svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button></div>
          {#if noteOpen && noteParts.length > 1}<ol class="np">{#each noteParts as x, i (i)}<li>{x}</li>{/each}</ol>
          {:else}<p class:clamp={!noteOpen}>{p.plan.notes}</p>{/if}
        </div>
      {/if}
      {#if p?.positioning.text}
        <div class="box">
          <div class="box-t"><Icon name="target" size={12} />站点定位<span class="faint">{p.positioning.by === 'user' ? '运营人员确认' : 'AI 归纳'}</span><span class="grow"></span>
            <button class="ic-btn ic-sm fold" class:up={posOpen} onclick={() => (posOpen = !posOpen)} aria-expanded={posOpen} aria-label={posOpen ? '收起' : '展开全文'} data-tip={posOpen ? '收起' : '展开全文'}><svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
            <button class="ic-btn ic-sm" onclick={() => openSite(cur.id, 'overview')} aria-label="修改站点定位" data-tip="修改站点定位（在站点概览里）"><Icon name="pencil" size={13} /></button></div>
          <p class:clamp={!posOpen}>{p.positioning.text}</p>
        </div>
      {/if}

      {#if p}
        <ol class="tasks">
          {#each p.plan.tasks as t, i (i)}
            {@const st = ST[p.task_status?.[i] ?? ''] ?? ST['']}
            {@const open = openTask.has(i)}
            <li class:open>
              <button class="row" onclick={() => toggle(i)} aria-expanded={open}>
                <span class="no">{i + 1}</span>
                <span class="tt">{t.title}</span>
                <span class="pill {st.c}">{st.l}</span>
                <svg class="chev" class:up={open} width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
              </button>
              {#if open}
                <div class="more">
                  <WhyChips change={t.change} source={sourceFor(t)} target={t.query} />
                  {#if t.why}<p>{t.why}</p>{/if}
                  {#if t.expect?.note}<p class="expect"><Icon name="target" size={11} />预期：{t.expect.note}</p>{/if}
                  <div class="foot">
                    {#if t.page}<span class="mono url">{t.page}</span>{/if}
                    <span class="grow"></span>
                    {#if p.task_item?.[i]}<button class="link-btn" onclick={() => openSite(cur.id, 'queue')}>在任务队列里看</button>{/if}
                    {#if ['', 'open', 'dismissed'].includes(p.task_status?.[i] ?? '')}<button class="btn tiny quiet" onclick={() => removeTask(i)} data-tip="从计划里删掉；以后 AI 还可能再提类似的">移出计划</button>{/if}
                    {#if p.task_item?.[i] && ['open', 'proposed', 'failed'].includes(p.task_status?.[i] ?? '')}<button class="btn tiny quiet" onclick={() => rejectTask(i)} data-tip="以后不再提这个机会"><Icon name="x" size={11} />驳回</button>{/if}
                  </div>
                </div>
              {/if}
            </li>
          {/each}
        </ol>
      {:else if !showJob}<p class="hint">每周自动制定一次；也可以现在就让 AI 先做一份。</p>{/if}
    </section>
  {:else}<p class="hint">还没有托管的站点。</p>{/if}
</div></div>

<style>
  .pvc { padding: 0; overflow: clip; container-type: inline-size; }
  .pv { display: grid; grid-template-columns: 220px minmax(0, 1fr); }
  /* 分隔线画在整列上（跟右边一样高）；列表本身滚动时贴在顶上 */
  .slist { border-right: 1px solid var(--border); min-width: 0; }
  .sl-in { position: sticky; top: 0; display: flex; flex-direction: column; gap: 2px; padding: 10px 8px; max-height: 80vh; overflow-y: auto; }
  .sq { position: sticky; top: -10px; z-index: 1; display: block; margin: -10px -8px 4px; padding: 10px 8px 6px; background: var(--card); color: var(--faint); }
  .sq :global(svg) { position: absolute; left: 17px; top: 50%; margin-top: -4px; pointer-events: none; }
  .sq input { display: block; width: 100%; height: 28px; padding: 0 8px 0 28px; border: 1px solid var(--border2); border-radius: 7px; background: transparent; font: inherit; font-size: 12.5px; color: var(--text); outline: none; }
  .sq input:focus { border-color: color-mix(in srgb, var(--text) 35%, var(--border2)); }
  .none { margin: 6px 8px; font-size: 12px; color: var(--faint); }
  .site { display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 8px; border: 0; border-radius: 7px; background: none; font: inherit; font-size: 12.5px; color: var(--dim); cursor: pointer; text-align: left; min-width: 0; }
  .site:hover { background: var(--su-hover); color: var(--text); }
  .site.on { background: var(--accent-soft); color: var(--text); font-weight: 550; }
  .site .nm { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .site .pg { flex: none; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; font-weight: 400; }
  .dot { flex: none; width: 6px; height: 6px; border-radius: 50%; }
  .dot.warn { background: var(--warn); }
  .dot.err { background: var(--err); }

  .detail { min-width: 0; padding: 16px 18px; display: flex; flex-direction: column; gap: 12px; }
  .hd { display: flex; align-items: center; gap: 8px; min-width: 0; flex-wrap: wrap; }
  .hd .title { font-size: 14px; font-weight: 600; }
  .hd .sub { font-size: 12px; color: var(--faint); }
  .by { display: inline-flex; align-items: center; gap: 4px; height: 20px; padding: 0 7px; border-radius: 999px; background: var(--accent-soft); font-size: 11px; color: var(--dim); white-space: nowrap; }
  .danger-q:hover:not(:disabled) { color: var(--err); }
  .grow { flex: 1; }
  .stg { flex: none; display: inline-flex; align-items: center; height: 18px; padding: 0 7px; border-radius: 999px; font-size: 10.5px; font-weight: 600; background: var(--su-warn-soft); color: var(--su-warn-ink); }
  .stg.growing { background: var(--su-run-soft); color: var(--su-run); }
  .stg.established { background: var(--su-ok-soft); color: var(--ok); }
  .job { border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; }
  .bar { height: 3px; border-radius: 2px; background: var(--accent-soft); overflow: hidden; margin-top: -4px; }
  .bar i { display: block; height: 100%; background: var(--ok); border-radius: 2px; }

  .box { padding: 9px 12px; border-radius: 9px; background: var(--rail); border: 1px solid var(--border); }
  .box-t { display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: var(--text); min-height: 22px; }
  .box-t .faint { font-weight: 400; font-size: 11.5px; }
  .box-t .fold svg { transition: transform .16s; }
  .box-t .fold.up svg { transform: rotate(180deg); }
  .box p { margin: 3px 0 0; font-size: 12.5px; color: var(--dim); line-height: 1.65; }
  .box p.clamp { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .np { margin: 4px 0 0; padding-left: 20px; font-size: 12.5px; color: var(--dim); line-height: 1.65; display: flex; flex-direction: column; gap: 3px; }

  .tasks { list-style: none; margin: 0; padding: 0; border: 1px solid var(--border); border-radius: 10px; }
  .tasks > li + li { border-top: 1px solid var(--border); }
  .row { width: 100%; display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 6px 12px; border: 0; background: none; font: inherit; color: inherit; text-align: left; cursor: pointer; }
  .row:hover { background: var(--su-hover); }
  .tasks > li:first-child .row { border-radius: 10px 10px 0 0; }
  .tasks > li:last-child:not(.open) .row { border-radius: 0 0 10px 10px; }
  .no { flex: none; width: 20px; height: 20px; border-radius: 50%; background: var(--accent-soft); color: var(--dim); font-size: 11px; font-weight: 600; display: inline-flex; align-items: center; justify-content: center; font-variant-numeric: tabular-nums; }
  .tt { flex: 1; min-width: 0; font-size: 13px; font-weight: 550; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  li.open .tt { white-space: normal; }
  .chev { flex: none; color: var(--faint); transition: transform .16s; }
  .chev.up { transform: rotate(180deg); }
  .row:hover .chev { color: var(--dim); }
  .more { padding: 0 12px 12px 42px; display: flex; flex-direction: column; gap: 6px; }
  .more p { margin: 0; font-size: 12.5px; color: var(--dim); line-height: 1.65; }
  .more .expect { display: flex; align-items: center; gap: 4px; }
  .foot { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .url { min-width: 0; font-size: 11px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .foot .link-btn { flex: none; font-size: 11.5px; }

  /* 窄的时候站点列表改成上面一排 */
  @container (max-width: 640px) {
    .pv { grid-template-columns: minmax(0, 1fr); }
    .slist { border-right: 0; border-bottom: 1px solid var(--border); }
    .sl-in { position: static; flex-direction: row; flex-wrap: wrap; max-height: none; }
    .sq { position: relative; flex-basis: 100%; margin: 0 0 4px; padding: 0; }
    .sq :global(svg) { left: 9px; margin-top: -6.5px; }
    .site { flex: none; }
    .site .nm { flex: none; }
  }
</style>
