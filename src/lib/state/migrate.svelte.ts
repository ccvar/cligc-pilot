// 站点迁移：给一个线上站点的域名，盘点 → 确认方案 → 抓取 → AI 重做 → 核对 → 切换 → 跟踪。
// 规则和后台作业在 Rust（site_migrate / migrate_extract / migrate_cmds）；这里只管调用和界面状态。
// 迁移中的站点 origin = 'migrating'、migrate_from = 旧站域名，domain 留空到切换那一刻（预览部署不碰线上域名）。
import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { homeDir } from '@tauri-apps/api/path';
import type { Site } from '$lib/types';
import { app, say, confirmDialog } from './app.svelte.js';
import { connections } from './connections.svelte.js';
import { sites, refreshSites, openSite, deploySite } from './sites.svelte.js';
import { startSiteChat, MAKING_PERM, openMaking } from './siteCreate.svelte.js';
import { startBackgroundChat, continueInBackground } from './turns.svelte.js';
import { makingConv, siteMake, migrateTodos } from './siteMake.svelte.js';
import { brainUsable } from './models';
import { launcher } from './launcher.svelte.js';
import type { Dynamic } from './dynamic.svelte.js';

export interface MGroup { key: string; count: number; ptype: string; clicks: number; samples: string[] }
export interface MFeature { key: string; label: string; pages: number; samples: string[]; plan: string; static_ok: boolean }
export interface MHosting { target: 'cf_static' | 'cf_workers' | 'vps'; label: string; reasons: string[]; paid: boolean; supported: boolean }
export interface MOwnership { cf_conn: string; cf_zone: string; gsc_site: string; txt_token: string; verified: boolean; by: string }
export interface MSummary {
  origin: string; trailing_slash: string; js_rendered: boolean; truncated: boolean; started_at: number; finished_at: number;
  pages: number; feeds: number; files: number; redirects: number; broken: string[]; blocked: number; query: number; duplicates: number; noindex: number; assets: number;
  groups: MGroup[]; top: { path: string; title: string; clicks: number; impressions: number }[]; features: MFeature[]; hosting: MHosting;
  analytics: string[]; generator: string; gsc_used: boolean; cn_share: number | null; ownership: MOwnership; notes: string[];
}
export interface MPlan { drop_groups: string[]; drop_paths: string[]; stack: string; deploy: string; visual: 'keep_brand' | 'fresh'; style: string; features: Record<string, 'keep' | 'drop'>; confirmed_at: number; direction?: string; auto?: boolean }
/** 重做前 AI 出的设计方向小样 */
export interface MDirection { id: string; name: string; idea: string; palette: string[]; fonts: string; shot: string }
export interface MParity { key: string; label: string; level: 'ok' | 'warn' | 'block'; detail: string; items: string[] }
export interface MRec { kind: string; name: string; content: string; proxied: boolean; priority?: number | null; ttl: number }
export interface MDns { at: number; cf_conn: string; zone_id: string; created_zone: boolean; ns: string[]; current_ns: string[]; zone_status: string; snapshot: MRec[]; missing: MRec[] }
export interface MMeta {
  surveyed_at: number; extracted_at: number; rebuild_at: number; parity_at: number; directions_at: number;
  extract: { pages: number; lists: number; assets: number; assets_failed: string[]; low_coverage: string[]; collections: Record<string, number>; large: string[]; comments: number; features: string[] };
  parity: MParity[];
  live: { at: number; base: string; checked: number; bad: string[] };
  /** 正式域名（空＝旧站的域名） */
  formal: string;
  /** 切换前先上线到的测试域名 */
  old_redirect: { at: number; from: string[]; to: string; verified: boolean; note: string };
  staging: { host: string; at: number; status: string; live: { at: number; base: string; checked: number; bad: string[] } };
  dns: MDns;
  cutover: { at: number; host: string; zone_id: string; cf_conn: string; manual_dns: boolean; before: MRec[]; notes: string[]; rolled_back_at: number };
  track: { at: number; checked: number; bad: string[]; base_clicks: number; after_clicks: number; after_days: number; sitemap_submitted: boolean; indexnow_submitted: boolean; notes: string[] };
}
export type MStage = 'survey' | 'plan' | 'extract' | 'rebuild' | 'verify' | 'cutover' | 'track' | 'done';
export interface MVps { slug: string; root: string; host: string; alias: string; ip: string; os: string; arch: string; api_port: number; https: boolean; https_at: number; setup_at: number; releases: string[]; current: string; notes: string[] }
export interface MVpsDns { host: string; ip: string; host_ips: string[]; alias: string; alias_ips: string[]; points: boolean; alias_points: boolean; cf_auto: boolean }
export interface MView { stage: MStage; domain: string; survey: MSummary | null; plan: MPlan | null; meta: MMeta; preview: string; deploy_ready: boolean; dynamic: Dynamic | null; hosting: MHosting | null; vps: MVps | null; origin_host: string; formal: string }

export const STAGES: { key: MStage; label: string }[] = [
  { key: 'survey', label: '盘点' },
  { key: 'plan', label: '确认方案' },
  { key: 'extract', label: '抓取' },
  { key: 'rebuild', label: '重做' },
  { key: 'verify', label: '核对' },
  { key: 'cutover', label: '正式上线' },
  { key: 'track', label: '上线后观察' },
];

