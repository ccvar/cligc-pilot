<script lang="ts">
  // 站点对话输入框上方的「构建并上线」：AI 在站点文件夹里改完、还没上线时出现，点一下走和站点页同一个上线流程（先确认）。
  // 只管 Pilot 自己构建上传的本地站点；GitHub 来源上线的是仓库默认分支、运营任务的对话在独立工作树里，都不在这里上线。
  import { untrack } from 'svelte';
  import type { Conversation } from '$lib/types';
  import Icon from '$lib/components/site/Icon.svelte';
  import { sites, deploySite, loadSiteStatus, openSite, pilotDeploys, SITE_PREFIX } from '$lib/state/sites.svelte.js';

  let { conv, busy }: { conv: Conversation | null; busy: boolean } = $props();

  const siteId = $derived(conv?.conn_id.startsWith(SITE_PREFIX) && !conv.workspace_dir ? conv.conn_id.slice(SITE_PREFIX.length) : '');
  const site = $derived(siteId ? sites.list.find((s) => s.id === siteId) : undefined);
  const eligible = $derived(!!site && site.source !== 'github' && pilotDeploys(site.deploy.mode));
  // 切到这个对话、或这轮跑完时重新看一眼站点文件夹：AI 刚改的要算进去
  $effect(() => { if (eligible && !busy) { const id = siteId; untrack(() => void loadSiteStatus(id)); } });
  const st = $derived(siteId ? sites.status[siteId] : undefined);
  const job = $derived(siteId ? sites.jobs[siteId] : undefined);
  const running = $derived(job?.status === 'running');
  // 有没上线的改动：文件夹里有未提交的改动，或者提交了但线上还不是这一版
  const pending = $derived(!!st?.exists && (st.dirty || (!!st.head && st.head !== st.last_deploy?.commit)));
  const target = $derived(site?.domain || site?.name || '');
</script>

{#if eligible && site && !busy && (pending || running)}
  <div class="deploy-bar">
    {#if running && job?.kind === 'deploy'}
      <span class="spin"></span><span class="t">正在上线 {target}…</span>
      <button class="db-btn dim" onclick={() => openSite(site.id, 'deploy')}>看进度</button>
    {:else}
      <Icon name="rocket" size={12} /><span class="t">{st?.last_deploy ? '这里的改动还没上线' : `${target} 还没上线过`}</span>
      <span class="tipwrap" data-tip={running ? `站点正在${job?.title ?? '跑别的任务'}，等它跑完再上线` : `构建、跑检查，通过后上线到 ${target}（先确认一次）`}>
        <button class="db-btn" onclick={() => void deploySite(site.id)} disabled={running}>构建并上线</button>
      </span>
    {/if}
  </div>
{/if}

<style>
  .deploy-bar { max-width: 760px; margin: 0 auto 6px; display: flex; align-items: center; gap: 6px; padding: 0 4px; font-size: 12px; color: var(--dim); }
  .t { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .db-btn { display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; font-size: 12px; padding: 3px 8px; border: none; background: none; color: var(--accent); cursor: pointer; font-weight: 500; border-radius: 6px; }
  .db-btn:hover:not(:disabled) { background: var(--rail); }
  .db-btn:disabled { opacity: .45; cursor: default; }
  .db-btn.dim { color: var(--dim); font-weight: 400; }
  .spin { flex: none; width: 10px; height: 10px; border: 1.5px solid var(--border2); border-top-color: var(--dim); border-radius: 50%; animation: db-spin .7s linear infinite; }
  @keyframes db-spin { to { transform: rotate(360deg); } }
</style>
