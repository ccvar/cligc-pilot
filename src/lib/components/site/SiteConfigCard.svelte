<script lang="ts">
  // 站点设置 → 配置变量：.env.example 里声明的变量（+ 手动加的），值存在 Pilot（钥匙串），
  // 构建时注入，并按部署方式同步到 Cloudflare 的环境变量 / Worker 密钥 / 服务器上的 env 文件。
  // 默认收起，标题行给一句概况（几个变量、几个没填、几个没同步）；展开后一行一个变量：名字 ｜ 值 ｜ 操作图标，说明缩成一行。
  import { invoke } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import type { ConfigState, ConfigVar, Site } from '$lib/types';
  import { say, confirmDialog } from '$lib/state/app.svelte.js';
  import { sites, setSiteEnv, pullSiteConfig } from '$lib/state/sites.svelte.js';
  import { ago, full } from './fmt';
  import InfoTip from '$lib/components/common/InfoTip.svelte';

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
  // 默认收起；换站点重新收起
  let open = $state(false);
  $effect(() => { void site.id; open = false; editing = ''; adding = false; });
  function toggle() { open = !open; if (!open) { editing = ''; adding = false; shown = {}; } }

  async function save(k: string, v: string) {
    if (!v.trim()) return;
    await setSiteEnv(site.id, [[k, v.trim()]], []);
    editing = ''; val = '';
    await load();
  }
  // 清掉以后会变成什么：声明过的变成「未填」，有自动值的改回自动，手动加的整行没了
  const clearKind = (v: ConfigVar) => (!v.declared ? 'delete' : v.auto ? 'reset' : 'clear');
  async function clear(v: ConfigVar) {
    const k = v.key;
    const kind = clearKind(v);
    const msg = kind === 'delete' ? `删除变量 ${k}？它是在 Pilot 里手动加的，删掉后值找不回来。已经同步过的话，下次整体同步会把那边的也删掉。`
      : kind === 'reset' ? `清除 ${k} 填的值，改回自动（${v.auto}）？原来填的值找不回来。`
      : `清除 ${k} 的值？变量保留、变成未填；原来的值找不回来，需要时得重新填。`;
    if (!(await confirmDialog(msg, { title: kind === 'delete' ? '删除变量' : kind === 'reset' ? '改回自动' : '清除值', kind: 'danger', confirmText: kind === 'delete' ? '删除' : kind === 'reset' ? '改回自动' : '清除' }))) return;
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
  // 同步到哪儿：说人话（不说「部署目标」）——Pages / Workers 是 Cloudflare，VPS 是你的服务器
  const dest = $derived(cs?.target === 'vps' ? '服务器' : 'Cloudflare');
  const missingN = $derived(cs?.missing.length ?? 0);
  const pendingN = $derived(cs ? cs.vars.filter((v) => v.pending).length + cs.will_remove.length : 0);
  const about = $derived(`邀请链接、统计 ID、联系邮箱、接口密钥这类配置不写死在代码里：值存在 Pilot 的钥匙串，Pilot 构建时注入，上线时同步到${cs?.target_label ?? dest}，也不会进仓库。\n「从线上读取」把${dest}上已有的值拿回来，只补 Pilot 里没填的。\n「同步到${dest}」会覆盖那边的同名变量，点了会先确认；也可以在某个变量上只同步它。`);
</script>

<div class="card cfg" class:open onpointerleave={hideAll} role="group" aria-label="配置变量">
  <h3 class="cfg-head" style:margin-bottom={open ? null : '0'}>
    <button class="cfg-toggle" onclick={toggle} aria-expanded={open}>
      <Icon name="key" />配置变量
      {#if cs}<span class="sum">
        {#if !cs.vars.length}还没有变量
        {:else}{cs.vars.length} 个{#if missingN}<span class="warn-ink"> · {missingN} 个没填</span>{/if}{#if cs.target && pendingN}<span class="warn-ink"> · {pendingN} 个没同步</span>{:else if cs.synced_at}<span data-tip={full(cs.synced_at)}> · {ago(cs.synced_at)}同步过</span>{/if}{/if}
      </span>{/if}
    </button>
    <InfoTip text={about} label="配置变量说明" />
    <span class="grow"></span>
    {#if cs?.target && cs.vars.length}
      <button class="btn tiny" onclick={pull} disabled={pulling || syncing} data-tip={cs.target === 'vps' ? `读 ${cs.env_file} 里已有的值，只补 Pilot 里没填的；两边不一样时保留 Pilot 的` : `读 Cloudflare 上已有的变量，只补 Pilot 里没填的；密钥 Cloudflare 不给看值，只标成「线上已有」`}>{#if pulling}<span class="spin"></span>读取中…{:else}<Icon name="download" size={11} />从线上读取{/if}</button>
      <button class="btn tiny warn" class:hot={cs.needs_sync} onclick={sync} disabled={syncing || pulling || !!syncingKey} data-tip={`覆盖 ${cs.target_label} 里的同名变量（会先确认）；下次上线时也会自动同步`}>{#if syncing}<span class="spin"></span>同步中…{:else}<Icon name="upload" size={11} />同步到{dest}{/if}</button>
    {/if}
    <button class="ic-btn chev-btn" onclick={toggle} aria-label={open ? '收起配置变量' : '展开配置变量'} data-tip={open ? '收起' : '展开'}><svg class="chev" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
  </h3>

  {#if open && cs}
    {#if cs.vars.length}
      <ul class="vars">
        {#each cs.vars as v (v.key)}
          {@const isShown = shown[v.key] !== undefined}
          <li>
            <div class="line">
              <div class="kcol">
                <span class="mono k" title={v.key}>{v.key}</span>
                {#if v.secret}<span class="tag secret">密钥</span>{/if}
                {#if !v.declared}<span class="tag" data-tip="不在 .env.example 里，是在 Pilot 里手动加的">手动</span>{/if}
              </div>
              {#if editing === v.key}
                <div class="vcol edit">
                  <!-- svelte-ignore a11y_autofocus -->
                  <input class="tin mono small" type={v.secret ? 'password' : 'text'} bind:value={val} placeholder={v.set ? '新值（覆盖原来的）' : '填写值'} autocomplete="off" spellcheck="false" autofocus onkeydown={(e) => { if (e.key === 'Enter') void save(v.key, val); if (e.key === 'Escape') editing = ''; }} />
                  <button class="btn tiny primary" onclick={() => save(v.key, val)} disabled={!val.trim()}>保存</button>
                  <button class="btn tiny ghost" onclick={() => (editing = '')}>取消</button>
                </div>
              {:else}
                <div class="vcol">
                  {#if v.set && !v.secret}<code class="val" title={v.value}>{v.value || '（空）'}</code>
                  {:else if v.set}<code class="val" class:masked={!isShown} title={isShown ? shown[v.key] : ''}>{isShown ? shown[v.key] || '（空）' : '••••••••'}</code><button class="ic-btn eye" class:on={isShown} onclick={() => reveal(v.key)} aria-label={isShown ? `隐藏 ${v.key}` : `显示 ${v.key}`} data-tip={isShown ? '隐藏' : '显示值（鼠标离开这张卡会自动隐藏）'}><Icon name={isShown ? 'eye-off' : 'eye'} size={12} /></button>
                  {:else if v.auto}<code class="val" title={v.auto}>{v.auto}</code><span class="flag ok" data-tip="没填值时按站点域名自动给，改域名会跟着变；填了就用你填的">自动</span>
                  {:else if v.remote}<span class="flag ok" data-tip="线上已经设了这个密钥，Cloudflare 不给读它的值；要改就填新值，同步时会覆盖线上的">线上已有</span>
                  {:else if v.declared}<span class="flag no">未填</span><button class="link-btn fill" onclick={() => { editing = v.key; val = ''; }}>填写</button>{/if}
                  {#if v.differ}<span class="flag warn" data-tip="上次从线上读取时，线上的值和 Pilot 里的不一样；保留了 Pilot 的，同步一次就会写上去">和线上不同</span>
                  {:else if v.pending && cs.target}<span class="flag warn" data-tip={`这个值还没写到${dest}`}>未同步</span>{/if}
                </div>
                <div class="acts">
                  {#if canSyncOne(v)}<button class="ic-btn" onclick={() => syncOne(v)} disabled={!!syncingKey || syncing} aria-label={`只同步 ${v.key}`} data-tip={`只把它同步到${dest}（会先确认）`}>{#if syncingKey === v.key}<span class="spin"></span>{:else}<Icon name="upload" size={13} />{/if}</button>{/if}
                  {#if v.set || v.auto || v.remote}<button class="ic-btn" onclick={() => { editing = v.key; val = ''; }} aria-label={`修改 ${v.key}`} data-tip={v.set ? '修改值' : v.auto ? '改成别的值' : '填新值'}><Icon name="pencil" size={13} /></button>{/if}
                  {#if v.set}{@const ck = clearKind(v)}<button class="ic-btn del" onclick={() => clear(v)} aria-label={ck === 'delete' ? `删除 ${v.key}` : ck === 'reset' ? `${v.key} 改回自动` : `清除 ${v.key} 的值`} data-tip={ck === 'delete' ? '删除这个变量' : ck === 'reset' ? '改回自动' : '清除值（变量保留，变成未填）'}><Icon name={ck === 'delete' ? 'trash' : ck === 'reset' ? 'undo' : 'eraser'} size={13} /></button>{/if}
                </div>
              {/if}
              <!-- 说明写的是「该填什么」：放在值下面，和值从同一个位置开始；最多两行，长的悬停看全文 -->
              {#if v.desc}<small class="desc" data-tip={v.desc.length > 70 ? v.desc : undefined}>{v.desc}</small>{/if}
            </div>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="hint">这个站点还没有声明配置变量（仓库里没有 <code>.env.example</code>）。需要的话可以在下面手动添加。</p>
    {/if}

    {#if adding}
      <div class="add">
        <input class="tin mono small" style="max-width:220px" bind:value={newKey} placeholder="变量名，如 PUBLIC_GA_ID" spellcheck="false" autocapitalize="off" />
        <input class="tin mono small" type="password" bind:value={newVal} placeholder="值" autocomplete="off" />
        <button class="btn tiny primary" onclick={addNew} disabled={!newKey.trim() || !newVal.trim() || sites.saving || addBusy}>{addBusy ? '保存中…' : '保存'}</button>
        <button class="btn tiny ghost" onclick={() => (adding = false)}>取消</button>
      </div>
    {:else}
      <button class="link-btn add-link" onclick={() => (adding = true)}>+ 添加其他变量</button>
    {/if}
  {/if}
</div>

<style>
  .cfg-toggle { display: flex; align-items: center; gap: 8px; min-width: 0; border: 0; background: none; padding: 0; font: inherit; color: inherit; cursor: pointer; text-align: left; }
  .sum { font-size: 12px; font-weight: 400; color: var(--faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .warn-ink { color: var(--su-warn-ink, #8a5a08); }
  .cfg-toggle:hover .sum { color: var(--dim); }
  .chev-btn { width: 24px; height: 24px; margin-right: -4px; }
  .chev { transition: transform .16s; }
  .cfg.open .chev { transform: rotate(180deg); }

  .vars { list-style: none; margin: 0 0 10px; padding: 0; border: 1px solid var(--border); border-radius: 10px; }
  .vars > li { padding: 7px 8px 7px 12px; border-top: 1px solid var(--border); }
  .vars > li:first-child { border-top: none; }
  /* 一行：名字 ｜ 值 ｜ 操作。名字列等宽，值从同一个位置开始 */
  .line { display: grid; grid-template-columns: minmax(120px, 230px) minmax(0, 1fr) auto; align-items: center; column-gap: 12px; row-gap: 2px; min-height: 26px; }
  .kcol { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .k { font-size: 12.5px; font-weight: 600; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .vcol { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .vcol.edit { grid-column: 2 / -1; }
  .vcol.edit .tin { flex: 1; min-width: 0; }
  .val { min-width: 0; max-width: 100%; padding: 1px 6px; border-radius: 5px; background: var(--rail); color: var(--text); font-size: 11.5px; line-height: 1.6; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .val.masked { color: var(--faint); letter-spacing: 1px; }
  .tag { flex: none; font-size: 10px; padding: 0 6px; line-height: 16px; border-radius: 999px; background: #f1efe9; color: var(--dim); }
  .tag.secret { background: #f3ecf9; color: #6b3fa0; }
  .flag { flex: none; font-size: 11px; white-space: nowrap; }
  .flag.ok { color: var(--ok); }
  .flag.no { color: var(--su-warn-ink, #8a5a08); }
  .flag.warn { padding: 0 6px; line-height: 16px; border-radius: 999px; font-size: 10px; background: var(--su-warn-soft, #fbf3dc); color: var(--su-warn-ink, #8a5a08); }
  .fill { font-size: 12px; }
  .acts { display: flex; align-items: center; gap: 0; }
  .acts .ic-btn, .eye { width: 24px; height: 24px; }
  .acts .del:hover:not(:disabled) { color: var(--err); }
  .desc { grid-column: 2 / -1; font-size: 11.5px; line-height: 1.5; color: var(--faint); display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; }
  .hint { margin: 0 0 8px; font-size: 12px; line-height: 1.6; }
  .grow { flex: 1; }
  .add { display: flex; align-items: center; gap: 6px; }
  .add-link { font-size: 12px; align-self: flex-start; }
  /* 同步会覆盖线上的值：按钮用警告色，有改动没同步时底色再深一点 */
  .btn.warn { color: var(--su-warn-ink, #8a5a08); border-color: color-mix(in srgb, var(--su-warn-ink, #8a5a08) 35%, transparent); }
  .btn.warn:hover:not(:disabled) { background: var(--su-warn-soft, #fbf3dc); }
  .btn.warn.hot { background: var(--su-warn-soft, #fbf3dc); border-color: color-mix(in srgb, var(--su-warn-ink, #8a5a08) 55%, transparent); }
  .spin { display: inline-block; width: 10px; height: 10px; border: 1.5px solid var(--border2); border-top-color: var(--dim); border-radius: 50%; animation: cfg-spin .7s linear infinite; }
  @keyframes cfg-spin { to { transform: rotate(360deg); } }
</style>