const PARENT_KEY = 'cligc.pilot.newSiteParent';

export const migrate = $state({
  view: {} as Record<string, MView>,
  loading: {} as Record<string, boolean>,
  /** 正在做的操作（按站点）：plan / dns / cutover / rollback / own …（同时有几件在做时显示最新那件，全部做完才清空） */
  busy: {} as Record<string, string>,
  dialog: { open: false, multi: false, domain: '', domains: '', parent: '', busy: false, err: '', results: [] as { domain: string; site_id: string; error: string }[] },
});

export function isMigrating(s: Site | undefined): boolean { return !!s && s.origin === 'migrating'; }
/** 概览里显示迁移卡片：迁移中，或者切换后还在跟踪期 */
export function showMigrateCard(s: Site | undefined): boolean { return !!s && (s.origin === 'migrating' || s.origin === 'migrated'); }

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

/** 第一次用、又没有别的站点可参照：默认放在家目录下的 Sites（不存在会自动建） */
let homeSites = '';
void homeDir().then((h) => { homeSites = `${h.replace(/[\\/]+$/, '')}/Sites`; }).catch(() => {});

export function openMigrate(multi = false) {
  connections.switcherOpen = false;
  migrate.dialog = { open: true, multi, domain: '', domains: '', parent: defaultParent(), busy: false, err: '', results: [] };
}
export function closeMigrate() { if (!migrate.dialog.busy) migrate.dialog.open = false; }

export async function pickMigrateParent() {
  const picked = await open({ title: '改版的站点放在哪个文件夹下', directory: true, multiple: false, defaultPath: migrate.dialog.parent || undefined });
  if (typeof picked === 'string' && picked.trim()) migrate.dialog.parent = picked;
}

/** 建迁移站点：后端先确认旧站打得开，建好文件夹和站点后自动开始盘点。 */
export async function createMigration() {
  const d = migrate.dialog;
  if (d.busy) return;
  d.busy = true; d.err = '';
  try {
    const site = await invoke<Site>('create_migration', { req: { domain: d.domain.trim(), parent_dir: d.parent } });
    try { localStorage.setItem(PARENT_KEY, d.parent); } catch { /* */ }
    d.busy = false; d.open = false;
    await refreshSites();
    openSite(site.id, 'overview');
    void loadMigrate(site.id);
  } catch (e) { d.err = String(e); }
  finally { d.busy = false; }
}

/** 批量：每行一个域名（逗号、空格分隔也行）。建好后排队盘点，结果留在弹窗里。 */
export async function createMigrations() {
  const d = migrate.dialog;
  if (d.busy) return;
  const domains = d.domains.split(/[\s,，;；]+/).map((x) => x.trim()).filter(Boolean);
  if (!domains.length) { d.err = '至少填一个域名'; return; }
  d.busy = true; d.err = ''; d.results = [];
  try {
    d.results = await invoke<{ domain: string; site_id: string; error: string }[]>('create_migrations', { domains, parentDir: d.parent });
    try { localStorage.setItem(PARENT_KEY, d.parent); } catch { /* */ }
    await refreshSites();
    for (const r of d.results) if (r.site_id) void loadMigrate(r.site_id);
    const ok = d.results.filter((r) => r.site_id).length;
    say(`建好 ${ok} 个改版站点，正在排队盘点${d.results.length > ok ? `；${d.results.length - ok} 个没建成` : ''}`, ok ? 'ok' : 'err');
    if (ok && d.results.every((r) => r.site_id)) { d.open = false; openMaking(); }
  } catch (e) { d.err = String(e); }
  finally { d.busy = false; }
}

export async function loadMigrate(id: string) {
  if (untrack(() => migrate.loading[id])) return;
  untrack(() => { migrate.loading[id] = true; });
  try { migrate.view[id] = await invoke<MView>('migrate_state', { id }); }
  catch { /* 不是迁移站点 */ }
  finally { migrate.loading[id] = false; }
}

// 一个站点可能同时在做几件事（比如后台自动推进和你手点的）：按栈记着，先结束的那件不能把别人的「忙」标志清掉
const busyOps: Record<string, string[]> = {};
function beginBusy(id: string, label: string) {
  (busyOps[id] ??= []).push(label);
  migrate.busy[id] = label;
}
function endBusy(id: string, label: string) {
  const ops = busyOps[id] ?? [];
  const i = ops.lastIndexOf(label);
  if (i >= 0) ops.splice(i, 1);
  migrate.busy[id] = ops.length ? ops[ops.length - 1] : '';
}

async function run(id: string, busy: string, f: () => Promise<unknown>, ok?: string) {
  beginBusy(id, busy);
  try { await f(); if (ok) say(ok, 'ok'); }
  catch (e) { say(String(e), 'err'); }
  finally { endBusy(id, busy); void loadMigrate(id); }
}

export const resurvey = (id: string) => run(id, 'survey', () => invoke('migrate_survey', { id }));

