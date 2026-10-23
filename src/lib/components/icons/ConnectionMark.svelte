<script lang="ts">
  import CfMark from './CfMark.svelte';
  import SshMark from './SshMark.svelte';
  import GhMark from './GhMark.svelte';

  // 按连接类型选标识；新增连接类型时在这里登记它的图标。
  let { kind, size }: { kind: string; size: number } = $props();
</script>

{#if kind === 'ssh'}<SshMark {size} />{:else if kind === 'cloudflare'}<CfMark {size} />{:else if kind === 'github'}<GhMark {size} />{/if}
