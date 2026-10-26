<script lang="ts">
  // 站点设置 → 配置变量：.env.example 里声明的变量（+ 手动加的），值存在 Pilot（钥匙串），
  // 构建时注入，并按部署方式同步到 Cloudflare 的环境变量 / Worker 密钥 / 服务器上的 env 文件。值从不显示。
  import { invoke } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import type { ConfigState, ConfigVar, Site } from '$lib/types';
  import { say, confirmDialog } from '$lib/state/app.svelte.js';
  import { sites, setSiteEnv } from '$lib/state/sites.svelte.js';
  import { ago, full } from './fmt';

  let { site }: { site: Site } = $props();
  let cs = $state<ConfigState | null>(null);
  let editing = $state('');
  let val = $state('');
  let newKey = $state('');
  let adding = $state(false);
  let newVal = $state('');
  let syncing = $state(false);

  async function load() {
    try { cs = await invoke<ConfigState>('site_config_state', { id: site.id }); } catch { cs = null; }
  }
  $effect(() => { void site.id; void site.build.env_keys.length; void site.deploy.mode; void load(); });

  async function save(k: string, v: string) {
    if (!v.trim()) return;
    await setSiteEnv(site.id, [[k, v.trim()]], []);
    editing = ''; val = '';
    await load();
  }
  async function clear(k: string) {
    // 值从不显示，清了就找不回来
    if (!(await confirmDialog(`清除 ${k} 的值？清掉以后找不回来，需要时得重新填。`, { title: '清除配置', kind: 'danger', confirmText: '清除' }))) return;
    await setSiteEnv(site.id, [], [k]);
    await load();
  }
  let addBusy = $state(false);
  async function addNew() {
    const k = newKey.trim();
    if (!k || !newVal.trim() || addBusy) return;
    addBusy = true;
    try { await save(k, newVal); } finally { addBusy = false; }
    newKey = ''; newVal = '';
    adding = false; // 存好了才收起表单；失败时值还在、可以改了再存
  }
  async function sync() {
    syncing = true;
    try { say(await invoke<string>('site_config_sync', { id: site.id })); await load(); }
    catch (e) { say(String(e), 'err'); }
    finally { syncing = false; }
  }
  const tag = (v: ConfigVar) => (v.secret ? '密钥' : '公开');
</script>

