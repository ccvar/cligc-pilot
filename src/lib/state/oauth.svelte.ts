import { invoke } from '@tauri-apps/api/core';
import type { Connection } from '$lib/types';
import { say, refreshConns } from './app.svelte.js';
import { datasrc } from './datasrc.svelte.js';

class OAuthState {
  busy = $state(false);
  err = $state('');
}

export const oauth = new OAuthState();

/** 用对话框里填的 OAuth 客户端在浏览器里登录 Google；成功后新建（或更新）连接。 */
export async function oauthLogin() {
  const f = datasrc.form;
  oauth.busy = true; oauth.err = '';
  try {
    const conn = await invoke<Connection>('google_oauth_login', {
      name: f.name, clientId: f.clientId, clientSecret: f.clientSecret, manage: f.manage, updateId: f.updateId,
    });
    datasrc.form = { ...datasrc.form, open: false };
    delete datasrc.googleRes[conn.id];
    await refreshConns();
    say(`已连接 Google 账号 ${conn.login || ''}`);
  } catch (e) {
    const msg = String(e);
    if (!msg.includes('已取消')) oauth.err = msg;
  } finally { oauth.busy = false; }
}

export async function cancelOauth() {
  if (oauth.busy) { try { await invoke('google_oauth_cancel'); } catch { /* */ } }
}
