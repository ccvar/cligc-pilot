<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Conversation, SiteRow } from '$lib/types';
  import SiteMark from '$lib/components/icons/SiteMark.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { sidebar } from '$lib/state/sidebar.svelte.js';
  import { sites, siteIdOf, openSite } from '$lib/state/sites.svelte.js';
  import { fleet, trafficDown, setPinned } from '$lib/state/fleet.svelte.js';

  // 左栏只做导航：置顶的站点 + 最近打开的站点（当前站点总在其中，并展开它的会话）。
  // 找站点、筛选站点都在「全部站点」总览和全局搜索里——站点再多，这里也只有十来行。
  let { convRow }: { convRow: Snippet<[Conversation]> } = $props();

  const activeId = $derived(siteIdOf(app.activeConnId));
  const pinned = $derived(fleet.rows.filter((r) => r.pinned).sort((a, b) => a.name.localeCompare(b.name, 'zh')));
  const recent = $derived.by(() => {
    const ids = sites.recent.filter((id) => !fleet.byId.get(id)?.pinned);
    if (activeId && !ids.includes(activeId) && !fleet.byId.get(activeId)?.pinned) ids.unshift(activeId);
    return ids.map((id) => fleet.byId.get(id)).filter((r): r is SiteRow => !!r).slice(0, 8);
  });
  // 站点少（不超过 8 个）时没必要分「置顶 / 最近」，直接全列出来。
  const few = $derived(fleet.rows.length <= 8);
  const all = $derived([...fleet.rows].sort((a, b) => Number(b.pinned) - Number(a.pinned) || a.name.localeCompare(b.name, 'zh')));
</script>

{#snippet siteRow(s: SiteRow)}
  <div class="site {s.id === activeId ? 'on' : ''}" role="button" tabindex="0" title={s.domain || s.name}
    onclick={() => openSite(s.id)} onkeydown={(e) => e.key === 'Enter' && openSite(s.id)}>
    <span class="ic"><SiteMark size={14} /></span>
    <span class="name">{s.name}</span>
    <span class="badges">
      {#if s.job}<span class="run" title={s.job}></span>{/if}
      {#if s.proposed}<span class="cnt" title="{s.proposed} 条待确认">{s.proposed}</span>{/if}
      {#if s.last_deploy_ok === false || s.failed}<span class="bad" title={s.last_deploy_ok === false ? '上次上线失败' : `${s.failed} 条运营任务失败`}>!</span>{/if}
      {#if trafficDown(s)}<span class="down" title="点击比前 28 天明显下滑">↓</span>{/if}
    </span>
    <button class="pin-btn" class:pinned={s.pinned} title={s.pinned ? '取消置顶' : '置顶'} onclick={(e) => { e.stopPropagation(); void setPinned(s.id, !s.pinned); }}>
      <svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M9.6 2.2 13.8 6.4l-2.1.7-2.3 2.3.2 3.1-1.2 1.2-2.6-2.6L2.6 14.3l-.9-.9 3.2-3.2L2.3 7.6l1.2-1.2 3.1.2 2.3-2.3.7-2.1Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill={s.pinned ? 'currentColor' : 'none'} /></svg>
    </button>
  </div>
  {#if s.id === activeId}
    <div class="convs">
      {#each sidebar.grouped as g (g.slug)}
        <div class="grp">{g.name}</div>
        {#each g.subs.flatMap((x) => x.items) as c (c.id)}{@render convRow(c)}{/each}
      {:else}
        <div class="grp empty">还没有对话</div>
      {/each}
    </div>
  {/if}
{/snippet}

<div class="sr">
  {#if !fleet.loaded}
    <p class="sr-empty">正在读取站点…</p>
  {:else if !fleet.rows.length}
    <p class="sr-empty">还没有站点。在「全部站点」里导入，或在 Cloudflare 建站会话里「转为站点」。</p>
  {:else if few}
    {#each all as s (s.id)}{@render siteRow(s)}{/each}
  {:else}
    {#if pinned.length}
      <div class="sec">置顶</div>
      {#each pinned as s (s.id)}{@render siteRow(s)}{/each}
    {/if}
    {#if recent.length}
      <div class="sec">最近</div>
      {#each recent as s (s.id)}{@render siteRow(s)}{/each}
    {/if}
    {#if !pinned.length && !recent.length}
      <p class="sr-empty">在「全部站点」里打开站点，最近用过的会出现在这里；常用的可以置顶。</p>
    {/if}
  {/if}
</div>

<style>
  .sr { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 0 8px; }
  .sec { padding: 10px 18px 3px; font-size: 11px; font-weight: 600; color: var(--faint); }
  .sr-empty { color: var(--faint); font-size: 12px; padding: 10px 18px; line-height: 1.7; margin: 0; }
  .site { margin: 0 8px; height: 32px; display: flex; align-items: center; gap: 8px; padding: 0 6px 0 10px; border-radius: 9px; cursor: pointer; min-width: 0; }
  .site:hover { background: #f1efe9; }
  .site.on { background: #e9e7e0; }
  .ic { color: var(--dim); display: inline-flex; flex: none; }
  .name { flex: 1; min-width: 0; font-size: 12.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badges { display: inline-flex; align-items: center; gap: 4px; flex: none; }
  .cnt { min-width: 16px; height: 16px; padding: 0 4px; box-sizing: border-box; border-radius: 999px; background: #faf1d8; color: #8a5a08; font-size: 10px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; }
  .bad { width: 15px; height: 15px; border-radius: 50%; background: var(--err-soft); color: var(--err); font-size: 10px; font-weight: 800; display: inline-flex; align-items: center; justify-content: center; }
  .down { color: var(--err); font-size: 12px; font-weight: 700; }
  .run { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: pulse 1.1s infinite; }
  .pin-btn { display: none; border: none; background: none; padding: 2px; color: var(--faint); cursor: pointer; border-radius: 5px; flex: none; }
  .pin-btn.pinned { display: inline-flex; color: var(--dim); }
  .site:hover .pin-btn { display: inline-flex; }
  .pin-btn:hover { color: var(--text); background: rgba(0,0,0,.05); }
  .convs { padding: 1px 0 6px 12px; }
  .grp { padding: 6px 8px 2px 30px; font-size: 11px; color: var(--faint); font-weight: 600; }
  .grp.empty { font-weight: 400; }
  @keyframes pulse { 50% { opacity: .35; } }
</style>
