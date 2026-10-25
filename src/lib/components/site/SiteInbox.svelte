<script lang="ts">
  // 站点概览里的「表单和评论」：表单提交和新评论都存在站点自己的 D1 里，这里直接读；评论在这里审核。
  import './site.css';
  import Icon from './Icon.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import type { Site } from '$lib/types';
  import { ago, full } from './fmt';
  import { dyn, loadInbox, inboxAct } from '$lib/state/dynamic.svelte.js';

  let { site }: { site: Site } = $props();
  const box = $derived(dyn.inbox[site.id]);
  const err = $derived(dyn.inboxErr[site.id] ?? '');
  const loading = $derived(!!dyn.loading[site.id]);
  const conf = $derived(dyn.conf[site.id]);
  let tab = $state<'sub' | 'com'>('sub');
  let shown = $state(10);
  $effect(() => { void site.id; void loadInbox(site.id); });
  // 有待审评论、没有未读提交时，默认看评论
  let picked = false;
  $effect(() => { if (box && !picked) { picked = true; if (!box.unread && box.pending) tab = 'com'; } });
  const subs = $derived(box?.submissions ?? []);
  const coms = $derived(box?.comments ?? []);
  const sec = (ms: number) => Math.floor(ms / 1000);
</script>

<div class="card ibx">
  <h3>
    <Icon name="inbox" />表单和评论
    {#if box?.unread}<span class="pill warn">{box.unread} 条未读</span>{/if}
    {#if box?.pending}<span class="pill warn">{box.pending} 条评论待审</span>{/if}
    <span class="grow"></span>
    <button class="ic-btn" onclick={() => loadInbox(site.id)} disabled={loading} aria-label="刷新" data-tip="重新读取"><RefreshIcon spinning={loading} /></button>
  </h3>
  {#if conf?.notes?.length}<ul class="notes">{#each conf.notes as n (n)}<li><Icon name="info" size={12} />{n}</li>{/each}</ul>{/if}
  {#if err}
    <div class="err-note">{err}</div>
  {:else if !box}
    <div class="bars"><i style="width:58%"></i><i style="width:34%"></i></div>
  {:else}
    <div class="row head">
      <div class="seg" role="tablist">
        <button class:on={tab === 'sub'} onclick={() => { tab = 'sub'; shown = 10; }}>表单提交 {subs.length || ''}</button>
        <button class:on={tab === 'com'} onclick={() => { tab = 'com'; shown = 10; }}>评论 {coms.length || ''}</button>
      </div>
      <span class="grow"></span>
      {#if tab === 'sub' && box.unread}<button class="btn tiny" onclick={() => inboxAct(site.id, 'submission', 'read', subs.filter((x) => !x.read).map((x) => x.id))}>全部标为已读</button>{/if}
      {#if tab === 'com' && box.pending}<button class="btn tiny" onclick={() => inboxAct(site.id, 'comment', 'approve', coms.filter((x) => x.status === 'pending').map((x) => x.id))}>全部通过</button>{/if}
    </div>
    {#if tab === 'sub'}
      {#if !subs.length}<p class="empty">还没有人提交过表单。</p>{/if}
      <ul class="items">
        {#each subs.slice(0, shown) as x (x.id)}
          <li class:unread={!x.read}>
            <div class="top">
              {#if !x.read}<span class="dot" aria-label="未读"></span>{/if}
              <span class="pill">{x.form}</span>
              {#if x.page}<span class="mono path">{x.page}</span>{/if}
              <span class="grow"></span>
              <small data-tip={full(sec(x.created_at))}>{ago(sec(x.created_at))}</small>
              {#if !x.read}<button class="btn tiny quiet" onclick={() => inboxAct(site.id, 'submission', 'read', [x.id])}>标为已读</button>{/if}
              <button class="btn tiny quiet del" onclick={() => inboxAct(site.id, 'submission', 'delete', [x.id])} aria-label="删除"><Icon name="trash" size={11} /></button>
            </div>
            <dl class="fields">
              {#each Object.entries(x.fields ?? {}) as [k, val] (k)}<dt>{k}</dt><dd>{val}</dd>{/each}
            </dl>
          </li>
        {/each}
      </ul>
      {#if subs.length > shown}<button class="link-btn more" onclick={() => (shown += 20)}>再看 {Math.min(20, subs.length - shown)} 条</button>{/if}
    {:else}
      {#if !coms.length}<p class="empty">还没有新评论（迁移来的旧评论在页面里静态显示，不在这里）。</p>{/if}
      <ul class="items">
        {#each coms.slice(0, shown) as x (x.id)}
          <li class:pending={x.status === 'pending'}>
            <div class="top">
              <b>{x.name}</b>
              {#if x.email}<small class="faint">{x.email}</small>{/if}
              <span class="mono path">{x.path}</span>
              {#if x.status === 'pending'}<span class="pill warn">待审</span>{/if}
              <span class="grow"></span>
              <small data-tip={full(sec(x.created_at))}>{ago(sec(x.created_at))}</small>
            </div>
            <p class="content">{x.content}</p>
            <div class="acts">
              {#if x.status === 'pending'}<button class="btn tiny" onclick={() => inboxAct(site.id, 'comment', 'approve', [x.id])}><Icon name="check" size={11} />通过</button>{/if}
              <button class="btn tiny quiet" onclick={() => inboxAct(site.id, 'comment', 'spam', [x.id])}>标为垃圾</button>
              <button class="btn tiny quiet del" onclick={() => inboxAct(site.id, 'comment', 'delete', [x.id])}><Icon name="trash" size={11} />删除</button>
            </div>
          </li>
        {/each}
      </ul>
      {#if coms.length > shown}<button class="link-btn more" onclick={() => (shown += 20)}>再看 {Math.min(20, coms.length - shown)} 条</button>{/if}
    {/if}
  {/if}
</div>

<style>
  .ic-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border: none; background: none; border-radius: 6px; color: var(--faint); cursor: pointer; }
  .ic-btn:hover { background: #f1efe9; color: var(--text); }
  .head { margin-bottom: 8px; }
  .grow { flex: 1; }
  .notes { list-style: none; margin: 0 0 10px; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .notes li { display: flex; gap: 6px; align-items: flex-start; font-size: 12px; color: var(--dim); line-height: 1.5; }
  .notes li :global(svg) { flex: none; margin-top: 3px; color: var(--faint); }
  .empty { margin: 6px 0 2px; font-size: 12.5px; color: var(--faint); }
  .items { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
  .items > li { padding: 10px 0; border-top: 1px solid var(--border); display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .items > li:first-child { border-top: none; }
  .top { display: flex; align-items: center; gap: 8px; min-width: 0; font-size: 12.5px; }
  .top small { font-size: 11.5px; color: var(--faint); white-space: nowrap; }
  .top b { font-weight: 600; }
  .path { font-size: 11.5px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--su-run, #2f5fb8); flex: none; }
  .fields { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 3px 12px; margin: 0; padding: 8px 10px; border-radius: 8px; background: var(--rail); font-size: 12.5px; }
  .fields dt { color: var(--faint); }
  .fields dd { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  .content { margin: 0; font-size: 13px; line-height: 1.6; white-space: pre-wrap; overflow-wrap: anywhere; }
  .acts { display: flex; gap: 6px; }
  .del:hover:not(:disabled) { color: var(--err); }
  .more { font-size: 12px; margin-top: 6px; align-self: flex-start; }
  .bars { display: flex; flex-direction: column; gap: 8px; }
  .bars i { display: block; height: 10px; border-radius: 6px; background: #f1efe9; }
</style>
