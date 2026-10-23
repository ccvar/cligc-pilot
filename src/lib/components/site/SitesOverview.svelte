<script lang="ts">
  import './site.css';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import CfMark from '$lib/components/icons/CfMark.svelte';
  import type { SiteRow } from '$lib/types';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { openSite, openSiteImport } from '$lib/state/sites.svelte.js';
  import { fleet, applyFilter, sortRows, needsAttention, trafficDown, clicksChange, setPinned, loadPending, PAGE_SIZE, type SiteSort } from '$lib/state/fleet.svelte.js';
  import { LEVEL_LABEL, STATUS_LABEL, KIND_LABEL } from '$lib/state/ops.svelte.js';
  import { ago, num, delta } from './fmt';

  const filtered = $derived(sortRows(applyFilter(fleet.rows, fleet.query, fleet.filter, fleet.tag), fleet.sort, fleet.desc));
  const pages = $derived(Math.max(1, Math.ceil(filtered.length / PAGE_SIZE)));
  const pageRows = $derived(filtered.slice(fleet.page * PAGE_SIZE, (fleet.page + 1) * PAGE_SIZE));
  $effect(() => { void fleet.query; void fleet.filter; void fleet.tag; fleet.page = 0; });
  $effect(() => { if (fleet.tab === 'pending') void loadPending(); });

  const totals = $derived({
    sites: fleet.rows.length,
    clicks: fleet.rows.reduce((n, r) => n + r.summary.gsc_clicks, 0),
    prev: fleet.rows.reduce((n, r) => n + r.summary.gsc_clicks_prev, 0),
    proposed: fleet.pendingCount,
    failed: fleet.rows.filter((r) => r.last_deploy_ok === false || r.failed > 0).length,
    down: fleet.rows.filter(trafficDown).length,
    nodata: fleet.rows.filter((r) => !r.has_data).length,
  });
  const n = num;
  const when = ago;
  const total = $derived(delta(totals.clicks, totals.prev));
  const MODE: Record<string, string> = { direct: 'Cloudflare Pages', workers: 'Cloudflare Workers', vps: 'VPS', pages_git: 'Pages · Git 自动构建', none: '未配置部署' };
  const lvlName = (l: number) => (LEVEL_LABEL[l] ?? '').replace(/^\d+\s*·\s*/, '');
  function sortBy(k: SiteSort) { if (fleet.sort === k) fleet.desc = !fleet.desc; else { fleet.sort = k; fleet.desc = false; } }
  const arrow = (k: SiteSort) => (fleet.sort === k ? (fleet.desc ? ' ↑' : ' ↓') : '');
  const FILTERS = $derived([
    { v: 'all' as const, l: `全部 ${totals.sites}` },
    { v: 'attention' as const, l: `需关注 ${fleet.attentionCount}` },
    { v: 'pinned' as const, l: '置顶' },
    { v: 'failed' as const, l: `失败 ${totals.failed}` },
    { v: 'down' as const, l: `流量下滑 ${totals.down}` },
    { v: 'nodata' as const, l: `未接数据 ${totals.nodata}` },
  ]);
  const status = (r: SiteRow) => r.job ? { t: r.job + '中', c: 'run' } : r.proposed ? { t: `${r.proposed} 条待确认`, c: 'warn' } : r.last_deploy_ok === false ? { t: '上线失败', c: 'err' } : r.failed ? { t: `${r.failed} 条失败`, c: 'err' } : r.summary.errors.length ? { t: '数据源出错', c: 'err' } : { t: '正常', c: '' };
</script>

