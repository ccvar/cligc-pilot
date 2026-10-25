<script lang="ts">
  let { size = 16 }: { size?: number } = $props();
</script>

<!-- WordPress 标识（蓝色圆里的 W），用在「连接 → WordPress.com」和站点「分发」。 -->
<svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
  <circle cx="12" cy="12" r="11" fill="#3858e9" />
  <path fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M5.5 8.2 8.6 16.5 12 9.6l3.4 6.9 3.1-8.3" />
</svg>
