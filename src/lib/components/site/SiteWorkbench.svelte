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
  import SiteDistributeTab from './SiteDistributeTab.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { sites, loadSiteStatus, deploySite, pilotDeploys, type SiteTab } from '$lib/state/sites.svelte.js';
  import { ops, loadQueue } from '$lib/state/ops.svelte.js';
  import { managed, loadManaged } from '$lib/state/managed.svelte.js';
  import { newChat } from '$lib/state/conversations';

  const site = $derived(sites.active);
  const st = $derived(site ? sites.status[site.id] : undefined);
  const job = $derived(site ? sites.jobs[site.id] : undefined);
  // 「待处理」的数字：只算要你处理的（待确认、失败、上线后待核对）；托管站点排着等 AI 的另外淡色显示，
  // 自己运营的站点没人替你处理，未处理的机会也算进去
  const queue = $derived(site ? ops.queue[site.id] ?? [] : []);
  const inPlan = $derived(!!site && !!managed.overview?.groups.some((g) => g.sites.includes(site.id)));
  const needCount = $derived(queue.filter((q) => ['proposed', 'failed'].includes(q.status) || (q.status === 'deployed' && !!q.attention)).length + (inPlan ? 0 : queue.filter((q) => q.status === 'open').length));
  const queuedCount = $derived(inPlan ? queue.filter((q) => q.status === 'open').length : 0);
  // 代码文件夹显示成 ~/work/xxx 这样的短路径
  const shortPath = (p: string) => (p || '').replace(/^\/Users\/[^/]+/, '~').replace(/^\/home\/[^/]+/, '~').replace(/^[A-Za-z]:\\Users\\[^\\]+/, '~');
  const TABS: { key: SiteTab; label: string; icon: string }[] = [
    { key: 'overview', label: '概览', icon: 'grid' },
    { key: 'data', label: '数据', icon: 'chart' },
    { key: 'queue', label: '待处理', icon: 'inbox' },
    { key: 'deploy', label: '部署', icon: 'rocket' },
    { key: 'distribute', label: '分发', icon: 'external' },
    { key: 'settings', label: '设置', icon: 'sliders' },
  ];
  $effect(() => { if (site) { void loadSiteStatus(site.id); void loadQueue(site.id); } });
  $effect(() => { if (!managed.overview) void loadManaged(); });
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
            {#if site.source === 'github'}<button class="link-btn plain src-link" onclick={() => openUrl(`https://github.com/${site.repo}`)} data-tip="在 GitHub 打开仓库"><GhMark size={12} />{site.repo}</button>{:else if st?.code_dir && st.exists}<button class="link-btn plain src-link" onclick={() => revealItemInDir(st.code_dir)} data-tip="在访达中打开代码文件夹"><Icon name="folder" size={12} />{shortPath(st.code_dir)}</button>{:else}<span class="src-link"><Icon name="folder" size={12} />{shortPath(site.local_dir) || '本地文件夹'}</span>{/if}{site.framework ? ` · ${site.framework}` : ''}
          </small>
        </div>
      </div>
      <div class="row">
        <button class="btn small" onclick={newChat}><Icon name="chat" size={13} />新对话</button>
        {#if pilotDeploys(site.deploy.mode)}
          <button class="btn small primary" onclick={() => deploySite(site.id)} disabled={job?.status === 'running'}>{#if job?.status === 'running'}<span class="spin"></span>{job.title}中{:else}<Icon name="rocket" size={13} />构建并上线{/if}</button>
        {/if}
      </div>
    </header>
    <nav class="tabs">
      {#each TABS as t (t.key)}
        <button class:on={sites.tab === t.key} aria-current={sites.tab === t.key ? 'page' : undefined} onclick={() => (sites.tab = t.key)}><Icon name={t.icon} size={13} />{t.label}{#if t.key === 'queue' && needCount}<span class="cnt" data-tip="{needCount} 条要你处理">{needCount}</span>{:else if t.key === 'queue' && queuedCount}<span class="cnt soft" data-tip="{queuedCount} 条排着等 AI 处理，不用你动手">{queuedCount}</span>{/if}</button>
      {/each}
    </nav>
    <div class="page-body">
      {#key `${site.id}:${sites.tab}`}
        <div class="pane">
          {#if sites.tab === 'overview'}<SiteOverview {site} />
          {:else if sites.tab === 'data'}<SiteDataTab {site} />
          {:else if sites.tab === 'queue'}<SiteQueue {site} />
          {:else if sites.tab === 'deploy'}<SiteDeployTab {site} />
          {:else if sites.tab === 'distribute'}<SiteDistributeTab {site} />
          {:else}<SiteSettings {site} />{/if}
        </div>
      {/key}
    </div>
  </div>
{:else}
  <div class="su page wb empty"><p class="hint">{sites.loaded ? '站点不存在或已被移除。' : '正在读取站点…'}</p></div>
{/if}

<style>
  .src-link { display: inline-flex; align-items: center; gap: 4px; vertical-align: -1px; }
  .wb.empty { align-items: center; justify-content: center; }
  .wb-title { display: flex; align-items: center; gap: 10px; min-width: 0; flex: 1 1 160px; }
  .wb-ic { width: 30px; height: 30px; border-radius: 8px; background: var(--rail); border: 1px solid var(--border); display: inline-flex; align-items: center; justify-content: center; color: var(--dim); flex: none; }
  .wb-name { min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .wb-name small { display: block; color: var(--dim); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .link-btn.plain { text-decoration: none; color: var(--dim); font-size: 12px; transition: color .12s; }
  .link-btn.plain:hover { color: var(--text); }
</style>
