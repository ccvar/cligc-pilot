<script lang="ts">
  // 客户端（Cligc Pilot）应用图标：墨黑圆角方 + 米白「C」，与 src-tauri/icons 的 app 图标一致。
  // 与旧版 gcms Pilot 的米色底红棕「C」深浅相反，两者同装时一眼可分。
  let { size = 18 }: { size?: number } = $props();
</script>

<svg class="ai" width={size} height={size} viewBox="0 0 64 64" role="img" aria-label="Cligc Pilot">
  <rect width="64" height="64" rx="15" fill="#2f2c27" />
  <path d="M44 24a14 14 0 1 0 0 16" fill="none" stroke="#f1ece1" stroke-width="7" stroke-linecap="round" />
  <circle cx="45.5" cy="42" r="4.6" fill="#f1ece1" />
</svg>

<style>
  .ai { flex: none; border-radius: 5px; display: block; }
</style>
