import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ManagedConfig, ManagedOverview, ManagedActivity, SitePlan, ManagedGroup } from '$lib/types';
import { app, say, confirmDialog } from './app.svelte.js';
import { refreshSites } from './sites.svelte.js';
import { loadFleet, loadPending } from './fleet.svelte.js';
import { selectConn } from './connections.svelte.js';
import { SITES_CONN_ID } from './env';

export type ManagedTab = 'plans' | 'needs' | 'digests' | 'groups';

class ManagedState {
  overview = $state<ManagedOverview | null>(null);
  activity = $state<ManagedActivity[]>([]);
  plans = $state<SitePlan[]>([]);
  digests = $state<string[]>([]);
  digest = $state<{ name: string; text: string } | null>(null);
  tab = $state<ManagedTab>('groups');
  draft = $state<ManagedConfig | null>(null);
  saving = $state(false);
  count = $derived(this.overview?.sites.length ?? 0);
  /** 正在新建 / 编辑的托管方案（弹窗）。 */
  editor = $state<ManagedGroup | null>(null);
  /** 打开编辑器时直接到哪一步、聚焦哪一栏（比如从方案卡片点「内容准则」）；编辑器读一次就清掉 */
  editorFocus = $state<'' | 'guidelines'>('');
  /** 托管方案里展开了进度的站点（切走再回来保持展开）。 */
  progress = $state<Set<string>>(new Set());
}

/** 托管方案允许的动作（和后端 managed::ACTIONS 一致）。 */
export const ACTIONS: { key: string; label: string; sub: string }[] = [
  { key: 'meta', label: '改标题和描述', sub: '点击率偏低的页面重写 title / description' },
  { key: 'content', label: '改进已有页面', sub: '补充、更新正文，回应排在 8–20 位的查询词' },
  { key: 'internal_links', label: '加站内链接', sub: '在相关页面之间补链接' },
  { key: 'new_content', label: '写新页面', sub: '按选题清单或内容缺口写新内容' },
  { key: 'technical', label: '修技术问题', sub: '死链、跳转、sitemap、抓取错误' },
];

