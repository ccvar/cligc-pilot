<script lang="ts">
  // 站点概览里的「推广链接」：每周自动把站内 /go/… 跳转和带邀请参数的站外链接访问一遍，看最后落在哪。
  // 失效的排前面、写清原因；被目标网站拦了自动访问的标「没法确认」；正常的收起来，点开看。
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { say } from '$lib/state/app.svelte.js';
  import { ago, full } from './fmt';
  import type { Site, HealthSum, LinkReport } from '$lib/types';

  let { site, embedded = false, onsummary }: { site: Site; embedded?: boolean; onsummary?: (s: HealthSum) => void } = $props();
  let rep = $state<LinkReport | null>(null);
  let busy = $state(false);
  let showOk = $state(false);

  async function load(id: string) {
    try { rep = await invoke<LinkReport>('site_links', { id }); } catch { rep = null; }
  }
  $effect(() => { const id = site.id; showOk = false; void load(id); });
  /** quiet：「全部重新检查」时不各自弹提示，出错照样提示 */
  export async function check(quiet = false) {
    if (busy) return;
    busy = true;
    try {
      rep = await invoke<LinkReport>('check_site_links', { id: site.id });
      const b = rep.links.filter((l) => l.verdict === 'broken').length;
      if (!quiet || b) say(rep.links.length ? (b ? `查了 ${rep.links.length} 个推广链接，${b} 个失效` : `查了 ${rep.links.length} 个推广链接，都能用`) : '没找到推广链接', b ? 'err' : undefined);
    } catch (e) { say(String(e), 'err'); }
    finally { busy = false; }
  }

  const bad = $derived(rep?.links.filter((l) => l.verdict !== 'ok') ?? []);
  const good = $derived(rep?.links.filter((l) => l.verdict === 'ok') ?? []);
  const broken = $derived(bad.filter((l) => l.verdict === 'broken').length);
  const host = (u: string) => { try { return new URL(u).host.replace(/^www\./, ''); } catch { return u; } };
  $effect(() => {
    if (!rep) return;
    const unv = bad.length - broken;
    onsummary?.({
      tone: !rep.checked_at || !rep.links.length ? 'none' : broken ? 'err' : unv ? 'warn' : 'ok',
      status: !rep.checked_at ? '还没检查' : !rep.links.length ? '没有推广链接' : broken ? `${broken} 个失效` : unv ? `${unv} 个没法确认` : '都能用',
      sub: rep.links.length ? `共 ${rep.links.length} 个` : '每周检查一次',
      checked: rep.checked_at,
    });
  });
</script>

<div class="lk" class:card={!embedded}>
  {#if embedded}<div class="ph">{@render head()}</div>
  {:else}<h3><Icon name="link" />推广链接{@render head()}</h3>{/if}
  {#if !rep?.checked_at}
    <p class="hint">会找出站内的 <code>/go/…</code> 跳转和带邀请参数的站外链接，真的访问一遍：打不开、404、跳回本站的算失效，发系统通知提醒你。</p>
  {:else if !rep.links.length}
    <p class="hint">没找到推广链接（站内 <code>/go/…</code> 跳转，或带 ref / invite / affiliate 等参数的站外链接）。</p>
  {:else}
    <ul class="rows">
      {#each bad as l (l.url)}
        {@render row(l)}
      {/each}
      {#if good.length}
        <li class="okline"><button class="link-btn" onclick={() => (showOk = !showOk)}>{showOk ? '收起' : `${good.length} 个正常`}</button></li>
        {#if showOk}{#each good as l (l.url)}{@render row(l)}{/each}{/if}
      {/if}
    </ul>
  {/if}
</div>

{#snippet row(l: LinkReport['links'][number])}
  <li>
    <span class="pill {l.verdict === 'broken' ? 'err' : l.verdict === 'unverified' ? 'warn' : 'ok'}">{l.verdict === 'broken' ? '失效' : l.verdict === 'unverified' ? '没法确认' : '正常'}</span>
    <button class="href mono" onclick={() => openUrl(l.url)} data-tip={`在浏览器打开：${l.url}`}>{l.href}</button>
    <span class="to faint" data-tip={l.final_url}>→ {host(l.final_url)}{l.hops ? `（跳了 ${l.hops} 次）` : ''}</span>
    <span class="grow"></span>
    <span class="pg faint" data-tip={`例如 ${l.sample_page}`}>用在 {l.pages} 个页面</span>
    {#if l.note}<p class="note" class:err-t={l.verdict === 'broken'}>{l.note}</p>{/if}
  </li>
{/snippet}

{#snippet head()}
    <span class="sub">{#if !rep?.checked_at}每周自动检查一次{:else}{rep.links.length} 个{#if broken} · <span class="err-t">{broken} 个失效</span>{/if} · <span data-tip={full(rep.checked_at)}>{ago(rep.checked_at)}检查</span>{#if rep.source === 'live'} · 只看了线上首页{/if}{/if}</span>
    <span class="grow"></span>
    <button class="btn tiny" onclick={() => check()} disabled={busy} data-tip="把这个站点的推广链接都访问一遍，跟到最后落地的页面">{#if busy}<span class="spin"></span>检查中…{:else}<Icon name="refresh" size={11} />立即检查{/if}</button>
{/snippet}

<style>
  .ph { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; min-height: 26px; }
  .ph .sub { font-size: 12px; color: var(--faint); }
  .ph .grow { flex: 1; }
  .lk .sub { font-size: 12px; color: var(--faint); font-weight: 400; }
  .err-t { color: var(--err); }
  .rows { list-style: none; margin: 0 -18px -16px; padding: 0; }
  .rows li { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 10px; padding: 9px 18px; border-top: 1px solid var(--border); font-size: 12.5px; }
  .rows li:last-child { border-radius: 0 0 12px 12px; }
  .href { border: 0; background: none; padding: 0; font-size: 12px; color: var(--text); cursor: pointer; }
  .href:hover { text-decoration: underline; text-underline-offset: 3px; }
  .to { font-size: 11.5px; max-width: 280px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pg { font-size: 11.5px; white-space: nowrap; }
  .grow { flex: 1; }
  .note { flex-basis: 100%; margin: 0; padding-left: 2px; font-size: 11.5px; color: var(--dim); }
  .okline { justify-content: center; }
  .okline .link-btn { font-size: 12px; color: var(--dim); }
  code { font-size: 11.5px; }
</style>
