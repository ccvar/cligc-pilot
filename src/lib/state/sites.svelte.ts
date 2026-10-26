import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type { Connection, Dashboard, CacheMeta, Site, SiteImportPreview, SiteJob, SiteStatus, DeployRecord, PagesDeployment, PagesProject, GhRepo } from '$lib/types';
import { app, say, confirmDialog, refreshConvos } from './app.svelte.js';
import { connections, selectConn } from './connections.svelte.js';

export const SITE_PREFIX = 'site:';
/** 由 Pilot 构建、检查、上传的部署方式（Pages 直接上传 / Workers）。 */
export const pilotDeploys = (mode: string) => mode === 'direct' || mode === 'workers' || mode === 'vps';
export const DEPLOY_LABEL: Record<string, string> = { direct: 'Pilot 构建后上传到 Pages', workers: 'Pilot 构建后部署到 Workers（wrangler deploy）', vps: 'VPS：检查通过后执行项目自带的发布脚本', pages_git: 'Pages 绑定 GitHub（合并即上线）', none: '暂不部署' };
export const isSiteConnId = (id: string) => id.startsWith(SITE_PREFIX);
export const siteIdOf = (connId: string) => (isSiteConnId(connId) ? connId.slice(SITE_PREFIX.length) : '');

/** 站点在侧栏 / 切换器里表现为一个合成连接（kind=site），不落 connections.json。 */
export function siteConnection(s: Site): Connection {
  return { id: SITE_PREFIX + s.id, name: s.name, kind: 'site', skill_dir: '', key_prefix: '', key_kind: '', account_id: '', created_at: '' };
}

export type SiteTab = 'overview' | 'data' | 'queue' | 'deploy' | 'distribute' | 'settings';

/** 识别结果里可以改的部分：单个导入的表单和批量导入的每一行都用它。 */
type ImportFields = {
  name: string; domain: string; install: string; build: string; outDir: string; root: string; checks: string;
  mode: 'direct' | 'workers' | 'vps' | 'pages_git' | 'none';
  sshConnId: string; command: string; project: string; branch: string; indexnowKey: string; keepEnv: boolean;
};
/** 批量导入的一行：一个文件夹或一个仓库。 */
export type ImportItem = ImportFields & {
  /** 本地＝文件夹路径；GitHub＝owner/repo */
  key: string;
  preview: SiteImportPreview | null;
  status: 'detecting' | 'ready' | 'error' | 'done';
  err: string;
  /** 识别时的说明（比如没找到对应的服务器连接，先按暂不部署导入） */
  note: string;
  /** 已经导入过的站点名（默认不勾） */
  dup: string;
  pick: boolean;
};

type ImportForm = {
  open: boolean;
  step: 'source' | 'review' | 'batch';
  /** GitHub 来源勾选的仓库（只勾一个时走单个导入的详细表单） */
  repoPicks: string[];
  batch: ImportItem[];
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
  /** 站点在仓库里的子目录 */
  root: string;
  checks: string;
  mode: 'direct' | 'workers' | 'vps' | 'pages_git' | 'none';
  sshConnId: string;
  command: string;
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
    open: false, step: 'source', repoPicks: [], batch: [], source: 'local', ghConnId: '', repo: '', repos: [], reposLoading: false, busy: false, err: '', preview: null,
    name: '', domain: '', install: '', build: '', outDir: '', root: '', checks: '', mode: 'direct', cfConnId: '', project: '', branch: 'main',
    projects: [], projectsLoading: false, indexnowKey: '', keepEnv: true, sshConnId: '', command: '',
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
  /** 打开「待处理」时要先看哪一栏（从别处跳过来时指定，用一次就清掉）；空＝自动挑有内容的那栏 */
  queueFilter = $state<'' | 'todo' | 'open' | 'live' | 'closed'>('');
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
  /** 站点图标（data URL），按站点 id；没有图标的站点不在里面。 */
  icons = $state<Record<string, string>>({});
  activeId = $derived(siteIdOf(app.activeConnId));
  active = $derived(this.list.find((s) => s.id === this.activeId) ?? null);
}

export const sites = new SitesState();

export async function refreshSites() {
  try { sites.list = await invoke<Site[]>('list_sites'); }
  catch (e) { say(String(e), 'err'); }
  finally { sites.loaded = true; }
  void loadSiteIcons();
}

