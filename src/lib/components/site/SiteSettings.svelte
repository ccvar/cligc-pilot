<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import Dropdown from '$lib/Dropdown.svelte';
  import SiteDataSettings from './SiteDataSettings.svelte';
  import VpsAuth from './VpsAuth.svelte';
  import SiteConfigCard from './SiteConfigCard.svelte';
  import SiteDynamicSettings from './SiteDynamicSettings.svelte';
  import { app, say } from '$lib/state/app.svelte.js';
  import { invoke } from '@tauri-apps/api/core';
  import { sites, editSite, saveSiteDraft, removeSite } from '$lib/state/sites.svelte.js';
  import { setTags, fleet } from '$lib/state/fleet.svelte.js';

  let { site }: { site: Site } = $props();
  $effect(() => { if (!sites.draft || sites.draft.id !== site.id) editSite(site); });
  const d = $derived(sites.draft && sites.draft.id === site.id ? sites.draft : null);
  const ghConns = $derived(app.conns.filter((c) => c.kind === 'github'));
  const cfConns = $derived(app.conns.filter((c) => c.kind === 'cloudflare'));
  const dirty = $derived(!!d && JSON.stringify({ ...d, updated_at: 0 }) !== JSON.stringify({ ...site, updated_at: 0 }));
  let checksText = $state('');
  $effect(() => { if (d) checksText = d.build.checks.join('\n'); });
  function syncChecks() { if (d) d.build.checks = checksText.split('\n').map((c) => c.trim()).filter(Boolean); }
  let tagIn = $state('');
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
  const modeOpts = $derived([
    { value: 'direct', label: 'Pilot 构建后上传到 Pages' },
    { value: 'workers', label: 'Pilot 构建后部署到 Workers（wrangler deploy）' },
    { value: 'vps', label: 'VPS（项目自带的发布脚本）' },
    { value: 'pages_git', label: site.source === 'github' ? 'Pages 绑定 GitHub（合并即上线）' : 'Pages 绑定 GitHub（推送到 GitHub 即上线）' },
    { value: 'none', label: '暂不部署' },
  ]);
</script>

