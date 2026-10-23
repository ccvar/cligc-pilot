<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import Dropdown from '$lib/Dropdown.svelte';
  import { mdRender } from '$lib/state/messages.svelte.js';
  import { app } from '$lib/state/app.svelte.js';
  import { brainLabel, launcherModelOpts } from '$lib/state/models';
  import { sites, editSite, saveSiteDraft } from '$lib/state/sites.svelte.js';
  import { ops, loadOverview, loadReports, openReport, runTask, WHITELIST, LEVEL_LABEL, CHANGE_LABEL } from '$lib/state/ops.svelte.js';
  import { openManaged, setManagedSites } from '$lib/state/managed.svelte.js';
  import { clock, num } from './fmt';

  let { site }: { site: Site } = $props();
  $effect(() => { if (!sites.draft || sites.draft.id !== site.id) editSite(site); });
  $effect(() => { void loadOverview(site.id); void loadReports(site.id); });
  const d = $derived(sites.draft && sites.draft.id === site.id ? sites.draft : null);
  const ov = $derived(ops.overview[site.id]);
  const reports = $derived(ops.reports[site.id] ?? []);
  const dirty = $derived(!!d && JSON.stringify(d.ops) !== JSON.stringify(site.ops));
  const job = $derived(sites.jobs[site.id]);
  const busy = $derived(job?.status === 'running');
  const brains = $derived(app.ALL_BRAINS.filter((b) => b !== 'codex'));
  const SCHED = [{ value: 'off', label: '关闭' }, { value: 'daily', label: '每天' }, { value: 'weekly', label: '每周' }];
  const TASKS = [
    { key: 'scan' as const, label: '机会扫描', sub: '拉数据、按规则找机会，并按档位处理' },
    { key: 'health' as const, label: '站点体检', sub: '构建 + 检查、抓取错误、sitemap 状态' },
    { key: 'report' as const, label: '周报', sub: '数据变化与本周运营记录' },
    { key: 'review' as const, label: '效果复盘', sub: '上线满 4 周的改动前后对比，持续负面自动降档' },
  ];
  const when = clock;
  const ratio = (a: number, b: number) => (b > 0 ? Math.min(1, a / b) : 0);
  function toggleWl(k: string, on: boolean) {
    if (!d) return;
    d.ops.whitelist = on ? [...new Set([...d.ops.whitelist, k])] : d.ops.whitelist.filter((x) => x !== k);
  }
</script>

