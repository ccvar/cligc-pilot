<script lang="ts">
  // 「连接」里的 WordPress.com 组：发文章用的 WordPress.com 账号。第一次用要先填自己注册的应用（Client ID / Secret）。
  // 没有账号、也没在填应用时不显示，从右上角 + 加。
  import ConnGroup from '$lib/components/settings/ConnGroup.svelte';
  import { untrack } from 'svelte';
  import WordpressMark from '$lib/components/icons/WordpressMark.svelte';
  import SettingsIcon from '$lib/SettingsIcon.svelte';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { connections, toggleConnCtx, openConnCtx } from '$lib/state/connections.svelte.js';
  import { wp, wpAccounts, startWpConnect, saveWpClient, wpLogin, loadWpClient } from '$lib/state/wordpress.svelte.js';
  import { setFolded } from '$lib/state/connFold.svelte.js';
  import { mask, maskTip } from '$lib/mask';

  const list = $derived(wpAccounts());
  let cid = $state('');
  let secret = $state('');
  let saving = $state(false);
  // untrack：setFolded 会读再写折叠状态，直接放在 effect 里会无限重跑
  $effect(() => { if (wp.setup) untrack(() => { setFolded('wordpress', false); void loadWpClient(); }); });
  async function saveAndLogin() {
    saving = true;
    const ok = await saveWpClient(cid, secret);
    saving = false;
    if (ok) { secret = ''; await wpLogin(''); }
  }
</script>

{#snippet icon()}<WordpressMark size={14} />{/snippet}
{#snippet acts()}
  <InfoTip text={'站点「分发」把新文章的摘要发到这个账号下的 WordPress.com 站点。\n第一次要在 developer.wordpress.com/apps 注册一个应用，把 Client ID / Secret 填进来（只填一次）。'} label="WordPress.com 怎么用" />
{/snippet}

{#if list.length || wp.busy || wp.setup}
  <ConnGroup kind="wordpress" label="WordPress.com" n={list.length} {icon} actions={acts} add={() => void startWpConnect()} addTip="添加 WordPress.com 账号">
    {#each list as c (c.id)}
      {@const nm = c.remark?.trim() || c.login || c.name}
      <div class="conn-row static" class:on={connections.connCtx?.conn.id === c.id} role="group" oncontextmenu={(e) => openConnCtx(e, c)}>
        <b class="cr-name" data-tip={maskTip(nm, mask(nm))}>{mask(nm)}</b>
        <small class="cr-sub"></small>
        <button class="gear cr-gear" class:active={connections.connCtx?.conn.id === c.id} title="连接设置" aria-label="连接设置" onclick={(e) => toggleConnCtx(e, c)}><SettingsIcon /></button>
      </div>
    {/each}
    {#if wp.busy}
      <div class="conn-row static"><small class="cr-sub">在浏览器里登录 WordPress.com，点「批准」…</small></div>
    {/if}
    {#if wp.setup}
      <div class="setup">
        <ol>
          <li>打开 <button class="lnk" onclick={() => openUrl('https://developer.wordpress.com/apps/new/')}>developer.wordpress.com/apps</button>，用你要发文章的 WordPress.com 账号登录，新建一个应用</li>
          <li>名称随意（如 Cligc Pilot），Website URL 填你的网站；<b>Redirect URLs</b> 填 <code>{wp.client?.redirect ?? 'http://127.0.0.1:47613/'}</code>；Type 选 Web</li>
          <li>创建后把应用页面上的 Client ID 和 Client Secret 填到下面（只存在钥匙串里）</li>
        </ol>
        <input class="mi" placeholder="Client ID（一串数字）" bind:value={cid} autocomplete="off" spellcheck="false" />
        <input class="mi" type="password" placeholder="Client Secret" bind:value={secret} autocomplete="off" />
        <div class="act">
          <button class="how" onclick={() => (wp.setup = false)}>取消</button>
          <button class="bind" onclick={saveAndLogin} disabled={saving || !cid.trim() || !secret.trim()}>{saving ? '保存中…' : '保存并登录'}</button>
        </div>
      </div>
    {/if}
  </ConnGroup>
{/if}

<style>
  .gear { background: none; border: none; cursor: pointer; padding: 4px; border-radius: 6px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .gear:hover { background: #f1efe9; color: var(--text); }
  .setup { display: flex; flex-direction: column; gap: 6px; padding: 8px 10px 10px 31px; border-top: 1px solid #f1f0ec; }
  .setup ol { margin: 0; padding-left: 16px; color: var(--dim); font-size: 11.5px; line-height: 1.7; }
  .setup code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; background: #f3f2ee; padding: 1px 4px; border-radius: 4px; color: var(--text); user-select: all; }
  .mi { height: 30px; box-sizing: border-box; padding: 0 9px; border: 1px solid var(--border2); border-radius: 7px; outline: none; background: #fff; color: var(--text); font: inherit; font-size: 12.5px; }
  .mi:focus { border-color: #b7b2a6; }
  .act { display: flex; justify-content: flex-end; gap: 6px; }
  .lnk { border: none; background: none; padding: 0; font: inherit; color: #1a73e8; cursor: pointer; }
  .lnk:hover { text-decoration: underline; }
  .how { flex: none; border: none; background: none; padding: 3px 6px; border-radius: 6px; font: inherit; font-size: 11.5px; color: var(--dim); cursor: pointer; }
  .how:hover { background: #f1efe9; color: var(--text); }
  .bind { flex: none; border: 1px solid var(--border2); background: #fff; border-radius: 7px; padding: 2px 9px; font: inherit; font-size: 11.5px; color: var(--text); cursor: pointer; white-space: nowrap; }
  .bind:hover { background: #f6f5f1; }
  .bind:disabled { opacity: .5; cursor: default; }
</style>
