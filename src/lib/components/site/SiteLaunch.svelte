<script lang="ts">
  import './site.css';
  import { untrack } from 'svelte';
  import type { Site } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Dropdown from '$lib/Dropdown.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import Icon from './Icon.svelte';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import CfMark from '$lib/components/icons/CfMark.svelte';
  import GoogleMark from '$lib/components/icons/GoogleMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import { app, say } from '$lib/state/app.svelte.js';
  import { invoke } from '@tauri-apps/api/core';
  import { sites, deploySite, pilotDeploys, refreshSites } from '$lib/state/sites.svelte.js';
  import { openDataConnect } from '$lib/state/datasrc.svelte.js';
  import { launch, loadLaunch, launchSteps, toggleLaunchOpen, toGithub, setupPages, setupGsc, setupIndexNow, askAiForGa4, syncConfig, replacePagesDns } from '$lib/state/launch.svelte.js';
  import { ga4ToConfig, gaReady, loadMake } from '$lib/state/siteMake.svelte.js';

  let { site }: { site: Site } = $props();
  let gaBusy = $state(false);
  const hasBing = $derived(app.conns.some((c) => c.kind === 'bing'));
  // Pilot 自己构建、上传的方式：IndexNow 验证文件上线时放进构建产物，不用改代码
  const pilotBuilds = $derived(site.deploy.mode === 'direct' || site.deploy.mode === 'workers');
  // 构建并上线是后台任务：跑完（成功或失败）重新读一遍向导状态，不然「上线一次」一直是灰的
  let lastJob = '';
  $effect(() => {
    const k = job ? `${job.id}:${job.status}` : '';
    if (k === lastJob) return;
    const was = lastJob;
    lastJob = k;
    if (was && job && job.status !== 'running') void loadLaunch(site.id, true);
  });
  // Pages 一步就地配部署：没配部署 / 没填项目时在这里选账号、填项目名
  const cfConns = $derived(app.conns.filter((c) => c.kind === 'cloudflare'));
  const needDeploy = $derived(site.deploy.mode === 'none' || site.deploy.mode === '' || !site.deploy.project || !site.deploy.cf_conn_id);
  let cfConn = $state('');
  let projName = $state('');
  let projFor = '';
  $effect(() => {
    const id = site.id;
    if (projFor === id) return;
    projFor = id;
    untrack(() => {
      cfConn = cfConns.find((c) => c.id === site.deploy.cf_conn_id)?.id || cfConns[0]?.id || '';
      projName = (site.deploy.project || site.domain.split('.')[0] || site.name).toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-+|-+$/g, '');
    });
  });
  // 连接列表晚到、或记着的连接已经删了：落到第一个现有的
  $effect(() => { if (cfConns.length && !cfConns.some((c) => c.id === cfConn)) cfConn = cfConns[0].id; });
  async function createPages() {
    let s2 = site;
    if (needDeploy) {
      try {
        s2 = await invoke<Site>('save_site', { site: { ...site, deploy: { ...site.deploy, mode: 'direct', cf_conn_id: cfConn, project: projName.trim(), branch: site.deploy.branch || 'main' } } });
        await refreshSites();
      } catch (e) { say(String(e), 'err'); return; }
    }
    await setupPages(s2);
  }
  // GA4 是否已接进配置变量要看 .env.example（轻量读一次）
  $effect(() => { void loadMake(site.id, false); });
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
  // 仓库名的默认值只在换站点时填一次（原来「一空就填回去」，删光后立刻被填回，看起来像改不了）
  let repoFor = '';
  $effect(() => {
    const id = site.id;
    if (repoFor === id) return;
    repoFor = id;
    repoName = untrack(() => (site.domain || site.deploy.project || site.name).replace(/[^a-zA-Z0-9._-]+/g, '-'));
  });
  const checking = $derived(!!launch.checking[site.id]);
  const manage = $derived(st?.google_manage_conn ?? '');
  const steps = $derived(st ? launchSteps(site, st) : []);
  const doneN = $derived(steps.filter((x) => x.done).length);
  const allDone = $derived(!!st && doneN === steps.length);
  const next = $derived(steps.find((x) => !x.done));
  const isOpen = $derived(launch.open.has(site.id));
</script>

