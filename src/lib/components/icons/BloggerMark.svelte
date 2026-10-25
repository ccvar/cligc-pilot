<script lang="ts">
  let { size = 16 }: { size?: number } = $props();
</script>

<!-- Blogger 标识（橙色圆角方块里的 B），用在「连接 → Blogger」和站点「分发」。 -->
<svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
  <rect x="1" y="1" width="22" height="22" rx="5" fill="#ff5722" />
  <path fill="#fff" d="M15.6 10.6h-.9c-.5 0-.9-.4-.9-.9C13.8 7.6 12.2 6 10.2 6H8.6C6.6 6 5 7.6 5 9.6v4.8C5 16.4 6.6 18 8.6 18h6.8c2 0 3.6-1.6 3.6-3.6v-2.6c0-.7-.6-1.2-1.2-1.2h-2.2zM8.9 9h2.7c.5 0 .9.4.9.9s-.4.9-.9.9H8.9c-.5 0-.9-.4-.9-.9s.4-.9.9-.9zm6.2 6H8.9c-.5 0-.9-.4-.9-.9s.4-.9.9-.9h6.2c.5 0 .9.4.9.9s-.4.9-.9.9z" />
</svg>
