// 站点制作：在「站点」里从零做一个新站。
// 填定位 → 选技术栈和风格 → create_site 建好文件夹 / git / 站点 / 定位 → 在新站点里开第一版对话。
// 技术栈不固定；每个新站都会写一份站点约定 .pilot/site.md，以后加内容、改页面都按它来（见 site_make.rs）。
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import type { Site } from '$lib/types';
import { app, say } from './app.svelte.js';
import { connections, selectConn } from './connections.svelte.js';
import { sites, refreshSites, SITE_PREFIX } from './sites.svelte.js';
import { launcher, STYLE_DIRECTIVES } from './launcher.svelte.js';
import { composer } from './composer.svelte.js';
import { brainUsable } from './models';
import { startChat } from './turns.svelte.js';

export const SITE_KINDS = [
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
export const LANGUAGES = ['简体中文', 'English', '繁體中文', '日本語', '한국어', '中英双语'];
export const GOALS = ['广告变现', '联盟推广', '获客留资', '品牌展示', '卖产品'];
export const PAGE_COUNTS = [5, 8, 12, 16];

const PARENT_KEY = 'cligc.pilot.newSiteParent';

function blank() {
  return {
    open: false, step: 'brief' as 'brief' | 'build',
    name: '', domain: '', about: '', kind: 'content', audience: '', language: '简体中文', goals: [] as string[],
    stack: '', stackTouched: false, style: '', pages: 8, parent: '',
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
  return [...count].sort((a, b) => b[1] - a[1])[0]?.[0] ?? '';
}

export function openSiteCreate() {
  connections.switcherOpen = false;
  Object.assign(siteCreate, blank(), { open: true, parent: defaultParent() });
}
export function closeSiteCreate() { if (!siteCreate.busy) Object.assign(siteCreate, blank()); }

/** 在 Pilot 里新建、还没配部署的站点：列表里标「制作中」。 */
export function isMaking(id: string): boolean {
  const s = sites.list.find((x) => x.id === id);
  return !!s && s.origin === 'created' && s.deploy.mode === 'none';
}

export function recommendedStack(kind: string): string { return SITE_KINDS.find((k) => k.value === kind)?.stack ?? 'astro'; }

export function toBuildStep() {
  const f = siteCreate;
  if (!f.name.trim() && f.domain.trim()) f.name = f.domain.trim();
  if (!f.stackTouched) f.stack = recommendedStack(f.kind);
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

export async function createSite() {
  const f = siteCreate;
  if (f.busy) return;
  f.busy = true; f.err = '';
  try {
    const r = await invoke<{ site: Site; message: string }>('create_site', {
      req: {
        name: f.name.trim(), domain: f.domain.trim(), about: f.about.trim(), kind: f.kind,
        audience: f.audience.trim(), language: f.language, goals: f.goals, stack: f.stack,
        style: f.style ? STYLE_DIRECTIVES[f.style] ?? '' : '', pages: f.pages, parent_dir: f.parent,
      },
    });
    try { localStorage.setItem(PARENT_KEY, f.parent); } catch { /* */ }
    f.busy = false;
    closeSiteCreate();
    await refreshSites();
    await selectConn(SITE_PREFIX + r.site.id);
    // 在新站点里开第一版对话：需求就是第一条消息（看得见、可追溯）
    app.activeConvId = ''; app.activeConv = null;
    composer.attachments = [];
    launcher.lDraft = r.message;
    if (brainUsable(launcher.lBrain)) await startChat();
    else { app.view = 'launcher'; say('站点已建好，需求也写好了：选一个可用的模型后发送，就开始制作第一版', 'err'); }
  } catch (e) {
    f.err = String(e);
  } finally {
    f.busy = false;
  }
}
