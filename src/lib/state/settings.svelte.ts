import { invoke } from '@tauri-apps/api/core';
import { untrack } from 'svelte';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { savePrefs } from '$lib/defaults';
import type { Brain, CliUpdateInfo } from '$lib/types';
import { say, app, refreshBrains, confirmDialog } from './app.svelte.js';
import { customsOf, defaultModelFor } from './models';
import { connections } from './connections.svelte.js';
import { launcher } from './launcher.svelte.js';

class SettingsState {
  importBusy = $state(false);
  setupOpen = $state(false);
  setupAddOpen = $state(false);
  setupAddEl = $state<HTMLElement | null>(null);
  claudeInstalling = $state(false);
  claudeElapsed = $state(0);
  claudeTimer: ReturnType<typeof setInterval> | undefined;
  codexInstalling = $state(false);
  codexElapsed = $state(0);
  codexTimer: ReturnType<typeof setInterval> | undefined;
  grokInstalling = $state(false);
  grokElapsed = $state(0);
  grokTimer: ReturnType<typeof setInterval> | undefined;
  // 托管 Node 自举进度（后端 "node-boot" 事件）：一键安装按钮上分步显示。
  nodeBoot = $state('');
  // 全局自定义模型 ID（按厂商，可多个，在「连接与模型」里增删）；作为该厂商模型下拉的附加档位。
  customDraft = $state<Record<string, string>>({ claude: '', codex: '', grok: '' });
  customOpen = $state<Record<string, boolean>>({ claude: false, codex: false, grok: false });
  /** 账号行（邮箱+重新绑定）默认收起：多数时候不需要看，展开才见。 */
  acctOpen = $state<Record<string, boolean>>({ claude: false, codex: false, grok: false });
  copiedCmd = $state('');
  brainsBusy = $state(false);
  // 一键授权：开系统终端跑官方 CLI 登录命令（浏览器 OAuth 在终端流程里完成，Pilot 绝不代输凭据），
  // 随后进入「等待授权…」：每 4s 静默重检 brains，logged_in 翻 true 即复位绿灯；120s 超时自动复位。
  authWaiting = $state<Brain | ''>('');
  authTimer: ReturnType<typeof setInterval> | undefined;
  claudeTokenOpen = $state(false);
  claudeTokenDraft = $state('');
  claudeTokenSaving = $state(false);
  /** Claude 凭据深度验证：auth status 只读本地记录会说谎（桌面端换号后旧 token 已废
   *  它照样报已登录）——打开设置时真跑一次最小无头调用（30 分钟缓存；跑一次是一次
   *  最小 haiku 计费，所以只在设置面板/手动刷新触发，不进 4 秒轮询）。 */
  claudeVerify = $state<'unknown' | 'checking' | 'ok' | 'bad'>('unknown');
  claudeVerifyAt = 0;

  // ---------- Claude CLI 版本提示 + 一键升级 ----------
  // 只在设置面板打开时查一次（要一次 npm registry 请求），不在启动路径上加网络往返。
  cliUpd = $state<CliUpdateInfo | null>(null);
  cliUpdBusy = $state(false);
  cliUpgrading = $state(false);
}

export const settings = new SettingsState();

