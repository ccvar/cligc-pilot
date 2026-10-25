<script lang="ts">
  // 站点「分发」：把站点文章的摘要同步到站外平台（第一个是 Blogger）。上面是设置（发到哪个博客、发布方式），
  // 下面一篇一行：生成摘要 → 预览 / 修改 → 发布。只发摘要不发全文，摘要里只有这一篇原文的链接（见 syndicate.rs）。
  import './site.css';
  import Icon from './Icon.svelte';
  import BloggerMark from '$lib/components/icons/BloggerMark.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import type { Site } from '$lib/types';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { say } from '$lib/state/app.svelte.js';
  import { blogger, bloggerAccounts, bloggerLogin, loadBlogs } from '$lib/state/blogger.svelte.js';
  import { mask } from '$lib/mask';

  let { site }: { site: Site } = $props();

  type Config = { blogger_conn_id: string; blog_id: string; blog_name: string; blog_url: string; mode: string };
  type Post = { path: string; title: string; summary: string; labels: string[]; status: string; post_id: string; post_url: string; error: string; updated_at: number; published_at: number };
  type Row = { path: string; url: string; title: string; date: string; post: Post | null };
  type State = { config: Config; articles: Row[]; built: boolean; brain: string };

  let st = $state<State | null>(null);
  let loadErr = $state('');
  async function load() {
    try { st = await invoke<State>('syndication_state', { siteId: site.id }); loadErr = ''; } catch (e) { loadErr = String(e); }
  }
  $effect(() => { void site.id; void load(); });

  const accounts = $derived(bloggerAccounts());
  const cfg = $derived(st?.config);
  $effect(() => { if (cfg?.blogger_conn_id) void loadBlogs(cfg.blogger_conn_id); });
  const blogs = $derived(cfg?.blogger_conn_id ? blogger.blogs[cfg.blogger_conn_id] ?? null : null);
  const blogsErr = $derived(cfg?.blogger_conn_id ? blogger.blogsErr[cfg.blogger_conn_id] ?? '' : '');
  const ready = $derived(!!cfg?.blogger_conn_id && !!cfg?.blog_id);
  const brainOk = $derived(!!st && (st.brain === 'claude' || st.brain.startsWith('api:')));

  async function saveConfig(patch: Partial<Config>) {
    if (!st) return;
    const next = { ...st.config, ...patch };
    try { st.config = await invoke<Config>('syndication_save_config', { siteId: site.id, config: next }); } catch (e) { say(String(e), 'err'); }
  }
  function pickAccount(id: string) { void saveConfig({ blogger_conn_id: id, blog_id: '', blog_name: '', blog_url: '' }); }
  function pickBlog(id: string) {
    const b = blogs?.find((x) => x.id === id);
    void saveConfig({ blog_id: id, blog_name: b?.name ?? '', blog_url: b?.url ?? '' });
  }
  // 接上第一个账号后直接选它
  $effect(() => { if (st && !st.config.blogger_conn_id && accounts.length === 1) pickAccount(accounts[0].id); });

  // 每篇的操作
  let busy = $state<Record<string, string>>({});
  let open = $state<string>('');
  let draft = $state<{ title: string; summary: string; labels: string }>({ title: '', summary: '', labels: '' });
  function setPost(p: Post) {
    if (!st) return;
    const r = st.articles.find((a) => a.path === p.path);
    if (r) r.post = p;
  }
  function openPreview(r: Row) {
    if (open === r.path) { open = ''; return; }
    open = r.path;
    draft = { title: r.post?.title ?? '', summary: r.post?.summary ?? '', labels: (r.post?.labels ?? []).join('，') };
  }
  async function run(path: string, label: string, f: () => Promise<Post>, done?: string) {
    busy = { ...busy, [path]: label };
    try { const p = await f(); setPost(p); if (done) say(done); return p; }
    catch (e) { say(String(e), 'err'); }
    finally { const b = { ...busy }; delete b[path]; busy = b; }
  }
  async function generate(r: Row) {
    const p = await run(r.path, '写摘要中…', () => invoke<Post>('syndication_generate', { siteId: site.id, path: r.path }));
    if (p) { open = r.path; draft = { title: p.title, summary: p.summary, labels: p.labels.join('，') }; }
  }
  const labelsOf = (s: string) => s.split(/[,，、]/).map((x) => x.trim()).filter(Boolean);
  async function saveEdit(r: Row) {
    await run(r.path, '保存中…', () => invoke<Post>('syndication_edit', { siteId: site.id, path: r.path, title: draft.title, summary: draft.summary, labels: labelsOf(draft.labels) }), '已保存');
  }
  async function publish(r: Row) {
    // 预览里改过的先存
    if (open === r.path && r.post && (draft.title !== r.post.title || draft.summary !== r.post.summary || draft.labels !== r.post.labels.join('，'))) {
      const ok = await run(r.path, '保存中…', () => invoke<Post>('syndication_edit', { siteId: site.id, path: r.path, title: draft.title, summary: draft.summary, labels: labelsOf(draft.labels) }));
      if (!ok) return;
    }
    const asDraft = cfg?.mode === 'draft';
    const p = await run(r.path, asDraft ? '存草稿中…' : '发布中…', () => invoke<Post>('syndication_publish', { siteId: site.id, path: r.path, draft: asDraft }), asDraft ? '已存成 Blogger 草稿' : '已发布到 Blogger');
    if (p) open = '';
  }
  async function skip(r: Row, on: boolean) {
    await run(r.path, '', () => invoke<Post>('syndication_skip', { siteId: site.id, path: r.path, skip: on }));
  }

  const STATUS: Record<string, string> = { ready: '摘要写好，等你确认', published: '已发布', draft: '已存为 Blogger 草稿', skipped: '跳过', failed: '发布失败' };
  const counts = $derived.by(() => {
    const a = st?.articles ?? [];
    return { all: a.length, published: a.filter((r) => r.post?.status === 'published' || r.post?.status === 'draft').length, ready: a.filter((r) => r.post?.status === 'ready').length };
  });
  const fmtDay = (ts: number) => { const d = new Date(ts * 1000); return `${d.getMonth() + 1}/${d.getDate()}`; };
