<script lang="ts">
  // 设置里「连接」的一组：组头（折叠、图标、名字、数量、附加按钮、+）+ 一行一条。
  // Cloudflare / GitHub / 远程机器、Google / Bing、安卓手机、邮箱都用它，长得一样。
  // 行用 .conn-row（.cr-name / .cr-sub / .cr-gear），样式在这里统一（:global，行写在各自的组件里）。
  import type { Snippet } from 'svelte';
  import PlusIcon from '$lib/components/icons/PlusIcon.svelte';
  import ConnectionMark from '$lib/components/icons/ConnectionMark.svelte';
  import { isFolded, setFolded } from '$lib/state/connFold.svelte.js';

  let { kind, label, n = 0, add, addTip, icon, actions, children }: {
    /** 折叠状态按它记 */
    kind: string;
    label: string;
    /** 组头的数量；0 不显示 */
    n?: number;
    add?: () => void;
    addTip?: string;
    /** 不给就按 kind 用 ConnectionMark */
    icon?: Snippet;
    /** 组头右侧、+ 前面的附加按钮 */
    actions?: Snippet;
    children: Snippet;
  } = $props();
  const folded = $derived(isFolded(kind, n));
</script>

<div class="cg">
  <div class="cg-head">
    <button class="cg-toggle" onclick={() => setFolded(kind, !folded)} aria-expanded={!folded}>
      <svg class="cg-chev" class:folded width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
      <span class="cg-ic">{#if icon}{@render icon()}{:else}<ConnectionMark {kind} size={14} />{/if}</span>
      <span class="cg-label">{label}</span>{#if n}<span class="cg-n">{n}</span>{/if}
    </button>
    {#if actions}<span class="cg-acts">{@render actions()}</span>{/if}
    {#if add}<button class="cg-add" data-tip={addTip ?? `添加${label}`} aria-label={addTip ?? `添加${label}`} onclick={() => add?.()}><PlusIcon /></button>{/if}
  </div>
  {#if !folded}
    <div class="cg-list">{@render children()}</div>
  {/if}
</div>

<style>
  .cg { border: 1px solid var(--border); border-radius: 11px; background: #fff; overflow: hidden; }
  .cg-head { display: flex; align-items: center; gap: 2px; padding: 3px 6px 3px 4px; }
  .cg-toggle { flex: 1; min-width: 0; display: flex; align-items: center; gap: 7px; border: none; background: none; padding: 5px 6px; border-radius: 7px; font: inherit; font-size: 13px; color: var(--text); cursor: pointer; text-align: left; }
  .cg-toggle:hover { background: #f5f4f1; }
  .cg-chev { flex: none; color: var(--faint); transition: transform .15s; }
  .cg-chev.folded { transform: rotate(-90deg); }
  .cg-ic { flex: none; display: inline-flex; width: 14px; justify-content: center; }
  .cg-label { font-weight: 600; }
  .cg-n { font-size: 11.5px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .cg-acts { flex: none; display: inline-flex; align-items: center; gap: 2px; }
  .cg-add { flex: none; background: none; border: none; cursor: pointer; padding: 4px; border-radius: 6px; color: var(--faint); display: inline-flex; align-items: center; justify-content: center; }
  .cg-add:hover { background: #f1efe9; color: var(--text); }
  .cg-list { border-top: 1px solid var(--border); }
  /* 组头里的小图标按钮（刷新、OAuth 客户端…） */
  .cg-acts :global(.cg-btn) { flex: none; background: none; border: none; cursor: pointer; padding: 4px; border-radius: 6px; color: var(--faint); display: inline-flex; align-items: center; justify-content: center; }
  .cg-acts :global(.cg-btn:hover) { background: #f1efe9; color: var(--text); }
  /* 一行一条：名字左对齐到组头图标后面（31px） */
  .cg-list :global(.conn-row) { display: flex; align-items: center; gap: 8px; min-height: 36px; padding: 4px 6px 4px 31px; cursor: pointer; transition: background .12s; }
  .cg-list :global(.conn-row.static) { cursor: default; }
  .cg-list :global(.conn-row + .conn-row) { border-top: 1px solid #f1f0ec; }
  .cg-list :global(.conn-row:hover) { background: #faf9f6; }
  .cg-list :global(.conn-row.on) { background: #f5f4f1; }
  .cg-list :global(.cr-name) { flex: 0 1 auto; min-width: 0; font-size: 13px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cg-list :global(.cr-sub) { flex: 1 1 0; min-width: 0; font-size: 11.5px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cg-list :global(.cr-gear) { flex: none; margin-left: auto; opacity: .55; }
  .cg-list :global(.conn-row:hover .cr-gear), .cg-list :global(.cr-gear.active) { opacity: 1; }
</style>
