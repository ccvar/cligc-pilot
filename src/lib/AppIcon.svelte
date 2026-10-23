<script lang="ts">
  // 客户端（Cligc Pilot）应用图标：浅色圆角方 + 绿色渐变 30° 斜置双柱（一长一短），与 src-tauri/icons 的 app 图标一致。
  // 界面里多处同时渲染，渐变 id 按实例区分，避免互相引用。
  let { size = 18 }: { size?: number } = $props();
  const uid = $props.id();
  const gid = `ai-g-${uid}`;
</script>

<svg class="ai" width={size} height={size} viewBox="6 6 88 88" role="img" aria-label="Cligc Pilot">
  <defs>
    <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#6be09a" />
      <stop offset="1" stop-color="#18a058" />
    </linearGradient>
  </defs>
  <rect x="6.5" y="6.5" width="87" height="87" rx="19.5" fill="#fff" stroke="#e3e1db" />
  <g transform="translate(3.5 0) rotate(30 50 50)">
    <rect x="35" y="24" width="13" height="52" rx="6.5" fill="url(#{gid})" />
    <rect x="53" y="40" width="13" height="36" rx="6.5" fill="url(#{gid})" />
  </g>
</svg>

<style>
  .ai { flex: none; display: block; }
</style>
