<script lang="ts">
  import { hideTip, tip as tipAction } from '$lib/tip';
  import SidebarConversationList from '$lib/SidebarConversationList.svelte';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import AppIcon from '$lib/AppIcon.svelte';
  import ConversationCanvas from '$lib/ConversationCanvas.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import ModelFx from '$lib/ModelFx.svelte';
  import UsageRing from '$lib/UsageRing.svelte';
  import SftpTransfers from '$lib/SftpTransfers.svelte';
  import SiteFav from '$lib/SiteFav.svelte';
  import type { Message, Conversation, ToolCall, Brain } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SettingsIcon from '$lib/SettingsIcon.svelte';
  import { leaves } from '$lib/conversationLayout';
  import { sheetWidth } from '$lib/sheetWidth';
  import ApiServices from '$lib/ApiServices.svelte';
  import type { ApiProfile } from '$lib/apiProfiles';
  import { dialogGeometry } from '$lib/dialogGeometry';
  import FileCodeEditor from '$lib/FileCodeEditor.svelte';
  import ConfirmDialog from '$lib/ConfirmDialog.svelte';
  import { isWindows, WORKSPACE_CONN_ID, isMac, CLAUDE_INSTALL_CMD, GROK_INSTALL_CMD, keystoreName, SHEET_WIDTH, isMainWindow, DIALOG_GEOMETRY } from '$lib/state/env';
  import { app, finishConfirm } from '$lib/state/app.svelte.js';
  import { brainLabel, modelIcon, permOptsFor, permTone, permTipFor, brainUsable, brainSetupHint, brainTitle, ctxLimitAdaptive, customsOf } from '$lib/state/models';
  import { connections, openLocalWorkspace, selectConn, openConnCtx, connectionDisplayName, distroOf, sshSub, probeOs, toggleConnCtx, openConnWindow, openConnRemark, removeConn, saveConnRemark } from '$lib/state/connections.svelte.js';
  import { startConversationPointerDrag, startConversationDrag, endConversationDrag, panes, splitConversation, singleConversationDragOver, addSplit, focusSplit, conversationPresentation, closeSplit, paneScroll, panePrefill, uploadPaneFiles, restoreSplitQueue, setPaneText, paneInput, splitSubmit, splitSetting, splitModelOptions, splitStop } from '$lib/state/panes.svelte.js';
  import { thread, onThreadScroll, persistThreadPerm, pickThreadCombo, persistThreadEffort, persistThreadFast } from '$lib/state/thread.svelte.js';
  import { startChat, turns, disarmLimitAuto, armLimitAuto, retry, rebuildSession, send, stop } from '$lib/state/turns.svelte.js';
  import { newChat, openConv, deleteConv } from '$lib/state/conversations';
  import { permits, permitDesc, togglePermitCmd, respondPermit } from '$lib/state/permits.svelte.js';
  import { autogrow, composer, onComposerKey, onComposerDragOver, onComposerDragLeave, onComposerDrop, removeAttachment, onComposerPaste, legacyComposer, editQueued, clearQueued, setLegacyDraft, queueMessage } from '$lib/state/composer.svelte.js';
  import { launcher, STYLE_OPTS, chooseWorkspaceFolder, workspaceFolderName, pickCombo, persistLauncherEffort, setFast } from '$lib/state/launcher.svelte.js';
  import { hostOf, badImgUrls, splitAttachments, isImgPath, media, thumbKey, fmtClock, fmtRemain, splitGenImages, mdClick, mdRender, revealWorkdir, segs, executionProgress, activityLabel, activityDetail, activityFailureReason, activityStatusLabel, elapsedLabel, faviconGuess, compactToolRows } from '$lib/state/messages.svelte.js';
  import { openCfConnect, startPreview, cf, fillDeploy, installWrangler, openCfUpdate, cfUpdateConnection, closeCfConnect, cfTokenTemplateUrl, resetCfVerification, verifyCf, confirmCf } from '$lib/state/cloudflare.svelte.js';
  import { openSshConnect, sshForm, sshAuthOpts, pickSshKey, saveSshEdit, testSsh, confirmSsh, openSshEdit } from '$lib/state/ssh.svelte.js';
  import { workbench, reconnectTerm, toggleWbTerm, toggleWbChat, toggleWbFiles, missingSshCredential, repairActiveSshCredential, startWbResize, openRemote } from '$lib/state/workbench.svelte.js';
  import { sftp, onSftpDragOver, onSftpDragLeave, onSftpDrop, sftpUp, commitPath, sftpGo, startPathEdit, sftpRefresh, openMkdir, isSftpDeleting, sftpUpload, sftpDownloadSelected, cancelSftpTransfer, clearSftpTransfers, revealSftpTransfer, retryOrRepairSftp, setSort, openFctx, isSftpDeleteTarget, onSftpDragStart, sftpOpenRow, toggleDir, focusFileRename, saveFileRename, openRename, fmtSize, fmtStamp, startColDrag, onColKey, sftpDownload, copySftpEntry, duplicateSftpEntry, pasteSftpEntry, copyPath, sftpDelete, openNewFile, confirmFsAsk } from '$lib/state/sftp.svelte.js';
  import { editor, onEditKey, closeEdit, saveEdit } from '$lib/state/editor.svelte.js';
  import { openTemplates, openPrompts, library, catOf, loadTemplates, previewTemplate, openUseTmpl, delTemplate, openNewPrompt, usePrompt, copyPrompt, openEditPrompt, deletePrompt, CAT_MINE, openSaveTmpl, confirmSaveTmpl, confirmUseTmpl, savePrompt } from '$lib/state/library.svelte.js';
  import { openConnectionSetup, refreshBrainsManual, settings, installClaude, installGrok, installCodex, copyCmd, authorize, rebind, upgradeCli, saveClaudeToken, closeClaudeToken, removeCustom, addCustom } from '$lib/state/settings.svelte.js';
  import { updater, openUpdateCenter, closeUpdateCenter, checkPilotUpdate, pilotUpdateMessage, pilotUpdateLabel, runUpdate } from '$lib/state/updater.svelte.js';
  import { openSearch, search, pickEntry } from '$lib/state/search.svelte.js';
  import { sidebar, toggleSite, faviconOf, relTime } from '$lib/state/sidebar.svelte.js';
  import { onCtxMenu, onTipHover, ui, startDrag, toggleRail, startResize } from '$lib/state/ui.svelte.js';
  import { initConnections } from '$lib/state/connections.svelte.js';
  import { initSettings } from '$lib/state/settings.svelte.js';
  import { initPanes } from '$lib/state/panes.svelte.js';
  import { initLauncher } from '$lib/state/launcher.svelte.js';
  import { initPermits } from '$lib/state/permits.svelte.js';
  import { initComposer } from '$lib/state/composer.svelte.js';
  import { initThread } from '$lib/state/thread.svelte.js';
  import { initUi } from '$lib/state/ui.svelte.js';
  import { initSearch } from '$lib/state/search.svelte.js';
  import { initUpdater } from '$lib/state/updater.svelte.js';
  import { initApp } from '$lib/state/app.svelte.js';
  import { initWorkbench } from '$lib/state/workbench.svelte.js';
  import { initSftp } from '$lib/state/sftp.svelte.js';
  import { initEditor } from '$lib/state/editor.svelte.js';
  import { initLibrary } from '$lib/state/library.svelte.js';
  import { initMessages } from '$lib/state/messages.svelte.js';
  import { initTurns } from '$lib/state/turns.svelte.js';

  // Effects keep the registration order of the original single-file page.
  initConnections();
  initSettings();
  initPanes();
  initLauncher();
  initPermits();
  initComposer();
  initThread();
  initUi();
  initSearch();
  initUpdater();
  initApp();
  initWorkbench();
  initSftp();
  initEditor();
  initLibrary();
  initMessages();
  initTurns();
</script>

<svelte:window
  oncontextmenu={onCtxMenu}
  onmouseover={onTipHover}
  onscrollcapture={() => { hideTip(); ui.imgTip = null; ui.hoverWant = ''; }}
  onresize={() => { hideTip(); ui.imgTip = null; ui.hoverWant = ''; }}
  onkeydown={(e) => { if (editor.edOpen) { onEditKey(e); return; } if (e.key === 'Escape') { if (ui.lightbox) ui.lightbox = ''; } }}
