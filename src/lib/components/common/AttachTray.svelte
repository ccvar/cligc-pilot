<script lang="ts">
  // 输入框上方的附件（和 Claude 客户端一样）：图片是方形缩略图（点一下看大图），其它文件是带类型的小卡片；鼠标移上去右上角出 ×。
  import type { Attach } from '$lib/state/composer.svelte.js';
  import { ui } from '$lib/state/ui.svelte.js';
  let { items, uploading = false, onremove }: { items: Attach[]; uploading?: boolean; onremove: (i: number) => void } = $props();
  const ext = (name: string) => (name.includes('.') ? name.split('.').pop()!.toUpperCase().slice(0, 6) : '文件');
</script>

{#if items.length || uploading}
  <div class="tray">
    {#each items as a, i (a.path)}
      <div class="tile" class:img={!!a.preview} title={a.name}>
        {#if a.preview}
          <button type="button" class="zoom" aria-label={`查看 ${a.name}`} onclick={() => (ui.lightbox = a.preview)}><img src={a.preview} alt={a.name} /></button>
        {:else}
          <span class="fi"><svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M9 1.8H4.5A1.3 1.3 0 0 0 3.2 3.1v9.8a1.3 1.3 0 0 0 1.3 1.3h7a1.3 1.3 0 0 0 1.3-1.3V5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9 1.8v3.7h3.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg></span>
          <span class="fn">{a.name}</span>
          <span class="fx">{ext(a.name)}</span>
        {/if}
        <button type="button" class="rm" aria-label={`移除 ${a.name}`} onclick={() => onremove(i)}>
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true"><path d="M2.5 2.5l5 5M7.5 2.5l-5 5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
        </button>
      </div>
    {/each}
    {#if uploading}<div class="tile wait" aria-label="上传中"></div>{/if}
  </div>
{/if}

<style>
  .tray { display: flex; flex-wrap: wrap; gap: 10px; padding: 12px 14px 2px; }
  /* 尺寸照 Claude 客户端：112 见方、圆角 14、不描边，只留一圈很淡的阴影线 */
  .tile { position: relative; flex: none; width: 112px; height: 112px; display: flex; flex-direction: column; gap: 4px; padding: 11px 11px 9px; border-radius: 14px; background: var(--panel); box-shadow: inset 0 0 0 1px rgba(30, 28, 24, .08); font-size: 12px; }
  .tile.img { padding: 0; overflow: hidden; box-shadow: 0 0 0 1px rgba(30, 28, 24, .06); background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 12px 12px; }
  .zoom { display: block; width: 100%; height: 100%; padding: 0; border: none; background: none; cursor: zoom-in; }
  .tile.img img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .fi { color: var(--dim); display: inline-flex; }
  .fn { color: var(--text); line-height: 1.35; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; word-break: break-all; }
  .fx { margin-top: auto; color: var(--faint); font-size: 10.5px; letter-spacing: .03em; }
  .rm { position: absolute; top: 6px; right: 6px; width: 22px; height: 22px; display: flex; align-items: center; justify-content: center; border: none; border-radius: 50%; background: rgba(30, 28, 24, .72); color: #fff; cursor: pointer; opacity: 0; transition: opacity .12s; }
  .tile:hover .rm, .rm:focus-visible { opacity: 1; }
  .rm:hover { background: var(--err); }
  .tile.wait { padding: 0; animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 0%, 100% { opacity: .5; } 50% { opacity: .9; } }
</style>
