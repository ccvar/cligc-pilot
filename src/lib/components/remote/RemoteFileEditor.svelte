<script lang="ts">
  import { dialogGeometry } from '$lib/dialogGeometry';
  import FileCodeEditor from '$lib/FileCodeEditor.svelte';
  import { editor, closeEdit, saveEdit } from '$lib/state/editor.svelte.js';
  import { DIALOG_GEOMETRY } from '$lib/state/env';
</script>

{#if editor.edOpen}
  <div class="mask" role="presentation" onclick={() => void closeEdit()}></div>
  <div class="modal ed-modal" class:ed-full={editor.edFull} use:dialogGeometry={{ ...DIALOG_GEOMETRY.fileEditor, enabled: !editor.edFull }} role="dialog" aria-modal="true" aria-label={`编辑文件 ${editor.edPath}`} aria-busy={editor.edSaving}>
    <header class="sheet-head ed-head">
      <div class="ed-heading"><div><b>编辑文件</b><span class="ed-badge" class:dirty={editor.edDirty}>{editor.edDirty ? '未保存' : editor.edSaved ? '已保存' : '远程文件'}</span></div><small class="dim ed-path" title={editor.edPath}>{editor.edPath}</small></div>
      <button type="button" class="ed-expand" title={editor.edFull ? '退出全屏' : '全屏编辑'} aria-label={editor.edFull ? '退出全屏' : '全屏编辑'} aria-pressed={editor.edFull} onclick={() => { editor.edFull = !editor.edFull; }}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          {#if editor.edFull}
            <path d="M3 8h5V3M16 3v5h5M21 16h-5v5M8 21v-5H3" />
          {:else}
            <path d="M8 3H3v5M16 3h5v5M21 16v5h-5M8 21H3v-5" />
          {/if}
        </svg>
      </button>
      <button class="x" aria-label="关闭文件编辑器" onclick={() => void closeEdit()} disabled={editor.edSaving || editor.edFormatting}>×</button>
    </header>
    <div class="sheet-body ed-body">
      {#if editor.edLoading}
        <div class="files-empty">读取中…</div>
      {:else if editor.edErr && editor.edReadOnly}
        <div class="err-note">{editor.edErr}</div>
      {:else}
        <FileCodeEditor bind:value={editor.edText} path={editor.edPath} indent={editor.edFormat.indent} readonly={editor.edSaving || editor.edClosing} bind:busy={editor.edFormatting}
          eol={`UTF-8${editor.edFormat.bom ? ' BOM' : ''} · ${editor.edFormat.mixed ? '混合换行' : editor.edFormat.eol === '\r\n' ? 'CRLF' : editor.edFormat.eol === '\r' ? 'CR' : 'LF'}`} onsave={() => void saveEdit()} />
        {#if editor.edErr}<div class="err-note">{editor.edErr}</div>{/if}
      {/if}
    </div>
    <div class="ed-footer">
      <span class="ed-save-note">{editor.edSaving ? '正在保存到服务器…' : '仅在点击保存时写入远端；不会自动格式化。'}</span>
      <button type="button" class="btn ghost" onclick={() => void closeEdit()} disabled={editor.edSaving || editor.edFormatting}>关闭</button>
      {#if !editor.edReadOnly && !editor.edLoading}
        <button type="button" class="btn primary" onclick={saveEdit} disabled={editor.edSaving || editor.edFormatting || editor.edClosing || !editor.edDirty}>{editor.edSaving ? '保存中…' : '保存'}</button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .files-empty { padding: 28px 0; text-align: center; color: var(--faint); font-size: 13px; }
  /* 在线编辑器 */
  .modal.ed-modal { width: min(1080px, 94vw); height: min(760px, 88vh); }
  .ed-modal.ed-full { inset: 38px 8px 8px; width: auto; height: auto; max-width: none; max-height: none; margin: 0; }
  .ed-head { gap: 10px; }
  .ed-heading { min-width: 0; flex: 1; display: grid; gap: 4px; }
  .ed-heading > div { display: flex; align-items: center; gap: 10px; }
  .ed-badge { color: var(--dim); background: var(--rail); border-radius: 5px; padding: 2px 6px; font-size: 10px; }
  .ed-badge.dirty { color: var(--accent); background: var(--accent-soft); }
  .ed-expand { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 30px; height: 30px; padding: 0; border: 0; border-radius: 5px; background: transparent; box-shadow: none; color: var(--dim); cursor: pointer; }
  .ed-expand:hover { background: var(--rail); color: var(--text); }
  .ed-expand:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .ed-path { display: block; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sheet-body.ed-body { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; gap: 0; overflow: hidden; padding: 0; }
  .ed-body > .err-note { margin: 10px 14px; max-height: 100px; overflow: auto; flex: none; }
  .ed-save-note { margin-right: auto; color: var(--dim); font-size: 11px; }
  .ed-footer { position: relative; z-index: 1; flex: none; display: flex; justify-content: flex-end; align-items: center; gap: 7px; padding: 10px 18px 14px; border-top: 1px solid var(--border); background: var(--bg); }
  .err-note { color: var(--err); background: var(--err-soft); border: 1px solid var(--err-border); border-radius: 8px; padding: 8px 10px; font-size: 13px; }
  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1.5px solid var(--border2); border-radius: 8px; padding: 6px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
  .btn:hover { background: #f6f5f1; }
  .btn:disabled { opacity: .5; cursor: default; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .btn.primary:hover { background: var(--accent-h); }
  .btn.ghost { border-color: var(--border); }
  .x { background: none; border: none; color: var(--faint); font-size: 20px; cursor: pointer; line-height: 1; }
  .x:hover { color: var(--err); }
  .mask { position: fixed; inset: 0; background: rgba(25,20,10,.28); z-index: 50; }
  .modal { position: fixed; z-index: 60; background: var(--bg); border: 1px solid var(--border); box-shadow: var(--shadow-lg); display: flex; flex-direction: column; }
  /* margin:auto 居中而非 transform——transform 会给内部 position:fixed 的下拉菜单
     制造 containing block，菜单被钳进弹窗坐标系再被 overflow:hidden 裁掉（遮挡）。 */
  .modal { inset: 0; margin: auto; height: fit-content; max-height: 88vh; width: min(440px, 92vw); border-radius: 14px; overflow: hidden; }
  .sheet-head { box-sizing: border-box; min-height: 48px; display: flex; justify-content: space-between; align-items: center; padding: 7px 14px; border-bottom: 1px solid var(--border); }
  /* 弹窗正文统一只允许纵向滚动。只声明 overflow-y:auto 会按 CSS 规则把横向轴
     隐式计算成 auto；贴边 footer 的负边距和 WebView 的浮点取整都会因此冒出横向条。 */
  .sheet-body { min-width: 0; padding: 16px 18px; overflow-x: hidden; overflow-y: auto; display: flex; flex-direction: column; gap: 7px; }
  .dim { color: var(--dim); }
  .ed-footer {
    box-sizing: border-box;
    min-height: 40px;
    gap: 6px;
    padding: 5px 14px 7px;
  }
  .ed-footer .btn {
    box-sizing: border-box;
    min-height: 28px;
    padding: 4px 10px;
    border-radius: 7px;
    font-size: 11.5px;
    line-height: 1.15;
  }
</style>
