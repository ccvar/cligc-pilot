<script lang="ts">
  import { connections, openConnWindow, selectConn, openConnRemark, removeConn } from '$lib/state/connections.svelte.js';
  import { openRemote } from '$lib/state/workbench.svelte.js';
  import { openSshEdit } from '$lib/state/ssh.svelte.js';
  import { openCfUpdate } from '$lib/state/cloudflare.svelte.js';
  import { openGhUpdate } from '$lib/state/github.svelte.js';
  import { openDataUpdate, isDataConn } from '$lib/state/datasrc.svelte.js';
</script>

{#if connections.connCtx}
  <!-- 连接切换器的右键菜单。菜单项按连接类型给：能做的才列，做不了的不摆在那儿灰着。 -->
  {@const c = connections.connCtx.conn}
  <div bind:this={connections.connCtxMenuEl} class="ctx-menu fctx conn-ctx-menu" class:ready={connections.connCtx.ready} style="left:{connections.connCtx.x}px; top:{connections.connCtx.y}px" role="menu" tabindex="-1">
    {#if !isDataConn(c)}<button class="ctx-item" role="menuitem" onclick={() => openConnWindow(c.id)}>新窗口打开</button>{/if}
    {#if c.kind === 'workspace' || c.kind === 'sites' || c.kind === 'remotes'}
      <!-- 合并的入口（本地对话 / 站点 / 远程连接）：只有「新窗口打开」，没有可编辑、可删除的东西 -->
    {:else if c.kind === 'ssh'}
      <button class="ctx-item" role="menuitem" onclick={() => { connections.switcherOpen = false; selectConn(c.id); openRemote(); }}>远程终端</button>
      <div class="ctx-div"></div>
      <button class="ctx-item" role="menuitem" onclick={() => { connections.switcherOpen = false; openSshEdit(c); }}>编辑连接…</button>
    {:else if c.kind === 'cloudflare'}
      <div class="ctx-div"></div>
      <button class="ctx-item" role="menuitem" onclick={() => openConnRemark(c)}>{c.remark?.trim() ? '修改备注…' : '设置备注…'}</button>
      <button class="ctx-item" role="menuitem" onclick={() => openCfUpdate(c)}>更新 Token…</button>
    {:else if c.kind === 'github'}
      <div class="ctx-div"></div>
      <button class="ctx-item" role="menuitem" onclick={() => openConnRemark(c)}>{c.remark?.trim() ? '修改备注…' : '设置备注…'}</button>
      <button class="ctx-item" role="menuitem" onclick={() => openGhUpdate(c)}>更新 Token…</button>
    {:else if isDataConn(c)}
      <button class="ctx-item" role="menuitem" onclick={() => openConnRemark(c)}>{c.remark?.trim() ? '修改备注…' : '设置备注…'}</button>
      {#if c.kind === 'google' && c.key_kind === 'oauth'}
        <button class="ctx-item" role="menuitem" onclick={() => openDataUpdate(c, { manage: true })}>重新授权…</button>
      {:else}
        <button class="ctx-item" role="menuitem" onclick={() => openDataUpdate(c)}>更新凭据…</button>
      {/if}
    {/if}
    {#if !['workspace', 'sites', 'remotes'].includes(c.kind)}
    <div class="ctx-div"></div>
    <button
      class="ctx-item danger"
      role="menuitem"
      data-keep-connection-menu
      onclick={(e) => { e.stopPropagation(); void removeConn(c.id, true); }}
    >删除连接…</button>
    {/if}
  </div>
{/if}

<style>
  /* 远程文件右键菜单：复用全局 .ctx-menu/.ctx-item 的样式，只补两处差异 */
  .fctx { min-width: 172px; }
  .fctx .ctx-item { justify-content: flex-start; }
  .ctx-item.danger { color: var(--err); }
  .ctx-item.danger:hover:not(:disabled) { background: var(--err-soft); }
  /* 自定义右键菜单（替换 WKWebView 默认英文菜单） */
  .ctx-menu { position: fixed; z-index: 120; min-width: 148px; max-width: calc(100vw - 16px); max-height: calc(100vh - 16px); overflow-x: hidden; overflow-y: auto; background: #fff; border: 1px solid var(--border); border-radius: 11px; box-shadow: 0 12px 32px rgba(30,25,15,.16); padding: 5px; animation: pop .1s ease-out; }
  .conn-ctx-menu { visibility: hidden; }
  .conn-ctx-menu.ready { visibility: visible; }
  .ctx-item { width: 100%; display: flex; align-items: center; justify-content: space-between; gap: 18px; background: none; border: none; border-radius: 7px; padding: 6px 10px; font: inherit; font-size: 13px; color: var(--text); cursor: pointer; text-align: left; }
  .ctx-item:hover:not(:disabled) { background: #f1efe9; }
  .ctx-item:disabled { color: var(--faint); cursor: default; }
  .ctx-div { height: 1px; background: var(--border); margin: 4px 6px; }
  @keyframes pop { from { opacity: 0; transform: translateY(4px); } }
</style>
