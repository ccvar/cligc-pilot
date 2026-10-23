<script lang="ts">
  import './site.css';
  import type { Site } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Dropdown from '$lib/Dropdown.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { sites, deploySite, pilotDeploys } from '$lib/state/sites.svelte.js';
  import { openDataConnect } from '$lib/state/datasrc.svelte.js';
  import { launch, loadLaunch, toGithub, setupPages, createGa4, setupGsc, setupIndexNow, askAiForGa4 } from '$lib/state/launch.svelte.js';

  let { site }: { site: Site } = $props();
  $effect(() => { void loadLaunch(site.id); });
  const st = $derived(launch.status[site.id]);
  const busy = $derived(launch.busy[site.id] ?? '');
  const notes = $derived(launch.notes[site.id] ?? {});
  const ghConns = $derived(app.conns.filter((c) => c.kind === 'github'));
  const job = $derived(sites.jobs[site.id]);
  let ghConn = $state('');
  let repoName = $state('');
  let create = $state(true);
  let priv = $state(true);
  let openGh = $state(false);
  $effect(() => { if (!ghConn && ghConns.length) ghConn = ghConns[0].id; });
  $effect(() => { if (!repoName) repoName = site.deploy.project || site.name.replace(/[^a-zA-Z0-9._-]+/g, '-'); });
  const manage = $derived(st?.google_manage_conn ?? '');
  const allDone = $derived(!!st && st.github && st.pages && (st.domain_attached || !site.domain) && st.deployed && st.ga4 && st.gsc && st.indexnow);
</script>

