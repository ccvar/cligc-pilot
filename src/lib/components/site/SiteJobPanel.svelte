<script lang="ts">
  import './site.css';
  import type { SiteJob } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { cancelSiteJob } from '$lib/state/sites.svelte.js';

  let { job, compact = false }: { job: SiteJob; compact?: boolean } = $props();
  let showLog = $state(false);
  let logEl: HTMLPreElement | undefined = $state();
  const mark = (s: string) => (s === 'ok' ? '✓' : s === 'failed' ? '!' : s === 'skipped' ? '–' : '');
  const elapsed = $derived(job.finished_at ? job.finished_at - job.started_at : 0);
  $effect(() => { void job.log.length; if (showLog && logEl) logEl.scrollTop = logEl.scrollHeight; });
  $effect(() => { if (job.status === 'failed') showLog = true; });
</script>

<div class="su job">
  <div class="row job-head">
    <b>{job.title}</b>
    <span class="pill {job.status === 'ok' ? 'ok' : job.status === 'failed' ? 'err' : job.status === 'canceled' ? 'warn' : 'run'}">
      {job.status === 'running' ? '进行中' : job.status === 'ok' ? '完成' : job.status === 'failed' ? '失败' : '已取消'}
    </span>
    {#if elapsed}<span class="faint" style="font-size:11.5px">{elapsed} 秒</span>{/if}
    <span style="flex:1"></span>
    {#if job.url}<button class="btn tiny" onclick={() => openUrl(job.url)}>打开 ↗</button>{/if}
    {#if job.status === 'running'}<button class="btn tiny danger" onclick={() => cancelSiteJob(job.site_id)}>停止</button>{/if}
    <button class="btn tiny" onclick={() => (showLog = !showLog)}>{showLog ? '收起日志' : '日志'}</button>
  </div>
  {#if !compact || job.status === 'running'}
    <div class="steps">
      {#each job.steps as s (s.key)}
        <div class="step {s.status}"><span class="dot">{mark(s.status)}</span><span>{s.label}</span>{#if s.detail}<span class="detail" title={s.detail}>{s.detail}</span>{/if}</div>
      {/each}
    </div>
  {/if}
  {#if job.error}<div class="err-note">{job.error}</div>{/if}
  {#if showLog}<pre class="log" bind:this={logEl}>{job.log.join('\n') || '（还没有输出）'}</pre>{/if}
</div>

<style>
  .job { display: flex; flex-direction: column; gap: 9px; }
  .job-head b { font-size: 13px; }
</style>