export function openConnectionSetup(event?: MouseEvent) {
  event?.preventDefault();
  event?.stopPropagation();
  settings.setupOpen = true;
  settings.setupAddOpen = false;
  connections.connCtx = null;
  connections.switcherOpen = false;
}
export async function installCodex() {
  if (settings.codexInstalling) return;
  settings.codexInstalling = true; settings.codexElapsed = 0;
  const t0 = Date.now();
  settings.codexTimer = setInterval(() => { settings.codexElapsed = Date.now() - t0; }, 500);
  try { const m = await invoke<string>('install_codex'); say(m); await refreshBrainsManual(); }
  catch (e) { say(String(e), 'err', 20000); }
  finally { settings.codexInstalling = false; settings.nodeBoot = ''; if (settings.codexTimer) { clearInterval(settings.codexTimer); settings.codexTimer = undefined; } }
}
export async function installGrok() {
  if (settings.grokInstalling) return;
  settings.grokInstalling = true; settings.grokElapsed = 0;
  const t0 = Date.now();
  settings.grokTimer = setInterval(() => { settings.grokElapsed = Date.now() - t0; }, 500);
  try { const m = await invoke<string>('install_grok'); say(m); await refreshBrainsManual(); }
  catch (e) { say(String(e), 'err', 20000); }
  finally { settings.grokInstalling = false; settings.nodeBoot = ''; if (settings.grokTimer) { clearInterval(settings.grokTimer); settings.grokTimer = undefined; } }
}
export async function installClaude() {
  if (settings.claudeInstalling) return;
  settings.claudeInstalling = true; settings.claudeElapsed = 0;
  const t0 = Date.now();
  settings.claudeTimer = setInterval(() => { settings.claudeElapsed = Date.now() - t0; }, 500);
  try { const m = await invoke<string>('install_claude'); say(m); await refreshBrainsManual(); }
  catch (e) { say(String(e), 'err', 20000); } // 详情含输出末尾，多留时间读/滚动
  finally { settings.claudeInstalling = false; settings.nodeBoot = ''; if (settings.claudeTimer) { clearInterval(settings.claudeTimer); settings.claudeTimer = undefined; } }
}
export function addCustom(b: string) {
  const v = (settings.customDraft[b] ?? '').trim();
  if (!v) return;
  const arr = customsOf(b);
  if (!arr.includes(v)) { arr.push(v); savePrefs(launcher.prefs); }
  settings.customDraft[b] = '';
}
export function removeCustom(b: string, id: string) {
  const arr = customsOf(b);
  const i = arr.indexOf(id);
  if (i < 0) return;
  arr.splice(i, 1);
  savePrefs(launcher.prefs);
  if (launcher.lBrain === b && launcher.lModel === id) launcher.lModel = defaultModelFor(b);
}
export async function copyCmd(cmd: string) {
  try { await navigator.clipboard.writeText(cmd); settings.copiedCmd = cmd; setTimeout(() => { if (settings.copiedCmd === cmd) settings.copiedCmd = ''; }, 1500); }
  catch { say('复制失败，请手动选中命令复制', 'err'); }
}
// 手动重新检测：走 redetect_brains（后端先重读登录 shell 的 PATH，再探测）——治「装/登录完仍显示未安装/未登录」。
export async function refreshBrainsManual() { settings.brainsBusy = true; try { app.brains = await invoke('redetect_brains'); } catch (e) { say(String(e), 'err'); } finally { settings.brainsBusy = false; } void verifyClaude(true); }
function stopAuthWait() { settings.authWaiting = ''; if (settings.authTimer) { clearInterval(settings.authTimer); settings.authTimer = undefined; } }
export async function authorize(b: Brain) {
  try {
    await invoke('open_brain_login', { brain: b });
    say('已打开终端，请在终端里完成登录——完成后这里自动变绿');
    settings.authWaiting = b;
    const deadline = Date.now() + 120_000;
    if (settings.authTimer) clearInterval(settings.authTimer);
    settings.authTimer = setInterval(() => {
      if (Date.now() > deadline) { stopAuthWait(); return; }
      void refreshBrains();
    }, 4000);
  } catch (e) { say(String(e), 'err'); }
}
export async function saveClaudeToken() {
  const token = settings.claudeTokenDraft.trim();
  if (!token || settings.claudeTokenSaving) return;
  settings.claudeTokenSaving = true;
  try {
    await invoke('save_claude_oauth_token', { token });
    settings.claudeTokenDraft = '';
    settings.claudeTokenOpen = false;
    // 后端已经用候选 Token 做过一次真实最小调用；不要在这里再计费验证第二次。
    settings.claudeVerify = 'ok';
    settings.claudeVerifyAt = Date.now();
    await refreshBrains();
    if (!app.brains?.claude.logged_in) {
      settings.claudeTokenOpen = true;
      say('Claude CLI 未接受这个 Token，请确认由 claude setup-token 生成且复制完整', 'err');
      return;
    }
    say('Claude 订阅 Token 已验证并安全保存');
  } catch (e) {
    say(String(e), 'err');
  } finally {
    settings.claudeTokenSaving = false;
  }
}
export function closeClaudeToken() {
  if (settings.claudeTokenSaving) return;
  settings.claudeTokenDraft = '';
  settings.claudeTokenOpen = false;
}
async function verifyClaude(force = false) {
  if (!app.brains?.claude.found || !app.brains.claude.logged_in) { settings.claudeVerify = 'unknown'; return settings.claudeVerify; }
  if (settings.claudeVerify === 'checking') return settings.claudeVerify;
  if (!force && Date.now() - settings.claudeVerifyAt < 30 * 60_000) return settings.claudeVerify;
  settings.claudeVerify = 'checking';
  try {
    const ok = await invoke<boolean>('brain_verify_claude');
    settings.claudeVerify = ok ? 'ok' : 'bad';
  } catch {
    settings.claudeVerify = 'unknown'; // 网络/档位类异常：不冤枉凭据，也不背书
  }
  settings.claudeVerifyAt = Date.now();
  return settings.claudeVerify;
}
async function checkCliUpdate() {
  if (settings.cliUpdBusy) return;
  settings.cliUpdBusy = true;
  try { settings.cliUpd = await invoke<CliUpdateInfo>('check_claude_update'); }
  catch { settings.cliUpd = null; } // 查不到（离线/无 npm）就当没有提示，别拿红字骚扰
  finally { settings.cliUpdBusy = false; }
}
export async function upgradeCli() {
  if (!settings.cliUpd || settings.cliUpgrading) return;
  // 会改本机环境 → 必须先确认，并把「升的是哪一个」摆明：多安装时只有 PATH 第一个生效。
  const multi = settings.cliUpd.all.length > 1
    ? `\n\n注意：PATH 上有 ${settings.cliUpd.all.length} 份 claude，只有第一个会被真正调用：\n${settings.cliUpd.all.map((p, i) => `${i === 0 ? '→ ' : '   '}${p}`).join('\n')}`
    : '';
  const yes = await confirmDialog(
    `将对 ${settings.cliUpd.target} 执行 claude update，从 ${settings.cliUpd.installed || '未知'} 升到 ${settings.cliUpd.latest}。${multi}`,
    { title: '升级 Claude Code CLI', kind: 'warning' },
  );
  if (!yes) return;
  settings.cliUpgrading = true;
  try {
    const out = await invoke<string>('upgrade_claude_cli');
    say(out.trim() ? out.trim().split('\n').slice(-2).join(' · ') : '升级完成');
    settings.cliUpd = null;
    await refreshBrainsManual(); // 重探版本，UI 立刻反映新版本与 fast 可用性
    void checkCliUpdate();
  } catch (e) { say(String(e), 'err'); }
  finally { settings.cliUpgrading = false; }
}
/** 重新绑定：先 headless 退出当前账号，再走原有授权终端登录新账号（等待轮询复用 authorize）。 */
export async function rebind(b: Brain) {
  try {
    await invoke('brain_logout', { brain: b });
  } catch (e) { say(String(e), 'err'); return; }
  say('已退出当前账号');
  void refreshBrains();
  await authorize(b);
}

