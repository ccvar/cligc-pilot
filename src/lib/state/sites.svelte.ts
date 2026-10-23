import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type { Connection, Dashboard, CacheMeta, Site, SiteImportPreview, SiteJob, SiteStatus, DeployRecord, PagesDeployment, PagesProject, GhRepo } from '$lib/types';
import { app, say, confirmDialog, refreshConvos } from './app.svelte.js';
import { connections, selectConn } from './connections.svelte.js';

export const SITE_PREFIX = 'site:';
export const isSiteConnId = (id: string) => id.startsWith(SITE_PREFIX);
export const siteIdOf = (connId: string) => (isSiteConnId(connId) ? connId.slice(SITE_PREFIX.length) : '');

/** 站点在侧栏 / 切换器里表现为一个合成连接（kind=site），不落 connections.json。 */
export function siteConnection(s: Site): Connection {
  return { id: SITE_PREFIX + s.id, name: s.name, kind: 'site', skill_dir: '', key_prefix: '', key_kind: '', account_id: '', created_at: '' };
}

export type SiteTab = 'overview' | 'data' | 'queue' | 'ops' | 'deploy' | 'settings';

type ImportForm = {
  open: boolean;
  step: 'source' | 'review';
  source: 'local' | 'github';
  ghConnId: string;
  repo: string;
  repos: GhRepo[];
  reposLoading: boolean;
  busy: boolean;
  err: string;
  preview: SiteImportPreview | null;
  // 可编辑的识别结果
  name: string;
  domain: string;
  install: string;
  build: string;
  outDir: string;
  checks: string;
  mode: 'direct' | 'pages_git' | 'none';
  cfConnId: string;
  project: string;
  branch: string;
  projects: PagesProject[];
  projectsLoading: boolean;
  indexnowKey: string;
  keepEnv: boolean;
};

function blankImport(): ImportForm {
  return {
    open: false, step: 'source', source: 'local', ghConnId: '', repo: '', repos: [], reposLoading: false, busy: false, err: '', preview: null,
    name: '', domain: '', install: '', build: '', outDir: '', checks: '', mode: 'direct', cfConnId: '', project: '', branch: 'main',
    projects: [], projectsLoading: false, indexnowKey: '', keepEnv: true,
  };
}

const RECENT_KEY = 'cligc.pilot.recentSites';
const RECENT_MAX = 8;
function loadRecent(): string[] { try { return JSON.parse(localStorage.getItem(RECENT_KEY) || '[]'); } catch { return []; } }

class SitesState {
  list = $state<Site[]>([]);
  /** 最近打开过的站点（左栏「最近」用），新的在前。 */
  recent = $state<string[]>(loadRecent());
  loaded = $state(false);
  tab = $state<SiteTab>('overview');
  status = $state<Record<string, SiteStatus>>({});
  jobs = $state<Record<string, SiteJob>>({});
  deploys = $state<Record<string, DeployRecord[]>>({});
  pagesDeploys = $state<Record<string, PagesDeployment[]>>({});
  pagesErr = $state<Record<string, string>>({});
  imp = $state<ImportForm>(blankImport());
  /** 设置页的编辑副本（保存前不动列表里的那份）。 */
  draft = $state<Site | null>(null);
  saving = $state(false);
  envKeys = $state<string[]>([]);
  dash = $state<Record<string, Dashboard>>({});
  dataBusy = $state<Record<string, boolean>>({});
  activeId = $derived(siteIdOf(app.activeConnId));
  active = $derived(this.list.find((s) => s.id === this.activeId) ?? null);
}

export const sites = new SitesState();

export async function refreshSites() {
  try { sites.list = await invoke<Site[]>('list_sites'); }
  catch (e) { say(String(e), 'err'); }
  finally { sites.loaded = true; }
}

export function openSite(id: string, tab: SiteTab = 'overview') {
  connections.switcherOpen = false;
  sites.tab = tab;
  if (app.activeConnId === SITE_PREFIX + id) { openSiteBoard(tab); return; }
  void selectConn(SITE_PREFIX + id);
}