let iconsFor = '';
/** 站点图标：站点集合变了才重新取（本地文件优先，线上结果后端缓存）。 */
export async function loadSiteIcons(force = false) {
  const key = sites.list.map((x) => `${x.id}:${x.domain}`).join(',');
  if (!force && key === iconsFor) return;
  iconsFor = key;
  try { sites.icons = (await invoke<Record<string, string>>('site_icons')) ?? {}; } catch { /* 图标拿不到就用默认标识 */ }
}

export function openSite(id: string, tab: SiteTab = 'overview', queueFilter: '' | 'todo' | 'open' | 'live' | 'closed' = '') {
  connections.switcherOpen = false;
  sites.tab = tab;
  sites.queueFilter = queueFilter;
  if (app.activeConnId === SITE_PREFIX + id) { openSiteBoard(tab); return; }
  void selectConn(SITE_PREFIX + id);
}

/** 记一次「打开过这个站点」：左栏只放最近的站点，全部站点在总览里找。 */
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
/** 补上线上缺的验证文件：上次上线的构建产物还在就原样补文件再传（不重新构建），否则走完整的构建并上线。 */
export async function publishFiles(id: string) {
  try { await invoke<string>('site_publish_files', { id, fallback: true }); }
  catch (e) { say(String(e), 'err'); }
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
  const picked = await open({ title: '选择站点代码所在的文件夹（按住 ⌘ 可以多选）', directory: true, multiple: true });
  const list = (Array.isArray(picked) ? picked : picked ? [picked] : []).filter((p) => p.trim());
  if (list.length === 1) await detectImport({ path: list[0] });
  else if (list.length > 1) await startBatch(list);
}

/** GitHub 来源的「下一步」：勾了一个走详细表单，勾了多个走批量清单。 */
export async function importPickedRepos() {
  const f = sites.imp;
  if (f.repoPicks.length === 1) { f.repo = f.repoPicks[0]; await detectImport({}); }
  else if (f.repoPicks.length > 1) await startBatch([...f.repoPicks]);
}

/** 按识别出的项目名 / 域名 / 仓库匹配一个已有的 Pages 项目，并据此推断部署方式。 */
function matchProjectOn(x: ImportFields, preview: SiteImportPreview | null, projects: PagesProject[], source: string, repoName: string) {
  if (!preview || !projects.length) return;
  const d = preview.detected;
  if (x.mode === 'workers' || x.mode === 'vps' || d.deploy_kind === 'workers' || d.deploy_kind === 'vps') return; // 不在 Pages 项目列表里
  const repo = source === 'github' ? repoName : d.github_repo;
  const hit = projects.find((p) => p.name === (x.project || d.cf_project))
    ?? projects.find((p) => !!x.domain && p.domains.includes(x.domain))
    ?? projects.find((p) => !!repo && p.source_repo === repo);
  if (!hit) return;
  x.project = hit.name;
  if (hit.production_branch) x.branch = hit.production_branch;
  // 绑定了 GitHub、绑的就是这个站点的仓库（GitHub 来源，或本地文件夹的 origin）：推送即上线
  x.mode = hit.source_type === 'github' && !!repo && hit.source_repo === repo ? 'pages_git' : 'direct';
}
function matchProject() {
  const f = sites.imp;
  matchProjectOn(f, f.preview, f.projects, f.source, f.repo);
  for (const it of f.batch) if (it.status === 'ready') matchProjectOn(it, it.preview, f.projects, f.source, it.key);
}

/** 发布脚本里写的 user@host → 主机相同的 SSH 连接（用户名也相同的优先）。 */
function pickSshConn(target: string): string {
  if (!target) return '';
  const [user, host] = target.split('@');
  const ssh = app.conns.filter((c) => c.kind === 'ssh' && c.ssh_host === host);
  return (ssh.find((c) => c.ssh_user === user) ?? ssh[0])?.id ?? '';
}

/** 识别结果 → 可编辑的字段（部署方式按识别结果和有没有 Cloudflare 连接推断）。 */
function fieldsFrom(preview: SiteImportPreview, cfConnId: string): ImportFields {
  const d = preview.detected;
  return {
    name: preview.name, domain: d.domain, install: d.build.install, build: d.build.build,
    outDir: d.build.out_dir, root: d.build.root ?? '', checks: d.build.checks.join('\n'), project: d.cf_project, branch: d.branch || 'main',
    indexnowKey: d.indexnow_key, mode: d.deploy_kind === 'vps' ? 'vps' : cfConnId ? (d.deploy_kind === 'workers' ? 'workers' : 'direct') : 'none',
    command: d.deploy_command, sshConnId: pickSshConn(d.deploy_target), keepEnv: true,
  };
}

