<script lang="ts">
  // 分屏里没聚焦的那几台机器：一行窄标题（机器、状态、文件开关、关闭）+ 它自己的终端和文件面板。点一下就成为当前机器。
  import DistroMark from '$lib/components/icons/DistroMark.svelte';
  import TermHost from '$lib/components/remote/TermHost.svelte';
  import SftpPanel from '$lib/components/remote/SftpPanel.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { workbench, startWbResize, filesOn, setFilesOn } from '$lib/state/workbench.svelte.js';
  import { connectionDisplayName, distroOf } from '$lib/state/connections.svelte.js';
  import { closeRemotePane, startRemoteDrag } from '$lib/state/remotePanes.svelte.js';

  let { connId, paneId }: { connId: string; paneId: string } = $props();
  const conn = $derived(app.conns.find((c) => c.id === connId));
  const st = $derived(workbench.termState[connId]);
  // 这个窗格开不开文件面板：按机器记，和它聚焦时的工作台是同一个开关
  const files = $derived(filesOn(connId));
  const toggleFiles = () => setFilesOn(connId, !files);
</script>

<!-- 按住标题拖到别的窗格边上：重排 -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<header class="rp-head" aria-label="拖动以重排窗格" onpointerdown={(e) => startRemoteDrag(e, connId)}>
  <span class="rp-ic"><DistroMark id={distroOf(conn)} size={13} /></span>
  <span class="rp-name" title="{conn?.ssh_user}@{conn?.ssh_host}">{conn ? connectionDisplayName(conn) : connId}</span>
  <span class="rp-st" class:on={st?.on}>{st?.busy ? '连接中…' : st?.on ? '已连接' : '未连接'}</span>
  <span class="grow"></span>
  <button class="rp-tg" class:on={files} onpointerdown={(e) => e.stopPropagation()} aria-pressed={files} aria-label="右侧文件" data-tip={files ? '收起这台的文件面板' : '显示这台的文件面板'} onclick={(e) => { e.stopPropagation(); toggleFiles(); }}>
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none"><rect x="1.8" y="2.6" width="12.4" height="10.8" rx="2" stroke="currentColor" stroke-width="1.3" /><path d="M9.6 2.6v10.8" stroke="currentColor" stroke-width="1.3" /><path d="M9.6 2.6h2.6a2 2 0 0 1 2 2v6.8a2 2 0 0 1-2 2H9.6z" fill="currentColor" opacity=".9" /></svg>
  </button>
  <button class="rp-x" onpointerdown={(e) => e.stopPropagation()} aria-label="关闭这个窗格" data-tip="关闭窗格（终端留在后台，不断开）" onclick={(e) => { e.stopPropagation(); closeRemotePane(paneId); }}>×</button>
</header>
<div class="rp-body">
  <div class="rp-term">
    <TermHost {connId} />
    {#if st?.busy}<div class="rp-connecting"><span class="tc-spin"></span>正在连接 {conn?.ssh_user}@{conn?.ssh_host}…</div>{/if}
  </div>
  {#if files}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="rp-split" title="拖动调整宽度" role="separator" aria-orientation="vertical" onpointerdown={(e) => e.stopPropagation()} onmousedown={(e) => startWbResize(e, 'v')}></div>
    <!-- 在这块文件面板里操作不把这个窗格切成当前机器（不然整块换成完整工作台，面板跟着跳）：文件操作照样作用在这台上 -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="rp-files" onpointerdown={(e) => e.stopPropagation()} onfocusin={(e) => e.stopPropagation()}><SftpPanel {connId} /></div>
  {/if}
</div>

<style>
  /* 高度和下边线跟旁边聚焦窗格的工作台头部（.thread-head.slim，40px + 1px 下边线、rgba(30,25,15,.05)）一致，两边终端才从同一条线开始 */
  .rp-head { flex: none; height: 41px; display: flex; align-items: center; gap: 7px; padding: 0 8px 0 12px; border-bottom: 1px solid rgba(30, 25, 15, .05); background: var(--bg); font-size: 12px; min-width: 0; }
  .rp-ic { display: inline-flex; flex: none; }
  /* 全局那条 translateY(-1px) 是给没有降部的英文小字补的；这里名字是粗体、常带中文，再上移就比字高了 */
  .rp-ic :global(svg.distro) { transform: none; }
  .rp-name { font-weight: 600; color: var(--text); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rp-st { flex: none; color: var(--faint); font-size: 11.5px; }
  .rp-st.on { color: var(--dim); }
  .grow { flex: 1; }
  .rp-x { flex: none; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 6px; color: var(--faint); font-size: 16px; line-height: 1; cursor: pointer; transition: background-color .12s, color .12s; }
  .rp-x:hover { background: #f1efe9; color: var(--text); }
  .rp-body { flex: 1; min-height: 0; display: flex; }
  .rp-term { position: relative; flex: 1; min-width: 0; min-height: 0; overflow: hidden; background: #1c1917; padding: 8px 10px; }
  .rp-files { flex: none; min-height: 0; display: flex; }
  .rp-split { flex: none; width: 5px; margin-right: -5px; cursor: col-resize; background: transparent; z-index: 2; }
  .rp-split:hover { background: var(--accent-soft); }
  .rp-tg { flex: none; width: 24px; height: 24px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 6px; color: var(--faint); cursor: pointer; transition: background-color .12s, color .12s; }
  .rp-tg:hover { background: #f1efe9; color: var(--text); }
  .rp-tg.on { color: var(--text); }
  .rp-term :global(.xterm .xterm-viewport) { background-color: #1c1917; }
  .rp-term :global(.xterm-decoration-overview-ruler) { display: none; }
  .tc-spin { width: 9px; height: 9px; border: 1.5px solid #4b4640; border-top-color: #8b857c; border-radius: 50%; animation: rp-spin .7s linear infinite; }
  @keyframes rp-spin { to { transform: rotate(360deg); } }
  .rp-connecting { position: absolute; left: 14px; top: 12px; display: flex; align-items: center; gap: 7px; color: #8b857c; font-size: 12px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; pointer-events: none; }
</style>
