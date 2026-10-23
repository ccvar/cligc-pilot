<script lang="ts">
  import SidebarConversationList from '$lib/SidebarConversationList.svelte';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import TemplateIcon from '$lib/components/icons/TemplateIcon.svelte';
  import PromptIcon from '$lib/components/icons/PromptIcon.svelte';
  import SshMark from '$lib/components/icons/SshMark.svelte';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import WorkspaceMark from '$lib/components/icons/WorkspaceMark.svelte';
  import SiteMark from '$lib/components/icons/SiteMark.svelte';
  import BoardIcon from '$lib/components/icons/BoardIcon.svelte';
  import { openSiteBoard } from '$lib/state/sites.svelte.js';
  import ConnectionSwitcher from '$lib/components/sidebar/ConnectionSwitcher.svelte';
  import { ui, startResize } from '$lib/state/ui.svelte.js';
  import { newChat, openConv, deleteConv } from '$lib/state/conversations';
  import { app } from '$lib/state/app.svelte.js';
  import { openTemplates, openPrompts } from '$lib/state/library.svelte.js';
  import { sidebar, toggleSite, faviconOf, relTime } from '$lib/state/sidebar.svelte.js';
  import { hostOf } from '$lib/state/messages.svelte.js';
  import { startConversationPointerDrag, startConversationDrag, endConversationDrag } from '$lib/state/panes.svelte.js';
  import { brainLabel, modelIcon } from '$lib/state/models';
  import { permits } from '$lib/state/permits.svelte.js';
  import type { Conversation } from '$lib/types';
  import SitesRail from '$lib/components/sidebar/SitesRail.svelte';
  import { fleet } from '$lib/state/fleet.svelte.js';
  import { managed, openManaged } from '$lib/state/managed.svelte.js';
  import { selectConn } from '$lib/state/connections.svelte.js';
  import { SITES_CONN_ID } from '$lib/state/env';
</script>

