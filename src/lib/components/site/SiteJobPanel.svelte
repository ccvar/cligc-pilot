<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import type { SiteJob } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { cancelSiteJob } from '$lib/state/sites.svelte.js';

  // bare：外层卡片已经有同名标题时不再重复显示作业名。
  let { job, compact = false, bare = false }: { job: SiteJob; compact?: boolean; bare?: boolean } = $props();
  let showLog = $state(false);
  let logEl: HTMLPreElement | undefined = $state();
  const mark = (s: string) => (s === 'ok' ? '✓' : s === 'failed' ? '!' : '');
  const elapsed = $derived(job.finished_at ? job.finished_at - job.started_at : 0);
  // 步骤条下面只说一件事：正在做的那一步，或失败的那一步。
  const focus = $derived(job.steps.find((s) => s.status === 'failed') ?? job.steps.find((s) => s.status === 'running'));
  $effect(() => { void job.log.length; if (showLog && logEl) logEl.scrollTop = logEl.scrollHeight; });
  $effect(() => { if (job.status === 'failed') showLog = true; });
</script>

<div class="su job">
  <div class="row job-head">
    {#if !bare}<b><Icon name={job.kind.startsWith('ops:') ? 'sparkles' : job.kind === 'check' ? 'checks' : 'rocket'} size={14} />{job.title}</b>{/if}
    <span class="pill {job.status === 'ok' ? 'ok' : job.status === 'failed' ? 'err' : job.status === 'canceled' ? 'warn' : 'run'}">
      {job.status === 'running' ? '进行中' : job.status === 'ok' ? '完成' : job.status === 'failed' ? '失败' : '已取消'}
    </span>
    {#if elapsed}<span class="faint" style="font-size:11.5px">用时 {elapsed} 秒</span>{/if}
    <span style="flex:1"></span>
    {#if job.url}<button class="btn tiny" onclick={() => openUrl(job.url)}><Icon name="external" size={11} />打开</button>{/if}
    {#if job.status === 'running'}<button class="btn tiny danger" onclick={() => cancelSiteJob(job.site_id)}><Icon name="stop" size={11} />停止</button>{/if}
    <button class="btn tiny" onclick={() => (showLog = !showLog)} aria-expanded={showLog}><Icon name="terminal" size={11} />{showLog ? '收起日志' : '日志'}</button>
  </div>
  {#if !compact || job.status === 'running'}
    <div class="steps" role="list">
      {#each job.steps as s (s.key)}
        <div class="step {s.status}" role="listitem" title={s.detail || undefined}><span class="dot">{mark(s.status)}</span><span>{s.label}</span></div>
      {/each}
    </div>
    {#if focus?.detail}<div class="step-detail mono" title={focus.detail}>{focus.label} · {focus.detail}</div>{/if}
  {/if}
  {#if job.error}<div class="err-note">{job.error}</div>{/if}
  {#if showLog}<pre class="log" bind:this={logEl}>{job.log.join('\n') || '（还没有输出）'}</pre>{/if}
</div>

<style>
  .job { display: flex; flex-direction: column; gap: 10px; }
  .job-head { min-height: 24px; }
  .job-head b { font-size: 13.5px; font-weight: 600; display: inline-flex; align-items: center; gap: 8px; }
  .job-head b :global(.ico) { color: var(--dim); }
  .step-detail { font-size: 11.5px; margin-top: -2px; }
</style>
