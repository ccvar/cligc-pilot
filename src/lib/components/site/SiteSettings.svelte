<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import Dropdown from '$lib/Dropdown.svelte';
  import SiteDataSettings from './SiteDataSettings.svelte';
  import VpsAuth from './VpsAuth.svelte';
  import SiteConfigCard from './SiteConfigCard.svelte';
  import SiteDynamicSettings from './SiteDynamicSettings.svelte';
  import { isPagesDirect, isVpsManaged } from '$lib/state/dynamic.svelte.js';
  import SiteVpsCard from './SiteVpsCard.svelte';
  import { app, say } from '$lib/state/app.svelte.js';
  import { invoke } from '@tauri-apps/api/core';
  import { sites, editSite, saveSiteDraft, removeSite, takedownSite } from '$lib/state/sites.svelte.js';
  import { setTags, fleet } from '$lib/state/fleet.svelte.js';

  let { site }: { site: Site } = $props();
  $effect(() => { if (!sites.draft || sites.draft.id !== site.id) editSite(site); });
  const d = $derived(sites.draft && sites.draft.id === site.id ? sites.draft : null);
  const ghConns = $derived(app.conns.filter((c) => c.kind === 'github'));
  const cfConns = $derived(app.conns.filter((c) => c.kind === 'cloudflare'));
  const dirty = $derived(!!d && JSON.stringify({ ...d, updated_at: 0 }) !== JSON.stringify({ ...site, updated_at: 0 }));
  let checksText = $state('');
  // 只在换了站点 / 草稿重建时回填一次。之前每次 syncChecks 写回 d.build.checks 都会触发回填，
  // 把刚敲的空格和换行吃掉——多个词、多行的检查命令只能粘贴、没法打字
  let checksFor: Site | null = null;
  $effect(() => { if (d && d !== checksFor) { checksFor = d; checksText = d.build.checks.join('\n'); } });
  function syncChecks() { if (d) d.build.checks = checksText.split('\n').map((c) => c.trim()).filter(Boolean); }
  let tagIn = $state('');
  // 危险操作默认收起，点标题才展开（每次进设置页都是收起的，免得误点）
  let dzOpen = $state(false);
  $effect(() => { void site.id; dzOpen = false; });
  async function addTag() {
    const t = tagIn.trim();
    if (!t) return;
    if (await setTags(site.id, [...site.tags, t])) tagIn = '';
  }
  // 从 Cloudflare 识别：按域名 / 仓库找这个站点的 Pages 项目，填进下面（保存后生效）
  let detecting = $state(false);
  async function detectDeploy() {
    if (!d) return;
    detecting = true;
    try {
      const r = await invoke<{ conn_id: string; conn_name: string; mode: string; project: string; branch: string; source_repo: string; matched_by: string } | null>('detect_cf_deploy', { id: site.id });
      if (!r) { say('在已连接的 Cloudflare 账号里没找到这个站点的 Pages 项目（按域名和 GitHub 仓库找的）。用 Workers 部署的请直接选「部署到 Workers」。', 'err'); return; }
      d.deploy.mode = r.mode as typeof d.deploy.mode; d.deploy.cf_conn_id = r.conn_id; d.deploy.project = r.project; d.deploy.branch = r.branch;
      say(`找到了：${r.conn_name} 下的 Pages 项目 ${r.project}${r.mode === 'pages_git' ? `（绑定 GitHub ${r.source_repo}，推送到 ${r.branch} 即上线）` : '（直接上传）'}，${r.matched_by === 'domain' ? '按域名认出' : '按仓库认出'}。确认无误后保存。`);
    } catch (e) { say(String(e), 'err'); }
    finally { detecting = false; }
  }
  // 每张卡片默认只显示现在的设置，点「修改」才出编辑框；一次只改一张，保存 / 取消都在卡片里
  let editing = $state('');
  function startEdit(k: string) { editSite(site); editing = k; }
  function cancelEdit() { editSite(site); editing = ''; }
  async function saveEdit() { if (await saveSiteDraft()) editing = ''; }
  const conn = (id: string) => app.conns.find((c) => c.id === id);
  const connName = (id: string) => { const c = conn(id); return c ? c.remark?.trim() || c.name : '未选择'; };
  const MODE_LABEL: Record<string, string> = { direct: 'Pilot 构建后上传到 Cloudflare Pages', workers: 'Pilot 构建后部署到 Cloudflare Workers', vps: 'VPS（自己的服务器）', pages_git: 'Cloudflare Pages 绑定 GitHub（推送即上线）', none: '暂不部署' };
  const modeOpts = $derived([
    { value: 'direct', label: 'Pilot 构建后上传到 Pages' },
    { value: 'workers', label: 'Pilot 构建后部署到 Workers（wrangler deploy）' },
    { value: 'vps', label: 'VPS（自己的服务器）' },
    { value: 'pages_git', label: site.source === 'github' ? 'Pages 绑定 GitHub（合并即上线）' : 'Pages 绑定 GitHub（推送到 GitHub 即上线）' },
    { value: 'none', label: '暂不部署' },
  ]);
