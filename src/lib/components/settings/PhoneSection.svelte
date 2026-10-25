<script lang="ts">
  // 安卓手机：AI 在对话里操作手机 app、截取真实截图（给教程文章配图）。这里装 adb、看连了哪些手机。
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import AndroidMark from '$lib/components/icons/AndroidMark.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import { say } from '$lib/state/app.svelte.js';
  import { phone, loadPhone } from '$lib/state/phone.svelte.js';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import GmailMark from '$lib/components/icons/GmailMark.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';

  let installing = $state(false);
  let el: HTMLDivElement | undefined = $state();
  const st = $derived(phone.status);
  const load = loadPhone;
  onMount(() => {
    void load();
    // 从左上角的手机图标进来：滚到这一节
    if (phone.focus) { phone.focus = false; setTimeout(() => el?.scrollIntoView({ block: 'center', behavior: 'smooth' }), 120); }
  });
  async function install() {
    installing = true;
    try { await invoke<string>('install_adb'); say('adb 已安装'); await load(); }
    catch (e) { say(String(e), 'err'); }
    finally { installing = false; }
  }
  // 第一次连接的步骤默认收起，没连上时点「怎么连接？」展开
  let howOpen = $state(false);
  const STATE: Record<string, string> = { device: '已连上', unauthorized: '等你在手机上点「允许」', offline: '离线' };

  // 邮箱验证码：注册时 Pilot 去这个邮箱里取验证码、直接填进手机（密码只进钥匙串，AI 读不到邮件）
  type MailLog = { ts: number; from: string; kind: string; ok: boolean; note: string };
  type MailStatus = { configured: boolean; email: string; host: string; plus_alias: boolean; log: MailLog[] };
  let mail = $state<MailStatus | null>(null);
  let mailOpen = $state(false);
  let mailEmail = $state('');
  let mailPass = $state('');
  let mailBusy = $state(false);
  let mailErr = $state('');
  async function loadMail() { try { mail = await invoke<MailStatus>('mail_status'); } catch {} }
  onMount(() => { void loadMail(); });
  async function connectMail() {
    mailBusy = true; mailErr = '';
    try {
      mail = await invoke<MailStatus>('mail_connect', { email: mailEmail, password: mailPass });
      mailOpen = false; mailPass = ''; say('邮箱已接上');
    } catch (e) { mailErr = String(e); }
    finally { mailBusy = false; }
  }
  async function removeMail() {
    try { await invoke('mail_remove'); await loadMail(); say('已断开邮箱'); } catch (e) { say(String(e), 'err'); }
  }
  const isGmail = $derived(/@(gmail|googlemail)\.com$/i.test(mail?.configured ? mail.email : mailEmail.trim()) || !(mail?.configured || mailEmail.trim()));
  const hhmm = (ts: number) => { const d = new Date(ts * 1000); const today = new Date().toDateString() === d.toDateString(); return (today ? '' : `${d.getMonth() + 1}/${d.getDate()} `) + d.toTimeString().slice(0, 5); };
</script>

