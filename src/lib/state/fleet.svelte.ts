import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { SiteRow, PendingItem, Site, OppItem, OppOverview, DataSync } from '$lib/types';
import { say, confirmDialog } from './app.svelte.js';
import { sites, refreshSites } from './sites.svelte.js';
import { ops } from './ops.svelte.js';
import { brainLabel, brainTitle, modelIcon } from './models.js';
import { launcher } from './launcher.svelte.js';
import { effortLabel } from '$lib/effortStops';

export type SiteFilter = 'all' | 'attention' | 'unsafe' | 'offline' | 'expiry' | 'failed' | 'links' | 'dataerr' | 'down' | 'proposed' | 'nodata';
export type SiteSort = 'name' | 'active' | 'clicks' | 'change' | 'pending' | 'users' | 'impr' | 'ctr' | 'indexed' | 'revenue';

/** 点击比前 28 天跌了 20% 以上（且原来至少 20 次）算「流量下滑」。 */
export function trafficDown(r: SiteRow): boolean {
  return r.has_data && r.summary.gsc_clicks_prev >= 20 && r.summary.gsc_clicks < r.summary.gsc_clicks_prev * 0.8;
}
/** 站点列表按选的统计时间段（近 7 / 14 / 28 / 90 天）取数：用户、搜索点击（Google + Bing）、展示、和前一段比、每天会话 */
export function win(r: SiteRow) {
  const s = r.summary;
  const w = s.wins?.[fleet.days];
  // clicks_base：算涨跌用的本段点击（只算前一段也有数据的来源）
  if (w) return { users: w.users, users_prev: w.users_prev, clicks: w.clicks, clicks_prev: w.clicks_prev, clicks_base: w.clicks_base ?? w.clicks, impr: w.impressions, trend: w.trend };
  return { users: s.users ?? 0, users_prev: s.users_prev ?? 0, clicks: s.search_clicks ?? s.gsc_clicks, clicks_prev: s.search_clicks_prev ?? s.gsc_clicks_prev, clicks_base: s.search_clicks ?? s.gsc_clicks, impr: s.search_impressions ?? s.gsc_impressions, trend: s.sessions_trend ?? [] };
}
/** 站点列表：Google + Bing 搜索点击率 */
export function searchCtr(r: SiteRow): number {
  const w = win(r);
  return w.impr > 0 ? w.clicks / w.impr : 0;
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
/** 需要你关注：被标记为危险网站、有待确认 / 失败的条目、上次上线失败、流量下滑、数据源出错。 */
export function needsAttention(r: SiteRow): boolean {
  return !!r.unsafe_by || !!r.site_down || expiring(r) || r.proposed > 0 || r.failed > 0 || r.last_deploy_ok === false || (r.links_broken ?? 0) > 0 || trafficDown(r) || r.summary.errors.length > 0;
}

// 「待处理」交给 AI 时选的模型记在本机
const BATCH_MODEL_KEY = 'cligc.pilot.batchModel';
const BATCH_EFFORT_KEY = 'cligc.pilot.batchEffort';
function readBatchEffort(): string {
  try { return localStorage.getItem(BATCH_EFFORT_KEY) ?? ''; } catch { return ''; }
}
function readBatchModel(): string {
  try { return localStorage.getItem(BATCH_MODEL_KEY) ?? ''; } catch { return ''; }
}

const DAYS_KEY = 'cligc.pilot.statDays';
/** 能选的统计时间段（和后端 site_data::WIN_DAYS 一致） */
export const STAT_DAYS = [7, 14, 28, 90] as const;
export type StatDays = (typeof STAT_DAYS)[number];
function readDays(): StatDays {
  try { const d = Number(localStorage.getItem(DAYS_KEY)); return (STAT_DAYS as readonly number[]).includes(d) ? (d as StatDays) : 28; } catch { return 28; }
}
export function setDays(d: StatDays) {
  fleet.days = d;
  try { localStorage.setItem(DAYS_KEY, String(d)); } catch { /* */ }
}

class FleetState {
  rows = $state<SiteRow[]>([]);
  loaded = $state(false);
  pending = $state<PendingItem[]>([]);
  /** 「待处理」里的运营机会（自己运营的站点）；切到哪一块：confirm 等你确认 / opps 运营机会 */
  opps = $state<OppOverview | null>(null);
  /** 「待处理」交给 AI 的默认模型（"<brain>::<model>"，记在本机，设置里改）；空＝还没设过，用 batchPick 的推断 */
  batchModel = $state(readBatchModel());
  /** 默认模型一起选的推理强度（空＝跟随模型默认） */
  batchEffort = $state(readBatchEffort());
  // 总览
  query = $state('');
  filter = $state<SiteFilter>('all');
  tag = $state('');
  sort = $state<SiteSort>('name');
  desc = $state(false);
  page = $state(0);
  tab = $state<'sites' | 'pending' | 'history'>('sites');
  /** 标题旁下拉里勾选的站点：只看这几个（站点列表、待处理、近期动态都按它筛）；空＝全部 */
  picks = $state<string[]>([]);
  /** 站点列表的统计时间段：近 7 / 14 / 28 / 90 天（记在本机） */
  days = $state<StatDays>(readDays());
  /** 正在拉数据 / 排队的站点、上次全部同步的时间、批量健康检查在不在跑 */
  sync = $state<DataSync>({ running: [], queued: [], health: false, last_at: 0 });
  byId = $derived(new Map(this.rows.map((r) => [r.id, r])));
  tags = $derived([...new Set(this.rows.flatMap((r) => r.tags))].sort((a, b) => a.localeCompare(b, 'zh')));
  attentionCount = $derived(this.rows.filter(needsAttention).length);
  pendingCount = $derived(this.rows.reduce((n, r) => n + r.proposed, 0));
  /** 没人自动处理的站点（自己运营的、托管停着的）里还没处理的运营机会；托管且在运行的由 AI 自动处理，不算 */
  oppCount = $derived(this.rows.filter((r) => !r.plan || r.autonomy === 'stopped').reduce((n, r) => n + r.open, 0));
}

export const fleet = new FleetState();

export const PAGE_SIZE = 50;

/** 这个站点在不在标题旁勾选的范围里（没勾任何站点＝都在） */
export function inPicks(id: string): boolean {
  return !fleet.picks.length || fleet.picks.includes(id);
}

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
      || (f === 'unsafe' && !!r.unsafe_by)
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
  { k: 'unsafe', l: '被标记为危险网站' },
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
  if (k === 'unsafe') return r.unsafe_by ?? '';
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
        : sort === 'clicks' ? -win(r).clicks
          : sort === 'change' ? clicksChange(r)
            : sort === 'users' ? -win(r).users
              : sort === 'impr' ? -win(r).impr
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
/** 导入站点后也跑一次：刚导入的站点没配部署、没接数据源，马上去 Cloudflare / Google / Bing 里找（数据流不用缓存）。 */
export async function autodetectDeploys(fresh = false) {
  autodetected = true;
  deployCheckedAt = Date.now();
  try {
    const [deploys, data] = await Promise.all([
      fresh || fleet.rows.some((r) => r.deploy_mode === 'none') ? invoke<string[]>('autodetect_cf_deploys').catch(() => []) : Promise.resolve([] as string[]),
      invoke<string[]>('autodetect_data_bindings', { fresh }).catch(() => [] as string[]),
    ]);
    if (deploys.length) say(`在 Cloudflare 里认出了 ${deploys.length} 个站点的部署：${deploys.join('；')}`);
    if (data.length) say(`按域名接上了 ${data.length} 个站点的数据源：${data.join('；')}。数据马上开始拉取`);
    if (fresh && !data.length) say('已连接的 Google / Bing 账号里没按域名找到新导入站点的数据源，可以在站点设置里手动选');
    if (deploys.length || data.length) {
      await refreshSites();
      void loadFleet();
    }
  } catch { /* 识别不到不打扰 */ }
}
// 启动后才在 Cloudflare 上配好的站点（比如刚把域名切到 Pages 项目）：还有没配置部署的就隔一会儿再找一次，不用重启
let deployCheckedAt = 0;
async function recheckDeploys() {
  if (!fleet.rows.some((r) => r.deploy_mode === 'none') || Date.now() - deployCheckedAt < 2 * 60_000) return;
  deployCheckedAt = Date.now();
  const deploys = await invoke<string[]>('autodetect_cf_deploys').catch(() => [] as string[]);
  if (!deploys.length) return;
  say(`在 Cloudflare 里认出了 ${deploys.length} 个站点的部署：${deploys.join('；')}`);
  await refreshSites();
  void loadFleet();
}
export async function loadFleet() {
  if (loading) { again = true; return; }
  loading = true;
  try {
    const rows = await invoke<SiteRow[]>('sites_overview');
    fleet.rows = rows;
    fleet.loaded = true;
    if (!autodetected) void autodetectDeploys();
    else void recheckDeploys();
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
/** 一个模型的短名字（下拉框、按钮上用）：Codex 本机默认 →「Codex」，其它用模型列表里的名字，如「Opus 5.5」 */
function shortName(brain: string, model: string): string {
  if (!model) return brain.startsWith('api:') ? brainLabel(brain).replace(/ · API$/, '') : brainTitle(brain);
  return launcher.comboOpts.find((o) => o.value === `${brain}::${model}`)?.label ?? model;
}
/** 交给 AI 的模型选项：和启动页同一份列表，下拉框上显示短名字 */
export function batchOptions() {
  return launcher.comboOpts.map((o) => { const [b, ...m] = o.value.split('::'); return { ...o, short: shortName(b, m.join('::')) }; });
}
/** 交给 AI 的默认模型：设过的（记在本机）；没设过就用列表里这些站点用得最多的那个，再不行用第一个能用的 */
export function batchPick(): string {
  const ok = (v: string) => launcher.comboOpts.some((o) => o.value === v && !o.disabled);
  if (fleet.batchModel && ok(fleet.batchModel)) return fleet.batchModel;
  const n = new Map<string, number>();
  for (const i of fleet.opps?.items ?? []) if (i.brain) n.set(`${i.brain}::${i.model}`, (n.get(`${i.brain}::${i.model}`) ?? 0) + 1);
  const common = [...n.entries()].sort((a, b) => b[1] - a[1]).map(([v]) => v).find(ok);
  return common ?? launcher.comboOpts.find((o) => !o.disabled)?.value ?? '';
}
export function setBatchModel(v: string, effort = '') {
  fleet.batchModel = v;
  fleet.batchEffort = effort;
  try { localStorage.setItem(BATCH_MODEL_KEY, v); localStorage.setItem(BATCH_EFFORT_KEY, effort); } catch { /* 存不了就只在这次有效 */ }
}
/** 模型的图标和短名字，如 { icon: 'claude', name: 'Opus 5.5' } */
export function modelTarget(pick: string): { icon: string; name: string } {
  const [b, ...m] = pick.split('::');
  return { icon: b ? modelIcon(b) : '', name: b ? shortName(b, m.join('::')) : 'AI' };
}
/** "brain::model" 或 model_used 的「brain · model」→ 短名字，如「Opus 5.5」 */
export function modelName(v: string): string {
  const [b, ...m] = v.includes('::') ? v.split('::') : v.split(' · ');
  return b ? shortName(b.trim(), m.join('::').trim()) : '';
}
/** 让 AI 改：按站点分组，每个站点一个作业，同时最多 3 个，其余排着。
 *  model 不传就用默认模型；都只管这一批，不改站点和托管方案里设置的模型。 */
export async function workOpps(items: OppItem[], ask = true, model?: string, effort?: string) {
  if (!items.length) return;
  const n = siteCount(items);
  const pick = model || batchPick();
  if (!pick) { say('没有能用的模型：先在设置里登录 Claude / Codex，或连上 API 服务', 'err'); return; }
  // 推理强度：临时选了模型就用一起选的那档；用默认模型就用默认存的那档（默认模型换过了就不带）
  const eff = model ? effort ?? '' : pick === fleet.batchModel ? fleet.batchEffort : '';
  const lab = effortLabel(pick, eff, launcher.comboOpts.find((o) => o.value === pick)?.apiProtocol);
  const name = modelTarget(pick).name + (lab ? ` · ${lab}` : '');
  const sitemap = items.some((i) => i.item.opp.kind === 'sitemap_error') ? '\nsitemap 报错先由程序检查：只是提交的地址不对或要重新提交的，直接处理好，不用 AI 改代码。' : '';
  const msg = `交给 ${name} 处理这 ${items.length} 条机会（${n} 个站点）？\n每个站点一个作业、一条一条改，同时最多 3 个站点在跑，其余排着等。改好、检查通过后留在「等你确认」里，你确认了才上线。${sitemap}\n\n只是这批用 ${name}${model && model !== batchPick() ? `（默认是 ${modelTarget(batchPick()).name}，不变）` : ''}，不改站点和托管方案里设置的模型。`;
  if (ask && items.length > 1 && !(await confirmDialog(msg, { title: `交给 ${name}`, kind: 'info', confirmText: `开始处理 ${items.length} 条` }))) return;
  const by = new Map<string, string[]>();
  for (const i of items) by.set(i.site_id, [...(by.get(i.site_id) ?? []), i.item.id]);
  try {
    const [b, ...m] = pick.split('::');
    await invoke('work_across_sites', { batches: [...by.entries()], model: [b, m.join('::')], effort: eff || null });
    ops.batch = { ...ops.batch, ...Object.fromEntries(by) };
    say(n > 3 ? `已交给 ${name}：${items.length} 条，先开始 3 个站点，其余排着` : `已交给 ${name}：${items.length} 条`);
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

export async function loadSync() {
  try { fleet.sync = await invoke<DataSync>('site_data_sync_state'); } catch { /* */ }
}
/** 「⋯ → 立即同步所有站点数据」 */
export async function syncAllData() {
  try {
    await invoke('sync_all_site_data');
    say('开始同步，一个站点一个站点拉，拉完的会先更新');
  } catch (e) { say(String(e), 'err'); }
}
/** 「⋯ → 全部站点重新检查健康」 */
export async function checkAllHealth() {
  try {
    const n = await invoke<number>('check_all_sites_health');
    say(`开始检查 ${n} 个站点的在线情况、证书、安全标记和推广链接，查完列表会自动更新`);
  } catch (e) { say(String(e), 'err'); }
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
    // 某个站点的数据拉完了（导入后、每天早上、⋯ 里立即同步）：列表上的数字跟着更新
    const un6 = listen('site-data', soon);
    const un7 = listen<Omit<DataSync, 'last_at'>>('site-data-sync', (e) => {
      const was = fleet.sync.queued.length + fleet.sync.running.length;
      fleet.sync = { ...fleet.sync, ...e.payload };
      // 一轮拉完：重新读一下上次同步时间
      if (was && !e.payload.queued.length && !e.payload.running.length) void loadSync();
    });
    void loadSync();
    // 去终端 / Cloudflare 后台部署完切回来，顺手再找一次没配置的部署
    const onFocus = () => { if (fleet.loaded) void recheckDeploys(); };
    window.addEventListener('focus', onFocus);
    return () => { for (const u of [un1, un2, un3, un4, un5, un6, un7]) void u.then((f) => f()); window.removeEventListener('focus', onFocus); };
  });
}

