<script lang="ts">
  // 安卓手机：AI 在对话里操作手机 app、截取真实截图（给教程文章配图）。这里装 adb、看连了哪些手机。
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import AndroidMark from '$lib/components/icons/AndroidMark.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import { say } from '$lib/state/app.svelte.js';
  import InfoTip from '$lib/components/common/InfoTip.svelte';

  type Device = { serial: string; state: string; model: string };
  type Status = { adb: string; managed: boolean; devices: Device[]; error: string };
  let st = $state<Status | null>(null);
  let loading = $state(false);
  let installing = $state(false);
  async function load() {
    loading = true;
    try { st = await invoke<Status>('phone_status'); } catch (e) { say(String(e), 'err'); }
    finally { loading = false; }
  }
  onMount(() => { void load(); });
  async function install() {
    installing = true;
    try { await invoke<string>('install_adb'); say('adb 已安装'); await load(); }
    catch (e) { say(String(e), 'err'); }
    finally { installing = false; }
  }
  // 第一次连接的步骤默认收起，没连上时点「怎么连接？」展开
  let howOpen = $state(false);
  const STATE: Record<string, string> = { device: '已连上', unauthorized: '等你在手机上点「允许」', offline: '离线' };
</script>

<div class="sec-head">
  <span class="sec-title">安卓手机<InfoTip text={'连上之后，在对话里说「打开 XX app，每一步截一张图」就行。\n登录、验证码、支付这些 AI 会停下来交给你。'} label="手机怎么用" /></span>
  {#if st?.adb}<button class="icon-btn" onclick={load} aria-label="重新检测" data-tip="重新检测连接的手机"><RefreshIcon spinning={loading} /></button>{/if}
</div>

{#if st && !st.adb}
  <div class="row">
    <span class="ic"><AndroidMark size={20} /></span>
    <span class="main"><b>还没装 adb</b><small>让 AI 操作手机 app、截取真实截图要用它（Google 官方工具，约 10MB）</small></span>
    <button class="bind" onclick={install} disabled={installing}>{installing ? '安装中…' : '安装'}</button>
  </div>
{:else if st}
  {#if st.devices.length}
    <div class="list">
      {#each st.devices as d (d.serial)}
        <div class="row">
          <span class="ic"><AndroidMark size={20} /></span>
          <span class="main"><b>{d.model || d.serial}</b><small>{d.serial}</small></span>
          <span class="used" class:ok={d.state === 'device'} class:warn={d.state !== 'device'}>{STATE[d.state] ?? d.state}</span>
        </div>
      {/each}
    </div>
  {:else}
    <div class="row">
      <span class="ic"><AndroidMark size={20} /></span>
      <span class="main"><b>没有连上手机</b><small>{st.error || '用 USB 线连接手机，再点右上角重新检测'}</small></span>
      <button class="how" onclick={() => (howOpen = !howOpen)} aria-expanded={howOpen}>{howOpen ? '收起' : '怎么连接？'}</button>
    </div>
    {#if howOpen}
      <ol class="steps">
        <li>手机「设置 → 关于手机」，连点「版本号」7 次，打开开发者选项</li>
        <li>在「开发者选项」里打开「USB 调试」</li>
        <li>用 USB 线连上电脑，在手机弹窗里点「允许」，再点右上角重新检测</li>
      </ol>
    {/if}
  {/if}
{/if}

<style>
  .sec-head { display: flex; justify-content: space-between; align-items: center; font-size: 11px; letter-spacing: .03em; text-transform: uppercase; color: var(--faint); font-weight: 600; margin: 18px 0 4px; min-height: 27px; }
  .icon-btn { background: none; border: none; cursor: pointer; padding: 6px; border-radius: 8px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .icon-btn:hover { background: #f1efe9; color: var(--text); }
  .sec-title { display: inline-flex; align-items: center; }
  .how { flex: none; border: none; background: none; padding: 3px 6px; border-radius: 6px; font: inherit; font-size: 11.5px; color: var(--dim); cursor: pointer; white-space: nowrap; }
  .how:hover { background: #f5f4f1; color: var(--text); }
  .steps { margin: 6px 2px 0; padding-left: 18px; color: var(--dim); font-size: 11.5px; line-height: 1.7; }
  .list { display: flex; flex-direction: column; gap: 5px; }
  .row { display: flex; align-items: center; gap: 10px; padding: 8px 8px 8px 10px; border: 1px solid var(--border); border-radius: 11px; }
  .ic { display: inline-flex; flex: none; width: 22px; justify-content: center; }
  .main { flex: 1; min-width: 0; }
  .main b { display: block; font-size: 13.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .main small { display: block; color: var(--dim); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .used { flex: none; font-size: 11px; color: var(--faint); }
  .used.ok { color: #12805c; }
  .used.warn { color: #b45309; }
  .bind { flex: none; border: 1px solid var(--border2); background: #fff; border-radius: 7px; padding: 3px 10px; font: inherit; font-size: 11.5px; color: var(--text); cursor: pointer; white-space: nowrap; }
  .bind:hover { background: #f6f5f1; }
  .bind:disabled { opacity: .5; cursor: default; }
</style>
