<script lang="ts">
  // 需要我处理的事：AI 写在 .pilot/todo.json 里。填值、勾选要 AI 做的，最后一起交给 AI（一轮处理完）；
  // 也可以单条标成已处理 / 不需要（只记在 Pilot 里）。
  import Icon from './Icon.svelte';
  import type { Site, SiteTodo } from '$lib/types';
  import { app } from '$lib/state/app.svelte.js';
  import { submitTodos, markTodo } from '$lib/state/siteMake.svelte.js';

  let { site, todos }: { site: Site; todos: SiteTodo[] } = $props();
  // 影响站点目标的排前面
  const list = $derived([...todos].sort((a, b) => Number(b.affects_goal) - Number(a.affects_goal)));
  const busy = $derived(app.convos.some((c) => c.conn_id === `site:${site.id}` && c.status === 'running'));
  let values = $state<Record<string, Record<string, string>>>({});
  function val(t: SiteTodo, key: string): string {
    return values[t.id]?.[key] ?? t.inputs.find((i) => i.key === key)?.value ?? '';
  }
  function setVal(t: SiteTodo, key: string, v: string) { values[t.id] = { ...(values[t.id] ?? {}), [key]: v }; }
  function filled(t: SiteTodo) { return t.inputs.some((i) => val(t, i.key).trim() && val(t, i.key) !== i.value); }
  // 不用填值的（交给 AI 做 / 确认 / 建议）：勾上才算进这一批
  let picked = $state<Set<string>>(new Set());
  function pick(t: SiteTodo) { const n = new Set(picked); if (n.has(t.id)) n.delete(t.id); else n.add(t.id); picked = n; }
  const needsInput = (t: SiteTodo) => t.kind === 'input' && t.inputs.length > 0;
  const batch = $derived(list.filter((t) => (needsInput(t) ? filled(t) : picked.has(t.id))));
  let sending = $state(false);
  async function send() {
    sending = true;
    try {
      await submitTodos(site, batch.map((t) => ({ todo: t, values: needsInput(t) ? Object.fromEntries(t.inputs.map((i) => [i.key, val(t, i.key)])) : {} })));
      values = {}; picked = new Set();
    } finally { sending = false; }
  }
  const KIND: Record<string, string> = { input: '需要你提供', ai: '交给 AI 就行', confirm: '需要你确认', suggest: '建议' };
</script>

<ul class="todos">
  {#each list as t (t.id)}
    <li class:goal={t.affects_goal}>
      <div class="t-head">
        <b>{t.title}</b>
        {#if t.affects_goal}<span class="tag goal">影响站点目标</span>{:else}<span class="tag">{KIND[t.kind] ?? '待处理'}</span>{/if}
      </div>
      {#if t.why}<p class="why">{t.why}</p>{/if}
      {#if t.where}<p class="where"><span data-tip={t.where}>改动的位置</span></p>{/if}
      {#if t.kind === 'input' && t.inputs.length}
        <div class="fields">
          {#each t.inputs as i (i.key)}
            <label class="fld">
              <span>{i.label || i.key}</span>
              <input class="tin" value={val(t, i.key)} placeholder={i.placeholder || ''} spellcheck="false" autocapitalize="off" oninput={(e) => setVal(t, i.key, (e.currentTarget as HTMLInputElement).value)} />
            </label>
          {/each}
        </div>
      {/if}
      <div class="t-acts">
        {#if !needsInput(t)}
          <button class="pick" class:on={picked.has(t.id)} aria-pressed={picked.has(t.id)} onclick={() => pick(t)}>
            <span class="box">{#if picked.has(t.id)}<Icon name="check" size={10} />{/if}</span>{t.kind === 'confirm' ? '确认，交给 AI' : '交给 AI'}
          </button>
        {:else if filled(t)}
          <span class="ready"><Icon name="check" size={11} />已填，会一起交给 AI</span>
        {/if}
        <span class="grow"></span>
        <button class="link-btn" onclick={() => markTodo(site, t)} data-tip="自己处理好了，或这件事不需要做（只在 Pilot 里隐藏，不改站点文件）">已处理 / 不需要</button>
      </div>
    </li>
  {/each}
</ul>
<!-- 选了东西才出现底栏（贴在列表底部跟着滚）；什么都没选时只留一行说明，不盖住别的内容 -->
{#if batch.length || sending}
  <div class="batch">
    <span>{#if busy}AI 正在这个站点里干活，等它这一轮结束再交。{:else}这一批 {batch.length} 项：填好的值和勾选的事会合成一条消息，AI 一轮处理完。{/if}</span>
    <button class="btn small primary" disabled={busy || sending || !batch.length} onclick={send}><Icon name="wand" size={12} />{sending ? '交给 AI…' : `一起交给 AI（${batch.length} 项）`}</button>
  </div>
{:else}
  <p class="batch-hint">填好要补的值、勾选要 AI 做的事，下面会出现「一起交给 AI」。</p>
{/if}

<style>
  .todos { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .todos > li { border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; display: flex; flex-direction: column; gap: 5px; }
  .todos > li.goal { border-color: #f0e4c4; background: #fdfaf2; }
  .t-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .t-head b { font-size: 13px; font-weight: 600; color: var(--text); }
  .tag { font-size: 10.5px; padding: 1px 7px; border-radius: 999px; background: #f1efe9; color: var(--dim); }
  .tag.goal { background: #faf1d8; color: #8a5a08; font-weight: 500; }
  .why { margin: 0; font-size: 12px; color: var(--dim); line-height: 1.5; }
  .where { margin: 0; font-size: 11px; color: var(--faint); }
  .fields { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 8px; margin-top: 2px; }
  .fld { display: flex; flex-direction: column; gap: 3px; font-size: 11.5px; color: var(--dim); }
  .fld .tin { font-size: 12.5px; }
  .t-acts { display: flex; align-items: center; gap: 8px; margin-top: 2px; }
  .t-acts .link-btn { font-size: 12px; color: var(--faint); }
  .t-acts .link-btn:hover { color: var(--text); }
  .grow { flex: 1; }
  .pick { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 9px 0 6px; border: 1px solid var(--border); border-radius: 7px; background: none; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; transition: border-color .12s, background-color .12s, color .12s; }
  .pick:hover { border-color: var(--border2); color: var(--text); }
  .pick.on { border-color: var(--text); color: var(--text); background: #f7f6f2; }
  .box { width: 13px; height: 13px; border-radius: 4px; border: 1.5px solid var(--border2); box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; color: #fff; }
  .pick.on .box { background: var(--text); border-color: var(--text); }
  .ready { display: inline-flex; align-items: center; gap: 4px; font-size: 12px; color: var(--ok); }
  .batch { position: sticky; bottom: 0; z-index: 2; display: flex; align-items: center; gap: 12px; margin-top: 10px; padding: 8px 8px 8px 14px; border-radius: 10px; background: var(--card, #fff); border: 1px solid var(--border2); box-shadow: 0 6px 18px rgba(30, 25, 15, .08); font-size: 12px; color: var(--dim); }
  .batch-hint { margin: 8px 0 0; font-size: 12px; color: var(--faint); }
  .batch span { flex: 1; min-width: 0; line-height: 1.5; }
  .batch .btn { flex: none; }
</style>
