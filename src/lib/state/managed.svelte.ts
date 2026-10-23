import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ManagedConfig, ManagedOverview, ManagedActivity, SitePlan, ManagedGroup } from '$lib/types';
import { app, say, confirmDialog } from './app.svelte.js';
import { refreshSites } from './sites.svelte.js';
import { loadFleet, loadPending } from './fleet.svelte.js';
import { selectConn } from './connections.svelte.js';
import { SITES_CONN_ID } from './env';

export type ManagedTab = 'activity' | 'plans' | 'needs' | 'digests' | 'groups';

class ManagedState {
  overview = $state<ManagedOverview | null>(null);
  activity = $state<ManagedActivity[]>([]);
  plans = $state<SitePlan[]>([]);
  digests = $state<string[]>([]);
  digest = $state<{ name: string; text: string } | null>(null);
  tab = $state<ManagedTab>('activity');
  draft = $state<ManagedConfig | null>(null);
  saving = $state(false);
  count = $derived(this.overview?.sites.length ?? 0);
  /** 正在新建 / 编辑的托管方案（弹窗）。 */
  editor = $state<ManagedGroup | null>(null);
}

/** 托管方案允许的动作（和后端 managed::ACTIONS 一致）。 */
export const ACTIONS: { key: string; label: string; sub: string }[] = [
  { key: 'meta', label: '改标题和描述', sub: '点击率偏低的页面重写 title / description' },
  { key: 'content', label: '改进已有页面', sub: '补充、更新正文，回应排在 8–20 位的查询词' },
  { key: 'internal_links', label: '加站内链接', sub: '在相关页面之间补链接' },
  { key: 'new_content', label: '写新页面', sub: '按选题清单或内容缺口写新内容' },
  { key: 'technical', label: '修技术问题', sub: '死链、跳转、sitemap、抓取错误' },
];

export const METRIC_LABEL: Record<string, string> = {
  clicks: '点击', ctr: '点击率', impressions: '展现和覆盖的查询词', position: '平均排名', conversions: 'GA4 转化事件', health: '体检问题数',
};

/** 运营目标预设：点一下填好目标文字、勾上推荐的动作、定好衡量标准；都可以再改。 */
export const GOAL_PRESETS: { key: string; name: string; icon: string; goal: string; actions: string[]; metric: string; fits: string }[] = [
  { key: 'traffic', name: '拉新流量', icon: 'trend', fits: '新站 / 成长期',
    goal: '扩大在 Google 和 Bing 的覆盖：让更多页面和查询词进入搜索结果前列，以写新页面和补内容缺口为主。',
    actions: ['meta', 'content', 'internal_links', 'new_content', 'technical'], metric: 'impressions' },
  { key: 'ctr', name: '提升点击率', icon: 'target', fits: '成熟期',
    goal: '已经排在前面的页面，重写标题和描述、补足回应搜索意图的内容，让更多看到的人点进来。',
    actions: ['meta', 'content'], metric: 'ctr' },
  { key: 'convert', name: '提高转化', icon: 'zap', fits: '已配好 GA4 事件',
    goal: '流量不少但注册 / 邀请转化少：加强落地页的行动引导、对比和信任内容，把高流量页面引到转化页（推荐链接本身不改）。',
    actions: ['meta', 'content', 'internal_links'], metric: 'conversions' },
  { key: 'authority', name: '建立主题权威', icon: 'grid', fits: '定位清楚、内容偏少',
    goal: '围绕核心主题建内容体系：支柱页加子话题集群，理顺站内链接，让每个主题都有完整的覆盖。',
    actions: ['content', 'internal_links', 'new_content'], metric: 'position' },
  { key: 'fresh', name: '保鲜存量内容', icon: 'refresh', fits: '内容较旧的站',
    goal: '更新过时的内容（费率、步骤、版本、截图说明），修复死链和跳转；不写新页面。',
    actions: ['meta', 'content', 'technical'], metric: 'clicks' },
  { key: 'safe', name: '稳健维护', icon: 'shield', fits: '不想多动的站',
    goal: '最低风险：只修技术问题、优化标题和描述，不改正文、不写新页面。',
    actions: ['meta', 'technical'], metric: 'health' },
];

