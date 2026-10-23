<script lang="ts">
  // 站点图标：有 favicon 用 favicon，没有就是通用的地球标识。
  import SiteMark from '$lib/components/icons/SiteMark.svelte';
  import { sites } from '$lib/state/sites.svelte.js';

  let { id, size = 14 }: { id: string; size?: number } = $props();
  const src = $derived(sites.icons[id] ?? '');
  let broken = $state(false);
  $effect(() => { void src; broken = false; });
</script>

{#if src && !broken}
  <img class="site-ico" {src} width={size} height={size} alt="" draggable="false" onerror={() => (broken = true)} />
{:else}
  <SiteMark {size} />
{/if}

<style>
  .site-ico { flex: none; display: block; object-fit: contain; border-radius: 3px; }
</style>