</script>

<div class="dist">
  <div class="card">
    <h3><BloggerMark size={15} />Blogger<span class="grow"></span>{#if cfg?.blog_url}<button class="btn tiny quiet" onclick={() => openUrl(cfg.blog_url)}><Icon name="external" size={11} />打开博客</button>{/if}</h3>
    {#if !accounts.length}
      <p class="hint">还没有 Blogger 账号。建议用一个单独的 Google 账号，只给它 Blogger 权限，和看网站数据的账号分开。</p>
      <div class="row"><button class="btn small primary" onclick={() => bloggerLogin('')} disabled={blogger.busy}>{blogger.busy ? '在浏览器里授权…' : '接上 Blogger 账号'}</button></div>
    {:else if cfg}
      <div class="grid">
        <span class="lbl">账号</span>
        <Dropdown value={cfg.blogger_conn_id} placeholder="选一个 Blogger 账号" ariaLabel="Blogger 账号"
          options={accounts.map((a) => ({ value: a.id, label: mask(a.remark?.trim() || a.login || a.name) }))} onchange={pickAccount} />
        <span class="lbl">博客</span>
        <div class="row">
          <div class="blog-dd"><Dropdown value={cfg.blog_id} disabled={!cfg.blogger_conn_id || !blogs?.length} ariaLabel="博客"
            placeholder={!cfg.blogger_conn_id ? '先选账号' : !blogs ? (blogsErr ? '读不到博客' : '读取中…') : blogs.length ? '选一个博客' : '这个账号下还没有博客'}
            options={(blogs ?? []).map((b) => ({ value: b.id, label: b.name || b.url, sub: b.url.replace(/^https?:\/\//, '').replace(/\/$/, '') }))} onchange={pickBlog} /></div>
          <button class="btn small" onclick={() => loadBlogs(cfg.blogger_conn_id, true)} disabled={!cfg.blogger_conn_id} data-tip="在 Blogger 新建博客后点这里刷新"><Icon name="refresh" size={12} />刷新</button>
          <button class="btn small" onclick={() => openUrl('https://www.blogger.com/')} data-tip="Blogger 的接口不能新建博客：在 blogger.com 上建一个，回来点刷新"><Icon name="external" size={12} />去 Blogger 新建</button>
        </div>
        <span class="lbl">发布方式</span>
        <Dropdown value={cfg.mode || 'review'} ariaLabel="发布方式" onchange={(v) => saveConfig({ mode: v })}
          options={[{ value: 'review', label: '先给我看', sub: '我点「发布」才发' }, { value: 'draft', label: '存成 Blogger 草稿', sub: '我去 Blogger 后台发' }]} />
      </div>
      {#if blogsErr}<p class="hint warn">{blogsErr}</p>{/if}
      <p class="hint">每个站只对应一个博客。只发 AI 写的导读（三五百字）和原文链接，不发全文，免得和你站上的原文抢排名；摘要里不放别的链接。</p>
      {#if !brainOk}<p class="hint warn">写摘要用站点「设置 → 运营」里的模型服务，现在是 {st?.brain || '未设置'}，不支持：请改成 Claude Code 或 API 连接。</p>{/if}
    {/if}
  </div>

  <div class="card">
    <h3><Icon name="doc" />文章<span class="cnt-n">{counts.all}</span>{#if counts.published}<span class="sub">已发 {counts.published}</span>{/if}{#if counts.ready}<span class="sub">待确认 {counts.ready}</span>{/if}<span class="grow"></span><button class="btn tiny quiet" onclick={load} data-tip="重新读取站点文章"><Icon name="refresh" size={11} /></button></h3>
    {#if loadErr}<p class="hint warn">{loadErr}</p>
    {:else if st && !st.built}<p class="hint">站点还没有构建产物，读不到文章：先在「部署」里构建一次。</p>
    {:else if st && !st.articles.length}<p class="hint">没找到文章页（带 Article / BlogPosting / HowTo 结构化数据的页面）。</p>
    {:else if st}
      <ul class="arts">
        {#each st.articles as r (r.path)}
          {@const s = r.post?.status ?? ''}
          {@const b = busy[r.path]}
          <li class:dim={s === 'skipped'}>
            <div class="art">
              <span class="t"><button class="link-btn" onclick={() => openUrl(r.url)} data-tip={r.url}>{r.title || r.path}</button>{#if r.date}<small>{r.date}</small>{/if}</span>
              <span class="st st-{s || 'none'}" data-tip={s === 'failed' ? r.post?.error : undefined}>{s ? STATUS[s] ?? s : ''}{#if (s === 'published' || s === 'draft') && r.post?.published_at} · {fmtDay(r.post.published_at)}{/if}</span>
              <span class="acts">
                {#if b}<span class="busy"><span class="spin"></span>{b}</span>
                {:else if s === 'published'}
                  {#if r.post?.post_url}<button class="btn tiny" onclick={() => r.post && openUrl(r.post.post_url)}>查看</button>{/if}
                {:else if s === 'draft'}
                  <button class="btn tiny" onclick={() => openUrl(`https://www.blogger.com/blog/posts/${cfg?.blog_id ?? ''}`)}>去 Blogger 发布</button>
                {:else if s === 'skipped'}
                  <button class="btn tiny quiet" onclick={() => skip(r, false)}>恢复</button>
                {:else if s === 'ready' || s === 'failed'}
                  <button class="btn tiny quiet" onclick={() => openPreview(r)}>{open === r.path ? '收起' : '预览'}</button>
                  <button class="btn tiny primary" onclick={() => publish(r)} disabled={!ready} data-tip={ready ? undefined : '先在上面选博客'}>{cfg?.mode === 'draft' ? '存草稿' : s === 'failed' ? '重试' : '发布'}</button>
                  <button class="btn tiny quiet" onclick={() => skip(r, true)}>跳过</button>
                {:else}
                  <button class="btn tiny" onclick={() => generate(r)} disabled={!brainOk}>生成摘要</button>
                  <button class="btn tiny quiet" onclick={() => skip(r, true)}>跳过</button>
                {/if}
              </span>
            </div>
            {#if open === r.path && r.post && (s === 'ready' || s === 'failed')}
              <div class="pv">
                <input class="tin" bind:value={draft.title} placeholder="标题" />
                <textarea class="tin" rows="9" bind:value={draft.summary} placeholder="摘要（段落之间空一行）"></textarea>
                <input class="tin" bind:value={draft.labels} placeholder="标签，用逗号分开" />
                <p class="hint">发出去时末尾会自动加「阅读全文：<button class="link-btn" onclick={() => openUrl(r.url)}>{r.title}</button>」。摘要里写的网址会被去掉。</p>
                <div class="row-end">
                  <button class="btn tiny quiet" onclick={() => generate(r)} disabled={!!b}>重新生成</button>
                  <button class="btn tiny" onclick={() => saveEdit(r)} disabled={!!b}>保存修改</button>
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
  .grid { display: grid; grid-template-columns: 72px 1fr; gap: 8px 12px; align-items: center; margin: 4px 0 8px; max-width: 620px; }
  .grid .lbl { font-size: 12.5px; color: var(--dim); }
  .grid .blog-dd { flex: 1; min-width: 0; }
  .hint.warn { color: var(--warn); }
  .cnt-n { font-size: 12px; color: var(--faint); font-weight: 500; margin-left: 2px; }
  .sub { font-size: 11.5px; color: var(--dim); font-weight: 400; margin-left: 8px; }
  .arts { list-style: none; margin: 4px 0 0; padding: 0; }
  .arts li { border-top: 1px solid #f1f0ec; }
  .arts li:first-child { border-top: 0; }
  .arts li.dim .t { opacity: .5; }
  .art { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 4px 0; }
  .t { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 8px; overflow: hidden; }
  .t .link-btn { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; font-size: 13px; color: var(--text); }
  .t small { flex: none; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .st { flex: none; font-size: 11.5px; color: var(--faint); }
  .st-published, .st-draft { color: var(--ok); }
  .st-ready { color: #b45309; }
  .st-failed { color: var(--err); cursor: help; }
  .acts { flex: none; display: inline-flex; gap: 4px; align-items: center; }
  .busy { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--dim); }
  .pv { display: flex; flex-direction: column; gap: 8px; padding: 4px 0 12px; }
  .pv textarea { resize: vertical; line-height: 1.6; font-size: 13px; }
</style>
