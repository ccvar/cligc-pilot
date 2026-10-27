<script lang="ts">
  // 站点设置 → 配置变量：.env.example 里声明的变量（+ 手动加的），值存在 Pilot（钥匙串），
  // 构建时注入，并按部署方式同步到 Cloudflare 的环境变量 / Worker 密钥 / 服务器上的 env 文件。值从不显示。
  import { invoke } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import type { ConfigState, ConfigVar, Site } from '$lib/types';
  import { say, confirmDialog } from '$lib/state/app.svelte.js';
  import { sites, setSiteEnv, pullSiteConfig } from '$lib/state/sites.svelte.js';
  import { ago, full } from './fmt';

  let { site }: { site: Site } = $props();
  let cs = $state<ConfigState | null>(null);
  let editing = $state('');
  let val = $state('');
  let newKey = $state('');
  let adding = $state(false);
  let newVal = $state('');
  let syncing = $state(false);
  let pulling = $state(false);
  // 点「显示」临时取出来的密钥值：离开这张卡、换站点就清掉
  let shown = $state<Record<string, string>>({});
  async function reveal(k: string) {
    if (shown[k] !== undefined) { const n = { ...shown }; delete n[k]; shown = n; return; }
    try { shown = { ...shown, [k]: await invoke<string>('site_config_reveal', { id: site.id, key: k }) }; }
    catch (e) { say(String(e), 'err'); }
  }
  const hideAll = () => { if (Object.keys(shown).length) shown = {}; };

  async function load() {
    try { cs = await invoke<ConfigState>('site_config_state', { id: site.id }); } catch { cs = null; }
  }
  $effect(() => { void site.id; void site.build.env_keys.length; void site.deploy.mode; shown = {}; void load(); });

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
  const names = (ks: string[]) => (ks.length > 6 ? `${ks.slice(0, 6).join('、')} 等 ${ks.length} 个` : ks.join('、'));
  async function sync() {
    if (!cs) return;
    const pend = cs.vars.filter((v) => v.pending).map((v) => v.key);
    const all = cs.vars.filter((v) => v.set || v.auto).length;
    const lines = [
      `把 Pilot 里的 ${all} 个变量写到${cs.target_label}，覆盖那边同名变量现在的值${cs.target === 'vps' ? '（文件里其它行不动）' : ''}。`,
      pend.length ? `其中有改动的：${names(pend)}。` : '',
      cs.will_remove.length ? `还会从那边删掉 ${names(cs.will_remove)}（Pilot 里已经删了）。` : '',
      cs.target === 'vps' ? '服务器上的程序一般要重启才会读到新值。' : '',
    ].filter(Boolean);
    if (!(await confirmDialog(lines.join('\n'), { title: `同步到${dest}`, kind: cs.will_remove.length ? 'danger' : 'warning', confirmText: '同步' }))) return;
    syncing = true;
    try { say(await invoke<string>('site_config_sync', { id: site.id })); await load(); }
    catch (e) { say(String(e), 'err'); }
    finally { syncing = false; }
  }
  let syncingKey = $state('');
  async function syncOne(v: ConfigVar) {
    if (!cs || syncingKey) return;
    if (!(await confirmDialog(`只把 ${v.key} 写到${cs.target_label}，覆盖那边 ${v.key} 现在的值；其它变量不动。${cs.target === 'vps' ? '\n服务器上的程序一般要重启才会读到新值。' : ''}`, { title: `同步 ${v.key}`, kind: 'warning', confirmText: '同步' }))) return;
    syncingKey = v.key;
    try { say(await invoke<string>('site_config_sync_one', { id: site.id, key: v.key })); await load(); }
    catch (e) { say(String(e), 'err'); }
    finally { syncingKey = ''; }
  }
  // Worker 的公开变量随每次上线带上，没法单独写
  const canSyncOne = (v: ConfigVar) => !!cs?.target && v.pending && !(cs.target === 'workers' && !v.secret);
  async function pull() {
    pulling = true;
    try { say(await pullSiteConfig(site.id)); await load(); }
    catch (e) { say(String(e), 'err'); }
    finally { pulling = false; }
  }
  const tag = (v: ConfigVar) => (v.secret ? '密钥' : '公开');
  // 同步到哪儿：说人话（不说「部署目标」）——Pages / Workers 是 Cloudflare，VPS 是你的服务器
  const dest = $derived(cs?.target === 'vps' ? '服务器' : 'Cloudflare');
</script>

