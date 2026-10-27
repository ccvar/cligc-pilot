<script lang="ts">
  import './site.css';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import CfMark from '$lib/components/icons/CfMark.svelte';
  import GscMark from '$lib/components/icons/GscMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import GaMark from '$lib/components/icons/GaMark.svelte';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import ConnectionMark from '$lib/components/icons/ConnectionMark.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import type { SiteRow, ChannelBrief } from '$lib/types';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { openSite, openSiteImport } from '$lib/state/sites.svelte.js';
  import { openSiteCreate, isMaking } from '$lib/state/siteCreate.svelte.js';
  import { fleet, applyFilter, sortRows, change, searchCtr, loadPending, PAGE_SIZE, ISSUE_GROUPS, issueRows, ignoreIssues, type SiteSort } from '$lib/state/fleet.svelte.js';
  import { STATUS_LABEL, KIND_LABEL } from '$lib/state/ops.svelte.js';
  import { AUTONOMY_LABEL } from '$lib/state/managed.svelte.js';
  import { ago, full, num } from './fmt';

  // 制作中的站点在「站点制作」里，不进全部站点的列表和统计
  const rows = $derived(fleet.rows.filter((r) => !isMaking(r.id)));
  const filtered = $derived(sortRows(applyFilter(rows, fleet.query, fleet.filter, fleet.tag).filter((r) => !fleet.mode || r.deploy_mode === fleet.mode), fleet.sort, fleet.desc));
  const pages = $derived(Math.max(1, Math.ceil(filtered.length / PAGE_SIZE)));
  const pageRows = $derived(filtered.slice(fleet.page * PAGE_SIZE, (fleet.page + 1) * PAGE_SIZE));
  $effect(() => { void fleet.query; void fleet.filter; void fleet.tag; void fleet.mode; fleet.page = 0; });
  $effect(() => { if (fleet.tab === 'pending') void loadPending(); });

  const totals = $derived({
    sites: rows.length,
    proposed: fleet.pendingCount,
  });
  const n = num;
  const when = ago;
  const MODE: Record<string, string> = { direct: 'Cloudflare Pages', workers: 'Cloudflare Workers', vps: 'VPS', pages_git: 'Pages · Git 自动构建', none: '未配置部署' };
  // 站点后面的下拉：按部署方式筛选（只列出现在有站点的方式，带个数）
  const MODE_ORDER = ['direct', 'workers', 'pages_git', 'vps', 'none'];
  const CF_SVG = '<svg viewBox="0 0 128 128"><path fill="#f38020" d="M87.295 89.022c.763-2.617.472-5.015-.8-6.796c-1.163-1.635-3.125-2.58-5.488-2.689l-44.737-.581c-.291 0-.545-.145-.691-.363s-.182-.509-.109-.8c.145-.436.581-.763 1.054-.8l45.137-.581c5.342-.254 11.157-4.579 13.192-9.885l2.58-6.723c.109-.291.145-.581.073-.872c-2.906-13.158-14.644-22.97-28.672-22.97c-12.938 0-23.913 8.359-27.838 19.952a13.35 13.35 0 0 0-9.267-2.58c-6.215.618-11.193 5.597-11.811 11.811c-.145 1.599-.036 3.162.327 4.615C10.104 70.051 2 78.337 2 88.549c0 .909.073 1.817.182 2.726a.895.895 0 0 0 .872.763h82.57c.472 0 .909-.327 1.054-.8z"/><path fill="#faae40" d="M101.542 60.275c-.4 0-.836 0-1.236.036c-.291 0-.545.218-.654.509l-1.744 6.069c-.763 2.617-.472 5.015.8 6.796c1.163 1.635 3.125 2.58 5.488 2.689l9.522.581c.291 0 .545.145.691.363s.182.545.109.8c-.145.436-.581.763-1.054.8l-9.924.582c-5.379.254-11.157 4.579-13.192 9.885l-.727 1.853c-.145.363.109.727.509.727h34.089c.4 0 .763-.254.872-.654c.581-2.108.909-4.325.909-6.614c0-13.447-10.975-24.422-24.458-24.422"/></svg>';
  const line = (d: string) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;
  const MODE_SVG: Record<string, string> = {
    '': line('<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>'),
    direct: CF_SVG, workers: CF_SVG, pages_git: CF_SVG,
    vps: line('<rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/><path d="M6 6h.01"/><path d="M6 18h.01"/>'),
    none: line('<circle cx="12" cy="12" r="9"/><path d="M8 12h8"/>'),
  };
  const modeOpts = $derived([
    { value: '', label: `全部 · ${rows.length}`, short: '部署方式', svg: MODE_SVG[''] },
    ...MODE_ORDER.filter((m) => rows.some((r) => r.deploy_mode === m)).map((m) => ({ value: m, label: `${MODE[m]} · ${rows.filter((r) => r.deploy_mode === m).length}`, short: MODE[m], svg: MODE_SVG[m] })),
  ]);
  function sortBy(k: SiteSort) { if (fleet.sort === k) fleet.desc = !fleet.desc; else { fleet.sort = k; fleet.desc = false; } }
  const arrow = (k: SiteSort) => (fleet.sort === k ? (fleet.desc ? ' ↑' : ' ↓') : '');
  // 右上角的提醒：有问题才出现，点开按问题分组，点一组就只看这组站点
  const issues = $derived(ISSUE_GROUPS.map((g) => ({ ...g, rows: issueRows(rows, g.k) })).filter((g) => g.rows.length));
  const problemSites = $derived(new Set(issues.filter((g) => !g.soft).flatMap((g) => g.rows.map((r) => r.id))).size);
  const onlyLabel = $derived(ISSUE_GROUPS.find((g) => g.k === fleet.filter)?.l ?? '');
  let issuesOpen = $state(false);
  // 弹出列表：点外面关（遮罩），按 Esc 也关
  $effect(() => {
    if (!issuesOpen) return;
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') issuesOpen = false; };
    document.addEventListener('keydown', esc);
    return () => document.removeEventListener('keydown', esc);
  });
  function pickIssue(k: typeof fleet.filter) { fleet.filter = k; fleet.tab = 'sites'; issuesOpen = false; }
  // 分发渠道图标的悬停说明：平台 · 博客名、发布方式、每天上限、已发几篇（失败几篇）
  const CH_KIND: Record<string, string> = { blogger: 'Blogger', wordpress: 'WordPress.com', telegram: 'Telegram' };
  const CH_MODE: Record<string, string> = { manual: '手动', review: '先给我看', draft: '存成草稿', auto: '自动发布' };
  const chTip = (c: ChannelBrief) => [
    `${CH_KIND[c.kind] ?? c.kind} · ${c.target_name || c.target_url}`,
    c.enabled ? `${CH_MODE[c.mode] ?? '先给我看'}${c.mode === 'auto' ? ` · 每天最多 ${c.daily_limit} 篇` : ''}` : '已暂停',
    `已发 ${c.published} 篇${c.failed ? ` · ${c.failed} 篇发布失败` : ''}`,
    '点一下打开分发',
  ].join('\n');
  const status = (r: SiteRow) => r.job ? { t: r.job + '中', c: 'run' } : r.proposed ? { t: `${r.proposed} 条待确认`, c: 'warn' } : r.last_deploy_ok === false ? { t: '上线失败', c: 'err' } : r.failed ? { t: `${r.failed} 条失败`, c: 'err' } : r.links_broken ? { t: `${r.links_broken} 个推广链接失效`, c: 'err' } : r.summary.errors.length ? { t: '数据源出错', c: 'err' } : { t: '正常', c: '' };
