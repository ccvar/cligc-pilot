import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { OpsItem, Opportunity, Site } from '$lib/types';
import { app, say, confirmDialog, refreshConvos } from './app.svelte.js';
import { launcher } from './launcher.svelte.js';
import { SITE_PREFIX, refreshSites } from './sites.svelte.js';
import { selectConn } from './connections.svelte.js';

/** 一条改动的来源（为什么做它）。 */
export const SOURCE_LABEL: Record<string, string> = {
  data: '数据', gap: '内容缺口', topic: '选题清单', positioning: '按站点定位推导', health: '体检', manual: '手动加入',
};
export function sourceOf(o: { source?: string; kind: string }): string {
  if (o.source) return o.source;
  return o.kind === 'crawl_issue' || o.kind === 'sitemap_error' ? 'health' : o.kind === 'content_gap' ? 'gap' : o.kind === 'plan' ? 'data' : 'data';
}
export const STAGE_LABEL: Record<string, string> = { new: '新站', growing: '成长期', established: '成熟期' };
export const STAGE_FOCUS: Record<string, string> = {
  new: '没有或很少搜索数据：不做数据调优，重点是体检修复和按站点定位补内容（支柱页、读者常问的问题、教程 / 对比 / 术语页），再补站内链接。',
  growing: '有展现、点击还少：先补排在 8–20 位的查询词对应的内容、改标题描述，再补内容缺口，最后做和已有展现主题相邻的增量内容。',
  established: '已有稳定流量：以 SEO 调整为主（低点击率、点击下滑、接近首页的页面），增量内容只做和已有流量主题紧密相关、有需求的选题。',
};
export const LEVEL_LABEL: Record<number, string> = { 1: '1 · 只分析', 2: '2 · 出方案', 3: '3 · 自动提 PR', 4: '4 · 低风险自动上线', 5: '5 · 全托管' };
export const KIND_LABEL: Record<string, string> = {
  plan: 'AI 计划', low_ctr: '点击率偏低', striking: '排名 8–20', declining: '点击下滑', content_gap: '内容缺口', crawl_issue: '抓取错误', sitemap_error: 'sitemap 报错', cannibal: '页面互相抢词', stale_year: '标题年份过时', stale: '内容久未更新', low_rpm: '访问多收入低', slow: '页面太慢', img_audit: '图片问题', schema: '结构化数据', serp_gap: '对比竞品补内容',
};
export const CHANGE_LABEL: Record<string, string> = {
  meta: '改标题 / 描述', content: '补内容', internal_links: '加内链', new_content: '写新内容', technical: '修技术问题',
  meta_description: 'meta 描述', title: '标题', mixed: '多类改动', plan: '方案',
};
export const STATUS_LABEL: Record<string, string> = {
  open: '待处理', working: 'AI 处理中', proposed: '待确认', merged: '已合并', deployed: '已上线', done: '已处理', dismissed: '已驳回', rolled_back: '已回滚', failed: '失败',
};
export const WHITELIST = [
  { key: 'meta_description', label: 'meta 描述', sub: '只改已有页面的 description，长度 30–170 字' },
  { key: 'title', label: '标题', sub: '只改 <title> / frontmatter 的 title，不改网址和正文标题' },
  { key: 'internal_links', label: '内链', sub: '只在正文里新增指向站内已有页面的链接' },
  { key: 'sitemap', label: 'sitemap 与 IndexNow', sub: '重新生成和提交，不改页面内容' },
];

export interface RunRecord { id: string; task: string; trigger: string; started: number; finished: number; ok: boolean; summary: string; error: string }
export interface OpsOverview {
  state: { last: Record<string, number>; runs: RunRecord[]; negatives: Record<string, number> };
  budget: { changes_used: number; changes_limit: number; tokens_used: number; tokens_limit: number };
  next: Record<string, number>;
}

class OpsState {
  queue = $state<Record<string, OpsItem[]>>({});
  overview = $state<Record<string, OpsOverview>>({});
  reports = $state<Record<string, { name: string; modified: number }[]>>({});
  report = $state<{ site: string; name: string; text: string } | null>(null);
  diff = $state<{ item: OpsItem; text: string } | null>(null);
  /** 每个站点这一批交给 AI 的条目（按顺序）：任务队列里还没轮到的写「排队中 · 第 k/N 条」 */
  batch = $state<Record<string, string[]>>({});
}

