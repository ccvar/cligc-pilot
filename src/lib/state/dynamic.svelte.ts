// 站点动态功能（表单、评论、R2 大文件）：跑在 Cloudflare Pages Functions 上，D1 存数据，Pilot 建资源、看提交、审评论。
// 规则和命令在 Rust（site_dynamic.rs）；迁移站点的动态功能在迁移流程里配，这里也给其它 Pages 站点开关用。
import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Site } from '$lib/types';
import { say, confirmDialog } from './app.svelte.js';
import { startSiteChat } from './siteCreate.svelte.js';

export interface Dynamic {
  features: string[]; large: string[]; uploaded: string[];
  d1_id: string; d1_name: string; r2_bucket: string; turnstile_sitekey: string; turnstile_domains: string[];
  setup_at: number; notes: string[];
}
export interface Submission { id: number; form: string; page: string; fields: Record<string, string> | null; created_at: number; read: boolean }
export interface Comment { id: number; path: string; parent_id: number | null; name: string; email: string; url: string; content: string; status: 'pending' | 'approved' | 'spam'; created_at: number }
export interface Inbox { submissions: Submission[]; comments: Comment[]; unread: number; pending: number }

export const FEATURE_LABEL: Record<string, string> = { form: '表单', comments: '评论', files: '大文件（R2）' };

export const dyn = $state({
  conf: {} as Record<string, Dynamic | null>,
  inbox: {} as Record<string, Inbox | undefined>,
  inboxErr: {} as Record<string, string>,
  loading: {} as Record<string, boolean>,
  busy: {} as Record<string, string>,
});

export async function loadDynamic(id: string) {
  try { dyn.conf[id] = await invoke<Dynamic | null>('site_dynamic_state', { id }); } catch { dyn.conf[id] = null; }
}

export async function loadInbox(id: string) {
  if (untrack(() => dyn.loading[id])) return;
  untrack(() => { dyn.loading[id] = true; });
  try { dyn.inbox[id] = await invoke<Inbox>('site_inbox', { id }); dyn.inboxErr[id] = ''; }
  catch (e) { dyn.inboxErr[id] = String(e); }
  finally { dyn.loading[id] = false; }
}

/** 收件箱操作。删除要二次确认。 */
export async function inboxAct(id: string, kind: 'submission' | 'comment', action: 'read' | 'delete' | 'approve' | 'spam', ids: number[]) {
  if (!ids.length) return;
  if (action === 'delete' && !await confirmDialog(`删除这 ${ids.length} 条${kind === 'comment' ? '评论' : '提交'}？删除后找不回来。`, { title: '删除', kind: 'danger', confirmText: '删除' })) return;
  try {
    await invoke('site_inbox_act', { id, kind, action, ids });
    await loadInbox(id);
  } catch (e) { say(String(e), 'err'); }
}

/** 开关功能并建资源：Cloudflare 上建数据库、人机验证、绑定；自己的服务器上重新配置一次（装表单评论服务）。 */
export async function setupDynamic(id: string, features?: string[], vps = false) {
  dyn.busy[id] = 'setup';
  try {
    if (features) dyn.conf[id] = await invoke<Dynamic>('site_dynamic_features', { id, features });
    await invoke(vps ? 'site_vps_setup' : 'site_dynamic_setup', { id });
  } catch (e) { say(String(e), 'err'); }
  finally { dyn.busy[id] = ''; }
}

/** 让 AI 把表单 / 评论接到页面上。 */
export async function wireWithAi(site: Site) {
  try {
    const text = await invoke<string>('site_dynamic_message', { id: site.id });
    await startSiteChat(site.id, text, '要求写好了：选一个可用的模型后发送');
  } catch (e) { say(String(e), 'err'); }
}

export function initDynamic() {
  $effect(() => {
    const un = listen<{ site_id: string; status: string; kind: string; error: string }>('site-job', (ev) => {
      const j = ev.payload;
      if (j.status === 'running' || (j.kind !== 'dynamic:setup' && j.kind !== 'vps:setup')) return;
      void loadDynamic(j.site_id);
      say(j.status === 'ok' ? '动态功能配置好了：上线一次就生效' : `配置动态功能没完成：${j.error.split('\n')[0]}`, j.status === 'ok' ? 'ok' : 'err');
    });
    return () => { void un.then((f) => f()); };
  });
}

export const isPagesDirect = (s: Site) => s.deploy.mode === 'direct' && !!s.deploy.project;
/** Pilot 托管的 VPS（发布命令留空） */
export const isVpsManaged = (s: Site) => s.deploy.mode === 'vps' && !s.deploy.command?.trim() && !!s.deploy.ssh_conn_id;
/** 收件箱可用：配置过、开了表单或评论 */
export const inboxReady = (d: Dynamic | null | undefined) => !!d?.setup_at && d.features.some((f) => f === 'form' || f === 'comments');
