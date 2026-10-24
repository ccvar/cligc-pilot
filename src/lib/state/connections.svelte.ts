import type { Connection } from '$lib/types';
import { tick } from 'svelte';
import { hideTip } from '$lib/tip';
import { invoke } from '@tauri-apps/api/core';
import { pilotWindowLabel, CONN_WIN_PREFIX, LAST_CONN_KEY, isMainWindow, WORKSPACE_CONN_ID, SITES_CONN_ID } from './env';
import { app, say, confirmDialog, refreshConns, refreshConvos } from './app.svelte.js';
import { switchConversationWorkspace } from './panes.svelte.js';
import { launcher } from './launcher.svelte.js';
import { disposeTerm } from './workbench.svelte.js';
import { loadGhRepos } from './github.svelte.js';
import { isSiteConnId, siteIdOf, loadSiteStatus, sites, touchRecent } from './sites.svelte.js';

class ConnectionsState {
  switcherOpen = $state(false);
  // 连接切换器里每条连接的右键菜单（新窗口打开 / 编辑 / 删除）。
  connCtx = $state<null | { x: number; y: number; conn: Connection; ready: boolean }>(null);
  connCtxMenuEl = $state<HTMLDivElement | null>(null);
  // 删除确认期间保留连接切换器与右键菜单，让用户始终看得见操作来源；
  // 取消后仍回到原菜单，确认后才由 removeConn 主动收起右键菜单。
  connCtxConfirmPinned = $state(false);
  connRemarkEdit = $state<null | { id: string; name: string; original: string; value: string; busy: boolean; error: string }>(null);
  footerEl = $state<HTMLElement | null>(null);

  selSeq = 0;
  osProbing = $state<Record<string, boolean>>({});
}

export const connections = new ConnectionsState();

