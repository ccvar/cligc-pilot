<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Conversation } from '$lib/types';
  import DistroMark from '$lib/components/icons/DistroMark.svelte';
  import { mask } from '$lib/mask';
  import { app } from '$lib/state/app.svelte.js';
  import { sidebar } from '$lib/state/sidebar.svelte.js';
  import { connections, selectConn, openConnCtx, connectionDisplayName, distroOf, sshSub, remoteConns } from '$lib/state/connections.svelte.js';
  import { startRemoteDrag } from '$lib/state/remotePanes.svelte.js';
  import { workbench } from '$lib/state/workbench.svelte.js';

  // 远程模式的左栏：顶上「新建远程连接」（在 Sidebar），下面列出全部远程连接（和站点模式一样），当前这台展开它的对话。
  let { convRow }: { convRow: Snippet<[Conversation]> } = $props();
  const list = $derived(remoteConns());
  // 每台机器有没有正在跑的对话
  const busy = $derived(new Set(app.convos.filter((c) => c.status === 'running').map((c) => c.conn_id)));
  // 终端连接状态：切到别的机器时终端只是挪到后台、不断开，回来还是原来那个会话；名字后面的点标出现在连着的
  const termOf = (id: string) => workbench.termState[id];
</script>

<div class="rr">
  {#each list as c (c.id)}
    {@const on = c.id === app.activeConnId}
    {@const t = termOf(c.id)}
    <div class="host" class:on={on && app.view === 'remote'} class:ctx={on && app.view !== 'remote'} class:menu={connections.connCtx?.conn.id === c.id}
      role="button" tabindex="0" title="{c.ssh_user}@{c.ssh_host}{c.ssh_port && c.ssh_port !== 22 ? ':' + c.ssh_port : ''}"
      onpointerdown={(e) => startRemoteDrag(e, c.id)} onclick={() => selectConn(c.id)} onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); void selectConn(c.id); } }} oncontextmenu={(e) => openConnCtx(e, c)}>
      <span class="ic"><DistroMark id={distroOf(c)} size={14} /></span>
      <span class="main"><b><span class="nm">{mask(connectionDisplayName(c))}</span></b><small>{mask(sshSub(c))}</small></span>
      {#if t?.on}<i class="st on" title="终端已连接（切到别的机器也不断开）" aria-label="已连接"></i>{:else if t?.busy}<i class="st wait" title="正在连接…" aria-label="正在连接"></i>{:else if t?.err}<i class="st err" title="连接断开或失败：{t.err}" aria-label="连接失败"></i>{/if}
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
  .main b { display: flex; align-items: center; gap: 6px; min-width: 0; font-size: 12.5px; font-weight: 600; }
  .main .nm { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st { flex: none; width: 7px; height: 7px; margin-right: 2px; border-radius: 50%; }
  .st.on { background: var(--ok, #2f9e5b); }
  .st.wait { background: var(--faint); animation: pulse 1.1s infinite; }
  .st.err { background: var(--err, #d14343); }
  .main small { font-size: 11px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .run { flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: pulse 1.1s infinite; }
  .convs { padding: 1px 0 6px 12px; animation: rr-in .18s cubic-bezier(.2, .7, .2, 1) backwards; }
  @keyframes pulse { 50% { opacity: .35; } }
  @keyframes rr-in { from { opacity: 0; transform: translateY(-3px); } }
  @media (prefers-reduced-motion: reduce) { .convs { animation: none; } }
</style>
