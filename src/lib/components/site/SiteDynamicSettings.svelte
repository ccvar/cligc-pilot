<script lang="ts">
  // 站点设置里的「表单和评论」：给 Pilot 上传到 Pages 的站点开表单、评论（Pages Functions + D1，Pilot 建好资源），
  // 然后让 AI 把它们接到页面上。迁移中的站点在迁移进度里配。
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import { ago, full } from './fmt';
  import { sites } from '$lib/state/sites.svelte.js';
  import { dyn, loadDynamic, setupDynamic, wireWithAi, isPagesDirect, isVpsManaged, FEATURE_LABEL } from '$lib/state/dynamic.svelte.js';

  let { site }: { site: Site } = $props();
  const conf = $derived(dyn.conf[site.id]);
  const vps = $derived(isVpsManaged(site));
  const busy = $derived(dyn.busy[site.id] === 'setup' || (sites.jobs[site.id]?.status === 'running' && ['dynamic:setup', 'vps:setup'].includes(sites.jobs[site.id]?.kind ?? '')));
  let picked = $state<Set<string>>(new Set());
  let inited = '';
  $effect(() => { void loadDynamic(site.id); });
  $effect(() => { const k = `${site.id}:${conf?.features.join(',') ?? ''}`; if (k !== inited) { inited = k; picked = new Set((conf?.features ?? []).filter((f) => f !== 'files')); } });
  function toggle(f: string) { const n = new Set(picked); if (n.has(f)) n.delete(f); else n.add(f); picked = n; }
  const changed = $derived([...picked].sort().join(',') !== (conf?.features ?? []).filter((f) => f !== 'files').sort().join(','));
  const keepFiles = $derived(conf?.features.includes('files') ? ['files'] : []);
  // 默认只显示开了哪些；点「修改 / 开启」才出勾选框和配置按钮
  let editing = $state(false);
  const on = $derived((conf?.features ?? []).filter((f) => f !== 'files'));
</script>

<div class="card dyn" class:editing>
  <h3><Icon name="inbox" />表单和评论<span class="grow"></span>{#if !editing && site.origin !== 'migrating' && (isPagesDirect(site) || vps)}<button class="ic-btn edit-ic" onclick={() => (editing = true)} aria-label={conf?.setup_at ? '修改表单和评论' : '开启表单和评论'} data-tip={conf?.setup_at ? '修改表单和评论' : '开启表单和评论'}><Icon name="pencil" size={14} /></button>{/if}</h3>
  {#if site.origin === 'migrating'}
    <p class="hint">迁移中的站点在概览的「迁移进度」里配置（按迁移方案自动带上旧站的表单和评论）。</p>
  {:else if !isPagesDirect(site) && !vps}
    <p class="hint">表单和评论要部署在 Cloudflare Pages（Pilot 构建后上传），或者 Pilot 托管的服务器上（VPS，发布命令留空）。</p>
  {:else if !editing}
    <dl class="kv">
      <dt>表单</dt><dd>{on.includes('form') ? '已开' : '没开'}</dd>
      <dt>评论</dt><dd>{on.includes('comments') ? '已开' : '没开'}</dd>
      {#if conf?.setup_at}<dt>配置于</dt><dd><span data-tip={full(conf.setup_at)}>{ago(conf.setup_at)}</span>{vps ? ' · 在你的服务器上' : ' · Cloudflare Pages'}</dd>{/if}
    </dl>
    {#if conf?.setup_at && on.length}<div class="row" style="margin-top:10px"><button class="btn small" onclick={() => wireWithAi(site)}><Icon name="wand" size={12} />让 AI 接到页面上</button></div>{/if}
  {:else}
    <p class="hint">{vps ? '留言、联系、订阅表单和文章评论，跑在你的服务器上（Go 写的小服务，数据存 SQLite），Pilot 配置服务器时一起装好。' : '留言、联系、订阅表单和文章评论，跑在这个站点的 Cloudflare Pages 上（Functions + D1 数据库 + Turnstile 人机验证），Pilot 自动建好。'}提交和评论在概览里看、审。</p>
    <div class="row wrap opts">
      {#each ['form', 'comments'] as f (f)}
        <label class="check"><input type="checkbox" checked={picked.has(f)} onchange={() => toggle(f)} />{FEATURE_LABEL[f]}</label>
      {/each}
    </div>
    {#if conf?.setup_at}
      <p class="status"><Icon name="check" size={12} />已配置 <span data-tip={full(conf.setup_at)}>{ago(conf.setup_at)}</span>{vps ? ' · 在服务器上' : ` · D1 ${conf.d1_name || '—'} · 人机验证 ${conf.turnstile_sitekey ? '已开' : '没开'}${conf.r2_bucket ? ` · R2 ${conf.r2_bucket}` : ''}`}</p>
    {/if}
    {#if conf?.notes?.length}<ul class="notes">{#each conf.notes as n (n)}<li>{n}</li>{/each}</ul>{/if}
    <div class="row">
      <button class="btn small primary" onclick={() => setupDynamic(site.id, [...picked, ...keepFiles], vps)} disabled={busy || (!changed && !!conf?.setup_at) || (!picked.size && !keepFiles.length && !conf?.setup_at)}>{busy ? '配置中…' : conf?.setup_at ? (changed ? '保存并重新配置' : '已配置') : '开启并配置'}</button>
      {#if conf?.setup_at && conf.features.some((f) => f !== 'files')}<button class="btn small" onclick={() => wireWithAi(site)}><Icon name="wand" size={12} />让 AI 接到页面上</button>{/if}
      <button class="btn small ghost" onclick={() => { editing = false; picked = new Set(on); }}>取消</button>
      <small class="faint">配置后上线一次生效</small>
    </div>
  {/if}
</div>

<style>
  .opts { gap: 16px; margin: 8px 0 10px; }
  .card.editing { border-color: var(--border2); box-shadow: 0 0 0 3px var(--su-hover, #f3f1ec); }
  .grow { flex: 1; }
  .edit-ic { width: 26px; height: 26px; margin: -4px -6px -4px 0; }
  .status { display: flex; align-items: center; gap: 6px; margin: 0 0 8px; font-size: 12px; color: var(--ok); }
  .notes { margin: 0 0 8px; padding-left: 18px; font-size: 12px; color: var(--dim); line-height: 1.55; }
</style>
