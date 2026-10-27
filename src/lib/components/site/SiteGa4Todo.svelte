<script lang="ts">
  // 「需要你处理」里由 Pilot 自己加的一条：接入 GA4 统计。能自动的自动，要写入的点一下，手动填写只是兜底：
  // - 已绑定 GA4：打开就自动读出衡量 ID 存进配置变量（只读，不用点）；
  // - 没绑定、有管理权限的 Google 账号：一键创建并接入；
  // - 都不行或读取失败：才展开手动填写。
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import { app } from '$lib/state/app.svelte.js';
  import { launch } from '$lib/state/launch.svelte.js';
  import { siteMake, ga4ToConfig, wireGa, gaNeedsWiring } from '$lib/state/siteMake.svelte.js';

  let { site }: { site: Site } = $props();
  const manage = $derived(launch.status[site.id]?.google_manage_conn ?? '');
  const bound = $derived(!!site.data.ga4_property);
  const canRead = $derived(bound && !!site.data.google_conn_id);
  const g = $derived(siteMake.ga[site.id] ?? {});
  const busyAi = $derived(app.convos.some((c) => c.conn_id === `site:${site.id}` && c.status === 'running'));
  // 自动走不通时才默认展开手动填写
  const autoPath = $derived(!g.error && (canRead || (!!manage && !!site.domain)));
  let showOwn = $state(false);
  const own = $derived(showOwn || !autoPath);
  let mid = $state('');

  // 值已经存了、只差代码接上：不再去 Google 读
  const wiring = $derived(gaNeedsWiring(site) || (!!g.mid && !g.declared));
  $effect(() => { if (canRead && !g.tried && !g.mid && !gaNeedsWiring(site)) void ga4ToConfig(site, '', '', true); });
</script>

<li class="ga">
  <div class="t-head"><b>接入 GA4 统计</b><span class="tag pilot">Pilot 帮你做</span></div>
  {#if wiring}
    <p class="why">衡量 ID {#if g.mid}<code>{g.mid}</code> {/if}已经存进配置变量 <code>{g.key || 'PUBLIC_GA_ID'}</code>，但这个站点的代码还没读这个变量，统计代码不会加载。交给 AI 接上：声明进 .env.example、有值才加载 gtag.js，并同步更新隐私政策。</p>
    <div class="acts"><button class="btn small primary" disabled={busyAi || g.wired} onclick={() => wireGa(site)}><Icon name="wand" size={12} />{g.wired ? '已交给 AI' : '交给 AI 接上'}</button></div>
  {:else}
    <p class="why">看访问量、来源和转化；上线后托管运营也要靠这些数据判断哪些页面该改。衡量 ID 存进配置变量 <code>PUBLIC_GA_ID</code>，不写进代码。</p>
    {#if g.working || g.error || !bound || (autoPath && !showOwn)}
    <div class="acts">
      {#if g.working}
        <span class="note"><span class="spin"></span>{bound ? '正在读取已绑定的 GA4…' : '正在创建 GA4 媒体资源…'}</span>
      {:else if g.error}
        <span class="err">{bound ? '自动读取没成功' : '创建没成功'}：{g.error}</span>
        {#if canRead}<button class="btn small" onclick={() => ga4ToConfig(site)}><Icon name="refresh" size={11} />重试</button>{/if}
      {:else if !bound && manage && site.domain}
        <button class="btn small primary" disabled={busyAi} onclick={() => ga4ToConfig(site, manage)}><Icon name="wand" size={12} />用 Google 账号创建并接入</button>
      {:else if !bound && manage}
        <span class="note">先填正式域名（GA4 的网页数据流要用），就能一键创建。</span>
      {:else if !bound}
        <span class="note">还没有能建 GA4 的 Google 账号：到连接里用浏览器登录一个，或手动填已有的衡量 ID。</span>
      {/if}
      {#if autoPath && !showOwn}<button class="link-btn own-link" onclick={() => (showOwn = true)}>手动填写衡量 ID</button>{/if}
    </div>
    {/if}
    {#if own}
      <div class="own">
        <div class="group">
          <input class="mono" bind:value={mid} placeholder="G-XXXXXXXXXX" spellcheck="false" autocapitalize="characters" aria-label="GA4 衡量 ID" onkeydown={(e) => { if (e.key === 'Enter' && mid.trim()) void ga4ToConfig(site, '', mid); }} />
          <button disabled={g.working || busyAi || !mid.trim()} onclick={() => ga4ToConfig(site, '', mid)}>保存</button>
        </div>
        {#if autoPath}<button class="link-btn own-link" onclick={() => (showOwn = false)}>收起</button>{/if}
      </div>
    {/if}
  {/if}
</li>

<style>
  .ga { border: 1px solid #d8e6f5; background: #f6faff; border-radius: 10px; padding: 10px 12px; display: flex; flex-direction: column; gap: 7px; list-style: none; }
  .t-head { display: flex; align-items: center; gap: 8px; }
  .t-head b { font-size: 13px; font-weight: 600; }
  .tag { font-size: 10.5px; padding: 1px 7px; border-radius: 999px; }
  .tag.pilot { background: #e3eefb; color: #2a5d9a; font-weight: 500; }
  .why { margin: 0; font-size: 12px; color: var(--dim); line-height: 1.5; }
  .acts { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .note { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--faint); }
  .err { font-size: 12px; color: var(--err); }
  .own-link { font-size: 12px; color: var(--faint); }
  .own-link:hover { color: var(--text); }
  .own { display: flex; align-items: center; gap: 10px; }
  /* 输入框和按钮拼成一组：同一高度、同一圆角，中间一条分隔线 */
  .group { display: inline-flex; align-items: stretch; height: 30px; border: 1px solid var(--border2); border-radius: 8px; background: var(--card, #fff); overflow: hidden; transition: border-color .12s, box-shadow .12s; }
  .group:focus-within { border-color: var(--text); }
  .group input { width: 180px; border: none; outline: none; background: none; padding: 0 10px; font-size: 12.5px; color: var(--text); }
  .group input::placeholder { color: var(--faint); }
  .group button { border: none; border-left: 1px solid var(--border); background: #f7f6f2; padding: 0 14px; font: inherit; font-size: 12.5px; font-weight: 500; color: var(--text); cursor: pointer; transition: background-color .12s, color .12s; }
  .group button:hover:not(:disabled) { background: #efede7; }
  .group button:disabled { color: var(--faint); cursor: default; }
  .spin { width: 10px; height: 10px; border: 1.5px solid var(--border2); border-top-color: var(--dim); border-radius: 50%; animation: ga-spin .7s linear infinite; }
  @keyframes ga-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
</style>
