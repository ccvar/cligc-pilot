<script lang="ts">
  // Cloudflare Pages / Workers 各自的产品图标（Simple Icons，CC0），Cloudflare 橙
  import { CF_PAGES_PATH, CF_WORKERS_PATH } from './cfProduct';
  let { kind, size }: { kind: 'pages' | 'workers'; size: number } = $props();
</script>

<svg class="cfp-mark" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true"><path fill="#f38020" d={kind === 'pages' ? CF_PAGES_PATH : CF_WORKERS_PATH} /></svg>

<style>
  .cfp-mark { flex: none; display: inline-block; vertical-align: middle; }
</style>