/** 记一次「打开过这个站点」：左栏只放置顶和最近的站点，全部站点在总览里找。 */
export function touchRecent(id: string) {
  const next = [id, ...sites.recent.filter((x) => x !== id)].slice(0, RECENT_MAX);
  sites.recent = next;
  try { localStorage.setItem(RECENT_KEY, JSON.stringify(next)); } catch { /* */ }
}

export function openSiteBoard(tab?: SiteTab) {
  if (tab) sites.tab = tab;
  app.activeConvId = ''; app.activeConv = null;
  app.view = 'site';
}

export async function loadSiteStatus(id: string) {
  try {
    const st = await invoke<SiteStatus>('site_status', { id });
    sites.status = { ...sites.status, [id]: st };
    if (st.job) sites.jobs = { ...sites.jobs, [id]: st.job };
  } catch (e) { say(String(e), 'err'); }
}
export async function loadDeploys(id: string) {
  try { const v = await invoke<DeployRecord[]>('site_deploys', { id }); sites.deploys = { ...sites.deploys, [id]: v }; }
  catch (e) { say(String(e), 'err'); }
}
export async function loadPagesDeploys(id: string) {
  try {
    const v = await invoke<PagesDeployment[]>('site_pages_deployments', { id }); sites.pagesDeploys = { ...sites.pagesDeploys, [id]: v };
    sites.pagesErr = { ...sites.pagesErr, [id]: '' };
  } catch (e) { sites.pagesErr = { ...sites.pagesErr, [id]: String(e) }; }
}

export async function deploySite(id: string) {
  const st = sites.status[id];
  if (st?.dirty) {
    const ok = await confirmDialog('站点文件夹里有未提交的改动，上线的将是文件夹当前的样子（包括这些改动）。继续？', { title: '构建并上线', kind: 'warning', confirmText: '继续上线' });
    if (!ok) return;
  }
  try {
    await invoke<string>('deploy_site', { id });
  } catch (e) { say(String(e), 'err'); }
}
export async function checkSite(id: string) {
  try { await invoke<string>('check_site', { id }); }
  catch (e) { say(String(e), 'err'); }
}
export async function cancelSiteJob(id: string) {
  try { await invoke('cancel_site_job', { id }); } catch (e) { say(String(e), 'err'); }
}
export async function retryPagesDeploy(id: string, deploymentId: string) {
  try { await invoke('retry_pages_deployment', { id, deploymentId }); say('已触发重新部署'); setTimeout(() => void loadPagesDeploys(id), 2500); }
  catch (e) { say(String(e), 'err'); }
}
export async function rollbackPagesDeploy(id: string, deploymentId: string) {
  const ok = await confirmDialog('把线上版本立即切回这次部署？代码不会改动，下次上线会覆盖它。', { title: '回滚线上版本', kind: 'warning', confirmText: '回滚' });
  if (!ok) return;
  try { await invoke('rollback_pages_deployment', { id, deploymentId }); say('已回滚到所选部署'); setTimeout(() => void loadPagesDeploys(id), 2500); }
  catch (e) { say(String(e), 'err'); }
}

// ---------- 数据 ----------
export async function loadDashboard(id: string) {
  try { const v = await invoke<Dashboard>('site_dashboard', { id }); sites.dash = { ...sites.dash, [id]: v }; }
  catch (e) { say(String(e), 'err'); }
}
/** 拉取数据（force=false 时只拉 6 小时内没拉过的源），完成后刷新看板。 */
export async function refreshSiteData(id: string, force = false) {
  if (sites.dataBusy[id]) return;
  sites.dataBusy = { ...sites.dataBusy, [id]: true };
  try {
    const meta = await invoke<CacheMeta>('refresh_site_data', { id, force });
    const errs = [meta.ga4.error && `GA4：${meta.ga4.error}`, meta.gsc.error && `Search Console：${meta.gsc.error}`, meta.bing.error && `Bing：${meta.bing.error}`].filter(Boolean);
    if (force && errs.length) say(errs.join('\n'), 'err');
    else if (force) say('数据已更新');
    await loadDashboard(id);
  } catch (e) { say(String(e), 'err'); }
  finally { sites.dataBusy = { ...sites.dataBusy, [id]: false }; }
}

