<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { mdRender } from '$lib/state/messages.svelte.js';
  import { launcher } from '$lib/state/launcher.svelte.js';
  import { openSite } from '$lib/state/sites.svelte.js';
  import { fleet, loadPending } from '$lib/state/fleet.svelte.js';
  import { runTask, sourceOf, CHANGE_LABEL, STATUS_LABEL, STAGE_LABEL } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';
  import { ago, clock } from './fmt';
  import ManagedGroupEditor from './ManagedGroupEditor.svelte';
  import WhyChips from './WhyChips.svelte';
  import { managed, ACTIONS, loadManaged, loadPlans, loadDigests, openDigest, digestNow, saveManaged, setPaused, newGroup, editGroup, setGroupPaused, setSiteActive } from '$lib/state/managed.svelte.js';

  const o = $derived(managed.overview);
  const d = $derived(managed.draft);
  const paused = $derived(!!o?.config.paused);
  const managedIds = $derived(new Set((o?.sites ?? []).map((s) => s.id)));
  const needs = $derived(fleet.pending.filter((p) => managedIds.has(p.site_id)));
  const TABS = [
    { k: 'activity' as const, l: '动态', i: 'activity' },
    { k: 'plans' as const, l: '计划', i: 'checks' },
    { k: 'needs' as const, l: '需要你', i: 'bell' },
    { k: 'digests' as const, l: '日报', i: 'news' },
    { k: 'groups' as const, l: '托管方案', i: 'sliders' },
  ];
  $effect(() => {
    void loadManaged();
    if (managed.tab === 'plans') void loadPlans();
    if (managed.tab === 'needs') void loadPending();
    if (managed.tab === 'digests') void loadDigests();
  });

  // 规则摘要里显示模型名：沿用启动页的「服务 + 模型」选项
  const modelLabel = (b: string, m: string) => launcher.comboOpts.find((x) => x.value === `${b}::${m}`)?.label ?? (m || b);
  const siteName = (id: string) => sitesById.get(id)?.name ?? id;
  const sitesById = $derived(new Map((o?.sites ?? []).map((s) => [s.id, s])));
  const groups = $derived(o?.groups ?? []);
  const ACT_LABEL = Object.fromEntries(ACTIONS.map((a) => [a.key, a.label]));
  // 日报设置（全局）：只比较这两项
  const dirty = $derived(!!d && !!o && (d.digest !== o.config.digest || d.digest_hour !== o.config.digest_hour));

  const KIND: Record<string, { l: string; c: string; i: string }> = {
    shipped: { l: '已上线', c: 'ok', i: 'rocket' }, planned: { l: '计划', c: 'run', i: 'checks' }, proposed: { l: '待你确认', c: 'warn', i: 'bell' }, rolled_back: { l: '已回滚', c: 'err', i: 'undo' },
    failed: { l: '失败', c: 'err', i: 'x' }, watch_ok: { l: '检查通过', c: '', i: 'check' }, watch_failed: { l: '检查失败', c: 'err', i: 'x' }, fallback: { l: '备用模型', c: 'warn', i: 'cpu' },
    budget: { l: '预算', c: 'warn', i: 'gauge' }, digest: { l: '日报', c: '', i: 'news' }, paused: { l: '暂停', c: 'warn', i: 'pause' }, resumed: { l: '恢复', c: 'ok', i: 'play' }, joined: { l: '开始托管', c: 'ok', i: 'plus' }, left: { l: '退出托管', c: '', i: 'x' },
  };
  const when = ago;
  const ratio = (a: number, b: number) => (b > 0 ? Math.min(1, a / b) : 0);
  const kTok = (v: number) => (v >= 1e6 ? `${(v / 1e6).toFixed(v >= 1e7 ? 0 : 1)}M` : v >= 1e3 ? `${Math.round(v / 1e3)}K` : String(v));
  const md = (iso: string) => { const m = /^\d{4}-(\d{2})-(\d{2})$/.exec(iso); return m ? `${+m[1]}月${+m[2]}日` : iso; };
</script>

