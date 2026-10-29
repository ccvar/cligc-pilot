<script lang="ts">
  // 全部站点标题旁的「N 个站点」：点开是和站点切换器一样的可搜索列表，但能多选。
  // 勾了几个就只看这几个（站点列表、待处理、处理记录都按它筛）；一个都没勾＝全部。
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import DeployMark from './DeployMark.svelte';
  import { sites } from '$lib/state/sites.svelte.js';
  import { fleet } from '$lib/state/fleet.svelte.js';
  import type { SiteRow } from '$lib/types';

  let { rows }: { rows: SiteRow[] } = $props();
  let open = $state(false);
  let q = $state('');
  // 搜索框下的轻量 tab：按部署方式分；站点只有一种部署方式时不显示
  let mode = $state('');
  const MODE_ORDER = ['direct', 'workers', 'pages_git', 'vps', 'none'];
  const MODE_L: Record<string, string> = { direct: 'Cloudflare Pages', workers: 'Cloudflare Workers', pages_git: 'Pages · Git 自动构建', vps: 'VPS', none: '未配置部署' };
  const modes = $derived(MODE_ORDER.map((m) => ({ m, l: MODE_L[m], n: rows.filter((r) => r.deploy_mode === m).length })).filter((x) => x.n));
  let hi = $state(0);
  let root: HTMLDivElement | undefined = $state();
  let input: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  // 站点删掉了还留在勾选里的不算
  const picked = $derived(new Set(fleet.picks.filter((id) => rows.some((r) => r.id === id))));
  const items = $derived.by(() => {
    const rank = new Map(sites.recent.map((id, i) => [id, i]));
    const needle = q.trim().toLowerCase();
    return [...rows]
      .filter((r) => !mode || r.deploy_mode === mode)
      .filter((r) => !needle || `${r.name} ${r.domain} ${r.repo} ${r.tags.map((t) => `#${t}`).join(' ')}`.toLowerCase().includes(needle))
      .sort((a, b) => (rank.get(a.id) ?? 1e9) - (rank.get(b.id) ?? 1e9) || (a.domain || a.name).localeCompare(b.domain || b.name, 'zh'));
  });
  $effect(() => { void q; void mode; hi = 0; });

  function toggle() {
    open = !open;
    if (open) { q = ''; mode = ''; hi = 0; requestAnimationFrame(() => input?.focus()); }
  }
  function flip(id: string) {
    fleet.picks = picked.has(id) ? [...picked].filter((x) => x !== id) : [...picked, id];
  }
  // 搜索或切了 tab 时：把列出来的都勾上；都没有：不用全选（一个不勾就是全部）
  function pickAllShown() { fleet.picks = [...new Set([...picked, ...items.map((r) => r.id)])]; }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); open = false; return; }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!items.length) return;
      hi = (hi + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      requestAnimationFrame(() => listEl?.querySelector<HTMLElement>('.sp-opt.hi')?.scrollIntoView({ block: 'nearest' }));
      return;
    }
    if (e.key === 'Enter' && !e.isComposing && items[hi]) { e.preventDefault(); flip(items[hi].id); }
  }
  $effect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => { if (root && !root.contains(e.target as Node)) open = false; };
    document.addEventListener('mousedown', close, true);
    return () => document.removeEventListener('mousedown', close, true);
  });
</script>