</script>

{#snippet head(icon: string, title: string, key: string)}
  <h3><Icon name={icon} />{title}<span class="grow"></span>{#if editing !== key}<button class="ic-btn ic-sm" onclick={() => startEdit(key)} disabled={!!editing && editing !== key} aria-label={`修改${title}`} data-tip={!!editing && editing !== key ? '先保存或取消正在改的那一张' : `修改${title}`}><Icon name="pencil" size={14} /></button>{/if}</h3>
{/snippet}
{#snippet editBar()}
  <div class="edit-bar"><span class="grow"></span><button class="btn small ghost" onclick={cancelEdit} disabled={sites.saving}>取消</button><button class="btn small primary" onclick={saveEdit} disabled={sites.saving || !dirty}>{#if sites.saving}<span class="spin"></span>保存中{:else}保存{/if}</button></div>
{/snippet}

{#if d}
  <div class="st">
    <div class="card" class:editing={editing === 'basic'}>
      {@render head('info', '基本信息', 'basic')}
      {#if editing === 'basic'}
        <div class="grid2">
          <div class="field"><label for="ss-name">站点名称</label><input id="ss-name" class="tin" bind:value={d.name} /></div>
          <div class="field"><label for="ss-domain">主域名</label><input id="ss-domain" class="tin" bind:value={d.domain} placeholder="example.com" spellcheck="false" /><small>用于数据匹配、IndexNow 提交与站点链接。</small></div>
          {#if site.source === 'github'}
            <div class="field"><span class="lbl">GitHub 连接</span><Dropdown bind:value={d.gh_conn_id} options={ghConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name, sub: c.login }))} /></div>
          {:else}
            <div class="field"><label for="ss-branch">默认分支</label><input id="ss-branch" class="tin mono" bind:value={d.default_branch} spellcheck="false" /><small>运营任务的本地分支从这里分出、审核通过后合并回这里。</small></div>
          {/if}
        </div>
        {@render editBar()}
      {:else}
        <dl class="kv">
          <dt>站点名称</dt><dd>{site.name}</dd>
          <dt>主域名</dt><dd>{site.domain || '还没填'}</dd>
          <dt>代码来源</dt><dd class="mono small">{site.source === 'github' ? `GitHub · ${site.repo}` : site.local_dir}</dd>
          {#if site.source === 'github'}<dt>GitHub 连接</dt><dd>{connName(site.gh_conn_id)}</dd>{:else}<dt>默认分支</dt><dd class="mono small">{site.default_branch || 'main'}</dd>{/if}
          <dt>标签</dt>
          <dd>
            <div class="row wrap tags">
              {#each site.tags as t (t)}<span class="pill">#{t}<button class="x mini" title="移除" onclick={() => setTags(site.id, site.tags.filter((x) => x !== t))}>×</button></span>{/each}
              <input class="tin tag-in" bind:value={tagIn} placeholder="+ 加标签，回车" list="ss-tags" onkeydown={(e) => e.key === 'Enter' && addTag()} />
              <datalist id="ss-tags">{#each fleet.tags.filter((t) => !site.tags.includes(t)) as t (t)}<option value={t}></option>{/each}</datalist>
            </div>
          </dd>
        </dl>
      {/if}
    </div>

    <!-- 第一行「站点是什么」：基本信息 ｜ 数据源 -->
    <SiteDataSettings draft={d} {site} editing={editing === 'data'} locked={!!editing && editing !== 'data'} onedit={() => startEdit('data')} oncancel={cancelEdit} onsave={saveEdit} saving={sites.saving} {dirty} />

    <!-- 第二行「怎么上线」：构建 ｜ 部署，下面接服务器（VPS）和配置变量 -->
    <div class="card" class:editing={editing === 'build'}>
      {@render head('hammer', '构建', 'build')}
      {#if editing === 'build'}
        <div class="field" style="margin-bottom:10px"><label for="ss-root">站点子目录</label><input id="ss-root" class="tin mono" bind:value={d.build.root} placeholder="（仓库根目录）" spellcheck="false" />
          <small>站点不在仓库根目录时填，如 <code>site</code>。安装、构建、检查和部署都在这个目录里执行。</small></div>
        <div class="grid3">
          <div class="field"><label for="ss-install">安装依赖</label><input id="ss-install" class="tin mono" bind:value={d.build.install} placeholder="（不需要）" spellcheck="false" /></div>
          <div class="field"><label for="ss-build">构建命令</label><input id="ss-build" class="tin mono" bind:value={d.build.build} placeholder="（纯静态）" spellcheck="false" /></div>
          <div class="field"><label for="ss-out">产物目录</label><input id="ss-out" class="tin mono" bind:value={d.build.out_dir} spellcheck="false" /></div>
        </div>
        <div class="field" style="margin-top:10px"><label for="ss-checks">上线前检查（每行一条）</label>
          <textarea id="ss-checks" class="tin" rows="2" bind:value={checksText} oninput={syncChecks} spellcheck="false"></textarea>
          <small>在构建之后执行；任何一条失败都不会上传，运营任务提交的改动也必须先通过它们。</small>
        </div>
        {@render editBar()}
      {:else}
        <dl class="kv">
          {#if site.build.root}<dt>站点子目录</dt><dd class="mono small">{site.build.root}</dd>{/if}
          <dt>安装依赖</dt><dd class="mono small">{site.build.install || '不需要'}</dd>
          <dt>构建命令</dt><dd class="mono small">{site.build.build || '纯静态，不用构建'}</dd>
          <dt>产物目录</dt><dd class="mono small">{site.build.out_dir}</dd>
          <dt>上线前检查</dt><dd class="mono small">{site.build.checks.length ? site.build.checks.join('；') : '没有'}</dd>
        </dl>
      {/if}
    </div>

    <div class="card" class:editing={editing === 'deploy'}>
      {@render head('rocket', '部署', 'deploy')}
      {#if editing === 'deploy'}
        <div class="row" style="margin-bottom:8px"><button class="btn tiny" onclick={detectDeploy} disabled={detecting} data-tip="按域名和 GitHub 仓库，在已连接的 Cloudflare 账号里找这个站点的 Pages 项目"><Icon name="search" size={11} />{detecting ? '识别中…' : '从 Cloudflare 识别'}</button></div>
        <div class="grid2">
          <div class="field"><span class="lbl">方式</span><Dropdown bind:value={d.deploy.mode} options={modeOpts} /></div>
          {#if d.deploy.mode === 'vps'}
            <div class="field"><span class="lbl">服务器（SSH 连接）</span><Dropdown bind:value={d.deploy.ssh_conn_id} placeholder="选择远程连接" options={app.conns.filter((c) => c.kind === 'ssh').map((c) => ({ value: c.id, label: c.name, sub: `${c.ssh_user}@${c.ssh_host}` }))} /></div>
            <div class="field" style="grid-column:1/-1"><label for="ss-cmd">发布命令</label><input id="ss-cmd" class="tin mono" bind:value={d.deploy.command} placeholder="留空＝由 Pilot 配置服务器并发布" spellcheck="false" /><small>{d.deploy.command.trim() ? '在站点根目录执行。发布脚本自己负责上传、备份、健康检查和失败恢复；Pilot 只在构建和检查通过后才执行它。' : '留空时由 Pilot 托管：Pilot 自己配置服务器（Caddy 自动 HTTPS），每次上线打包发布、原子切换，可以回退。项目自带发布脚本的，在这里填上命令。'}</small></div>
            {#if d.deploy.command.trim()}<div class="field" style="grid-column:1/-1"><label for="ss-envf">服务器上的配置文件</label><input id="ss-envf" class="tin mono" bind:value={d.deploy.env_file} placeholder={`/opt/${(site.domain.split('.')[0] || 'site')}/config/${(site.domain.split('.')[0] || 'site')}.env`} spellcheck="false" /><small>上线时 Pilot 把「配置变量」合并写进这个文件（权限 600，文件里 Pilot 没管的行原样保留），服务或发布脚本从这里读。留空用默认路径。</small></div>{/if}
          {:else if d.deploy.mode !== 'none'}
            <div class="field"><span class="lbl">Cloudflare 连接</span><Dropdown bind:value={d.deploy.cf_conn_id} placeholder="选择连接" options={cfConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name }))} /></div>
            <div class="field"><label for="ss-proj">{d.deploy.mode === 'workers' ? 'Worker 名称' : 'Pages 项目'}</label><input id="ss-proj" class="tin mono" bind:value={d.deploy.project} spellcheck="false" />{#if d.deploy.mode === 'workers'}<small>路由、绑定、变量都按仓库里的 wrangler 配置部署。</small>{/if}</div>
            {#if d.deploy.mode !== 'workers'}<div class="field"><label for="ss-pbranch">生产分支</label><input id="ss-pbranch" class="tin mono" bind:value={d.deploy.branch} spellcheck="false" />{#if d.deploy.mode === 'pages_git' && site.source !== 'github'}<small>上线时 Pilot 把本地的提交推到 GitHub 的这个分支，Cloudflare 拉代码构建；推送用你本机 git 的登录。</small>{/if}</div>{/if}
          {/if}
        </div>
        {@render editBar()}
      {:else}
        <dl class="kv">
          <dt>方式</dt><dd>{MODE_LABEL[site.deploy.mode] ?? site.deploy.mode}</dd>
          {#if site.deploy.mode === 'vps'}
            <dt>服务器</dt><dd>{connName(site.deploy.ssh_conn_id)}</dd>
            <dt>发布命令</dt><dd class="mono small">{site.deploy.command.trim() || '由 Pilot 托管发布'}</dd>
          {:else if site.deploy.mode !== 'none'}
            <dt>Cloudflare 连接</dt><dd>{connName(site.deploy.cf_conn_id)}</dd>
            <dt>{site.deploy.mode === 'workers' ? 'Worker' : 'Pages 项目'}</dt><dd class="mono small">{site.deploy.project || '还没填'}</dd>
            {#if site.deploy.mode !== 'workers'}<dt>生产分支</dt><dd class="mono small">{site.deploy.branch || 'main'}</dd>{/if}
          {/if}
        </dl>
      {/if}
    </div>

    {#if site.deploy.mode === 'vps' && site.deploy.command.trim()}
      <div class="card wide"><h3><Icon name="key" />VPS 部署授权</h3><VpsAuth {site} /></div>
    {:else if site.deploy.mode === 'vps'}
      <div class="wide"><SiteVpsCard {site} /></div>
    {/if}

    <div class="wide"><SiteConfigCard {site} /></div>

    <!-- 第三行「功能」：表单和评论，平时一行。只有用得上的站点才显示（Pilot 上传到 Pages，或 Pilot 托管的服务器）；
         Workers / 绑定 GitHub 的 Pages / 自带发布脚本的服务器用不了，改版中的站点在改版进度里配，都不放这张卡 -->
    {#if site.origin !== 'migrating' && (isPagesDirect(site) || isVpsManaged(site))}
      <div class="wide"><SiteDynamicSettings {site} /></div>
    {/if}

    <div class="card danger-zone wide" class:open={dzOpen}>
      <h3 class="dz-head"><button class="dz-toggle" onclick={() => (dzOpen = !dzOpen)} aria-expanded={dzOpen}><Icon name="info" />危险操作<span class="dz-sub">下线并删除 · 只从 Pilot 移除</span><svg class="chev" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button></h3>
      {#if dzOpen}
      <div class="dz">
        <div class="dz-t"><b><Icon name="trash" size={13} />下线并删除</b><p class="hint">把网站从网上撤下来{site.deploy.project && (site.deploy.mode === 'direct' || site.deploy.mode === 'pages_git') ? `（删除 Cloudflare Pages 项目 ${site.deploy.project}，解除域名指向）` : ''}{site.origin === 'migrated' ? '，改版切换过的域名恢复成上线前的样子' : ''}，再从 Pilot 移除；可以选择把代码文件夹移到废纸篓。不能撤销。</p></div>
        <button class="btn small danger solid" onclick={() => takedownSite(site)}>下线并删除</button>
      </div>
      <div class="dz">
        <div class="dz-t"><b><Icon name="x" size={13} />只从 Pilot 移除</b><p class="hint">网站照常在线，只删除 Pilot 里的站点记录、数据缓存、运营记录和站点会话；代码文件夹与 GitHub 仓库不会被改动。</p></div>
        <button class="btn small danger" onclick={() => removeSite(site)}>只从 Pilot 移除</button>
      </div>
    {/if}
    </div>
  </div>
{/if}

<style>
  /* 按意思分组：「站点是什么」基本信息 ｜ 数据源，「怎么上线」构建 ｜ 部署 + 服务器 + 配置变量，「功能」表单和评论，最后危险操作。
     同一行的两张卡拉成一样高（stretch），配置变量、服务器、表单和评论、危险操作和正在改的卡片占满一行 */
  .st { display: grid; grid-template-columns: repeat(auto-fit, minmax(380px, 1fr)); gap: 14px; align-items: stretch; }
  /* 所有卡片的标签列一样宽：同一列里各张卡的值从同一个位置开始 */
  .st :global(.kv) { grid-template-columns: 120px minmax(0, 1fr); }
  .st :global(.kv > dt) { overflow: hidden; text-overflow: ellipsis; }
  .st > .wide, .st > :global(.card.editing) { grid-column: 1 / -1; }
  .st > .wide > :global(.card) { margin: 0; }
  .st :global(.kv > dt) { white-space: nowrap; }
  .x.mini { width: 16px; height: 16px; font-size: 13px; margin-right: -4px; }
  .small { font-size: 12px; }
  .card.editing { border-color: var(--border2); box-shadow: 0 0 0 3px var(--su-hover, #f3f1ec); }
  .edit-bar { display: flex; align-items: center; gap: 8px; margin-top: 14px; padding-top: 12px; border-top: 1px solid var(--border); }
  .grow { flex: 1; }
  .tags { gap: 6px; align-items: center; }
  .tag-in { max-width: 150px; height: 24px !important; font-size: 12px; }
  /* 危险操作：红色的边和底，和别的设置分开 */
  .danger-zone { display: flex; flex-direction: column; gap: 14px; border-color: var(--err-border, #f0c9c9); background: color-mix(in srgb, var(--err-soft, #fdf1f1) 55%, var(--card, #fff)); }
  .dz-head { color: var(--err); margin-bottom: -2px; }
  .danger-zone:not(.open) { gap: 0; }
  .danger-zone:not(.open) .dz-head { margin-bottom: 0; }
  .dz-toggle { flex: 1; display: flex; align-items: center; gap: 8px; border: 0; background: none; padding: 0; font: inherit; color: inherit; cursor: pointer; text-align: left; }
  .dz-sub { font-size: 12px; font-weight: 400; color: var(--faint); }
  .dz-toggle .chev { margin-left: auto; color: var(--faint); transition: transform .16s; }
  .danger-zone.open .dz-toggle .chev { transform: rotate(180deg); }
  .dz-toggle:hover .chev, .dz-toggle:hover .dz-sub { color: var(--dim); }
  .dz-head :global(svg) { color: var(--err); }
  .dz + .dz { padding-top: 14px; border-top: 1px solid var(--err-border, #f0c9c9); }
  .dz { display: flex; align-items: center; justify-content: space-between; gap: 12px 24px; flex-wrap: wrap; }
  .dz-t { min-width: 0; }
  .dz b { display: inline-flex; align-items: center; gap: 6px; font-size: 13.5px; font-weight: 600; }
  .dz .hint { margin: 4px 0 0; max-width: 62ch; }
  .btn.danger.solid { background: var(--err); border-color: var(--err); color: #fff; }
  .btn.danger.solid:hover:not(:disabled) { background: color-mix(in srgb, var(--err) 88%, #000); border-color: color-mix(in srgb, var(--err) 88%, #000); }
</style>
