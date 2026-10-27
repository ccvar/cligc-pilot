<script lang="ts">
  import { escClose } from '$lib/escClose';
  import { sheetWidth } from '$lib/sheetWidth';
  import SettingsIcon from '$lib/SettingsIcon.svelte';
  import type { Brain } from '$lib/types';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import ApiServices from '$lib/ApiServices.svelte';
  import TypeSafeSection from './TypeSafeSection.svelte';
  import BackgroundSection from './BackgroundSection.svelte';
  import type { ApiProfile } from '$lib/apiProfiles';
  import AppIcon from '$lib/AppIcon.svelte';
  import PlusIcon from '$lib/components/icons/PlusIcon.svelte';
  import CfMark from '$lib/components/icons/CfMark.svelte';
  import SshMark from '$lib/components/icons/SshMark.svelte';
  import DistroMark from '$lib/components/icons/DistroMark.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import UpdateIcon from '$lib/components/icons/UpdateIcon.svelte';
  import ConnectionMark from '$lib/components/icons/ConnectionMark.svelte';
  import { settings, refreshBrainsManual, authorize, rebind, upgradeCli, saveClaudeToken, closeClaudeToken, installClaude, installCodex, installGrok, copyCmd, removeCustom, addCustom } from '$lib/state/settings.svelte.js';
  import { SHEET_WIDTH, CLAUDE_INSTALL_CMD, GROK_INSTALL_CMD, keystoreName, isMainWindow } from '$lib/state/env';
  import { openCfConnect } from '$lib/state/cloudflare.svelte.js';
  import { openGhConnect } from '$lib/state/github.svelte.js';
  import { isServiceConn, openDataConnect } from '$lib/state/datasrc.svelte.js';
  import BloggerSection from '$lib/components/settings/BloggerSection.svelte';
  import WordpressSection from '$lib/components/settings/WordpressSection.svelte';
  import WordpressMark from '$lib/components/icons/WordpressMark.svelte';
  import { startWpConnect } from '$lib/state/wordpress.svelte.js';
  import BloggerMark from '$lib/components/icons/BloggerMark.svelte';
  import { bloggerLogin } from '$lib/state/blogger.svelte.js';
  import DataSourcesSection from '$lib/components/settings/DataSourcesSection.svelte';
  import PhoneSection from '$lib/components/settings/PhoneSection.svelte';
  import ConnGroup from '$lib/components/settings/ConnGroup.svelte';
  import { mask, maskTip } from '$lib/mask';
  import GoogleMark from '$lib/components/icons/GoogleMark.svelte';
  import BingMark from '$lib/components/icons/BingMark.svelte';
  import AndroidMark from '$lib/components/icons/AndroidMark.svelte';
  import GmailMark from '$lib/components/icons/GmailMark.svelte';
  import { showPhoneGroup } from '$lib/state/phone.svelte.js';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import { openSshConnect } from '$lib/state/ssh.svelte.js';
  import { app } from '$lib/state/app.svelte.js';
  import { selectConn, openConnCtx, connectionDisplayName, distroOf, sshSub, connections, toggleConnCtx } from '$lib/state/connections.svelte.js';
  import { elapsedLabel } from '$lib/state/messages.svelte.js';
  import { customsOf } from '$lib/state/models';
  import { updater, openUpdateCenter } from '$lib/state/updater.svelte.js';
  import type { Connection } from '$lib/types';

  // 连接按类型分组、每条一行（组的样子见 ConnGroup）。Google / Bing（网站数据）、安卓手机、邮箱也是组，
  // 写在各自的组件里，接在这几组后面；右上角的 + 能加所有类型。
  const CONN_GROUPS: { kind: string; label: string; add?: () => void }[] = [
    { kind: 'cloudflare', label: 'Cloudflare', add: openCfConnect },
    { kind: 'github', label: 'GitHub', add: openGhConnect },
    { kind: 'ssh', label: '远程机器', add: openSshConnect },
  ];
  const connGroups = $derived.by(() => {
    const list = app.conns.filter((c) => !isServiceConn(c));
    const known = new Set(CONN_GROUPS.map((g) => g.kind));
    const groups = CONN_GROUPS.map((g) => ({ ...g, items: list.filter((c) => c.kind === g.kind) }));
    const other = list.filter((c) => !known.has(c.kind));
    if (other.length) groups.push({ kind: 'other', label: '其他', add: undefined, items: other });
    return groups.filter((g) => g.items.length);
  });
  function addFromMenu(f: () => void) { settings.setupAddOpen = false; f(); }
  function connSub(c: Connection): string {
    if (c.kind === 'cloudflare') return c.remark?.trim() || c.key_prefix || '';
    if (c.kind === 'github') return c.remark?.trim() || c.login || '';
    return '';
  }
