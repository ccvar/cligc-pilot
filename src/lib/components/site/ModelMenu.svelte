<script lang="ts">
  // 「待处理」里换个模型交给 AI 的弹出菜单：拆分按钮的 ▾、魔杖上右键都用它。
  // 两步：先点模型，下面按这个模型能用的档位选推理强度，再点底部按钮开始（或设为按钮用的模型）。
  // 只用这一次（不改默认）；勾上「同时设为默认」才改默认模型。位置用 fixed（坐标给锚点矩形或鼠标位置）。
  import BrainIcon from '$lib/BrainIcon.svelte';
  import { batchOptions } from '$lib/state/fleet.svelte.js';
  import { stopsFor, effortValue } from '$lib/effortStops';

  let { at, current, currentEffort = '', onpick, onclose, pickOnly = false, note = '' }: {
    /** 锚点：按钮的矩形，或右键时的鼠标位置 */
    at: { left: number; top: number; bottom: number; right: number };
    /** 默认模型（标「默认」，打开时先选中它） */
    current: string;
    /** 默认模型现在的推理强度（打开时先选中这一档） */
    currentEffort?: string;
    onpick: (model: string, makeDefault: boolean, effort: string) => void;
    onclose: () => void;
    /** 只换按钮用的模型、不马上开始（批量按钮的 ▾）：不显示「同时设为默认」，选了就是新的默认 */
    pickOnly?: boolean;
    /** 底部换成一句说明（不显示「同时设为默认」），比如站点任务队列里「只用这一次」 */
    note?: string;
  } = $props();

  const opts = $derived(batchOptions());
  // 选中的模型和推理强度：打开时先选默认模型（能用的话）和它现在的档位
  // svelte-ignore state_referenced_locally
  let sel = $state(current);
  // svelte-ignore state_referenced_locally
  let eff = $state(currentEffort);
  const selOpt = $derived(opts.find((o) => o.value === sel && !o.disabled));
  const selBrain = $derived(sel.split('::')[0]);
  const stops = $derived(selOpt ? stopsFor(selBrain, sel.split('::').slice(1).join('::'), selOpt.apiProtocol) : []);
  const effStop = $derived(selBrain.startsWith('api:') ? eff.replace(/^api:/, '') : eff);
  function choose(v: string) {
    if (v === sel) return;
    sel = v;
    // 换模型：新模型没有这一档就回到默认
    const o = opts.find((x) => x.value === v);
    const st = o ? stopsFor(v.split('::')[0], v.split('::').slice(1).join('::'), o.apiProtocol) : [];
    if (!st.some((x) => x.v === effStop)) eff = '';
  }
  function go() { if (selOpt) onpick(sel, pickOnly || (!note && makeDefault), eff); }
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
  <input class="mm-q" bind:this={searchEl} bind:value={query} placeholder={pickOnly ? '交给哪个模型…' : '换个模型处理…'} aria-label="搜索模型" />
  <div class="mm-list">
    {#each shown as o (o.value)}
      <button class="mopt" class:on={o.value === sel} disabled={o.disabled} role="menuitemradio" aria-checked={o.value === sel} onclick={() => choose(o.value)} ondblclick={go}>
        {#if o.icon}<BrainIcon brain={o.icon} size={15} />{/if}
        <span class="mm-t"><b>{o.short}{#if o.short !== o.label}<em> · {o.label}</em>{/if}</b>{#if o.sub}<small>{o.sub}</small>{/if}</span>
        {#if o.value === current}<span class="mm-def">默认</span>{/if}
      </button>
    {/each}
    {#if !shown.length}<p class="mm-none">没有匹配的模型</p>{/if}
  </div>
  <!-- 推理强度：按选中模型支持的档位显示（和对话里的模型面板同一套） -->
  {#if selOpt && stops.length > 1}
    <div class="mm-eff">
      <div class="mm-eff-h">推理强度<span>{stops.find((x) => x.v === effStop)?.d ?? ''}</span></div>
      <div class="mm-seg" role="radiogroup" aria-label="推理强度">
        {#each stops as st (st.v)}<button role="radio" aria-checked={st.v === effStop} class:on={st.v === effStop} onclick={() => (eff = effortValue(selBrain, st.v))} data-tip={st.d}>{st.l}</button>{/each}
      </div>
    </div>
  {/if}
  <div class="mm-go">
    {#if pickOnly || note}<span class="mm-note">{note || '只换按钮上的模型，点按钮才开始'}</span>
    {:else}<label class="mm-note"><input type="checkbox" bind:checked={makeDefault} />同时设为默认</label>{/if}
    <button class="mm-btn" disabled={!selOpt} onclick={go}>{pickOnly ? '用这个' : '开始'}</button>
  </div>
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
  .mopt.on { background: #f1efe9; }
  .mm-eff { margin-top: 4px; padding: 8px 6px 4px; border-top: 1px solid var(--border, #ecebe6); }
  .mm-eff-h { display: flex; justify-content: space-between; gap: 8px; margin-bottom: 6px; font-size: 11.5px; color: var(--dim, #6f6b62); }
  .mm-eff-h span { color: var(--faint, #a29d93); }
  .mm-seg { display: flex; padding: 2px; border-radius: 8px; background: #f1efe9; }
  .mm-seg button { flex: 1; padding: 4px 0; border: 0; border-radius: 6px; background: none; font: inherit; font-size: 11.5px; color: var(--dim, #6f6b62); cursor: pointer; white-space: nowrap; }
  .mm-seg button.on { background: #fff; color: var(--text, #26241f); font-weight: 550; box-shadow: 0 1px 2px rgba(0, 0, 0, .08); }
  .mm-go { display: flex; align-items: center; gap: 8px; margin-top: 4px; padding: 7px 6px 3px; border-top: 1px solid var(--border, #ecebe6); }
  .mm-note { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--faint, #a29d93); }
  label.mm-note { color: var(--dim, #6f6b62); cursor: pointer; }
  .mm-note input { margin: 0; accent-color: var(--accent, #33302a); }
  .mm-btn { flex: none; padding: 4px 12px; border: 0; border-radius: 7px; background: var(--accent, #33302a); color: #fff; font: inherit; font-size: 12px; cursor: pointer; }
  .mm-btn:disabled { opacity: .4; cursor: default; }
</style>
