<script lang="ts">
  import { hideTip } from '$lib/tip';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import ConversationCanvas from '$lib/ConversationCanvas.svelte';
  import ConfirmDialog from '$lib/ConfirmDialog.svelte';
  import Sidebar from '$lib/components/sidebar/Sidebar.svelte';
  import ConversationView, { splitContent } from '$lib/components/conversation/ConversationView.svelte';
  import Welcome from '$lib/components/launcher/Welcome.svelte';
  import Launcher from '$lib/components/launcher/Launcher.svelte';
  import TemplateLibrary from '$lib/components/library/TemplateLibrary.svelte';
  import RemoteWorkbench from '$lib/components/remote/RemoteWorkbench.svelte';
  import RemotePane from '$lib/components/remote/RemotePane.svelte';
  import { remotePanes, initRemotePanes, focusRemotePane, addRemotePane, resizeRemoteLayout, endRemoteDrag } from '$lib/state/remotePanes.svelte.js';
  import PromptLibrary from '$lib/components/library/PromptLibrary.svelte';
  import SearchDialog from '$lib/components/SearchDialog.svelte';
  import SettingsSheet from '$lib/components/settings/SettingsSheet.svelte';
  import UpdateCenter from '$lib/components/settings/UpdateCenter.svelte';
  import SshConnectDialog from '$lib/components/remote/SshConnectDialog.svelte';
  import ConnectionMenu from '$lib/components/sidebar/ConnectionMenu.svelte';
  import ConnectionRemarkDialog from '$lib/components/sidebar/ConnectionRemarkDialog.svelte';
  import SftpContextMenu from '$lib/components/remote/SftpContextMenu.svelte';
  import SftpFileDialog from '$lib/components/remote/SftpFileDialog.svelte';
  import RemoteFileEditor from '$lib/components/remote/RemoteFileEditor.svelte';
  import CloudflareConnectDialog from '$lib/components/cloudflare/CloudflareConnectDialog.svelte';
  import GithubConnectDialog from '$lib/components/github/GithubConnectDialog.svelte';
  import SiteImportDialog from '$lib/components/site/SiteImportDialog.svelte';
  import SiteCreateDialog from '$lib/components/site/SiteCreateDialog.svelte';
  import SiteMakingList from '$lib/components/site/SiteMakingList.svelte';
  import SiteWorkbench from '$lib/components/site/SiteWorkbench.svelte';
  import { initSites } from '$lib/state/sites.svelte.js';
  import { initOps } from '$lib/state/ops.svelte.js';
  import { initMigrate } from '$lib/state/migrate.svelte.js';
  import { initDynamic } from '$lib/state/dynamic.svelte.js';
  import SiteMigrateDialog from '$lib/components/site/SiteMigrateDialog.svelte';
  import { initFleet } from '$lib/state/fleet.svelte.js';
  import SitesOverview from '$lib/components/site/SitesOverview.svelte';
  import ManagedConsole from '$lib/components/site/ManagedConsole.svelte';
  import { initManaged } from '$lib/state/managed.svelte.js';
  import { phone, initPhone } from '$lib/state/phone.svelte.js';
  import { initSuggest } from '$lib/state/suggest.svelte.js';
  import AndroidMark from '$lib/components/icons/AndroidMark.svelte';
  import { openConnectionSetup } from '$lib/state/settings.svelte.js';
  import { initOpsLive } from '$lib/state/turns.svelte.js';
  import DataConnectDialog from '$lib/components/site/DataConnectDialog.svelte';
  import TemplateSaveDialog from '$lib/components/library/TemplateSaveDialog.svelte';
  import TemplateUseDialog from '$lib/components/library/TemplateUseDialog.svelte';
  import PromptEditDialog from '$lib/components/library/PromptEditDialog.svelte';
  import { onCtxMenu, onTipHover, ui, startDrag, toggleRail, initUi } from '$lib/state/ui.svelte.js';
  import { editor, onEditKey, initEditor } from '$lib/state/editor.svelte.js';
  import { isWindows } from '$lib/state/env';
  import { updater, openUpdateCenter, initUpdater } from '$lib/state/updater.svelte.js';
  import { openSearch, initSearch } from '$lib/state/search.svelte.js';
  import { app, finishConfirm, initApp } from '$lib/state/app.svelte.js';
  import { panes, splitConversation, singleConversationDragOver, endConversationDrag, addSplit, focusSplit, initPanes, isPageRef, paneRefLabel, closeSplit, startConversationPointerDrag } from '$lib/state/panes.svelte.js';
  import { leaves } from '$lib/conversationLayout';
  import { modelIcon } from '$lib/state/models';
  import { badImgUrls, initMessages } from '$lib/state/messages.svelte.js';
  import { initConnections, distroOf, connectionDisplayName } from '$lib/state/connections.svelte.js';
  import DistroMark from '$lib/components/icons/DistroMark.svelte';
  import { initSettings } from '$lib/state/settings.svelte.js';
  import { initLauncher } from '$lib/state/launcher.svelte.js';
  import { initPermits } from '$lib/state/permits.svelte.js';
  import { initComposer } from '$lib/state/composer.svelte.js';
  import { initThread } from '$lib/state/thread.svelte.js';
  import { initWorkbench } from '$lib/state/workbench.svelte.js';
  import { initSftp } from '$lib/state/sftp.svelte.js';
  import { initLibrary } from '$lib/state/library.svelte.js';
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
  initRemotePanes();
  initSftp();
  initEditor();
  initLibrary();
  initMessages();
  initTurns();
  initSites();
  initOps();
  initMigrate();
  initDynamic();
  initFleet();
  initManaged();
  initPhone();
  initSuggest();
  initOpsLive();