export const ops = new OpsState();

export async function loadQueue(id: string) {
  try { const v = await invoke<OpsItem[]>('site_queue', { id }); ops.queue = { ...ops.queue, [id]: v }; }
  catch (e) { say(String(e), 'err'); }
}
export async function loadOverview(id: string) {
  try { const v = await invoke<OpsOverview>('ops_overview', { id }); ops.overview = { ...ops.overview, [id]: v }; }
  catch (e) { say(String(e), 'err'); }
}
export async function loadReports(id: string) {
  try { const v = await invoke<{ name: string; modified: number }[]>('list_reports', { id }); ops.reports = { ...ops.reports, [id]: v }; }
  catch (e) { say(String(e), 'err'); }
}
export async function openReport(id: string, name: string) {
  try { ops.report = { site: id, name, text: await invoke<string>('read_report', { id, name }) }; }
  catch (e) { say(String(e), 'err'); }
}

export async function addToQueue(siteId: string, opp: Opportunity) {
  try { await invoke('queue_add', { id: siteId, opp }); await loadQueue(siteId); say('已加入待处理'); }
  catch (e) { say(String(e), 'err'); }
}
/** 队列里还没了结的条目：排队、处理中、等你确认、已合并没上线、失败 */
export const LIVE = new Set(['open', 'working', 'proposed', 'merged', 'failed']);
/** 站点健康里「交给 AI 优化 / 修」：what = speed（url 为空就把有问题的页面打包成一条）| audit。
 *  放进待处理并马上让 AI 处理；这几类改动都要你确认后才上线。 */