<!-- 找站点走顶部的全局搜索（名称 / 域名 / 仓库 / #标签，直接跳转），这里只留筛选。 -->
<div class="su page ov">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="page-head" data-tauri-drag-region onmousedown={startDrag}>
    <div class="ttl">
      <h1>全部站点</h1>
      <p class="hint">{totals.sites} 个站点 · 近 28 天 Google 点击 {n(totals.clicks)}{#if total.text} <span class:up={total.dir > 0} class:down={total.dir < 0}>{total.text}</span>{/if}</p>
    </div>
    <div class="row">
      <button class="btn small primary" onclick={() => openSiteImport()}><Icon name="upload" size={13} />导入站点</button>
    </div>
  </header>

  <div class="tabs">
    <button class:on={fleet.tab === 'sites'} onclick={() => (fleet.tab = 'sites')}><Icon name="globe" size={13} />站点</button>
    <button class:on={fleet.tab === 'pending'} onclick={() => (fleet.tab = 'pending')}><Icon name="inbox" size={13} />待确认{#if totals.proposed}<span class="cnt">{totals.proposed}</span>{/if}</button>
  </div>

  <div class="page-body">
   {#key fleet.tab}
   <div class="pane">
    {#if fleet.tab === 'sites'}
      <div class="ov-tools">
        <div class="chips">{#each FILTERS as f (f.v)}<button class:on={fleet.filter === f.v} onclick={() => (fleet.filter = f.v)}>{f.l}</button>{/each}</div>
        {#if fleet.tags.length}
          <select class="tin tagsel" bind:value={fleet.tag}><option value="">全部标签</option>{#each fleet.tags as t (t)}<option value={t}>#{t}</option>{/each}</select>
        {/if}
      </div>
      {#if !fleet.rows.length}
        <div class="card"><p class="hint">还没有站点。导入一个用代码管理的静态站，或在 Cloudflare 建站会话里点「转为站点」。</p></div>
      {:else}
        <div class="card tbl-card">
          <table class="tbl">
            <thead><tr>
              <th class="pin-col"></th>
              <th><button class="th" onclick={() => sortBy('name')}>站点{arrow('name')}</button></th>
              <th class="num"><button class="th" onclick={() => sortBy('clicks')}>Google 点击{arrow('clicks')}</button></th>
              <th class="num"><button class="th" onclick={() => sortBy('change')}>变化{arrow('change')}</button></th>
              <th class="num c-opp">机会</th>
              <th><button class="th" onclick={() => sortBy('pending')}>状态{arrow('pending')}</button></th>
              <th class="c-lvl">档位</th>
              <th class="c-when"><button class="th" onclick={() => sortBy('active')}>最近活动{arrow('active')}</button></th>
            </tr></thead>
            <tbody>
              {#each pageRows as r (r.id)}
                {@const st = status(r)}
                {@const ch = clicksChange(r)}
                <tr class="clickable" onclick={() => openSite(r.id)}>
                  <td><button class="pin" class:on={r.pinned} title={r.pinned ? '取消置顶' : '置顶'} onclick={(e) => { e.stopPropagation(); void setPinned(r.id, !r.pinned); }}>★</button></td>
                  <td class="site-cell">
                    <b><SiteIcon id={r.id} size={15} /><span class="nm">{r.name}</span>{#if r.domain}<button class="ext" onclick={(e) => { e.stopPropagation(); void openUrl(`https://${r.domain}`); }} aria-label="打开 {r.domain}" data-tip="打开 {r.domain}"><Icon name="external" size={11} /></button>{/if}</b>
                    <small>{#if r.domain && r.domain !== r.name}<span>{r.domain}</span>{/if}<span class="dm">{#if r.deploy_mode === 'vps'}<Icon name="server" size={11} />{:else if r.deploy_mode !== 'none'}<CfMark size={12} />{/if}{MODE[r.deploy_mode] ?? r.deploy_mode}</span>{#if r.source === 'github'}<span>{r.repo}</span>{/if}{#each r.tags as t (t)}<span class="tag">#{t}</span>{/each}</small>
                  </td>
                  {#if r.has_data}
                    <td class="num">{r.summary.fetched_at ? n(r.summary.gsc_clicks) : '—'}</td>
                    <td class="num"><span class:up={ch > 0.05} class:down={ch < -0.05}>{r.summary.gsc_clicks_prev ? `${ch > 0 ? '↑' : ch < 0 ? '↓' : ''}${Math.abs(Math.round(ch * 100))}%` : '—'}</span></td>
                    <td class="num c-opp">{r.summary.opportunities || '—'}</td>
                  {:else}
                    <td class="num nodata" colspan="2">未接数据</td><td class="c-opp"></td>
                  {/if}
                  <td><span class="pill {st.c}">{#if st.c === 'run'}<span class="spin"></span>{/if}{st.t}</span></td>
                  <td class="c-lvl"><span class="lvl" title={LEVEL_LABEL[r.level]}><span class="bars">{#each [1, 2, 3, 4, 5] as i (i)}<i class:on={i <= r.level}></i>{/each}</span><span class="lvl-t">{lvlName(r.level)}</span></span></td>
                  <td class="faint when c-when">{when(r.active_at)}</td>
                </tr>
              {:else}
                <tr><td colspan="8" class="faint">没有符合条件的站点</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
        {#if pages > 1}
          <div class="row-end pager">
            <span class="faint" style="font-size:12px">{filtered.length} 个 · 第 {fleet.page + 1} / {pages} 页</span>
            <button class="btn tiny" onclick={() => (fleet.page = Math.max(0, fleet.page - 1))} disabled={fleet.page === 0}>上一页</button>
            <button class="btn tiny" onclick={() => (fleet.page = Math.min(pages - 1, fleet.page + 1))} disabled={fleet.page >= pages - 1}>下一页</button>
          </div>
        {/if}
        {#if fleet.rows.some(needsAttention) && fleet.filter === 'all'}
          <p class="hint">「需关注」= 有待确认或失败的运营条目、上次上线失败、点击比前 28 天跌了 20% 以上，或数据源出错。</p>
        {/if}
      {/if}
    {:else}
      {#if !fleet.pending.length}
        <div class="card"><p class="hint">所有站点都没有需要你确认的改动。</p></div>
      {:else}
        <div class="card tbl-card">
          <table class="tbl">
            <thead><tr><th style="width:170px">站点</th><th>事项</th><th style="width:84px">状态</th><th style="width:96px">更新</th></tr></thead>
            <tbody>
              {#each fleet.pending as p (p.item.id)}
                <tr class="clickable" onclick={() => openSite(p.site_id, 'queue')}>
                  <td class="sn-cell"><b class="sn"><SiteIcon id={p.site_id} size={13} />{p.site_name}</b></td>
                  <td class="trunc">{p.item.opp.title}<small class="kind">{KIND_LABEL[p.item.opp.kind] ?? ''}</small></td>
                  <td><span class="pill {p.item.status === 'failed' ? 'err' : 'warn'}">{STATUS_LABEL[p.item.status]}</span></td>
                  <td class="faint when">{when(p.item.updated_at)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
   </div>
   {/key}
  </div>
</div>

<style>
  .ov .page-body > .pane { max-width: 1180px; gap: 12px; }
  .ov-tools { display: flex; align-items: center; gap: 8px 12px; flex-wrap: wrap; }
  .tagsel { width: auto; height: var(--ctl-h); padding: 0 8px; }
  .chips { display: flex; gap: 2px; flex-wrap: wrap; }
  .chips button { border: none; background: none; height: 26px; padding: 0 9px; border-radius: 7px; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; font-variant-numeric: tabular-nums; transition: background-color .12s, color .12s; }
  .chips button:hover:not(.on) { background: var(--su-hover); color: var(--text); }
  .chips button.on { background: var(--accent-soft); color: var(--text); font-weight: 550; }
  .tbl-card { padding: 2px 16px; overflow-x: auto; }
  .tbl-card table.tbl td { vertical-align: middle; padding-top: 9px; padding-bottom: 9px; }
  .pin-col { width: 22px; }
  .th { border: none; background: none; padding: 0; font: inherit; color: inherit; cursor: pointer; transition: color .12s; }
  .th:hover { color: var(--text); }
  tr.clickable { cursor: pointer; }
  .site-cell b { display: block; font-size: 13px; font-weight: 600; }
  .site-cell { max-width: 0; width: 34%; }
  .site-cell b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .site-cell b { display: flex !important; align-items: center; gap: 8px; min-width: 0; }
  .site-cell .nm { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ext { flex: none; width: 20px; height: 20px; margin-left: -3px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 5px; color: var(--faint); cursor: pointer; opacity: .55; transition: opacity .12s, background-color .12s, color .12s; }
  tr:hover .ext { opacity: 1; }
  .ext:hover { background: var(--su-press); color: var(--text); }
  .site-cell small { padding-left: 23px; display: flex; flex-wrap: nowrap; overflow: hidden; white-space: nowrap; gap: 0 8px; font-size: 11.5px; color: var(--faint); margin-top: 1px; }
  .site-cell small span + span:not(.tag)::before { content: '·'; margin-right: 8px; color: var(--border2); }
  .tag { color: var(--dim); }
  .dm { display: inline-flex; align-items: center; gap: 5px; }
  .dm :global(svg) { flex: none; }
  .dm :global(.ico) { color: var(--dim); }
  .nodata { color: var(--faint); font-size: 12px; }
  .lvl { display: inline-flex; align-items: center; gap: 7px; font-size: 12px; color: var(--dim); white-space: nowrap; }
  .bars { display: inline-flex; gap: 2px; align-items: flex-end; }
  .bars i { width: 3px; border-radius: 1px; background: var(--border2); }
  .bars i:nth-child(1) { height: 5px; } .bars i:nth-child(2) { height: 7px; } .bars i:nth-child(3) { height: 9px; } .bars i:nth-child(4) { height: 11px; } .bars i:nth-child(5) { height: 13px; }
  .bars i.on { background: var(--accent); }
  .when { font-size: 12px; white-space: nowrap; }
  .pin { border: none; background: none; padding: 2px; color: var(--border2); cursor: pointer; font-size: 13px; line-height: 1; border-radius: 4px; transition: color .12s, transform .12s; }
  .pin:hover { color: var(--dim); transform: scale(1.12); }
  .pin.on { color: #c28a12; }
  .sn-cell { max-width: 170px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sn { font-weight: 600; font-size: 12.5px; display: inline-flex; align-items: center; gap: 7px; }
  .kind { margin-left: 8px; color: var(--faint); font-size: 11.5px; }
  .pager { gap: 8px; }
  @container page (max-width: 760px) {
    .c-when { display: none; }
    .lvl-t { display: none; }
  }
  @container page (max-width: 560px) {
    .c-opp, .c-lvl { display: none; }
  }
</style>