/>
<main class="app" class:win={isWindows} class:fs={ui.isFullscreen} class:rail-collapsed={ui.railCollapsed} class:top-update-visible={updater.topUpdateVisible}>
  <!-- 融合式标题栏：透明拖拽条铺满顶部，红绿灯与工具按钮浮在其上（macOS Overlay） -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="titlebar" data-tauri-drag-region aria-hidden="true" onmousedown={startDrag} style="width:{ui.railCollapsed ? (updater.topUpdateVisible ? 167 : 140) : ui.railWidth}px"></div>

  <!-- 顶部工具：折叠侧栏 + 搜索会话（窗口模式紧挨红绿灯右侧；全屏时无红绿灯，与左栏菜单左对齐） -->
  <div class="win-tools" class:fs={ui.isFullscreen} class:win={isWindows}>
    <button class="wt" onclick={toggleRail} title={ui.railCollapsed ? '展开侧栏' : '折叠侧栏'} aria-label="折叠侧栏">
      <svg width="16" height="16" viewBox="0 0 18 18" fill="none">
        <rect x="2.5" y="3.5" width="13" height="11" rx="2" stroke="currentColor" stroke-width="1.3" />
        <path d="M7 3.7v10.6" stroke="currentColor" stroke-width="1.3" />
        <rect x="3.4" y="4.4" width="2.7" height="9.2" rx="1" fill="currentColor" opacity=".28" />
      </svg>
    </button>
    <button class="wt" onclick={openSearch} disabled={!app.activeConn} title="搜索会话" aria-label="搜索会话">
      <svg width="16" height="16" viewBox="0 0 18 18" fill="none">
        <circle cx="8" cy="8" r="5" stroke="currentColor" stroke-width="1.4" />
        <path d="M11.7 11.7L15 15" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
      </svg>
    </button>
    {#if updater.topUpdateVisible}
      <button class="wt upd" class:active={updater.updateCenterOpen} onclick={openUpdateCenter}
        title={updater.pilotUpdateStatus === 'checking' ? '正在检查更新' : updater.updAvail ? `发现新版本 ${updater.updAvail}，打开更新中心` : '打开更新中心'} aria-label="打开更新中心">
        {#if updater.updBusy || updater.pilotUpdateStatus === 'checking'}
          <svg class="upd-spin" width="15" height="15" viewBox="0 0 18 18" fill="none"><path d="M15.5 9a6.5 6.5 0 1 1-2-4.72" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>
        {:else}
          <svg width="15" height="15" viewBox="0 0 18 18" fill="none">
            <path d="M9 3v7.4M5.8 7.2 9 10.4l3.2-3.2" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
            <path d="M4.2 14.3h9.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
          </svg>
          {#if updater.updAvail}<span class="upd-count">1</span>{/if}
        {/if}
      </button>
    {/if}
  </div>

  {#if updater.updBusy}
    <div class="upd-toast" role="status">
      <svg class="upd-spin" width="13" height="13" viewBox="0 0 18 18" fill="none"><path d="M15.5 9a6.5 6.5 0 1 1-2-4.72" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg>
      <span class="upd-toast-msg">{updater.updMsg}</span>
      {#if updater.updBusy && updater.updPct >= 0}<span class="upd-toast-bar"><span style="width:{updater.updPct}%"></span></span>{/if}
    </div>
  {/if}

  <!-- 全局操作反馈挂在应用根层：无论当前打开抽屉、弹窗还是确认框，都应位于最外层。 -->
  {#if app.flash}<div class="flash {app.flashKind}" role="status" aria-live="polite">{app.flash}</div>{/if}

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
      {#if app.isCfConn}
      <button class="railnav {app.view === 'templates' ? 'on' : ''}" onclick={openTemplates}>
        {@render templateIcon(14)}
        模板库
      </button>
      <button class="railnav {app.view === 'prompts' ? 'on' : ''}" onclick={openPrompts}>
        {@render promptIcon(14)}
        提示词
      </button>
      {/if}
    </div>

    <div class="convos" class:scrolled={ui.convosScrolled} onscroll={(e) => (ui.convosScrolled = (e.currentTarget as HTMLElement).scrollTop > 2)}>
      {#if sidebar.grouped.length === 0}
        <p class="rail-empty">还没有对话。<br />{app.isSshConn ? '在下方面板里直接说你想让这台机器做什么。' : app.isCfConn ? '给项目起个名，直接说你想要什么样的网站。' : '选好模型，直接说你想做什么。'}</p>
      {/if}
      {#each sidebar.grouped as g (JSON.stringify([app.activeConnId, g.slug]))}
        <button class="site-grp" onclick={() => toggleSite(g.slug)} title={g.name}>
          <svg class="site-grp-chev" class:collapsed={sidebar.collapsedSites.has(g.slug)} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
          {#if app.isSshConn}
            {@render sshMark(13)}
          {:else if app.isCfConn}
            {#if g.url}<img class="site-grp-fav" src={faviconOf(g.url)} alt="" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.display = 'none')} />{/if}
          {:else}
            {@render workspaceMark(14)}
          {/if}
          <span class="site-grp-name">{g.name}</span>
          {#if g.url}<span class="site-grp-host" title={hostOf(g.url)}>{hostOf(g.url)}</span>{/if}
        </button>
        {#if !sidebar.collapsedSites.has(g.slug)}
          <SidebarConversationList items={g.subs.flatMap(sub => sub.items)} label={g.name}>
            {#snippet row(c)}
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
          </SidebarConversationList>
        {/if}
      {/each}
    </div>

    <div class="foot-wrap" bind:this={connections.footerEl}>
      {#if connections.switcherOpen}
        <div class="conn-switch" role="menu" aria-label="切换连接">
          <div class="cs-scroll" aria-label="连接列表">
            <button class="cs-item {app.activeConnId === WORKSPACE_CONN_ID ? 'on' : ''}" role="menuitemradio" aria-checked={app.activeConnId === WORKSPACE_CONN_ID} onclick={openLocalWorkspace}>
              {@render workspaceMark(18)}
              <span class="cs-main"><b>本地对话</b><small>不连接任何服务，可接入本地文件夹</small></span>
              {#if app.activeConnId === WORKSPACE_CONN_ID}<span class="cs-check">✓</span>{/if}
            </button>
            {#each app.conns as c (c.id)}
              <button class="cs-item {app.activeConnId === c.id ? 'on' : ''}" role="menuitemradio" aria-checked={app.activeConnId === c.id} onclick={() => { selectConn(c.id); connections.switcherOpen = false; }}
                oncontextmenu={(e) => openConnCtx(e, c)}>
                {#if c.kind === 'ssh'}{@render sshMark(18)}{:else}{@render cfMark(18)}{/if}
                <span class="cs-main"><b>{connectionDisplayName(c)}</b>
                  {#if c.kind === 'ssh'}
                    <small class="cs-os">{@render distroMark(distroOf(c), 12)}<span class="cs-os-label">{sshSub(c)}</span></small>
                  {:else}
                    <small><span class:conn-remark={!!c.remark?.trim()} data-tip={c.remark?.trim() ? `脱敏 Key：${c.key_prefix}` : undefined}>{c.remark?.trim() || c.key_prefix}</span> · Cloudflare</small>
                  {/if}</span>
                {#if app.activeConnId === c.id}<span class="cs-check">✓</span>{/if}
              </button>
            {/each}
          </div>
          <div class="cs-actions">
            <div class="cs-div"></div>
            <button class="cs-act" role="menuitem" onclick={openCfConnect}>{@render cfMark(15)}连接 Cloudflare</button>
            <button class="cs-act" role="menuitem" onclick={() => { connections.switcherOpen = false; openSshConnect(); }}>{@render sshMark(15)}新建远程连接</button>
            <button type="button" class="cs-act" role="menuitem" onclick={openConnectionSetup}>{@render gearIcon()}连接与模型设置…</button>
          </div>
        </div>
      {/if}
    <button class="rail-foot" class:open={connections.switcherOpen} onclick={() => { connections.switcherOpen = !connections.switcherOpen; }}>
      {#if app.isCfConn}{@render cfMark(18)}{:else if app.isSshConn}{@render sshMark(18)}{:else if app.activeConn}{@render workspaceMark(18)}{:else}<AppIcon size={18} />{/if}
      <span class="foot-main">
        <b>{app.activeConn ? connectionDisplayName(app.activeConn) : '未连接'}</b>
        {#if app.activeConn}<small>{#if app.isCfConn}{app.cfProjects.length} 个项目{sidebar.cfSiteCount ? ` · ${sidebar.cfSiteCount} 个站点` : ''}{:else if app.isSshConn}<!--
          远程连接：这里放远端系统版本（点一下重探，比如刚 do-release-upgrade 过）。
        --><span class="foot-os" role="button" tabindex="-1" title="远端系统 · 点击重新检测"
          onclick={(e) => { e.stopPropagation(); probeOs(app.activeConnId, true); }}
          onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); probeOs(app.activeConnId, true); } }}
          >{@render distroMark(distroOf(app.activeConn), 12)}{connections.osProbing[app.activeConnId] ? '检测系统…' : sshSub(app.activeConn)}</span>{:else}本地 · 不连接任何服务{/if}</small>{/if}
      </span>
      <!-- 永远是展开箭头：这是个切换器，不是设置快捷键。 -->
      <svg class="foot-chev" class:up={connections.switcherOpen} width="13" height="13" viewBox="0 0 12 12" fill="none">
        <path d="M2.75 7.5L6 4.25L9.25 7.5" stroke="currentColor" stroke-width="1.15" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
    </div>
  </aside>

  {#if panes.splitDragId && panes.splitDragPoint}
    {@const dragConversation=splitConversation(panes.splitDragId)}
    {#if dragConversation}
      <div class="conversation-drag-ghost" style={`--drag-x:${panes.splitDragPoint.x}px;--drag-y:${panes.splitDragPoint.y}px`} aria-hidden="true">
        <span class="conversation-drag-icon"><BrainIcon brain={modelIcon(dragConversation.brain)} size={12} /></span>
        <span class="conversation-drag-title">{dragConversation.title}</span>
      </div>
    {/if}
  {/if}

  <!-- 主区 -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section class="main" aria-label="工作区" data-pilot-workarea ondragover={singleConversationDragOver}
    ondragleave={(e)=>{if(!(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node))panes.singleDropEdge=null;}}
    ondrop={(e)=>{if(panes.splitDragId&&panes.singleDropEdge){e.preventDefault();const id=panes.splitDragId,edge=panes.singleDropEdge;endConversationDrag();void addSplit(id,edge);}}}>
    {#if panes.singleDropEdge && !panes.splitVisible}<div class={`single-split-preview ${panes.singleDropEdge}`} aria-hidden="true"><span>分屏视图</span></div>{/if}
    {#if ui.imgTip}
      {@const iu = ui.imgTip.url}
      <div class="imgtip" class:ready={ui.imgTipReady} class:below={ui.imgTip.below} style="left:{ui.imgTip.x}px; top:{ui.imgTip.y}px">
        <img src={iu} alt="" onload={() => { if (ui.imgTip?.url === iu) ui.imgTipReady = true; }} onerror={() => { badImgUrls.add(iu); if (ui.imgTip?.url === iu) ui.imgTip = null; }} />
      </div>
    {/if}
    {#if ui.lightbox}
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
      <div class="lightbox" onclick={() => (ui.lightbox = '')}><img src={ui.lightbox} alt="附件大图" /></div>
    {/if}

    {#if panes.splitVisible && panes.conversationLayout}
      <ConversationCanvas layout={panes.conversationLayout} focused={panes.splitFocused} dragging={!!panes.splitDragId} dragConversation={panes.splitDragId} preview={panes.splitPointerPreview} content={splitContent}
        onfocus={focusSplit} ondrop={(target,edge)=>{const id=panes.splitDragId;endConversationDrag();void addSplit(id,edge,target);}} onresize={(layout)=>panes.conversationLayout=layout}/>
    {:else if !app.activeConn}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="center" data-tauri-drag-region onmousedown={startDrag}>
        <div class="hero-card">
          <div class="hero-mark"><AppIcon size={54} /></div>
          <h1>开始之前，先选一个工作方式</h1>
          <p>直接和本地 Claude / Codex / Grok 或 API 模型对话；连接 Cloudflare 让它帮你建站并部署；或者连上一台远程服务器，让它帮你运维。</p>
          <div class="hero-btns">
            <button class="btn soft hero-import" onclick={openLocalWorkspace}>{@render workspaceMark(15)}本地对话</button>
            <button class="btn soft hero-import" onclick={openCfConnect}>{@render cfMark(15)}连接 Cloudflare</button>
            <button class="btn soft hero-import" onclick={openSshConnect}>{@render sshMark(15)}新建远程连接</button>
          </div>
          {@render cliGuide()}
        </div>
      </div>

    {:else if app.view === 'launcher'}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="center launch-center" data-tauri-drag-region onmousedown={startDrag}>
        <div class="launcher">
          {#if app.isCfConn}
            <h1>让 AI 帮你建个站</h1>
            <p class="sub">给项目起个名，描述你想要的网站——边聊边建，随时本地预览，满意了再部署到 Cloudflare。</p>
          {:else if app.isSshConn}
            <h1>想让它在这台机器上做点什么？</h1>
            <p class="sub">它通过 Pilot 在 {app.activeConn?.ssh_user}@{app.activeConn?.ssh_host} 上执行命令；每条命令按你选的权限档位确认，凭据始终不交给模型。</p>
          {:else}
            <h1>想让它帮你做点什么？</h1>
            <p class="sub">像聊天一样把需求说清楚；需要处理文件时，可以接入一个本地文件夹。</p>
          {/if}

            <div class="composer big">
                <textarea bind:value={launcher.lDraft} bind:this={launcher.lDraftEl} use:autogrow rows="3"
                  placeholder={app.isSshConn ? '例如：看看这台机器的配置和已装的东西，然后帮我装上 Docker' : app.isCfConn ? '例如：做个卖手冲咖啡的落地页，深色调，留个邮箱订阅表单存到 D1，先给我方案' : app.isWorkspaceChat ? '任意聊点什么；需要处理本地文件时，可在左下角添加一个文件夹' : '例如：分析这个站点最近的内容表现，并给出下一步运营计划'}
                  oncompositionstart={() => (composer.composing = true)} oncompositionend={() => (composer.composing = false)}
                  onkeydown={(e) => onComposerKey(e, startChat)}></textarea>
              <div class="composer-bar">
                <div class="cb-left">
                  {#if app.isSshConn}
                    <!-- 远程连接没有站点/项目可选：对象就是那台机器 -->
                    <span class="ssh-target">{@render sshMark(13)}{app.activeConn?.ssh_user}@{app.activeConn?.ssh_host}:{app.activeConn?.ssh_port}</span>
                  {:else if app.isCfConn}
                    <input class="cf-proj-in" bind:value={launcher.lSite} placeholder="项目名，如 coffee-landing" spellcheck="false" autocapitalize="off" autocorrect="off" />
                    <Dropdown compact bind:value={launcher.lStyle} options={STYLE_OPTS} />
                  {:else}
                    <div class="workspace-picker">
                      <button type="button" class="workspace-folder" class:selected={!!launcher.workspaceDir} data-tip={launcher.workspaceDir || '选择一个本地文件夹作为本次自由对话的工作目录'} onclick={chooseWorkspaceFolder}>
                        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M1.8 4.3a1.5 1.5 0 0 1 1.5-1.5h3l1.4 1.6h5a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5H3.3a1.5 1.5 0 0 1-1.5-1.5V4.3Z" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round" /></svg>
                        <span>{launcher.workspaceDir ? workspaceFolderName(launcher.workspaceDir) : '添加文件夹'}</span>
                      </button>
                      {#if launcher.workspaceDir}<button type="button" class="workspace-clear" aria-label="移除文件夹" data-tip="移除文件夹，改为纯对话" onclick={() => (launcher.workspaceDir = '')}>×</button>{/if}
                    </div>
                  {/if}
                </div>
                <div class="cb-right">
                  <Dropdown compact bind:value={launcher.lPerm} options={permOptsFor(launcher.lBrain, app.isSshConn)} tone={permTone(launcher.lPerm)} tip={permTipFor(launcher.lBrain, launcher.lPerm, app.isSshConn)} />
                  <ModelFx options={launcher.comboOpts} value={`${launcher.lBrain}::${launcher.lModel}`} effort={launcher.lEffort} fast={launcher.lFast} fastOk={app.brains?.claude_fast_ok ?? false} onpick={pickCombo} oneffort={persistLauncherEffort} onfast={setFast} />
                  <UsageRing />
                  <button class="send" onclick={startChat} disabled={(app.isCfConn && !launcher.lSite.trim()) || !launcher.lDraft.trim() || !brainUsable(launcher.lBrain)} title="发送（Enter）">↑</button>
                </div>
              </div>
            </div>
          {#if !brainUsable(launcher.lBrain)}<p class="hint warn-text">{brainSetupHint(launcher.lBrain)}</p>{/if}
          {#if app.isWorkspaceChat && app.brains && !app.anyBrainUsable}{@render cliGuide()}{/if}
          {#if app.isCfConn && app.brains?.wrangler && !app.brains.wrangler.found}{@render wrNote()}{/if}
        </div>
      </div>

    {:else if app.view === 'templates'}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <header class="thread-head" data-tauri-drag-region onmousedown={startDrag}>
        <div class="th-info">
          <div class="th-line"><b class="th-title">{@render templateIcon(16)}<span>模板库</span></b><small class="tmpl-hint">把做好的站点存成模板，之后引用它快速起新项目</small></div>
          {#if library.templates.length}
            <div class="tmpl-chips">
              <button class="tmpl-chip {library.tmplCat === '全部' ? 'on' : ''}" onclick={() => (library.tmplCat = '全部')}>{@render catIcon('全部')}全部<span class="tc-n">{library.templates.length}</span></button>
              {#each library.tmplCats as c (c)}
                <button class="tmpl-chip {library.tmplCat === c ? 'on' : ''}" onclick={() => (library.tmplCat = c)}>{@render catIcon(c)}{c}<span class="tc-n">{library.templates.filter((t) => catOf(t) === c).length}</span></button>
              {/each}
            </div>
          {/if}
        </div>
        <button class="icon-btn" onclick={loadTemplates} disabled={library.templatesLoading} title="刷新">{@render refreshIcon(library.templatesLoading)}</button>
      </header>
      <div class="thread">
        <div class="tmpl-inner">
          {#if library.templates.length === 0}
            <div class="sched-empty">
              <div class="cal-mark">🧩</div>
              <b>还没有模板</b>
              <p>在一个 Cloudflare 站点项目对话里点「存模板」，做得好的站就能沉淀下来复用。</p>
            </div>
          {:else}
            <div class="tmpl-grid">
              {#each library.tmplShown as t (t.slug)}
                <div class="tmpl-card" id="tmpl-{t.slug}">
                  <div class="tmpl-thumb">
                    {#if library.tmplHtml[t.slug]}
                      <iframe class="tmpl-frame" srcdoc={library.tmplHtml[t.slug]} sandbox="allow-scripts" title={t.name} tabindex="-1" scrolling="no"></iframe>
                    {:else}
                      <span class="tmpl-letter">{t.name.slice(0, 1)}</span>
                    {/if}
                    <div class="tmpl-hover">
                      <button class="tmpl-act" aria-label="预览真实页面" onclick={() => previewTemplate(t)} data-tip="预览真实页面"><svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M1.6 8s2.4-4.4 6.4-4.4S14.4 8 14.4 8s-2.4 4.4-6.4 4.4S1.6 8 1.6 8Z" stroke="currentColor" stroke-width="1.2" /><circle cx="8" cy="8" r="1.9" stroke="currentColor" stroke-width="1.2" /></svg></button>
                      <button class="tmpl-act primary" aria-label="用它建新站" onclick={() => openUseTmpl(t)} data-tip="用它建新站"><svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M8 3.2v9.6M3.2 8h9.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg></button>
                      <!-- 随附模板不给删（后端也拒）：按钮摆着只会让人点了吃一个错 -->
                      {#if !t.builtin}
                        <button class="tmpl-act" aria-label="删除模板" onclick={() => delTemplate(t)} data-tip="删除模板"><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M3 4.6h10M6.4 4.6V3.3h3.2V4.6M4.6 4.6l.6 8.1h5.6l.6-8.1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
                      {/if}
                    </div>
                  </div>
                  <div class="tmpl-body">
                    <b>{t.name}</b>
                    <!-- 随附模板 created_at 恒 0，relTime 会渲染成 1970 的「1/1」——改显「内置」 -->
                    <!-- 「N 页」只在多页时挂：模板的单位是「一个站」，几页是**引用前就该知道**的事 -->
                    <div class="tmpl-sub"><span class="tmpl-desc">{t.desc || ''}</span><span class="tmpl-meta">{#if t.pages > 1}<span class="tmpl-pg">{t.pages} 页</span>{/if}{t.builtin ? '内置' : relTime(t.created_at)}</span></div>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      </div>

    {:else if app.view === 'remote'}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- 工作台头部只留一行（机器地址 + 状态）：标题「远程连接」是废话——侧栏已经写着你在哪台机器上。 -->
      <header class="thread-head slim" data-tauri-drag-region onmousedown={startDrag}>
        <div class="th-info"><small class="rhead-line">{app.activeConn?.ssh_user}@{app.activeConn?.ssh_host}:{app.activeConn?.ssh_port} · {workbench.termOn ? '已连接' : '未连接'}
          <button class="th-rfz" data-tip={workbench.termBusy ? '连接中…' : '重新连接'} aria-label="重新连接" disabled={workbench.termBusy} onclick={reconnectTerm}>{@render refreshIcon(workbench.termBusy)}</button>
          {#if workbench.stat}
            <span class="hstats">
              {@render loadStat('CPU', workbench.stat.cpu, workbench.stat.cpu === null ? '正在取样…（要两次采样才算得出占用）' : `CPU 占用 ${workbench.stat.cpu}%`)}
              {@render loadStat('内存', workbench.stat.memPct, `内存 ${workbench.stat.memText}`)}
              {@render loadStat('磁盘', workbench.stat.diskPct, `根分区 ${workbench.stat.diskText}`)}
            </span>
          {/if}</small></div>
        <div class="rhead-acts">
          <!-- 三个面板开关（VS Code 那套）：命令行 / 底部对话 / 右侧文件，各自可开可关、可同时开。 -->
          <button class="wb-tg" class:on={workbench.wb.term} aria-pressed={workbench.wb.term} data-tip="命令行窗口" aria-label="命令行窗口" onclick={toggleWbTerm}>
            {@render sshMark(15)}
          </button>
          <button class="wb-tg" class:on={workbench.wb.chat} aria-pressed={workbench.wb.chat} data-tip="底部对话框" aria-label="底部对话框" onclick={toggleWbChat}>
            <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="1.8" y="2.6" width="12.4" height="10.8" rx="2" stroke="currentColor" stroke-width="1.3" /><path d="M1.8 9.8h12.4" stroke="currentColor" stroke-width="1.3" /><rect x="1.8" y="9.8" width="12.4" height="3.6" fill="currentColor" opacity=".9" /></svg>
          </button>
          <button class="wb-tg" class:on={workbench.wb.files} aria-pressed={workbench.wb.files} data-tip="右侧文件" aria-label="右侧文件" onclick={toggleWbFiles}>
            <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="1.8" y="2.6" width="12.4" height="10.8" rx="2" stroke="currentColor" stroke-width="1.3" /><path d="M9.6 2.6v10.8" stroke="currentColor" stroke-width="1.3" /><path d="M9.6 2.6h2.6a2 2 0 0 1 2 2v6.8a2 2 0 0 1-2 2H9.6z" fill="currentColor" opacity=".9" /></svg>
          </button>
        </div>
      </header>
      <!-- 工作台：终端占主区，底部对话 / 右侧文件按需开，两条分隔线可拖。
           终端的尺寸变化交给 ResizeObserver 自动 fit（见 startTerm 上面那个 $effect）。 -->
      <div class="wb">
        <!-- 文件栏单开时把主区收起来（display:none）：终端就在里头，只藏不拆，xterm 和 SSH 会话都不掉。 -->
        <div class="wb-main" class:collapsed={workbench.wbFilesSolo}>
          <!-- 终端自己管滚动：别套 .thread（它的 overflow-y:auto 会和 xterm 打架）。
               ★ 关掉命令行只是 display:none **藏起来**，绝不拆 DOM —— 拆了 xterm 就没了，
               回来还得重连一次（连接是真的，掉一次要重登服务器）。 -->
          <div class="term-wrap" class:hid={!workbench.wb.term} class:with-chat={workbench.wb.chat}>
            <div class="term" bind:this={workbench.termEl}></div>
            {#if workbench.termBusy}
              <!-- 建连要走网络+认证，一两秒是常事，别让黑框干愣着。浮层而非写进终端：见 startTerm。 -->
              <div class="term-connecting"><span class="tc-spin"></span>正在连接 {app.activeConn?.ssh_user}@{app.activeConn?.ssh_host}…</div>
            {:else if workbench.termErr && missingSshCredential(workbench.termErr)}
              <button type="button" class="term-credential-repair" onclick={repairActiveSshCredential}>连接凭据缺失 · 补录密码</button>
            {/if}
          </div>
          {#if workbench.wb.chat}
            {#if workbench.wb.term}
              <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
              <div class="wb-split h" title="拖动调整高度" role="separator" aria-orientation="horizontal" onmousedown={(e) => startWbResize(e, 'h')}></div>
            {/if}
            <!-- 命令行关着时对话吃满整个主区（固定高度只在两者共存时才有意义） -->
            <div class="wb-chat" class:solo={!workbench.wb.term} style={workbench.wb.term ? `height:${workbench.wb.chatH}px` : ''}>
              {#if app.activeConv && app.activeConv.conn_id === app.activeConnId}
                {@render convPane(app.activeConv)}
              {:else}
                <!-- 还没有对话：这里就是起点（发出去就新建一条，走 startChat 那套）。
                     外层必须是 .composer-wrap —— 输入框的样式挂在 `.composer.big, .composer-wrap .composer` 上，
                     裸 .composer 一条都套不上（就是那次「样式缺失」）。 -->
                <div class="wb-start composer-wrap">
                  <div class="composer" class:fileover={composer.composerFileOver} role="group" aria-label="远程消息输入"
                    ondragover={onComposerDragOver} ondragleave={onComposerDragLeave} ondrop={onComposerDrop}>
                    {#if composer.attachments.length || composer.attaching}
                      <div class="attach-row">
                        {#each composer.attachments as a, i (a.path)}
                          <div class="attach-chip">
                            {#if a.preview}<img class="attach-img" src={a.preview} alt="" />{:else}<span class="attach-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M9 1.8H4.5A1.3 1.3 0 0 0 3.2 3.1v9.8a1.3 1.3 0 0 0 1.3 1.3h7a1.3 1.3 0 0 0 1.3-1.3V5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9 1.8v3.7h3.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg></span>{/if}
                            <span class="attach-name" title={a.name}>{a.name}</span>
                            <button class="attach-x" aria-label="移除附件" onclick={() => removeAttachment(i)}>×</button>
                          </div>
                        {/each}
                        {#if composer.attaching}<span class="attach-loading">上传中…</span>{/if}
                      </div>
                    {/if}
                    <textarea bind:this={workbench.wbStartEl} bind:value={launcher.lDraft} use:autogrow rows="2" placeholder="让 AI 在这台机器上做点什么…例如：看看装了什么，帮我装上 Docker"
                      oncompositionstart={() => (composer.composing = true)} oncompositionend={() => (composer.composing = false)}
                      onpaste={onComposerPaste}
                      onkeydown={(e) => onComposerKey(e, startChat)}></textarea>
                    <div class="composer-bar">
                      <div class="cb-left"><span class="ssh-target" title={app.activeConn?.ssh_os || '远端系统未知'}>{@render distroMark(distroOf(app.activeConn), 13)}{app.activeConn?.ssh_user}@{app.activeConn?.ssh_host}</span></div>
                      <div class="cb-right">
                        <Dropdown compact bind:value={launcher.lPerm} options={permOptsFor(launcher.lBrain, true)} tone={permTone(launcher.lPerm)} tip={permTipFor(launcher.lBrain, launcher.lPerm, true)} />
                        <ModelFx options={launcher.comboOpts} value={`${launcher.lBrain}::${launcher.lModel}`} effort={launcher.lEffort} fast={launcher.lFast} fastOk={app.brains?.claude_fast_ok ?? false} onpick={pickCombo} oneffort={persistLauncherEffort} onfast={setFast} />
                        <button class="send" onclick={startChat} disabled={(!launcher.lDraft.trim() && !composer.attachments.length) || composer.attaching || !brainUsable(launcher.lBrain)} title="发送（Enter）">↑</button>
                      </div>
                    </div>
                  </div>
                  <!-- 档位警告见权限下拉的 tip（permTipFor）；这里只留「厂商没就绪」这种当下堵路的事。 -->
                  {#if !brainUsable(launcher.lBrain)}
                    <p class="hint warn-text">{brainSetupHint(launcher.lBrain)}</p>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        </div>
        {#if workbench.wb.files}
          <!-- 文件栏单开时没有可拖的对象（主区已收起），分隔线也就不出 -->
          {#if !workbench.wbFilesSolo}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <div class="wb-split v" title="拖动调整宽度" role="separator" aria-orientation="vertical" onmousedown={(e) => startWbResize(e, 'v')}></div>
          {/if}
          <aside class="files-wrap" class:solo={workbench.wbFilesSolo} class:fileover={sftp.sftpFileOver}
            style="{workbench.wbFilesSolo ? '' : `width:${workbench.wb.filesW}px;`} --fw-perm:{sftp.colW.perm}px; --fw-size:{sftp.colW.size}px; --fw-date:{sftp.colW.date}px"
            ondragenter={onSftpDragOver} ondragover={onSftpDragOver} ondragleave={onSftpDragLeave} ondrop={onSftpDrop}>
          {#if sftp.sftpFileOver}
            <div class="files-drop-overlay"><b>松开上传</b><small title={sftp.sftpDropTarget?.remoteDir}>到 {sftp.sftpDropTarget?.remoteDir || sftp.sftpPath || '当前目录'}</small></div>
          {/if}
          <div class="files-bar">
            <button class="fbtn" aria-label="上一级" data-tip="上一级" onclick={sftpUp} disabled={sftp.sftpBusy || !sftp.sftpPath || sftp.sftpPath === '/'}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M8 12.5v-9M4.2 7.3 8 3.5l3.8 3.8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
            {#if sftp.sftpPathEdit}
              <input class="tin fpath" bind:this={sftp.sftpPathEl} bind:value={sftp.sftpPathDraft} spellcheck="false" autocapitalize="off" autocorrect="off" placeholder="/"
                onblur={() => (sftp.sftpPathEdit = false)}
                onkeydown={(e) => { if (e.key === 'Enter') commitPath(); else if (e.key === 'Escape') sftp.sftpPathEdit = false; }} />
            {:else}
              <!-- 面包屑：点哪一段跳哪一层。尾巴上那块空白本身是个按钮 ——
                   点它＝切成输入框直接敲路径（资源管理器那套），键盘也 Tab 得到。 -->
              <div class="fcrumbs" bind:this={sftp.crumbEl}>
                {#each sftp.sftpCrumbs as c, i (c.path)}
                  {#if i > 0}<span class="fcrumb-sep">›</span>{/if}
                  <button class="fcrumb" class:cur={c.path === sftp.sftpPath} disabled={sftp.sftpBusy} onclick={() => sftpGo(c.path)}>{c.name}</button>
                {/each}
                <button class="fcrumb-blank" aria-label="直接输入路径" data-tip="点这里可直接输入路径" onclick={startPathEdit}></button>
              </div>
            {/if}
            <button class="fbtn" aria-label="刷新" data-tip="刷新" onclick={sftpRefresh} disabled={sftp.sftpBusy}>{@render refreshIcon(sftp.sftpBusy)}</button>
            <button class="fbtn" aria-label="新建文件夹" data-tip="新建文件夹" onclick={() => openMkdir()} disabled={sftp.sftpBusy}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M1.8 4.2A1.4 1.4 0 0 1 3.2 2.8h3l1.4 1.6h5.2a1.4 1.4 0 0 1 1.4 1.4v6a1.4 1.4 0 0 1-1.4 1.4H3.2a1.4 1.4 0 0 1-1.4-1.4v-7.6z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M8 7.4v3.4M6.3 9.1h3.4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg></button>
            <SftpTransfers records={sftp.sftpTransfers} busy={sftp.sftpBusy || !!sftp.sftpXfer || sftp.sftpNativeDragging}
              canDownload={!sftp.fileRename && !!sftp.sftpSelectedRow && !isSftpDeleting(sftp.sftpSelectedRow.path)} selectedName={sftp.sftpSelectedRow?.name}
              cancellableIds={sftp.sftpCancellableIds} onupload={() => sftpUpload()} ondownload={sftpDownloadSelected}
              oncancel={cancelSftpTransfer} onclear={clearSftpTransfers} onreveal={revealSftpTransfer} />
          </div>
          <!-- 出错就地给个重试：首次加载失败时 sftpPath 还是空的，刷新键（要有当前目录）帮不上忙 -->
          {#if sftp.sftpErr}<button type="button" class="err-note files-err" onclick={retryOrRepairSftp} title={missingSshCredential(sftp.sftpErr) ? '打开连接设置并补录密码' : '点击重试'}>{sftp.sftpErr} · {missingSshCredential(sftp.sftpErr) ? '补录密码' : '点此重试'}</button>{/if}
          <div class="fhead">
            <button class="fh fh-name" onclick={() => setSort('name')}>名称{#if sftp.sftpSort.key === 'name'}<span class="fh-ar" class:desc={!sftp.sftpSort.asc}>^</span>{/if}</button>
            <span class="fh fh-perm">{@render colGrip('perm')}权限</span>
            <button class="fh fh-size" onclick={() => setSort('size')}>{@render colGrip('size')}大小{#if sftp.sftpSort.key === 'size'}<span class="fh-ar" class:desc={!sftp.sftpSort.asc}>^</span>{/if}</button>
            <button class="fh fh-date" onclick={() => setSort('mtime')}>{@render colGrip('date')}日期{#if sftp.sftpSort.key === 'mtime'}<span class="fh-ar" class:desc={!sftp.sftpSort.asc}>^</span>{/if}</button>
          </div>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="files-list" oncontextmenu={(e) => openFctx(e, null)}>
            {#if sftp.sftpBusy && !sftp.sftpRows.length}
              <div class="files-empty">读取中…</div>
            {:else if !sftp.sftpRows.length}
              <div class="files-empty">{sftp.sftpErr ? '' : '空目录'}</div>
            {:else}
              {#each sftp.sftpRows as r (r.path)}
                {@const deleting = isSftpDeleting(r.path)}
                {@const deleteTarget = isSftpDeleteTarget(r.path)}
                {@const renaming = sftp.fileRename?.connId === sftp.sftpConnId && sftp.fileRename?.path === r.path}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class="frow" class:on={sftp.sftpSel === r.path} class:deleting class:drop-target={sftp.sftpFileOver && sftp.sftpDropTarget?.remoteDir === r.path && r.dir && !r.link} data-sftp-drop-path={r.path} role="row" tabindex="-1" aria-busy={deleting}
                  draggable={isMac && !deleting && !renaming && !sftp.sftpBusy && !sftp.sftpXfer && !sftp.sftpNativeDragging} ondragstart={(e) => onSftpDragStart(e, r)}
                  onclick={() => { if (!deleting) sftp.sftpSel = r.path; }}
                  ondblclick={() => { if (!deleting && !renaming) sftpOpenRow(r); }}
                  oncontextmenu={(e) => { e.stopPropagation(); if (!deleting) openFctx(e, r); }}
                  onkeydown={(e) => { if (!deleting && !renaming && e.key === 'Enter') sftpOpenRow(r); }}>
                  <span class="fname" style="padding-left:{r.depth * 15}px">
                    {#if r.dir && !r.link}
                      <button class="fchev" class:open={sftp.sftpOpenDirs.has(r.path)} aria-label="展开" disabled={deleting} onclick={(e) => { e.stopPropagation(); if (!deleting) toggleDir(r); }}>
                        {#if sftp.sftpLoading[r.path]}<span class="fspin"></span>{:else}<svg width="9" height="9" viewBox="0 0 12 12" fill="none"><path d="M4.5 2.5 8 6l-3.5 3.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
                      </button>
                    {:else}
                      <span class="fchev-sp"></span>
                    {/if}
                    {#if r.link}
                      <!-- 软链：sftp 的 readdir 是 lstat 语义，指向目录的软链也归这里（点开时才知道） -->
                      <svg class="fic link" width="14" height="14" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" fill="currentColor" opacity=".14" /><path d="M5.6 10.4 10.4 5.6M6.6 5.6h3.8v3.8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                    {:else if r.dir}
                      <svg class="fic dir" width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M1.8 4.2A1.4 1.4 0 0 1 3.2 2.8h3l1.4 1.6h5.2a1.4 1.4 0 0 1 1.4 1.4v6a1.4 1.4 0 0 1-1.4 1.4H3.2a1.4 1.4 0 0 1-1.4-1.4v-7.6z" fill="currentColor" opacity=".16" /><path d="M1.8 4.2A1.4 1.4 0 0 1 3.2 2.8h3l1.4 1.6h5.2a1.4 1.4 0 0 1 1.4 1.4v6a1.4 1.4 0 0 1-1.4 1.4H3.2a1.4 1.4 0 0 1-1.4-1.4v-7.6z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg>
                    {:else}
                      <svg class="fic" width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M4 1.8h5.2L12.8 5.4v8.8H4V1.8z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9.2 1.8v3.6h3.6" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg>
                    {/if}
                    {#if renaming && sftp.fileRename}
                      <!-- svelte-ignore a11y_no_static_element_interactions -->
                      <span class="file-rename" ondblclick={(e) => e.stopPropagation()} oncontextmenu={(e) => e.stopPropagation()}>
                        <input use:focusFileRename bind:value={sftp.fileRename.value} aria-label={`重命名 ${r.name}`}
                          aria-invalid={!!sftp.fileRename.err} readonly={sftp.fileRename.busy} spellcheck="false" autocapitalize="off" autocorrect="off"
                          oninput={() => { if (sftp.fileRename) sftp.fileRename.err = ''; }}
                          onblur={() => { if (!sftp.fileRename?.err) void saveFileRename(); }}
                          onkeydown={(e) => {
                            e.stopPropagation();
                            if (e.isComposing) return;
                            if (e.key === 'Enter') { e.preventDefault(); void saveFileRename(); }
                            if (e.key === 'Escape' && !sftp.fileRename?.busy) { e.preventDefault(); sftp.fileRename = null; }
                          }} />
                        {#if sftp.fileRename.err}<small role="alert">{sftp.fileRename.err}</small>{/if}
                      </span>
                    {:else}
                      <button type="button" class="fname-t" title="双击名称重命名" disabled={deleting}
                        ondblclick={(e) => { e.preventDefault(); e.stopPropagation(); openRename(r); }}>{r.name}</button>
                    {/if}
                    {#if deleteTarget}<span class="fdelete-state"><span class="fdelete-spin" aria-hidden="true"></span>删除中…</span>{/if}
                  </span>
                  <span class="fperm">{r.perms}</span>
                  <span class="fsize">{r.dir && !r.link ? '·' : fmtSize(r.size)}</span>
                  <span class="fdate">{fmtStamp(r.mtime)}</span>
                </div>
              {/each}
            {/if}
          </div>
        </aside>
        {/if}
      </div>

    {:else if app.view === 'prompts'}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <header class="thread-head" data-tauri-drag-region onmousedown={startDrag}>
        <div class="th-info"><b class="th-title">{@render promptIcon(16)}<span>提示词库</span></b><small>常用建站需求，点「用它建站」填进对话框就能开始；也能存自己的。</small></div>
        <button class="btn soft bare" onclick={openNewPrompt}>{@render plusIcon()}新增提示词</button>
      </header>
      <div class="thread">
        <div class="sched-inner">
          <div class="prompt-grid">
            {#each library.allPrompts as p (p.id)}
              <div class="prompt-card">
                <div class="prompt-top">
                  <b class="prompt-title">{p.title}</b>
                  {#if p.builtin}<span class="prompt-tag">预设</span>{/if}
                </div>
                <p class="prompt-body">{p.body}</p>
                <div class="prompt-acts">
                  <button class="prompt-act" aria-label="用它建站" data-tip="用它建站" onclick={() => usePrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M8 3.2v9.6M3.2 8h9.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg></button>
                  <button class="prompt-act" aria-label="复制提示词" data-tip="复制" onclick={() => copyPrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><rect x="5.5" y="5.5" width="7.6" height="8" rx="1.4" stroke="currentColor" stroke-width="1.2" /><path d="M10.6 5.5V4.2A1.4 1.4 0 0 0 9.2 2.8H4A1.4 1.4 0 0 0 2.6 4.2v5.2A1.4 1.4 0 0 0 4 10.8h1.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg></button>
                  {#if !p.builtin}
                    <button class="prompt-act" aria-label="编辑提示词" data-tip="编辑" onclick={() => openEditPrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M10.8 2.9l2.3 2.3M11.4 2.3l2.3 2.3-8 8-3 .7.7-3 8-8z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg></button>
                    <button class="prompt-act" aria-label="删除提示词" data-tip="删除" onclick={() => deletePrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M3 4.6h10M6.4 4.6V3.3h3.2V4.6M4.6 4.6l.6 8.1h5.6l.6-8.1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        </div>
      </div>

    {:else}
      {@render conversationHeader(app.activeConv)}
      {@render convPane(app.activeConv)}
    {/if}
  </section>
</main>

<!-- 会话搜索（Claude Code 风格：顶部圆角搜索框 + 结果列表） -->
{#if search.searchOpen}
  <div class="mask" role="presentation" onclick={() => (search.searchOpen = false)}></div>
  <div class="search-box" role="dialog" aria-modal="true" aria-label="搜索会话">
      <div class="search-head">
        <svg class="si-ic" width="16" height="16" viewBox="0 0 18 18" fill="none">
          <circle cx="8" cy="8" r="5" stroke="currentColor" stroke-width="1.4" />
          <path d="M11.7 11.7L15 15" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
        <input bind:this={search.searchInput} bind:value={search.searchQ} placeholder="搜索会话、连接（名称/IP）、模板…" spellcheck="false" autocapitalize="off" autocorrect="off" />
        <kbd>esc</kbd>
      </div>
      <div class="search-list">
        {#if search.searchFlat.length === 0}
          <p class="search-empty">没有匹配结果</p>
        {:else}
          {#each search.searchSections as sec, si (sec.title)}
            {@const base = search.searchSections.slice(0, si).reduce((n, s) => n + s.entries.length, 0)}
            <div class="search-sec">{sec.title}</div>
            {#each sec.entries as it, i (`${sec.title}-${i}`)}
              <button class="search-item {base + i === search.searchIdx ? 'on' : ''}" onclick={() => pickEntry(it)} onmouseenter={() => (search.searchIdx = base + i)}>
                {#if it.kind === 'view'}
                  {#if it.view === 'templates'}{@render templateIcon(15)}{:else}{@render promptIcon(15)}{/if}
                  <span class="si-main"><b>{it.label}</b><small>切换到{it.label}视图</small></span>
                {:else if it.kind === 'conv'}
                  {#if it.c.task_type === 'workspace'}
                    {@render workspaceMark(15)}
                  {:else if it.c.task_type === 'remote'}
                    {@render sshMark(15)}
                  {:else}
                    <SiteFav src="" label={it.c.site_slug} size={15} />
                  {/if}
                  <span class="si-main"><b>{it.c.title || '未命名会话'}</b><small>{#if it.c.task_type === 'workspace'}{it.c.workspace_dir ? workspaceFolderName(it.c.workspace_dir) : '纯对话'}{:else}{it.c.site_name || it.c.site_slug}{/if}</small></span>
                  {@render brainTag(it.c.brain, brainLabel(it.c.brain))}
                {:else if it.kind === 'template'}
                  <!-- 与左栏「模板库」同一个图标：搜到的东西该看得出是同一样东西 -->
                  <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="2.5" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /></svg>
                  <span class="si-main"><b>{it.t.name}</b><small>{catOf(it.t)}{it.t.desc ? ` · ${it.t.desc}` : ''}</small></span>
                  {#if it.t.builtin}<span class="si-tag">内置</span>{/if}
                {:else}
                  <!-- 与连接切换器同一个标 -->
                  {#if it.c.kind === 'ssh'}{@render sshMark(15)}{:else}{@render cfMark(15)}{/if}
                  <span class="si-main"><b>{it.c.name}</b><small>{#if it.c.kind === 'ssh'}{it.c.ssh_user}@{it.c.ssh_host}{it.c.ssh_port && it.c.ssh_port !== 22 ? ':' + it.c.ssh_port : ''}{it.c.ssh_os ? ` · ${it.c.ssh_os}` : ''}{:else}Cloudflare · {it.c.remark?.trim() || it.c.key_prefix}{/if}</small></span>
                  {#if app.activeConnId === it.c.id}<span class="si-tag">当前</span>{/if}
                {/if}
              </button>
            {/each}
          {/each}
        {/if}
      </div>
  </div>
{/if}

<!-- 消息气泡片段 -->
{#snippet bubble(m: Message, isLast: boolean, context:Conversation|null=app.activeConv)}
  {@const bubbleConv=context}
  {@const bubbleId=context?.id??''}
  {@const bubbleBusy=!!app.running[bubbleId]}
  {#if m.role === 'user'}
    {@const ua = splitAttachments(m.text)}
    <div class="msg user"><div class="ubody">
      {#if ua.body.trim()}<span class="ub-text">{@render richText(ua.body)}</span>{/if}
      {#if ua.atts.length}
        <div class="ub-atts" class:only={!ua.body.trim()}>
          {#each ua.atts as p (p)}
            {#if isImgPath(p) && media.thumbs[thumbKey(p,context)] !== ''}
              {#if media.thumbs[thumbKey(p,context)]}
                <button class="ub-att-img" data-tip={p.split('/').pop()} onclick={() => (ui.lightbox = media.thumbs[thumbKey(p,context)])}><img src={media.thumbs[thumbKey(p,context)]} alt={p.split('/').pop()} onerror={() => (media.thumbs = { ...media.thumbs, [thumbKey(p,context)]: '' })} /></button>
              {:else}
                <!-- 数据未到：同尺寸占位，图片就位零布局跳动 -->
                <span class="ub-att-img ph" data-tip={p.split('/').pop()}></span>
              {/if}
            {:else}
              <span class="ub-att">
                <span class="ub-att-ic">
                  {#if isImgPath(p)}<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="2.2" y="2.8" width="11.6" height="10.4" rx="1.6" stroke="currentColor" stroke-width="1.2" /><circle cx="5.7" cy="6.2" r="1.1" fill="currentColor" /><path d="M3 12.4l3.3-3.1 2.1 1.9 2.4-2.6 2.2 2.3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  {:else}<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M9 1.8H4.5A1.3 1.3 0 0 0 3.2 3.1v9.8a1.3 1.3 0 0 0 1.3 1.3h7a1.3 1.3 0 0 0 1.3-1.3V5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9 1.8v3.7h3.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg>{/if}
                </span>
                <span class="ub-att-n" title={p}>{p.split('/').pop()}</span>
              </span>
            {/if}
          {/each}
        </div>
      {/if}
    </div></div>
  {:else}
    <div class="msg assistant">
      <div class="body">
        {#if m.tools.length}{@render cmds(m.tools)}{/if}
        {#if m.error && m.limit_reset != null}
          <div class="limit-card">
            <div class="limit-head">⏳ {brainTitle(bubbleConv?.brain)} 额度已用完</div>
            {#if (m.limit_reset ?? 0) > 0}
              <div class="limit-sub">预计 {fmtClock(m.limit_reset ?? 0)} 恢复{#if (m.limit_reset ?? 0) * 1000 > turns.nowTick} · 还有 {fmtRemain((m.limit_reset ?? 0) * 1000 - turns.nowTick)}{/if}</div>
            {:else}
              <div class="limit-sub">订阅套餐的时间窗限额已触顶，稍后会自动恢复；也可以稍等片刻手动重试。</div>
            {/if}
            {#if isLast && !bubbleBusy}
              <div class="limit-actions">
                {#if turns.limitAuto[bubbleId]}
                  <button class="retry-btn is-armed" onclick={() => disarmLimitAuto(bubbleId)}>✓ 已排队，到点自动续跑 · 点击取消</button>
                {:else if (m.limit_reset ?? 0) * 1000 > turns.nowTick}
                  <button class="retry-btn" onclick={() => armLimitAuto(bubbleId, m.limit_reset ?? 0)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="5.6" stroke="currentColor" stroke-width="1.4" /><path d="M8 5.2V8l2 1.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>到点自动续跑</button>
                {/if}
                {#if bubbleConv?.session_ref}<button class="retry-btn" onclick={() => retry(bubbleId, true)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M13 8a5 5 0 1 1-1.5-3.6M13 2v3h-3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>立即重试</button>{/if}
              </div>
              <div class="limit-hint">也可以在右下角把模型切到另一家继续。</div>
            {/if}
          </div>
        {:else if m.error}
          <div class="text is-err">{@render richText(m.text)}{#if isLast && !bubbleBusy}{#if bubbleConv?.session_ref && !turns.retryExhausted[bubbleId]}<button class="retry-btn" onclick={() => retry(bubbleId, true)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M13 8a5 5 0 1 1-1.5-3.6M13 2v3h-3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>重试</button>{/if}<button class="retry-btn" title="换一个全新会话原地续跑（自动带上历史摘要）——用于会话状态损坏、重试无效时" onclick={() => rebuildSession(bubbleId)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M2.6 8a5.4 5.4 0 0 1 9.3-3.7M13.4 8a5.4 5.4 0 0 1-9.3 3.7" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><path d="M11.6 1.6v2.9h2.9M4.4 14.4v-2.9H1.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>重建继续</button>{/if}</div>
        {:else}
          {@const ga = splitGenImages(m.text)}
          <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
          <div class="text md" onclick={(e)=>mdClick(e,context)} onauxclick={(e)=>mdClick(e,context)}>{@html mdRender(ga.body)}</div>
          {#if ga.atts.length}
            <!-- codex 生图产物：缩略图（点击放大）；读不到的退回文件名卡（点击访达定位） -->
            <div class="ub-atts gen-imgs">
              {#each ga.atts as p (p)}
                {#if isImgPath(p) && media.thumbs[thumbKey(p,context)] !== ''}
                  {#if media.thumbs[thumbKey(p,context)]}
                    <button class="ub-att-img" data-tip={p.split(/[\\/]/).pop()} onclick={() => (ui.lightbox = media.thumbs[thumbKey(p,context)])}><img src={media.thumbs[thumbKey(p,context)]} alt={p.split(/[\\/]/).pop()} onerror={() => (media.thumbs = { ...media.thumbs, [thumbKey(p,context)]: '' })} /></button>
                  {:else}
                    <span class="ub-att-img ph" data-tip={p.split(/[\\/]/).pop()}></span>
                  {/if}
                {:else}
                  <button class="ub-att as-btn" title={p} onclick={() => void revealWorkdir(p,context)}>
                    <span class="ub-att-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="2.2" y="2.8" width="11.6" height="10.4" rx="1.6" stroke="currentColor" stroke-width="1.2" /><circle cx="5.7" cy="6.2" r="1.1" fill="currentColor" /><path d="M3 12.4l3.3-3.1 2.1 1.9 2.4-2.6 2.2 2.3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg></span>
                    <span class="ub-att-n">{p.split(/[\\/]/).pop()}</span>
                  </button>
                {/if}
              {/each}
            </div>
          {/if}
        {/if}
      </div>
    </div>
  {/if}
{/snippet}

{#snippet richText(text: string)}{#each text.split('\n') as line, i (i)}{#if i > 0}<br />{/if}{#each segs(line) as s, si (si)}{#if s.link}<button class="inlink" onclick={() => openUrl(s.t)}>{s.t}</button>{:else}{s.t}{/if}{/each}{/each}{/snippet}

{#snippet toolChip(t: ToolCall & { count?: number })}
  <div class="tool"><span class="tcode">{t.label}</span><span class="tdetail">{t.detail}</span>{#if (t.count ?? 0) > 1}<span class="tool-count">×{t.count}</span>{/if}</div>
{/snippet}

{#snippet brainTag(brain: string, label: string)}<span class="btag"><BrainIcon brain={modelIcon(brain)} size={12} />{label}</span>{/snippet}

<!-- 只有「全部」「自建」带图标：这两个是**看什么范围**，后面那排是**按页型筛**。
     给每个都配图标反而把这层区别抹平了。 -->
{#snippet catIcon(c: string)}
  {#if c === '全部'}
    <svg class="tc-ic" width="11" height="11" viewBox="0 0 16 16" aria-hidden="true"><rect x="1.5" y="1.5" width="5.6" height="5.6" rx="1.3" fill="currentColor" /><rect x="8.9" y="1.5" width="5.6" height="5.6" rx="1.3" fill="currentColor" /><rect x="1.5" y="8.9" width="5.6" height="5.6" rx="1.3" fill="currentColor" /><rect x="8.9" y="8.9" width="5.6" height="5.6" rx="1.3" fill="currentColor" /></svg>
  {:else if c === CAT_MINE}
    <svg class="tc-ic" width="11" height="11" viewBox="0 0 16 16" fill="none" aria-hidden="true"><circle cx="8" cy="5" r="2.7" stroke="currentColor" stroke-width="1.5" /><path d="M2.9 14c0-2.7 2.3-4.4 5.1-4.4s5.1 1.7 5.1 4.4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
  {/if}
{/snippet}
{#snippet refreshIcon(spinning: boolean)}
  <svg class="rfz {spinning ? 'spin' : ''}" width="15" height="15" viewBox="0 0 24 24" fill="none">
    <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" />
    <path d="M21 3v5h-5" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{/snippet}
{#snippet backChevronIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="m9.8 3.5-4.5 4.5 4.5 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}

{#snippet plusIcon()}<svg class="plus-ic" width="13" height="13" viewBox="0 0 14 14" fill="none"><path d="M7 2.4v9.2M2.4 7h9.2" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg>{/snippet}
{#snippet templateIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="2.5" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /></svg>{/snippet}
{#snippet promptIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3 3h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H6.5L4 13.2V11H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /><path d="M5.2 6.2h5.6M5.2 8.4h3.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /></svg>{/snippet}

{#snippet gearIcon()}<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M2 5.3h3.5M9.3 5.3H14M2 10.7h5.1M10.9 10.7H14" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /><circle cx="7.4" cy="5.3" r="1.6" stroke="currentColor" stroke-width="1.3" /><circle cx="9" cy="10.7" r="1.6" stroke="currentColor" stroke-width="1.3" /></svg>{/snippet}
{#snippet settingsIcon()}<SettingsIcon />{/snippet}
{#snippet updateIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 2.2v7.2M5.3 6.8 8 9.5l2.7-2.7M3 11.2v1.3c0 .7.6 1.3 1.3 1.3h7.4c.7 0 1.3-.6 1.3-1.3v-1.3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}
{#snippet externalLinkIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M6 3.5h6.5V10M12.2 3.8 3.8 12.2" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}
{#snippet trashIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3.4 4.5h9.2M6 2.5h4l.6 2M5 6.5v6M8 6.5v6M11 6.5v6M4.2 4.5l.6 9h6.4l.6-9" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}
{#snippet cfMark(size: number)}<svg class="cf-mark" width={size} height={size} viewBox="0 0 128 128"><path fill="#fff" d="m115.679 69.288l-15.591-8.94l-2.689-1.163l-63.781.436v32.381h82.061z" /><path fill="#f38020" d="M87.295 89.022c.763-2.617.472-5.015-.8-6.796c-1.163-1.635-3.125-2.58-5.488-2.689l-44.737-.581c-.291 0-.545-.145-.691-.363s-.182-.509-.109-.8c.145-.436.581-.763 1.054-.8l45.137-.581c5.342-.254 11.157-4.579 13.192-9.885l2.58-6.723c.109-.291.145-.581.073-.872c-2.906-13.158-14.644-22.97-28.672-22.97c-12.938 0-23.913 8.359-27.838 19.952a13.35 13.35 0 0 0-9.267-2.58c-6.215.618-11.193 5.597-11.811 11.811c-.145 1.599-.036 3.162.327 4.615C10.104 70.051 2 78.337 2 88.549c0 .909.073 1.817.182 2.726a.895.895 0 0 0 .872.763h82.57c.472 0 .909-.327 1.054-.8z" /><path fill="#faae40" d="M101.542 60.275c-.4 0-.836 0-1.236.036c-.291 0-.545.218-.654.509l-1.744 6.069c-.763 2.617-.472 5.015.8 6.796c1.163 1.635 3.125 2.58 5.488 2.689l9.522.581c.291 0 .545.145.691.363s.182.545.109.8c-.145.436-.581.763-1.054.8l-9.924.582c-5.379.254-11.157 4.579-13.192 9.885l-.727 1.853c-.145.363.109.727.509.727h34.089c.4 0 .763-.254.872-.654c.581-2.108.909-4.325.909-6.614c0-13.447-10.975-24.422-24.458-24.422" /></svg>{/snippet}
<!-- 单会话、多个窗格和远程工作台共用原始对话组件。
     运行状态按 context.id 查找；输入、滚动和操作由所属窗格绑定，不能读取其他窗格的焦点状态。 -->
{#snippet conversationActivity(context:Conversation)}
  {@const activeConv=context}
  {@const viewBusy=!!app.running[context.id]}
  {@const liveView=app.lives[context.id]}
  {@const activePermits=permits.pendingPermits.filter(p=>p.conv===context.id)}
      {#if viewBusy && liveView}
        {@const progress = executionProgress(liveView, thread.nowMs)}
        <div class="msg assistant">
          <div class="body">
            {#if liveView.tools.length}{@render cmds(liveView.tools)}{/if}
            {#if liveView.compactedAt}
              <div class="context-compact-note">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3 5.2h10M5 2.8l-2 2.4 2 2.4M13 10.8H3M11 8.4l2 2.4-2 2.4" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" /></svg>
                <span>Claude 已自动整理上下文</span>
                <small>同一会话继续，聊天记录保持不变</small>
              </div>
            {/if}
            <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
            {#if liveView.renderedText}<div class="text md live-text" onclick={(e)=>mdClick(e,activeConv)} onauxclick={(e)=>mdClick(e,activeConv)}>{@html liveView.renderedText}</div>{/if}
            {#if progress.thinking}
              <div class="codex-thinking" role="status" aria-live="polite">
                <span>正在思考</span>
              </div>
            {:else if progress.hasActivity}
              <div class="execution-progress-stack">
                <details
                  class="execution-progress execution-progress-details is-running"
                  class:quiet={progress.quiet}
                  class:has-failure={progress.failed}
                  class:recovered-failure={progress.continuedAfterFailure}
                >
                  <summary>
                    <span class="execution-progress-mark" aria-hidden="true">
                      <span></span>
                    </span>
                    <span class="execution-progress-copy">
                      <b>{progress.title}</b>
                      <small>{progress.meta}</small>
                    </span>
                    <span class="execution-failure-summary">
                      {#if progress.failedActivities.length && !progress.continuedAfterFailure}
                        <b>{progress.failedActivities.length} 项异常</b>
                      {/if}
                      <small>查看详情</small>
                      <svg width="11" height="11" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5 6 7.5l3-3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                    </span>
                  </summary>
                  <div class="execution-activity-list">
                    {#if progress.trueProgress}
                      <div class="execution-real-progress">
                        <div>
                          <b>{progress.trueProgress.current} / {progress.trueProgress.total}</b>
                          <span>{activityLabel(progress.trueProgress)}</span>
                        </div>
                        <span><i style={`width:${Math.round(((progress.trueProgress.current ?? 0) / (progress.trueProgress.total ?? 1)) * 100)}%`}></i></span>
                      </div>
                    {/if}
                    {#each progress.activities as activity (activity.id)}
                      {@const detail = activityDetail(activity)}
                      <div class="execution-activity-item" class:is-failed={activity.status === 'failed'}>
                        <span class={`execution-activity-state ${activity.status}`}></span>
                        <div>
                          <b>{activityLabel(activity)}</b>
                          {#if activity.status === 'failed'}
                            <p>{activityFailureReason(activity)}</p>
                          {:else if detail}
                            <p>{detail}</p>
                          {/if}
                        </div>
                        <small>{activityStatusLabel(activity)}</small>
                      </div>
                    {/each}
                    {#if progress.failedActivities.length}
                      <p class="execution-failure-impact">
                        {progress.continuedAfterFailure
                          ? '后续步骤已继续执行；最终答复会说明该异常是否影响结果。'
                          : '本轮仍在继续；最终答复会说明该异常是否影响结果。'}
                      </p>
                    {/if}
                  </div>
                </details>
              </div>
            {/if}
            {#if liveView.error}<div class="err-note">{liveView.error}</div>
            {:else if !progress.thinking}
              <div class="working" aria-label="思考中">
                <svg class="wl" viewBox="0 0 64 64" width="17" height="17" aria-hidden="true">
                  <path class="wl-trace" d="M44 24a14 14 0 1 0 0 16" fill="none" stroke="#a03c2b" stroke-width="7" stroke-linecap="round" pathLength="100" stroke-dasharray="100" />
                  <circle class="wl-dot" cx="45.5" cy="42" r="4.6" fill="#a03c2b" />
                </svg>
                <span class="working-t">{elapsedLabel(thread.nowMs - liveView.startedAt)}</span>
              </div>
            {/if}
          </div>
        </div>
      {/if}
      {#each activePermits as p (p.id)}
        <div class="msg assistant">
          <div class="permit-card" class:danger={p.dangerous}>
            <div class="permit-head"><span class="permit-dot"></span>需要你确认这个操作{#if p.dangerous}<span class="permit-tag">危险 · 对线上生效</span>{/if}</div>
            <div class="permit-desc">{permitDesc(p)}</div>
            <div class="permit-meta">{p.tool === 'SSH' ? '远程命令' : p.tool}{#if p.tool === 'SSH' && p.arg} · {p.arg}{/if}{#if p.mode === 'ask'} · 询问档{/if}</div>
            {#if p.cmd}
              {#if p.tool === 'SSH'}
                <!-- 远程命令不给收起：它要在用户的真机上跑，不看着它就没法做决定。 -->
                <code class="permit-cmd">{p.cmd}</code>
              {:else if permits.permitCmdOpen.has(p.id)}
                <button class="permit-more" onclick={() => togglePermitCmd(p.id)}>收起命令 ▴</button>
                <code class="permit-cmd">{p.cmd}</code>
              {:else}
                <button class="permit-more" onclick={() => togglePermitCmd(p.id)}>查看具体命令 ▸</button>
              {/if}
            {/if}
            <div class="permit-act">
              <button class="btn sm" onclick={() => respondPermit(p.id, false)}>拒绝</button>
              <button class="btn sm primary" onclick={() => respondPermit(p.id, true)}>批准</button>
            </div>
          </div>
        </div>
      {/each}

{/snippet}

{#snippet conversationHeader(context:Conversation|null, paneId='')}
  {@const meta=conversationPresentation(context)}
  {@const activeConv=context}
  {@const activeSiteUrl=meta.url}
  {@const headerDomain=activeSiteUrl?hostOf(activeSiteUrl):''}
  {@const headerSiteIcon=headerDomain?faviconGuess(`https://${headerDomain}`):''}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <header class="thread-head conversation-thread-head" class:multi={leaves(panes.conversationLayout).length>1} onmousedown={(e)=>{if(!paneId)startDrag(e);}}>
        <div class="th-info" role="button" tabindex="0" aria-label={`拖动对话 ${context?.title} 以分屏`} onpointerdown={(e)=>{if(context)startConversationPointerDrag(e,context.id);}} onkeydown={(e)=>{if(e.key==='Escape')endConversationDrag();}}>
          <b class="conversation-head-title" data-tip={activeConv?.title}>
            <span class="conversation-head-model" data-tip={brainLabel(activeConv?.brain??'claude')}><BrainIcon brain={modelIcon(activeConv?.brain??'claude')} size={14} /></span>
            <span>{activeConv?.title}</span>
          </b>
        </div>
        {#if activeSiteUrl}
          <button class="th-open conversation-site-link" onclick={() => openUrl(activeSiteUrl)} title="打开 {activeSiteUrl}"><SiteFav src={headerSiteIcon} label={headerDomain} size={14} /><span>{headerDomain}</span><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M6 3.5h6.5V10M12.2 3.8 3.8 12.2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
        {:else if headerDomain}
          <span class="th-open conversation-site-link static" title={headerDomain}><SiteFav src={headerSiteIcon} label={headerDomain} size={14} /><span>{headerDomain}</span></span>
        {/if}
        {#if paneId && leaves(panes.conversationLayout).length>1}<button class="x pane-close" title="关闭窗格，不停止任务" aria-label="关闭窗格" onclick={()=>closeSplit(paneId)}>×</button>{/if}
      </header>

{/snippet}

{#snippet splitContent(id:string, paneId:string)}
  {@const c=splitConversation(id)}
  {#if c && panes.splitDrafts[id]}
    {#key c.id}
      {@render conversationHeader(c,paneId)}
      {@render convPane(c,paneId)}
    {/key}
  {/if}
{/snippet}

{#snippet convPane(context:Conversation|null, paneId='')}
  {@const meta=conversationPresentation(context)}
  {@const activeConv=context}
  {@const activeConvConn=meta.conn}
  {@const activeConvIsCf=meta.conn?.kind==='cloudflare'}
  {@const activeConvIsWorkspace=context?.task_type==='workspace'}
  {@const scoped=!!paneId && !!context}
  {@const id=context?.id??''}
  {@const activeConvId=id}
  {@const activeConvIsSsh=meta.conn?.kind==='ssh'}
  {@const viewBusy=!!app.running[id]}
  {@const paneState=scoped?panes.splitDrafts[id]:null}
  {@const draft=paneState?paneState.text:legacyComposer().text}
  {@const attachments=paneState?paneState.files:legacyComposer().files}
  {@const queued=scoped?(panes.splitQueued[id]?{convId:id,text:panes.splitQueued[id].text,atts:panes.splitQueued[id].files}:null):legacyComposer().queued}
  {@const cfReady=scoped?panes.paneCfReady[id]??false:legacyComposer().cfReady}
  {@const uploading=scoped?!!panes.paneUploading[id]:composer.attaching}
  {@const messages=context?.messages.filter(m=>!m.hidden)??[]}
  <div class="thread" use:paneScroll={scoped?id:''} onscroll={(e)=>{if(!scoped)onThreadScroll(e);}}>
    <div class="thread-inner" >
      {#each messages as m, i (i)}
        {@render bubble(m, i === messages.length - 1,context)}
      {/each}
      {#if activeConv}{@render conversationActivity(activeConv)}{/if}
    </div>
  </div>

  <div class="composer-wrap">
    {#if activeConvIsCf}
      <div class="cf-bar">
        <span class="tipwrap" data-tip={cfReady ? '在本机跑起来看效果（关预览窗即停）' : '先让 AI 建出页面再预览'}><button class="cb-prev" onclick={()=>scoped?panePrefill(id,()=>void startPreview()):void startPreview()} disabled={cf.previewBusy || !cfReady}><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M1.6 8s2.4-4.4 6.4-4.4S14.4 8 14.4 8s-2.4 4.4-6.4 4.4S1.6 8 1.6 8Z" stroke="currentColor" stroke-width="1.2" /><circle cx="8" cy="8" r="1.9" stroke="currentColor" stroke-width="1.2" /></svg>{cf.previewBusy ? '启动中…' : '预览'}</button></span>
        <span class="tipwrap" data-tip={cfReady ? '发布到 Cloudflare 上线' : '先让 AI 建出页面再部署'}><button class="cb-prev" onclick={()=>scoped?panePrefill(id,fillDeploy):fillDeploy()} disabled={viewBusy || !cfReady}><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M8 12.5V4M4.5 7 8 3.5 11.5 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>部署</button></span>
        <span class="tipwrap" data-tip={cfReady ? '存成模板，以后一键复用' : '先让 AI 建出页面再存'}><button class="cb-prev dim" onclick={()=>scoped?panePrefill(id,openSaveTmpl):openSaveTmpl()} disabled={!cfReady}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><rect x="2.6" y="2.6" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9.1" y="2.6" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="2.6" y="9.1" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9.1" y="9.1" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /></svg>存模板</button></span>
      </div>
    {/if}
    <div class="composer" class:fileover={!scoped&&composer.composerFileOver} role="group" aria-label="消息输入"
      ondragover={(e)=>{if(scoped){if(e.dataTransfer?.types.includes('Files'))e.preventDefault();}else onComposerDragOver(e);}}
      ondragleave={(e)=>{if(!scoped)onComposerDragLeave(e);}}
      ondrop={(e)=>{if(scoped&&e.dataTransfer?.files.length){e.preventDefault();e.stopPropagation();void uploadPaneFiles(id,e.dataTransfer.files);}else if(!scoped)onComposerDrop(e);}}>
      {#if scoped && panes.splitErrors[id]}<p class="pane-error" role="alert">{panes.splitErrors[id]}</p>{/if}
      {#if queued && queued.convId === activeConvId}
        <div class="queued-row">
          <span class="queued-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.3" /><path d="M8 4.7V8l2.2 1.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg></span>
          <span class="queued-t">等这轮结束后发送：{queued.text || '（仅附件）'}{#if queued.atts.length} · {queued.atts.length} 个文件{/if}</span>
          <button class="queued-btn" onclick={()=>scoped?restoreSplitQueue(id):editQueued()}>编辑</button>
          <button class="queued-x" aria-label="清除等待消息" onclick={()=>{if(scoped)delete panes.splitQueued[id];else clearQueued();}}>×</button>
        </div>
      {/if}
      {#if attachments.length}
        <div class="attach-row">
          {#each attachments as a, i (a.path)}
            <div class="attach-chip">
              {#if a.preview}<img class="attach-img" src={a.preview} alt="" />{:else}<span class="attach-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M9 1.8H4.5A1.3 1.3 0 0 0 3.2 3.1v9.8a1.3 1.3 0 0 0 1.3 1.3h7a1.3 1.3 0 0 0 1.3-1.3V5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9 1.8v3.7h3.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg></span>{/if}
              <span class="attach-name" title={a.name}>{a.name}</span>
              <button class="attach-x" aria-label="移除附件" onclick={() => {if(paneState)paneState.files=paneState.files.filter((_,n)=>n!==i);else removeAttachment(i);}}>×</button>
            </div>
          {/each}
        </div>
      {/if}
      {#if uploading}<span class="attach-loading">上传中…</span>{/if}
      <textarea bind:value={()=>draft,(v)=>scoped?setPaneText(id,v):setLegacyDraft(v)} use:paneInput={scoped?id:''} aria-label={`给 ${context?.title||'当前对话'} 发消息`} rows="1" placeholder={viewBusy ? '输入下一条，回车排队，等这轮结束自动发送' : '继续说…（Enter 发送，Shift+Enter 换行，可粘贴/拖入文件）'}
        oncompositionstart={() => (composer.composing = true)} oncompositionend={() => (composer.composing = false)}
        onpaste={(e)=>onComposerPaste(e,scoped?id:'')}
        onkeydown={(e) => onComposerKey(e, scoped ? ()=>void splitSubmit(id) : viewBusy ? queueMessage : send)}></textarea>
      <div class="composer-bar">
        <div class="cb-left">
          {#if activeConvIsCf}
            <span class="cb-ro" title="项目已固定"><span class="cb-ro-t">{activeConv?.site_slug}</span></span>
          {:else if activeConvIsSsh}
            <!-- 远程对话：这里放远端系统的图标 + 机器地址。 -->
            <span class="cb-ro" title={activeConvConn?.ssh_os || '远端系统未知'}>{@render distroMark(distroOf(activeConvConn), 15)}<span class="cb-ro-t">{activeConv?.site_name}</span></span>
          {:else}
            <span class="cb-ro" title={activeConv?.workspace_dir || '本会话未关联文件夹'}>{@render workspaceMark(15)}<span class="cb-ro-t">{activeConv?.workspace_dir ? workspaceFolderName(activeConv.workspace_dir) : '纯对话'}</span></span>
          {/if}
        </div>
        <div class="cb-right">
          <Dropdown compact value={context?.perm_mode||'full'} options={permOptsFor(activeConv?.brain ?? 'claude', activeConvIsSsh)} tone={permTone(context?.perm_mode||'full')} tip={permTipFor(activeConv?.brain ?? 'claude', context?.perm_mode||'full', activeConvIsSsh)} onchange={(v)=>scoped&&context?void splitSetting(context,'perm',v):persistThreadPerm(v)} />
          <!-- 模型下拉列全部厂商（图标区分）：同厂商下一轮生效；跨厂商确认后以历史摘要重建续跑 -->
          <ModelFx options={scoped&&context?splitModelOptions(context):thread.threadComboOpts} value={`${activeConv?.brain ?? 'claude'}::${context?.model??''}`} effort={context?.effort??''} fast={activeConv?.fast ?? false} fastOk={app.brains?.claude_fast_ok ?? false} lockModel={viewBusy} onpick={(v: string) => { if(scoped&&context)void splitSetting(context,'model',v);else void pickThreadCombo(v); }} oneffort={(v:string)=>scoped&&context?void splitSetting(context,'effort',v):persistThreadEffort(v)} onfast={(v: boolean) => { if(scoped&&context)void splitSetting(context,'fast',v);else void persistThreadFast(v); }} />
          <UsageRing ctx={activeConv?.ctx_tokens ?? 0} limit={ctxLimitAdaptive(activeConv?.brain ?? 'claude', activeConv?.model ?? '', activeConv?.ctx_tokens ?? 0)} total={activeConv?.total_tokens ?? 0} />
          {#if viewBusy}
            {#if draft.trim() || attachments.length}
              <button class="send queue" disabled={uploading} onclick={()=>scoped?void splitSubmit(id):queueMessage()} title="排队：等这轮结束后自动发送">↑</button>
            {/if}
            <button class="send stop" onclick={()=>scoped?void splitStop(id):stop()} title="停止">■</button>
          {:else}
            <button class="send" onclick={()=>scoped?void splitSubmit(id):send()} disabled={uploading || (!draft.trim() && !attachments.length)} title="发送（Enter）">↑</button>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/snippet}

<!-- 顶栏的一个负载读数：标签 + 百分比 + 一条细底纹。
     pct=null＝还没算出来（CPU 要两次采样求差，见 pollStats）。 -->
{#snippet loadStat(label: string, pct: number | null, tip: string)}
  <span class="hstat" class:warn={pct !== null && pct >= 75} class:hot={pct !== null && pct >= 90} data-tip={tip}>
    <span class="hstat-l">{label}</span>
    <span class="hstat-v">{pct === null ? '—' : pct + '%'}</span>
  </span>
{/snippet}

<!-- 列宽手柄：坐在定宽列的左边缘上（就是列头之间那根分隔线）。做成 button 是为了键盘也够得着
     （←/→ 微调），顺带免掉给 div 挂事件的一堆 a11y 警告。 -->
{#snippet colGrip(k: 'perm' | 'size' | 'date')}
  <button class="fh-grip" aria-label="调整列宽（左右方向键微调）" data-tip="拖动调整列宽"
    onmousedown={(e) => startColDrag(e, k)}
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => onColKey(e, k)}></button>
{/snippet}

{#snippet sshMark(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none"><rect x="1.6" y="2.6" width="12.8" height="10.8" rx="2" stroke="currentColor" stroke-width="1.3" /><path d="M4.4 6.2 6.6 8l-2.2 1.8M8.2 10.2h3.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}

<!-- 发行版图标：按 os-release 的 ID 选（distroOf 做归一化 + ID_LIKE 兜底）。
     都是简化几何形，认得出即可；认不出的发行版走最后那个通用「服务器」形。 -->
{#snippet distroMark(id: string, size: number)}
  {#if id === 'ubuntu'}
    <!-- Ubuntu：三点「朋友圈」 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="5.6" stroke="#E95420" stroke-width="1.5" />
      <circle cx="8" cy="2.9" r="1.75" fill="#E95420" /><circle cx="3.6" cy="10.6" r="1.75" fill="#E95420" /><circle cx="12.4" cy="10.6" r="1.75" fill="#E95420" />
    </svg>
  {:else if id === 'debian'}
    <!-- Debian：开口螺旋 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path d="M11.6 4.2A5.2 5.2 0 1 0 12 11" stroke="#A80030" stroke-width="1.5" stroke-linecap="round" />
      <path d="M9.9 6.1a3 3 0 1 0 .5 3.6" stroke="#A80030" stroke-width="1.4" stroke-linecap="round" />
    </svg>
  {:else if id === 'alpine'}
    <!-- Alpine：山 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="6.2" fill="#0D597F" />
      <path d="M4 10.6 6.3 6.6l1.5 2.6M7.4 10.6l2.4-4.2 2.4 4.2z" stroke="#fff" stroke-width="1.1" stroke-linejoin="round" />
    </svg>
  {:else if id === 'fedora'}
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="6.2" fill="#51A2DA" />
      <path d="M9.9 4.6h-.8a1.9 1.9 0 0 0-1.9 1.9v4a1.5 1.5 0 1 1-1.5-1.5h3.4" stroke="#fff" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  {:else if id === 'arch'}
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path d="M8 1.8 14 13.6c-1.6-.9-3-1.5-3.6-1.7L8 7.2l-2.4 4.7c-.6.2-2 .8-3.6 1.7L8 1.8z" fill="#1793D1" />
    </svg>
  {:else if id === 'centos' || id === 'rhel' || id === 'rocky' || id === 'almalinux'}
    <!-- RHEL 系：四色方块 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <rect x="1.8" y="1.8" width="5.4" height="5.4" rx="1" fill="#932279" /><rect x="8.8" y="1.8" width="5.4" height="5.4" rx="1" fill="#EFA724" />
      <rect x="1.8" y="8.8" width="5.4" height="5.4" rx="1" fill="#79BCE8" /><rect x="8.8" y="8.8" width="5.4" height="5.4" rx="1" fill="#9CCD2A" />
    </svg>
  {:else}
    <!-- 认不出的（含没探到系统信息时）：中性服务器形 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <rect x="2" y="2.6" width="12" height="4.6" rx="1.2" stroke="currentColor" stroke-width="1.2" />
      <rect x="2" y="8.8" width="12" height="4.6" rx="1.2" stroke="currentColor" stroke-width="1.2" />
      <circle cx="4.6" cy="4.9" r=".8" fill="currentColor" /><circle cx="4.6" cy="11.1" r=".8" fill="currentColor" />
    </svg>
  {/if}
{/snippet}

{#snippet wrNote()}<div class="wr-note"><svg class="wr-ic" width="15" height="15" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.6" stroke="currentColor" stroke-width="1.3" /><path d="M8 4.8v4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /><circle cx="8" cy="11.1" r="0.6" fill="currentColor" /></svg><span>{cf.wrInstalling ? '正在从 npm 装 wrangler，首次约 1–2 分钟，别关窗…' : '还没装 wrangler，预览 / 部署要用'}</span><button class="wr-btn" onclick={installWrangler} disabled={cf.wrInstalling}>{#if cf.wrInstalling}<span class="wr-spin"></span>安装中 {elapsedLabel(cf.wrElapsed)}{:else}一键安装{/if}</button></div>{/snippet}

{#snippet cliGuide()}
  {#if app.brains}
    <div class="cli-guide" data-no-drag>
      <div class="cli-guide-h"><span>模型准备（本地客户端或设置中的 API 服务）</span><button class="cli-redetect" onclick={refreshBrainsManual} disabled={settings.brainsBusy} title="装好 / 登录完点这里重新检测（会重新读取 PATH）">{@render refreshIcon(settings.brainsBusy)}<span>重新检测</span></button></div>
      {#each [{ b: 'claude' as Brain, st: app.brains.claude, name: 'Claude Code', cmd: CLAUDE_INSTALL_CMD }, { b: 'codex' as Brain, st: app.brains.codex, name: 'Codex', cmd: 'npm i -g @openai/codex' }, { b: 'grok' as Brain, st: app.brains.grok, name: 'Grok', cmd: GROK_INSTALL_CMD }] as r (r.b)}
        <div class="cli-row">
          <BrainIcon brain={r.b} size={16} />
          <span class="cli-name">{r.name}</span>
          {#if !r.st.found}
            <span class="cli-tag bad">未安装</span>
            {#if r.b === 'claude'}
              <button class="wr-btn" use:tipAction={'安装失败排障：VPN/代理需覆盖 claude.ai 与下载域名 storage.googleapis.com（规则模式加进规则或临时切全局）；也可复制右侧命令到终端手动执行看完整输出。'} onclick={installClaude} disabled={settings.claudeInstalling}>{#if settings.claudeInstalling}<span class="wr-spin"></span>{settings.nodeBoot || `安装中 ${elapsedLabel(settings.claudeElapsed)}`}{:else}一键安装{/if}</button>
            {:else if r.b === 'grok'}
              <button class="wr-btn" onclick={installGrok} disabled={settings.grokInstalling}>{#if settings.grokInstalling}<span class="wr-spin"></span>{settings.nodeBoot || `安装中 ${elapsedLabel(settings.grokElapsed)}`}{:else}一键安装{/if}</button>
            {:else if r.b === 'codex'}
              <button class="wr-btn" onclick={installCodex} disabled={settings.codexInstalling}>{#if settings.codexInstalling}<span class="wr-spin"></span>{settings.nodeBoot || `安装中 ${elapsedLabel(settings.codexElapsed)}`}{:else}一键安装{/if}</button>
            {/if}
            <code class="cli-cmd" title={r.cmd}>{r.cmd}</code>
            <button class="cli-copy" title="复制命令" aria-label="复制安装命令" onclick={() => copyCmd(r.cmd)}>
              {#if settings.copiedCmd === r.cmd}
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M3.5 8.5l3 3 6-7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" /></svg>
              {:else}
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="5.4" y="5.4" width="8.2" height="8.2" rx="1.6" stroke="currentColor" stroke-width="1.3" /><path d="M10.6 5.4V4.1A1.6 1.6 0 0 0 9 2.5H4.1A1.6 1.6 0 0 0 2.5 4.1V9a1.6 1.6 0 0 0 1.6 1.6h1.3" stroke="currentColor" stroke-width="1.3" /></svg>
              {/if}
            </button>
          {:else if r.st.logged_in === false}
            <span class="cli-tag warn">未登录</span><button class="authbtn" onclick={() => authorize(r.b)} disabled={settings.authWaiting === r.b}>{#if settings.authWaiting === r.b}<span class="wr-spin"></span>等待授权…{:else}去授权 ↗{/if}</button>
          {:else}
            <span class="cli-tag ok">✓ {r.st.version || '已就绪'}</span>
          {/if}
        </div>
      {/each}
      <p class="cli-note"><svg class="cli-note-ic" width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.3" /><path d="M8 7.3v3.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><circle cx="8" cy="4.8" r="0.95" fill="currentColor" /></svg><span>安装/登录后状态灯自动变绿；密钥只进 {keystoreName}，绝不落盘。</span></p>
      <p class="cli-note"><svg class="cli-note-ic" width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.3" /><path d="M8 7.3v3.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><circle cx="8" cy="4.8" r="0.95" fill="currentColor" /></svg><span>没有 Node 的机器，一键安装会自动下载托管版 Node(v22 LTS) 到应用数据目录，并写入用户级 PATH（只追加、绝不覆盖或截断；写入失败会提示手动配置）。</span></p>
      {#if !app.brains.browser.found}
        <p class="cli-note"><svg class="cli-note-ic" width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.3" /><path d="M1.9 8h12.2M8 1.6c-4.4 4.2-4.4 8.6 0 12.8 4.4-4.2 4.4-8.6 0-12.8Z" stroke="currentColor" stroke-width="1.1" /></svg><span>未检测到 Chrome / Edge——「AI 网页截图配图」不可用（可选功能，装个 Chrome 即启用）。</span></p>
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet workspaceMark(size: number)}
  <svg width={size} height={size} viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M4 6.6A2.6 2.6 0 0 1 6.6 4h6.8A2.6 2.6 0 0 1 16 6.6v3.9a2.6 2.6 0 0 1-2.6 2.6H8.2l-3.1 2.7a.42.42 0 0 1-.7-.32V6.6Z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" /></svg>
{/snippet}

{#snippet cmds(tools: ToolCall[])}
  <details class="cmds">
    <summary>
      <svg class="cmd-ic" width="13" height="13" viewBox="0 0 16 16" fill="none">
        <rect x="1.5" y="2.5" width="13" height="11" rx="2" stroke="currentColor" stroke-width="1.3" />
        <path d="M4.5 6l2 2-2 2M8.5 10h3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <span>{tools.length} 条执行记录</span>
      <svg class="cmd-chev" width="11" height="11" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </summary>
    <div class="tools">{#each compactToolRows(tools) as t, i (i)}{@render toolChip(t)}{/each}</div>
  </details>
{/snippet}

<!-- 设置弹窗 -->
{#if settings.setupOpen}
  <div class="mask" role="presentation" onclick={() => { settings.setupAddOpen = false; settings.setupOpen = false; }}></div>
  <div class="sheet" use:sheetWidth={SHEET_WIDTH.setup} role="dialog" aria-modal="true" aria-label="连接与模型">
    <header class="sheet-head"><b>连接与模型</b><button class="x" onclick={() => { settings.setupAddOpen = false; settings.setupOpen = false; }}>×</button></header>
    <div class="sheet-body">
      <div class="sec-head">
        <span>连接</span>
        <div class="setup-add" bind:this={settings.setupAddEl}>
          <button
            class="icon-btn setup-add-trigger"
            class:open={settings.setupAddOpen}
            aria-label="添加连接"
            aria-haspopup="menu"
            aria-expanded={settings.setupAddOpen}
            data-tip={settings.setupAddOpen ? undefined : '添加连接'}
            onclick={() => (settings.setupAddOpen = !settings.setupAddOpen)}
          >{@render plusIcon()}</button>
          {#if settings.setupAddOpen}
            <div class="setup-add-menu" role="menu">
              <button role="menuitem" onclick={() => { settings.setupAddOpen = false; openCfConnect(); }}>
                {@render cfMark(15)}<span>连接 Cloudflare</span>
              </button>
              <button role="menuitem" onclick={() => { settings.setupAddOpen = false; openSshConnect(); }}>
                {@render sshMark(15)}<span>新建远程连接</span>
              </button>
            </div>
          {/if}
        </div>
      </div>
      {#if app.conns.length === 0}<p class="hint">还没有连接。可以连接 Cloudflare 建站部署，或新建一个 SSH 远程连接。本地对话不需要连接。</p>{/if}
      <div class="conn-list">
        {#each app.conns as c (c.id)}
          <div class="conn-row {app.activeConnId === c.id ? 'on' : ''}" role="button" tabindex="0"
            onclick={() => selectConn(c.id)}
            onkeydown={(e) => e.key === 'Enter' && e.currentTarget === e.target && selectConn(c.id)}
            oncontextmenu={(e) => openConnCtx(e, c)}>
            {#if c.kind === 'ssh'}{@render sshMark(22)}{:else}{@render cfMark(22)}{/if}
            <span class="conn-main"><b>{connectionDisplayName(c)}</b>
              {#if c.kind === 'ssh'}
                <small class="cs-os">{@render distroMark(distroOf(c), 12)}<span class="cs-os-label">{sshSub(c)}</span></small>
              {:else}
                <small><span class:conn-remark={!!c.remark?.trim()} data-tip={c.remark?.trim() ? `脱敏 Key：${c.key_prefix}` : undefined}>{c.remark?.trim() || c.key_prefix}</span> · Cloudflare</small>
              {/if}</span>
            <button
              class="icon-btn sm"
              class:active={connections.connCtx?.conn.id === c.id}
              title="连接设置"
              aria-label={`打开 ${connectionDisplayName(c)} 的连接设置`}
              aria-haspopup="menu"
              aria-expanded={connections.connCtx?.conn.id === c.id}
              onclick={(e) => toggleConnCtx(e, c)}
            >{@render settingsIcon()}</button>
          </div>
        {/each}
      </div>

      <div class="sec-head mt"><span>模型服务 · 本地客户端</span><button class="icon-btn" onclick={refreshBrainsManual} title="刷新本地客户端">{@render refreshIcon(settings.brainsBusy)}</button></div>
      {#if app.brains}
        <div class="brains-list">
        {#each [{ b: 'claude' as Brain, st: app.brains.claude, name: 'Claude Code', cmd: CLAUDE_INSTALL_CMD }, { b: 'codex' as Brain, st: app.brains.codex, name: 'Codex', cmd: 'npm i -g @openai/codex' }, { b: 'grok' as Brain, st: app.brains.grok, name: 'Grok', cmd: GROK_INSTALL_CMD }] as r (r.b)}
          <div class="brain-block">
          <div class="brain-row">
            <span class="brain-ic"><BrainIcon brain={r.b} size={18} /></span>
            <span class="brain-main"><b>{r.name}</b>
              <small>{#if !r.st.found}未安装{:else if r.st.logged_in === false}未登录{:else}{r.st.version || '已就绪'}{#if r.b === 'claude' && settings.cliUpd?.has_update} · <span class="cli-new">新版 {settings.cliUpd.latest}</span>{/if}{/if}</small></span>
            <span class="brain-dot"><span class="dot {r.st.found && r.st.logged_in && !(r.b === 'claude' && settings.claudeVerify === 'bad') ? 'ok' : r.st.found ? 'warn' : 'off'}"></span></span>
            {#if r.st.found && r.st.logged_in === false}
              {#if r.b === 'claude'}
                <span class="brain-auth-actions">
                  <button
                    class="claude-token-trigger"
                    class:active={settings.claudeTokenOpen}
                    type="button"
                    onclick={() => (settings.claudeTokenOpen = !settings.claudeTokenOpen)}
                    aria-label="粘贴 Claude 订阅 Token"
                    aria-expanded={settings.claudeTokenOpen}
                    data-tip="粘贴订阅 Token&#10;使用 claude setup-token 生成"
                  ><svg width="15" height="15" viewBox="0 0 18 18" fill="none" aria-hidden="true"><circle cx="5.5" cy="12.5" r="3.25" stroke="currentColor" stroke-width="1.45"/><path d="m7.85 10.15 6.55-6.55M11.45 6.55l1.7 1.7M13.15 4.85l1.35 1.35" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
                  <button class="authbtn" onclick={() => authorize(r.b)} disabled={settings.authWaiting === r.b}>{#if settings.authWaiting === r.b}<span class="wr-spin"></span>等待授权…{:else}去授权 ↗{/if}</button>
                </span>
              {:else}
                <button class="authbtn" onclick={() => authorize(r.b)} disabled={settings.authWaiting === r.b}>{#if settings.authWaiting === r.b}<span class="wr-spin"></span>等待授权…{:else}去授权 ↗{/if}</button>
              {/if}
            {:else if r.b === 'claude' && r.st.logged_in && settings.claudeVerify === 'bad'}<button class="authbtn" onclick={() => rebind('claude')} disabled={settings.authWaiting === 'claude'}>{#if settings.authWaiting === 'claude'}<span class="wr-spin"></span>等待授权…{:else}重新绑定 ↗{/if}</button>
            {:else if r.b === 'claude' && settings.cliUpd?.has_update}
              <!-- 图标 + tips：这一行右侧本来是「去授权 / 重新绑定」那类**必须做**的动作，
                   升级是可选的，用文字按钮会跟它们抢同一级视觉重量。tips 里写全「升到哪个版本、
                   会动哪个路径」——多安装时只有 PATH 第一个生效，这信息不能只藏在确认弹窗里。
                   升级中不另写 tip：disabled 的按钮在 Chrome 下根本不派发 mouseover，气泡永远弹不出来，
                   状态只能靠 spinner 表达。 -->
              <button class="cli-upg" onclick={() => void upgradeCli()} disabled={settings.cliUpgrading}
                aria-label={`升级 Claude Code 到 ${settings.cliUpd.latest}`}
                data-tip={`升级到 ${settings.cliUpd.latest}\n${settings.cliUpd.target}${settings.cliUpd.all.length > 1 ? `\nPATH 上有 ${settings.cliUpd.all.length} 份，只有这个生效` : ''}`}>
                {#if settings.cliUpgrading}<span class="wr-spin"></span>{:else}{@render updateIcon(14)}{/if}
              </button>{/if}
          </div>
          {#if r.b === 'claude' && settings.claudeTokenOpen}
            <div class="claude-token-form">
              <div class="claude-token-copy">
                <b>Claude Code 订阅 Token</b>
                <small>在终端运行 <code>claude setup-token</code>，完成授权后复制生成的长期 Token。仅支持 Pro、Max、Team 或 Enterprise 订阅。</small>
              </div>
              <div class="claude-token-entry">
                <input
                  type="password"
                  bind:value={settings.claudeTokenDraft}
                  placeholder="粘贴 Token"
                  autocomplete="off"
                  spellcheck="false"
                  autocapitalize="off"
                  autocorrect="off"
                  onkeydown={(event) => event.key === 'Enter' && void saveClaudeToken()}
                />
                <button class="btn sm" type="button" onclick={() => void saveClaudeToken()} disabled={!settings.claudeTokenDraft.trim() || settings.claudeTokenSaving}>{settings.claudeTokenSaving ? '验证中…' : '保存并验证'}</button>
                <button class="btn sm ghost" type="button" onclick={closeClaudeToken} disabled={settings.claudeTokenSaving}>取消</button>
              </div>
              <small class="claude-token-safe">Token 只保存到 {keystoreName}，不会写入 Pilot 配置、导出文件或对话内容。</small>
            </div>
          {/if}
          {#if !r.st.found}
            <!-- 与主界面同款一键安装（同 invoke/进度态/错误处理），放在「未安装」状态文字下方、小号靠左；
                 手动命令降级为同行的复制小图标 -->
            <div class="brain-install">
              <button class="wr-btn" onclick={r.b === 'claude' ? installClaude : r.b === 'codex' ? installCodex : installGrok}
                disabled={r.b === 'claude' ? settings.claudeInstalling : r.b === 'codex' ? settings.codexInstalling : settings.grokInstalling}>
                {#if (r.b === 'claude' && settings.claudeInstalling) || (r.b === 'codex' && settings.codexInstalling) || (r.b === 'grok' && settings.grokInstalling)}
                  <span class="wr-spin"></span>{settings.nodeBoot || `安装中 ${elapsedLabel(r.b === 'claude' ? settings.claudeElapsed : r.b === 'codex' ? settings.codexElapsed : settings.grokElapsed)}`}
                {:else}一键安装{/if}
              </button>
              <button class="cli-copy" title={`手动安装命令：${r.cmd}（点击复制）`} aria-label="复制手动安装命令" onclick={() => copyCmd(r.cmd)}>
                {#if settings.copiedCmd === r.cmd}
                  <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M3.5 8.5l3 3 6-7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" /></svg>
                {:else}
                  <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="5.4" y="5.4" width="8.2" height="8.2" rx="1.6" stroke="currentColor" stroke-width="1.3" /><path d="M10.6 5.4V4.1A1.6 1.6 0 0 0 9 2.5H4.1A1.6 1.6 0 0 0 2.5 4.1V9a1.6 1.6 0 0 0 1.6 1.6h1.3" stroke="currentColor" stroke-width="1.3" /></svg>
                {/if}
              </button>
            </div>
          {/if}
          {#if r.st.found}
            <div class="cust">
              <!-- 自定义模型 与 账号 两个折叠头并排一行（默认都收起），展开体各自出现在行下方。
                   账号从各 CLI 本地凭据只读提取（brains.rs）；重新绑定 = headless logout
                   + 原有授权终端流程登录新账号 -->
              <div class="cust-heads">
                <button class="cust-head" type="button" onclick={() => (settings.customOpen[r.b] = !settings.customOpen[r.b])}>
                  <svg class="cust-chev" class:open={settings.customOpen[r.b]} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  自定义模型{#if customsOf(r.b).length}<span class="cust-n">{customsOf(r.b).length}</span>{/if}
                </button>
                {#if r.st.logged_in}
                  <button class="cust-head" type="button" onclick={() => (settings.acctOpen[r.b] = !settings.acctOpen[r.b])}>
                    <svg class="cust-chev" class:open={settings.acctOpen[r.b]} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                    账号
                    {#if r.b === 'claude' && settings.claudeVerify === 'bad'}
                      <span class="verify-bad">凭据已失效</span>
                    {:else if r.b === 'claude' && settings.claudeVerify === 'checking'}
                      <span class="verify-checking">验证中…</span>
                    {/if}
                  </button>
                {/if}
              </div>
              {#if settings.customOpen[r.b]}
                <div class="cust-body">
                  {#each customsOf(r.b) as id (id)}
                    <div class="cust-chip"><span class="cust-id">{id}</span><button class="cust-x" title="删除" onclick={() => removeCustom(r.b, id)}>×</button></div>
                  {/each}
                  <div class="cust-add">
                    <input class="tin" bind:value={settings.customDraft[r.b]} placeholder={r.b === 'codex' ? '如 gpt-5.5 / o3' : r.b === 'grok' ? '如 grok-4.6' : '如 claude-opus-4-8'}
                      spellcheck="false" autocapitalize="off" autocorrect="off" onkeydown={(e) => e.key === 'Enter' && addCustom(r.b)} />
                    <button class="btn sm" onclick={() => addCustom(r.b)} disabled={!(settings.customDraft[r.b] ?? '').trim()}>添加</button>
                  </div>
                </div>
              {/if}
              {#if settings.acctOpen[r.b] && r.st.logged_in}
                <div class="brain-auth">
                  {#if r.st.account}<span class="brain-acct" title={`当前账号：${r.st.account}`}>{r.st.account}</span>{/if}
                  {#if r.st.account}<span class="brain-auth-sep" aria-hidden="true">·</span>{/if}
                  {#if r.b === 'claude'}
                    <button
                      class="brain-auth-icon"
                      class:active={settings.claudeTokenOpen}
                      type="button"
                      aria-label="更换 Claude 订阅 Token"
                      aria-expanded={settings.claudeTokenOpen}
                      data-tip="更换订阅 Token&#10;粘贴 claude setup-token 生成的长期授权 Token"
                      onclick={() => (settings.claudeTokenOpen = !settings.claudeTokenOpen)}
                    ><svg width="14" height="14" viewBox="0 0 18 18" fill="none" aria-hidden="true"><circle cx="5.5" cy="12.5" r="3.25" stroke="currentColor" stroke-width="1.45"/><path d="m7.85 10.15 6.55-6.55M11.45 6.55l1.7 1.7M13.15 4.85l1.35 1.35" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
                  {/if}
                  <button
                    class="brain-auth-icon"
                    type="button"
                    aria-label={settings.authWaiting === r.b ? '正在重新绑定账号' : '重新绑定账号'}
                    data-tip={settings.authWaiting === r.b ? '正在等待账号授权…' : '退出当前账号并重新登录'}
                    onclick={() => rebind(r.b)}
                    disabled={settings.authWaiting === r.b}
                  >{#if settings.authWaiting === r.b}<span class="wr-spin"></span>{:else}{@render refreshIcon(false)}{/if}</button>
                </div>
              {/if}
            </div>
          {/if}
          </div>
        {/each}
        </div>
      {/if}
      <p class="hint tos">自定义模型 ID 会作为该厂商模型下拉里的附加档位（可加多个）；仅限本人订阅账户驱动本地官方 CLI，密钥存 {keystoreName}。</p>
      <ApiServices profiles={app.apiProfiles} onchange={(profiles: ApiProfile[]) => { app.apiProfiles = profiles; }} />

      <div class="sec-head mt"><span>关于</span></div>
      <div class="brain-row">
        <span class="brain-ic"><AppIcon size={20} /></span>
        <span class="brain-main"><b>Cligc Pilot</b><small>{updater.appVersion ? `v${updater.appVersion}` : ''}{updater.updMsg ? ` · ${updater.updMsg}` : ''}</small></span>
        <button class="btn small ghost bare about-update-entry" onclick={openUpdateCenter} disabled={!isMainWindow}>
          {@render updateIcon(14)}
          <span>{updater.updAvail ? `新版本 ${updater.updAvail}` : '检查更新'}</span>
        </button>
      </div>
    </div>
  </div>
{/if}

{#if updater.updateCenterOpen}
  <div class="mask update-center-mask" role="presentation" onclick={closeUpdateCenter}></div>
  <div class="sheet update-center-sheet" use:sheetWidth={SHEET_WIDTH.updateCenter} role="dialog" aria-modal="true" aria-label="更新中心">
    <header class="sheet-head update-center-head">
      <div class="update-center-title">
        <span class="update-center-title-icon" aria-hidden="true">{@render updateIcon(16)}</span>
        <span class="update-center-title-copy">
          <b>更新中心</b>
          <small>{updater.updateCheckedAt ? `上次检查 ${new Date(updater.updateCheckedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}` : '尚未检查'}</small>
        </span>
      </div>
      <div class="update-center-head-actions">
        <button
          type="button"
          class="icon-btn"
          aria-label={updater.pilotUpdateStatus === 'checking' ? '正在重新检查更新' : '重新检查更新'}
          data-tip={updater.pilotUpdateStatus === 'checking' ? '正在重新检查更新…' : '重新检查更新'}
          onclick={() => void checkPilotUpdate(false)}
          disabled={updater.pilotUpdateStatus === 'checking' || updater.updBusy}
        >{@render refreshIcon(updater.pilotUpdateStatus === 'checking')}</button>
        <button class="x" onclick={closeUpdateCenter} disabled={updater.updBusy}>×</button>
      </div>
    </header>
    <div class="sheet-body update-center-body">
      {#if updater.pilotUpdateReceipt}<div class="update-center-notice success">{updater.pilotUpdateReceipt}</div>{/if}
      <section class="update-section">
        <div class="update-item pilot">
          <span class="update-item-icon"><AppIcon size={20} /></span>
          <span class="update-item-copy">
            <b>Cligc Pilot</b>
            <small>{pilotUpdateMessage()}</small>
            {#if updater.pilotReleaseNotes && updater.pilotUpdateStatus === 'available'}<i>{updater.pilotReleaseNotes}</i>{/if}
          </span>
          <span class="update-state {updater.pilotUpdateStatus}" title={pilotUpdateMessage()}>{pilotUpdateLabel(updater.pilotUpdateStatus)}</span>
          {#if updater.pilotUpdateStatus === 'available'}
            <button class="btn small ghost" onclick={() => void runUpdate()} disabled={updater.updBusy}>更新</button>
          {/if}
        </div>
      </section>
      <p class="update-center-footnote">更新包经 ed25519 签名校验后安装，安装完成会自动重启客户端；正在进行的对话会在重启前保存。</p>
    </div>
    <footer class="update-center-footer">
      <button class="btn ghost" onclick={closeUpdateCenter} disabled={updater.updBusy}>关闭</button>
      <button class="btn primary" onclick={() => void runUpdate()} disabled={updater.updBusy || updater.pilotUpdateStatus !== 'available'}>
        {updater.updBusy ? '正在更新…' : updater.updAvail ? `更新到 ${updater.updAvail}` : '当前无需更新'}
      </button>
    </footer>
  </div>
{/if}

<!-- 连接 Cloudflare -->
{#if sshForm.sshOpen}
  <div class="mask" role="presentation" onclick={() => (sshForm.sshOpen = false)}></div>
  <div class="modal wide ssh-modal" use:dialogGeometry={DIALOG_GEOMETRY.ssh} role="dialog" aria-modal="true" aria-label={sshForm.sshCredentialRepair ? '补录远程连接密码' : sshForm.sshEditId ? '编辑远程连接' : '新建远程连接'}>
    <header class="sheet-head"><b>{sshForm.sshCredentialRepair ? '补录远程连接密码' : sshForm.sshEditId ? '编辑远程连接' : '新建远程连接'}</b><button class="x" onclick={() => (sshForm.sshOpen = false)}>×</button></header>
    <div class="sheet-body">
      <p class="hint">{#if sshForm.sshCredentialRepair}这条连接的密码已不在 {keystoreName} 中。输入一次密码并测试连接，保存后会自动恢复终端和文件列表。{:else if sshForm.sshEditId}改完地址 / 用户 / 认证方式后要重新「测试连接」核对指纹再保存；只改名字可以直接保存。密码 / 私钥口令留空＝沿用 {keystoreName} 里已存的那条。{:else}SSH 到一台远程机器：能开终端、管文件、让 AI 帮你干活。密码 / 私钥口令只进 {keystoreName}，绝不落盘。{/if}</p>
      <div class="trow">
        <div class="tfield" style="flex:2"><span>主机</span><input class="tin" bind:value={sshForm.sshF.host} placeholder="例如 1.2.3.4 或 server.example.com" spellcheck="false" autocapitalize="off" autocorrect="off" /></div>
        <div class="tfield" style="flex:1"><span>端口</span><input class="tin" type="number" bind:value={sshForm.sshF.port} placeholder="22" /></div>
      </div>
      <div class="trow">
        <div class="tfield"><span>用户名</span><input class="tin" bind:value={sshForm.sshF.user} placeholder="root" spellcheck="false" autocapitalize="off" autocorrect="off" /></div>
        <div class="tfield"><span>认证方式</span><Dropdown bind:value={sshForm.sshF.auth} options={sshAuthOpts} /></div>
      </div>
      {#if sshForm.sshF.auth === 'password'}
        <div class="tfield"><span>密码{#if sshForm.sshEditId && sshForm.sshF0.auth === 'password' && !sshForm.sshCredentialRepair}（留空＝不改）{/if}</span><input class="tin" type="password" bind:value={sshForm.sshF.password} autocomplete="off" placeholder={sshForm.sshCredentialRepair ? '请输入这条连接的密码' : sshForm.sshEditId && sshForm.sshF0.auth === 'password' ? '不改就别填' : ''} /></div>
      {:else}
        <div class="tfield"><span>私钥文件（只记住路径，不会拷贝你的私钥）</span>
          <div class="ssh-key-row">
            <input class="tin" bind:value={sshForm.sshF.keyPath} placeholder="~/.ssh/id_ed25519" spellcheck="false" autocapitalize="off" autocorrect="off" />
            <button class="btn sm" onclick={pickSshKey}>选择…</button>
          </div>
        </div>
        <div class="tfield"><span>私钥口令（没有就留空{#if sshForm.sshEditId && sshForm.sshF0.auth === 'key'}；不改也留空{/if}）</span><input class="tin" type="password" bind:value={sshForm.sshF.keyPass} autocomplete="off" /></div>
      {/if}
      <div class="tfield"><span>名称（可选）</span><input class="tin" bind:value={sshForm.sshF.name} placeholder={sshForm.sshF.user && sshForm.sshF.host ? `${sshForm.sshF.user}@${sshForm.sshF.host}` : '留空自动用 user@host'} /></div>

      {#if sshForm.sshErr}<div class="err-note">{sshForm.sshErr}</div>{/if}

      {#if sshForm.sshFp}
        <!-- TOFU：指纹必须由人确认过才保存（后端无指纹直接拒）。 -->
        <div class="ssh-fp" class:alarm={sshForm.sshFpChanged}>
          {#if sshForm.sshFpChanged}
            <!-- 这台机器的主机密钥变了。可能是重装/换机，也可能是有人在中间——不能轻轻放过。 -->
            <b class="fp-alarm">⚠ 这台机器的指纹变了！</b>
            <small>之前记住的是：</small>
            <code class="fp-old">{sshForm.sshOldFp}</code>
            <small>现在连上看到的是：</small>
            <code>{sshForm.sshFp}</code>
            <small>服务器重装 / 换机器会这样，<b>被中间人劫持也会这样</b>。请到服务器上执行
              <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code> 亲自核对；对不上就别保存。</small>
          {:else}
            <b>认证通过。请核对主机指纹：</b>
            <code>{sshForm.sshFp}</code>
            <small>请确认它和你在服务器上执行 <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code> 看到的一致 —— 对不上可能是中间人。确认后此指纹会被记住，以后变了会直接拒连。</small>
          {/if}
        </div>
      {/if}

      <div class="row-end">
        <button class="btn ghost" onclick={() => (sshForm.sshOpen = false)}>取消</button>
        {#if sshForm.sshRenameOnly}
          <!-- 只改了名字：和「能不能连上」无关，不必为此去连一次机器 -->
          <button class="btn primary" onclick={saveSshEdit} disabled={sshForm.sshConnecting}>{sshForm.sshConnecting ? '保存中…' : '保存'}</button>
        {:else if !sshForm.sshFp}
          <button class="btn primary" onclick={testSsh} disabled={sshForm.sshTesting || !sshForm.sshCanTest}>{sshForm.sshTesting ? '连接中…' : '测试连接'}</button>
        {:else if sshForm.sshEditId}
          <button class="btn primary" class:danger={sshForm.sshFpChanged} onclick={saveSshEdit} disabled={sshForm.sshConnecting}>{sshForm.sshConnecting ? '保存中…' : sshForm.sshFpChanged ? '我已核对，接受新指纹' : '指纹无误，保存'}</button>
        {:else}
          <button class="btn primary" onclick={confirmSsh} disabled={sshForm.sshConnecting}>{sshForm.sshConnecting ? '添加中…' : '指纹无误，添加'}</button>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if connections.connCtx}
  <!-- 连接切换器的右键菜单。菜单项按连接类型给：能做的才列，做不了的不摆在那儿灰着。 -->
  {@const c = connections.connCtx.conn}
  <div bind:this={connections.connCtxMenuEl} class="ctx-menu fctx conn-ctx-menu" class:ready={connections.connCtx.ready} style="left:{connections.connCtx.x}px; top:{connections.connCtx.y}px" role="menu" tabindex="-1">
    <button class="ctx-item" role="menuitem" onclick={() => openConnWindow(c.id)}>新窗口打开</button>
    {#if c.kind === 'ssh'}
      <button class="ctx-item" role="menuitem" onclick={() => { connections.switcherOpen = false; selectConn(c.id); openRemote(); }}>远程终端</button>
      <div class="ctx-div"></div>
      <button class="ctx-item" role="menuitem" onclick={() => { connections.switcherOpen = false; openSshEdit(c); }}>编辑连接…</button>
    {:else}
      <div class="ctx-div"></div>
      <button class="ctx-item" role="menuitem" onclick={() => openConnRemark(c)}>{c.remark?.trim() ? '修改备注…' : '设置备注…'}</button>
      <button class="ctx-item" role="menuitem" onclick={() => openCfUpdate(c)}>更新 Token…</button>
    {/if}
    <div class="ctx-div"></div>
    <button
      class="ctx-item danger"
      role="menuitem"
      data-keep-connection-menu
      onclick={(e) => { e.stopPropagation(); void removeConn(c.id, true); }}
    >删除连接…</button>
  </div>
{/if}

{#if connections.connRemarkEdit}
  <div class="mask" role="presentation" onclick={() => !connections.connRemarkEdit?.busy && (connections.connRemarkEdit = null)}></div>
  <div class="modal conn-remark-modal" use:dialogGeometry={DIALOG_GEOMETRY.connectionRemark} role="dialog" aria-modal="true" aria-label="设置连接备注">
    <header class="sheet-head"><div><b>设置备注</b><small class="dim">{connections.connRemarkEdit.name}</small></div><button class="x" onclick={() => (connections.connRemarkEdit = null)} disabled={connections.connRemarkEdit.busy}>×</button></header>
    <div class="sheet-body">
      <p class="hint">备注会替代列表里的脱敏 Key；鼠标悬停仍可查看原来的脱敏 Key。留空保存即可清除。</p>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="tin" bind:value={connections.connRemarkEdit.value} maxlength="60" placeholder="例如 cms.ccvar.com" spellcheck="false" autocapitalize="off" autocorrect="off" autofocus disabled={connections.connRemarkEdit.busy} onkeydown={(e) => e.key === 'Enter' && void saveConnRemark()} />
      {#if connections.connRemarkEdit.error}<div class="err-note">{connections.connRemarkEdit.error}</div>{/if}
      <div class="row-end">
        <button class="btn ghost" onclick={() => (connections.connRemarkEdit = null)} disabled={connections.connRemarkEdit.busy}>取消</button>
        <button class="btn primary" onclick={() => void saveConnRemark()} disabled={connections.connRemarkEdit.busy || connections.connRemarkEdit.value.trim() === connections.connRemarkEdit.original}>{connections.connRemarkEdit.busy ? '保存中…' : '保存'}</button>
      </div>
    </div>
  </div>
{/if}

{#if sftp.fctxMenu}
  <!-- 远程文件右键菜单。挂最外层：列表自己是滚动容器，菜单放里面会被裁掉。 -->
  <div bind:this={sftp.fctxMenuEl} class="ctx-menu fctx" style="left:{sftp.fctxMenu.x}px; top:{sftp.fctxMenu.y}px" role="menu" tabindex="-1">
    {#if sftp.fctxMenu.row}
      {@const r = sftp.fctxMenu.row}
      <button class="ctx-item" role="menuitem" onclick={() => sftpOpenRow(r)}>{r.dir && !r.link ? '打开' : r.link ? '打开（跟随链接）' : '打开（编辑）'}</button>
      <button class="ctx-item" role="menuitem" onclick={() => sftpDownload(r)} disabled={!!sftp.sftpXfer}>下载…</button>
      {#if r.dir && !r.link}<button class="ctx-item" role="menuitem" onclick={() => sftpDownload(r, true)} disabled={!!sftp.sftpXfer}>打包下载（.tar.gz）…</button>{/if}
      {#if r.dir && !r.link}
        <button class="ctx-item" role="menuitem" onclick={() => sftpUpload(r.path)} disabled={!!sftp.sftpXfer}>上传到这里…</button>
        <button class="ctx-item" role="menuitem" onclick={() => openMkdir(r.path)}>在这里新建文件夹</button>
      {/if}
      <div class="ctx-div"></div>
      <button class="ctx-item" role="menuitem" onclick={() => copySftpEntry(r)}>复制</button>
      <button class="ctx-item" role="menuitem" onclick={() => duplicateSftpEntry(r)} disabled={!!sftp.sftpXfer}>复制到当前目录</button>
      {#if r.dir && !r.link && sftp.sftpClipboard}
        <button class="ctx-item" role="menuitem" onclick={() => pasteSftpEntry(r.path)} disabled={!!sftp.sftpXfer || sftp.sftpClipboard.connId !== sftp.sftpConnId}>粘贴到这里</button>
      {/if}
      <button class="ctx-item" role="menuitem" onclick={() => copyPath(r.path)}>复制路径</button>
      <button class="ctx-item" role="menuitem" onclick={() => openRename(r)}>重命名</button>
      <div class="ctx-div"></div>
      <button class="ctx-item danger" role="menuitem" onclick={() => sftpDelete(r)}>删除…</button>
    {:else}
      <button class="ctx-item" role="menuitem" onclick={() => openMkdir()}>新建文件夹</button>
      <button class="ctx-item" role="menuitem" onclick={() => openNewFile()}>新建文件</button>
      <button class="ctx-item" role="menuitem" onclick={() => sftpUpload()} disabled={!!sftp.sftpXfer}>上传文件…</button>
      <div class="ctx-div"></div>
      {#if sftp.sftpClipboard}
        <button class="ctx-item" role="menuitem" onclick={() => pasteSftpEntry()} disabled={!!sftp.sftpXfer || sftp.sftpClipboard.connId !== sftp.sftpConnId}>粘贴</button>
      {/if}
      <button class="ctx-item" role="menuitem" onclick={() => copyPath(sftp.sftpPath)}>复制当前路径</button>
      <button class="ctx-item" role="menuitem" onclick={sftpRefresh}>刷新</button>
    {/if}
  </div>
{/if}

{#if sftp.fsAsk}
  <div class="mask" role="presentation" onclick={() => !sftp.fsAsk?.busy && (sftp.fsAsk = null)}></div>
  <div class="modal" use:dialogGeometry={DIALOG_GEOMETRY.fileOperation} role="dialog" aria-modal="true" aria-label={sftp.fsAsk.mode === 'mkdir' ? '新建文件夹' : '新建文件'}>
    <header class="sheet-head"><b>{sftp.fsAsk.mode === 'mkdir' ? '新建文件夹' : '新建文件'}</b><button class="x" onclick={() => (sftp.fsAsk = null)} disabled={sftp.fsAsk.busy}>×</button></header>
    <div class="sheet-body">
      <p class="hint">位置：<code>{sftp.fsAsk.dir}</code></p>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="tin" bind:value={sftp.fsAsk.value} placeholder={sftp.fsAsk.mode === 'mkdir' ? '文件夹名' : '文件名（例如 notes.txt）'} spellcheck="false" autocapitalize="off" autocorrect="off" autofocus disabled={sftp.fsAsk.busy} onkeydown={(e) => e.key === 'Enter' && confirmFsAsk()} />
      {#if sftp.fsAsk.err}<div class="err-note">{sftp.fsAsk.err}</div>{/if}
      <div class="row-end">
        <button class="btn ghost" onclick={() => (sftp.fsAsk = null)} disabled={sftp.fsAsk.busy}>取消</button>
        <button class="btn primary" onclick={confirmFsAsk} disabled={sftp.fsAsk.busy || !sftp.fsAsk.value.trim()}>{sftp.fsAsk.busy ? '处理中…' : '确定'}</button>
      </div>
    </div>
  </div>
{/if}

{#if editor.edOpen}
  <div class="mask" role="presentation" onclick={() => void closeEdit()}></div>
  <div class="modal ed-modal" class:ed-full={editor.edFull} use:dialogGeometry={{ ...DIALOG_GEOMETRY.fileEditor, enabled: !editor.edFull }} role="dialog" aria-modal="true" aria-label={`编辑文件 ${editor.edPath}`} aria-busy={editor.edSaving}>
    <header class="sheet-head ed-head">
      <div class="ed-heading"><div><b>编辑文件</b><span class="ed-badge" class:dirty={editor.edDirty}>{editor.edDirty ? '未保存' : editor.edSaved ? '已保存' : '远程文件'}</span></div><small class="dim ed-path" title={editor.edPath}>{editor.edPath}</small></div>
      <button type="button" class="ed-expand" title={editor.edFull ? '退出全屏' : '全屏编辑'} aria-label={editor.edFull ? '退出全屏' : '全屏编辑'} aria-pressed={editor.edFull} onclick={() => { editor.edFull = !editor.edFull; }}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          {#if editor.edFull}
            <path d="M3 8h5V3M16 3v5h5M21 16h-5v5M8 21v-5H3" />
          {:else}
            <path d="M8 3H3v5M16 3h5v5M21 16v5h-5M8 21H3v-5" />
          {/if}
        </svg>
      </button>
      <button class="x" aria-label="关闭文件编辑器" onclick={() => void closeEdit()} disabled={editor.edSaving || editor.edFormatting}>×</button>
    </header>
    <div class="sheet-body ed-body">
      {#if editor.edLoading}
        <div class="files-empty">读取中…</div>
      {:else if editor.edErr && editor.edReadOnly}
        <div class="err-note">{editor.edErr}</div>
      {:else}
        <FileCodeEditor bind:value={editor.edText} path={editor.edPath} indent={editor.edFormat.indent} readonly={editor.edSaving || editor.edClosing} bind:busy={editor.edFormatting}
          eol={`UTF-8${editor.edFormat.bom ? ' BOM' : ''} · ${editor.edFormat.mixed ? '混合换行' : editor.edFormat.eol === '\r\n' ? 'CRLF' : editor.edFormat.eol === '\r' ? 'CR' : 'LF'}`} onsave={() => void saveEdit()} />
        {#if editor.edErr}<div class="err-note">{editor.edErr}</div>{/if}
      {/if}
    </div>
    <div class="ed-footer">
      <span class="ed-save-note">{editor.edSaving ? '正在保存到服务器…' : '仅在点击保存时写入远端；不会自动格式化。'}</span>
      <button type="button" class="btn ghost" onclick={() => void closeEdit()} disabled={editor.edSaving || editor.edFormatting}>关闭</button>
      {#if !editor.edReadOnly && !editor.edLoading}
        <button type="button" class="btn primary" onclick={saveEdit} disabled={editor.edSaving || editor.edFormatting || editor.edClosing || !editor.edDirty}>{editor.edSaving ? '保存中…' : '保存'}</button>
      {/if}
    </div>
  </div>
{/if}

{#if cf.cfOpen}
  {@const updatingCf = cfUpdateConnection()}
  <div class="mask cf-mask" role="presentation" onclick={closeCfConnect}></div>
  <div class="modal cf-modal" use:dialogGeometry={DIALOG_GEOMETRY.cloudflare} role="dialog" aria-modal="true" aria-label={updatingCf ? '更新 Cloudflare 连接' : '连接 Cloudflare'}>
    <header class="sheet-head"><div><b>{updatingCf ? '更新' : '连接'} {@render cfMark(15)} Cloudflare</b><small class="dim" style="margin-left:10px">token 只进钥匙串，绝不落盘</small></div><button class="x" onclick={closeCfConnect} disabled={cf.cfConnecting || cf.cfVerifying}>×</button></header>
    <div class="sheet-body">
      {#if updatingCf}
        <div class="cf-update-target">
          <span>{@render cfMark(17)}</span>
          <div><b>{updatingCf.remark?.trim() || updatingCf.name}</b><small>{updatingCf.remark?.trim() ? `${updatingCf.name} · ` : ''}{updatingCf.key_prefix} · 账号 {updatingCf.account_id}</small></div>
        </div>
        <p class="hint">不用在已有的很多 Token 中猜哪一个需要修改。建议直接为这条 Pilot 连接新建一枚权限完整的 Token；验证成功后只替换这一条连接，其他连接不受影响。</p>
      {/if}
      <div class="cf-step">
        <p class="cf-step-t">① {updatingCf ? '为这条连接新建一枚 API Token' : '在 Cloudflare 建一个 API Token'}（下列权限已预选）</p>
        <button class="cf-perms-toggle" type="button" onclick={() => (cf.cfPermsOpen = !cf.cfPermsOpen)}>
          <svg class="cf-chev" class:open={cf.cfPermsOpen} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5 6 7.5 9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
          所需权限（5 项）
        </button>
        {#if cf.cfPermsOpen}
          <ul class="cf-perms">
            <li>Account · Cloudflare Pages · <b>Edit</b></li>
            <li>Account · Workers Scripts · <b>Edit</b></li>
            <li>Account · D1 · <b>Edit</b></li>
            <li>Zone · DNS · <b>Edit</b>（绑定自定义域名用）</li>
            <li>Zone · Zone · <b>Read</b></li>
          </ul>
        {/if}
        <button class="btn soft" onclick={() => openUrl(cfTokenTemplateUrl())}>{updatingCf ? '为此连接新建' : '打开'} {@render cfMark(14)} Cloudflare Token ↗</button>
      </div>
      <div class="cf-step">
        <p class="cf-step-t">② 把{updatingCf ? '新的' : '生成的'} Token 粘到这里：</p>
        <div class="cf-row">
          <input class="tin" type="password" bind:value={cf.cfToken} placeholder="粘贴 API Token" autocomplete="off" spellcheck="false" oninput={resetCfVerification} onkeydown={(e) => e.key === 'Enter' && verifyCf()} />
          <button class="btn small primary" onclick={verifyCf} disabled={cf.cfVerifying || !cf.cfToken.trim()}>{cf.cfVerifying ? '验证中…' : '验证'}</button>
        </div>
      </div>
      {#if cf.cfAccounts.length}
        {#if updatingCf}
          <div class="cf-update-verified" class:ok={cf.cfAccounts.some((account) => account.id === updatingCf.account_id)}>
            <span>{cf.cfAccounts.some((account) => account.id === updatingCf.account_id) ? '✓' : '!'}</span>
            <div><b>{cf.cfAccounts.some((account) => account.id === updatingCf.account_id) ? '已确认属于原 Cloudflare 账号' : '不是原 Cloudflare 账号'}</b><small>保存后连接名称和已有设置保持不变。</small></div>
          </div>
        {:else}
          <div class="cf-step">
            <p class="cf-step-t">③ 选择账号 · 给连接起个名：</p>
            <Dropdown bind:value={cf.cfAccountId} options={cf.cfAccounts.map((a) => ({ value: a.id, label: a.name, sub: a.id }))} placeholder="选择账号" />
            <input class="tin" style="margin-top:7px" bind:value={cf.cfName} placeholder="连接名称（可留默认）" />
            {#if cf.cfZones.length}<p class="hint">可管理域名：{cf.cfZones.slice(0, 6).map((z) => z.name).join('、')}{cf.cfZones.length > 6 ? ` 等 ${cf.cfZones.length} 个` : ''}</p>{:else}<p class="hint">没检测到可管理域名——绑定自定义域名需 Zone 权限，可稍后补 token。</p>{/if}
          </div>
        {/if}
      {/if}
      {#if app.brains?.wrangler && !app.brains.wrangler.found}{@render wrNote()}{/if}
      {#if cf.cfErr}<div class="err-note">{cf.cfErr}</div>{/if}
      <div class="row-end">
        <button class="btn small ghost" onclick={closeCfConnect} disabled={cf.cfConnecting || cf.cfVerifying}>取消</button>
        <button class="btn small primary" onclick={confirmCf} disabled={cf.cfConnecting || !cf.cfAccountId || !cf.cfAccounts.some((account) => account.id === cf.cfAccountId)}>{cf.cfConnecting ? (updatingCf ? '更新中…' : '连接中…') : updatingCf ? '更新此连接' : '连接'}</button>
      </div>
    </div>
  </div>
{/if}

<!-- 存为模板 -->
{#if library.saveTmplOpen}
  <div class="mask" role="presentation" onclick={() => !library.saveTmplBusy && (library.saveTmplOpen = false)}></div>
  <div class="modal" use:dialogGeometry={DIALOG_GEOMETRY.templateSave} role="dialog" aria-modal="true" aria-label="存为模板">
    <header class="sheet-head"><b>存为模板</b><button class="x" onclick={() => (library.saveTmplOpen = false)} disabled={library.saveTmplBusy}>×</button></header>
    <div class="sheet-body">
      <p class="hint">当前项目会被复制成模板（自动去掉密钥、依赖、构建产物），之后可引用它快速建站。</p>
      <input class="tin" bind:value={library.saveTmplName} placeholder="模板名，如 minimal-landing" spellcheck="false" />
      <input class="tin" style="margin-top:7px" bind:value={library.saveTmplDesc} placeholder="一句话描述（可选）" />
      {#if library.saveTmplErr}<div class="err-note">{library.saveTmplErr}</div>{/if}
      <div class="row-end">
        <button class="btn ghost" onclick={() => (library.saveTmplOpen = false)} disabled={library.saveTmplBusy}>取消</button>
        <button class="btn primary" onclick={confirmSaveTmpl} disabled={library.saveTmplBusy || !library.saveTmplName.trim()}>{library.saveTmplBusy ? '保存中…' : '存为模板'}</button>
      </div>
    </div>
  </div>
{/if}

<!-- 用模板建站 -->
{#if library.useTmplOpen}
  <div class="mask" role="presentation" onclick={() => !library.useTmplBusy && (library.useTmplOpen = false)}></div>
  <div class="modal" use:dialogGeometry={DIALOG_GEOMETRY.templateUse} role="dialog" aria-modal="true" aria-label="用模板建站">
    <header class="sheet-head"><div><b>用模板建站</b><small class="dim" style="margin-left:10px">{library.useTmplName}</small></div><button class="x" onclick={() => (library.useTmplOpen = false)} disabled={library.useTmplBusy}>×</button></header>
    <div class="sheet-body">
      {#if !app.isCfConn}<p class="hint warn-text">请先在左下角切到一个 Cloudflare 连接，再用模板建站。</p>{/if}
      <p class="hint">给新项目起个名，模板文件会拷进去，然后在对话里描述你的定制。</p>
      <input class="tin" bind:value={library.useTmplProject} placeholder="新项目名，如 my-landing" spellcheck="false" onkeydown={(e) => e.key === 'Enter' && confirmUseTmpl()} />
      {#if library.useTmplErr}<div class="err-note">{library.useTmplErr}</div>{/if}
      <div class="row-end">
        <button class="btn ghost" onclick={() => (library.useTmplOpen = false)} disabled={library.useTmplBusy}>取消</button>
        <button class="btn primary" onclick={confirmUseTmpl} disabled={library.useTmplBusy || !library.useTmplProject.trim() || !app.isCfConn}>{library.useTmplBusy ? '创建中…' : '创建项目'}</button>
      </div>
    </div>
  </div>
{/if}

<!-- 提示词 新建/编辑 -->
{#if library.promptEditOpen}
  <div class="mask" role="presentation" onclick={() => (library.promptEditOpen = false)}></div>
  <div class="modal wide" use:dialogGeometry={DIALOG_GEOMETRY.promptEditor} role="dialog" aria-modal="true" aria-label={library.promptEditId ? '编辑提示词' : '新增提示词'}>
    <header class="sheet-head"><b>{library.promptEditId ? '编辑提示词' : '新增提示词'}</b><button class="x" onclick={() => (library.promptEditOpen = false)}>×</button></header>
    <div class="sheet-body">
      <input class="tin" bind:value={library.promptEditTitle} placeholder="标题，如 手冲咖啡落地页" spellcheck="false" />
      <textarea class="tin" style="margin-top:7px; min-height:150px; line-height:1.5" bind:value={library.promptEditBody} placeholder="写清楚你要什么样的网站：给谁看、要哪些区块、风格方向，合适的话让它把表单存进 D1，结尾让它先给方案。"></textarea>
      <div class="row-end">
        <button class="btn ghost" onclick={() => (library.promptEditOpen = false)}>取消</button>
        <button class="btn primary" onclick={savePrompt} disabled={!library.promptEditTitle.trim() || !library.promptEditBody.trim()}>保存</button>
      </div>
    </div>
  </div>
{/if}


{#if app.confirmState}
  <ConfirmDialog
    title={app.confirmState.title}
    message={app.confirmState.message}
    kind={app.confirmState.kind}
    confirmText={app.confirmState.confirmText}
    cancelText={app.confirmState.cancelText}
    onConfirm={() => finishConfirm(true)}
    onCancel={() => finishConfirm(false)}
  />
{/if}

<style>
  .pane-close{position:static;flex:none;align-self:center}
  .thread-head.conversation-thread-head{align-items:center}
  .thread-head.multi{padding:7px 16px;min-height:38px;align-items:center}
  .thread-head.multi .th-info{display:block;min-width:0}
  .conversation-head-title{display:flex!important;align-items:center;gap:7px;width:100%;min-width:0}
  .conversation-head-title>span:last-child{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .conversation-head-model{display:inline-flex;align-items:center;flex:none;line-height:0}
  .conversation-head-model :global(.bi){display:block}
  .conversation-site-link{max-width:52%;min-width:0;flex:none}
  .conversation-site-link>span{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .conversation-site-link.static{cursor:default;color:var(--dim)}
  .thread-head .th-info[role="button"]{cursor:grab}
  .pane-error{color:var(--danger,#ad3228);font-size:13px;margin:8px 16px;overflow-wrap:anywhere}
  :global(.pane) .thread{min-height:0;overscroll-behavior:contain}
  :global(.canvas.multi .pane) .thread-inner{padding:18px 20px 8px}
  :global(.canvas.multi .pane) .composer-wrap{padding:8px 16px 14px;max-height:55%;overflow-y:auto}
  :global(.canvas.multi .pane) .composer-bar{flex-wrap:wrap}
  :global(.canvas.multi .pane) .cb-right{margin-left:auto;max-width:100%;flex-wrap:wrap}
  .conversation-drag-ghost{position:fixed;z-index:1000;left:clamp(12px,calc(var(--drag-x) + 14px),calc(100vw - 312px))!important;top:clamp(12px,calc(var(--drag-y) + 12px),calc(100vh - 50px))!important;width:300px;max-width:calc(100vw - 24px);height:38px;box-sizing:border-box;display:flex;align-items:center;gap:8px;padding:0 12px;background:color-mix(in srgb,var(--bg,#fff) 96%,transparent);border:1px solid var(--border,#e7e5df);border-radius:8px;box-shadow:0 8px 24px #2d29231c;pointer-events:none;color:var(--fg,#282723)}
  .conversation-drag-icon{display:inline-flex;align-items:center;line-height:0;flex:none}
  .conversation-drag-title{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px;font-weight:500}
  .single-split-preview{position:absolute;z-index:100;pointer-events:none;box-sizing:border-box;display:grid;place-items:center;background:color-mix(in srgb,var(--bg,#fff) 60%,transparent);backdrop-filter:blur(4px);-webkit-backdrop-filter:blur(4px);border:2px solid #2f80ed;border-radius:8px;box-shadow:0 0 0 1px #2f80ed24}
  .single-split-preview.left{inset:0 50% 0 0}.single-split-preview.right{inset:0 0 0 50%}.single-split-preview.top{inset:0 0 50%}.single-split-preview.bottom{inset:50% 0 0}
  .single-split-preview span{padding:6px 12px;border-radius:18px;background:#2f80ed;color:#fff;font-size:12px;font-weight:500;box-shadow:0 4px 14px #1a5ca733}

  /* 远程工作台：终端主区 + 底部对话 + 右侧文件（VS Code 那套）。
     每层都 min-*:0 —— flex 子项默认 min-size:auto，不清零的话终端撑着不缩、面板挤不出来。 */
  .wb { flex: 1; min-height: 0; display: flex; }
  .wb-main { flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  /* 文件栏单开：主区收起（终端只藏不拆，DOM 还在），让文件栏独占整宽。 */
  .wb-main.collapsed { display: none; }
  /* 终端与对话的接缝不再用 border-top 那条黑白硬边，改由上面 .term-wrap.with-chat 的圆底+柔和阴影收口 */
  .wb-chat { flex: none; min-height: 0; display: flex; flex-direction: column; background: var(--bg); }
  .wb-chat.solo { flex: 1; }
  /* 起点输入框：贴着面板底（justify-content:flex-end），面板矮时也不顶头。
     ★ .composer 必须显式 width:100% —— 它自带 `margin:0 auto`，而这里是 flex 列容器，
     交叉轴上的 auto margin 会**压制 stretch**，不给宽度它就缩成 textarea 的默认宽（~192px）。
     对话页那边的 .composer-wrap 是普通块容器，没这问题。 */
  .wb-start { flex: 1; min-height: 0; overflow-y: auto; display: flex; flex-direction: column; justify-content: flex-end; gap: 6px; }
  .wb-start .composer { width: 100%; }
  .wb-start .hint { width: 100%; max-width: 760px; margin: 0 auto; }
  /* 分隔线：命中区比看到的粗，好抓 */
  .wb-split { flex: none; background: transparent; z-index: 2; }
  .wb-split.h { height: 5px; margin-bottom: -5px; cursor: row-resize; }
  .wb-split.v { width: 5px; margin-right: -5px; cursor: col-resize; }
  .wb-split:hover { background: var(--accent-soft); }
  /* 面板开关（头部两枚 VS Code 式图标）：不给底，开关状态只用图标本身的深浅表示 */
  .wb-tg { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: 0; background: transparent; color: var(--faint); cursor: pointer; -webkit-app-region: no-drag; }
  .wb-tg:hover { color: var(--dim); }
  .wb-tg.on { color: var(--text); }
  .wb-tg.on:hover { color: var(--accent-h); }
  /* 工作台头部：单行，比常规 thread-head 矮一截。
     ★ margin-top:0 是必须的 —— `.th-info small` 那 2px 是给它上面那行标题留的空隙，
     而这里标题已经去掉了，留着就是净偏移，文字会比右边的图标低 2px（对不上一条水平线）。 */
  /* 高度对齐左上角那排工具图标：`.win-tools` 是 fixed/top:0/height:30px，中心在 15px。
     这里最高的子元素是 24px 的面板开关 → 上下各留 3px 正好 30px，中心也落在 15px，
     于是「地址行 / 刷新键 / 三枚开关 / 折叠侧栏 / 搜索」全在同一条水平线上。 */
  /* 这条**必须自己写回 align-items:center**：上面 .thread-head 改成了 flex-start（为了让标题行
     对齐 15px 的图标带），而 slim 是靠 center 把「地址行 / 刷新键 / 三枚开关」这些不等高的子元素
     一起压到 15px 那条线上的 —— 被 flex-start 带走就全顶到 3px 去了。 */
  .thread-head.slim { padding-top: 3px; padding-bottom: 3px; align-items: center; container-type: inline-size; }
  /* 选择器要带上 .th-info small，否则压不过 `.th-info small` 那条（它带元素选择器，分更高）——
     实测：只写 .rhead-line 的话 margin-top 仍是 2px，文字比图标低 1px。 */
  .th-info small.rhead-line { display: flex; align-items: center; gap: 5px; margin-top: 0; }
  /* 终端：自己管滚动，容器不能给 overflow-y:auto（会和 xterm 打架）。 */
  .term-wrap { flex: 1; min-height: 0; overflow: hidden; background: #1c1917; padding: 8px 10px; }
  /* 关掉命令行＝只藏不拆：DOM 留着，xterm 和 SSH 会话都不掉，开回来立刻还在。 */
  .term-wrap.hid { display: none; }
  /* 「正在连接」浮层：盖在终端上，连上即消失，不进终端缓冲（所以不会留下擦不掉的一行）。 */
  .term-wrap { position: relative; }
  /* 终端**在对话框上方**时（with-chat）把接缝做软：一道朝下的柔和阴影落在对话面板上，
     替掉原来黑白直接相撞的硬边（不收圆角——直角保持齐整）。阴影几乎只朝下（负 spread 收掉左右溢出，
     正 offset 让顶边不糊）。z-index 让阴影压在对话面板之上（对话是后面的兄弟，默认会盖住它）。
     终端独占/无对话时不加，保持齐边。 */
  .term-wrap.with-chat { z-index: 1; box-shadow: 0 6px 12px -6px rgba(20, 16, 13, .4); }
  .term-connecting { position: absolute; left: 14px; top: 12px; display: flex; align-items: center; gap: 7px; color: #8b857c; font-size: 12px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; pointer-events: none; }
  .term-credential-repair { position: absolute; left: 14px; top: 12px; border: 1px solid rgba(248, 113, 113, .35); border-radius: 7px; background: rgba(127, 29, 29, .3); color: #fca5a5; padding: 6px 9px; font: 12px ui-monospace, SFMono-Regular, Menlo, monospace; cursor: pointer; }
  .term-credential-repair:hover { background: rgba(153, 27, 27, .42); color: #fecaca; }
  .tc-spin { width: 9px; height: 9px; border: 1.5px solid #4b4640; border-top-color: #8b857c; border-radius: 50%; animation: spin .7s linear infinite; }
  .term { width: 100%; height: 100%; }
  /* 远程视图头部：终端/文件 页签 */
  .rhead-acts { display: flex; align-items: center; gap: 10px; -webkit-app-region: no-drag; }
  /* 远程文件面板（工作台右栏；宽度由 wb.filesW 内联给） */
  .files-wrap { position: relative; flex: none; min-height: 0; display: flex; flex-direction: column; border-left: 1px solid var(--border); background: var(--bg); }
  .files-drop-overlay { position: absolute; inset: auto 8px 8px; z-index: 40; display: flex; align-items: center; gap: 8px; padding: 8px 10px; border: 1px solid var(--border2); border-radius: 6px; background: var(--bg); color: var(--text); pointer-events: none; box-shadow: 0 2px 8px #0001; }
  .files-drop-overlay b { flex: none; font-size: 12px; font-weight: 500; }
  .files-drop-overlay small { min-width: 0; overflow: hidden; color: var(--dim); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  /* 单开时吃满整宽（撤掉固定宽 flex:none）；左边没东西了，左边框也去掉。 */
  .files-wrap.solo { flex: 1; min-width: 0; border-left: 0; }
  .files-bar { flex: none; display: flex; align-items: center; gap: 6px; padding: 10px 16px; border-bottom: 1px solid var(--border); }
  .fpath { flex: 1; min-width: 0; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; }
  /* 面包屑：不画框（它是一行路径，不是输入框——要输入点空白处会切成真的 input），
     撑满路径栏，路径深了横向滚动 */
  .fcrumbs { flex: 1; min-width: 0; display: flex; align-items: center; gap: 1px; overflow-x: auto; overflow-y: hidden; scrollbar-width: none; cursor: text; padding: 4px 2px; }
  .fcrumbs::-webkit-scrollbar { display: none; }
  .fcrumb { flex: none; border: 0; background: transparent; color: var(--dim); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; padding: 2px 5px; border-radius: 5px; cursor: pointer; white-space: nowrap; }
  .fcrumb:hover:not(:disabled) { background: var(--accent-soft); color: var(--text); }
  .fcrumb.cur { color: var(--text); font-weight: 500; }
  .fcrumb-sep { flex: none; color: var(--faint); font-size: 11px; user-select: none; }
  /* 面包屑尾巴的空白＝一个撑满剩余宽度的按钮（点它进输入模式）。min-width 保证路径很长时也还点得着。 */
  .fcrumb-blank { flex: 1; min-width: 28px; align-self: stretch; border: 0; background: transparent; cursor: text; border-radius: 5px; }
  .fcrumb-blank:focus-visible { outline: 2px solid var(--accent-soft); outline-offset: -2px; }
  .fbtn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; flex: none; border: 1px solid var(--border); background: var(--bg); color: var(--dim); border-radius: 6px; cursor: pointer; }
  .fbtn :global(svg) { width: 13px; height: 13px; }
  .fbtn:hover:not(:disabled) { color: var(--text); border-color: var(--border2); }
  .fbtn:disabled { opacity: .45; cursor: default; }
  .fbtn.sm { width: 22px; height: 22px; border: 0; border-radius: 5px; }
  .fbtn.sm:hover:not(:disabled) { background: var(--accent-soft); }
  .fbtn.danger:hover:not(:disabled) { color: var(--err); background: var(--err-soft); }
  /* 现在是个重试按钮：清掉按钮默认样式，只留 err-note 的红底红字 + 左对齐 + 手型 */
  .files-err { flex: none; margin: 10px 16px 0; display: block; width: calc(100% - 32px); text-align: left; font: inherit; font-size: 13px; cursor: pointer; }
  .files-err:hover { background: var(--err-border); }
  /* 列表＝四列表格（名称/权限/大小/日期），行内缩进出树形。列宽用同一套变量对齐表头与行。
     ★ 窄面板里按容器宽度砍列：右栏默认才 380px，四列定宽合计就 300px，名称会被挤成 0
     （文件名全没了，只剩权限/大小/日期——那就本末倒置了）。名称永远优先。
     用容器查询而不是媒体查询：这里要看的是**面板自己的宽度**，不是窗口宽度（面板还能拖）。 */
  .files-wrap { --fw-perm: 96px; --fw-size: 76px; --fw-date: 128px; container-type: inline-size; }
  .fhead { flex: none; display: flex; align-items: stretch; gap: 0; padding: 0 14px; border-bottom: 1px solid var(--border); background: var(--rail); }
  .fh { position: relative; display: flex; align-items: center; gap: 3px; border: 0; background: transparent; color: var(--dim); font-size: 11.5px; padding: 5px 6px; text-align: left; }
  .fhead button.fh { cursor: pointer; }
  .fhead button.fh:hover { color: var(--text); }
  .fh + .fh { border-left: 1px solid var(--border); }
  /* 列宽手柄：骑在列头分隔线上，比线宽一点好抓 */
  .fh-grip { position: absolute; left: -4px; top: 0; bottom: 0; width: 8px; padding: 0; border: 0; background: transparent; cursor: col-resize; z-index: 1; }
  .fh-grip:hover { background: linear-gradient(to right, transparent 2px, var(--border2) 2px, var(--border2) 6px, transparent 6px); }
  .fh-grip:focus-visible { outline: 2px solid var(--accent-soft); outline-offset: -2px; }
  .fh-name { flex: 1; min-width: 0; }
  .fh-perm { width: var(--fw-perm); flex: none; }
  .fh-size { width: var(--fw-size); flex: none; justify-content: flex-end; }
  .fh-date { width: var(--fw-date); flex: none; }
  .fh-ar { font-size: 10px; line-height: 1; }
  .fh-ar.desc { transform: rotate(180deg); }
  .files-list { flex: 1; min-height: 0; overflow-y: auto; padding: 2px 6px 14px; }
  .files-empty { padding: 28px 0; text-align: center; color: var(--faint); font-size: 13px; }
  .frow { display: flex; align-items: center; padding: 0 8px; border-radius: 6px; cursor: default; user-select: none; }
  .frow:hover { background: var(--rail); }
  .frow.on { background: var(--accent-soft); }
  .frow.drop-target { background: var(--accent-soft); outline: 1px solid var(--border2); outline-offset: -1px; }
  .frow.deleting, .frow.deleting:hover, .frow.deleting.on { background: var(--rail); opacity: .52; cursor: progress; }
  .fname { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; color: var(--text); font-size: 13px; padding: 5px 0; }
  .fname-t { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; border: 0; padding: 0; background: transparent; color: inherit; font: inherit; text-align: left; cursor: default; }
  .file-rename { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 3px; }
  .file-rename input { min-width: 0; width: 100%; box-sizing: border-box; border: 1px solid var(--border2); border-radius: 3px; padding: 1px 3px; background: var(--bg, white); color: var(--text); font: inherit; }
  .file-rename input:focus { outline: 1px solid var(--dim); }
  .file-rename small { color: #c53b32; font-size: 11px; white-space: normal; }
  .fdelete-state { flex: none; display: inline-flex; align-items: center; gap: 4px; color: var(--dim); font-size: 11.5px; white-space: nowrap; }
  .fdelete-spin { width: 9px; height: 9px; border: 1.4px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: rspin .7s linear infinite; }
  .fchev, .fchev-sp { width: 14px; height: 14px; flex: none; }
  .fchev { display: inline-flex; align-items: center; justify-content: center; border: 0; background: transparent; color: var(--faint); border-radius: 4px; cursor: pointer; padding: 0; transition: transform .12s; }
  .fchev:hover { color: var(--text); }
  .fchev.open { transform: rotate(90deg); }
  .fspin { width: 8px; height: 8px; border: 1.4px solid var(--border2); border-top-color: var(--dim); border-radius: 50%; animation: spin .7s linear infinite; }
  .fic { flex: none; color: var(--faint); }
  .fic.dir { color: #4a90d9; }
  .fic.link { color: #d98a2b; }
  .fperm { width: var(--fw-perm); flex: none; padding: 0 6px; color: var(--dim); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; }
  .fsize { width: var(--fw-size); flex: none; padding: 0 6px; text-align: right; color: var(--dim); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .fdate { width: var(--fw-date); flex: none; padding: 0 6px; color: var(--dim); font-size: 11.5px; font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  /* ★ 砍列规则必须放在列样式**之后**：@container 不加优先级，和 `.fh{display:flex}` 同分，
     写在前面会被它盖掉（实测过：380px 下权限列照样 96px）。 */
  @container (max-width: 560px) { .fh-perm, .fperm { display: none; } }
  @container (max-width: 400px) { .fh-date, .fdate { display: none; } }
  @container (max-width: 300px) { .fh-size, .fsize { display: none; } }
  /* 远程文件右键菜单：复用全局 .ctx-menu/.ctx-item 的样式，只补两处差异 */
  .fctx { min-width: 172px; }
  .fctx .ctx-item { justify-content: flex-start; }
  .ctx-item.danger { color: var(--err); }
  .ctx-item.danger:hover:not(:disabled) { background: var(--err-soft); }
  /* 在线编辑器 */
  .modal.ed-modal { width: min(1080px, 94vw); height: min(760px, 88vh); }
  .ed-modal.ed-full { inset: 38px 8px 8px; width: auto; height: auto; max-width: none; max-height: none; margin: 0; }
  .ed-head { gap: 10px; }
  .ed-heading { min-width: 0; flex: 1; display: grid; gap: 4px; }
  .ed-heading > div { display: flex; align-items: center; gap: 10px; }
  .ed-badge { color: var(--dim); background: var(--rail); border-radius: 5px; padding: 2px 6px; font-size: 10px; }
  .ed-badge.dirty { color: var(--accent); background: var(--accent-soft); }
  .ed-expand { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 30px; height: 30px; padding: 0; border: 0; border-radius: 5px; background: transparent; box-shadow: none; color: var(--dim); cursor: pointer; }
  .ed-expand:hover { background: var(--rail); color: var(--text); }
  .ed-expand:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .ed-path { display: block; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sheet-body.ed-body { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; gap: 0; overflow: hidden; padding: 0; }
  .ed-body > .err-note { margin: 10px 14px; max-height: 100px; overflow: auto; flex: none; }
  .ed-save-note { margin-right: auto; color: var(--dim); font-size: 11px; }
  .ed-footer { position: relative; z-index: 1; flex: none; display: flex; justify-content: flex-end; align-items: center; gap: 7px; padding: 10px 18px 14px; border-top: 1px solid var(--border); background: var(--bg); }
  /* 远端系统信息（页脚可点＝重新检测；下拉/设置里只读） */
  .foot-os, .cs-os { display: inline-flex; align-items: center; gap: 4px; min-width: 0; }
  .foot-os { cursor: pointer; border-radius: 5px; padding: 0 3px; margin-left: -3px; }
  .foot-os:hover { background: var(--accent-soft); color: var(--text); }
  .foot-os :global(svg.distro) { transform: translateY(1px); }
  .cs-os { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* ★ 上移 1px 不是玄学：align-items:center 对齐的是**盒子**，而文字行盒底下有一截没人用的
     降部空间（Ubuntu/22.04 这类没有 g/p/y），于是「看得见的字」整体偏上、图标按盒居中就显得低。
     发行版图标是纯圆/纯环，没有基线可言，只能按视觉中线手动补这 1px。 */
  :global(svg.distro) { flex: none; transform: translateY(-1px); }
  /* 头部的「重新连接」：只留图标，紧跟在连接状态后面 */
  .th-rfz { display: inline-flex; align-items: center; justify-content: center; width: 15px; height: 15px; border: 0; background: transparent; color: var(--faint); cursor: pointer; padding: 0; -webkit-app-region: no-drag; }
  .th-rfz :global(svg) { width: 12px; height: 12px; }
  .th-rfz:disabled { cursor: default; color: var(--dim); }
  /* 顶栏的远端负载：三个小读数。
     ★ 做得很紧（不画迷你条）是有原因的：应用最小窗口 820px、侧栏 240 → 头部只剩 532px，
     地址+刷新(215) + 三枚开关(84) + 间距(20) 已占 319，留给负载的只有 213px。
     带条的版本要 290px —— 于是在最小窗口下永远塞不下。 */
  /* 分隔线用伪元素画：直接给容器 border-left 会顶满整行高，太抢眼；这里只画 11px 的一小截。 */
  .hstats { position: relative; display: flex; align-items: center; gap: 9px; margin-left: 5px; padding-left: 10px; }
  .hstats::before { content: ''; position: absolute; left: 0; top: 50%; transform: translateY(-50%); width: 1px; height: 11px; background: var(--border2); }
  .hstat { display: inline-flex; align-items: center; gap: 3px; font-size: 11px; color: var(--dim); white-space: nowrap; cursor: default; }
  .hstat-l { color: var(--faint); }
  .hstat-v { font-variant-numeric: tabular-nums; }
  .hstat.warn { color: var(--warn); font-weight: 500; }
  .hstat.hot { color: var(--err); font-weight: 500; }
  /* 真挤不下了才收起（比如侧栏被拖到很宽）。看的是**头部自己的宽度**，不是窗口宽 ——
     侧栏可拖 190~460，窗口宽说明不了头部还剩多少。
     ★ 必须写在 .hstats 之后：@container 不加优先级，同分靠后取胜（上次在文件列表踩过）。 */
  @container (max-width: 470px) { .hstats { display: none; } }
  .th-rfz:hover { color: var(--text); }
  /* 启动页：远程连接的目标机器（占站点选择器的位子——那台机器就是本次对话的对象） */
  .ssh-target { display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px; border: 1px solid var(--border); border-radius: 999px; background: var(--rail); color: var(--dim); font-size: 12px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  .ssh-target :global(svg) { color: var(--faint); flex: none; }
  .ssh-target :global(svg.distro) { transform: translateY(1px); }
  .workspace-picker { min-width: 0; display: inline-flex; align-items: center; gap: 2px; }
  .workspace-folder { min-width: 0; max-width: 210px; height: 27px; display: inline-flex; align-items: center; gap: 6px; padding: 0 9px; border: 1px solid var(--border); border-radius: 999px; background: var(--rail); color: var(--dim); font: inherit; font-size: 11.5px; cursor: pointer; }
  .workspace-folder:hover { background: #f4f2ed; color: var(--text); }
  .workspace-folder.selected { color: var(--text); background: #f7f6f2; }
  .workspace-folder svg { flex: none; color: var(--faint); }
  .workspace-folder span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .workspace-clear { width: 23px; height: 23px; padding: 0; border: 0; border-radius: 50%; background: transparent; color: var(--faint); font-size: 15px; line-height: 1; cursor: pointer; }
  .workspace-clear:hover { background: #eeeae3; color: var(--text); }
  .ssh-key-row { display: flex; gap: 6px; align-items: center; }
  .ssh-key-row .tin { flex: 1; min-width: 0; }
  .ssh-fp { display: flex; flex-direction: column; gap: 5px; padding: 9px 11px; border: 1px solid #cfc9ec; background: #f7f6ff; border-radius: 10px; }
  .ssh-fp b { font-size: 12.5px; }
  .ssh-fp > code { font-family: ui-monospace, monospace; font-size: 11.5px; color: var(--text); background: #fff; border: 1px solid var(--border); border-radius: 6px; padding: 4px 7px; overflow-wrap: anywhere; user-select: text; }
  .ssh-fp small { color: var(--dim); font-size: 11px; line-height: 1.5; }
  .ssh-fp small code { font-family: ui-monospace, monospace; font-size: 10.5px; background: #fff; border-radius: 4px; padding: 1px 4px; user-select: text; }
  :global(:root) {
    --bg: #ffffff; --rail: #faf9f7; --card: #ffffff;
    /* 浮层实底（更新吐司/附件 chip 等用）：此前未定义导致 var(--panel) 落空、浮层透明。 */
    --panel: #ffffff;
    --border: #ecebe6; --border2: #e1dfd8;
    --text: #26241f; --dim: #6f6b62; --faint: #a29d93;
    /* 强调色走暖黑/灰（Codex 式安静路线），不用蓝紫；风险色（红/琥珀）另有专用变量。 */
    --accent: #33302a; --accent-h: #1d1b17; --accent-soft: #edece7;
    --user-bg: #f3f1ec;
    --ok: #12805c; --warn: #b45309; --err: #dc2626;
    --err-soft: #fef2f2; --err-border: #f4cccc;
    --shadow-sm: 0 1px 2px rgba(30,25,15,.05);
    --shadow: 0 4px 16px rgba(30,25,15,.08);
    --shadow-lg: 0 16px 48px rgba(30,25,15,.16);
    /* 单行表单控件统一高度 token：.tin(单行)/.dd-trigger/.tfield-fx 壳全部吃它，别再靠 padding 推算。 */
    --ctl-h: 30px;
    /* composer 底栏 chip 统一高度 token：站点/权限（Dropdown compact）、模型（ModelFx 触发器）、
       用量圆环按钮全部定高——chip 内容各异（17px favicon/「低」徽章/纯文字），靠 padding 撑高必然参差。 */
    --chip-h: 24px;
  }
  :global(html, body) { margin: 0; height: 100%; background: var(--bg); color: var(--text);
    font: 15px/1.65 -apple-system, 'PingFang SC', 'Segoe UI', 'Helvetica Neue', sans-serif; -webkit-font-smoothing: antialiased; }
  :global(*) { box-sizing: border-box; }
  /* 去掉原生按钮外观（避免 WKWebView 给 <button> 画默认边框/底纹，即那个「黑框」）。 */
  :global(button) { -webkit-appearance: none; appearance: none; font: inherit; }
  /* 去掉点击时 WKWebView 的黑色焦点框；键盘可达性用 hover/active 态足够（桌面鼠标为主）。 */
  :global(button:focus), :global(button:focus-visible), :global([role='button']:focus),
  :global([role='button']:focus-visible), :global(summary:focus), :global(a:focus) { outline: none; box-shadow: none; }
  .app { display: flex; height: 100vh; overflow: hidden; }
  /* Windows：主内容左上角轻微圆角，跟原生标题栏之间更柔和（角落露出 rail 色）。macOS 无原生标题栏不需要。
     侧栏分隔线在 Windows 去掉——通高的 border-right 会在圆角上方留一截两侧同色的「悬空线头」；
     边界靠 rail/main 底色对比 + 圆角呈现（Codex 客户端同款做法）。 */
  .app.win { background: var(--rail); }
  /* 圆角回归：当初去掉它是因为页头有暖底、和 DWM 窗框几乎同色，圆角单独露成一道台阶；
     暖底已拿掉（0.2.22），.main 的白又直接顶到窗沿——12px 圆角恢复「白面板嵌在 rail 色
     窗框里」的收边（用户实测：背景拿掉后这里正常了）。
     顶栏左缘那条竖线也随之取消：白/rail 色差已经把边界撑起来了，再画线反而多余。 */
  .app.win .main { background: var(--bg); border-top-left-radius: 12px; }
  .app.win .rail { border-right: none; }

  /* 细滚动条（macOS overlay 风格）：细、圆角、透明轨道，thumb 用 padding-box 内缩显得更细。 */
  :global(::-webkit-scrollbar) { width: 10px; height: 10px; }
  :global(::-webkit-scrollbar-track) { background: transparent; }
  :global(::-webkit-scrollbar-thumb) { background-color: rgba(60, 54, 44, .22); border-radius: 8px; border: 3px solid transparent; background-clip: padding-box; }
  :global(::-webkit-scrollbar-thumb:hover) { background-color: rgba(60, 54, 44, .4); border: 2px solid transparent; background-clip: padding-box; }
  :global(::-webkit-scrollbar-corner) { background: transparent; }

  /* 融合标题栏：全宽透明拖拽条，红绿灯浮在其上，两列各自的底色透出来 */
  /* 拖拽条只盖住左栏顶部（红绿灯+工具区）；主区域顶部改由各页头/启动页自身承担拖拽，
     这样内容能贴近顶部又不被拖拽条挡住按钮。 */
  .titlebar { position: fixed; top: 0; left: 0; height: 30px; z-index: 6; }
  /* 顶部工具按钮：浮在拖拽条之上、紧挨红绿灯右侧。 */
  .win-tools { position: fixed; top: 0; left: 80px; height: 30px; display: flex; align-items: center; gap: 1px; z-index: 8; }
  /* 全屏无红绿灯：左移到与左栏菜单图标同一左边界（rail-head 8px + 按钮 padding 让开）。 */
  .win-tools.fs { left: 12px; }
  /* Windows 无红绿灯：靠左边缘，不飘在中间。 */
  .win-tools.win { left: 12px; }
  .wt { display: inline-flex; align-items: center; justify-content: center; width: 27px; height: 24px; border: none; background: none; border-radius: 6px; color: var(--dim); cursor: pointer; -webkit-app-region: no-drag; }
  .wt:hover { background: rgba(0, 0, 0, .06); color: var(--text); }
  .wt:disabled { opacity: .4; cursor: default; }
  .wt:disabled:hover { background: none; color: var(--dim); }
  /* 待更新图标：锚在侧栏右上角（跟随 rail 右边界），静默检查发现新版时出现。 */
  /* 待更新指示：并入 win-tools，样式同折叠/搜索（无底、灰色 + 悬停高亮），仅右上角一个柔和强调色小点。 */
  .wt.upd { position: relative; }
  .wt.upd.active { background: rgba(0, 0, 0, .07); color: var(--text); }
  .wt.upd:disabled { opacity: .55; }
  .upd-count { position: absolute; top: -1px; right: -3px; min-width: 12px; height: 12px; padding: 0 3px; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; border: 1.5px solid var(--rail); border-radius: 999px; background: var(--accent); color: #fff; font-size: 7.5px; font-weight: 750; line-height: 1; }
  .upd-spin { animation: upd-spin .7s linear infinite; transform-origin: center; }
  @keyframes upd-spin { to { transform: rotate(360deg); } }
  /* 更新进度小药丸：点更新后顶部居中显示 状态文字 + 进度条，直到重启。 */
  .upd-toast { position: fixed; top: 38px; left: 50%; transform: translateX(-50%); z-index: 200; display: flex; align-items: center; gap: 8px; padding: 5px 12px; background: var(--panel); border: 1px solid var(--border); border-radius: 999px; box-shadow: var(--shadow-lg); font-size: 12px; color: var(--text); -webkit-app-region: no-drag; }
  .upd-toast .upd-spin { color: var(--accent); flex: none; }
  .upd-toast-msg { white-space: nowrap; }
  /* 下载进度条：实底浅灰轨道 + 主题色填充（容器 --panel 已实底，文字对比度由 --text/白底保证） */
  .upd-toast-bar { width: 84px; height: 5px; border-radius: 3px; background: var(--border2); overflow: hidden; flex: none; }
  .upd-toast-bar > span { display: block; height: 100%; background: var(--accent); border-radius: 3px; transition: width .2s ease; }

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
  .railnav svg { flex: none; }
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
  /* 线程头右上角：站点域名 + ↗，点开站点 */
  .th-open { flex: none; display: inline-flex; align-items: center; gap: 4px; background: none; border: none; padding: 4px 6px; border-radius: 7px; color: var(--accent); font: inherit; font-size: 12.5px; cursor: pointer; }
  .th-open:hover { background: #f1efe9; }
  .th-open:disabled { opacity: .5; cursor: wait; }
  .th-open svg { flex: none; }
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

  .rail-foot { width: 100%; display: flex; align-items: center; gap: 8px; padding: 8px 12px; border: none; border-top: 1px solid var(--border); background: none; cursor: pointer; text-align: left; -webkit-appearance: none; appearance: none; box-shadow: none; }
  .rail-foot:focus, .rail-foot:active { outline: none; box-shadow: none; }
  .rail-foot:hover { background: #f1efe9; }
  .rail-foot.open { background: #f1efe9; border-top-color: transparent; }
  .foot-main { flex: 1; min-width: 0; }
  .foot-main b { display: block; font-size: 12.5px; line-height: 1.15; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .foot-main small { display: block; color: var(--dim); font-size: 10.5px; line-height: 1.1; }
  .foot-chev { color: var(--faint); flex: none; transition: transform .15s; }
  .foot-chev.up { transform: rotate(180deg); }

  .foot-wrap { position: relative; }
  .conn-switch {
    position: absolute; left: 8px; right: 8px; bottom: calc(100% + 4px); z-index: 40;
    background: #fff; border: 1px solid var(--border2); border-radius: 12px;
    box-shadow: 0 10px 30px rgba(30,25,15,.15); padding: 5px;
    display: flex; flex-direction: column; max-height: min(640px, calc(100vh - 86px)); overflow: hidden;
    animation: pop .1s ease-out;
  }
  .cs-scroll {
    min-height: 0; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable;
  }
  .cs-actions { flex: none; }
  .cs-item {
    width: 100%; display: flex; align-items: center; gap: 9px;
    background: none; border: none; border-radius: 8px; padding: 5.5px 9px; cursor: pointer; text-align: left; font: inherit;
    color: var(--text);
  }
  .cs-item:hover { background: #f4f3ef; }
  .cs-item.on { background: #efeee9; }
  .cs-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .cs-main b { font-weight: 500; font-size: 13px; line-height: 1.2; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* 连接副标题（Key 前缀 · Cloudflare / Ubuntu … LTS 之类）：更小更淡，让连接名更突出 */
  .cs-main small {
    display: block;
    width: 100%;
    min-width: 0;
    color: var(--faint);
    font-size: 10px;
    line-height: 1.25;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cs-main small.cs-os {
    display: flex;
    align-items: center;
  }
  .cs-main small.cs-os :global(svg.distro) {
    transform: translateY(1px);
  }
  .cs-os-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cs-check { color: var(--accent); flex: none; font-size: 13px; }
  .cs-div { height: 1px; background: var(--border); margin: 3.5px 4px; }
  /* 动作行是单行：比原来(7px)收，但别收到发挤——padding 5.5px + line-height 1.3 落在约 28px，
     比原来 37px 紧一截、又留了呼吸感（23px 太挤，实测反馈）。连接行是两行，另按 .cs-item 定高。 */
  .cs-act {
    width: 100%; display: flex; align-items: center; gap: 8px;
    background: none; border: none; border-radius: 8px; padding: 5.5px 9px; cursor: pointer; text-align: left;
    font: inherit; font-size: 12.5px; line-height: 1.3; color: var(--dim);
  }
  .cs-act:hover { background: #f4f3ef; color: var(--text); }
  .cs-act.on { background: #efeee9; color: var(--text); font-weight: 550; }
  .cs-act :global(svg) { color: var(--faint); flex: none; }
  .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; }
  .dot.ok { background: #16a34a; } .dot.warn { background: #d97706; } .dot.off { background: #cfccc4; }

  /* ---- 主区 ---- */
  .main { flex: 1; position: relative; display: flex; flex-direction: column; min-width: 0; padding-top: 0; }
  /* 吐司：pre-wrap 让「——详情——」多行错误可读；err 形态限高可滚动（安装失败输出末尾等） */
  .flash { position: fixed; top: 40px; left: 50%; transform: translateX(-50%); z-index: 1200; background: #14231a; color: #fff; padding: 9px 16px; border-radius: 10px; font-size: 13px; box-shadow: var(--shadow); max-width: 70%; white-space: pre-wrap; overflow-wrap: anywhere; -webkit-app-region: no-drag; }
  .flash.err { max-height: 42vh; overflow-x: hidden; overflow-y: auto; }
  /* 自定义右键菜单（替换 WKWebView 默认英文菜单） */
  .ctx-menu { position: fixed; z-index: 120; min-width: 148px; max-width: calc(100vw - 16px); max-height: calc(100vh - 16px); overflow-x: hidden; overflow-y: auto; background: #fff; border: 1px solid var(--border); border-radius: 11px; box-shadow: 0 12px 32px rgba(30,25,15,.16); padding: 5px; animation: pop .1s ease-out; }
  .conn-ctx-menu { visibility: hidden; }
  .conn-ctx-menu.ready { visibility: visible; }
  .ctx-item { width: 100%; display: flex; align-items: center; justify-content: space-between; gap: 18px; background: none; border: none; border-radius: 7px; padding: 6px 10px; font: inherit; font-size: 13px; color: var(--text); cursor: pointer; text-align: left; }
  .ctx-item:hover:not(:disabled) { background: #f1efe9; }
  .ctx-item:disabled { color: var(--faint); cursor: default; }
  .ctx-div { height: 1px; background: var(--border); margin: 4px 6px; }
  .imgtip { position: fixed; z-index: 130; transform: translate(-50%, -100%); padding: 5px; border-radius: 10px; background: #fff; border: 1px solid rgba(0, 0, 0, .1); box-shadow: 0 8px 24px rgba(0, 0, 0, .18); pointer-events: none; visibility: hidden; }
  .imgtip.ready { visibility: visible; animation: tipin .12s ease-out both; }
  .imgtip.below { transform: translate(-50%, 0); }
  /* min-* + contain：极端长宽比（整页截图/超宽横幅）装进保底盒子里，不再缩成一条细缝 */
  .imgtip img { display: block; max-width: 320px; max-height: 240px; min-width: 140px; min-height: 90px; object-fit: contain; border-radius: 6px; background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 14px 14px; }
  .lightbox { position: fixed; inset: 0; z-index: 140; background: rgba(24, 22, 18, .72); display: flex; align-items: center; justify-content: center; cursor: zoom-out; }
  .lightbox img { max-width: 86vw; max-height: 86vh; border-radius: 12px; background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 16px 16px; box-shadow: 0 24px 64px rgba(0, 0, 0, .4); }
  @keyframes tipin { from { opacity: 0; } to { opacity: 1; } }
  .tipwrap { display: inline-flex; }
  .flash.err { background: var(--err); }

  /* safe center：内容比可视区高时退回顶对齐可滚动，避免居中把顶部裁掉。 */
  .center { flex: 1; display: flex; align-items: safe center; justify-content: center; overflow-y: auto; padding: 24px; }
  /* 启动页：大屏不无限拉宽，入口区与下方的新站引导共用同一条 920px 内容基线。 */
  .center.launch-center { align-items: safe center; justify-content: flex-start; padding: 24px 40px; }
  .center.launch-center .launcher { width: min(920px, 100%); margin-inline: auto; }
  .hero-card { text-align: center; max-width: 460px; }
  .hero-mark { display: flex; justify-content: center; margin-bottom: 6px; }
  .hero-card h1 { font-size: 22px; margin: 12px 0 8px; }
  .hero-card p { color: var(--dim); margin: 0 0 18px; }
  .cli-guide { margin-top: 22px; text-align: left; border-top: 1px solid var(--border); padding-top: 14px; }
  .cli-guide-h { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 11px; letter-spacing: .03em; text-transform: uppercase; color: var(--faint); font-weight: 600; margin-bottom: 6px; }
  .cli-redetect { display: inline-flex; align-items: center; gap: 4px; flex: none; background: none; border: none; padding: 2px 6px; border-radius: 6px; cursor: pointer; color: var(--dim); font: inherit; font-size: 11px; font-weight: 500; letter-spacing: 0; text-transform: none; }
  .cli-redetect:hover { background: rgba(0, 0, 0, .06); color: var(--text); }
  .cli-redetect:disabled { opacity: .55; cursor: default; }
  .cli-redetect :global(svg) { width: 12px; height: 12px; }
  .cli-row { display: flex; align-items: center; gap: 8px; padding: 3px 0; font-size: 13px; }
  .cli-name { width: 100px; flex: none; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .cli-copy { flex: none; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 5px; color: var(--faint); cursor: pointer; }
  .cli-copy:hover { background: rgba(0, 0, 0, .06); color: var(--text); }
  .cli-tag { flex: none; font-size: 11px; padding: 1px 7px; border-radius: 6px; font-weight: 600; }
  .cli-tag.ok { color: #1a7f4b; background: #e7f4ec; }
  .cli-tag.warn { color: #9a6a00; background: #f7efd9; }
  .cli-tag.bad { color: #9a3b2f; background: #f6e7e3; }
  .cli-cmd { flex: 0 1 auto; min-width: 0; font-family: ui-monospace, monospace; font-size: 10.5px; color: var(--faint); background: #f6f5f1; padding: 3px 8px; border-radius: 5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; user-select: text; cursor: text; }
  /* 提高优先级压过 .hero-card p（后者 margin/color 更 specific，会把 top margin 顶成 0）。 */
  .cli-guide .cli-note { display: flex; align-items: center; gap: 5px; font-size: 11.5px; line-height: 1.5; color: var(--faint); margin: 12px 0 0; }
  .cli-note-ic { flex: none; color: var(--faint); }

  .launcher { width: min(680px, 100%); }
  .launcher h1 { font-size: 26px; margin: 0 0 6px; letter-spacing: -.01em; }
  .launcher .sub { color: var(--dim); margin: 0 0 22px; }








  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  .tin, textarea { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1.5px solid var(--border2); border-radius: 8px; padding: 5px 10px; }
  /* 单行输入（text/password/number/datetime-local）：显式统一高 --ctl-h，内容垂直居中；
     去掉竖向 padding 防 macOS 原生步进器/日期控件把高度顶开。textarea 不定高，不吃这条。 */
  input.tin { height: var(--ctl-h); box-sizing: border-box; padding-top: 0; padding-bottom: 0; line-height: normal; }
  .tin:focus, textarea:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }

  /* 会话搜索（命令面板式；.mask 提供遮罩，box 居中于顶部） */
  .search-box { position: fixed; z-index: 61; top: 12vh; left: 50%; transform: translateX(-50%); width: min(560px, 92vw); background: #fff; border: 1px solid var(--border); border-radius: 14px; box-shadow: 0 24px 60px rgba(20, 15, 8, .28); overflow: hidden; display: flex; flex-direction: column; max-height: 66vh; }
  .search-head { display: flex; align-items: center; gap: 10px; padding: 12px 15px; border-bottom: 1px solid var(--border); }
  .search-head .si-ic { color: var(--faint); flex: none; }
  .search-head input { flex: 1; min-width: 0; border: none; outline: none; background: none; font: inherit; font-size: 15px; color: var(--text); }
  .search-head kbd { font: inherit; font-size: 10.5px; color: var(--faint); border: 1px solid var(--border2); border-radius: 5px; padding: 1px 6px; background: var(--rail); flex: none; }
  .search-list { min-width: 0; overflow-x: hidden; overflow-y: auto; padding: 6px; }
  .search-sec { padding: 8px 10px 3px; font-size: 11px; font-weight: 600; letter-spacing: .04em; color: var(--faint); }
  .search-empty { text-align: center; color: var(--faint); font-size: 13px; padding: 26px 12px; margin: 0; }
  .search-item { width: 100%; display: flex; align-items: center; gap: 10px; background: none; border: none; border-radius: 9px; padding: 9px 10px; cursor: pointer; text-align: left; color: var(--text); }
  .search-item:hover, .search-item.on { background: #f4f3ef; }
  .si-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .si-main b { font-weight: 500; font-size: 13.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .si-main small { color: var(--dim); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .si-tag { flex: none; font-size: 10px; color: var(--faint); border: 1px solid var(--border2); border-radius: 4px; padding: 0 4px; line-height: 15px; }

  /* 输入框（仿 Claude Code：整块圆角边框，聚焦时高亮，发送按钮嵌在框内） */
  .composer.big, .composer-wrap .composer { position: relative; background: #fff; border: 1px solid var(--border2); border-radius: 12px; box-shadow: none; transition: border-color .12s, box-shadow .12s; }
  .composer.big:focus-within, .composer-wrap .composer:focus-within { border-color: #b7b2a6; box-shadow: none; }
  .composer-wrap .composer.fileover { border-color: var(--accent); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 16%, transparent); }
  .composer-wrap .composer.fileover::after {
    content: '松开即可添加文件'; position: absolute; inset: 4px; z-index: 8; display: flex; align-items: center; justify-content: center;
    border: 1px dashed color-mix(in srgb, var(--accent) 58%, transparent); border-radius: 9px;
    background: color-mix(in srgb, #fff 92%, var(--accent)); color: var(--accent); font-size: 13px; font-weight: 600; pointer-events: none;
  }
  .composer.big textarea, .composer-wrap textarea { width: 100%; resize: none; border: none; background: none; box-shadow: none; padding: 14px 52px 14px 17px; font-size: 14px; line-height: 1.6; max-height: 200px; display: block; }
  .composer.big textarea:focus, .composer-wrap textarea:focus { outline: none; box-shadow: none; border: none; }
  .composer .send { position: absolute; right: 9px; bottom: 9px; width: 32px; height: 32px; border-radius: 50%; border: none; background: var(--accent); color: #fff; font-size: 16px; cursor: pointer; display: flex; align-items: center; justify-content: center; transition: background .12s, transform .08s; }
  .composer .send:hover { background: var(--accent-h); }
  .composer .send:active { transform: scale(.92); }
  .composer .send:disabled { background: #dcdad2; cursor: default; transform: none; }
  .composer .send.stop { background: var(--text); }
  /* composer：竖排 textarea + 底栏（启动页站点在左；会话页只读厂商在左，模型/发送在右）。 */
  .composer.big, .composer-wrap .composer { display: flex; flex-direction: column; }
  .composer.big textarea, .composer-wrap textarea { padding: 12px 16px 6px 16px; }
  .composer-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 8px 8px; }
  .cb-left { display: flex; align-items: center; gap: 1px; flex: 1; min-width: 0; }
  .cb-right { display: flex; align-items: center; gap: 3px; flex: none; }
  .composer-bar .send { position: static; width: 30px; height: 30px; margin-left: 3px; font-size: 15px; }
  /* 会话页只读徽标（站点/厂商，不可改）。 */
  .cb-ro { display: inline-flex; align-items: center; gap: 6px; padding: 4px 8px; color: var(--dim); font-size: 13px; opacity: .9; min-width: 0; }
  .cb-ro-t { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  /* ---- 线程 ---- */
  /* ★ 标题要和左上角那排工具图标在同一条水平线上。
     `.win-tools` 是 fixed / top:0 / height:30px + align-items:center → **图标中心恒在 15px**
     （和 .thread-head.slim 那条注释是同一个基准）。标题行高 20.3 → 上内边距取 5px，
     标题中心落在 15.15，对上。原来 13px 时中心在 23.1 —— **每个页面都低 8px**。
     ★ 上边距被「对齐那条 fixed 图标带」锁死＝5px，所以**下边距只能跟着收成 5px**，
     不然上下就不对称（原来 5/13，下面明显空一截）。头因此比以前矮 8px，这是这条对齐的代价。
     ★ align-items 必须 flex-start，不能用 center：th-info 比右侧按钮（28px）矮时
     （例如只有标题、没有副标题的头），center 会让 th-info 跟着按钮居中，标题又掉到 26.8。 */
  /* ★ 和内容区之间那条缝：原来是一条 1px 实线直接把两块切开，太硬。**两个状态分开治**：
     ① 静止态（治「硬」）：页头与内容**同底不加色**——曾试过一层极淡暖底做色阶差，
        但那条色带在页面顶上像贴了块补丁（用户点名拿掉）；现在静止态只留几乎看不见的
        底线。**刻意不上 backdrop-filter**：实测它只会把卡片的红字糊成粉影漂在标题背后
        （是噪点不是层次），还要赔上脱流 + ResizeObserver + 三处 scrollIntoView 的静默回归。
     ② 滚动态（治「切」）：见 .elevated —— 内容真滑到底下时才淡入柔和阴影、把线淡掉。
        滚到顶时页头底下是**空的**，那时打阴影＝给不存在的遮挡关系编故事
        （同 .term-wrap.with-chat 那条「独占/无对话时不加，保持齐边」的分寸）。
     z-index：阴影得压在 .thread 之上 —— .thread 是后面的兄弟，默认会盖住它。 */
  .thread-head { flex: none; padding: 5px 24px; border-bottom: 1px solid rgba(30, 25, 15, .05); position: relative; z-index: 1; transition: border-bottom-color .18s ease; display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  /* ★ 阴影用伪元素而不是 box-shadow：box-shadow 天然四周都有 —— `0 6px 12px -6px` 水平方向是
     「收缩 6 再模糊 6」，**正好到边**，于是左右两端各留一段 6px 渐弱，看着就是两个圆头戳在窗沿上。
     负 spread 再加也没用：加到不露头，朝下的阴影也就没了。
     伪元素 left:0/right:0 精确铺满、只朝下，左右**零溢出、零圆头**。 */
  .thread-head::after {
    content: ''; position: absolute; left: 0; right: 0; top: 100%; height: 6px;
    background: linear-gradient(rgba(20, 16, 13, .03), rgba(20, 16, 13, 0)); /* 原 .13 → .05 → .03：用户连续点名太重，压到将将可感；再嫌重就整个去掉 */
    opacity: 0; transition: opacity .18s ease; pointer-events: none;
  }
  /* 只在内容真滑到页头底下时才抬升。border 只改 color 不改宽度 —— 改宽度会让下面整块跳 1px。 */
  .thread-head.elevated { border-bottom-color: transparent; }
  .thread-head.elevated::after { opacity: 1; }
  /* 侧栏收起：红绿灯 + 悬浮的折叠/搜索钮压在内容区左上，页头统一左让位（全部视图受益）。
     发现更新时顶部临时增加更新入口，页头再让出一个按钮宽度，避免压住页面标题图标。 */
  .app.rail-collapsed .thread-head { padding-left: 140px; }
  .app.rail-collapsed.fs .thread-head, .app.rail-collapsed.win .thread-head { padding-left: 100px; }
  .app.rail-collapsed.top-update-visible .thread-head { padding-left: 167px; }
  .app.rail-collapsed.top-update-visible.fs .thread-head,
  .app.rail-collapsed.top-update-visible.win .thread-head { padding-left: 127px; }
  /* 会话头同时承担应用标题栏：展开、折叠与分屏都固定成与 win-tools 相同的 30px，
     避免域名按钮的上下 padding 把标题整行撑低。 */
  .thread-head.conversation-thread-head {
    height: 30px; min-height: 30px; padding-top: 0; padding-bottom: 0; align-items: center;
  }
  .th-info { min-width: 0; flex: 1 1 auto; overflow: hidden; }
  .th-info b { display: block; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 15px; line-height: 1.35; }
  .th-info b.th-title { display: inline-flex; align-items: center; gap: 7px; }
  .th-title :global(svg) { flex: none; color: var(--dim); }
  .th-info small { display: block; min-width: 0; overflow: hidden; color: var(--dim); font-size: 12px; margin-top: 2px; text-overflow: ellipsis; white-space: nowrap; }
  .th-info small :global(svg), .th-info small :global(img) { vertical-align: -2px; }
  .btag { display: inline-flex; align-items: center; gap: 4px; }
  .thread { flex: 1; overflow-y: auto; }
  .thread-inner { max-width: 760px; margin: 0 auto; padding: 22px 24px 8px; display: flex; flex-direction: column; gap: 20px; }

  .msg { display: flex; gap: 12px; }
  .msg.user { justify-content: flex-end; }
  .ubody { background: var(--user-bg); border-radius: 16px 16px 5px 16px; padding: 10px 14px; max-width: 78%; word-break: break-word; }
  .ub-text { white-space: pre-wrap; }
  .msg.assistant .body { flex: 1; min-width: 0; }
  .text { white-space: pre-wrap; word-break: break-word; }
  /* 助手消息的 Markdown 阅读版式（渲染出的 HTML 不在 Svelte 作用域内，须 :global） */
  .text.md { white-space: normal; font-size: 14px; line-height: 1.7; }
  .text.md :global(p) { margin: 0 0 10px; }
  .text.md :global(p:last-child), .text.md :global(ul:last-child), .text.md :global(ol:last-child),
  .text.md :global(pre:last-child), .text.md :global(table:last-child) { margin-bottom: 0; }
  .text.md :global(strong) { font-weight: 650; }
  .text.md :global(ul), .text.md :global(ol) { margin: 0 0 10px; padding-left: 22px; }
  .text.md :global(li) { margin: 3px 0; }
  .text.md :global(li::marker) { color: var(--faint); }
  .text.md :global(h1), .text.md :global(h2) { font-size: 16px; font-weight: 650; margin: 16px 0 6px; }
  .text.md :global(h3), .text.md :global(h4) { font-size: 14.5px; font-weight: 650; margin: 14px 0 5px; }
  .text.md :global(h1:first-child), .text.md :global(h2:first-child),
  .text.md :global(h3:first-child), .text.md :global(h4:first-child) { margin-top: 0; }
  .text.md :global(code) { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: .88em; background: #f1efe9; padding: 1px 5px; border-radius: 5px; }
  .text.md :global(pre) { background: #f6f5f1; border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; overflow-x: auto; margin: 0 0 10px; }
  .text.md :global(pre code) { background: none; padding: 0; font-size: 12.5px; line-height: 1.6; }
  .text.md :global(a) { color: var(--accent); text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
  .text.md :global(table) { border-collapse: collapse; margin: 0 0 10px; font-size: 13px; display: block; overflow-x: auto; max-width: 100%; }
  .text.md :global(th), .text.md :global(td) { border: 1px solid var(--border2); padding: 5px 10px; text-align: left; }
  .text.md :global(th) { background: #f6f5f1; font-weight: 600; }
  .text.md :global(blockquote) { margin: 0 0 10px; padding: 2px 12px; border-left: 3px solid var(--border2); color: var(--dim); }
  .text.md :global(hr) { border: none; border-top: 1px solid var(--border); margin: 12px 0; }
  .text.md :global(img) { max-width: 100%; border-radius: 8px; }
  .text.is-err { color: var(--err); }
  /* 重试：无边框，内联跟在红色错误文字后面 */
  .retry-btn { display: inline-flex; align-items: center; gap: 4px; margin-left: 10px; padding: 0; background: none; border: none; color: var(--accent); font: inherit; font-size: 12.5px; cursor: pointer; vertical-align: baseline; }
  .retry-btn:hover { text-decoration: underline; }
  .retry-btn svg { flex: none; }
  /* 额度耗尽卡：明确状态 + 倒计时 + 到点自动续跑 */
  .limit-card { border: 1px solid var(--border2, #e2dfd7); border-left: 3px solid #c98d70; border-radius: 10px; padding: 10px 13px; background: #fbf9f5; max-width: 520px; }
  .limit-head { font-size: 13.5px; font-weight: 600; color: var(--text, #26241f); }
  .limit-sub { margin-top: 3px; font-size: 12.5px; color: var(--dim, #6b675f); }
  .limit-actions { margin-top: 8px; display: flex; flex-wrap: wrap; gap: 4px 8px; }
  .limit-actions .retry-btn { margin-left: 0; }
  .limit-actions .retry-btn.is-armed { color: #3e7a4e; }
  .limit-hint { margin-top: 6px; font-size: 11.5px; color: var(--faint, #9b968c); }
  /* 运行中活动：只展示协议里真实收到的工具生命周期，不把轮询次数冒充业务完成度。 */
  .codex-thinking {
    width: max-content;
    margin: 13px 0 11px;
    font-size: 15px;
    font-weight: 500;
    line-height: 1.5;
  }
  .codex-thinking > span {
    color: transparent;
    background: linear-gradient(90deg, #aaa7a0 20%, #dedbd4 48%, #aaa7a0 76%);
    background-size: 220% 100%;
    background-position: 100% 50%;
    background-clip: text;
    -webkit-background-clip: text;
    animation: codexThinkingShimmer 2.1s ease-in-out infinite;
  }
  @keyframes codexThinkingShimmer {
    0%, 100% { background-position: 100% 50%; opacity: .62; }
    50% { background-position: 0 50%; opacity: 1; }
  }
  .execution-progress-stack { width: min(620px, 100%); margin: 7px 0 5px; }
  .execution-progress {
    width: 100%; box-sizing: border-box; margin: 0; border: 1px solid var(--border); border-radius: 9px;
    background: #faf9f6;
  }
  .execution-progress-mark { width: 16px; height: 16px; flex: none; display: grid; place-items: center; border-radius: 5px; background: #eeeae3; }
  .execution-progress-mark > span { width: 5px; height: 5px; border-radius: 50%; background: var(--accent); }
  .execution-progress-copy { min-width: 0; display: grid; gap: 0; }
  .execution-progress-copy b, .execution-progress-copy small { min-width: 0; overflow-wrap: anywhere; white-space: normal; }
  .execution-progress-copy b { font-size: 11.5px; line-height: 1.42; font-weight: 600; color: var(--text); }
  .execution-progress-copy small { margin-top: 1px; font-size: 10px; line-height: 1.4; color: var(--faint); font-variant-numeric: tabular-nums; }
  .execution-progress.quiet { border-color: #ead8b7; background: #fcf8ef; }
  .execution-progress.quiet .execution-progress-mark { background: #f4ead6; }
  .execution-progress.quiet .execution-progress-mark > span { background: #b87920; }
  .execution-progress.has-failure:not(.quiet) { border-color: #ead8b7; background: #fcf8ef; }
  .execution-progress.has-failure:not(.quiet) .execution-progress-mark { background: #f4ead6; }
  .execution-progress.has-failure:not(.quiet) .execution-progress-mark > span { background: #b87920; }
  .execution-progress.recovered-failure:not(.quiet) { border-color: var(--border); background: #faf9f6; }
  .execution-progress.recovered-failure:not(.quiet) .execution-progress-mark { background: #eeeae3; }
  .execution-progress.recovered-failure:not(.quiet) .execution-progress-mark > span { background: var(--accent); }
  .execution-progress-details {
    display: block; padding: 0; overflow: hidden;
  }
  .execution-progress-details > summary {
    display: grid; grid-template-columns: 16px minmax(0, 1fr) auto; align-items: start; gap: 7px; min-width: 0; padding: 6px 8px;
    cursor: pointer; list-style: none; user-select: none;
  }
  .execution-progress-details > summary::-webkit-details-marker { display: none; }
  .execution-progress-details .execution-progress-copy { flex: 1 1 auto; }
  .execution-failure-summary {
    display: flex; align-items: center; gap: 4px; flex: none; margin: 1px 0 0 auto;
    color: #8a5d1b; white-space: nowrap;
  }
  .execution-failure-summary b { font-size: 10px; font-weight: 650; }
  .execution-failure-summary small { color: var(--faint); font-size: 10px; font-weight: 500; }
  .execution-failure-summary svg { flex: none; transition: transform .15s ease; }
  .execution-progress-details[open] .execution-failure-summary svg { transform: rotate(180deg); }
  .execution-activity-list { max-height: min(300px, 42vh); overflow: auto; padding: 5px 8px 7px; border-top: 1px solid rgba(128,122,110,.16); }
  .execution-activity-item { display: grid; grid-template-columns: 7px minmax(0,1fr) auto; gap: 7px; align-items: start; padding: 6px 0; border-bottom: 1px solid rgba(128,122,110,.12); }
  .execution-activity-item > div { min-width: 0; }
  .execution-activity-item b { display: block; font-size: 11.5px; font-weight: 650; line-height: 1.4; overflow-wrap: anywhere; }
  .execution-activity-item p { margin: 2px 0 0; color: var(--dim); font-size: 10.5px; line-height: 1.45; overflow-wrap: anywhere; }
  .execution-activity-item > small { color: var(--faint); font-size: 10px; white-space: nowrap; }
  .execution-activity-item.is-failed > small { color: #9a6420; }
  .execution-activity-state { width: 6px; height: 6px; margin-top: 5px; border-radius: 50%; background: #aaa59a; }
  .execution-activity-state.running { background: var(--accent); box-shadow: 0 0 0 3px rgba(160,60,43,.09); }
  .execution-activity-state.completed { background: #4e9861; }
  .execution-activity-state.failed { background: #b87920; }
  .execution-real-progress { padding: 5px 0 8px; }
  .execution-real-progress > div { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; font-size: 10.5px; color: var(--dim); }
  .execution-real-progress > div b { color: var(--text); font-size: 11px; }
  .execution-real-progress > span { display: block; height: 3px; margin-top: 5px; overflow: hidden; border-radius: 9px; background: #e8e4dc; }
  .execution-real-progress > span i { display: block; height: 100%; border-radius: inherit; background: var(--accent); }
  .execution-progress.is-running .execution-progress-mark > span { animation: executionPulse 1.6s ease-in-out infinite; }
  @keyframes executionPulse {
    0%, 100% { transform: scale(.82); opacity: .72; box-shadow: 0 0 0 0 rgba(160,60,43,0); }
    50% { transform: scale(1.08); opacity: 1; box-shadow: 0 0 0 4px rgba(160,60,43,.11); }
  }
  .execution-failure-impact { margin: 8px 0 0; color: #8a6d43; font-size: 10.5px; line-height: 1.55; }
  @media (max-width: 620px) {
    .execution-progress-stack { width: 100%; }
    .execution-progress-details > summary { padding: 6px 8px; }
    .execution-failure-summary small { display: none; }
    .execution-failure-summary b { max-width: 76px; overflow: hidden; text-overflow: ellipsis; }
  }
  @media (prefers-reduced-motion: reduce) {
    .codex-thinking > span {
      color: var(--faint);
      background: none;
      animation: none;
      opacity: 1;
    }
    .execution-progress.is-running .execution-progress-mark > span { animation: none; }
  }
  /* 命令列表：默认收起，点击展开 */
  .cmds { margin-bottom: 9px; }
  .cmds summary { display: inline-flex; align-items: center; gap: 6px; cursor: pointer; list-style: none; width: fit-content;
    font-size: 12px; color: var(--dim); border-radius: 8px; padding: 4px 8px; margin-left: -8px; user-select: none; }
  .cmds summary::-webkit-details-marker { display: none; }
  .cmds summary:hover { background: #f1efe9; }
  .cmds .cmd-ic { color: var(--accent); flex: none; }
  .cmds .cmd-chev { flex: none; transition: transform .15s; }
  .cmds[open] .cmd-chev { transform: rotate(180deg); }
  .cmds .tools { margin: 7px 0 0; }
  .tools { display: flex; flex-direction: column; gap: 5px; margin-bottom: 8px; }
  .tool { display: flex; gap: 8px; align-items: baseline; background: #f6f5f1; border: 1px solid var(--border); border-radius: 8px; padding: 5px 10px; font-size: 12px; }
  .tcode { color: var(--accent); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; flex: none; font-weight: 600; }
  .tdetail { color: var(--dim); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tool-count { flex: none; margin-left: auto; color: var(--faint); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .inlink { background: none; border: none; color: var(--accent); text-decoration: underline; cursor: pointer; padding: 0; font: inherit; word-break: break-all; }
  .err-note { color: var(--err); background: var(--err-soft); border: 1px solid var(--err-border); border-radius: 8px; padding: 8px 10px; font-size: 13px; }
  /* 加载：logo 的 C 一笔描出、圆点落定，循环；后面带本轮耗时。尊重"减弱动态"。 */
  .working { display: flex; align-items: center; gap: 7px; padding: 3px 0; }
  .working:not(:first-child) { margin-top: 12px; }
  .wl { flex: none; }
  .working-t { font-size: 12px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .context-compact-note { display: flex; align-items: center; gap: 6px; width: fit-content; margin: 2px 0 9px; padding: 5px 8px; border-radius: 8px; color: #267457; background: #edf7f1; font-size: 12px; }
  .context-compact-note small { color: #6e8d7d; font-size: 11px; }
  .wl-trace { animation: wldraw 1.5s ease-in-out infinite; }
  @keyframes wldraw { 0% { stroke-dashoffset: 100; opacity: 1; } 55% { stroke-dashoffset: 0; opacity: 1; } 82% { stroke-dashoffset: 0; opacity: 1; } 100% { stroke-dashoffset: 0; opacity: 0; } }
  .wl-dot { transform-box: fill-box; transform-origin: center; animation: wldot 1.5s ease-in-out infinite; }
  @keyframes wldot { 0%, 45% { opacity: 0; transform: scale(.2); } 62% { opacity: 1; transform: scale(1); } 82% { opacity: 1; } 100% { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .wl-trace, .wl-dot { animation: none; } .wl-trace { stroke-dashoffset: 0; } .wl-dot { opacity: 1; } }
  @keyframes pulse { 50% { opacity: .35; } }


  @keyframes gsc-shimmer { to { background-position-x: -220%; } }


  @keyframes theme-thumb-shimmer {
    0% { transform: translateX(-42%); }
    68%, 100% { transform: translateX(42%); }
  }


  /* 排期视图 */
  .sched-inner { max-width: 720px; margin: 0 auto; padding: 18px 24px 24px; }
 
 
  .sched-empty { text-align: center; color: var(--dim); padding: 12vh 24px; }
  .sched-empty .cal-mark { font-size: 34px; }
  .sched-empty b { display: block; margin: 12px 0 6px; color: var(--text); font-size: 16px; }
  .sched-empty p { margin: 0 auto; max-width: 380px; font-size: 13px; }


  .switch { width: 34px; height: 20px; border-radius: 12px; border: none; background: #d3d0c8; cursor: pointer; padding: 0; position: relative; transition: background .15s; }
  .switch.on { background: var(--accent); }
  .switch span { position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: #fff; transition: transform .15s; box-shadow: var(--shadow-sm); }
  .switch.on span { transform: translateX(14px); }
 

  .modal.wide { width: min(520px, 94vw); }
  .modal.ssh-modal .trow,
  .modal.ssh-modal .tfield,
  .modal.ssh-modal .ssh-key-row { max-width: 100%; }
  .trow { display: flex; gap: 12px; }
  .tfield { display: flex; flex-direction: column; gap: 5px; flex: 1; min-width: 0; }
  .tfield > span { font-size: 12px; color: var(--dim); }

  .composer-wrap { flex: none; padding: 10px 24px 20px; }
  .composer-wrap .composer { max-width: 760px; margin: 0 auto; }

  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1.5px solid var(--border2); border-radius: 8px; padding: 6px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
  .btn:hover { background: #f6f5f1; } .btn:disabled { opacity: .5; cursor: default; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; } .btn.primary:hover { background: var(--accent-h); }
  .btn.ghost { border-color: var(--border); }
  .btn.small { padding: 4px 10px; font-size: 12px; }
  .btn.sm { padding: 5px 12px; font-size: 12px; border-radius: 8px; }
  .btn.soft { display: inline-flex; align-items: center; gap: 6px; padding: 3px 12px; background: #fff; color: var(--text); border: 1px solid var(--border2); border-radius: 9px; font-weight: 500; }
  .btn.soft:hover { background: var(--rail); border-color: #cfccc2; }
  .btn.soft .plus-ic { color: var(--dim); }
  /* 无边框动作按钮（头部次要动作）：去边框去底，仅 hover 有淡底 */
  .btn.bare { border: none; background: none; }
  .btn.bare:hover { background: var(--rail); border-color: transparent; }
  .authbtn { display: inline-flex; align-items: center; gap: 3px; background: none; border: none; padding: 1px 2px; color: var(--accent); font-size: 12px; font-weight: 550; line-height: 1; cursor: pointer; }
  .authbtn:hover { color: var(--accent-h); }
  .permit-card { border: 1px solid var(--border2); border-radius: 11px; overflow: hidden; background: #fff; max-width: 480px; }
  .permit-card.danger { border-color: #f0d9a8; }
  .permit-head { display: flex; align-items: center; gap: 7px; padding: 8px 12px; font-size: 12px; font-weight: 600; color: var(--text); background: #f6f5f1; border-bottom: 0.5px solid var(--border); }
  .permit-card.danger .permit-head { color: #8a5a08; background: #fdf6e6; border-bottom-color: #f0e3c4; }
  .permit-dot { width: 7px; height: 7px; border-radius: 50%; background: #b45309; flex: none; }
  .permit-tag { margin-left: auto; font-size: 10px; font-weight: 600; color: #b45309; background: #faf1d8; padding: 1px 7px; border-radius: 5px; }
  .permit-desc { padding: 10px 12px 0; font-size: 13px; font-weight: 550; color: var(--text); line-height: 1.55; word-break: break-word; }
  .permit-meta { padding: 3px 12px 0; font-size: 11px; color: var(--dim); }
  .permit-more { display: block; margin: 7px 12px 0; padding: 0; border: 0; background: none; font-size: 11px; color: var(--dim); cursor: pointer; text-align: left; }
  .permit-more:hover { color: var(--text); }
  .permit-cmd { display: block; margin: 6px 12px 0; font-size: 12px; background: #17130f; color: #e8dcc8; padding: 8px 10px; border-radius: 7px; white-space: pre-wrap; word-break: break-all; max-height: 180px; overflow-y: auto; }
  .permit-act { display: flex; gap: 8px; padding: 10px 12px 12px; }
  .permit-act .btn { flex: 1; }
  .cf-mark { flex: none; display: inline-block; vertical-align: middle; }
  .hero-btns { display: flex; gap: 10px; flex-wrap: wrap; justify-content: center; }
  .setup-add { position: relative; display: inline-flex; }
  .setup-add-trigger { width: 27px; height: 27px; padding: 0; }
  .setup-add-trigger.open { color: var(--text); background: #f1efe9; }
  .setup-add-menu {
    position: absolute; z-index: 8; top: calc(100% + 5px); right: 0; width: 216px; padding: 5px;
    background: var(--panel); border: 1px solid var(--border); border-radius: 10px;
    box-shadow: 0 10px 28px rgba(30, 25, 15, .15); animation: pop .1s ease-out;
  }
  .setup-add-menu button {
    width: 100%; min-height: 34px; padding: 7px 9px; border: 0; border-radius: 7px; background: none;
    display: flex; align-items: center; gap: 9px; color: var(--text); cursor: pointer; text-align: left;
    font: inherit; font-size: 12.5px; font-weight: 500; letter-spacing: 0; text-transform: none;
  }
  .setup-add-menu button:hover { background: #f4f3ef; }
  .setup-add-menu button:disabled { opacity: .5; cursor: default; }
  .setup-add-menu button :global(svg) { flex: none; color: var(--dim); }
  .mask.cf-mask { z-index: 70; }
  .modal.cf-modal { z-index: 80; width: min(440px, 94vw); max-height: 88vh; }
  .cf-step { border-top: 0.5px solid var(--border); padding-top: 12px; margin-top: 6px; }
  .cf-step:first-child { border-top: none; padding-top: 0; margin-top: 0; }
  .cf-step-t { font-size: 13px; color: var(--text); margin: 0 0 7px; line-height: 1.5; }
  .cf-update-target { display: flex; align-items: center; gap: 9px; margin-bottom: 9px; padding: 9px 10px; border: 1px solid #dfe7e2; border-radius: 9px; background: #f4f8f5; }
  .cf-update-target > span { width: 28px; height: 28px; border-radius: 8px; background: #fff; display: inline-flex; align-items: center; justify-content: center; flex: none; }
  .cf-update-target > div { min-width: 0; }
  .cf-update-target b, .cf-update-target small { display: block; }
  .cf-update-target b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .cf-update-target small { margin-top: 2px; color: var(--dim); font-size: 10.5px; overflow-wrap: anywhere; }
  .cf-update-verified { display: flex; align-items: center; gap: 8px; margin-top: 8px; padding: 8px 9px; border-radius: 9px; background: #fbf3e7; color: #8a5a08; }
  .cf-update-verified.ok { background: #edf7f0; color: #287248; }
  .cf-update-verified > span { width: 22px; height: 22px; border-radius: 50%; background: rgb(255 255 255 / .75); display: inline-flex; align-items: center; justify-content: center; flex: none; font-weight: 700; }
  .cf-update-verified b, .cf-update-verified small { display: block; }
  .cf-update-verified b { font-size: 11.5px; }
  .cf-update-verified small { margin-top: 1px; color: var(--dim); font-size: 10px; }
  .cf-perms { margin: 0 0 9px; padding-left: 0; list-style: none; display: flex; flex-direction: column; gap: 3px; }
  .cf-perms li { font-size: 12px; color: var(--dim); padding-left: 13px; position: relative; }
  .cf-perms li::before { content: '·'; position: absolute; left: 4px; color: #f6821f; font-weight: 700; }
  .cf-perms li b { color: var(--text); font-weight: 550; }
  .cf-row { display: flex; gap: 8px; }
  .cf-row .tin { flex: 1; }
  .wr-note { display: flex; align-items: center; gap: 7px; width: fit-content; max-width: 100%; margin-top: 12px; font-size: 12px; color: #8a5a08; }
  .wr-note .wr-ic { flex: none; color: #d9a400; }
  .wr-note span { color: #8a5a08; }
  .wr-btn { flex: none; border: none; background: #d9a400; color: #fff; font-size: 11px; font-weight: 500; padding: 3px 9px; border-radius: 6px; cursor: pointer; }
  .wr-btn:hover { background: #c08f00; }
  .wr-btn:disabled { opacity: .85; cursor: default; }
  .wr-spin { display: inline-block; width: 10px; height: 10px; border: 1.6px solid #fff; border-right-color: transparent; border-radius: 50%; animation: rspin .7s linear infinite; margin-right: 5px; vertical-align: -1px; }
  .cf-perms-toggle { display: inline-flex; align-items: center; gap: 5px; background: none; border: none; padding: 0 0 8px; font-size: 12px; color: var(--dim); cursor: pointer; }
  .cf-perms-toggle:hover { color: var(--text); }
  .cf-chev { flex: none; transition: transform .12s; }
  .cf-chev.open { transform: rotate(180deg); }
  /* 启动页底栏空间不足时先收项目名，避免它把后面的风格/权限控件挤到一起。 */
  .cf-proj-in {
    flex: 0 1 190px; width: 190px; min-width: 72px; max-width: 190px;
    border: none; background: none; font-size: 13px; color: var(--text); padding: 3px 6px; outline: none;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .cf-proj-in::placeholder { color: #b3ada2; }
  .cb-prev { position: relative; display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; font-size: 12px; padding: 3px 8px; border: none; background: none; color: var(--accent); cursor: pointer; font-weight: 500; border-radius: 6px; }
  .cb-prev:hover:not(:disabled) { background: var(--rail); }
  .cb-prev:disabled { opacity: .45; cursor: default; }
  .cb-prev.dim { color: var(--dim); font-weight: 400; }
  .cb-prev svg { flex: none; }
  /* 自定义悬停提示（不用系统 title） */
  .cf-bar { max-width: 760px; margin: 0 auto 6px; display: flex; align-items: center; gap: 2px; flex-wrap: wrap; }
  /* 等待消息（排队）条：琥珀色调，区别于普通输入 */
  .queued-row { display: flex; align-items: center; gap: 8px; margin: 10px 12px 0; padding: 6px 8px 6px 11px; background: #fbf6ea; border: 1px solid #f0e2c0; border-radius: 9px; font-size: 12.5px; }
  .queued-ic { flex: none; color: #b45309; display: inline-flex; }
  .queued-t { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--dim); }
  .queued-btn { flex: none; border: none; background: none; color: var(--accent); font-size: 12px; cursor: pointer; padding: 2px 4px; }
  .queued-btn:hover { text-decoration: underline; }
  .queued-x { flex: none; border: none; background: none; color: var(--faint); font-size: 15px; line-height: 1; cursor: pointer; padding: 0 3px; }
  .queued-x:hover { color: var(--err); }
  .send.queue { background: var(--accent); color: #fff; }
  .attach-row { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding: 10px 14px 2px; }
  .attach-chip { display: inline-flex; align-items: center; gap: 6px; max-width: 190px; padding: 3px 4px 3px 6px; background: var(--panel); border: 1px solid var(--border2); border-radius: 8px; font-size: 12px; }
  .attach-img { width: 24px; height: 24px; object-fit: cover; border-radius: 4px; flex: none; }
  .attach-ic { color: var(--dim); flex: none; display: inline-flex; }
  .attach-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .attach-x { flex: none; border: none; background: none; color: var(--dim); cursor: pointer; font-size: 14px; line-height: 1; padding: 0 2px; }
  /* 用户气泡里的附件卡（只读，白底衬在浅色气泡上） */
  .ub-atts { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .ub-atts.only { margin-top: 0; }
  .ub-att { display: inline-flex; align-items: center; gap: 6px; max-width: 220px; padding: 4px 10px 4px 8px; background: #fff; border: 1px solid var(--border2); border-radius: 8px; font-size: 12px; }
  .ub-att-img { padding: 0; border: 1px solid var(--border2); border-radius: 10px; overflow: hidden; cursor: zoom-in; line-height: 0; background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 14px 14px; display: inline-flex; align-items: center; justify-content: center; min-width: 44px; min-height: 34px; }
  /* 按图片原始比例呈现（纯像素上限、不裁切），宽 logo/竖图都能看到全貌；点击仍可放大。
     注意不能用 max-width:min(…,100%)：容器 shrink-to-fit，百分比循环引用会让整条声明失效。 */
  .ub-att-img img { display: block; max-width: 260px; max-height: 180px; width: auto; height: auto; }
  .ub-att-img.ph { display: inline-block; width: 128px; height: 96px; cursor: default; animation: phpulse 1.2s ease-in-out infinite; }
  /* 助手消息里的生图产物：与用户附件同款卡片，气泡外基底微调 */
  .gen-imgs { margin-top: 6px; }
  .ub-att.as-btn { cursor: pointer; font: inherit; font-size: 12px; color: inherit; }
  .ub-att.as-btn:hover { border-color: var(--border3, var(--border2)); }
  @keyframes phpulse { 0%, 100% { opacity: .55; } 50% { opacity: .9; } }
  .ub-att-ic { flex: none; display: inline-flex; color: var(--dim); }
  .ub-att-n { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .attach-x:hover { color: var(--err); }
  .attach-loading { font-size: 11px; color: var(--dim); }
  /* 提示词库 */
  .prompt-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(272px, 1fr)); gap: 12px; }
  .prompt-card { display: flex; flex-direction: column; background: var(--card); border: 1px solid var(--border2); border-radius: 12px; padding: 13px 14px 11px; }
  .prompt-top { display: flex; align-items: center; gap: 7px; margin-bottom: 6px; }
  .prompt-title { font-size: 13.5px; font-weight: 600; color: var(--text); flex: 1; min-width: 0; }
  .prompt-tag { flex: none; font-size: 10px; font-weight: 600; color: var(--dim); background: #e7e4dd; padding: 1px 6px; border-radius: 5px; }
  .prompt-body { margin: 0 0 11px; font-size: 12.5px; line-height: 1.5; color: var(--dim); display: -webkit-box; -webkit-line-clamp: 5; line-clamp: 5; -webkit-box-orient: vertical; overflow: hidden; }
  .prompt-acts { display: flex; align-items: center; gap: 4px; margin-top: auto; }
  .prompt-act { position: relative; width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: none; border-radius: 8px; background: none; color: var(--dim); cursor: pointer; }
  .prompt-act:hover { color: var(--text); background: var(--rail); }
  .prompt-act svg { flex: none; }
  /* 模板库单独一个更宽的容器：grid 本来就是 auto-fill（会自己长列），真正卡住它的是
     .sched-inner 的 720px —— 三列要 3*272+2*12+48 = 876px，720 差得远，所以窗口再宽也只有两列。
     日程页还得用窄栏（那是读列表的），所以不改 .sched-inner，另起一个。 */
  .tmpl-inner { max-width: 1180px; margin: 0 auto; padding: 18px 24px 24px; }
  .th-line { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  /* 提示挪到标题**同一行**了。`.th-info small` 那 2px 上间距是给「标题下面那行」准备的，
     这里得清掉；选择器带上 .tmpl-hint 才压得过它（它带元素选择器，分更高）。 */
  .th-info small.tmpl-hint { margin-top: 0; color: var(--faint); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* 负边距抵掉筹码自己的内边距 —— 那 8px/3px 是**点击热区**，不该算进视觉间距：
     - 左：不抵的话整排比上面的「模板库」缩进 8px，看着就是没对齐。
     - 下：不抵的话筹码文字离底边比标题离顶边多出 3px 的死空（上 6 / 下 10.5），上下就不一致。 */
  .tmpl-chips { display: flex; flex-wrap: wrap; gap: 2px; margin-top: 5px; margin-left: -8px; margin-bottom: -3px; }
  /* 无外框、无底：选中只靠**颜色加重**（--dim → --text）。
     ★ 故意不动 font-weight：字重一变宽度就变，点一下整排筹码会跟着挪位。 */
  .tmpl-chip { display: inline-flex; align-items: center; gap: 3px; padding: 3px 8px; border: 0; border-radius: 999px; background: transparent; color: var(--dim); font-size: 12px; cursor: pointer; }
  .tmpl-chip:hover { color: var(--text); }
  .tmpl-chip:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .tmpl-chip.on { color: var(--text); }
  .tc-ic { flex: none; margin-right: 1px; opacity: 0.7; }
  .tc-n { font-size: 10px; opacity: 0.55; font-variant-numeric: tabular-nums; }
  /* ★ 240 是**默认窗口能放下两列**倒推出来的，别再往上调：
     默认 820 宽、左栏 240 → 内容 580，减 48 内边距 = 532。两列 = 2*240+12 = 492 ✓。
     上一版写 272 要 556 > 532，就差这一点，默认窗口直接掉成**一列**。
     再宽才 3 列（≈1050）、4 列（≈1300，到 1180 封顶就不再加）。 */
  .tmpl-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; }
  /* 从全局搜索跳过来时闪一下：一屏十几张卡，跳到了也认不出是哪张 */
  @keyframes tmplFlash { from { box-shadow: 0 0 0 2px var(--accent); } to { box-shadow: 0 0 0 2px transparent; } }
  .tmpl-card.tmpl-flash { animation: tmplFlash 1.4s ease-out; }
  .tmpl-card { display: flex; flex-direction: column; background: var(--card); border: 1px solid var(--border2); border-radius: 12px; overflow: hidden; }
  .tmpl-thumb { position: relative; height: 158px; overflow: hidden; background: #fff; display: flex; align-items: center; justify-content: center; border-bottom: 1px solid var(--border); }
  .tmpl-frame { position: absolute; top: 0; left: 0; width: 1280px; height: 860px; border: 0; transform: scale(0.23); transform-origin: top left; pointer-events: none; background: #fff; }
  .tmpl-letter { font-size: 40px; font-weight: 600; color: #a03c2b; }
  .tmpl-hover { position: absolute; inset: 0; display: flex; align-items: flex-end; justify-content: center; gap: 8px; padding: 10px; background: linear-gradient(to top, rgba(0, 0, 0, .4), rgba(0, 0, 0, .05) 45%, transparent 70%); opacity: 0; transition: opacity .12s; }
  .tmpl-card:hover .tmpl-hover { opacity: 1; }
  .tmpl-act { position: relative; width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center; border: none; border-radius: 7px; background: rgba(255, 255, 255, .95); color: var(--text); cursor: pointer; box-shadow: 0 2px 8px rgba(0, 0, 0, .15); }
  .tmpl-act.primary { background: var(--accent); color: #fff; }
  .tmpl-act:hover { transform: translateY(-1px); }
  .tmpl-act svg { flex: none; width: 13px; height: 13px; }
  .tmpl-body { padding: 10px 12px 12px; flex: 1; }
  .tmpl-body b { font-size: 13px; }
  .tmpl-sub { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; margin-top: 4px; }
  .tmpl-desc { flex: 1; min-width: 0; font-size: 12px; color: var(--dim); line-height: 1.4; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tmpl-meta { flex: none; display: inline-flex; align-items: center; gap: 6px; font-size: 11px; color: #b3ada2; }
  .tmpl-pg { color: var(--dim); background: var(--border); border-radius: 4px; padding: 0 4px; line-height: 15px; }
  .site-grp-fav { width: 14px; height: 14px; border-radius: 3px; flex: none; object-fit: cover; }
  .plus-ic { flex: none; }
  .icon-btn { background: none; border: none; cursor: pointer; padding: 6px; border-radius: 8px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .icon-btn:hover { background: #f1efe9; color: var(--text); }
  .icon-btn:disabled { opacity: .55; cursor: default; }
  .rfz { display: block; }
  .rfz.spin { animation: rspin .8s linear infinite; }
  @keyframes rspin { to { transform: rotate(360deg); } }
  @keyframes pop { from { opacity: 0; transform: translateY(4px); } }
  .x { background: none; border: none; color: var(--faint); font-size: 20px; cursor: pointer; line-height: 1; }
  .x:hover { color: var(--err); } .x.sm { font-size: 15px; }

  .mask { position: fixed; inset: 0; background: rgba(25,20,10,.28); z-index: 50; }
  .sheet, .modal { position: fixed; z-index: 60; background: var(--bg); border: 1px solid var(--border); box-shadow: var(--shadow-lg); display: flex; flex-direction: column; }
  .sheet { top: 0; right: 0; bottom: 0; width: min(400px, 92vw); border-radius: 0; }
  /* margin:auto 居中而非 transform——transform 会给内部 position:fixed 的下拉菜单
     制造 containing block，菜单被钳进弹窗坐标系再被 overflow:hidden 裁掉（遮挡）。 */
  .modal { inset: 0; margin: auto; height: fit-content; max-height: 88vh; width: min(440px, 92vw); border-radius: 14px; overflow: hidden; }
  .sheet-head { box-sizing: border-box; min-height: 48px; display: flex; justify-content: space-between; align-items: center; padding: 7px 14px; border-bottom: 1px solid var(--border); }
  .sheet-head > b { min-width: 0; overflow: hidden; font-size: 13.5px; line-height: 1.2; text-overflow: ellipsis; white-space: nowrap; }
  /* 弹窗正文统一只允许纵向滚动。只声明 overflow-y:auto 会按 CSS 规则把横向轴
     隐式计算成 auto；贴边 footer 的负边距和 WebView 的浮点取整都会因此冒出横向条。 */
  .sheet-body { min-width: 0; padding: 16px 18px; overflow-x: hidden; overflow-y: auto; display: flex; flex-direction: column; gap: 7px; }
  :global(.modal[data-dialog-geometry="true"] > .sheet-head) { flex: none; user-select: none; }
  :global(.modal[data-dialog-geometry="true"] > .sheet-head[data-dialog-dragging="true"]) { cursor: grabbing !important; }
  :global(.modal[data-dialog-resizable="true"] > .sheet-body) { flex: 1 1 auto; min-width: 0; min-height: 0; }
  :global(.modal[data-dialog-resizable="true"] > .sheet-body > textarea.tin) { flex: 1 1 auto; min-height: 120px; resize: none; }
  :global([data-dialog-resize="se"])::after {
    position: absolute;
    right: 3px;
    bottom: 3px;
    width: 6px;
    height: 6px;
    border-right: 1.5px solid var(--faint);
    border-bottom: 1.5px solid var(--faint);
    content: "";
    opacity: .52;
    transition: border-color .15s, opacity .15s;
  }
  :global([data-dialog-resize="se"]:hover)::after,
  :global([data-dialog-resize="se"]:focus-visible)::after { border-color: var(--accent); opacity: .9; }
  :global(.sheet-width-handle:hover .sheet-width-handle-grip),
  :global(.sheet-width-handle:focus-visible .sheet-width-handle-grip) { background: var(--accent) !important; opacity: .82 !important; }
  :global(.sheet-width-handle:focus-visible) { outline: 2px solid var(--accent) !important; outline-offset: -4px !important; }
  .update-center-mask { z-index: 65; }
  .update-center-sheet { z-index: 70; width: min(540px, 96vw); background: #fbfaf7; }
  .about-update-entry { display: inline-flex; align-items: center; gap: 5px; }
  .about-update-entry :global(svg) { flex: none; color: var(--dim); }
  .update-center-title { min-width: 0; display: flex; align-items: center; gap: 7px; }
  .update-center-title-icon { flex: none; width: 26px; height: 26px; display: grid; place-items: center; border-radius: 7px; background: var(--accent-soft); color: var(--accent); }
  .update-center-title-copy { min-width: 0; display: grid; gap: 1px; }
  .update-center-title-copy b { font-size: 13.5px; line-height: 1.2; }
  .update-center-title-copy small { color: var(--faint); font-size: 9.5px; line-height: 1.2; font-weight: 400; }
  .update-center-head-actions { display: flex; align-items: center; gap: 5px; }
  .update-center-body { flex: 1 1 auto; min-height: 0; gap: 13px; padding: 15px 17px 18px; }
  .update-center-notice { padding: 8px 10px; border: 1px solid #e6dfd3; border-radius: 9px; background: #f7f4ee; color: var(--dim); font-size: 11px; line-height: 1.5; }
  .update-center-notice.success { border-color: #cfe5d8; background: #eef8f2; color: var(--ok); }
  .update-center-notice.error { border-color: var(--err-border); background: var(--err-soft); color: var(--err); }
  .update-section { display: grid; gap: 7px; }
  .update-item { min-width: 0; display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 9px; padding: 9px 10px; }
  .update-item.pilot { border: 1px solid var(--border); border-radius: 11px; background: #fff; }
  .update-item-icon { flex: none; width: 28px; height: 28px; display: grid; place-items: center; border-radius: 8px; background: var(--soft); color: var(--dim); }
  .update-item-icon.package { color: var(--accent); }
  .update-item-copy { min-width: 0; display: grid; gap: 2px; }
  .update-item-copy b { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11.5px; }
  .update-item-copy small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--dim); font-size: 10.5px; }
  .update-item-copy i { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--faint); font-size: 9.5px; font-style: normal; }
  .update-state { flex: none; min-width: 48px; padding: 3px 6px; border-radius: 999px; background: var(--soft); color: var(--dim); font-size: 9.5px; font-weight: 650; line-height: 1.25; text-align: center; white-space: nowrap; }
  .update-state.available { background: #fff1dc; color: #a65b00; }
  .update-state.current, .update-state.done { background: #eaf6ee; color: var(--ok); }
  .update-state.checking, .update-state.updating { background: #eceff6; color: #46619b; }
  .update-state.waiting { background: #f3f0e9; color: #826c47; }
  .update-state.unreachable, .update-state.failed { background: var(--err-soft); color: var(--err); }
  .update-item .btn.small { flex: none; min-width: 42px; padding-inline: 7px; }
  .update-center-footnote { margin: 0; padding: 9px 10px; border-radius: 9px; background: #f1efe9; color: var(--dim); font-size: 10px; line-height: 1.55; }
  .update-center-footer { flex: none; display: flex; align-items: center; justify-content: flex-end; gap: 8px; padding: 11px 17px; border-top: 1px solid var(--border); background: #fff; }
  .conn-remark { cursor: help; }
  .conn-remark-modal { width: min(370px, 92vw); }
  .conn-remark-modal .sheet-head > div { min-width: 0; display: flex; align-items: baseline; gap: 8px; }
  .conn-remark-modal .sheet-head b { flex: none; }
  .conn-remark-modal .sheet-head small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sec-head { display: flex; justify-content: space-between; align-items: center; font-size: 11px; letter-spacing: .03em; text-transform: uppercase; color: var(--faint); font-weight: 600; margin-bottom: 1px; }
  .sec-head.mt { margin-top: 12px; }
  .conn-list { display: flex; flex-direction: column; gap: 5px; }
  .conn-row { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; padding: 8px 10px; border: 1px solid var(--border); border-radius: 11px; cursor: pointer; transition: border-color .12s, background .12s; }
  .conn-row:hover { background: #faf9f6; }
  .conn-row.on { border-color: #cfc9ec; background: #f7f6ff; }
  .conn-row :global(.sm) { border-radius: 6px; }
  .conn-main { flex: 1; min-width: 0; } .conn-main b { display: block; font-size: 13.5px; } .conn-main small { display: block; color: var(--dim); font-size: 11px; }
  /* ★ 上面这条 display:block（0,1,1）会压过 .cs-os 的 inline-flex（0,1,0）：设置弹窗的连接行里
     发行版图标于是退回**行内基线对齐**——12px 的图标坐在 11px 文字的基线上，整个上蹿一截。
     这里把 flex 垂直居中赢回来（用 flex 而非 inline-flex：副标题本来就独占一行）。 */
  .conn-main small.cs-os { display: flex; align-items: center; }
  .icon-btn.sm { padding: 4px; border-radius: 7px; }
  .brain-row { display: flex; align-items: center; gap: 11px; padding: 0; }
  .brain-auth-actions { flex: none; display: inline-flex; align-items: center; gap: 8px; }
  .claude-token-trigger { flex: none; width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center; padding: 0; border: 0; border-radius: 7px; background: transparent; color: var(--dim); cursor: pointer; }
  .claude-token-trigger:hover, .claude-token-trigger.active { background: #f1efe9; color: var(--accent); }
  .claude-token-form { margin: 7px 0 2px 33px; padding: 10px; display: grid; gap: 8px; border: 1px solid var(--border); border-radius: 10px; background: #faf9f6; }
  .claude-token-copy { min-width: 0; display: grid; gap: 3px; }
  .claude-token-copy b { font-size: 11.5px; font-weight: 650; }
  .claude-token-copy small, .claude-token-safe { color: var(--dim); font-size: 9.5px; line-height: 1.45; }
  .claude-token-copy code { padding: 1px 4px; border-radius: 4px; background: #efede7; color: var(--text); font: 500 9.5px/1.3 ui-monospace, SFMono-Regular, Menlo, monospace; }
  .claude-token-entry { display: flex; align-items: center; gap: 6px; }
  .claude-token-entry input { min-width: 0; flex: 1; height: 30px; box-sizing: border-box; padding: 0 9px; border: 1px solid var(--border2); border-radius: 7px; outline: none; background: #fff; color: var(--text); font: 11px ui-monospace, SFMono-Regular, Menlo, monospace; }
  .claude-token-entry input:focus { border-color: #c7a599; box-shadow: 0 0 0 2px rgb(160 60 43 / 8%); }
  .claude-token-entry .btn { flex: none; }
  /* 状态点放进与 .icon-btn 同宽(27px)的居中盒，右缘、图标中心都与本地模型区的刷新图标对齐。 */
  .brain-dot { flex: none; width: 27px; display: inline-flex; align-items: center; justify-content: center; }
  /* 「账号 / 自定义模型」两个折叠头并排一行；展开体各自出现在行下 */
  .cust-heads { display: flex; align-items: center; gap: 16px; }
  /* 当前账号 + 紧凑操作：展开体缩进对齐 chevron 后的文字；完整动作通过 tips 说明。 */
  .brain-auth { display: flex; align-items: center; gap: 6px; margin: 3px 0 1px 15px; min-width: 0; }
  .brain-acct { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--dim); font-size: 11px; }
  .brain-auth-sep { color: var(--faint); font-size: 11px; }
  .brain-auth-icon { flex: none; width: 20px; height: 20px; display: inline-flex; align-items: center; justify-content: center; padding: 0; border: none; border-radius: 5px; background: transparent; color: var(--faint); cursor: pointer; }
  .brain-auth-icon:hover, .brain-auth-icon.active { color: var(--accent); background: #f1efe9; }
  .brain-auth-icon:disabled { opacity: .6; cursor: default; }
  .brain-auth-icon :global(.rfz) { width: 12px; height: 12px; }
  .brain-auth-icon .wr-spin { width: 10px; height: 10px; }
  .cli-new { color: var(--ok); }
  .cli-upg { flex: none; width: 24px; height: 24px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 6px; color: var(--ok); cursor: pointer; }
  .cli-upg:hover:not(:disabled) { background: #f1efe9; }
  .cli-upg:disabled { color: var(--faint); cursor: default; }
  .verify-bad { color: #b45309; font-size: 10px; font-weight: 550; white-space: nowrap; }
  .verify-checking { color: var(--faint); font-size: 10px; white-space: nowrap; }
  .brain-ic { flex: none; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; }
  .brain-main { flex: 1; min-width: 0; } .brain-main b { display: block; font-size: 13.5px; line-height: 1.15; } .brain-main small { display: block; color: var(--dim); font-size: 11px; line-height: 1.1; }
  /* 自定义模型：折叠管理器（对齐大脑名，缩进 33px = 图标 22 + gap 11） */
  /* 大脑列表：整列作为一个 sheet-body 子项，块间距自控（不吃 sheet-body 的 7px gap）。 */
  .brains-list { display: flex; flex-direction: column; gap: 14px; }
  /* 每个大脑（行 + 自定义模型）成一块，块内贴紧。 */
  .brain-block { display: flex; flex-direction: column; gap: 0; }
  /* 未安装行：一键安装挪到状态文字下方（缩进对齐 brain-main），复制小图标同行在旁 */
  .brain-install { display: flex; align-items: center; gap: 5px; margin: 3px 0 2px 33px; }
  .cust { margin: 4px 0 0 33px; }
  .cust-head { display: inline-flex; align-items: center; gap: 5px; background: none; border: none; padding: 0; cursor: pointer; font: inherit; font-size: 11.5px; color: var(--dim); -webkit-appearance: none; appearance: none; }
  .cust-head:hover { color: var(--text); }
  .cust-chev { color: var(--faint); flex: none; transition: transform .12s; }
  .cust-chev:not(.open) { transform: rotate(-90deg); }
  .cust-n { min-width: 15px; height: 15px; padding: 0 4px; display: inline-flex; align-items: center; justify-content: center; background: var(--border); color: var(--dim); border-radius: 8px; font-size: 10px; font-weight: 600; }
  .cust-body { display: flex; flex-direction: column; gap: 5px; margin: 4px 0 4px; }
  .cust-chip { display: flex; align-items: center; gap: 6px; background: #f6f5f1; border: 1px solid var(--border); border-radius: 8px; padding: 4px 6px 4px 9px; }
  .cust-id { flex: 1; min-width: 0; font-family: ui-monospace, monospace; font-size: 11.5px; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .cust-x { flex: none; background: none; border: none; color: var(--faint); font-size: 15px; line-height: 1; padding: 0 3px; border-radius: 5px; cursor: pointer; }
  .cust-x:hover { color: var(--err); background: #fff; }
  .cust-add { display: flex; gap: 6px; align-items: center; }
  .cust-add .tin { flex: 1; min-width: 0; width: auto; }
  .cust-add .btn { flex: none; }
  .hint { color: var(--dim); font-size: 12px; margin: 2px 0; line-height: 1.6; }
  .hint.tos { color: var(--faint); margin-top: 12px; }
  .warn-text { color: var(--warn); }
  .dim { color: var(--dim); }
  .tin { width: 100%; }
  .row-end { display: flex; justify-content: flex-end; align-items: center; gap: 7px; margin-top: 2px; }

  /* 全局共享 hover 气泡：挂到 body，真实测量后避让四边；高于弹窗，永远不被局部层级压住。 */
  :global(.ui-tip) { position: fixed; z-index: 1400; box-sizing: border-box; pointer-events: none; visibility: hidden; background: #26241f; color: #fff; font-size: 11px; line-height: 1.5; padding: 5px 9px; border-radius: 7px; width: max-content; max-width: min(320px, calc(100vw - 16px)); white-space: pre-line; overflow-wrap: anywhere; text-align: left; box-shadow: 0 5px 16px rgba(0, 0, 0, .18); animation: tipin .12s ease-out both; }

  /* 通用文字链按钮（任务卡「查看对话/运行记录」、托管卡操作等）——此前从未定义,一直是系统默认灰凸样式 */
  .link { border: none; background: none; padding: 0; font: inherit; font-size: 12.5px; color: var(--accent); cursor: pointer; white-space: nowrap; }
  .link:hover { text-decoration: underline; }

  /* 弹窗底部操作区统一使用紧凑规格。正文末尾的 row-end 也铺到弹窗边缘，
     看起来和独立 footer 是同一套组件，同时不影响表单内部的小型操作行。 */
  .modal > .sheet-body > .row-end:last-child {
    box-sizing: border-box;
    min-height: 40px;
    flex: none;
    margin: 5px -18px -16px;
    padding: 5px 14px 7px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  .update-center-footer,
  .ed-footer {
    box-sizing: border-box;
    min-height: 40px;
    gap: 6px;
    padding: 5px 14px 7px;
  }
  .modal > .sheet-body > .row-end:last-child .btn,
  .update-center-footer .btn,
  .ed-footer .btn {
    box-sizing: border-box;
    min-height: 28px;
    padding: 4px 10px;
    border-radius: 7px;
    font-size: 11.5px;
    line-height: 1.15;
  }
  @media (max-width: 760px) {
    .center.launch-center { padding: 20px 18px; }
  }
</style>
