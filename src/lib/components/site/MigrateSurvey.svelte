<script lang="ts">
  // 迁移的盘点报告：页数、分组（可以勾掉不迁的）、流量高的页面、动态功能、部署去向和理由。
  // editable 时在下面确认方案（视觉方向 + 补充要求），确认后直接开始抓取。
  import './site.css';
  import Icon from './Icon.svelte';
  import type { MView } from '$lib/state/migrate.svelte.js';
  import { migrate, confirmPlan, resurvey } from '$lib/state/migrate.svelte.js';

  let { id, v, editable }: { id: string; v: MView; editable: boolean } = $props();
  const s = $derived(v.survey!);
  const busy = $derived(migrate.busy[id] ?? '');
  // 方案草稿：默认全部保留
  let dropped = $state<Set<string>>(new Set());
  let visual = $state<'keep_brand' | 'fresh'>('keep_brand');
  let style = $state('');
  let inited = '';
  $effect(() => {
    const k = `${id}:${v.plan?.confirmed_at ?? 0}`;
    if (k === inited) return;
    inited = k;
    dropped = new Set(v.plan?.drop_groups ?? []);
    visual = v.plan?.visual ?? 'keep_brand';
    style = v.plan?.style ?? '';
  });
  function toggle(g: string) { const n = new Set(dropped); if (n.has(g)) n.delete(g); else n.add(g); dropped = n; }
  const keepN = $derived(s.groups.filter((g) => !dropped.has(g.key)).reduce((a, g) => a + g.count, 0));
  const PTYPE: Record<string, string> = { home: '首页', article: '文章', list: '列表', page: '单页' };
  let showBroken = $state(false);
  let showTop = $state(false);
  const groupLabel = (k: string) => (k === '/' ? '首页' : k === '/*' ? '一级页面' : k);
</script>