export const AUTONOMY_LABEL: Record<string, string> = { suggest: '只出建议', review: '改好等你确认', auto: '全自动' };
export const AUTONOMY: { key: string; label: string; sub: string }[] = [
  { key: 'suggest', label: '只出建议', sub: 'AI 每周读数据和站点，出计划和改进建议，不改代码。想做哪条，在「待处理」里点「让 AI 改」。' },
  { key: 'review', label: '改好等你确认', sub: 'AI 在独立分支里改好、跑完构建和检查，每一条都等你确认再上线。' },
  { key: 'auto', label: '全自动', sub: 'AI 按下面的发布规则自动上线，上线后检查页面，效果变差自动回滚；只把例外交给你。' },
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
    goal: '优先更新过时的内容（费率、步骤、版本、截图说明），修复死链和跳转。',
    actions: ['meta', 'content', 'technical'], metric: 'clicks' },
  { key: 'safe', name: '稳健维护', icon: 'shield', fits: '不想多动的站',
    goal: '最低风险：以修技术问题、优化标题和描述为主。',
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

## 公开文案
- 面向访客的页面里不出现内部运营表达、内部术语、工程化表达、内部说明、系统提示或开发注释，例如「承接流量」「转化闭环」「内容支柱」「待补图」「上线前替换」「TODO」「本文由 AI 生成」。工作说明、缺素材、待办只写在会话里，也不要写进 HTML 注释。
- 标题、小标题和 FAQ 用读者真实会搜的长尾说法，写清对象、人群、场景、问题或限制条件。例如不写「提现路径优化」，写「USDT 提现选哪个网络手续费最低」。不编造搜索量，不机械重复关键词，也不要把所有标题都写成问句。
- 按钮、导航这类短标签保持简短，不为长尾词硬加字。
- 改到哪个页面，就顺带检查这一页的公开文案，发现上面的问题一并改掉。

## 结构
- 每页一个明确主题，H1 和页面标题一致，正文用 H2 / H3 分段。
- 开头两三句直接回答读者的问题；适合时加对比表和常见问题（FAQ）。
- 正文长度按页面类型（不算标题、导航和代码）：
  - 完整教程 / 指南（注册、认证、提现步骤等）：2000–4000 字，配真实截图
  - 对比 / 评测（费率、平台对比等）：2500 字以上，有对比表
  - 单个问题解答（如「USDT 提现选哪个网络」）：1200–2000 字
  - 术语 / 概念解释：800–1500 字
- 长度跟着把问题讲完整走，不为凑字数加内容。
- 标题不超过 32 个汉字，描述 70–120 字，包含主要关键词但读起来自然。

## 内容质量（SEO / GEO）
- 每个页面都要有别处没有的价值：真实步骤、具体数字、对比、注意事项、常见错误。不要用同一个模板换关键词批量做相似页面——Google 会把它当成规模化的低质内容或门页，整站降权。
- 写长尾选题前先看已有页面：能扩写已有页面就不新开页；意图相近的几个长尾词合并成一页，用小标题分别回答。
- 一个选题撑不起上面对应的长度、讲不完整一个问题，说明选题太窄：换一个，或者并进已有页面。
- 让 AI 搜索（Google AI 概览、ChatGPT、Perplexity 等）容易引用：开头直接给结论；关键事实用短句、列表或表格写清楚；术语第一次出现时解释；FAQ 一问一答；写清更新时间；适合时加结构化数据（FAQPage、HowTo、Article）。
- 体现亲身经验和可信度：写清操作时的版本、地区和时间，只写验证过的步骤。

## 事实
- 费率、时间、数字以站点现有内容或官方来源为准，并写明来源或时间；不确定的不写；不编造数据和用户评价。

## 截图
- 需要截图说明的步骤（注册、认证、设置、下单、界面说明等），用真实网页截图：用 Pilot 的截图工具打开真实页面截取，存进站点的图片目录，正文里引用，并写清 alt 和图注。
- 不用占位图、示意图或 AI 生成的界面图冒充截图。需要登录才能看到的页面截不到时，在会话里说明缺哪几张，不要用假图顶上。
- 截图里有个人信息、账号、余额等敏感内容时先裁掉或打码。截完打开图片确认画面正确：不是错误页、验证码、空白页或 Cookie 弹窗。

## 链接
- 新页面至少链接两个相关的已有页面，并在一个相关的已有页面里加一条指向它的链接。
- 不改推荐链接、邀请码和价格。
`;

function blankGroup(c: ManagedConfig | undefined, n: number): ManagedGroup {
  return {
    id: '', name: n ? `托管方案 ${n + 1}` : '托管方案', sites: [], stopped: [], paused: false, autonomy: 'auto',
    brain: c?.brain || 'claude', model: c?.model ?? '', fallback_brain: c?.fallback_brain ?? '', fallback_model: c?.fallback_model ?? '',
    new_content: 'auto', code_changes: 'all', daily_changes: c?.daily_changes ?? 3, tokens_per_week: c?.tokens_per_week ?? 2000000,
    goal: '', actions: ACTIONS.map((a) => a.key), guidelines: GUIDELINE_TEMPLATE, topics: '', metric: 'clicks', min_page_chars: 1500, jev_gate: true, skip_offtopic: true, created_at: 0,
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
export const CODE_LABEL: Record<string, string> = { review: '代码改动人审', small: '小的代码改动自动上线', all: '代码改动自动上线' };
export const kTok = (v: number) => (v >= 1e6 ? `${(v / 1e6).toFixed(v >= 1e7 ? 0 : 1)}M` : v >= 1e3 ? `${Math.round(v / 1e3)}K` : String(v));
/** 这个站点本周的托管额度满了没有：满了返回一句说明（和后端 over_budget 同口径），没满返回空。 */
export function budgetFull(b?: { changes_used: number; changes_limit: number; tokens_used: number; tokens_limit: number }): string {
  if (!b) return '';
  if (b.changes_limit > 0 && b.changes_used >= b.changes_limit) return `本周改动已满（${b.changes_used} / ${b.changes_limit} 条）`;
  if (b.tokens_limit > 0 && b.tokens_used >= b.tokens_limit) return `本周 AI 用量已满（${kTok(b.tokens_used)} / ${kTok(b.tokens_limit)} token）`;
  return '';
}

export function editGroup(g: ManagedGroup, focus: '' | 'guidelines' = '') { managed.editorFocus = focus; managed.editor = JSON.parse(JSON.stringify(g)); }
export async function saveGroup(): Promise<boolean> {
  const g = managed.editor;
  if (!g) return false;
  const how = g.autonomy === 'suggest' ? 'AI 会按这个方案每周读数据和站点，出计划和改进建议，不改代码。' : g.autonomy === 'review' ? 'AI 会按这个方案的规则改好页面、跑完检查，每一条都等你确认再上线。' : 'AI 会按这个方案的规则制定计划、自动修改并上线页面内容（依赖、配置、代码、推荐 / 邀请相关内容永远不会自动改），上线后自动检查，变差就回滚。';
  if (!g.id && g.sites.length && !(await confirmDialog(`把 ${g.sites.length} 个站点交给 AI 运营？${how}可以随时停止某个站点、暂停整个方案。`, { title: '新建托管方案', kind: 'info', confirmText: '开始托管' }))) return false;
  // 这次新选了 Codex：说清楚它在托管里会怎么跑（没有逐条确认，断网、只能改工作文件夹）
  const before = managed.overview?.groups.find((x) => x.id === g.id);
  const codexNow = g.brain === 'codex' || g.fallback_brain === 'codex';
  const codexBefore = !!before && (before.brain === 'codex' || before.fallback_brain === 'codex');
  if (codexNow && !codexBefore && !(await confirmDialog('Codex 没有逐条确认的钩子，Pilot 拦不住它执行的命令。托管时会把它关在每次改动的工作文件夹里并断开网络：改不到别的文件、推送不了、上不了线；但它也看不了外部网页、截不了图、装不了新依赖。推送和上线照常由 Pilot 在检查通过后执行。确定用 Codex 托管？', { title: '用 Codex 托管', kind: 'warning', confirmText: '确定使用' }))) return false;
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
/** 方案卡片上直接开关一条规则（不用打开编辑器）：判断模型的上线前质检 / 跳过无关搜索词。 */
export async function setGroupRule(g: ManagedGroup, rule: 'jev_gate' | 'skip_offtopic', on: boolean) {
  try {
    await invoke<ManagedGroup>('save_managed_group', { group: { ...g, [rule]: on } });
    await loadManaged();
    const label = rule === 'jev_gate' ? '上线前内容质检' : '跳过和站点定位无关的搜索词';
    say(`「${g.name}」${on ? '开启' : '关闭'}了${label}`);
  } catch (e) { say(String(e), 'err'); }
}
export async function deleteGroup(g: ManagedGroup) {
  if (!(await confirmDialog(`删除托管方案「${g.name}」？里面的 ${g.sites.length} 个站点会退出托管，改为你自己运营（Pilot 照常同步数据、找机会、体检，不再自动改代码）。`, { title: '删除托管方案', kind: 'warning', confirmText: '删除' }))) return;
  try { await invoke('delete_managed_group', { id: g.id }); managed.editor = null; await refreshSites(); void loadFleet(); await loadManaged(); say(`已删除「${g.name}」`); }
  catch (e) { say(String(e), 'err'); }
}
export async function setGroupPaused(g: ManagedGroup, paused: boolean) {
  try { await invoke('set_group_paused', { id: g.id, paused }); await loadManaged(); say(paused ? `已暂停「${g.name}」` : `已恢复「${g.name}」`); }
  catch (e) { say(String(e), 'err'); }
}
/** 一次打开 / 关掉一批站点的托管开关（方案的「暂停 / 恢复方案」、右上角的「暂停 / 恢复全部」都是它）。 */
export async function setSitesActive(groups: ManagedGroup[], active: boolean, what: string) {
  const ids = groups.flatMap((g) => g.sites.filter((x) => g.stopped.includes(x) === active));
  if (!ids.length) return;
  if (!active && !(await confirmDialog(`暂停${what}的 ${ids.length} 个站点？它们都不再自动运行（正在进行的那一项会跑完），规则保留，随时可以一起或一个个恢复。`, { title: `暂停${what}`, kind: 'warning', confirmText: '暂停' }))) return;
  try {
    for (const id of ids) await invoke('set_site_managed_active', { siteId: id, active });
    await loadManaged();
    say(active ? `已恢复 ${ids.length} 个站点的托管` : `已暂停 ${ids.length} 个站点的托管`);
  } catch (e) { say(String(e), 'err'); }
}
/** 单独启用 / 停止某个站点的托管。 */
export async function setSiteActive(siteId: string, active: boolean) {
  try { await invoke('set_site_managed_active', { siteId, active }); await loadManaged(); say(active ? '已恢复托管' : '已暂停托管，规则保留'); }
  catch (e) { say(String(e), 'err'); }
}

// 同时被几处叫到（打开控制台、保存后、定时刷新）时只跑一次：里面有旧版配置的迁移写盘，并发跑会互相覆盖
let managedLoading: Promise<void> | null = null;
export function loadManaged(): Promise<void> {
  if (managedLoading) return managedLoading;
  managedLoading = loadManagedOnce().finally(() => { managedLoading = null; });
  return managedLoading;
}
async function loadManagedOnce() {
  try {
    let [o, a] = await Promise.all([invoke<ManagedOverview>('managed_overview'), invoke<ManagedActivity[]>('managed_activity', { limit: 300 })]);
    // 暂停只有一层：每个站点自己的开关。旧版的「全部暂停」和「方案暂停」换成把对应站点的开关关掉（效果一样，什么都不跑）
    const pausedGroups = o.config.groups.filter((g) => o.config.paused || g.paused);
    if (pausedGroups.length) {
      for (const g of pausedGroups) {
        for (const id of g.sites.filter((x) => !g.stopped.includes(x))) await invoke('set_site_managed_active', { siteId: id, active: false });
        if (g.paused) await invoke('set_group_paused', { id: g.id, paused: false });
      }
      if (o.config.paused) await invoke('set_managed_paused', { paused: false });
      o = await invoke<ManagedOverview>('managed_overview');
      if (managed.draft) managed.draft.paused = false;
    }
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
  const g = managed.overview?.groups.find((x) => x.id === groupId);
  if (on && !(await confirmDialog(`把 ${ids.length} 个站点加入托管方案${g ? `「${g.name}」（${AUTONOMY_LABEL[g.autonomy] ?? ''}）` : ''}？之后按这个方案的规则运营，随时可以停止或退出。`, { title: '加入托管', kind: 'info', confirmText: '加入' }))) return;
  try {
    await invoke<number>('set_sites_managed', { ids, managed: on, groupId: groupId ?? null });
    await refreshSites(); void loadFleet(); await loadManaged();
    say(on ? `已托管 ${ids.length} 个站点` : `${ids.length} 个站点已退出托管，改为自己运营`);
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
