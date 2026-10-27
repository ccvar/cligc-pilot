<script lang="ts">
  // 托管日报（新版）：顶部四个数字 → 需要你 → 今天有动静的站点 → 其余没变化的一行带过 → 流量小表。
  // 旧日报只有 Markdown，由 ManagedConsole 原样显示。
  import Icon from './Icon.svelte';
  import type { DigestData, DigestTraffic } from '$lib/types';
  import { managed } from '$lib/state/managed.svelte.js';
  import { openSite } from '$lib/state/sites.svelte.js';

  let { data }: { data: DigestData } = $props();
  let openDetail = $state<Set<string>>(new Set());
  let showQuiet = $state(false);
  let showNoTraffic = $state(false);
  $effect(() => { void data.date; openDetail = new Set(); showQuiet = false; showNoTraffic = false; });
  function toggle(k: string) { const n = new Set(openDetail); if (n.has(k)) n.delete(k); else n.add(k); openDetail = n; }

  const time = $derived(data.generated_at ? new Date(data.generated_at * 1000).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' }) : '');
  const tiles = $derived([
    { k: 'shipped', l: '自动上线', n: data.shipped, tone: 'ok' },
    { k: 'needs', l: '需要你', n: data.needs.length, tone: 'warn' },
    { k: 'broken', l: '回滚或失败', n: data.broken, tone: 'err' },
    { k: 'planned', l: '出计划', n: data.planned, tone: '' },
  ]);
  const TONE: Record<string, string> = { shipped: 'ok', proposed: 'warn', rolled_back: 'err', failed: 'err', watch_failed: 'err', budget: 'warn', planned: '', fallback: '' };
  const WHY_TONE: Record<string, string> = { 待你确认: 'warn', 失败了: 'err', 上线后待核对: 'warn' };
  // 计划的说明是「；」连起来的任务标题：拆成一条一条
  const parts = (s: string) => s.split(/[；;]\s*/).map((x) => x.trim()).filter(Boolean);
  function delta(now: number, prev: number) {
    const d = now - prev;
    if (!d) return { t: '持平', c: 'flat' };
    if (!prev) return { t: `+${d}`, c: 'up' };
    const pct = Math.round((d / prev) * 100);
    return { t: `${d > 0 ? '+' : ''}${d}（${pct > 0 ? '+' : ''}${pct}%）`, c: d > 0 ? 'up' : 'down' };
  }
  const hasViews = $derived(data.traffic.some((t: DigestTraffic) => t.views || t.views_prev));
</script>

<div class="dg">
  <p class="meta">托管 {data.sites_total} 个站点 · 过去 24 小时{#if time} · {time} 生成{/if}</p>

  <div class="tiles">
    {#each tiles as t (t.k)}
      <div class="tile {t.n ? t.tone : 'zero'}"><b>{t.n}</b><span>{t.l}</span></div>
    {/each}
  </div>

  {#if data.needs.length}
    <section>
      <h4><Icon name="bell" size={13} />需要你<span class="cnt">{data.needs.length}</span><span class="grow"></span><button class="link-btn" onclick={() => (managed.tab = 'needs')}>去处理</button></h4>
      <ul class="rows">
        {#each data.needs as n, i (i)}
          <li><button class="row-btn" onclick={() => (managed.tab = 'needs')}><span class="pill {WHY_TONE[n.why] ?? ''}">{n.why}</span><span class="t">{n.title}</span><span class="site">{n.site_name}</span></button></li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if data.active.length}
    <section>
      <h4><Icon name="activity" size={13} />今天有动静的站点<span class="cnt">{data.active.length}</span></h4>
      <div class="sites">
        {#each data.active as s (s.site_id)}
          <div class="site-card">
            <button class="site-name" onclick={() => openSite(s.site_id, 'overview')} data-tip="打开这个站点">{s.site_name}</button>
            <ul class="events">
              {#each s.events as e, i (i)}
                {@const key = `${s.site_id}:${i}`}
                <li>
                  <div class="ev">
                    <span class="pill {TONE[e.kind] ?? ''}">{e.label}</span>
                    <span class="t">{e.title}</span>
                    {#if e.detail}<button class="ic-btn more" class:on={openDetail.has(key)} onclick={() => toggle(key)} aria-expanded={openDetail.has(key)} aria-label={openDetail.has(key) ? '收起详情' : '展开详情'} data-tip={openDetail.has(key) ? '收起' : '看详情'}><svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>{/if}
                  </div>
                  {#if e.detail && openDetail.has(key)}
                    {#if e.kind === 'planned' && parts(e.detail).length > 1}<ol class="detail">{#each parts(e.detail) as p, j (j)}<li>{p}</li>{/each}</ol>
                    {:else}<p class="detail">{e.detail}</p>{/if}
                  {/if}
                </li>
              {/each}
            </ul>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if data.quiet.length}
    <p class="line"><span>{data.active.length || data.needs.length ? '其余' : '全部'} {data.quiet.length} 个站点今天没有变化</span><button class="link-btn" onclick={() => (showQuiet = !showQuiet)}>{showQuiet ? '收起' : '看名单'}</button></p>
    {#if showQuiet}<p class="names">{data.quiet.join('、')}</p>{/if}
  {/if}

  {#if data.traffic.length}
    <section>
      <h4><Icon name="trend" size={13} />流量<span class="sub">近 28 天，和前 28 天比</span></h4>
      <div class="tbl-wrap">
        <table class="tr">
          <thead><tr><th>站点</th><th class="num">Google 点击</th><th class="num">变化</th>{#if hasViews}<th class="num">浏览量</th><th class="num">变化</th>{/if}</tr></thead>
          <tbody>
            {#each data.traffic as t (t.site_id)}
              {@const dc = delta(t.clicks, t.clicks_prev)}
              {@const dv = delta(t.views, t.views_prev)}
              <tr>
                <td><button class="site-link" onclick={() => openSite(t.site_id, 'overview')}>{t.site_name}</button></td>
                <td class="num">{t.clicks}</td><td class="num {dc.c}">{dc.t}</td>
                {#if hasViews}<td class="num">{t.views}</td><td class="num {dv.c}">{dv.t}</td>{/if}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {/if}
  {#if data.no_traffic.length}
    <p class="line"><span>{data.no_traffic.length} 个站点还没有点击和浏览</span><button class="link-btn" onclick={() => (showNoTraffic = !showNoTraffic)}>{showNoTraffic ? '收起' : '看名单'}</button></p>
    {#if showNoTraffic}<p class="names">{data.no_traffic.join('、')}</p>{/if}
  {/if}
</div>

<style>
  .dg { margin-top: 14px; border-top: 1px solid var(--border); padding-top: 14px; display: flex; flex-direction: column; gap: 16px; }
  .meta { margin: 0; font-size: 12px; color: var(--faint); }
  .tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; }
  .tile { display: flex; flex-direction: column; gap: 2px; padding: 10px 12px; border: 1px solid var(--border); border-radius: 10px; }
  .tile b { font-size: 20px; line-height: 1.2; font-weight: 650; color: var(--text); font-variant-numeric: tabular-nums; }
  .tile span { font-size: 12px; color: var(--dim); }
  .tile.zero b, .tile.zero span { color: var(--faint); }
  .tile.ok b { color: var(--ok); }
  .tile.warn { background: var(--su-warn-soft, #fbf3dc); border-color: transparent; }
  .tile.warn b, .tile.warn span { color: var(--su-warn-ink, #8a5a08); }
  .tile.err { background: var(--err-soft); border-color: transparent; }
  .tile.err b, .tile.err span { color: var(--err); }
  section { display: flex; flex-direction: column; gap: 8px; }
  h4 { margin: 0; display: flex; align-items: center; gap: 6px; font-size: 13px; font-weight: 600; color: var(--text); }
  h4 :global(svg) { color: var(--dim); }
  .cnt { font-size: 11px; font-weight: 500; color: var(--faint); }
  .sub { font-size: 12px; font-weight: 400; color: var(--faint); }
  .grow { flex: 1; }
  .rows { list-style: none; margin: 0; padding: 0; border: 1px solid var(--border); border-radius: 10px; overflow: hidden; }
  .rows > li + li { border-top: 1px solid var(--border); }
  .row-btn { width: 100%; display: flex; align-items: center; gap: 8px; padding: 8px 12px; border: 0; background: none; font: inherit; color: inherit; text-align: left; cursor: pointer; }
  .row-btn:hover { background: var(--rail); }
  .t { min-width: 0; flex: 1; font-size: 12.5px; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .site { flex: none; font-size: 12px; color: var(--faint); }
  .sites { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 10px; }
  .site-card { border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .site-name, .site-link { align-self: flex-start; border: 0; background: none; padding: 0; font: inherit; font-size: 13px; font-weight: 600; color: var(--text); cursor: pointer; }
  .site-link { font-weight: 500; font-size: 12.5px; }
  .site-name:hover, .site-link:hover { text-decoration: underline; text-underline-offset: 2px; }
  .events { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .ev { display: flex; align-items: center; gap: 8px; min-height: 24px; min-width: 0; }
  .ev .more { width: 22px; height: 22px; flex: none; }
  .ev .more svg { transition: transform .16s; }
  .ev .more.on svg { transform: rotate(180deg); }
  .detail { margin: 2px 0 4px; padding: 6px 10px 6px 26px; border-radius: 8px; background: var(--rail); font-size: 12px; line-height: 1.6; color: var(--dim); white-space: pre-wrap; overflow-wrap: anywhere; }
  p.detail { padding-left: 10px; }
  .line { margin: 0; display: flex; align-items: center; gap: 10px; font-size: 12.5px; color: var(--dim); }
  .line .link-btn { font-size: 12px; }
  .names { margin: -8px 0 0; font-size: 12px; line-height: 1.7; color: var(--faint); }
  .tbl-wrap { border: 1px solid var(--border); border-radius: 10px; overflow-x: auto; }
  .tr { width: 100%; border-collapse: collapse; font-size: 12.5px; }
  .tr th { text-align: left; font-weight: 500; font-size: 11.5px; color: var(--faint); padding: 7px 12px; border-bottom: 1px solid var(--border); white-space: nowrap; }
  .tr td { padding: 7px 12px; border-top: 1px solid var(--border); white-space: nowrap; }
  .tr tbody tr:first-child td { border-top: 0; }
  .num { text-align: right !important; font-variant-numeric: tabular-nums; }
  .up { color: var(--ok); }
  .down { color: var(--err); }
  .flat { color: var(--faint); }
  @media (max-width: 720px) { .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
