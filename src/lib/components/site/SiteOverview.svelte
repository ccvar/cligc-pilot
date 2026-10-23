<script lang="ts">
  import './site.css';
  import type { Site } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import SiteSummary from './SiteSummary.svelte';
  import SiteLaunch from './SiteLaunch.svelte';
  import { clock } from './fmt';
  import { sites, deploySite, pilotDeploys, DEPLOY_LABEL } from '$lib/state/sites.svelte.js';
  import { ops, LEVEL_LABEL } from '$lib/state/ops.svelte.js';
  import { app } from '$lib/state/app.svelte.js';

  let { site }: { site: Site } = $props();
  const st = $derived(sites.status[site.id]);
  const job = $derived(sites.jobs[site.id]);
  const last = $derived(st?.last_deploy);
  const cf = $derived(app.conns.find((c) => c.id === site.deploy.cf_conn_id));
  const hasData = $derived(!!(site.data.ga4_property || site.data.gsc_site || site.data.bing_site));
  // 家目录缩写成 ~，路径太长时在中间省略交给 CSS
  const shortPath = (p: string) => p.replace(/^\/Users\/[^/]+/, '~').replace(/^\/home\/[^/]+/, '~');
  const queue = $derived((ops.queue[site.id] ?? []).filter((q) => q.status === 'open' || q.status === 'proposed'));
</script>

<div class="ov">
  <SiteLaunch {site} />

  {#if job && (job.status === 'running' || job.finished_at > Date.now() / 1000 - 600)}
    <div class="card"><SiteJobPanel {job} /></div>
  {/if}

  {#if hasData}<SiteSummary {site} />{/if}

  <div class="cards">
    <div class="card">
      <h3>代码</h3>
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
      <h3>部署 <span class="grow"></span>{#if pilotDeploys(site.deploy.mode)}<button class="btn tiny" onclick={() => deploySite(site.id)} disabled={job?.status === 'running'}>构建并上线</button>{/if}</h3>
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
      </dl>
    </div>
    <div class="card">
      <h3>运营 <span class="grow"></span><button class="btn tiny" onclick={() => (sites.tab = 'ops')}>设置</button></h3>
      <dl class="kv">
        <dt>自动化档位</dt><dd>{LEVEL_LABEL[site.ops.level] ?? site.ops.level}</dd>
        <dt>待处理</dt><dd>{#if queue.length}<button class="link-btn" onclick={() => (sites.tab = 'queue')}>{queue.length} 条</button>{:else}暂无{/if}</dd>
        <dt>数据源</dt><dd>{[site.data.ga4_property && 'GA4', site.data.gsc_site && 'Search Console', site.data.bing_site && 'Bing'].filter(Boolean).join(' · ') || '未接入'}</dd>
      </dl>
    </div>
  </div>
</div>

<style>
  .ov { display: flex; flex-direction: column; gap: 14px; }
  .cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 14px; }
  .small { font-size: 11.5px; }
  .one { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
