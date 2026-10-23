<script lang="ts">
  import './site.css';
  import type { Site, Opportunity } from '$lib/types';
  import LineChart from './LineChart.svelte';
  import OpportunityRow from './OpportunityRow.svelte';
  import { sites, loadDashboard, refreshSiteData } from '$lib/state/sites.svelte.js';

  let { site }: { site: Site } = $props();
  const d = $derived(sites.dash[site.id]);
  const busy = $derived(!!sites.dataBusy[site.id]);
  const hasSources = $derived(!!(site.data.ga4_property || site.data.gsc_site || site.data.bing_site));
  $effect(() => { const id = site.id; void loadDashboard(id).then(() => { if (hasSources) void refreshSiteData(id, false); }); });

  let qTab = $state<'gsc' | 'bing'>('gsc');
  let pTab = $state<'gsc' | 'ga4' | 'bing'>('gsc');
  const n = (v: number) => Math.round(v).toLocaleString();
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  const delta = (cur: number, prev: number) => (prev > 0 ? (cur - prev) / prev : cur > 0 ? 1 : 0);
  const path = (u: string) => { try { const x = new URL(u); return x.pathname + x.search; } catch { return u; } };
  const ago = (ts: number) => { if (!ts) return '未拉取'; const m = Math.round((Date.now() / 1000 - ts) / 60); return m < 60 ? `${m} 分钟前` : m < 1440 ? `${Math.round(m / 60)} 小时前` : `${Math.round(m / 1440)} 天前`; };
  const events = $derived((d?.ga4_events ?? []).filter((e) => !['page_view', 'session_start', 'first_visit', 'user_engagement', 'scroll'].includes(e.event)));
  const evNames = $derived([...new Set(events.filter((e) => !e.dim).map((e) => e.event))]);
  const opps = $derived<Opportunity[]>(d?.opportunities ?? []);
  const prevOf = (r: object) => (r as { prev_clicks?: number }).prev_clicks;
</script>