async function detectOne(source: string, ghConnId: string, key: string): Promise<SiteImportPreview> {
  return source === 'github'
    ? await invoke<SiteImportPreview>('detect_site_repo', { connId: ghConnId, repo: key })
    : await invoke<SiteImportPreview>('detect_site_folder', { path: key });
}

export async function detectImport(arg: { path?: string }) {
  const f = sites.imp;
  f.busy = true; f.err = '';
  try {
    const preview = await detectOne(f.source, f.ghConnId, f.source === 'github' ? f.repo : arg.path ?? '');
    Object.assign(f, { preview, step: 'review', ...fieldsFrom(preview, f.cfConnId) });
    matchProject();
  } catch (e) { f.err = String(e); }
  finally { f.busy = false; }
}

/** 批量：每个文件夹 / 仓库各识别一遍（GitHub 要先克隆，并发少一点），识别完就能勾选导入。 */
async function startBatch(keys: string[]) {
  const f = sites.imp;
  f.err = ''; f.step = 'batch';
  f.batch = [...new Set(keys)].map((key) => ({
    key, preview: null, status: 'detecting', err: '', note: '', dup: '', pick: false,
    name: '', domain: '', install: '', build: '', outDir: '', root: '', checks: '', mode: 'none', sshConnId: '', command: '', project: '', branch: 'main', indexnowKey: '', keepEnv: true,
  }));
  const items = f.batch;
  const { source, ghConnId, cfConnId } = f;
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const it = items[next++];
      try {
        const preview = await detectOne(source, ghConnId, it.key);
        Object.assign(it, fieldsFrom(preview, cfConnId), { preview });
        if (it.mode === 'vps' && !it.sshConnId) { it.mode = 'none'; it.note = '发布脚本里的服务器没有对应的远程连接，先按「暂不部署」导入，之后在设置里配'; }
        matchProjectOn(it, preview, sites.imp.projects, source, it.key);
        const same = sites.list.find((s) => source === 'github' ? s.source === 'github' && s.repo === it.key : s.local_dir === preview.dir);
        it.dup = same?.name ?? '';
        it.pick = !same;
        it.status = 'ready';
      } catch (e) { it.status = 'error'; it.err = String(e); }
    }
  };
  await Promise.all(Array.from({ length: Math.min(source === 'github' ? 2 : 4, items.length) }, worker));
}

function importReq(x: ImportFields, p: SiteImportPreview, source: string, ghConnId: string, repoName: string, repos: GhRepo[]) {
  const d = p.detected;
  const cf = x.mode === 'none' || x.mode === 'vps';
  return {
    source,
    local_dir: source === 'local' ? p.dir : '',
    gh_conn_id: source === 'github' ? ghConnId : '',
    repo: source === 'github' ? repoName : d.github_repo,
    name: x.name,
    domain: x.domain,
    framework: d.framework,
    default_branch: source === 'github' ? (repos.find((r) => r.full_name === repoName)?.default_branch ?? d.git_default_branch) : (d.git_default_branch || d.git_branch),
    build: { root: x.root.trim(), install: x.install.trim(), build: x.build.trim(), out_dir: x.outDir.trim(), checks: x.checks.split('\n').map((c) => c.trim()).filter(Boolean), env_keys: [] },
    deploy: { mode: x.mode, cf_conn_id: cf ? '' : sites.imp.cfConnId, project: cf ? '' : x.project.trim(), branch: x.branch.trim(), ssh_conn_id: x.mode === 'vps' ? x.sshConnId : '', command: x.mode === 'vps' ? x.command.trim() : '' },
    env: x.keepEnv ? d.env : [],
    indexnow_key: x.indexnowKey.trim(),
  };
}

export async function confirmImport() {
  const f = sites.imp;
  const p = f.preview;
  if (!p) return;
  f.busy = true; f.err = '';
  try {
    const site = await invoke<Site>('import_site', { req: importReq(f, p, f.source, f.ghConnId, f.repo, f.repos) });
    f.busy = false;
    sites.imp = blankImport();
    await refreshSites();
    say(`已导入站点「${site.name}」`);
    openSite(site.id);
  } catch (e) { f.err = String(e); f.busy = false; }
}

