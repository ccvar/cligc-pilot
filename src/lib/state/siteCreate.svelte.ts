// 站点制作：在「站点」里从零做一个新站。
// 快速制作：一句话定位，其余交给 AI；详细设置：填定位 → 选技术栈和风格。都由 create_site 建好文件夹 / git / 站点 / 定位，再在新站点里开第一版对话。
// 技术栈不固定；每个新站都会写一份站点约定 .pilot/site.md，以后加内容、改页面都按它来（见 site_make.rs）。
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import type { Site } from '$lib/types';
import { app, say } from './app.svelte.js';
import { friendly } from '$lib/friendlyError';
import { connections, selectConn } from './connections.svelte.js';
import { sites, refreshSites, SITE_PREFIX } from './sites.svelte.js';
import { SITES_CONN_ID } from './env';
import { launcher, STYLE_DIRECTIVES } from './launcher.svelte.js';
import { composer } from './composer.svelte.js';
import { brainUsable } from './models';
import { startChat } from './turns.svelte.js';
import { homeDir } from '@tauri-apps/api/path';

export const SITE_KINDS = [
  { value: '', label: '让 AI 判断', sub: '按上面的描述判断是哪类站', stack: '' },
  { value: 'content', label: '内容 / SEO 站', sub: '文章、指南、评测，靠搜索带来流量', stack: 'astro' },
  { value: 'docs', label: '文档 / 知识库', sub: '教程、帮助中心、API 文档', stack: 'vitepress' },
  { value: 'landing', label: '落地页 / 产品介绍', sub: '一个或几个页面，讲清楚一件事', stack: 'html' },
  { value: 'tool', label: '工具 / 导航站', sub: '有交互的小工具、资源导航', stack: 'astro' },
];
export const STACKS = [
  { value: 'astro', label: 'Astro', sub: 'Markdown 写内容，构建出纯静态 HTML，速度快，内容站首选' },
  { value: 'hugo', label: 'Hugo', sub: '构建极快，适合文章很多的站；本机需要装 hugo' },
  { value: 'vitepress', label: 'VitePress', sub: '文档、教程、知识库：自带侧边栏和站内搜索' },
  { value: 'html', label: '纯 HTML', sub: '不需要构建，几页的落地页改起来最直接' },
  { value: 'next', label: 'Next.js 静态导出', sub: '交互较多的工具类站点；只用静态导出' },
];
export const LANGUAGES = [
  { value: '', label: '让 AI 判断', sub: '按定位和读者判断' },
  ...['简体中文', 'English', '繁體中文', '日本語', '한국어', '中英双语'].map((l) => ({ value: l, label: l })),
];
export const GOALS = ['广告变现', '联盟推广', '获客留资', '品牌展示', '卖产品'];
export const PAGE_COUNTS = [5, 8, 12, 16];

const PARENT_KEY = 'cligc.pilot.newSiteParent';
const MODE_KEY = 'cligc.pilot.newSiteMode';
let homeSites = '';
void homeDir().then((h) => { homeSites = `${h.replace(/[\\/]+$/, '')}/Sites`; }).catch(() => {});

export type CreateMode = 'quick' | 'full';
function savedMode(): CreateMode {
  try { return localStorage.getItem(MODE_KEY) === 'full' ? 'full' : 'quick'; } catch { return 'quick'; }
}
export function setCreateMode(m: CreateMode) {
  siteCreate.mode = m; siteCreate.err = '';
  try { localStorage.setItem(MODE_KEY, m); } catch { /* */ }
}

function blank() {
  return {
    open: false, mode: 'quick' as CreateMode, step: 'brief' as 'brief' | 'build',
    name: '', domain: '', about: '', kind: '', audience: '', language: '', goals: [] as string[],
    stack: '', stackTouched: false, style: '', pages: 8, parent: '',
    contentExtra: '', visualExtra: '',
    busy: false, err: '',
  };
}
export const siteCreate = $state(blank());

/** 默认放在已有本地站点最常用的那个上级目录（比如 ~/work），用过一次就记住。 */
function defaultParent(): string {
  try { const v = localStorage.getItem(PARENT_KEY); if (v) return v; } catch { /* */ }
  const count = new Map<string, number>();
  for (const s of sites.list) {
    if (s.source !== 'local' || !s.local_dir) continue;
    const p = s.local_dir.replace(/[\\/][^\\/]+[\\/]?$/, '');
    if (p) count.set(p, (count.get(p) ?? 0) + 1);
  }
  return [...count].sort((a, b) => b[1] - a[1])[0]?.[0] ?? homeSites;
}

export function openSiteCreate() {
  connections.switcherOpen = false;
  Object.assign(siteCreate, blank(), { open: true, mode: savedMode(), parent: defaultParent() });
}
export function closeSiteCreate() { if (!siteCreate.busy) Object.assign(siteCreate, blank()); }

/** 在 Pilot 里新建、还没配部署的站点，以及还没切换的迁移站点：列表里标「制作中」。 */
export function isMaking(id: string): boolean {
  const s = sites.list.find((x) => x.id === id);
  return !!s && ((s.origin === 'created' && s.deploy.mode === 'none') || s.origin === 'migrating');
}

