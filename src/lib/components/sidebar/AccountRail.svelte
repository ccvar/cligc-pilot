<script lang="ts">
  import type { Snippet } from 'svelte';
  import CfMark from '$lib/components/icons/CfMark.svelte';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { connections, selectConn, openConnCtx, connectionDisplayName, accountConns, type AccountKind } from '$lib/state/connections.svelte.js';
  import { openCfConnect } from '$lib/state/cloudflare.svelte.js';
  import { gh, openGhConnect } from '$lib/state/github.svelte.js';
  import { sidebar } from '$lib/state/sidebar.svelte.js';

  // Cloudflare / GitHub 模式的左栏：新对话下面列出这一类的全部账号（和远程连接一样），当前账号展开它的对话。
  let { kind, current }: { kind: AccountKind; current: Snippet } = $props();
  const list = $derived(accountConns(kind));
  const busy = $derived(new Set(app.convos.filter((c) => c.status === 'running').map((c) => c.conn_id)));
  // 第二行：备注 > 账号标识；当前账号再带上项目 / 仓库数
  function sub(c: (typeof list)[number], on: boolean): string {
    // GitHub 的标题已经是用户名，这里不再重复
    const id = c.remark?.trim() || c.key_prefix;
    if (!on) return id;
    if (kind === 'cloudflare' && app.cfProjects.length) return `${id} · ${app.cfProjects.length} 个项目${sidebar.cfSiteCount ? ` · ${sidebar.cfSiteCount} 个站点` : ''}`;
    if (kind === 'github' && gh.repos.length) return `${id} · ${gh.repos.length} 个仓库`;
    return id;
  }
</script>

<div class="cr">
  {#each list as c (c.id)}
    {@const on = c.id === app.activeConnId}
    <div class="acct" class:on={on && (app.view === 'launcher' || app.view === 'thread')} class:menu={connections.connCtx?.conn.id === c.id}
      role="button" tabindex="0" title={c.key_prefix ? `脱敏 Token：${c.key_prefix}` : undefined}
      onclick={() => selectConn(c.id)} onkeydown={(e) => e.key === 'Enter' && selectConn(c.id)} oncontextmenu={(e) => openConnCtx(e, c)}>
      <span class="ic">{#if kind === 'github'}<GhMark size={14} />{:else}<CfMark size={14} />{/if}</span>
      <span class="main"><b>{kind === 'github' && c.login ? c.login : connectionDisplayName(c)}</b><small>{sub(c, on)}</small></span>
      {#if busy.has(c.id)}<span class="run" title="有对话正在进行"></span>{/if}
    </div>
    {#if on && sidebar.grouped.length}<div class="convs">{@render current()}</div>{/if}
  {/each}
  <button class="add" onclick={kind === 'github' ? openGhConnect : openCfConnect}>
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M6 2v8M2 6h8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>
    {kind === 'github' ? '连接 GitHub 账号' : '连接 Cloudflare 账号'}
  </button>
</div>

<style>
  .cr { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 0 8px; }
  .acct { margin: 0 8px; min-height: 40px; display: flex; align-items: center; gap: 9px; padding: 4px 8px 4px 10px; border-radius: 9px; cursor: pointer; min-width: 0; transition: background-color .12s; }
  .acct:hover, .acct.menu { background: #f5f4f1; }
  .acct.on { background: #f0efeb; }
  .ic { display: inline-flex; flex: none; color: var(--text); }
  .main { flex: 1; min-width: 0; display: flex; flex-direction: column; line-height: 1.3; }
  .main b { font-size: 12.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .main small { font-size: 11px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .run { flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: pulse 1.1s infinite; }
  .convs { padding: 1px 0 6px 8px; animation: cr-in .18s cubic-bezier(.2, .7, .2, 1) backwards; }
  .add { margin: 6px 8px 0; width: calc(100% - 16px); display: flex; align-items: center; gap: 8px; height: 30px; padding: 0 10px; border: none; background: none; border-radius: 9px; font: inherit; font-size: 12px; color: var(--faint); cursor: pointer; transition: background-color .12s, color .12s; }
  .add:hover { background: #f5f4f1; color: var(--text); }
  @keyframes pulse { 50% { opacity: .35; } }
  @keyframes cr-in { from { opacity: 0; transform: translateY(-3px); } }
  @media (prefers-reduced-motion: reduce) { .convs { animation: none; } }
</style>
