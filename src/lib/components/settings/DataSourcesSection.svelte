<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import type { DataSourceRow, BindReport } from '$lib/types';
  import GoogleMark from '$lib/components/icons/GoogleMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import PlusIcon from '$lib/components/icons/PlusIcon.svelte';
  import SettingsIcon from '$lib/SettingsIcon.svelte';
  import { app, say } from '$lib/state/app.svelte.js';
  import { openDataConnect } from '$lib/state/datasrc.svelte.js';
  import { connections, toggleConnCtx, openConnCtx } from '$lib/state/connections.svelte.js';
  import { refreshSites } from '$lib/state/sites.svelte.js';
  import { loadFleet } from '$lib/state/fleet.svelte.js';

  // 数据源是全局的：一个 Google / Bing 账号可以服务任意多个站点；站点里只选用哪个资源。
  // 版式与上面的「连接」分区一致：小标题 + 「+」菜单 + 卡片行。
  let rows = $state<DataSourceRow[]>([]);
  let busy = $state('');
  let addOpen = $state(false);
  let addEl: HTMLDivElement | undefined = $state();
  let report = $state<{ conn: string; r: BindReport } | null>(null);
  async function load() {
    try { const v = await invoke<DataSourceRow[]>('data_sources_overview'); rows = v; } catch (e) { say(String(e), 'err'); }
  }
  $effect(() => { void app.conns.length; void load(); });
  $effect(() => {
    if (!addOpen) return;
    const close = (e: MouseEvent) => { if (addEl && !addEl.contains(e.target as Node)) addOpen = false; };
    document.addEventListener('mousedown', close, true);
    return () => document.removeEventListener('mousedown', close, true);
  });
  async function bind(connId: string, overwrite: boolean) {
    busy = connId;
    try {
      const r = await invoke<BindReport>('auto_bind_sites', { connId, overwrite });
      report = { conn: connId, r };
      await refreshSites(); await load(); void loadFleet();
    } catch (e) { say(String(e), 'err'); }
    finally { busy = ''; }
  }
  function add(kind: 'google' | 'bing') { addOpen = false; openDataConnect(kind); }
</script>

