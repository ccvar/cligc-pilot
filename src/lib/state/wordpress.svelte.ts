// WordPress.com 账号（「连接 → WordPress.com」）：OAuth 应用是用户自己在 developer.wordpress.com/apps 注册的，
// 客户端 ID / 密钥填一次；每个账号在浏览器里授权一次（令牌不过期）。站点列表按账号缓存。
import { invoke } from '@tauri-apps/api/core';
import type { Connection } from '$lib/types';
import { app, say, refreshConns } from './app.svelte.js';

export type WpSite = { id: string; name: string; url: string };
export type WpClient = { configured: boolean; client_id: string; redirect: string };

class WpState {
  busy = $state(false);
  /** 正在填应用信息（「连接」里展开那张表） */
  setup = $state(false);
  client = $state<WpClient | null>(null);
  sites = $state<Record<string, WpSite[]>>({});
  sitesErr = $state<Record<string, string>>({});
}
export const wp = new WpState();

export const wpAccounts = () => app.conns.filter((c) => c.kind === 'wordpress');

export async function loadWpClient() {
  try { wp.client = await invoke<WpClient>('wpcom_client_info'); } catch { /* */ }
}

/** 加一个 WordPress.com 账号：还没配应用就先展开填应用信息，配好了直接去浏览器授权。 */
export async function startWpConnect() {
  await loadWpClient();
  if (!wp.client?.configured) { wp.setup = true; return; }
  await wpLogin('');
}

export async function saveWpClient(clientId: string, clientSecret: string): Promise<boolean> {
  try { wp.client = await invoke<WpClient>('wpcom_save_client', { clientId, clientSecret }); wp.setup = false; return true; }
  catch (e) { say(String(e), 'err'); return false; }
}

export async function wpLogin(updateId: string) {
  if (wp.busy) return;
  wp.busy = true;
  say('在浏览器里登录 WordPress.com，点「批准」');
  try {
    const c = await invoke<Connection>('wpcom_login', { updateId });
    await refreshConns();
    delete wp.sites[c.id];
    say(`已接上 WordPress.com：${c.login || c.name}`);
  } catch (e) { say(String(e), 'err'); }
  finally { wp.busy = false; }
}

export async function loadWpSites(connId: string, force = false) {
  if (!connId || (!force && wp.sites[connId])) return;
  try {
    const list = await invoke<WpSite[]>('wpcom_sites', { connId });
    wp.sites = { ...wp.sites, [connId]: list };
    wp.sitesErr = { ...wp.sitesErr, [connId]: '' };
  } catch (e) { wp.sitesErr = { ...wp.sitesErr, [connId]: String(e) }; }
}