// ---------- 导入 ----------
export function openSiteImport(source: 'local' | 'github' = 'local') {
  connections.switcherOpen = false;
  const firstGh = app.conns.find((c) => c.kind === 'github');
  const firstCf = app.conns.find((c) => c.kind === 'cloudflare');
  sites.imp = { ...blankImport(), open: true, source, ghConnId: firstGh?.id ?? '', cfConnId: firstCf?.id ?? '' };
  if (source === 'github' && firstGh) void loadImportRepos();
  if (firstCf) void loadImportProjects();
}
export function closeSiteImport() { if (!sites.imp.busy) sites.imp = blankImport(); }

export async function loadImportRepos() {
  const f = sites.imp;
  if (!f.ghConnId) return;
  f.reposLoading = true;
  try { f.repos = await invoke<GhRepo[]>('list_github_repos', { connId: f.ghConnId }); }
  catch (e) { f.err = String(e); }
  finally { f.reposLoading = false; }
}
export async function loadImportProjects() {
  const f = sites.imp;
  if (!f.cfConnId) { f.projects = []; return; }
  f.projectsLoading = true;
  try { f.projects = await invoke<PagesProject[]>('list_cf_pages_projects', { connId: f.cfConnId }); }
  catch (e) { f.projects = []; f.err = `读取 Pages 项目失败：${String(e)}`; }
  finally { f.projectsLoading = false; matchProject(); }
}

export async function pickImportFolder() {
  const picked = await open({ title: '选择站点代码所在的文件夹', directory: true, multiple: false });
  if (typeof picked === 'string' && picked.trim()) await detectImport({ path: picked });
}

/** 按识别出的项目名 / 域名 / 仓库匹配一个已有的 Pages 项目，并据此推断部署方式。 */
function matchProject() {
  const f = sites.imp;
  if (!f.preview || !f.projects.length) return;
  const d = f.preview.detected;
  const repo = f.source === 'github' ? f.repo : d.github_repo;
  const hit = f.projects.find((p) => p.name === (f.project || d.cf_project))
    ?? f.projects.find((p) => !!f.domain && p.domains.includes(f.domain))
    ?? f.projects.find((p) => !!repo && p.source_repo === repo);
  if (!hit) return;
  f.project = hit.name;
  if (hit.production_branch) f.branch = hit.production_branch;
  f.mode = hit.source_type === 'github' && f.source === 'github' ? 'pages_git' : 'direct';
}

export async function detectImport(arg: { path?: string }) {
  const f = sites.imp;
  f.busy = true; f.err = '';
  try {
    const preview = f.source === 'github'
      ? await invoke<SiteImportPreview>('detect_site_repo', { connId: f.ghConnId, repo: f.repo })
      : await invoke<SiteImportPreview>('detect_site_folder', { path: arg.path ?? '' });
    const d = preview.detected;
    Object.assign(f, {
      preview, step: 'review', name: preview.name, domain: d.domain, install: d.build.install, build: d.build.build,
      outDir: d.build.out_dir, checks: d.build.checks.join('\n'), project: d.cf_project, branch: d.branch || 'main',
      indexnowKey: d.indexnow_key, mode: f.cfConnId ? 'direct' : 'none',
    });
    matchProject();
  } catch (e) { f.err = String(e); }
  finally { f.busy = false; }
}

export async function confirmImport() {
  const f = sites.imp;
  const p = f.preview;
  if (!p) return;
  f.busy = true; f.err = '';
  try {
    const d = p.detected;
    const site = await invoke<Site>('import_site', {
      req: {
        source: f.source,
        local_dir: f.source === 'local' ? p.dir : '',
        gh_conn_id: f.source === 'github' ? f.ghConnId : '',
        repo: f.source === 'github' ? f.repo : d.github_repo,
        name: f.name,
        domain: f.domain,
        framework: d.framework,
        default_branch: f.source === 'github' ? (f.repos.find((r) => r.full_name === f.repo)?.default_branch ?? d.git_default_branch) : (d.git_default_branch || d.git_branch),
        build: { install: f.install.trim(), build: f.build.trim(), out_dir: f.outDir.trim(), checks: f.checks.split('\n').map((c) => c.trim()).filter(Boolean), env_keys: [] },
        deploy: { mode: f.mode, cf_conn_id: f.mode === 'none' ? '' : f.cfConnId, project: f.mode === 'none' ? '' : f.project.trim(), branch: f.branch.trim() },
        env: f.keepEnv ? d.env : [],
        indexnow_key: f.indexnowKey.trim(),
      },
    });
    f.busy = false;
    sites.imp = blankImport();
    await refreshSites();
    say(`已导入站点「${site.name}」`);
    openSite(site.id);
  } catch (e) { f.err = String(e); f.busy = false; }
}