<div class="sp" bind:this={root}>
  <button type="button" class="sp-trigger" class:open class:on={picked.size > 0} onclick={toggle} aria-haspopup="listbox" aria-expanded={open} data-tip={open ? null : picked.size ? `只看勾选的 ${picked.size} 个站点（点开改）` : '选几个站点，只看它们'}>
    {#if picked.size}已选 {picked.size} 个站点<span class="of">/ {rows.length}</span>{:else}{rows.length} 个站点{/if}
    <svg class="chev" width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
  </button>
  {#if picked.size}<button type="button" class="sp-clear" onclick={() => (fleet.picks = [])} aria-label="看全部站点" data-tip="看全部站点"><Icon name="x" size={11} /></button>{/if}
  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="sp-pop" onkeydown={onKey} onmousedown={(e) => e.stopPropagation()}>
      <div class="sp-search">
        <Icon name="search" size={13} />
        <input bind:this={input} bind:value={q} placeholder={`在 ${rows.length} 个站点里搜索`} spellcheck="false" autocapitalize="off" autocorrect="off" />
      </div>
      {#if modes.length > 1}
        <div class="sp-tabs" role="tablist" aria-label="按部署方式">
          <button type="button" role="tab" class:on={!mode} aria-selected={!mode} onclick={() => (mode = '')} aria-label="全部" data-tip="全部">全部<span class="n">{rows.length}</span></button>
          {#each modes as x (x.m)}<button type="button" role="tab" class:on={mode === x.m} aria-selected={mode === x.m} onclick={() => (mode = x.m)} aria-label={x.l} data-tip={`${x.l} · ${x.n} 个`}>{#if x.m === 'none'}<Icon name="x" size={12} />{:else}<DeployMark mode={x.m} />{/if}<span class="n">{x.n}</span></button>{/each}
        </div>
      {/if}
      <div class="sp-list" role="listbox" aria-multiselectable="true" bind:this={listEl}>
        {#each items as r, i (r.id)}
          {@const on = picked.has(r.id)}
          <button type="button" class="sp-opt" class:hi={i === hi} class:on role="option" aria-selected={on}
            onmouseenter={() => (hi = i)} onclick={() => flip(r.id)}>
            <span class="ck" aria-hidden="true">{#if on}<svg width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M2.5 6.2 5 8.5l4.5-5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}</span>
            <span class="ic"><SiteIcon id={r.id} size={16} /></span>
            <span class="txt"><b>{r.domain || r.name}</b>{#if r.domain && r.domain !== r.name}<small>{r.name}</small>{/if}</span>
            <span class="dep" data-tip={MODE_L[r.deploy_mode] ?? null}><DeployMark mode={r.deploy_mode} size={11} /></span>
          </button>
        {:else}
          <p class="empty">没有匹配的站点</p>
        {/each}
      </div>
      <div class="sp-foot">
        <span>{picked.size ? `已选 ${picked.size} 个` : '没勾＝看全部'} · ↑↓ Enter 勾选</span>
        <span class="grow"></span>
        {#if (q.trim() || mode) && items.length}<button type="button" onclick={pickAllShown}>{q.trim() ? `勾选搜到的 ${items.length} 个` : `这 ${items.length} 个全勾上`}</button>{/if}
        {#if picked.size}<button type="button" onclick={() => (fleet.picks = [])}>清空</button>{/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .sp { position: relative; display: inline-flex; align-items: center; gap: 2px; min-width: 0; }
  .sp-trigger { display: inline-flex; align-items: center; gap: 5px; border: none; background: none; padding: 2px 6px; margin-left: -6px; border-radius: 7px; font: inherit; font-size: 12.5px; color: var(--faint); cursor: pointer; white-space: nowrap; transition: background-color .12s, color .12s; }
  .sp-trigger:hover, .sp-trigger.open { background: var(--su-hover); color: var(--text); }
  .sp-trigger.on { color: var(--text); font-weight: 550; }
  .sp-trigger .of { color: var(--faint); font-weight: 400; }
  .sp-trigger .chev { flex: none; transition: transform .16s cubic-bezier(.2, .7, .2, 1); }
  .sp-trigger.open .chev { transform: rotate(180deg); }
  .sp-clear { display: inline-flex; align-items: center; justify-content: center; width: 20px; height: 20px; border: none; background: none; border-radius: 6px; color: var(--faint); cursor: pointer; }
  .sp-clear:hover { background: var(--su-hover); color: var(--text); }
  .sp-pop { position: absolute; z-index: 60; top: calc(100% + 6px); left: -8px; width: 320px; max-width: calc(100vw - 32px); background: var(--panel, #fff); border: 1px solid var(--border); border-radius: 12px; box-shadow: var(--shadow-lg); overflow: hidden; animation: sp-in .16s cubic-bezier(.2, .7, .2, 1) backwards; font-weight: 400; }
  @keyframes sp-in { from { opacity: 0; transform: translateY(-4px); } }
  .sp-search { display: flex; align-items: center; gap: 8px; padding: 9px 12px; border-bottom: 1px solid var(--border); color: var(--faint); }
  .sp-search input { flex: 1; min-width: 0; border: none; outline: none; background: none; font: inherit; font-size: 13px; color: var(--text); }
  .sp-search input::placeholder { color: var(--faint); }
  .sp-tabs { display: flex; gap: 2px; padding: 6px 8px 0; overflow-x: auto; scrollbar-width: none; }
  .sp-tabs::-webkit-scrollbar { display: none; }
  .sp-tabs button { flex: none; display: inline-flex; align-items: center; gap: 4px; height: 24px; padding: 0 7px; border: none; background: none; border-radius: 6px; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; white-space: nowrap; transition: background-color .12s, color .12s; }
  .sp-tabs button:hover { color: var(--text); background: var(--su-hover); }
  .sp-tabs button.on { color: var(--text); background: var(--su-hover); font-weight: 550; }
  .sp-tabs .n { font-size: 11px; color: var(--faint); font-weight: 400; font-variant-numeric: tabular-nums; }
  .sp-list { max-height: 340px; overflow-y: auto; padding: 4px; }
  .sp-opt { width: 100%; display: flex; align-items: center; gap: 10px; padding: 6px 8px; border: none; background: none; border-radius: 8px; font: inherit; color: var(--text); text-align: left; cursor: pointer; }
  .sp-opt.hi { background: #f4f3ef; }
  .sp-opt .ck { flex: none; width: 14px; height: 14px; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; border: 1.5px solid var(--border2); border-radius: 4px; color: #fff; transition: background-color .12s, border-color .12s; }
  .sp-opt.on .ck { background: var(--accent); border-color: var(--accent); }
  .sp-opt .ic { flex: none; width: 16px; display: inline-flex; justify-content: center; }
  .sp-opt .txt { flex: 1; min-width: 0; display: flex; flex-direction: column; line-height: 1.35; }
  .sp-opt .txt b { font-size: 13px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sp-opt.on .txt b { font-weight: 600; }
  .sp-opt .txt small { font-size: 11px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sp-opt .dep { flex: none; display: inline-flex; width: 14px; justify-content: center; color: var(--faint); opacity: .8; }
  .empty { margin: 14px 8px; text-align: center; font-size: 12px; color: var(--faint); }
  .sp-foot { display: flex; align-items: center; gap: 10px; padding: 7px 12px; border-top: 1px solid var(--border); background: var(--rail); font-size: 11px; color: var(--faint); }
  .sp-foot .grow { flex: 1; }
  .sp-foot button { border: none; background: none; padding: 0; font: inherit; font-size: 11.5px; color: var(--dim); cursor: pointer; text-decoration: underline; text-underline-offset: 2px; white-space: nowrap; }
  .sp-foot button:hover { color: var(--text); }
  @media (prefers-reduced-motion: reduce) { .sp-pop { animation: none; } }
</style>