export function initSettings() {
  $effect(() => {
    if (!settings.setupAddOpen) return;
    const onDoc = (e: MouseEvent) => {
      if (settings.setupAddEl && !settings.setupAddEl.contains(e.target as Node)) settings.setupAddOpen = false;
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') settings.setupAddOpen = false; };
    document.addEventListener('mousedown', onDoc, true);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDoc, true);
      document.removeEventListener('keydown', onKey);
    };
  });
  $effect(() => {
    const un = getCurrentWindow().listen<{ phase: string; pct: number }>('node-boot', (e) => {
      const p = e.payload;
      settings.nodeBoot = p.phase === 'download' ? `下载 Node ${p.pct}%`
        : p.phase === 'extract' ? '解压 Node…'
        : p.phase === 'verify' ? '校验 Node…'
        : p.phase === 'grok-download' ? `下载 Grok ${p.pct}%`
        : p.phase === 'grok-verify' ? '校验 Grok…'
        : '安装 CLI…';
    });
    return () => { void un.then((f) => f()); };
  });
  $effect(() => {
    if (!settings.authWaiting || !app.brains) return;
    const st = settings.authWaiting === 'claude' ? app.brains.claude : settings.authWaiting === 'codex' ? app.brains.codex : app.brains.grok;
    if (st.found && st.logged_in) {
      const wasClaude = settings.authWaiting === 'claude';
      stopAuthWait(); say('授权完成，已就绪');
      if (wasClaude) void verifyClaude(true); // 新登录立即复验，别让旧的「失效」标记赖着
    }
  });
  // untrack：这两个函数一进来就读写 busy / 时间戳，被 effect 追踪的话会自己触发自己。
  // 尤其 checkCliUpdate 查不到（离线 / 没有 npm）时 cliUpd 一直是 null，会无限重查、把页面卡死。
  $effect(() => { if (settings.setupOpen) untrack(() => void verifyClaude()); });
  $effect(() => { if (settings.setupOpen && app.brains?.claude.found && !settings.cliUpd) untrack(() => void checkCliUpdate()); });
}
