<script lang="ts">
  // 站点「数据」里的收益：本月 / 上月收入（AdSense + 手动记的推广佣金、赞助）、本月 AI 花费和差额、
  // 近 28 天每千次浏览收入（RPM）和收入最多的页面；可以手动记一笔、删掉记错的。金额都按美元。
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Dropdown from '$lib/Dropdown.svelte';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import { say, confirmDialog } from '$lib/state/app.svelte.js';
  import { openConnectionSetup } from '$lib/state/settings.svelte.js';
  import { usd } from '$lib/state/growth.svelte.js';
  import { costTip } from '$lib/state/managed.svelte.js';
  import type { Site, SiteRevenue } from '$lib/types';

  let { site }: { site: Site } = $props();
  let rv = $state<SiteRevenue | null>(null);
  let adding = $state(false);
  const thisMonth = () => { const d = new Date(); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`; };
  let form = $state({ month: thisMonth(), amount: '', source: 'affiliate', note: '' });
  const SOURCES = [{ value: 'affiliate', label: '推广佣金' }, { value: 'sponsor', label: '赞助' }, { value: 'other', label: '其它' }];
  const srcLabel = (s: string) => SOURCES.find((x) => x.value === s)?.label ?? s;

  $effect(() => { const id = site.id; invoke<SiteRevenue>('site_revenue', { id }).then((r) => (rv = r)).catch(() => (rv = null)); });
  async function add() {
    const amount = Number(form.amount);
    if (!Number.isFinite(amount) || amount < 0 || !/^\d{4}-\d{2}$/.test(form.month)) { say('月份选一个，金额填大于等于 0 的数', 'err'); return; }
    try {
      rv = await invoke<SiteRevenue>('revenue_add', { id: site.id, month: form.month, amount, source: form.source, note: form.note });
      form = { ...form, amount: '', note: '' };
      adding = false;
    } catch (e) { say(String(e), 'err'); }
  }
  async function remove(entryId: string, label: string) {
    if (!(await confirmDialog(`删掉这一笔：${label}？`, { title: '删除收益记录', kind: 'danger', confirmText: '删除' }))) return;
    try { rv = await invoke<SiteRevenue>('revenue_remove', { id: site.id, entryId }); } catch (e) { say(String(e), 'err'); }
  }
  const m = $derived(rv?.money);
  const ai = $derived(rv?.cost.usd ?? 0);
  const net = $derived((m?.month ?? 0) - ai);
  const path = (u: string) => { try { return new URL(u).pathname; } catch { return u; } };
</script>

{#if rv && m}
  <div class="card rev">
    <h3><Icon name="coin" />收益 <span class="sub">美元 · {rv.adsense ? 'AdSense 每天更新' : '手动记录'}</span>
      <InfoTip label="收益怎么算" text={'本月＝AdSense 本月到今天的预估收入＋这个月手动记的。\nAI 花费是本月这个站点按官方价能估出来的部分（订阅额度内的不算钱）。\nAdSense 里「网站」列表的域名和站点域名对上才算到这个站点。'} />
      <span class="grow"></span>
      <button class="btn tiny ghost" onclick={() => (adding = !adding)}><Icon name="plus" size={11} />记一笔</button>
    </h3>
    {#if !m.tracked && !adding}
      <p class="hint">还没有收益数据。{#if !rv.adsense}在设置里连上 AdSense 自动拿广告收入，{/if}推广佣金、赞助可以点「记一笔」按月记，或在设置里导入 CSV。
        {#if !rv.adsense}<button class="link-btn" onclick={() => openConnectionSetup()}>去设置</button>{/if}</p>
    {:else}
      <div class="stats">
        <div class="stat"><small>本月</small><b>{usd(m.month)}</b><em>{m.adsense_month ? `AdSense ${usd(m.adsense_month)}` : ' '}</em></div>
        <div class="stat"><small>上月</small><b>{usd(m.last_month)}</b><em>{m.adsense_last ? `AdSense ${usd(m.adsense_last)}` : ' '}</em></div>
        <div class="stat" data-tip={costTip(rv.cost, 0)}><small>本月 AI 花费</small><b>{usd(ai)}</b><em>{rv.cost.sub_tokens ? '另有订阅额度内的' : ' '}</em></div>
        <div class="stat"><small>收益减 AI 花费</small><b class:pos={net > 0} class:neg={net < 0}>{usd(net)}</b><em>{m.rpm ? `每千次浏览 ${usd(m.rpm)}` : ' '}</em></div>
      </div>
    {/if}
    {#if adding}
      <div class="addf">
        <input class="tin" type="month" bind:value={form.month} aria-label="月份" />
        <input class="tin amt" type="number" min="0" step="0.01" placeholder="金额（美元）" bind:value={form.amount} aria-label="金额" />
        <Dropdown compact menuCompact value={form.source} options={SOURCES} ariaLabel="来源" onchange={(v: string) => (form.source = v)} />
        <input class="tin note" placeholder="备注（可不填），如 OKX 返佣" bind:value={form.note} aria-label="备注" />
        <button class="btn tiny" onclick={add} disabled={!form.amount}>记下</button>
        <button class="btn tiny ghost" onclick={() => (adding = false)}>取消</button>
      </div>
    {/if}
    {#if rv.entries.length || rv.top_pages.length}
      <div class="two">
        {#if rv.entries.length}
          <div>
            <div class="lbl">手动记的</div>
            <ul class="ents">
              {#each rv.entries.slice(0, 12) as e (e.id)}
                <li><span class="mono faint">{e.month}</span><span>{srcLabel(e.source)}</span><span class="faint nt" title={e.note}>{e.note}</span><span class="grow"></span><b>{usd(e.amount)}</b>
                  <button class="ic-btn ic-sm" onclick={() => remove(e.id, `${e.month} ${srcLabel(e.source)} ${usd(e.amount)}`)} aria-label="删除这一笔" data-tip="删除这一笔"><Icon name="trash" size={12} /></button></li>
              {/each}
            </ul>
          </div>
        {/if}
        {#if rv.top_pages.length}
          <div>
            <div class="lbl">收入最多的页面 <span class="faint">AdSense · 近 28 天</span></div>
            <table class="tbl">
              <thead><tr><th>页面</th><th class="num">收入</th><th class="num">浏览</th><th class="num">千次收入</th></tr></thead>
              <tbody>{#each rv.top_pages as p (p.url)}<tr><td class="trunc" title={p.url}>{path(p.url)}</td><td class="num">{usd(p.earnings)}</td><td class="num">{Math.round(p.views).toLocaleString()}</td><td class="num">{p.views ? usd((p.earnings / p.views) * 1000) : '—'}</td></tr>{/each}</tbody>
            </table>
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .rev .sub { font-size: 12px; color: var(--faint); font-weight: 400; }
  .rev h3 :global(.info-tip) { margin-left: -4px; }
  .stats { grid-template-columns: repeat(4, minmax(0, 1fr)); }
  .stat b.pos { color: var(--ok); }
  .stat b.neg { color: var(--err); }
  .addf { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 12px; }
  .addf .tin { width: auto; padding: 4.5px 9px; font-size: 12.5px; }
  .addf input[type='month'] { width: 150px; }
  .addf .amt { width: 120px; }
  .addf .note { flex: 1; min-width: 160px; }
  .two { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 18px; margin-top: 14px; }
  .two :global(table.tbl th:first-child) { width: 46%; }
  .lbl { font-size: 12px; font-weight: 600; margin-bottom: 6px; }
  .lbl .faint { font-weight: 400; margin-left: 4px; }
  .ents { list-style: none; margin: 0; padding: 0; }
  .ents li { display: flex; align-items: center; gap: 8px; padding: 5px 0; border-top: 1px solid var(--border); font-size: 12.5px; }
  .ents li:first-child { border-top: none; }
  .ents .nt { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ents b { font-weight: 600; font-variant-numeric: tabular-nums; }
  .grow { flex: 1; }
  .mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11.5px; }
</style>
