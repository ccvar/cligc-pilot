<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Conversation, SiteRow } from '$lib/types';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { sidebar } from '$lib/state/sidebar.svelte.js';
  import { siteIdOf, openSite } from '$lib/state/sites.svelte.js';
  import { fleet, trafficDown } from '$lib/state/fleet.svelte.js';
  import { isMaking } from '$lib/state/siteCreate.svelte.js';

  // 左栏只放「有对话的站点」：它是对话的入口，不是站点目录。没建过对话的站点不出现——
  // 找站点、管站点都在「全部站点」总览和全局搜索里。顺序：按最近一次对话。
  // 例外：当前打开的站点总在（它这一行就是站点看板的入口）。
  let { convRow }: { convRow: Snippet<[Conversation]> } = $props();

  const activeId = $derived(siteIdOf(app.activeConnId));
  // 运营任务开出来的会话每次一条，只露最近几条（在跑的、正在看的总会露出），更早的折叠；完整记录在「托管运营 → 操作日志」
  const OPS_SHOWN = 3;
  let showOld = $state<Set<string>>(new Set());
  function toggleOld(id: string) { const n = new Set(showOld); if (n.has(id)) n.delete(id); else n.add(id); showOld = n; }
  const lastConv = $derived.by(() => {
    const m = new Map<string, number>();
    for (const c of app.convos) {
      if (!c.conn_id.startsWith('site:')) continue;
      const id = c.conn_id.slice(5);
      m.set(id, Math.max(m.get(id) ?? 0, c.updated_at));
    }
    return m;
  });
  const list = $derived(
    fleet.rows
      .filter((r) => lastConv.has(r.id) || r.id === activeId)
      .sort((a, b) => (lastConv.get(b.id) ?? 0) - (lastConv.get(a.id) ?? 0)),
  );
</script>

<div class="sr">
  {#each list as s (s.id)}
    {@render siteRow(s)}
  {/each}
</div>

{#snippet siteRow(s: SiteRow)}
  <div class="site" class:on={s.id === activeId && app.view === 'site'} class:ctx={s.id === activeId && app.view !== 'site'} role="button" tabindex="0" title={s.id === activeId ? '打开站点看板' : s.domain || s.name}
    onclick={() => openSite(s.id)} onkeydown={(e) => e.key === 'Enter' && openSite(s.id)}>
    <span class="ic"><SiteIcon id={s.id} size={14} /></span>
    <span class="name">{s.name}</span>
    <span class="badges">
      {#if isMaking(s.id)}<span class="making" title="在 Pilot 里制作的站点，还没配置上线">制作中</span>{/if}
      {#if s.job}<span class="run" title={s.job}></span>{/if}
      {#if s.proposed}<span class="cnt" title="{s.proposed} 条待确认">{s.proposed}</span>{/if}
      {#if s.last_deploy_ok === false || s.failed}<span class="bad" title={s.last_deploy_ok === false ? '上次上线失败' : `${s.failed} 条运营任务失败`}>!</span>{/if}
      {#if trafficDown(s)}<span class="down" title="点击比前 28 天明显下滑">↓</span>{/if}
    </span>
  </div>
  {#if s.id === activeId}
    <div class="convs">
      {#each sidebar.grouped as g (g.slug)}
        {#if sidebar.grouped.length > 1}<div class="grp">{g.name}</div>{/if}
        {@const items = g.subs.flatMap((x) => x.items)}
        {#if g.slug === '__site_ops__'}
          {@const sorted = [...items].sort((a, b) => b.updated_at - a.updated_at)}
          {@const shown = showOld.has(s.id) ? sorted : sorted.filter((c, i) => i < OPS_SHOWN || app.running[c.id] || c.id === app.activeConvId)}
          {#each shown as c (c.id)}{@render convRow(c)}{/each}
          {#if sorted.length > OPS_SHOWN}
            <button class="more" onclick={() => toggleOld(s.id)}>
              {showOld.has(s.id) ? '收起' : `更早 ${sorted.length - shown.length} 条`}
            </button>
          {/if}
        {:else}
          {#each items as c (c.id)}{@render convRow(c)}{/each}
        {/if}
      {/each}
    </div>
  {/if}
{/snippet}

<style>
  .sr { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 0 8px; }
  .site { margin: 0 8px; height: 32px; display: flex; align-items: center; gap: 8px; padding: 0 8px 0 10px; border-radius: 9px; cursor: pointer; min-width: 0; }
  .site { transition: background-color .12s; }
  .site:hover { background: #f1efe9; }
  .site.on { background: #e9e7e0; }
  .site.ctx .ic { color: var(--text); }
  .ic { color: var(--dim); display: inline-flex; flex: none; }
  .name { flex: 1; min-width: 0; font-size: 12.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badges { display: inline-flex; align-items: center; gap: 4px; flex: none; }
  .making { font-size: 10px; color: var(--faint); border: 1px solid var(--border); border-radius: 999px; padding: 0 5px; line-height: 14px; }
  .cnt { min-width: 16px; height: 16px; padding: 0 4px; box-sizing: border-box; border-radius: 999px; background: #faf1d8; color: #8a5a08; font-size: 10px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; }
  .bad { width: 15px; height: 15px; border-radius: 50%; background: var(--err-soft); color: var(--err); font-size: 10px; font-weight: 800; display: inline-flex; align-items: center; justify-content: center; }
  .down { color: var(--err); font-size: 12px; font-weight: 700; }
  .run { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: pulse 1.1s infinite; }
  .convs { padding: 1px 0 6px 12px; animation: sr-in .18s cubic-bezier(.2, .7, .2, 1) backwards; }
  @keyframes sr-in { from { opacity: 0; transform: translateY(-3px); } }
  @media (prefers-reduced-motion: reduce) { .convs { animation: none; } }
  .more { display: block; margin: 1px 8px 2px 30px; padding: 3px 8px; border: none; background: none; border-radius: 6px; font: inherit; font-size: 11.5px; color: var(--faint); cursor: pointer; text-align: left; }
  .more:hover { color: var(--text); background: #f1efe9; }
  .grp { padding: 6px 8px 2px 30px; font-size: 11px; color: var(--faint); font-weight: 600; }
  @keyframes pulse { 50% { opacity: .35; } }
</style>
