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
  const when = (ts: number) => { const s = Date.now() / 1000 - ts; return s < 3600 ? `${Math.max(1, Math.round(s / 60))} 分钟前` : s < 86400 ? `${Math.round(s / 3600)} 小时前` : new Date(ts * 1000).toLocaleDateString(); };
</script>

<div class="su mc">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="mc-head" data-tauri-drag-region onmousedown={startDrag}>
    <div>
      <h1>托管运营 {#if paused}<span class="pill warn">已暂停</span>{/if}</h1>
      <p class="hint">{o ? `托管 ${o.sites.length} 个站点 · 今天 AI 自动上线 ${o.shipped_today} 项 · ${o.waiting} 项需要你` : '读取中…'}</p>
    </div>
    <div class="row">
      {#if o?.sites.length}<button class="btn small ghost" onclick={digestNow}>生成今日日报</button>{/if}
      {#if o}<button class="btn small {paused ? 'primary' : ''}" onclick={() => setPaused(!paused)}>{paused ? '恢复托管' : '暂停托管'}</button>{/if}
    </div>
  </header>
  <nav class="mc-tabs">
    {#each TABS as t (t.k)}<button class:on={managed.tab === t.k} onclick={() => (managed.tab = t.k)}>{t.l}{#if t.k === 'needs' && o?.waiting}<span class="cnt">{o.waiting}</span>{/if}</button>{/each}
  </nav>

  <div class="mc-body">
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
              <b>{s.name}</b>
              <small>{s.job ? `${s.job}中` : s.plan_date ? `计划 ${s.plan_count} 项（${s.plan_date}）` : '还没有计划'}</small>
              <span class="row">
                <span class="pill ok">本周上线 {s.shipped_week}</span>
                {#if s.waiting}<span class="pill warn">需要你 {s.waiting}</span>{/if}
                {#if s.failed}<span class="pill err">失败 {s.failed}</span>{/if}
              </span>
            </button>
          {/each}
        </div>
      {/if}
      <div class="card">
        <h3>动态</h3>
        {#if !managed.activity.length}<p class="hint">还没有动态。托管站点的计划、上线、检查、回滚都会记在这里。</p>{/if}
        <div class="feed">
          {#each managed.activity as a, i (i)}
            <div class="ev">
              <span class="pill {KIND[a.kind]?.c ?? ''}">{KIND[a.kind]?.l ?? a.kind}</span>
              <div class="ev-main">
                <b>{#if a.site_name}<button class="link-btn plain" onclick={() => openSite(a.site_id)}>{a.site_name}</button> · {/if}{a.title}</b>
                {#if a.detail}<small>{a.detail}</small>{/if}
              </div>
              <span class="faint t">{when(a.ts)}</span>
            </div>
          {/each}
        </div>
      </div>
    {:else if managed.tab === 'plans'}
      {#each o?.sites ?? [] as s (s.id)}
        {@const p = managed.plans.find((x) => x.site_id === s.id)}
        <div class="card">
          <h3>{s.name} <span class="faint" style="font-weight:400;font-size:12px">{p ? `${p.plan.date} 制定` : '还没有计划'}</span><span class="grow"></span>
            {#if p?.plan.conv_id}<button class="btn tiny" onclick={() => openConv(p.plan.conv_id)}>看 AI 的分析</button>{/if}
            <button class="btn tiny" onclick={() => runTask(s.id, 'plan')} disabled={!!s.job}>{p ? '重新制定' : '现在制定'}</button>
          </h3>
          {#if p}
            <ol class="plan">
              {#each p.plan.tasks as t, i (i)}
                <li><b>{t.title}</b> <span class="pill">{CHANGE_LABEL[t.change] ?? t.change}</span>{#if t.page}<small class="mono">{t.page}</small>{/if}{#if t.why}<small>{t.why}</small>{/if}</li>
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
        <div class="row wrap">{#each managed.digests as n (n)}<button class="btn tiny" class:primary={managed.digest?.name === n} onclick={() => openDigest(n)}>{n}</button>{/each}</div>
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
              <label class="check"><input type="checkbox" checked={picked.has(r.id)} onchange={() => toggle(r.id)} /><span>{r.name}{#if !r.has_data} <span class="faint">· 未接数据</span>{/if}</span></label>
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

      {#if dirty}<div class="row-end"><button class="btn small ghost" onclick={() => o && (managed.draft = JSON.parse(JSON.stringify(o.config)))}>撤销</button><button class="btn small primary" onclick={saveManaged} disabled={managed.saving}>保存托管设置</button></div>{/if}
    {/if}
  </div>
</div>

<style>
  .mc { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .mc-head { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; padding: 34px 28px 8px; }
  .mc-head h1 { margin: 0; font-size: 20px; display: flex; align-items: center; gap: 8px; }
  .mc-head .hint { margin: 2px 0 0; }
  .mc-tabs { display: flex; gap: 2px; padding: 0 22px; border-bottom: 1px solid var(--border); }
  .mc-tabs button { position: relative; border: none; background: none; padding: 8px 10px 9px; font: inherit; font-size: 13px; color: var(--dim); cursor: pointer; }
  .mc-tabs button.on { color: var(--text); font-weight: 600; }
  .mc-tabs button.on::after { content: ''; position: absolute; left: 8px; right: 8px; bottom: -1px; height: 2px; border-radius: 2px; background: var(--accent); }
  .cnt { margin-left: 5px; display: inline-flex; min-width: 16px; height: 16px; padding: 0 4px; box-sizing: border-box; align-items: center; justify-content: center; border-radius: 999px; background: var(--accent); color: #fff; font-size: 10px; font-weight: 700; }
  .mc-body { flex: 1; min-height: 0; overflow-y: auto; padding: 16px 28px 40px; display: flex; flex-direction: column; gap: 14px; }
  .mc-body > :global(*) { max-width: 1080px; }
  .intro ul { margin: 6px 0 12px; padding-left: 18px; font-size: 12.5px; line-height: 1.8; color: var(--dim); }
  .sites { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 10px; }
  .sc { text-align: left; border: 1px solid var(--border); border-radius: 12px; background: var(--card); padding: 10px 12px; cursor: pointer; font: inherit; display: flex; flex-direction: column; gap: 4px; }
  .sc:hover { border-color: var(--border2); background: #faf9f6; }
  .sc b { font-size: 13px; }
  .sc small { font-size: 11.5px; color: var(--dim); }
  .feed { display: flex; flex-direction: column; }
  .ev { display: flex; align-items: flex-start; gap: 10px; padding: 7px 0; border-top: 1px solid var(--border); }
  .ev:first-child { border-top: none; }
  .ev .pill { flex: none; margin-top: 1px; }
  .ev-main { flex: 1; min-width: 0; }
  .ev-main b { display: block; font-size: 12.5px; font-weight: 550; }
  .ev-main small { display: block; font-size: 11.5px; color: var(--dim); overflow-wrap: anywhere; }
  .ev .t { flex: none; font-size: 11px; }
  .link-btn.plain { text-decoration: none; color: var(--text); font-weight: 600; }
  .plan { margin: 0; padding-left: 20px; display: flex; flex-direction: column; gap: 8px; font-size: 12.5px; }
  .plan small { display: block; color: var(--dim); font-size: 11.5px; }
  .pick { max-height: 220px; overflow-y: auto; border: 1px solid var(--border); border-radius: 9px; padding: 6px 10px; display: flex; flex-direction: column; gap: 4px; margin: 6px 0; }
  .x.mini { font-size: 13px; padding: 0 0 0 3px; }
  tr.clickable { cursor: pointer; }
  tr.clickable:hover td { background: #faf9f6; }
  .md { margin-top: 12px; font-size: 13px; line-height: 1.7; border-top: 1px solid var(--border); padding-top: 10px; }
</style>
