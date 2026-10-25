<script lang="ts">
  // 站点「分发」：把站点文章的摘要同步到站外渠道（Blogger 博客，之后是 WordPress.com、Telegram 频道……）。
  // 上面是渠道（发到哪儿、发布方式），下面一篇文章一行：生成摘要 → 预览 / 修改 → 发到各渠道，每个渠道一个状态标记。
  // 只发摘要不发全文，摘要里只有这一篇原文的链接（见 syndicate.rs）。
  import './site.css';
  import Icon from './Icon.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import ConnectionMark from '$lib/components/icons/ConnectionMark.svelte';
  import type { Site } from '$lib/types';
  import { invoke } from '@tauri-apps/api/core';
  import { untrack } from 'svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { say } from '$lib/state/app.svelte.js';
  import { blogger, bloggerAccounts, bloggerLogin, loadBlogs } from '$lib/state/blogger.svelte.js';
  import { mask } from '$lib/mask';

  let { site }: { site: Site } = $props();

  type Channel = { id: string; kind: string; conn_id: string; target_id: string; target_name: string; target_url: string; mode: string; daily_limit: number; enabled: boolean; created_at: number };
  type Summary = { path: string; title: string; summary: string; labels: string[]; skipped: boolean; updated_at: number };
  type Delivery = { path: string; channel_id: string; status: string; post_id: string; post_url: string; error: string; updated_at: number };
  type Row = { path: string; url: string; title: string; date: string; summary: Summary | null; deliveries: Delivery[] };
  type Kind = { kind: string; label: string; available: boolean };
  type State = { channels: Channel[]; articles: Row[]; built: boolean; brain: string; used_targets: Record<string, string>; kinds: Kind[] };

  let st = $state<State | null>(null);
  let loadErr = $state('');
  async function load() {
    try { st = await invoke<State>('syndication_state', { siteId: site.id }); loadErr = ''; } catch (e) { loadErr = String(e); }
  }
  $effect(() => { void site.id; void load(); });

  const brainOk = $derived(!!st && (st.brain === 'claude' || st.brain.startsWith('api:')));
  const live = $derived((st?.channels ?? []).filter((c) => c.enabled));
  const kindLabel = (k: string) => st?.kinds.find((x) => x.kind === k)?.label ?? k;
  const MODE: Record<string, string> = { review: '先给我看', draft: '存成草稿', auto: '自动发布' };

  // ── 渠道：新建 / 修改（同一时间只开一个编辑器）──
  let edit = $state<Channel | null>(null);
  let addOpen = $state(false);
  let addEl: HTMLSpanElement | undefined = $state();
  $effect(() => {
    if (!addOpen) return;
    const close = (e: MouseEvent) => { if (addEl && !addEl.contains(e.target as Node)) addOpen = false; };
    document.addEventListener('mousedown', close, true);
    return () => document.removeEventListener('mousedown', close, true);
  });
  function newChannel(kind: string) {
    addOpen = false;
    const accts = kind === 'blogger' ? bloggerAccounts() : [];
    edit = { id: '', kind, conn_id: accts.length === 1 ? accts[0].id : '', target_id: '', target_name: '', target_url: '', mode: 'review', daily_limit: 2, enabled: true, created_at: 0 };
  }
  const accounts = $derived(edit?.kind === 'blogger' ? bloggerAccounts() : []);
  $effect(() => { if (edit?.kind === 'blogger' && edit.conn_id) void loadBlogs(edit.conn_id); });
  const blogs = $derived(edit?.kind === 'blogger' && edit.conn_id ? blogger.blogs[edit.conn_id] ?? null : null);
  const blogsErr = $derived(edit?.conn_id ? blogger.blogsErr[edit.conn_id] ?? '' : '');
  /** 这个目标被谁占了：别的站名 / 'this'（这个站已经加过）/ undefined */
  function takenBy(id: string): string | undefined {
    if (!edit) return undefined;
    const other = st?.used_targets[`${edit.kind}:${id}`];
    if (other) return other;
    return st?.channels.some((c) => c.id !== edit?.id && c.kind === edit?.kind && c.target_id === id) ? 'this' : undefined;
  }
  const freeBlogs = $derived((blogs ?? []).filter((b) => !takenBy(b.id)));
  // 新建时默认选第一个没被占用的博客（一个博客只对应一个站）
  $effect(() => { if (edit && !edit.id && !edit.target_id && freeBlogs.length) untrack(() => pickBlog(freeBlogs[0].id)); });
  function pickBlog(id: string) {
    if (!edit) return;
    const b = blogs?.find((x) => x.id === id);
    edit = { ...edit, target_id: id, target_name: b?.name ?? '', target_url: b?.url ?? '' };
  }
  let saving = $state(false);
  async function saveChannel() {
    if (!edit) return;
    saving = true;
    try { await invoke('syndication_save_channel', { siteId: site.id, channel: edit }); edit = null; await load(); say('渠道已保存'); }
    catch (e) { say(String(e), 'err'); }
    finally { saving = false; }
  }
  async function removeChannel(c: Channel) {
    try { await invoke('syndication_remove_channel', { siteId: site.id, channelId: c.id }); edit = null; await load(); say('已删除渠道（已经发出去的文章还在平台上）'); }
    catch (e) { say(String(e), 'err'); }
  }
  async function toggleChannel(c: Channel) {
    try { await invoke('syndication_save_channel', { siteId: site.id, channel: { ...c, enabled: !c.enabled } }); await load(); }
    catch (e) { say(String(e), 'err'); }
  }
  const sentCount = (c: Channel) => (st?.articles ?? []).filter((r) => r.deliveries.some((d) => d.channel_id === c.id && d.status !== 'failed')).length;

  // ── 文章 ──
  let filter = $state<'all' | 'todo' | 'done' | 'skipped'>('all');
  const deliveryOf = (r: Row, c: Channel) => r.deliveries.find((d) => d.channel_id === c.id);
  const pendingOf = (r: Row) => live.filter((c) => { const d = deliveryOf(r, c); return !d || d.status === 'failed'; });
  const rows = $derived((st?.articles ?? []).filter((r) => {
    if (filter === 'skipped') return !!r.summary?.skipped;
    if (r.summary?.skipped) return filter === 'all';
    if (filter === 'todo') return live.length > 0 && pendingOf(r).length > 0;
    if (filter === 'done') return live.length > 0 && pendingOf(r).length === 0;
    return true;
  }));
  const counts = $derived.by(() => {
    const a = (st?.articles ?? []).filter((r) => !r.summary?.skipped);
    return { all: st?.articles.length ?? 0, todo: live.length ? a.filter((r) => pendingOf(r).length).length : 0, done: live.length ? a.filter((r) => !pendingOf(r).length).length : 0, skipped: (st?.articles ?? []).length - a.length };
  });
  const FILTERS = $derived([['all', '全部', counts.all], ['todo', '待发', counts.todo], ['done', '已发', counts.done], ['skipped', '跳过', counts.skipped]] as const);

  let busy = $state<Record<string, string>>({});
  let open = $state('');
  let draft = $state<{ title: string; summary: string; labels: string }>({ title: '', summary: '', labels: '' });
  function openPreview(r: Row) {
    if (open === r.path) { open = ''; return; }
    open = r.path;
    draft = { title: r.summary?.title ?? '', summary: r.summary?.summary ?? '', labels: (r.summary?.labels ?? []).join('，') };
  }
  async function run<T>(path: string, label: string, f: () => Promise<T>, done?: string): Promise<T | undefined> {
    busy = { ...busy, [path]: label };
    try { const v = await f(); if (done) say(done); return v; }
    catch (e) { if (String(e) === '已停止') say('已停止生成'); else say(String(e), 'err'); }
    finally { const b = { ...busy }; delete b[path]; busy = b; }
  }
  function setSummary(s: Summary) { const r = st?.articles.find((a) => a.path === s.path); if (r) r.summary = s; }
  async function generate(r: Row) {
    const s = await run(r.path, '写摘要中…', () => invoke<Summary>('syndication_generate', { siteId: site.id, path: r.path }));
    if (s) { setSummary(s); open = r.path; draft = { title: s.title, summary: s.summary, labels: s.labels.join('，') }; }
  }
  function stopGen(r: Row) { void invoke('syndication_cancel', { siteId: site.id, path: r.path }); }
  const labelsOf = (s: string) => s.split(/[,，、]/).map((x) => x.trim()).filter(Boolean);
  const dirty = (r: Row) => open === r.path && !!r.summary && (draft.title !== r.summary.title || draft.summary !== r.summary.summary || draft.labels !== r.summary.labels.join('，'));
  async function saveEdit(r: Row, quiet = false) {
    const s = await run(r.path, '保存中…', () => invoke<Summary>('syndication_edit', { siteId: site.id, path: r.path, title: draft.title, summary: draft.summary, labels: labelsOf(draft.labels) }), quiet ? undefined : '已保存');
    if (s) setSummary(s);
    return !!s;
  }
  async function publish(r: Row, only: string[] = []) {
    if (dirty(r) && !(await saveEdit(r, true))) return;
    const ds = await run(r.path, '发布中…', () => invoke<Delivery[]>('syndication_publish', { siteId: site.id, path: r.path, channelIds: only }));
    if (!ds) return;
    r.deliveries = ds;
    const failed = ds.filter((d) => d.status === 'failed' && (only.length ? only.includes(d.channel_id) : true));
    if (failed.length) say(`有 ${failed.length} 个渠道没发出去：${failed[0].error}`, 'err');
    else { say('已发布'); open = ''; }
  }
  async function skip(r: Row, on: boolean) {
    const s = await run(r.path, '', () => invoke<Summary>('syndication_skip', { siteId: site.id, path: r.path, skip: on }));
    if (s) setSummary(s);
  }
  function chipClick(r: Row, c: Channel) {
    const d = deliveryOf(r, c);
    if (d?.status === 'published' && d.post_url) void openUrl(d.post_url);
    else if (d?.status === 'draft' && c.kind === 'blogger') void openUrl(`https://www.blogger.com/blog/posts/${c.target_id}`);
    else if (d?.status === 'failed' && r.summary?.summary) void publish(r, [c.id]);
  }
  function chipTip(r: Row, c: Channel) {
    const d = deliveryOf(r, c);
    const name = `${kindLabel(c.kind)} · ${c.target_name || c.target_id}`;
    if (!d) return `${name}：还没发`;
    if (d.status === 'published') return `${name}：已发布，点击查看`;
    if (d.status === 'draft') return `${name}：已存为草稿，点击去后台发布`;
    return `${name}：发布失败（${d.error}），点击重试`;
  }
