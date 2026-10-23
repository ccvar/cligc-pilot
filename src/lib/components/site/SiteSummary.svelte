<script lang="ts">
  import './site.css';
  import type { Site } from '$lib/types';
  import OpportunityRow from './OpportunityRow.svelte';
  import { num as n, delta } from './fmt';
  import { sites, loadDashboard } from '$lib/state/sites.svelte.js';

  let { site }: { site: Site } = $props();
  $effect(() => { if (!sites.dash[site.id]) void loadDashboard(site.id); });
  const d = $derived(sites.dash[site.id]);
  const stats = $derived.by(() => {
    if (!d) return [];
    const out: { l: string; v: string; dl?: ReturnType<typeof delta> }[] = [];
    if (site.data.gsc_site) {
      out.push({ l: 'Google 点击', v: n(d.gsc_totals.clicks), dl: delta(d.gsc_totals.clicks, d.gsc_prev_totals.clicks) });
      out.push({ l: '展现', v: n(d.gsc_totals.impressions), dl: delta(d.gsc_totals.impressions, d.gsc_prev_totals.impressions) });
    }
    if (site.data.ga4_property) out.push({ l: '浏览量', v: n(d.ga4_totals.views), dl: delta(d.ga4_totals.views, d.ga4_prev_views) });
    if (site.data.bing_site) out.push({ l: 'Bing 点击', v: n(d.bing_totals.clicks) });
    out.push({ l: '运营机会', v: String(d.opportunities.length) });
    return out;
  });
</script>

{#if d}
  <div class="card">
    <h3>近 28 天 <span class="sub">与前 28 天对比</span><span class="grow"></span><button class="btn tiny" onclick={() => (sites.tab = 'data')}>完整数据</button></h3>
    <div class="stats">
      {#each stats as k (k.l)}
        <div class="stat"><small>{k.l}</small><b>{k.v}</b>{#if k.dl}<em class:up={k.dl.dir > 0} class:down={k.dl.dir < 0}>{k.dl.text || '—'}</em>{:else}<em>&nbsp;</em>{/if}</div>
      {/each}
    </div>
    {#if d.opportunities.length}
      <div class="opps-t">优先处理</div>
      <div class="list">{#each d.opportunities.slice(0, 3) as o (o.key)}<OpportunityRow {site} opp={o} />{/each}</div>
    {/if}
  </div>
{/if}

<style>
  .opps-t { margin: 14px 0 4px; font-size: 11.5px; font-weight: 600; color: var(--dim); }
</style>