{#if d}
  <div class="st">
    <div class="card">
      <h3><Icon name="info" />基本信息</h3>
      <div class="grid2">
        <div class="field"><label for="ss-name">站点名称</label><input id="ss-name" class="tin" bind:value={d.name} /></div>
        <div class="field"><label for="ss-domain">主域名</label><input id="ss-domain" class="tin" bind:value={d.domain} placeholder="example.com" spellcheck="false" /><small>用于数据匹配、IndexNow 提交与站点链接。</small></div>
      </div>
      <div class="field" style="margin-top:10px"><span class="lbl">标签</span>
        <div class="row wrap">
          {#each site.tags as t (t)}<span class="pill">#{t}<button class="x mini" title="移除" onclick={() => setTags(site.id, site.tags.filter((x) => x !== t))}>×</button></span>{/each}
          <input class="tin" style="max-width:160px" bind:value={tagIn} placeholder="加标签，回车" list="ss-tags" onkeydown={(e) => e.key === 'Enter' && addTag()} />
          <datalist id="ss-tags">{#each fleet.tags.filter((t) => !site.tags.includes(t)) as t (t)}<option value={t}></option>{/each}</datalist>
        </div>
        <small>站点多的时候用标签分组、筛选（比如按语言、市场、业务线）。标签即时保存。</small>
      </div>
      <div class="grid2" style="margin-top:10px">
        <div class="field"><span class="lbl">代码来源</span><div class="mono dim" style="font-size:12px;padding-top:4px;overflow-wrap:anywhere">{site.source === 'github' ? `GitHub · ${site.repo}` : site.local_dir}</div><small>导入后不能修改；换来源请移除后重新导入。</small></div>
        {#if site.source === 'github'}
          <div class="field"><span class="lbl">GitHub 连接</span><Dropdown bind:value={d.gh_conn_id} options={ghConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name, sub: c.login }))} /></div>
        {:else}
          <div class="field"><label for="ss-branch">默认分支</label><input id="ss-branch" class="tin mono" bind:value={d.default_branch} spellcheck="false" /><small>运营任务的本地分支从这里分出、审核通过后合并回这里。</small></div>
        {/if}
      </div>
    </div>

    <div class="card">
      <h3><Icon name="hammer" />构建</h3>
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
    </div>

    <SiteConfigCard {site} />

    <div class="card">
      <h3><Icon name="rocket" />部署<span class="grow"></span><button class="btn tiny" onclick={detectDeploy} disabled={detecting} data-tip="按域名和 GitHub 仓库，在已连接的 Cloudflare 账号里找这个站点的 Pages 项目"><Icon name="search" size={11} />{detecting ? '识别中…' : '从 Cloudflare 识别'}</button></h3>
      <div class="grid2">
        <div class="field"><span class="lbl">方式</span><Dropdown bind:value={d.deploy.mode} options={modeOpts} /></div>
        {#if d.deploy.mode === 'vps'}
          <div class="field"><span class="lbl">服务器（SSH 连接）</span><Dropdown bind:value={d.deploy.ssh_conn_id} placeholder="选择远程连接" options={app.conns.filter((c) => c.kind === 'ssh').map((c) => ({ value: c.id, label: c.name, sub: `${c.ssh_user}@${c.ssh_host}` }))} /></div>
          <div class="field" style="grid-column:1/-1"><label for="ss-cmd">发布命令</label><input id="ss-cmd" class="tin mono" bind:value={d.deploy.command} spellcheck="false" /><small>在站点根目录执行。发布脚本自己负责上传、备份、健康检查和失败恢复；Pilot 只在构建和检查通过后才执行它。</small></div>
          <div class="field" style="grid-column:1/-1"><label for="ss-envf">服务器上的配置文件</label><input id="ss-envf" class="tin mono" bind:value={d.deploy.env_file} placeholder={`/opt/${(site.domain.split('.')[0] || 'site')}/config/${(site.domain.split('.')[0] || 'site')}.env`} spellcheck="false" /><small>上线时 Pilot 把「配置变量」合并写进这个文件（权限 600，文件里 Pilot 没管的行原样保留），服务或发布脚本从这里读。留空用默认路径。</small></div>
        {:else if d.deploy.mode !== 'none'}
          <div class="field"><span class="lbl">Cloudflare 连接</span><Dropdown bind:value={d.deploy.cf_conn_id} placeholder="选择连接" options={cfConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name }))} /></div>
          <div class="field"><label for="ss-proj">{d.deploy.mode === 'workers' ? 'Worker 名称' : 'Pages 项目'}</label><input id="ss-proj" class="tin mono" bind:value={d.deploy.project} spellcheck="false" />{#if d.deploy.mode === 'workers'}<small>路由、绑定、变量都按仓库里的 wrangler 配置部署。</small>{/if}</div>
          {#if d.deploy.mode !== 'workers'}<div class="field"><label for="ss-pbranch">生产分支</label><input id="ss-pbranch" class="tin mono" bind:value={d.deploy.branch} spellcheck="false" />{#if d.deploy.mode === 'pages_git' && site.source !== 'github'}<small>上线时 Pilot 把本地的提交推到 GitHub 的这个分支，Cloudflare 拉代码构建；推送用你本机 git 的登录。</small>{/if}</div>{/if}
        {/if}
      </div>
    </div>

    {#if site.deploy.mode === 'vps'}
      <div class="card"><h3><Icon name="key" />VPS 部署授权</h3><VpsAuth {site} /></div>
    {/if}

    <SiteDataSettings draft={d} {site} />

    <SiteDynamicSettings {site} />

    {#if dirty}
      <div class="savebar">
        <span>有未保存的修改</span><span class="grow"></span>
        <button class="btn small ghost" onclick={() => editSite(site)} disabled={sites.saving}>撤销</button>
        <button class="btn small primary" onclick={saveSiteDraft} disabled={sites.saving}>{#if sites.saving}<span class="spin"></span>保存中{:else}保存设置{/if}</button>
      </div>
    {/if}

    <div class="card danger-zone">
      <div class="dz">
        <div><b><Icon name="trash" />移除站点</b><p class="hint">只删除 Pilot 里的站点记录、数据缓存、运营记录和站点会话；代码文件夹与 GitHub 仓库不会被改动。</p></div>
        <button class="btn small danger" onclick={() => removeSite(site)}>移除这个站点</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .st { display: flex; flex-direction: column; gap: 14px; }
  .x.mini { width: 16px; height: 16px; font-size: 13px; margin-right: -4px; }
  .dz { display: flex; align-items: center; justify-content: space-between; gap: 12px 24px; flex-wrap: wrap; }
  .dz b { font-size: 13.5px; font-weight: 600; }
  .dz .hint { margin: 2px 0 0; max-width: 60ch; }
</style>
