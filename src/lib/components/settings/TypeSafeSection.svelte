<script lang="ts">
  // 设置里的「判断模型」：TypeSafe 的 Jev。它不聊天、不写文字，只对内容做打分 / 是否判断。
  // 这里只管连接（Key、模型、测试、用量）；用在哪是托管方案里的规则（上线前质检、跳过无关搜索词），
  // 运营机会排序接上就生效。没配就按原来的规则走。
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import TypeSafeMark from '$lib/components/icons/TypeSafeMark.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { onMount } from 'svelte';
  import { confirmDialog, say } from '$lib/state/app.svelte.js';
  import type { TypeSafeStatus } from '$lib/types';
  import { setJevConfigured } from '$lib/state/typesafe.svelte.js';

  const MODELS = [
    { value: 'jev-latest', label: 'jev-latest', sub: '跟着最新版走（推荐）' },
    { value: 'jev-1.14.0', label: 'jev-1.14.0', sub: '固定版本' },
    { value: 'jev-1.13.0', label: 'jev-1.13.0', sub: '固定版本' },
  ];
  let st = $state<TypeSafeStatus | null>(null);
  let editing = $state(false);
  let key = $state('');
  let model = $state('jev-latest');
  let busy = $state('');
  let note = $state<{ ok: boolean; text: string } | null>(null);

  async function load() {
    try { st = await invoke<TypeSafeStatus>('typesafe_status'); setJevConfigured(st.configured); } catch (e) { note = { ok: false, text: String(e) }; }
  }
  onMount(() => { void load(); });

  function edit() {
    if (!st) return;
    key = ''; model = st.model || 'jev-latest'; note = null; editing = true;
  }
  async function save() {
    if (busy) return;
    busy = 'save'; note = null;
    try {
      st = await invoke<TypeSafeStatus>('typesafe_save', { key: key.trim(), model });
      setJevConfigured(st.configured);
      editing = false; key = '';
      note = { ok: true, text: '已保存，Key 验证通过' };
    } catch (e) { note = { ok: false, text: String(e) }; }
    finally { busy = ''; }
  }
  async function test() {
    if (busy) return;
    busy = 'test'; note = null;
    try { note = { ok: true, text: await invoke<string>('typesafe_test') }; await load(); }
    catch (e) { note = { ok: false, text: String(e) }; }
    finally { busy = ''; }
  }
  async function remove() {
    if (busy) return;
    if (!(await confirmDialog('删除 TypeSafe 的 API Key？之后托管方案里的内容质检、跳过无关搜索词和运营机会排序都停用，按原来的规则判断。', { title: '删除判断模型', kind: 'danger', confirmText: '删除' }))) return;
    busy = 'remove';
    try { st = await invoke<TypeSafeStatus>('typesafe_remove'); setJevConfigured(false); editing = false; note = null; say('已删除 TypeSafe 的 API Key'); }
    catch (e) { note = { ok: false, text: String(e) }; }
    finally { busy = ''; }
  }

  const stateLabel = $derived(!st ? '' : !st.configured ? '未配置' : st.verified_at ? '已验证' : '已配置 · 未测试');
  const cost = $derived(!st || !st.month_tokens ? '' : `本月 ${(st.month_tokens / 10000).toFixed(1)} 万 token ≈ $${st.month_cost_usd < 0.01 ? '<0.01' : st.month_cost_usd.toFixed(2)}`);
</script>

