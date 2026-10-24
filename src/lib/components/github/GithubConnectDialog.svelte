<script lang="ts">
  import CopyLinkButton from '$lib/components/common/CopyLinkButton.svelte';
  import { dialogGeometry } from '$lib/dialogGeometry';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import { gh, ghUpdateConnection, closeGhConnect, ghTokenTemplateUrl, resetGhVerification, verifyGh, confirmGh } from '$lib/state/github.svelte.js';
  import { DIALOG_GEOMETRY } from '$lib/state/env';
</script>

<!-- 连接 GitHub（fine-grained Token） -->
{#if gh.ghOpen}
  {@const updating = ghUpdateConnection()}
  {@const v = gh.ghVerified}
  {@const sameAccount = !updating || (!!v && String(v.user_id) === updating.account_id)}
  <div class="mask gh-mask" role="presentation" onclick={closeGhConnect}></div>
  <div class="modal gh-modal" use:dialogGeometry={DIALOG_GEOMETRY.github} role="dialog" aria-modal="true" aria-label={updating ? '更新 GitHub 连接' : '连接 GitHub'}>
    <header class="sheet-head"><div><b>{updating ? '更新' : '连接'} <GhMark size={15} /> GitHub</b><small class="dim" style="margin-left:10px">token 只进钥匙串，绝不落盘</small></div><button class="x" onclick={closeGhConnect} disabled={gh.ghConnecting || gh.ghVerifying}>×</button></header>
    <div class="sheet-body">
      {#if updating}
        <div class="gh-update-target">
          <span><GhMark size={17} /></span>
          <div><b>{updating.remark?.trim() || updating.name}</b><small>{updating.key_prefix} · 账号 {updating.login}</small></div>
        </div>
        <p class="hint">新 Token 必须属于同一个 GitHub 账号；验证通过后只替换这一条连接，已克隆的仓库和对话都保留。</p>
      {/if}
      <div class="gh-step">
        <p class="gh-step-t">① 在 GitHub 新建一枚 <b>fine-grained</b> Token：仓库范围选 <b>All repositories</b>，并给 <b>Administration</b> 写权限（Pilot 要能为新站点建仓库）</p>
        <button class="gh-perms-toggle" type="button" onclick={() => (gh.ghPermsOpen = !gh.ghPermsOpen)}>
          <svg class="gh-chev" class:open={gh.ghPermsOpen} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5 6 7.5 9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
          所需设置
        </button>
        {#if gh.ghPermsOpen}
          <ul class="gh-perms">
            <li>Repository access · <b>All repositories</b>（Pilot 会为新做的站点建仓库；选 Only select repositories 也能用，但不能自动建仓库，只能用你事先建好、并加进 Token 的空仓库）</li>
            <li>Administration · <b>Read and write</b>（新建仓库：站点制作、上线向导「代码放到 GitHub」）</li>
            <li>Contents · <b>Read and write</b>（读代码、推分支）</li>
            <li>Pull requests · <b>Read and write</b>（提 PR）</li>
            <li>Issues · <b>Read and write</b>（可选：读写 Issue）</li>
            <li>要改 <code>.github/workflows</code> 时，另需 Workflows · <b>Read and write</b></li>
          </ul>
        {/if}
        <div class="tok-open"><button class="btn soft" onclick={() => openUrl(ghTokenTemplateUrl())}>{updating ? '为此连接新建' : '打开'} <GhMark size={14} /> GitHub Token 页 ↗</button><CopyLinkButton url={() => ghTokenTemplateUrl()} tip="复制链接（在别的浏览器或账号里打开）" /></div>
      </div>
      <div class="gh-step">
        <p class="gh-step-t">② 把{updating ? '新的' : '生成的'} Token 粘到这里（以 <code>github_pat_</code> 开头）：</p>
        <div class="gh-row">
          <input class="tin" type="password" bind:value={gh.ghToken} placeholder="粘贴 fine-grained Token" autocomplete="off" spellcheck="false" oninput={resetGhVerification} onkeydown={(e) => e.key === 'Enter' && verifyGh()} />
          <button class="btn small primary" onclick={verifyGh} disabled={gh.ghVerifying || !gh.ghToken.trim()}>{gh.ghVerifying ? '验证中…' : '验证'}</button>
        </div>
      </div>
      {#if v}
        {#if updating}
          <div class="gh-update-verified" class:ok={sameAccount}>
            <span>{sameAccount ? '✓' : '!'}</span>
            <div><b>{sameAccount ? `已确认属于 ${v.login}` : `属于 ${v.login}，不是原账号`}</b><small>可访问 {v.repos.length} 个仓库；保存后连接名称和已有设置保持不变。</small></div>
          </div>
        {:else}
          <div class="gh-step">
            <p class="gh-step-t">③ 账号 <b>{v.login}</b>{v.name ? `（${v.name}）` : ''}，可访问 {v.repos.length} 个仓库：</p>
            {#if v.repos.length}
              <ul class="gh-repos">
                {#each v.repos as r (r.full_name)}
                  <li><span>{r.full_name}</span><em>{r.private ? '私有' : '公开'}</em>{#if !r.can_push}<em class="ro">只读</em>{/if}</li>
                {/each}
              </ul>
            {/if}
            <input class="tin" style="margin-top:7px" bind:value={gh.ghName} placeholder="连接名称（可留默认）" />
          </div>
        {/if}
      {/if}
      {#if gh.ghErr}<div class="err-note">{gh.ghErr}</div>{/if}
      <div class="row-end">
        <button class="btn small ghost" onclick={closeGhConnect} disabled={gh.ghConnecting || gh.ghVerifying}>取消</button>
        <button class="btn small primary" onclick={confirmGh} disabled={gh.ghConnecting || !v || !sameAccount || !v.repos.length}>{gh.ghConnecting ? (updating ? '更新中…' : '连接中…') : updating ? '更新此连接' : '连接'}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .tok-open { display: flex; align-items: center; gap: 4px; }
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
  .mask.gh-mask { z-index: 70; }
  .modal.gh-modal { z-index: 80; width: min(440px, 94vw); max-height: 88vh; }
  .gh-step { border-top: 0.5px solid var(--border); padding-top: 12px; margin-top: 6px; }
  .gh-step:first-child { border-top: none; padding-top: 0; margin-top: 0; }
  .gh-step-t { font-size: 13px; color: var(--text); margin: 0 0 7px; line-height: 1.5; }
  .gh-update-target { display: flex; align-items: center; gap: 9px; margin-bottom: 9px; padding: 9px 10px; border: 1px solid #dfe7e2; border-radius: 9px; background: #f4f8f5; }
  .gh-update-target > span { width: 28px; height: 28px; border-radius: 8px; background: #fff; display: inline-flex; align-items: center; justify-content: center; flex: none; }
  .gh-update-target > div { min-width: 0; }
  .gh-update-target b, .gh-update-target small { display: block; }
  .gh-update-target b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .gh-update-target small { margin-top: 2px; color: var(--dim); font-size: 10.5px; overflow-wrap: anywhere; }
  .gh-update-verified { display: flex; align-items: center; gap: 8px; margin-top: 8px; padding: 8px 9px; border-radius: 9px; background: #fbf3e7; color: #8a5a08; }
  .gh-update-verified.ok { background: #edf7f0; color: #287248; }
  .gh-update-verified > span { width: 22px; height: 22px; border-radius: 50%; background: rgb(255 255 255 / .75); display: inline-flex; align-items: center; justify-content: center; flex: none; font-weight: 700; }
  .gh-update-verified b, .gh-update-verified small { display: block; }
  .gh-update-verified b { font-size: 11.5px; }
  .gh-update-verified small { margin-top: 1px; color: var(--dim); font-size: 10px; }
  .gh-perms { margin: 0 0 9px; padding-left: 0; list-style: none; display: flex; flex-direction: column; gap: 3px; }
  .gh-perms li { font-size: 12px; color: var(--dim); padding-left: 13px; position: relative; }
  .gh-perms li::before { content: '·'; position: absolute; left: 4px; color: var(--dim); font-weight: 700; }
  .gh-perms li b { color: var(--text); font-weight: 550; }
  .gh-row { display: flex; gap: 8px; }
  .gh-row .tin { flex: 1; }
  .gh-perms-toggle { display: inline-flex; align-items: center; gap: 5px; background: none; border: none; padding: 0 0 8px; font-size: 12px; color: var(--dim); cursor: pointer; }
  .gh-perms-toggle:hover { color: var(--text); }
  .gh-chev { flex: none; transition: transform .12s; }
  .gh-chev.open { transform: rotate(180deg); }
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
  .gh-repos { margin: 4px 0 0; padding: 0; list-style: none; display: flex; flex-direction: column; gap: 2px; max-height: 132px; overflow-y: auto; }
  .gh-repos li { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text); min-width: 0; }
  .gh-repos li span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11.5px; }
  .gh-repos li em { flex: none; font-style: normal; font-size: 10.5px; color: var(--faint); }
  .gh-repos li em.ro { color: #8a5a08; }
</style>
