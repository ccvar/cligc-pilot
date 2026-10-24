<script lang="ts">
  // 「需要你处理」里由 Pilot 自己加的一条：接入 GA4 统计。
  // 不用 AI 写待办：Pilot 能用已连接的 Google 账号建媒体资源、读出衡量 ID，直接存进配置变量再重新构建。
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import { app } from '$lib/state/app.svelte.js';
  import { launch } from '$lib/state/launch.svelte.js';
  import { ga4ToConfig } from '$lib/state/siteMake.svelte.js';

  let { site }: { site: Site } = $props();
  const manage = $derived(launch.status[site.id]?.google_manage_conn ?? '');
  const bound = $derived(!!site.data.ga4_property);
  const busyAi = $derived(app.convos.some((c) => c.conn_id === `site:${site.id}` && c.status === 'running'));
  let mid = $state('');
  let working = $state(false);
  async function run(connId = '', id = '') { working = true; try { await ga4ToConfig(site, connId, id); mid = ''; } finally { working = false; } }
</script>

<li class="ga">
  <div class="t-head"><b>接入 GA4 统计</b><span class="tag pilot">Pilot 帮你做</span></div>
  <p class="why">看访问量、来源和转化；上线后托管运营也要靠这些数据判断哪些页面该改。衡量 ID 存进配置变量 <code>PUBLIC_GA_ID</code>，不写进代码。</p>
  <div class="acts">
    {#if bound}
      <button class="btn tiny primary" disabled={working || busyAi} onclick={() => run()}><Icon name="wand" size={11} />{working ? '处理中…' : '读取已绑定的 GA4 并填好'}</button>
    {:else if manage && site.domain}
      <button class="btn tiny primary" disabled={working || busyAi} onclick={() => run(manage)}><Icon name="wand" size={11} />{working ? '创建中…' : '用 Google 账号创建并接入'}</button>
    {:else if manage}
      <span class="note">先填正式域名（GA4 的网页数据流要用），就能一键创建。</span>
    {:else}
      <span class="note">还没有能建 GA4 的 Google 账号：到连接里用浏览器登录一个，或直接填已有的衡量 ID。</span>
    {/if}
  </div>
  <div class="own">
    <span>或填已有的衡量 ID</span>
    <input class="tin mono" bind:value={mid} placeholder="G-XXXXXXXXXX" spellcheck="false" autocapitalize="characters" onkeydown={(e) => { if (e.key === 'Enter' && mid.trim()) void run('', mid); }} />
    <button class="btn tiny" disabled={working || busyAi || !mid.trim()} onclick={() => run('', mid)}>保存</button>
  </div>
</li>

<style>
  .ga { border: 1px solid #d8e6f5; background: #f6faff; border-radius: 10px; padding: 10px 12px; display: flex; flex-direction: column; gap: 6px; list-style: none; }
  .t-head { display: flex; align-items: center; gap: 8px; }
  .t-head b { font-size: 13px; font-weight: 600; }
  .tag { font-size: 10.5px; padding: 1px 7px; border-radius: 999px; }
  .tag.pilot { background: #e3eefb; color: #2a5d9a; font-weight: 500; }
  .why { margin: 0; font-size: 12px; color: var(--dim); line-height: 1.5; }
  .acts { display: flex; align-items: center; gap: 8px; }
  .note { font-size: 12px; color: var(--faint); }
  .own { display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--dim); }
  .own .tin { max-width: 200px; font-size: 12.5px; }
</style>
