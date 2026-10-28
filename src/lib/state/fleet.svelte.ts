import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { SiteRow, PendingItem, Site, OppItem, OppOverview } from '$lib/types';
import { say, confirmDialog } from './app.svelte.js';
import { sites, refreshSites } from './sites.svelte.js';
import { brainLabel, brainTitle, modelIcon } from './models.js';
import { launcher } from './launcher.svelte.js';

export type SiteFilter = 'all' | 'attention' | 'offline' | 'expiry' | 'failed' | 'links' | 'dataerr' | 'down' | 'proposed' | 'nodata';
export type SiteSort = 'name' | 'active' | 'clicks' | 'change' | 'pending' | 'users' | 'impr' | 'ctr' | 'indexed' | 'revenue';

/** 点击比前 28 天跌了 20% 以上（且原来至少 20 次）算「流量下滑」。 */
export function trafficDown(r: SiteRow): boolean {
  return r.has_data && r.summary.gsc_clicks_prev >= 20 && r.summary.gsc_clicks < r.summary.gsc_clicks_prev * 0.8;
}
/** 站点列表：Google + Bing 搜索点击率 */
export function searchCtr(r: SiteRow): number {
  const i = r.summary.search_impressions ?? 0;
  return i > 0 ? (r.summary.search_clicks ?? 0) / i : 0;
}
/** 和前 28 天比的变化：null＝前后都没有，'new'＝之前没有、现在有 */
export function change(cur: number, prev: number): number | 'new' | null {
  if (!prev) return cur ? 'new' : null;
  return (cur - prev) / prev;
}
export function clicksChange(r: SiteRow): number {
  const p = r.summary.gsc_clicks_prev;
  return p > 0 ? (r.summary.gsc_clicks - p) / p : 0;
}
/** 证书 14 天内、域名 60 天内到期，或证书出错（和 monitor.rs 的提醒天数一致） */
export function expiring(r: SiteRow): boolean {
  return !!r.cert_err || (r.cert_days != null && r.cert_days <= 14) || (r.domain_days != null && r.domain_days <= 60);
}
/** 需要你关注：有待确认 / 失败的条目、上次上线失败、流量下滑、数据源出错。 */
export function needsAttention(r: SiteRow): boolean {
  return !!r.site_down || expiring(r) || r.proposed > 0 || r.failed > 0 || r.last_deploy_ok === false || (r.links_broken ?? 0) > 0 || trafficDown(r) || r.summary.errors.length > 0;
}