// ---------- 设置 ----------
export function editSite(s: Site) {
  sites.draft = JSON.parse(JSON.stringify(s));
  void invoke<string[]>('site_env_keys', { id: s.id }).then((k) => (sites.envKeys = k)).catch(() => (sites.envKeys = []));
}
export async function saveSiteDraft() {
  const d = sites.draft;
  if (!d) return;
  sites.saving = true;
  try {
    const saved = await invoke<Site>('save_site', { site: d });
    sites.list = sites.list.map((s) => (s.id === saved.id ? saved : s));
    sites.draft = JSON.parse(JSON.stringify(saved));
    say('站点设置已保存');
  } catch (e) { say(String(e), 'err'); }
  finally { sites.saving = false; }
}
export async function setSiteEnv(id: string, set: [string, string][], remove: string[]) {
  try {
    const saved = await invoke<Site>('set_site_env', { id, set, remove });
    sites.list = sites.list.map((s) => (s.id === saved.id ? saved : s));
    sites.envKeys = saved.build.env_keys;
    if (sites.draft?.id === id) sites.draft.build.env_keys = saved.build.env_keys;
    return true;
  } catch (e) { say(String(e), 'err'); return false; }
}
export async function removeSite(s: Site) {
  const ok = await confirmDialog(`移除站点「${s.name}」？Pilot 里的数据缓存、运营记录和站点会话会一并删除；你的代码文件夹和 GitHub 仓库不会被改动。`, { title: '移除站点', kind: 'danger', confirmText: '移除' });
  if (!ok) return;
  try {
    await invoke('remove_site', { id: s.id });
    await refreshSites();
    await refreshConvos();
    void selectConn('__sites__');
  } catch (e) { say(String(e), 'err'); }
}

export function initSites() {
  $effect(() => { void refreshSites(); });
  $effect(() => {
    const un1 = listen<SiteJob>('site-job', (ev) => {
      const j = ev.payload;
      const prev = sites.jobs[j.site_id];
      // 行日志由 site-job-line 增量追加；快照里的 log 只在作业切换时整段替换。
      sites.jobs = { ...sites.jobs, [j.site_id]: prev && prev.id === j.id ? { ...j, log: prev.log.length > j.log.length ? prev.log : j.log } : j };
      if (j.status !== 'running') {
        void loadSiteStatus(j.site_id);
        void loadDeploys(j.site_id);
        if (j.kind.startsWith('ops:')) say(j.status === 'ok' ? `${j.title}完成` : j.status === 'canceled' ? `已停止${j.title}` : `${j.title}失败：${j.error.split('\n')[0]}`, j.status === 'ok' ? 'ok' : 'err');
        if (j.kind === 'check') say(j.status === 'ok' ? '预检通过：构建和检查都没问题' : j.status === 'canceled' ? '已取消预检' : `预检没通过：${j.error.split('\n')[0]}`, j.status === 'ok' ? 'ok' : 'err');
        if (j.kind === 'deploy') say(j.status === 'ok' ? '已上线' : j.status === 'canceled' ? '已取消上线' : `上线失败：${j.error.split('\n')[0]}`, j.status === 'ok' ? 'ok' : 'err');
      }
    });
    const un2 = listen<{ site_id: string; job_id: string; line: string }>('site-job-line', (ev) => {
      const { site_id, job_id, line } = ev.payload;
      const j = sites.jobs[site_id];
      if (!j || j.id !== job_id) return;
      const log = [...j.log, line];
      if (log.length > 400) log.splice(0, log.length - 400);
      sites.jobs = { ...sites.jobs, [site_id]: { ...j, log } };
    });
    return () => { void un1.then((f) => f()); void un2.then((f) => f()); };
  });
}
