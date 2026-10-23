<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site, Opportunity } from '$lib/types';
  import LineChart from './LineChart.svelte';
  import OpportunityRow from './OpportunityRow.svelte';
  import SiteReports from './SiteReports.svelte';
  import GoogleMark from '$lib/components/icons/GoogleMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import GaMark from '$lib/components/icons/GaMark.svelte';
  import { ago as agoTs, num, pct as pctF, delta as dlt } from './fmt';
  import { sites, loadDashboard, refreshSiteData } from '$lib/state/sites.svelte.js';

  let { site }: { site: Site } = $props();
  const d = $derived(sites.dash[site.id]);
  const busy = $derived(!!sites.dataBusy[site.id]);
  const hasSources = $derived(!!(site.data.ga4_property || site.data.gsc_site || site.data.bing_site));
  $effect(() => { const id = site.id; void loadDashboard(id).then(() => { if (hasSources) void refreshSiteData(id, false); }); });

  let qTab = $state<'gsc' | 'bing'>('gsc');
  let pTab = $state<'gsc' | 'ga4' | 'bing'>('gsc');
  const n = num;
  const pct = pctF;
  const md = (iso: string) => { const m = /^\d{4}-(\d{2})-(\d{2})$/.exec(iso); return m ? `${+m[1]}月${+m[2]}日` : iso; };
  const fetched = $derived(Math.max(d?.meta.gsc.fetched_at ?? 0, d?.meta.ga4.fetched_at ?? 0, d?.meta.bing.fetched_at ?? 0));
  const path = (u: string) => { try { const x = new URL(u); return x.pathname + x.search; } catch { return u; } };
  const events = $derived((d?.ga4_events ?? []).filter((e) => !['page_view', 'session_start', 'first_visit', 'user_engagement', 'scroll'].includes(e.event)));
  const evNames = $derived([...new Set(events.filter((e) => !e.dim).map((e) => e.event))]);
  const opps = $derived<Opportunity[]>(d?.opportunities ?? []);
  const prevOf = (r: object) => (r as { prev_clicks?: number }).prev_clicks;
</script>

