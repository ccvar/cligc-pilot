<script lang="ts">
  import { sftp, sftpOpenRow, sftpDownload, sftpUpload, openMkdir, copySftpEntry, duplicateSftpEntry, pasteSftpEntry, copyPath, openRename, sftpDelete, openNewFile, sftpRefresh } from '$lib/state/sftp.svelte.js';
</script>

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

<style>
  /* 远程文件右键菜单：复用全局 .ctx-menu/.ctx-item 的样式，只补两处差异 */
  .fctx { min-width: 172px; }
  .fctx .ctx-item { justify-content: flex-start; }
  .ctx-item.danger { color: var(--err); }
  .ctx-item.danger:hover:not(:disabled) { background: var(--err-soft); }
  /* 自定义右键菜单（替换 WKWebView 默认英文菜单） */
  .ctx-menu { position: fixed; z-index: 120; min-width: 148px; max-width: calc(100vw - 16px); max-height: calc(100vh - 16px); overflow-x: hidden; overflow-y: auto; background: #fff; border: 1px solid var(--border); border-radius: 11px; box-shadow: 0 12px 32px rgba(30,25,15,.16); padding: 5px; animation: pop .1s ease-out; }
  .ctx-item { width: 100%; display: flex; align-items: center; justify-content: space-between; gap: 18px; background: none; border: none; border-radius: 7px; padding: 6px 10px; font: inherit; font-size: 13px; color: var(--text); cursor: pointer; text-align: left; }
  .ctx-item:hover:not(:disabled) { background: #f1efe9; }
  .ctx-item:disabled { color: var(--faint); cursor: default; }
  .ctx-div { height: 1px; background: var(--border); margin: 4px 6px; }
  @keyframes pop { from { opacity: 0; transform: translateY(4px); } }
</style>