/** 确认方案并直接开始抓取。 */
export async function confirmPlan(id: string, plan: Pick<MPlan, 'drop_groups' | 'drop_paths' | 'visual' | 'style' | 'features' | 'deploy' | 'auto'>) {
  await run(id, 'plan', async () => {
    await invoke('migrate_save_plan', { id, plan: { ...plan, stack: 'astro', confirmed_at: 0 } });
    await invoke('migrate_extract', { id });
  });
}
/** 按「保留 / 去掉」预览部署去向（确认方案前）。 */
export async function previewHosting(id: string, features: Record<string, string>): Promise<MHosting | null> {
  try { return await invoke<MHosting>('migrate_hosting', { id, features }); } catch { return null; }
}

/** 建动态功能的资源（数据库、人机验证、R2 大文件、绑定到 Pages 项目）。 */
export const setupDynamic = (id: string) => run(id, 'dynamic', () => invoke('site_dynamic_setup', { id }));

/** 自己的服务器：选好 SSH 连接、配置服务器（装 Caddy，有表单评论时装 pilot-site）。 */
export const setupVps = (id: string, sshConnId: string) => run(id, 'vps', () => invoke('site_vps_setup', { id, sshConnId }));

/** 自己的服务器：域名现在解析到哪、和服务器 IP 对不对得上。 */
export const vpsDns = $state({} as Record<string, MVpsDns | undefined>);
export async function checkVpsDns(id: string) {
  beginBusy(id, 'vpsdns');
  try { vpsDns[id] = await invoke<MVpsDns>('migrate_vps_dns', { id }); }
  catch (e) { say(String(e), 'err'); }
  finally { endBusy(id, 'vpsdns'); }
}

export const extract = (id: string) => run(id, 'extract', () => invoke('migrate_extract', { id }));

/** 重做（开始 / 继续 / 修核对出的问题）：都在后台跑，不跳走；进度在迁移卡片的「重做」这一步，想看再打开对话。
 *  已经有重做对话就接着那一条说（AI 看得到之前做过的全部），第一次才开新对话；没有可用模型时才去启动页选。
 *  全自动档：在 Pilot 新建的本地文件夹里干活，拿不到部署凭据。 */
export async function startRebuild(site: Site) {
  try {
    const text = await invoke<string>('migrate_rebuild_message', { id: site.id });
    await sendMaking(site, text, 'AI 在后台开始重做了，进度看「重做」这一步', 'AI 在后台接着重做，进度看「重做」这一步', '重做要求写好了：选一个可用的模型后发送，就开始重做');
  } catch (e) { say(String(e), 'err'); }
  finally { void loadMigrate(site.id); }
}

/** 交给重做那条对话（在后台跑）：有就接着说，没有才开新的；没有可用模型时才去启动页选。 */
async function sendMaking(site: Site, text: string, startMsg: string, continueMsg: string, pickModelMsg: string) {
  const conv = makingConv(site.id);
  const reuse = !!conv && /^(请(迁移|改版)重做|继续(迁移|改版)重做)/.test(conv.title) && conv.status !== 'running' && !app.running[conv.id];
  if (!reuse && !brainUsable(launcher.lBrain)) {
    await startSiteChat(site.id, text, pickModelMsg, MAKING_PERM);
    return;
  }
  say(reuse ? continueMsg : startMsg, 'ok');
  const r = reuse ? continueInBackground(conv!.id, text) : startBackgroundChat({ siteId: site.id, siteName: site.name, text, perm: MAKING_PERM });
  void r.finally(() => { void loadMigrate(site.id); void loadDirections(site.id); });
}

// ───────── 重做前：挑设计方向 ─────────

export const directions = $state({} as Record<string, MDirection[]>);

export async function loadDirections(id: string) {
  try { directions[id] = await invoke<MDirection[]>('migrate_directions', { id }); } catch { /* 还没有 */ }
}

/** 让 AI 先画 3 个首页小样（feedback：都不满意时的意见，AI 照着重出） */
export async function askDirections(site: Site, feedback = '') {
  try {
    const text = await invoke<string>('migrate_directions_message', { id: site.id, feedback: feedback.trim() || null });
    await sendMaking(site, text, 'AI 在后台画 3 个设计方向，好了这里会出现', 'AI 按你的意见重新画 3 个方向', '要求写好了：选一个可用的模型后发送，AI 就开始画设计方向');
  } catch (e) { say(String(e), 'err'); }
  finally { void loadMigrate(site.id); }
}

/** 挑定方向（auto＝跳过，让 AI 自己定），然后开始重做 */
export async function pickDirection(site: Site, direction: string) {
  try {
    await invoke('migrate_pick_direction', { id: site.id, direction });
    await loadMigrate(site.id);
    await startRebuild(site);
  } catch (e) { say(String(e), 'err'); }
}

export async function openDirection(id: string, direction: string) {
  try { await invoke('migrate_direction_open', { id, direction }); } catch (e) { say(String(e), 'err'); }
}

export const verify = (id: string) => run(id, 'verify', () => invoke('migrate_verify', { id }));
/** 已经上线过：只实测每个旧地址（上线成功后 Pilot 也会自动测一次） */
export const liveCheck = (id: string) => run(id, 'live', () => invoke('migrate_live', { id }));
export const checkOwnership = (id: string) => run(id, 'own', () => invoke('migrate_ownership', { id }));
export const prepareDns = (id: string, create = false) => run(id, 'dns', () => invoke('migrate_dns_prepare', { id, create }));