{#if st && !allDone}
  <div class="card launch">
    <h3>上线向导 <span class="faint" style="font-weight:400;font-size:12px">从零到可运营，每一步都由你点了才执行</span><span class="grow"></span><button class="btn tiny" onclick={() => loadLaunch(site.id)}>刷新</button></h3>
    {#each st.errors as e (e)}<p class="hint warn-text">{e}</p>{/each}

    <div class="lstep" class:done={st.github}>
      <span class="n">{st.github ? '✓' : 1}</span>
      <div class="lb">
        <b>代码放到 GitHub</b>
        <small>{st.github ? (st.repo || '已在 GitHub') : '可选，推荐：之后的运营改动走分支和 PR，回滚也更清楚。'}</small>
        {#if !st.github}
          {#if !ghConns.length}
            <small>还没有 GitHub 连接，先到连接设置里连接一个。</small>
          {:else if openGh}
            <div class="ghform">
              <div class="grid3">
                <div class="field"><span class="lbl">GitHub 连接</span><Dropdown bind:value={ghConn} options={ghConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name, sub: c.login }))} /></div>
                <div class="field"><span class="lbl">方式</span><div class="seg"><button class:on={create} onclick={() => (create = true)}>新建仓库</button><button class:on={!create} onclick={() => (create = false)}>用已有的空仓库</button></div></div>
                <div class="field"><label for="ln-repo">{create ? '仓库名' : 'owner/repo'}</label><input id="ln-repo" class="tin mono" bind:value={repoName} spellcheck="false" /></div>
              </div>
              {#if create}<label class="check"><input type="checkbox" bind:checked={priv} /><span>私有仓库</span></label>{/if}
              <div class="row"><button class="btn small primary" onclick={() => toGithub(site, ghConn, repoName.trim(), create, priv)} disabled={!!busy || !repoName.trim()}>{busy === 'github' ? '推送中…' : '推送并切换为 GitHub 管理'}</button><button class="btn small ghost" onclick={() => (openGh = false)}>取消</button></div>
            </div>
          {:else}
            <div class="row"><button class="btn tiny" onclick={() => (openGh = true)}>设置…</button></div>
          {/if}
        {/if}
        {#if notes.github}<small class:err={notes.github.startsWith('失败')}>{notes.github}</small>{/if}
      </div>
    </div>

    {#if site.deploy.mode !== 'workers' && site.deploy.mode !== 'vps'}
    <div class="lstep" class:done={st.pages && (st.domain_attached || !site.domain)}>
      <span class="n">{st.pages && (st.domain_attached || !site.domain) ? '✓' : 2}</span>
      <div class="lb">
        <b>Cloudflare Pages 项目与域名</b>
        <small>{st.pages ? `项目 ${site.deploy.project}${st.pages_domains.length ? ` · ${st.pages_domains.join('、')}` : ''}` : `将创建项目 ${site.deploy.project || '（先在设置里填写）'}`}{site.domain && !st.domain_attached ? `，并绑定 ${site.domain}（同账号的域名会自动加 CNAME）` : ''}</small>
        {#if !(st.pages && (st.domain_attached || !site.domain))}<div class="row"><button class="btn tiny" onclick={() => setupPages(site)} disabled={!!busy || !site.deploy.project || site.deploy.mode === 'none'}>{busy === 'pages' ? '处理中…' : '创建并绑定'}</button>{#if !site.domain}<button class="link-btn" onclick={() => (sites.tab = 'settings')}>先填写域名</button>{/if}</div>{/if}
        {#if notes.pages}<small class:err={notes.pages.startsWith('失败')}>{notes.pages}</small>{/if}
      </div>
    </div>
    {/if}

    <div class="lstep" class:done={st.deployed}>
      <span class="n">{st.deployed ? '✓' : 3}</span>
      <div class="lb">
        <b>在 Pilot 里上线一次</b>
        <small>{st.deployed ? '已上线过' : site.deploy.mode === 'vps' ? 'Pilot 跑完检查后执行发布脚本，发布到你的服务器。' : site.deploy.mode === 'workers' ? 'Pilot 构建、检查后部署到 Workers。' : 'Pilot 构建、检查后上传到 Pages。'}</small>
        {#if !st.deployed && pilotDeploys(site.deploy.mode)}<div class="row"><button class="btn tiny" onclick={() => deploySite(site.id)} disabled={job?.status === 'running'}>构建并上线</button></div>{/if}
      </div>
    </div>

    <div class="lstep" class:done={st.ga4}>
      <span class="n">{st.ga4 ? '✓' : 4}</span>
      <div class="lb">
        <b>GA4</b>
        <small>{st.ga4 ? `已绑定 ${site.data.ga4_property_name || site.data.ga4_property}` : manage ? '用你的 Google 账号新建一个 GA4 媒体资源和网页数据流。' : '自动创建需要授予管理权限的 Google 账号登录；也可以在设置里绑定已有的媒体资源。'}</small>
        <div class="row">
          {#if !st.ga4 && manage}<button class="btn tiny" onclick={() => createGa4(site, manage)} disabled={!!busy || !site.domain}>{busy === 'ga4' ? '创建中…' : '自动创建'}</button>{/if}
          {#if !st.ga4 && !manage}<button class="btn tiny" onclick={() => openDataConnect('google', 'oauth', { manage: true })}>用 Google 账号登录…</button><button class="link-btn" onclick={() => (sites.tab = 'settings')}>绑定已有的</button>{/if}
          {#if launch.measurement[site.id]}<button class="btn tiny primary" onclick={() => askAiForGa4(site, launch.measurement[site.id])}>让 AI 把统计代码加进站点</button>{/if}
        </div>
        {#if notes.ga4}<small class:err={notes.ga4.startsWith('失败')}>{notes.ga4}</small>{/if}
      </div>
    </div>

    <div class="lstep" class:done={st.gsc}>
      <span class="n">{st.gsc ? '✓' : 5}</span>
      <div class="lb">
        <b>Search Console</b>
        <small>{st.gsc ? `已绑定 ${site.data.gsc_site}` : manage ? '在 Cloudflare 写 DNS 验证记录、验证所有权、添加网域资源并提交 sitemap。' : '自动验证需要授予管理权限的 Google 账号登录；也可以在设置里绑定已有的资源。'}</small>
        {#if !st.gsc}<div class="row">
          {#if manage}<button class="btn tiny" onclick={() => setupGsc(site, manage)} disabled={!!busy || !site.domain}>{busy === 'gsc' ? '验证中（最多约 1 分半）…' : '自动验证并添加'}</button>
          {:else}<button class="link-btn" onclick={() => (sites.tab = 'settings')}>绑定已有的</button>{/if}
        </div>{/if}
        {#if notes.gsc}<small class:err={notes.gsc.startsWith('失败')}>{notes.gsc}</small>{/if}
      </div>
    </div>

    <div class="lstep" class:done={st.indexnow}>
      <span class="n">{st.indexnow ? '✓' : 6}</span>
      <div class="lb">
        <b>IndexNow</b>
        <small>{st.indexnow ? '已配置：每次上线后自动提交内容有变化的页面（Bing、Yandex 等共享）。' : '生成验证密钥并写进站点，下次上线后生效。'}</small>
        {#if !st.indexnow}<div class="row"><button class="btn tiny" onclick={() => setupIndexNow(site)} disabled={!!busy || !site.domain}>{busy === 'indexnow' ? '处理中…' : '生成并写入'}</button></div>{/if}
        {#if notes.indexnow}<small class:err={notes.indexnow.startsWith('失败')}>{notes.indexnow}</small>{/if}
      </div>
    </div>

    <div class="lstep" class:done={st.bing}>
      <span class="n">{st.bing ? '✓' : 7}</span>
      <div class="lb">
        <b>Bing Webmaster（可选）</b>
        <small>{st.bing ? `已绑定 ${site.data.bing_site}` : '在 Bing Webmaster 里「从 Google Search Console 导入」最省事；导入后连接 Bing 并在设置里绑定。'}</small>
        {#if !st.bing}<div class="row"><button class="btn tiny" onclick={() => openUrl('https://www.bing.com/webmasters/')}>打开 Bing Webmaster ↗</button><button class="link-btn" onclick={() => (sites.tab = 'settings')}>去绑定</button></div>{/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .launch { display: flex; flex-direction: column; gap: 2px; }
  .lstep { display: flex; gap: 10px; padding: 9px 0; border-top: 1px solid var(--border); }
  .lstep:first-of-type { border-top: none; }
  .lstep .n { width: 20px; height: 20px; border-radius: 50%; flex: none; display: inline-flex; align-items: center; justify-content: center; font-size: 11px; font-weight: 700; background: var(--accent-soft); color: var(--dim); margin-top: 1px; }
  .lstep.done .n { background: var(--ok); color: #fff; }
  .lb { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .lb b { font-size: 13px; }
  .lb small { color: var(--dim); font-size: 12px; line-height: 1.55; overflow-wrap: anywhere; }
  .lb small.err { color: var(--err); white-space: pre-wrap; }
  .lstep.done b { color: var(--dim); }
  .ghform { display: flex; flex-direction: column; gap: 8px; margin-top: 4px; }
</style>