/** 类型交给 AI 判断时，先按描述里的字眼猜一个，只用来在第二步标「推荐」的技术栈。 */
export function guessKind(about: string): string {
  const t = about.toLowerCase();
  if (/文档|教程|帮助中心|知识库|手册|api|docs?\b|wiki/.test(t)) return 'docs';
  if (/工具|导航|计算器|生成器|转换|查询|在线.{0,4}(测|算)|tool|calculator|generator/.test(t)) return 'tool';
  if (/落地页|官网|产品介绍|单页|landing|预约|报名/.test(t)) return 'landing';
  return 'content';
}
/** 制作中的站点（不出现在「全部站点」里，放在「站点制作」）。 */
export function makingSites(): Site[] { return sites.list.filter((s) => isMaking(s.id)); }

/** 打开「站点制作」：制作中的站点 + 制作新站点。 */
export function openMaking() {
  connections.switcherOpen = false;
  void selectConn(SITES_CONN_ID).then(() => { app.view = 'making'; });
}

/** 手动标记做完：不等配置部署，直接移到「全部站点」。 */
export async function finishMaking(site: Site) {
  try {
    await invoke('save_site', { site: { ...site, origin: 'made' } });
    await refreshSites();
    say(`「${site.name}」已移到全部站点`);
  } catch (e) { say(String(e), 'err'); }
}

export function recommendedStack(kind: string, about = ''): string {
  const k = kind || guessKind(about);
  return SITE_KINDS.find((x) => x.value === k)?.stack || 'astro';
}

export function toBuildStep() {
  const f = siteCreate;
  if (!f.name.trim() && f.domain.trim()) f.name = f.domain.trim();
  if (!f.stackTouched) f.stack = recommendedStack(f.kind, f.about);
  f.err = '';
  f.step = 'build';
}

/** 和后端 site_make::folder_name 一致：有域名用域名，否则用站点名的 slug。只用来预览路径。 */
export function folderName(domain: string, name: string): string {
  const d = domain.trim().toLowerCase().replace(/^https?:\/\//, '').replace(/[^a-z0-9.-]/g, '').replace(/^[.-]+|[.-]+$/g, '');
  if (d) return d;
  const s = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40).replace(/-+$/, '');
  return s || 'new-site';
}

export async function pickParent() {
  const picked = await open({ title: '新站点放在哪个文件夹下', directory: true, multiple: false, defaultPath: siteCreate.parent || undefined });
  if (typeof picked === 'string' && picked.trim()) siteCreate.parent = picked;
}

/** 在站点里开一个新对话，把 text 作为第一条消息发出去；没有可用模型时停在启动页、消息留在输入框里。 */
/** 在站点里开一个新对话，把 text 作为第一条消息发出去；没有可用模型时停在启动页、消息留在输入框里。
 *  perm 给了就只用于这一条对话（比如制作站点用「全自动」），不改你平时选的档位。 */
export async function startSiteChat(siteId: string, text: string, noBrainHint = '消息已经写好：选一个可用的模型后发送', perm = '') {
  await selectConn(SITE_PREFIX + siteId);
  app.activeConvId = ''; app.activeConv = null;
  composer.attachments = [];
  launcher.lDraft = text;
  if (!brainUsable(launcher.lBrain)) { app.view = 'launcher'; say(noBrainHint, 'err'); return; }
  await startChat(perm ? { perm } : {});
}

/** 制作站点（第一版、继续制作、修上线前检查的问题）用「全自动」：在 Pilot 新建的本地文件夹里干活，
 *  拿不到部署凭据、提示词也要求不部署，每步都停下来等批准只会打断它。 */
export const MAKING_PERM = 'full';

export async function createSite() {
  const f = siteCreate;
  if (f.busy) return;
  f.busy = true; f.err = '';
  // 快速制作：只带定位、名称、域名；类型、语言、风格交给 AI，技术栈按描述推荐，内容和视觉标准照常写进第一条消息
  const quick = f.mode === 'quick';
  const extra = { content_extra: f.contentExtra.trim(), visual_extra: f.visualExtra.trim() };
  const req = quick
    ? { name: f.name.trim(), domain: f.domain.trim(), about: f.about.trim(), stack: recommendedStack('', f.about), pages: 8, parent_dir: f.parent, quick: true, ...extra }
    : {
        ...extra,
        name: f.name.trim(), domain: f.domain.trim(), about: f.about.trim(), kind: f.kind,
        audience: f.audience.trim(), language: f.language, goals: f.goals, stack: f.stack,
        style: f.style ? STYLE_DIRECTIVES[f.style] ?? '' : '', pages: f.pages, parent_dir: f.parent,
      };
  try {
    const r = await invoke<{ site: Site; message: string }>('create_site', { req });
    try { localStorage.setItem(PARENT_KEY, f.parent); } catch { /* */ }
    f.busy = false;
    closeSiteCreate();
    await refreshSites();
    // 在新站点里开第一版对话：需求就是第一条消息（看得见、可追溯）
    await startSiteChat(r.site.id, r.message, '站点已建好，需求也写好了：选一个可用的模型后发送，就开始制作第一版', MAKING_PERM);
  } catch (e) {
    f.err = friendly(e);
  } finally {
    f.busy = false;
  }
}
