<script lang="ts">
  // 站点页标题上的切换器：点站名弹出可搜索的站点列表，选中后留在同一个标签页。
  // 站名旁的铅笔（或双击站名）就地改名，回车保存、Esc 取消。
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import DeployMark from './DeployMark.svelte';
  import { sites, openSite } from '$lib/state/sites.svelte.js';
  import { invoke } from '@tauri-apps/api/core';
  import { say } from '$lib/state/app.svelte.js';
  import type { Site } from '$lib/types';
  import { selectConn } from '$lib/state/connections.svelte.js';
  import { SITES_CONN_ID } from '$lib/state/env';

  let open = $state(false);
  let q = $state('');
  let hi = $state(0);
  let root: HTMLDivElement | undefined = $state();
  let input: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  // site：分屏里的看板传自己的站点（没聚焦的看板不能显示成当前站点的名字）
  let { site = null }: { site?: Site | null } = $props();
  const cur = $derived(site ?? sites.active);
  const items = $derived.by(() => {
    const rank = new Map(sites.recent.map((id, i) => [id, i]));
    const needle = q.trim().toLowerCase();
    return [...sites.list]
      .filter((s) => !needle || `${s.name} ${s.domain} ${s.repo} ${s.tags.map((t) => `#${t}`).join(' ')}`.toLowerCase().includes(needle))
      .sort((a, b) => (rank.get(a.id) ?? 1e9) - (rank.get(b.id) ?? 1e9) || a.name.localeCompare(b.name, 'zh'));
  });
  $effect(() => { void q; hi = 0; });

  let editing = $state(false);
  let nameVal = $state('');
  let nameInput: HTMLInputElement | undefined = $state();
  function startRename() {
    if (!cur) return;
    open = false; editing = true; nameVal = cur.name;
    requestAnimationFrame(() => { nameInput?.focus(); nameInput?.select(); });
  }
  let renaming = false;
  async function commitRename() {
    if (!editing || renaming || !cur) return;
    const name = nameVal.trim();
    if (!name || name === cur.name) { editing = false; return; }
    renaming = true;
    try {
      const saved = await invoke<Site>('save_site', { site: { ...cur, name } });
      sites.list = sites.list.map((x) => (x.id === saved.id ? saved : x));
      // 设置页的草稿只同步名字，别把那边没保存的改动冲掉
      if (sites.draft?.id === saved.id) sites.draft.name = saved.name;
      say(`已改名为「${saved.name}」`);
    } catch (e) { say(String(e), 'err'); }
    finally { renaming = false; editing = false; }
  }
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
  {#if editing}
    <input class="sw-name" bind:this={nameInput} bind:value={nameVal} maxlength="60" spellcheck="false" aria-label="站点名称"
      onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); void commitRename(); } if (e.key === 'Escape') { e.preventDefault(); editing = false; } }}
      onblur={() => void commitRename()} />
  {:else}
    <button type="button" class="sw-trigger" class:open onclick={toggle} ondblclick={startRename} aria-haspopup="listbox" aria-expanded={open} data-tip={open ? null : '切换站点（双击改名）'}>
      <b>{cur?.name}</b>
      <svg class="chev" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </button>
    <button type="button" class="sw-edit" onclick={startRename} aria-label="重命名站点" data-tip="重命名">
      <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M10.5 2.5l3 3L6 13l-3.5.5L3 10z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" /></svg>
    </button>
  {/if}
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
            <span class="dep" data-tip={s.deploy.mode === 'vps' ? 'VPS' : s.deploy.mode === 'none' ? '未配置部署' : s.deploy.mode === 'workers' ? 'Cloudflare Workers' : s.deploy.mode === 'pages_git' ? 'Pages · Git 自动构建' : 'Cloudflare Pages'}><DeployMark mode={s.deploy.mode} size={11} /></span>
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
  .sw { position: relative; min-width: 0; display: flex; align-items: center; gap: 2px; }
  .sw-edit { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border: none; background: none; border-radius: 6px; color: var(--faint); cursor: pointer; opacity: 0; transition: opacity .12s, background-color .12s, color .12s; }
  .sw:hover .sw-edit, .sw-edit:focus-visible { opacity: 1; }
  .sw-edit:hover { background: rgba(0, 0, 0, .05); color: var(--text); }
  .sw-name { min-width: 0; width: 260px; max-width: 100%; font: inherit; font-size: 15px; font-weight: 600; line-height: 1.3; color: var(--text); padding: 1px 6px; margin-left: -7px; border: 1px solid var(--border2); border-radius: 7px; background: #fff; outline: none; }
  .sw-name:focus { border-color: var(--accent); }
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
