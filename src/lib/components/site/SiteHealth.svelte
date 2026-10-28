<script lang="ts">
  // 站点概览里的「站点健康」：在线情况、推广链接、页面速度、页面体检合成一张卡。
  // 上面四格各报一个状态（色点 + 一句话 + 关键数字），哪里有问题不用点开就能看到；点哪格下面显示它的详情。
  // 默认打开第一个有问题的格。四个都一直挂着（只是藏起来），这样各自的数据都会读出来、状态都能报上来。
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';
  import SiteMonitor from './SiteMonitor.svelte';
  import SiteLinks from './SiteLinks.svelte';
  import SiteSpeed from './SiteSpeed.svelte';
  import SiteAudit from './SiteAudit.svelte';
  import { say } from '$lib/state/app.svelte.js';
  import { ago, full } from './fmt';
  import type { Site, HealthSum } from '$lib/types';

  let { site }: { site: Site } = $props();
  const TILES = [
    { k: 'mon', l: '在线情况', i: 'activity' },
    { k: 'lk', l: '推广链接', i: 'link' },
    { k: 'spd', l: '页面速度', i: 'gauge' },
    { k: 'aud', l: '页面体检', i: 'checks' },
  ] as const;
  type K = (typeof TILES)[number]['k'];
  let sums = $state<Partial<Record<K, HealthSum>>>({});
  let picked = $state<K | ''>('');
  let busy = $state(false);
  let refs = $state<Partial<Record<K, { check: (quiet?: boolean) => Promise<void> }>>>({});
  $effect(() => { void site.id; sums = {}; picked = ''; });

  // 没点过就打开最要紧的那格：先红后黄，都正常就第一格
  const sel = $derived<K>(picked || (TILES.find((t) => sums[t.k]?.tone === 'err') ?? TILES.find((t) => sums[t.k]?.tone === 'warn') ?? TILES[0]).k);
  const last = $derived(Math.max(0, ...Object.values(sums).map((s) => s?.checked ?? 0)));

  async function checkAll() {
    if (busy) return;
    busy = true;
    try {
      // 速度最慢（一个页面半分钟左右），四项一起跑
      await Promise.allSettled(TILES.map((t) => refs[t.k]?.check(true)));
      await tick();
      const bad = TILES.filter((t) => ['err', 'warn'].includes(sums[t.k]?.tone ?? '')).map((t) => `${t.l}${sums[t.k]?.status}`);
      say(bad.length ? `都查过了：${bad.join('，')}` : '都查过了，没发现问题', bad.length ? 'err' : undefined);
    } finally { busy = false; }
  }
</script>

<!-- 状态图标：正常对勾、提醒感叹号、有问题叉、还没检查时钟（颜色跟着状态） -->
{#snippet mark(tone: string)}
  <svg class="mk" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if tone === 'ok'}<circle cx="12" cy="12" r="9.5" /><path d="m8 12.3 2.7 2.7L16 9.6" />
    {:else if tone === 'warn'}<path d="M10.3 3.9 2.6 17.5A2 2 0 0 0 4.3 20.5h15.4a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" /><path d="M12 9.5v4" /><path d="M12 17h.01" />
    {:else if tone === 'err'}<circle cx="12" cy="12" r="9.5" /><path d="m9 9 6 6M15 9l-6 6" />
    {:else}<circle cx="12" cy="12" r="9.5" /><path d="M12 7.5V12l3 2" />{/if}
  </svg>
{/snippet}

<div class="card hl">
  <h3><Icon name="shield" />站点健康
    <span class="sub">{#if last}<span data-tip={full(last)}>最近 {ago(last)}检查</span>{:else}在线、推广链接、速度、页面都会定期自动检查{/if}</span>
    <span class="grow"></span>
    <button class="btn tiny" onclick={checkAll} disabled={busy} data-tip="四项一起重新查一遍（测速要几分钟）">{#if busy}<span class="spin"></span>检查中…{:else}<Icon name="refresh" size={11} />全部重新检查{/if}</button>
  </h3>
  <div class="tiles" role="tablist">
    {#each TILES as t (t.k)}
      {@const s = sums[t.k]}
      <button class="tile" class:on={sel === t.k} role="tab" aria-selected={sel === t.k} onclick={() => (picked = t.k)}>
        <span class="tl"><Icon name={t.i} size={12} />{t.l}</span>
        <span class="ts {s?.tone ?? 'none'}">{@render mark(s?.tone ?? 'none')}{s?.status ?? '读取中…'}</span>
        <span class="tsub">{s?.sub ?? ''}</span>
      </button>
    {/each}
  </div>
  <div class="detail">
    <div hidden={sel !== 'mon'}><SiteMonitor {site} embedded bind:this={refs.mon} onsummary={(s) => (sums.mon = s)} /></div>
    <div hidden={sel !== 'lk'}><SiteLinks {site} embedded bind:this={refs.lk} onsummary={(s) => (sums.lk = s)} /></div>
    <div hidden={sel !== 'spd'}><SiteSpeed {site} embedded bind:this={refs.spd} onsummary={(s) => (sums.spd = s)} /></div>
    <div hidden={sel !== 'aud'}><SiteAudit {site} embedded bind:this={refs.aud} onsummary={(s) => (sums.aud = s)} /></div>
  </div>
</div>

<style>
  /* 卡片自己不留内边距：四格和详情各自留，详情里列表的分隔线顶到卡片两边 */
  .hl { padding: 0; overflow: hidden; container-type: inline-size; }
  .hl h3 { padding: 14px 18px 0; }
  .hl .sub { font-size: 12px; color: var(--faint); font-weight: 400; }
  .grow { flex: 1; }
  .tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; padding: 4px 18px 14px; }
  .tile { display: flex; flex-direction: column; align-items: flex-start; gap: 3px; min-width: 0; padding: 10px 12px; border: 1px solid var(--border); border-radius: 10px; background: none; font: inherit; text-align: left; cursor: pointer; }
  .tile:hover { background: var(--su-hover, #f6f5f1); }
  .tile.on { border-color: var(--border2, #cfccc2); background: var(--su-hover, #f6f5f1); }
  .tl { display: inline-flex; align-items: center; gap: 5px; font-size: 11.5px; color: var(--dim); }
  .tl :global(svg) { color: var(--faint); }
  .ts { display: inline-flex; align-items: center; gap: 6px; font-size: 13.5px; font-weight: 600; color: var(--text); white-space: nowrap; max-width: 100%; overflow: hidden; text-overflow: ellipsis; }
  .mk { flex: none; color: var(--faint); }
  .ts.ok .mk { color: var(--ok); }
  .ts.warn { color: var(--warn); }
  .ts.warn .mk { color: var(--warn); }
  .ts.err { color: var(--err); }
  .ts.err .mk { color: var(--err); }
  .ts.none { color: var(--dim); font-weight: 500; }
  .tsub { font-size: 11.5px; color: var(--faint); white-space: nowrap; max-width: 100%; overflow: hidden; text-overflow: ellipsis; min-height: 1em; }
  .detail { padding: 14px 18px 16px; border-top: 1px solid var(--border); }
  @container (max-width: 560px) {
    .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }
</style>