<div class="sec-head">
  <span>数据源</span>
  <div class="add" bind:this={addEl}>
    <button class="icon-btn add-trigger" class:open={addOpen} aria-label="添加数据源" aria-haspopup="menu" aria-expanded={addOpen}
      data-tip={addOpen ? undefined : '添加数据源'} onclick={() => (addOpen = !addOpen)}><PlusIcon /></button>
    {#if addOpen}
      <div class="add-menu" role="menu">
        <button role="menuitem" onclick={() => add('google')}><GoogleMark size={15} /><span>Google（GA4 + Search Console）</span></button>
        <button role="menuitem" onclick={() => add('bing')}><BingMark size={15} /><span>Bing Webmaster</span></button>
      </div>
    {/if}
  </div>
</div>

{#if !rows.length}
  <div class="empty">
    <button onclick={() => add('google')}><GoogleMark size={16} /><span><b>连接 Google</b><small>GA4 + Search Console</small></span></button>
    <button onclick={() => add('bing')}><BingMark size={16} /><span><b>连接 Bing</b><small>Bing Webmaster</small></span></button>
  </div>
  <p class="hint">一个账号服务所有站点；连上后按域名一次绑定。</p>
{:else}
  <div class="list">
    {#each rows as d (d.conn.id)}
      <div class="row" class:on={connections.connCtx?.conn.id === d.conn.id} role="group" oncontextmenu={(e) => openConnCtx(e, d.conn)}>
        <span class="ic">{#if d.conn.kind === 'google'}<GoogleMark size={20} />{:else}<BingMark size={20} />{/if}</span>
        <span class="main">
          <b>{d.conn.remark?.trim() || d.conn.name}</b>
          <small>{d.conn.kind === 'google' ? (d.conn.key_kind === 'oauth' ? 'Google 账号' : '服务账号') : 'Bing Webmaster'}{d.conn.login ? ` · ${d.conn.login}` : ''}</small>
        </span>
        <span class="used" data-tip={d.sites.length ? d.sites.join('\n') : undefined}>{d.sites.length ? `${d.sites.length} 个站点` : '未使用'}</span>
        <button class="bind" onclick={() => bind(d.conn.id, false)} disabled={!!busy} data-tip="给还没绑定的站点按域名自动匹配资源">{busy === d.conn.id ? '匹配中…' : '按域名绑定'}</button>
        <button class="icon-btn sm" title="数据源设置" aria-label="数据源设置" onclick={(e) => toggleConnCtx(e, d.conn)}><SettingsIcon /></button>
      </div>
      {#if report && report.conn === d.conn.id}
        <div class="report">
          {#if report.r.bound.length}<p>已绑定 {report.r.bound.length} 个站点：{report.r.bound.slice(0, 6).map(([s]) => s).join('、')}{report.r.bound.length > 6 ? ' 等' : ''}</p>{/if}
          {#if report.r.kept}<p class="dim">{report.r.kept} 个站点原本已绑好，没有改动。</p>{/if}
          {#if report.r.unmatched.length}<p class="warn">没找到对应资源：{report.r.unmatched.slice(0, 8).join('、')}{report.r.unmatched.length > 8 ? ` 等 ${report.r.unmatched.length} 个` : ''}{d.conn.kind === 'google' && d.conn.key_kind !== 'oauth' ? '（服务账号需要先被加进这些站点的 GA4 / Search Console）' : ''}</p>{/if}
          {#each report.r.errors as e (e)}<p class="warn">{e}</p>{/each}
          {#if !report.r.bound.length && !report.r.unmatched.length && !report.r.kept}<p class="dim">没有填写了域名的站点。</p>{/if}
          <span class="report-acts">
            <button onclick={() => bind(d.conn.id, true)} disabled={!!busy}>全部重新绑定</button>
            <button onclick={() => (report = null)}>收起</button>
          </span>
        </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .sec-head { display: flex; justify-content: space-between; align-items: center; font-size: 11px; letter-spacing: .03em; text-transform: uppercase; color: var(--faint); font-weight: 600; margin: 12px 0 1px; }
  .icon-btn { background: none; border: none; cursor: pointer; padding: 6px; border-radius: 8px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .icon-btn:hover { background: #f1efe9; color: var(--text); }
  .icon-btn.sm { padding: 4px; border-radius: 7px; }
  .add { position: relative; display: inline-flex; }
  .add-trigger { width: 27px; height: 27px; padding: 0; }
  .add-trigger.open { color: var(--text); background: #f1efe9; }
  .add-menu { position: absolute; z-index: 8; top: calc(100% + 5px); right: 0; width: 236px; padding: 5px; background: var(--panel); border: 1px solid var(--border); border-radius: 10px; box-shadow: 0 10px 28px rgba(30, 25, 15, .15); }
  .add-menu button { width: 100%; min-height: 34px; padding: 7px 9px; border: 0; border-radius: 7px; background: none; display: flex; align-items: center; gap: 9px; color: var(--text); cursor: pointer; text-align: left; font: inherit; font-size: 12.5px; font-weight: 500; letter-spacing: 0; text-transform: none; }
  .add-menu button:hover { background: #f4f3ef; }
  .empty { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; margin-top: 4px; }
  .empty button { display: flex; align-items: center; gap: 9px; padding: 9px 10px; border: 1px dashed var(--border2); border-radius: 11px; background: none; cursor: pointer; text-align: left; font: inherit; color: var(--text); }
  .empty button:hover { background: #faf9f6; border-style: solid; }
  .empty button :global(svg) { flex: none; }
  .empty b { display: block; font-size: 12.5px; font-weight: 550; }
  .empty small { display: block; font-size: 10.5px; color: var(--faint); }
  .hint { color: var(--faint); font-size: 11px; margin: 5px 2px 0; line-height: 1.5; }
  .list { display: flex; flex-direction: column; gap: 5px; margin-top: 2px; }
  .row { display: flex; align-items: center; gap: 10px; padding: 8px 8px 8px 10px; border: 1px solid var(--border); border-radius: 11px; }
  .row:hover, .row.on { background: #faf9f6; }
  .ic { display: inline-flex; flex: none; width: 22px; justify-content: center; }
  .main { flex: 1; min-width: 0; }
  .main b { display: block; font-size: 13.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .main small { display: block; color: var(--dim); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .used { flex: none; font-size: 11px; color: var(--faint); }
  .bind { flex: none; border: 1px solid var(--border2); background: #fff; border-radius: 7px; padding: 3px 8px; font: inherit; font-size: 11.5px; color: var(--text); cursor: pointer; white-space: nowrap; }
  .bind:hover { background: #f6f5f1; }
  .bind:disabled { opacity: .5; cursor: default; }
  .report { margin: -1px 2px 3px; padding: 7px 10px; border-radius: 9px; background: var(--rail); font-size: 11.5px; line-height: 1.55; color: var(--text); }
  .report p { margin: 0 0 2px; }
  .report .dim { color: var(--dim); }
  .report .warn { color: var(--warn); }
  .report-acts { display: flex; gap: 12px; margin-top: 3px; }
  .report-acts button { border: none; background: none; padding: 0; font: inherit; font-size: 11px; color: var(--dim); cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .report-acts button:hover { color: var(--text); }
</style>