{#if !st}
  {#if !launch.done.has(site.id)}
    <div class="card launch skel" aria-busy="true">
      <div class="lhead static"><Icon name="checks" /><b>上线向导</b><span class="sub"><span class="spin"></span>检查中…</span></div>
      <div class="bars"><i style="width:62%"></i><i style="width:38%"></i></div>
    </div>
  {/if}
{:else if !allDone}
  <div class="card launch" class:open={isOpen}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="lhead" role="button" tabindex="0" aria-expanded={isOpen} onclick={() => toggleLaunchOpen(site.id)} onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), toggleLaunchOpen(site.id))}>
      <svg class="chev" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
      <Icon name="checks" /><b>上线向导</b>
      <span class="prog"><span class="meter"><i style="width:{(doneN / steps.length) * 100}%"></i></span>{doneN}/{steps.length}</span>
      {#if next && !isOpen}<span class="next">下一步：{next.label}</span>{:else if isOpen}<span class="sub">每一步都由你点了才执行</span>{/if}
      <span class="grow"></span>
      <button class="ic-btn" onclick={(e) => { e.stopPropagation(); void loadLaunch(site.id, true); }} disabled={checking} aria-label="重新检查" data-tip={checking ? '检查中…' : '重新检查：读取最新状态，并按域名到已连接的 Google / Bing 账号里找现成的资源绑上'}><RefreshIcon spinning={checking} /></button>
    </div>
    {#if st.auto_bound?.length}<p class="ok-note">已按域名自动绑定：{st.auto_bound.join('、')}</p>{/if}
    {#if isOpen}
    <div class="lbody">
    {#each st.errors as e (e)}<p class="hint warn-text">{e}</p>{/each}

    <div class="lstep" class:done={st.github}>
      <span class="n">{st.github ? '✓' : 1}</span>
      <div class="lb">
        <b><span class="si"><GhMark size={13} /></span>代码放到 GitHub</b>
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
            <div class="row"><button class="btn tiny" onclick={() => (openGh = true)}><Icon name="sliders" size={11} />设置…</button></div>
          {/if}
        {/if}
        {#if notes.github}<small class:err={notes.github.startsWith('失败')}>{notes.github}</small>{/if}
      </div>
    </div>

    {#if site.deploy.mode !== 'workers' && site.deploy.mode !== 'vps'}
    <div class="lstep" class:done={st.pages && (st.domain_attached || !site.domain)}>
      <span class="n">{st.pages && (st.domain_attached || !site.domain) ? '✓' : 2}</span>
      <div class="lb">
        <b><span class="si"><CfMark size={14} /></span>Cloudflare Pages 项目与域名</b>
        <small>{st.pages ? `项目 ${site.deploy.project}${st.pages_domains.length ? ` · ${st.pages_domains.join('、')}` : ''}` : needDeploy ? '选好 Cloudflare 账号和项目名，创建 Pages 项目' : `将创建项目 ${site.deploy.project}`}{site.domain && !st.domain_attached ? `，并绑定 ${site.domain}（同账号的域名会自动加 CNAME）` : ''}</small>
        {#if !(st.pages && (st.domain_attached || !site.domain))}
          {#if needDeploy}
            <!-- 还没配部署：就在这里选 Cloudflare 账号、填项目名，点一下就配好部署（Pilot 构建后上传到 Pages）并建项目、绑域名 -->
            {#if !cfConns.length}
              <small>还没有 Cloudflare 连接：先在左下角的切换器里「连接 Cloudflare」。要部署到 Workers 或 VPS，到<button class="link-btn" onclick={() => (sites.tab = 'settings')}>站点设置</button>里选。</small>
            {:else}
              <div class="grid2 pg">
                <div class="field"><span class="lbl">Cloudflare 账号</span><Dropdown bind:value={cfConn} options={cfConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name }))} /></div>
                <div class="field"><label for="ln-proj">Pages 项目名</label><input id="ln-proj" class="tin mono" bind:value={projName} spellcheck="false" autocapitalize="off" /></div>
              </div>
              <small>部署方式设为「Pilot 构建后上传到 Pages」；要用 Workers、VPS 或 Pages 绑定 GitHub，到<button class="link-btn" onclick={() => (sites.tab = 'settings')}>站点设置</button>里选。</small>
            {/if}
          {/if}
          <div class="row"><button class="btn tiny" onclick={createPages} disabled={!!busy || (needDeploy ? !cfConn || !projName.trim() : !site.deploy.project)}><Icon name="plus" size={11} />{busy === 'pages' ? '处理中…' : '创建并绑定'}</button>{#if !site.domain}<button class="link-btn" onclick={() => (sites.tab = 'settings')}>先填写域名</button>{/if}</div>
        {/if}
        {#if st.domain_status}<small class="pending-note">{site.domain} 已加到项目，Cloudflare 正在验证（DNS 生效后几分钟内自动完成，可以点右上角刷新看看）。</small>{/if}
        {#if notes.pages}<small class:err={notes.pages.startsWith('失败')}>{notes.pages}</small>{/if}
        {#if launch.dnsConflict[site.id]?.length}<div class="row"><button class="btn tiny" onclick={() => replacePagesDns(site)} disabled={!!busy}><Icon name="refresh" size={11} />替换成指向 Pages…</button></div>{/if}
      </div>
    </div>
    {/if}

    {#if (st.config_vars ?? 0) > 0}
    {@const cfgDone = !(st.config_missing?.length) && !st.config_needs_sync}
    <div class="lstep" class:done={cfgDone}>
      <span class="n">{cfgDone ? '✓' : '·'}</span>
      <div class="lb">
        <b><span class="si"><Icon name="key" size={14} /></span>配置变量</b>
        <small>{#if st.config_missing?.length}还没填：<span class="mono">{st.config_missing.join('、')}</span>（到站点设置的「配置变量」里填，或在「需要你处理」里填）{:else if st.config_needs_sync}值改过，还没同步到{st.config_target}。上线时会自动同步，也可以现在同步。{:else if site.deploy.mode === 'none' || !site.deploy.mode}{st.config_vars} 个变量都好了；还没配置部署，暂时只在 Pilot 构建时使用。{:else}{st.config_vars} 个变量都好了，已同步到{st.config_target}。{/if}</small>
        {#if st.config_missing?.length}
          <div class="row"><button class="btn tiny" onclick={() => (sites.tab = 'settings')}><Icon name="sliders" size={11} />去填写</button></div>
        {:else if st.config_needs_sync && site.deploy.mode !== 'none'}
          <div class="row"><button class="btn tiny" onclick={() => syncConfig(site)} disabled={!!busy}><Icon name="refresh" size={11} />{busy === 'config' ? '同步中…' : '现在同步'}</button></div>
        {/if}
        {#if notes.config}<small class:err={notes.config.startsWith('失败')}>{notes.config}</small>{/if}
      </div>
    </div>
    {/if}

    <div class="lstep" class:done={st.deployed}>
      <span class="n">{st.deployed ? '✓' : 3}</span>
      <div class="lb">
        <b><span class="si"><Icon name="rocket" size={14} /></span>在 Pilot 里上线一次</b>
        <small>{st.deployed ? '已上线过' : site.deploy.mode === 'vps' ? 'Pilot 跑完检查后执行发布脚本，发布到你的服务器。' : site.deploy.mode === 'workers' ? 'Pilot 构建、检查后部署到 Workers。' : 'Pilot 构建、检查后上传到 Pages。'}</small>
        {#if !st.deployed && pilotDeploys(site.deploy.mode)}<div class="row"><button class="btn tiny" onclick={() => deploySite(site.id)} disabled={job?.status === 'running'}><Icon name="rocket" size={11} />构建并上线</button></div>{/if}
      </div>
    </div>

    <div class="lstep" class:done={st.ga4}>
      <span class="n">{st.ga4 ? '✓' : 4}</span>
      <div class="lb">
        <b><span class="si ga"><Icon name="chart" size={14} /></span>GA4</b>
        <small>{st.ga4 ? `已绑定 ${site.data.ga4_property_name || site.data.ga4_property}` : manage ? '用你的 Google 账号新建一个 GA4 媒体资源和网页数据流。' : '自动创建需要授予管理权限的 Google 账号登录；也可以在设置里绑定已有的媒体资源。'}</small>
        <div class="row">
          {#if !st.ga4 && manage}<button class="btn tiny" onclick={async () => { gaBusy = true; try { await ga4ToConfig(site, manage); await loadLaunch(site.id, true); } finally { gaBusy = false; } }} disabled={!!busy || gaBusy || !site.domain} data-tip="新建 GA4 媒体资源和网页数据流，衡量 ID 存进配置变量 PUBLIC_GA_ID（不写进代码），然后重新构建"><Icon name="wand" size={11} />{gaBusy ? '创建中…' : '自动创建并接入'}</button>{/if}
          {#if st.ga4 && !gaReady(site)}<button class="btn tiny" onclick={async () => { gaBusy = true; try { await ga4ToConfig(site); } finally { gaBusy = false; } }} disabled={gaBusy} data-tip="读出已绑定 GA4 的衡量 ID，存进配置变量 PUBLIC_GA_ID"><Icon name="wand" size={11} />{gaBusy ? '读取中…' : '把衡量 ID 接进站点'}</button>{/if}
          {#if !st.ga4 && !manage}<button class="btn tiny" onclick={() => openDataConnect('google', 'oauth', { manage: true })}><GoogleMark size={11} />用 Google 账号登录…</button><button class="link-btn" onclick={() => (sites.tab = 'settings')}>绑定已有的</button>{/if}
          {#if launch.measurement[site.id]}<button class="btn tiny primary" onclick={() => askAiForGa4(site, launch.measurement[site.id])}><Icon name="sparkles" size={11} />让 AI 把统计代码加进站点</button>{/if}
        </div>
        {#if notes.ga4}<small class:err={notes.ga4.startsWith('失败')}>{notes.ga4}</small>{/if}
      </div>
    </div>

    <div class="lstep" class:done={st.gsc}>
      <span class="n">{st.gsc ? '✓' : 5}</span>
      <div class="lb">
        <b><span class="si"><GoogleMark size={13} /></span>Search Console</b>
        <small>{st.gsc ? `已绑定 ${site.data.gsc_site}` : manage ? '在 Cloudflare 写 DNS 验证记录、验证所有权、添加网域资源并提交 sitemap。' : 'Search Console 里已经有这个站点的话，点右上角刷新会按域名自动绑上；自动验证并添加需要授予管理权限的 Google 账号。'}</small>
        {#if !st.gsc}<div class="row">
          {#if manage}<button class="btn tiny" onclick={() => setupGsc(site, manage)} disabled={!!busy || !site.domain}><Icon name="shield" size={11} />{busy === 'gsc' ? '验证中（最多约 1 分半）…' : '自动验证并添加'}</button>
          {:else}<button class="link-btn" onclick={() => (sites.tab = 'settings')}>绑定已有的</button>{/if}
        </div>{/if}
        {#if notes.gsc}<small class:err={notes.gsc.startsWith('失败')}>{notes.gsc}</small>{/if}
      </div>
    </div>

    <div class="lstep" class:done={st.indexnow}>
      <span class="n">{st.indexnow ? '✓' : 6}</span>
      <div class="lb">
        <b><span class="si"><Icon name="zap" size={14} /></span>IndexNow</b>
        <small>{st.indexnow ? (pilotBuilds ? '已自动配好：验证文件随每次上线放进构建产物（不改代码），上线后自动提交内容有变化的页面（Bing、Yandex 等共享）。' : '已配置：每次上线后自动提交内容有变化的页面（Bing、Yandex 等共享）。') : pilotBuilds ? '下次「构建并上线」时自动配好：Pilot 生成密钥、把验证文件放进构建产物，不用改代码。' : '生成验证密钥并写进站点，下次上线后生效。'}</small>
        {#if !st.indexnow}<div class="row"><button class="btn tiny" onclick={() => setupIndexNow(site)} disabled={!!busy || !site.domain}><Icon name="key" size={11} />{busy === 'indexnow' ? '处理中…' : pilotBuilds ? '现在生成密钥' : '生成并写入'}</button></div>{/if}
        {#if notes.indexnow}<small class:err={notes.indexnow.startsWith('失败')}>{notes.indexnow}</small>{/if}
      </div>
    </div>

    <div class="lstep" class:done={st.bing}>
      <span class="n">{st.bing ? '✓' : 7}</span>
      <div class="lb">
        <b><span class="si"><BingMark size={13} /></span>Bing Webmaster<span class="opt">可选</span></b>
        {#if st.bing}
          <small>已绑定 {site.data.bing_site}</small>
        {:else if hasBing}
          <small>Bing 里还没有 {site.domain || '这个站点'}：在 Bing Webmaster 里再「从 Google Search Console 导入」一次（新站点要重新导入），回来点右上角刷新，Pilot 会按域名自动绑上。</small>
          <div class="row"><button class="btn tiny" onclick={() => openUrl('https://www.bing.com/webmasters/')}><Icon name="external" size={11} />打开 Bing Webmaster</button></div>
        {:else}
          <small>只用看 Bing 的搜索数据才需要（收录不受影响：IndexNow 已经会直接通知 Bing）。一次性设置，之后所有站点自动绑定：① 在 Bing Webmaster 里「从 Google Search Console 导入」，GSC 里的站点会一起导进去；② 在 Bing 的设置里生成 API 密钥，连接到 Pilot。</small>
          <div class="row"><button class="btn tiny" onclick={() => openUrl('https://www.bing.com/webmasters/')}><Icon name="external" size={11} />打开 Bing Webmaster</button><button class="btn tiny" onclick={() => openDataConnect('bing')}><BingMark size={11} />连接 Bing</button></div>
        {/if}
      </div>
    </div>
    </div>
    {/if}
  </div>
{/if}

<style>
  .pg { margin: 6px 0 2px; max-width: 560px; }
  .pending-note { color: #8a5a08 !important; }
  .launch { display: flex; flex-direction: column; gap: 2px; padding-top: 12px; padding-bottom: 12px; animation: su-in .2s var(--su-ease) backwards; }
  .lhead { display: flex; align-items: center; gap: 8px; min-height: 28px; margin: -2px -6px; padding: 2px 6px; border-radius: 8px; cursor: pointer; user-select: none; transition: background-color .12s; }
  .lhead:not(.static):hover { background: var(--su-hover); }
  .lhead .grow { flex: 1; }
  .lhead.static { cursor: default; }
  .lhead b { font-size: 13.5px; font-weight: 600; }
  .lhead :global(.ico) { color: var(--dim); }
  .lhead .chev { flex: none; color: var(--faint); transition: transform .16s var(--su-ease); }
  .launch.open .lhead .chev { transform: rotate(90deg); }
  .lhead .sub { font-size: 12px; color: var(--faint); display: inline-flex; align-items: center; gap: 6px; }
  .prog { display: inline-flex; align-items: center; gap: 7px; font-size: 12px; color: var(--dim); font-variant-numeric: tabular-nums; }
  .meter { width: 56px; height: 4px; border-radius: 4px; background: var(--accent-soft); overflow: hidden; }
  .meter i { display: block; height: 100%; border-radius: 4px; background: var(--ok); transition: width .4s var(--su-ease); }
  .next { font-size: 12px; color: var(--dim); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .lbody { margin-top: 8px; animation: su-in .18s var(--su-ease) backwards; }
  .launch > .ok-note { margin-top: 8px; }
  .skel .bars { display: flex; flex-direction: column; gap: 8px; padding: 12px 0 4px 26px; }
  .skel .bars i { display: block; height: 8px; border-radius: 4px; background: linear-gradient(90deg, var(--accent-soft) 25%, #f6f5f1 50%, var(--accent-soft) 75%); background-size: 200% 100%; animation: shimmer 1.2s linear infinite; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel .bars i, .launch, .lbody { animation: none; } }
  .lstep { display: flex; gap: 10px; padding: 9px 0; border-top: 1px solid var(--border); }
  .lbody > .lstep:first-of-type { border-top: none; }
  .lstep .n { width: 20px; height: 20px; border-radius: 50%; flex: none; display: inline-flex; align-items: center; justify-content: center; font-size: 11px; font-weight: 700; background: var(--accent-soft); color: var(--dim); margin-top: 1px; }
  .lstep.done .n { background: var(--ok); color: #fff; }
  .lb { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .lb b { font-size: 13px; display: inline-flex; align-items: center; gap: 7px; }
  .si { display: inline-flex; width: 16px; justify-content: center; color: var(--dim); }
  .lstep.done .si { opacity: .7; }
  .opt { font-size: 11px; font-weight: 500; color: var(--faint); margin-left: 2px; }
  .lb small { color: var(--dim); font-size: 12px; padding-left: 23px; line-height: 1.55; overflow-wrap: anywhere; }
  .lb small.err { color: var(--err); white-space: pre-wrap; }
  .lstep.done b { color: var(--dim); }
  .ic-btn { width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 7px; color: var(--dim); cursor: pointer; transition: background-color .12s, color .12s; }
  .ic-btn:hover:not(:disabled) { background: var(--su-hover); color: var(--text); }
  .ic-btn:disabled { cursor: default; color: var(--faint); }
  .ic-btn :global(svg) { width: 14px; height: 14px; }
  .lb > .row, .lb > .ghform { padding-left: 23px; }
  .lb .link-btn { font-size: 12px; }
  .ghform { display: flex; flex-direction: column; gap: 8px; margin-top: 4px; }
</style>
