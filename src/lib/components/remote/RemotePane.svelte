<script lang="ts">
  // 分屏里没聚焦的那几台机器：一行窄标题（机器、状态、关闭）+ 它自己的终端。点一下就成为当前机器。
  import DistroMark from '$lib/components/icons/DistroMark.svelte';
  import TermHost from '$lib/components/remote/TermHost.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { workbench } from '$lib/state/workbench.svelte.js';
  import { connectionDisplayName, distroOf } from '$lib/state/connections.svelte.js';
  import { closeRemotePane } from '$lib/state/remotePanes.svelte.js';

  let { connId, paneId }: { connId: string; paneId: string } = $props();
  const conn = $derived(app.conns.find((c) => c.id === connId));
  const st = $derived(workbench.termState[connId]);
</script>

<header class="rp-head">
  <span class="rp-ic"><DistroMark id={distroOf(conn)} size={13} /></span>
  <span class="rp-name" title="{conn?.ssh_user}@{conn?.ssh_host}">{conn ? connectionDisplayName(conn) : connId}</span>
  <span class="rp-st" class:on={st?.on}>{st?.busy ? '连接中…' : st?.on ? '已连接' : '未连接'}</span>
  <span class="grow"></span>
  <button class="rp-x" onpointerdown={(e) => e.stopPropagation()} aria-label="关闭这个窗格" data-tip="关闭窗格（终端留在后台，不断开）" onclick={(e) => { e.stopPropagation(); closeRemotePane(paneId); }}>×</button>
</header>
<div class="rp-term">
  <TermHost {connId} />
  {#if st?.busy}<div class="rp-connecting"><span class="tc-spin"></span>正在连接 {conn?.ssh_user}@{conn?.ssh_host}…</div>{/if}
</div>

<style>
  .rp-head { flex: none; height: 32px; display: flex; align-items: center; gap: 7px; padding: 0 8px 0 12px; border-bottom: 1px solid var(--border); background: var(--bg); font-size: 12px; min-width: 0; }
  .rp-ic { display: inline-flex; flex: none; }
  .rp-name { font-weight: 600; color: var(--text); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rp-st { flex: none; color: var(--faint); font-size: 11.5px; }
  .rp-st.on { color: var(--dim); }
  .grow { flex: 1; }
  .rp-x { flex: none; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 6px; color: var(--faint); font-size: 16px; line-height: 1; cursor: pointer; transition: background-color .12s, color .12s; }
  .rp-x:hover { background: #f1efe9; color: var(--text); }
  .rp-term { position: relative; flex: 1; min-height: 0; overflow: hidden; background: #1c1917; padding: 8px 10px; }
  .rp-term :global(.xterm .xterm-viewport) { background-color: #1c1917; }
  .rp-term :global(.xterm-decoration-overview-ruler) { display: none; }
  .tc-spin { width: 9px; height: 9px; border: 1.5px solid #4b4640; border-top-color: #8b857c; border-radius: 50%; animation: rp-spin .7s linear infinite; }
  @keyframes rp-spin { to { transform: rotate(360deg); } }
  .rp-connecting { position: absolute; left: 14px; top: 12px; display: flex; align-items: center; gap: 7px; color: #8b857c; font-size: 12px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; pointer-events: none; }
</style>
