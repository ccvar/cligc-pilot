<script lang="ts">
  import AppIcon from '$lib/AppIcon.svelte';
  import WorkspaceMark from '$lib/components/icons/WorkspaceMark.svelte';
  import SshMark from '$lib/components/icons/SshMark.svelte';
  import CfMark from '$lib/components/icons/CfMark.svelte';
  import DistroMark from '$lib/components/icons/DistroMark.svelte';
  import GearIcon from '$lib/components/icons/GearIcon.svelte';
  import ConnectionMark from '$lib/components/icons/ConnectionMark.svelte';
  import { connections, openLocalWorkspace, selectConn, openConnCtx, connectionDisplayName, distroOf, sshSub, probeOs, remoteConns, openRemote, accountConns, openAccounts } from '$lib/state/connections.svelte.js';
  import { app } from '$lib/state/app.svelte.js';
  import { WORKSPACE_CONN_ID } from '$lib/state/env';
  import { openCfConnect } from '$lib/state/cloudflare.svelte.js';
  import { gh, openGhConnect } from '$lib/state/github.svelte.js';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import { openSshConnect } from '$lib/state/ssh.svelte.js';
  import { openConnectionSetup } from '$lib/state/settings.svelte.js';
  import { sidebar } from '$lib/state/sidebar.svelte.js';
  import SiteMark from '$lib/components/icons/SiteMark.svelte';
  import Icon from '$lib/components/site/Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import { sites, openSiteImport } from '$lib/state/sites.svelte.js';
  import { openSiteCreate, makingSites } from '$lib/state/siteCreate.svelte.js';
  import { openMigrate } from '$lib/state/migrate.svelte.js';
  import { fleet } from '$lib/state/fleet.svelte.js';
  import { SITES_CONN_ID, SITES_CONN, LOCAL_CONN, REMOTES_CONN, CLOUDFLARES_CONN, GITHUBS_CONN } from '$lib/state/env';
  import { isServiceConn } from '$lib/state/datasrc.svelte.js';
  import { mask, maskTip } from '$lib/mask';
  const footName = $derived(app.activeConn ? connectionDisplayName(app.activeConn) : '未连接');
  // 制作中的站点单独计数，不算进「N 个站点」（和全部站点页一致）
  const makingN = $derived(makingSites().length);
  const siteCount = $derived(sites.list.length - makingN);
</script>

