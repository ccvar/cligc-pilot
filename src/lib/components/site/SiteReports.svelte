<script lang="ts">
  // 数据页底部：体检 / 周报 / 复盘报告，Pilot 后台任务的运行记录，以及手动跑一次。
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import { mdRender } from '$lib/state/messages.svelte.js';
  import { sites } from '$lib/state/sites.svelte.js';
  import { ops, loadOverview, loadReports, openReport, runTask } from '$lib/state/ops.svelte.js';
  import { managed } from '$lib/state/managed.svelte.js';
  import { clock } from './fmt';

  let { site }: { site: Site } = $props();
  $effect(() => { void loadOverview(site.id); void loadReports(site.id); });
  const ov = $derived(ops.overview[site.id]);
  const reports = $derived(ops.reports[site.id] ?? []);
  const busy = $derived(sites.jobs[site.id]?.status === 'running');
  const inPlan = $derived(!!managed.overview?.groups.some((g) => g.sites.includes(site.id)));
  const TASKS: { key: 'health' | 'report' | 'review' | 'plan'; label: string; sub: string }[] = [
    { key: 'health', label: '站点体检', sub: '构建 + 检查、抓取错误、sitemap' },
    { key: 'report', label: '周报', sub: '数据变化与本周运营记录' },
    { key: 'review', label: '效果复盘', sub: '上线满 4 周的改动前后对比' },
    { key: 'plan', label: '制定本周计划', sub: '托管方案里的站点' },
  ];
  const label = (k: string) => (k === 'scan' ? '机会扫描' : TASKS.find((t) => t.key === k)?.label ?? k);
  const nameOf = (n: string) => n.replace(/^(\d{4}-\d{2}-\d{2})-(health|weekly|review)$/, (_, d, k) => `${d.slice(5).replace('-', '月')}日 ${k === 'health' ? '体检' : k === 'weekly' ? '周报' : '复盘'}`);
</script>

<div class="card">
  <h3><Icon name="doc" />报告与运行记录<span class="grow"></span>
    {#each TASKS.filter((t) => t.key !== 'plan' || inPlan) as t (t.key)}
      <button class="btn tiny" onclick={() => runTask(site.id, t.key)} disabled={busy} data-tip={t.sub}><Icon name="play" size={10} />{t.label}</button>
    {/each}
  </h3>
  {#if !reports.length}<p class="hint">还没有报告。Pilot 每周自动体检、出周报，也可以点上面的按钮现在跑一次。</p>{/if}
  {#if reports.length}
    <div class="seg reports">
      {#each reports.slice(0, 12) as r (r.name)}<button class:on={ops.report?.name === r.name} onclick={() => openReport(site.id, r.name)}>{nameOf(r.name)}</button>{/each}
    </div>
  {/if}
  {#if ops.report && ops.report.site === site.id}<div class="md">{@html mdRender(ops.report.text)}</div>{/if}

  {#if ov?.state.runs.length}
    <div class="runs-t">运行记录</div>
    <table class="tbl">
      <thead><tr><th style="width:130px">时间</th><th style="width:100px">任务</th><th style="width:56px">触发</th><th>结果</th></tr></thead>
      <tbody>
        {#each ov.state.runs.slice(0, 12) as r (r.id)}
          <tr><td>{clock(r.started)}</td><td>{label(r.task)}</td><td>{r.trigger === 'schedule' ? '定时' : '手动'}</td>
            <td class="trunc" title={r.error || r.summary}><span class="pill {r.ok ? 'ok' : 'err'}">{r.ok ? '完成' : '失败'}</span><span class="rs">{r.error || r.summary}</span></td></tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .reports { flex-wrap: wrap; }
  .md { margin-top: 14px; font-size: 13px; line-height: 1.75; border-top: 1px solid var(--border); padding-top: 12px; }
  .md :global(h1) { font-size: 16px; margin: 0 0 8px; }
  .md :global(h2) { font-size: 14px; margin: 16px 0 6px; }
  .md :global(table) { border-collapse: collapse; font-size: 12.5px; }
  .md :global(td), .md :global(th) { border: 1px solid var(--border); padding: 4px 10px; }
  .runs-t { margin: 18px 0 4px; font-size: 12px; font-weight: 600; color: var(--dim); }
  .rs { margin-left: 8px; color: var(--dim); }
</style>
