<script lang="ts">
  import { dialogGeometry } from '$lib/dialogGeometry';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Dropdown from '$lib/Dropdown.svelte';
  import CfMark from '$lib/components/icons/CfMark.svelte';
  import WranglerNote from '$lib/components/launcher/WranglerNote.svelte';
  import { cf, cfUpdateConnection, closeCfConnect, cfTokenTemplateUrl, resetCfVerification, verifyCf, confirmCf } from '$lib/state/cloudflare.svelte.js';
  import { DIALOG_GEOMETRY } from '$lib/state/env';
  import { app } from '$lib/state/app.svelte.js';
</script>

<!-- 连接 Cloudflare -->
{#if cf.cfOpen}
  {@const updatingCf = cfUpdateConnection()}
  <div class="mask cf-mask" role="presentation" onclick={closeCfConnect}></div>
  <div class="modal cf-modal" use:dialogGeometry={DIALOG_GEOMETRY.cloudflare} role="dialog" aria-modal="true" aria-label={updatingCf ? '更新 Cloudflare 连接' : '连接 Cloudflare'}>
    <header class="sheet-head"><div><b>{updatingCf ? '更新' : '连接'} <CfMark size={15} /> Cloudflare</b><small class="dim" style="margin-left:10px">token 只进钥匙串，绝不落盘</small></div><button class="x" onclick={closeCfConnect} disabled={cf.cfConnecting || cf.cfVerifying}>×</button></header>
    <div class="sheet-body">
      {#if updatingCf}
        <div class="cf-update-target">
          <span><CfMark size={17} /></span>
          <div><b>{updatingCf.remark?.trim() || updatingCf.name}</b><small>{updatingCf.remark?.trim() ? `${updatingCf.name} · ` : ''}{updatingCf.key_prefix} · 账号 {updatingCf.account_id}</small></div>
        </div>
        <p class="hint">不用在已有的很多 Token 中猜哪一个需要修改。建议直接为这条 Pilot 连接新建一枚权限完整的 Token；验证成功后只替换这一条连接，其他连接不受影响。</p>
      {/if}
      <div class="cf-step">
        <p class="cf-step-t">① {updatingCf ? '为这条连接新建一枚 API Token' : '在 Cloudflare 建一个 API Token'}（下列权限已预选）</p>
        <button class="cf-perms-toggle" type="button" onclick={() => (cf.cfPermsOpen = !cf.cfPermsOpen)}>
          <svg class="cf-chev" class:open={cf.cfPermsOpen} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5 6 7.5 9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
          所需权限（5 项 + 1 项可选）
        </button>
        {#if cf.cfPermsOpen}
          <ul class="cf-perms">
            <li>Account · Cloudflare Pages · <b>Edit</b></li>
            <li>Account · Workers Scripts · <b>Edit</b></li>
            <li>Account · D1 · <b>Edit</b></li>
            <li>Zone · DNS · <b>Edit</b>（绑定自定义域名用）</li>
            <li>Zone · Zone · <b>Read</b></li>
            <li>Zone · Workers Routes · <b>Edit</b><span class="cf-opt">可选 · 用 Workers 部署、配置了路由的站点需要，没有它代码照样上线，只是更新路由那一步会报错</span></li>
          </ul>
        {/if}
        <button class="btn soft" onclick={() => openUrl(cfTokenTemplateUrl())}>{updatingCf ? '为此连接新建' : '打开'} <CfMark size={14} /> Cloudflare Token ↗</button>
      </div>
      <div class="cf-step">
        <p class="cf-step-t">② 把{updatingCf ? '新的' : '生成的'} Token 粘到这里：</p>
        <div class="cf-row">
          <input class="tin" type="password" bind:value={cf.cfToken} placeholder="粘贴 API Token" autocomplete="off" spellcheck="false" oninput={resetCfVerification} onkeydown={(e) => e.key === 'Enter' && verifyCf()} />
          <button class="btn small primary" onclick={verifyCf} disabled={cf.cfVerifying || !cf.cfToken.trim()}>{cf.cfVerifying ? '验证中…' : '验证'}</button>
        </div>
      </div>
      {#if cf.cfAccounts.length}
        {#if updatingCf}
          <div class="cf-update-verified" class:ok={cf.cfAccounts.some((account) => account.id === updatingCf.account_id)}>
            <span>{cf.cfAccounts.some((account) => account.id === updatingCf.account_id) ? '✓' : '!'}</span>
            <div><b>{cf.cfAccounts.some((account) => account.id === updatingCf.account_id) ? '已确认属于原 Cloudflare 账号' : '不是原 Cloudflare 账号'}</b><small>保存后连接名称和已有设置保持不变。</small></div>
          </div>
        {:else}
          <div class="cf-step">
            <p class="cf-step-t">③ 选择账号 · 给连接起个名：</p>
            <Dropdown bind:value={cf.cfAccountId} options={cf.cfAccounts.map((a) => ({ value: a.id, label: a.name, sub: a.id }))} placeholder="选择账号" />
            <input class="tin" style="margin-top:7px" bind:value={cf.cfName} placeholder="连接名称（可留默认）" />
            {#if cf.cfZones.length}<p class="hint">可管理域名：{cf.cfZones.slice(0, 6).map((z) => z.name).join('、')}{cf.cfZones.length > 6 ? ` 等 ${cf.cfZones.length} 个` : ''}</p>{:else}<p class="hint">没检测到可管理域名——绑定自定义域名需 Zone 权限，可稍后补 token。</p>{/if}
          </div>
        {/if}
      {/if}
      {#if app.brains?.wrangler && !app.brains.wrangler.found}<WranglerNote />{/if}
      {#if cf.cfErr}<div class="err-note">{cf.cfErr}</div>{/if}
      <div class="row-end">
        <button class="btn small ghost" onclick={closeCfConnect} disabled={cf.cfConnecting || cf.cfVerifying}>取消</button>
        <button class="btn small primary" onclick={confirmCf} disabled={cf.cfConnecting || !cf.cfAccountId || !cf.cfAccounts.some((account) => account.id === cf.cfAccountId)}>{cf.cfConnecting ? (updatingCf ? '更新中…' : '连接中…') : updatingCf ? '更新此连接' : '连接'}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .cf-opt { display: block; color: var(--faint); font-size: 11.5px; line-height: 1.5; margin-top: 1px; }
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
  .btn.small { padding: 4px 10px; font-size: 12px; }
  .btn.soft { display: inline-flex; align-items: center; gap: 6px; padding: 3px 12px; background: #fff; color: var(--text); border: 1px solid var(--border2); border-radius: 9px; font-weight: 500; }
  .btn.soft:hover { background: var(--rail); border-color: #cfccc2; }
  .btn.soft :global(.plus-ic) { color: var(--dim); }
  .mask.cf-mask { z-index: 70; }
  .modal.cf-modal { z-index: 80; width: min(440px, 94vw); max-height: 88vh; }
  .cf-step { border-top: 0.5px solid var(--border); padding-top: 12px; margin-top: 6px; }
  .cf-step:first-child { border-top: none; padding-top: 0; margin-top: 0; }
  .cf-step-t { font-size: 13px; color: var(--text); margin: 0 0 7px; line-height: 1.5; }
  .cf-update-target { display: flex; align-items: center; gap: 9px; margin-bottom: 9px; padding: 9px 10px; border: 1px solid #dfe7e2; border-radius: 9px; background: #f4f8f5; }
  .cf-update-target > span { width: 28px; height: 28px; border-radius: 8px; background: #fff; display: inline-flex; align-items: center; justify-content: center; flex: none; }
  .cf-update-target > div { min-width: 0; }
  .cf-update-target b, .cf-update-target small { display: block; }
  .cf-update-target b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .cf-update-target small { margin-top: 2px; color: var(--dim); font-size: 10.5px; overflow-wrap: anywhere; }
  .cf-update-verified { display: flex; align-items: center; gap: 8px; margin-top: 8px; padding: 8px 9px; border-radius: 9px; background: #fbf3e7; color: #8a5a08; }
  .cf-update-verified.ok { background: #edf7f0; color: #287248; }
  .cf-update-verified > span { width: 22px; height: 22px; border-radius: 50%; background: rgb(255 255 255 / .75); display: inline-flex; align-items: center; justify-content: center; flex: none; font-weight: 700; }
  .cf-update-verified b, .cf-update-verified small { display: block; }
  .cf-update-verified b { font-size: 11.5px; }
  .cf-update-verified small { margin-top: 1px; color: var(--dim); font-size: 10px; }
  .cf-perms { margin: 0 0 9px; padding-left: 0; list-style: none; display: flex; flex-direction: column; gap: 3px; }
  .cf-perms li { font-size: 12px; color: var(--dim); padding-left: 13px; position: relative; }
  .cf-perms li::before { content: '·'; position: absolute; left: 4px; color: #f6821f; font-weight: 700; }
  .cf-perms li b { color: var(--text); font-weight: 550; }
  .cf-row { display: flex; gap: 8px; }
  .cf-row .tin { flex: 1; }
  .cf-perms-toggle { display: inline-flex; align-items: center; gap: 5px; background: none; border: none; padding: 0 0 8px; font-size: 12px; color: var(--dim); cursor: pointer; }
  .cf-perms-toggle:hover { color: var(--text); }
  .cf-chev { flex: none; transition: transform .12s; }
  .cf-chev.open { transform: rotate(180deg); }
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
