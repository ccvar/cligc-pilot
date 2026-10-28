<script lang="ts">
  // 「待处理」里换个模型交给 AI 的弹出菜单：拆分按钮的 ▾、魔杖上右键都用它。
  // 选一个就用它处理这一次（不改默认）；勾上「同时设为默认」才改默认模型。位置用 fixed（坐标给锚点矩形或鼠标位置）。
  import BrainIcon from '$lib/BrainIcon.svelte';
  import { batchOptions } from '$lib/state/fleet.svelte.js';

  let { at, current, onpick, onclose }: {
    /** 锚点：按钮的矩形，或右键时的鼠标位置 */
    at: { left: number; top: number; bottom: number; right: number };
    /** 默认模型（打勾） */
    current: string;
    onpick: (model: string, makeDefault: boolean) => void;
    onclose: () => void;
  } = $props();

  const opts = $derived(batchOptions());
  let query = $state('');
  let makeDefault = $state(false);
  let el: HTMLDivElement | undefined = $state();
  let searchEl: HTMLInputElement | undefined = $state();
  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? opts.filter((o) => `${o.label} ${o.sub ?? ''} ${o.value}`.toLowerCase().includes(q)) : opts;
  });
  // 靠右对齐锚点，下面放不下就往上翻
  const W = 260;
  const style = $derived.by(() => {
    const below = window.innerHeight - at.bottom - 12;
    const up = below < 240 && at.top - 12 > below;
    const maxH = Math.min(340, Math.max(120, up ? at.top - 12 : below));
    const left = Math.max(12, Math.min(at.right - W, window.innerWidth - W - 12));
    return `${up ? `bottom:${Math.round(window.innerHeight - at.top + 6)}px` : `top:${Math.round(at.bottom + 6)}px`};left:${Math.round(left)}px;width:${W}px;max-height:${Math.round(maxH)}px`;
  });

  $effect(() => {
    requestAnimationFrame(() => searchEl?.focus());
    // 右键按下不算「点外面」：魔杖上右键打开菜单时，有的环境 mousedown 会排在 contextmenu 后面，一开就被关掉
    const out = (e: MouseEvent) => { if (e.button !== 2 && el && !el.contains(e.target as Node)) onclose(); };
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onclose(); } };
    const scroll = (e: Event) => { if (el && !el.contains(e.target as Node)) onclose(); };
    document.addEventListener('mousedown', out, true);
    document.addEventListener('keydown', key, true);
    window.addEventListener('scroll', scroll, true);
    window.addEventListener('resize', onclose);
    return () => {
      document.removeEventListener('mousedown', out, true);
      document.removeEventListener('keydown', key, true);
      window.removeEventListener('scroll', scroll, true);
      window.removeEventListener('resize', onclose);
    };
  });
</script>

<div class="mm" bind:this={el} {style} role="menu">
  <input class="mm-q" bind:this={searchEl} bind:value={query} placeholder="换个模型处理…" aria-label="搜索模型" />
  <div class="mm-list">
    {#each shown as o (o.value)}
      <button class="mopt" disabled={o.disabled} role="menuitem" onclick={() => onpick(o.value, makeDefault)}>
        {#if o.icon}<BrainIcon brain={o.icon} size={15} />{/if}
        <span class="mm-t"><b>{o.short}{#if o.short !== o.label}<em> · {o.label}</em>{/if}</b>{#if o.sub}<small>{o.sub}</small>{/if}</span>
        {#if o.value === current}<span class="mm-def">默认</span>{/if}
      </button>
    {/each}
    {#if !shown.length}<p class="mm-none">没有匹配的模型</p>{/if}
  </div>
  <label class="mm-foot"><input type="checkbox" bind:checked={makeDefault} />同时设为默认模型</label>
</div>

<style>
  .mm { position: fixed; z-index: 95; display: flex; flex-direction: column; padding: 4px; background: #fff; border: 1px solid var(--border2, #e1dfd8); border-radius: 10px; box-shadow: 0 8px 28px rgba(0, 0, 0, 0.12); font-size: 12.5px; }
  .mm-q { margin: 2px 2px 4px; padding: 5px 8px; border: 1px solid var(--border, #ecebe6); border-radius: 7px; font: inherit; font-size: 12.5px; outline: none; }
  .mm-q:focus { border-color: var(--border2, #cfccc2); }
  .mm-list { flex: 1; min-height: 0; overflow-y: auto; }
  .mopt { display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 8px; border: 0; border-radius: 7px; background: none; font: inherit; color: var(--text, #26241f); text-align: left; cursor: pointer; }
  .mopt:hover:not(:disabled), .mopt:focus-visible { background: #f3f2ee; outline: none; }
  .mopt:disabled { opacity: 0.45; cursor: default; }
  .mopt :global(.bi) { flex: none; }
  .mm-t { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .mm-t b { font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .mm-t em { font-style: normal; color: var(--faint, #a29d93); font-weight: 400; }
  .mm-t small { color: var(--faint, #a29d93); font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .mm-def { flex: none; font-size: 10.5px; color: var(--dim, #6f6b62); padding: 1px 6px; border-radius: 6px; background: #f1efe9; }
  .mm-none { margin: 8px; color: var(--faint, #a29d93); }
  .mm-foot { display: flex; align-items: center; gap: 6px; margin-top: 4px; padding: 7px 8px 4px; border-top: 1px solid var(--border, #ecebe6); color: var(--dim, #6f6b62); cursor: pointer; }
  .mm-foot input { margin: 0; accent-color: var(--accent, #33302a); }
</style>