export function connectionDisplayName(connection: Connection): string {
  return connection.name;
}
async function settleConnCtxMenu(connId: string, anchor?: { top: number; bottom: number; right: number }) {
  await tick();
  if (!connections.connCtx || connections.connCtx.conn.id !== connId || !connections.connCtxMenuEl) return;
  const menuRect = connections.connCtxMenuEl.getBoundingClientRect();
  const pad = 8;
  const x = anchor
    ? Math.max(pad, Math.min(anchor.right - menuRect.width, window.innerWidth - menuRect.width - pad))
    : Math.max(pad, Math.min(connections.connCtx.x, window.innerWidth - menuRect.width - pad));
  const y = anchor
    ? anchor.bottom + 5 + menuRect.height <= window.innerHeight - pad
      ? anchor.bottom + 5
      : Math.max(pad, anchor.top - menuRect.height - 5)
    : Math.max(pad, Math.min(connections.connCtx.y, window.innerHeight - menuRect.height - pad));
  connections.connCtx = { ...connections.connCtx, x, y, ready: true };
}
export async function openConnCtx(e: MouseEvent, c: Connection) {
  e.preventDefault();
  e.stopPropagation(); // 别让全局那个输入框右键菜单也跳出来
  hideTip();
  connections.connCtx = { x: e.clientX, y: e.clientY, conn: c, ready: false };
  await settleConnCtxMenu(c.id);
}
export async function toggleConnCtx(e: MouseEvent, c: Connection) {
  e.preventDefault();
  e.stopPropagation();
  hideTip();
  if (connections.connCtx?.conn.id === c.id) {
    connections.connCtx = null;
    return;
  }
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
  connections.connCtx = {
    x: rect.right - 180,
    y: rect.bottom + 5,
    conn: c,
    ready: false,
  };
  await settleConnCtxMenu(c.id, { top: rect.top, bottom: rect.bottom, right: rect.right });
}
export function openConnRemark(c: Connection) {
  connections.connCtx = null;
  connections.switcherOpen = false;
  const value = c.remark?.trim() ?? '';
  connections.connRemarkEdit = { id: c.id, name: c.name, original: value, value, busy: false, error: '' };
}
export async function saveConnRemark() {
  if (!connections.connRemarkEdit || connections.connRemarkEdit.busy) return;
  connections.connRemarkEdit.busy = true;
  connections.connRemarkEdit.error = '';
  const value = connections.connRemarkEdit.value.trim();
  try {
    const updated = await invoke<Connection>('set_connection_remark', { connId: connections.connRemarkEdit.id, remark: value });
    app.conns = app.conns.map((c) => (c.id === updated.id ? updated : c));
    connections.connRemarkEdit = null;
    say(value ? '连接备注已保存' : '连接备注已清除');
  } catch (e) {
    if (connections.connRemarkEdit) {
      connections.connRemarkEdit.error = String(e);
      connections.connRemarkEdit.busy = false;
    }
  }
}
export async function openConnWindow(id: string) {
  connections.connCtx = null;
  // 「远程连接」整组：新窗口打开上次用过的那台，窗口里左栏照样列出全部机器
  if (id === '__remotes__') id = lastRemoteId();
  if (id === '__cloudflares__') id = lastCfId();
  if (!id) return;
  try { await invoke('open_conn_window', { connId: id }); }
  catch (e) { say(String(e), 'err'); }
}
export function bootConnId(): string {
  if (pilotWindowLabel.startsWith(CONN_WIN_PREFIX)) return pilotWindowLabel.slice(CONN_WIN_PREFIX.length);
  try { return localStorage.getItem(LAST_CONN_KEY) || ''; } catch { return ''; }
}
const LAST_REMOTE_KEY = 'cligc.pilot.lastRemote';
/** 所有远程连接（SSH），切换器里合并成一行「远程连接」。 */
export const remoteConns = () => app.conns.filter((c) => c.kind === 'ssh');
/** 进入远程连接：打开上次用过的那台，没有就第一台。 */
function lastRemoteId(): string {
  const list = remoteConns();
  let last = '';
  try { last = localStorage.getItem(LAST_REMOTE_KEY) || ''; } catch { /* */ }
  return list.find((c) => c.id === last)?.id ?? list[0]?.id ?? '';
}
const LAST_CF_KEY = 'cligc.pilot.lastCloudflare';
/** 所有 Cloudflare 账号，切换器里合并成一行「Cloudflare」。 */
export const cfConns = () => app.conns.filter((c) => c.kind === 'cloudflare');
function lastCfId(): string {
  const list = cfConns();
  let last = '';
  try { last = localStorage.getItem(LAST_CF_KEY) || ''; } catch { /* */ }
  return list.find((c) => c.id === last)?.id ?? list[0]?.id ?? '';
}
/** 进入 Cloudflare：打开上次用过的那个账号，没有就第一个。 */
export function openCloudflare() {
  connections.switcherOpen = false;
  const id = lastCfId();
  if (id) void selectConn(id);
}
export function openRemote() {
  connections.switcherOpen = false;
  const id = lastRemoteId();
  if (id) void selectConn(id);
}

