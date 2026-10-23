<script lang="ts">
  import './site.css';
  import type { Site, OpsItem } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import { sites } from '$lib/state/sites.svelte.js';
  import { ops, loadQueue, runTask, workItem, acceptItem, dismissItem, restoreItem, markDone, rollbackItem, showDiff, KIND_LABEL, CHANGE_LABEL, STATUS_LABEL, LEVEL_LABEL } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';

  let { site }: { site: Site } = $props();
  $effect(() => { void loadQueue(site.id); });
  const all = $derived(ops.queue[site.id] ?? []);
  let filter = $state<'todo' | 'open' | 'live' | 'closed'>('todo');
  const groups = {
    todo: ['proposed', 'working', 'failed'],
    open: ['open'],
    live: ['merged', 'deployed', 'done'],
    closed: ['dismissed', 'rolled_back'],
  } as const;
  const shown = $derived(all.filter((i) => (groups[filter] as readonly string[]).includes(i.status)));
  const counts = $derived(Object.fromEntries(Object.entries(groups).map(([k, v]) => [k, all.filter((i) => (v as readonly string[]).includes(i.status)).length])) as Record<string, number>);
  const job = $derived(sites.jobs[site.id]);
  const busy = $derived(job?.status === 'running');
  const when = (ts: number) => (ts ? new Date(ts * 1000).toLocaleString() : '');
  const tone = (s: string) => (s === 'proposed' ? 'warn' : s === 'deployed' || s === 'done' ? 'ok' : s === 'failed' ? 'err' : s === 'working' ? 'run' : '');
  const canWork = (i: OpsItem) => i.status === 'open' || i.status === 'failed';
</script>

