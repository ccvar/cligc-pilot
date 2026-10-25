<script lang="ts">
  // 「连接」里的 Blogger 组：发博客用的 Google 账号（和看网站数据的 Google 账号分开）。没有账号时不显示，从右上角 + 加。
  import ConnGroup from '$lib/components/settings/ConnGroup.svelte';
  import BloggerMark from '$lib/components/icons/BloggerMark.svelte';
  import SettingsIcon from '$lib/SettingsIcon.svelte';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import { connections, toggleConnCtx, openConnCtx } from '$lib/state/connections.svelte.js';
  import { blogger, bloggerAccounts, bloggerLogin, cancelBloggerLogin } from '$lib/state/blogger.svelte.js';
  import { mask, maskTip } from '$lib/mask';

  const list = $derived(bloggerAccounts());
</script>

{#snippet icon()}<BloggerMark size={14} />{/snippet}
{#snippet acts()}
  <InfoTip text={'站点「分发」把新文章的摘要发到这个账号下的博客。\n只申请 Blogger 一项权限，和看网站数据的 Google 账号分开。\n博客要先在 blogger.com 上建好。'} label="Blogger 怎么用" />
{/snippet}

{#if list.length || blogger.busy}
  <ConnGroup kind="blogger" label="Blogger" n={list.length} {icon} actions={acts} add={() => void bloggerLogin('')} addTip="添加 Blogger 账号">
    {#each list as c (c.id)}
      {@const nm = c.remark?.trim() || c.login || c.name}
      <div class="conn-row static" class:on={connections.connCtx?.conn.id === c.id} role="group" oncontextmenu={(e) => openConnCtx(e, c)}>
        <b class="cr-name" data-tip={maskTip(nm, mask(nm))}>{mask(nm)}</b>
        <small class="cr-sub"></small>
        <button class="gear cr-gear" class:active={connections.connCtx?.conn.id === c.id} title="连接设置" aria-label="连接设置" onclick={(e) => toggleConnCtx(e, c)}><SettingsIcon /></button>
      </div>
    {/each}
    {#if blogger.busy}
      <div class="conn-row static">
        <small class="cr-sub">在浏览器里选 Google 账号、勾上 Blogger 权限…</small>
        <button class="cancel" onclick={cancelBloggerLogin}>取消</button>
      </div>
    {/if}
  </ConnGroup>
{/if}

<style>
  .gear { background: none; border: none; cursor: pointer; padding: 4px; border-radius: 6px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .gear:hover { background: #f1efe9; color: var(--text); }
  .cancel { flex: none; border: none; background: none; padding: 3px 6px; border-radius: 6px; font: inherit; font-size: 11.5px; color: var(--dim); cursor: pointer; }
  .cancel:hover { background: #f1efe9; color: var(--text); }
</style>