/** 批量导入勾选的那些：一个个导，失败的留在清单里显示原因，全部成功就关掉。 */
export async function confirmBatchImport() {
  const f = sites.imp;
  const todo = f.batch.filter((it) => it.pick && it.status === 'ready' && it.preview);
  if (!todo.length) return;
  f.busy = true; f.err = '';
  let ok = 0;
  for (const it of todo) {
    try {
      await invoke<Site>('import_site', { req: importReq(it, it.preview!, f.source, f.ghConnId, it.key, f.repos) });
      it.status = 'done'; it.pick = false; ok++;
    } catch (e) { it.err = String(e); }
  }
  f.busy = false;
  await refreshSites();
  const failed = todo.length - ok;
  if (!failed) { sites.imp = blankImport(); say(`已导入 ${ok} 个站点`); }
  else say(ok ? `已导入 ${ok} 个，${failed} 个没导进去，原因见清单` : '都没导进去，原因见清单', 'err');
}

// ---------- 设置 ----------
export function editSite(s: Site) {
  sites.draft = JSON.parse(JSON.stringify(s));
  void invoke<string[]>('site_env_keys', { id: s.id }).then((k) => (sites.envKeys = k)).catch(() => (sites.envKeys = []));
}
export async function saveSiteDraft(): Promise<boolean> {
  const d = sites.draft;
  if (!d) return false;
  sites.saving = true;
  try {
    const saved = await invoke<Site>('save_site', { site: d });
    sites.list = sites.list.map((s) => (s.id === saved.id ? saved : s));
    sites.draft = JSON.parse(JSON.stringify(saved));
    say('站点设置已保存');
    return true;
  } catch (e) { say(String(e), 'err'); return false; }
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
/** 下线并删除：从网上撤下来（Cloudflare Pages 项目、域名指向；迁移站点先恢复域名），再从 Pilot 移除，可选把文件夹移到废纸篓。 */
export async function takedownSite(s: Site) {
  const live = s.domain || (s.deploy.project ? `${s.deploy.project}.pages.dev` : '');
  const what: string[] = [];
  if (s.origin === 'migrated' || s.origin === 'migrating') what.push('迁移这边：旧域名的跳转取消、测试域名删掉，正式域名恢复成上线前的样子（旧站不受影响）');
  if ((s.deploy.mode === 'direct' || s.deploy.mode === 'pages_git') && s.deploy.project) what.push(`Cloudflare：解除自定义域名、删掉指向它的 DNS 记录，删除 Pages 项目 ${s.deploy.project}`);
  else if (s.deploy.mode === 'workers' || s.deploy.mode === 'vps') what.push(`${s.deploy.mode === 'vps' ? '服务器' : 'Cloudflare Workers'} 上的网站 Pilot 还不能替你删，完成后会告诉你去哪删`);
  what.push('Pilot 里的站点记录、数据缓存、运营记录和站点会话一起删除');
  if (!await confirmDialog(`下线后${live ? ` ${live} 就打不开了，` : ''}这一步不能撤销：\n\n${what.map((w) => `· ${w}`).join('\n')}`, { title: `下线并删除「${s.name}」`, kind: 'danger', confirmText: '下线并删除' })) return;
  const trash = s.source === 'local' && !!s.local_dir && await confirmDialog(`代码文件夹 ${s.local_dir} 要不要一起移到废纸篓？移到废纸篓以后还能从废纸篓找回来。`, { title: '代码文件夹怎么办', kind: 'warning', confirmText: '移到废纸篓', cancelText: '保留文件夹' });
  try {
    say('正在下线…');
    const notes = await invoke<string[]>('site_takedown', { id: s.id, trashFolder: trash });
    await refreshSites();
    await refreshConvos();
    void selectConn('__sites__');
    say(notes.join('\n'), 'ok');
  } catch (e) { say(String(e), 'err'); }
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

// 数据拉完后刷新站点总览（「未接数据」、点击数），短时间内多次只刷一次
let fleetTimer: ReturnType<typeof setTimeout> | undefined;
function loadFleetSoon() {
  clearTimeout(fleetTimer);
  fleetTimer = setTimeout(() => { void import('./fleet.svelte.js').then((m) => m.loadFleet()); }, 1500);
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
    // 每天早上的自动同步：拉完一个站点，开着的看板就刷新
    const un3 = listen<{ site_id: string; errors: string[] }>('site-data', (ev) => {
      const id = ev.payload.site_id;
      if (sites.dash[id]) void loadDashboard(id);
      void loadFleetSoon();
    });
    return () => { void un1.then((f) => f()); void un2.then((f) => f()); void un3.then((f) => f()); };
  });
}