<div class="dt">
  {#if !hasSources}
    <div class="card">
      <h3><Icon name="database" />还没有接入数据源</h3>
      <p class="hint">在站点设置里接入 Google（GA4 + Search Console）和 Bing 后，这里会显示趋势、查询词、页面和 Pilot 按规则找出的运营机会。</p>
      <div class="row"><button class="btn small primary" onclick={() => (sites.tab = 'settings')}>去接入</button></div>
    </div>
  {:else}
    <div class="bar">
      <span class="faint">近 28 天，与前 28 天对比 · {fetched ? `${agoTs(fetched)}同步` : '还没同步过'} · 每天早上 7 点自动同步</span>
      <button class="ic-btn" onclick={() => refreshSiteData(site.id, true)} disabled={busy} aria-label="同步数据" data-tip={busy ? '同步中…' : '同步数据：从 Search Console、GA4 和 Bing 重新拉取'}>{#if busy}<span class="spin"></span>{:else}<Icon name="refresh" size={15} />{/if}</button>
    </div>
    {#if d?.meta.gsc.error}<div class="err-note src"><GoogleMark size={13} />Search Console：{d.meta.gsc.error}</div>{/if}
    {#if d?.meta.ga4.error}<div class="err-note src"><GaMark size={13} />GA4：{d.meta.ga4.error}</div>{/if}
    {#if d?.meta.bing.error}<div class="err-note src"><BingMark size={13} />Bing：{d.meta.bing.error}</div>{/if}

    {#if d}
      <div class="card groups" style="--g:{[site.data.gsc_site && '4fr', site.data.ga4_property && '2fr', site.data.bing_site && '1.2fr'].filter(Boolean).join(' ')}">
        {#if site.data.gsc_site}
          {@const moved = d.gsc_prev_totals.position > 0 ? d.gsc_prev_totals.position - d.gsc_totals.position : 0}
          <section class="grp" style="--n:4">
            <header><GoogleMark size={12} />Google 搜索<span class="faint">{d.meta.gsc.end ? `截至 ${md(d.meta.gsc.end)}` : ''}</span></header>
            <div class="stats">
              {#each [
                { l: '点击', v: n(d.gsc_totals.clicks), dl: dlt(d.gsc_totals.clicks, d.gsc_prev_totals.clicks) },
                { l: '展现', v: n(d.gsc_totals.impressions), dl: dlt(d.gsc_totals.impressions, d.gsc_prev_totals.impressions) },
                { l: '点击率', v: pct(d.gsc_totals.ctr), dl: dlt(d.gsc_totals.ctr, d.gsc_prev_totals.ctr) },
              ] as k (k.l)}
                <div class="stat"><small>{k.l}</small><b>{k.v}</b><em class:up={k.dl.dir > 0} class:down={k.dl.dir < 0}>{k.dl.text || '—'}</em></div>
              {/each}
              <div class="stat"><small>平均排名</small><b>{d.gsc_totals.position.toFixed(1)}</b><em class:up={moved > 0.05} class:down={moved < -0.05}>{moved > 0.05 ? `↑${moved.toFixed(1)} 位` : moved < -0.05 ? `↓${(-moved).toFixed(1)} 位` : '持平'}</em></div>
            </div>
          </section>
        {/if}
        {#if site.data.ga4_property}
          {@const v = dlt(d.ga4_totals.views, d.ga4_prev_views)}
          <section class="grp" style="--n:2">
            <header><GaMark size={13} />GA4 站内访问</header>
            <div class="stats">
              <div class="stat"><small>浏览量</small><b>{n(d.ga4_totals.views)}</b><em class:up={v.dir > 0} class:down={v.dir < 0}>{v.text || '—'}</em></div>
              <div class="stat"><small>会话</small><b>{n(d.ga4_totals.sessions)}</b><em>用户 {n(d.ga4_totals.users)}</em></div>
            </div>
          </section>
        {/if}
        {#if site.data.bing_site}
          <section class="grp" style="--n:1">
            <header><BingMark size={12} />Bing<span class="faint">{d.meta.bing.end ? `截至 ${md(d.meta.bing.end)}` : ''}</span></header>
            <div class="stats">
              <div class="stat"><small>点击</small><b>{n(d.bing_totals.clicks)}</b><em>展现 {n(d.bing_totals.impressions)}</em></div>
            </div>
          </section>
        {/if}
      </div>
      <p class="faint note src"><GoogleMark size={11} />Search Console 数据有 2–3 天延迟。</p>

      {#if d.gsc_daily.length || d.ga4_daily.length}
        <div class="card">
          <h3><Icon name="trend" />趋势 <span class="sub">近 90 天</span></h3>
          {#if d.gsc_daily.length}
            <div class="chart-src src"><GoogleMark size={12} />Google 搜索</div>
            <LineChart labels={d.gsc_daily.map((r) => r.key)} series={[
              { label: '点击', color: '#2f6fd8', values: d.gsc_daily.map((r) => r.clicks) },
              { label: '展现', color: '#9b8ee0', values: d.gsc_daily.map((r) => r.impressions) },
            ]} />
          {/if}
          {#if d.ga4_daily.length}
            <div class="chart-src src" style="margin-top:18px"><GaMark size={12} />GA4 站内访问</div>
            <div><LineChart height={110} labels={d.ga4_daily.map((r) => r.key)} series={[
              { label: '会话', color: '#12805c', values: d.ga4_daily.map((r) => r.sessions) },
              { label: '浏览量', color: '#b45309', values: d.ga4_daily.map((r) => r.views) },
            ]} /></div>
          {/if}
        </div>
      {/if}

      <div class="card">
        <h3><Icon name="bulb" />运营机会 <span class="sub">按固定规则从数据里算出，可复现</span></h3>
        {#if opps.length}
          <div class="list">{#each opps.slice(0, 12) as o (o.key)}<OpportunityRow {site} opp={o} />{/each}</div>
        {:else}
          <p class="hint">暂时没有发现明显的机会（或数据还不够）。小站可以在「设置 → 数据源」里调低机会阈值。</p>
        {/if}
      </div>

      <div class="two">
        <div class="card">
          <h3><Icon name="search" />查询词 <span class="grow"></span>
            {#if site.data.bing_site && site.data.gsc_site}<span class="seg mini"><button class="src" class:on={qTab === 'gsc'} onclick={() => (qTab = 'gsc')}><GoogleMark size={11} />Google</button><button class="src" class:on={qTab === 'bing'} onclick={() => (qTab = 'bing')}><BingMark size={11} />Bing</button></span>{/if}
          </h3>
          <table class="tbl">
            <thead><tr><th>查询词</th><th class="num">点击</th><th class="num">展现</th><th class="num">点击率</th><th class="num">排名</th></tr></thead>
            <tbody>
              {#each (qTab === 'gsc' && site.data.gsc_site ? d.gsc_queries : d.bing_queries).slice(0, 25) as r (r.key)}
                <tr>
                  <td class="trunc" title={r.key}>{r.key}</td>
                  <td class="num">{n(r.clicks)}{#if prevOf(r) !== undefined && prevOf(r) !== r.clicks}<small class="tr" class:up={r.clicks > (prevOf(r) ?? 0)} class:down={r.clicks < (prevOf(r) ?? 0)}>{r.clicks > (prevOf(r) ?? 0) ? '↑' : '↓'}</small>{/if}</td>
                  <td class="num">{n(r.impressions)}</td>
                  <td class="num">{pct(r.ctr)}</td>
                  <td class="num">{r.position.toFixed(1)}</td>
                </tr>
              {:else}<tr><td colspan="5" class="faint">没有数据</td></tr>{/each}
            </tbody>
          </table>
        </div>
        <div class="card">
          <h3><Icon name="file" />页面 <span class="grow"></span>
            <span class="seg mini">
              {#if site.data.gsc_site}<button class="src" class:on={pTab === 'gsc'} onclick={() => (pTab = 'gsc')}><GoogleMark size={11} />Google</button>{/if}
              {#if site.data.ga4_property}<button class="src" class:on={pTab === 'ga4'} onclick={() => (pTab = 'ga4')}><GaMark size={11} />GA4</button>{/if}
              {#if site.data.bing_site}<button class="src" class:on={pTab === 'bing'} onclick={() => (pTab = 'bing')}><BingMark size={11} />Bing</button>{/if}
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
            <h3><Icon name="route" />流量来源 <span class="sub src"><GaMark size={11} />GA4 · 近 28 天</span></h3>
            <table class="tbl">
              <thead><tr><th>来源 / 媒介</th><th class="num">会话</th><th class="num">用户</th></tr></thead>
              <tbody>{#each d.ga4_sources.slice(0, 15) as r (r.key)}<tr><td class="trunc">{r.key}</td><td class="num">{n(r.sessions)}</td><td class="num">{n(r.users)}</td></tr>{:else}<tr><td colspan="3" class="faint">没有数据</td></tr>{/each}</tbody>
            </table>
          </div>
          <div class="card">
            <h3><Icon name="target" />转化事件 <span class="sub src"><GaMark size={11} />GA4 · 近 28 天</span></h3>
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
  <SiteReports {site} />
</div>

<style>
  .dt { display: flex; flex-direction: column; gap: 14px; }
  .bar { display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap; font-size: 12px; }
  .groups { display: grid; grid-template-columns: var(--g); padding: 0; overflow: hidden; }
  .grp { min-width: 0; padding: 14px 18px 16px; border-left: 1px solid var(--border); }
  .grp:first-child { border-left: none; }
  .grp header { display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; margin-bottom: 10px; white-space: nowrap; }
  .grp header .faint { font-weight: 400; margin-left: 2px; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .grp header :global(svg) { flex: none; }
  .grp .stats { grid-template-columns: repeat(var(--n), minmax(0, 1fr)); gap: 12px 16px; }
  @container page (max-width: 900px) {
    .groups { grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); }
    .grp:first-child { grid-column: 1 / -1; border-bottom: 1px solid var(--border); }
    .grp:nth-child(2) { border-left: none; }
  }
  .chart-src { font-size: 12px; color: var(--dim); margin: 2px 0 4px; }
  .note { margin: -6px 0 0; font-size: 11.5px; }
  .two :global(table.tbl th:first-child) { width: 44%; }
  .two { display: grid; grid-template-columns: repeat(auto-fit, minmax(360px, 1fr)); gap: 14px; }
  .seg.mini button { padding: 2px 8px; font-size: 11.5px; }
  .tr { font-size: 10.5px; margin-left: 2px; }
  .ev { display: flex; align-items: baseline; gap: 8px; font-size: 12.5px; margin-top: 8px; }
  .ev-dim { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; margin: 4px 0 0 2px; }
  .ev-dim small { color: var(--dim); font-size: 11.5px; }
</style>
