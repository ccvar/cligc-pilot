<script lang="ts">
  let { size }: { size: number } = $props();
</script>

<!-- 站点标识（单色地球）：表示一个导入的站点或站点会话。 -->
<svg width={size} height={size} viewBox="0 0 20 20" fill="none" aria-hidden="true"><circle cx="10" cy="10" r="6.6" stroke="currentColor" stroke-width="1.35" /><path d="M3.6 10h12.8M10 3.4c1.9 1.8 2.9 4.1 2.9 6.6s-1 4.8-2.9 6.6M10 3.4C8.1 5.2 7.1 7.5 7.1 10s1 4.8 2.9 6.6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg>