<ManagedGroupEditor />
<div class="su page mc">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="page-head" data-tauri-drag-region onmousedown={startDrag}>
    <div class="ttl">
      <h1>托管运营 {#if paused}<span class="pill warn">已暂停</span>{/if}</h1>
      <p class="hint">{o ? `托管 ${o.sites.length} 个站点 · 今天 AI 自动上线 ${o.shipped_today} 项 · ${o.waiting} 项需要你` : '读取中…'}</p>
    </div>
    <div class="row">
      {#if o?.sites.length}<button class="btn small ghost" onclick={digestNow}><Icon name="news" size={13} />生成今日日报</button>{/if}
      {#if o}<button class="btn small {paused ? 'primary' : ''}" onclick={() => setPaused(!paused)}><Icon name={paused ? 'play' : 'pause'} size={12} />{paused ? '恢复托管' : '暂停托管'}</button>{/if}
    </div>
  </header>
  <nav class="tabs">
    {#each TABS as t (t.k)}<button class:on={managed.tab === t.k} onclick={() => (managed.tab = t.k)}><Icon name={t.i} size={13} />{t.l}{#if t.k === 'needs' && o?.waiting}<span class="cnt">{o.waiting}</span>{/if}</button>{/each}
  </nav>

  <div class="page-body">
   {#key managed.tab}
   <div class="pane">
    {#if o && !groups.length && managed.tab !== 'digests'}
      <div class="card intro">
        <h3><Icon name="sparkles" />把站点交给 AI 全托管</h3>
        <p class="hint">托管后，AI 每周读数据和代码给每个站点定计划，然后在独立分支里改好、跑构建和检查、自动上线；上线 1 小时后检查页面，效果复盘变差就自动回滚。你只需要处理「需要你」里的例外。</p>
        <ul>
          <li>依赖、构建 / 部署配置、Worker 代码、推荐与邀请相关的内容，永远不会自动改。</li>
          <li>新页面可以自动发布，也可以设成运营人员点头后再发布。</li>
          <li>主模型额度不够时自动换备用模型；每站每天、每周都有上限，随时可以一键暂停。</li>
        </ul>
        <ul>
          <li>可以建多个托管方案：每个方案管一个或多个站点，各有自己的模型、发布方式和上限；方案里的每个站点也能单独停止。</li>
        </ul>
        <div class="row"><button class="btn small primary" onclick={() => newGroup()}><Icon name="plus" size={12} />新增托管方案</button></div>
      </div>
    {:else if managed.tab === 'activity'}
      {#if o?.sites.length}
        <div class="sites">
          {#each o.sites as s (s.id)}
            <button class="sc" onclick={() => openSite(s.id)}>
              <span class="sc-top"><span class="sc-ic"><SiteIcon id={s.id} size={16} /></span><b>{s.name}</b>{#if s.job}<span class="pill run"><span class="spin"></span>{s.job}中</span>{:else if !s.active}<span class="pill">已停止</span>{/if}</span>
              <small>{s.plan_date ? `本周计划 ${s.plan_count} 项 · ${md(s.plan_date)}制定` : '还没有计划'}</small>
              <span class="sc-nums">
                <span><b>{s.shipped_week}</b>本周上线</span>
                <span class:warn={s.waiting > 0}><b>{s.waiting}</b>需要你</span>
                <span class:bad={s.failed > 0}><b>{s.failed}</b>失败</span>
              </span>
              <span class="meter" title="本周改动 {s.budget.changes_used} / {s.budget.changes_limit} 条"><i style="width:{ratio(s.budget.changes_used, s.budget.changes_limit) * 100}%"></i></span>
              <span class="meter-l"><span>改动 {s.budget.changes_used}/{s.budget.changes_limit}</span><span>模型 {kTok(s.budget.tokens_used)}{s.budget.tokens_limit ? ` / ${kTok(s.budget.tokens_limit)}` : ''}</span></span>
            </button>
          {/each}
        </div>
      {/if}
      <div class="card">
        <h3><Icon name="activity" />动态</h3>
        {#if !managed.activity.length}<p class="hint">还没有动态。托管站点的计划、上线、检查、回滚都会记在这里。</p>{/if}
        <div class="list feed">
          {#each managed.activity as a, i (i)}
            <div class="li ev">
              <span class="pill {KIND[a.kind]?.c ?? ''}"><Icon name={KIND[a.kind]?.i ?? 'activity'} size={11} />{KIND[a.kind]?.l ?? a.kind}</span>
              <div class="ev-main">
                <b>{a.title}</b>
                {#if a.change || a.source}<div class="ev-why"><WhyChips change={a.change} source={a.source} /></div>{/if}
                <small>{#if a.site_name}<button class="link-btn plain site-l" onclick={() => openSite(a.site_id)}><SiteIcon id={a.site_id} size={11} />{a.site_name}</button>{/if}{#if a.site_name && a.detail}<span class="sep">·</span>{/if}{a.detail}</small>
              </div>
              <span class="ev-r">
                <span class="faint t" title={clock(a.ts)}>{when(a.ts)}</span>
                {#if a.item_id && a.site_id}<button class="link-btn plain see" onclick={() => openSite(a.site_id, 'queue')}>看详情</button>{/if}
              </span>
            </div>
          {/each}
        </div>
      </div>
    {:else if managed.tab === 'plans'}
      {#each o?.sites ?? [] as s (s.id)}
        {@const p = managed.plans.find((x) => x.site_id === s.id)}
        <div class="card">
          <h3><SiteIcon id={s.id} size={15} />{s.name}<span class="stg {s.stage}">{STAGE_LABEL[s.stage] ?? s.stage}</span><span class="sub">{p ? `${md(p.plan.date)}制定` : '还没有计划'}</span><span class="grow"></span>
            {#if p?.plan.conv_id}<button class="btn tiny" onclick={() => openConv(p.plan.conv_id)}><Icon name="sparkles" size={11} />看 AI 的分析</button>{/if}
            <button class="btn tiny" onclick={() => runTask(s.id, 'plan')} disabled={!!s.job}><Icon name="refresh" size={11} />{p ? '重新制定' : '现在制定'}</button>
          </h3>
          {#if p?.positioning.text}
            <div class="pos">
              <div class="pos-t"><Icon name="target" size={12} />站点定位<span class="faint">{p.positioning.by === 'user' ? '运营人员确认' : 'AI 归纳'}</span><span class="grow"></span><button class="link-btn" onclick={() => openSite(s.id, 'ops')}>修改</button></div>
              <p>{p.positioning.text}</p>
            </div>
          {/if}
          {#if p}
            <ol class="plan">
              {#each p.plan.tasks as t, i (i)}
                <li><span class="no">{i + 1}</span><div><div class="pt"><b>{t.title}</b></div><div class="pw"><WhyChips change={t.change} source={t.source || (t.change === 'new_content' ? 'positioning' : t.change === 'technical' ? 'health' : 'data')} target={t.query} /></div>{#if t.why}<small>{t.why}</small>{/if}{#if t.page}<small class="mono pg">{t.page}</small>{/if}</div></li>
              {/each}
            </ol>
          {:else}<p class="hint">每周自动制定一次；也可以现在就让 AI 先做一份。</p>{/if}
        </div>
      {/each}
    {:else if managed.tab === 'needs'}
      <div class="card">
        <h3><Icon name="bell" />需要你</h3>
        <p class="hint">只放例外：判定不能自动上线的改动（新页面待审、改到推荐 / 邀请内容、超出每日上限）和失败的任务。</p>
        {#if !needs.length}<p class="hint">没有需要你处理的事。</p>{/if}
        <table class="tbl">
          <tbody>
            {#each needs as n (n.item.id)}
              <tr class="clickable" onclick={() => openSite(n.site_id, 'queue')}>
                <td style="width:140px" class="trunc">{n.site_name}</td>
                <td class="trunc">{n.item.opp.title}<div class="nw"><WhyChips change={n.item.opp.change} source={sourceOf(n.item.opp)} kind={n.item.opp.kind} target={n.item.opp.target} /></div><div class="faint" style="font-size:11.5px">{n.item.verdict && n.item.verdict !== 'auto' ? n.item.verdict : n.item.note}</div></td>
                <td style="width:90px"><span class="pill {n.item.status === 'failed' ? 'err' : 'warn'}">{STATUS_LABEL[n.item.status]}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if managed.tab === 'digests'}
      <div class="card">
        <h3><Icon name="news" />日报 <span class="grow"></span><button class="btn tiny" onclick={digestNow}><Icon name="news" size={11} />生成今日日报</button></h3>
        {#if !managed.digests.length}<p class="hint">还没有日报。每天{o?.config.digest_hour ?? 9} 点后自动生成一份，并发系统通知。</p>{/if}
        <div class="seg digs">{#each managed.digests as n (n)}<button class:on={managed.digest?.name === n} onclick={() => openDigest(n)}>{md(n)}</button>{/each}</div>
        {#if managed.digest}<div class="md">{@html mdRender(managed.digest.text)}</div>{/if}
      </div>
      {#if d}
        <div class="card">
          <h3><Icon name="calendar" />日报设置 <span class="sub">所有托管方案共用</span></h3>
          <div class="row">
            <label class="check"><input type="checkbox" bind:checked={d.digest} /><span>每天发一份托管日报（系统通知 + 报告）</span></label>
            {#if d.digest}<span class="faint" style="font-size:12px">时间</span><input class="tin" type="number" min="0" max="23" style="width:70px" bind:value={d.digest_hour} /><span class="faint" style="font-size:12px">点之后</span>{/if}
          </div>
          <p class="hint">托管需要客户端在运行（关掉窗口后托盘常驻即可）；电脑关机期间不会运营，开机后补跑一次。</p>
        </div>
        {#if dirty}<div class="savebar"><span>日报设置有未保存的修改</span><span class="grow"></span><button class="btn small ghost" onclick={() => o && (managed.draft = JSON.parse(JSON.stringify(o.config)))}>撤销</button><button class="btn small primary" onclick={saveManaged} disabled={managed.saving}>保存</button></div>{/if}
      {/if}
    {:else}
      <div class="gbar">
        <p class="hint">每个方案管一个或多个站点，各有自己的规则；一个站点只属于一个方案。方案和站点都能单独暂停。</p>
        <button class="btn small primary" onclick={() => newGroup()}><Icon name="plus" size={12} />新增托管方案</button>
      </div>
      {#each groups as g (g.id)}
        <div class="card grp" class:off={g.paused}>
          <h3><Icon name="sparkles" />{g.name}{#if g.paused}<span class="pill warn">已暂停</span>{/if}<span class="sub">{g.sites.length} 个站点</span><span class="grow"></span>
            <button class="btn tiny" onclick={() => setGroupPaused(g, !g.paused)}><Icon name={g.paused ? 'play' : 'pause'} size={11} />{g.paused ? '恢复方案' : '暂停方案'}</button>
            <button class="btn tiny" onclick={() => editGroup(g)}><Icon name="sliders" size={11} />编辑</button>
          </h3>
          {#if g.goal?.trim()}<p class="goal">{g.goal}</p>{/if}
          <div class="rules">
            <span><Icon name="checks" size={12} />{(g.actions ?? []).length === 5 ? '全部动作' : (g.actions ?? []).map((a) => ACT_LABEL[a] ?? a).join('、')}</span>
            {#if g.guidelines?.trim()}<span><Icon name="doc" size={12} />有内容准则</span>{/if}
            {#if g.topics?.trim()}<span><Icon name="bulb" size={12} />{g.topics.split('\n').filter((l) => l.trim()).length} 个选题</span>{/if}
            <span><Icon name="cpu" size={12} />{modelLabel(g.brain, g.model)}{#if g.fallback_brain}<span class="faint"> · 备用 {modelLabel(g.fallback_brain, g.fallback_model)}</span>{/if}</span>
            <span><Icon name="file" size={12} />新页面{g.new_content === 'auto' ? '自动发布' : '点头后发布'}</span>
            <span><Icon name="rocket" size={12} />每站每天 ≤ {g.daily_changes} 条</span>
            <span><Icon name="gauge" size={12} />每站每周 {g.tokens_per_week ? `${kTok(g.tokens_per_week)} token` : '不限 token'}</span>
          </div>
          {#if g.sites.length}
            <div class="list gsites">
              {#each g.sites as id (id)}
                {@const s = sitesById.get(id)}
                {@const on = !g.stopped.includes(id)}
                <div class="li gs">
                  <button class="gs-name" onclick={() => openSite(id)}><SiteIcon {id} size={15} /><span>{siteName(id)}</span></button>
                  {#if s}<span class="stg {s.stage}" data-tip={s.stage_manual ? '手动指定的阶段' : '按 Search Console 数据自动判断'}>{STAGE_LABEL[s.stage] ?? s.stage}</span>{/if}
                  {#if s}<span class="gs-meta faint">本周上线 {s.shipped_week}{#if s.waiting} · <span class="warn-text">需要你 {s.waiting}</span>{/if}</span>{/if}
                  <span class="grow"></span>
                  <span class="gs-state" class:on={on && !g.paused}>{g.paused ? '方案已暂停' : on ? '托管中' : '已停止'}</span>
                  <button class="switch" class:on class:dim={g.paused} role="switch" aria-checked={on} aria-label="{on ? '停止' : '启用'}托管 {siteName(id)}" data-tip={on ? '停止这个站点的托管（规则保留）' : '恢复这个站点的托管'} onclick={() => setSiteActive(id, !on)}><i></i></button>
                </div>
              {/each}
            </div>
          {:else}<p class="hint">这个方案还没有站点。<button class="link-btn" onclick={() => editGroup(g)}>选择站点</button></p>{/if}
        </div>
      {/each}
    {/if}
   </div>
   {/key}
  </div>
</div>

<style>
  .gbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  .gbar .hint { margin: 0; }
  .grp.off { background: #fcfbf9; }
  .goal { margin: -6px 0 10px; font-size: 12.5px; color: var(--text); line-height: 1.6; max-width: 80ch; }
  .rules { display: flex; flex-wrap: wrap; gap: 6px 16px; font-size: 12px; color: var(--dim); margin: -4px 0 10px; }
  .rules > span { display: inline-flex; align-items: center; gap: 5px; }
  .rules :global(.ico) { color: var(--faint); }
  .gsites { margin-bottom: -8px; }
  .gs { display: flex; align-items: center; gap: 10px; padding-top: 8px; padding-bottom: 8px; }
  .gs-name { display: inline-flex; align-items: center; gap: 8px; border: none; background: none; padding: 0; font: inherit; font-size: 13px; font-weight: 550; color: var(--text); cursor: pointer; min-width: 0; }
  .gs-name span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .gs-name:hover span { text-decoration: underline; text-underline-offset: 3px; text-decoration-color: var(--border2); }
  .stg { flex: none; display: inline-flex; align-items: center; height: 18px; padding: 0 7px; border-radius: 999px; font-size: 10.5px; font-weight: 600; background: var(--su-warn-soft); color: var(--su-warn-ink); }
  .stg.growing { background: var(--su-run-soft); color: var(--su-run); }
  .stg.established { background: var(--su-ok-soft); color: var(--ok); }
  .pos { margin: -2px 0 14px; padding: 10px 12px; border-radius: 10px; background: var(--rail); border: 1px solid var(--border); }
  .pos-t { display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: var(--text); }
  .pos-t .faint { font-weight: 400; font-size: 11.5px; }
  .pos-t .grow { flex: 1; }
  .pos-t .link-btn { font-size: 12px; font-weight: 400; }
  .pos p { margin: 4px 0 0; font-size: 12.5px; color: var(--dim); line-height: 1.65; }
  .gs-meta { font-size: 11.5px; white-space: nowrap; }
  .gs .grow { flex: 1; }
  .gs-state { font-size: 11.5px; color: var(--faint); white-space: nowrap; }
  .gs-state.on { color: var(--ok); }
  .switch { position: relative; flex: none; width: 32px; height: 18px; border: none; border-radius: 999px; background: var(--border2); cursor: pointer; padding: 0; transition: background-color .18s; }
  .switch i { position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; box-shadow: 0 1px 2px rgba(0, 0, 0, .2); transition: transform .18s var(--su-ease); }
  .switch.on { background: var(--ok); }
  .switch.on i { transform: translateX(14px); }
  .switch.dim { opacity: .45; }
  .switch:focus-visible { box-shadow: var(--su-ring); }
  .intro ul { margin: 6px 0 14px; padding-left: 18px; font-size: 12.5px; line-height: 1.8; color: var(--dim); }
  .sites { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; }
  .sc { text-align: left; border: 1px solid var(--border); border-radius: 12px; background: var(--card); padding: 14px 16px; cursor: pointer; font: inherit; color: var(--text); display: flex; flex-direction: column; gap: 4px; transition: border-color .15s, box-shadow .15s, transform .15s var(--su-ease); }
  .sc:hover { border-color: var(--border2); box-shadow: var(--shadow); transform: translateY(-1px); }
  .sc:active { transform: none; box-shadow: var(--shadow-sm); }
  .sc-top { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .sc-ic { color: var(--dim); display: inline-flex; }
  .sc-top b { font-size: 13.5px; font-weight: 600; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sc small { font-size: 11.5px; color: var(--dim); }
  .sc-nums { display: flex; gap: 18px; margin: 8px 0 10px; }
  .sc-nums > span { display: flex; flex-direction: column; font-size: 11px; color: var(--dim); }
  .sc-nums b { font-size: 18px; font-weight: 600; line-height: 1.25; color: var(--text); font-variant-numeric: tabular-nums; }
  .sc-nums .warn b { color: var(--su-warn-ink); }
  .sc-nums .bad b { color: var(--err); }
  .meter { height: 4px; border-radius: 4px; background: var(--accent-soft); overflow: hidden; }
  .meter i { display: block; height: 100%; border-radius: 4px; background: var(--accent); transition: width .4s var(--su-ease); }
  .meter-l { display: flex; justify-content: space-between; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .ev { display: flex; align-items: flex-start; gap: 12px; }
  .ev .pill { flex: none; min-width: 76px; justify-content: flex-start; margin-top: 1px; }
  .ev-main { flex: 1; min-width: 0; }
  .ev-main b { display: block; font-size: 13px; font-weight: 550; line-height: 1.5; }
  .ev-main small { display: block; font-size: 11.5px; color: var(--dim); overflow-wrap: anywhere; line-height: 1.55; }
  .site-l { display: inline-flex; align-items: center; gap: 5px; vertical-align: -1px; }
  .sep { margin: 0 6px; color: var(--border2); }
  .ev .t { flex: none; font-size: 11.5px; white-space: nowrap; }
  .ev-r { flex: none; display: flex; flex-direction: column; align-items: flex-end; gap: 2px; margin-top: 2px; }
  .see { font-size: 11.5px; opacity: 0; transition: opacity .12s; }
  .ev:hover .see { opacity: 1; }
  .ev-why { margin: 3px 0 2px; }
  .pw { margin: 4px 0 2px; }
  .nw { margin: 3px 0; }
  .link-btn.plain { text-decoration: none; color: var(--dim); font-size: inherit; }
  .link-btn.plain:hover { color: var(--text); text-decoration: underline; }
  .plan { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 12px; font-size: 13px; }
  .plan li { display: flex; gap: 10px; align-items: flex-start; }
  .plan .no { flex: none; width: 20px; height: 20px; border-radius: 50%; background: var(--accent-soft); color: var(--dim); font-size: 11px; font-weight: 600; display: inline-flex; align-items: center; justify-content: center; margin-top: 1px; font-variant-numeric: tabular-nums; }
  .plan .pt { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .plan b { font-weight: 550; }
  .plan small { display: block; color: var(--dim); font-size: 12px; margin-top: 2px; }
  .plan small.pg { color: var(--faint); font-size: 11px; }
  .x.mini { width: 16px; height: 16px; font-size: 13px; margin-right: -4px; }
  .digs { flex-wrap: wrap; margin-bottom: 4px; }
  tr.clickable { cursor: pointer; }
  .md { margin-top: 14px; font-size: 13px; line-height: 1.75; border-top: 1px solid var(--border); padding-top: 12px; }
  .md :global(h1) { font-size: 16px; margin: 0 0 8px; }
  .md :global(h2) { font-size: 14px; margin: 16px 0 6px; }
  .md :global(ul) { padding-left: 18px; margin: 6px 0; }
</style>