</script>

{#snippet delta(cur: number, prev: number)}
  <!-- 前 28 天是 0 算不出变化，就不显示 -->
  {@const d = change(cur, prev)}
  {#if typeof d === 'number' && Math.abs(d) >= 0.005}<span class="dl" class:up={d > 0} class:down={d < 0}>{d > 0 ? '↑' : '↓'}{Math.abs(d * 100) >= 100 ? Math.round(Math.abs(d * 100)) : Math.abs(d * 100).toFixed(1)}%</span>{/if}
{/snippet}

{#snippet spark(vals: number[])}
  {#if vals.length > 1 && vals.some((v) => v > 0)}
    {@const W = 88}
    {@const H = 24}
    {@const top = Math.max(...vals)}
    {@const pts = vals.map((v, i) => `${((i / (vals.length - 1)) * W).toFixed(1)},${(H - 2 - (v / top) * (H - 4)).toFixed(1)}`)}
    <svg class="spark" width={W} height={H} viewBox="0 0 {W} {H}" aria-label="近 28 天会话趋势" data-tip={`近 28 天会话 ${num(vals.reduce((a, b) => a + b, 0))}，最多一天 ${num(top)}`}>
      <polygon points="0,{H} {pts.join(' ')} {W},{H}" fill="rgba(60, 54, 44, .05)" />
      <polyline points={pts.join(' ')} fill="none" stroke="var(--dim)" stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round" />
    </svg>
  {:else}<span class="nodata">—</span>{/if}
{/snippet}

<!-- 找站点走顶部的全局搜索（名称 / 域名 / 仓库 / #标签，直接跳转），这里只留筛选。 -->
<div class="su page ov">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="page-head" data-tauri-drag-region onmousedown={startDrag}>
    <div class="ttl">
      <h1>全部站点</h1>
      <p class="hint">{totals.sites} 个站点</p>
    </div>
    <div class="row">
      {#if issues.length}
        <div class="iss-wrap">
          <button class="iss" class:soft={!problemSites} onclick={() => (issuesOpen = !issuesOpen)} aria-expanded={issuesOpen}>
            {#if problemSites}<Icon name="bell" size={13} />{problemSites} 个站点要看看{:else}<Icon name="database" size={13} />{issues[0].rows.length} 个站点没接数据{/if}
            <svg class="chev" class:open={issuesOpen} width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5 6 7.5 9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
          </button>
          {#if issuesOpen}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="iss-mask" onclick={() => (issuesOpen = false)}></div>
            <div class="iss-pop">
              {#each issues as g (g.k)}
                <div class="iss-row">
                  <button class="iss-g" class:soft={g.soft} onclick={() => pickIssue(g.k)}>
                    <span class="iss-t"><b>{g.l}</b><span class="iss-n">{g.rows.length}</span></span>
                    <span class="iss-sites">{g.rows.slice(0, 3).map((r) => r.domain || r.name).join('、')}{g.rows.length > 3 ? ` 等 ${g.rows.length} 个` : ''}</span>
                  </button>
                  <button class="iss-x" onclick={() => ignoreIssues(g.rows, g.k)} aria-label="忽略这条提醒" data-tip="忽略：情况有变化（比如又失败一次）会再提醒"><Icon name="x" size={12} /></button>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
      {#if fleet.tags.length}
        <select class="tin tagsel" bind:value={fleet.tag}><option value="">全部标签</option>{#each fleet.tags as t (t)}<option value={t}>#{t}</option>{/each}</select>
      {/if}
      <button class="btn small" onclick={() => openSiteImport()}><Icon name="upload" size={13} />导入站点</button>
      <button class="btn small primary" onclick={() => openSiteCreate()}><Icon name="wand" size={13} />制作站点</button>
    </div>
  </header>

  <div class="tabs">
    <button class:on={fleet.tab === 'sites'} onclick={() => (fleet.tab = 'sites')}><Icon name="globe" size={13} />站点</button>
    {#if fleet.tab === 'sites' && modeOpts.length > 2}
      <div class="mode-dd"><Dropdown bare menuCompact menuMin={200} triggerIcon={false} value={fleet.mode} options={modeOpts} ariaLabel="按部署方式筛选" onchange={(v: string) => (fleet.mode = v)} /></div>
    {/if}
    <button class:on={fleet.tab === 'pending'} onclick={() => (fleet.tab = 'pending')}><Icon name="inbox" size={13} />待确认{#if totals.proposed}<span class="cnt">{totals.proposed}</span>{/if}</button>
  </div>

  <div class="page-body">
   {#key fleet.tab}
   <div class="pane">
    {#if fleet.tab === 'sites'}
      {#if fleet.filter !== 'all' || fleet.tag}
        <div class="only">只看：{onlyLabel}{onlyLabel && fleet.tag ? ' · ' : ''}{fleet.tag ? `#${fleet.tag}` : ''}<span class="faint"> · {filtered.length} 个站点</span><button class="link-btn" onclick={() => { fleet.filter = 'all'; fleet.tag = ''; }}>看全部</button></div>
      {/if}
      {#if !fleet.loaded && !rows.length}
        <div class="card tbl-card skel" aria-busy="true">{#each [64, 52, 58, 46] as w, i (i)}<div class="sk-row"><i class="sk-ic"></i><i style="width:{w * 3}px"></i><span class="grow"></span><i style="width:80px"></i></div>{/each}</div>
      {:else if !rows.length}
        <div class="card"><p class="hint">还没有站点。导入一个已有的站点，或到「站点制作」让 AI 从零做一个；做好并配置上线后会出现在这里。</p></div>
      {:else}
        <div class="card tbl-card">
          <table class="tbl">
            <thead><tr>
              <th><button class="th" onclick={() => sortBy('name')}>站点{arrow('name')}</button></th>
              <th class="num"><button class="th" onclick={() => sortBy('users')} data-tip="GA4 近 28 天用户（按天加总），下面是和前 28 天比"><GaMark size={11} />用户{arrow('users')}</button></th>
              <th class="num"><button class="th" onclick={() => sortBy('clicks')} data-tip="Google + Bing 近 28 天搜索点击，下面是和前 28 天比"><GscMark size={11} /><BingMark size={11} />搜索点击{arrow('clicks')}</button></th>
              <th class="num c-impr"><button class="th" onclick={() => sortBy('impr')} data-tip="Google + Bing 近 28 天展示"><GscMark size={11} /><BingMark size={11} />展示{arrow('impr')}</button></th>
              <th class="num c-impr"><button class="th" onclick={() => sortBy('ctr')} data-tip="Google + Bing 近 28 天点击率"><GscMark size={11} /><BingMark size={11} />点击率{arrow('ctr')}</button></th>
              <th class="num c-idx"><button class="th" onclick={() => sortBy('indexed')} data-tip={'Google：近 28 天有展示的页面 / 站点地图提交的网址（Search Console 接口不再给收录数，用有展示的页面近似）\nBing：索引里的网址数'}><GscMark size={11} /><BingMark size={11} />已编入{arrow('indexed')}</button></th>
              <th class="c-trend"><span class="th-l"><GaMark size={11} />会话趋势</span></th>
              <th class="c-lvl">托管</th>
              <th class="c-when"><button class="th" onclick={() => sortBy('active')}>最近活动{arrow('active')}</button></th>
            </tr></thead>
            <tbody>
              {#each pageRows as r (r.id)}
                {@const st = status(r)}
                <tr class="clickable" onclick={() => openSite(r.id)}>
                  <td class="site-cell">
                    <b><SiteIcon id={r.id} size={15} /><span class="nm" data-tip={r.domain && r.domain !== r.name ? `站点名称：${r.name}` : null}>{r.domain || r.name}</span>{#if r.domain}<button class="ext" onclick={(e) => { e.stopPropagation(); void openUrl(`https://${r.domain}`); }} aria-label="打开 {r.domain}" data-tip="打开 {r.domain}">↗</button>{/if}{#if st.c}<span class="pill st {st.c}" data-tip={r.summary.errors.length && st.t === '数据源出错' ? r.summary.errors.join('\n') : null}>{#if st.c === 'run'}<span class="spin"></span>{/if}{st.t}</span>{/if}</b>
                    <small><span class="dm">{#if r.deploy_mode === 'vps'}<Icon name="server" size={11} />{:else if r.deploy_mode !== 'none'}<CfMark size={12} />{/if}{MODE[r.deploy_mode] ?? r.deploy_mode}</span>{#if r.source === 'github' && r.repo}<button class="gh" onclick={(e) => { e.stopPropagation(); void openUrl(`https://github.com/${r.repo}`); }} aria-label="GitHub 仓库 {r.repo}" data-tip="GitHub 仓库：{r.repo}（点一下打开）"><GhMark size={12} /></button>{/if}{#if r.channels?.length}<span class="chs">{#each r.channels as c, i (i)}<button class="ch" class:off={!c.enabled} class:bad={c.failed > 0} onclick={(e) => { e.stopPropagation(); openSite(r.id, 'distribute'); }} aria-label={chTip(c)} data-tip={chTip(c)}><ConnectionMark kind={c.kind} size={12} /></button>{/each}</span>{/if}{#each r.tags as t (t)}<span class="tag">#{t}</span>{/each}</small>
                  </td>
                  {#if r.has_data}
                    {@const sm = r.summary}
                    <td class="num">{#if sm.fetched_at}<span class="v">{n(sm.users ?? 0)}</span>{@render delta(sm.users ?? 0, sm.users_prev ?? 0)}{:else}—{/if}</td>
                    <td class="num"><span class="v">{n(sm.search_clicks ?? sm.gsc_clicks)}</span>{@render delta(sm.search_clicks ?? sm.gsc_clicks, sm.search_clicks_prev ?? sm.gsc_clicks_prev)}</td>
                    <td class="num c-impr"><span class="v">{n(sm.search_impressions ?? sm.gsc_impressions)}</span></td>
                    <td class="num c-impr"><span class="v">{(sm.search_impressions ?? 0) > 0 ? `${(searchCtr(r) * 100).toFixed(1)}%` : '—'}</span></td>
                    <td class="num c-idx"><span class="v">{sm.google_pages ?? 0}{#if sm.sitemap_urls}<span class="of">/{n(sm.sitemap_urls)}</span>{/if}</span>{#if sm.bing_indexed}<small class="bi"><BingMark size={10} />{n(sm.bing_indexed)}</small>{/if}</td>
                    <td class="c-trend">{@render spark(sm.sessions_trend ?? [])}</td>
                  {:else}
                    <td class="num nodata" colspan="2">未接数据</td><td class="c-impr"></td><td class="c-impr"></td><td class="c-idx"></td><td class="c-trend"></td>
                  {/if}
                  <td class="c-lvl"><span class="am am-{r.autonomy || 'none'}" title={r.plan ? `托管方案「${r.plan}」` : '不在任何托管方案里'}>{r.autonomy === 'stopped' ? '已停止' : r.autonomy ? AUTONOMY_LABEL[r.autonomy] : '自己运营'}</span></td>
                  <td class="faint when c-when"><span data-tip={full(r.active_at)}>{when(r.active_at)}</span></td>
                </tr>
              {:else}
                <tr><td colspan="9" class="faint">没有符合条件的站点</td></tr>
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
                  <td class="faint when"><span data-tip={full(p.item.updated_at)}>{when(p.item.updated_at)}</span></td>
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
  .iss-wrap { position: relative; }
  /* 和后面的小按钮（.btn.small，26px、8px 圆角）一样高、一样的圆角 */
  .iss { display: inline-flex; align-items: center; gap: 6px; height: 26px; box-sizing: border-box; padding: 0 10px; border: none; border-radius: 8px; background: var(--su-warn-soft); color: var(--su-warn-ink); font-size: 12px; line-height: 1; font-weight: 550; cursor: pointer; }
  .iss .chev { transition: transform .15s; }
  .iss .chev.open { transform: rotate(180deg); }
  .iss.soft { background: var(--su-hover); color: var(--dim); font-weight: 500; }
  .iss-row { position: relative; }
  .iss-g { width: 100%; box-sizing: border-box; }
  .iss-x { position: absolute; bottom: 7px; right: 8px; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; border: 0; border-radius: 6px; background: none; color: var(--faint); cursor: pointer; opacity: 0; transition: opacity .12s; }
  .iss-row:hover .iss-x, .iss-x:focus-visible { opacity: 1; }
  .iss-x:hover { background: var(--su-hover); color: var(--text); }
  .iss-mask { position: fixed; inset: 0; z-index: 30; }
  .iss-pop { position: absolute; right: 0; top: calc(100% + 6px); z-index: 31; width: 300px; padding: 6px; border: 1px solid var(--border); border-radius: 12px; background: var(--card); box-shadow: 0 12px 32px rgba(0, 0, 0, .12); display: flex; flex-direction: column; gap: 2px; }
  .iss-g { display: flex; flex-direction: column; align-items: stretch; gap: 2px; text-align: left; padding: 8px 10px; border: none; background: none; border-radius: 8px; cursor: pointer; color: var(--text); }
  .iss-g:hover { background: var(--su-hover); }
  .iss-t { display: flex; align-items: center; gap: 8px; font-size: 13px; }
  .iss-t b { font-weight: 550; flex: 1; }
  .iss-n { min-width: 18px; height: 18px; padding: 0 5px; border-radius: 999px; background: var(--su-warn-soft); color: var(--su-warn-ink); font-size: 11px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; }
  .iss-g.soft .iss-n { background: var(--su-hover); color: var(--dim); }
  .iss-sites { font-size: 12px; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .only { display: flex; align-items: center; gap: 6px; margin-bottom: 10px; font-size: 13px; }
  .only .link-btn { margin-left: 6px; font-size: 12.5px; }
  .ov .page-body > .pane { max-width: 1180px; gap: 12px; }
  .mode-dd { display: flex; align-items: center; margin: 0 6px 2px 4px; }
  .tagsel { width: auto; height: 26px; padding: 0 8px; font-size: 12px; }
  .tbl-card { padding: 2px 16px; overflow-x: auto; }
  .tbl-card table.tbl td { vertical-align: middle; padding-top: 9px; padding-bottom: 9px; }
  .th { display: inline-flex; align-items: center; gap: 2px; border: none; background: none; padding: 0; font: inherit; color: inherit; cursor: pointer; white-space: nowrap; transition: color .12s; }
  .th:hover { color: var(--text); }
  tr.clickable { cursor: pointer; }
  .site-cell b { display: block; font-size: 13px; font-weight: 600; }
  .site-cell { max-width: 0; width: 34%; }
  .site-cell b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .site-cell b { display: flex !important; align-items: center; gap: 8px; min-width: 0; }
  .site-cell .nm { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ext { flex: none; width: 20px; height: 20px; margin-left: -4px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 5px; font: inherit; font-size: 12px; font-weight: 400; line-height: 1; color: var(--dim); cursor: pointer; opacity: .55; transition: opacity .12s, background-color .12s, color .12s; }
  tr:hover .ext { opacity: 1; }
  .ext:hover { background: var(--su-press); color: var(--text); }
  .site-cell small { padding-left: 23px; display: flex; flex-wrap: nowrap; overflow: hidden; white-space: nowrap; gap: 0 8px; font-size: 11.5px; color: var(--faint); margin-top: 1px; }
  .site-cell small span + span:not(.tag)::before { content: '·'; margin-right: 8px; color: var(--border2); }
  .tag { color: var(--dim); }
  .dm { display: inline-flex; align-items: center; gap: 5px; }
  .dm :global(svg) { flex: none; }
  .dm :global(.ico) { color: var(--dim); }
  .nodata { color: var(--faint); font-size: 12px; }
  /* 状态不单占一列：正常的不显示，有事的（进行中、待确认、上线失败、失败、数据源出错）跟在站点名后面 */
  .pill.st { flex: none; margin-left: 4px; font-weight: 500; }
  .th :global(svg), .th-l :global(svg) { flex: none; margin-right: 2px; }
  .th-l { display: inline-flex; align-items: center; gap: 2px; }
  td.num .v { display: block; font-variant-numeric: tabular-nums; }
  .dl { display: block; font-size: 11px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .of { color: var(--faint); font-weight: 400; }
  .bi { display: inline-flex; align-items: center; gap: 3px; font-size: 11px; color: var(--dim); }
  .c-trend { width: 96px; }
  .spark { display: block; }
  .gh { display: inline-flex; align-items: center; border: 0; background: none; padding: 0; color: var(--faint); cursor: pointer; }
  .gh:hover { color: var(--text); }
  /* 分发渠道：平台图标一排，暂停的变淡，有发布失败的加个红点 */
  .chs { display: inline-flex; align-items: center; gap: 4px; }
  .ch { position: relative; display: inline-flex; align-items: center; border: 0; background: none; padding: 0; cursor: pointer; }
  .ch.off { opacity: .4; filter: grayscale(1); }
  .ch.bad::after { content: ''; position: absolute; top: -2px; right: -2px; width: 5px; height: 5px; border-radius: 50%; background: var(--err); }
  .ch:hover { transform: translateY(-1px); }
  .am { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--dim); white-space: nowrap; }
  .am::before { content: ''; width: 6px; height: 6px; border-radius: 50%; background: var(--border2); }
  .am-auto::before { background: var(--ok); }
  .am-review::before { background: var(--su-run); }
  .am-suggest::before { background: #b9a36a; }
  .am-stopped::before { background: var(--faint); }
  .am-none { color: var(--faint); }
  .when { font-size: 12px; white-space: nowrap; }
  .sn-cell { max-width: 170px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sn { font-weight: 600; font-size: 12.5px; display: inline-flex; align-items: center; gap: 7px; }
  .kind { margin-left: 8px; color: var(--faint); font-size: 11.5px; }
  .pager { gap: 8px; }
  .skel { padding: 6px 16px; }
  .sk-row { display: flex; align-items: center; gap: 12px; height: 52px; border-top: 1px solid var(--border); }
  .sk-row:first-child { border-top: none; }
  .sk-row .grow { flex: 1; }
  .sk-row i { display: block; height: 9px; border-radius: 4px; background: linear-gradient(90deg, var(--accent-soft) 25%, #f6f5f1 50%, var(--accent-soft) 75%); background-size: 200% 100%; animation: sk 1.2s linear infinite; }
  .sk-row .sk-ic { width: 16px; height: 16px; border-radius: 4px; }
  @keyframes sk { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .sk-row i { animation: none; } }
  @container page (max-width: 760px) {
    .c-when { display: none; }
  }
  @container page (max-width: 980px) {
    .c-trend { display: none; }
  }
  @container page (max-width: 880px) {
    .c-impr { display: none; }
  }
  @container page (max-width: 680px) {
    .c-idx { display: none; }
  }
  @container page (max-width: 560px) {
    .c-lvl { display: none; }
  }
</style>
