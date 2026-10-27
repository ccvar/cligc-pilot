<script lang="ts">
  import PromptIcon from '$lib/components/icons/PromptIcon.svelte';
  import PlusIcon from '$lib/components/icons/PlusIcon.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { openNewPrompt, library, usePrompt, copyPrompt, openEditPrompt, deletePrompt } from '$lib/state/library.svelte.js';
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header class="thread-head" data-tauri-drag-region onmousedown={startDrag}>
  <div class="th-info"><b class="th-title"><PromptIcon size={16} /><span>提示词库</span></b><small>常用建站需求，点「用它建站」填进对话框就能开始；也能存自己的。</small></div>
  <button class="btn soft bare" onclick={openNewPrompt}><PlusIcon />新增提示词</button>
</header>
<div class="thread">
  <div class="sched-inner">
    <div class="prompt-grid">
      {#each library.allPrompts as p (p.id)}
        <div class="prompt-card">
          <div class="prompt-top">
            <b class="prompt-title">{p.title}</b>
            {#if p.builtin}<span class="prompt-tag">预设</span>{/if}
          </div>
          <p class="prompt-body">{p.body}</p>
          <div class="prompt-acts">
            <button class="prompt-act" aria-label="用它建站" data-tip="用它建站" onclick={() => usePrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M8 3.2v9.6M3.2 8h9.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg></button>
            <button class="prompt-act" aria-label="复制提示词" data-tip="复制" onclick={() => copyPrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><rect x="5.5" y="5.5" width="7.6" height="8" rx="1.4" stroke="currentColor" stroke-width="1.2" /><path d="M10.6 5.5V4.2A1.4 1.4 0 0 0 9.2 2.8H4A1.4 1.4 0 0 0 2.6 4.2v5.2A1.4 1.4 0 0 0 4 10.8h1.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg></button>
            {#if !p.builtin}
              <button class="prompt-act" aria-label="编辑提示词" data-tip="编辑" onclick={() => openEditPrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M10.8 2.9l2.3 2.3M11.4 2.3l2.3 2.3-8 8-3 .7.7-3 8-8z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg></button>
              <button class="prompt-act" aria-label="删除提示词" data-tip="删除" onclick={() => deletePrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M3 4.6h10M6.4 4.6V3.3h3.2V4.6M4.6 4.6l.6 8.1h5.6l.6-8.1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  :global(.pane) .thread{min-height:0;overscroll-behavior:contain}
  /* ---- 线程 ---- */
  /* ★ 标题要和左上角那排工具图标在同一条水平线上。
     `.win-tools` 是 fixed / top:0 / height:30px + align-items:center → **图标中心恒在 15px**
     （和 .thread-head.slim 那条注释是同一个基准）。标题行高 20.3 → 上内边距取 5px，
     标题中心落在 15.15，对上。原来 13px 时中心在 23.1 —— **每个页面都低 8px**。
     ★ 上边距被「对齐那条 fixed 图标带」锁死＝5px，所以**下边距只能跟着收成 5px**，
     不然上下就不对称（原来 5/13，下面明显空一截）。头因此比以前矮 8px，这是这条对齐的代价。
     ★ align-items 必须 flex-start，不能用 center：th-info 比右侧按钮（28px）矮时
     （例如只有标题、没有副标题的头），center 会让 th-info 跟着按钮居中，标题又掉到 26.8。 */
  /* ★ 和内容区之间那条缝：原来是一条 1px 实线直接把两块切开，太硬。**两个状态分开治**：
     ① 静止态（治「硬」）：页头与内容**同底不加色**——曾试过一层极淡暖底做色阶差，
        但那条色带在页面顶上像贴了块补丁（用户点名拿掉）；现在静止态只留几乎看不见的
        底线。**刻意不上 backdrop-filter**：实测它只会把卡片的红字糊成粉影漂在标题背后
        （是噪点不是层次），还要赔上脱流 + ResizeObserver + 三处 scrollIntoView 的静默回归。
     ② 滚动态（治「切」）：见 .elevated —— 内容真滑到底下时才淡入柔和阴影、把线淡掉。
        滚到顶时页头底下是**空的**，那时打阴影＝给不存在的遮挡关系编故事
        （同 .term-wrap.with-chat 那条「独占/无对话时不加，保持齐边」的分寸）。
     z-index：阴影得压在 .thread 之上 —— .thread 是后面的兄弟，默认会盖住它。 */
  .thread-head { flex: none; padding: 5px 24px; border-bottom: 1px solid rgba(30, 25, 15, .05); position: relative; z-index: 1; transition: border-bottom-color .18s ease; display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  /* ★ 阴影用伪元素而不是 box-shadow：box-shadow 天然四周都有 —— `0 6px 12px -6px` 水平方向是
     「收缩 6 再模糊 6」，**正好到边**，于是左右两端各留一段 6px 渐弱，看着就是两个圆头戳在窗沿上。
     负 spread 再加也没用：加到不露头，朝下的阴影也就没了。
     伪元素 left:0/right:0 精确铺满、只朝下，左右**零溢出、零圆头**。 */
  .thread-head::after {
    content: ''; position: absolute; left: 0; right: 0; top: 100%; height: 6px;
    background: linear-gradient(rgba(20, 16, 13, .03), rgba(20, 16, 13, 0)); /* 原 .13 → .05 → .03：用户连续点名太重，压到将将可感；再嫌重就整个去掉 */
    opacity: 0; transition: opacity .18s ease; pointer-events: none;
  }
  /* 只在内容真滑到页头底下时才抬升。border 只改 color 不改宽度 —— 改宽度会让下面整块跳 1px。 */
  .thread-head:global(.elevated) { border-bottom-color: transparent; }
  .thread-head:global(.elevated)::after { opacity: 1; }
  /* 侧栏收起：红绿灯 + 悬浮的折叠/搜索钮压在内容区左上，页头统一左让位（全部视图受益）。
     发现更新时顶部临时增加更新入口，页头再让出一个按钮宽度，避免压住页面标题图标。 */
  :global(.app.rail-collapsed) .thread-head { padding-left: 140px; }
  :global(.app.rail-collapsed.fs) .thread-head, :global(.app.rail-collapsed.win) .thread-head { padding-left: 100px; }
  :global(.app.rail-collapsed.top-update-visible) .thread-head { padding-left: 167px; }
  :global(.app.rail-collapsed.top-update-visible.fs) .thread-head,
  :global(.app.rail-collapsed.top-update-visible.win) .thread-head { padding-left: 127px; }
  .th-info { min-width: 0; flex: 1 1 auto; overflow: hidden; }
  .th-info b { display: block; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 15px; line-height: 1.35; }
  .th-info b.th-title { display: inline-flex; align-items: center; gap: 7px; }
  .th-title :global(svg) { flex: none; color: var(--dim); }
  .th-info small { display: block; min-width: 0; overflow: hidden; color: var(--dim); font-size: 12px; margin-top: 2px; text-overflow: ellipsis; white-space: nowrap; }
  .th-info small :global(svg), .th-info small :global(img) { vertical-align: -2px; }
  .thread { flex: 1; overflow-y: auto; }
  /* 排期视图 */
  .sched-inner { max-width: 720px; margin: 0 auto; padding: 18px 24px 24px; }
  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1px solid var(--border2); border-radius: 8px; padding: 6.5px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
  .btn:hover { background: #f6f5f1; }
  .btn:disabled { opacity: .5; cursor: default; }
  .btn.soft { display: inline-flex; align-items: center; gap: 6px; padding: 3px 12px; background: #fff; color: var(--text); border: 1px solid var(--border2); border-radius: 9px; font-weight: 500; }
  .btn.soft:hover { background: var(--rail); border-color: #cfccc2; }
  .btn.soft :global(.plus-ic) { color: var(--dim); }
  /* 无边框动作按钮（头部次要动作）：去边框去底，仅 hover 有淡底 */
  .btn.bare { border: none; background: none; }
  .btn.bare:hover { background: var(--rail); border-color: transparent; }
  /* 提示词库 */
  .prompt-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(272px, 1fr)); gap: 12px; }
  .prompt-card { display: flex; flex-direction: column; background: var(--card); border: 1px solid var(--border2); border-radius: 12px; padding: 13px 14px 11px; }
  .prompt-top { display: flex; align-items: center; gap: 7px; margin-bottom: 6px; }
  .prompt-title { font-size: 13.5px; font-weight: 600; color: var(--text); flex: 1; min-width: 0; }
  .prompt-tag { flex: none; font-size: 10px; font-weight: 600; color: var(--dim); background: #e7e4dd; padding: 1px 6px; border-radius: 5px; }
  .prompt-body { margin: 0 0 11px; font-size: 12.5px; line-height: 1.5; color: var(--dim); display: -webkit-box; -webkit-line-clamp: 5; line-clamp: 5; -webkit-box-orient: vertical; overflow: hidden; }
  .prompt-acts { display: flex; align-items: center; gap: 4px; margin-top: auto; }
  .prompt-act { position: relative; width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: none; border-radius: 8px; background: none; color: var(--dim); cursor: pointer; }
  .prompt-act:hover { color: var(--text); background: var(--rail); }
  .prompt-act svg { flex: none; }
</style>