/** 切换这一步自动检查 DNS：不弹提示，失败了界面上照样能手动查 */
export async function prepareDnsQuiet(id: string) {
  try { await invoke('migrate_dns_prepare', { id, create: false }); } catch { /* 界面上有「检查 DNS」 */ }
  void loadMigrate(id);
}

export async function addToCloudflare(id: string, domain: string) {
  if (!await confirmDialog(`会用站点的 Cloudflare 连接把 ${domain} 加进 Cloudflare，对照现在的 DNS 记录，缺的（邮件等）照原样补上。之后只剩一件事要你做：去域名注册商把名称服务器改成 Cloudflare 给的那两个——改之前网站和邮件都不受影响。`, { title: '把域名接进 Cloudflare', kind: 'info', confirmText: '加进 Cloudflare' })) return;
  await run(id, 'dns', async () => {
    const p = await invoke<{ missing: unknown[] }>('migrate_dns_prepare', { id, create: true });
    if (p.missing.length) { await invoke('migrate_dns_fix', { id }); say(`已加进 Cloudflare，补上了 ${p.missing.length} 条原有记录`, 'ok'); }
  });
}

export async function fixDns(id: string, n: number) {
  if (!await confirmDialog(`把 ${n} 条原有记录补进 Cloudflare（照原样，不开代理）。补齐后再改名称服务器，邮件和其它子域名就不会断。`, { title: '补上缺的 DNS 记录', kind: 'info', confirmText: '补上' })) return;
  await run(id, 'dns', () => invoke('migrate_dns_fix', { id }), 'DNS 记录已补上');
}

/** 一个域名现在有没有在用（改正式域名、正式上线前给你看） */
export interface HostUse { host: string; in_cf: boolean; records: string[]; ours: boolean; other_pages: string; status: number; title: string; old_site: boolean }

export async function hostUse(id: string, host: string): Promise<HostUse | null> {
  try { return await invoke<HostUse>('migrate_host_use', { id, host }); } catch { return null; }
}

/** 「现在打开它是什么」的一句话 */
export function useText(u: HostUse): string {
  if (u.ours) return `${u.host} 已经指向这个新站`;
  if (!u.records.length) return `${u.host} 现在没在用`;
  if (u.old_site) return `${u.host} 现在打开的是旧站`;
  const what = u.other_pages ? `另一个 Cloudflare Pages 项目（${u.other_pages}）` : u.status ? (u.title ? `「${u.title}」` : `一个网站（返回 ${u.status}）`) : '别的服务（现在打不开）';
  return `${u.host} 正在用：打开是${what}`;
}

export async function cutover(id: string, host: string, vps = false, cfAuto = false, oldHost = '', oldOwned = true) {
  const u = await hostUse(id, host);
  const moved = !!oldHost && host.replace(/^www\./, '') !== oldHost.replace(/^www\./, '');
  const now = !u ? '' : u.ours ? '' : !u.records.length ? `${host} 现在没在用，` : u.old_site ? `现在打开 ${host} 看到的是旧站，` : `注意：${useText(u)}，上线后那里原来的内容就打不开了。`;
  const how = vps
    ? `${now}上线后 ${host} 会打开你服务器上的新站，Pilot 会${cfAuto ? '自动改好域名指向，并' : ''}自动申请免费的 HTTPS 证书（Let's Encrypt）。`
    : `${now}上线后打开 ${host} 就是新站。`;
  const back = u && !u.records.length ? '' : '原来的设置已经存好，出问题点「退回旧站」就能恢复。';
  const keep = !moved ? '旧站的服务器先别关，新站稳定后（建议两周）再停。' : oldOwned ? `旧域名 ${oldHost} 还是打开旧站，等新站稳定了再让它跳转到新域名。` : `旧站 ${oldHost} 不受影响。`;
  if (!await confirmDialog(`${how}${back}${keep}`, { title: `正式上线到 ${host}`, kind: u && u.records.length && !u.old_site && !u.ours ? 'danger' : 'warning', confirmText: '上线' })) return;
  await run(id, 'cutover', async () => { await invoke('migrate_cutover', { id }); await refreshSites(); }, `已正式上线到 ${host}，一般几分钟内生效`);
}

