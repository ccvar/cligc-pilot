<script lang="ts">
  import Dropdown from '$lib/Dropdown.svelte';
  import ModelFx from '$lib/ModelFx.svelte';
  import UsageRing from '$lib/UsageRing.svelte';
  import SshMark from '$lib/components/icons/SshMark.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import { sites, openSiteBoard, DEPLOY_LABEL } from '$lib/state/sites.svelte.js';
  import { gh, loadGhRepos } from '$lib/state/github.svelte.js';
  import CliGuide from '$lib/components/launcher/CliGuide.svelte';
  import WranglerNote from '$lib/components/launcher/WranglerNote.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { app } from '$lib/state/app.svelte.js';
  import { launcher, STYLE_OPTS, chooseWorkspaceFolder, workspaceFolderName, pickCombo, persistLauncherEffort, setFast } from '$lib/state/launcher.svelte.js';
  import { autogrow, composer, onComposerKey } from '$lib/state/composer.svelte.js';
  import { startChat } from '$lib/state/turns.svelte.js';
  import { permOptsFor, permTone, permTipFor, brainUsable, brainSetupHint, CODEX_GITHUB_NOTE } from '$lib/state/models';

  // 全部站点里开新对话：最近用过的站点排前面
  const siteOpts = $derived.by(() => {
    const rank = new Map(sites.recent.map((id, i) => [id, i]));
    return [...sites.list]
      .sort((a, b) => (rank.get(a.id) ?? 1e9) - (rank.get(b.id) ?? 1e9) || Number(b.pinned) - Number(a.pinned) || a.name.localeCompare(b.name, 'zh'))
      .map((s) => ({ value: s.id, label: s.name, img: sites.icons[s.id] ?? '', sub: s.domain && s.domain !== s.name ? s.domain : (DEPLOY_LABEL[s.deploy.mode] ?? '').split('（')[0] }));
  });
  const hubSite = $derived(sites.list.find((s) => s.id === launcher.lSite));
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="center launch-center" data-tauri-drag-region onmousedown={startDrag}>
  <div class="launcher">
    {#if app.isSitesHub}
      <h1>想让它在{hubSite ? `「${hubSite.name}」` : '哪个站点'}里做点什么？</h1>
      <p class="sub">在左下角选站点。它能改这个站点的代码、查 GA4 / Search Console / Bing 数据；上线由 Pilot 在站点页执行，模型拿不到 Cloudflare 凭据。</p>
    {:else if app.isSiteConn}
      <h1>想让它在「{sites.active?.name}」里做点什么？</h1>
      <p class="sub">{sites.active?.source === 'github' ? '它在仓库检出里改代码、走分支和 PR；' : '改动直接落在站点的本地文件夹；'}能查这个站点的 GA4 / Search Console / Bing 数据。上线由 Pilot 在站点页执行，模型拿不到 Cloudflare 凭据。</p>
    {:else if app.isCfConn}
      <h1>让 AI 帮你建个站</h1>
      <p class="sub">给项目起个名，描述你想要的网站——边聊边建，随时本地预览，满意了再部署到 Cloudflare。</p>
    {:else if app.isGhConn}
      <h1>想让它在仓库里做点什么？</h1>
      <p class="sub">选一个仓库，说清要改什么——它会克隆仓库、新建分支改代码并跑测试；在「询问」「自动」档下，推送和提 PR 前都会先问你。</p>
    {:else if app.isSshConn}
      <h1>想让它在这台机器上做点什么？</h1>
      <p class="sub">它通过 Pilot 在 {app.activeConn?.ssh_user}@{app.activeConn?.ssh_host} 上执行命令；每条命令按你选的权限档位确认，凭据始终不交给模型。</p>
    {:else}
      <h1>想让它帮你做点什么？</h1>
      <p class="sub">像聊天一样把需求说清楚；需要处理文件时，可以接入一个本地文件夹。</p>
    {/if}

      <div class="composer big">
          <textarea bind:value={launcher.lDraft} bind:this={launcher.lDraftEl} use:autogrow rows="3"
            placeholder={app.isSiteConn ? '例如：看看最近 28 天展现多但点击率低的查询词，给对应页面重写标题和描述' : app.isSshConn ? '例如：看看这台机器的配置和已装的东西，然后帮我装上 Docker' : app.isCfConn ? '例如：做个卖手冲咖啡的落地页，深色调，留个邮箱订阅表单存到 D1，先给我方案' : app.isGhConn ? '例如：修复登录页在 Safari 上的布局错位，补一个回归测试，然后提个 PR' : app.isWorkspaceChat ? '任意聊点什么；需要处理本地文件时，可在左下角添加一个文件夹' : '例如：分析这个站点最近的内容表现，并给出下一步运营计划'}
            oncompositionstart={() => (composer.composing = true)} oncompositionend={() => (composer.composing = false)}
            onkeydown={(e) => onComposerKey(e, startChat)}></textarea>
        <div class="composer-bar">
          <div class="cb-left">
            {#if app.isSitesHub}
              <Dropdown compact searchable bind:value={launcher.lSite} options={siteOpts} placeholder={sites.list.length ? '选择站点' : '还没有站点'} />
            {:else if app.isSiteConn}
              <button type="button" class="ssh-target site-target" onclick={() => openSiteBoard()} data-tip="打开站点看板"><SiteIcon id={sites.active?.id ?? ''} size={13} />{sites.active?.name}</button>
            {:else if app.isSshConn}
              <!-- 远程连接没有站点/项目可选：对象就是那台机器 -->
              <span class="ssh-target"><SshMark size={13} />{app.activeConn?.ssh_user}@{app.activeConn?.ssh_host}:{app.activeConn?.ssh_port}</span>
            {:else if app.isGhConn}
              <Dropdown compact searchable bind:value={launcher.lSite}
                options={gh.repos.map((r) => ({ value: r.full_name, label: r.full_name, sub: `${r.private ? '私有' : '公开'}${r.can_push ? '' : ' · 只读'}${gh.checkouts.includes(r.full_name) ? ' · 已克隆' : ''}` }))}
                placeholder={gh.reposLoading ? '正在读取仓库…' : gh.repos.length ? '选择仓库' : '没有可用仓库'} />
            {:else if app.isCfConn}
              <input class="cf-proj-in" bind:value={launcher.lSite} placeholder="项目名，如 coffee-landing" spellcheck="false" autocapitalize="off" autocorrect="off" />
              <Dropdown compact bind:value={launcher.lStyle} options={STYLE_OPTS} />
            {:else}
              <div class="workspace-picker">
                <button type="button" class="workspace-folder" class:selected={!!launcher.workspaceDir} data-tip={launcher.workspaceDir || '选择一个本地文件夹作为本次自由对话的工作目录'} onclick={chooseWorkspaceFolder}>
                  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M1.8 4.3a1.5 1.5 0 0 1 1.5-1.5h3l1.4 1.6h5a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5H3.3a1.5 1.5 0 0 1-1.5-1.5V4.3Z" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round" /></svg>
                  <span>{launcher.workspaceDir ? workspaceFolderName(launcher.workspaceDir) : '添加文件夹'}</span>
                </button>
                {#if launcher.workspaceDir}<button type="button" class="workspace-clear" aria-label="移除文件夹" data-tip="移除文件夹，改为纯对话" onclick={() => (launcher.workspaceDir = '')}>×</button>{/if}
              </div>
            {/if}
          </div>
          <div class="cb-right">
            <Dropdown compact bind:value={launcher.lPerm} options={permOptsFor(launcher.lBrain, app.isSshConn)} tone={permTone(launcher.lPerm)} tip={permTipFor(launcher.lBrain, launcher.lPerm, app.isSshConn)} />
            <ModelFx options={launcher.comboOpts} value={`${launcher.lBrain}::${launcher.lModel}`} effort={launcher.lEffort} fast={launcher.lFast} fastOk={app.brains?.claude_fast_ok ?? false} onpick={pickCombo} oneffort={persistLauncherEffort} onfast={setFast} />
            <UsageRing />
            <button class="send" onclick={startChat} disabled={((app.isCfConn || app.isGhConn || app.isSitesHub) && !launcher.lSite.trim()) || !launcher.lDraft.trim() || !brainUsable(launcher.lBrain)} title="发送（Enter）">↑</button>
          </div>
        </div>
      </div>
    {#if !brainUsable(launcher.lBrain)}<p class="hint warn-text">{brainSetupHint(launcher.lBrain)}</p>{/if}
    {#if app.isWorkspaceChat && app.brains && !app.anyBrainUsable}<CliGuide />{/if}
    {#if app.isCfConn && app.brains?.wrangler && !app.brains.wrangler.found}<WranglerNote />{/if}
    {#if (app.isGhConn || (app.isSiteConn && sites.active?.source === 'github')) && launcher.lBrain === 'codex'}<p class="hint warn-text">{CODEX_GITHUB_NOTE}</p>{/if}
    {#if app.isGhConn && gh.reposErr}<p class="hint warn-text">读取仓库失败：{gh.reposErr} <button class="link-btn" type="button" onclick={() => app.activeConnId && loadGhRepos(app.activeConnId, true)}>重试</button></p>{/if}
    {#if app.isGhConn && launcher.lSite && gh.repos.find((r) => r.full_name === launcher.lSite)?.can_push === false}<p class="hint">这枚 Token 对该仓库只有读权限：可以读代码、在本地改，但推不了分支、提不了 PR。</p>{/if}
  </div>
</div>

<style>
  :global(.canvas.multi .pane) .composer-bar{flex-wrap:wrap}
  :global(.canvas.multi .pane) .cb-right{margin-left:auto;max-width:100%;flex-wrap:wrap}
  /* 启动页：远程连接的目标机器（占站点选择器的位子——那台机器就是本次对话的对象） */
  .ssh-target { display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px; border: 1px solid var(--border); border-radius: 999px; background: var(--rail); color: var(--dim); font-size: 12px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  .ssh-target :global(svg) { color: var(--faint); flex: none; }
  .site-target { cursor: pointer; font-family: inherit; }
  .site-target:hover { color: var(--text); }
  .ssh-target :global(svg.distro) { transform: translateY(1px); }
  .workspace-picker { min-width: 0; display: inline-flex; align-items: center; gap: 2px; }
  .workspace-folder { min-width: 0; max-width: 210px; height: 27px; display: inline-flex; align-items: center; gap: 6px; padding: 0 9px; border: 1px solid var(--border); border-radius: 999px; background: var(--rail); color: var(--dim); font: inherit; font-size: 11.5px; cursor: pointer; }
  .workspace-folder:hover { background: #f4f2ed; color: var(--text); }
  .workspace-folder.selected { color: var(--text); background: #f7f6f2; }
  .workspace-folder svg { flex: none; color: var(--faint); }
  .workspace-folder span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .workspace-clear { width: 23px; height: 23px; padding: 0; border: 0; border-radius: 50%; background: transparent; color: var(--faint); font-size: 15px; line-height: 1; cursor: pointer; }
  .workspace-clear:hover { background: #eeeae3; color: var(--text); }
  /* safe center：内容比可视区高时退回顶对齐可滚动，避免居中把顶部裁掉。 */
  .center { flex: 1; display: flex; align-items: safe center; justify-content: center; overflow-y: auto; padding: 24px; }
  /* 启动页：大屏不无限拉宽，入口区与下方的新站引导共用同一条 920px 内容基线。 */
  .center.launch-center { align-items: safe center; justify-content: flex-start; padding: 24px 40px; }
  .center.launch-center .launcher { width: min(920px, 100%); margin-inline: auto; }
  .launcher { width: min(680px, 100%); }
  .launcher h1 { font-size: 26px; margin: 0 0 6px; letter-spacing: -.01em; }
  .launcher .sub { color: var(--dim); margin: 0 0 22px; }
  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  textarea { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1.5px solid var(--border2); border-radius: 8px; padding: 5px 10px; }
  textarea:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }
  /* 输入框（仿 Claude Code：整块圆角边框，聚焦时高亮，发送按钮嵌在框内） */
  .composer.big { position: relative; background: #fff; border: 1px solid var(--border2); border-radius: 12px; box-shadow: none; transition: border-color .12s, box-shadow .12s; }
  .composer.big:focus-within { border-color: #b7b2a6; box-shadow: none; }
  .composer.big textarea { width: 100%; resize: none; border: none; background: none; box-shadow: none; padding: 14px 52px 14px 17px; font-size: 14px; line-height: 1.6; max-height: 200px; display: block; }
  .composer.big textarea:focus { outline: none; box-shadow: none; border: none; }
  .composer .send { position: absolute; right: 9px; bottom: 9px; width: 32px; height: 32px; border-radius: 50%; border: none; background: var(--accent); color: #fff; font-size: 16px; cursor: pointer; display: flex; align-items: center; justify-content: center; transition: background .12s, transform .08s; }
  .composer .send:hover { background: var(--accent-h); }
  .composer .send:active { transform: scale(.92); }
  .composer .send:disabled { background: #dcdad2; cursor: default; transform: none; }
  /* composer：竖排 textarea + 底栏（启动页站点在左；会话页只读厂商在左，模型/发送在右）。 */
  .composer.big { display: flex; flex-direction: column; }
  .composer.big textarea { padding: 12px 16px 6px 16px; }
  .composer-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 8px 8px; }
  .cb-left { display: flex; align-items: center; gap: 1px; flex: 1; min-width: 0; }
  .cb-right { display: flex; align-items: center; gap: 3px; flex: none; }
  .composer-bar .send { position: static; width: 30px; height: 30px; margin-left: 3px; font-size: 15px; }
  /* 启动页底栏空间不足时先收项目名，避免它把后面的风格/权限控件挤到一起。 */
  .cf-proj-in {
    flex: 0 1 190px; width: 190px; min-width: 72px; max-width: 190px;
    border: none; background: none; font-size: 13px; color: var(--text); padding: 3px 6px; outline: none;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .cf-proj-in::placeholder { color: #b3ada2; }
  .hint { color: var(--dim); font-size: 12px; margin: 2px 0; line-height: 1.6; }
  .warn-text { color: var(--warn); }
  @media (max-width: 760px) {
    .center.launch-center { padding: 20px 18px; }
  }
  .link-btn { padding: 0; border: 0; background: none; color: var(--accent); font: inherit; text-decoration: underline; cursor: pointer; }
</style>
