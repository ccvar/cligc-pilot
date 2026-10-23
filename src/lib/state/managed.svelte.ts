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
    goal: '', actions: ACTIONS.map((a) => a.key), guidelines: GUIDELINE_TEMPLATE, topics: '', created_at: 0,
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
