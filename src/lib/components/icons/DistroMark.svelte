<script lang="ts">
  let { id, size }: { id: string; size: number } = $props();
</script>

<!-- 发行版图标：按 os-release 的 ID 选（distroOf 做归一化 + ID_LIKE 兜底）。
     都是简化几何形，认得出即可；认不出的发行版走最后那个通用「服务器」形。 -->
{#if id === 'ubuntu'}
  <!-- Ubuntu：三点「朋友圈」 -->
  <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <circle cx="8" cy="8" r="5.6" stroke="#E95420" stroke-width="1.5" />
    <circle cx="8" cy="2.9" r="1.75" fill="#E95420" /><circle cx="3.6" cy="10.6" r="1.75" fill="#E95420" /><circle cx="12.4" cy="10.6" r="1.75" fill="#E95420" />
  </svg>
{:else if id === 'debian'}
  <!-- Debian：开口螺旋 -->
  <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path d="M11.6 4.2A5.2 5.2 0 1 0 12 11" stroke="#A80030" stroke-width="1.5" stroke-linecap="round" />
    <path d="M9.9 6.1a3 3 0 1 0 .5 3.6" stroke="#A80030" stroke-width="1.4" stroke-linecap="round" />
  </svg>
{:else if id === 'alpine'}
  <!-- Alpine：山 -->
  <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <circle cx="8" cy="8" r="6.2" fill="#0D597F" />
    <path d="M4 10.6 6.3 6.6l1.5 2.6M7.4 10.6l2.4-4.2 2.4 4.2z" stroke="#fff" stroke-width="1.1" stroke-linejoin="round" />
  </svg>
{:else if id === 'fedora'}
  <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <circle cx="8" cy="8" r="6.2" fill="#51A2DA" />
    <path d="M9.9 4.6h-.8a1.9 1.9 0 0 0-1.9 1.9v4a1.5 1.5 0 1 1-1.5-1.5h3.4" stroke="#fff" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{:else if id === 'arch'}
  <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path d="M8 1.8 14 13.6c-1.6-.9-3-1.5-3.6-1.7L8 7.2l-2.4 4.7c-.6.2-2 .8-3.6 1.7L8 1.8z" fill="#1793D1" />
  </svg>
{:else if id === 'centos' || id === 'rhel' || id === 'rocky' || id === 'almalinux'}
  <!-- RHEL 系：四色方块 -->
  <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <rect x="1.8" y="1.8" width="5.4" height="5.4" rx="1" fill="#932279" /><rect x="8.8" y="1.8" width="5.4" height="5.4" rx="1" fill="#EFA724" />
    <rect x="1.8" y="8.8" width="5.4" height="5.4" rx="1" fill="#79BCE8" /><rect x="8.8" y="8.8" width="5.4" height="5.4" rx="1" fill="#9CCD2A" />
  </svg>
{:else}
  <!-- 认不出的（含没探到系统信息时）：中性服务器形 -->
  <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <rect x="2" y="2.6" width="12" height="4.6" rx="1.2" stroke="currentColor" stroke-width="1.2" />
    <rect x="2" y="8.8" width="12" height="4.6" rx="1.2" stroke="currentColor" stroke-width="1.2" />
    <circle cx="4.6" cy="4.9" r=".8" fill="currentColor" /><circle cx="4.6" cy="11.1" r=".8" fill="currentColor" />
  </svg>
{/if}
