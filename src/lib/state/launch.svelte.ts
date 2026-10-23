import { invoke } from '@tauri-apps/api/core';
import type { Site } from '$lib/types';
import { app, say } from './app.svelte.js';
import { sites, refreshSites, openSite, SITE_PREFIX } from './sites.svelte.js';
import { launcher } from './launcher.svelte.js';
import { selectConn } from './connections.svelte.js';

export interface LaunchStatus {
  github: boolean;
  repo: string;
  pages: boolean;
  pages_domains: string[];
  domain_attached: boolean;
  ga4: boolean;
  gsc: boolean;
  indexnow: boolean;
  bing: boolean;
  deployed: boolean;
  google_manage_conn: string;
  errors: string[];
}

class LaunchState {
  status = $state<Record<string, LaunchStatus>>({});
  busy = $state<Record<string, string>>({});
  notes = $state<Record<string, Record<string, string>>>({});
  measurement = $state<Record<string, string>>({});
}

export const launch = new LaunchState();

export async function loadLaunch(id: string) {
  try { const v = await invoke<LaunchStatus>('launch_status', { id }); launch.status = { ...launch.status, [id]: v }; }
  catch (e) { say(String(e), 'err'); }
}

function note(id: string, step: string, text: string) {
  launch.notes = { ...launch.notes, [id]: { ...(launch.notes[id] ?? {}), [step]: text } };
}

async function step<T>(id: string, key: string, fn: () => Promise<T>, done: (r: T) => string) {
  launch.busy = { ...launch.busy, [id]: key };
  try {
    const r = await fn();
    note(id, key, done(r));
    await refreshSites();
    await loadLaunch(id);
  } catch (e) { note(id, key, `失败：${String(e)}`); }
  finally { launch.busy = { ...launch.busy, [id]: '' }; }
}

/** CF 建站项目 → 站点（本地来源，Pilot 直接上传），然后打开站点页的上线向导。 */
export async function importCfProject(connId: string, project: string) {
  try {
    const s = await invoke<Site>('import_cf_project', { connId, project });
    await refreshSites();
    say(`「${project}」已成为站点，按向导把它接上 GitHub、域名和数据`);
    openSite(s.id, 'overview');
  } catch (e) { say(String(e), 'err'); }
}

export function toGithub(site: Site, ghConnId: string, repo: string, create: boolean, priv: boolean) {
  return step(site.id, 'github', () => invoke<Site>('site_to_github', { id: site.id, ghConnId, repo, create, private: priv }), (s) => `代码已推到 ${s.repo}，之后的改动走分支和 PR`);
}
export function setupPages(site: Site) {
  return step(site.id, 'pages', () => invoke<{ created: boolean; domain_added: boolean; dns: string }>('site_setup_pages', { id: site.id }),
    (r) => `${r.created ? '已创建 Pages 项目；' : ''}${r.domain_added ? '已绑定域名；' : ''}${r.dns}`);
}
export function createGa4(site: Site, connId: string) {
  return step(site.id, 'ga4', () => invoke<{ property: string; measurement_id: string }>('ga4_create_property', { id: site.id, connId, account: '' }), (r) => {
    launch.measurement = { ...launch.measurement, [site.id]: r.measurement_id };
    return `已建 GA4 媒体资源 ${r.property}，衡量 ID ${r.measurement_id}。下一步：让 AI 把统计代码加进站点。`;
  });
}
export function setupGsc(site: Site, connId: string) {
  return step(site.id, 'gsc', () => invoke<string>('gsc_setup_domain', { id: site.id, connId }), (r) => r);
}
export function setupIndexNow(site: Site) {
  return step(site.id, 'indexnow', () => invoke<{ key: string; file: string; pr_url: string; note: string }>('setup_indexnow', { id: site.id }), (r) => `已生成 ${r.file}。${r.note}`);
}

/** 让 AI 在站点里埋 GA4 代码：开一个站点对话，把要求预先填好，由你确认后发送。 */
export function askAiForGa4(site: Site, measurementId: string) {
  const text = `给全站加上 GA4 统计代码，衡量 ID 是 ${measurementId}。\n\n要求：\n- 加在所有页面共用的 <head> 里（布局 / 模板文件），每个页面只加载一次；\n- 用官方 gtag.js 写法（async 加载 https://www.googletagmanager.com/gtag/js?id=${measurementId}，然后 gtag('config', '${measurementId}')）；\n- 如果站点已经有别的统计代码，先告诉我再决定；\n- 改完跑一次构建，确认产物里的页面都带上了这段代码。`;
  void selectConn(SITE_PREFIX + site.id).then(() => {
    app.activeConvId = ''; app.activeConv = null;
    launcher.lDraft = text;
    app.view = 'launcher';
  });
}

export function openSettingsTab() { sites.tab = 'settings'; }
