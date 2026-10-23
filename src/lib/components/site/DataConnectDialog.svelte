<script lang="ts">
  import './site.css';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import GoogleMark from '$lib/components/icons/GoogleMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import { datasrc, closeDataConnect, pickJsonFile, verifyData, confirmData } from '$lib/state/datasrc.svelte.js';
  import { oauthLogin, oauth } from '$lib/state/oauth.svelte.js';

  const f = $derived(datasrc.form);
  const verified = $derived(f.kind === 'google' ? !!f.google : !!f.bing);
</script>

{#if f.open}
  <div class="su-mask" role="presentation" onclick={closeDataConnect}></div>
  <div class="su modal" role="dialog" aria-modal="true" aria-label="连接数据源">
    <header class="sheet-head">
      <b>{#if f.kind === 'google'}<GoogleMark size={15} /> {f.updateId ? '更新' : '连接'} Google（GA4 + Search Console）{:else}<BingMark size={15} /> {f.updateId ? '更新' : '连接'} Bing Webmaster{/if}</b>
      <button class="x" onclick={closeDataConnect} disabled={f.busy}>×</button>
    </header>
    <div class="sheet-body">
      {#if f.kind === 'google'}
        {#if !f.updateId}
          <div class="seg">
            <button class:on={f.mode === 'service_account'} onclick={() => (f.mode = 'service_account')}>服务账号</button>
            <button class:on={f.mode === 'oauth'} onclick={() => (f.mode = 'oauth')}>Google 账号登录（OAuth）</button>
          </div>
        {/if}
        {#if f.mode === 'service_account'}
          <p class="hint">只申请只读权限（analytics.readonly、webmasters.readonly）。密钥只进钥匙串，Pilot 用它拉数据，模型拿不到。</p>
          <ol class="steps-list">
            <li>在 Google Cloud 建一个项目，启用 <b>Google Analytics Data API</b>、<b>Google Analytics Admin API</b> 和 <b>Google Search Console API</b>。
              <button class="link-btn" onclick={() => openUrl('https://console.cloud.google.com/apis/library')}>打开 API 库 ↗</button></li>
            <li>建一个服务账号，在「密钥」里新建 JSON 密钥并下载。
              <button class="link-btn" onclick={() => openUrl('https://console.cloud.google.com/iam-admin/serviceaccounts')}>打开服务账号页 ↗</button></li>
            <li>把服务账号邮箱加进每个站点的 GA4 媒体资源（查看者）和 Search Console 资源（用户）。</li>
          </ol>
          <div class="field"><span class="lbl">服务账号 JSON 密钥</span>
            <textarea class="tin" rows="4" bind:value={f.secret} placeholder={'{"type": "service_account", ...}'} spellcheck="false" oninput={() => { f.google = null; f.err = ''; }}></textarea>
            <div class="row"><button class="btn small" onclick={pickJsonFile}>选择 JSON 文件…</button><span style="flex:1"></span><button class="btn small primary" onclick={verifyData} disabled={f.busy || !f.secret.trim()}>{f.busy && !verified ? '验证中…' : '验证'}</button></div>
          </div>
        {:else}
          <p class="hint">用你自己的 Google 账号在浏览器里授权：能看到账号下的全部 GA4 媒体资源和 Search Console 资源，Pilot 会自动列出让你选。需要先在 Google Cloud 建一个「桌面应用」类型的 OAuth 客户端。</p>
          <ol class="steps-list">
            <li>在 Google Cloud 启用上面三个 API，然后在「API 和服务 → 凭据」里新建 OAuth 客户端 ID，类型选 <b>桌面应用</b>。
              <button class="link-btn" onclick={() => openUrl('https://console.cloud.google.com/apis/credentials')}>打开凭据页 ↗</button></li>
            <li>OAuth 同意屏幕保持「测试」状态时授权 7 天过期；改为「正式发布」后不过期（登录时会提示“未经验证的应用”，选继续即可）。</li>
          </ol>
          <div class="grid2">
            <div class="field"><label for="oa-id">客户端 ID</label><input id="oa-id" class="tin mono" bind:value={f.clientId} spellcheck="false" /></div>
            <div class="field"><label for="oa-secret">客户端密钥</label><input id="oa-secret" class="tin mono" type="password" bind:value={f.clientSecret} autocomplete="off" /></div>
          </div>
          <label class="check"><input type="checkbox" bind:checked={f.manage} /><span>同时授予管理权限：用于「新站闭环」里自动创建 GA4 媒体资源、添加 Search Console 资源并用 DNS 验证所有权、提交 sitemap。不勾选则只读。</span></label>
          <div class="row-end"><button class="btn small primary" onclick={() => oauthLogin()} disabled={oauth.busy || !f.clientId.trim() || !f.clientSecret.trim()}>{oauth.busy ? '等待浏览器授权…' : '在浏览器中登录'}</button></div>
          {#if oauth.busy}<p class="hint">已打开浏览器。完成授权后这里会自动继续；不想继续可以关掉这个窗口。</p>{/if}
        {/if}
        {#if f.google}
          <div class="ok-note">
            已验证 <b>{f.google.email}</b>：可访问 {f.google.properties.length} 个 GA4 媒体资源、{f.google.sites.length} 个 Search Console 资源。
            {#if f.google.ga4_error}<div class="warn-text">GA4：{f.google.ga4_error}</div>{/if}
            {#if f.google.gsc_error}<div class="warn-text">Search Console：{f.google.gsc_error}</div>{/if}
          </div>
          {#if !f.google.properties.length && !f.google.sites.length}
            <p class="hint warn-text">密钥有效，但还没有任何资源。把 {f.google.email} 加进 GA4 媒体资源和 Search Console 资源后再回来绑定站点即可。</p>
          {/if}
        {/if}
      {:else}
        <p class="hint">在 Bing Webmaster Tools 右上角「设置 → API 访问」里生成 API Key。Pilot 用它只读查询词、页面与抓取问题。</p>
        <div class="row"><button class="btn small" onclick={() => openUrl('https://www.bing.com/webmasters/')}>打开 Bing Webmaster ↗</button></div>
        <div class="field"><span class="lbl">API Key</span>
          <div class="row">
            <input class="tin mono" type="password" bind:value={f.secret} autocomplete="off" placeholder="粘贴 API Key" oninput={() => { f.bing = null; f.err = ''; }} onkeydown={(e) => e.key === 'Enter' && verifyData()} />
            <button class="btn small primary" onclick={verifyData} disabled={f.busy || !f.secret.trim()}>{f.busy && !verified ? '验证中…' : '验证'}</button>
          </div>
        </div>
        {#if f.bing}
          <div class="ok-note">已验证：这个账号下有 {f.bing.length} 个站点{f.bing.length ? `（${f.bing.slice(0, 3).map((s) => s.url).join('、')}${f.bing.length > 3 ? ' 等' : ''}）` : ''}。</div>
        {/if}
      {/if}
      {#if verified && !f.updateId}
        <div class="field"><label for="dc-name">连接名称</label><input id="dc-name" class="tin" bind:value={f.name} /></div>
      {/if}
      {#if f.err}<div class="err-note">{f.err}</div>{/if}
      {#if oauth.err && f.mode === 'oauth'}<div class="err-note">{oauth.err}</div>{/if}
    </div>
    {#if f.mode === 'service_account' || f.kind === 'bing'}
      <footer class="sheet-foot">
        <button class="btn small ghost" onclick={closeDataConnect} disabled={f.busy}>取消</button>
        <button class="btn small primary" onclick={confirmData} disabled={f.busy || !verified}>{f.busy && verified ? '保存中…' : f.updateId ? '更新此连接' : '连接'}</button>
      </footer>
    {/if}
  </div>
{/if}

<style>
  .steps-list { margin: 0; padding-left: 18px; display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; line-height: 1.6; }
</style>
