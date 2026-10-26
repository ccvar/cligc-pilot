<script lang="ts">
  import { escClose } from '$lib/escClose';
  import { dialogGeometry } from '$lib/dialogGeometry';
  import { library, confirmUseTmpl } from '$lib/state/library.svelte.js';
  import { DIALOG_GEOMETRY } from '$lib/state/env';
  import { app } from '$lib/state/app.svelte.js';
</script>

<!-- 用模板建站 -->
{#if library.useTmplOpen}
  <div class="mask" role="presentation" onclick={() => !library.useTmplBusy && (library.useTmplOpen = false)}></div>
  <div class="modal" use:dialogGeometry={DIALOG_GEOMETRY.templateUse} use:escClose={() => !library.useTmplBusy && (library.useTmplOpen = false)} role="dialog" aria-modal="true" aria-label="用模板建站">
    <header class="sheet-head"><div><b>用模板建站</b><small class="dim" style="margin-left:10px">{library.useTmplName}</small></div><button class="x" aria-label="关闭" data-tip="关闭" onclick={() => (library.useTmplOpen = false)} disabled={library.useTmplBusy}>×</button></header>
    <div class="sheet-body">
      {#if !app.isCfConn}<p class="hint warn-text">请先在左下角切到一个 Cloudflare 连接，再用模板建站。</p>{/if}
      <p class="hint">给新项目起个名，模板文件会拷进去，然后在对话里描述你的定制。</p>
      <input class="tin" bind:value={library.useTmplProject} placeholder="新项目名，如 my-landing" spellcheck="false" onkeydown={(e) => e.key === 'Enter' && confirmUseTmpl()} />
      {#if library.useTmplErr}<div class="err-note">{library.useTmplErr}</div>{/if}
      <div class="row-end">
        <button class="btn ghost" onclick={() => (library.useTmplOpen = false)} disabled={library.useTmplBusy}>取消</button>
        <button class="btn primary" onclick={confirmUseTmpl} disabled={library.useTmplBusy || !library.useTmplProject.trim() || !app.isCfConn}>{library.useTmplBusy ? '创建中…' : '创建项目'}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  .tin { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1.5px solid var(--border2); border-radius: 8px; padding: 5px 10px; }
  /* 单行输入（text/password/number/datetime-local）：显式统一高 --ctl-h，内容垂直居中；
     去掉竖向 padding 防 macOS 原生步进器/日期控件把高度顶开。textarea 不定高，不吃这条。 */
  input.tin { height: var(--ctl-h); box-sizing: border-box; padding-top: 0; padding-bottom: 0; line-height: normal; }
  .tin:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }
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
  .hint { color: var(--dim); font-size: 12px; margin: 2px 0; line-height: 1.6; }
  .warn-text { color: var(--warn); }
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