/** 按站点阶段推荐一个预设（选中站点里最多的那个阶段）。 */
export function suggestPreset(stages: string[]): string {
  if (!stages.length) return '';
  const n = (k: string) => stages.filter((x) => x === k).length;
  const top = ['new', 'growing', 'established'].sort((a, b) => n(b) - n(a))[0];
  return top === 'established' ? 'ctr' : 'traffic';
}

/** 新建方案时预填的内容准则：只是起点，运营人员可以整段改写或清空。 */
export const GUIDELINE_TEMPLATE = `# 内容准则

## 读者与语气
- 面向普通用户，用简体中文；句子短、直接，先给结论再解释。
- 不夸大、不堆砌关键词；不写「稳赚」「100%」这类绝对化承诺。

## 结构
- 每页一个明确主题，H1 和页面标题一致，正文用 H2 / H3 分段。
- 新页面 800–1500 字，开头两三句直接回答读者的问题；适合时加对比表和常见问题（FAQ）。
- 标题不超过 32 个汉字，描述 70–120 字，包含主要关键词但读起来自然。

## 事实
- 费率、时间、数字以站点现有内容或官方来源为准，不确定的不写；不编造数据和用户评价。

## 链接
- 新页面至少链接两个相关的已有页面，并在一个相关的已有页面里加一条指向它的链接。
- 不改推荐链接、邀请码和价格。
`;

function blankGroup(c: ManagedConfig | undefined, n: number): ManagedGroup {
  return {
    id: '', name: n ? `托管方案 ${n + 1}` : '托管方案', sites: [], stopped: [], paused: false,
    brain: c?.brain || 'claude', model: c?.model ?? '', fallback_brain: c?.fallback_brain ?? '', fallback_model: c?.fallback_model ?? '',
    new_content: c?.new_content ?? 'review', daily_changes: c?.daily_changes ?? 3, tokens_per_week: c?.tokens_per_week ?? 2000000,
    goal: '', actions: ACTIONS.map((a) => a.key), guidelines: GUIDELINE_TEMPLATE, topics: '', metric: 'clicks', created_at: 0,
  };
}

export const managed = new ManagedState();

export function openManaged(tab?: ManagedTab) {
  if (tab) managed.tab = tab;
  void selectConn(SITES_CONN_ID).then(() => { app.view = 'managed'; });
}

/** 新建托管方案（可以预先勾上几个站点）。 */
export function newGroup(sites: string[] = []) {
  const o = managed.overview;
  managed.editor = { ...blankGroup(o?.config, o?.groups.length ?? 0), sites: [...sites] };
}
export function editGroup(g: ManagedGroup) { managed.editor = JSON.parse(JSON.stringify(g)); }
export async function saveGroup(): Promise<boolean> {
  const g = managed.editor;
  if (!g) return false;
  if (!g.id && g.sites.length && !(await confirmDialog(`把 ${g.sites.length} 个站点交给 AI 全托管？AI 会按这个方案的规则制定计划、自动修改并上线页面内容（依赖、配置、代码、推荐 / 邀请相关内容永远不会自动改），上线后自动检查，变差就回滚。可以随时停止某个站点、暂停整个方案。`, { title: '新建托管方案', kind: 'info', confirmText: '开始托管' }))) return false;
  managed.saving = true;
  try {
    await invoke<ManagedGroup>('save_managed_group', { group: g });
    managed.editor = null;
    await refreshSites(); void loadFleet(); await loadManaged();
    say(g.id ? `已保存「${g.name}」` : `已新建托管方案「${g.name}」`);
    return true;
  } catch (e) { say(String(e), 'err'); return false; }
  finally { managed.saving = false; }
}
export async function deleteGroup(g: ManagedGroup) {
  if (!(await confirmDialog(`删除托管方案「${g.name}」？里面的 ${g.sites.length} 个站点会退出托管（改为第 3 档：AI 改好后由你确认再上线）。`, { title: '删除托管方案', kind: 'warning', confirmText: '删除' }))) return;
  try { await invoke('delete_managed_group', { id: g.id }); managed.editor = null; await refreshSites(); void loadFleet(); await loadManaged(); say(`已删除「${g.name}」`); }
  catch (e) { say(String(e), 'err'); }
}
export async function setGroupPaused(g: ManagedGroup, paused: boolean) {
  try { await invoke('set_group_paused', { id: g.id, paused }); await loadManaged(); say(paused ? `已暂停「${g.name}」` : `已恢复「${g.name}」`); }
  catch (e) { say(String(e), 'err'); }
}
/** 单独启用 / 停止某个站点的托管。 */
export async function setSiteActive(siteId: string, active: boolean) {
  try { await invoke('set_site_managed_active', { siteId, active }); await loadManaged(); say(active ? '已恢复托管' : '已停止托管，规则保留'); }
  catch (e) { say(String(e), 'err'); }
}

