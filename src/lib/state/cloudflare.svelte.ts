import { invoke } from '@tauri-apps/api/core';
import type { Connection } from '$lib/types';
import { app, say, refreshConns } from './app.svelte.js';
import { connections, selectConn } from './connections.svelte.js';
import { panes } from './panes.svelte.js';
import { thread } from './thread.svelte.js';
import { composer } from './composer.svelte.js';
import { refreshBrainsManual } from './settings.svelte.js';

class CloudflareState {
  previewBusy = $state(false);
  cfReady = $state(false); // 项目是否已建出可预览/部署的内容（否则预览/部署置灰）
  wrInstalling = $state(false);
  wrElapsed = $state(0); // 安装 wrangler 已用时（毫秒），给个"在动"的实时反馈
  wrTimer: ReturnType<typeof setInterval> | undefined;
  cfOpen = $state(false);
  cfToken = $state('');
  cfVerifying = $state(false);
  cfConnecting = $state(false);
  cfErr = $state('');
  cfAccounts = $state<{ id: string; name: string }[]>([]);
  cfZones = $state<{ id: string; name: string }[]>([]);
  cfAccountId = $state('');
  cfName = $state('');
  cfPermsOpen = $state(false);
  cfUpdateId = $state('');
}

export const cf = new CloudflareState();

export async function checkCfReady() {
  if (!app.activeConvIsCf || !app.activeConv) { cf.cfReady = false; return; }
  const c=app.activeConv;
  try {
    const ready=await invoke<boolean>('cf_project_ready', { connId:c.conn_id, project:c.site_slug });
    panes.paneCfReady[c.id]=ready;
    if(app.activeConvId===c.id)cf.cfReady=ready;
  }
  catch { panes.paneCfReady[c.id]=false;if(app.activeConvId===c.id)cf.cfReady=false; }
}
/** 从预览 URL 里取 ":端口" 给提示用（端口是后端挑的，不再恒等于 8788）。 */
export function portOf(url: string): string {
  try { return ':' + new URL(url).port; } catch { return url; }
}
export async function startPreview() {
  if (!app.activeConv || cf.previewBusy) return;
  cf.previewBusy = true;
  say('正在启动本地预览…（起来就开窗）');
  try {
    // 端口是后端挑的空端口（8788 被别的程序占着时会自动往上让），所以别写死——用它返回的真 URL。
    const u = await invoke<string>('cf_preview_start', { connId: app.activeConv.conn_id, project: app.activeConv.site_slug });
    say(`本地预览已打开（${portOf(u)}）·关掉预览窗即停止`);
  } catch (e) { say(String(e), 'err'); }
  finally { cf.previewBusy = false; }
}
export async function installWrangler() {
  if (cf.wrInstalling) return;
  cf.wrInstalling = true; cf.wrElapsed = 0;
  const t0 = Date.now();
  cf.wrTimer = setInterval(() => { cf.wrElapsed = Date.now() - t0; }, 500);
  try { const m = await invoke<string>('install_wrangler'); say(m); await refreshBrainsManual(); }
  catch (e) { say(String(e), 'err'); }
  finally { cf.wrInstalling = false; if (cf.wrTimer) { clearInterval(cf.wrTimer); cf.wrTimer = undefined; } }
}
export function fillDeploy() {
  if (thread.viewBusy) return;
  // 预填一条部署指令，用户可补上域名后发送；真正的 wrangler deploy 会经权限确认。
  composer.draft = '把当前项目构建并部署到 Cloudflare Pages。若要绑定自定义域名，用这个域名：';
}

