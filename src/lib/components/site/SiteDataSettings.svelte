<script lang="ts">
  import './site.css';
  import type { Site } from '$lib/types';
  import Dropdown from '$lib/Dropdown.svelte';
  import GoogleMark from '$lib/components/icons/GoogleMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { app, say } from '$lib/state/app.svelte.js';
  import { refreshSites } from '$lib/state/sites.svelte.js';
  import { datasrc, openDataConnect, loadGoogleRes, loadBingSites } from '$lib/state/datasrc.svelte.js';

  let { site, draft }: { site: Site; draft: Site } = $props();
  const googleConns = $derived(app.conns.filter((c) => c.kind === 'google'));
  const bingConns = $derived(app.conns.filter((c) => c.kind === 'bing'));
  const gres = $derived(draft.data.google_conn_id ? datasrc.googleRes[draft.data.google_conn_id] : undefined);
  const gerr = $derived(draft.data.google_conn_id ? datasrc.googleErr[draft.data.google_conn_id] : '');
  const bsites = $derived(draft.data.bing_conn_id ? datasrc.bingSites[draft.data.bing_conn_id] : undefined);
  const berr = $derived(draft.data.bing_conn_id ? datasrc.bingErr[draft.data.bing_conn_id] : '');
  $effect(() => { if (draft.data.google_conn_id) void loadGoogleRes(draft.data.google_conn_id); });
  $effect(() => { if (draft.data.bing_conn_id) void loadBingSites(draft.data.bing_conn_id); });

  // 按域名自动挑选：选了连接、还没选资源时，挑最匹配的那个。
  const bare = $derived(draft.domain.replace(/^www\./, ''));
  $effect(() => {
    if (!gres || !draft.domain) return;
    if (!draft.data.gsc_site) {
      const hit = gres.sites.find((s) => s.url === `sc-domain:${bare}`) ?? gres.sites.find((s) => s.url === `https://${draft.domain}/`) ?? gres.sites.find((s) => s.url.includes(bare));
      if (hit) draft.data.gsc_site = hit.url;
    }
    if (!draft.data.ga4_property) {
      const hit = gres.properties.find((p) => p.name.toLowerCase().includes(bare.split('.')[0]));
      if (hit) { draft.data.ga4_property = hit.property; draft.data.ga4_property_name = hit.name; }
    }
  });
  $effect(() => {
    if (!bsites || draft.data.bing_site || !draft.domain) return;
    const hit = bsites.find((s) => s.url.includes(bare));
    if (hit) draft.data.bing_site = hit.url;
  });

  let ixBusy = $state(false);
  async function setupIndexNow() {
    ixBusy = true;
    try {
      const r = await invoke<{ key: string; file: string; pr_url: string; note: string }>('setup_indexnow', { id: site.id });
      draft.data.indexnow_key = r.key;
      await refreshSites();
      say(`IndexNow 密钥已生成：${r.file}。${r.note}`);
      if (r.pr_url) void openUrl(r.pr_url);
    } catch (e) { say(String(e), 'err'); }
    finally { ixBusy = false; }
  }
</script>

