<script lang="ts">
  // 托管页「最近上线 / 操作日志」卡片标题旁的「所有托管站点 ▾」：点开是可搜索的托管站点列表，单选。
  // 选了一个就只看它的记录（两个 tab 一起），第一项「所有托管站点」切回全部。不跨会话记住。
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';

  type Row = { id: string; name: string; domain: string };
  let { sites, counts, value = $bindable(''), unit = '条' }: { sites: Row[]; counts: Map<string, number>; value?: string; unit?: string } = $props();

  let open = $state(false);
  let q = $state('');
  let hi = $state(0);
  let root: HTMLDivElement | undefined = $state();
  let input: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  const label = (r: Row) => r.domain || r.name;
  const current = $derived(sites.find((s) => s.id === value));
  const total = $derived([...counts.values()].reduce((a, b) => a + b, 0));
  // 有记录的排前面（条数多的在前），没记录的按名字排在后面、变灰；搜索按域名和站点名
  const items = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    const rows = sites
      .filter((r) => !needle || `${r.name} ${r.domain}`.toLowerCase().includes(needle))
      .sort((a, b) => (counts.get(b.id) ?? 0) - (counts.get(a.id) ?? 0) || label(a).localeCompare(label(b), 'zh'));
    // 第一项「所有托管站点」：没在搜索时才列出来
    return [...(needle ? [] : [null]), ...rows] as (Row | null)[];
  });
  $effect(() => { void q; hi = 0; });

  function toggle() {
    open = !open;
    if (open) {
      q = '';
      hi = Math.max(0, items.findIndex((r) => (r?.id ?? '') === value));
      requestAnimationFrame(() => { input?.focus(); listEl?.querySelector<HTMLElement>('.mf-opt.hi')?.scrollIntoView({ block: 'nearest' }); });
    }
  }
  function pick(r: Row | null) { value = r?.id ?? ''; open = false; }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); open = false; return; }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!items.length) return;
      hi = (hi + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      requestAnimationFrame(() => listEl?.querySelector<HTMLElement>('.mf-opt.hi')?.scrollIntoView({ block: 'nearest' }));
      return;
    }
    if (e.key === 'Enter' && !e.isComposing && items.length) { e.preventDefault(); pick(items[hi] ?? null); }
  }
  $effect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => { if (root && !root.contains(e.target as Node)) open = false; };
    document.addEventListener('mousedown', close, true);
    return () => document.removeEventListener('mousedown', close, true);
  });
</script>

