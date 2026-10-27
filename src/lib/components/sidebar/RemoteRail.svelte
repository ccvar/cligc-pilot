<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Conversation } from '$lib/types';
  import DistroMark from '$lib/components/icons/DistroMark.svelte';
  import { mask } from '$lib/mask';
  import { app } from '$lib/state/app.svelte.js';
  import { sidebar } from '$lib/state/sidebar.svelte.js';
  import { connections, selectConn, openConnCtx, connectionDisplayName, distroOf, sshSub, remoteConns } from '$lib/state/connections.svelte.js';
  import { startRemoteDrag } from '$lib/state/remotePanes.svelte.js';

  // 远程模式的左栏：顶上「新建远程连接」（在 Sidebar），下面列出全部远程连接（和站点模式一样），当前这台展开它的对话。
  let { convRow }: { convRow: Snippet<[Conversation]> } = $props();
  const list = $derived(remoteConns());
  // 每台机器有没有正在跑的对话
  const busy = $derived(new Set(app.convos.filter((c) => c.status === 'running').map((c) => c.conn_id)));
</script>

<div class="rr">
  {#each list as c (c.id)}
    {@const on = c.id === app.activeConnId}
    <div class="host" class:on={on && app.view === 'remote'} class:ctx={on && app.view !== 'remote'} class:menu={connections.connCtx?.conn.id === c.id}
      role="button" tabindex="0" title="{c.ssh_user}@{c.ssh_host}{c.ssh_port && c.ssh_port !== 22 ? ':' + c.ssh_port : ''}"
      onpointerdown={(e) => startRemoteDrag(e, c.id)} onclick={() => selectConn(c.id)} onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); void selectConn(c.id); } }} oncontextmenu={(e) => openConnCtx(e, c)}>
      <span class="ic"><DistroMark id={distroOf(c)} size={14} /></span>
      <span class="main"><b>{mask(connectionDisplayName(c))}</b><small>{mask(sshSub(c))}</small></span>
      {#if busy.has(c.id)}<span class="run" title="有对话正在进行"></span>{/if}
    </div>
    {#if on}
      {@const convs = sidebar.grouped.flatMap((g) => g.subs.flatMap((x) => x.items))}
      {#if convs.length}<div class="convs">{#each convs as conv (conv.id)}{@render convRow(conv)}{/each}</div>{/if}
    {/if}
  {/each}
</div>

<style>
  .rr { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 0 8px; }
  .host { margin: 0 8px; min-height: 40px; display: flex; align-items: center; gap: 9px; padding: 4px 8px 4px 10px; border-radius: 9px; cursor: pointer; min-width: 0; transition: background-color .12s; }
  .host:hover, .host.menu { background: #f5f4f1; }
  .host.on { background: #f0efeb; }
  .host.ctx .ic { color: var(--text); }
  .ic { display: inline-flex; flex: none; color: var(--dim); }
  .main { flex: 1; min-width: 0; display: flex; flex-direction: column; line-height: 1.3; }
  .main b { font-size: 12.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .main small { font-size: 11px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .run { flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: pulse 1.1s infinite; }
  .convs { padding: 1px 0 6px 12px; animation: rr-in .18s cubic-bezier(.2, .7, .2, 1) backwards; }
  @keyframes pulse { 50% { opacity: .35; } }
  @keyframes rr-in { from { opacity: 0; transform: translateY(-3px); } }
  @media (prefers-reduced-motion: reduce) { .convs { animation: none; } }
</style>
