<script lang="ts">
  import './site.css';
  import type { Site } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import { sites, deploySite, checkSite, loadDeploys, loadPagesDeploys, retryPagesDeploy, rollbackPagesDeploy } from '$lib/state/sites.svelte.js';

  let { site }: { site: Site } = $props();
  const job = $derived(sites.jobs[site.id]);
  const history = $derived(sites.deploys[site.id] ?? []);
  const pages = $derived(sites.pagesDeploys[site.id] ?? []);
  const when = (ts: number) => new Date(ts * 1000).toLocaleString();
  const trig = (t: string) => (t === 'ops' ? '运营任务' : t === 'rollback' ? '回滚' : '手动');
  $effect(() => { void loadDeploys(site.id); if (site.deploy.mode !== 'none' && site.deploy.project) void loadPagesDeploys(site.id); });
</script>

<div class="dp">
  {#if site.deploy.mode === 'none'}
    <div class="card"><p class="hint">这个站点还没有配置部署。<button class="link-btn" onclick={() => (sites.tab = 'settings')}>去设置</button></p></div>
  {:else}
    {#if site.deploy.mode === 'direct' || site.deploy.mode === 'workers' || site.deploy.mode === 'vps'}
      <div class="card">
        <h3>构建并上线 <span class="grow"></span><button class="btn small" onclick={() => checkSite(site.id)} disabled={job?.status === 'running'} data-tip="只安装、构建并跑检查，不上传">预检</button><button class="btn small primary" onclick={() => deploySite(site.id)} disabled={job?.status === 'running'}>{job?.status === 'running' ? '进行中…' : '构建并上线'}</button></h3>
        <p class="hint">
          Pilot 依次执行：{site.source === 'github' ? `拉取 ${site.deploy.branch} 最新代码 → ` : ''}安装依赖 → <code>{site.build.build || '（无构建）'}</code> →
          {site.build.checks.length ? site.build.checks.map((c) => c).join(' → ') + ' →' : ''} {#if site.deploy.mode === 'vps'}执行发布命令 <code>{site.deploy.command}</code>（SSH 用 Pilot 部署密钥）{:else if site.deploy.mode === 'workers'}<code>wrangler deploy</code> 到 Worker <code>{site.deploy.project}</code>（按仓库里的 wrangler 配置）{:else}上传 <code>{site.build.out_dir}</code> 到 Pages 项目 <code>{site.deploy.project}</code>{/if}
          {site.data.indexnow_key ? '→ 向 IndexNow 提交内容有变化的页面' : ''}。任何一步失败都会停下，不会上传。
        </p>
        {#if job}<SiteJobPanel {job} />{/if}
      </div>
    {:else}
      <div class="card">
        <h3>由 Cloudflare 自动构建</h3>
        <p class="hint">这个 Pages 项目绑定了 GitHub：合并到 <code>{site.deploy.branch}</code> 后 Cloudflare 会自动构建上线。可以在下方对某次部署重新构建，或把线上版本切回历史部署。</p>
      </div>
    {/if}

    {#if site.deploy.mode !== 'vps'}
    <div class="card">
      <h3>{site.deploy.mode === 'workers' ? 'Workers' : 'Cloudflare Pages'} 部署记录 <span class="grow"></span><button class="btn tiny" onclick={() => loadPagesDeploys(site.id)}>刷新</button></h3>
      {#if sites.pagesErr[site.id]}<div class="err-note">{sites.pagesErr[site.id]}</div>
      {:else if !pages.length}<p class="hint">还没有部署记录（项目尚未创建，或首次上线前）。</p>
      {:else}
        <table class="tbl">
          <thead><tr><th>时间</th><th>环境</th><th>提交</th><th>状态</th><th></th></tr></thead>
          <tbody>
            {#each pages as p (p.id)}
              <tr>
                <td>{new Date(p.created_on).toLocaleString()}</td>
                <td>{p.environment === 'production' ? '生产' : '预览'}{p.branch ? ` · ${p.branch}` : ''}</td>
                <td class="trunc" title={p.message}><span class="mono">{p.commit.slice(0, 7)}</span> {p.message}</td>
                <td><span class="pill {p.status === 'success' ? 'ok' : p.status === 'failure' ? 'err' : 'run'}">{p.stage} · {p.status}</span></td>
                <td class="num">
                  {#if p.url}<button class="btn tiny" onclick={() => openUrl(p.url)}>打开</button>{/if}
                  {#if site.deploy.mode === 'pages_git'}<button class="btn tiny" onclick={() => retryPagesDeploy(site.id, p.id)}>重新构建</button>{/if}
                  {#if site.deploy.mode !== 'workers' && p.environment === 'production' && p.status === 'success'}<button class="btn tiny" onclick={() => rollbackPagesDeploy(site.id, p.id)}>切回此版本</button>{/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
    {/if}
  {/if}

  {#if history.length}
    <div class="card">
      <h3>Pilot 上线记录</h3>
      <table class="tbl">
        <thead><tr><th>时间</th><th>触发</th><th>提交</th><th class="num">变化页面</th><th>IndexNow</th><th>结果</th></tr></thead>
        <tbody>
          {#each history as r (r.id)}
            <tr>
              <td>{when(r.ts)}</td>
              <td>{trig(r.trigger)}</td>
              <td class="trunc" title={r.message}><span class="mono">{r.commit.slice(0, 7) || '—'}</span>{r.dirty ? '（含未提交改动）' : ''} {r.message}</td>
              <td class="num">{r.ok ? r.changed : '—'}</td>
              <td>{r.indexnow === 'ok' ? '已提交' : r.indexnow === 'nothing' ? '无变化' : r.indexnow === 'skipped' ? '未配置' : r.indexnow ? '失败' : '—'}</td>
              <td>{#if r.ok}<span class="pill ok">成功</span>{:else}<span class="pill err" title={r.error}>失败</span>{/if} <span class="faint">{r.duration}s</span></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .dp { display: flex; flex-direction: column; gap: 14px; }
  .dp .card :global(.job) { margin-top: 10px; }
</style>
