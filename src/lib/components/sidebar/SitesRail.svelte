<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Conversation, SiteRow } from '$lib/types';
  import SiteMark from '$lib/components/icons/SiteMark.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { sidebar } from '$lib/state/sidebar.svelte.js';
  import { siteIdOf, openSite } from '$lib/state/sites.svelte.js';
  import { fleet, applyFilter, railOrder, needsAttention, trafficDown, setPinned } from '$lib/state/fleet.svelte.js';

  let { convRow }: { convRow: Snippet<[Conversation]> } = $props();

  // 站点不设上限：只渲染可见窗口里的行（每行定高），展开的当前站点把它的会话插在自己下面。
  const SITE_H = 40, GRP_H = 26, CONV_H = 28, EMPTY_H = 28;
  type Row =
    | { t: 'site'; key: string; site: SiteRow; h: number }
    | { t: 'grp'; key: string; label: string; h: number }
    | { t: 'conv'; key: string; c: Conversation; h: number }
    | { t: 'empty'; key: string; h: number };

  const activeId = $derived(siteIdOf(app.activeConnId));
  const list = $derived(railOrder(applyFilter(fleet.rows, fleet.railQuery, fleet.railFilter, fleet.railTag)));
  const rows = $derived.by(() => {
    const out: Row[] = [];
    for (const s of list) {
      out.push({ t: 'site', key: `s:${s.id}`, site: s, h: SITE_H });
      if (s.id !== activeId) continue;
      const groups = sidebar.grouped;
      if (!groups.length) out.push({ t: 'empty', key: `e:${s.id}`, h: EMPTY_H });
      for (const g of groups) {
        out.push({ t: 'grp', key: `g:${g.slug}`, label: g.name, h: GRP_H });
        for (const c of g.subs.flatMap((x) => x.items)) out.push({ t: 'conv', key: `c:${c.id}`, c, h: CONV_H });
      }
    }
    return out;
  });
  const offsets = $derived.by(() => {
    const o = [0];
    for (const r of rows) o.push(o[o.length - 1] + r.h);
    return o;
  });
  const total = $derived(offsets[offsets.length - 1] ?? 0);

  let scroller: HTMLDivElement | undefined = $state();
  let top = $state(0);
  let height = $state(600);
  const OVERSCAN = 300;
  function first(y: number) {
    let lo = 0, hi = rows.length - 1;
    while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (offsets[mid] <= y) lo = mid; else hi = mid - 1; }
    return Math.max(0, lo);
  }
  const start = $derived(first(Math.max(0, top - OVERSCAN)));
  const end = $derived.by(() => { let i = start; const limit = top + height + OVERSCAN; while (i < rows.length && offsets[i] < limit) i++; return i; });
  const visible = $derived(rows.slice(start, end).map((r, i) => ({ r, y: offsets[start + i] })));

  $effect(() => {
    if (!scroller) return;
    const ro = new ResizeObserver(() => { height = scroller?.clientHeight ?? 600; });
    ro.observe(scroller);
    return () => ro.disconnect();
  });
  // 切到某个站点时把它滚进可见区
  $effect(() => {
    const id = activeId;
    if (!id || !scroller) return;
    const i = rows.findIndex((r) => r.t === 'site' && r.site.id === id);
    if (i < 0) return;
    const y = offsets[i];
    if (y < scroller.scrollTop || y + SITE_H > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = Math.max(0, y - 60);
  });

  const FILTERS = $derived([
    { v: 'all' as const, l: '全部', n: fleet.rows.length },
    { v: 'pinned' as const, l: '置顶', n: fleet.rows.filter((r) => r.pinned).length },
    { v: 'attention' as const, l: '需关注', n: fleet.attentionCount },
  ]);
</script>

<div class="sr">
  <div class="sr-tools">
    <input class="sr-search" bind:value={fleet.railQuery} placeholder={`搜索 ${fleet.rows.length} 个站点`} spellcheck="false" />
    <div class="sr-chips">
      {#each FILTERS as f (f.v)}
        <button class:on={fleet.railFilter === f.v} onclick={() => (fleet.railFilter = f.v)}>{f.l}{#if f.n && f.v !== 'all'} <b>{f.n}</b>{/if}</button>
      {/each}
      {#if fleet.tags.length}
        <select class="sr-tag" bind:value={fleet.railTag} title="按标签筛选">
          <option value="">标签</option>
          {#each fleet.tags as t (t)}<option value={t}>#{t}</option>{/each}
        </select>
      {/if}
    </div>
  </div>
  <div class="sr-list" bind:this={scroller} onscroll={(e) => (top = (e.currentTarget as HTMLElement).scrollTop)}>
    {#if !fleet.loaded}
      <p class="sr-empty">正在读取站点…</p>
    {:else if !fleet.rows.length}
      <p class="sr-empty">还没有站点。点上面的「导入站点」，或在 Cloudflare 建站会话里「转为站点」。</p>
    {:else if !list.length}
      <p class="sr-empty">没有符合条件的站点。</p>
    {/if}
    <div class="sr-canvas" style="height:{total}px">
      {#each visible as { r, y } (r.key)}
        <div class="sr-row" style="transform:translateY({y}px);height:{r.h}px">
          {#if r.t === 'site'}
            {@const s = r.site}
            <div class="site {s.id === activeId ? 'on' : ''}" role="button" tabindex="0" title={s.domain || s.name}
              onclick={() => openSite(s.id)} onkeydown={(e) => e.key === 'Enter' && openSite(s.id)}>
              <span class="ic"><SiteMark size={15} /></span>
              <span class="main"><b>{s.name}</b><small>{s.domain && s.domain !== s.name ? s.domain : s.tags.length ? s.tags.map((t) => `#${t}`).join(' ') : s.source === 'github' ? s.repo : '本地文件夹'}</small></span>
              <span class="badges">
                {#if s.job}<span class="run" title={s.job}></span>{/if}
                {#if s.proposed}<span class="cnt" title="{s.proposed} 条待确认">{s.proposed}</span>{/if}
                {#if s.last_deploy_ok === false || s.failed}<span class="bad" title={s.last_deploy_ok === false ? '上次上线失败' : `${s.failed} 条运营任务失败`}>!</span>{/if}
                {#if trafficDown(s)}<span class="down" title="点击比前 28 天明显下滑">↓</span>{/if}
                {#if !needsAttention(s) && !s.job && s.pinned}<span class="pin" title="已置顶">●</span>{/if}
              </span>
              <button class="pin-btn" class:pinned={s.pinned} title={s.pinned ? '取消置顶' : '置顶'} onclick={(e) => { e.stopPropagation(); void setPinned(s.id, !s.pinned); }}>
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M9.6 2.2 13.8 6.4l-2.1.7-2.3 2.3.2 3.1-1.2 1.2-2.6-2.6L2.6 14.3l-.9-.9 3.2-3.2L2.3 7.6l1.2-1.2 3.1.2 2.3-2.3.7-2.1Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill={s.pinned ? 'currentColor' : 'none'} /></svg>
              </button>
            </div>
          {:else if r.t === 'grp'}
            <div class="grp">{r.label}</div>
          {:else if r.t === 'empty'}
            <div class="grp empty">还没有对话</div>
          {:else}
            <div class="conv">{@render convRow(r.c)}</div>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .sr { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .sr-tools { padding: 6px 10px 6px; display: flex; flex-direction: column; gap: 6px; border-bottom: 1px solid var(--border); }
  .sr-search { height: 28px; border: 1.5px solid var(--border2); border-radius: 8px; padding: 0 9px; font: inherit; font-size: 12.5px; background: #fff; color: var(--text); }
  .sr-search:focus { outline: none; border-color: #b7b2a6; }
  .sr-chips { display: flex; gap: 3px; align-items: center; flex-wrap: wrap; }
  .sr-chips button { border: none; background: none; padding: 2px 7px; border-radius: 6px; font: inherit; font-size: 11.5px; color: var(--dim); cursor: pointer; }
  .sr-chips button:hover { background: #f1efe9; }
  .sr-chips button.on { background: #e9e7e0; color: var(--text); font-weight: 550; }
  .sr-chips b { font-weight: 650; color: #b45309; }
  .sr-tag { margin-left: auto; border: none; background: none; font: inherit; font-size: 11.5px; color: var(--dim); max-width: 90px; }
  .sr-list { flex: 1; min-height: 0; overflow-y: auto; position: relative; }
  .sr-canvas { position: relative; }
  .sr-row { position: absolute; left: 0; right: 0; top: 0; display: flex; align-items: center; }
  .sr-empty { color: var(--faint); font-size: 12px; padding: 10px 14px; line-height: 1.7; margin: 0; }
  .site { margin: 0 8px; width: calc(100% - 16px); height: 36px; display: flex; align-items: center; gap: 8px; padding: 0 6px 0 8px; border-radius: 9px; cursor: pointer; min-width: 0; }
  .site:hover { background: #f1efe9; }
  .site.on { background: #e9e7e0; }
  .ic { color: var(--dim); display: inline-flex; flex: none; }
  .main { flex: 1; min-width: 0; display: flex; flex-direction: column; line-height: 1.2; }
  .main b { font-size: 12.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .main small { font-size: 10.5px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badges { display: inline-flex; align-items: center; gap: 4px; flex: none; }
  .cnt { min-width: 16px; height: 16px; padding: 0 4px; box-sizing: border-box; border-radius: 999px; background: #faf1d8; color: #8a5a08; font-size: 10px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; }
  .bad { width: 15px; height: 15px; border-radius: 50%; background: var(--err-soft); color: var(--err); font-size: 10px; font-weight: 800; display: inline-flex; align-items: center; justify-content: center; }
  .down { color: var(--err); font-size: 12px; font-weight: 700; }
  .run { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: pulse 1.1s infinite; }
  .pin { color: var(--faint); font-size: 7px; }
  .pin-btn { display: none; border: none; background: none; padding: 2px; color: var(--faint); cursor: pointer; border-radius: 5px; flex: none; }
  .pin-btn.pinned { color: var(--dim); }
  .site:hover .pin-btn { display: inline-flex; }
  .site:hover .pin { display: none; }
  .pin-btn:hover { color: var(--text); background: rgba(0,0,0,.05); }
  .grp { padding: 0 8px 0 40px; font-size: 11px; color: var(--faint); font-weight: 600; width: 100%; }
  .grp.empty { font-weight: 400; }
  .conv { width: 100%; padding-left: 14px; }
  @keyframes pulse { 50% { opacity: .35; } }
</style>
