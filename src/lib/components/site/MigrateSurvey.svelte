<script lang="ts">
  // 迁移的盘点报告：页数、分组（可以勾掉不迁的）、流量高的页面、动态功能、部署去向和理由。
  // editable 时在下面确认方案（视觉方向 + 补充要求），确认后直接开始抓取。
  import './site.css';
  import Icon from './Icon.svelte';
  import type { MView } from '$lib/state/migrate.svelte.js';
  import { migrate, confirmPlan, resurvey, previewHosting } from '$lib/state/migrate.svelte.js';
  import type { MHosting } from '$lib/state/migrate.svelte.js';

  let { id, v, editable, oncancel }: { id: string; v: MView; editable: boolean; oncancel?: () => void } = $props();
  const s = $derived(v.survey!);
  const busy = $derived(migrate.busy[id] ?? '');
  // 方案草稿：默认全部保留
  let dropped = $state<Set<string>>(new Set());
  let visual = $state<'keep_brand' | 'fresh'>('keep_brand');
  let style = $state('');
  /** 动态功能保留 / 去掉（站内搜索固定改成静态，不用选） */
  let feats = $state<Record<string, 'keep' | 'drop'>>({});
  /** 部署到哪：Cloudflare（默认）或自己的服务器 */
  let deploy = $state<'direct' | 'vps'>('direct');
  /** 确认后自动往下做（默认开） */
  let auto = $state(true);
  let inited = '';
  $effect(() => {
    const k = `${id}:${v.plan?.confirmed_at ?? 0}`;
    if (k === inited) return;
    inited = k;
    dropped = new Set(v.plan?.drop_groups ?? []);
    visual = v.plan?.visual ?? 'keep_brand';
    style = v.plan?.style ?? '';
    deploy = v.plan?.deploy === 'vps' || (!v.plan && (s.cn_share ?? 0) >= 0.5) ? 'vps' : 'direct';
    // 默认保留表单、评论；登录、购物车做不了，默认去掉（不去掉就要 VPS）
    feats = Object.fromEntries(s.features.filter((f) => f.key !== 'search').map((f) => [f.key, v.plan?.features?.[f.key] ?? (f.key === 'login' || f.key === 'cart' ? 'drop' : 'keep')]));
  });
  // 选择变了就重新判断部署去向
  let hosting = $state<MHosting | null>(null);
  $effect(() => {
    const f = { ...feats };
    if (!editable) { hosting = v.hosting; return; }
    void previewHosting(id, f).then((h) => { hosting = h; });
  });
  const host = $derived(hosting ?? v.hosting ?? s.hosting);
  function toggle(g: string) { const n = new Set(dropped); if (n.has(g)) n.delete(g); else n.add(g); dropped = n; }
  const keepN = $derived(s.groups.filter((g) => !dropped.has(g.key)).reduce((a, g) => a + g.count, 0));
  const PTYPE: Record<string, string> = { home: '首页', article: '文章', list: '列表', page: '单页' };
  let showBroken = $state(false);
  let showTop = $state(false);
  // 盘点那一步回头看时：建议是当时的，下面写新站现在实际在哪，免得以为还有事没做
  const nowText = $derived.by(() => {
    const preview = v.preview.replace(/^https?:\/\//, '').replace(/\/$/, '');
    const where = v.plan?.deploy === 'vps' ? `你的服务器${v.vps?.ip ? ` ${v.vps.ip}` : ''}` : `Cloudflare Pages${v.deploy_ready && preview ? `（${preview}）` : ''}`;
    if (v.meta.cutover.at) return `新站现在就在 ${where}，已经正式上线到 ${v.meta.cutover.host}`;
    if (v.meta.live.at) return `新站已经部署到 ${where}，还没正式上线（旧站照常）`;
    if (v.deploy_ready) return `预览用的地方已经建好（${where}），到「核对」这一步会部署上去`;
    return '新站还没部署：到「核对」这一步会自动建好、部署上去，不用另外操作';
  });
  // 分组的通配写法（/es-ar/*/*）换成人话：「/es-ar/ 下两层的页面」
  const DEPTH = ['', '一层', '两层', '三层', '四层', '五层'];
  const hasClicks = $derived(s.groups.some((g) => g.clicks > 0));
  const groupLabel = (k: string) => {
    if (k === '/') return '首页';
    const stars = (k.match(/\*/g) ?? []).length;
    if (!stars) return k;
    const prefix = k.slice(0, k.indexOf('*'));
    return prefix === '/' ? `${DEPTH[stars] ?? stars + ' 层'}的页面` : `${prefix} 下${DEPTH[stars] ?? stars + ' 层'}的页面`;
  };
</script>

<div class="sv">
  {#if editable}
    <!-- 确认方案：要你决定的三件事放最上面（都有默认值），盘点报告收起来 -->
    <p class="summary">找到 <b>{s.pages}</b> 个网页、<b>{s.assets}</b> 个图片和附件{#if s.gsc_used}，已用 Search Console 补全有搜索流量的页面{/if}。将迁 <b>{keepN}</b> 页，地址一个不变。</p>
  {#if host.paid || !host.supported}
  <div class="host" class:ok={host.supported}>
    <div class="hh"><b>建议放在：{host.label}</b>{#if host.paid}<span class="pill warn">需要付费套餐</span>{/if}{#if !host.supported}<span class="pill warn">还不支持</span>{/if}</div>
    <ul>{#each host.reasons as r (r)}<li>{r}</li>{/each}</ul>
    {#if !host.supported}<small>部署到 VPS 还不支持。{s.features.some((f) => f.key === 'login' || f.key === 'cart') ? '登录、购物车这类功能可以在下面选「去掉」，就能放到 Cloudflare。' : ''}</small>{/if}
  </div>
  {/if}

  {#if s.features.length}
    <div class="blk"><b class="bt">动态功能{#if editable}<small>选保留还是去掉</small>{/if}</b>
      <ul class="feats">
        {#each s.features as f (f.key)}
          <li>
            <span class="pill {f.static_ok ? 'ok' : feats[f.key] === 'drop' ? '' : 'warn'}">{f.label} · {f.pages} 页</span>
            <small>{f.key === 'search' ? f.plan : feats[f.key] === 'drop' ? '去掉：新站上不放这个功能的入口' : f.plan}</small>
            {#if editable && f.key !== 'search'}
              <span class="grow"></span>
              <div class="seg mini" role="tablist">
                <button class:on={feats[f.key] !== 'drop'} onclick={() => (feats = { ...feats, [f.key]: 'keep' })}>保留</button>
                <button class:on={feats[f.key] === 'drop'} onclick={() => (feats = { ...feats, [f.key]: 'drop' })}>去掉</button>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/if}

    <div class="blk plan">
      <b class="bt">部署到哪</b>
      <div class="seg" role="tablist">
        <button class:on={deploy === 'direct' && host.target !== 'vps'} disabled={host.target === 'vps'} onclick={() => (deploy = 'direct')}>Cloudflare{host.target !== 'vps' && (s.cn_share ?? 0) < 0.5 ? '（推荐）' : ''}</button>
        <button class:on={deploy === 'vps' || host.target === 'vps'} onclick={() => (deploy = 'vps')}>自己的服务器{host.target === 'vps' || (s.cn_share ?? 0) >= 0.5 ? '（推荐）' : ''}</button>
      </div>
      <small class="faint">{deploy === 'vps' || host.target === 'vps' ? '放在你自己的服务器上：Pilot 替你装好网页服务、免费 HTTPS 证书和表单评论服务，不用懂运维。访客主要在中国大陆时更合适（大陆的服务器要先 ICP 备案）。' : `${host.label}：全球都快、免费额度够用、不用维护服务器。`}</small>
      <b class="bt">新站的样子</b>
      <div class="seg" role="tablist">
        <button class:on={visual === 'keep_brand'} onclick={() => (visual = 'keep_brand')}>保留品牌，升级设计</button>
        <button class:on={visual === 'fresh'} onclick={() => (visual = 'fresh')}>全新视觉</button>
      </div>
      <small class="faint">{visual === 'keep_brand' ? '沿用 logo、站名和主色，排版、版式和细节按获奖级网站的标准重做。' : '只保留站名，设计方向重新定。'} 正文一字不改写，URL 一个不变。</small>
      <textarea class="tin" rows="2" bind:value={style} placeholder="对新站样子的补充要求（可选）：比如「更简洁，参考 Stripe 文档的排版」"></textarea>
      <label class="auto-ck"><input type="checkbox" bind:checked={auto} />确认后自动往下做：抓取、画设计方向、重做、核对、部署到预览，到挑设计方向和正式上线时停下来等你</label>
      <div class="row">
        <button class="btn small primary" onclick={() => confirmPlan(id, { drop_groups: [...dropped], drop_paths: [], visual, style, features: feats, deploy: host.target === 'vps' ? 'vps' : deploy, auto })} disabled={!!busy || !host.supported || keepN === 0}>{busy === 'plan' ? '处理中…' : '确认方案，开始抓取'}</button>
        {#if oncancel}<button class="btn small quiet" onclick={oncancel} disabled={!!busy}>取消，不改了</button>{/if}
        <span class="grow"></span>
        <button class="link-btn sm" onclick={() => resurvey(id)} disabled={!!busy} data-tip="重新找一遍旧站的页面（页面有增减时用）">重新盘点</button>
      </div>
    </div>
      <details class="report">
      <summary>盘点详情：页面分组（可以勾掉不迁的）、流量最高的页面、打不开的地址</summary>
      <div class="report-body">
  <div class="stats">
    <div class="stat"><small>网页</small><b>{s.pages}</b>{#if s.truncated}<em>达到上限</em>{/if}</div>
    <div class="stat"><small>图片和附件</small><b>{s.assets}</b><em>{s.files ? `其中下载文件 ${s.files}` : ''}</em></div>
    <div class="stat"><small>订阅源（RSS）</small><b>{s.feeds}</b></div>
    <div class="stat"><small>旧站已有跳转</small><b>{s.redirects}</b></div>
    <div class="stat"><small>打不开的地址</small><b>{s.broken.length}</b>{#if s.broken.length}<em><button class="link-btn" onclick={() => (showBroken = !showBroken)}>{showBroken ? '收起' : '查看'}</button></em>{/if}</div>
  </div>
  {#if showBroken}<ul class="items">{#each s.broken as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
  <p class="meta-line">
    {s.origin}
    {#if s.js_rendered} · 页面靠浏览器脚本生成（已用浏览器抓取）{/if}
    {#if s.generator} · {s.generator}{/if}
    {#if s.analytics.length} · 已装 Google Analytics（{s.analytics.join('、')}），新站沿用{/if}
    {#if s.gsc_used} · 已用 Search Console 补漏{/if}
  </p>

  {#if s.notes.length}
    <ul class="notes">{#each s.notes as n (n)}<li><Icon name="info" size={12} />{n}</li>{/each}</ul>
  {/if}

  <div class="blk">
    <b class="bt">页面分组 <small>{editable ? `勾掉不想迁的组 · 将迁 ${keepN} 页` : `${keepN} 页`}</small></b>
    <table class="tbl">
      <thead><tr>{#if editable}<th></th>{/if}<th>分组</th><th class="num">页数</th><th>类型</th>{#if hasClicks}<th class="num">搜索点击</th>{/if}<th>示例</th></tr></thead>
      <tbody>
        {#each s.groups as g (g.key)}
          <tr class:off={dropped.has(g.key)}>
            {#if editable}<td class="ck"><input type="checkbox" checked={!dropped.has(g.key)} onchange={() => toggle(g.key)} aria-label="保留 {g.key}" /></td>{/if}
            <td class="mono gk">{groupLabel(g.key)}</td>
            <td class="num">{g.count}</td>
            <td>{PTYPE[g.ptype] ?? g.ptype}</td>
            {#if hasClicks}<td class="num">{g.clicks || ''}</td>{/if}
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

      </div>
    </details>
  {:else}
  <div class="stats">
    <div class="stat"><small>网页</small><b>{s.pages}</b>{#if s.truncated}<em>达到上限</em>{/if}</div>
    <div class="stat"><small>图片和附件</small><b>{s.assets}</b><em>{s.files ? `其中下载文件 ${s.files}` : ''}</em></div>
    <div class="stat"><small>订阅源（RSS）</small><b>{s.feeds}</b></div>
    <div class="stat"><small>旧站已有跳转</small><b>{s.redirects}</b></div>
    <div class="stat"><small>打不开的地址</small><b>{s.broken.length}</b>{#if s.broken.length}<em><button class="link-btn" onclick={() => (showBroken = !showBroken)}>{showBroken ? '收起' : '查看'}</button></em>{/if}</div>
  </div>
  {#if showBroken}<ul class="items">{#each s.broken as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
  <p class="meta-line">
    {s.origin}
    {#if s.js_rendered} · 页面靠浏览器脚本生成（已用浏览器抓取）{/if}
    {#if s.generator} · {s.generator}{/if}
    {#if s.analytics.length} · 已装 Google Analytics（{s.analytics.join('、')}），新站沿用{/if}
    {#if s.gsc_used} · 已用 Search Console 补漏{/if}
  </p>

  <div class="host" class:ok={host.supported}>
    <div class="hh"><b>盘点时的建议：放在 {host.label}</b>{#if host.paid}<span class="pill warn">需要付费套餐</span>{/if}{#if !host.supported}<span class="pill warn">还不支持</span>{/if}</div>
    <ul>{#each host.reasons as r (r)}<li>{r}</li>{/each}</ul>
    {#if !host.supported}<small>部署到 VPS 还不支持。{s.features.some((f) => f.key === 'login' || f.key === 'cart') ? '登录、购物车这类功能可以在下面选「去掉」，就能放到 Cloudflare。' : ''}</small>{/if}
    <p class="now"><Icon name="info" size={12} />{nowText}</p>
  </div>

  {#if s.features.length}
    <div class="blk"><b class="bt">动态功能{#if editable}<small>选保留还是去掉</small>{/if}</b>
      <ul class="feats">
        {#each s.features as f (f.key)}
          <li>
            <span class="pill {f.static_ok ? 'ok' : feats[f.key] === 'drop' ? '' : 'warn'}">{f.label} · {f.pages} 页</span>
            <small>{f.key === 'search' ? f.plan : feats[f.key] === 'drop' ? '去掉：新站上不放这个功能的入口' : f.plan}</small>
            {#if editable && f.key !== 'search'}
              <span class="grow"></span>
              <div class="seg mini" role="tablist">
                <button class:on={feats[f.key] !== 'drop'} onclick={() => (feats = { ...feats, [f.key]: 'keep' })}>保留</button>
                <button class:on={feats[f.key] === 'drop'} onclick={() => (feats = { ...feats, [f.key]: 'drop' })}>去掉</button>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if s.notes.length}
    <ul class="notes">{#each s.notes as n (n)}<li><Icon name="info" size={12} />{n}</li>{/each}</ul>
  {/if}

  <div class="blk">
    <b class="bt">页面分组 <small>{editable ? `勾掉不想迁的组 · 将迁 ${keepN} 页` : `${keepN} 页`}</small></b>
    <table class="tbl">
      <thead><tr>{#if editable}<th></th>{/if}<th>分组</th><th class="num">页数</th><th>类型</th>{#if hasClicks}<th class="num">搜索点击</th>{/if}<th>示例</th></tr></thead>
      <tbody>
        {#each s.groups as g (g.key)}
          <tr class:off={dropped.has(g.key)}>
            {#if editable}<td class="ck"><input type="checkbox" checked={!dropped.has(g.key)} onchange={() => toggle(g.key)} aria-label="保留 {g.key}" /></td>{/if}
            <td class="mono gk">{groupLabel(g.key)}</td>
            <td class="num">{g.count}</td>
            <td>{PTYPE[g.ptype] ?? g.ptype}</td>
            {#if hasClicks}<td class="num">{g.clicks || ''}</td>{/if}
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
  .host .now { display: flex; align-items: center; gap: 6px; margin: 8px 0 0; padding-top: 8px; border-top: 1px solid currentColor; border-top-color: rgba(31, 90, 55, .15); font-weight: 600; }
  .blk { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .bt { font-size: 12.5px; font-weight: 600; display: flex; align-items: baseline; gap: 8px; }
  .bt small { font-weight: 400; color: var(--faint); font-size: 11.5px; }
  .feats { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 5px; }
  .feats li { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  .feats .grow { flex: 1; }
  .seg.mini button { padding: 2px 9px; font-size: 11.5px; }
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
  .plan { padding-top: 4px; gap: 8px; }
  .plan .row .grow { flex: 1; }
  .summary { margin: 0; font-size: 13px; color: var(--dim); line-height: 1.6; }
  .summary b { color: var(--text); font-weight: 600; }
  .report { border-top: 1px solid var(--border); padding-top: 10px; }
  .report summary { cursor: pointer; font-size: 12.5px; color: var(--dim); }
  .report summary:hover { color: var(--text); }
  .report-body { display: flex; flex-direction: column; gap: 12px; margin-top: 12px; }
  .auto-ck { display: flex; align-items: flex-start; gap: 8px; font-size: 12.5px; color: var(--dim); line-height: 1.5; margin-top: 4px; }
  .auto-ck input { margin-top: 3px; accent-color: var(--accent); }
  .plan textarea { font-family: inherit; font-size: 12.5px; }
  .link-btn.sm { font-size: 11.5px; }
  .top td.num { width: 1%; padding-left: 16px; }
</style>