<!-- 对话行：普通工作区和站点列表共用 -->
{#snippet convRow(c: Conversation)}
<div class="convo {app.activeConvId === c.id ? 'on' : ''}" onpointerdown={(e)=>startConversationPointerDrag(e,c.id)} draggable="true" ondragstart={(e)=>startConversationDrag(e,c.id)} ondragend={endConversationDrag} role="button" tabindex="0"
  onclick={() => openConv(c.id)} onkeydown={(e) => e.key === 'Enter' && openConv(c.id)}>
  <div class="convo-body">
    <span class="convo-bi" title={brainLabel(c.brain)}><BrainIcon brain={modelIcon(c.brain)} size={11} /></span>
    <span class="convo-title">{c.title}</span>
    {#if c.status === 'running' || app.running[c.id]}<span class="mini-run"></span>{/if}
    {#if permits.permConvs.has(c.id)}<span class="mini-permit" title="有操作等你批准">待批</span>{/if}
    <span class="convo-when">{relTime(c.updated_at)}</span>
  </div>
  <button class="convo-x" title="删除对话" onclick={(e) => { e.stopPropagation(); deleteConv(c.id); }}>×</button>
</div>
{/snippet}

<!-- 左栏 -->
<aside class="rail" class:collapsed={ui.railCollapsed} style="width:{ui.railWidth}px">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="rail-resize" title="拖动调整宽度" onmousedown={startResize} role="separator" aria-orientation="vertical"></div>
  <div class="rail-head">
    <button class="newchat" onclick={newChat} disabled={!app.activeConn} title="新对话">
      <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
        <path d="M11.5 2.5l2 2L6 12l-2.5.5L4 10l7.5-7.5z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
        <path d="M2.5 13.5h11" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
      </svg>
      新对话
    </button>
    {#if app.isSitesMode}
    <button class="railnav {app.view === 'sites' ? 'on' : ''}" onclick={() => selectConn(SITES_CONN_ID)}>
      <SiteMark size={14} />
      全部站点
      {#if fleet.pendingCount}<span class="nav-cnt" title="{fleet.pendingCount} 条改动等你确认">{fleet.pendingCount}</span>{/if}
    </button>
    <button class="railnav {app.view === 'managed' ? 'on' : ''}" onclick={() => openManaged()}>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 1.8 9.5 5l3.5.4-2.6 2.4.7 3.5L8 9.6l-3.1 1.7.7-3.5L3 5.4 6.5 5 8 1.8Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M3.5 14h9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg>
      托管运营
      {#if managed.overview?.config.paused}<span class="nav-dim">已暂停</span>{:else if managed.overview?.waiting}<span class="nav-cnt" title="{managed.overview.waiting} 项需要你">{managed.overview.waiting}</span>{:else if managed.count}<span class="nav-dim">{managed.count}</span>{/if}
    </button>
    {/if}
    {#if app.isSiteConn && !app.isSitesMode}
    <button class="railnav {app.view === 'site' ? 'on' : ''}" onclick={() => openSiteBoard()}>
      <BoardIcon size={14} />
      站点看板
    </button>
    {/if}
    {#if app.isCfConn}
    <button class="railnav {app.view === 'templates' ? 'on' : ''}" onclick={openTemplates}>
      <TemplateIcon size={14} />
      模板库
    </button>
    <button class="railnav {app.view === 'prompts' ? 'on' : ''}" onclick={openPrompts}>
      <PromptIcon size={14} />
      提示词
    </button>
    {/if}
  </div>

  {#if app.isSitesMode}
    <SitesRail {convRow} />
  {:else}
  <div class="convos" class:scrolled={ui.convosScrolled} onscroll={(e) => (ui.convosScrolled = (e.currentTarget as HTMLElement).scrollTop > 2)}>
    {#if sidebar.grouped.length === 0}
      <p class="rail-empty">还没有对话。<br />{app.isSiteConn ? '在这个站点里开一个对话，让 AI 改代码、看数据。' : app.isSshConn ? '在下方面板里直接说你想让这台机器做什么。' : app.isCfConn ? '给项目起个名，直接说你想要什么样的网站。' : app.isGhConn ? '选一个仓库，直接说你想改什么。' : '选好模型，直接说你想做什么。'}</p>
    {/if}
    {#each sidebar.grouped as g (JSON.stringify([app.activeConnId, g.slug]))}
      <button class="site-grp" onclick={() => toggleSite(g.slug)} title={g.name}>
        <svg class="site-grp-chev" class:collapsed={sidebar.collapsedSites.has(g.slug)} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
        {#if app.isSiteConn}
          <SiteMark size={13} />
        {:else if app.isSshConn}
          <SshMark size={13} />
        {:else if app.isGhConn}
          <GhMark size={13} />
        {:else if app.isCfConn}
          {#if g.url}<img class="site-grp-fav" src={faviconOf(g.url)} alt="" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.display = 'none')} />{/if}
        {:else}
          <WorkspaceMark size={14} />
        {/if}
        <span class="site-grp-name">{g.name}</span>
        {#if g.url}<span class="site-grp-host" title={hostOf(g.url)}>{hostOf(g.url)}</span>{/if}
      </button>
      {#if !sidebar.collapsedSites.has(g.slug)}
        <SidebarConversationList items={g.subs.flatMap(sub => sub.items)} label={g.name}>
          {#snippet row(c)}{@render convRow(c)}{/snippet}
        </SidebarConversationList>
      {/if}
    {/each}
  </div>
  {/if}

  <ConnectionSwitcher />
</aside>

<style>
  :global(.app.win) .rail { border-right: none; }
  /* ---- 左栏 ---- */
  .rail { position: relative; width: 240px; flex: none; display: flex; flex-direction: column; background: var(--rail); border-right: 1px solid var(--border); padding-top: 30px; }
  .rail.collapsed { display: none; }
  /* 右缘拖拽把手：改侧栏宽度。 */
  /* 只用 col-resize 光标提示，不画任何指示线（避免拖动时多出一条竖线）。 */
  .rail-resize { position: absolute; top: 30px; right: -3px; bottom: 0; width: 7px; cursor: col-resize; z-index: 5; }
  .rail-head { padding: 8px 8px 2px; display: flex; flex-direction: column; gap: 2px; }
  .newchat { display: flex; align-items: center; gap: 8px; width: 100%; padding: 7px 10px;
    background: none; color: var(--text); border: none; border-radius: 9px; font-size: 13.5px; font-weight: 550; cursor: pointer; text-align: left; }
  .newchat:hover { background: #f1efe9; }
  .newchat:disabled { opacity: .5; cursor: default; }
  .newchat svg { flex: none; color: var(--accent); }
  .railnav { display: flex; align-items: center; gap: 8px; width: 100%; padding: 5px 10px; background: none;
    border: none; border-radius: 9px; font-size: 13px; color: var(--dim); cursor: pointer; text-align: left; margin-top: -4px; }
  .railnav:hover { background: #f1efe9; color: var(--text); }
  .railnav.on { background: #e9e7e0; color: var(--text); font-weight: 550; }
  .railnav:disabled { opacity: .45; cursor: default; }
  .railnav :global(svg) { flex: none; }
  .nav-dim { margin-left: auto; font-size: 11px; color: var(--faint); }
  .nav-cnt { margin-left: auto; min-width: 17px; height: 17px; padding: 0 5px; box-sizing: border-box; border-radius: 999px; background: #faf1d8; color: #8a5a08; font-size: 10.5px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; }
  .convos { flex: 1; overflow-y: auto; padding: 2px 8px 8px; display: flex; flex-direction: column; gap: 1px; border-top: 1px solid transparent; transition: border-color .15s; }
  .convos > button.site-grp:first-of-type { padding-top: 3px; margin-top: 0; }
  .convos.scrolled { border-top-color: rgba(30, 25, 15, 0.07); }
  .rail-empty { color: var(--faint); font-size: 12px; padding: 10px 8px; line-height: 1.7; }
  /* 站点分组头（可折叠） */
  .site-grp { width: 100%; display: flex; align-items: center; gap: 6px; padding: 9px 8px 4px 10px; margin-top: 2px; background: none; border: none; cursor: pointer; text-align: left; -webkit-appearance: none; appearance: none; }
  .site-grp:first-child { margin-top: 0; }
  .site-grp:hover .site-grp-name { color: var(--accent); }
  .site-grp-chev { color: var(--faint); flex: none; transition: transform .12s; }
  .site-grp-chev.collapsed { transform: rotate(-90deg); }
  .site-grp-name { flex: 0 1 auto; max-width: 58%; min-width: 0; font-size: 12.5px; font-weight: 600; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .site-grp-host { flex: 0 1 auto; min-width: 0; font-size: 10.5px; color: var(--accent); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* 任务类型子组标签 */
  /* padding-left 19：让对话行的厂商图标中心对齐分组头里的站点图标中心
     （分组头：10 左内边距 + 10 chevron + 6 gap，14px 图标中心落在 33px；
      对话行：8 外边距 + 19 左内边距，12px 图标中心也落在 33px）。顺带标题也更贴近站点名。 */
  .convo { position: relative; display: flex; align-items: center; gap: 6px; border-radius: 8px; margin: 0 8px; padding: 1px 6px 1px 19px; cursor: pointer; }
  .convo:hover { background: #f1efe9; }
  .convo.on { background: #e9e7e0; }
  .convo-body { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; }
  .convo-bi { flex: none; display: inline-flex; align-items: center; }
  /* 对话标题：比分组头（.site-grp-name 12.5px/var(--text)）小半号、淡一档——让站点头当主、对话当项 */
  .convo-title { flex: 1; min-width: 0; font-size: 12px; font-weight: 500; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .convo-when { flex: none; font-size: 11px; color: var(--dim); }
  .mini-run { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: pulse 1.1s infinite; flex: none; }
  .mini-permit { font-size: 10px; font-weight: 600; color: #b45309; background: #faf1d8; padding: 0 5px; border-radius: 4px; flex: none; }
  .convo-x { display: none; background: none; border: none; color: var(--faint); font-size: 16px; line-height: 1; padding: 1px 2px 1px 4px; border-radius: 5px; cursor: pointer; flex: none; }
  .convo:hover .convo-x, .convo:focus-within .convo-x { display: block; }
  .convo:hover .convo-when, .convo:focus-within .convo-when { display: none; }
  .convo-x:hover { color: var(--dim); }
  @keyframes pulse { 50% { opacity: .35; } }
  .site-grp-fav { width: 14px; height: 14px; border-radius: 3px; flex: none; object-fit: cover; }
</style>
