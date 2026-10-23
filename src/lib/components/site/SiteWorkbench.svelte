<script lang="ts">
  import './site.css';
  import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
  import SiteMark from '$lib/components/icons/SiteMark.svelte';
  import SiteOverview from './SiteOverview.svelte';
  import SiteDeployTab from './SiteDeployTab.svelte';
  import SiteSettings from './SiteSettings.svelte';
  import SiteDataTab from './SiteDataTab.svelte';
  import SiteQueue from './SiteQueue.svelte';
  import SiteOpsTab from './SiteOpsTab.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { sites, loadSiteStatus, deploySite, type SiteTab } from '$lib/state/sites.svelte.js';
  import { ops, loadQueue } from '$lib/state/ops.svelte.js';
  import { newChat } from '$lib/state/conversations';

  const site = $derived(sites.active);
  const st = $derived(site ? sites.status[site.id] : undefined);
  const job = $derived(site ? sites.jobs[site.id] : undefined);
  const openCount = $derived(site ? (ops.queue[site.id] ?? []).filter((q) => q.status === 'open' || q.status === 'proposed').length : 0);
  const TABS: { key: SiteTab; label: string }[] = [
    { key: 'overview', label: '概览' },
    { key: 'data', label: '数据' },
    { key: 'queue', label: '待处理' },
    { key: 'ops', label: '运营' },
    { key: 'deploy', label: '部署' },
    { key: 'settings', label: '设置' },
  ];
  $effect(() => { if (site) { void loadSiteStatus(site.id); void loadQueue(site.id); } });
</script>

{#if site}
  <div class="su wb">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header class="wb-head" data-tauri-drag-region onmousedown={startDrag}>
      <div class="wb-title">
        <span class="wb-ic"><SiteMark size={18} /></span>
        <div class="wb-name">
          <b>{site.name}</b>
          <small>
            {#if site.domain}<button class="link-btn plain" onclick={() => openUrl(`https://${site.domain}`)}>{site.domain} ↗</button> · {/if}
            {site.source === 'github' ? site.repo : '本地文件夹'}{site.framework ? ` · ${site.framework}` : ''}
          </small>
        </div>
      </div>
      <div class="row">
        {#if site.source === 'github'}<button class="btn small ghost" onclick={() => openUrl(`https://github.com/${site.repo}`)}>仓库 ↗</button>{/if}
        {#if st?.code_dir && st.exists}<button class="btn small ghost" onclick={() => revealItemInDir(st.code_dir)}>打开文件夹</button>{/if}
        <button class="btn small" onclick={newChat}>新对话</button>
        {#if site.deploy.mode === 'direct'}
          <button class="btn small primary" onclick={() => deploySite(site.id)} disabled={job?.status === 'running'}>{job?.status === 'running' ? `${job.title}中…` : '构建并上线'}</button>
        {/if}
      </div>
    </header>
    <nav class="wb-tabs">
      {#each TABS as t (t.key)}
        <button class:on={sites.tab === t.key} onclick={() => (sites.tab = t.key)}>{t.label}{#if t.key === 'queue' && openCount}<span class="cnt">{openCount}</span>{/if}</button>
      {/each}
    </nav>
    <div class="wb-body">
      {#if sites.tab === 'overview'}<SiteOverview {site} />
      {:else if sites.tab === 'data'}<SiteDataTab {site} />
      {:else if sites.tab === 'queue'}<SiteQueue {site} />
      {:else if sites.tab === 'ops'}<SiteOpsTab {site} />
      {:else if sites.tab === 'deploy'}<SiteDeployTab {site} />
      {:else}<SiteSettings {site} />{/if}
    </div>
  </div>
{:else}
  <div class="su wb empty"><p class="hint">{sites.loaded ? '站点不存在或已被移除。' : '正在读取站点…'}</p></div>
{/if}

<style>
  .wb { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .wb.empty { align-items: center; justify-content: center; }
  .wb-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 34px 28px 10px; flex-wrap: wrap; }
  .wb-title { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .wb-ic { width: 34px; height: 34px; border-radius: 10px; background: var(--rail); border: 1px solid var(--border); display: inline-flex; align-items: center; justify-content: center; color: var(--dim); flex: none; }
  .wb-name { min-width: 0; }
  .wb-name b { display: block; font-size: 17px; line-height: 1.25; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .wb-name small { display: block; color: var(--dim); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .link-btn.plain { text-decoration: none; color: var(--accent); font-size: 12px; }
  .wb-tabs { display: flex; gap: 2px; padding: 0 22px; border-bottom: 1px solid var(--border); }
  .wb-tabs button { position: relative; border: none; background: none; padding: 8px 10px 9px; font: inherit; font-size: 13px; color: var(--dim); cursor: pointer; }
  .wb-tabs button:hover { color: var(--text); }
  .wb-tabs button.on { color: var(--text); font-weight: 600; }
  .wb-tabs button.on::after { content: ''; position: absolute; left: 8px; right: 8px; bottom: -1px; height: 2px; border-radius: 2px; background: var(--accent); }
  .cnt { margin-left: 5px; display: inline-flex; min-width: 16px; height: 16px; padding: 0 4px; box-sizing: border-box; align-items: center; justify-content: center; border-radius: 999px; background: var(--accent); color: #fff; font-size: 10px; font-weight: 700; }
  .wb-body { flex: 1; min-height: 0; overflow-y: auto; padding: 18px 28px 40px; }
  .wb-body > :global(*) { max-width: 1080px; }
</style>
