<script lang="ts">
  import { sheetWidth } from '$lib/sheetWidth';
  import AppIcon from '$lib/AppIcon.svelte';
  import UpdateIcon from '$lib/components/icons/UpdateIcon.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import { updater, closeUpdateCenter, checkPilotUpdate, pilotUpdateMessage, pilotUpdateLabel, runUpdate } from '$lib/state/updater.svelte.js';
  import { SHEET_WIDTH } from '$lib/state/env';
</script>

{#if updater.updateCenterOpen}
  <div class="mask update-center-mask" role="presentation" onclick={closeUpdateCenter}></div>
  <div class="sheet update-center-sheet" use:sheetWidth={SHEET_WIDTH.updateCenter} role="dialog" aria-modal="true" aria-label="更新中心">
    <header class="sheet-head update-center-head">
      <div class="update-center-title">
        <span class="update-center-title-icon" aria-hidden="true"><UpdateIcon size={16} /></span>
        <span class="update-center-title-copy">
          <b>更新中心</b>
          <small>{updater.updateCheckedAt ? `上次检查 ${new Date(updater.updateCheckedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}` : '尚未检查'}</small>
        </span>
      </div>
      <div class="update-center-head-actions">
        <button
          type="button"
          class="icon-btn"
          aria-label={updater.pilotUpdateStatus === 'checking' ? '正在重新检查更新' : '重新检查更新'}
          data-tip={updater.pilotUpdateStatus === 'checking' ? '正在重新检查更新…' : '重新检查更新'}
          onclick={() => void checkPilotUpdate(false)}
          disabled={updater.pilotUpdateStatus === 'checking' || updater.updBusy}
        ><RefreshIcon spinning={updater.pilotUpdateStatus === 'checking'} /></button>
        <button class="x" onclick={closeUpdateCenter} disabled={updater.updBusy}>×</button>
      </div>
    </header>
    <div class="sheet-body update-center-body">
      {#if updater.pilotUpdateReceipt}<div class="update-center-notice success">{updater.pilotUpdateReceipt}</div>{/if}
      <section class="update-section">
        <div class="update-item pilot">
          <span class="update-item-icon"><AppIcon size={20} /></span>
          <span class="update-item-copy">
            <b>Cligc Pilot</b>
            <small>{pilotUpdateMessage()}</small>
            {#if updater.pilotReleaseNotes && updater.pilotUpdateStatus === 'available'}<i>{updater.pilotReleaseNotes}</i>{/if}
          </span>
          <span class="update-state {updater.pilotUpdateStatus}" title={pilotUpdateMessage()}>{pilotUpdateLabel(updater.pilotUpdateStatus)}</span>
          {#if updater.pilotUpdateStatus === 'available'}
            <button class="btn small ghost" onclick={() => void runUpdate()} disabled={updater.updBusy}>更新</button>
          {/if}
        </div>
      </section>
      <p class="update-center-footnote">更新包经 ed25519 签名校验后安装，安装完成会自动重启客户端；正在进行的对话会在重启前保存。</p>
    </div>
    <footer class="update-center-footer">
      <button class="btn ghost" onclick={closeUpdateCenter} disabled={updater.updBusy}>关闭</button>
      <button class="btn primary" onclick={() => void runUpdate()} disabled={updater.updBusy || updater.pilotUpdateStatus !== 'available'}>
        {updater.updBusy ? '正在更新…' : updater.updAvail ? `更新到 ${updater.updAvail}` : '当前无需更新'}
      </button>
    </footer>
  </div>
{/if}

<style>
  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1.5px solid var(--border2); border-radius: 8px; padding: 6px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
  .btn:hover { background: #f6f5f1; }
  .btn:disabled { opacity: .5; cursor: default; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .btn.primary:hover { background: var(--accent-h); }
  .btn.ghost { border-color: var(--border); }
  .btn.small { padding: 4px 10px; font-size: 12px; }
  .icon-btn { background: none; border: none; cursor: pointer; padding: 6px; border-radius: 8px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .icon-btn:hover { background: #f1efe9; color: var(--text); }
  .icon-btn:disabled { opacity: .55; cursor: default; }
  .x { background: none; border: none; color: var(--faint); font-size: 20px; cursor: pointer; line-height: 1; }
  .x:hover { color: var(--err); }
  .mask { position: fixed; inset: 0; background: rgba(25,20,10,.28); z-index: 50; }
  .sheet { position: fixed; z-index: 60; background: var(--bg); border: 1px solid var(--border); box-shadow: var(--shadow-lg); display: flex; flex-direction: column; }
  .sheet { top: 0; right: 0; bottom: 0; width: min(400px, 92vw); border-radius: 0; }
  .sheet-head { box-sizing: border-box; min-height: 48px; display: flex; justify-content: space-between; align-items: center; padding: 7px 14px; border-bottom: 1px solid var(--border); }
  /* 弹窗正文统一只允许纵向滚动。只声明 overflow-y:auto 会按 CSS 规则把横向轴
     隐式计算成 auto；贴边 footer 的负边距和 WebView 的浮点取整都会因此冒出横向条。 */
  .sheet-body { min-width: 0; padding: 16px 18px; overflow-x: hidden; overflow-y: auto; display: flex; flex-direction: column; gap: 7px; }
  .update-center-mask { z-index: 65; }
  .update-center-sheet { z-index: 70; width: min(540px, 96vw); background: #fbfaf7; }
  .update-center-title { min-width: 0; display: flex; align-items: center; gap: 7px; }
  .update-center-title-icon { flex: none; width: 26px; height: 26px; display: grid; place-items: center; border-radius: 7px; background: var(--accent-soft); color: var(--accent); }
  .update-center-title-copy { min-width: 0; display: grid; gap: 1px; }
  .update-center-title-copy b { font-size: 13.5px; line-height: 1.2; }
  .update-center-title-copy small { color: var(--faint); font-size: 9.5px; line-height: 1.2; font-weight: 400; }
  .update-center-head-actions { display: flex; align-items: center; gap: 5px; }
  .update-center-body { flex: 1 1 auto; min-height: 0; gap: 13px; padding: 15px 17px 18px; }
  .update-center-notice { padding: 8px 10px; border: 1px solid #e6dfd3; border-radius: 9px; background: #f7f4ee; color: var(--dim); font-size: 11px; line-height: 1.5; }
  .update-center-notice.success { border-color: #cfe5d8; background: #eef8f2; color: var(--ok); }
  .update-section { display: grid; gap: 7px; }
  .update-item { min-width: 0; display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 9px; padding: 9px 10px; }
  .update-item.pilot { border: 1px solid var(--border); border-radius: 11px; background: #fff; }
  .update-item-icon { flex: none; width: 28px; height: 28px; display: grid; place-items: center; border-radius: 8px; background: var(--soft); color: var(--dim); }
  .update-item-copy { min-width: 0; display: grid; gap: 2px; }
  .update-item-copy b { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11.5px; }
  .update-item-copy small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--dim); font-size: 10.5px; }
  .update-item-copy i { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--faint); font-size: 9.5px; font-style: normal; }
  .update-state { flex: none; min-width: 48px; padding: 3px 6px; border-radius: 999px; background: var(--soft); color: var(--dim); font-size: 9.5px; font-weight: 650; line-height: 1.25; text-align: center; white-space: nowrap; }
  .update-state.available { background: #fff1dc; color: #a65b00; }
  .update-state.current, .update-state.done { background: #eaf6ee; color: var(--ok); }
  .update-state.checking, .update-state.updating { background: #eceff6; color: #46619b; }
  .update-state.waiting { background: #f3f0e9; color: #826c47; }
  .update-state.unreachable, .update-state.failed { background: var(--err-soft); color: var(--err); }
  .update-item .btn.small { flex: none; min-width: 42px; padding-inline: 7px; }
  .update-center-footnote { margin: 0; padding: 9px 10px; border-radius: 9px; background: #f1efe9; color: var(--dim); font-size: 10px; line-height: 1.55; }
  .update-center-footer { flex: none; display: flex; align-items: center; justify-content: flex-end; gap: 8px; padding: 11px 17px; border-top: 1px solid var(--border); background: #fff; }
  .update-center-footer {
    box-sizing: border-box;
    min-height: 40px;
    gap: 6px;
    padding: 5px 14px 7px;
  }
  .update-center-footer .btn {
    box-sizing: border-box;
    min-height: 28px;
    padding: 4px 10px;
    border-radius: 7px;
    font-size: 11.5px;
    line-height: 1.15;
  }
</style>