<div class="card cfg">
  <h3><Icon name="key" />配置变量<span class="grow"></span>
    {#if cs?.target && cs.vars.length}<button class="btn tiny" onclick={sync} disabled={syncing} data-tip="写到 {cs.target_label}">{#if syncing}<span class="spin"></span>同步中…{:else}<Icon name="refresh" size={11} />同步到部署目标{/if}</button>{/if}
  </h3>
  <p class="hint">邀请链接、统计 ID、联系邮箱、接口密钥这类配置不写死在代码里：值存在 Pilot 的钥匙串，Pilot 构建时注入，上线时同步到 {cs?.target_label ?? '部署目标'}。值不会显示，也不会进仓库。</p>
  {#if cs?.needs_sync}<p class="sync-note"><Icon name="info" size={12} />改动还没同步到部署目标：下次上线时会自动同步，也可以现在点「同步到部署目标」。</p>{:else if cs?.synced_at}<p class="sync-ok"><span data-tip={full(cs.synced_at)}>上次同步：{ago(cs.synced_at)}</span></p>{/if}

  {#if cs && cs.vars.length}
    <ul class="vars">
      {#each cs.vars as v (v.key)}
        <li class:missing={v.declared && !v.set && !v.auto}>
          <div class="vk">
            <span class="mono k">{v.key}</span>
            <span class="tag" class:secret={v.secret}>{tag(v)}</span>
            {#if !v.declared}<span class="tag">手动添加</span>{/if}
            <span class="grow"></span>
            {#if v.set}<span class="st ok"><Icon name="check" size={11} />已填</span>{:else if v.auto}<span class="st ok" data-tip="没填值时按站点域名自动给；填了就用你填的"><Icon name="check" size={11} />自动</span>{:else}<span class="st no">未填</span>{/if}
          </div>
          {#if v.desc}<small class="desc">{v.desc}</small>{/if}
          {#if !v.set && v.auto}<small class="auto">自动用 <code>{v.auto}</code>（按站点域名，改域名会跟着变）</small>{/if}
          {#if editing === v.key}
            <div class="row">
              <input class="tin mono" type={v.secret ? 'password' : 'text'} bind:value={val} placeholder={v.set ? '输入新值（会覆盖原来的）' : '填写值'} autocomplete="off" spellcheck="false" onkeydown={(e) => { if (e.key === 'Enter') void save(v.key, val); if (e.key === 'Escape') editing = ''; }} />
              <button class="btn small primary" onclick={() => save(v.key, val)} disabled={!val.trim()}>保存</button>
              <button class="btn small ghost" onclick={() => (editing = '')}>取消</button>
            </div>
          {:else}
            <div class="acts">
              <button class="link-btn" onclick={() => { editing = v.key; val = ''; }}>{v.set ? '修改值' : v.auto ? '改成别的' : '填写'}</button>
              {#if v.set}<button class="link-btn danger" onclick={() => clear(v.key)}>{v.auto ? '清除（改回自动）' : '清除'}</button>{/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {:else if cs}
    <p class="hint">这个站点还没有声明配置变量（仓库里没有 <code>.env.example</code>）。需要的话可以在下面手动添加。</p>
  {/if}

  {#if adding}
    <div class="add">
      <span class="lbl">添加其他变量</span>
      <div class="row">
        <input class="tin mono" style="max-width:220px" bind:value={newKey} placeholder="变量名，如 PUBLIC_GA_ID" spellcheck="false" autocapitalize="off" />
        <input class="tin mono" type="password" bind:value={newVal} placeholder="值" autocomplete="off" />
        <button class="btn small primary" onclick={addNew} disabled={!newKey.trim() || !newVal.trim() || sites.saving || addBusy}>{addBusy ? '保存中…' : '保存'}</button>
        <button class="btn small ghost" onclick={() => (adding = false)}>取消</button>
      </div>
    </div>
  {:else}
    <button class="link-btn add-link" onclick={() => (adding = true)}>+ 添加其他变量</button>
  {/if}
</div>

<style>
  .cfg .hint { margin: 0 0 8px; font-size: 12px; line-height: 1.6; }
  .sync-note { display: flex; align-items: center; gap: 6px; margin: 0 0 10px; padding: 7px 10px; border-radius: 8px; background: #fdf8ec; border: 1px solid #f0e4c4; font-size: 12px; color: #8a5a08; }
  .sync-ok { margin: 0 0 10px; font-size: 11.5px; color: var(--faint); }
  .vars { list-style: none; margin: 0 0 12px; padding: 0; border: 1px solid var(--border); border-radius: 10px; overflow: hidden; }
  .vars > li { padding: 9px 12px; border-top: 1px solid var(--border); display: flex; flex-direction: column; gap: 4px; }
  .vars > li:first-child { border-top: none; }
  .vars > li.missing { background: #fdfaf2; }
  .vk { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .k { font-size: 12.5px; font-weight: 600; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tag { flex: none; font-size: 10.5px; padding: 1px 7px; border-radius: 999px; background: #f1efe9; color: var(--dim); }
  .tag.secret { background: #f3ecf9; color: #6b3fa0; }
  .st { flex: none; display: inline-flex; align-items: center; gap: 3px; font-size: 11.5px; }
  .st.ok { color: var(--ok); }
  .st.no { color: #8a5a08; }
  .desc { font-size: 12px; color: var(--faint); }
  .auto { font-size: 12px; color: var(--dim); }
  .auto code { font-size: 11.5px; }
  .acts { display: flex; gap: 12px; }
  .acts .link-btn { font-size: 12px; }
  .acts .link-btn.danger { color: var(--err); }
  .grow { flex: 1; }
  .add { display: flex; flex-direction: column; gap: 6px; }
  .add-link { font-size: 12px; align-self: flex-start; }
  .add .lbl { font-size: 12px; color: var(--dim); }
  .spin { display: inline-block; width: 10px; height: 10px; border: 1.5px solid var(--border2); border-top-color: var(--dim); border-radius: 50%; animation: cfg-spin .7s linear infinite; }
  @keyframes cfg-spin { to { transform: rotate(360deg); } }
</style>
