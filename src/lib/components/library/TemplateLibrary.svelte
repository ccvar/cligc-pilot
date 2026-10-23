<script lang="ts">
  import TemplateIcon from '$lib/components/icons/TemplateIcon.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { library, catOf, loadTemplates, previewTemplate, openUseTmpl, delTemplate, CAT_MINE } from '$lib/state/library.svelte.js';
  import { relTime } from '$lib/state/sidebar.svelte.js';
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header class="thread-head" data-tauri-drag-region onmousedown={startDrag}>
  <div class="th-info">
    <div class="th-line"><b class="th-title"><TemplateIcon size={16} /><span>模板库</span></b><small class="tmpl-hint">把做好的站点存成模板，之后引用它快速起新项目</small></div>
    {#if library.templates.length}
      <div class="tmpl-chips">
        <button class="tmpl-chip {library.tmplCat === '全部' ? 'on' : ''}" onclick={() => (library.tmplCat = '全部')}>{@render catIcon('全部')}全部<span class="tc-n">{library.templates.length}</span></button>
        {#each library.tmplCats as c (c)}
          <button class="tmpl-chip {library.tmplCat === c ? 'on' : ''}" onclick={() => (library.tmplCat = c)}>{@render catIcon(c)}{c}<span class="tc-n">{library.templates.filter((t) => catOf(t) === c).length}</span></button>
        {/each}
      </div>
    {/if}
  </div>
  <button class="icon-btn" onclick={loadTemplates} disabled={library.templatesLoading} title="刷新"><RefreshIcon spinning={library.templatesLoading} /></button>
</header>
<div class="thread">
  <div class="tmpl-inner">
    {#if library.templates.length === 0}
      <div class="sched-empty">
        <div class="cal-mark">🧩</div>
        <b>还没有模板</b>
        <p>在一个 Cloudflare 站点项目对话里点「存模板」，做得好的站就能沉淀下来复用。</p>
      </div>
    {:else}
      <div class="tmpl-grid">
        {#each library.tmplShown as t (t.slug)}
          <div class="tmpl-card" id="tmpl-{t.slug}">
            <div class="tmpl-thumb">
              {#if library.tmplHtml[t.slug]}
                <iframe class="tmpl-frame" srcdoc={library.tmplHtml[t.slug]} sandbox="allow-scripts" title={t.name} tabindex="-1" scrolling="no"></iframe>
              {:else}
                <span class="tmpl-letter">{t.name.slice(0, 1)}</span>
              {/if}
              <div class="tmpl-hover">
                <button class="tmpl-act" aria-label="预览真实页面" onclick={() => previewTemplate(t)} data-tip="预览真实页面"><svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M1.6 8s2.4-4.4 6.4-4.4S14.4 8 14.4 8s-2.4 4.4-6.4 4.4S1.6 8 1.6 8Z" stroke="currentColor" stroke-width="1.2" /><circle cx="8" cy="8" r="1.9" stroke="currentColor" stroke-width="1.2" /></svg></button>
                <button class="tmpl-act primary" aria-label="用它建新站" onclick={() => openUseTmpl(t)} data-tip="用它建新站"><svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M8 3.2v9.6M3.2 8h9.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg></button>
                <!-- 随附模板不给删（后端也拒）：按钮摆着只会让人点了吃一个错 -->
                {#if !t.builtin}
                  <button class="tmpl-act" aria-label="删除模板" onclick={() => delTemplate(t)} data-tip="删除模板"><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M3 4.6h10M6.4 4.6V3.3h3.2V4.6M4.6 4.6l.6 8.1h5.6l.6-8.1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
                {/if}
              </div>
            </div>
            <div class="tmpl-body">
              <b>{t.name}</b>
              <!-- 随附模板 created_at 恒 0，relTime 会渲染成 1970 的「1/1」——改显「内置」 -->
              <!-- 「N 页」只在多页时挂：模板的单位是「一个站」，几页是**引用前就该知道**的事 -->
              <div class="tmpl-sub"><span class="tmpl-desc">{t.desc || ''}</span><span class="tmpl-meta">{#if t.pages > 1}<span class="tmpl-pg">{t.pages} 页</span>{/if}{t.builtin ? '内置' : relTime(t.created_at)}</span></div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<!-- 只有「全部」「自建」带图标：这两个是**看什么范围**，后面那排是**按页型筛**。
     给每个都配图标反而把这层区别抹平了。 -->
{#snippet catIcon(c: string)}
  {#if c === '全部'}
    <svg class="tc-ic" width="11" height="11" viewBox="0 0 16 16" aria-hidden="true"><rect x="1.5" y="1.5" width="5.6" height="5.6" rx="1.3" fill="currentColor" /><rect x="8.9" y="1.5" width="5.6" height="5.6" rx="1.3" fill="currentColor" /><rect x="1.5" y="8.9" width="5.6" height="5.6" rx="1.3" fill="currentColor" /><rect x="8.9" y="8.9" width="5.6" height="5.6" rx="1.3" fill="currentColor" /></svg>
  {:else if c === CAT_MINE}
    <svg class="tc-ic" width="11" height="11" viewBox="0 0 16 16" fill="none" aria-hidden="true"><circle cx="8" cy="5" r="2.7" stroke="currentColor" stroke-width="1.5" /><path d="M2.9 14c0-2.7 2.3-4.4 5.1-4.4s5.1 1.7 5.1 4.4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
  {/if}
{/snippet}

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
  .sched-empty { text-align: center; color: var(--dim); padding: 12vh 24px; }
  .sched-empty .cal-mark { font-size: 34px; }
  .sched-empty b { display: block; margin: 12px 0 6px; color: var(--text); font-size: 16px; }
  .sched-empty p { margin: 0 auto; max-width: 380px; font-size: 13px; }
  /* 模板库单独一个更宽的容器：grid 本来就是 auto-fill（会自己长列），真正卡住它的是
     .sched-inner 的 720px —— 三列要 3*272+2*12+48 = 876px，720 差得远，所以窗口再宽也只有两列。
     日程页还得用窄栏（那是读列表的），所以不改 .sched-inner，另起一个。 */
  .tmpl-inner { max-width: 1180px; margin: 0 auto; padding: 18px 24px 24px; }
  .th-line { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  /* 提示挪到标题**同一行**了。`.th-info small` 那 2px 上间距是给「标题下面那行」准备的，
     这里得清掉；选择器带上 .tmpl-hint 才压得过它（它带元素选择器，分更高）。 */
  .th-info small.tmpl-hint { margin-top: 0; color: var(--faint); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* 负边距抵掉筹码自己的内边距 —— 那 8px/3px 是**点击热区**，不该算进视觉间距：
     - 左：不抵的话整排比上面的「模板库」缩进 8px，看着就是没对齐。
     - 下：不抵的话筹码文字离底边比标题离顶边多出 3px 的死空（上 6 / 下 10.5），上下就不一致。 */
  .tmpl-chips { display: flex; flex-wrap: wrap; gap: 2px; margin-top: 5px; margin-left: -8px; margin-bottom: -3px; }
  /* 无外框、无底：选中只靠**颜色加重**（--dim → --text）。
     ★ 故意不动 font-weight：字重一变宽度就变，点一下整排筹码会跟着挪位。 */
  .tmpl-chip { display: inline-flex; align-items: center; gap: 3px; padding: 3px 8px; border: 0; border-radius: 999px; background: transparent; color: var(--dim); font-size: 12px; cursor: pointer; }
  .tmpl-chip:hover { color: var(--text); }
  .tmpl-chip:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .tmpl-chip.on { color: var(--text); }
  .tc-ic { flex: none; margin-right: 1px; opacity: 0.7; }
  .tc-n { font-size: 10px; opacity: 0.55; font-variant-numeric: tabular-nums; }
  /* ★ 240 是**默认窗口能放下两列**倒推出来的，别再往上调：
     默认 820 宽、左栏 240 → 内容 580，减 48 内边距 = 532。两列 = 2*240+12 = 492 ✓。
     上一版写 272 要 556 > 532，就差这一点，默认窗口直接掉成**一列**。
     再宽才 3 列（≈1050）、4 列（≈1300，到 1180 封顶就不再加）。 */
  .tmpl-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; }
  /* 从全局搜索跳过来时闪一下：一屏十几张卡，跳到了也认不出是哪张 */
  @keyframes tmplFlash { from { box-shadow: 0 0 0 2px var(--accent); } to { box-shadow: 0 0 0 2px transparent; } }
  .tmpl-card:global(.tmpl-flash) { animation: tmplFlash 1.4s ease-out; }
  .tmpl-card { display: flex; flex-direction: column; background: var(--card); border: 1px solid var(--border2); border-radius: 12px; overflow: hidden; }
  .tmpl-thumb { position: relative; height: 158px; overflow: hidden; background: #fff; display: flex; align-items: center; justify-content: center; border-bottom: 1px solid var(--border); }
  .tmpl-frame { position: absolute; top: 0; left: 0; width: 1280px; height: 860px; border: 0; transform: scale(0.23); transform-origin: top left; pointer-events: none; background: #fff; }
  .tmpl-letter { font-size: 40px; font-weight: 600; color: #a03c2b; }
  .tmpl-hover { position: absolute; inset: 0; display: flex; align-items: flex-end; justify-content: center; gap: 8px; padding: 10px; background: linear-gradient(to top, rgba(0, 0, 0, .4), rgba(0, 0, 0, .05) 45%, transparent 70%); opacity: 0; transition: opacity .12s; }
  .tmpl-card:hover .tmpl-hover { opacity: 1; }
  .tmpl-act { position: relative; width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center; border: none; border-radius: 7px; background: rgba(255, 255, 255, .95); color: var(--text); cursor: pointer; box-shadow: 0 2px 8px rgba(0, 0, 0, .15); }
  .tmpl-act.primary { background: var(--accent); color: #fff; }
  .tmpl-act:hover { transform: translateY(-1px); }
  .tmpl-act svg { flex: none; width: 13px; height: 13px; }
  .tmpl-body { padding: 10px 12px 12px; flex: 1; }
  .tmpl-body b { font-size: 13px; }
  .tmpl-sub { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; margin-top: 4px; }
  .tmpl-desc { flex: 1; min-width: 0; font-size: 12px; color: var(--dim); line-height: 1.4; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tmpl-meta { flex: none; display: inline-flex; align-items: center; gap: 6px; font-size: 11px; color: #b3ada2; }
  .tmpl-pg { color: var(--dim); background: var(--border); border-radius: 4px; padding: 0 4px; line-height: 15px; }
  .icon-btn { background: none; border: none; cursor: pointer; padding: 6px; border-radius: 8px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .icon-btn:hover { background: #f1efe9; color: var(--text); }
  .icon-btn:disabled { opacity: .55; cursor: default; }
</style>
