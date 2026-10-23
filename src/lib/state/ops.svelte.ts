import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { OpsItem, Opportunity, Site } from '$lib/types';
import { app, say, confirmDialog, refreshConvos } from './app.svelte.js';
import { launcher } from './launcher.svelte.js';
import { SITE_PREFIX } from './sites.svelte.js';
import { selectConn } from './connections.svelte.js';

export const STAGE_LABEL: Record<string, string> = { new: '新站', growing: '成长期', established: '成熟期' };
export const STAGE_FOCUS: Record<string, string> = {
  new: '没有或很少搜索数据：不做数据调优，重点是体检修复和按站点定位补内容（支柱页、读者常问的问题、教程 / 对比 / 术语页），再补站内链接。',
  growing: '有展现、点击还少：先补排在 8–20 位的查询词对应的内容、改标题描述，再补内容缺口，最后做和已有展现主题相邻的增量内容。',
  established: '已有稳定流量：以 SEO 调整为主（低点击率、点击下滑、接近首页的页面），增量内容只做和已有流量主题紧密相关、有需求的选题。',
};
export const LEVEL_LABEL: Record<number, string> = { 1: '1 · 只分析', 2: '2 · 出方案', 3: '3 · 自动提 PR', 4: '4 · 低风险自动上线', 5: '5 · 全托管' };
export const KIND_LABEL: Record<string, string> = {
  plan: 'AI 计划', low_ctr: '点击率偏低', striking: '排名 8–20', declining: '点击下滑', content_gap: '内容缺口', crawl_issue: '抓取错误', sitemap_error: 'sitemap 报错',
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
export async function runTask(siteId: string, task: string) {
  try { await invoke('run_ops_task', { id: siteId, task }); }
  catch (e) { say(String(e), 'err'); }
}
export async function workItem(siteId: string, item: OpsItem) {
  try { await invoke('queue_work', { id: siteId, itemId: item.id }); }
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
export async function dismissItem(site: Site, item: OpsItem) {
  const extra = item.pr_number ? `，并关闭 PR #${item.pr_number}` : item.branch ? '，并删除本地分支' : '';
  if (!(await confirmDialog(`驳回「${item.opp.title}」${extra}？之后不会再提这个机会。`, { title: '驳回', kind: 'warning', confirmText: '驳回' }))) return;
  try { await invoke('queue_dismiss', { id: site.id, itemId: item.id }); await loadQueue(site.id); }
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