</script>

{#snippet blogFoot(close: () => void)}
  <button onclick={() => { close(); if (edit) void loadBlogs(edit.conn_id, true); }} disabled={!edit?.conn_id}><Icon name="refresh" size={13} />刷新博客列表</button>
  <button onclick={() => { close(); void openUrl('https://www.blogger.com/'); }} data-tip="Blogger 的接口不能新建博客：在 blogger.com 上建好，回来点「刷新博客列表」"><Icon name="plus" size={13} />去 Blogger 新建博客</button>
{/snippet}

{#snippet editor(c: Channel)}
  <div class="ed">
    {#if c.kind === 'blogger'}
      {#if !accounts.length}
        <p class="hint">还没有 Blogger 账号。建议用一个单独的 Google 账号，只给它 Blogger 权限，和看网站数据的账号分开。</p>
        <div class="row"><button class="btn small primary" onclick={() => bloggerLogin('')} disabled={blogger.busy}>{blogger.busy ? '在浏览器里授权…' : '接上 Blogger 账号'}</button></div>
      {:else}
        <div class="grid">
          <span class="lbl">账号</span>
          <Dropdown value={c.conn_id} placeholder="选一个 Blogger 账号" ariaLabel="Blogger 账号"
            options={accounts.map((a) => ({ value: a.id, label: mask(a.remark?.trim() || a.login || a.name) }))}
            onchange={(v) => { if (edit) edit = { ...edit, conn_id: v, target_id: '', target_name: '', target_url: '' }; }} />
          <span class="lbl">博客</span>
          <Dropdown value={c.target_id} disabled={!c.conn_id} ariaLabel="博客" footer={blogFoot}
            emptyText={blogsErr ? '读不到博客' : !blogs ? '读取中…' : '这个账号下还没有博客'}
            placeholder={!c.conn_id ? '先选账号' : !blogs ? (blogsErr ? '读不到博客' : '读取中…') : !blogs.length ? '这个账号下还没有博客' : freeBlogs.length ? '选一个博客' : '博客都给别的站用了：在 Blogger 再建一个'}
            options={(blogs ?? []).map((b) => { const t = takenBy(b.id); return { value: b.id, label: b.name || b.url, sub: t ? (t === 'this' ? '这个站已经加过了' : `已用于 ${t}`) : b.url.replace(/^https?:\/\//, '').replace(/\/$/, ''), disabled: !!t }; })}
            onchange={pickBlog} />
          <span class="lbl">发布方式</span>
          <Dropdown value={c.mode || 'review'} ariaLabel="发布方式" onchange={(v) => { if (edit) edit = { ...edit, mode: v }; }}
            options={[{ value: 'review', label: '先给我看', sub: '我点「发布」才发' }, { value: 'draft', label: '存成 Blogger 草稿', sub: '我去 Blogger 后台发' }, { value: 'auto', label: '自动发布', sub: '下一期支持', disabled: true }]} />
        </div>
        {#if blogsErr}<p class="hint warn">{blogsErr}</p>{/if}
      {/if}
    {/if}
    <div class="row-end">
      {#if c.id}<button class="btn small danger" onclick={() => removeChannel(c)}>删除渠道</button><span class="grow"></span>{/if}
      <button class="btn small quiet" onclick={() => (edit = null)}>取消</button>
      <button class="btn small primary" onclick={saveChannel} disabled={saving || !c.conn_id || !c.target_id}>{saving ? '保存中…' : c.id ? '保存' : '添加'}</button>
    </div>
  </div>
{/snippet}

<div class="dist">
  <div class="card">
    <h3><Icon name="share" />渠道<span class="grow"></span>
      <span class="add" bind:this={addEl}>
        <button class="btn tiny" onclick={() => (addOpen = !addOpen)} aria-expanded={addOpen}><Icon name="plus" size={11} />添加渠道</button>
        {#if addOpen}
          <div class="add-menu" role="menu">
            {#each st?.kinds ?? [] as k (k.kind)}
              <button role="menuitem" disabled={!k.available} onclick={() => newChannel(k.kind)}><ConnectionMark kind={k.kind} size={15} /><span>{k.label}{#if !k.available}<small>即将支持</small>{/if}</span></button>
            {/each}
          </div>
        {/if}
      </span>
    </h3>
    {#if st && !st.channels.length && !edit}
      <p class="hint">还没有渠道。添加一个 Blogger 博客，站上的文章就能把导读同步过去。每个渠道只对应一个站；只发 AI 写的导读（三五百字）和原文链接，不发全文。</p>
    {/if}
    <ul class="chs">
      {#each st?.channels ?? [] as c (c.id)}
        <li>
          <div class="ch" class:off={!c.enabled}>
            <span class="ic"><ConnectionMark kind={c.kind} size={16} /></span>
            <span class="nm"><b>{kindLabel(c.kind)} · {c.target_name || c.target_id}</b><small>{MODE[c.mode] ?? c.mode} · 已发 {sentCount(c)}{#if !c.enabled} · 已停用{/if}</small></span>
            {#if c.target_url}<button class="btn tiny quiet" onclick={() => openUrl(c.target_url)} data-tip={c.target_url}><Icon name="external" size={11} /></button>{/if}
            <button class="btn tiny quiet" onclick={() => toggleChannel(c)}>{c.enabled ? '停用' : '启用'}</button>
            <button class="btn tiny quiet" onclick={() => (edit = edit?.id === c.id ? null : { ...c })} aria-expanded={edit?.id === c.id}>{edit?.id === c.id ? '收起' : '设置'}</button>
          </div>
          {#if edit && edit.id === c.id}{@render editor(edit)}{/if}
        </li>
      {/each}
      {#if edit && !edit.id}
        <li><div class="ch"><span class="ic"><ConnectionMark kind={edit.kind} size={16} /></span><span class="nm"><b>新渠道 · {kindLabel(edit.kind)}</b></span></div>{@render editor(edit)}</li>
      {/if}
    </ul>
    {#if st && !brainOk}<p class="hint warn">写摘要用站点「设置 → 运营」里的模型服务，现在是 {st.brain || '未设置'}，不支持：请改成 Claude Code 或 API 连接。</p>{/if}
  </div>

  <div class="card">
    <h3><Icon name="doc" />文章<span class="grow"></span>
      <span class="seg">
        {#each FILTERS as [k, l, n] (k)}
          <button class:on={filter === k} onclick={() => (filter = k)}>{l}<small>{n}</small></button>
        {/each}
      </span>
      <button class="btn tiny quiet" onclick={load} data-tip="重新读取站点文章"><Icon name="refresh" size={11} /></button>
    </h3>
    {#if loadErr}<p class="hint warn">{loadErr}</p>
    {:else if st && !st.built}<p class="hint">站点还没有构建产物，读不到文章：先在「部署」里构建一次。</p>
    {:else if st && !st.articles.length}<p class="hint">没找到文章页（带 Article / BlogPosting / HowTo 结构化数据的页面）。</p>
    {:else if st && !rows.length}<p class="hint">这一栏没有文章。</p>
    {:else if st}
      <ul class="arts">
        {#each rows as r (r.path)}
          {@const b = busy[r.path]}
          {@const skipped = !!r.summary?.skipped}
          {@const pending = pendingOf(r)}
          {@const has = !!r.summary?.summary}
          <li class:dim={skipped}>
            <div class="art">
              <span class="t"><button class="link-btn" onclick={() => openUrl(r.url)} data-tip={r.url}>{r.title || r.path}</button>{#if r.date}<small>{r.date}</small>{/if}</span>
              {#if !skipped}
                <span class="chips">
                  {#each live as c (c.id)}
                    {@const d = deliveryOf(r, c)}
                    <button class="chip st-{d?.status ?? 'none'}" onclick={() => chipClick(r, c)} data-tip={chipTip(r, c)}><ConnectionMark kind={c.kind} size={11} />{d?.status === 'published' ? '✓' : d?.status === 'draft' ? '草稿' : d?.status === 'failed' ? '!' : '—'}</button>
                  {/each}
                </span>
              {/if}
              <span class="acts">
                {#if b}<span class="busy"><span class="spin"></span>{b}</span>{#if b === '写摘要中…'}<button class="btn tiny quiet" onclick={() => stopGen(r)}>停止</button>{/if}
                {:else if skipped}
                  <button class="btn tiny quiet" onclick={() => skip(r, false)}>恢复</button>
                {:else if !has}
                  <button class="btn tiny" onclick={() => generate(r)} disabled={!brainOk}>生成摘要</button>
                  <button class="btn tiny quiet" onclick={() => skip(r, true)}>跳过</button>
                {:else}
                  <button class="btn tiny quiet" onclick={() => openPreview(r)}>{open === r.path ? '收起' : '预览'}</button>
                  {#if pending.length}
                    <button class="btn tiny primary" onclick={() => publish(r)} data-tip={pending.map((c) => `${kindLabel(c.kind)} · ${c.target_name}`).join('\n')}>发布{#if live.length > 1}（{pending.length}）{/if}</button>
                    <button class="btn tiny quiet" onclick={() => skip(r, true)}>跳过</button>
                  {:else if !live.length}
                    <button class="btn tiny primary" disabled data-tip="先在上面添加渠道">发布</button>
                  {/if}
                {/if}
              </span>
            </div>
            {#if open === r.path && has}
              <div class="pv">
                <input class="tin" bind:value={draft.title} placeholder="标题" />
                <textarea class="tin" rows="9" bind:value={draft.summary} placeholder="摘要（段落之间空一行）"></textarea>
                <input class="tin" bind:value={draft.labels} placeholder="标签，用逗号分开" />
                <p class="hint">发出去时末尾会自动加「阅读全文：<button class="link-btn" onclick={() => openUrl(r.url)}>{r.title}</button>」。摘要里写的网址会被去掉。已经发出去的渠道不会因为这里的修改而更新。</p>
                <div class="row-end">
                  <button class="btn tiny quiet" onclick={() => generate(r)} disabled={!!b}>重新生成</button>
                  <button class="btn tiny" onclick={() => saveEdit(r)} disabled={!!b || !dirty(r)}>保存修改</button>
                </div>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .dist { display: flex; flex-direction: column; gap: 12px; }
  .hint.warn { color: var(--warn); }
  .add { position: relative; display: inline-flex; }
  .add-menu { position: absolute; z-index: 20; top: calc(100% + 4px); right: 0; width: 200px; padding: 5px; background: #fff; border: 1px solid var(--border); border-radius: 10px; box-shadow: 0 10px 28px rgba(30, 25, 15, .15); }
  .add-menu button { width: 100%; display: flex; align-items: center; gap: 9px; padding: 7px 9px; border: 0; border-radius: 7px; background: none; font: inherit; font-size: 12.5px; color: var(--text); cursor: pointer; text-align: left; }
  .add-menu button:hover:not(:disabled) { background: #f4f3ef; }
  .add-menu button:disabled { opacity: .5; cursor: default; }
  .add-menu span { display: flex; flex-direction: column; line-height: 1.3; }
  .add-menu small { font-size: 10.5px; color: var(--faint); }
  .chs { list-style: none; margin: 4px 0 0; padding: 0; }
  .chs li + li { border-top: 1px solid #f1f0ec; }
  .ch { display: flex; align-items: center; gap: 10px; min-height: 42px; }
  .ch.off .nm { opacity: .5; }
  .ch .ic { flex: none; display: inline-flex; width: 18px; justify-content: center; }
  .ch .nm { flex: 1; min-width: 0; display: flex; flex-direction: column; line-height: 1.35; }
  .ch .nm b { font-size: 13px; font-weight: 550; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ch .nm small { font-size: 11.5px; color: var(--dim); }
  .ed { padding: 2px 0 12px 28px; }
  .grid { display: grid; grid-template-columns: 72px 1fr; gap: 8px 12px; align-items: center; margin: 4px 0 8px; max-width: 620px; }
  .grid .lbl { font-size: 12.5px; color: var(--dim); }
  .ed .row-end .grow { flex: 1; }
  .seg { display: inline-flex; gap: 2px; background: #f3f2ee; border-radius: 8px; padding: 2px; margin-right: 6px; }
  .seg button { border: 0; background: none; font: inherit; font-size: 12px; color: var(--dim); padding: 3px 9px; border-radius: 6px; cursor: pointer; display: inline-flex; gap: 4px; align-items: baseline; }
  .seg button.on { background: #fff; color: var(--text); box-shadow: 0 1px 2px rgba(0, 0, 0, .06); }
  .seg small { font-size: 10.5px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .arts { list-style: none; margin: 4px 0 0; padding: 0; }
  .arts li { border-top: 1px solid #f1f0ec; }
  .arts li:first-child { border-top: 0; }
  .arts li.dim .t { opacity: .5; }
  .art { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 4px 0; }
  .t { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 8px; overflow: hidden; }
  .t .link-btn { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; font-size: 13px; color: var(--text); }
  .t small { flex: none; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .chips { flex: none; display: inline-flex; gap: 4px; }
  .chip { display: inline-flex; align-items: center; gap: 3px; border: 1px solid var(--border); background: #fff; border-radius: 6px; padding: 1px 6px; font: inherit; font-size: 11px; color: var(--faint); cursor: pointer; }
  .chip.st-published { color: var(--ok); border-color: var(--su-ok-line, #cfe7d8); }
  .chip.st-draft { color: #b45309; }
  .chip.st-failed { color: var(--err); border-color: var(--err-border, #f0c9c9); }
  .chip.st-none { cursor: default; }
  .acts { flex: none; display: inline-flex; gap: 4px; align-items: center; }
  .busy { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--dim); }
  .pv { display: flex; flex-direction: column; gap: 8px; padding: 4px 0 12px; }
  .pv textarea { resize: vertical; line-height: 1.6; font-size: 13px; }
</style>
