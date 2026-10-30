<script lang="ts">
  import './site.css';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import GscMark from '$lib/components/icons/GscMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import GaMark from '$lib/components/icons/GaMark.svelte';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import ConnectionMark from '$lib/components/icons/ConnectionMark.svelte';
  import type { SiteRow, ChannelBrief } from '$lib/types';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { openSite, openSiteImport } from '$lib/state/sites.svelte.js';
  import { openSiteCreate, isMaking } from '$lib/state/siteCreate.svelte.js';
  import { fleet, inPicks, applyFilter, sortRows, change, searchCtr, win, setDays, syncAllData, checkAllHealth, loadPending, PAGE_SIZE, ISSUE_GROUPS, issueRows, ignoreIssues, type SiteSort } from '$lib/state/fleet.svelte.js';
  import { STATUS_LABEL, KIND_LABEL } from '$lib/state/ops.svelte.js';
  import { AUTONOMY_LABEL } from '$lib/state/managed.svelte.js';
  import { ago, full, num } from './fmt';
  import BackgroundNotice from './BackgroundNotice.svelte';
  import OppsBoard from './OppsBoard.svelte';
  import HistoryBoard from './HistoryBoard.svelte';
  import SitePicker from './SitePicker.svelte';
  import DeployMark from './DeployMark.svelte';
  import { usd } from '$lib/state/growth.svelte.js';

  // 制作中的站点在「站点制作」里，不进全部站点的列表和统计
  const all = $derived(fleet.rows.filter((r) => !isMaking(r.id)));
  // 标题旁勾了几个站点就只看它们（列表、提醒、待处理、近期动态都按它）
  const rows = $derived(all.filter((r) => inPicks(r.id)));
  const filtered = $derived(sortRows(applyFilter(rows, fleet.query, fleet.filter, fleet.tag), fleet.sort, fleet.desc));
  const pages = $derived(Math.max(1, Math.ceil(filtered.length / PAGE_SIZE)));
  const pageRows = $derived(filtered.slice(fleet.page * PAGE_SIZE, (fleet.page + 1) * PAGE_SIZE));
  $effect(() => { void fleet.query; void fleet.filter; void fleet.tag; void fleet.picks; fleet.page = 0; });
  $effect(() => { if (fleet.tab === 'pending') void loadPending(); });

  const proposedN = $derived(fleet.picks.length ? rows.reduce((n, r) => n + r.proposed, 0) : fleet.pendingCount);
  const oppN = $derived(fleet.picks.length ? rows.filter((r) => !r.plan || r.autonomy === 'stopped').reduce((n, r) => n + r.open, 0) : fleet.oppCount);
  const pending = $derived(fleet.pending.filter((p) => inPicks(p.site_id)));
  const n = num;
  // 有站点在记收益（连了 AdSense 或手动记过）才显示「本月收益」这一列
  const showRev = $derived(fleet.rows.some((r) => r.money?.tracked));
  function revTip(r: SiteRow) {
    const m = r.money;
    if (!m) return '';
    const ai = r.cost_month_usd ?? 0;
    const lines = [`本月收益 ${usd(m.month)}${m.adsense_month ? `（AdSense ${usd(m.adsense_month)}）` : ''}`, `上月 ${usd(m.last_month)}`];
    if (ai >= 0.01) lines.push(`本月 AI 花费 ${usd(ai)}，差额 ${usd(m.month - ai)}`);
    if (m.rpm) lines.push(`近 28 天每千次浏览 ${usd(m.rpm)}`);
    return lines.join('\n');
  }
  const when = ago;
  // 统计时间段：近 7 / 28 天，截至各数据源最新的那天（各站一般相同，取最多的那个）
  const D = $derived(fleet.days);
  const commonEnd = (k: 'ga4_end' | 'gsc_end' | 'bing_end') => {
    const c = new Map<string, number>();
    for (const r of rows) { const e = r.summary[k]; if (e) c.set(e, (c.get(e) ?? 0) + 1); }
    return [...c].sort((a, b) => b[1] - a[1] || b[0].localeCompare(a[0]))[0]?.[0] ?? '';
  };
  const ends = $derived({ ga4: commonEnd('ga4_end'), gsc: commonEnd('gsc_end'), bing: commonEnd('bing_end') });
  const md = (d: string) => { const [, m, dd] = d.split('-'); return `${+m}月${+dd}日`; };
  const endLabel = $derived([ends.ga4, ends.gsc].filter(Boolean).sort().pop() ?? '');
  const endsTip = $derived([ends.ga4 && `GA4 到 ${md(ends.ga4)}`, ends.gsc && `Google 到 ${md(ends.gsc)}`, ends.bing && `Bing 到 ${md(ends.bing)}`].filter(Boolean).join(' · '));
  let moreOpen = $state(false);
  $effect(() => {
    if (!moreOpen) return;
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') moreOpen = false; };
    document.addEventListener('keydown', esc);
    return () => document.removeEventListener('keydown', esc);
  });
  // 正在拉数据 / 排队的站点（导入后、每天早上 7 点、⋯ 里立即同步）
  const syncRun = $derived(new Set(fleet.sync.running));
  const syncQ = $derived(new Set(fleet.sync.queued));
  const syncLeft = $derived(new Set([...fleet.sync.running, ...fleet.sync.queued]).size);
  const RUN_TIP = '正在拉 GA4、Search Console、Bing 的数据，拉完自动刷新';
  const Q_TIP = '排队中：等前面的站点拉完就拉它';
  const MODE: Record<string, string> = { direct: 'Cloudflare Pages', workers: 'Cloudflare Workers', vps: 'VPS', pages_git: 'Pages · Git 自动构建', none: '未配置部署' };
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
  const status = (r: SiteRow) => r.unsafe_by ? { t: '被标记为危险', c: 'err' } : r.site_down ? { t: '打不开', c: 'err' } : r.job ? { t: r.job + '中', c: 'run' } : r.proposed ? { t: `${r.proposed} 条待确认`, c: 'warn' } : r.last_deploy_ok === false ? { t: '上线失败', c: 'err' } : r.failed ? { t: `${r.failed} 条失败`, c: 'err' } : r.links_broken ? { t: `${r.links_broken} 个推广链接失效`, c: 'err' } : r.cert_err ? { t: '证书有问题', c: 'err' } : r.cert_days != null && r.cert_days <= 14 ? { t: r.cert_days < 0 ? '证书已过期' : `证书 ${r.cert_days} 天后过期`, c: r.cert_days <= 3 ? 'err' : 'warn' } : r.domain_days != null && r.domain_days <= 60 ? { t: r.domain_days < 0 ? '域名已过期' : `域名 ${r.domain_days} 天后到期`, c: r.domain_days <= 7 ? 'err' : 'warn' } : r.summary.errors.length ? { t: '数据源出错', c: 'err' } : { t: '正常', c: '' };
