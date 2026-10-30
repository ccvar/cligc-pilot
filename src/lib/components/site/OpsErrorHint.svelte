<script lang="ts">
  // 站点运营作业失败的报错下面那一行：问题在哪、去哪改（充值页、API 服务设置）；认不出的报错不显示。
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { opsErrorHint } from '$lib/friendlyError';
  import { openConnectionSetup } from '$lib/state/settings.svelte.js';
  let { text }: { text: string } = $props();
  const h = $derived(opsErrorHint(text));
</script>

{#if h}
  <div class="oeh">
    <span>{h.text}</span>
    {#if h.billing}<button class="link-btn" onclick={() => openUrl(h.billing)}>去充值 ↗</button>{/if}
    {#if h.settings}<button class="link-btn" onclick={(e) => openConnectionSetup(e)}>打开 API 设置</button>{/if}
  </div>
{/if}

<style>
  .oeh { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 12px; font-size: 12px; line-height: 1.6; color: var(--dim); }
  .oeh span { flex: 1 1 320px; min-width: 0; }
  .oeh .link-btn { flex: none; font-size: 12px; }
</style>
