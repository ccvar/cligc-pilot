<script lang="ts">
  // 复制链接的小图标按钮（配提示）：在别的浏览器 / 账号里打开时用。复制后图标变成对勾 1.5 秒。
  import { say } from '$lib/state/app.svelte.js';
  let { url, tip = '复制链接' }: { url: string | (() => string); tip?: string } = $props();
  let done = $state(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(typeof url === 'function' ? url() : url);
      done = true;
      setTimeout(() => (done = false), 1500);
    } catch { say('复制失败，请手动复制', 'err'); }
  }
</script>

<button type="button" class="copy-link" onclick={copy} aria-label={tip} data-tip={done ? '已复制' : tip}>
  {#if done}
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="m3.5 8.5 3 3 6-7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
  {:else}
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M6.6 9.4a2.6 2.6 0 0 0 3.7 0l2.3-2.3a2.6 2.6 0 0 0-3.7-3.7l-.6.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><path d="M9.4 6.6a2.6 2.6 0 0 0-3.7 0L3.4 8.9a2.6 2.6 0 0 0 3.7 3.7l.6-.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>
  {/if}
</button>

<style>
  .copy-link { width: 32px; height: 32px; display: inline-flex; align-items: center; justify-content: center; flex: none; border: none; background: none; border-radius: 8px; color: var(--dim); cursor: pointer; transition: background-color .12s, color .12s; }
  .copy-link:hover { background: var(--hover, rgba(0, 0, 0, .05)); color: var(--text); }
</style>