<div class="dt">
  {#if !hasSources}
    <div class="card">
      <h3>还没有接入数据源</h3>
      <p class="hint">在站点设置里接入 Google（GA4 + Search Console）和 Bing 后，这里会显示趋势、查询词、页面和 Pilot 按规则找出的运营机会。</p>
      <div class="row"><button class="btn small primary" onclick={() => (sites.tab = 'settings')}>去接入</button></div>
    </div>
  {:else}
    <div class="bar">
      <div class="srcs">
        {#if site.data.gsc_site}<span class="pill {d?.meta.gsc.error ? 'err' : ''}" title={d?.meta.gsc.error}>Search Console · {d?.meta.gsc.end ? `截至 ${d.meta.gsc.end}` : ''} · {ago(d?.meta.gsc.fetched_at ?? 0)}</span>{/if}
        {#if site.data.ga4_property}<span class="pill {d?.meta.ga4.error ? 'err' : ''}" title={d?.meta.ga4.error}>GA4 · {ago(d?.meta.ga4.fetched_at ?? 0)}</span>{/if}
        {#if site.data.bing_site}<span class="pill {d?.meta.bing.error ? 'err' : ''}" title={d?.meta.bing.error}>Bing · {d?.meta.bing.end ? `截至 ${d.meta.bing.end}` : ''} · {ago(d?.meta.bing.fetched_at ?? 0)}</span>{/if}
      </div>
      <button class="btn small" onclick={() => refreshSiteData(site.id, true)} disabled={busy}>{busy ? '拉取中…' : '刷新数据'}</button>
    </div>
    {#each [['Search Console', d?.meta.gsc.error], ['GA4', d?.meta.ga4.error], ['Bing', d?.meta.bing.error]] as [label, err] (label)}
      {#if err}<div class="err-note">{label}：{err}</div>{/if}
    {/each}

    {#if d}
      <div class="kpis">
        {#if site.data.gsc_site}
          {#each [
            { l: 'Google 点击', v: n(d.gsc_totals.clicks), dl: delta(d.gsc_totals.clicks, d.gsc_prev_totals.clicks) },
            { l: 'Google 展现', v: n(d.gsc_totals.impressions), dl: delta(d.gsc_totals.impressions, d.gsc_prev_totals.impressions) },
            { l: '平均点击率', v: pct(d.gsc_totals.ctr), dl: delta(d.gsc_totals.ctr, d.gsc_prev_totals.ctr) },
          ] as k (k.l)}
            <div class="kpi"><small>{k.l}</small><b>{k.v}</b><span class:up={k.dl > 0.005} class:down={k.dl < -0.005}>{k.dl > 0 ? '+' : ''}{(k.dl * 100).toFixed(0)}%</span></div>
          {/each}
          {@const moved = d.gsc_prev_totals.position > 0 ? d.gsc_prev_totals.position - d.gsc_totals.position : 0}
          <div class="kpi"><small>平均排名</small><b>{d.gsc_totals.position.toFixed(1)}</b><span class:up={moved > 0.05} class:down={moved < -0.05}>{moved > 0.05 ? `上升 ${moved.toFixed(1)} 位` : moved < -0.05 ? `下降 ${(-moved).toFixed(1)} 位` : '持平'}</span></div>
        {/if}
        {#if site.data.ga4_property}
          <div class="kpi"><small>浏览量</small><b>{n(d.ga4_totals.views)}</b><span class:up={d.ga4_totals.views > d.ga4_prev_views} class:down={d.ga4_totals.views < d.ga4_prev_views}>{(delta(d.ga4_totals.views, d.ga4_prev_views) * 100).toFixed(0)}%</span></div>
          <div class="kpi"><small>会话</small><b>{n(d.ga4_totals.sessions)}</b><span class="faint">近 28 天</span></div>
        {/if}
        {#if site.data.bing_site}
          <div class="kpi"><small>Bing 点击</small><b>{n(d.bing_totals.clicks)}</b><span class="faint">展现 {n(d.bing_totals.impressions)}</span></div>
        {/if}
      </div>
      <p class="faint note">与前 28 天对比。Search Console 数据有 2–3 天延迟。</p>

      {#if d.gsc_daily.length || d.ga4_daily.length}
        <div class="card">
          <h3>趋势（近 90 天）</h3>
          {#if d.gsc_daily.length}
            <LineChart labels={d.gsc_daily.map((r) => r.key)} series={[
              { label: '点击', color: '#2f6fd8', values: d.gsc_daily.map((r) => r.clicks) },
              { label: '展现', color: '#9b8ee0', values: d.gsc_daily.map((r) => r.impressions) },
            ]} />
          {/if}
          {#if d.ga4_daily.length}
            <div style="margin-top:12px"><LineChart height={110} labels={d.ga4_daily.map((r) => r.key)} series={[
              { label: '会话', color: '#12805c', values: d.ga4_daily.map((r) => r.sessions) },
              { label: '浏览量', color: '#b45309', values: d.ga4_daily.map((r) => r.views) },
            ]} /></div>
          {/if}
        </div>
      {/if}

      <div class="card">
        <h3>运营机会 <span class="faint" style="font-weight:400;font-size:12px">Pilot 按固定规则从数据里算出，可复现</span></h3>
        {#if opps.length}
          <div class="opps">{#each opps.slice(0, 12) as o (o.key)}<OpportunityRow {site} opp={o} />{/each}</div>
        {:else}
          <p class="hint">暂时没有发现明显的机会（或数据还不够）。阈值可以在「运营」里调低。</p>
        {/if}
      </div>

      <div class="two">
        <div class="card">
          <h3>查询词 <span class="grow"></span>
            {#if site.data.bing_site && site.data.gsc_site}<span class="seg mini"><button class:on={qTab === 'gsc'} onclick={() => (qTab = 'gsc')}>Google</button><button class:on={qTab === 'bing'} onclick={() => (qTab = 'bing')}>Bing</button></span>{/if}
          </h3>
          <table class="tbl">
            <thead><tr><th>查询词</th><th class="num">点击</th><th class="num">展现</th><th class="num">点击率</th><th class="num">排名</th></tr></thead>
            <tbody>
              {#each (qTab === 'gsc' && site.data.gsc_site ? d.gsc_queries : d.bing_queries).slice(0, 25) as r (r.key)}
                <tr>
                  <td class="trunc" title={r.key}>{r.key}</td>
                  <td class="num">{n(r.clicks)}{#if prevOf(r) !== undefined && prevOf(r) !== r.clicks}<small class:up={r.clicks > (prevOf(r) ?? 0)} class:down={r.clicks < (prevOf(r) ?? 0)}> {r.clicks > (prevOf(r) ?? 0) ? '↑' : '↓'}</small>{/if}</td>
                  <td class="num">{n(r.impressions)}</td>
                  <td class="num">{pct(r.ctr)}</td>
                  <td class="num">{r.position.toFixed(1)}</td>
                </tr>
              {:else}<tr><td colspan="5" class="faint">没有数据</td></tr>{/each}
            </tbody>
          </table>
        </div>
        <div class="card">
          <h3>页面 <span class="grow"></span>
            <span class="seg mini">
              {#if site.data.gsc_site}<button class:on={pTab === 'gsc'} onclick={() => (pTab = 'gsc')}>Google</button>{/if}
              {#if site.data.ga4_property}<button class:on={pTab === 'ga4'} onclick={() => (pTab = 'ga4')}>GA4</button>{/if}
              {#if site.data.bing_site}<button class:on={pTab === 'bing'} onclick={() => (pTab = 'bing')}>Bing</button>{/if}
            </span>
          </h3>
          {#if pTab === 'ga4'}
            <table class="tbl">
              <thead><tr><th>页面</th><th class="num">浏览</th><th class="num">用户</th><th class="num">互动会话</th></tr></thead>
              <tbody>{#each d.ga4_pages.slice(0, 25) as r (r.key)}<tr><td class="trunc" title={r.key}>{r.key}</td><td class="num">{n(r.views)}</td><td class="num">{n(r.users)}</td><td class="num">{n(r.engaged)}</td></tr>{:else}<tr><td colspan="4" class="faint">没有数据</td></tr>{/each}</tbody>
            </table>
          {:else}
            <table class="tbl">
              <thead><tr><th>页面</th><th class="num">点击</th><th class="num">展现</th><th class="num">点击率</th><th class="num">排名</th></tr></thead>
              <tbody>
                {#each (pTab === 'bing' ? d.bing_pages : d.gsc_pages).slice(0, 25) as r (r.key)}
                  <tr><td class="trunc" title={r.key}>{path(r.key)}</td><td class="num">{n(r.clicks)}</td><td class="num">{n(r.impressions)}</td><td class="num">{pct(r.ctr)}</td><td class="num">{r.position.toFixed(1)}</td></tr>
                {:else}<tr><td colspan="5" class="faint">没有数据</td></tr>{/each}
              </tbody>
            </table>
          {/if}
        </div>
      </div>

      {#if site.data.ga4_property}
        <div class="two">
          <div class="card">
            <h3>流量来源（近 28 天）</h3>
            <table class="tbl">
              <thead><tr><th>来源 / 媒介</th><th class="num">会话</th><th class="num">用户</th></tr></thead>
              <tbody>{#each d.ga4_sources.slice(0, 15) as r (r.key)}<tr><td class="trunc">{r.key}</td><td class="num">{n(r.sessions)}</td><td class="num">{n(r.users)}</td></tr>{:else}<tr><td colspan="3" class="faint">没有数据</td></tr>{/each}</tbody>
            </table>
          </div>
          <div class="card">
            <h3>转化事件（近 28 天）</h3>
            {#if evNames.length}
              {#each evNames as ev (ev)}
                {@const total = events.find((e) => e.event === ev && !e.dim)}
                <div class="ev"><b>{ev}</b><span class="faint">{n(total?.count ?? 0)} 次 · {n(total?.users ?? 0)} 人</span></div>
                {#each [...new Set(events.filter((e) => e.event === ev && e.dim).map((e) => e.dim))] as dim (dim)}
                  <div class="ev-dim">
                    <small>按 {dim}：</small>
                    {#each events.filter((e) => e.event === ev && e.dim === dim).sort((a, b) => b.count - a.count).slice(0, 8) as e (e.value)}<span class="pill">{e.value} · {n(e.count)}</span>{/each}
                  </div>
                {/each}
              {/each}
            {:else}
              <p class="hint">没有自定义事件。站点里埋的事件（如推荐链接点击）会出现在这里；要按参数拆分，需在 GA4 里把参数注册为自定义维度。</p>
            {/if}
          </div>
        </div>
      {/if}
    {:else}
      <p class="hint">正在读取…</p>
    {/if}
  {/if}
</div>

<style>
  .dt { display: flex; flex-direction: column; gap: 14px; }
  .bar { display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap; }
  .srcs { display: flex; flex-wrap: wrap; gap: 6px; }
  .kpis { display: grid; grid-template-columns: repeat(auto-fill, minmax(140px, 1fr)); gap: 10px; }
  .kpi { border: 1px solid var(--border); border-radius: 12px; padding: 10px 12px; display: flex; flex-direction: column; gap: 2px; background: var(--card); }
  .kpi small { color: var(--dim); font-size: 11.5px; }
  .kpi b { font-size: 20px; font-variant-numeric: tabular-nums; letter-spacing: -.01em; }
  .kpi span { font-size: 11.5px; color: var(--faint); }
  .up { color: var(--ok) !important; }
  .down { color: var(--err) !important; }
  .note { margin: -6px 0 0; font-size: 11.5px; }
  .two { display: grid; grid-template-columns: repeat(auto-fit, minmax(360px, 1fr)); gap: 14px; }
  .seg.mini button { padding: 2px 8px; font-size: 11.5px; }
  .opps { display: flex; flex-direction: column; gap: 6px; }
  .ev { display: flex; align-items: baseline; gap: 8px; font-size: 12.5px; margin-top: 8px; }
  .ev-dim { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; margin: 4px 0 0 2px; }
  .ev-dim small { color: var(--dim); font-size: 11.5px; }
</style>
