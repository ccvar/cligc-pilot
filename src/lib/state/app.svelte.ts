import type { Connection, BrainsInfo, Brain, Conversation } from '$lib/types';
import type { ApiProfile } from '$lib/apiProfiles';
import { invoke } from '@tauri-apps/api/core';
import { WORKSPACE_CONN_ID, LOCAL_CONN } from './env';
import { brainUsable } from './models';
import { selectConn, bootConnId } from './connections.svelte.js';
import { detectSiteUrl } from './messages.svelte.js';
import type { LiveTurn } from './messages.svelte.js';
import { sites, siteConnection, isSiteConnId, siteIdOf } from './sites.svelte.js';

type ConfirmKind = 'warning' | 'danger' | 'info';

class AppState {
  // ---------- setup ----------
  conns = $state<Connection[]>([]);
  activeConnId = $state('');
  brains = $state<BrainsInfo | null>(null);
  apiProfiles = $state<ApiProfile[]>([]);
  apiProfilesLoaded = $state(false);
  flash = $state('');
  flashKind = $state<'ok' | 'err'>('ok');
  confirmState = $state<null | { message: string; title: string; kind: ConfirmKind; confirmText: string; cancelText: string }>(null);
  confirmResolve: ((value: boolean) => void) | null = null;
  activeConn = $derived.by(() => {
    if (this.activeConnId === WORKSPACE_CONN_ID) return LOCAL_CONN;
    if (isSiteConnId(this.activeConnId)) {
      const s = sites.list.find((x) => x.id === siteIdOf(this.activeConnId));
      return s ? siteConnection(s) : null;
    }
    return this.conns.find((c) => c.id === this.activeConnId) ?? null;
  });
  /** 全部厂商（顺序即下拉展示顺序）。新增厂商在此登记，brainUsable/brainLabel/模型档随之生效。 */
  ALL_BRAINS = $derived<Brain[]>(['claude', 'codex', 'grok', ...this.apiProfiles.map((p): Brain => `api:${p.id}`)]);

  // ---------- conversations ----------
  convos = $state<Conversation[]>([]);
  activeConvId = $state('');
  activeConv = $state<Conversation | null>(null);
  view = $state<'launcher' | 'thread' | 'site' | 'templates' | 'prompts' | 'remote'>('launcher');
  // Cloudflare 建站：连接是 CF 时，lSite 复用为「项目名」，站点选择器换成项目输入。
  isCfConn = $derived(this.activeConn?.kind === 'cloudflare');
  cfProjects = $state<string[]>([]);
  // 自由对话的 conn_id 只表示侧栏创建位置，绝不能把该连接当成运行上下文。
  activeConvIsCf = $derived(!!this.activeConv && this.activeConv.task_type !== 'workspace' && this.conns.find((c) => c.id === this.activeConv!.conn_id)?.kind === 'cloudflare');
  activeConvConn = $derived(this.activeConv && this.activeConv.task_type !== 'workspace' ? (this.conns.find((c) => c.id === this.activeConv!.conn_id) ?? null) : null);
  activeConvIsSsh = $derived(this.activeConvConn?.kind === 'ssh');
  activeConvIsWorkspace = $derived(this.activeConv?.task_type === 'workspace');
  // 并发对话：running = convId → connId（在跑的对话；带 connId 便于删连接时可靠判定）；lives = 每个对话的流式缓冲。
  running = $state<Record<string, string>>({});
  lives = $state<Record<string, LiveTurn>>({});
  // 当前会话的外链：CF 从对话里探测部署地址；GitHub 指向仓库页面；其它会话没有。
  activeSiteUrl = $derived.by(() => {
    const c = this.activeConv;
    if (!c) return '';
    if (c.task_type === 'site') { const s = sites.list.find((x) => x.id === c.site_slug); return s?.domain ? `https://${s.domain}` : ''; }
    if (this.activeConvIsGh) return c.site_slug ? `https://github.com/${c.site_slug}` : '';
    if (!this.activeConvIsCf) return '';
    return detectSiteUrl([c]);
  });
  anyBrainUsable = $derived(this.ALL_BRAINS.some((b) => brainUsable(b)));

  isSshConn = $derived(this.activeConn?.kind === 'ssh');
  isGhConn = $derived(this.activeConn?.kind === 'github');
  activeConvIsGh = $derived(this.activeConvConn?.kind === 'github');
  isWorkspaceChat = $derived(this.activeConn?.kind === 'workspace');
  /** 当前选中的是站点（合成连接 site:<id>）。 */
  isSiteConn = $derived(this.activeConn?.kind === 'site');
  activeConvIsSite = $derived(this.activeConv?.task_type === 'site');
}

export const app = new AppState();

async function refreshApiProfiles() {
  try { app.apiProfiles = await invoke<ApiProfile[]>('list_api_profiles'); }
  catch (e) { say(`读取 API 服务失败：${String(e)}`, 'err'); }
  finally { app.apiProfilesLoaded = true; }
}

export function confirmDialog(message: string, options: { title?: string; kind?: ConfirmKind; confirmText?: string; cancelText?: string } = {}) {
  if (app.confirmResolve) app.confirmResolve(false);
  return new Promise<boolean>((resolve) => {
    app.confirmResolve = resolve;
    app.confirmState = {
      message,
      title: options.title || '请确认操作',
      kind: options.kind || 'warning',
      confirmText: options.confirmText || '确认',
      cancelText: options.cancelText || '取消',
    };
  });
}

export function finishConfirm(value: boolean) {
  const resolve = app.confirmResolve;
  app.confirmResolve = null;
  app.confirmState = null;
  resolve?.(value);
}
export function say(m: string, k: 'ok' | 'err' = 'ok', ms = 0) { app.flash = m; app.flashKind = k; setTimeout(() => (app.flash = ''), ms || (k === 'err' ? 8000 : 4000)); }
export async function refreshConns() {
  try {
    app.conns = await invoke<Connection[]>('list_connections');
    if (app.activeConnId && app.activeConnId !== WORKSPACE_CONN_ID && !isSiteConnId(app.activeConnId) && !app.conns.some((c) => c.id === app.activeConnId)) {
      // 当前连接被删掉了：退回本地对话，不留一个指向空连接的界面。
      void selectConn(WORKSPACE_CONN_ID);
    } else if (!app.activeConnId) {
      const want = bootConnId();
      void selectConn(want && (app.conns.some((c) => c.id === want) || isSiteConnId(want)) ? want : WORKSPACE_CONN_ID);
    }
  } catch (e) {
    say(String(e), 'err');
  }
}
export async function refreshBrains() { try { app.brains = await invoke('detect_brains'); } catch (e) { say(String(e), 'err'); } }
export async function refreshConvos() { try { app.convos = await invoke('list_conversations'); } catch (e) { say(String(e), 'err'); } }

export function initApp() {
  $effect(() => {
    void refreshConns();
    void refreshBrains();
    void refreshApiProfiles();
    void refreshConvos();
  });
  $effect(() => {
    const need = !!app.brains && ((app.brains.claude.found && app.brains.claude.logged_in === false) || (app.brains.codex.found && app.brains.codex.logged_in === false) || (app.brains.grok.found && app.brains.grok.logged_in === false));
    if (!need) return;
    const t = setInterval(() => { if (!document.hidden) refreshBrains(); }, 6000);
    return () => clearInterval(t);
  });
}
