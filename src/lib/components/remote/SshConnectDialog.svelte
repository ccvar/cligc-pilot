<script lang="ts">
  import { escClose } from '$lib/escClose';
  import { dialogGeometry } from '$lib/dialogGeometry';
  import Dropdown from '$lib/Dropdown.svelte';
  import { sshForm, sshAuthOpts, pickSshKey, saveSshEdit, testSsh, confirmSsh } from '$lib/state/ssh.svelte.js';
  import { DIALOG_GEOMETRY, keystoreName } from '$lib/state/env';
</script>

{#if sshForm.sshOpen}
  <div class="mask" role="presentation" onclick={() => !(sshForm.sshTesting || sshForm.sshConnecting) && (sshForm.sshOpen = false)}></div>
  <div class="modal wide ssh-modal" use:dialogGeometry={DIALOG_GEOMETRY.ssh} use:escClose={() => !(sshForm.sshTesting || sshForm.sshConnecting) && (sshForm.sshOpen = false)} role="dialog" aria-modal="true" aria-label={sshForm.sshCredentialRepair ? '补录远程连接密码' : sshForm.sshEditId ? '编辑远程连接' : '新建远程连接'}>
    <header class="sheet-head"><b>{sshForm.sshCredentialRepair ? '补录远程连接密码' : sshForm.sshEditId ? '编辑远程连接' : '新建远程连接'}</b><button class="x" aria-label="关闭" data-tip="关闭" onclick={() => (sshForm.sshOpen = false)} disabled={sshForm.sshTesting || sshForm.sshConnecting}>×</button></header>
    <div class="sheet-body">
      <p class="hint">{#if sshForm.sshCredentialRepair}这条连接的密码已不在 {keystoreName} 中。输入一次密码并测试连接，保存后会自动恢复终端和文件列表。{:else if sshForm.sshEditId}改完地址 / 用户 / 认证方式后要重新「测试连接」核对指纹再保存；只改名字可以直接保存。密码 / 私钥口令留空＝沿用 {keystoreName} 里已存的那条。{:else}SSH 到一台远程机器：能开终端、管文件、让 AI 帮你干活。密码 / 私钥口令只进 {keystoreName}，绝不落盘。{/if}</p>
      <div class="trow">
        <div class="tfield" style="flex:2"><span>主机</span><input class="tin" bind:value={sshForm.sshF.host} placeholder="例如 1.2.3.4 或 server.example.com" spellcheck="false" autocapitalize="off" autocorrect="off" /></div>
        <div class="tfield" style="flex:1"><span>端口</span><input class="tin" type="number" bind:value={sshForm.sshF.port} placeholder="22" /></div>
      </div>
      <div class="trow">
        <div class="tfield"><span>用户名</span><input class="tin" bind:value={sshForm.sshF.user} placeholder="root" spellcheck="false" autocapitalize="off" autocorrect="off" /></div>
        <div class="tfield"><span>认证方式</span><Dropdown bind:value={sshForm.sshF.auth} options={sshAuthOpts} /></div>
      </div>
      {#if sshForm.sshF.auth === 'password'}
        <div class="tfield"><span>密码{#if sshForm.sshEditId && sshForm.sshF0.auth === 'password' && !sshForm.sshCredentialRepair}（留空＝不改）{/if}</span><input class="tin" type="password" bind:value={sshForm.sshF.password} autocomplete="off" placeholder={sshForm.sshCredentialRepair ? '请输入这条连接的密码' : sshForm.sshEditId && sshForm.sshF0.auth === 'password' ? '不改就别填' : ''} /></div>
      {:else}
        <div class="tfield"><span>私钥文件（只记住路径，不会拷贝你的私钥）</span>
          <div class="ssh-key-row">
            <input class="tin" bind:value={sshForm.sshF.keyPath} placeholder="~/.ssh/id_ed25519" spellcheck="false" autocapitalize="off" autocorrect="off" />
            <button class="btn sm" onclick={pickSshKey}>选择…</button>
          </div>
        </div>
        <div class="tfield"><span>私钥口令（没有就留空{#if sshForm.sshEditId && sshForm.sshF0.auth === 'key'}；不改也留空{/if}）</span><input class="tin" type="password" bind:value={sshForm.sshF.keyPass} autocomplete="off" /></div>
      {/if}
      <div class="tfield"><span>名称（可选）</span><input class="tin" bind:value={sshForm.sshF.name} placeholder={sshForm.sshF.user && sshForm.sshF.host ? `${sshForm.sshF.user}@${sshForm.sshF.host}` : '留空自动用 user@host'} /></div>

      {#if sshForm.sshErr}<div class="err-note">{sshForm.sshErr}</div>{/if}

      {#if sshForm.sshFp}
        <!-- TOFU：指纹必须由人确认过才保存（后端无指纹直接拒）。 -->
        <div class="ssh-fp" class:alarm={sshForm.sshFpChanged}>
          {#if sshForm.sshFpChanged}
            <!-- 这台机器的主机密钥变了。可能是重装/换机，也可能是有人在中间——不能轻轻放过。 -->
            <b class="fp-alarm">⚠ 这台机器的指纹变了！</b>
            <small>之前记住的是：</small>
            <code class="fp-old">{sshForm.sshOldFp}</code>
            <small>现在连上看到的是：</small>
            <code>{sshForm.sshFp}</code>
            <small>服务器重装 / 换机器会这样，<b>被中间人劫持也会这样</b>。{sshForm.sshFpNote || '密码没有发出去。'}请到服务器上执行
              <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code> 亲自核对；对不上就别保存。</small>
          {:else if sshForm.sshAuthed}
            <b>认证通过。请核对主机指纹：</b>
            <code>{sshForm.sshFp}</code>
            <small>请确认它和你在服务器上执行 <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code> 看到的一致 —— 对不上可能是中间人。确认后此指纹会被记住，以后变了会直接拒连。</small>
          {:else}
            <!-- 只取了指纹、还没登录：密码要等你核对完指纹、点下一步才会发出去。 -->
            <b>已连上这台机器。请先核对主机指纹：</b>
            <code>{sshForm.sshFp}</code>
            <small>请确认它和你在服务器上执行 <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code> 看到的一致 —— 对不上可能是中间人。密码 / 私钥还没有发出去：点下面的按钮才会用它登录，然后记住这枚指纹，以后变了会直接拒连。</small>
          {/if}
        </div>
      {/if}

      <div class="row-end">
        <button class="btn ghost" onclick={() => (sshForm.sshOpen = false)}>取消</button>
        {#if sshForm.sshRenameOnly}
          <!-- 只改了名字：和「能不能连上」无关，不必为此去连一次机器 -->
          <button class="btn primary" onclick={saveSshEdit} disabled={sshForm.sshConnecting}>{sshForm.sshConnecting ? '保存中…' : '保存'}</button>
        {:else if !sshForm.sshFp}
          <button class="btn primary" onclick={testSsh} disabled={sshForm.sshTesting || !sshForm.sshCanTest}>{sshForm.sshTesting ? '连接中…' : '测试连接'}</button>
        {:else if sshForm.sshEditId}
          <button class="btn primary" class:danger={sshForm.sshFpChanged} onclick={saveSshEdit} disabled={sshForm.sshConnecting}>{sshForm.sshConnecting ? (sshForm.sshAuthed ? '保存中…' : '登录中…') : sshForm.sshFpChanged ? '我已核对，接受新指纹并登录' : sshForm.sshAuthed ? '指纹无误，保存' : '指纹无误，登录并保存'}</button>
        {:else}
          <button class="btn primary" onclick={confirmSsh} disabled={sshForm.sshConnecting}>{sshForm.sshConnecting ? '登录中…' : '指纹无误，登录并添加'}</button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .ssh-key-row { display: flex; gap: 6px; align-items: center; }
  .ssh-key-row .tin { flex: 1; min-width: 0; }
  .ssh-fp { display: flex; flex-direction: column; gap: 5px; padding: 9px 11px; border: 1px solid #cfc9ec; background: #f7f6ff; border-radius: 10px; }
  .ssh-fp b { font-size: 12.5px; }
  .ssh-fp > code { font-family: ui-monospace, monospace; font-size: 11.5px; color: var(--text); background: #fff; border: 1px solid var(--border); border-radius: 6px; padding: 4px 7px; overflow-wrap: anywhere; user-select: text; }
  .ssh-fp small { color: var(--dim); font-size: 11px; line-height: 1.5; }
  .ssh-fp small code { font-family: ui-monospace, monospace; font-size: 10.5px; background: #fff; border-radius: 4px; padding: 1px 4px; user-select: text; }
  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  .tin { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1px solid var(--border2); border-radius: 8px; padding: 5.5px 10px; }
  /* 单行输入（text/password/number/datetime-local）：显式统一高 --ctl-h，内容垂直居中；
     去掉竖向 padding 防 macOS 原生步进器/日期控件把高度顶开。textarea 不定高，不吃这条。 */
  input.tin { height: var(--ctl-h); box-sizing: border-box; padding-top: 0; padding-bottom: 0; line-height: normal; }
  .tin:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }
  .err-note { color: var(--err); background: var(--err-soft); border: 1px solid var(--err-border); border-radius: 8px; padding: 8px 10px; font-size: 13px; }
  .modal.wide { width: min(520px, 94vw); }
  .modal.ssh-modal .trow,
  .modal.ssh-modal .tfield,
  .modal.ssh-modal .ssh-key-row { max-width: 100%; }
  .trow { display: flex; gap: 12px; }
  .tfield { display: flex; flex-direction: column; gap: 5px; flex: 1; min-width: 0; }
  .tfield > span { font-size: 12px; color: var(--dim); }
  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1px solid var(--border2); border-radius: 8px; padding: 6.5px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
  .btn:hover { background: #f6f5f1; }
  .btn:disabled { opacity: .5; cursor: default; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .btn.primary:hover { background: var(--accent-h); }
  .btn.ghost { border-color: var(--border); }
  .btn.sm { padding: 5px 12px; font-size: 12px; border-radius: 8px; }
  .x { background: none; border: none; color: var(--faint); font-size: 20px; cursor: pointer; line-height: 1; }
  .x:hover { color: var(--err); }
  .mask { position: fixed; inset: 0; background: rgba(25,20,10,.28); z-index: 50; }
  .modal { position: fixed; z-index: 60; background: var(--bg); border: 1px solid var(--border); box-shadow: var(--shadow-lg); display: flex; flex-direction: column; }
  /* margin:auto 居中而非 transform——transform 会给内部 position:fixed 的下拉菜单
     制造 containing block，菜单被钳进弹窗坐标系再被 overflow:hidden 裁掉（遮挡）。 */
  .modal { inset: 0; margin: auto; height: fit-content; max-height: 88vh; width: min(440px, 92vw); border-radius: 14px; overflow: hidden; }
  .sheet-head { box-sizing: border-box; min-height: 48px; display: flex; justify-content: space-between; align-items: center; padding: 7px 14px; border-bottom: 1px solid var(--border); }
  .sheet-head > b { min-width: 0; overflow: hidden; font-size: 13.5px; line-height: 1.2; text-overflow: ellipsis; white-space: nowrap; }
  /* 弹窗正文统一只允许纵向滚动。只声明 overflow-y:auto 会按 CSS 规则把横向轴
     隐式计算成 auto；贴边 footer 的负边距和 WebView 的浮点取整都会因此冒出横向条。 */
  .sheet-body { min-width: 0; padding: 16px 18px; overflow-x: hidden; overflow-y: auto; display: flex; flex-direction: column; gap: 7px; }
  .hint { color: var(--dim); font-size: 12px; margin: 2px 0; line-height: 1.6; }
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