class FleetState {
  rows = $state<SiteRow[]>([]);
  loaded = $state(false);
  pending = $state<PendingItem[]>([]);
  /** 「待处理」里的运营机会（自己运营的站点）；切到哪一块：confirm 等你确认 / opps 运营机会 */
  opps = $state<OppOverview | null>(null);
  /** 「待处理」里交给 AI 时这批临时用的模型（"<brain>::<model>"）；空＝按站点设置 */
  batchModel = $state('');
  // 总览
  query = $state('');
  filter = $state<SiteFilter>('all');
  tag = $state('');
  /** 按部署方式筛选：direct / workers / pages_git / vps / none；空＝全部 */
  mode = $state('');
  sort = $state<SiteSort>('name');
  desc = $state(false);
  page = $state(0);
  tab = $state<'sites' | 'pending'>('sites');
  byId = $derived(new Map(this.rows.map((r) => [r.id, r])));
  tags = $derived([...new Set(this.rows.flatMap((r) => r.tags))].sort((a, b) => a.localeCompare(b, 'zh')));
  attentionCount = $derived(this.rows.filter(needsAttention).length);
  pendingCount = $derived(this.rows.reduce((n, r) => n + r.proposed, 0));
  /** 没人自动处理的站点（自己运营的、托管停着的）里还没处理的运营机会；托管且在运行的由 AI 自动处理，不算 */
  oppCount = $derived(this.rows.filter((r) => !r.plan || r.autonomy === 'stopped').reduce((n, r) => n + r.open, 0));
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
      || (f === 'attention' && needsAttention(r))
      || (f === 'offline' && !!r.site_down)
      || (f === 'expiry' && expiring(r))
      || (f === 'failed' && (r.last_deploy_ok === false || r.failed > 0))
      || (f === 'links' && (r.links_broken ?? 0) > 0)
      || (f === 'dataerr' && r.summary.errors.length > 0)
      || (f === 'down' && trafficDown(r))
      || (f === 'proposed' && r.proposed > 0)
      || (f === 'nodata' && !r.has_data)));
}
/** 右上角提醒里的问题分组（按要紧程度排）；soft＝不算问题，只是提示。 */
export const ISSUE_GROUPS: { k: SiteFilter; l: string; soft?: boolean }[] = [
  { k: 'offline', l: '网站打不开' },
  { k: 'expiry', l: '证书或域名快到期' },
  { k: 'failed', l: '上线或运营任务失败' },
  { k: 'links', l: '推广链接失效' },
  { k: 'dataerr', l: '数据源出错' },
  { k: 'down', l: '流量明显下滑' },
  { k: 'proposed', l: '有改动等你确认' },
  { k: 'nodata', l: '数据源没接上', soft: true },
];
// 「要看看」里的提醒可以忽略：记下当时的情况（签名），情况变了（又失败一次、出了别的错）会重新提醒。只存在这台电脑上
const IGNORE_KEY = 'pilot.fleet.ignoredIssues';
function issueSig(r: SiteRow, k: SiteFilter): string {
  if (k === 'failed') return `${r.last_deploy_ok === false ? r.last_deploy_ts : 0}:${r.failed}`;
  if (k === 'links') return String(r.links_broken ?? 0);
  if (k === 'offline') return String(r.down_since ?? 0);
  // 按「哪几样要处理」记：天数每天在变，不能用天数，不然忽略了第二天又冒出来
  if (k === 'expiry') return `${r.cert_err ? 'e' : ''}${r.cert_days != null && r.cert_days <= 14 ? 'c' : ''}${r.domain_days != null && r.domain_days <= 60 ? 'd' : ''}`;
  if (k === 'dataerr') return r.summary.errors.join('|');
  if (k === 'down') return r.summary.data_end;
  if (k === 'proposed') return String(r.proposed);
  return 'x';
}
function loadIgnored(): Record<string, string> {
  try { return JSON.parse(localStorage.getItem(IGNORE_KEY) || '{}'); } catch { return {}; }
}
export const ignored = $state<{ map: Record<string, string> }>({ map: loadIgnored() });
export function ignoreIssues(rows: SiteRow[], k: SiteFilter) {
  const map = { ...ignored.map };
  for (const r of rows) map[`${k}:${r.id}`] = issueSig(r, k);
  ignored.map = map;
  try { localStorage.setItem(IGNORE_KEY, JSON.stringify(map)); } catch { /* 存不了就只在这次有效 */ }
}
export function issueRows(rows: SiteRow[], k: SiteFilter): SiteRow[] {
  return applyFilter(rows, '', k, '').filter((r) => ignored.map[`${k}:${r.id}`] !== issueSig(r, k));
}

export function sortRows(rows: SiteRow[], sort: SiteSort, desc: boolean): SiteRow[] {
  const key = (r: SiteRow): number | string =>
    sort === 'name' ? r.name.toLowerCase()
      : sort === 'active' ? -r.active_at
        : sort === 'clicks' ? -(r.summary.search_clicks ?? r.summary.gsc_clicks)
          : sort === 'change' ? clicksChange(r)
            : sort === 'users' ? -(r.summary.users ?? 0)
              : sort === 'impr' ? -(r.summary.search_impressions ?? 0)
                : sort === 'ctr' ? -searchCtr(r)
                  : sort === 'indexed' ? -(r.summary.google_pages ?? 0)
                    : sort === 'revenue' ? -(r.money?.month ?? 0)
                    : -(r.proposed * 1000 + r.failed);
  const out = [...rows].sort((a, b) => {
    const ka = key(a), kb = key(b);
    const c = typeof ka === 'string' ? ka.localeCompare(kb as string, 'zh') : (ka as number) - (kb as number);
    return c || a.name.localeCompare(b.name, 'zh');
  });
  return desc ? out.reverse() : out;
}