<section class="ts" aria-label="判断模型">
  <div class="section-head">
    <span class="section-title">判断模型<InfoTip text={'TypeSafe 的 Jev：不聊天、不写文字，只给内容打分、判断是非，便宜又快（每百万 token 约 $0.042）。\n接上以后运营机会按站点定位排序；托管方案里可以开「上线前内容质检」（没过就留给你确认）和「跳过无关搜索词」（不交给 AI 处理）。\n页面正文和搜索词会发给 TypeSafe 判断；不配就按原来的规则走。Key 只存系统凭据库。'} label="判断模型说明" /></span>
    {#if st && !st.configured && !editing}<button type="button" class="add" onclick={edit}><svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>添加</button>{/if}
  </div>
  {#if st}
    {#if st.configured || editing}
      <div class="row">
        <span class="icon" aria-hidden="true"><TypeSafeMark size={20} /></span>
        <div class="copy"><b>TypeSafe Jev</b><small title={`api.typesafe.ai · ${st.model}`}>api.typesafe.ai · {st.model}</small></div>
        {#if st.configured}<span class="state" class:verified={!!st.verified_at}><i aria-hidden="true"></i><span>{stateLabel}</span></span>{/if}
        {#if st.configured && !editing}
          <button type="button" class="ic" onclick={test} disabled={!!busy} aria-label="测试连接" data-tip="测试连接（只读模型列表，不计费）">{#if busy === 'test'}<span class="spin"></span>{:else}<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><path d="M13.6 2.6v2.8h-2.8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}</button>
          <button type="button" class="ic" onclick={edit} disabled={!!busy} aria-label="修改" data-tip="修改 Key 和用途"><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M11.5 2.5l2 2L6 12l-2.5.5L4 10l7.5-7.5z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /></svg></button>
        {/if}
      </div>
      <p class="msg slot" class:on={!!note || !!cost} class:error={note && !note.ok} role="status">{note?.text ?? cost}</p>
    {:else}
      <div class="empty-row">
        <span class="icon" aria-hidden="true"><TypeSafeMark size={20} /></span>
        <p class="empty">接上 <b>TypeSafe</b> 后，托管自动上线前会多一道内容质检，运营机会会按站点定位排序。<button type="button" class="link" onclick={() => openUrl('https://docs.typesafe.ai/introduction/quickstart')}>怎么拿 API Key</button></p>
      </div>
    {/if}
    {#if editing}
      <form class="editor" onsubmit={(e) => { e.preventDefault(); void save(); }}>
        <label class="field"><span>API Key{st.configured ? `（留空＝沿用 ${st.key_prefix}…）` : ''}</span>
          <input type="password" bind:value={key} autocomplete="off" spellcheck="false" placeholder={st.configured ? '不换就留空' : '粘贴 TypeSafe 的 API Key'} /></label>
        <div class="field"><span>模型</span>
          <Dropdown bind:value={model} options={MODELS} ariaLabel="判断模型的版本" /></div>
        <p class="where">接上以后：运营机会按站点定位排序；托管方案里可以开「上线前内容质检」和「跳过无关搜索词」（默认都开）。</p>
        <div class="foot">
          {#if st.configured}<button type="button" class="button danger" onclick={remove} disabled={!!busy}>删除</button>{/if}
          <span class="grow"></span>
          <button type="button" class="button" onclick={() => { editing = false; note = null; }} disabled={!!busy}>取消</button>
          <button type="submit" class="button primary" disabled={!!busy || (!st.configured && !key.trim())}>{busy === 'save' ? '验证中…' : '保存'}</button>
        </div>
      </form>
    {/if}
  {/if}
</section>

<style>
  .ts { margin-top:20px; min-width:0; }
  .section-head { display:flex; align-items:center; justify-content:space-between; gap:12px; color:var(--faint); font-size:11px; font-weight:600; margin-bottom:10px; }
  .add,.link { display:inline-flex; align-items:center; gap:5px; background:none; border:0; font:inherit; cursor:pointer; color:var(--dim); }
  .add { padding:3px 5px; border-radius:6px; font-size:11.5px; font-weight:400; }
  .add svg { display:block; width:13px; height:13px; }
  .add:hover,.link:hover { color:var(--text); background:var(--rail); }
  .link { padding:0 2px; font-size:11px; text-decoration:underline; text-underline-offset:2px; }
  .row { display:flex; align-items:center; gap:11px; min-width:0; }
  .icon { width:22px; height:22px; flex:none; display:flex; align-items:center; justify-content:center; color:var(--dim); }
  .copy { min-width:0; flex:1; display:flex; flex-direction:column; gap:2px; }
  .copy b { font-size:13.5px; line-height:1.25; font-weight:600; }
  .copy small { font-size:11px; line-height:1.3; color:var(--dim); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .state { display:inline-flex; align-items:center; gap:5px; color:var(--faint); font-size:10.5px; white-space:nowrap; }
  .state i { display:block; width:6px; height:6px; border-radius:50%; background:currentColor; }
  .state.verified { color:var(--ok,#24965c); }
  .ic { flex:none; width:26px; height:26px; display:inline-flex; align-items:center; justify-content:center; border:0; border-radius:7px; background:none; color:var(--faint); cursor:pointer; transition:color .12s, background-color .12s; }
  .ts:hover .ic, .ic:focus-visible { color:var(--dim); }
  .ic:hover:not(:disabled) { background:var(--rail); color:var(--text); }
  .ic:disabled { opacity:.45; cursor:default; }
  .spin { width:12px; height:12px; border:1.5px solid currentColor; border-right-color:transparent; border-radius:50%; animation:spin .8s linear infinite; }
  @keyframes spin { to { transform:rotate(360deg); } }
  .empty-row { display:flex; align-items:flex-start; gap:11px; }
  .empty-row .icon { margin-top:1px; }
  .empty { margin:0; color:var(--faint); font-size:11px; line-height:1.6; }
  .empty b { color:var(--dim); font-weight:600; }
  .msg { margin:5px 0 0 33px; color:var(--dim); font-size:10.5px; line-height:1.4; overflow-wrap:anywhere; }
  .msg.error { color:var(--err); }
  .slot:not(.on) { visibility:hidden; }
  .slot:not(.on)::before { content:'\200b'; }
  .editor { padding:8px 0 0 33px; display:flex; flex-direction:column; gap:9px; min-width:0; }
  .field { display:flex; flex-direction:column; gap:4px; min-width:0; }
  .field > span { color:var(--dim); font-size:11px; }
  input:not([type="checkbox"]) { box-sizing:border-box; width:100%; height:30px; padding:0 9px; border:1px solid var(--border2,#e1dfd8); border-radius:8px; color:var(--text); background:var(--bg,#fff); font:inherit; font-size:12px; outline:none; }
  input:focus { border-color:var(--accent); }
  .where { margin:0; color:var(--faint); font-size:10.5px; line-height:1.5; }
  .foot { display:flex; align-items:center; gap:6px; }
  .grow { flex:1; }
  .button { height:27px; border:1px solid var(--border2); border-radius:7px; background:transparent; padding:0 10px; color:var(--text); font:inherit; font-size:11.5px; cursor:pointer; }
  .button:hover:not(:disabled) { background:var(--rail); }
  .button:disabled { opacity:.45; cursor:default; }
  .button.primary { color:#fff; background:var(--accent); border-color:var(--accent); }
  .button.primary:hover:not(:disabled) { background:var(--accent-h); }
  .button.danger { color:var(--err); }
</style>
