<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import type { DataSourceRow, BindReport } from '$lib/types';
  import GoogleMark from '$lib/components/icons/GoogleMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import { app, say } from '$lib/state/app.svelte.js';
  import { openDataConnect, openDataUpdate } from '$lib/state/datasrc.svelte.js';
  import { openConnCtx } from '$lib/state/connections.svelte.js';
  import { refreshSites } from '$lib/state/sites.svelte.js';
  import { loadFleet } from '$lib/state/fleet.svelte.js';

  // 数据源是全局的：一个 Google / Bing 账号可以服务任意多个站点；站点里只选用哪个资源。
  let rows = $state<DataSourceRow[]>([]);
  let busy = $state('');
  let report = $state<{ conn: string; r: BindReport } | null>(null);
  async function load() {
    try { const v = await invoke<DataSourceRow[]>('data_sources_overview'); rows = v; } catch (e) { say(String(e), 'err'); }
  }
  $effect(() => { void app.conns.length; void load(); });
  async function bind(connId: string, overwrite: boolean) {
    busy = connId;
    try {
      const r = await invoke<BindReport>('auto_bind_sites', { connId, overwrite });
      report = { conn: connId, r };
      await refreshSites(); await load(); void loadFleet();
    } catch (e) { say(String(e), 'err'); }
    finally { busy = ''; }
  }
</script>

<div class="sec-head mt"><span>数据源</span>
  <span class="ds-add">
    <button class="icon-btn" title="连接 Google（GA4 + Search Console）" onclick={() => openDataConnect('google')}><GoogleMark size={14} /></button>
    <button class="icon-btn" title="连接 Bing Webmaster" onclick={() => openDataConnect('bing')}><BingMark size={14} /></button>
  </span>
</div>
<p class="hint">数据源是全局的：一个 Google 或 Bing 账号可以服务所有站点。连上后点「按域名绑定」，Pilot 会把每个站点对应的 GA4 媒体资源、Search Console 资源和 Bing 站点一次配好。</p>
{#if !rows.length}
  <div class="ds-empty">
    <button class="btn small" onclick={() => openDataConnect('google')}><GoogleMark size={13} /> 连接 Google</button>
    <button class="btn small" onclick={() => openDataConnect('bing')}><BingMark size={13} /> 连接 Bing</button>
  </div>
{/if}
<div class="ds-list">
  {#each rows as d (d.conn.id)}
    <div class="ds-row" role="button" tabindex="0" oncontextmenu={(e) => openConnCtx(e, d.conn)}>
      <span class="ds-ic">{#if d.conn.kind === 'google'}<GoogleMark size={18} />{:else}<BingMark size={18} />{/if}</span>
      <span class="ds-main">
        <b>{d.conn.remark?.trim() || d.conn.name}</b>
        <small>{d.conn.kind === 'google' ? `${d.conn.key_kind === 'oauth' ? 'Google 账号' : '服务账号'} · ${d.conn.login}` : 'Bing Webmaster'} · {d.sites.length ? `${d.sites.length} 个站点在用` : '还没有站点在用'}</small>
      </span>
      <button class="btn tiny" onclick={() => bind(d.conn.id, false)} disabled={!!busy} title="给还没绑定的站点按域名自动匹配">{busy === d.conn.id ? '匹配中…' : '按域名绑定'}</button>
      <button class="btn tiny" onclick={() => openDataUpdate(d.conn)}>更新凭据</button>
    </div>
    {#if report && report.conn === d.conn.id}
      <div class="ds-report">
        {#if report.r.bound.length}<p>已绑定 {report.r.bound.length} 个站点：{report.r.bound.slice(0, 8).map(([s, w]) => `${s}（${w}）`).join('；')}{report.r.bound.length > 8 ? ' 等' : ''}</p>{/if}
        {#if report.r.kept}<p class="dim">{report.r.kept} 个站点已经绑好，没有改动。</p>{/if}
        {#if report.r.unmatched.length}<p class="warn">没找到对应资源：{report.r.unmatched.slice(0, 10).join('、')}{report.r.unmatched.length > 10 ? ` 等 ${report.r.unmatched.length} 个` : ''}。{d.conn.kind === 'google' && d.conn.key_kind !== 'oauth' ? '服务账号需要先被加进这些站点的 GA4 / Search Console。' : ''}</p>{/if}
        {#each report.r.errors as e (e)}<p class="warn">{e}</p>{/each}
        {#if !report.r.bound.length && !report.r.unmatched.length && !report.r.kept}<p class="dim">没有填写了域名的站点。</p>{/if}
        <button class="link" onclick={() => bind(d.conn.id, true)} disabled={!!busy}>按域名全部重新绑定（覆盖已有绑定）</button>
      </div>
    {/if}
  {/each}
</div>

<style>
  .ds-add { display: inline-flex; gap: 2px; }
  .ds-empty { display: flex; gap: 8px; margin: 6px 0 2px; }
  .ds-list { display: flex; flex-direction: column; gap: 2px; margin-top: 4px; }
  .ds-row { display: flex; align-items: center; gap: 9px; padding: 6px 8px; border-radius: 9px; }
  .ds-row:hover { background: #f4f3ef; }
  .ds-ic { display: inline-flex; flex: none; }
  .ds-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .ds-main b { font-size: 13px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ds-main small { font-size: 10.5px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ds-report { margin: 0 8px 6px 35px; font-size: 11.5px; line-height: 1.6; color: var(--text); }
  .ds-report p { margin: 2px 0; }
  .ds-report .dim { color: var(--dim); }
  .ds-report .warn { color: var(--warn); }
  .link { border: none; background: none; padding: 0; color: var(--dim); text-decoration: underline; font: inherit; font-size: 11.5px; cursor: pointer; }
  .btn.tiny { padding: 2px 8px; font-size: 11.5px; border: 1px solid var(--border2); border-radius: 6px; background: #fff; cursor: pointer; white-space: nowrap; }
  .btn.tiny:hover { background: #f6f5f1; }
  .btn.tiny:disabled { opacity: .5; }
  .btn.small { display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px; font-size: 12px; border: 1.5px solid var(--border2); border-radius: 8px; background: #fff; cursor: pointer; }
</style>
