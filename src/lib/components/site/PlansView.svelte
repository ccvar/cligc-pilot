<script lang="ts">
  // 托管控制台「计划」页：左边站点列表（带进度），右边一次只看一个站点的计划。
  // 每条计划一行（标题 + 动作 + 在队列里走到哪了），理由、预期、页面点开再看；AI 的说明和站点定位默认只露两行。
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import WhyChips from './WhyChips.svelte';
  import { runTask, STAGE_LABEL } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';
  import { openSite } from '$lib/state/sites.svelte.js';
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
  const cur = $derived(sites.find((s) => s.id === picked) ?? sites.find((s) => progress(planOf(s.id)).tone) ?? sites[0]);
  const p = $derived(cur ? planOf(cur.id) : undefined);
  const prog = $derived(progress(p));

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
  <nav class="list" aria-label="站点">
    {#each sites as s (s.id)}
      {@const sp = planOf(s.id)}
      {@const g = progress(sp)}
      <button class="site" class:on={cur?.id === s.id} onclick={() => (picked = s.id)} aria-current={cur?.id === s.id}>
        <SiteIcon id={s.id} size={15} />
        <span class="nm">{s.name}</span>
        {#if g.tone}<i class="dot {g.tone}" aria-label={g.tone === 'err' ? '有失败' : '有要你处理的'}></i>{/if}
        <span class="pg">{sp ? `${g.done}/${g.total}` : '—'}</span>
      </button>
    {/each}
  </nav>

  {#if cur}
    <section class="detail">
      <header class="hd">
        <SiteIcon id={cur.id} size={16} />
        <b class="title">{cur.name}</b>
        <span class="stg {cur.stage}">{STAGE_LABEL[cur.stage] ?? cur.stage}</span>
        <span class="sub">{p ? `${md(p.plan.date)}制定 · ${prog.done}/${prog.total} 已上线` : '还没有计划'}</span>
        <span class="grow"></span>
        {#if p?.plan.conv_id}<button class="btn tiny" onclick={() => openConv(p.plan.conv_id)}><Icon name="sparkles" size={11} />看 AI 的分析</button>{/if}
        <button class="btn tiny" onclick={() => runTask(cur.id, 'plan')} disabled={!!cur.job}><Icon name="refresh" size={11} />{p ? '重新制定' : '现在制定'}</button>
      </header>
      {#if p && prog.total}<div class="bar" aria-hidden="true"><i style:width="{(prog.done / prog.total) * 100}%"></i></div>{/if}

      {#if p?.plan.notes}
        <div class="box">
          <div class="box-t"><Icon name="info" size={12} />AI 给你的说明<span class="grow"></span><button class="link-btn" onclick={() => (noteOpen = !noteOpen)}>{noteOpen ? '收起' : '展开'}</button></div>
          {#if noteOpen && noteParts.length > 1}<ol class="np">{#each noteParts as x, i (i)}<li>{x}</li>{/each}</ol>
          {:else}<p class:clamp={!noteOpen}>{p.plan.notes}</p>{/if}
        </div>
      {/if}
      {#if p?.positioning.text}
        <div class="box">
          <div class="box-t"><Icon name="target" size={12} />站点定位<span class="faint">{p.positioning.by === 'user' ? '运营人员确认' : 'AI 归纳'}</span><span class="grow"></span>
            <button class="link-btn" onclick={() => (posOpen = !posOpen)}>{posOpen ? '收起' : '展开'}</button>
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
                  </div>
                </div>
              {/if}
            </li>
          {/each}
        </ol>
      {:else}<p class="hint">每周自动制定一次；也可以现在就让 AI 先做一份。</p>{/if}
    </section>
  {:else}<p class="hint">还没有托管的站点。</p>{/if}
</div></div>

<style>
  .pvc { padding: 0; overflow: hidden; container-type: inline-size; }
  .pv { display: grid; grid-template-columns: 220px minmax(0, 1fr); }
  .list { display: flex; flex-direction: column; gap: 2px; padding: 10px 8px; border-right: 1px solid var(--border); max-height: 70vh; overflow-y: auto; }
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
  .grow { flex: 1; }
  .stg { flex: none; display: inline-flex; align-items: center; height: 18px; padding: 0 7px; border-radius: 999px; font-size: 10.5px; font-weight: 600; background: var(--su-warn-soft); color: var(--su-warn-ink); }
  .stg.growing { background: var(--su-run-soft); color: var(--su-run); }
  .stg.established { background: var(--su-ok-soft); color: var(--ok); }
  .bar { height: 3px; border-radius: 2px; background: var(--accent-soft); overflow: hidden; margin-top: -4px; }
  .bar i { display: block; height: 100%; background: var(--ok); border-radius: 2px; }

  .box { padding: 9px 12px; border-radius: 9px; background: var(--rail); border: 1px solid var(--border); }
  .box-t { display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: var(--text); min-height: 22px; }
  .box-t .faint { font-weight: 400; font-size: 11.5px; }
  .box-t .link-btn { font-size: 11.5px; font-weight: 400; color: var(--dim); }
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
    .list { flex-direction: row; flex-wrap: wrap; border-right: 0; border-bottom: 1px solid var(--border); max-height: none; }
    .site { flex: none; }
    .site .nm { flex: none; }
  }
</style>
