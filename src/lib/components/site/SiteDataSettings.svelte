<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import Dropdown from '$lib/Dropdown.svelte';
  import GoogleMark from '$lib/components/icons/GoogleMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { app, say } from '$lib/state/app.svelte.js';
  import { refreshSites } from '$lib/state/sites.svelte.js';
  import { launch, verifyBing } from '$lib/state/launch.svelte.js';
  import { datasrc, openDataConnect, loadGoogleRes, loadBingSites } from '$lib/state/datasrc.svelte.js';

  let { site, draft }: { site: Site; draft: Site } = $props();
  const googleConns = $derived(app.conns.filter((c) => c.kind === 'google'));
  const bingConns = $derived(app.conns.filter((c) => c.kind === 'bing'));
  const gres = $derived(draft.data.google_conn_id ? datasrc.googleRes[draft.data.google_conn_id] : undefined);
  const gerr = $derived(draft.data.google_conn_id ? datasrc.googleErr[draft.data.google_conn_id] : '');
  // Search Console 可以用另一个 Google 账号；没单独选时跟 GA4 同一个
  const scConn = $derived(draft.data.gsc_conn_id || draft.data.google_conn_id);
  const sres = $derived(scConn ? datasrc.googleRes[scConn] : undefined);
  const serr = $derived(scConn && scConn !== draft.data.google_conn_id ? datasrc.googleErr[scConn] : '');
  const connOpts = $derived(googleConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name, sub: `${c.key_kind === 'oauth' ? 'Google 账号' : '服务账号'} · ${c.login}` })));
  const bsites = $derived(draft.data.bing_conn_id ? datasrc.bingSites[draft.data.bing_conn_id] : undefined);
  const berr = $derived(draft.data.bing_conn_id ? datasrc.bingErr[draft.data.bing_conn_id] : '');
  $effect(() => { if (draft.data.google_conn_id) void loadGoogleRes(draft.data.google_conn_id); });
  $effect(() => { if (scConn) void loadGoogleRes(scConn); });
  // 单独选的 Search Console 账号和 GA4 选成同一个时，就记成「同一个」
  $effect(() => { if (draft.data.gsc_conn_id && draft.data.gsc_conn_id === draft.data.google_conn_id) draft.data.gsc_conn_id = ''; });
  $effect(() => { if (draft.data.bing_conn_id) void loadBingSites(draft.data.bing_conn_id); });

  // 按域名自动挑选：选了连接、还没选资源时，挑最匹配的那个。
  const bare = $derived(draft.domain.replace(/^www\./, ''));
  $effect(() => {
    if (sres && draft.domain && !draft.data.gsc_site) {
      const hit = sres.sites.find((s) => s.url === `sc-domain:${bare}`) ?? sres.sites.find((s) => s.url === `https://${draft.domain}/`) ?? sres.sites.find((s) => s.url.includes(bare));
      if (hit) draft.data.gsc_site = hit.url;
    }
    if (!gres || !draft.domain) return;
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
  <h3><Icon name="database" />数据源</h3>
  <p class="hint">Google / Bing 账号在「管理连接与模型」里统一管理，这里只选这个站点用哪个资源（按域名自动匹配）。模型只能通过只读工具查询本地缓存，拿不到凭据。</p>

  <div class="ds-block">
    <div class="ds-title"><GoogleMark size={15} /> Google（GA4 + Search Console，可以分属两个账号）</div>
    {#if !googleConns.length}
      <div class="row"><span class="hint">还没有 Google 数据源。</span><button class="link-btn" onclick={() => openDataConnect('google')}>连接一个</button><span class="hint">（服务账号或 Google 账号登录，所有站点共用）</span></div>
    {:else}
      <div class="grid2">
        <div class="field"><span class="lbl">GA4 用的账号</span>
          <Dropdown bind:value={draft.data.google_conn_id} placeholder="不接入" options={[{ value: '', label: '不接入' }, ...connOpts]} />
        </div>
        <div class="field"><span class="lbl">GA4 媒体资源</span>
          <Dropdown searchable bind:value={draft.data.ga4_property} disabled={!draft.data.google_conn_id} placeholder={!draft.data.google_conn_id ? '先选账号' : gres ? (gres.properties.length ? '选择媒体资源' : '这个账号下没有媒体资源') : '读取中…'}
            onchange={(v) => (draft.data.ga4_property_name = gres?.properties.find((p) => p.property === v)?.name ?? '')}
            options={[{ value: '', label: '不接入 GA4' }, ...(gres?.properties ?? []).map((p) => ({ value: p.property, label: p.name, sub: `${p.account} · ${p.property}` }))]} />
        </div>
        <div class="field"><span class="lbl">Search Console 用的账号</span>
          <Dropdown bind:value={draft.data.gsc_conn_id} onchange={() => (draft.data.gsc_site = '')}
            options={[{ value: '', label: draft.data.google_conn_id ? '和 GA4 同一个账号' : '不接入' }, ...connOpts.filter((o) => o.value !== draft.data.google_conn_id)]} />
        </div>
        <div class="field"><span class="lbl">Search Console 资源</span>
          <Dropdown searchable bind:value={draft.data.gsc_site} disabled={!scConn} placeholder={!scConn ? '先选账号' : sres ? (sres.sites.length ? '选择资源' : '这个账号下没有资源') : '读取中…'}
            options={[{ value: '', label: '不接入 Search Console' }, ...(sres?.sites ?? []).map((s) => ({ value: s.url, label: s.url, sub: s.permission }))]} />
        </div>
      </div>
      {#if gerr}<div class="err-note">{gerr}</div>{/if}
      {#if gres?.ga4_error}<p class="hint warn-text">GA4：{gres.ga4_error}</p>{/if}
      {#if serr}<div class="err-note">{serr}</div>{/if}
      {#if sres?.gsc_error}<p class="hint warn-text">Search Console：{sres.gsc_error}</p>{/if}
      {#if gres && draft.data.google_conn_id && googleConns.find((c) => c.id === draft.data.google_conn_id)?.key_kind === 'service_account' && (!gres.properties.length || !gres.sites.length)}
        <p class="hint">列表里没有这个站点？把服务账号 <code>{gres.email}</code> 加进 GA4 媒体资源（查看者）和 Search Console 资源（用户），再 <button class="link-btn" onclick={() => loadGoogleRes(draft.data.google_conn_id, true)}>刷新</button>。</p>
      {/if}
      {#if draft.data.google_conn_id || scConn}<div class="row"><button class="btn tiny" onclick={() => { if (draft.data.google_conn_id) void loadGoogleRes(draft.data.google_conn_id, true); if (scConn && scConn !== draft.data.google_conn_id) void loadGoogleRes(scConn, true); }}><Icon name="refresh" size={11} />刷新资源列表</button></div>{/if}
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
      {#if !site.data.bing_site && site.data.bing_auth}
        <!-- Pilot 已经把站点加到 Bing、还在等验证：验证通过前这里是空的，别让人以为没接 -->
        <div class="bing-pending">
          <span><b>正在验证</b> Pilot 已把 {site.domain} 加到 Bing{launch.status[site.id]?.bing_note ? `：${launch.status[site.id]?.bing_note}` : '，通过后自动绑定。'}</span>
          <button class="btn tiny" onclick={() => verifyBing(site)} disabled={launch.busy[site.id] === 'bing'}>{launch.busy[site.id] === 'bing' ? '验证中…' : '立即验证'}</button>
        </div>
        {#if launch.notes[site.id]?.bing}<div class="hint">{launch.notes[site.id]?.bing}</div>{/if}
      {/if}
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

  <div class="ds-block">
    <div class="ds-title">运营机会</div>
    <div class="row">
      <label for="ds-minimp" class="dim" style="font-size:12.5px">展现量下限</label>
      <input id="ds-minimp" class="tin" type="number" min="10" step="10" style="max-width:140px" bind:value={draft.ops.min_impressions} />
    </div>
    <small class="faint">近 28 天展现低于它的页面和查询词不算运营机会。小站可以调低（默认 100）。</small>
  </div>
</div>

<style>
  .ds-block { border-top: 1px solid var(--border); padding-top: 12px; margin-top: 12px; display: flex; flex-direction: column; gap: 8px; }
  .ds-title { display: flex; align-items: center; gap: 7px; font-size: 12.5px; font-weight: 600; }
  .bing-pending { display: flex; align-items: flex-start; gap: 10px; margin-top: 8px; padding: 8px 10px; border-radius: 8px; background: #fdf8ec; border: 1px solid #f0e4c4; font-size: 12px; line-height: 1.55; color: #6b4a10; }
  .bing-pending span { flex: 1; min-width: 0; }
  .bing-pending b { font-weight: 600; margin-right: 4px; }
  .bing-pending .btn { flex: none; }
</style>
