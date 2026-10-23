<script lang="ts">
  import './site.css';
  import type { SiteRow } from '$lib/types';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { openSite, openSiteImport } from '$lib/state/sites.svelte.js';
  import { fleet, applyFilter, sortRows, needsAttention, trafficDown, clicksChange, setPinned, loadPending, PAGE_SIZE, type SiteSort } from '$lib/state/fleet.svelte.js';
  import { LEVEL_LABEL, STATUS_LABEL, KIND_LABEL } from '$lib/state/ops.svelte.js';

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
  const n = (v: number) => Math.round(v).toLocaleString();
  const when = (ts: number) => { if (!ts) return '—'; const d = Math.round((Date.now() / 1000 - ts) / 86400); return d <= 0 ? '今天' : d === 1 ? '昨天' : `${d} 天前`; };
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

<div class="su ov">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="ov-head" data-tauri-drag-region onmousedown={startDrag}>
    <div>
      <h1>全部站点</h1>
      <p class="hint">{totals.sites} 个站点 · 近 28 天 Google 点击 {n(totals.clicks)}{totals.prev ? `（${totals.clicks >= totals.prev ? '+' : ''}${Math.round(((totals.clicks - totals.prev) / totals.prev) * 100)}%）` : ''}</p>
    </div>
    <div class="row">
      <button class="btn small primary" onclick={() => openSiteImport()}>导入站点</button>
    </div>
  </header>

  <div class="ov-tabs">
    <button class:on={fleet.tab === 'sites'} onclick={() => (fleet.tab = 'sites')}>站点</button>
    <button class:on={fleet.tab === 'pending'} onclick={() => (fleet.tab = 'pending')}>待确认{#if totals.proposed}<span class="cnt">{totals.proposed}</span>{/if}</button>
  </div>

  <div class="ov-body">
    {#if fleet.tab === 'sites'}
      <div class="ov-tools">
        <input class="tin" style="max-width:280px" bind:value={fleet.query} placeholder="搜索名称、域名、仓库、标签" spellcheck="false" />
        <div class="chips">{#each FILTERS as f (f.v)}<button class:on={fleet.filter === f.v} onclick={() => (fleet.filter = f.v)}>{f.l}</button>{/each}</div>
        {#if fleet.tags.length}
          <select class="tin" style="width:auto" bind:value={fleet.tag}><option value="">全部标签</option>{#each fleet.tags as t (t)}<option value={t}>#{t}</option>{/each}</select>
        {/if}
      </div>
      {#if !fleet.rows.length}
        <div class="card"><p class="hint">还没有站点。导入一个用代码管理的静态站，或在 Cloudflare 建站会话里点「转为站点」。</p></div>
      {:else}
        <div class="card tbl-card">
          <table class="tbl">
            <thead><tr>
              <th style="width:26px"></th>
              <th><button class="th" onclick={() => sortBy('name')}>站点{arrow('name')}</button></th>
              <th class="num"><button class="th" onclick={() => sortBy('clicks')}>Google 点击{arrow('clicks')}</button></th>
              <th class="num"><button class="th" onclick={() => sortBy('change')}>变化{arrow('change')}</button></th>
              <th class="num">机会</th>
              <th><button class="th" onclick={() => sortBy('pending')}>状态{arrow('pending')}</button></th>
              <th>档位</th>
              <th><button class="th" onclick={() => sortBy('active')}>最近活动{arrow('active')}</button></th>
            </tr></thead>
            <tbody>
              {#each pageRows as r (r.id)}
                {@const st = status(r)}
                {@const ch = clicksChange(r)}
                <tr class="clickable" onclick={() => openSite(r.id)}>
                  <td><button class="pin" class:on={r.pinned} title={r.pinned ? '取消置顶' : '置顶'} onclick={(e) => { e.stopPropagation(); void setPinned(r.id, !r.pinned); }}>★</button></td>
                  <td class="site-cell">
                    <b>{r.name}</b>
                    <small>{r.domain && r.domain !== r.name ? r.domain : r.source === 'github' ? r.repo : '本地文件夹'}{#each r.tags as t (t)}<span class="tag">#{t}</span>{/each}</small>
                  </td>
                  <td class="num">{r.has_data && r.summary.fetched_at ? n(r.summary.gsc_clicks) : '—'}</td>
                  <td class="num"><span class:up={ch > 0.05} class:down={ch < -0.05}>{r.has_data && r.summary.gsc_clicks_prev ? `${ch > 0 ? '+' : ''}${Math.round(ch * 100)}%` : '—'}</span></td>
                  <td class="num">{(r.has_data && r.summary.opportunities) || '—'}</td>
                  <td><span class="pill {st.c}">{st.t}</span>{#if !r.has_data}<span class="faint" style="font-size:11px"> · 未接数据</span>{/if}</td>
                  <td class="faint" style="font-size:12px">{LEVEL_LABEL[r.level]}</td>
                  <td class="faint" style="font-size:12px">{when(r.active_at)}</td>
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
            <thead><tr><th>站点</th><th>事项</th><th>状态</th><th>更新</th></tr></thead>
            <tbody>
              {#each fleet.pending as p (p.item.id)}
                <tr class="clickable" onclick={() => openSite(p.site_id, 'queue')}>
                  <td class="trunc" style="max-width:160px">{p.site_name}</td>
                  <td class="trunc"><span class="faint">{KIND_LABEL[p.item.opp.kind] ?? ''}</span> {p.item.opp.title}</td>
                  <td><span class="pill {p.item.status === 'failed' ? 'err' : 'warn'}">{STATUS_LABEL[p.item.status]}</span></td>
                  <td class="faint" style="font-size:12px">{when(p.item.updated_at)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .ov { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .ov-head { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; padding: 34px 28px 8px; }
  .ov-head h1 { margin: 0; font-size: 20px; }
  .ov-head .hint { margin: 2px 0 0; }
  .ov-tabs { display: flex; gap: 2px; padding: 0 22px; border-bottom: 1px solid var(--border); }
  .ov-tabs button { position: relative; border: none; background: none; padding: 8px 10px 9px; font: inherit; font-size: 13px; color: var(--dim); cursor: pointer; }
  .ov-tabs button.on { color: var(--text); font-weight: 600; }
  .ov-tabs button.on::after { content: ''; position: absolute; left: 8px; right: 8px; bottom: -1px; height: 2px; border-radius: 2px; background: var(--accent); }
  .cnt { margin-left: 5px; display: inline-flex; min-width: 16px; height: 16px; padding: 0 4px; box-sizing: border-box; align-items: center; justify-content: center; border-radius: 999px; background: var(--accent); color: #fff; font-size: 10px; font-weight: 700; }
  .ov-body { flex: 1; min-height: 0; overflow-y: auto; padding: 16px 28px 40px; display: flex; flex-direction: column; gap: 12px; }
  .ov-tools { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .chips { display: flex; gap: 3px; flex-wrap: wrap; }
  .chips button { border: none; background: none; padding: 4px 9px; border-radius: 7px; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; }
  .chips button:hover { background: #f1efe9; }
  .chips button.on { background: var(--accent-soft); color: var(--text); font-weight: 550; }
  .tbl-card { padding: 4px 6px; }
  .th { border: none; background: none; padding: 0; font: inherit; color: inherit; cursor: pointer; }
  tr.clickable { cursor: pointer; }
  tr.clickable:hover td { background: #faf9f6; }
  .site-cell b { display: block; font-size: 12.5px; }
  .site-cell small { display: flex; gap: 6px; font-size: 11px; color: var(--faint); }
  .tag { color: var(--dim); }
  .pin { border: none; background: none; padding: 0; color: var(--border2); cursor: pointer; font-size: 13px; }
  .pin.on { color: #c28a12; }
  .up { color: var(--ok); }
  .down { color: var(--err); }
  .pager { gap: 8px; }
</style>
