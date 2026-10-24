<script lang="ts">
  // 需要我处理的事：AI 写在 .pilot/todo.json 里，这里逐条处理——
  // 填值交给 AI 写进站点、交给 AI 去做、或标成已处理 / 不需要（只记在 Pilot 里）。
  import Icon from './Icon.svelte';
  import type { Site, SiteTodo } from '$lib/types';
  import { app } from '$lib/state/app.svelte.js';
  import { submitTodo, delegateTodo, markTodo } from '$lib/state/siteMake.svelte.js';

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
      {#if t.where}<p class="where mono">{t.where}</p>{/if}
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
        {#if t.kind === 'input' && t.inputs.length}
          <button class="btn tiny primary" disabled={busy || !filled(t)} onclick={() => submitTodo(site, t, Object.fromEntries(t.inputs.map((i) => [i.key, val(t, i.key)])))}><Icon name="wand" size={11} />交给 AI 写进站点</button>
        {:else}
          <button class="btn tiny" disabled={busy} onclick={() => delegateTodo(site, t)}><Icon name="wand" size={11} />{t.kind === 'confirm' ? '确认，交给 AI' : '交给 AI 处理'}</button>
        {/if}
        <span class="grow"></span>
        <button class="link-btn" onclick={() => markTodo(site, t)} data-tip="自己处理好了，或这件事不需要做（只在 Pilot 里隐藏，不改站点文件）">已处理 / 不需要</button>
      </div>
    </li>
  {/each}
</ul>
{#if busy}<p class="busy-note">AI 正在这个站点里干活，等它这一轮结束再交新的事。</p>{/if}

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
  .busy-note { margin: 6px 0 0; font-size: 11.5px; color: var(--faint); }
</style>