{#if d}
  <div class="op">
    <div class="card">
      <h3><Icon name="gauge" />自动化档位</h3>
      <div class="levels">
        {#each [1, 2, 3, 4, 5] as lv (lv)}
          <button class="lv" class:on={d.ops.level === lv} aria-pressed={d.ops.level === lv} onclick={() => (lv === 5 ? void setManagedSites([site.id], true).then(() => { const s = sites.list.find((x) => x.id === site.id); if (s) editSite(s); }) : (d.ops.level = lv))}>
            <span class="lv-top"><span class="lv-n">{lv}</span><b>{(LEVEL_LABEL[lv] ?? '').replace(/^\d+\s*·\s*/, '')}</b>{#if d.ops.level === lv}<svg class="ok-ic" width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}</span>
            <small>{lv === 1 ? '定期拉数据、出报告和机会列表，不动代码' : lv === 2 ? '对每个机会写改动建议，开成会话等你处理' : lv === 3 ? `直接改好、跑检查、${site.source === 'github' ? '提 PR' : '留在本地分支'}，你确认后合并上线` : lv === 4 ? '白名单内的改动自动合并并上线；其余按第 3 档' : 'AI 每周定计划、自动改动上线、上线后检查，变差自动回滚；只把例外交给你'}</small>
          </button>
        {/each}
      </div>
      {#if site.ops.level === 5}
        <p class="hint after">这个站点由「托管运营」统一管理：模型、新页面发布方式、每日上限都在那里设置。<button class="link-btn" onclick={() => openManaged('settings')}>打开托管运营</button></p>
      {/if}
      {#if d.ops.level === 4}
        <div class="wl">
          <span class="lbl">低风险白名单（默认全部不勾；Pilot 按文件差异判定类型，超出边界自动降为待确认）</span>
          {#each WHITELIST as w (w.key)}
            <label class="check"><input type="checkbox" checked={d.ops.whitelist.includes(w.key)} onchange={(e) => toggleWl(w.key, (e.currentTarget as HTMLInputElement).checked)} /><span><b>{w.label}</b> · <span class="dim">{w.sub}</span>{#if d.ops.downgraded.includes(w.key)} <span class="pill warn">复盘负面，已降档</span>{/if}</span></label>
          {/each}
          <p class="hint">正文内容、网址、代码、依赖、部署配置、推荐 / 邀请链接永远不会自动上线。</p>
        </div>
      {/if}
      {#if d.ops.downgraded.length}
        <p class="hint warn-text">以下类型的改动因效果复盘持续负面，在这个站点最高只到第 3 档：{d.ops.downgraded.map((k) => CHANGE_LABEL[k] ?? k).join('、')}
          <button class="link-btn" onclick={() => (d.ops.downgraded = [])}>恢复</button></p>
      {/if}
    </div>

    <div class="card">
      <h3><Icon name="calendar" />任务与周期</h3>
      <table class="tbl">
        <thead><tr><th>任务</th><th>周期</th><th>上次</th><th>下次</th><th></th></tr></thead>
        <tbody>
          {#each TASKS as t (t.key)}
            <tr>
              <td><b>{t.label}</b><div class="faint" style="font-size:11.5px">{t.sub}</div></td>
              <td style="width:120px"><Dropdown compact bind:value={d.ops[t.key]} options={SCHED} /></td>
              <td class="t">{when(ov?.state.last[t.key] ?? 0)}</td>
              <td class="t">{ov?.next[t.key] ? when(ov.next[t.key]) : '—'}</td>
              <td class="act"><button class="btn tiny" onclick={() => runTask(site.id, t.key)} disabled={busy}><Icon name="play" size={11} />立即运行</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="hint">定时任务需要客户端在运行（关掉窗口后托盘常驻即可）；错过的任务在下次启动时补跑一次。</p>
    </div>

    <div class="card">
      <h3><Icon name="cpu" />模型与预算</h3>
      <div class="grid3">
        <div class="field"><span class="lbl">无人值守用的模型服务</span>
          <Dropdown bind:value={d.ops.brain} options={brains.map((b) => ({ value: b, label: brainLabel(b) }))} />
          <small>不能选 Codex：它没有逐条确认的钩子。</small>
        </div>
        <div class="field"><span class="lbl">模型</span>
          <Dropdown bind:value={d.ops.model} options={[{ value: '', label: '默认' }, ...launcherModelOpts(d.ops.brain).map((m) => ({ value: m.value, label: m.label }))]} />
        </div>
        <div class="field"><label for="op-min">机会阈值：展现量下限</label><input id="op-min" class="tin" type="number" min="10" bind:value={d.ops.min_impressions} /><small>小站可以调低（默认 100）。</small></div>
        <div class="field"><label for="op-cw">每周最多改动</label><input id="op-cw" class="tin" type="number" min="0" bind:value={d.ops.budget.changes_per_week} /></div>
        <div class="field"><label for="op-ir">每次最多处理</label><input id="op-ir" class="tin" type="number" min="1" bind:value={d.ops.budget.items_per_run} /></div>
        <div class="field"><label for="op-tw">每周模型用量上限（token，0＝不限）</label><input id="op-tw" class="tin" type="number" min="0" step="100000" bind:value={d.ops.budget.tokens_per_week} /></div>
      </div>
      {#if ov}
        <div class="used">
          <div><span class="ul">本周改动<b>{ov.budget.changes_used} / {ov.budget.changes_limit}</b></span><span class="meter"><i style="width:{ratio(ov.budget.changes_used, ov.budget.changes_limit) * 100}%"></i></span></div>
          <div><span class="ul">本周模型用量<b>{num(ov.budget.tokens_used)}{ov.budget.tokens_limit ? ` / ${num(ov.budget.tokens_limit)}` : ''}</b></span><span class="meter"><i style="width:{ratio(ov.budget.tokens_used, ov.budget.tokens_limit) * 100}%"></i></span></div>
        </div>
        <p class="hint">超出任一上限即暂停并通知。</p>
      {/if}
    </div>

    {#if dirty}
      <div class="savebar"><span>运营设置有未保存的修改</span><span class="grow"></span><button class="btn small ghost" onclick={() => editSite(site)}>撤销</button><button class="btn small primary" onclick={saveSiteDraft} disabled={sites.saving}>保存</button></div>
    {/if}

    <div class="card">
      <h3><Icon name="doc" />报告</h3>
      {#if !reports.length}<p class="hint">还没有报告。运行一次周报或体检就会生成。</p>{/if}
      {#if reports.length}
        <div class="seg reports">
          {#each reports as r (r.name)}<button class:on={ops.report?.name === r.name} onclick={() => openReport(site.id, r.name)}>{r.name}</button>{/each}
        </div>
      {/if}
      {#if ops.report && ops.report.site === site.id}
        <div class="md">{@html mdRender(ops.report.text)}</div>
      {/if}
    </div>

    {#if ov?.state.runs.length}
      <div class="card">
        <h3><Icon name="history" />运行记录</h3>
        <table class="tbl">
          <thead><tr><th style="width:130px">时间</th><th style="width:90px">任务</th><th style="width:56px">触发</th><th>结果</th></tr></thead>
          <tbody>
            {#each ov.state.runs.slice(0, 20) as r (r.id)}
              <tr><td>{when(r.started)}</td><td>{TASKS.find((t) => t.key === r.task)?.label ?? r.task}</td><td>{r.trigger === 'schedule' ? '定时' : '手动'}</td>
                <td class="trunc" title={r.error || r.summary}><span class="pill {r.ok ? 'ok' : 'err'}">{r.ok ? '完成' : '失败'}</span><span class="rs">{r.error || r.summary}</span></td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
{/if}

<style>
  .op { display: flex; flex-direction: column; gap: 14px; }
  .levels { display: grid; grid-template-columns: repeat(auto-fit, minmax(136px, 1fr)); gap: 8px; }
  .lv { text-align: left; border: 1px solid var(--border2); border-radius: 10px; background: #fff; padding: 10px 12px 11px; cursor: pointer; font: inherit; color: var(--text); display: flex; flex-direction: column; gap: 5px; transition: border-color .15s, background-color .15s, box-shadow .15s; }
  .lv:hover:not(.on) { border-color: #cfcbc1; background: #fdfcfa; }
  .lv:focus-visible { box-shadow: var(--su-ring); }
  .lv-top { display: flex; align-items: center; gap: 7px; }
  .lv-n { width: 18px; height: 18px; border-radius: 5px; background: var(--accent-soft); color: var(--dim); font-size: 11px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; flex: none; font-variant-numeric: tabular-nums; transition: background-color .15s, color .15s; }
  .lv b { font-size: 13px; font-weight: 600; flex: 1; min-width: 0; }
  .lv small { color: var(--dim); font-size: 11.5px; line-height: 1.55; }
  .lv.on { border-color: var(--accent); box-shadow: inset 0 0 0 .5px var(--accent); background: #fcfbf8; }
  .lv.on .lv-n { background: var(--accent); color: #fff; }
  .ok-ic { color: var(--accent); flex: none; }
  .after { margin-top: 12px; }
  .wl { margin-top: 14px; display: flex; flex-direction: column; gap: 8px; }
  .wl .lbl { font-size: 12px; font-weight: 550; }
  .t { white-space: nowrap; color: var(--dim); }
  .used { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 10px 24px; margin-top: 16px; }
  .ul { display: flex; justify-content: space-between; font-size: 12px; color: var(--dim); margin-bottom: 5px; }
  .ul b { font-weight: 600; color: var(--text); font-variant-numeric: tabular-nums; }
  .meter { display: block; height: 4px; border-radius: 4px; background: var(--accent-soft); overflow: hidden; }
  .meter i { display: block; height: 100%; border-radius: 4px; background: var(--accent); transition: width .4s var(--su-ease); }
  .reports { flex-wrap: wrap; }
  .rs { margin-left: 8px; color: var(--dim); }
  .md { margin-top: 14px; font-size: 13px; line-height: 1.75; border-top: 1px solid var(--border); padding-top: 12px; }
  .md :global(h1) { font-size: 16px; margin: 0 0 8px; }
  .md :global(h2) { font-size: 14px; margin: 16px 0 6px; }
  .md :global(table) { border-collapse: collapse; font-size: 12.5px; }
  .md :global(td), .md :global(th) { border: 1px solid var(--border); padding: 4px 10px; }
  .md :global(th) { background: var(--rail); font-weight: 550; }
</style>
