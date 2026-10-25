import { invoke } from '@tauri-apps/api/core';
import type { Connection } from '$lib/types';
import { app, say, refreshConns } from './app.svelte.js';
import { datasrc, autoBindSites } from './datasrc.svelte.js';

// Google OAuth 客户端是全局的（整个 Pilot 配一次）；「在浏览器中登录」只是再授权一个账号。
export interface OAuthClientInfo { configured: boolean; client_id: string }

class OAuthState {
  busy = $state(false);
  err = $state('');
  client = $state<OAuthClientInfo | null>(null);
  /** 正在填 / 改客户端（没配置时也是这个界面）。 */
  editing = $state(false);
  draftId = $state('');
  draftSecret = $state('');
  saving = $state(false);
}

export const oauth = new OAuthState();

export async function loadOauthClient() {
  try { oauth.client = await invoke<OAuthClientInfo>('google_oauth_client'); }
  catch { oauth.client = { configured: false, client_id: '' }; }
}

export function editOauthClient(on = true) {
  oauth.editing = on;
  oauth.err = '';
  oauth.draftId = on ? oauth.client?.client_id ?? '' : '';
  oauth.draftSecret = '';
}

export async function saveOauthClient(): Promise<boolean> {
  oauth.saving = true; oauth.err = '';
  try {
    oauth.client = await invoke<OAuthClientInfo>('save_google_oauth_client', { clientId: oauth.draftId, clientSecret: oauth.draftSecret });
    oauth.editing = false; oauth.draftSecret = '';
    return true;
  } catch (e) { oauth.err = String(e); return false; }
  finally { oauth.saving = false; }
}

/** 已经用浏览器授权过的 Google 账号（每个账号一条连接）。 */
export const oauthAccounts = () => app.conns.filter((c) => c.kind === 'google' && c.key_kind === 'oauth');

/** 用全局客户端在浏览器里授权一个 Google 账号：新账号新建连接，授权过的账号原地更新。 */
export async function oauthLogin() {
  const f = datasrc.form;
  oauth.busy = true; oauth.err = '';
  try {
    const before = new Set(oauthAccounts().map((c) => c.id));
    const conn = await invoke<Connection>('google_oauth_login', { name: f.name, manage: f.manage, updateId: f.updateId });
    datasrc.form = { ...datasrc.form, open: false };
    delete datasrc.googleRes[conn.id];
    await refreshConns();
    say(before.has(conn.id) ? `已重新授权 ${conn.login || conn.name}` : `已添加 Google 账号 ${conn.login || ''}`);
    // 接上就按域名把还没绑定的站点匹配一遍（只填空着的）
    void autoBindSites(conn.id);
  } catch (e) {
    const msg = String(e);
    if (!msg.includes('已取消')) oauth.err = msg;
  } finally { oauth.busy = false; }
}

export async function cancelOauth() {
  if (oauth.busy) { try { await invoke('google_oauth_cancel'); } catch { /* */ } }
}
