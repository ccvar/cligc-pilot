<script lang="ts">
  // 输入框左下角的「+」（和 Claude 客户端一样）：点开是菜单——添加文件或图片（⌘U），启动页自由对话还能添加文件夹。
  // 选文件用系统对话框拿到路径，交给后端直接复制（不经过前端读内容，所以不限大小）。
  // 菜单用 fixed 定位贴在按钮上方：分屏格子里的输入框外层会裁切溢出，absolute 会被切掉。
  // ⌘U 只给焦点所在的那个输入框（分屏时每格各有一个）；焦点不在任何输入框、页面上又只有一个时也给它。
  import { onMount } from 'svelte';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  let { onpaths, folder = null, disabled = false }: {
    onpaths: (paths: string[]) => void;
    /** 启动页自由对话：选工作文件夹 */
    folder?: { label: string; onpick: () => void } | null;
    disabled?: boolean;
  } = $props();
  let btn = $state<HTMLButtonElement | undefined>();
  let open = $state(false);
  let pos = $state({ left: 0, bottom: 0 });
  const mac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);

  function toggle() {
    if (open) { open = false; return; }
    const r = btn!.getBoundingClientRect();
    pos = { left: Math.max(8, Math.min(r.left, window.innerWidth - 248)), bottom: window.innerHeight - r.top + 6 };
    open = true;
  }
  async function pickFiles() {
    open = false;
    const picked = await openDialog({ title: '添加文件或图片', multiple: true, directory: false });
    const paths = picked == null ? [] : Array.isArray(picked) ? picked : [picked];
    if (paths.length) onpaths(paths);
  }
  function pickFolder() { open = false; folder?.onpick(); }

  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (open && e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); open = false; return; }
      if (disabled || !(mac ? e.metaKey : e.ctrlKey) || e.shiftKey || e.altKey || e.key.toLowerCase() !== 'u') return;
      const mine = btn?.closest('.composer');
      const act = document.activeElement;
      const focusedHere = !!mine && !!act && mine.contains(act);
      const lone = (!act || act === document.body) && document.querySelectorAll('.attach-plus').length === 1;
      if (!focusedHere && !lone) return;
      e.preventDefault();
      void pickFiles();
    };
    const onDown = (e: PointerEvent) => {
      if (!open) return;
      const t = e.target as Node;
      if (btn?.contains(t) || (t instanceof Element && t.closest('.attach-pop'))) return;
      open = false;
    };
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('pointerdown', onDown, true);
    return () => { window.removeEventListener('keydown', onKey, true); window.removeEventListener('pointerdown', onDown, true); };
  });
</script>

<button type="button" class="attach-plus" class:on={open} bind:this={btn} {disabled} onclick={toggle} aria-label="添加" aria-haspopup="menu" aria-expanded={open} data-tip={open ? undefined : '添加文件或图片'}>
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 2.8v10.4M2.8 8h10.4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
</button>
{#if open}
  <div class="attach-pop" role="menu" style:left="{pos.left}px" style:bottom="{pos.bottom}px">
    <button type="button" role="menuitem" onclick={pickFiles}>
      <svg width="17" height="17" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M13.2 7.4 8 12.6a3.3 3.3 0 0 1-4.7-4.7l5.6-5.6a2.2 2.2 0 0 1 3.1 3.1L6.4 11a1.1 1.1 0 0 1-1.6-1.6l5-5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg>
      <span class="t">添加文件或图片</span><kbd>{mac ? '⌘' : 'Ctrl+'}U</kbd>
    </button>
    {#if folder}
      <button type="button" role="menuitem" onclick={pickFolder}>
        <svg width="17" height="17" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M1.8 4.3a1.5 1.5 0 0 1 1.5-1.5h3l1.4 1.6h5a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5H3.3a1.5 1.5 0 0 1-1.5-1.5V4.3Z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /></svg>
        <span class="t">{folder.label}</span>
      </button>
    {/if}
    <p class="foot">也可以直接拖进来，或粘贴截图</p>
  </div>
{/if}

<style>
  .attach-plus { width: 30px; height: 30px; display: inline-flex; align-items: center; justify-content: center; flex: none; border: none; background: none; border-radius: 8px; color: var(--dim); cursor: pointer; transition: background-color .12s, color .12s; }
  .attach-plus:hover:not(:disabled), .attach-plus.on { background: #efede8; color: var(--text); }
  .attach-plus:disabled { opacity: .45; cursor: default; }
  .attach-pop { position: fixed; z-index: 60; width: 240px; padding: 6px; background: #fff; border: 1px solid var(--border2); border-radius: 14px; box-shadow: 0 10px 30px rgba(30, 28, 24, .12), 0 2px 6px rgba(30, 28, 24, .06); }
  .attach-pop button { width: 100%; display: flex; align-items: center; gap: 11px; padding: 8px 10px; border: none; background: none; border-radius: 9px; color: var(--text); font: inherit; font-size: 13.5px; text-align: left; cursor: pointer; }
  .attach-pop button:hover { background: #f4f3ef; }
  .attach-pop button svg { flex: none; color: var(--dim); }
  .attach-pop .t { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .attach-pop kbd { flex: none; font-family: inherit; font-size: 12px; color: var(--faint); }
  .attach-pop .foot { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; margin: 4px 10px 4px; padding-top: 7px; border-top: 1px solid var(--border); color: var(--faint); font-size: 11.5px; line-height: 1.5; }
</style>