</script>

<!-- 分屏的一格：对话照旧；页面（新对话 / 全部站点 / 站点制作 / 托管运营 / 站点看板）顶上加一条能拖动的标题栏 -->
{#snippet paneContent(ref: string, paneId: string)}
  {#if isPageRef(ref)}
    {#if leaves(panes.conversationLayout).length > 1}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="page-pane-bar" class:focused={panes.splitFocused === paneId} onpointerdown={(e) => startConversationPointerDrag(e, ref)} data-tip="拖动可以换位置">
        <span class="ppb-t">{paneRefLabel(ref)}</span>
        <button class="ppb-x" data-no-drag aria-label="关闭窗格" data-tip="关闭窗格" onclick={() => closeSplit(paneId)}>×</button>
      </div>
    {/if}
    <div class="page-pane">
      {#if ref === 'page:sites'}<SitesOverview />
      {:else if ref === 'page:making'}<SiteMakingList />
      {:else if ref === 'page:managed'}<ManagedConsole />
      {:else if ref === 'page:launcher'}<Launcher />
      {:else}<SiteWorkbench siteId={ref.slice(6)} />{/if}
    </div>
  {:else}
    {@render splitContent(ref, paneId)}
  {/if}
{/snippet}

{#snippet remoteContent(connId: string, paneId: string)}
  {#if connId === app.activeConnId}<RemoteWorkbench {paneId} />{:else}<RemotePane {connId} {paneId} />{/if}
{/snippet}

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
  <div class="titlebar" data-tauri-drag-region aria-hidden="true" onmousedown={startDrag} style="width:{ui.railCollapsed ? 140 + (updater.topUpdateVisible ? 27 : 0) + (phone.ready.length || phone.pending.length ? 27 : 0) : ui.railWidth}px"></div>

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
    {#if phone.ready.length || phone.pending.length}
      <!-- 连着安卓手机时显示：AI 可以在对话里操作手机、截图 -->
      <button class="wt ph" class:pend={!phone.ready.length} onclick={(e) => { phone.focus = 'phone'; openConnectionSetup(e); }}
        data-tip={phone.ready.length ? `${phone.ready.map((d) => d.model || d.serial).join('、')} 已连上\nAI 可以在对话里操作手机、截图` : '手机已连上，等你在手机上点「允许 USB 调试」'} aria-label="安卓手机">
        <AndroidMark size={17} />
      </button>
    {/if}
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
  {#if app.flash}<div class="flash {app.flashKind}" role="status" aria-live="polite">{app.flash}{#if app.flashRaw}<span class="flash-raw">原文：{app.flashRaw}</span>{/if}</div>{/if}

  <Sidebar />

  {#if remotePanes.dragId && remotePanes.dragPoint}
    {@const dragConn=app.conns.find((c)=>c.id===remotePanes.dragId)}
    {#if dragConn}
      <div class="conversation-drag-ghost" style={`--drag-x:${remotePanes.dragPoint.x}px;--drag-y:${remotePanes.dragPoint.y}px`} aria-hidden="true">
        <span class="conversation-drag-icon"><DistroMark id={distroOf(dragConn)} size={13} /></span>
        <span class="conversation-drag-title">{connectionDisplayName(dragConn)}</span>
        <small class="conversation-drag-sub">{dragConn.ssh_user}@{dragConn.ssh_host}</small>
      </div>
    {/if}
  {/if}

  {#if panes.splitDragId && panes.splitDragPoint && isPageRef(panes.splitDragId)}
    <div class="conversation-drag-ghost" style={`--drag-x:${panes.splitDragPoint.x}px;--drag-y:${panes.splitDragPoint.y}px`} aria-hidden="true">
      <span class="conversation-drag-title">{paneRefLabel(panes.splitDragId)}</span>
      <small class="conversation-drag-sub">拖到右边或下边分屏</small>
    </div>
  {:else if panes.splitDragId && panes.splitDragPoint}
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
    {#if remotePanes.singleEdge && !remotePanes.visible}<div class={`single-split-preview ${remotePanes.singleEdge}`} aria-hidden="true"><span>分屏视图</span></div>{/if}
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
      <ConversationCanvas layout={panes.conversationLayout} focused={panes.splitFocused} dragging={!!panes.splitDragId} dragConversation={panes.splitDragId} preview={panes.splitPointerPreview} content={paneContent}
        onfocus={focusSplit} ondrop={(target,edge)=>{const id=panes.splitDragId;endConversationDrag();void addSplit(id,edge,target);}} onresize={(layout)=>panes.conversationLayout=layout}/>
    {:else if !app.activeConn}
      <Welcome />

    {:else if app.activeConn.kind === 'sites' && app.view === 'making'}
      <SiteMakingList />

    {:else if app.activeConn.kind === 'sites' && app.view === 'managed'}
      <ManagedConsole />

    {:else if app.activeConn.kind === 'sites' && app.view !== 'launcher'}
      <SitesOverview />

    {:else if app.view === 'site' && app.isSiteConn}
      <SiteWorkbench />

    {:else if app.view === 'launcher'}
      <Launcher />

    {:else if app.view === 'templates'}
      <TemplateLibrary />

    {:else if remotePanes.visible && remotePanes.layout}
      <!-- 远程分屏：聚焦的窗格是完整工作台，其余窗格只显示各自的终端 -->
      <div class="remote-split"><ConversationCanvas layout={remotePanes.layout} focused={remotePanes.focused} dragging={!!remotePanes.dragId} dragConversation={remotePanes.dragId} preview={remotePanes.preview} content={remoteContent}
        onfocus={focusRemotePane} ondrop={(target,edge)=>{const id=remotePanes.dragId;endRemoteDrag();addRemotePane(id,edge,target);}} onresize={resizeRemoteLayout}/></div>

    {:else if app.view === 'remote'}
      <RemoteWorkbench />

    {:else if app.view === 'prompts'}
      <PromptLibrary />

    {:else}
      <ConversationView />
    {/if}
  </section>
</main>

<SearchDialog />
<SettingsSheet />
<UpdateCenter />
<SshConnectDialog />
<ConnectionMenu />
<ConnectionRemarkDialog />
<SftpContextMenu />
<SftpFileDialog />
<RemoteFileEditor />
<CloudflareConnectDialog />
<GithubConnectDialog />
<SiteImportDialog />
<SiteCreateDialog />
<SiteMigrateDialog />
<DataConnectDialog />
<TemplateSaveDialog />
<TemplateUseDialog />
<PromptEditDialog />

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
  .conversation-drag-ghost{position:fixed;z-index:1000;left:clamp(12px,calc(var(--drag-x) + 14px),calc(100vw - 312px))!important;top:clamp(12px,calc(var(--drag-y) + 12px),calc(100vh - 50px))!important;width:300px;max-width:calc(100vw - 24px);height:38px;box-sizing:border-box;display:flex;align-items:center;gap:8px;padding:0 12px;background:color-mix(in srgb,var(--bg,#fff) 96%,transparent);border:1px solid var(--border,#e7e5df);border-radius:8px;box-shadow:0 8px 24px #2d29231c;pointer-events:none;color:var(--fg,#282723)}
  .conversation-drag-icon{display:inline-flex;align-items:center;line-height:0;flex:none}
  /* 分屏里的页面：顶上一条细标题栏（拖动换位置、× 关掉这一格），下面是页面本身 */
  .page-pane-bar{flex:none;display:flex;align-items:center;gap:8px;height:32px;padding:0 8px 0 16px;border-bottom:1px solid var(--border);background:var(--rail);cursor:grab;user-select:none}
  .page-pane-bar.focused{background:var(--card)}
  .page-pane-bar:active{cursor:grabbing}
  .ppb-t{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px;font-weight:500;color:var(--dim)}
  .page-pane-bar.focused .ppb-t{color:var(--text)}
  .ppb-x{flex:none;width:22px;height:22px;display:inline-flex;align-items:center;justify-content:center;border:0;border-radius:6px;background:none;color:var(--faint);font-size:16px;line-height:1;cursor:pointer}
  .ppb-x:hover{background:var(--accent-soft);color:var(--text)}
  .page-pane{flex:1;min-height:0;min-width:0;display:flex;flex-direction:column;overflow:hidden}
  /* 远程分屏：分隔线夹在两块黑底终端之间，用细的深色线，不要一道亮白缝 */
  .remote-split{display:contents;--pane-divider:#3a3632;--pane-divider-hover:#5a544c;--pane-divider-size:.5px}
  .conversation-drag-sub{flex:none;margin-left:auto;font-size:11px;color:var(--faint,#9a958c)}
  .conversation-drag-title{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px;font-weight:500}
  .single-split-preview{position:absolute;z-index:100;pointer-events:none;box-sizing:border-box;display:grid;place-items:center;background:color-mix(in srgb,var(--bg,#fff) 60%,transparent);backdrop-filter:blur(4px);-webkit-backdrop-filter:blur(4px);border:2px solid #2f80ed;border-radius:8px;box-shadow:0 0 0 1px #2f80ed24}
  .single-split-preview.left{inset:0 50% 0 0}
  .single-split-preview.right{inset:0 0 0 50%}
  .single-split-preview.top{inset:0 0 50%}
  .single-split-preview.bottom{inset:50% 0 0}
  .single-split-preview span{padding:6px 12px;border-radius:18px;background:#2f80ed;color:#fff;font-size:12px;font-weight:500;box-shadow:0 4px 14px #1a5ca733}
  /* ★ 上移 1px 不是玄学：align-items:center 对齐的是**盒子**，而文字行盒底下有一截没人用的
     降部空间（Ubuntu/22.04 这类没有 g/p/y），于是「看得见的字」整体偏上、图标按盒居中就显得低。
     发行版图标是纯圆/纯环，没有基线可言，只能按视觉中线手动补这 1px。 */
  :global(svg.distro) { flex: none; transform: translateY(-1px); }
  :global(:root) {
    /* 主区纯白；侧栏是一点点米灰，两栏之间只靠一条发丝线（见 Sidebar 的 .rail）。
       卡片 / 输入框和主区同为白色，靠边框分开。 */
    --bg: #ffffff; --rail: #f9f9f7; --card: #ffffff;
    /* 浮层实底（更新吐司/附件 chip 等用）：此前未定义导致 var(--panel) 落空、浮层透明。 */
    --panel: #ffffff;
    --border: #ecebe6; --border2: #e1dfd8;
    --text: #26241f; --dim: #6f6b62; --faint: #a29d93;
    /* 强调色走暖黑/灰（Codex 式安静路线），不用蓝紫；风险色（红/琥珀）另有专用变量。 */
    --accent: #33302a; --accent-h: #1d1b17; --accent-soft: #edece7; --soft: #edece7;
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
  .wt.ph { position: relative; }
  /* 还没在手机上点「允许」：图标变淡 */
  .wt.ph.pend { opacity: .45; }
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
  /* ---- 主区 ---- */
  .main { flex: 1; position: relative; display: flex; flex-direction: column; min-width: 0; padding-top: 0; }
  /* 吐司：pre-wrap 让「——详情——」多行错误可读；err 形态限高可滚动（安装失败输出末尾等） */
  .flash { position: fixed; top: 40px; left: 50%; transform: translateX(-50%); z-index: 1200; background: #14231a; color: #fff; padding: 9px 16px; border-radius: 10px; font-size: 13px; box-shadow: var(--shadow); max-width: 70%; white-space: pre-wrap; overflow-wrap: anywhere; -webkit-app-region: no-drag; }
  .flash.err { max-height: 42vh; overflow-x: hidden; overflow-y: auto; }
  .flash-raw { display: block; margin-top: 6px; font-size: 11px; opacity: .75; }
  .imgtip { position: fixed; z-index: 130; transform: translate(-50%, -100%); padding: 5px; border-radius: 10px; background: #fff; border: 1px solid rgba(0, 0, 0, .1); box-shadow: 0 8px 24px rgba(0, 0, 0, .18); pointer-events: none; visibility: hidden; }
  .imgtip.ready { visibility: visible; animation: tipin .12s ease-out both; }
  .imgtip.below { transform: translate(-50%, 0); }
  /* min-* + contain：极端长宽比（整页截图/超宽横幅）装进保底盒子里，不再缩成一条细缝 */
  .imgtip img { display: block; max-width: 320px; max-height: 240px; min-width: 140px; min-height: 90px; object-fit: contain; border-radius: 6px; background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 14px 14px; }
  .lightbox { position: fixed; inset: 0; z-index: 140; background: rgba(24, 22, 18, .72); display: flex; align-items: center; justify-content: center; cursor: zoom-out; }
  .lightbox img { max-width: 86vw; max-height: 86vh; border-radius: 12px; background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 16px 16px; box-shadow: 0 24px 64px rgba(0, 0, 0, .4); }
  @keyframes tipin { from { opacity: 0; } to { opacity: 1; } }
  .flash.err { background: var(--err); }
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
  /* 全局共享 hover 气泡：挂到 body，真实测量后避让四边；高于弹窗，永远不被局部层级压住。 */
  :global(.ui-tip) { position: fixed; z-index: 1400; box-sizing: border-box; pointer-events: none; visibility: hidden; background: #26241f; color: #fff; font-size: 11px; line-height: 1.5; padding: 5px 9px; border-radius: 7px; width: max-content; max-width: min(320px, calc(100vw - 16px)); white-space: pre-line; overflow-wrap: anywhere; text-align: left; box-shadow: 0 5px 16px rgba(0, 0, 0, .18); animation: tipin .12s ease-out both; }
</style>
