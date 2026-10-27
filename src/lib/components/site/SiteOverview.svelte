<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import SiteSummary from './SiteSummary.svelte';
  import SiteLinks from './SiteLinks.svelte';
  import SiteMonitor from './SiteMonitor.svelte';
  import SiteLaunch from './SiteLaunch.svelte';
  import SiteManagedCard from './SiteManagedCard.svelte';
  import SiteMakeCard from './SiteMakeCard.svelte';
  import SiteMigrate from './SiteMigrate.svelte';
  import SiteInbox from './SiteInbox.svelte';
  import { dyn, loadDynamic, inboxReady } from '$lib/state/dynamic.svelte.js';
  import { showMigrateCard } from '$lib/state/migrate.svelte.js';
  import SiteTodos from './SiteTodos.svelte';
  import SiteDomainLive from './SiteDomainLive.svelte';
  import { launch } from '$lib/state/launch.svelte.js';
  import { isMaking } from '$lib/state/siteCreate.svelte.js';
  import { siteMake, loadMake, writeConventions, hideConventionsHint } from '$lib/state/siteMake.svelte.js';
  import { clock } from './fmt';
  import { sites, deploySite, publishFiles, pilotDeploys, DEPLOY_LABEL } from '$lib/state/sites.svelte.js';
  import { ops } from "$lib/state/ops.svelte.js";
  import { app } from '$lib/state/app.svelte.js';

  let { site }: { site: Site } = $props();
  const st = $derived(sites.status[site.id]);
  const job = $derived(sites.jobs[site.id]);
  const last = $derived(st?.last_deploy);
  const cf = $derived(app.conns.find((c) => c.id === site.deploy.cf_conn_id));
  const hasData = $derived(!!(site.data.ga4_property || site.data.gsc_site || site.data.bing_site));
  // 家目录缩写成 ~，路径太长时在中间省略交给 CSS
  const shortPath = (p: string) => p.replace(/^\/Users\/[^/]+/, '~').replace(/^\/home\/[^/]+/, '~');
  const making = $derived(isMaking(site.id));
  $effect(() => { void loadDynamic(site.id); });
  // 老站点：没有站点约定就提示补一份（托管运营和 AI 加内容都按它来）
  $effect(() => { if (!isMaking(site.id)) void loadMake(site.id, false); });
  // 正式域名已经定了（填了域名、或者迁移上线过）：AI 留的「确认正式域名」不用再显示
  const todos = $derived((siteMake.state[site.id]?.todos ?? []).filter((t) => !(t.id === 'domain' && site.domain)));
  const needConventions = $derived(!making && siteMake.state[site.id]?.conventions === false && !siteMake.hidden.includes(site.id));
  // 数据和收录接到了哪些：一眼看清哪些绑好了、哪些还差（细节在设置里）
  const binds = $derived([
    { k: 'GA4', ok: !!site.data.ga4_property, text: site.data.ga4_property ? (site.data.ga4_property_name || site.data.ga4_property) : '未绑定' },
    { k: 'Search Console', ok: !!site.data.gsc_site, text: site.data.gsc_site || '未绑定' },
    { k: 'Bing', ok: !!site.data.bing_site, pending: !site.data.bing_site && !!site.data.bing_auth, text: site.data.bing_site || (site.data.bing_auth ? '已加到 Bing，正在验证' : '未接入') },
    { k: 'IndexNow', ok: !!site.data.indexnow_key, text: site.data.indexnow_key ? '上线后自动提交有变化的页面' : '未配置' },
  ]);
  const queue = $derived((ops.queue[site.id] ?? []).filter((q) => q.status === 'open' || q.status === 'proposed'));
</script>