// ---------- 连接 Cloudflare ----------
export const CF_TOKEN_URL = 'https://dash.cloudflare.com/profile/api-tokens';
export const CF_TOKEN_PERMISSIONS = [
  { key: 'page', type: 'edit' },
  { key: 'workers_scripts', type: 'edit' },
  { key: 'd1', type: 'edit' },
  { key: 'dns', type: 'edit' },
  { key: 'zone', type: 'read' },
] as const;
export const CF_DASH_URL = 'https://dash.cloudflare.com/';
export function cfUpdateConnection(): Connection | null {
  return app.conns.find((connection) => connection.id === cf.cfUpdateId) ?? null;
}
export function cfTokenTemplateUrl(): string {
  const url = new URL(CF_TOKEN_URL);
  url.searchParams.set('permissionGroupKeys', JSON.stringify(CF_TOKEN_PERMISSIONS));
  url.searchParams.set('accountId', cfUpdateConnection()?.account_id || '*');
  url.searchParams.set('zoneId', 'all');
  url.searchParams.set('name', 'Cligc Pilot');
  return url.toString();
}
export function resetCfConnect(updateId = '') {
  const updating = app.conns.find((connection) => connection.id === updateId);
  cf.cfUpdateId = updating?.id ?? '';
  cf.cfOpen = true; cf.cfToken = ''; cf.cfErr = ''; cf.cfAccounts = []; cf.cfZones = [];
  cf.cfAccountId = updating?.account_id ?? '';
  cf.cfName = updating?.name ?? '';
  cf.cfPermsOpen = !!updating;
}
export function openCfConnect() { connections.switcherOpen = false; resetCfConnect(); }
export function openCfUpdate(c: Connection) { connections.connCtx = null; connections.switcherOpen = false; resetCfConnect(c.id); }
export function closeCfConnect() {
  if (cf.cfConnecting || cf.cfVerifying) return;
  cf.cfOpen = false;
  cf.cfUpdateId = '';
}
export async function verifyCf() {
  if (!cf.cfToken.trim()) return;
  cf.cfVerifying = true; cf.cfErr = '';
  try {
    const v = await invoke<{ token_status: string; accounts: { id: string; name: string }[]; zones: { id: string; name: string }[] }>('verify_cf_token', { token: cf.cfToken.trim() });
    cf.cfAccounts = v.accounts; cf.cfZones = v.zones;
    const updating = cfUpdateConnection();
    if (updating) {
      cf.cfAccountId = updating.account_id;
      cf.cfName = updating.name;
      if (!v.accounts.some((account) => account.id === updating.account_id)) {
        cf.cfErr = `Token 有效，但无法访问原账号 ${updating.account_id}。请为这条连接创建属于同一账号的 Token。`;
      }
    } else {
      if (v.accounts.length === 1) cf.cfAccountId = v.accounts[0].id;
      if (!cf.cfName.trim() && v.accounts.length) cf.cfName = (v.accounts.find((a) => a.id === cf.cfAccountId) ?? v.accounts[0]).name;
      if (!v.accounts.length) cf.cfErr = 'Token 有效，但没有可用账号（需要 Account 级权限）。';
    }
  } catch (e) { cf.cfErr = String(e); }
  finally { cf.cfVerifying = false; }
}
export function resetCfVerification() {
  cf.cfAccounts = [];
  cf.cfZones = [];
  cf.cfErr = '';
  if (!cf.cfUpdateId) cf.cfAccountId = '';
}
export async function confirmCf() {
  if (!cf.cfAccountId) { cf.cfErr = '请先选择一个账号'; return; }
  if (!cf.cfAccounts.some((account) => account.id === cf.cfAccountId)) { cf.cfErr = '请先验证这枚 Token'; return; }
  cf.cfConnecting = true; cf.cfErr = '';
  try {
    const updating = cfUpdateConnection();
    const conn = updating
      ? await invoke<Connection>('update_cloudflare_token', { connId: updating.id, token: cf.cfToken.trim() })
      : await invoke<Connection>('connect_cloudflare', { name: cf.cfName.trim(), token: cf.cfToken.trim(), accountId: cf.cfAccountId });
    cf.cfOpen = false; cf.cfUpdateId = '';
    say(updating ? `已更新 Cloudflare 连接「${updating.remark?.trim() || conn.name}」` : `已连接 Cloudflare「${conn.name}」`);
    await refreshConns();
    await selectConn(conn.id);
  } catch (e) { cf.cfErr = String(e); }
  finally { cf.cfConnecting = false; }
}
