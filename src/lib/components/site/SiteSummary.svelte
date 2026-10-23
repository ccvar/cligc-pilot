<script lang="ts">
  import './site.css';
  import type { Site } from '$lib/types';
  import OpportunityRow from './OpportunityRow.svelte';
  import { sites, loadDashboard } from '$lib/state/sites.svelte.js';

  let { site }: { site: Site } = $props();
  $effect(() => { if (!sites.dash[site.id]) void loadDashboard(site.id); });
  const d = $derived(sites.dash[site.id]);
  const n = (v: number) => Math.round(v).toLocaleString();
  const delta = (cur: number, prev: number) => (prev > 0 ? Math.round(((cur - prev) / prev) * 100) : 0);
</script>

{#if d}
  <div class="card">
    <h3>近 28 天 <span class="grow"></span><button class="btn tiny" onclick={() => (sites.tab = 'data')}>看板</button></h3>
    <div class="sum">
      {#if site.data.gsc_site}
        <span><small>Google 点击</small><b>{n(d.gsc_totals.clicks)}</b><em class:up={d.gsc_totals.clicks >= d.gsc_prev_totals.clicks}>{delta(d.gsc_totals.clicks, d.gsc_prev_totals.clicks)}%</em></span>
        <span><small>展现</small><b>{n(d.gsc_totals.impressions)}</b><em class:up={d.gsc_totals.impressions >= d.gsc_prev_totals.impressions}>{delta(d.gsc_totals.impressions, d.gsc_prev_totals.impressions)}%</em></span>
      {/if}
      {#if site.data.ga4_property}<span><small>浏览量</small><b>{n(d.ga4_totals.views)}</b><em class:up={d.ga4_totals.views >= d.ga4_prev_views}>{delta(d.ga4_totals.views, d.ga4_prev_views)}%</em></span>{/if}
      {#if site.data.bing_site}<span><small>Bing 点击</small><b>{n(d.bing_totals.clicks)}</b></span>{/if}
      <span><small>运营机会</small><b>{d.opportunities.length}</b></span>
    </div>
    {#if d.opportunities.length}
      <div class="opps">{#each d.opportunities.slice(0, 3) as o (o.key)}<OpportunityRow {site} opp={o} />{/each}</div>
    {/if}
  </div>
{/if}

<style>
  .sum { display: flex; flex-wrap: wrap; gap: 8px 28px; margin-bottom: 10px; }
  .sum span { display: flex; flex-direction: column; }
  .sum small { color: var(--dim); font-size: 11.5px; }
  .sum b { font-size: 18px; font-variant-numeric: tabular-nums; }
  .sum em { font-style: normal; font-size: 11px; color: var(--err); }
  .sum em.up { color: var(--ok); }
  .opps { display: flex; flex-direction: column; gap: 6px; }
</style>
