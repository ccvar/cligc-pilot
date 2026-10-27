<script lang="ts">
  // 「连接」里的两组：安卓手机（AI 在对话里操作手机 app、截真实截图；这里装 adb、看连了哪些手机）
  // 和邮箱（注册时 Pilot 去邮箱取验证码填进手机）。没装 adb / 没接邮箱时默认不显示，从设置右上角的 + 加。
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, untrack } from 'svelte';
  import ConnGroup from '$lib/components/settings/ConnGroup.svelte';
  import { mask, maskId, maskTip } from '$lib/mask';
  import { setFolded } from '$lib/state/connFold.svelte.js';
  import AndroidMark from '$lib/components/icons/AndroidMark.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import { say, confirmDialog } from '$lib/state/app.svelte.js';
  import { phone, loadPhone } from '$lib/state/phone.svelte.js';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import GmailMark from '$lib/components/icons/GmailMark.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { loadOauthClient } from '$lib/state/oauth.svelte.js';

  let installing = $state(false);
  let phoneEl: HTMLDivElement | undefined = $state();
  let mailEl: HTMLDivElement | undefined = $state();
  const st = $derived(phone.status);
  const load = loadPhone;
  onMount(() => { void load(); });
  // 从左上角的手机图标、或 + 菜单进来：展开那一组并滚过去
  $effect(() => {
    const f = phone.focus;
    if (!f) return;
    untrack(() => {
      phone.focus = '';
      setFolded(f, false);
    });
    const t = setTimeout(() => (f === 'phone' ? phoneEl : mailEl)?.scrollIntoView({ block: 'center', behavior: 'smooth' }), 120);
    return () => clearTimeout(t);
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
  type MailStatus = { configured: boolean; email: string; host: string; plus_alias: boolean; auth: string; google_ready: boolean; log: MailLog[] };
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
  // 用 Google 登录：系统浏览器里选账号、点允许，Pilot 这边一直等到浏览器跳回来（或点取消）。
  // 还没有 OAuth 客户端时先填一次客户端 ID / 密钥（和网站数据 GA4 / Search Console 共用）。
  let googleBusy = $state(false);
  let clientOpen = $state(false);
  let clientId = $state('');
  let clientSecret = $state('');
  async function connectGoogle() {
    if (!mail?.google_ready) { clientOpen = !clientOpen; mailOpen = false; mailErr = ''; return; }
    googleBusy = true; mailErr = ''; mailOpen = false; clientOpen = false;
    try { mail = await invoke<MailStatus>('mail_google_connect'); say('Gmail 已接上'); }
    catch (e) { if (!String(e).startsWith('已取消')) mailErr = String(e); }
    finally { googleBusy = false; }
  }
  async function saveClientAndConnect() {
    mailBusy = true; mailErr = '';
    try {
      await invoke('save_google_oauth_client', { clientId, clientSecret });
      clientSecret = '';
      if (mail) mail.google_ready = true;
      void loadOauthClient();
    } catch (e) { mailErr = String(e); return; }
    finally { mailBusy = false; }
    await connectGoogle();
  }
  function cancelGoogle() { invoke('google_oauth_cancel').catch(() => {}); }
  // 屏幕常亮：手机开发者选项「不锁定屏幕」——连着电脑时不熄屏，AI 操作到一半就不会锁住
  let awakeBusy = $state('');
  async function toggleAwake(serial: string, on: boolean) {
    awakeBusy = serial;
    try { await invoke('phone_stay_awake', { serial, on }); await load(); say(on ? '已打开屏幕常亮：连着电脑时手机不会熄屏' : '已关闭屏幕常亮'); }
    catch (e) { say(String(e), 'err'); }
    finally { awakeBusy = ''; }
  }
  // 填验证码（兜底，不经过 AI）：用户在手机上点了「获取验证码」、点进输入框后点这里
  let fillBusy = $state(false);
  async function fillCode() {
    const serial = phone.ready[0]?.serial ?? '';
    fillBusy = true;
    try { say(await invoke<string>('mail_fill_code', { serial })); }
    catch (e) { say(String(e), 'err'); }
    finally { fillBusy = false; await loadMail(); }
  }
  async function removeMail() {
    const ok = await confirmDialog(`断开 ${mail?.email ?? '邮箱'}？钥匙串里的登录凭证会删掉，注册时就不能再自动取邮箱验证码了，要用得重新接。`, { title: '断开邮箱', kind: 'danger', confirmText: '断开' });
    if (!ok) return;
    try { await invoke('mail_remove'); await loadMail(); say('已断开邮箱'); } catch (e) { say(String(e), 'err'); }
  }
  const isGmail = $derived(/@(gmail|googlemail)\.com$/i.test(mail?.configured ? mail.email : mailEmail.trim()) || !(mail?.configured || mailEmail.trim()));
  // 表单里的说明：能用 Google 登录时，「其他邮箱」默认说 QQ / 163 的授权码
  const formGmail = $derived(mailEmail.trim() ? /@(gmail|googlemail)\.com$/i.test(mailEmail.trim()) : !mail?.google_ready);
  const hhmm = (ts: number) => { const d = new Date(ts * 1000); const today = new Date().toDateString() === d.toDateString(); return (today ? '' : `${d.getMonth() + 1}/${d.getDate()} `) + d.toTimeString().slice(0, 5); };
</script>

{#snippet phoneIcon()}<AndroidMark size={14} />{/snippet}
{#snippet phoneActs()}
  <InfoTip text={'连上之后，在对话里说「打开 XX app，每一步截一张图」就行。\n登录、密码、短信验证码、支付这些 AI 会停下来交给你；接上邮箱后，注册时的邮箱验证码 Pilot 会自己取来填好。'} label="手机怎么用" />
  {#if st?.adb}<button class="cg-btn" onclick={load} aria-label="重新检测" data-tip="重新检测连接的手机"><RefreshIcon spinning={phone.loading} /></button>{/if}
{/snippet}

{#if st && (st.adb || phone.showPhone)}
  <div bind:this={phoneEl}>
    <ConnGroup kind="phone" label="安卓手机" n={st.devices.length} icon={phoneIcon} actions={phoneActs}>
      {#if !st.adb}
        <div class="conn-row static">
          <b class="cr-name">还没装 adb</b><small class="cr-sub">AI 操作手机要用它（Google 官方工具，约 10MB）</small>
          <button class="bind" onclick={install} disabled={installing}>{installing ? '安装中…' : '安装'}</button>
        </div>
      {:else if st.devices.length}
        {#each st.devices as d (d.serial)}
          <div class="conn-row static">
            <b class="cr-name" data-tip={d.model ? undefined : maskTip(d.serial, maskId(d.serial))}>{d.model || maskId(d.serial)}</b><small class="cr-sub" data-tip={d.model ? maskTip(d.serial, maskId(d.serial)) : undefined}>{d.model ? maskId(d.serial) : ''}</small>
            {#if d.state === 'device'}
              <button class="how" class:on={d.stay_on} onclick={() => toggleAwake(d.serial, !d.stay_on)} disabled={awakeBusy === d.serial}
                data-tip={d.stay_on ? '连着电脑时屏幕常亮（手机开发者选项「不锁定屏幕」已打开）。点一下关闭' : '连着电脑时让手机屏幕常亮，AI 操作到一半不会熄屏锁住（就是手机开发者选项里的「不锁定屏幕」）'}>{d.stay_on ? '屏幕常亮 ✓' : '屏幕常亮'}</button>
            {/if}
            <span class="used" class:ok={d.state === 'device'} class:warn={d.state !== 'device'}>{STATE[d.state] ?? d.state}</span>
          </div>
        {/each}
      {:else}
        <div class="conn-row static">
          <b class="cr-name">没有连上手机</b><small class="cr-sub" data-tip={st.error || undefined}>{st.error || '用 USB 线连接手机，再点上面的刷新'}</small>
          <button class="how" onclick={() => (howOpen = !howOpen)} aria-expanded={howOpen}>{howOpen ? '收起' : '怎么连接？'}</button>
        </div>
        {#if howOpen}
          <ol class="steps">
            <li>手机「设置 → 关于手机」，连点「版本号」7 次，打开开发者选项</li>
            <li>在「开发者选项」里打开「USB 调试」</li>
            <li>用 USB 线连上电脑，在手机弹窗里点「允许」，再点上面的刷新</li>
          </ol>
        {/if}
      {/if}
    </ConnGroup>
  </div>
{/if}

{#snippet mailIcon()}{#if isGmail}<GmailMark size={14} />{:else}<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="2" /><path d="m3.5 6.5 8.5 6.5 8.5-6.5" /></svg>{/if}{/snippet}
{#snippet mailActs()}
  <InfoTip text={'AI 在手机上注册时，Pilot 去这个邮箱里找网站刚发来的验证码，直接填进手机。\nAI 只知道「填好了」，读不到你的邮件；谷歌、苹果、微软账号的验证码不代取。\n短信验证码、密码还是交给你。'} label="邮箱验证码怎么用" />
{/snippet}

{#if mail && (mail.configured || st?.adb || phone.showMail)}
  <div bind:this={mailEl}>
    <ConnGroup kind="mail" label="邮箱" n={mail.configured ? 1 : 0} icon={mailIcon} actions={mailActs}>
      {#if mail.configured}
        <div class="conn-row static">
          <b class="cr-name" data-tip={maskTip(mail.email, mask(mail.email))}>{mask(mail.email)}</b><small class="cr-sub">注册时自动取验证码</small>
          {#if phone.ready.length}
            <button class="bind" onclick={fillCode} disabled={fillBusy}
              data-tip={'自己在手机上注册时用：点完「获取验证码」、点进验证码输入框，再点这里。\nPilot 去邮箱取最新的验证码填进去（最多等 60 秒），不经过 AI。'}>{fillBusy ? '取验证码中…' : '填验证码'}</button>
          {/if}
          <button class="how" onclick={removeMail}>断开</button>
        </div>
        {#if mail.log.length}
          <ul class="mail-log">
            {#each mail.log.slice(0, 3) as l, i (`${l.ts}:${l.from}:${i}`)}
              <li class:bad={!l.ok}><span class="t">{hhmm(l.ts)}</span>{l.from} · {l.kind === 'link' ? '激活链接' : '验证码'} · {l.ok ? (l.kind === 'link' ? '已在手机上打开' : '已填到手机') : l.note || '没取到'}</li>
            {/each}
          </ul>
        {/if}
      {:else}
        <div class="conn-row static">
          <small class="cr-sub">{googleBusy ? '在浏览器里选你的 Gmail 账号，点「继续」和「允许」…' : '注册时自动取验证码填进手机'}</small>
          {#if googleBusy}
            <button class="how" onclick={cancelGoogle}>取消</button>
          {:else}
            <button class="how" onclick={() => { mailOpen = !mailOpen; clientOpen = false; mailErr = ''; }} aria-expanded={mailOpen}>{mailOpen ? '收起' : '其他邮箱'}</button>
            <button class="bind g-btn" onclick={connectGoogle} aria-expanded={clientOpen}><GmailMark size={12} />用 Google 登录</button>
          {/if}
        </div>
        {#if !mailOpen && !clientOpen}<p class="err row-err slot" class:on={!!mailErr} role="alert">{mailErr}</p>{/if}
        {#if clientOpen}
          <div class="mail-form">
            <p class="hint">第一次用要填一次 Google 登录凭证：在 Google Cloud 建一个「桌面应用」类型的 OAuth 客户端，并启用 Gmail API（<button class="lnk" onclick={() => openUrl('https://console.cloud.google.com/apis/credentials')}>打开 Google Cloud 凭据页</button>）。和网站数据（GA4 / Search Console）共用这一份，只存在钥匙串里。</p>
            <input class="mi" placeholder="客户端 ID（….apps.googleusercontent.com）" bind:value={clientId} autocomplete="off" spellcheck="false" />
            <input class="mi" type="password" placeholder="客户端密钥（GOCSPX-…）" bind:value={clientSecret} autocomplete="off"
              onkeydown={(e) => { if (e.key === 'Enter' && clientId && clientSecret && !mailBusy) void saveClientAndConnect(); }} />
            <p class="err slot" class:on={!!mailErr} role="alert">{mailErr}</p>
            <div class="mail-act"><button class="bind" onclick={saveClientAndConnect} disabled={mailBusy || !clientId.trim() || !clientSecret.trim()}>{mailBusy ? '保存中…' : '保存并登录'}</button></div>
          </div>
        {/if}
        {#if mailOpen}
          <div class="mail-form">
            <input class="mi" type="email" placeholder="你的邮箱，如 name@qq.com" bind:value={mailEmail} autocomplete="off" spellcheck="false" />
            <input class="mi" type="password" placeholder={formGmail ? '应用专用密码（16 位）' : '应用专用密码 / 授权码'} bind:value={mailPass} autocomplete="off"
              onkeydown={(e) => { if (e.key === 'Enter' && mailEmail && mailPass && !mailBusy) void connectMail(); }} />
            <p class="hint">
              {#if formGmail}
                不是 Gmail 的登录密码：在 Google 账号里打开两步验证，再到 <button class="lnk" onclick={() => openUrl('https://myaccount.google.com/apppasswords')}>应用专用密码</button> 生成一个。密码只存在这台电脑的钥匙串里，随时可以在 Google 那边作废。
              {:else}
                QQ / 163 邮箱在网页版「设置 → 账户」里开启 IMAP，生成授权码填在这里。密码只存在这台电脑的钥匙串里。
              {/if}
            </p>
            <p class="err slot" class:on={!!mailErr} role="alert">{mailErr}</p>
            <div class="mail-act"><button class="bind" onclick={connectMail} disabled={mailBusy || !mailEmail.trim() || !mailPass.trim()}>{mailBusy ? '连接中…' : '连接'}</button></div>
          </div>
        {/if}
      {/if}
    </ConnGroup>
  </div>
{/if}

<style>
  .how { flex: none; border: none; background: none; padding: 3px 6px; border-radius: 6px; font: inherit; font-size: 11.5px; color: var(--dim); cursor: pointer; white-space: nowrap; }
  .how:hover { background: #f1efe9; color: var(--text); }
  .how.on { color: #12805c; }
  .how:disabled { opacity: .5; cursor: default; }
  .steps { margin: 0; padding: 2px 12px 8px 49px; color: var(--dim); font-size: 11.5px; line-height: 1.7; }
  .used { flex: none; font-size: 11px; color: var(--faint); padding-right: 4px; }
  .used.ok { color: #12805c; }
  .used.warn { color: #b45309; }
  .bind { flex: none; border: 1px solid var(--border2); background: #fff; border-radius: 7px; padding: 2px 9px; font: inherit; font-size: 11.5px; color: var(--text); cursor: pointer; white-space: nowrap; }
  .bind:hover { background: #f6f5f1; }
  .bind:disabled { opacity: .5; cursor: default; }
  .g-btn { display: inline-flex; align-items: center; gap: 5px; }
  .mail-form { display: flex; flex-direction: column; gap: 6px; padding: 8px 10px 10px 31px; border-top: 1px solid #f1f0ec; }
  .mi { height: 30px; box-sizing: border-box; padding: 0 9px; border: 1px solid var(--border2); border-radius: 7px; outline: none; background: #fff; color: var(--text); font: inherit; font-size: 12.5px; }
  .mi:focus { border-color: #c7a599; }
  .hint { margin: 0; color: var(--dim); font-size: 11.5px; line-height: 1.6; }
  .lnk { border: none; background: none; padding: 0; font: inherit; color: #1a73e8; cursor: pointer; }
  .lnk:hover { text-decoration: underline; }
  .err { margin: 0; color: #b42318; font-size: 11.5px; line-height: 1.5; }
  /* 报错位始终占一行：出现时只是淡入，表单不跳 */
  .slot:not(.on) { visibility: hidden; }
  .slot:not(.on)::before { content: '\200b'; }
  .slot.on { animation: noteIn .18s ease-out; }
  @keyframes noteIn { from { opacity: 0; } }
  .row-err { padding: 0 10px 8px 31px; }
  .mail-act { display: flex; justify-content: flex-end; }
  .mail-log { list-style: none; margin: 0; padding: 0 10px 7px 31px; color: var(--dim); font-size: 11px; line-height: 1.7; }
  .mail-log li { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mail-log li.bad { color: #b45309; }
  .mail-log .t { color: var(--faint); margin-right: 6px; font-variant-numeric: tabular-nums; }
</style>
