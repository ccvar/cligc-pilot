<script lang="ts">
  import './site.css';
  import { invoke } from '@tauri-apps/api/core';
  import type { Site } from '$lib/types';
  import { say, confirmDialog } from '$lib/state/app.svelte.js';

  // VPS 部署授权：Pilot 专用部署密钥是否已加到服务器上。依据的是「已保存」的站点设置。
  let { site }: { site: Site } = $props();
  type Status = { has_key: boolean; public_key: string; host: string };
  let st = $state<Status | null>(null);
  let err = $state('');
  let busy = $state(false);
  async function load() {
    err = '';
    if (!site.deploy.ssh_conn_id) { st = null; return; }
    try { st = await invoke<Status>('vps_key_status', { id: site.id }); } catch (e) { err = String(e); st = null; }
  }
  $effect(() => { void site.deploy.ssh_conn_id; void load(); });
  async function setup() {
    if (!(await confirmDialog(`通过这条 SSH 连接登录 ${st?.host ?? '服务器'}，在 ~/.ssh/authorized_keys 里追加一把 Pilot 专用的部署公钥，然后用它登录验证一次。以后发布不用再输密码，随时可以撤销。`, { title: '为部署授权', kind: 'info', confirmText: '授权' }))) return;
    busy = true; err = '';
    try { st = await invoke<Status>('setup_vps_key', { id: site.id }); say('已授权：部署密钥已加到服务器并验证通过'); }
    catch (e) { err = String(e); }
    finally { busy = false; }
  }
  async function revoke() {
    if (!(await confirmDialog('从服务器的 authorized_keys 删掉这把部署公钥，并删除本机保存的私钥？之后发布需要重新授权。', { title: '撤销部署授权', kind: 'danger', confirmText: '撤销' }))) return;
    busy = true; err = '';
    try { await invoke('revoke_vps_key', { id: site.id }); say('已撤销部署授权'); await load(); }
    catch (e) { err = String(e); }
    finally { busy = false; }
  }
</script>

<div class="vps">
  {#if !site.deploy.ssh_conn_id}
    <p class="hint">选好 SSH 连接并保存设置后，可以在这里为部署授权。</p>
  {:else if st}
    <div class="row">
      <span class="pill {st.has_key ? 'ok' : 'warn'}">{st.has_key ? '已授权' : '未授权'}</span>
      <span class="dim" style="font-size:12px">{st.host}</span>
      <span style="flex:1"></span>
      {#if st.has_key}
        <button class="btn tiny" onclick={setup} disabled={busy} title="重新把公钥写到服务器并验证">{busy ? '处理中…' : '重新验证'}</button>
        <button class="btn tiny danger" onclick={revoke} disabled={busy}>撤销授权</button>
      {:else}
        <button class="btn small primary" onclick={setup} disabled={busy}>{busy ? '授权中…' : '为部署授权'}</button>
      {/if}
    </div>
    {#if st.has_key}<small class="mono key" title={st.public_key}>{st.public_key}</small>{/if}
  {/if}
  {#if err}<div class="err-note">{err}</div>{/if}
  <small class="faint">发布时 Pilot 只在发布命令运行期间提供这把密钥（ssh / scp 被强制只用它、只认已知主机、不交互），私钥存钥匙串，模型拿不到。</small>
</div>

<style>
  .vps { display: flex; flex-direction: column; gap: 6px; margin-top: 10px; padding: 10px 12px; border: 1px solid var(--border); border-radius: 10px; background: var(--rail); }
  .key { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 10.5px; color: var(--faint); }
</style>
