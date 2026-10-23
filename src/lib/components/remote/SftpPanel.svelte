<script lang="ts">
  import SftpTransfers from '$lib/SftpTransfers.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import { workbench, missingSshCredential } from '$lib/state/workbench.svelte.js';
  import { sftp, onSftpDragOver, onSftpDragLeave, onSftpDrop, sftpUp, commitPath, sftpGo, startPathEdit, sftpRefresh, openMkdir, isSftpDeleting, sftpUpload, sftpDownloadSelected, cancelSftpTransfer, clearSftpTransfers, revealSftpTransfer, retryOrRepairSftp, setSort, openFctx, isSftpDeleteTarget, onSftpDragStart, sftpOpenRow, toggleDir, focusFileRename, saveFileRename, openRename, fmtSize, fmtStamp, startColDrag, onColKey } from '$lib/state/sftp.svelte.js';
  import { isMac } from '$lib/state/env';
</script>

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
    <button class="fbtn" aria-label="刷新" data-tip="刷新" onclick={sftpRefresh} disabled={sftp.sftpBusy}><RefreshIcon spinning={sftp.sftpBusy} /></button>
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

<!-- 列宽手柄：坐在定宽列的左边缘上（就是列头之间那根分隔线）。做成 button 是为了键盘也够得着
     （←/→ 微调），顺带免掉给 div 挂事件的一堆 a11y 警告。 -->
{#snippet colGrip(k: 'perm' | 'size' | 'date')}
  <button class="fh-grip" aria-label="调整列宽（左右方向键微调）" data-tip="拖动调整列宽"
    onmousedown={(e) => startColDrag(e, k)}
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => onColKey(e, k)}></button>
{/snippet}

<style>
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
  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  .tin { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1.5px solid var(--border2); border-radius: 8px; padding: 5px 10px; }
  /* 单行输入（text/password/number/datetime-local）：显式统一高 --ctl-h，内容垂直居中；
     去掉竖向 padding 防 macOS 原生步进器/日期控件把高度顶开。textarea 不定高，不吃这条。 */
  input.tin { height: var(--ctl-h); box-sizing: border-box; padding-top: 0; padding-bottom: 0; line-height: normal; }
  .tin:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }
  .err-note { color: var(--err); background: var(--err-soft); border: 1px solid var(--err-border); border-radius: 8px; padding: 8px 10px; font-size: 13px; }
  @keyframes rspin { to { transform: rotate(360deg); } }
  .tin { width: 100%; }
  /* 通用文字链按钮（任务卡「查看对话/运行记录」、托管卡操作等）——此前从未定义,一直是系统默认灰凸样式 */
  .link { border: none; background: none; padding: 0; font: inherit; font-size: 12.5px; color: var(--accent); cursor: pointer; white-space: nowrap; }
  .link:hover { text-decoration: underline; }
</style>
