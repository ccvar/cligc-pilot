import { invoke } from '@tauri-apps/api/core';
import { untrack } from 'svelte';
import { open } from '@tauri-apps/plugin-dialog';
import type { Connection } from '$lib/types';
import { app, say, refreshConns } from './app.svelte.js';
import { connections } from './connections.svelte.js';

export const DATA_KINDS = ['google', 'bing'];
export const isDataConn = (c: Connection | null | undefined) => !!c && DATA_KINDS.includes(c.kind);

export interface Ga4Property { property: string; name: string; account: string }
export interface GscSite { url: string; permission: string }
export interface GoogleResources { email: string; project: string; properties: Ga4Property[]; sites: GscSite[]; ga4_error: string; gsc_error: string }
export interface BingSite { url: string; verified: boolean }

type DataForm = {
  open: boolean;
  kind: 'google' | 'bing';
  /** google：service_account | oauth */
  mode: 'service_account' | 'oauth';
  updateId: string;
  name: string;
  secret: string;
  busy: boolean;
  err: string;
  google: GoogleResources | null;
  bing: BingSite[] | null;
  /** OAuth：同时申请管理权限（新站闭环用）。客户端本身是全局的，见 oauth.svelte.ts。 */
  manage: boolean;
};

function blank(): DataForm {
  return { open: false, kind: 'google', mode: 'service_account', updateId: '', name: '', secret: '', busy: false, err: '', google: null, bing: null, manage: false };
}

class DataSrcState {
  form = $state<DataForm>(blank());
  /** 按连接缓存的资源列表（站点设置里选媒体资源 / 站点用）。 */
  googleRes = $state<Record<string, GoogleResources>>({});
  googleErr = $state<Record<string, string>>({});
  bingSites = $state<Record<string, BingSite[]>>({});
  bingErr = $state<Record<string, string>>({});
}

export const datasrc = new DataSrcState();

export function openDataConnect(kind: 'google' | 'bing', mode: 'service_account' | 'oauth' = 'oauth', opts: { manage?: boolean; editClient?: boolean } = {}) {
  connections.switcherOpen = false;
  datasrc.form = { ...blank(), open: true, kind, mode, manage: !!opts.manage };
  // oauth.svelte.ts 反过来依赖本文件，这里按需加载避免循环引用
  if (kind === 'google') void import('./oauth.svelte.js').then(async (m) => { await m.loadOauthClient(); m.editOauthClient(!!opts.editClient); });
}
export function openDataUpdate(c: Connection) {
  connections.connCtx = null;
  connections.switcherOpen = false;
  datasrc.form = { ...blank(), open: true, kind: c.kind as 'google' | 'bing', mode: c.key_kind === 'oauth' ? 'oauth' : 'service_account', updateId: c.id, name: c.name };
  if (c.key_kind === 'oauth') void import('./oauth.svelte.js').then((m) => { m.editOauthClient(false); return m.loadOauthClient(); });
}
export function closeDataConnect() {
  if (datasrc.form.busy) return;
  void import('./oauth.svelte.js').then((m) => m.cancelOauth());
  datasrc.form = blank();
}

export async function pickJsonFile() {
  const picked = await open({ title: '选择服务账号 JSON 密钥文件', multiple: false, filters: [{ name: 'JSON', extensions: ['json'] }] });
  if (typeof picked !== 'string') return;
  try { datasrc.form.secret = await invoke<string>('read_key_file', { path: picked }); datasrc.form.google = null; datasrc.form.err = ''; }
  catch (e) { datasrc.form.err = `读取文件失败：${String(e)}`; }
}

export async function verifyData() {
  const f = datasrc.form;
  f.busy = true; f.err = ''; f.google = null; f.bing = null;
  try {
    if (f.kind === 'google') {
      f.google = await invoke<GoogleResources>('verify_google_sa', { json: f.secret });
      if (!f.name.trim() && !f.updateId) f.name = `Google · ${f.google.email.split('@')[0]}`;
    } else {
      f.bing = await invoke<BingSite[]>('verify_bing_key', { key: f.secret });
      if (!f.name.trim() && !f.updateId) f.name = 'Bing Webmaster';
    }
  } catch (e) { f.err = String(e); }
  finally { f.busy = false; }
}

export async function confirmData() {
  const f = datasrc.form;
  f.busy = true; f.err = '';
  try {
    let conn: Connection;
    if (f.kind === 'google') {
      conn = f.updateId
        ? await invoke<Connection>('update_google_sa', { connId: f.updateId, json: f.secret })
        : await invoke<Connection>('connect_google_sa', { name: f.name, json: f.secret });
    } else {
      conn = f.updateId
        ? await invoke<Connection>('update_bing_key', { connId: f.updateId, key: f.secret })
        : await invoke<Connection>('connect_bing', { name: f.name, key: f.secret });
    }
    f.busy = false;
    datasrc.form = blank();
    delete datasrc.googleRes[conn.id]; delete datasrc.bingSites[conn.id];
    await refreshConns();
    say(f.updateId ? `已更新「${conn.name}」` : `已连接「${conn.name}」`);
  } catch (e) { f.err = String(e); f.busy = false; }
}

export async function loadGoogleRes(connId: string, force = false) {
  // 由 $effect 调用：同步读取不能被追踪，否则写回后会再次触发、失败时无限重试。
  if (!connId || (!force && untrack(() => datasrc.googleRes[connId] || datasrc.googleErr[connId]))) return;
  try {
    const v = await invoke<GoogleResources>('list_google_resources', { connId }); datasrc.googleRes = { ...datasrc.googleRes, [connId]: v };
    datasrc.googleErr = { ...datasrc.googleErr, [connId]: '' };
  } catch (e) { datasrc.googleErr = { ...datasrc.googleErr, [connId]: String(e) }; }
}
export async function loadBingSites(connId: string, force = false) {
  if (!connId || (!force && untrack(() => datasrc.bingSites[connId] || datasrc.bingErr[connId]))) return;
  try {
    const v = await invoke<BingSite[]>('list_bing_sites', { connId }); datasrc.bingSites = { ...datasrc.bingSites, [connId]: v };
    datasrc.bingErr = { ...datasrc.bingErr, [connId]: '' };
  } catch (e) { datasrc.bingErr = { ...datasrc.bingErr, [connId]: String(e) }; }
}

export function googleConns() { return app.conns.filter((c) => c.kind === 'google'); }
export function bingConns() { return app.conns.filter((c) => c.kind === 'bing'); }
