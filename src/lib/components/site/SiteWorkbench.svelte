<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import SiteSwitcher from './SiteSwitcher.svelte';
  import SiteOverview from './SiteOverview.svelte';
  import SiteDeployTab from './SiteDeployTab.svelte';
  import SiteSettings from './SiteSettings.svelte';
  import SiteDataTab from './SiteDataTab.svelte';
  import SiteQueue from './SiteQueue.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { sites, loadSiteStatus, deploySite, pilotDeploys, type SiteTab } from '$lib/state/sites.svelte.js';
  import { ops, loadQueue } from '$lib/state/ops.svelte.js';
  import { newChat } from '$lib/state/conversations';

  const site = $derived(sites.active);
  const st = $derived(site ? sites.status[site.id] : undefined);
  const job = $derived(site ? sites.jobs[site.id] : undefined);
  const openCount = $derived(site ? (ops.queue[site.id] ?? []).filter((q) => q.status === 'open' || q.status === 'proposed').length : 0);
  const TABS: { key: SiteTab; label: string; icon: string }[] = [
    { key: 'overview', label: '概览', icon: 'grid' },
    { key: 'data', label: '数据', icon: 'chart' },
    { key: 'queue', label: '待处理', icon: 'inbox' },
    { key: 'deploy', label: '部署', icon: 'rocket' },
    { key: 'settings', label: '设置', icon: 'sliders' },
  ];
  $effect(() => { if (site) { void loadSiteStatus(site.id); void loadQueue(site.id); } });
</script>

{#if site}
  <div class="su page wb">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header class="page-head wb-head" data-tauri-drag-region onmousedown={startDrag}>
      <div class="wb-title">
        <span class="wb-ic"><SiteIcon id={site.id} size={18} /></span>
        <div class="wb-name">
          <SiteSwitcher />
          <small>
            {#if site.domain}<button class="link-btn plain" onclick={() => openUrl(`https://${site.domain}`)}>{site.domain} ↗</button> · {/if}
            {site.source === 'github' ? site.repo : '本地文件夹'}{site.framework ? ` · ${site.framework}` : ''}
          </small>
        </div>
      </div>
      <div class="row">
        {#if site.source === 'github'}<button class="btn small ghost" onclick={() => openUrl(`https://github.com/${site.repo}`)}><GhMark size={13} />仓库</button>{/if}
        {#if st?.code_dir && st.exists}<button class="btn small ghost" onclick={() => revealItemInDir(st.code_dir)}><Icon name="folder" size={13} />打开文件夹</button>{/if}
        <button class="btn small" onclick={newChat}><Icon name="chat" size={13} />新对话</button>
        {#if pilotDeploys(site.deploy.mode)}
          <button class="btn small primary" onclick={() => deploySite(site.id)} disabled={job?.status === 'running'}>{#if job?.status === 'running'}<span class="spin"></span>{job.title}中{:else}<Icon name="rocket" size={13} />构建并上线{/if}</button>
        {/if}
      </div>
    </header>
    <nav class="tabs">
      {#each TABS as t (t.key)}
        <button class:on={sites.tab === t.key} aria-current={sites.tab === t.key ? 'page' : undefined} onclick={() => (sites.tab = t.key)}><Icon name={t.icon} size={13} />{t.label}{#if t.key === 'queue' && openCount}<span class="cnt">{openCount}</span>{/if}</button>
      {/each}
    </nav>
    <div class="page-body">
      {#key `${site.id}:${sites.tab}`}
        <div class="pane">
          {#if sites.tab === 'overview'}<SiteOverview {site} />
          {:else if sites.tab === 'data'}<SiteDataTab {site} />
          {:else if sites.tab === 'queue'}<SiteQueue {site} />
          {:else if sites.tab === 'deploy'}<SiteDeployTab {site} />
          {:else}<SiteSettings {site} />{/if}
        </div>
      {/key}
    </div>
  </div>
{:else}
  <div class="su page wb empty"><p class="hint">{sites.loaded ? '站点不存在或已被移除。' : '正在读取站点…'}</p></div>
{/if}

<style>
  .wb.empty { align-items: center; justify-content: center; }
  .wb-title { display: flex; align-items: center; gap: 10px; min-width: 0; flex: 1 1 160px; }
  .wb-ic { width: 30px; height: 30px; border-radius: 8px; background: var(--rail); border: 1px solid var(--border); display: inline-flex; align-items: center; justify-content: center; color: var(--dim); flex: none; }
  .wb-name { min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .wb-name small { display: block; color: var(--dim); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .link-btn.plain { text-decoration: none; color: var(--dim); font-size: 12px; transition: color .12s; }
  .link-btn.plain:hover { color: var(--text); }
</style>
