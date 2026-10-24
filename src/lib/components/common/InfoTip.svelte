<script lang="ts">
  // 标题旁的 ⓘ：说明文字收进悬停提示（全局 .ui-tip，支持 \n 换行），不再平铺成一段灰字占地方。
  let { text, label = '说明' }: { text: string; label?: string } = $props();
</script>

<span class="info-tip" data-tip={text} role="img" aria-label={`${label}：${text}`} tabindex="-1">
  <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true"><circle cx="8" cy="8" r="6.25" stroke="currentColor" stroke-width="1.3" /><path d="M8 7.2v4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><circle cx="8" cy="4.9" r=".85" fill="currentColor" /></svg>
</span>

<style>
  .info-tip { display: inline-flex; align-items: center; justify-content: center; width: 18px; height: 18px; margin-left: 2px; border-radius: 50%; color: var(--faint); cursor: help; vertical-align: middle; text-transform: none; letter-spacing: 0; transition: color .12s; }
  .info-tip:hover { color: var(--text); }
</style>
