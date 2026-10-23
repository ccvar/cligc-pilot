<script lang="ts">
  import './site.css';
  import Dropdown from '$lib/Dropdown.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { mdRender } from '$lib/state/messages.svelte.js';
  import { launcher } from '$lib/state/launcher.svelte.js';
  import { openSite } from '$lib/state/sites.svelte.js';
  import { fleet, loadPending } from '$lib/state/fleet.svelte.js';
  import { runTask, CHANGE_LABEL, STATUS_LABEL } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';
  import { ago, clock } from './fmt';
  import { managed, loadManaged, loadPlans, loadDigests, openDigest, digestNow, saveManaged, setPaused, setManagedSites } from '$lib/state/managed.svelte.js';

  const o = $derived(managed.overview);
  const d = $derived(managed.draft);
  const paused = $derived(!!o?.config.paused);
  const managedIds = $derived(new Set((o?.sites ?? []).map((s) => s.id)));
  const needs = $derived(fleet.pending.filter((p) => managedIds.has(p.site_id)));
  const TABS = [
    { k: 'activity' as const, l: '动态' },
    { k: 'plans' as const, l: '计划' },
    { k: 'needs' as const, l: '需要你' },
    { k: 'digests' as const, l: '日报' },
    { k: 'settings' as const, l: '设置' },
  ];
  $effect(() => {
    void loadManaged();
    if (managed.tab === 'plans') void loadPlans();
    if (managed.tab === 'needs') void loadPending();
    if (managed.tab === 'digests') void loadDigests();
  });

  // 模型选择：沿用启动页的「服务 + 模型」选项，去掉 Codex（无人值守不能用）。
  const modelOpts = $derived(launcher.comboOpts.filter((x) => !x.value.startsWith('codex::')).map((x) => ({ value: x.value, label: x.label, sub: x.sub, icon: x.icon, disabled: x.disabled })));
  const primary = $derived(d ? `${d.brain}::${d.model}` : '');
  const fallback = $derived(d && d.fallback_brain ? `${d.fallback_brain}::${d.fallback_model}` : '');
  function pick(v: string, which: 'primary' | 'fallback') {
    if (!d) return;
    const i = v.indexOf('::');
    const [b, m] = i < 0 ? ['', ''] : [v.slice(0, i), v.slice(i + 2)];
    if (which === 'primary') { d.brain = b; d.model = m; } else { d.fallback_brain = b; d.fallback_model = m; }
  }
  const dirty = $derived(!!d && !!o && JSON.stringify({ ...d, last_digest: '' }) !== JSON.stringify({ ...o.config, last_digest: '' }));

  let siteQ = $state('');
  let picked = $state(new Set<string>());
  const candidates = $derived(fleet.rows.filter((r) => !managedIds.has(r.id) && (!siteQ.trim() || `${r.name} ${r.domain} ${r.tags.join(' ')}`.toLowerCase().includes(siteQ.trim().toLowerCase()))));
  function toggle(id: string) { const n = new Set(picked); if (n.has(id)) n.delete(id); else n.add(id); picked = n; }

  const KIND: Record<string, { l: string; c: string }> = {
    shipped: { l: '已上线', c: 'ok' }, planned: { l: '计划', c: 'run' }, proposed: { l: '待你确认', c: 'warn' }, rolled_back: { l: '已回滚', c: 'err' },
    failed: { l: '失败', c: 'err' }, watch_ok: { l: '检查通过', c: '' }, watch_failed: { l: '检查失败', c: 'err' }, fallback: { l: '备用模型', c: 'warn' },
    budget: { l: '预算', c: 'warn' }, digest: { l: '日报', c: '' }, paused: { l: '暂停', c: 'warn' }, resumed: { l: '恢复', c: 'ok' }, joined: { l: '开始托管', c: 'ok' }, left: { l: '退出托管', c: '' },
  };
  const when = ago;
  const ratio = (a: number, b: number) => (b > 0 ? Math.min(1, a / b) : 0);
  const kTok = (v: number) => (v >= 1e6 ? `${(v / 1e6).toFixed(v >= 1e7 ? 0 : 1)}M` : v >= 1e3 ? `${Math.round(v / 1e3)}K` : String(v));
  const md = (iso: string) => { const m = /^\d{4}-(\d{2})-(\d{2})$/.exec(iso); return m ? `${+m[1]}月${+m[2]}日` : iso; };
</script>

