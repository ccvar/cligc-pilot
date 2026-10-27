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
  import { hostOf, convTitle } from '$lib/state/messages.svelte.js';
  import { startConversationPointerDrag, startConversationDrag, endConversationDrag } from '$lib/state/panes.svelte.js';
  import { brainLabel, modelIcon } from '$lib/state/models';
  import { permits } from '$lib/state/permits.svelte.js';
  import type { Conversation } from '$lib/types';
  import SitesRail from '$lib/components/sidebar/SitesRail.svelte';
  import RemoteRail from '$lib/components/sidebar/RemoteRail.svelte';
  import { openMaking, makingSites } from '$lib/state/siteCreate.svelte.js';
  import AccountRail from '$lib/components/sidebar/AccountRail.svelte';
  import { fleet } from '$lib/state/fleet.svelte.js';
  import { managed, openManaged } from '$lib/state/managed.svelte.js';
  import { selectConn } from '$lib/state/connections.svelte.js';
  import { SITES_CONN_ID } from '$lib/state/env';
  import { openSshConnect } from '$lib/state/ssh.svelte.js';
</script>

<!-- 按站点 / 项目分组的对话列表（普通模式直接放；Cloudflare / GitHub 模式放在当前账号下面） -->
{#snippet groupList()}
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
{/snippet}

<!-- 对话行：普通工作区和站点列表共用 -->
{#snippet convRow(c: Conversation)}
<!-- 行本身不是按钮（里面还有删除键，按钮不能套按钮）：标题区是打开对话的按钮，× 是并列的另一个 -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="convo {app.activeConvId === c.id ? 'on' : ''}" onpointerdown={(e)=>startConversationPointerDrag(e,c.id)} draggable="true" ondragstart={(e)=>startConversationDrag(e,c.id)} ondragend={endConversationDrag}>
  <button type="button" class="convo-body" onclick={() => openConv(c.id)}>
    <span class="convo-bi" title={brainLabel(c.brain)}><BrainIcon brain={modelIcon(c.brain)} size={11} /></span>
    <span class="convo-title">{convTitle(c.title)}</span>
    {#if c.status === 'running' || app.running[c.id]}<span class="mini-run"></span>{/if}
    {#if permits.permConvs.has(c.id)}<span class="mini-permit" title="有操作等你批准">待批</span>{/if}
    <span class="convo-when">{relTime(c.updated_at)}</span>
  </button>
  <button class="convo-x" title="删除对话" aria-label="删除对话" data-no-drag onclick={(e) => { e.stopPropagation(); deleteConv(c.id); }}>×</button>
</div>
{/snippet}

<!-- 左栏 -->
<aside class="rail" class:collapsed={ui.railCollapsed} style="width:{ui.railWidth}px">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="rail-resize" title="拖动调整宽度" onmousedown={startResize} role="separator" aria-orientation="vertical"></div>
  <div class="rail-head">
    <!-- 菜单项都能拖进主区分屏（点击＝在当前这一格打开，拖动＝新开一格）。
         远程模式顶上不放「新对话」（在终端旁边的输入框里就能和 AI 聊这台机器），换成「新建远程连接」 -->
    {#if app.isSshConn}
    <button class="newchat" onclick={() => openSshConnect()}>
      <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>
      新建远程连接
    </button>
    {:else}
    <button class="newchat" onclick={newChat} disabled={!app.activeConn} title="新对话（拖到右边可以分屏）" onpointerdown={(e) => { if (app.activeConn) startConversationPointerDrag(e, 'page:launcher'); }}>
      <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
        <path d="M11.5 2.5l2 2L6 12l-2.5.5L4 10l7.5-7.5z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
        <path d="M2.5 13.5h11" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
      </svg>
      新对话
    </button>
    {/if}
    {#if app.isSitesMode}
    <button class="railnav {app.view === 'sites' ? 'on' : ''}" onclick={() => selectConn(SITES_CONN_ID)} onpointerdown={(e) => startConversationPointerDrag(e, 'page:sites')}>
      <SiteMark size={14} />
      全部站点
      {#if fleet.pendingCount}<span class="nav-cnt" title="{fleet.pendingCount} 条改动等你确认">{fleet.pendingCount}</span>{/if}
    </button>
    <button class="railnav {app.view === 'making' ? 'on' : ''}" onclick={() => openMaking()} onpointerdown={(e) => startConversationPointerDrag(e, 'page:making')}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m21.6 2.4-1.2-1.2a1.2 1.2 0 0 0-1.7 0L2.4 17.5a1.2 1.2 0 0 0 0 1.7l1.2 1.2a1.2 1.2 0 0 0 1.7 0L21.6 4.1a1.2 1.2 0 0 0 0-1.7Z" /><path d="m14 7 3 3" /><path d="M5 6v4M19 14v4M10 2v2M7 8H3M21 16h-4M11 3H9" /></svg>
      站点制作
      {#if makingSites().length}<span class="nav-dim">{makingSites().length}</span>{/if}
    </button>
    <button class="railnav {app.view === 'managed' ? 'on' : ''}" onclick={() => openManaged()} onpointerdown={(e) => startConversationPointerDrag(e, 'page:managed')}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true"><path d="M12 8V4H8" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /><rect x="4" y="8" width="16" height="12" rx="2" stroke="currentColor" stroke-width="2" stroke-linejoin="round" /><path d="M2 14h2M20 14h2M9 13v2M15 13v2" stroke="currentColor" stroke-width="2" stroke-linecap="round" /></svg>
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
  {:else if app.isSshConn}
    <RemoteRail {convRow} />
  {:else if app.isCfConn || app.isGhConn}
    <AccountRail kind={app.isGhConn ? 'github' : 'cloudflare'} current={groupList} />
  {:else}
  <div class="convos" class:scrolled={ui.convosScrolled} onscroll={(e) => (ui.convosScrolled = (e.currentTarget as HTMLElement).scrollTop > 2)}>
    {#if sidebar.grouped.length === 0}
      <p class="rail-empty">还没有对话。<br />{app.isSiteConn ? '在这个站点里开一个对话，让 AI 改代码、看数据。' : app.isSshConn ? '在下方面板里直接说你想让这台机器做什么。' : app.isCfConn ? '给项目起个名，直接说你想要什么样的网站。' : app.isGhConn ? '选一个仓库，直接说你想改什么。' : '选好模型，直接说你想做什么。'}</p>
    {/if}
    {@render groupList()}
  </div>
  {/if}

  <ConnectionSwitcher />
</aside>

<style>
  :global(.app.win) .rail { border-right: none; }
  /* ---- 左栏 ---- */
  .rail { position: relative; width: 240px; flex: none; display: flex; flex-direction: column; background: var(--rail); border-right: .5px solid rgba(30, 25, 15, .1); padding-top: 30px; }
  .rail.collapsed { display: none; }
  /* 右缘拖拽把手：改侧栏宽度。 */
  /* 只用 col-resize 光标提示，不画任何指示线（避免拖动时多出一条竖线）。 */
  .rail-resize { position: absolute; top: 30px; right: -3px; bottom: 0; width: 7px; cursor: col-resize; z-index: 5; }
  .rail-head { padding: 8px 8px 2px; display: flex; flex-direction: column; gap: 2px; }
  .newchat { display: flex; align-items: center; gap: 8px; width: 100%; padding: 7px 10px;
    background: none; color: var(--text); border: none; border-radius: 9px; font-size: 13.5px; font-weight: 550; cursor: pointer; text-align: left; }
  .newchat:hover { background: #f5f4f1; }
  .newchat:disabled { opacity: .5; cursor: default; }
  .newchat svg { flex: none; color: var(--accent); }
  .railnav { display: flex; align-items: center; gap: 8px; width: 100%; padding: 5px 10px; background: none;
    border: none; border-radius: 9px; font-size: 13px; color: var(--dim); cursor: pointer; text-align: left; margin-top: -4px; }
  .railnav:hover { background: #f5f4f1; color: var(--text); }
  .railnav.on { background: #f0efeb; color: var(--text); font-weight: 550; }
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
  .convo:hover { background: #f5f4f1; }
  .convo.on { background: #f0efeb; }
  .convo-body { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; background: none; border: 0; padding: 0; margin: 0; font: inherit; color: inherit; text-align: left; cursor: pointer; border-radius: 6px; }
  .convo-body:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .convo-bi { flex: none; display: inline-flex; align-items: center; }
  /* 对话标题：比分组头（.site-grp-name 12.5px/var(--text)）小半号、淡一档——让站点头当主、对话当项 */
  .convo-title { flex: 1; min-width: 0; font-size: 12px; font-weight: 500; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* 时间和悬停时换上来的 × 占同一个 44px 位：切换时标题的截断位置不跳 */
  .convo-when { flex: none; width: 44px; text-align: right; font-size: 11px; color: var(--dim); white-space: nowrap; overflow: hidden; }
  .mini-run { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: pulse 1.1s infinite; flex: none; }
  .mini-permit { font-size: 10px; font-weight: 600; color: #b45309; background: #faf1d8; padding: 0 5px; border-radius: 4px; flex: none; }
  .convo-x { display: none; box-sizing: border-box; width: 44px; text-align: right; background: none; border: none; color: var(--faint); font-size: 16px; line-height: 1; padding: 1px 2px 1px 4px; border-radius: 5px; cursor: pointer; flex: none; }
  .convo:hover .convo-x, .convo:focus-within .convo-x { display: block; }
  .convo:hover .convo-when, .convo:focus-within .convo-when { display: none; }
  /* × 顶替时间的位置：从标题按钮里把时间收掉、× 在外面同宽补上，整行宽度不变 */
  .convo:hover .convo-body, .convo:focus-within .convo-body { margin-right: -6px; }
  .convo-x:hover { color: var(--dim); }
  @keyframes pulse { 50% { opacity: .35; } }
  .site-grp-fav { width: 14px; height: 14px; border-radius: 3px; flex: none; object-fit: cover; }
</style>