<div class="qq">
  <div class="card">
    <h3>待处理队列 <span class="grow"></span>
      <span class="faint" style="font-weight:400;font-size:12px">档位 {LEVEL_LABEL[site.ops.level]}</span>
      <button class="btn small" onclick={() => runTask(site.id, 'scan')} disabled={busy}>立即扫描</button>
    </h3>
    <p class="hint">所有建议、PR、自动上线记录都在这里：逐条确认、驳回或回滚。驳回过的机会不会再提；每条改动在默认分支上是独立提交，回滚即回退那一次提交并重新上线。</p>
    {#if job && job.kind.startsWith('ops:') && (job.status === 'running' || job.finished_at > Date.now() / 1000 - 300)}<SiteJobPanel {job} compact />{/if}
  </div>

  <div class="seg">
    <button class:on={filter === 'todo'} onclick={() => (filter = 'todo')}>需要你处理 {counts.todo || ''}</button>
    <button class:on={filter === 'open'} onclick={() => (filter = 'open')}>未处理的机会 {counts.open || ''}</button>
    <button class:on={filter === 'live'} onclick={() => (filter = 'live')}>已上线 {counts.live || ''}</button>
    <button class:on={filter === 'closed'} onclick={() => (filter = 'closed')}>已驳回 / 回滚 {counts.closed || ''}</button>
  </div>

  {#if !shown.length}
    <p class="hint" style="padding:8px 2px">{filter === 'todo' ? '没有需要你确认的事项。' : filter === 'open' ? '队列里没有未处理的机会。可以「立即扫描」，或在「数据」页把机会加进来。' : '暂无记录。'}</p>
  {/if}
  {#each shown as i (i.id)}
    <div class="card item">
      <div class="row it-head">
        <span class="pill {tone(i.status)}">{STATUS_LABEL[i.status] ?? i.status}</span>
        <b class="it-title" title={i.opp.title}>{i.opp.title}</b>
        <span class="faint" style="font-size:11px;white-space:nowrap">{KIND_LABEL[i.opp.kind] ?? i.opp.kind} · {when(i.updated_at)}</span>
      </div>
      <p class="it-ev">{i.opp.evidence}</p>
      {#if i.branch || i.change_kind}
        <div class="row wrap meta">
          {#if i.change_kind}<span class="pill">{i.change_kind.split(',').map((k) => CHANGE_LABEL[k] ?? k).join(' + ')}</span>{/if}
          {#if i.auto}<span class="pill ok">自动上线</span>{/if}
          {#if i.branch}<span class="mono faint" style="font-size:11.5px">{i.branch}</span>{/if}
          {#if i.pr_url}<button class="link-btn" onclick={() => openUrl(i.pr_url)}>PR #{i.pr_number} ↗</button>{/if}
        </div>
      {/if}
      {#if i.verdict && i.status === 'proposed'}
        <p class="hint {i.verdict === 'auto' ? '' : 'warn-text'}">{i.verdict === 'auto' ? '整份改动都在白名单内，检查已通过。' : `不能自动上线：${i.verdict}`}</p>
      {/if}
      {#if i.note}<p class="hint warn-text">{i.note}</p>{/if}
      {#if i.review}<p class="hint">效果复盘：<b class={i.review === 'positive' ? 'up' : i.review === 'negative' ? 'down' : ''}>{i.review === 'positive' ? '正面' : i.review === 'negative' ? '负面' : '持平'}</b> · {i.review_note}</p>{/if}
      <div class="row wrap acts">
        {#if i.conv_id}<button class="btn tiny" onclick={() => openConv(i.conv_id)}>打开会话</button>{/if}
        {#if i.head_sha}<button class="btn tiny" onclick={() => showDiff(site.id, i)}>查看差异</button>{/if}
        {#if canWork(i)}<button class="btn tiny" onclick={() => workItem(site.id, i)} disabled={busy || site.ops.level < 2}>{site.ops.level < 2 ? '第 1 档只分析' : site.ops.level === 2 ? '让 AI 出方案' : '让 AI 改'}</button>{/if}
        {#if i.status === 'proposed'}<button class="btn tiny primary" onclick={() => acceptItem(site, i)} disabled={busy}>{i.branch ? (site.source === 'github' ? '合并并上线' : '合并到主分支并上线') : '标记已处理'}</button>{/if}
        {#if i.status === 'failed' && i.conv_id}<button class="btn tiny" onclick={() => markDone(site.id, i)}>我已手动处理</button>{/if}
        {#if ['open', 'proposed', 'failed'].includes(i.status)}<button class="btn tiny" onclick={() => dismissItem(site, i)} disabled={busy}>驳回</button>{/if}
        {#if ['dismissed', 'failed'].includes(i.status)}<button class="btn tiny" onclick={() => restoreItem(site.id, i)}>放回待处理</button>{/if}
        {#if ['merged', 'deployed'].includes(i.status) && i.merge_sha}<button class="btn tiny danger" onclick={() => rollbackItem(site, i)} disabled={busy}>回滚</button>{/if}
      </div>
    </div>
  {/each}
</div>

{#if ops.diff}
  <div class="su-mask" role="presentation" onclick={() => (ops.diff = null)}></div>
  <div class="su modal diff-modal" role="dialog" aria-modal="true" aria-label="改动差异">
    <header class="sheet-head"><b>{ops.diff.item.opp.title}</b><button class="x" onclick={() => (ops.diff = null)}>×</button></header>
    <div class="sheet-body"><pre class="diff">{#each ops.diff.text.split('\n') as l, n (n)}<span class={l.startsWith('+') && !l.startsWith('+++') ? 'add' : l.startsWith('-') && !l.startsWith('---') ? 'del' : l.startsWith('@@') ? 'hunk' : ''}>{l}{'\n'}</span>{/each}</pre></div>
  </div>
{/if}

<style>
  .qq { display: flex; flex-direction: column; gap: 12px; }
  .item { display: flex; flex-direction: column; gap: 6px; padding: 12px 14px; }
  .it-head { min-width: 0; }
  .it-title { flex: 1; min-width: 0; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .it-ev { margin: 0; font-size: 12.5px; color: var(--dim); line-height: 1.6; }
  .meta { gap: 6px; }
  .acts { gap: 6px; margin-top: 2px; }
  .up { color: var(--ok); }
  .down { color: var(--err); }
  .diff-modal { width: min(900px, 94vw); }
  pre.diff { margin: 0; font-size: 11.5px; line-height: 1.5; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
  pre.diff .add { background: #e8f5ec; color: #135c34; }
  pre.diff .del { background: #fdecec; color: #8f1d1d; }
  pre.diff .hunk { color: #2f5fb8; }
</style>