export async function selectConn(id: string) {
  const switching = id !== app.activeConnId;
  const conn = app.conns.find((c) => c.id === id);
  switchConversationWorkspace(id);
  const seq = ++connections.selSeq; app.activeConnId = id;
  if (isMainWindow) { try { localStorage.setItem(LAST_CONN_KEY, id); } catch { /* */ } }
  if (conn?.kind === 'ssh') { try { localStorage.setItem(LAST_REMOTE_KEY, id); } catch { /* */ } }
  if (conn?.kind === 'cloudflare') { try { localStorage.setItem(LAST_CF_KEY, id); } catch { /* */ } }
  if (switching) {
    // 换连接＝换工作区：关掉上个连接的对话视图，别串场。模板库 / 提示词库只在 CF 连接下有。
    app.activeConvId = ''; app.activeConv = null;
    if (app.view === 'thread' || app.view === 'remote' || app.view === 'site' || app.view === 'sites' || app.view === 'managed'
      || ((app.view === 'templates' || app.view === 'prompts') && conn?.kind !== 'cloudflare')) app.view = 'launcher';
  }
  if (id === SITES_CONN_ID) {
    // 全部站点：主区是总览，左栏是站点列表。
    app.view = 'sites';
    return;
  }
  if (isSiteConnId(id)) {
    // 站点：进入站点页（看板 / 待处理 / 部署 / 设置），侧栏是该站的会话。
    // 站点会话会改代码、可能推送：和 GitHub / CF 一样默认「自动」而非「全自动」。
    if (switching) {
      app.view = 'site';
      launcher.lSite = '';
      if (launcher.lPerm === 'full') launcher.lPerm = 'auto';
      if (sites.tab === 'settings') sites.draft = null;
    }
    touchRecent(siteIdOf(id));
    void loadSiteStatus(siteIdOf(id));
    return;
  }
  if (conn?.kind === 'ssh') {
    // SSH 连接没有项目可发现，也没有独立的启动页/对话页——**一切都在远程工作台里**
    //（终端为主，底部对话、右侧文件按需开）。
    void probeOs(id); // 系统版本：没探过才连，探到就存着，之后一直是本地读
    app.view = 'remote';
    if (switching) {
      // 不拆终端：每台机器的终端常驻，切回来就是原样（分屏时还要同时显示好几台）
      // 远程连接绝不默认「全自动」：那是别人的真机，默认得让每条命令都过一次你的眼。
      if (launcher.lPerm === 'full') launcher.lPerm = 'auto';
    }
    return;
  }
  if (conn?.kind === 'cloudflare') {
    // 只在真正换连接时清输入 + 拨默认档；同连接重选（如刷新）不动用户已填的项目名。
    if (switching) {
      app.cfProjects = []; launcher.lSite = ''; launcher.lStyle = '';
      if (launcher.lPerm === 'full') launcher.lPerm = 'auto'; // CF 默认「自动」——部署/改 DNS 弹确认，防手滑
    }
    try { const ps = await invoke<string[]>('list_cf_projects', { connId: id }); if (seq === connections.selSeq) app.cfProjects = ps; }
    catch (e) { if (seq === connections.selSeq) say(String(e), 'err'); }
    return;
  }
  if (conn?.kind === 'github') {
    // GitHub：选仓库再开会话。Codex / Claude 推送、提 PR 都走权限确认，默认「自动」而非「全自动」。
    if (switching) {
      launcher.lSite = '';
      if (launcher.lPerm === 'full') launcher.lPerm = 'auto';
    }
    void loadGhRepos(id);
    return;
  }
  // 本地对话：没有项目可选，进启动页即可。
  if (switching) { launcher.lSite = ''; app.view = 'launcher'; }
}
export function openLocalWorkspace() {
  connections.switcherOpen = false;
  void selectConn(WORKSPACE_CONN_ID);
}

// ---------- 远程连接：系统信息 ----------
// os-release 的 ID 归一化到我们画得出图标的那几家；认不出的返回 '' 走通用形。
export function distroOf(c: Connection | undefined | null): string {
  const id = (c?.ssh_os_id || '').toLowerCase();
  if (['ubuntu', 'debian', 'alpine', 'fedora', 'arch', 'centos', 'rocky', 'almalinux', 'rhel'].includes(id)) return id;
  if (id === 'raspbian' || id === 'linuxmint' || id === 'pop' || id === 'kali') return 'debian'; // 都是 debian 系
  if (id === 'arch' || id === 'manjaro' || id === 'endeavouros') return 'arch';
  if (id.startsWith('opensuse') || id === 'sles') return ''; // 没画 suse，走通用形
  return '';
}
// 页脚/下拉的副标题：探到了就显示系统，没探到就显示 user@host（总得有东西）。
/** 远程连接的搜索索引：名称 / IP / user@host / 端口 / 系统都能命中。
 *  机器一多就记不住「哪台是哪个 IP」——所以 IP 必须能搜，这正是用户要它的原因。 */