export async function loadManaged() {
  try {
    const [o, a] = await Promise.all([invoke<ManagedOverview>('managed_overview'), invoke<ManagedActivity[]>('managed_activity', { limit: 300 })]);
    managed.overview = o; managed.activity = a;
    if (!managed.draft) managed.draft = JSON.parse(JSON.stringify(o.config));
  } catch (e) { say(String(e), 'err'); }
}
export async function loadPlans() {
  try { const v = await invoke<SitePlan[]>('managed_plans'); managed.plans = v; } catch (e) { say(String(e), 'err'); }
}
export async function loadDigests() {
  try { const v = await invoke<string[]>('list_digests'); managed.digests = v; } catch (e) { say(String(e), 'err'); }
}
export async function openDigest(name: string) {
  try { const text = await invoke<string>('read_digest', { name }); managed.digest = { name, text }; } catch (e) { say(String(e), 'err'); }
}
export async function digestNow() {
  try { const r = await invoke<string>('digest_now'); say(`已生成日报：${r}`); await loadDigests(); if (managed.digests[0]) void openDigest(managed.digests[0]); }
  catch (e) { say(String(e), 'err'); }
}
export async function saveManaged() {
  if (!managed.draft) return;
  managed.saving = true;
  try {
    const c = await invoke<ManagedConfig>('save_managed_config', { config: managed.draft });
    managed.draft = JSON.parse(JSON.stringify(c));
    if (managed.overview) managed.overview.config = c;
    say('日报设置已保存');
  } catch (e) { say(String(e), 'err'); }
  finally { managed.saving = false; }
}
export async function setPaused(paused: boolean) {
  if (paused && !(await confirmDialog('暂停后所有托管站点都不再自动运行（正在进行的那一项会跑完）。随时可以恢复。', { title: '暂停托管', kind: 'warning', confirmText: '暂停' }))) return;
  try {
    const c = await invoke<ManagedConfig>('set_managed_paused', { paused });
    if (managed.overview) managed.overview.config = c;
    if (managed.draft) managed.draft.paused = c.paused;
    say(paused ? '已暂停托管' : '已恢复托管');
    void loadManaged();
  } catch (e) { say(String(e), 'err'); }
}
export async function setManagedSites(ids: string[], on: boolean, groupId?: string) {
  if (!ids.length) return;
  if (on && !(await confirmDialog(`把 ${ids.length} 个站点交给 AI 全托管？AI 会每周制定计划、自动修改并上线页面内容（依赖、配置、代码、推荐 / 邀请相关内容永远不会自动改），上线后自动检查，变差就回滚。随时可以退出或暂停。`, { title: '开始托管', kind: 'info', confirmText: '开始托管' }))) return;
  try {
    await invoke<number>('set_sites_managed', { ids, managed: on, groupId: groupId ?? null });
    await refreshSites(); void loadFleet(); await loadManaged();
    say(on ? `已托管 ${ids.length} 个站点` : `${ids.length} 个站点已退出托管（改为第 3 档）`);
  } catch (e) { say(String(e), 'err'); }
}

let timer: ReturnType<typeof setTimeout> | null = null;
function soon() {
  if (timer) return;
  timer = setTimeout(() => { timer = null; void loadManaged(); if (managed.tab === 'needs') void loadPending(); }, 1000);
}

export function initManaged() {
  $effect(() => { void loadManaged(); });
  $effect(() => {
    const un1 = listen('site-queue', soon);
    const un2 = listen<{ status: string }>('site-job', (e) => { if (e.payload.status !== 'running') soon(); });
    return () => { void un1.then((f) => f()); void un2.then((f) => f()); };
  });
}
