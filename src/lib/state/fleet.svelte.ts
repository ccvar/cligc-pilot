import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { SiteRow, PendingItem, Site } from '$lib/types';
import { say } from './app.svelte.js';
import { sites } from './sites.svelte.js';

export type SiteFilter = 'all' | 'pinned' | 'attention' | 'failed' | 'down' | 'nodata';
export type SiteSort = 'name' | 'active' | 'clicks' | 'change' | 'pending';

/** 点击比前 28 天跌了 20% 以上（且原来至少 20 次）算「流量下滑」。 */
export function trafficDown(r: SiteRow): boolean {
  return r.has_data && r.summary.gsc_clicks_prev >= 20 && r.summary.gsc_clicks < r.summary.gsc_clicks_prev * 0.8;
}
export function clicksChange(r: SiteRow): number {
  const p = r.summary.gsc_clicks_prev;
  return p > 0 ? (r.summary.gsc_clicks - p) / p : 0;
}
/** 需要你关注：有待确认 / 失败的条目、上次上线失败、流量下滑、数据源出错。 */
export function needsAttention(r: SiteRow): boolean {
  return r.proposed > 0 || r.failed > 0 || r.last_deploy_ok === false || trafficDown(r) || r.summary.errors.length > 0;
}

class FleetState {
  rows = $state<SiteRow[]>([]);
  loaded = $state(false);
  pending = $state<PendingItem[]>([]);
  // 左栏
  railQuery = $state('');
  railFilter = $state<SiteFilter>('all');
  railTag = $state('');
  // 总览
  query = $state('');
  filter = $state<SiteFilter>('all');
  tag = $state('');
  sort = $state<SiteSort>('name');
  desc = $state(false);
  page = $state(0);
  tab = $state<'sites' | 'pending'>('sites');
  byId = $derived(new Map(this.rows.map((r) => [r.id, r])));
  tags = $derived([...new Set(this.rows.flatMap((r) => r.tags))].sort((a, b) => a.localeCompare(b, 'zh')));
  attentionCount = $derived(this.rows.filter(needsAttention).length);
  pendingCount = $derived(this.rows.reduce((n, r) => n + r.proposed, 0));
}

export const fleet = new FleetState();

export const PAGE_SIZE = 50;

function hay(r: SiteRow) {
  return `${r.name} ${r.domain} ${r.repo} ${r.tags.join(' ')}`.toLowerCase();
}
export function applyFilter(rows: SiteRow[], q: string, f: SiteFilter, tag: string): SiteRow[] {
  const query = q.trim().toLowerCase();
  return rows.filter((r) =>
    (!query || hay(r).includes(query))
    && (!tag || r.tags.includes(tag))
    && (f === 'all'
      || (f === 'pinned' && r.pinned)
      || (f === 'attention' && needsAttention(r))
      || (f === 'failed' && (r.last_deploy_ok === false || r.failed > 0))
      || (f === 'down' && trafficDown(r))
      || (f === 'nodata' && !r.has_data)));
}
export function sortRows(rows: SiteRow[], sort: SiteSort, desc: boolean): SiteRow[] {
  const key = (r: SiteRow): number | string =>
    sort === 'name' ? r.name.toLowerCase()
      : sort === 'active' ? -r.active_at
        : sort === 'clicks' ? -r.summary.gsc_clicks
          : sort === 'change' ? clicksChange(r)
            : -(r.proposed * 1000 + r.failed);
  const out = [...rows].sort((a, b) => {
    const ka = key(a), kb = key(b);
    const c = typeof ka === 'string' ? ka.localeCompare(kb as string, 'zh') : (ka as number) - (kb as number);
    return c || a.name.localeCompare(b.name, 'zh');
  });
  return desc ? out.reverse() : out;
}
/** 左栏顺序：置顶在前，其余按名称（稳定，不随活动跳来跳去）。 */
export function railOrder(rows: SiteRow[]): SiteRow[] {
  return [...rows].sort((a, b) => Number(b.pinned) - Number(a.pinned) || a.name.localeCompare(b.name, 'zh'));
}

let loading = false;
let again = false;
export async function loadFleet() {
  if (loading) { again = true; return; }
  loading = true;
  try {
    const rows = await invoke<SiteRow[]>('sites_overview');
    fleet.rows = rows;
    fleet.loaded = true;
  } catch (e) { say(String(e), 'err'); }
  finally {
    loading = false;
    if (again) { again = false; void loadFleet(); }
  }
}
export async function loadPending() {
  try { const v = await invoke<PendingItem[]>('pending_across_sites'); fleet.pending = v; }
  catch (e) { say(String(e), 'err'); }
}

export async function setPinned(id: string, pinned: boolean) {
  try {
    const s = await invoke<Site>('set_site_flags', { id, pinned });
    sites.list = sites.list.map((x) => (x.id === s.id ? s : x));
    fleet.rows = fleet.rows.map((r) => (r.id === id ? { ...r, pinned } : r));
  } catch (e) { say(String(e), 'err'); }
}
export async function setTags(id: string, tags: string[]) {
  try {
    const s = await invoke<Site>('set_site_flags', { id, tags });
    sites.list = sites.list.map((x) => (x.id === s.id ? s : x));
    if (sites.draft?.id === id) sites.draft.tags = s.tags;
    fleet.rows = fleet.rows.map((r) => (r.id === id ? { ...r, tags: s.tags } : r));
    return true;
  } catch (e) { say(String(e), 'err'); return false; }
}

let timer: ReturnType<typeof setTimeout> | null = null;
/** 作业 / 队列事件很密：合并成一次刷新。 */
function soon() {
  if (timer) return;
  timer = setTimeout(() => { timer = null; void loadFleet(); if (fleet.tab === 'pending') void loadPending(); }, 800);
}

export function initFleet() {
  $effect(() => {
    // 站点列表变了（导入、删除、改名）就重读总览
    void sites.list.length;
    void loadFleet();
  });
  $effect(() => {
    const un1 = listen('site-queue', soon);
    const un2 = listen('site-job', soon);
    return () => { void un1.then((f) => f()); void un2.then((f) => f()); };
  });
}

