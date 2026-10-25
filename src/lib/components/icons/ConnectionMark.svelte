<script lang="ts">
  import CfMark from './CfMark.svelte';
  import SshMark from './SshMark.svelte';
  import GhMark from './GhMark.svelte';
  import SiteMark from './SiteMark.svelte';
  import GoogleMark from './GoogleMark.svelte';
  import BingMark from './BingMark.svelte';
  import BloggerMark from './BloggerMark.svelte';
  import WordpressMark from './WordpressMark.svelte';

  // 按连接类型选标识；新增连接类型时在这里登记它的图标。
  let { kind, size }: { kind: string; size: number } = $props();
</script>

{#if kind === 'ssh'}<SshMark {size} />{:else if kind === 'cloudflare'}<CfMark {size} />{:else if kind === 'github'}<GhMark {size} />{:else if kind === 'site'}<SiteMark {size} />{:else if kind === 'google'}<GoogleMark {size} />{:else if kind === 'bing'}<BingMark {size} />{:else if kind === 'blogger'}<BloggerMark {size} />{:else if kind === 'wordpress'}<WordpressMark {size} />{/if}
