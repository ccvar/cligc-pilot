<script lang="ts">
  import { escClose } from '$lib/escClose';
  import { dialogGeometry } from '$lib/dialogGeometry';
  import { connections, saveConnRemark } from '$lib/state/connections.svelte.js';
  import { DIALOG_GEOMETRY } from '$lib/state/env';
</script>

{#if connections.connRemarkEdit}
  <div class="mask" role="presentation" onclick={() => !connections.connRemarkEdit?.busy && (connections.connRemarkEdit = null)}></div>
  <div class="modal conn-remark-modal" use:dialogGeometry={DIALOG_GEOMETRY.connectionRemark} use:escClose={() => !connections.connRemarkEdit?.busy && (connections.connRemarkEdit = null)} role="dialog" aria-modal="true" aria-label="设置连接备注">
    <header class="sheet-head"><div><b>设置备注</b><small class="dim">{connections.connRemarkEdit.name}</small></div><button class="x" aria-label="关闭" data-tip="关闭" onclick={() => (connections.connRemarkEdit = null)} disabled={connections.connRemarkEdit.busy}>×</button></header>
    <div class="sheet-body">
      <p class="hint">备注会替代列表里的脱敏 Key；鼠标悬停仍可查看原来的脱敏 Key。留空保存即可清除。</p>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="tin" bind:value={connections.connRemarkEdit.value} maxlength="60" placeholder="例如 cms.ccvar.com" spellcheck="false" autocapitalize="off" autocorrect="off" autofocus disabled={connections.connRemarkEdit.busy} onkeydown={(e) => e.key === 'Enter' && void saveConnRemark()} />
      <div class="err-note slot" class:on={!!connections.connRemarkEdit.error} role="alert">{connections.connRemarkEdit.error}</div>
      <div class="row-end">
        <button class="btn ghost" onclick={() => (connections.connRemarkEdit = null)} disabled={connections.connRemarkEdit.busy}>取消</button>
        <button class="btn primary" onclick={() => void saveConnRemark()} disabled={connections.connRemarkEdit.busy || connections.connRemarkEdit.value.trim() === connections.connRemarkEdit.original}>{connections.connRemarkEdit.busy ? '保存中…' : '保存'}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  .tin { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1px solid var(--border2); border-radius: 8px; padding: 5.5px 10px; }
  /* 单行输入（text/password/number/datetime-local）：显式统一高 --ctl-h，内容垂直居中；
     去掉竖向 padding 防 macOS 原生步进器/日期控件把高度顶开。textarea 不定高，不吃这条。 */
  input.tin { height: var(--ctl-h); box-sizing: border-box; padding-top: 0; padding-bottom: 0; line-height: normal; }
  .tin:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }
  .err-note { color: var(--err); background: var(--err-soft); border: 1px solid var(--err-border); border-radius: 8px; padding: 8px 10px; font-size: 13px; }
  /* 报错位始终占一行：出现时只是淡入，弹窗不长高、不重新居中 */
  .err-note.slot:not(.on) { visibility: hidden; }
  .err-note.slot:not(.on)::before { content: '\200b'; }
  .err-note.slot.on { animation: noteIn .18s ease-out; }
  @keyframes noteIn { from { opacity: 0; } }
  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1px solid var(--border2); border-radius: 8px; padding: 6.5px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
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
  .conn-remark-modal { width: min(370px, 92vw); }
  .conn-remark-modal .sheet-head > div { min-width: 0; display: flex; align-items: baseline; gap: 8px; }
  .conn-remark-modal .sheet-head b { flex: none; }
  .conn-remark-modal .sheet-head small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint { color: var(--dim); font-size: 12px; margin: 2px 0; line-height: 1.6; }
  .dim { color: var(--dim); }
  .tin { width: 100%; }
  .row-end { display: flex; justify-content: flex-end; align-items: center; gap: 7px; margin-top: 2px; }
  /* 弹窗底部操作区统一使用紧凑规格。正文末尾的 row-end 也铺到弹窗边缘，
     看起来和独立 footer 是同一套组件，同时不影响表单内部的小型操作行。 */
  .modal > .sheet-body > .row-end:last-child {
    box-sizing: border-box;
    min-height: 40px;
    flex: none;
    margin: 5px -18px -16px;
    padding: 5px 14px 7px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  .modal > .sheet-body > .row-end:last-child .btn {
    box-sizing: border-box;
    min-height: 28px;
    padding: 4px 10px;
    border-radius: 7px;
    font-size: 11.5px;
    line-height: 1.15;
  }
</style>