<div class="su page mc">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="page-head" data-tauri-drag-region onmousedown={startDrag}>
    <div class="ttl">
      <h1>托管运营 {#if paused}<span class="pill warn">已暂停</span>{/if}</h1>
      <p class="hint">{o ? `托管 ${o.sites.length} 个站点 · 今天 AI 自动上线 ${o.shipped_today} 项 · ${o.waiting} 项需要你` : '读取中…'}</p>
    </div>
    <div class="row">
      {#if o?.sites.length}<button class="btn small ghost" onclick={digestNow}>生成今日日报</button>{/if}
      {#if o}<button class="btn small {paused ? 'primary' : ''}" onclick={() => setPaused(!paused)}>{paused ? '恢复托管' : '暂停托管'}</button>{/if}
    </div>
  </header>
  <nav class="tabs">
    {#each TABS as t (t.k)}<button class:on={managed.tab === t.k} onclick={() => (managed.tab = t.k)}>{t.l}{#if t.k === 'needs' && o?.waiting}<span class="cnt">{o.waiting}</span>{/if}</button>{/each}
  </nav>

  <div class="page-body">
   {#key managed.tab}
   <div class="pane">
    {#if o && !o.sites.length && managed.tab !== 'settings'}
      <div class="card intro">
        <h3>把站点交给 AI 全托管</h3>
        <p class="hint">托管后，AI 每周读数据和代码给每个站点定计划，然后在独立分支里改好、跑构建和检查、自动上线；上线 1 小时后检查页面，效果复盘变差就自动回滚。你只需要处理「需要你」里的例外。</p>
        <ul>
          <li>依赖、构建 / 部署配置、Worker 代码、推荐与邀请相关的内容，永远不会自动改。</li>
          <li>新页面可以自动发布，也可以设成运营人员点头后再发布。</li>
          <li>主模型额度不够时自动换备用模型；每站每天、每周都有上限，随时可以一键暂停。</li>
        </ul>
        <div class="row"><button class="btn small primary" onclick={() => (managed.tab = 'settings')}>选择要托管的站点</button></div>
      </div>
    {:else if managed.tab === 'activity'}
      {#if o?.sites.length}
        <div class="sites">
          {#each o.sites as s (s.id)}
            <button class="sc" onclick={() => openSite(s.id)}>
              <span class="sc-top"><b>{s.name}</b>{#if s.job}<span class="pill run"><span class="spin"></span>{s.job}中</span>{/if}</span>
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
        <h3>动态</h3>
        {#if !managed.activity.length}<p class="hint">还没有动态。托管站点的计划、上线、检查、回滚都会记在这里。</p>{/if}
        <div class="list feed">
          {#each managed.activity as a, i (i)}
            <div class="li ev">
              <span class="pill {KIND[a.kind]?.c ?? ''}">{KIND[a.kind]?.l ?? a.kind}</span>
              <div class="ev-main">
                <b>{a.title}</b>
                <small>{#if a.site_name}<button class="link-btn plain" onclick={() => openSite(a.site_id)}>{a.site_name}</button>{/if}{#if a.site_name && a.detail}<span class="sep">·</span>{/if}{a.detail}</small>
              </div>
              <span class="faint t" title={clock(a.ts)}>{when(a.ts)}</span>
            </div>
          {/each}
        </div>
      </div>
    {:else if managed.tab === 'plans'}
      {#each o?.sites ?? [] as s (s.id)}
        {@const p = managed.plans.find((x) => x.site_id === s.id)}
        <div class="card">
          <h3>{s.name} <span class="sub">{p ? `${md(p.plan.date)}制定` : '还没有计划'}</span><span class="grow"></span>
            {#if p?.plan.conv_id}<button class="btn tiny" onclick={() => openConv(p.plan.conv_id)}>看 AI 的分析</button>{/if}
            <button class="btn tiny" onclick={() => runTask(s.id, 'plan')} disabled={!!s.job}>{p ? '重新制定' : '现在制定'}</button>
          </h3>
          {#if p}
            <ol class="plan">
              {#each p.plan.tasks as t, i (i)}
                <li><span class="no">{i + 1}</span><div><div class="pt"><b>{t.title}</b><span class="pill">{CHANGE_LABEL[t.change] ?? t.change}</span></div>{#if t.why}<small>{t.why}</small>{/if}{#if t.page}<small class="mono pg">{t.page}</small>{/if}</div></li>
              {/each}
            </ol>
          {:else}<p class="hint">每周自动制定一次；也可以现在就让 AI 先做一份。</p>{/if}
        </div>
      {/each}
    {:else if managed.tab === 'needs'}
      <div class="card">
        <h3>需要你</h3>
        <p class="hint">只放例外：判定不能自动上线的改动（新页面待审、改到推荐 / 邀请内容、超出每日上限）和失败的任务。</p>
        {#if !needs.length}<p class="hint">没有需要你处理的事。</p>{/if}
        <table class="tbl">
          <tbody>
            {#each needs as n (n.item.id)}
              <tr class="clickable" onclick={() => openSite(n.site_id, 'queue')}>
                <td style="width:140px" class="trunc">{n.site_name}</td>
                <td class="trunc">{n.item.opp.title}<div class="faint" style="font-size:11.5px">{n.item.verdict && n.item.verdict !== 'auto' ? n.item.verdict : n.item.note}</div></td>
                <td style="width:90px"><span class="pill {n.item.status === 'failed' ? 'err' : 'warn'}">{STATUS_LABEL[n.item.status]}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if managed.tab === 'digests'}
      <div class="card">
        <h3>日报 <span class="grow"></span><button class="btn tiny" onclick={digestNow}>生成今日日报</button></h3>
        {#if !managed.digests.length}<p class="hint">还没有日报。每天{o?.config.digest_hour ?? 9} 点后自动生成一份，并发系统通知。</p>{/if}
        <div class="seg digs">{#each managed.digests as n (n)}<button class:on={managed.digest?.name === n} onclick={() => openDigest(n)}>{md(n)}</button>{/each}</div>
        {#if managed.digest}<div class="md">{@html mdRender(managed.digest.text)}</div>{/if}
      </div>
    {:else if d}
      <div class="card">
        <h3>托管的站点</h3>
        {#if o?.sites.length}
          <div class="row wrap">
            {#each o.sites as s (s.id)}<span class="pill">{s.name}<button class="x mini" title="退出托管" onclick={() => setManagedSites([s.id], false)}>×</button></span>{/each}
          </div>
        {/if}
        <div class="field" style="margin-top:10px">
          <span class="lbl">加入托管</span>
          <input class="tin" bind:value={siteQ} placeholder={`在 ${candidates.length} 个站点里搜索`} spellcheck="false" />
          <div class="pick">
            {#each candidates.slice(0, 200) as r (r.id)}
              <label class="check"><input type="checkbox" checked={picked.has(r.id)} onchange={() => toggle(r.id)} /><span>{r.name}{#if !r.has_data}<span class="faint nd">未接数据</span>{/if}</span></label>
            {:else}<p class="hint">没有可加入的站点。</p>{/each}
          </div>
          <div class="row"><button class="btn small primary" onclick={async () => { await setManagedSites([...picked], true); picked = new Set(); }} disabled={!picked.size}>托管选中的 {picked.size} 个站点</button>
            <button class="link-btn" onclick={() => (picked = new Set(candidates.map((c) => c.id)))}>全选</button></div>
        </div>
      </div>

      <div class="card">
        <h3>发布规则</h3>
        <div class="field"><span class="lbl">新页面（整篇新内容）</span>
          <div class="seg"><button class:on={d.new_content === 'auto'} onclick={() => (d.new_content = 'auto')}>自动发布</button><button class:on={d.new_content === 'review'} onclick={() => (d.new_content = 'review')}>运营人员点头后发布</button></div>
          <small>标题、描述、内链和已有页面的内容改动，检查通过就自动上线。依赖、配置、代码、推荐 / 邀请相关内容、超过 400 行的改动永远不自动上线。</small>
        </div>
        <div class="grid2" style="margin-top:10px">
          <div class="field"><label for="mc-daily">每个站点每天最多自动上线</label><input id="mc-daily" class="tin" type="number" min="0" max="50" bind:value={d.daily_changes} /></div>
          <div class="field"><label for="mc-tok">每个站点每周模型用量上限（token，0＝不限）</label><input id="mc-tok" class="tin" type="number" min="0" step="100000" bind:value={d.tokens_per_week} /></div>
        </div>
      </div>

      <div class="card">
        <h3>模型</h3>
        <div class="grid2">
          <div class="field"><span class="lbl">主模型</span><Dropdown bind:value={() => primary, (v) => pick(v, 'primary')} options={modelOpts} /></div>
          <div class="field"><span class="lbl">备用模型</span><Dropdown bind:value={() => fallback, (v) => pick(v, 'fallback')} options={[{ value: '', label: '不用备用模型' }, ...modelOpts]} />
            <small>主模型这一轮失败（额度用完、限流、不可用）时自动换它重跑一次，动态里会记一笔。</small></div>
        </div>
        <p class="hint">Codex 不能用于托管：它没有逐条确认的钩子。</p>
      </div>

      <div class="card">
        <h3>日报</h3>
        <div class="row">
          <label class="check"><input type="checkbox" bind:checked={d.digest} /><span>每天发一份托管日报（系统通知 + 报告）</span></label>
          {#if d.digest}<span class="faint" style="font-size:12px">时间</span><input class="tin" type="number" min="0" max="23" style="width:70px" bind:value={d.digest_hour} /><span class="faint" style="font-size:12px">点之后</span>{/if}
        </div>
        <p class="hint">托管需要客户端在运行（关掉窗口后托盘常驻即可）；电脑关机期间不会运营，开机后补跑一次。</p>
      </div>

      {#if dirty}<div class="savebar"><span>有未保存的修改</span><span class="grow"></span><button class="btn small ghost" onclick={() => o && (managed.draft = JSON.parse(JSON.stringify(o.config)))}>撤销</button><button class="btn small primary" onclick={saveManaged} disabled={managed.saving}>保存托管设置</button></div>{/if}
    {/if}
   </div>
   {/key}
  </div>
</div>

<style>
  .intro ul { margin: 6px 0 14px; padding-left: 18px; font-size: 12.5px; line-height: 1.8; color: var(--dim); }
  .sites { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; }
  .sc { text-align: left; border: 1px solid var(--border); border-radius: 12px; background: var(--card); padding: 14px 16px; cursor: pointer; font: inherit; color: var(--text); display: flex; flex-direction: column; gap: 4px; transition: border-color .15s, box-shadow .15s, transform .15s var(--su-ease); }
  .sc:hover { border-color: var(--border2); box-shadow: var(--shadow); transform: translateY(-1px); }
  .sc:active { transform: none; box-shadow: var(--shadow-sm); }
  .sc-top { display: flex; align-items: center; gap: 8px; min-width: 0; }
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
  .ev .pill { flex: none; min-width: 64px; justify-content: center; margin-top: 1px; }
  .ev-main { flex: 1; min-width: 0; }
  .ev-main b { display: block; font-size: 13px; font-weight: 550; line-height: 1.5; }
  .ev-main small { display: block; font-size: 11.5px; color: var(--dim); overflow-wrap: anywhere; line-height: 1.55; }
  .sep { margin: 0 6px; color: var(--border2); }
  .ev .t { flex: none; font-size: 11.5px; white-space: nowrap; margin-top: 2px; }
  .link-btn.plain { text-decoration: none; color: var(--dim); font-size: inherit; }
  .link-btn.plain:hover { color: var(--text); text-decoration: underline; }
  .plan { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 12px; font-size: 13px; }
  .plan li { display: flex; gap: 10px; align-items: flex-start; }
  .plan .no { flex: none; width: 20px; height: 20px; border-radius: 50%; background: var(--accent-soft); color: var(--dim); font-size: 11px; font-weight: 600; display: inline-flex; align-items: center; justify-content: center; margin-top: 1px; font-variant-numeric: tabular-nums; }
  .plan .pt { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .plan b { font-weight: 550; }
  .plan small { display: block; color: var(--dim); font-size: 12px; margin-top: 2px; }
  .plan small.pg { color: var(--faint); font-size: 11px; }
  .pick { max-height: 220px; overflow-y: auto; border: 1px solid var(--border); border-radius: 9px; padding: 8px 10px; display: flex; flex-direction: column; gap: 6px; margin: 6px 0; }
  .nd { margin-left: 8px; font-size: 11.5px; }
  .pick .check { padding: 2px 4px; border-radius: 6px; transition: background-color .12s; }
  .pick .check:hover { background: var(--su-hover); }
  .x.mini { width: 16px; height: 16px; font-size: 13px; margin-right: -4px; }
  .digs { flex-wrap: wrap; margin-bottom: 4px; }
  tr.clickable { cursor: pointer; }
  .md { margin-top: 14px; font-size: 13px; line-height: 1.75; border-top: 1px solid var(--border); padding-top: 12px; }
  .md :global(h1) { font-size: 16px; margin: 0 0 8px; }
  .md :global(h2) { font-size: 14px; margin: 16px 0 6px; }
  .md :global(ul) { padding-left: 18px; margin: 6px 0; }
</style>
