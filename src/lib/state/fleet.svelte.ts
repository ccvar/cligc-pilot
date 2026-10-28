import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { SiteRow, PendingItem, Site, OppItem, OppOverview } from '$lib/types';
import { say, confirmDialog } from './app.svelte.js';
import { sites, refreshSites } from './sites.svelte.js';

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
  /** 开发版：「待处理」显示示例数据（只在界面上，不写进任何站点的队列） */
  demo = $state(false);
  /** 刚交给 AI、还没开始的条目（列表上先标出来，作业开始后由队列状态接手） */
  handed = $state<string[]>([]);
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
  /** 自己运营的站点里还没处理的运营机会（托管站点的由 AI 自动处理，不算） */
  oppCount = $derived(this.rows.filter((r) => !r.plan).reduce((n, r) => n + r.open, 0));
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
/** 开发版：打开 / 关掉示例数据 */
export function setDemo(on: boolean) {
  fleet.demo = on;
  if (on) { const d = demoData(fleet.rows); fleet.opps = d.opps; fleet.pending = d.pending; }
  else { void loadPending(); void loadOpps(); }
}
export async function loadPending() {
  if (fleet.demo) return;
  try { const v = await invoke<PendingItem[]>('pending_across_sites'); fleet.pending = v; }
  catch (e) { say(String(e), 'err'); }
}

// ── 开发版示例数据：借用前几个站点的名字和图标，条目都是假的，点了也不会处理 ──
function demoItem(id: string, kind: string, title: string, evidence: string, status = 'open', score = 0): PendingItem['item'] {
  const now = Math.floor(Date.now() / 1000);
  return {
    id: `demo-${id}`, status, created_at: now - 86400, updated_at: now - Number(id) * 3700, conv_id: '', branch: '', base_sha: '', head_sha: '', pr_number: 0, pr_url: '', change_kind: '', verdict: '', merge_sha: '', deployed_at: 0, review: '', review_note: '', note: '', auto: false,
    opp: { key: `demo:${id}`, kind, title, page: '', queries: [], clicks: 0, impressions: 0, ctr: 0, position: 0, prev_clicks: 0, score, change: 'content', evidence },
  } as PendingItem['item'];
}
export function demoData(rows: SiteRow[]): { opps: OppOverview; pending: PendingItem[] } {
  const pick = (n: number) => rows[n % Math.max(rows.length, 1)] ?? { id: 'demo', name: '示例站', domain: 'example.com' };
  const opp = (n: number, tier: OppItem['tier'], gain: number, ...a: [string, string, string, string]) => {
    const r = pick(n);
    return { site_id: r.id, site_name: r.name, domain: r.domain, tier, gain, item: demoItem(a[0], a[1], a[2], a[3], 'open', gain) } as OppItem;
  };
  const opps: OppOverview = {
    managed_open: 37, running: [pick(3).id], waiting: 0,
    items: [
      opp(0, 'urgent', 0, '1', 'crawl_issue', 'Bing 抓取 /go/old-exchange/ 返回 404', 'Bing 报告这个网址抓取失败。检查是否为死链、是否该加跳转。'),
      opp(1, 'urgent', 0, '2', 'slow', '/guides/buy-usdt/ 打开太慢', 'PageSpeed 手机版性能分 38，LCP 5.6 秒。主要问题：适当调整图片大小（约省 1.4 秒）。'),
      opp(2, 'urgent', 0, '3', 'declining', '/news/ 点击掉了 52%', '近 28 天点击 210（前 28 天 438），平均排名 6.1 → 11.4。'),
      opp(0, 'value', 182, '4', 'low_ctr', '/exchanges/okx/ 排第 4，点击率只有 1.2%', '近 28 天平均排名 4.2、展现 15200、点击率 1.2%，同排名的常见点击率约 6%。'),
      opp(3, 'value', 96, '5', 'striking', '「usdt 怎么买」排第 11，差一点进首页', '这些查询词近 28 天合计展现 8800、平均排名 11.2。'),
      opp(1, 'value', 64, '6', 'serp_gap', '「欧易交易所」排第 7，前 3 名都是别人', 'Google 上排在前面的是 okx.com、binance.com、zhihu.com。'),
      opp(4, 'value', 40, '7', 'content_gap', '没有专门回应「btc 手续费」的页面', '近 28 天展现 3100，最好的页面排在 18 位左右。'),
      opp(2, 'value', 0, '8', 'low_rpm', '/guides/wallet/ 访问多、广告收入低', '近 28 天 4200 次浏览，每千次收入约 $0.80，站内中位数约 $4.10。'),
      opp(0, 'easy', 0, '9', 'stale_year', '/guides/best-wallet/ 标题年份是 2025', '搜索的人看到旧年份会以为内容过时。'),
      opp(1, 'easy', 0, '10', 'img_audit', '图片问题：23 张缺替代文字', '体检时在构建产物里发现 23 张图片没有 alt。'),
      opp(4, 'easy', 0, '11', 'schema', '3 个页面的结构化数据有问题', 'Article 缺 作者 author、图片 image。'),
    ],
  };
  const pend = (n: number, id: string, kind: string, title: string, status: string) => { const r = pick(n); return { site_id: r.id, site_name: r.name, item: demoItem(id, kind, title, '', status) }; };
  const pending: PendingItem[] = [
    pend(0, '21', 'low_ctr', '改 /exchanges/binance/ 的标题和描述', 'proposed'),
    pend(2, '22', 'content_gap', '新页面：「usdt trc20 和 erc20 区别」', 'proposed'),
    pend(1, '23', 'crawl_issue', '修 /go/bybit/ 的跳转', 'failed'),
  ];
  return { opps, pending };
}

export async function loadOpps() {
  if (fleet.demo) return;
  try { fleet.opps = await invoke<OppOverview>('opportunities_across_sites'); }
  catch (e) { say(String(e), 'err'); }
}
const siteCount = (items: OppItem[]) => new Set(items.map((i) => i.site_id)).size;
/** 让 AI 改：按站点分组，每个站点一个作业，同时最多 3 个，其余排着 */
export async function workOpps(items: OppItem[], ask = true) {
  if (!items.length) return;
  if (fleet.demo) { say('这是示例数据，不会真的处理'); return; }
  const n = siteCount(items);
  if (ask && items.length > 1 && !(await confirmDialog(`让 AI 处理这 ${items.length} 条机会（${n} 个站点）？\n每个站点一个作业、一条一条改，同时最多 3 个站点在跑，其余排着等。改好、检查通过后留在「等你确认」里，你确认了才上线。`, { title: '批量让 AI 改', kind: 'info', confirmText: `开始处理 ${items.length} 条` }))) return;
  const by = new Map<string, string[]>();
  for (const i of items) by.set(i.site_id, [...(by.get(i.site_id) ?? []), i.item.id]);
  try {
    await invoke('work_across_sites', { batches: [...by.entries()] });
    fleet.handed = [...new Set([...fleet.handed, ...items.map((i) => i.item.id)])];
    say(n > 3 ? `已交给 AI：${items.length} 条，先开始 3 个站点，其余排着` : `已交给 AI：${items.length} 条`);
    void loadOpps();
  } catch (e) { say(String(e), 'err'); }
}
/** 驳回（只问一次）：之后不会再提这些机会，驳回的在站点「待处理」里能恢复 */
export async function dismissOpps(items: OppItem[]) {
  if (!items.length) return;
  if (fleet.demo) { say('这是示例数据，不会真的处理'); return; }
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
    return () => { void un1.then((f) => f()); void un2.then((f) => f()); void un3.then((f) => f()); void un4.then((f) => f()); };
  });
}

