<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site, Opportunity } from '$lib/types';
  import { ops, addToQueue, workOnOpportunity, KIND_LABEL, CHANGE_LABEL } from '$lib/state/ops.svelte.js';

  let { site, opp }: { site: Site; opp: Opportunity } = $props();
  let open = $state(false);
  const queued = $derived((ops.queue[site.id] ?? []).find((q) => q.opp.key === opp.key && !['deployed', 'rolled_back', 'failed'].includes(q.status)));
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
</script>

<div class="li opp" class:open>
  <div class="opp-head">
    <span class="pill">{KIND_LABEL[opp.kind] ?? opp.kind}</span>
    <button class="opp-title" onclick={() => (open = !open)} aria-expanded={open}>
      <svg class="chev" width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
      <span>{opp.title}</span>
    </button>
    <span class="chg">{CHANGE_LABEL[opp.change] ?? opp.change}</span>
    {#if queued}
      <span class="pill {queued.status === 'dismissed' ? '' : 'line'}">{queued.status === 'dismissed' ? '已驳回' : '已在待处理'}</span>
    {:else}
      <span class="acts">
        <button class="btn tiny" onclick={() => addToQueue(site.id, opp)}><Icon name="plus" size={11} />加入待处理</button>
        <button class="btn tiny" onclick={() => workOnOpportunity(site, opp)}><Icon name="chat" size={11} />开会话处理</button>
      </span>
    {/if}
  </div>
  {#if open}
    <div class="opp-body">
      <p>{opp.evidence}</p>
      {#if opp.queries.length}
        <table class="tbl" style="margin-top:4px">
          <thead><tr><th>查询词</th><th class="num">点击</th><th class="num">展现</th><th class="num">点击率</th><th class="num">排名</th></tr></thead>
          <tbody>{#each opp.queries as q (q.key + (q.key2 ?? ''))}<tr><td class="trunc">{q.key}</td><td class="num">{Math.round(q.clicks)}</td><td class="num">{Math.round(q.impressions)}</td><td class="num">{pct(q.ctr)}</td><td class="num">{q.position.toFixed(1)}</td></tr>{/each}</tbody>
        </table>
      {/if}
    </div>
  {/if}
</div>

<style>
  .opp-head { display: flex; align-items: center; gap: 10px; min-width: 0; min-height: 24px; }
  .opp-head > .pill { flex: none; min-width: 64px; justify-content: center; }
  .opp-title { flex: 1; min-width: 0; display: inline-flex; align-items: center; gap: 6px; text-align: left; background: none; border: none; padding: 0; font: inherit; font-size: 13px; color: var(--text); cursor: pointer; }
  .opp-title span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chev { flex: none; color: var(--faint); transition: transform .16s var(--su-ease), color .12s; }
  .opp-title:hover .chev { color: var(--text); }
  .opp.open .chev { transform: rotate(90deg); }
  .chg { flex: none; color: var(--faint); font-size: 11.5px; white-space: nowrap; }
  .acts { display: inline-flex; gap: 4px; flex: none; }
  .opp-body { margin: 8px 0 2px 16px; font-size: 12.5px; color: var(--dim); animation: su-in .18s var(--su-ease) both; }
  .opp-body p { margin: 0 0 6px; line-height: 1.65; max-width: 72ch; }
  @media (max-width: 760px) { .chg { display: none; } }
</style>