export const connHay = (c: Connection) =>
  `${c.name} ${c.remark ?? ''} ${c.login ?? ''} ${c.ssh_user ?? ''}@${c.ssh_host ?? ''} ${c.ssh_host ?? ''}:${c.ssh_port ?? ''} ${c.ssh_os ?? ''} ${c.ssh_os_id ?? ''}`.toLowerCase();
export function sshSub(c: Connection | undefined | null): string {
  if (!c) return '';
  return c.ssh_os || `${c.ssh_user}@${c.ssh_host}${c.ssh_port && c.ssh_port !== 22 ? ':' + c.ssh_port : ''}`;
}
/** 探一次远端系统版本并存进连接。已探过就不再连（除非 force）。 */
export async function probeOs(id: string, force = false) {
  const c = app.conns.find((x) => x.id === id);
  if (!c || c.kind !== 'ssh' || connections.osProbing[id]) return;
  if (c.ssh_os && !force) return;
  connections.osProbing = { ...connections.osProbing, [id]: true };
  try {
    const u = await invoke<Connection>('ssh_os_probe', { connId: id });
    app.conns = app.conns.map((x) => (x.id === u.id ? u : x));
  } catch (e) {
    // 机器关着/网络不通都很正常：静默（真要用连接时自然会报错），只在手动点刷新时说话。
    if (force) say(String(e), 'err');
  } finally {
    connections.osProbing = { ...connections.osProbing, [id]: false };
  }
}

export async function removeConn(id: string, preserveMenu = false) {
  // 该连接下有对话正在跑一轮时不删：否则删掉会话行会让在途回合回写失败（「会话丢失」），子进程还在用已删的密钥/目录。
  const runningUnderConn = Object.values(app.running).includes(id);
  if (runningUnderConn || app.convos.some((c) => c.conn_id === id && c.status === 'running')) {
    say('该连接下有对话正在运行，请先点停止结束这一轮，再删除连接。', 'err');
    return;
  }
  if (preserveMenu) connections.connCtxConfirmPinned = true;
  const yes = await confirmDialog('删除这个连接？连接工作区目录、钥匙串里的密钥，以及该连接下的所有对话都会一并删除。', { title: '删除连接', kind: 'danger' });
  if (preserveMenu) connections.connCtxConfirmPinned = false;
  if (!yes) return;
  if (preserveMenu) connections.connCtx = null;
  try {
    await invoke('remove_connection', { id });
    disposeTerm(id);
    if (app.activeConv?.conn_id === id) { app.activeConv = null; app.activeConvId = ''; app.view = 'launcher'; }
    await refreshConns();
    await refreshConvos();
  } catch (e) { say(String(e), 'err'); }
}

export function initConnections() {
  $effect(() => {
    if (!connections.connCtx) return;
    const close = () => { if (!connections.connCtxConfirmPinned) connections.connCtx = null; };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape' && !connections.connCtxConfirmPinned) connections.connCtx = null; };
    const onScroll = (e: Event) => {
      const target = e.target;
      if (target instanceof Node && connections.connCtxMenuEl?.contains(target)) return;
      close();
    };
    window.addEventListener('click', close);
    window.addEventListener('contextmenu', close);
    window.addEventListener('keydown', onKey);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('contextmenu', close);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', close);
    };
  });
  $effect(() => {
    if (!connections.switcherOpen) return;
    const onDoc = (e: MouseEvent) => {
      const target = e.target as Element | null;
      if (connections.connCtxConfirmPinned || target?.closest('[data-keep-connection-menu]')) return;
      if (connections.footerEl && !connections.footerEl.contains(e.target as Node)) connections.switcherOpen = false;
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape' && !connections.connCtxConfirmPinned) connections.switcherOpen = false; };
    // 捕获阶段：点空白（含拖拽区）也能关闭切换器。
    document.addEventListener('mousedown', onDoc, true);
    document.addEventListener('keydown', onKey);
    return () => { document.removeEventListener('mousedown', onDoc, true); document.removeEventListener('keydown', onKey); };
  });
}