<div class="card cfg" onpointerleave={hideAll} role="group" aria-label="配置变量">
  <h3><Icon name="key" />配置变量<span class="grow"></span>
    {#if cs?.target && cs.vars.length}<button class="btn tiny" onclick={pull} disabled={pulling || syncing} data-tip={cs.target === 'vps' ? `读 ${cs.env_file} 里已有的值，只补 Pilot 里没填的；两边不一样时保留 Pilot 的` : `读 Cloudflare 上已有的变量，只补 Pilot 里没填的；密钥 Cloudflare 不给看值，只标成「线上已有」`}>{#if pulling}<span class="spin"></span>读取中…{:else}<Icon name="download" size={11} />从线上读取{/if}</button>{/if}
    {#if cs?.target && cs.vars.length}<button class="btn tiny warn" class:hot={cs.needs_sync} onclick={sync} disabled={syncing || pulling || !!syncingKey} data-tip={`覆盖 ${cs.target_label} 里的同名变量（会先确认）；下次上线时也会自动同步`}>{#if syncing}<span class="spin"></span>同步中…{:else}<Icon name="upload" size={11} />同步到{dest}{/if}</button>{/if}
  </h3>
  <p class="hint">邀请链接、统计 ID、联系邮箱、接口密钥这类配置不写死在代码里：值存在 Pilot 的钥匙串，Pilot 构建时注入，上线时同步到 {cs?.target_label ?? (dest === '服务器' ? '服务器' : 'Cloudflare')}。值不会显示，也不会进仓库。</p>
  {#if cs?.needs_sync}<p class="sync-note"><Icon name="info" size={12} />改动还没同步到{dest}：下次上线时会自动同步；也可以现在点「同步到{dest}」全部写上去，或在下面只同步某一个。</p>{:else if cs?.synced_at}<p class="sync-ok"><span data-tip={full(cs.synced_at)}>上次同步：{ago(cs.synced_at)}</span></p>{/if}
  {#if cs?.target && cs.missing.length}<p class="sync-ok">还有 {cs.missing.length} 个没填。{dest}上要是已经配过，点「从线上读取」把已有的值拿回来。</p>{/if}

  {#if cs && cs.vars.length}
    <ul class="vars">
      {#each cs.vars as v (v.key)}
        <li class:missing={v.declared && !v.set && !v.auto && !v.remote}>
          <div class="vk">
            <span class="mono k">{v.key}</span>
            <span class="tag" class:secret={v.secret}>{tag(v)}</span>
            {#if !v.declared}<span class="tag">手动添加</span>{/if}
            {#if v.pending && !v.differ && cs.target}<span class="tag warn" data-tip={`这个值还没写到${dest}`}>未同步</span>{/if}
            {#if v.differ}<span class="tag warn" data-tip="上次从线上读取时，线上的值和 Pilot 里的不一样；保留了 Pilot 的，同步一次就会写上去">和线上不同</span>{/if}
            <span class="grow"></span>
            {#if v.set}<span class="st ok"><Icon name="check" size={11} />已填</span>{:else if v.auto}<span class="st ok" data-tip="没填值时按站点域名自动给；填了就用你填的"><Icon name="check" size={11} />自动</span>{:else if v.remote}<span class="st ok" data-tip="线上已经设了这个密钥，Cloudflare 不给读它的值；要改就在这里填新值，同步时会覆盖线上的"><Icon name="check" size={11} />线上已有</span>{:else}<span class="st no">未填</span>{/if}
          </div>
          {#if v.set && !v.secret}<code class="val" title={v.value}>{v.value || '（空）'}</code>
          {:else if v.set && v.secret}<div class="val-row">{#if shown[v.key] !== undefined}<code class="val" title={shown[v.key]}>{shown[v.key] || '（空）'}</code>{:else}<code class="val masked">••••••••</code>{/if}<button class="ic-btn eye" class:on={shown[v.key] !== undefined} onclick={() => reveal(v.key)} aria-label={shown[v.key] !== undefined ? `隐藏 ${v.key}` : `显示 ${v.key}`} data-tip={shown[v.key] !== undefined ? '隐藏' : '显示值（鼠标离开这张卡会自动隐藏）'}><Icon name={shown[v.key] !== undefined ? 'eye-off' : 'eye'} size={12} /></button></div>{/if}
          {#if v.desc}<small class="desc">{v.desc}</small>{/if}
          {#if !v.set && v.auto}<small class="auto">自动用 <code>{v.auto}</code>（按站点域名，改域名会跟着变）</small>{/if}
          {#if editing === v.key}
            <div class="row">
              <input class="tin mono small" type={v.secret ? 'password' : 'text'} bind:value={val} placeholder={v.set ? '输入新值（会覆盖原来的）' : '填写值'} autocomplete="off" spellcheck="false" onkeydown={(e) => { if (e.key === 'Enter') void save(v.key, val); if (e.key === 'Escape') editing = ''; }} />
              <button class="btn small primary" onclick={() => save(v.key, val)} disabled={!val.trim()}>保存</button>
              <button class="btn small ghost" onclick={() => (editing = '')}>取消</button>
            </div>
          {:else}
            <div class="acts">
              <button class="link-btn" onclick={() => { editing = v.key; val = ''; }}>{v.set || v.remote ? '修改值' : v.auto ? '改成别的' : '填写'}</button>
              {#if canSyncOne(v)}<button class="link-btn" onclick={() => syncOne(v)} disabled={!!syncingKey || syncing}>{syncingKey === v.key ? '同步中…' : `只同步这个到${dest}`}</button>{/if}
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
        <input class="tin mono small" style="max-width:220px" bind:value={newKey} placeholder="变量名，如 PUBLIC_GA_ID" spellcheck="false" autocapitalize="off" />
        <input class="tin mono small" type="password" bind:value={newVal} placeholder="值" autocomplete="off" />
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
  .tag.warn { background: var(--su-warn-soft, #fbf3dc); color: var(--su-warn-ink, #8a5a08); }
  /* 同步会覆盖线上的值：按钮用警告色，有改动没同步时底色再深一点 */
  .btn.warn { color: var(--su-warn-ink, #8a5a08); border-color: color-mix(in srgb, var(--su-warn-ink, #8a5a08) 35%, transparent); }
  .btn.warn:hover:not(:disabled) { background: var(--su-warn-soft, #fbf3dc); }
  .btn.warn.hot { background: var(--su-warn-soft, #fbf3dc); border-color: color-mix(in srgb, var(--su-warn-ink, #8a5a08) 55%, transparent); }
  .val { align-self: flex-start; max-width: 100%; box-sizing: border-box; padding: 1px 6px; border-radius: 5px; background: var(--rail); color: var(--text); font-size: 11.5px; line-height: 1.6; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .val.masked { color: var(--faint); letter-spacing: 1px; }
  .val-row { display: flex; align-items: center; gap: 4px; min-width: 0; }
  .val-row .val { min-width: 0; }
  .val-row .eye { width: 22px; height: 22px; }
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