</script>

{#snippet delta(cur: number, prev: number)}
  <!-- 前一段是 0 算不出变化，就不显示 -->
  {@const d = change(cur, prev)}
  {#if typeof d === 'number' && Math.abs(d) >= 0.005}<span class="dl" class:up={d > 0} class:down={d < 0}>{d > 0 ? '↑' : '↓'}{Math.abs(d * 100) >= 100 ? Math.round(Math.abs(d * 100)) : Math.abs(d * 100).toFixed(1)}%</span>{/if}
{/snippet}

{#snippet spark(vals: number[])}
  {#if vals.length > 1 && vals.some((v) => v > 0)}
    {@const W = 88}
    {@const H = 24}
    {@const top = Math.max(...vals)}
    {@const pts = vals.map((v, i) => `${((i / (vals.length - 1)) * W).toFixed(1)},${(H - 2 - (v / top) * (H - 4)).toFixed(1)}`)}
    <svg class="spark" width={W} height={H} viewBox="0 0 {W} {H}" aria-label="近 {D} 天会话趋势" data-tip={`近 ${D} 天会话 ${num(vals.reduce((a, b) => a + b, 0))}，最多一天 ${num(top)}`}>
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
      <SitePicker rows={all} />
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

  <div class="tabs-bar">
  <div class="tabs">
    <button class:on={fleet.tab === 'sites'} onclick={() => (fleet.tab = 'sites')}><Icon name="globe" size={13} />站点</button>
    <button class:on={fleet.tab === 'pending'} onclick={() => (fleet.tab = 'pending')}><Icon name="inbox" size={13} />待处理{#if proposedN}<span class="cnt" data-tip="{proposedN} 条改动等你确认">{proposedN}</span>{/if}{#if oppN}<span class="cnt soft" data-tip="{oppN} 个运营机会没处理（自己运营的、托管停着的站点）">{oppN}</span>{/if}</button>
    <button class:on={fleet.tab === 'history'} onclick={() => (fleet.tab = 'history')} data-tip="交给 AI 处理过的：已上线、已处理、没成功、回滚、你驳回的，按天看，已上线的能看效果"><Icon name="history" size={13} />近期动态</button>
  </div>
  <div class="tab-tools">
    <!-- 统计时间段和不常用的批量操作（立即同步数据、重新检查健康）都收在 ⋯ 里 -->
    <span class="pop-wrap">
      <button class="ic-btn ic-more" class:on={moreOpen} onclick={() => (moreOpen = !moreOpen)} aria-label="更多操作" aria-haspopup="menu" aria-expanded={moreOpen} data-tip={moreOpen ? null : `统计时间段（现在近 ${D} 天）、立即同步数据、重新检查健康`}>{#if syncLeft || fleet.sync.health}<span class="spin"></span>{:else}<Icon name="more" size={16} />{/if}</button>
      {#if moreOpen}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <span class="pop-mask" onclick={() => (moreOpen = false)}></span>
        <span class="pop more" role="menu">
          {#if fleet.tab === 'sites' && endLabel}
            <span class="range-sec">
              <span class="range-h"><Icon name="calendar" size={13} />统计时间段</span>
              <span class="rseg" role="radiogroup" aria-label="统计时间段">
                {#each [7, 28] as d (d)}<button role="radio" aria-checked={D === d} class:on={D === d} onclick={() => setDays(d as 7 | 28)}>近 {d} 天</button>{/each}
              </span>
              <small data-tip={`${endsTip}\n各家数据出得有早有晚，Google 最近一两天的数字过后还会补上来；「已编入」不随时间段变`}>截至 {md(endLabel)}，涨跌和再往前 {D} 天比</small>
            </span>
          {/if}
          <button role="menuitem" disabled={syncLeft > 0} onclick={() => { moreOpen = false; void syncAllData(); }}>
            <Icon name="refresh" size={14} /><span><b>立即同步所有站点数据</b><small>{syncLeft ? `正在同步，还剩 ${syncLeft} 个站点` : `${fleet.sync.last_at ? `上次 ${ago(fleet.sync.last_at)}；` : ''}每天早上 7 点会自动同步`}</small></span>
          </button>
          <button role="menuitem" disabled={fleet.sync.health} onclick={() => { moreOpen = false; void checkAllHealth(); }}>
            <Icon name="activity" size={14} /><span><b>全部站点重新检查健康</b><small>{fleet.sync.health ? '正在检查…' : '在线情况、证书、域名、安全标记、推广链接'}</small></span>
          </button>
        </span>
      {/if}
    </span>
  </div>
  </div>

  <div class="page-body">
   {#key fleet.tab}
   <div class="pane">
    {#if fleet.tab === 'sites'}
      <BackgroundNotice />
      {#if fleet.filter !== 'all' || fleet.tag}
        <div class="only">只看：{onlyLabel}{onlyLabel && fleet.tag ? ' · ' : ''}{fleet.tag ? `#${fleet.tag}` : ''}<span class="faint"> · {filtered.length} 个站点</span><button class="link-btn" onclick={() => { fleet.filter = 'all'; fleet.tag = ''; }}>看全部</button></div>
      {/if}
      {#if !fleet.loaded && !rows.length}
        <div class="card tbl-card skel" aria-busy="true">{#each [64, 52, 58, 46] as w, i (i)}<div class="sk-row"><i class="sk-ic"></i><i style="width:{w * 3}px"></i><span class="grow"></span><i style="width:80px"></i></div>{/each}</div>
      {:else if !all.length}
        <div class="card"><p class="hint">还没有站点。导入一个已有的站点，或到「站点制作」让 AI 从零做一个；做好并配置上线后会出现在这里。</p></div>
      {:else}
        <div class="card tbl-card">
          <table class="tbl">
            <thead><tr>
              <th><button class="th" onclick={() => sortBy('name')}>站点{arrow('name')}</button></th>
              <th class="c-lvl">托管</th>
              <th class="num"><button class="th" onclick={() => sortBy('users')} data-tip="GA4 近 {D} 天用户（按天加总），下面是和前 {D} 天比"><GaMark size={11} />用户{arrow('users')}</button></th>
              <th class="num"><button class="th" onclick={() => sortBy('clicks')} data-tip="Google + Bing 近 {D} 天搜索点击，下面是和前 {D} 天比"><GscMark size={11} /><BingMark size={11} />搜索点击{arrow('clicks')}</button></th>
              <th class="num c-impr"><button class="th" onclick={() => sortBy('impr')} data-tip="Google + Bing 近 {D} 天展示"><GscMark size={11} /><BingMark size={11} />展示{arrow('impr')}</button></th>
              <th class="num c-impr"><button class="th" onclick={() => sortBy('ctr')} data-tip="Google + Bing 近 {D} 天点击率"><GscMark size={11} /><BingMark size={11} />点击率{arrow('ctr')}</button></th>
              <th class="num c-idx"><button class="th" onclick={() => sortBy('indexed')} data-tip={'Google：近 28 天有展示的页面 / 站点地图提交的网址（Search Console 接口不再给收录数，用有展示的页面近似）\nBing：索引里的网址数'}><GscMark size={11} /><BingMark size={11} />已编入{arrow('indexed')}</button></th>
              <th class="c-trend"><span class="th-l"><GaMark size={11} />会话趋势</span></th>
              {#if showRev}<th class="num c-rev"><button class="th" onclick={() => sortBy('revenue')} data-tip="这个月的收益（AdSense + 手动记的，美元），下面是本月 AI 花费">本月收益{arrow('revenue')}</button></th>{/if}
              <th class="c-when"><button class="th" onclick={() => sortBy('active')}>最近活动{arrow('active')}</button></th>
            </tr></thead>
            <tbody>
              {#each pageRows as r (r.id)}
                {@const st = status(r)}
                <tr class="clickable" onclick={() => openSite(r.id)}>
                  <td class="site-cell">
                    <b><SiteIcon id={r.id} size={15} /><span class="nm" data-tip={r.domain && r.domain !== r.name ? `站点名称：${r.name}` : null}>{r.domain || r.name}</span>{#if r.domain}<button class="ext" onclick={(e) => { e.stopPropagation(); void openUrl(`https://${r.domain}`); }} aria-label="打开 {r.domain}" data-tip="打开 {r.domain}">↗</button>{/if}{#if syncRun.has(r.id)}<span class="sync" data-tip={RUN_TIP} aria-label={RUN_TIP}><span class="spin"></span></span>{:else if syncQ.has(r.id)}<span class="sync q" data-tip={Q_TIP} aria-label={Q_TIP}><Icon name="clock" size={11} /></span>{/if}{#if st.c}<span class="pill st {st.c}" data-tip={st.t === '被标记为危险' ? `${r.unsafe_by} 把这个站标记成了危险网站，访客会看到警告页或打不开；自动上线已暂停（站点概览 → 站点健康 → 安全标记）` : st.t === '数据源出错' ? r.summary.errors.join('\n') : st.t === '打不开' ? `${r.down_err || '首页打不开'}（连续两次检查都没打开）` : st.t === '证书有问题' ? r.cert_err : null}>{#if st.c === 'run'}<span class="spin"></span>{/if}{st.t}</span>{/if}</b>
                    <small><span class="dm"><DeployMark mode={r.deploy_mode} size={11} />{MODE[r.deploy_mode] ?? r.deploy_mode}</span>{#if r.repo}<button class="gh" onclick={(e) => { e.stopPropagation(); void openUrl(`https://github.com/${r.repo}`); }} aria-label="GitHub 仓库 {r.repo}" data-tip="GitHub 仓库：{r.repo}（点一下打开）"><GhMark size={12} /></button>{/if}{#if r.channels?.length}<span class="chs">{#each r.channels as c, i (i)}<button class="ch" class:off={!c.enabled} class:bad={c.failed > 0} onclick={(e) => { e.stopPropagation(); openSite(r.id, 'distribute'); }} aria-label={chTip(c)} data-tip={chTip(c)}><ConnectionMark kind={c.kind} size={12} /></button>{/each}</span>{/if}{#each r.tags as t (t)}<span class="tag">#{t}</span>{/each}</small>
                  </td>
                  <td class="c-lvl"><span class="am am-{r.autonomy || 'none'}" data-tip={r.plan ? `托管方案「${r.plan}」` : '不在任何托管方案里'}>{r.autonomy === 'stopped' ? '已停止' : r.autonomy ? AUTONOMY_LABEL[r.autonomy] : '自己运营'}</span></td>
                  {#if r.has_data && r.summary.fetched_at}
                    {@const sm = r.summary}
                    {@const w = win(r)}
                    <td class="num"><span class="v">{n(w.users)}</span>{@render delta(w.users, w.users_prev)}</td>
                    {#if r.has_gsc && !sm.gsc_days && !w.clicks && !w.impr}
                      <!-- 接了 Search Console 但一天数据都没有：多半是新验证的站，Google 还没出数据 -->
                      <td class="num nodata" data-tip="Search Console 还没出数据：新验证的站一般要等几天才有">Google 还没数据</td><td class="num c-impr nodata">—</td><td class="num c-impr nodata">—</td>
                    {:else}
                      <td class="num"><span class="v">{n(w.clicks)}</span>{@render delta(w.clicks, w.clicks_prev)}</td>
                      <td class="num c-impr"><span class="v">{n(w.impr)}</span></td>
                      <td class="num c-impr"><span class="v">{w.impr > 0 ? `${(searchCtr(r) * 100).toFixed(1)}%` : '—'}</span></td>
                    {/if}
                    <td class="num c-idx"><span class="v">{sm.google_pages ?? 0}{#if sm.sitemap_urls}<span class="of">/{n(sm.sitemap_urls)}</span>{/if}</span>{#if sm.bing_indexed}<small class="bi"><BingMark size={10} />{n(sm.bing_indexed)}</small>{/if}</td>
                    <td class="c-trend">{@render spark(w.trend)}</td>
                  {:else}
                    <!-- 没有数据的三种情况分开说：正在拉 / 还没接数据源 / 接了但还没拉到 -->
                    <td class="num nodata" colspan="2">
                      {#if syncRun.has(r.id) || syncQ.has(r.id)}<span class="pulling"><span class="spin"></span>{syncRun.has(r.id) ? '正在拉数据…' : '排队拉数据…'}</span>
                      {:else if !r.has_data}<button class="link-btn" onclick={(e) => { e.stopPropagation(); openSite(r.id, 'settings'); }} data-tip="在站点设置里接上 GA4、Search Console、Bing">还没接数据源</button>
                      {:else if r.summary.errors.length}<span class="err-t" data-tip={r.summary.errors.join('\n')}>没拉到数据</span>
                      {:else}<span data-tip="每天早上 7 点自动同步，也可以点右上角 ⋯ 立即同步">还没拉到数据</span>{/if}
                    </td><td class="c-impr"></td><td class="c-impr"></td><td class="c-idx"></td><td class="c-trend"></td>
                  {/if}
                  {#if showRev}<td class="num c-rev">{#if r.money?.tracked}<span class="v" data-tip={revTip(r)}>{usd(r.money.month)}</span>{#if (r.cost_month_usd ?? 0) >= 0.01}<small class="ai">AI {usd(r.cost_month_usd ?? 0)}</small>{/if}{:else}<span class="faint">—</span>{/if}</td>{/if}
                  <td class="faint when c-when"><span data-tip={full(r.active_at)}>{when(r.active_at)}</span></td>
                </tr>
              {:else}
                <tr><td colspan={showRev ? 10 : 9} class="faint">没有符合条件的站点</td></tr>
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
    {:else if fleet.tab === 'history'}
      <HistoryBoard />
    {:else}
      <!-- 有 AI 改好等你确认的：上下两块（等你确认 / 运营机会）；没有就只显示运营机会，不留空块 -->
      {#if pending.length}
        <section class="blk">
          <h4 class="sec">等你确认<span class="sc">{pending.length}</span></h4>
          <div class="card tbl-card">
            <table class="tbl">
              <thead><tr><th style="width:170px">站点</th><th>事项</th><th style="width:84px">状态</th><th style="width:96px">更新</th></tr></thead>
              <tbody>
                {#each pending as p (p.item.id)}
                  <tr class="clickable" onclick={() => openSite(p.site_id, 'queue', '', p.item.id)}>
                    <td class="sn-cell"><b class="sn"><SiteIcon id={p.site_id} size={13} />{p.site_name}</b></td>
                    <td class="trunc">{p.item.opp.title}<small class="kind">{KIND_LABEL[p.item.opp.kind] ?? ''}</small></td>
                    <td><span class="pill {p.item.status === 'failed' ? 'err' : 'warn'}">{STATUS_LABEL[p.item.status]}</span></td>
                    <td class="faint when"><span data-tip={full(p.item.updated_at)}>{when(p.item.updated_at)}</span></td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </section>
        <section class="blk mt">
          <h4 class="sec">运营机会{#if oppN}<span class="sc">{oppN}</span>{/if}</h4>
          <OppsBoard />
        </section>
      {:else}
        <OppsBoard />
      {/if}
    {/if}
   </div>
   {/key}
  </div>
</div>

<style>
  .iss-wrap { position: relative; }
  /* tab 行右边放统计时间段和 ⋯；tab 自己会横向滚动（会裁掉弹出层），所以工具放在外面 */
  .tabs-bar { display: flex; align-items: stretch; border-bottom: 1px solid var(--border); padding-right: 20px; flex: none; }
  .tabs-bar .tabs { border-bottom: none; flex: 1; min-width: 0; }
  .tab-tools { display: flex; align-items: center; gap: 4px; padding-bottom: 2px; }
  .pop-wrap { position: relative; display: inline-flex; }
  .ic-more { width: 28px; height: 28px; }
  /* ⋯ 菜单顶部：统计时间段（两段切换）+ 截至哪天，下面一条线再接批量操作 */
  .range-sec { display: flex; flex-direction: column; gap: 6px; padding: 8px 10px 10px; margin-bottom: 2px; border-bottom: 1px solid var(--border); }
  .range-h { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--dim); }
  .rseg { display: flex; padding: 2px; border-radius: 8px; background: var(--su-hover); }
  .pop .rseg button { flex: 1; justify-content: center; align-items: center; padding: 4px 0; border-radius: 6px; font-size: 12px; line-height: 1.4; white-space: nowrap; color: var(--dim); }
  .pop .rseg button:hover:not(.on) { background: none; color: var(--text); }
  .pop .rseg button.on { background: var(--card); color: var(--text); font-weight: 550; box-shadow: 0 1px 2px rgba(0, 0, 0, .08); }
  .range-sec small { font-size: 11px; color: var(--faint); }
  .ic-more.on { background: var(--su-hover); color: var(--text); }
  .pop-mask { position: fixed; inset: 0; z-index: 30; }
  .pop { position: absolute; right: 0; top: calc(100% + 6px); z-index: 31; width: max-content; min-width: 200px; max-width: 280px; box-sizing: border-box; padding: 4px; border: 1px solid var(--border); border-radius: 12px; background: var(--card); box-shadow: 0 12px 32px rgba(0, 0, 0, .12); display: flex; flex-direction: column; gap: 2px; }
  .pop button { display: flex; align-items: flex-start; gap: 8px; padding: 7px 10px 7px 8px; border: none; border-radius: 8px; background: none; font: inherit; text-align: left; color: var(--text); cursor: pointer; }
  .pop button:hover:not(:disabled) { background: var(--su-hover); }
  .pop button:disabled { cursor: default; opacity: .6; }
  .pop button > :global(svg) { flex: none; margin-top: 2px; color: var(--dim); }
  .pop button > span { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; }
  .pop b { font-size: 13px; font-weight: 500; }
  .pop small { font-size: 11px; color: var(--faint); line-height: 1.4; }
  /* 域名后面：正在拉数据转圈、排队用小钟 */
  .sync { flex: none; display: inline-flex; align-items: center; color: var(--dim); font-weight: 400; }
  .sync .spin { width: 10px; height: 10px; }
  .sync.q { color: var(--faint); }
  .pulling { display: inline-flex; align-items: center; gap: 6px; }
  .pulling .spin { width: 10px; height: 10px; }
  .err-t { color: var(--err); }
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
  /* 宽度跟着内容走：至少和按钮一样宽，最宽 320px，站点名太长就省略 */
  .iss-pop { position: absolute; right: 0; top: calc(100% + 6px); z-index: 31; width: max-content; min-width: max(100%, 200px); max-width: 320px; box-sizing: border-box; padding: 4px; border: 1px solid var(--border); border-radius: 12px; background: var(--card); box-shadow: 0 12px 32px rgba(0, 0, 0, .12); display: flex; flex-direction: column; gap: 2px; }
  .iss-g { display: flex; flex-direction: column; align-items: stretch; gap: 1px; text-align: left; padding: 7px 10px; border: none; background: none; border-radius: 8px; cursor: pointer; color: var(--text); }
  .iss-g:hover { background: var(--su-hover); }
  .iss-t { display: flex; align-items: center; gap: 16px; font-size: 13px; }
  .iss-t b { font-weight: 550; flex: 1; }
  .iss-n { min-width: 18px; height: 18px; padding: 0 5px; border-radius: 999px; background: var(--su-warn-soft); color: var(--su-warn-ink); font-size: 11px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; }
  .iss-g.soft .iss-n { background: var(--su-hover); color: var(--dim); }
  .iss-sites { font-size: 12px; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; padding-right: 24px; } /* 给悬停出现的忽略按钮留位置 */
  .only { display: flex; align-items: center; gap: 6px; margin-bottom: 10px; font-size: 13px; }
  .only .link-btn { margin-left: 6px; font-size: 12.5px; }
  .ov .page-body > .pane { max-width: 1180px; gap: 12px; }
  .tagsel { width: auto; height: 26px; padding: 0 8px; font-size: 12px; }
  .tbl-card { padding: 2px 16px; overflow-x: auto; }
  .tbl-card table.tbl td { vertical-align: middle; padding-top: 9px; padding-bottom: 9px; }
  .th { display: inline-flex; align-items: center; gap: 2px; border: none; background: none; padding: 0; font: inherit; color: inherit; cursor: pointer; white-space: nowrap; transition: color .12s; }
  .th:hover { color: var(--text); }
  tr.clickable { cursor: pointer; }
  .site-cell b { display: block; font-size: 13px; font-weight: 600; }
  /* 站点列按内容宽、至少 230px、最多 300px（更长的省略），多出来的宽度分给后面的数字列；
     原来是 34%，窗口一宽它就占掉一大块，窄的时候又把后面挤出卡片 */
  .site-cell { width: 230px; }
  .site-cell > b, .site-cell > small { max-width: 300px; }
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
  /* 自己运营：灰色但不发虚（原来浅得像禁用） */
  .am-none { color: var(--dim); }
  .am-none::before { background: #c9c5bb; }
  .when { font-size: 12px; white-space: nowrap; }
  /* 块里：标题和内容隔 8px；两块之间多空一点（加上页面本身的 12px 间距约 28px） */
  .blk { display: flex; flex-direction: column; gap: 8px; }
  .blk.mt { margin-top: 16px; }
  .sec { margin: 0 2px; display: flex; align-items: baseline; gap: 6px; font-size: 13px; font-weight: 600; }
  .sec .sc { font-size: 12px; font-weight: 400; color: var(--faint); font-variant-numeric: tabular-nums; }

  .c-rev .ai { display: block; font-size: 11px; color: var(--faint); white-space: nowrap; }
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
  @container page (max-width: 820px) {
    .c-rev { display: none; }
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
