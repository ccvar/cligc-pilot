<script lang="ts">
  // 站点页标题上的切换器：点站名弹出可搜索的站点列表，选中后留在同一个标签页。
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import CfMark from '$lib/components/icons/CfMark.svelte';
  import { sites, openSite } from '$lib/state/sites.svelte.js';
  import { selectConn } from '$lib/state/connections.svelte.js';
  import { SITES_CONN_ID } from '$lib/state/env';

  let open = $state(false);
  let q = $state('');
  let hi = $state(0);
  let root: HTMLDivElement | undefined = $state();
  let input: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  const cur = $derived(sites.active);
  const items = $derived.by(() => {
    const rank = new Map(sites.recent.map((id, i) => [id, i]));
    const needle = q.trim().toLowerCase();
    return [...sites.list]
      .filter((s) => !needle || `${s.name} ${s.domain} ${s.repo} ${s.tags.map((t) => `#${t}`).join(' ')}`.toLowerCase().includes(needle))
      .sort((a, b) => (rank.get(a.id) ?? 1e9) - (rank.get(b.id) ?? 1e9) || a.name.localeCompare(b.name, 'zh'));
  });
  $effect(() => { void q; hi = 0; });

  function toggle() {
    open = !open;
    if (open) { q = ''; hi = 0; requestAnimationFrame(() => input?.focus()); }
  }
  function pick(id: string) {
    open = false;
    if (id !== cur?.id) openSite(id, sites.tab);
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); open = false; return; }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!items.length) return;
      hi = (hi + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      requestAnimationFrame(() => listEl?.querySelector<HTMLElement>('.sw-opt.hi')?.scrollIntoView({ block: 'nearest' }));
      return;
    }
    if (e.key === 'Enter' && !e.isComposing && items[hi]) { e.preventDefault(); pick(items[hi].id); }
  }
  $effect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => { if (root && !root.contains(e.target as Node)) open = false; };
    document.addEventListener('mousedown', close, true);
    return () => document.removeEventListener('mousedown', close, true);
  });
</script>

<div class="sw" bind:this={root}>
  <button type="button" class="sw-trigger" class:open onclick={toggle} aria-haspopup="listbox" aria-expanded={open} data-tip={open ? null : '切换站点'}>
    <b>{cur?.name}</b>
    <svg class="chev" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
  </button>
  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="sw-pop" onkeydown={onKey} onmousedown={(e) => e.stopPropagation()}>
      <div class="sw-search">
        <Icon name="search" size={13} />
        <input bind:this={input} bind:value={q} placeholder={`在 ${sites.list.length} 个站点里搜索`} spellcheck="false" autocapitalize="off" autocorrect="off" />
      </div>
      <div class="sw-list" role="listbox" bind:this={listEl}>
        {#each items as s, i (s.id)}
          <button type="button" class="sw-opt" class:hi={i === hi} class:cur={s.id === cur?.id} role="option" aria-selected={s.id === cur?.id}
            onmouseenter={() => (hi = i)} onclick={() => pick(s.id)}>
            <span class="ic"><SiteIcon id={s.id} size={16} /></span>
            <span class="txt"><b>{s.name}</b>{#if s.domain && s.domain !== s.name}<small>{s.domain}</small>{/if}</span>
            <span class="dep" data-tip={s.deploy.mode === 'vps' ? 'VPS' : s.deploy.mode === 'none' ? '未配置部署' : s.deploy.mode === 'workers' ? 'Cloudflare Workers' : 'Cloudflare Pages'}>{#if s.deploy.mode === 'vps'}<Icon name="server" size={11} />{:else if s.deploy.mode !== 'none'}<CfMark size={11} />{/if}</span>
            {#if s.id === cur?.id}<Icon name="check" size={13} />{/if}
          </button>
        {:else}
          <p class="empty">没有匹配的站点</p>
        {/each}
      </div>
      <div class="sw-foot">
        <span>↑↓ 选择 · Enter 打开 · Esc 关闭</span>
        <button type="button" onclick={() => { open = false; void selectConn(SITES_CONN_ID); }}>全部站点</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .sw { position: relative; min-width: 0; display: flex; }
  .sw-trigger { display: inline-flex; align-items: center; gap: 6px; min-width: 0; max-width: 100%; border: none; background: none; padding: 1px 6px 1px 0; margin: 0; border-radius: 7px; font: inherit; color: var(--text); cursor: pointer; transition: background-color .12s; }
  .sw-trigger b { font-size: 15px; font-weight: 600; line-height: 1.3; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sw-trigger .chev { flex: none; color: var(--faint); transition: transform .16s cubic-bezier(.2, .7, .2, 1), color .12s; }
  .sw-trigger:hover .chev, .sw-trigger.open .chev { color: var(--text); }
  .sw-trigger.open .chev { transform: rotate(180deg); }
  .sw-pop { position: absolute; z-index: 60; top: calc(100% + 6px); left: -8px; width: 320px; max-width: calc(100vw - 32px); background: var(--panel, #fff); border: 1px solid var(--border); border-radius: 12px; box-shadow: var(--shadow-lg); overflow: hidden; animation: sw-in .16s cubic-bezier(.2, .7, .2, 1) backwards; }
  @keyframes sw-in { from { opacity: 0; transform: translateY(-4px); } }
  .sw-search { display: flex; align-items: center; gap: 8px; padding: 9px 12px; border-bottom: 1px solid var(--border); color: var(--faint); }
  .sw-search input { flex: 1; min-width: 0; border: none; outline: none; background: none; font: inherit; font-size: 13px; color: var(--text); }
  .sw-search input::placeholder { color: var(--faint); }
  .sw-list { max-height: 340px; overflow-y: auto; padding: 4px; }
  .sw-opt { width: 100%; display: flex; align-items: center; gap: 10px; padding: 6px 8px; border: none; background: none; border-radius: 8px; font: inherit; color: var(--text); text-align: left; cursor: pointer; }
  .sw-opt.hi { background: #f4f3ef; }
  .sw-opt .ic { flex: none; width: 16px; display: inline-flex; justify-content: center; color: var(--dim); }
  .sw-opt .txt { flex: 1; min-width: 0; display: flex; flex-direction: column; line-height: 1.35; }
  .sw-opt .txt b { font-size: 13px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sw-opt.cur .txt b { font-weight: 600; }
  .sw-opt .txt small { font-size: 11px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sw-opt .dep { flex: none; display: inline-flex; width: 14px; justify-content: center; color: var(--faint); opacity: .8; }
  .sw-opt :global(.ico:last-child) { color: var(--text); }
  .empty { margin: 14px 8px; text-align: center; font-size: 12px; color: var(--faint); }
  .sw-foot { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 7px 12px; border-top: 1px solid var(--border); background: var(--rail); font-size: 11px; color: var(--faint); }
  .sw-foot button { border: none; background: none; padding: 0; font: inherit; font-size: 11.5px; color: var(--dim); cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .sw-foot button:hover { color: var(--text); }
  @media (prefers-reduced-motion: reduce) { .sw-pop { animation: none; } }
</style>
