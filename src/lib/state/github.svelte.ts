import { invoke } from '@tauri-apps/api/core';
import type { Connection, GhRepo } from '$lib/types';
import { app, say, refreshConns } from './app.svelte.js';
import { connections, selectConn } from './connections.svelte.js';

type GhVerify = { login: string; user_id: number; name: string; repos: GhRepo[] };

class GithubState {
  // ---------- 连接 / 更新 Token 弹窗 ----------
  ghOpen = $state(false);
  ghToken = $state('');
  ghVerifying = $state(false);
  ghConnecting = $state(false);
  ghErr = $state('');
  ghVerified = $state<GhVerify | null>(null);
  ghName = $state('');
  ghPermsOpen = $state(false);
  ghUpdateId = $state('');
  // ---------- 当前连接的仓库 ----------
  /** Token 能访问的仓库（新建会话时选）；按连接缓存，切换连接时重拉。 */
  repos = $state<GhRepo[]>([]);
  reposConnId = $state('');
  reposLoading = $state(false);
  reposErr = $state('');
  /** 本地已检出的仓库（owner/repo）。 */
  checkouts = $state<string[]>([]);
}

export const gh = new GithubState();

// fine-grained Token 创建页：URL 参数预填名称、有效期与所需权限（GitHub 不认识的参数会被忽略）。
const GH_TOKEN_URL = 'https://github.com/settings/personal-access-tokens/new';
export function ghTokenTemplateUrl(): string {
  const url = new URL(GH_TOKEN_URL);
  url.searchParams.set('name', 'Cligc Pilot');
  url.searchParams.set('description', 'Cligc Pilot：建仓库、改代码、推分支、提 PR');
  url.searchParams.set('expires_in', '90');
  // 建仓库要 Administration 写权限（站点制作、上线向导「代码放到 GitHub」新建仓库时用）
  url.searchParams.set('administration', 'write');
  url.searchParams.set('contents', 'write');
  url.searchParams.set('pull_requests', 'write');
  url.searchParams.set('issues', 'write');
  return url.toString();
}

export function ghUpdateConnection(): Connection | null {
  return app.conns.find((connection) => connection.id === gh.ghUpdateId) ?? null;
}
function resetGhConnect(updateId = '') {
  const updating = app.conns.find((connection) => connection.id === updateId);
  gh.ghUpdateId = updating?.id ?? '';
  gh.ghOpen = true; gh.ghToken = ''; gh.ghErr = ''; gh.ghVerified = null;
  gh.ghName = updating?.name ?? '';
  gh.ghPermsOpen = !!updating;
}
export function openGhConnect() { connections.switcherOpen = false; resetGhConnect(); }
export function openGhUpdate(c: Connection) { connections.connCtx = null; connections.switcherOpen = false; resetGhConnect(c.id); }
export function closeGhConnect() {
  if (gh.ghConnecting || gh.ghVerifying) return;
  gh.ghOpen = false;
  gh.ghUpdateId = '';
}
export function resetGhVerification() { gh.ghVerified = null; gh.ghErr = ''; }

export async function verifyGh() {
  if (!gh.ghToken.trim() || gh.ghVerifying) return;
  gh.ghVerifying = true; gh.ghErr = ''; gh.ghVerified = null;
  try {
    const v = await invoke<GhVerify>('verify_github_token', { token: gh.ghToken.trim() });
    gh.ghVerified = v;
    const updating = ghUpdateConnection();
    if (updating && String(v.user_id) !== updating.account_id) {
      gh.ghErr = `Token 有效，但属于 ${v.login}，不是这条连接的账号 ${updating.login}。请用原账号新建 Token。`;
    } else if (!updating && !gh.ghName.trim()) {
      gh.ghName = `GitHub · ${v.login}`;
    }
    if (!gh.ghErr && !v.repos.length) gh.ghErr = 'Token 有效，但没有授权任何仓库。请在 Token 设置里选择要让 AI 处理的仓库。';
  } catch (e) { gh.ghErr = String(e); }
  finally { gh.ghVerifying = false; }
}

export async function confirmGh() {
  const v = gh.ghVerified;
  if (!v) { gh.ghErr = '请先验证这枚 Token'; return; }
  const updating = ghUpdateConnection();
  if (updating && String(v.user_id) !== updating.account_id) return;
  gh.ghConnecting = true; gh.ghErr = '';
  try {
    const conn = updating
      ? await invoke<Connection>('update_github_token', { connId: updating.id, token: gh.ghToken.trim() })
      : await invoke<Connection>('connect_github', { name: gh.ghName.trim(), token: gh.ghToken.trim() });
    gh.ghOpen = false; gh.ghUpdateId = ''; gh.ghToken = '';
    say(updating ? `已更新 GitHub 连接「${updating.remark?.trim() || conn.name}」` : `已连接 GitHub「${conn.name}」`);
    gh.reposConnId = ''; // Token 变了，可访问的仓库可能也变了
    await refreshConns();
    await selectConn(conn.id);
  } catch (e) { gh.ghErr = String(e); }
  finally { gh.ghConnecting = false; }
}

/** 选中 GitHub 连接时调用：列本地检出，并（按连接缓存）拉取可访问的仓库。 */
export async function loadGhRepos(connId: string, force = false) {
  try { const list = await invoke<string[]>('list_github_checkouts', { connId }); if (app.activeConnId === connId) gh.checkouts = list; }
  catch { if (app.activeConnId === connId) gh.checkouts = []; }
  if (!force && gh.reposConnId === connId && gh.repos.length) return;
  gh.reposLoading = true; gh.reposErr = '';
  try {
    const repos = await invoke<GhRepo[]>('list_github_repos', { connId });
    if (app.activeConnId !== connId) return;
    gh.repos = repos; gh.reposConnId = connId;
  } catch (e) {
    if (app.activeConnId === connId) { gh.repos = []; gh.reposConnId = ''; gh.reposErr = String(e); }
  } finally { gh.reposLoading = false; }
}
