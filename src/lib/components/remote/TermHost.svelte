<script lang="ts">
  // 显示某台机器的终端：挂上时把它的终端挪进来（没有就新建并连上），卸下时挪回后台，不断开。
  // 尺寸变化（开关面板、拖分隔线、拉窗口）统一由 ResizeObserver 触发重排。
  import { mountTerm, unmountTerm, fitTerm } from '$lib/state/workbench.svelte.js';

  let { connId }: { connId: string } = $props();
  let host: HTMLDivElement | undefined = $state();

  $effect(() => {
    const el = host;
    const id = connId;
    if (!el || !id) return;
    mountTerm(id, el);
    let raf = 0;
    const ro = new ResizeObserver(() => {
      // 合并同一帧里的连续变化（拖拽时每像素都会触发）
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => fitTerm(id));
    });
    ro.observe(el);
    return () => { cancelAnimationFrame(raf); ro.disconnect(); unmountTerm(id, el); };
  });
</script>

<div class="term" bind:this={host}></div>

<style>
  .term { width: 100%; height: 100%; }
  .term :global(.term-inst) { width: 100%; height: 100%; }
</style>