<div class="mf" bind:this={root}>
  <button type="button" class="mf-trigger" class:open class:on={!!current} onclick={toggle} aria-haspopup="listbox" aria-expanded={open} data-tip={open ? null : current ? `只看 ${label(current)} 的记录（点开换站点）` : '选一个站点，只看它的记录'}>
    {#if current}<SiteIcon id={current.id} size={12} />{label(current)}{:else}所有托管站点{/if}
    <svg class="chev" width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
  </button>
  {#if current}<button type="button" class="mf-clear" onclick={() => (value = '')} aria-label="看所有托管站点" data-tip="看所有托管站点"><Icon name="x" size={11} /></button>{/if}
  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="mf-pop" onkeydown={onKey} onmousedown={(e) => e.stopPropagation()}>
      <div class="mf-search">
        <Icon name="search" size={13} />
        <input bind:this={input} bind:value={q} placeholder={`在 ${sites.length} 个托管站点里搜索`} spellcheck="false" autocapitalize="off" autocorrect="off" />
      </div>
      <div class="mf-list" role="listbox" bind:this={listEl}>
        {#each items as r, i (r?.id ?? '__all')}
          {@const on = (r?.id ?? '') === value}
          {@const n = r ? (counts.get(r.id) ?? 0) : total}
          <button type="button" class="mf-opt" class:hi={i === hi} class:on class:none={r && !n} role="option" aria-selected={on}
            onmouseenter={() => (hi = i)} onclick={() => pick(r)}>
            <span class="ic">{#if r}<SiteIcon id={r.id} size={16} />{:else}<Icon name="grid" size={14} />{/if}</span>
            <span class="txt"><b>{r ? label(r) : '所有托管站点'}</b>{#if r && r.domain && r.domain !== r.name}<small>{r.name}</small>{/if}</span>
            <span class="n">{n ? `${n} ${unit}` : '没有记录'}</span>
            <span class="ck" aria-hidden="true">{#if on}<svg width="11" height="11" viewBox="0 0 12 12" fill="none"><path d="M2.5 6.2 5 8.5l4.5-5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}</span>
          </button>
        {:else}
          <p class="empty">没有匹配的站点</p>
        {/each}
      </div>
      <div class="mf-foot">↑↓ 选择 · Enter 切换 · Esc 关闭</div>
    </div>
  {/if}
</div>

<style>
  .mf { position: relative; display: inline-flex; align-items: center; gap: 1px; min-width: 0; }
  /* 左右负边距抵掉内边距：不悬停时和前后的文字挨着，悬停的底色才露出来 */
  .mf-trigger { display: inline-flex; align-items: center; gap: 4px; height: 20px; border: none; background: none; padding: 0 5px; margin: 0 -3px; border-radius: 6px; font: inherit; font-size: 12px; line-height: 20px; color: var(--faint); cursor: pointer; white-space: nowrap; transition: background-color .12s, color .12s; }
  .mf-trigger:hover, .mf-trigger.open { background: var(--su-hover); color: var(--text); }
  .mf-trigger.on { color: var(--text); font-weight: 550; }
  .mf-trigger .chev { flex: none; transition: transform .16s cubic-bezier(.2, .7, .2, 1); }
  .mf-trigger.open .chev { transform: rotate(180deg); }
  .mf-clear { display: inline-flex; align-items: center; justify-content: center; width: 18px; height: 18px; border: none; background: none; border-radius: 6px; color: var(--faint); cursor: pointer; }
  .mf-clear:hover { background: var(--su-hover); color: var(--text); }
  .mf-pop { position: absolute; z-index: 60; top: calc(100% + 6px); right: -8px; width: 320px; max-width: calc(100vw - 32px); background: var(--panel, #fff); border: 1px solid var(--border); border-radius: 12px; box-shadow: var(--shadow-lg); overflow: hidden; animation: mf-in .16s cubic-bezier(.2, .7, .2, 1) backwards; font-weight: 400; text-align: left; white-space: normal; }
  @keyframes mf-in { from { opacity: 0; transform: translateY(-4px); } }
  .mf-search { display: flex; align-items: center; gap: 8px; padding: 9px 12px; border-bottom: 1px solid var(--border); color: var(--faint); }
  .mf-search input { flex: 1; min-width: 0; border: none; outline: none; background: none; font: inherit; font-size: 13px; color: var(--text); }
  .mf-search input::placeholder { color: var(--faint); }
  .mf-list { max-height: 340px; overflow-y: auto; padding: 4px; }
  .mf-opt { width: 100%; display: flex; align-items: center; gap: 10px; padding: 6px 8px; border: none; background: none; border-radius: 8px; font: inherit; color: var(--text); text-align: left; cursor: pointer; }
  .mf-opt.hi { background: #f4f3ef; }
  .mf-opt .ic { flex: none; width: 16px; display: inline-flex; justify-content: center; color: var(--dim); }
  .mf-opt .txt { flex: 1; min-width: 0; display: flex; flex-direction: column; line-height: 1.35; }
  .mf-opt .txt b { font-size: 13px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mf-opt.on .txt b { font-weight: 600; }
  .mf-opt .txt small { font-size: 11px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mf-opt .n { flex: none; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .mf-opt.none .txt b, .mf-opt.none .ic { opacity: .55; }
  .mf-opt .ck { flex: none; width: 12px; display: inline-flex; color: var(--accent); }
  .empty { margin: 14px 8px; text-align: center; font-size: 12px; color: var(--faint); }
  .mf-foot { padding: 7px 12px; border-top: 1px solid var(--border); background: var(--rail); font-size: 11px; color: var(--faint); }
  @media (prefers-reduced-motion: reduce) { .mf-pop { animation: none; } }
</style>
