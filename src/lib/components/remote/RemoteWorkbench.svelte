<script lang="ts">
  import Dropdown from '$lib/Dropdown.svelte';
  import ModelFx from '$lib/ModelFx.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import SshMark from '$lib/components/icons/SshMark.svelte';
  import { convPane } from '$lib/components/conversation/ConversationView.svelte';
  import DistroMark from '$lib/components/icons/DistroMark.svelte';
  import SftpPanel from '$lib/components/remote/SftpPanel.svelte';
  import TermHost from '$lib/components/remote/TermHost.svelte';
  import { closeRemotePane, startRemoteDrag } from '$lib/state/remotePanes.svelte.js';
  // 分屏时作为聚焦窗格显示：多一个关闭按钮
  let { paneId = '' }: { paneId?: string } = $props();
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { app } from '$lib/state/app.svelte.js';
  import { workbench, reconnectTerm, toggleWbTerm, toggleWbChat, toggleWbFiles, missingSshCredential, repairActiveSshCredential, startWbResize } from '$lib/state/workbench.svelte.js';
  import { composer, onComposerDragOver, onComposerDragLeave, onComposerDrop, removeAttachment, autogrow, onComposerPaste, onComposerKey, attachFiles, composerGrown } from '$lib/state/composer.svelte.js';
  import AttachMenu from '$lib/components/common/AttachMenu.svelte';
  import AttachTray from '$lib/components/common/AttachTray.svelte';
  // 输入框空着时整块只有一行（文字和底栏按钮并排）；打字超过一行才展开成两层，和对话输入框一样
  let grown = $state(false);
  const compact = $derived((!launcher.lDraft || !grown) && !launcher.lDraft.includes('\n') && !composer.attachments.length && !composer.attaching);
  import { launcher, pickCombo, persistLauncherEffort, setFast } from '$lib/state/launcher.svelte.js';
  import { startChat } from '$lib/state/turns.svelte.js';
  import { distroOf, connectionDisplayName } from '$lib/state/connections.svelte.js';
  import { permOptsFor, permTone, permTipFor, brainUsable, brainSetupHint } from '$lib/state/models';
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- 工作台头部只留一行（机器地址 + 状态）：标题「远程连接」是废话——侧栏已经写着你在哪台机器上。 -->
<header class="thread-head slim" class:in-pane={!!paneId} data-tauri-drag-region onmousedown={startDrag}>
  <!-- 分屏时按住这一行拖去别的窗格边上重排（role=button 让 startDrag 不去拖窗口） -->
  <div class="th-info" role={paneId ? 'button' : undefined} aria-label={paneId ? '拖动以重排窗格' : undefined}
    onpointerdown={(e) => { if (paneId && app.activeConnId) startRemoteDrag(e, app.activeConnId); }}>{#if paneId && app.activeConn}<span class="wb-name"><DistroMark id={distroOf(app.activeConn)} size={13} /><b>{connectionDisplayName(app.activeConn)}</b></span>{/if}<small class="rhead-line">{app.activeConn?.ssh_user}@{app.activeConn?.ssh_host}:{app.activeConn?.ssh_port} · {workbench.termOn ? '已连接' : '未连接'}
    <button class="th-rfz" data-tip={workbench.termBusy ? '连接中…' : '重新连接'} aria-label="重新连接" disabled={workbench.termBusy} onclick={() => reconnectTerm()}><RefreshIcon spinning={workbench.termBusy} /></button>
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
      <SshMark size={15} />
    </button>
    <button class="wb-tg" class:on={workbench.wb.chat} aria-pressed={workbench.wb.chat} data-tip="底部对话框" aria-label="底部对话框" onclick={toggleWbChat}>
      <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="1.8" y="2.6" width="12.4" height="10.8" rx="2" stroke="currentColor" stroke-width="1.3" /><path d="M1.8 9.8h12.4" stroke="currentColor" stroke-width="1.3" /><rect x="1.8" y="9.8" width="12.4" height="3.6" fill="currentColor" opacity=".9" /></svg>
    </button>
    <button class="wb-tg" class:on={workbench.wb.files} aria-pressed={workbench.wb.files} data-tip="右侧文件" aria-label="右侧文件" onclick={toggleWbFiles}>
      <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="1.8" y="2.6" width="12.4" height="10.8" rx="2" stroke="currentColor" stroke-width="1.3" /><path d="M9.6 2.6v10.8" stroke="currentColor" stroke-width="1.3" /><path d="M9.6 2.6h2.6a2 2 0 0 1 2 2v6.8a2 2 0 0 1-2 2H9.6z" fill="currentColor" opacity=".9" /></svg>
    </button>
    {#if paneId}<button class="wb-tg wb-close" data-tip="关闭窗格（终端留在后台，不断开）" aria-label="关闭这个窗格" onclick={() => closeRemotePane(paneId)}>×</button>{/if}
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
      {#if app.activeConnId}<TermHost connId={app.activeConnId} />{/if}
      {#if workbench.termBusy}
        <!-- 建连要走网络+认证，一两秒是常事，别让黑框干愣着。浮层而非写进终端：见 startTerm。 -->
        <div class="term-connecting"><span class="tc-spin"></span>正在连接 {app.activeConn?.ssh_user}@{app.activeConn?.ssh_host}…</div>
      {:else if workbench.termErr && missingSshCredential(workbench.termErr)}
        <button type="button" class="term-credential-repair" onclick={repairActiveSshCredential}>连接凭据缺失 · 补录密码</button>
      {/if}
    </div>
    {#if workbench.wb.chat}
      <!-- 还没有对话时底部只有一个输入框：高度跟着输入框走，终端吃满上面；有了对话才用可拖的固定高度 -->
      {@const hasConv = !!app.activeConv && app.activeConv.conn_id === app.activeConnId}
      {#if workbench.wb.term && hasConv}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div class="wb-split h" title="拖动调整高度" role="separator" aria-orientation="horizontal" onmousedown={(e) => startWbResize(e, 'h')}></div>
      {/if}
      <!-- 命令行关着时对话吃满整个主区（固定高度只在两者共存时才有意义） -->
      <div class="wb-chat" class:solo={!workbench.wb.term} style={workbench.wb.term && hasConv ? `height:${workbench.wb.chatH}px` : ''}>
        {#if hasConv && app.activeConv}
          {@render convPane(app.activeConv)}
        {:else}
          <!-- 还没有对话：这里就是起点（发出去就新建一条，走 startChat 那套）。
               外层必须是 .composer-wrap —— 输入框的样式挂在 `.composer.big, .composer-wrap .composer` 上，
               裸 .composer 一条都套不上（就是那次「样式缺失」）。 -->
          <div class="wb-start composer-wrap">
            <div class="composer" class:compact class:fileover={composer.composerFileOver} role="group" aria-label="远程消息输入"
              ondragover={onComposerDragOver} ondragleave={onComposerDragLeave} ondrop={onComposerDrop}>
              <AttachTray items={composer.attachments} uploading={composer.attaching} onremove={removeAttachment} />
              <span class="cb-plus"><AttachMenu onfiles={(fs) => attachFiles(fs)} /></span>
              <textarea bind:this={workbench.wbStartEl} bind:value={launcher.lDraft} use:autogrow rows="1" placeholder="让 AI 在这台机器上做点什么…例如：看看装了什么，帮我装上 Docker"
                oninput={(e) => composerGrown(e.currentTarget, grown, (v) => (grown = v))}
                oncompositionstart={() => (composer.composing = true)} oncompositionend={() => (composer.composing = false)}
                onpaste={onComposerPaste}
                onkeydown={(e) => onComposerKey(e, () => void startChat())}></textarea>
              <div class="composer-bar">
                <div class="cb-left"><span class="ssh-target" title={app.activeConn?.ssh_os || '远端系统未知'}><DistroMark id={distroOf(app.activeConn)} size={13} /><span class="st-addr">{app.activeConn?.ssh_user}@{app.activeConn?.ssh_host}</span></span></div>
                <div class="cb-right">
                  <Dropdown compact bind:value={launcher.lPerm} options={permOptsFor(launcher.lBrain, true)} tone={permTone(launcher.lPerm)} tip={permTipFor(launcher.lBrain, launcher.lPerm, true)} />
                  <ModelFx options={launcher.comboOpts} value={`${launcher.lBrain}::${launcher.lModel}`} effort={launcher.lEffort} fast={launcher.lFast} fastOk={app.brains?.claude_fast_ok ?? false} onpick={pickCombo} oneffort={persistLauncherEffort} onfast={setFast} />
                  <button class="send" onclick={() => void startChat()} disabled={(!launcher.lDraft.trim() && !composer.attachments.length) || composer.attaching || !brainUsable(launcher.lBrain)} title="发送（Enter）">↑</button>
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
    <SftpPanel />
  {/if}
</div>

<!-- 顶栏的一个负载读数：标签 + 百分比 + 一条细底纹。
     pct=null＝还没算出来（CPU 要两次采样求差，见 pollStats）。 -->
{#snippet loadStat(label: string, pct: number | null, tip: string)}
  <span class="hstat" class:warn={pct !== null && pct >= 75} class:hot={pct !== null && pct >= 90} data-tip={tip}>
    <span class="hstat-l">{label}</span>
    <span class="hstat-v">{pct === null ? '—' : pct + '%'}</span>
  </span>
{/snippet}

<style>
  :global(.canvas.multi .pane) .composer-wrap{padding:8px 16px 14px;max-height:55%;overflow-y:auto}
  :global(.canvas.multi .pane) .composer-bar{flex-wrap:wrap}
  :global(.canvas.multi .pane) .cb-right{margin-left:auto;max-width:100%;flex-wrap:wrap}
  /* 分屏时窗格标题已经写着是哪台机器，输入框里就不再重复，给权限和模型腾地方 */
  :global(.canvas.multi .pane) .ssh-target{display:none}
  /* 上面那条 55% 是给对话里的输入框的；这里的起点区要撑满底部对话框，输入框才沉到底 */
  :global(.canvas.multi .pane) .wb-start.composer-wrap{max-height:none}
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
  /* ★ 要带上 .composer-wrap：后面那条 `.composer-wrap { flex: none }` 写在后头会盖掉 flex:1，
     起点区就不撑满，输入框贴在终端下面而不是沉到底。 */
  .wb-start.composer-wrap { flex: 1; }
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
  /* xterm 6 的滚动条是自绘的（宽度来自 JS 的 overviewRuler 选项）。这里只把 viewport 的底色抹成终端底色：
     xterm.css 给它钉了 background:#000，不盖掉的话滚动条那一条会和终端底色差一截。
     多带一级 .xterm 是为了与 xterm.css 的 `.xterm .xterm-viewport` 同分时稳赢，不必 !important。
     注意：这两条选择器里的类名是 xterm 运行时才加的，源码里搜不到——清理样式时别当成死代码删掉。 */
  :global(.term-wrap .xterm .xterm-viewport) { background-color: #1c1917; }
  /* 装饰栏：设 overviewRuler.width 时被顺带打开，和滚动条完全重叠，却会在左沿画一条贯穿全高的
     1px 竖线（就是终端右侧那条白线）。我们从不用 decoration → 整个关掉。 */
  :global(.term-wrap .xterm-decoration-overview-ruler) { display: none; }
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
  .wb-close { font-size: 17px; line-height: 1; }
  /* 分屏时和旁边的窗格一样，先写机器名 */
  /* 窗格窄的时候先收地址那一行，机器名最后才省略 */
  .th-info:has(.wb-name) { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .th-info:has(.wb-name) small.rhead-line { flex: 0 100 auto; min-width: 0; }
  .wb-name { display: inline-flex; align-items: center; gap: 7px; flex: 0 1 auto; min-width: 0; font-size: 12px; }
  .wb-name b { font-weight: 600; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .wb-name :global(svg.distro) { flex: none; transform: none; }
  .tc-spin { width: 9px; height: 9px; border: 1.5px solid #4b4640; border-top-color: #8b857c; border-radius: 50%; animation: spin .7s linear infinite; }
  /* 远程视图头部：终端/文件 页签 */
  .rhead-acts { display: flex; align-items: center; gap: 10px; -webkit-app-region: no-drag; }
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
  .ssh-target { min-width: 0; max-width: 100%; }
  .st-addr { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ssh-target :global(svg.distro) { transform: translateY(1px); }
  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  textarea { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1.5px solid var(--border2); border-radius: 8px; padding: 5px 10px; }
  textarea:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }
  /* 输入框（仿 Claude Code：整块圆角边框，聚焦时高亮，发送按钮嵌在框内） */
  .composer-wrap .composer { position: relative; background: #fff; border: 1px solid var(--border2); border-radius: 12px; box-shadow: none; transition: border-color .12s, box-shadow .12s; }
  .composer-wrap .composer:focus-within { border-color: #b7b2a6; box-shadow: none; }
  .composer-wrap .composer.fileover { border-color: var(--accent); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 16%, transparent); }
  .composer-wrap .composer.fileover::after {
    content: '松开即可添加文件'; position: absolute; inset: 4px; z-index: 8; display: flex; align-items: center; justify-content: center;
    border: 1px dashed color-mix(in srgb, var(--accent) 58%, transparent); border-radius: 9px;
    background: color-mix(in srgb, #fff 92%, var(--accent)); color: var(--accent); font-size: 13px; font-weight: 600; pointer-events: none;
  }
  .composer-wrap textarea { width: 100%; resize: none; border: none; background: none; box-shadow: none; padding: 14px 52px 14px 17px; font-size: 14px; line-height: 1.6; max-height: 200px; display: block; }
  .composer-wrap textarea:focus { outline: none; box-shadow: none; border: none; }
  .composer .send { position: absolute; right: 9px; bottom: 9px; width: 32px; height: 32px; border-radius: 50%; border: none; background: var(--accent); color: #fff; font-size: 16px; cursor: pointer; display: flex; align-items: center; justify-content: center; transition: background .12s, transform .08s; }
  .composer .send:hover { background: var(--accent-h); }
  .composer .send:active { transform: scale(.92); }
  .composer .send:disabled { background: #dcdad2; cursor: default; transform: none; }
  /* composer：竖排 textarea + 底栏（启动页站点在左；会话页只读厂商在左，模型/发送在右）。 */
  .composer-wrap .composer { display: flex; flex-direction: column; }
  .composer-wrap textarea { padding: 12px 16px 6px 16px; }
  .composer-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 8px 8px; }
  .cb-left { display: flex; align-items: center; gap: 1px; flex: 1; min-width: 0; }
  .cb-right { display: flex; align-items: center; gap: 3px; flex: none; }
  .composer-bar .send { position: static; width: 30px; height: 30px; margin-left: 3px; font-size: 15px; }
  /* 一行模式：文字和右边的按钮并排，左边的机器地址先收起来（展开成两层时再出现） */
  .composer-wrap .composer.compact { flex-direction: row; align-items: center; }
  .composer-wrap .composer.compact textarea { flex: 1; min-width: 0; padding: 12px 8px 12px 16px; }
  .composer-wrap .composer.compact textarea::placeholder { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .composer.compact .composer-bar { flex: none; padding: 0 8px 0 0; }
  .composer.compact .cb-left { display: none; }
  /* 「+」在左下角（和 Claude 客户端一样）：两层时贴在底栏最左、底栏左边让出位置；一行模式时排在文字前面 */
  .cb-plus { position: absolute; left: 8px; bottom: 8px; z-index: 1; display: flex; }
  .composer:has(.cb-plus) .cb-left { padding-left: 32px; }
  .composer.compact .cb-plus { position: static; flex: none; margin-left: 8px; }
  .composer.compact textarea { padding-left: 6px !important; }
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
  .thread-head:global(.elevated) { border-bottom-color: transparent; }
  .thread-head:global(.elevated)::after { opacity: 1; }
  /* 侧栏收起：红绿灯 + 悬浮的折叠/搜索钮压在内容区左上，页头统一左让位（全部视图受益）。
     发现更新时顶部临时增加更新入口，页头再让出一个按钮宽度，避免压住页面标题图标。 */
  :global(.app.rail-collapsed) .thread-head { padding-left: 140px; }
  :global(.app.rail-collapsed.fs) .thread-head, :global(.app.rail-collapsed.win) .thread-head { padding-left: 100px; }
  :global(.app.rail-collapsed.top-update-visible) .thread-head { padding-left: 167px; }
  :global(.app.rail-collapsed.top-update-visible.fs) .thread-head,
  :global(.app.rail-collapsed.top-update-visible.win) .thread-head { padding-left: 127px; }
  /* 分屏里：左右内边距和旁边窗格的标题（RemotePane .rp-head：12px / 8px）一致，× 和图标才在同一位置。
     侧栏收起时上面那几条（给红绿灯让位）分数更高，照样生效。 */
  .thread-head.in-pane { padding-left: 12px; padding-right: 8px; }
  .thread-head.in-pane .wb-close { width: 22px; height: 22px; }
  .th-info { min-width: 0; flex: 1 1 auto; overflow: hidden; }
  .th-info small { display: block; min-width: 0; overflow: hidden; color: var(--dim); font-size: 12px; margin-top: 2px; text-overflow: ellipsis; white-space: nowrap; }
  .th-info small :global(svg), .th-info small :global(img) { vertical-align: -2px; }
  .composer-wrap { flex: none; padding: 10px 24px 20px; }
  .composer-wrap .composer { max-width: 760px; margin: 0 auto; }
  .hint { color: var(--dim); font-size: 12px; margin: 2px 0; line-height: 1.6; }
  .warn-text { color: var(--warn); }
</style>
