<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import type { DataSourceRow } from '$lib/types';
  import ConnGroup from '$lib/components/settings/ConnGroup.svelte';
  import SettingsIcon from '$lib/SettingsIcon.svelte';
  import { app, say } from '$lib/state/app.svelte.js';
  import { openDataConnect, datasrc, autoBindSites } from '$lib/state/datasrc.svelte.js';
  import { connections, toggleConnCtx, openConnCtx } from '$lib/state/connections.svelte.js';
  import { oauth, loadOauthClient } from '$lib/state/oauth.svelte.js';

  // 网站数据的账号（Google / Bing）是全局的：一个账号可以服务任意多个站点；站点里只选用哪个资源。
  // 在「连接」里各是一组（ConnGroup），和 Cloudflare / GitHub 一样一行一条；添加走组头的 + 或设置右上角的 +。
  let rows = $state<DataSourceRow[]>([]);
  // 「匹配站点」在每行 ⚙ 的菜单里（ConnectionMenu），结果显示在那一行下面
  const report = $derived(datasrc.bindReport);
  async function load() {
    try { const v = await invoke<DataSourceRow[]>('data_sources_overview'); rows = v; } catch (e) { say(String(e), 'err'); }
  }
  $effect(() => { void app.conns.length; void datasrc.rowsVer; void load(); void loadOauthClient(); });
  const changeClient = () => openDataConnect('google', 'oauth', { editClient: true });
  const groups = $derived([
    { kind: 'google' as const, label: 'Google', rows: rows.filter((d) => d.conn.kind === 'google') },
    { kind: 'bing' as const, label: 'Bing', rows: rows.filter((d) => d.conn.kind === 'bing') },
  ].filter((g) => g.rows.length));
  // 组名已经说了是 Google / Bing，行上只放账号本身
  const rowName = (d: DataSourceRow) => d.conn.remark?.trim() || d.conn.login || d.conn.name;
  const rowSub = (d: DataSourceRow) => (d.conn.kind === 'google' ? (d.conn.key_kind === 'oauth' ? '' : '服务账号') : d.conn.login && d.conn.remark?.trim() ? d.conn.login : '');
</script>

{#snippet clientBtn()}
  <button class="cg-btn" data-tip={`Google 登录用的 OAuth 客户端（网站数据和 Gmail 共用）\n${oauth.client?.client_id ?? ''}\n点击更改`} aria-label="更改 Google OAuth 客户端" onclick={changeClient}>
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><circle cx="5.5" cy="10.5" r="3" stroke="currentColor" stroke-width="1.4" /><path d="M7.7 8.3 13.5 2.5M11.5 4.5l1.6 1.6M10 6l1.2 1.2" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>
  </button>
{/snippet}

{#each groups as g (g.kind)}
  <ConnGroup kind={g.kind} label={g.label} n={g.rows.length} add={() => openDataConnect(g.kind)} addTip={g.kind === 'google' ? '添加 Google 账号' : '添加 Bing Webmaster'}
    actions={g.kind === 'google' && oauth.client?.configured ? clientBtn : undefined}>
    {#each g.rows as d (d.conn.id)}
      <div class="conn-row static" class:on={connections.connCtx?.conn.id === d.conn.id} role="group" oncontextmenu={(e) => openConnCtx(e, d.conn)}>
        <b class="cr-name">{rowName(d)}</b>
        <small class="cr-sub">{rowSub(d)}</small>
        <span class="used" data-tip={d.sites.length ? d.sites.join('\n') : undefined}>{datasrc.binding === d.conn.id ? '匹配站点中…' : d.sites.length ? `${d.sites.length} 个站点` : '未使用'}</span>
        <button class="icon-btn sm cr-gear" class:active={connections.connCtx?.conn.id === d.conn.id} title="连接设置" aria-label="连接设置" onclick={(e) => toggleConnCtx(e, d.conn)}><SettingsIcon /></button>
      </div>
      {#if report && report.conn === d.conn.id}
        <div class="report">
          {#if report.r.bound.length}<p>已绑定 {report.r.bound.length} 个站点：{report.r.bound.slice(0, 6).map(([s]) => s).join('、')}{report.r.bound.length > 6 ? ' 等' : ''}</p>{/if}
          {#if report.r.kept}<p class="dim">{report.r.kept} 个站点原本已绑好，没有改动。</p>{/if}
          {#if report.r.unmatched.length}<p class="warn">没找到对应资源：{report.r.unmatched.slice(0, 8).join('、')}{report.r.unmatched.length > 8 ? ` 等 ${report.r.unmatched.length} 个` : ''}{d.conn.kind === 'google' && d.conn.key_kind !== 'oauth' ? '（服务账号需要先被加进这些站点的 GA4 / Search Console）' : ''}</p>{/if}
          {#each report.r.errors as e (e)}<p class="warn">{e}</p>{/each}
          {#if !report.r.bound.length && !report.r.unmatched.length && !report.r.kept}<p class="dim">没有填写了域名的站点。</p>{/if}
          <span class="report-acts">
            <button onclick={() => void autoBindSites(d.conn.id, true)} disabled={!!datasrc.binding}>全部重新绑定</button>
            <button onclick={() => (datasrc.bindReport = null)}>收起</button>
          </span>
        </div>
      {/if}
    {/each}
  </ConnGroup>
{/each}

<style>
  .icon-btn { background: none; border: none; cursor: pointer; padding: 6px; border-radius: 8px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .icon-btn:hover { background: #f1efe9; color: var(--text); }
  .icon-btn.sm { padding: 4px; border-radius: 6px; }
  .used { flex: none; font-size: 11px; color: var(--faint); }
  .report { margin: 0 8px 6px 31px; padding: 7px 10px; border-radius: 9px; background: var(--rail); font-size: 11.5px; line-height: 1.55; color: var(--text); }
  .report p { margin: 0 0 2px; }
  .report .dim { color: var(--dim); }
  .report .warn { color: var(--warn); }
  .report-acts { display: flex; gap: 12px; margin-top: 3px; }
  .report-acts button { border: none; background: none; padding: 0; font: inherit; font-size: 11px; color: var(--dim); cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .report-acts button:hover { color: var(--text); }
</style>
