<script lang="ts">
  import './site.css';
  import type { Site } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import { clock, clockIso } from './fmt';
  import { sites, deploySite, checkSite, loadDeploys, loadPagesDeploys, retryPagesDeploy, rollbackPagesDeploy } from '$lib/state/sites.svelte.js';

  let { site }: { site: Site } = $props();
  const job = $derived(sites.jobs[site.id]);
  const history = $derived(sites.deploys[site.id] ?? []);
  const pages = $derived(sites.pagesDeploys[site.id] ?? []);
  const running = $derived(job?.status === 'running');
  // 流水线每一步：命令用等宽，其余是说明文字
  const chain = $derived([
    ...(site.source === 'github' ? [{ t: `拉取 ${site.deploy.branch}`, code: false }] : []),
    { t: site.build.install || '安装依赖', code: !!site.build.install },
    ...(site.build.build ? [{ t: site.build.build, code: true }] : []),
    ...site.build.checks.map((c) => ({ t: c, code: true })),
    site.deploy.mode === 'vps' ? { t: site.deploy.command || '发布命令', code: !!site.deploy.command }
      : site.deploy.mode === 'workers' ? { t: `wrangler deploy → ${site.deploy.project}`, code: true }
      : { t: `上传 ${site.build.out_dir} → Pages ${site.deploy.project}`, code: false },
    ...(site.data.indexnow_key ? [{ t: '提交 IndexNow', code: false }] : []),
  ]);
  const pagesStatus = (s: string) => (s === 'success' ? '已上线' : s === 'failure' ? '失败' : s === 'canceled' ? '已取消' : '进行中');
  const trig = (t: string) => (t === 'ops' ? '运营任务' : t === 'rollback' ? '回滚' : '手动');
  $effect(() => { void loadDeploys(site.id); if (site.deploy.mode !== 'none' && site.deploy.project) void loadPagesDeploys(site.id); });
</script>

<div class="dp">
  {#if site.deploy.mode === 'none'}
    <div class="card"><p class="hint">这个站点还没有配置部署。<button class="link-btn" onclick={() => (sites.tab = 'settings')}>去设置</button></p></div>
  {:else}
    {#if site.deploy.mode === 'direct' || site.deploy.mode === 'workers' || site.deploy.mode === 'vps'}
      <div class="card">
        <h3>构建并上线 <span class="grow"></span><button class="btn small" onclick={() => checkSite(site.id)} disabled={running} data-tip="只安装、构建并跑检查，不上传">预检</button><button class="btn small primary" onclick={() => deploySite(site.id)} disabled={running}>{#if running}<span class="spin"></span>{job?.title}中{:else}构建并上线{/if}</button></h3>
        <ol class="chain">
          {#each chain as c, i (i)}<li>{#if c.code}<code>{c.t}</code>{:else}{c.t}{/if}</li>{/each}
        </ol>
        <p class="hint">任何一步失败都会停下，不会上传。{#if site.deploy.mode === 'vps'}SSH 只用 Pilot 部署密钥。{:else if site.deploy.mode === 'workers'}按仓库里的 wrangler 配置部署。{/if}</p>
        {#if job}<div class="job-wrap"><SiteJobPanel {job} bare={job.kind === 'deploy'} /></div>{/if}
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
          <thead><tr><th style="width:120px">时间</th><th style="width:96px">环境</th><th>提交</th><th style="width:72px">状态</th><th style="width:1%"></th></tr></thead>
          <tbody>
            {#each pages as p (p.id)}
              <tr>
                <td>{clockIso(p.created_on)}</td>
                <td>{p.environment === 'production' ? '生产' : '预览'}{p.branch ? ` · ${p.branch}` : ''}</td>
                <td class="trunc" title={p.message}><span class="mono faint">{p.commit.slice(0, 7)}</span> {p.message}</td>
                <td><span class="pill {p.status === 'success' ? 'ok' : p.status === 'failure' ? 'err' : 'run'}" title="{p.stage} · {p.status}">{pagesStatus(p.status)}</span></td>
                <td class="act">
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
        <thead><tr><th style="width:120px">时间</th><th style="width:72px">触发</th><th>提交</th><th class="num" style="width:72px">变化页面</th><th style="width:80px">IndexNow</th><th class="num" style="width:104px">结果</th></tr></thead>
        <tbody>
          {#each history as r (r.id)}
            <tr>
              <td>{clock(r.ts)}</td>
              <td>{trig(r.trigger)}</td>
              <td class="trunc" title={r.message}><span class="mono faint">{r.commit.slice(0, 7) || '—'}</span>{r.dirty ? '（含未提交改动）' : ''} {r.message}</td>
              <td class="num">{r.ok ? r.changed : '—'}</td>
              <td>{r.indexnow === 'ok' ? '已提交' : r.indexnow === 'nothing' ? '无变化' : r.indexnow === 'skipped' ? '未配置' : r.indexnow ? '失败' : '—'}</td>
              <td class="num">{#if r.ok}<span class="pill ok">成功</span>{:else}<span class="pill err" title={r.error}>失败</span>{/if} <span class="faint">{r.duration} 秒</span></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .dp { display: flex; flex-direction: column; gap: 14px; }
  .dp :global(td) { white-space: nowrap; }
  .chain { list-style: none; margin: 0 0 6px; padding: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 6px 0; font-size: 12.5px; color: var(--dim); }
  .chain li { display: inline-flex; align-items: center; min-width: 0; }
  .chain li + li::before { content: '→'; color: var(--faint); margin: 0 8px; }
  .chain code { font-size: 11.5px; padding: 1px 6px; color: var(--text); }
  .job-wrap { margin: 14px -18px -16px; padding: 14px 18px 16px; border-top: 1px solid var(--border); background: #fcfbf9; border-radius: 0 0 12px 12px; }
</style>