let loading = false;
let again = false;
// 每次启动只自动识别一次：没配置部署的站点去 Cloudflare 找对应的 Pages 项目，没接的数据源去 Google / Bing 账号里按域名找
let autodetected = false;
async function autodetectDeploys() {
  autodetected = true;
  try {
    const [deploys, data] = await Promise.all([
      fleet.rows.some((r) => r.deploy_mode === 'none') ? invoke<string[]>('autodetect_cf_deploys').catch(() => []) : Promise.resolve([] as string[]),
      invoke<string[]>('autodetect_data_bindings').catch(() => [] as string[]),
    ]);
    if (deploys.length) say(`在 Cloudflare 里认出了 ${deploys.length} 个站点的部署：${deploys.join('；')}`);
    if (data.length) say(`按域名接上了 ${data.length} 个站点的数据源：${data.join('；')}。数据马上开始拉取`);
    if (deploys.length || data.length) {
      await refreshSites();
      void loadFleet();
    }
  } catch { /* 识别不到不打扰 */ }
}
export async function loadFleet() {
  if (loading) { again = true; return; }
  loading = true;
  try {
    const rows = await invoke<SiteRow[]>('sites_overview');
    fleet.rows = rows;
    fleet.loaded = true;
    if (!autodetected) void autodetectDeploys();
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

export async function loadOpps() {
  try { fleet.opps = await invoke<OppOverview>('opportunities_across_sites'); }
  catch (e) { say(String(e), 'err'); }
}
const siteCount = (items: OppItem[]) => new Set(items.map((i) => i.site_id)).size;
/** 交给 AI 时用的模型：托管方案里的站点用方案选的，其它用站点「运营」设置 */
export function oppModel(i: Pick<OppItem, 'brain' | 'model'>): string {
  if (!i.brain) return '还没选模型';
  const m = i.model ? shortName(i.brain, i.model) : i.brain === 'codex' ? '本机默认模型' : '';
  return m ? `${brainLabel(i.brain)} · ${m}` : brainLabel(i.brain);
}
/** 按模型数站点：「Codex · 本机默认模型 11 个站点；Claude · Sonnet 1 个站点」 */
function modelSummary(items: OppItem[]): string {
  const by = new Map<string, Set<string>>();
  for (const i of items) by.set(oppModel(i), (by.get(oppModel(i)) ?? new Set()).add(i.site_id));
  return [...by.entries()].map(([m, s]) => `${m} ${s.size} 个站点`).join('；');
}
/** 这批临时选的模型的名字，如「Codex · GPT-5.5」 */
export function batchModelLabel(v = fleet.batchModel): string {
  if (!v) return '';
  const b = v.split('::')[0];
  return `${brainLabel(b)} · ${launcher.comboOpts.find((o) => o.value === v)?.label ?? (v.slice(b.length + 2) || '默认模型')}`;
}
/** 一个模型的短名字（按钮、下拉框上用）：Codex 本机默认 →「Codex」，sonnet →「Sonnet」，其它用模型列表里的名字 */
function shortName(brain: string, model: string): string {
  if (!model) return brain.startsWith('api:') ? brainLabel(brain).replace(/ · API$/, '') : brainTitle(brain);
  return launcher.comboOpts.find((o) => o.value === `${brain}::${model}`)?.label ?? model.replace(/^sonnet$/, 'Sonnet').replace(/^haiku$/, 'Haiku');
}
/** 交给谁：临时选了模型就是它；按站点设置时这些条目只用到一种模型就是那种，几种混着就是「AI」（没有图标） */
export function handTarget(items: Pick<OppItem, 'brain' | 'model'>[], pick = fleet.batchModel): { icon: string; name: string } {
  if (pick) {
    const [b, ...m] = pick.split('::');
    return { icon: modelIcon(b), name: shortName(b, m.join('::')) };
  }
  const kinds = [...new Set(items.filter((i) => i.brain).map((i) => `${i.brain}::${i.model}`))];
  if (kinds.length !== 1) return { icon: '', name: 'AI' };
  const [b, ...m] = kinds[0].split('::');
  return { icon: modelIcon(b), name: shortName(b, m.join('::')) };
}
/** 让 AI 改：按站点分组，每个站点一个作业，同时最多 3 个，其余排着 */
export async function workOpps(items: OppItem[], ask = true) {
  if (!items.length) return;
  const n = siteCount(items);
  const pick = fleet.batchModel;
  const none = pick ? [] : items.filter((i) => !i.brain);
  const models = pick ? `${batchModelLabel(pick)}（这批临时用，不改站点和托管方案的设置）。` : `${modelSummary(items)}。托管方案里的站点用方案选的模型，其它站点用站点「运营」设置里的；要临时换，在列表上方「交给 AI 时用」里选。`;
  const sitemap = items.some((i) => i.item.opp.kind === 'sitemap_error') ? '\nsitemap 报错先由程序检查：只是提交的地址不对或要重新提交的，直接处理好，不用 AI 改代码。' : '';
  const msg = `让 AI 处理这 ${items.length} 条机会（${n} 个站点）？\n每个站点一个作业、一条一条改，同时最多 3 个站点在跑，其余排着等。改好、检查通过后留在「等你确认」里，你确认了才上线。${sitemap}\n\n用的模型：${models}${none.length ? `\n有 ${siteCount(none)} 个站点还没选模型，这些会失败，先去设置里选好。` : ''}`;
  if (ask && items.length > 1 && !(await confirmDialog(msg, { title: '批量让 AI 改', kind: 'info', confirmText: `开始处理 ${items.length} 条` }))) return;
  const by = new Map<string, string[]>();
  for (const i of items) by.set(i.site_id, [...(by.get(i.site_id) ?? []), i.item.id]);
  try {
    const [b, ...m] = pick.split('::');
    await invoke('work_across_sites', { batches: [...by.entries()], model: pick ? [b, m.join('::')] : null });
    say(n > 3 ? `已交给 AI：${items.length} 条，先开始 3 个站点，其余排着` : `已交给 AI：${items.length} 条`);
    void loadOpps();
  } catch (e) { say(String(e), 'err'); }
}
/** 停止排队：还没开始的不做了（留在待处理里），已经在跑的不受影响 */
export async function stopAcross() {
  const n = fleet.opps?.queued.length ?? 0;
  if (!n || !(await confirmDialog(`还有 ${n} 条排着没开始，都不做了？\n它们会回到列表里，之后可以再交给 AI。正在跑的不受影响，要停就点那一条旁边的停止。`, { title: '停止排队', kind: 'warning', confirmText: '停止排队' }))) return;
  try {
    const k = await invoke<number>('stop_across_sites');
    say(`已停止排队，${k} 条回到列表`);
    void loadOpps();
  } catch (e) { say(String(e), 'err'); }
}
/** 驳回（只问一次）：之后不会再提这些机会，驳回的在站点「待处理」里能恢复 */
export async function dismissOpps(items: OppItem[]) {
  if (!items.length) return;
  const one = items.length === 1;
  if (!(await confirmDialog(one ? `驳回「${items[0].item.opp.title}」？之后不会再提这个机会。` : `驳回这 ${items.length} 条机会？之后不会再提这些机会。`, { title: one ? '驳回' : '批量驳回', kind: 'warning', confirmText: one ? '驳回' : `驳回 ${items.length} 条` }))) return;
  let failed = 0;
  for (const i of items) {
    try { await invoke('queue_dismiss', { id: i.site_id, itemId: i.item.id }); } catch { failed++; }
  }
  if (!one || failed) say(failed ? `驳回了 ${items.length - failed} 条，${failed} 条没成功` : `已驳回 ${items.length} 条`, failed ? 'err' : undefined);
  void loadOpps();
  void loadFleet();
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
  timer = setTimeout(() => { timer = null; void loadFleet(); if (fleet.tab === 'pending') { void loadPending(); void loadOpps(); } }, 800);
}

export function initFleet() {
  $effect(() => {
    // 站点列表变了（导入、删除、改名、改域名）就重读总览——只看长度会漏掉改名
    void sites.list.map((s) => s.id + ':' + s.updated_at).join();
    void loadFleet();
  });
  $effect(() => {
    const un1 = listen('site-queue', soon);
    const un2 = listen('site-job', soon);
    // 每周的推广链接检查跑完（后台），列表上的「推广链接失效」跟着更新
    const un3 = listen('links-checked', soon);
    const un4 = listen('monitor-checked', soon);
    // 跨站点批量排队变了（交给 AI、开始一个站点、停止排队）
    const un5 = listen('across-changed', soon);
    return () => { for (const u of [un1, un2, un3, un4, un5]) void u.then((f) => f()); };
  });
}

