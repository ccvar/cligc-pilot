<script lang="ts">
  import './site.css';
  import type { Site } from '$lib/types';
  import Dropdown from '$lib/Dropdown.svelte';
  import { mdRender } from '$lib/state/messages.svelte.js';
  import { app } from '$lib/state/app.svelte.js';
  import { brainLabel, launcherModelOpts } from '$lib/state/models';
  import { sites, editSite, saveSiteDraft } from '$lib/state/sites.svelte.js';
  import { ops, loadOverview, loadReports, openReport, runTask, WHITELIST, LEVEL_LABEL, CHANGE_LABEL } from '$lib/state/ops.svelte.js';

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
  const when = (ts: number) => (ts ? new Date(ts * 1000).toLocaleString() : '—');
  function toggleWl(k: string, on: boolean) {
    if (!d) return;
    d.ops.whitelist = on ? [...new Set([...d.ops.whitelist, k])] : d.ops.whitelist.filter((x) => x !== k);
  }
</script>

{#if d}
  <div class="op">
    <div class="card">
      <h3>自动化档位</h3>
      <div class="levels">
        {#each [1, 2, 3, 4] as lv (lv)}
          <button class="lv" class:on={d.ops.level === lv} onclick={() => (d.ops.level = lv)}>
            <b>{LEVEL_LABEL[lv]}</b>
            <small>{lv === 1 ? '定期拉数据、出报告和机会列表，不动代码' : lv === 2 ? '对每个机会写改动建议，开成会话等你处理' : lv === 3 ? `直接改好、跑检查、${site.source === 'github' ? '提 PR' : '留在本地分支'}，你确认后合并上线` : '白名单内的改动自动合并并上线；其余按第 3 档'}</small>
          </button>
        {/each}
      </div>
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
      <h3>任务与周期</h3>
      <table class="tbl">
        <thead><tr><th>任务</th><th>周期</th><th>上次</th><th>下次</th><th></th></tr></thead>
        <tbody>
          {#each TASKS as t (t.key)}
            <tr>
              <td><b>{t.label}</b><div class="faint" style="font-size:11.5px">{t.sub}</div></td>
              <td style="width:120px"><Dropdown compact bind:value={d.ops[t.key]} options={SCHED} /></td>
              <td>{when(ov?.state.last[t.key] ?? 0)}</td>
              <td>{ov?.next[t.key] ? when(ov.next[t.key]) : '—'}</td>
              <td class="num"><button class="btn tiny" onclick={() => runTask(site.id, t.key)} disabled={busy}>立即运行</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="hint">定时任务需要客户端在运行（关掉窗口后托盘常驻即可）；错过的任务在下次启动时补跑一次。</p>
    </div>

    <div class="card">
      <h3>模型与预算</h3>
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
      {#if ov}<p class="hint">本周已用：改动 {ov.budget.changes_used} / {ov.budget.changes_limit} 条，模型 {ov.budget.tokens_used.toLocaleString()}{ov.budget.tokens_limit ? ` / ${ov.budget.tokens_limit.toLocaleString()}` : ''} token。超出即暂停并通知。</p>{/if}
    </div>

    {#if dirty}
      <div class="row-end"><button class="btn small ghost" onclick={() => editSite(site)}>撤销</button><button class="btn small primary" onclick={saveSiteDraft} disabled={sites.saving}>保存运营设置</button></div>
    {/if}

    <div class="card">
      <h3>报告</h3>
      {#if !reports.length}<p class="hint">还没有报告。运行一次周报或体检就会生成。</p>{/if}
      <div class="reports">
        {#each reports as r (r.name)}<button class="btn tiny" class:primary={ops.report?.name === r.name} onclick={() => openReport(site.id, r.name)}>{r.name}</button>{/each}
      </div>
      {#if ops.report && ops.report.site === site.id}
        <div class="md">{@html mdRender(ops.report.text)}</div>
      {/if}
    </div>

    {#if ov?.state.runs.length}
      <div class="card">
        <h3>运行记录</h3>
        <table class="tbl">
          <thead><tr><th>时间</th><th>任务</th><th>触发</th><th>结果</th></tr></thead>
          <tbody>
            {#each ov.state.runs.slice(0, 20) as r (r.id)}
              <tr><td>{when(r.started)}</td><td>{TASKS.find((t) => t.key === r.task)?.label ?? r.task}</td><td>{r.trigger === 'schedule' ? '定时' : '手动'}</td>
                <td class="trunc" title={r.error || r.summary}><span class="pill {r.ok ? 'ok' : 'err'}">{r.ok ? '完成' : '失败'}</span> {r.error || r.summary}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
{/if}

<style>
  .op { display: flex; flex-direction: column; gap: 14px; }
  .levels { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 8px; }
  .lv { text-align: left; border: 1.5px solid var(--border2); border-radius: 10px; background: #fff; padding: 9px 11px; cursor: pointer; font: inherit; display: flex; flex-direction: column; gap: 3px; }
  .lv b { font-size: 13px; }
  .lv small { color: var(--dim); font-size: 11.5px; line-height: 1.5; }
  .lv.on { border-color: var(--accent); background: var(--accent-soft); }
  .wl { margin-top: 12px; display: flex; flex-direction: column; gap: 6px; }
  .reports { display: flex; flex-wrap: wrap; gap: 6px; }
  .md { margin-top: 12px; font-size: 13px; line-height: 1.7; border-top: 1px solid var(--border); padding-top: 10px; }
  .md :global(table) { border-collapse: collapse; }
  .md :global(td), .md :global(th) { border: 1px solid var(--border); padding: 3px 8px; }
</style>