<div class="foot-wrap" bind:this={connections.footerEl}>
  {#if connections.switcherOpen}
    <div class="conn-switch" role="menu" aria-label="切换连接">
      <div class="cs-scroll" aria-label="连接列表">
        <button class="cs-item {app.activeConnId === WORKSPACE_CONN_ID ? 'on' : ''}" role="menuitemradio" aria-checked={app.activeConnId === WORKSPACE_CONN_ID} onclick={openLocalWorkspace} oncontextmenu={(e) => openConnCtx(e, LOCAL_CONN)}>
          <WorkspaceMark size={18} />
          <span class="cs-main"><b>本地对话</b><small>不连接任何服务，可接入本地文件夹</small></span>
          {#if app.activeConnId === WORKSPACE_CONN_ID}<span class="cs-check">✓</span>{/if}
        </button>
        <!-- 站点再多也只占一行：进去之后左栏就是可搜索的站点列表。 -->
        <button class="cs-item {app.isSitesMode ? 'on' : ''}" role="menuitemradio" aria-checked={app.isSitesMode} onclick={() => { connections.switcherOpen = false; void selectConn(SITES_CONN_ID); }} oncontextmenu={(e) => openConnCtx(e, SITES_CONN)}>
          <SiteMark size={18} />
          <span class="cs-main"><b>站点</b><small>{sites.list.length ? `${siteCount} 个站点${makingN ? ` · ${makingN} 个制作中` : ''}${fleet.pendingCount ? ` · ${fleet.pendingCount} 条待确认` : ''}` : '导入已有的站点，或让 AI 从零做一个'}</small></span>
          {#if app.isSitesMode}<span class="cs-check">✓</span>{/if}
        </button>
        <!-- 远程连接同样合并成一行：进去之后左栏列出全部机器。一台都没有就不显示。 -->
        {#if remoteConns().length}
          <button class="cs-item {app.isSshConn ? 'on' : ''}" role="menuitemradio" aria-checked={app.isSshConn} onclick={openRemote} oncontextmenu={(e) => openConnCtx(e, REMOTES_CONN)}>
            <SshMark size={18} />
            <span class="cs-main"><b>远程连接</b><small>{remoteConns().length} 台机器{app.isSshConn && app.activeConn ? ` · 当前 ${connectionDisplayName(app.activeConn)}` : ''}</small></span>
            {#if app.isSshConn}<span class="cs-check">✓</span>{/if}
          </button>
        {/if}
        <!-- Cloudflare / GitHub 账号也各自合并成一行：进去之后左栏列出这一类的全部账号。 -->
        {#if accountConns('cloudflare').length}
          <button class="cs-item {app.isCfConn ? 'on' : ''}" role="menuitemradio" aria-checked={app.isCfConn} onclick={() => openAccounts('cloudflare')} oncontextmenu={(e) => openConnCtx(e, CLOUDFLARES_CONN)}>
            <CfMark size={18} />
            <span class="cs-main"><b>Cloudflare</b><small>{accountConns('cloudflare').length} 个账号{app.isCfConn && app.activeConn ? ` · 当前 ${connectionDisplayName(app.activeConn)}` : ''}</small></span>
            {#if app.isCfConn}<span class="cs-check">✓</span>{/if}
          </button>
        {/if}
        {#if accountConns('github').length}
          <button class="cs-item {app.isGhConn ? 'on' : ''}" role="menuitemradio" aria-checked={app.isGhConn} onclick={() => openAccounts('github')} oncontextmenu={(e) => openConnCtx(e, GITHUBS_CONN)}>
            <GhMark size={18} />
            <span class="cs-main"><b>GitHub</b><small>{accountConns('github').length} 个账号{app.isGhConn && app.activeConn ? ` · 当前 ${app.activeConn.login || connectionDisplayName(app.activeConn)}` : ''}</small></span>
            {#if app.isGhConn}<span class="cs-check">✓</span>{/if}
          </button>
        {/if}
        <!-- 其它类型的连接（如果有）还是一条一行 -->
        {#each app.conns.filter((c) => !isServiceConn(c) && !['ssh', 'cloudflare', 'github'].includes(c.kind)) as c (c.id)}
          <button class="cs-item {app.activeConnId === c.id ? 'on' : ''}" role="menuitemradio" aria-checked={app.activeConnId === c.id} onclick={() => { selectConn(c.id); connections.switcherOpen = false; }}
            oncontextmenu={(e) => openConnCtx(e, c)}>
            <ConnectionMark kind={c.kind} size={18} />
            <span class="cs-main"><b>{connectionDisplayName(c)}</b></span>
            {#if app.activeConnId === c.id}<span class="cs-check">✓</span>{/if}
          </button>
        {/each}
      </div>
      <div class="cs-actions">
        <div class="cs-div"></div>
        <!-- 和站点页的按钮同一套图标：制作 = 魔法棒，导入 = 上传箭头（原来两个都是地球，分不出来） -->
        <button class="cs-act" role="menuitem" onclick={() => openSiteCreate()}><Icon name="wand" size={15} />制作站点</button>
        <button class="cs-act" role="menuitem" onclick={() => openMigrate()}><Icon name="route" size={15} />改版已有站点</button>
        <button class="cs-act" role="menuitem" onclick={() => openSiteImport()}><Icon name="upload" size={15} />导入站点</button>
        <button class="cs-act" role="menuitem" onclick={openCfConnect}><CfMark size={15} />连接 Cloudflare</button>
        <button class="cs-act" role="menuitem" onclick={openGhConnect}><GhMark size={15} />连接 GitHub</button>
        <button class="cs-act" role="menuitem" onclick={() => { connections.switcherOpen = false; openSshConnect(); }}><SshMark size={15} />新建远程连接</button>
        <button type="button" class="cs-act" role="menuitem" onclick={openConnectionSetup}><GearIcon />管理连接与模型…</button>
      </div>
    </div>
  {/if}
<button class="rail-foot" class:open={connections.switcherOpen} onclick={() => { connections.switcherOpen = !connections.switcherOpen; }}>
  {#if app.isSiteConn && sites.active}<SiteIcon id={sites.active.id} size={18} />{:else if app.isSitesMode}<SiteMark size={18} />{:else if app.isCfConn}<CfMark size={18} />{:else if app.isGhConn}<GhMark size={18} />{:else if app.isSshConn}<SshMark size={18} />{:else if app.activeConn}<WorkspaceMark size={18} />{:else}<AppIcon size={18} />{/if}
  <span class="foot-main">
    <b data-tip={maskTip(footName, mask(footName))}>{mask(footName)}</b>
    {#if app.activeConn}<small>{#if app.activeConn.kind === 'sites'}{siteCount} 个站点{makingN ? ` · ${makingN} 个制作中` : ''}{:else if app.isSiteConn}{sites.active?.domain || '站点'} · 共 {siteCount} 个站点{:else if app.isCfConn}{app.cfProjects.length} 个项目{sidebar.cfSiteCount ? ` · ${sidebar.cfSiteCount} 个站点` : ''}{:else if app.isGhConn}{app.activeConn.login}{gh.repos.length ? ` · ${gh.repos.length} 个仓库` : ''}{:else if app.isSshConn}<!--
      远程连接：这里放远端系统版本（点一下重探，比如刚 do-release-upgrade 过）。
      外层已经是按钮，这里不能再套一个可聚焦的「按钮」：只留鼠标点击重探（键盘走外层按钮打开切换器）。
    --><!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions --><span class="foot-os" title="远端系统 · 点击重新检测"
      onclick={(e) => { e.stopPropagation(); probeOs(app.activeConnId, true); }}
      ><DistroMark id={distroOf(app.activeConn)} size={12} />{connections.osProbing[app.activeConnId] ? '检测系统…' : sshSub(app.activeConn)}</span>{:else}本地 · 不连接任何服务{/if}</small>{/if}
  </span>
  <!-- 永远是展开箭头：这是个切换器，不是设置快捷键。 -->
  <svg class="foot-chev" class:up={connections.switcherOpen} width="13" height="13" viewBox="0 0 12 12" fill="none">
    <path d="M2.75 7.5L6 4.25L9.25 7.5" stroke="currentColor" stroke-width="1.15" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
</button>
</div>

<style>
  /* 远端系统信息（页脚可点＝重新检测） */
  .foot-os { display: inline-flex; align-items: center; gap: 4px; min-width: 0; }
  .foot-os { cursor: pointer; border-radius: 5px; padding: 0 3px; margin-left: -3px; }
  .foot-os:hover { background: var(--accent-soft); color: var(--text); }
  .foot-os :global(svg.distro) { transform: translateY(1px); }
  .rail-foot { width: 100%; display: flex; align-items: center; gap: 8px; padding: 8px 12px; border: none; border-top: 1px solid var(--border); background: none; cursor: pointer; text-align: left; -webkit-appearance: none; appearance: none; box-shadow: none; }
  .rail-foot:focus, .rail-foot:active { outline: none; box-shadow: none; }
  .rail-foot:hover { background: #f5f4f1; }
  .rail-foot.open { background: #f5f4f1; border-top-color: transparent; }
  .foot-main { flex: 1; min-width: 0; }
  .foot-main b { display: block; font-size: 12.5px; line-height: 1.15; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .foot-main small { display: block; color: var(--dim); font-size: 10.5px; line-height: 1.1; }
  .foot-chev { color: var(--faint); flex: none; transition: transform .15s; }
  .foot-chev.up { transform: rotate(180deg); }
  .foot-wrap { position: relative; }
  .conn-switch {
    position: absolute; left: 8px; right: 8px; bottom: calc(100% + 4px); z-index: 40;
    background: #fff; border: 1px solid var(--border2); border-radius: 12px;
    box-shadow: 0 10px 30px rgba(30,25,15,.15); padding: 5px;
    display: flex; flex-direction: column; max-height: min(640px, calc(100vh - 86px)); overflow: hidden;
    animation: pop .1s ease-out;
  }
  .cs-scroll {
    min-height: 0; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable;
  }
  .cs-actions { flex: none; }
  .cs-item {
    width: 100%; display: flex; align-items: center; gap: 9px;
    background: none; border: none; border-radius: 8px; padding: 5.5px 9px; cursor: pointer; text-align: left; font: inherit;
    color: var(--text);
  }
  .cs-item:hover { background: #f4f3ef; }
  .cs-item.on { background: #efeee9; }
  .cs-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .cs-main b { font-weight: 500; font-size: 13px; line-height: 1.2; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* 连接副标题（Key 前缀 · Cloudflare / Ubuntu … LTS 之类）：更小更淡，让连接名更突出 */
  .cs-main small {
    display: block;
    width: 100%;
    min-width: 0;
    color: var(--faint);
    font-size: 10px;
    line-height: 1.25;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cs-check { color: var(--accent); flex: none; font-size: 13px; }
  .cs-div { height: 1px; background: var(--border); margin: 3.5px 4px; }
  /* 动作行是单行：比原来(7px)收，但别收到发挤——padding 5.5px + line-height 1.3 落在约 28px，
     比原来 37px 紧一截、又留了呼吸感（23px 太挤，实测反馈）。连接行是两行，另按 .cs-item 定高。 */
  .cs-act {
    width: 100%; display: flex; align-items: center; gap: 8px;
    background: none; border: none; border-radius: 8px; padding: 5.5px 9px; cursor: pointer; text-align: left;
    font: inherit; font-size: 12.5px; line-height: 1.3; color: var(--dim);
  }
  .cs-act:hover { background: #f4f3ef; color: var(--text); }
  .cs-act :global(svg) { color: var(--faint); flex: none; }
  @keyframes pop { from { opacity: 0; transform: translateY(4px); } }
</style>
