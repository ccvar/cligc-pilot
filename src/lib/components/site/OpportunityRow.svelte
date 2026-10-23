<script lang="ts">
  import './site.css';
  import type { Site, Opportunity } from '$lib/types';
  import { ops, addToQueue, workOnOpportunity, KIND_LABEL, CHANGE_LABEL } from '$lib/state/ops.svelte.js';

  let { site, opp }: { site: Site; opp: Opportunity } = $props();
  let open = $state(false);
  const queued = $derived((ops.queue[site.id] ?? []).find((q) => q.opp.key === opp.key && !['deployed', 'rolled_back', 'failed'].includes(q.status)));
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
</script>

<div class="opp">
  <div class="opp-head">
    <span class="pill">{KIND_LABEL[opp.kind] ?? opp.kind}</span>
    <button class="opp-title" onclick={() => (open = !open)}>{opp.title}</button>
    <span class="faint" style="font-size:11px">{CHANGE_LABEL[opp.change] ?? opp.change}</span>
    {#if queued}
      <span class="pill {queued.status === 'dismissed' ? '' : 'run'}">{queued.status === 'dismissed' ? '已驳回' : '在队列中'}</span>
    {:else}
      <button class="btn tiny" onclick={() => addToQueue(site.id, opp)}>加入待处理</button>
      <button class="btn tiny" onclick={() => workOnOpportunity(site, opp)}>开会话处理</button>
    {/if}
  </div>
  {#if open}
    <div class="opp-body">
      <p>{opp.evidence}</p>
      {#if opp.queries.length}
        <table class="tbl">
          <thead><tr><th>查询词</th><th class="num">点击</th><th class="num">展现</th><th class="num">点击率</th><th class="num">排名</th></tr></thead>
          <tbody>{#each opp.queries as q (q.key + (q.key2 ?? ''))}<tr><td class="trunc">{q.key}</td><td class="num">{Math.round(q.clicks)}</td><td class="num">{Math.round(q.impressions)}</td><td class="num">{pct(q.ctr)}</td><td class="num">{q.position.toFixed(1)}</td></tr>{/each}</tbody>
        </table>
      {/if}
    </div>
  {/if}
</div>

<style>
  .opp { border: 1px solid var(--border); border-radius: 10px; padding: 7px 10px; }
  .opp-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .opp-title { flex: 1; min-width: 0; text-align: left; background: none; border: none; padding: 0; font: inherit; font-size: 12.5px; color: var(--text); cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .opp-title:hover { color: var(--accent); }
  .opp-body { margin-top: 8px; font-size: 12.5px; color: var(--dim); }
  .opp-body p { margin: 0 0 6px; line-height: 1.6; }
</style>