<div class="sv">
  <div class="stats">
    <div class="stat"><small>网页</small><b>{s.pages}</b>{#if s.truncated}<em>达到上限</em>{/if}</div>
    <div class="stat"><small>图片和附件</small><b>{s.assets}</b><em>{s.files ? `其中下载文件 ${s.files}` : ''}</em></div>
    <div class="stat"><small>RSS / Atom</small><b>{s.feeds}</b></div>
    <div class="stat"><small>旧站已有跳转</small><b>{s.redirects}</b></div>
    <div class="stat"><small>打不开的地址</small><b>{s.broken.length}</b>{#if s.broken.length}<em><button class="link-btn" onclick={() => (showBroken = !showBroken)}>{showBroken ? '收起' : '查看'}</button></em>{/if}</div>
  </div>
  {#if showBroken}<ul class="items">{#each s.broken as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
  <p class="meta-line">
    {s.origin}
    · 路径{s.trailing_slash === 'always' ? '都以 / 结尾' : s.trailing_slash === 'never' ? '都不以 / 结尾' : '结尾有的带 / 有的不带'}
    {#if s.js_rendered} · 浏览器端渲染（已用浏览器抓取）{/if}
    {#if s.generator} · {s.generator}{/if}
    {#if s.analytics.length} · 统计 {s.analytics.join('、')}{/if}
    {#if s.gsc_used} · 已用 Search Console 补漏{/if}
  </p>

  <div class="host" class:ok={s.hosting.supported}>
    <div class="hh"><b>部署去向：{s.hosting.label}</b>{#if s.hosting.paid}<span class="pill warn">需要付费套餐</span>{/if}{#if !s.hosting.supported}<span class="pill warn">下一步支持</span>{/if}</div>
    <ul>{#each s.hosting.reasons as r (r)}<li>{r}</li>{/each}</ul>
    {#if !s.hosting.supported}<small>这一版先支持 Cloudflare 纯静态站。{s.hosting.target === 'cf_workers' ? '表单、评论这类动态功能用 Workers 实现，下一步支持。' : '部署到 VPS 下一步支持。'}</small>{/if}
  </div>

  {#if s.features.length}
    <div class="blk"><b class="bt">动态功能</b>
      <ul class="feats">{#each s.features as f (f.key)}<li><span class="pill {f.static_ok ? 'ok' : 'warn'}">{f.label} · {f.pages} 页</span><small>{f.plan}</small></li>{/each}</ul>
    </div>
  {/if}

  {#if s.notes.length}
    <ul class="notes">{#each s.notes as n (n)}<li><Icon name="info" size={12} />{n}</li>{/each}</ul>
  {/if}

  <div class="blk">
    <b class="bt">页面分组 <small>{editable ? `勾掉不想迁的组 · 将迁 ${keepN} 页` : `${keepN} 页`}</small></b>
    <table class="tbl">
      <thead><tr>{#if editable}<th></th>{/if}<th>分组</th><th class="num">页数</th><th>类型</th>{#if s.gsc_used}<th class="num">点击</th>{/if}<th>示例</th></tr></thead>
      <tbody>
        {#each s.groups as g (g.key)}
          <tr class:off={dropped.has(g.key)}>
            {#if editable}<td class="ck"><input type="checkbox" checked={!dropped.has(g.key)} onchange={() => toggle(g.key)} aria-label="迁移 {g.key}" /></td>{/if}
            <td class="mono gk">{groupLabel(g.key)}</td>
            <td class="num">{g.count}</td>
            <td>{PTYPE[g.ptype] ?? g.ptype}</td>
            {#if s.gsc_used}<td class="num">{g.clicks || ''}</td>{/if}
            <td class="trunc mono sm" title={g.samples.join('\n')}>{g.samples.slice(0, 2).join('  ')}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if s.top.length}
    <div class="blk"><b class="bt">搜索流量最高的页面 <small>近 16 个月 · Search Console</small>{#if s.top.length > 5}<button class="link-btn sm" onclick={() => (showTop = !showTop)}>{showTop ? '收起' : `全部 ${s.top.length} 个`}</button>{/if}</b>
      <table class="tbl top"><tbody>
        {#each s.top.slice(0, showTop ? 20 : 5) as t (t.path)}<tr><td class="trunc" title={`${t.path}\n${t.title}`}><span class="mono sm">{t.path}</span> <small class="faint">{t.title}</small></td><td class="num">{t.clicks} 点击</td><td class="num faint">{t.impressions} 展示</td></tr>{/each}
      </tbody></table>
    </div>
  {/if}

  {#if editable}
    <div class="blk plan">
      <b class="bt">新站的样子</b>
      <div class="seg" role="tablist">
        <button class:on={visual === 'keep_brand'} onclick={() => (visual = 'keep_brand')}>保留品牌，升级设计</button>
        <button class:on={visual === 'fresh'} onclick={() => (visual = 'fresh')}>全新视觉</button>
      </div>
      <small class="faint">{visual === 'keep_brand' ? '沿用 logo、站名和主色，排版、版式和细节按获奖级网站的标准重做。' : '只保留站名，设计方向重新定。'} 正文一字不改写，URL 一个不变。</small>
      <textarea class="tin" rows="2" bind:value={style} placeholder="补充要求（可选）：比如「更简洁，参考 Stripe 文档的排版」"></textarea>
      <div class="row">
        <button class="btn small primary" onclick={() => confirmPlan(id, { drop_groups: [...dropped], drop_paths: [], visual, style })} disabled={!!busy || !s.hosting.supported || keepN === 0}>{busy === 'plan' ? '处理中…' : '确认方案，开始抓取'}</button>
        <button class="btn small quiet" onclick={() => resurvey(id)} disabled={!!busy}>重新盘点</button>
        <small class="faint">技术栈 Astro · 部署到 Cloudflare Pages</small>
      </div>
    </div>
  {/if}
</div>

<style>
  .sv { display: flex; flex-direction: column; gap: 12px; }
  .meta-line { margin: -2px 0 0; font-size: 11.5px; color: var(--faint); line-height: 1.6; overflow-wrap: anywhere; }
  .host { padding: 10px 12px; border-radius: 10px; background: #fdf8ec; border: 1px solid #f0e4c4; color: #6b4a10; font-size: 12.5px; }
  .host.ok { background: var(--su-ok-soft); border-color: var(--su-ok-line); color: #1f5a37; }
  .hh { display: flex; align-items: center; gap: 8px; }
  .host ul { margin: 6px 0 0; padding-left: 18px; line-height: 1.6; }
  .host small { display: block; margin-top: 6px; }
  .blk { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .bt { font-size: 12.5px; font-weight: 600; display: flex; align-items: baseline; gap: 8px; }
  .bt small { font-weight: 400; color: var(--faint); font-size: 11.5px; }
  .feats { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 5px; }
  .feats li { display: flex; align-items: baseline; gap: 8px; font-size: 12px; }
  .feats small { color: var(--dim); line-height: 1.5; }
  .notes { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .notes li { display: flex; gap: 6px; align-items: flex-start; font-size: 12px; color: var(--dim); line-height: 1.5; }
  .notes li :global(svg) { flex: none; margin-top: 3px; color: var(--faint); }
  .ck { width: 18px; }
  .ck input { accent-color: var(--accent); margin: 0; }
  tr.off td { color: var(--faint); }
  tr.off .gk { text-decoration: line-through; }
  .gk { white-space: nowrap; }
  .sm { font-size: 11.5px; }
  .items { margin: 0; padding: 6px 10px; list-style: none; background: var(--rail); border-radius: 8px; display: flex; flex-direction: column; gap: 2px; max-height: 160px; overflow-y: auto; }
  .items li { font-size: 11.5px; color: var(--dim); }
  .plan { padding-top: 12px; border-top: 1px solid var(--border); gap: 8px; }
  .plan textarea { font-family: inherit; font-size: 12.5px; }
  .link-btn.sm { font-size: 11.5px; }
  .top td.num { width: 1%; padding-left: 16px; }
</style>