</script>

<!-- 设置弹窗 -->
{#if settings.setupOpen}
  <div class="mask" role="presentation" onclick={() => { settings.setupAddOpen = false; settings.setupOpen = false; }}></div>
  <div class="sheet" use:sheetWidth={SHEET_WIDTH.setup} use:escClose={() => { settings.setupAddOpen = false; settings.setupOpen = false; }} role="dialog" aria-modal="true" aria-label="设置">
    <header class="sheet-head"><b>设置</b><button class="x" aria-label="关闭" data-tip="关闭" onclick={() => { settings.setupAddOpen = false; settings.setupOpen = false; }}>×</button></header>
    <div class="sheet-body">
      <div class="sec-head">
        <span>连接</span>
        <div class="setup-add" bind:this={settings.setupAddEl}>
          <button
            class="icon-btn setup-add-trigger"
            class:open={settings.setupAddOpen}
            aria-label="添加连接"
            aria-haspopup="menu"
            aria-expanded={settings.setupAddOpen}
            data-tip={settings.setupAddOpen ? undefined : '添加连接'}
            onclick={() => (settings.setupAddOpen = !settings.setupAddOpen)}
          ><PlusIcon /></button>
          {#if settings.setupAddOpen}
            <div class="setup-add-menu" role="menu">
              <button role="menuitem" onclick={() => { settings.setupAddOpen = false; openCfConnect(); }}>
                <CfMark size={15} /><span>连接 Cloudflare</span>
              </button>
              <button role="menuitem" onclick={() => { settings.setupAddOpen = false; openGhConnect(); }}>
                <GhMark size={15} /><span>连接 GitHub</span>
              </button>
              <button role="menuitem" onclick={() => { settings.setupAddOpen = false; openSshConnect(); }}>
                <SshMark size={15} /><span>新建远程连接</span>
              </button>
              <div class="menu-sep"></div>
              <button role="menuitem" onclick={() => addFromMenu(() => openDataConnect('google'))}>
                <GoogleMark size={15} /><span>Google 账号<small>GA4 + Search Console，浏览器授权</small></span>
              </button>
              <button role="menuitem" onclick={() => addFromMenu(() => openDataConnect('google', 'service_account'))}>
                <GoogleMark size={15} /><span>Google 服务账号<small>JSON 密钥</small></span>
              </button>
              <button role="menuitem" onclick={() => addFromMenu(() => openDataConnect('bing'))}>
                <BingMark size={15} /><span>Bing Webmaster<small>API Key</small></span>
              </button>
              <button role="menuitem" onclick={() => addFromMenu(() => void bloggerLogin(''))}>
                <BloggerMark size={15} /><span>Blogger 账号<small>「分发」把新文章摘要发到你的博客</small></span>
              </button>
              <button role="menuitem" onclick={() => addFromMenu(() => void startWpConnect())}>
                <WordpressMark size={15} /><span>WordPress.com 账号<small>「分发」把新文章摘要发到你的站点</small></span>
              </button>
              <div class="menu-sep"></div>
              <button role="menuitem" onclick={() => addFromMenu(() => showPhoneGroup('phone'))}>
                <AndroidMark size={15} /><span>安卓手机<small>让 AI 操作手机 app、截图</small></span>
              </button>
              <button role="menuitem" onclick={() => addFromMenu(() => showPhoneGroup('mail'))}>
                <GmailMark size={14} /><span>邮箱验证码<small>注册时自动取验证码填进手机</small></span>
              </button>
            </div>
          {/if}
        </div>
      </div>
      {#if app.conns.length === 0}<p class="hint">还没有连接。点右上角 + 可以连接 Cloudflare 建站部署、GitHub 改仓库代码、SSH 远程机器、Google / Bing 看网站数据，或接上安卓手机。本地对话不需要连接。</p>{/if}
      <div class="conn-groups">
        {#each connGroups as g (g.kind)}
          <ConnGroup kind={g.kind} label={g.label} n={g.items.length} add={g.add}>
            {#snippet icon()}<ConnectionMark kind={g.kind === 'other' ? g.items[0].kind : g.kind} size={14} />{/snippet}
            {#each g.items as c (c.id)}
              {@const nm = connectionDisplayName(c)}
              <!-- 行里有齿轮按钮，按钮不能套按钮：名字那块自己是「选中这个连接」的按钮（原生支持 Enter / 空格） -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="conn-row {app.activeConnId === c.id ? 'on' : ''}" oncontextmenu={(e) => openConnCtx(e, c)}>
                <button type="button" class="cr-main" onclick={() => selectConn(c.id)}>
                  {#if c.kind === 'ssh'}<span class="cr-os" data-tip={sshSub(c)}><DistroMark id={distroOf(c)} size={14} /></span>{/if}
                  <b class="cr-name" data-tip={maskTip(nm, mask(nm))}>{mask(nm)}</b>
                  {#if c.kind === 'ssh'}
                    <small class="cr-sub" data-tip={maskTip(sshSub(c), mask(sshSub(c)))}>{mask(sshSub(c))}</small>
                  {:else if connSub(c)}
                    <small class="cr-sub" class:conn-remark={!!c.remark?.trim()} data-tip={c.remark?.trim() ? `脱敏 Key：${c.key_prefix}` : maskTip(connSub(c), mask(connSub(c)))}>{mask(connSub(c))}</small>
                  {/if}
                </button>
                <button
                  class="icon-btn sm cr-gear"
                  class:active={connections.connCtx?.conn.id === c.id}
                  title="连接设置"
                  aria-label={`打开 ${connectionDisplayName(c)} 的连接设置`}
                  aria-haspopup="menu"
                  aria-expanded={connections.connCtx?.conn.id === c.id}
                  onclick={(e) => { e.stopPropagation(); toggleConnCtx(e, c); }}
                ><SettingsIcon /></button>
              </div>
            {/each}
          </ConnGroup>
        {/each}
        <DataSourcesSection />
        <BloggerSection />
        <WordpressSection />
        <PhoneSection />
      </div>

      <div class="sec-head mt"><span class="sec-title">模型服务 · 本地客户端<InfoTip text={`自定义模型 ID 会作为该厂商模型下拉里的附加档位（可加多个）。\n仅限本人订阅账户驱动本地官方 CLI，密钥存 ${keystoreName}。`} /></span><button class="icon-btn quiet" onclick={refreshBrainsManual} title="刷新本地客户端" aria-label="刷新本地客户端"><RefreshIcon spinning={settings.brainsBusy} /></button></div>
      {#if app.brains}
        <div class="brains-list">
        {#each [{ b: 'claude' as Brain, st: app.brains.claude, name: 'Claude Code', cmd: CLAUDE_INSTALL_CMD }, { b: 'codex' as Brain, st: app.brains.codex, name: 'Codex', cmd: 'npm i -g @openai/codex' }, { b: 'grok' as Brain, st: app.brains.grok, name: 'Grok', cmd: GROK_INSTALL_CMD }] as r (r.b)}
          <div class="brain-block">
          <div class="brain-row">
            <span class="brain-ic"><BrainIcon brain={r.b} size={18} /></span>
            <span class="brain-main"><b>{r.name}</b>
              <small>{#if !r.st.found}未安装{:else if r.st.logged_in === false}未登录{:else}{r.st.version || '已就绪'}{#if r.b === 'claude' && settings.cliUpd?.has_update} · <span class="cli-new">新版 {settings.cliUpd.latest}</span>{/if}{/if}</small></span>
            <span class="brain-dot"><span class="dot {r.st.found && r.st.logged_in && !(r.b === 'claude' && settings.claudeVerify === 'bad') ? 'ok' : r.st.found ? 'warn' : 'off'}"></span></span>
            {#if r.st.found && r.st.logged_in === false}
              {#if r.b === 'claude'}
                <span class="brain-auth-actions">
                  <button
                    class="claude-token-trigger"
                    class:active={settings.claudeTokenOpen}
                    type="button"
                    onclick={() => (settings.claudeTokenOpen = !settings.claudeTokenOpen)}
                    aria-label="粘贴 Claude 订阅 Token"
                    aria-expanded={settings.claudeTokenOpen}
                    data-tip="粘贴订阅 Token&#10;使用 claude setup-token 生成"
                  ><svg width="15" height="15" viewBox="0 0 18 18" fill="none" aria-hidden="true"><circle cx="5.5" cy="12.5" r="3.25" stroke="currentColor" stroke-width="1.45"/><path d="m7.85 10.15 6.55-6.55M11.45 6.55l1.7 1.7M13.15 4.85l1.35 1.35" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
                  <button class="authbtn" onclick={() => authorize(r.b)} disabled={settings.authWaiting === r.b}>{#if settings.authWaiting === r.b}<span class="wr-spin"></span>等待授权…{:else}去授权 ↗{/if}</button>
                </span>
              {:else}
                <button class="authbtn" onclick={() => authorize(r.b)} disabled={settings.authWaiting === r.b}>{#if settings.authWaiting === r.b}<span class="wr-spin"></span>等待授权…{:else}去授权 ↗{/if}</button>
              {/if}
            {:else if r.b === 'claude' && r.st.logged_in && settings.claudeVerify === 'bad'}<button class="authbtn" onclick={() => rebind('claude')} disabled={settings.authWaiting === 'claude'}>{#if settings.authWaiting === 'claude'}<span class="wr-spin"></span>等待授权…{:else}重新绑定 ↗{/if}</button>
            {:else if r.b === 'claude' && settings.cliUpd?.has_update}
              <!-- 图标 + tips：这一行右侧本来是「去授权 / 重新绑定」那类**必须做**的动作，
                   升级是可选的，用文字按钮会跟它们抢同一级视觉重量。tips 里写全「升到哪个版本、
                   会动哪个路径」——多安装时只有 PATH 第一个生效，这信息不能只藏在确认弹窗里。
                   升级中不另写 tip：disabled 的按钮在 Chrome 下根本不派发 mouseover，气泡永远弹不出来，
                   状态只能靠 spinner 表达。 -->
              <button class="cli-upg" onclick={() => void upgradeCli()} disabled={settings.cliUpgrading}
                aria-label={`升级 Claude Code 到 ${settings.cliUpd.latest}`}
                data-tip={`升级到 ${settings.cliUpd.latest}\n${settings.cliUpd.target}${settings.cliUpd.all.length > 1 ? `\nPATH 上有 ${settings.cliUpd.all.length} 份，只有这个生效` : ''}`}>
                {#if settings.cliUpgrading}<span class="wr-spin"></span>{:else}<UpdateIcon size={14} />{/if}
              </button>{/if}
          </div>
          {#if r.b === 'claude' && settings.claudeTokenOpen}
            <div class="claude-token-form">
              <div class="claude-token-copy">
                <b>Claude Code 订阅 Token</b>
                <small>在终端运行 <code>claude setup-token</code>，完成授权后复制生成的长期 Token。仅支持 Pro、Max、Team 或 Enterprise 订阅。</small>
              </div>
              <div class="claude-token-entry">
                <input
                  type="password"
                  bind:value={settings.claudeTokenDraft}
                  placeholder="粘贴 Token"
                  autocomplete="off"
                  spellcheck="false"
                  autocapitalize="off"
                  autocorrect="off"
                  onkeydown={(event) => event.key === 'Enter' && void saveClaudeToken()}
                />
                <button class="btn sm" type="button" onclick={() => void saveClaudeToken()} disabled={!settings.claudeTokenDraft.trim() || settings.claudeTokenSaving}>{settings.claudeTokenSaving ? '验证中…' : '保存并验证'}</button>
                <button class="btn sm ghost" type="button" onclick={closeClaudeToken} disabled={settings.claudeTokenSaving}>取消</button>
              </div>
              <small class="claude-token-safe">Token 只保存到 {keystoreName}，不会写入 Pilot 配置、导出文件或对话内容。</small>
            </div>
          {/if}
          {#if !r.st.found}
            <!-- 与主界面同款一键安装（同 invoke/进度态/错误处理），放在「未安装」状态文字下方、小号靠左；
                 手动命令降级为同行的复制小图标 -->
            <div class="brain-install">
              <button class="wr-btn" onclick={r.b === 'claude' ? installClaude : r.b === 'codex' ? installCodex : installGrok}
                disabled={r.b === 'claude' ? settings.claudeInstalling : r.b === 'codex' ? settings.codexInstalling : settings.grokInstalling}>
                {#if (r.b === 'claude' && settings.claudeInstalling) || (r.b === 'codex' && settings.codexInstalling) || (r.b === 'grok' && settings.grokInstalling)}
                  <span class="wr-spin"></span>{settings.nodeBoot || `安装中 ${elapsedLabel(r.b === 'claude' ? settings.claudeElapsed : r.b === 'codex' ? settings.codexElapsed : settings.grokElapsed)}`}
                {:else}一键安装{/if}
              </button>
              <button class="cli-copy" title={`手动安装命令：${r.cmd}（点击复制）`} aria-label="复制手动安装命令" onclick={() => copyCmd(r.cmd)}>
                {#if settings.copiedCmd === r.cmd}
                  <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M3.5 8.5l3 3 6-7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" /></svg>
                {:else}
                  <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="5.4" y="5.4" width="8.2" height="8.2" rx="1.6" stroke="currentColor" stroke-width="1.3" /><path d="M10.6 5.4V4.1A1.6 1.6 0 0 0 9 2.5H4.1A1.6 1.6 0 0 0 2.5 4.1V9a1.6 1.6 0 0 0 1.6 1.6h1.3" stroke="currentColor" stroke-width="1.3" /></svg>
                {/if}
              </button>
            </div>
          {/if}
          {#if r.st.found}
            <div class="cust">
              <!-- 自定义模型 与 账号 两个折叠头并排一行（默认都收起），展开体各自出现在行下方。
                   账号从各 CLI 本地凭据只读提取（brains.rs）；重新绑定 = headless logout
                   + 原有授权终端流程登录新账号 -->
              <div class="cust-heads">
                <button class="cust-head" type="button" onclick={() => (settings.customOpen[r.b] = !settings.customOpen[r.b])}>
                  <svg class="cust-chev" class:open={settings.customOpen[r.b]} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  自定义模型{#if customsOf(r.b).length}<span class="cust-n">{customsOf(r.b).length}</span>{/if}
                </button>
                {#if r.st.logged_in}
                  <button class="cust-head" type="button" onclick={() => (settings.acctOpen[r.b] = !settings.acctOpen[r.b])}>
                    <svg class="cust-chev" class:open={settings.acctOpen[r.b]} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                    账号
                    {#if r.b === 'claude' && settings.claudeVerify === 'bad'}
                      <span class="verify-bad">凭据已失效</span>
                    {:else if r.b === 'claude' && settings.claudeVerify === 'checking'}
                      <span class="verify-checking">验证中…</span>
                    {/if}
                  </button>
                {/if}
              </div>
              {#if settings.customOpen[r.b]}
                <div class="cust-body">
                  {#each customsOf(r.b) as id (id)}
                    <div class="cust-chip"><span class="cust-id">{id}</span><button class="cust-x" title="删除" onclick={() => removeCustom(r.b, id)}>×</button></div>
                  {/each}
                  <div class="cust-add">
                    <input class="tin" bind:value={settings.customDraft[r.b]} placeholder={r.b === 'codex' ? '如 gpt-5.5 / o3' : r.b === 'grok' ? '如 grok-4.6' : '如 claude-opus-4-8'}
                      spellcheck="false" autocapitalize="off" autocorrect="off" onkeydown={(e) => e.key === 'Enter' && addCustom(r.b)} />
                    <button class="btn sm" onclick={() => addCustom(r.b)} disabled={!(settings.customDraft[r.b] ?? '').trim()}>添加</button>
                  </div>
                </div>
              {/if}
              {#if settings.acctOpen[r.b] && r.st.logged_in}
                <div class="brain-auth">
                  {#if r.st.account}<span class="brain-acct" title={`当前账号：${r.st.account}`}>{r.st.account}</span>{/if}
                  {#if r.st.account}<span class="brain-auth-sep" aria-hidden="true">·</span>{/if}
                  {#if r.b === 'claude'}
                    <button
                      class="brain-auth-icon"
                      class:active={settings.claudeTokenOpen}
                      type="button"
                      aria-label="更换 Claude 订阅 Token"
                      aria-expanded={settings.claudeTokenOpen}
                      data-tip="更换订阅 Token&#10;粘贴 claude setup-token 生成的长期授权 Token"
                      onclick={() => (settings.claudeTokenOpen = !settings.claudeTokenOpen)}
                    ><svg width="14" height="14" viewBox="0 0 18 18" fill="none" aria-hidden="true"><circle cx="5.5" cy="12.5" r="3.25" stroke="currentColor" stroke-width="1.45"/><path d="m7.85 10.15 6.55-6.55M11.45 6.55l1.7 1.7M13.15 4.85l1.35 1.35" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
                  {/if}
                  <button
                    class="brain-auth-icon"
                    type="button"
                    aria-label={settings.authWaiting === r.b ? '正在重新绑定账号' : '重新绑定账号'}
                    data-tip={settings.authWaiting === r.b ? '正在等待账号授权…' : '退出当前账号并重新登录'}
                    onclick={() => rebind(r.b)}
                    disabled={settings.authWaiting === r.b}
                  >{#if settings.authWaiting === r.b}<span class="wr-spin"></span>{:else}<RefreshIcon spinning={false} />{/if}</button>
                </div>
              {/if}
            </div>
          {/if}
          </div>
        {/each}
        </div>
      {/if}
      <ApiServices profiles={app.apiProfiles} onchange={(profiles: ApiProfile[]) => { app.apiProfiles = profiles; }} />
      <TypeSafeSection />

      <BackgroundSection />

      <div class="brain-row about-card">
        <span class="brain-ic"><AppIcon size={20} /></span>
        <span class="brain-main"><b>Cligc Pilot</b><small>{updater.appVersion ? `v${updater.appVersion}` : ''}{updater.updMsg ? ` · ${updater.updMsg}` : ''}</small></span>
        <button class="btn small ghost bare about-update-entry" onclick={openUpdateCenter} disabled={!isMainWindow}>
          <UpdateIcon size={14} />
          <span>{updater.updAvail ? `新版本 ${updater.updAvail}` : '检查更新'}</span>
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; }
  .dot.ok { background: #16a34a; }
  .dot.warn { background: #d97706; }
  .dot.off { background: #cfccc4; }
  .cli-copy { flex: none; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 5px; color: var(--faint); cursor: pointer; }
  .cli-copy:hover { background: rgba(0, 0, 0, .06); color: var(--text); }
  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  .tin { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1.5px solid var(--border2); border-radius: 8px; padding: 5px 10px; }
  /* 单行输入（text/password/number/datetime-local）：显式统一高 --ctl-h，内容垂直居中；
     去掉竖向 padding 防 macOS 原生步进器/日期控件把高度顶开。textarea 不定高，不吃这条。 */
  input.tin { height: var(--ctl-h); box-sizing: border-box; padding-top: 0; padding-bottom: 0; line-height: normal; }
  .tin:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }
  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1.5px solid var(--border2); border-radius: 8px; padding: 6px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
  .btn:hover { background: #f6f5f1; }
  .btn:disabled { opacity: .5; cursor: default; }
  .btn.ghost { border-color: var(--border); }
  .btn.small { padding: 4px 10px; font-size: 12px; }
  .btn.sm { padding: 5px 12px; font-size: 12px; border-radius: 8px; }
  /* 无边框动作按钮（头部次要动作）：去边框去底，仅 hover 有淡底 */
  .btn.bare { border: none; background: none; }
  .btn.bare:hover { background: var(--rail); border-color: transparent; }
  /* 这一行右侧的动作位（去授权 / 粘贴 Token / 升级）统一 26px 高，换哪个都不动行高 */
  .authbtn { display: inline-flex; align-items: center; gap: 3px; box-sizing: border-box; height: 26px; background: none; border: none; padding: 0 2px; color: var(--accent); font-size: 12px; font-weight: 550; line-height: 1; cursor: pointer; }
  .authbtn:hover { color: var(--accent-h); }
  .setup-add { position: relative; display: inline-flex; }
  .setup-add-trigger { width: 27px; height: 27px; padding: 0; }
  .setup-add-trigger.open { color: var(--text); background: #f1efe9; }
  .setup-add-menu {
    position: absolute; z-index: 8; top: calc(100% + 5px); right: 0; width: 240px; padding: 5px;
    background: var(--panel); border: 1px solid var(--border); border-radius: 10px;
    box-shadow: 0 10px 28px rgba(30, 25, 15, .15); animation: pop .1s ease-out;
  }
  .setup-add-menu button {
    width: 100%; min-height: 34px; padding: 7px 9px; border: 0; border-radius: 7px; background: none;
    display: flex; align-items: center; gap: 9px; color: var(--text); cursor: pointer; text-align: left;
    font: inherit; font-size: 12.5px; font-weight: 500; letter-spacing: 0; text-transform: none;
  }
  .setup-add-menu button:hover { background: #f4f3ef; }
  .setup-add-menu button:disabled { opacity: .5; cursor: default; }
  .setup-add-menu button :global(svg) { flex: none; color: var(--dim); }
  .setup-add-menu button span { display: flex; flex-direction: column; line-height: 1.35; }
  .setup-add-menu small { font-size: 10.5px; color: var(--faint); font-weight: 400; }
  .menu-sep { height: 1px; margin: 4px 6px; background: var(--border); }
  .wr-btn { flex: none; border: none; background: #d9a400; color: #fff; font-size: 11px; font-weight: 500; padding: 3px 9px; border-radius: 6px; cursor: pointer; }
  .wr-btn:hover { background: #c08f00; }
  .wr-btn:disabled { opacity: .85; cursor: default; }
  .wr-spin { display: inline-block; width: 10px; height: 10px; border: 1.6px solid #fff; border-right-color: transparent; border-radius: 50%; animation: rspin .7s linear infinite; margin-right: 5px; vertical-align: -1px; }
  .icon-btn { background: none; border: none; cursor: pointer; padding: 6px; border-radius: 8px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .icon-btn:hover { background: #f1efe9; color: var(--text); }
  .icon-btn:disabled { opacity: .55; cursor: default; }
  /* 标题旁的辅助图标（刷新）：平时淡一点，停在上面才深 */
  .icon-btn.quiet { color: var(--faint); }
  .icon-btn.quiet:hover, .icon-btn.quiet:focus-visible { color: var(--text); }
  @keyframes rspin { to { transform: rotate(360deg); } }
  @keyframes pop { from { opacity: 0; transform: translateY(4px); } }
  .x { background: none; border: none; color: var(--faint); font-size: 20px; cursor: pointer; line-height: 1; }
  .x:hover { color: var(--err); }
  .mask { position: fixed; inset: 0; background: rgba(25,20,10,.28); z-index: 50; }
  .sheet { position: fixed; z-index: 60; background: var(--bg); border: 1px solid var(--border); box-shadow: var(--shadow-lg); display: flex; flex-direction: column; }
  .sheet { top: 0; right: 0; bottom: 0; width: min(400px, 92vw); border-radius: 0; }
  .sheet-head { box-sizing: border-box; min-height: 48px; display: flex; justify-content: space-between; align-items: center; padding: 7px 14px; border-bottom: 1px solid var(--border); }
  .sheet-head > b { min-width: 0; overflow: hidden; font-size: 13.5px; line-height: 1.2; text-overflow: ellipsis; white-space: nowrap; }
  /* 弹窗正文统一只允许纵向滚动。只声明 overflow-y:auto 会按 CSS 规则把横向轴
     隐式计算成 auto；贴边 footer 的负边距和 WebView 的浮点取整都会因此冒出横向条。 */
  /* scrollbar-gutter: stable —— 一直给滚动条留位置：展开 / 收起连接组时滚动条忽有忽无，整页会左右跳一下 */
  .sheet-body { min-width: 0; padding: 16px 12px 16px 18px; overflow-x: hidden; overflow-y: auto; scrollbar-gutter: stable; display: flex; flex-direction: column; gap: 7px; }
  .about-update-entry { display: inline-flex; align-items: center; gap: 5px; }
  .about-update-entry :global(svg) { flex: none; color: var(--dim); }
  .conn-remark { cursor: help; }
  /* 设置里每一组的标题：和 API 服务、判断模型、后台运行与备份同一个样子。
     组与组之间 24px（17 + 正文 gap 7），标题到内容 8px（1 + 7）；内容统一放在白底细边框的卡片里 */
  .sec-head { min-height: 24px; display: flex; justify-content: space-between; align-items: center; font-size: 11px; letter-spacing: .03em; text-transform: uppercase; color: var(--faint); font-weight: 600; margin-bottom: 1px; }
  .sec-head.mt { margin-top: 17px; }
  .sec-title { display: inline-flex; align-items: center; }
  .conn-groups { display: flex; flex-direction: column; gap: 6px; }
  .cr-os { flex: none; display: inline-flex; width: 14px; margin-left: -22px; } /* 放进左侧缩进里，机器名和其他组的名字对齐 */
  .conn-row :global(.sm) { border-radius: 6px; }
  .cr-main { flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; background: none; border: 0; padding: 0; margin: 0; font: inherit; color: inherit; text-align: left; cursor: pointer; border-radius: 6px; }
  .cr-main:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .icon-btn.sm { padding: 4px; border-radius: 7px; }
  .brain-row { display: flex; align-items: center; gap: 11px; padding: 0; }
  .brain-auth-actions { flex: none; display: inline-flex; align-items: center; gap: 8px; }
  .claude-token-trigger { flex: none; width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center; padding: 0; border: 0; border-radius: 7px; background: transparent; color: var(--dim); cursor: pointer; }
  .claude-token-trigger:hover, .claude-token-trigger.active { background: #f1efe9; color: var(--accent); }
  .claude-token-form { margin: 7px 0 2px 33px; padding: 10px; display: grid; gap: 8px; border: 1px solid var(--border); border-radius: 10px; background: #faf9f6; }
  .claude-token-copy { min-width: 0; display: grid; gap: 3px; }
  .claude-token-copy b { font-size: 11.5px; font-weight: 650; }
  .claude-token-copy small, .claude-token-safe { color: var(--dim); font-size: 9.5px; line-height: 1.45; }
  .claude-token-copy code { padding: 1px 4px; border-radius: 4px; background: #efede7; color: var(--text); font: 500 9.5px/1.3 ui-monospace, SFMono-Regular, Menlo, monospace; }
  .claude-token-entry { display: flex; align-items: center; gap: 6px; }
  .claude-token-entry input { min-width: 0; flex: 1; height: 30px; box-sizing: border-box; padding: 0 9px; border: 1px solid var(--border2); border-radius: 7px; outline: none; background: #fff; color: var(--text); font: 11px ui-monospace, SFMono-Regular, Menlo, monospace; }
  .claude-token-entry input:focus { border-color: #c7a599; box-shadow: 0 0 0 2px rgb(160 60 43 / 8%); }
  .claude-token-entry .btn { flex: none; }
  /* 状态点放进与 .icon-btn 同宽(27px)的居中盒，右缘、图标中心都与本地模型区的刷新图标对齐。 */
  .brain-dot { flex: none; width: 27px; display: inline-flex; align-items: center; justify-content: center; }
  /* 「账号 / 自定义模型」两个折叠头并排一行；展开体各自出现在行下 */
  .cust-heads { display: flex; align-items: center; gap: 16px; }
  /* 当前账号 + 紧凑操作：展开体缩进对齐 chevron 后的文字；完整动作通过 tips 说明。 */
  .brain-auth { display: flex; align-items: center; gap: 6px; margin: 3px 0 1px 15px; min-width: 0; }
  .brain-acct { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--dim); font-size: 11px; }
  .brain-auth-sep { color: var(--faint); font-size: 11px; }
  .brain-auth-icon { flex: none; width: 20px; height: 20px; display: inline-flex; align-items: center; justify-content: center; padding: 0; border: none; border-radius: 5px; background: transparent; color: var(--faint); cursor: pointer; }
  .brain-auth-icon:hover, .brain-auth-icon.active { color: var(--accent); background: #f1efe9; }
  .brain-auth-icon:disabled { opacity: .6; cursor: default; }
  .brain-auth-icon :global(.rfz) { width: 12px; height: 12px; }
  .brain-auth-icon .wr-spin { width: 10px; height: 10px; }
  .cli-new { color: var(--ok); }
  .cli-upg { flex: none; width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 7px; color: var(--ok); cursor: pointer; }
  .cli-upg:hover:not(:disabled) { background: #f1efe9; }
  .cli-upg:disabled { color: var(--faint); cursor: default; }
  .verify-bad { color: #b45309; font-size: 10px; font-weight: 550; white-space: nowrap; }
  .verify-checking { color: var(--faint); font-size: 10px; white-space: nowrap; }
  .brain-ic { flex: none; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; }
  .brain-main { flex: 1; min-width: 0; }
  .brain-main b { display: block; font-size: 13.5px; line-height: 1.15; }
  .brain-main small { display: block; color: var(--dim); font-size: 11px; line-height: 1.1; }
  /* 自定义模型：折叠管理器（对齐大脑名，缩进 33px = 图标 22 + gap 11） */
  /* 大脑列表：整列作为一个 sheet-body 子项，块间距自控（不吃 sheet-body 的 7px gap）。 */
  .brains-list { display: flex; flex-direction: column; border: 1px solid var(--border); border-radius: 11px; background: #fff; overflow: hidden; }
  .brains-list > .brain-block { padding: 10px 12px; }
  .brains-list > .brain-block + .brain-block { border-top: 1px solid var(--border); }
  /* 版本和检查更新：只有一行，不要标题和卡片、不缩进，左右和上面卡片的边对齐 */
  .about-card { padding: 2px 0; margin-top: 17px; } /* 没有标题：和上一组隔开的距离同组与组之间（17 + gap 7） */
  .about-card .about-update-entry { margin-right: -10px; } /* 按钮的内边距伸出去，文字贴齐右边；悬停底色照样左右对称 */
  /* 每个大脑（行 + 自定义模型）成一块，块内贴紧。 */
  .brain-block { display: flex; flex-direction: column; gap: 0; }
  /* 未安装行：一键安装挪到状态文字下方（缩进对齐 brain-main），复制小图标同行在旁 */
  .brain-install { display: flex; align-items: center; gap: 5px; margin: 3px 0 2px 33px; }
  .cust { margin: 4px 0 0 33px; }
  .cust-head { display: inline-flex; align-items: center; gap: 5px; background: none; border: none; padding: 0; cursor: pointer; font: inherit; font-size: 11.5px; color: var(--dim); -webkit-appearance: none; appearance: none; }
  .cust-head:hover { color: var(--text); }
  .cust-chev { color: var(--faint); flex: none; transition: transform .12s; }
  .cust-chev:not(.open) { transform: rotate(-90deg); }
  .cust-n { min-width: 15px; height: 15px; padding: 0 4px; display: inline-flex; align-items: center; justify-content: center; background: var(--border); color: var(--dim); border-radius: 8px; font-size: 10px; font-weight: 600; }
  .cust-body { display: flex; flex-direction: column; gap: 5px; margin: 4px 0 4px; }
  .cust-chip { display: flex; align-items: center; gap: 6px; background: #f6f5f1; border: 1px solid var(--border); border-radius: 8px; padding: 4px 6px 4px 9px; }
  .cust-id { flex: 1; min-width: 0; font-family: ui-monospace, monospace; font-size: 11.5px; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .cust-x { flex: none; background: none; border: none; color: var(--faint); font-size: 15px; line-height: 1; padding: 0 3px; border-radius: 5px; cursor: pointer; }
  .cust-x:hover { color: var(--err); background: #fff; }
  .cust-add { display: flex; gap: 6px; align-items: center; }
  .cust-add .tin { flex: 1; min-width: 0; width: auto; }
  .cust-add .btn { flex: none; }
  .hint { color: var(--dim); font-size: 12px; margin: 2px 0; line-height: 1.6; }
  .tin { width: 100%; }
</style>