/** 改正式域名（空＝恢复成旧站的域名）。先查这个域名有没有在用，在用的要你确认 */
export async function setFormal(id: string, host: string, old: string, oldOwned = true) {
  const h = host.trim().replace(/^https?:\/\//, '').replace(/\/.*$/, '').toLowerCase();
  if (h && h.replace(/^www\./, '') !== old.replace(/^www\./, '')) {
    beginBusy(id, 'formal');
    const u = await hostUse(id, h);
    endBusy(id, 'formal');
    if (!u) { say(`「${h}」不是有效的域名`, 'err'); return false; }
    const inUse = u.records.length > 0 && !u.ours;
    const seo = oldOwned
      ? `换了正式域名，搜索引擎会把新站算在 ${h} 上；新站稳定后记得让旧域名 ${old} 跳转到新域名，原来的搜索排名才能带过来。`
      : `旧站 ${old} 不在你的账号里，没法让它跳转过来：搜索排名要在 ${h} 上重新积累。`;
    let title: string, lines: string[], ok: string;
    if (!u.in_cf) {
      // 不在你的 Cloudflare 账号里：可能是你在别处的域名，也可能是别人的
      title = `${h} 是你的域名吗？`;
      lines = [`它不在你的 Cloudflare 账号里${u.records.length ? `，而且${useText(u).replace(`${h} `, '')}` : ''}。`, `是你的：接着往下，上线前按提示把它接进 Cloudflare（要去买域名的地方改两项名称服务器）。不是你的：没法上线到它，换一个你自己的域名。`, seo];
      ok = '是我的，继续';
    } else if (inUse) {
      title = `${h} 正在使用中`;
      lines = [`${h} 在你的 Cloudflare 账号里，现在${useText(u).replace(`${h} 正在用：`, '')}。`, `用它当正式域名，上线时它会改为打开新站，那里原来的内容就打不开了（设置会存好，能退回）。`, seo];
      ok = '仍然用它';
    } else {
      title = `正式域名改成 ${h}`;
      lines = [`${h} 在你的 Cloudflare 账号里，现在没在用，可以直接用。`, seo];
      ok = '改成新域名';
    }
    if (!await confirmDialog(lines.join('\n\n'), { title, kind: inUse || !u.in_cf ? 'danger' : 'warning', confirmText: ok, cancelText: '换一个' })) return false;
  }
  await run(id, 'formal', () => invoke('migrate_set_formal', { id, host: h }), h ? `正式域名改成了 ${h}` : '正式域名恢复成旧站的域名');
  return true;
}

/** 先部署到测试域名：绑到 Pages 项目、加一条 CNAME，页面不让收录 */
export async function stagingUp(id: string, host: string) {
  const h = host.trim().replace(/^https?:\/\//, '').replace(/\/.*$/, '').toLowerCase();
  if (!h) return;
  if (!await confirmDialog(`在 ${h} 上打开新站给你看，几分钟后生效。页面不让搜索引擎收录，旧站不受影响。${h} 要是已经在用，Pilot 不会动它，会提示你换一个。`, { title: '部署到测试域名', kind: 'info', confirmText: '部署' })) return;
  await run(id, 'staging', () => invoke('migrate_staging', { id, host: h }), `已部署到 ${h}，证书签好一般要几分钟，好了会自动实测`);
}

export async function stagingRemove(id: string, host: string) {
  if (!await confirmDialog(`${host} 不再打开新站（Pilot 加的设置会删掉）。正式域名和旧站都不受影响。`, { title: '删除测试域名', kind: 'danger', confirmText: '删除' })) return;
  await run(id, 'staging', async () => { say(await invoke<string>('migrate_staging_remove', { id }), 'ok'); });
}

export const stagingCheck = (id: string) => run(id, 'staging', () => invoke('migrate_staging_check', { id }));

/** 测试域名还在生效中：问一次（生效了后台会自动实测） */
export async function stagingStatus(id: string) {
  try { const st = await invoke<string>('migrate_staging_status', { id }); if (st === 'active') void loadMigrate(id); return st; }
  catch { return ''; }
}


/** 让旧域名跳到新域名（换了域名上线以后） */
export async function oldRedirect(id: string, old: string, to: string) {
  if (!await confirmDialog(`之后有人打开 ${old} 的任何地址，都会被带到 ${to} 的同一地址，旧站就看不到了。搜索引擎会把 ${old} 积累的排名慢慢转给 ${to}。\n\n建议新站稳定了再做；想反悔随时能取消。`, { title: `让 ${old} 跳到新站`, kind: 'warning', confirmText: '设置跳转' })) return;
  await run(id, 'redirect', async () => {
    const r = await invoke<{ verified: boolean; note: string }>('migrate_old_redirect', { id });
    say(r.verified ? `${old} 已经跳到 ${to}，实测通过` : `跳转已设置，还在生效中${r.note ? `（${r.note}）` : ''}，过几分钟再看`, r.verified ? 'ok' : undefined);
  });
}

export async function oldRedirectUndo(id: string, old: string) {
  if (!await confirmDialog(`${old} 不再跳转，又会打开旧站。`, { title: '取消跳转', kind: 'warning', confirmText: '取消跳转', cancelText: '先不' })) return;
  await run(id, 'redirect', async () => { say(await invoke<string>('migrate_old_redirect_undo', { id }), 'ok'); });
}

export async function rollback(id: string, host: string, moved = false) {
  const msg = moved
    ? `${host} 恢复成上线前的样子，不再打开新站；旧域名要是设置了跳转，也会一起取消。新站和抓好的内容都保留，改好后可以再上线。`
    : `${host} 恢复成上线前的设置，打开的又会是旧站。新站和抓好的内容都保留，改好后可以再上线。`;
  if (!await confirmDialog(msg, { title: moved ? `撤下 ${host} 上的新站` : '退回旧站', kind: 'danger', confirmText: moved ? '撤下' : '退回旧站' })) return;
  await run(id, 'rollback', async () => { const m = await invoke<string>('migrate_rollback', { id }); say(m, 'ok'); await refreshSites(); });
}

export const track = (id: string) => run(id, 'track', () => invoke('migrate_track', { id }));

// ───────── 列表卡片上的「继续」：当前这一步的主操作 ─────────

export interface MNext {
  /** 状态一句话 */
  text: string;
  tone: '' | 'live' | 'warn' | 'ok';
  /** 主按钮（没有＝正在跑，或者没什么可做） */
  label?: string;
  icon?: string;
  run?: () => void;
}

/** 上一轮是不是出错停下的（卡住、连续失败、网络……） */
function aiStopped(siteId: string): boolean {
  const conv = makingConv(siteId);
  const last = conv?.messages?.[conv.messages.length - 1];
  return !!last && last.role !== 'user' && !!last.error;
}

/** AI 留下的待办（站点概览的迁移卡片里处理） */
function todoNote(siteId: string): string {
  const n = migrateTodos(siteMake.state[siteId]?.todos).length;
  return n ? ` · ${n} 件事要你处理` : '';
}

/** 站点制作列表的迁移卡片：现在走到哪、点「继续」做什么。要你做选择的步骤（确认方案、配置部署、切换）只打开站点。 */
export function migrateNext(site: Site): MNext {
  const v = migrate.view[site.id];
  const job = sites.jobs[site.id];
  const open = (label: string): Pick<MNext, 'label' | 'icon' | 'run'> => ({ label, icon: 'checks', run: () => openSite(site.id, 'overview') });
  if (!v) return { text: '读取中…', tone: '' };
  if (job?.status === 'running') return { text: `正在${job.title}…`, tone: 'live' };
  if (aiRunningFor(site.id)) return { text: v.stage === 'rebuild' && !v.meta.rebuild_at ? 'AI 正在画设计方向…' : 'AI 正在重做…', tone: 'live' };
  const failed = job?.status === 'failed' && job.finished_at > Date.now() / 1000 - 3600;
  switch (v.stage) {
    case 'survey':
      return { text: failed ? `盘点没完成：${job!.error.split('\n')[0]}` : '还没盘点', tone: failed ? 'warn' : '', label: '开始盘点', icon: 'play', run: () => void resurvey(site.id) };
    case 'plan':
      return { text: '盘点好了，等你确认方案', tone: '', ...open('去确认方案') };
    case 'extract':
      return { text: failed ? '抓取没完成，可以再试一次' : '方案已确认，等抓取内容', tone: failed ? 'warn' : '', label: '开始抓取', icon: 'play', run: () => void extract(site.id) };
    case 'rebuild':
      if (!v.meta.rebuild_at && !v.plan?.direction) {
        if (v.meta.directions_at && (directions[site.id]?.length ?? 0) > 0) return { text: '3 个设计方向画好了，等你挑', tone: 'ok', ...open('去挑方向') };
        return { text: '内容已抓好，先让 AI 画设计方向', tone: '', label: '画设计方向', icon: 'wand', run: () => void askDirections(site) };
      }
      if (!v.meta.rebuild_at) return { text: '设计方向选好了，等 AI 重做', tone: '', label: '开始重做', icon: 'wand', run: () => void startRebuild(site) };
      if (aiStopped(site.id)) return { text: 'AI 停下了，可以继续', tone: 'warn', label: '继续重做', icon: 'play', run: () => void startRebuild(site) };
      return { text: `AI 做完了，等核对${todoNote(site.id)}`, tone: '', label: '核对', icon: 'checks', run: () => void verify(site.id) };
    case 'verify': {
      const blocks = v.meta.parity.some((p) => p.level === 'block');
      if (blocks || v.meta.live.bad.length) return { text: '核对有问题要修', tone: 'warn', label: '让 AI 修', icon: 'wand', run: () => void startRebuild(site) };
      if (!v.deploy_ready) return { text: `核对通过，等你配置${v.plan?.deploy === 'vps' ? '服务器' : '预览地址'}`, tone: '', ...open('去配置') };
      if (v.dynamic?.features.length && !v.dynamic.setup_at && v.plan?.deploy !== 'vps') return { text: '等你配置动态功能', tone: '', ...open('去配置') };
      if (!v.meta.live.at && sites.status[site.id]?.last_deploy?.ok) return { text: `已${v.plan?.deploy === 'vps' ? '发布' : '部署'}，等实测每个旧地址`, tone: '', label: '实测', icon: 'checks', run: () => void liveCheck(site.id) };
      if (!v.meta.live.at) return { text: `等${v.plan?.deploy === 'vps' ? '发布到服务器' : '部署到预览地址'}实测`, tone: '', label: v.plan?.deploy === 'vps' ? '发布到服务器' : '部署到预览', icon: 'rocket', run: () => void deploySite(site.id) };
      return { text: '核对中', tone: '', ...open('查看') };
    }
    case 'cutover':
      return { text: '核对通过，可以正式上线了', tone: 'ok', ...open('去上线') };
    case 'track':
      return { text: '已正式上线，观察中', tone: 'ok' };
    default:
      return { text: '改版完成', tone: 'ok' };
  }
}

// ───────── 自动推进：单个站点（方案里勾了）或者批量迁移打开了「自动推进」 ─────────

/** 自动推进：抓取 → AI 画设计方向（等你挑）→ AI 重做 → 核对 → 没通过让 AI 修（最多 2 轮）
 *  → 建 Cloudflare 预览项目（只有一个连接时）→ 配动态功能 → 部署到预览 → 实测。
 *  不会替你：确认方案、挑设计方向、配置服务器、正式上线。
 *  batch.auto：批量改版的总开关；单个站点看方案里的 auto。 */
export const batch = $state({
  auto: false,
  aiMax: 1,
  deployed: {} as Record<string, number>,
  note: '',
  /** 自动推进停下来在等你的事（按站点） */
  waiting: {} as Record<string, string>,
  /** 这一轮会话里自动让 AI 修 / 接着做了几次（防止来回打转） */
  fixes: {} as Record<string, number>,
});

export function migratingSites(): Site[] { return sites.list.filter((s) => s.origin === 'migrating'); }

/** 默认方案：全部页面组保留、保留品牌升级设计、表单评论保留、登录购物车去掉；访客主要在大陆时部署到服务器。 */
function defaultPlan(v: MView) {
  const s = v.survey!;
  const features = Object.fromEntries(s.features.filter((f) => f.key !== 'search').map((f) => [f.key, f.key === 'login' || f.key === 'cart' ? 'drop' : 'keep'])) as Record<string, 'keep' | 'drop'>;
  return { drop_groups: [], drop_paths: [], visual: 'keep_brand' as const, style: '', features, deploy: ((s.cn_share ?? 0) >= 0.5 ? 'vps' : 'direct') as 'vps' | 'direct', auto: true };
}

export async function confirmAllDefault() {
  const list = migratingSites().filter((s) => migrate.view[s.id]?.stage === 'plan' && migrate.view[s.id]?.survey);
  if (!list.length) { say('没有等确认方案的站点'); return; }
  if (!await confirmDialog(`按默认方案确认 ${list.length} 个站点并开始抓取：页面全部保留、保留品牌升级设计、表单和评论保留、登录和购物车去掉；访客主要在中国大陆的部署到自己的服务器，其余部署到 Cloudflare。确认后自动往下做，到挑设计方向、正式上线时停下来等你。\n\n${list.map((s) => s.migrate_from).join('、')}`, { title: '按默认方案确认', kind: 'info', confirmText: '确认' })) return;
  for (const s of list) await confirmPlan(s.id, defaultPlan(migrate.view[s.id]!));
}

export async function setAuto(id: string, on: boolean) {
  try { await invoke('migrate_set_auto', { id, auto: on }); await loadMigrate(id); if (on) void autoTick(); else delete batch.waiting[id]; }
  catch (e) { say(String(e), 'err'); }
}

const aiRunningFor = (id: string) => app.convos.some((c) => c.conn_id === `site:${id}` && c.status === 'running');

/** 只有一个 Cloudflare 连接时，替你建预览用的 Pages 项目（名字按旧站域名 + -new） */
export async function setupPreviewProject(site: Site, cfConn: string, project: string) {
  beginBusy(site.id, 'pages');
  try {
    await invoke('save_site', { site: { ...site, deploy: { ...site.deploy, mode: 'direct', cf_conn_id: cfConn, project: project.trim(), branch: site.deploy.branch || 'main' } } });
    await refreshSites();
    await invoke('site_setup_pages', { id: site.id });
    say(`已建好预览用的 Cloudflare Pages 项目 ${project.trim()}`, 'ok');
  } catch (e) { say(String(e), 'err'); }
  finally { endBusy(site.id, 'pages'); void loadMigrate(site.id); }
}

export function defaultProject(site: Site): string {
  return (site.deploy.project || (site.migrate_from || site.name).replace(/^www\./, '').split('.')[0]).toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-+|-+$/g, '') + (site.deploy.project ? '' : '-new');
}

/** 往前推一步；返回停下来在等你的事（没有就是空） */
async function advance(s: Site, slots: { ai: number }): Promise<string> {
  const v = migrate.view[s.id];
  if (!v || migrate.busy[s.id] || sites.jobs[s.id]?.status === 'running' || aiRunningFor(s.id)) return '';
  const lastJob = sites.jobs[s.id];
  const justFailed = !!lastJob && lastJob.status === 'failed' && lastJob.finished_at > Date.now() / 1000 - 600;
  const fixes = batch.fixes[s.id] ?? 0;
  switch (v.stage) {
    case 'plan': return '等你确认方案';
    case 'extract':
      if (justFailed && lastJob?.kind === 'migrate:extract') return '抓取没成功，要你看一下';
      await extract(s.id); return '';
    case 'rebuild': {
      if (!v.meta.rebuild_at && !v.plan?.direction) {
        if (v.meta.directions_at) return '等你挑设计方向';
        if (slots.ai <= 0) return '';
        slots.ai -= 1; await askDirections(s); return '';
      }
      if (!v.meta.rebuild_at) { if (slots.ai <= 0) return ''; slots.ai -= 1; await startRebuild(s); return ''; }
      if (aiStopped(s.id)) {
        if (fixes >= 2) return 'AI 停下了好几次，要你看一下';
        if (slots.ai <= 0) return '';
        batch.fixes[s.id] = fixes + 1; slots.ai -= 1; await startRebuild(s); return '';
      }
      if (justFailed && lastJob?.kind === 'migrate:verify') return '核对没跑成，要你看一下';
      await verify(s.id); return '';
    }
    case 'verify': {
      // AI 修过（重做要求比上次核对新）：先重新核对，再决定下一步
      if (v.meta.rebuild_at > v.meta.parity_at) {
        if (justFailed && lastJob?.kind === 'migrate:verify') return '核对没跑成，要你看一下';
        await verify(s.id); return '';
      }
      const blocks = v.meta.parity.some((p) => p.level === 'block');
      if (blocks || v.meta.live.bad.length) {
        if (fixes >= 2) return '核对没通过，AI 修了两轮还有问题，要你看一下';
        if (slots.ai <= 0) return '';
        batch.fixes[s.id] = fixes + 1; slots.ai -= 1; delete batch.deployed[s.id]; await startRebuild(s); return '';
      }
      if (!v.deploy_ready) {
        if (v.plan?.deploy === 'vps') return '等你选服务器';
        const cfs = app.conns.filter((c) => c.kind === 'cloudflare');
        const conn = s.deploy.cf_conn_id || (cfs.length === 1 ? cfs[0].id : '');
        if (!conn) return cfs.length ? '等你选用哪个 Cloudflare 账号' : '等你连接 Cloudflare';
        await setupPreviewProject(s, conn, defaultProject(s)); return '';
      }
      if (v.dynamic?.features.length && !v.dynamic.setup_at && v.plan?.deploy !== 'vps') { await setupDynamic(s.id); return ''; }
      if (!v.meta.live.at) {
        // 部署成功后后台会自己接着实测；过了 3 分钟还没实测结果才补一次
        if (batch.deployed[s.id] && justFailed && lastJob?.kind === 'deploy') { delete batch.deployed[s.id]; return '部署没成功，要你看一下'; }
        if (sites.status[s.id]?.last_deploy?.ok && batch.deployed[s.id] && Date.now() - batch.deployed[s.id] > 180000) { await liveCheck(s.id); return ''; }
        // 文件夹里有未提交的改动时 deploySite 会弹确认框；定时器里不弹，停下来等你手动部署
        if (sites.status[s.id]?.dirty) return '有未提交的改动，等你确认';
        if (!batch.deployed[s.id]) { batch.deployed[s.id] = Date.now(); await deploySite(s.id); }
        return '';
      }
      return '';
    }
    case 'cutover': delete batch.fixes[s.id]; return '核对都通过了，等你正式上线';
    default: return '';
  }
}

let ticking = false;
export async function autoTick() {
  if (ticking) return;
  ticking = true;
  try {
    const all = migratingSites();
    const list = all.filter((s) => batch.auto || migrate.view[s.id]?.plan?.auto);
    if (!list.length) return;
    // 同时在跑的 AI：批量时按上限，单个站点自己的不占别人的名额
    const slots = { ai: Math.max(0, (batch.auto ? batch.aiMax : list.length) - all.filter((s) => aiRunningFor(s.id)).length) };
    for (const s of list) {
      await loadMigrate(s.id);
      const w = await advance(s, slots);
      if (w) batch.waiting[s.id] = w; else delete batch.waiting[s.id];
    }
    batch.note = list.filter((s) => batch.waiting[s.id]).map((s) => `${s.migrate_from} ${batch.waiting[s.id]}`).join('；');
  } finally { ticking = false; }
}
/** 旧名字：批量面板还在用 */
export const batchTick = autoTick;

export function setBatchAuto(on: boolean) {
  batch.auto = on;
  if (on) void autoTick();
  else batch.note = '';
}

/** 迁移作业结束（或者部署到预览地址结束）就刷新迁移状态。 */
export function initMigrate() {
  $effect(() => {
    const un = listen<{ site_id: string; status: string; kind: string; title: string; error: string }>('site-job', (ev) => {
      const j = ev.payload;
      if (j.status === 'running') return;
      const s = sites.list.find((x) => x.id === j.site_id);
      if (!showMigrateCard(s)) return;
      void loadMigrate(j.site_id).then(() => autoTick());
      if (j.kind.startsWith('migrate:')) say(j.status === 'ok' ? `${j.title}完成` : j.status === 'canceled' ? `已停止${j.title}` : `${j.title}没通过：${j.error.split('\n')[0]}`, j.status === 'ok' ? 'ok' : 'err');
    });
    // 自动推进的兜底：每 20 秒看一眼（AI 一轮做完、作业结束都会立刻推，这里防漏）
    const t = setInterval(() => void autoTick(), 20000);
    return () => { void un.then((f) => f()); clearInterval(t); };
  });
  // 迁移站点的 AI 一轮做完：刷新，自动推进接着往下
  let was = new Set<string>();
  $effect(() => {
    const now = new Set(app.convos.filter((c) => c.status === 'running' && c.conn_id.startsWith('site:')).map((c) => c.conn_id.slice(5)));
    const ended = [...was].filter((id) => !now.has(id));
    was = now;
    if (ended.length) untrack(() => { for (const id of ended) if (migrate.view[id]) void loadMigrate(id).then(() => { void loadDirections(id); void autoTick(); }); });
  });
}

/** 切换后的跟踪：打开时超过 12 小时没查过就查一次。 */
export function trackIfStale(id: string) {
  const v = migrate.view[id];
  if (!v || v.stage !== 'track' || migrate.busy[id] || sites.jobs[id]?.status === 'running') return;
  if (Date.now() / 1000 - (v.meta.track.at || 0) > 12 * 3600) void track(id);
}

