<script lang="ts">
  // 站点运营作业失败的报错下面那一行：问题在哪、去哪改（充值页、API 服务设置）；认不出的报错不显示。
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { opsErrorHint } from '$lib/friendlyError';
  import { openConnectionSetup } from '$lib/state/settings.svelte.js';
  import { invoke } from '@tauri-apps/api/core';
  import { sites, refreshSites, openSite, authorizeDeploy } from '$lib/state/sites.svelte.js';
  import { say, confirmDialog } from '$lib/state/app.svelte.js';
  let { text, siteId = '' }: { text: string; siteId?: string } = $props();
  const h = $derived(opsErrorHint(text));
  // 发布脚本嫌参数不够、打印了用法：后端从用法里认出了该用的命令，这里一键改好
  const site = $derived(siteId ? sites.list.find((s) => s.id === siteId) : undefined);
  const cmdFix = $derived(text?.match(/发布命令少了参数。脚本的用法是「(.+?)」/)?.[1] ?? usageLine(text, site?.deploy.command ?? ''));
  // 旧版本记下的报错是脚本的原始输出：同样在里面找「现在的命令 + 更多参数」那一行（规则和后端 site_deploy::missing_args 一样）
  function usageLine(t: string, cmd: string) {
    const norm = (x: string) => x.trim().split(/\s+/).join(' ');
    const c = norm(cmd);
    if (!c || !/^发布失败：退出码/.test(t ?? '')) return '';
    return t.split('\n').map(norm).find((l) => l.startsWith(c + ' ') && !l.slice(c.length).trim().split(' ').some((x) => /[<[]/.test(x) || (x.length > 1 && /^[A-Z0-9_]+$/.test(x)))) ?? '';
  }
  // 服务器还没授权：就在这里授权。授权是按连接算的，同一条连接上别的站点授权过这里也算好了
  const needsAuth = $derived(!!siteId && !!text?.includes('还没有为部署授权'));
  let authed = $state(false);
  let authing = $state(false);
  $effect(() => {
    if (!needsAuth) return;
    invoke<{ has_key: boolean }>('vps_key_status', { id: siteId }).then((s) => { authed = s.has_key; }).catch(() => { authed = false; });
  });
  async function authorize() {
    authing = true;
    try { if (await authorizeDeploy(siteId)) authed = true; }
    catch (e) { say(String(e), 'err'); }
    finally { authing = false; }
  }
  async function fixCommand() {
    if (!site || !cmdFix) return;
    if (!(await confirmDialog(`把 ${site.name} 的发布命令改成：\n${cmdFix}\n\n（现在是：${site.deploy.command}）改好后点「重新上线」就会用新命令发布。`, { title: '改发布命令', kind: 'info', confirmText: '改好' }))) return;
    try {
      await invoke('save_site', { site: { ...site, deploy: { ...site.deploy, command: cmdFix } } });
      await refreshSites();
      say('发布命令改好了，重新上线就会用它');
    } catch (e) { say(String(e), 'err'); }
  }
</script>

{#if cmdFix}
  <div class="oeh">
    <span>发布脚本说参数不够，打印了它的用法。用法里对应的命令是 <code>{cmdFix}</code>。</span>
    {#if site && site.deploy.command.trim() !== cmdFix}<button class="link-btn" onclick={fixCommand}>改成这个命令</button>{:else if site}<span class="ok">已改好，重新上线就行</span>{/if}
    {#if siteId}<button class="link-btn" onclick={() => openSite(siteId, 'settings')}>去部署设置</button>{/if}
  </div>
{/if}

{#if needsAuth}
  <div class="oeh">
    {#if authed}<span class="ok">已授权，可以重新上线了</span>
    {:else}<span>这台服务器还没给 Pilot 授权，授权后才能发布。</span>
      <button class="btn tiny primary" onclick={authorize} disabled={authing}>{authing ? '授权中…' : '为部署授权'}</button>
      <button class="link-btn" onclick={() => openSite(siteId, 'settings')}>去站点设置</button>{/if}
  </div>
{/if}

{#if h}
  <div class="oeh">
    <span>{h.text}</span>
    {#if h.billing}<button class="link-btn" onclick={() => openUrl(h.billing)}>去充值 ↗</button>{/if}
    {#if h.settings}<button class="link-btn" onclick={(e) => openConnectionSetup(e)}>打开 API 设置</button>{/if}
  </div>
{/if}

<style>
  .oeh { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 12px; font-size: 12px; line-height: 1.6; color: var(--dim); }
  .oeh span { flex: 1 1 320px; min-width: 0; }
  .oeh .link-btn { flex: none; font-size: 12px; }
  .oeh .btn { flex: none; }
  .oeh code { font-size: 11.5px; padding: 0 4px; border-radius: 4px; background: rgba(0, 0, 0, .05); }
  .oeh .ok { flex: none; color: var(--ok); }
</style>
