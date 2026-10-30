<script lang="ts">
  // 站点健康各块右上角的「交给 AI」：还没交就是按钮；已经在待处理里（排队、处理中、等你确认…）就显示状态，点了跳过去。
  import Icon from './Icon.svelte';
  import { ops, loadQueue, healthToAi, LIVE } from '$lib/state/ops.svelte.js';
  import { openSite } from '$lib/state/sites.svelte.js';

  let { siteId, what, match, label, tip, url = '', link = false }: {
    siteId: string;
    what: 'speed' | 'audit';
    /** 队列里哪些条目算这一块交出去的（按机会的 key） */
    match: (key: string) => boolean;
    label: string;
    tip: string;
    /** 只交这一个页面（页面速度） */
    url?: string;
    /** 文字链接样式（放在展开的清单里） */
    link?: boolean;
  } = $props();
  let busy = $state(false);
  $effect(() => { if (!ops.queue[siteId]) void loadQueue(siteId); });
  // 接在「已交给 AI · 」后面，不再重复「AI」
  const STATE: Record<string, string> = { open: '排队中', working: '处理中', proposed: '等你确认', merged: '已合并', failed: '没成功' };
  const live = $derived((ops.queue[siteId] ?? []).find((i) => LIVE.has(i.status) && match(i.opp.key)));
  async function go() {
    busy = true;
    try { await healthToAi(siteId, what, url || undefined); } finally { busy = false; }
  }
</script>

{#if live}
  <button class={link ? 'link-btn' : 'btn tiny'} onclick={() => openSite(siteId, 'queue', '', live.id)} data-tip="「{live.opp.title}」{STATE[live.status] ?? live.status}，点一下去待处理看">
    {#if live.status === 'working'}<span class="spin"></span>{:else if !link}<Icon name="sparkles" size={11} />{/if}已交给 AI · {STATE[live.status] ?? live.status}
  </button>
{:else}
  <button class={link ? 'link-btn' : 'btn tiny'} onclick={go} disabled={busy} data-tip={tip}>
    {#if busy}<span class="spin"></span>{:else if !link}<Icon name="wand" size={11} />{/if}{label}
  </button>
{/if}
