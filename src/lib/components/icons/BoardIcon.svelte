<script lang="ts">
  let { size = 14 }: { size?: number } = $props();
</script>

<!-- 站点看板入口（柱状图）。 -->
<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M2.5 13.5h11" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /><rect x="3.4" y="8" width="2.3" height="4" rx=".6" stroke="currentColor" stroke-width="1.2" /><rect x="6.85" y="4.5" width="2.3" height="7.5" rx=".6" stroke="currentColor" stroke-width="1.2" /><rect x="10.3" y="6.5" width="2.3" height="5.5" rx=".6" stroke="currentColor" stroke-width="1.2" /></svg>
