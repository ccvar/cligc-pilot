<script lang="ts">
  import OpsErrorHint from './OpsErrorHint.svelte';
  import './site.css';
  import Icon from './Icon.svelte';
  import type { SiteJob } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { cancelSiteJob } from '$lib/state/sites.svelte.js';
  import ModelMenu from './ModelMenu.svelte';
  import { ops, loadQueue, workItem } from '$lib/state/ops.svelte.js';
  import { opsErrorHint } from '$lib/friendlyError';

  // bare：外层卡片已经有同名标题时不再重复显示作业名。
  let { job, compact = false, bare = false, logOpen = false }: { job: SiteJob; compact?: boolean; bare?: boolean; logOpen?: boolean } = $props();
  // svelte-ignore state_referenced_locally
  let showLog = $state(logOpen);
  let logEl: HTMLPreElement | undefined = $state();
  const mark = (s: string) => (s === 'ok' ? '✓' : s === 'failed' ? '!' : '');
  const elapsed = $derived(job.finished_at ? job.finished_at - job.started_at : 0);
  // 步骤条下面只说一件事：正在做的那一步，或失败的那一步。
  const focus = $derived(job.steps.find((s) => s.status === 'failed') ?? job.steps.find((s) => s.status === 'running'));
  $effect(() => { void job.log.length; if (showLog && logEl) logEl.scrollTop = logEl.scrollHeight; });
  $effect(() => { if (job.status === 'failed') showLog = true; });
  // AI 改动因为模型这边的原因没成（余额、密钥、模型用不了、限流…）：直接在这里换个模型重试那一条。
  // 作业记录里没有条目 id：找这个作业开始后变成「失败」的那一条
  const modelErr = $derived(job.status === 'failed' && ['ops:work', 'ops:batch', 'ops:cycle'].includes(job.kind) && !!opsErrorHint(job.error));
  $effect(() => { if (modelErr && !ops.queue[job.site_id]) void loadQueue(job.site_id); });
  const failedItem = $derived(modelErr ? (ops.queue[job.site_id] ?? []).filter((i) => i.status === 'failed' && i.updated_at >= job.started_at).sort((a, b) => b.updated_at - a.updated_at)[0] : undefined);
  let mAt = $state<{ left: number; top: number; bottom: number; right: number } | null>(null);
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
  {#if job.error}<div class="err-note">{job.error}</div><OpsErrorHint text={job.error} />
    {#if failedItem}<div class="retry-row"><button class="btn tiny primary" onclick={(e) => { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); mAt = { left: r.left, right: r.right, top: r.top, bottom: r.bottom }; }}><Icon name="refresh" size={11} />换个模型重试「{failedItem.opp.title}」<svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button></div>{/if}
  {/if}
  {#if mAt && failedItem}<ModelMenu at={mAt} current="" note="只用这一次，站点和托管方案的模型不变" onpick={(m) => { const it = failedItem; mAt = null; if (it) void workItem(job.site_id, it, m); }} onclose={() => (mAt = null)} />{/if}
  {#if showLog}<pre class="log" bind:this={logEl}>{job.log.join('\n') || '（还没有输出）'}</pre>{/if}
</div>

<style>
  .retry-row { display: flex; margin-top: 2px; }
  .retry-row .btn { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .job { display: flex; flex-direction: column; gap: 10px; }
  .job-head { min-height: 24px; }
  .job-head b { font-size: 13.5px; font-weight: 600; display: inline-flex; align-items: center; gap: 8px; }
  .job-head b :global(.ico) { color: var(--dim); }
  .step-detail { font-size: 11.5px; margin-top: -2px; }
</style>
