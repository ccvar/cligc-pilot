import type { Conversation } from '$lib/types';
import { WORKSPACE_CONN_ID } from './env';
import { app } from './app.svelte.js';
import { workspaceFolderName } from './launcher.svelte.js';
import { detectSiteUrl } from './messages.svelte.js';

class SidebarState {
  grouped = $derived.by(() => {
    const bySite = new Map<string, { slug: string; name: string; recent: number; convs: Conversation[] }>();
    for (const c of app.convos) {
      // 侧栏位置必须稳定：所有会话都只显示在创建它的连接下。
      if (c.conn_id !== app.activeConnId) continue;
      const key = conversationGroupKey(c);
      let g = bySite.get(key);
      const groupName = key === '__site__' ? '对话' : key === '__site_ops__' ? '运营任务'
        : key === WORKSPACE_CONN_ID ? '自由对话'
        : c.task_type === 'workspace' ? workspaceFolderName(key)
          : c.site_name || c.site_slug || key;
      if (!g) { g = { slug: key, name: groupName, recent: 0, convs: [] }; bySite.set(key, g); }
      g.convs.push(c);
      if (c.updated_at > g.recent) g.recent = c.updated_at;
    }
    const groups = [...bySite.values()].sort((a, b) => b.recent - a.recent);
    return groups.map((s) => {
      const subs: { type: string; label: string; items: Conversation[] }[] = [];
      for (const t of TASK_ORDER) {
        const items = s.convs.filter((c) => c.task_type === t).sort((a, b) => b.updated_at - a.updated_at);
        if (items.length) subs.push({ type: t, label: taskLabel(t), items });
      }
      // 分组头的域名：CF 从对话里探测部署地址。
      return { slug: s.slug, name: s.name, count: s.convs.length, subs, url: app.isCfConn ? detectSiteUrl(s.convs) : '' };
    });
  });
  // CF 页脚：有域名（已部署）的项目算「站点」。
  cfSiteCount = $derived(this.grouped.filter((g) => g.url).length);
  // 站点分组折叠态（持久化）；默认全展开，打开某会话时自动展开其站点。
  collapsedSites = $state(loadCollapsedSites());
}

export const sidebar = new SidebarState();

function taskLabel(t: string): string {
  return t === 'sitebuild' ? '建站' : t === 'remote' ? '远程运维' : t === 'repo' ? '仓库' : t === 'site' ? '站点' : '自由对话';
}
function conversationTaskLabel(c: Conversation | null | undefined): string {
  return taskLabel(c?.task_type ?? '');
}
// 侧栏分组键：CF 按项目、GitHub 按仓库；SSH 一台机器一组；本地对话按接入的文件夹（未接文件夹的归「自由对话」）。
export function conversationGroupKey(c: Conversation): string {
  if (c.task_type === 'workspace') return c.workspace_dir?.trim() || WORKSPACE_CONN_ID;
  if (c.task_type === 'remote') return '__remote__';
  // 站点会话按来源分两组：你发起的对话 / 运营任务开出来的会话。
  if (c.task_type === 'site') return c.origin === 'ops' ? '__site_ops__' : '__site__';
  return c.site_slug || '(未命名项目)';
}

// 会话按「分组 → 任务类型」两级分组：分组按最近活动倒序；任务类型固定顺序，只留有会话的。
const TASK_ORDER = ['site', 'sitebuild', 'repo', 'remote', 'workspace'];
export function faviconOf(u: string): string { try { return new URL(u).origin + '/favicon.ico'; } catch { return ''; } }
function loadCollapsedSites(): Set<string> { try { return new Set<string>(JSON.parse(localStorage.getItem('cligc.pilot.collapsedSites') || '[]')); } catch { return new Set(); } }
function persistCollapsedSites() { try { localStorage.setItem('cligc.pilot.collapsedSites', JSON.stringify([...sidebar.collapsedSites])); } catch { /* */ } }
export function toggleSite(slug: string) { const n = new Set(sidebar.collapsedSites); if (n.has(slug)) n.delete(slug); else n.add(slug); sidebar.collapsedSites = n; persistCollapsedSites(); }
export function expandSite(slug: string) { if (sidebar.collapsedSites.has(slug)) { const n = new Set(sidebar.collapsedSites); n.delete(slug); sidebar.collapsedSites = n; persistCollapsedSites(); } }
// 侧栏会话时间标签：今天 / 昨天 / N 天前 / 更早日期。
export function relTime(secs: number): string {
  const t0 = new Date(); t0.setHours(0, 0, 0, 0);
  const day = t0.getTime(); const ms = secs * 1000;
  if (ms >= day) return '今天';
  if (ms >= day - 864e5) return '昨天';
  const n = Math.floor((day - ms) / 864e5) + 1;
  if (n <= 7) return `${n} 天前`;
  const d = new Date(ms); return `${d.getMonth() + 1}/${d.getDate()}`;
}
