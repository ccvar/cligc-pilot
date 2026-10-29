<script lang="ts">
  // 部署方式的小图标，各处统一：Pages、Workers 用 Cloudflare 各自的产品图标，Git 自动构建用分支，VPS 用服务器，没配部署不画
  import Icon from './Icon.svelte';
  import CfProductMark from '$lib/components/icons/CfProductMark.svelte';
  let { mode, size = 12 }: { mode: string; size?: number } = $props();
</script>

{#if mode === 'direct'}<CfProductMark kind="pages" {size} />{:else if mode === 'workers'}<CfProductMark kind="workers" {size} />{:else if mode === 'pages_git'}<Icon name="branch" {size} />{:else if mode === 'vps'}<Icon name="server" {size} />{/if}