<div class="ov">
  {#if showMigrateCard(site)}<SiteMigrate {site} />{/if}
  {#if making && site.origin !== 'migrating'}<SiteMakeCard {site} />{/if}
  <!-- 迁移中不显示上线向导：预览部署在迁移卡片里配（只建 Pages 项目、不绑域名），域名到切换时才指过来 -->
  {#if site.origin !== 'migrating'}<SiteLaunch {site} />{/if}

  <!-- 迁移卡片在的时候（迁移中、上线后观察期）：任务进度显示在迁移卡片当前那一步里，不再另起一张；托管、数据这些到正式上线以后才有意义，先不显示 -->
  {#if !showMigrateCard(site) && job && (job.status === 'running' || job.finished_at > Date.now() / 1000 - 600)}
    <div class="card"><SiteJobPanel {job} /></div>
  {/if}

  {#if inboxReady(dyn.conf[site.id])}<SiteInbox {site} />{/if}

  {#if site.origin !== 'migrating'}
  <SiteManagedCard {site} />

  {#if !making && todos.length}
    <div class="card"><h3><Icon name="inbox" />需要你处理</h3><SiteTodos {site} {todos} /></div>
  {/if}

  {#if needConventions}
    <div class="card conv-hint">
      <Icon name="doc" />
      <div class="ch-main"><b>补一份站点约定</b><small>写清内容放哪、页面格式、新增一页要改哪些地方（<code>.pilot/site.md</code>），托管运营和 AI 加内容会照着来，更稳。</small></div>
      <button class="btn tiny" onclick={() => writeConventions(site)}><Icon name="wand" size={11} />让 AI 写一份</button>
      <button class="link-btn" onclick={() => hideConventionsHint(site.id)}>以后再说</button>
    </div>
  {/if}

  {#if hasData}<SiteSummary {site} {binds} />{/if}

  {#if site.domain}<div class="cards"><SiteMonitor {site} /><SiteLinks {site} /></div>{/if}

  <!-- 有数据时数据源并进「近 28 天」的标题行；还没有数据时单独一张卡，方便去接入 -->
  {#if !hasData}
  <div class="card">
    <h3><Icon name="chart" />数据与收录 <span class="grow"></span><button class="ic-btn ic-sm" onclick={() => (sites.tab = 'settings')} aria-label="数据与收录设置" data-tip="在设置里接入 GA4、Search Console、Bing"><Icon name="sliders" size={14} /></button></h3>
    <div class="binds">
      {#each binds as b (b.k)}
        <div class="bind">
          <div class="bh"><span class="bk">{b.k}</span><span class="pill {b.ok ? 'ok' : b.pending ? 'warn' : ''}">{b.ok ? '已接入' : b.pending ? '验证中' : '未接入'}</span></div>
          <small class="bt" title={b.text}>{b.text}</small>
        </div>
      {/each}
    </div>
  </div>
  {/if}

  <div class="cards">
    <div class="card">
      <h3><Icon name="code" />代码</h3>
      <dl class="kv">
        <dt>来源</dt><dd>{site.source === 'github' ? `GitHub · ${site.repo}` : '本地文件夹'}</dd>
        <dt>位置</dt><dd class="mono small one" title={st?.code_dir}>{st?.code_dir ? shortPath(st.code_dir) : '—'}{st && !st.exists ? '（尚未克隆，首次对话或上线时自动克隆）' : ''}</dd>
        {#if st?.git}
          <dt>分支</dt><dd>{st.branch || '（分离 HEAD）'}{#if st.dirty} <span class="pill warn">有未提交改动</span>{/if}</dd>
          <dt>最新提交</dt><dd><span class="mono small faint">{st.head.slice(0, 7)}</span> {st.head_subject}</dd>
        {/if}
        <dt>构建</dt><dd class="mono small">{site.build.build || '（纯静态）'} → {site.build.out_dir}</dd>
      </dl>
    </div>
    <div class="card">
      <h3><Icon name="rocket" />部署 <span class="grow"></span>{#if pilotDeploys(site.deploy.mode)}<button class="btn tiny" onclick={() => deploySite(site.id)} disabled={job?.status === 'running'}><Icon name="rocket" size={12} />构建并上线</button>{/if}</h3>
      <dl class="kv">
        <dt>方式</dt><dd>{DEPLOY_LABEL[site.deploy.mode] ?? '暂不部署'}</dd>
        {#if site.deploy.mode !== 'none'}
          {#if site.deploy.mode === 'vps'}
            <dt>服务器</dt><dd>{app.conns.find((c) => c.id === site.deploy.ssh_conn_id)?.name ?? '未选择连接'}</dd>
            <dt>发布命令</dt><dd class="mono small">{site.deploy.command}</dd>
          {:else}
          <dt>{site.deploy.mode === 'workers' ? 'Worker' : 'Pages'}</dt><dd>{site.deploy.project || '—'} · {cf ? cf.remark?.trim() || cf.name : '未选择连接'}</dd>
          {/if}
          {#if site.deploy.mode !== 'workers' && site.deploy.mode !== 'vps'}<dt>生产分支</dt><dd class="mono">{site.deploy.branch}</dd>{/if}
        {/if}
        {#if last}
          <dt>上次上线</dt>
          <dd>
            <span class="pill {last.ok ? 'ok' : 'err'}">{last.ok ? '成功' : '失败'}</span> {clock(last.ts)}
            {#if last.url}<button class="link-btn" onclick={() => openUrl(last.url)}>预览地址</button>{/if}
            {#if last.ok && last.changed}<div class="faint">{last.changed} 个页面有变化</div>{/if}
          </dd>
        {/if}
        {#if last?.ok && site.domain && !['none', 'workers', 'vps'].includes(site.deploy.mode)}<SiteDomainLive {site} deployedAt={last.ts} />{/if}
        {#if launch.status[site.id]?.needs_redeploy?.length && job?.status !== 'running'}
          <dt>待生效</dt>
          <dd class="redeploy"><span>{launch.status[site.id]?.needs_redeploy?.join('、')}还没在线上。Pilot 会自动拿上次上线的构建产物补上再传一次（不重新构建，页面不变）；自动那次没成功的话，也可以现在补上。</span><button class="btn tiny" onclick={() => publishFiles(site.id)}><Icon name="rocket" size={11} />现在补上</button></dd>
        {/if}
      </dl>
    </div>
  </div>
  {/if}
</div>

<style>
  .conv-hint { display: flex; align-items: center; gap: 12px; padding: 12px 16px; }
  .conv-hint > :global(svg) { flex: none; color: var(--dim); }
  .ch-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .ch-main b { font-size: 13px; font-weight: 600; }
  .ch-main small { font-size: 12px; color: var(--faint); line-height: 1.5; }
  .conv-hint .link-btn { flex: none; font-size: 12px; color: var(--faint); }
  .conv-hint .link-btn:hover { color: var(--text); }
  .ov { display: flex; flex-direction: column; gap: 14px; }
  .cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 14px; }
  .small { font-size: 11.5px; }
  .binds { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px 24px; }
  .redeploy { display: flex; align-items: flex-start; gap: 10px; }
  .redeploy span { flex: 1; min-width: 0; color: var(--su-warn-ink, #8a5a08); }
  .redeploy .btn { flex: none; }
  .bind { min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .bh { display: flex; align-items: center; gap: 8px; }
  .bk { font-size: 12.5px; font-weight: 600; color: var(--text); }
  .bt { font-size: 12px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .one { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