<div class="sec-head">
  <span class="sec-title">安卓手机<InfoTip text={'连上之后，在对话里说「打开 XX app，每一步截一张图」就行。\n登录、密码、短信验证码、支付这些 AI 会停下来交给你；接上邮箱后，注册时的邮箱验证码 Pilot 会自己取来填好。'} label="手机怎么用" /></span>
  {#if st?.adb}<button class="icon-btn" onclick={load} aria-label="重新检测" data-tip="重新检测连接的手机"><RefreshIcon spinning={phone.loading} /></button>{/if}
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

{#if st?.adb && mail}
  <div class="row mail-row">
    <span class="ic">{#if isGmail}<GmailMark size={18} />{:else}<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="2" /><path d="m3.5 6.5 8.5 6.5 8.5-6.5" /></svg>{/if}</span>
    <span class="main">
      <b>邮箱验证码<InfoTip text={'AI 在手机上注册时，Pilot 去这个邮箱里找网站刚发来的验证码，直接填进手机。\nAI 只知道「填好了」，读不到你的邮件；谷歌、苹果、微软账号的验证码不代取。\n短信验证码、密码还是交给你。'} label="邮箱验证码怎么用" /></b>
      <small>{mail.configured ? `${mail.email} · 注册时自动取验证码` : '注册时让 Pilot 从你的邮箱取验证码，填进手机'}</small>
    </span>
    {#if mail.configured}
      <button class="how" onclick={removeMail}>断开</button>
    {:else}
      <button class="bind" onclick={() => { mailOpen = !mailOpen; mailErr = ''; }} aria-expanded={mailOpen}>{mailOpen ? '收起' : '接上'}</button>
    {/if}
  </div>
  {#if !mail.configured && mailOpen}
    <div class="mail-form">
      <input class="mi" type="email" placeholder="你的邮箱，如 name@gmail.com" bind:value={mailEmail} autocomplete="off" spellcheck="false" />
      <input class="mi" type="password" placeholder={isGmail ? '应用专用密码（16 位）' : '应用专用密码 / 授权码'} bind:value={mailPass} autocomplete="off"
        onkeydown={(e) => { if (e.key === 'Enter' && mailEmail && mailPass && !mailBusy) void connectMail(); }} />
      <p class="hint">
        {#if isGmail}
          不是 Gmail 的登录密码：在 Google 账号里打开两步验证，再到 <button class="lnk" onclick={() => openUrl('https://myaccount.google.com/apppasswords')}>应用专用密码</button> 生成一个。密码只存在这台电脑的钥匙串里，随时可以在 Google 那边作废。
        {:else}
          QQ / 163 邮箱在网页版「设置 → 账户」里开启 IMAP，生成授权码填在这里。密码只存在这台电脑的钥匙串里。
        {/if}
      </p>
      {#if mailErr}<p class="err">{mailErr}</p>{/if}
      <div class="mail-act"><button class="bind" onclick={connectMail} disabled={mailBusy || !mailEmail.trim() || !mailPass.trim()}>{mailBusy ? '连接中…' : '连接'}</button></div>
    </div>
  {/if}
  {#if mail.configured && mail.log.length}
    <ul class="mail-log">
      {#each mail.log.slice(0, 3) as l (l.ts + l.from)}
        <li class:bad={!l.ok}><span class="t">{hhmm(l.ts)}</span>{l.from} · {l.kind === 'link' ? '激活链接' : '验证码'} · {l.ok ? (l.kind === 'link' ? '已在手机上打开' : '已填到手机') : l.note || '没取到'}</li>
      {/each}
    </ul>
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
  .mail-row { margin-top: 5px; }
  .mail-row .main b { display: inline-flex; align-items: center; }
  .mail-row .ic { color: var(--dim); }
  .mail-form { display: flex; flex-direction: column; gap: 6px; margin: 6px 0 0; padding: 10px; border: 1px solid var(--border); border-radius: 11px; }
  .mi { height: 30px; box-sizing: border-box; padding: 0 9px; border: 1px solid var(--border2); border-radius: 7px; outline: none; background: #fff; color: var(--text); font: inherit; font-size: 12.5px; }
  .mi:focus { border-color: #c7a599; box-shadow: 0 0 0 2px rgb(160 60 43 / 8%); }
  .hint { margin: 0; color: var(--dim); font-size: 11.5px; line-height: 1.6; }
  .lnk { border: none; background: none; padding: 0; font: inherit; color: #1a73e8; cursor: pointer; }
  .lnk:hover { text-decoration: underline; }
  .err { margin: 0; color: #b42318; font-size: 11.5px; line-height: 1.5; }
  .mail-act { display: flex; justify-content: flex-end; }
  .mail-log { list-style: none; margin: 4px 0 0; padding: 0 4px 0 42px; color: var(--dim); font-size: 11px; line-height: 1.7; }
  .mail-log li { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mail-log li.bad { color: #b45309; }
  .mail-log .t { color: var(--faint); margin-right: 6px; font-variant-numeric: tabular-nums; }
</style>