<div class="card">
  <h3>数据源</h3>
  <p class="hint">数据源账号在「连接与模型设置 → 数据源」里统一管理，这里只选这个站点用哪个资源（按域名自动匹配）。模型只能通过只读工具查询本地缓存，拿不到凭据。</p>

  <div class="ds-block">
    <div class="ds-title"><GoogleMark size={15} /> Google（GA4 + Search Console）</div>
    {#if !googleConns.length}
      <div class="row"><span class="hint">还没有 Google 数据源。</span><button class="link-btn" onclick={() => openDataConnect('google')}>连接一个</button><span class="hint">（服务账号或 Google 账号登录，所有站点共用）</span></div>
    {:else}
      <div class="grid3">
        <div class="field"><span class="lbl">连接</span>
          <Dropdown bind:value={draft.data.google_conn_id} placeholder="不接入" options={[{ value: '', label: '不接入' }, ...googleConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name, sub: `${c.key_kind === 'oauth' ? 'OAuth' : '服务账号'} · ${c.login}` }))]} />
        </div>
        {#if draft.data.google_conn_id}
          <div class="field"><span class="lbl">GA4 媒体资源</span>
            <Dropdown searchable bind:value={draft.data.ga4_property} placeholder={gres ? (gres.properties.length ? '选择媒体资源' : '没有可访问的媒体资源') : '读取中…'}
              onchange={(v) => (draft.data.ga4_property_name = gres?.properties.find((p) => p.property === v)?.name ?? '')}
              options={[{ value: '', label: '不接入 GA4' }, ...(gres?.properties ?? []).map((p) => ({ value: p.property, label: p.name, sub: `${p.account} · ${p.property}` }))]} />
          </div>
          <div class="field"><span class="lbl">Search Console 资源</span>
            <Dropdown searchable bind:value={draft.data.gsc_site} placeholder={gres ? (gres.sites.length ? '选择资源' : '没有可访问的资源') : '读取中…'}
              options={[{ value: '', label: '不接入 Search Console' }, ...(gres?.sites ?? []).map((s) => ({ value: s.url, label: s.url, sub: s.permission }))]} />
          </div>
        {/if}
      </div>
      {#if gerr}<div class="err-note">{gerr}</div>{/if}
      {#if gres?.ga4_error}<p class="hint warn-text">GA4：{gres.ga4_error}</p>{/if}
      {#if gres?.gsc_error}<p class="hint warn-text">Search Console：{gres.gsc_error}</p>{/if}
      {#if gres && draft.data.google_conn_id && googleConns.find((c) => c.id === draft.data.google_conn_id)?.key_kind === 'service_account' && (!gres.properties.length || !gres.sites.length)}
        <p class="hint">列表里没有这个站点？把服务账号 <code>{gres.email}</code> 加进 GA4 媒体资源（查看者）和 Search Console 资源（用户），再 <button class="link-btn" onclick={() => loadGoogleRes(draft.data.google_conn_id, true)}>刷新</button>。</p>
      {/if}
      {#if draft.data.google_conn_id}<div class="row"><button class="btn tiny" onclick={() => loadGoogleRes(draft.data.google_conn_id, true)}>刷新资源列表</button></div>{/if}
    {/if}
  </div>

  <div class="ds-block">
    <div class="ds-title"><BingMark size={15} /> Bing Webmaster</div>
    {#if !bingConns.length}
      <div class="row"><span class="hint">还没有 Bing 数据源。</span><button class="link-btn" onclick={() => openDataConnect('bing')}>连接一个</button><span class="hint">（Bing Webmaster 的 API Key，所有站点共用）</span></div>
    {:else}
      <div class="grid2">
        <div class="field"><span class="lbl">连接</span>
          <Dropdown bind:value={draft.data.bing_conn_id} placeholder="不接入" options={[{ value: '', label: '不接入' }, ...bingConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name }))]} />
        </div>
        {#if draft.data.bing_conn_id}
          <div class="field"><span class="lbl">站点</span>
            <Dropdown searchable bind:value={draft.data.bing_site} placeholder={bsites ? (bsites.length ? '选择站点' : '账号下没有站点') : '读取中…'}
              options={[{ value: '', label: '不接入' }, ...(bsites ?? []).map((s) => ({ value: s.url, label: s.url, sub: s.verified ? '已验证' : '未验证' }))]} />
          </div>
        {/if}
      </div>
      {#if berr}<div class="err-note">{berr}</div>{/if}
    {/if}
  </div>

  <div class="ds-block">
    <div class="ds-title">IndexNow</div>
    <div class="row">
      <input class="tin mono" style="max-width:340px" bind:value={draft.data.indexnow_key} placeholder="验证密钥（站点根目录 &lt;key&gt;.txt 的内容）" spellcheck="false" />
      <button class="btn small" onclick={setupIndexNow} disabled={ixBusy || !draft.domain}>{ixBusy ? '处理中…' : draft.data.indexnow_key ? '重新写入验证文件' : '生成密钥并写入验证文件'}</button>
    </div>
    <small class="faint">每次上线后，Pilot 只把内容有变化的页面提交给 IndexNow（Bing、Yandex 等共享）。{site.source === 'github' ? 'GitHub 来源会开一个 PR 加入验证文件。' : ''}</small>
  </div>
</div>

<style>
  .ds-block { border-top: 1px solid var(--border); padding-top: 12px; margin-top: 12px; display: flex; flex-direction: column; gap: 8px; }
  .ds-title { display: flex; align-items: center; gap: 7px; font-size: 12.5px; font-weight: 600; }
</style>