export async function healthToAi(siteId: string, what: 'speed' | 'audit', url?: string) {
  try {
    const r = await invoke<{ items: OpsItem[]; started: boolean; note: string }>('health_to_ai', { id: siteId, what, url: url ?? null });
    await loadQueue(siteId);
    say(r.started ? '已交给 AI：改好后在「待处理」里等你确认，确认了才上线' : r.note);
  } catch (e) { say(String(e), 'err'); }
}
/** hold：「执行但不上线」——这一轮改好的都留给你确认（只对 cycle 有用） */
export async function runTask(siteId: string, task: string, hold = false) {
  try { await invoke('run_ops_task', { id: siteId, task, hold }); }
  catch (e) { say(String(e), 'err'); }
}
/** model：这一次临时换的模型（"brain::model"），不改站点和托管方案的设置 */
export async function workItem(siteId: string, item: OpsItem, model = '', effort = '') {
  const [b, ...m] = model.split('::');
  try { await invoke('queue_work', { id: siteId, itemId: item.id, model: model ? [b, m.join('::')] : null, effort: model && effort ? effort : null }); }
  catch (e) { say(String(e), 'err'); }
}
/** 待你确认的这一版合不进主分支：放回待处理，马上让 AI 在最新的主分支上重改 */
export async function redoItem(siteId: string, item: OpsItem) {
  try { await invoke('queue_redo', { id: siteId, itemId: item.id }); say('先试着直接接到最新代码上，接不上再让 AI 照着这一版重做；好了照常检查'); }
  catch (e) { say(String(e), 'err'); }
}
export async function recheckItem(siteId: string, item: OpsItem) {
  try { await invoke('queue_recheck', { id: siteId, itemId: item.id }); }
  catch (e) { say(String(e), 'err'); }
}
export async function acceptItem(site: Site, item: OpsItem) {
  const what = item.branch
    ? site.source === 'github' ? `合并 PR #${item.pr_number}` : `把本地分支 ${item.branch} 压缩合并进 ${site.default_branch}`
    : '标记为已处理';
  const then = item.branch && site.deploy.mode !== 'none' ? (site.deploy.mode === 'direct' || site.deploy.mode === 'workers' ? '，然后构建并上线' : '，Cloudflare 会自动构建上线') : '';
  if (!(await confirmDialog(`确认这条改动：${what}${then}？`, { title: '确认改动', kind: 'info', confirmText: '确认' }))) return;
  try { await invoke('queue_accept', { id: site.id, itemId: item.id }); }
  catch (e) { say(String(e), 'err'); }
}
// ---- 算网站内容的代码文件（站点级放行清单）----
// 和后端 managed::never_allow 同一套：依赖、锁文件、构建 / 部署 / CI 配置、密钥配置、迁移、脚本永远不放行（后端加的时候会再查一遍）
const NEVER_NAMES = new Set(['package.json', 'package-lock.json', 'pnpm-lock.yaml', 'yarn.lock', 'bun.lock', 'bun.lockb', 'wrangler.toml', 'wrangler.json', 'wrangler.jsonc', 'pilot.json', '.env', '.dev.vars', '_headers', '_redirects', '_worker.js', 'tsconfig.json', 'go.mod', 'go.sum', 'Makefile', 'Dockerfile', 'Cargo.toml', 'Cargo.lock', 'requirements.txt', 'pyproject.toml']);
function neverAllow(path: string) {
  const p = path.replace(/^\.\//, '');
  const name = p.split('/').pop() ?? p;
  const first = p.split('/')[0];
  return NEVER_NAMES.has(name) || name.startsWith('.env')
    || ['astro.config', 'vite.config', 'next.config', 'svelte.config', 'tailwind.config'].some((x) => name.startsWith(x))
    || ['.github', 'node_modules', 'deploy', 'migrations', 'scripts', 'bin'].includes(first)
    || /\.(sh|toml|ya?ml|sql)$/.test(name);
}
function listedIn(path: string, list: string[]) {
  return list.some((x) => { const v = x.trim().replace(/^\.\//, ''); return !!v && (v.endsWith('/') ? path.startsWith(v) : path === v); });
}
/** 「不能自动上线」的原因里一律拦着、但可以按站点放行的代码文件（比如 Go 站把页面写在 internal/site/build.go 里） */
export function blockedCode(verdict: string, list: string[] = []): string[] {
  const out: string[] = [];
  for (const m of (verdict || '').matchAll(/(?:^|；)([^；：]+)：依赖、构建 \/ 部署配置、服务端代码不自动改/g)) {
    const f = m[1].trim();
    if (f && !neverAllow(f) && !listedIn(f, list) && !out.includes(f)) out.push(f);
  }
  return out;
}
const CODE_RULE: Record<string, string> = { review: '「稳妥」，代码改动留给你确认', small: '「稳妥」，代码改动留给你确认', all: '代码改动检查通过就自动上线' };
/** 把一个代码文件加进 / 移出站点的「算网站内容的代码文件」。plan：站点所在的托管方案（说明按什么规则判断） */
export async function setContentCode(site: Site, path: string, on: boolean, plan?: { name: string; autonomy: string; code_changes?: string }) {
  const rule = plan ? (plan.autonomy === 'auto' ? `「${plan.name}」现在是${CODE_RULE[plan.code_changes ?? 'all'] ?? ''}` : `「${plan.name}」现在是改好等你确认，放行后仍要你确认；方案改成全自动时才会自动上线`) : '';
  const msg = on
    ? `以后 AI 改 ${path} 时，不再一律留给你确认，改成按托管方案的「拦截等级」判断${rule ? `（${rule}）` : ''}。\n\n只放行这个文件：新加的代码里读环境变量、执行命令、请求外部服务的照样拦；依赖和构建 / 部署配置不受影响。确定这个文件里是页面内容（标题、描述、文章）再放行。`
    : `把 ${path} 移出清单？之后 AI 改到它，又会一律留给你确认。`;
  if (!(await confirmDialog(msg, { title: on ? '算作网站内容' : '移出清单', kind: on ? 'info' : 'warning', confirmText: on ? '放行' : '移出' }))) return false;
  try {
    await invoke<string[]>('site_content_code', { id: site.id, path, on });
    await refreshSites();
    say(on ? `已放行 ${path}。等你确认的改动点「重新检查」就按新规则判断` : `已把 ${path} 移出清单`);
    return true;
  } catch (e) { say(String(e), 'err'); return false; }
}
/** 一起合并上线：几条待确认的改动按顺序合并进主分支，合完只构建上线一次；合不进去的跳过、留在待确认里。 */
export async function acceptItems(site: Site, items: OpsItem[]) {
  const list = items.filter((i) => i.status === 'proposed' && i.branch);
  if (!list.length) return;
  const how = site.source === 'github' ? `按顺序合并这 ${list.length} 个 PR` : `按顺序把这 ${list.length} 个分支压缩合并进 ${site.default_branch}`;
  const then = site.deploy.mode === 'none' ? '' : site.deploy.mode === 'pages_git' ? '，合完 Cloudflare 构建上线一次' : '，合完构建并上线一次';
  const names = list.map((i) => `· ${i.opp.title}`).join('\n');
  // 改到同一个文件的：后面的多半合不进去（会自动接到最新代码上，接不上再让 AI 重做）
  const seen = new Map<string, number>();
  for (const i of list) for (const f of new Set(i.files ?? [])) seen.set(f, (seen.get(f) ?? 0) + 1);
  const shared = [...seen].filter(([, n]) => n > 1).map(([f]) => f);
  const warn = shared.length ? `\n\n注意：有几条改了同一个文件（${shared.slice(0, 3).join('、')}${shared.length > 3 ? ' 等' : ''}），后面的可能合不进去。合不进去的会自动接到最新代码上，接不上再让 AI 在最新代码上重做，之后照常检查。` : '';
  if (!(await confirmDialog(`${how}${then}？\n\n${names}\n\n合不进去的会跳过，之后自动接到最新代码上（接不上再让 AI 重做）。上线后验收不通过会把这次合并的全部回退。${warn}`, { title: '一起合并上线', kind: 'info', confirmText: `合并上线 ${list.length} 条` }))) return;
  try { await invoke('queue_accept_many', { id: site.id, itemIds: list.map((i) => i.id) }); }
  catch (e) { say(String(e), 'err'); }
}
/** 已经合并、上线那一步失败的改动：不再合并，直接重新上线。 */
export async function reshipItem(site: Site, item: OpsItem) {
  // 上线的是主分支最新的代码：别的已合并、没上线的改动也在里面，一起上线、一起验收
  const others = (ops.queue[site.id] ?? []).filter((i) => i.id !== item.id && i.status === 'merged' && !i.deployed_at && i.merge_sha);
  const also = others.length ? `\n\n另外 ${others.length} 条已合并、没上线的也会一起上线：\n${others.map((i) => `· ${i.opp.title}`).join('\n')}` : '';
  if (!(await confirmDialog(`「${item.opp.title}」已经合并进 ${site.default_branch}，上次上线没完成。现在用 ${site.default_branch} 最新的代码重新构建并上线到 ${site.domain || site.name}？${also}`, { title: '重新上线', kind: 'warning', confirmText: others.length ? `上线 ${others.length + 1} 条` : '重新上线' }))) return;
  try { await invoke('queue_accept', { id: site.id, itemId: item.id }); }
  catch (e) { say(String(e), 'err'); }
}
export async function dismissItem(site: Site, item: OpsItem) {
  const extra = item.pr_number ? `，并关闭 PR #${item.pr_number}` : item.branch ? '，并删除本地分支' : '';
  if (!(await confirmDialog(`驳回「${item.opp.title}」${extra}？之后不会再提这个机会。`, { title: '驳回', kind: 'warning', confirmText: '驳回' }))) return;
  try { await invoke('queue_dismiss', { id: site.id, itemId: item.id }); await loadQueue(site.id); }
  catch (e) { say(String(e), 'err'); }
}
/** 批量：选中的未处理机会按顺序让 AI 改（一个作业里一条一条做）。 */
/** model：这一批临时换的模型（"brain::model"），不改站点和托管方案的设置 */
export async function workItems(siteId: string, ids: string[], model = '', effort = '') {
  if (!ids.length) return;
  const [b, ...m] = model.split('::');
  try {
    await invoke('queue_work_many', { id: siteId, itemIds: ids, model: model ? [b, m.join('::')] : null, effort: model && effort ? effort : null });
    ops.batch = { ...ops.batch, [siteId]: ids };
  }
  catch (e) { say(String(e), 'err'); }
}
/** 批量驳回：只问一次。 */
export async function dismissItems(site: Site, items: OpsItem[]) {
  if (!items.length) return;
  if (!(await confirmDialog(`驳回选中的 ${items.length} 条机会？之后不会再提这些机会。`, { title: '批量驳回', kind: 'warning', confirmText: `驳回 ${items.length} 条` }))) return;
  let failed = 0;
  for (const i of items) {
    try { await invoke('queue_dismiss', { id: site.id, itemId: i.id }); } catch { failed++; }
  }
  await loadQueue(site.id);
  say(failed ? `驳回了 ${items.length - failed} 条，${failed} 条没成功` : `已驳回 ${items.length} 条`, failed ? 'err' : undefined);
}
/** 上线后验收的待核对问题看过了。 */
/** 上线后验收有几项没过，你看过、确认没问题：先二次确认（点了就不再提醒这几项） */
export async function ackItem(siteId: string, item: OpsItem) {
  const ok = await confirmDialog(`确认「${item.opp.title}」上线后没问题？\n\n要你核对的：${item.attention || '线上验收没过的几项'}\n\n确认后这几项就不再提醒，改动保留在线上。有问题的话点「回滚」，不要点这个。`, { title: '已核对', kind: 'info', confirmText: '没问题，已核对' });
  if (!ok) return;
  try { await invoke('queue_ack', { id: siteId, itemId: item.id }); await loadQueue(siteId); }
  catch (e) { say(String(e), 'err'); }
}
export async function restoreItem(siteId: string, item: OpsItem) {
  try { await invoke('queue_restore', { id: siteId, itemId: item.id }); await loadQueue(siteId); }
  catch (e) { say(String(e), 'err'); }
}
export async function markDone(siteId: string, item: OpsItem) {
  try { await invoke('queue_mark_done', { id: siteId, itemId: item.id }); await loadQueue(siteId); }
  catch (e) { say(String(e), 'err'); }
}
export async function rollbackItem(site: Site, item: OpsItem) {
  const how = site.source === 'github' ? '开一个回退 PR 并合并' : '在站点文件夹里回退那次提交';
  if (!(await confirmDialog(`回滚「${item.opp.title}」？Pilot 会${how}，然后重新上线。`, { title: '回滚', kind: 'danger', confirmText: '回滚' }))) return;
  try { await invoke('queue_rollback', { id: site.id, itemId: item.id }); }
  catch (e) { say(String(e), 'err'); }
}
export async function showDiff(siteId: string, item: OpsItem) {
  try { ops.diff = { item, text: await invoke<string>('queue_diff', { id: siteId, itemId: item.id }) }; }
  catch (e) { say(String(e), 'err'); }
}

/** 「开会话处理」：在站点里起一个新对话，把机会和数据依据预先填进输入框，由你确认后发送。 */
export function workOnOpportunity(site: Site, opp: Opportunity) {
  const qs = opp.queries.slice(0, 8).map((q) => `- 「${q.key}」展现 ${Math.round(q.impressions)}、点击 ${Math.round(q.clicks)}、点击率 ${(q.ctr * 100).toFixed(1)}%、排名 ${q.position.toFixed(1)}`).join('\n');
  const text = `${opp.title}\n\n${opp.page ? `页面：${opp.page}\n` : ''}依据：${opp.evidence}\n${qs ? `相关查询词（近 28 天）：\n${qs}\n` : ''}\n先看这个页面对应的源文件，给我一个具体的改动方案（改哪些地方、改成什么、为什么），我确认后再动手。`;
  void selectConn(SITE_PREFIX + site.id).then(() => {
    app.activeConvId = ''; app.activeConv = null;
    launcher.lDraft = text;
    app.view = 'launcher';
  });
}

export function initOps() {
  $effect(() => {
    const un1 = listen<{ site_id: string; queue: OpsItem[] }>('site-queue', (ev) => {
      ops.queue = { ...ops.queue, [ev.payload.site_id]: ev.payload.queue };
    });
    const un2 = listen('conversations-changed', () => { void refreshConvos(); });
    const un3 = listen<{ site_id: string; status: string; kind: string }>('site-job', (ev) => {
      const j = ev.payload;
      if (j.status !== 'running' && j.kind.startsWith('ops:')) {
        void loadQueue(j.site_id); void loadOverview(j.site_id); void loadReports(j.site_id);
        void refreshConvos();
      }
    });
    return () => { for (const u of [un1, un2, un3]) void u.then((f) => f()); };
  });
}
