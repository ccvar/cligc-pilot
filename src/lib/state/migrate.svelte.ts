// 站点迁移：给一个线上站点的域名，盘点 → 确认方案 → 抓取 → AI 重做 → 核对 → 切换 → 跟踪。
// 规则和后台作业在 Rust（site_migrate / migrate_extract / migrate_cmds）；这里只管调用和界面状态。
// 迁移中的站点 origin = 'migrating'、migrate_from = 旧站域名，domain 留空到切换那一刻（预览部署不碰线上域名）。
import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type { Site } from '$lib/types';
import { say, confirmDialog } from './app.svelte.js';
import { connections } from './connections.svelte.js';
import { sites, refreshSites, openSite } from './sites.svelte.js';
import { startSiteChat, MAKING_PERM } from './siteCreate.svelte.js';
import { sendToMaking } from './siteMake.svelte.js';

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
export interface MPlan { drop_groups: string[]; drop_paths: string[]; stack: string; deploy: string; visual: 'keep_brand' | 'fresh'; style: string; confirmed_at: number }
export interface MParity { key: string; label: string; level: 'ok' | 'warn' | 'block'; detail: string; items: string[] }
export interface MRec { kind: string; name: string; content: string; proxied: boolean; priority?: number | null; ttl: number }
export interface MDns { at: number; cf_conn: string; zone_id: string; created_zone: boolean; ns: string[]; current_ns: string[]; zone_status: string; snapshot: MRec[]; missing: MRec[] }
export interface MMeta {
  surveyed_at: number; extracted_at: number; rebuild_at: number; parity_at: number;
  extract: { pages: number; lists: number; assets: number; assets_failed: string[]; low_coverage: string[]; collections: Record<string, number> };
  parity: MParity[];
  live: { at: number; base: string; checked: number; bad: string[] };
  dns: MDns;
  cutover: { at: number; host: string; zone_id: string; before: MRec[]; notes: string[]; rolled_back_at: number };
  track: { at: number; checked: number; bad: string[]; base_clicks: number; after_clicks: number; after_days: number; sitemap_submitted: boolean; indexnow_submitted: boolean; notes: string[] };
}
export type MStage = 'survey' | 'plan' | 'extract' | 'rebuild' | 'verify' | 'cutover' | 'track' | 'done';
export interface MView { stage: MStage; domain: string; survey: MSummary | null; plan: MPlan | null; meta: MMeta; preview: string; deploy_ready: boolean }

export const STAGES: { key: MStage; label: string }[] = [
  { key: 'survey', label: '盘点' },
  { key: 'plan', label: '确认方案' },
  { key: 'extract', label: '抓取' },
  { key: 'rebuild', label: '重做' },
  { key: 'verify', label: '核对' },
  { key: 'cutover', label: '切换' },
  { key: 'track', label: '跟踪' },
];

const PARENT_KEY = 'cligc.pilot.newSiteParent';

export const migrate = $state({
  view: {} as Record<string, MView>,
  loading: {} as Record<string, boolean>,
  /** 正在做的操作（按站点）：plan / dns / cutover / rollback / own … */
  busy: {} as Record<string, string>,
  dialog: { open: false, domain: '', parent: '', busy: false, err: '' },
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
  return [...count].sort((a, b) => b[1] - a[1])[0]?.[0] ?? '';
}

export function openMigrate() {
  connections.switcherOpen = false;
  migrate.dialog = { open: true, domain: '', parent: defaultParent(), busy: false, err: '' };
}
export function closeMigrate() { if (!migrate.dialog.busy) migrate.dialog.open = false; }

export async function pickMigrateParent() {
  const picked = await open({ title: '迁移来的站点放在哪个文件夹下', directory: true, multiple: false, defaultPath: migrate.dialog.parent || undefined });
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

export async function loadMigrate(id: string) {
  if (untrack(() => migrate.loading[id])) return;
  untrack(() => { migrate.loading[id] = true; });
  try { migrate.view[id] = await invoke<MView>('migrate_state', { id }); }
  catch { /* 不是迁移站点 */ }
  finally { migrate.loading[id] = false; }
}

async function run(id: string, busy: string, f: () => Promise<unknown>, ok?: string) {
  migrate.busy[id] = busy;
  try { await f(); if (ok) say(ok, 'ok'); }
  catch (e) { say(String(e), 'err'); }
  finally { migrate.busy[id] = ''; void loadMigrate(id); }
}

export const resurvey = (id: string) => run(id, 'survey', () => invoke('migrate_survey', { id }));

/** 确认方案并直接开始抓取。 */
export async function confirmPlan(id: string, plan: Pick<MPlan, 'drop_groups' | 'drop_paths' | 'visual' | 'style'>) {
  await run(id, 'plan', async () => {
    await invoke('migrate_save_plan', { id, plan: { ...plan, stack: 'astro', deploy: 'direct', confirmed_at: 0 } });
    await invoke('migrate_extract', { id });
  });
}
export const extract = (id: string) => run(id, 'extract', () => invoke('migrate_extract', { id }));

/** 重做：第一次开一条新的制作对话；之后（继续重做、修核对出的问题）接在原来那条里。
 *  全自动档：在 Pilot 新建的本地文件夹里干活，拿不到部署凭据。 */
export async function startRebuild(site: Site) {
  try {
    const first = !migrate.view[site.id]?.meta.rebuild_at;
    const text = await invoke<string>('migrate_rebuild_message', { id: site.id });
    if (first) await startSiteChat(site.id, text, '内容已经抓好，重做要求也写好了：选一个可用的模型后发送，就开始重做', MAKING_PERM);
    else await sendToMaking(site, text);
  } catch (e) { say(String(e), 'err'); }
  finally { void loadMigrate(site.id); }
}

export const verify = (id: string) => run(id, 'verify', () => invoke('migrate_verify', { id }));
export const checkOwnership = (id: string) => run(id, 'own', () => invoke('migrate_ownership', { id }));
export const prepareDns = (id: string, create = false) => run(id, 'dns', () => invoke('migrate_dns_prepare', { id, create }));

export async function addToCloudflare(id: string, domain: string) {
  if (!await confirmDialog(`会用站点的 Cloudflare 连接把 ${domain} 加进 Cloudflare，并对照现在的 DNS 记录。之后要你去域名注册商把名称服务器改成 Cloudflare 给的那两个——改之前网站和邮件都不受影响。`, { title: '把域名接进 Cloudflare', kind: 'info', confirmText: '加进 Cloudflare' })) return;
  await prepareDns(id, true);
}

export async function fixDns(id: string, n: number) {
  if (!await confirmDialog(`把 ${n} 条原有记录补进 Cloudflare（照原样，不开代理）。补齐后再改名称服务器，邮件和其它子域名就不会断。`, { title: '补上缺的 DNS 记录', kind: 'info', confirmText: '补上' })) return;
  await run(id, 'dns', () => invoke('migrate_dns_fix', { id }), 'DNS 记录已补上');
}

export async function cutover(id: string, host: string) {
  if (!await confirmDialog(`${host} 会改为指向新站：原来这个主机名的 DNS 记录会被替换（已存下来，可以一键回退）。旧主机先别停。`, { title: '切换到新站', kind: 'danger', confirmText: '切换' })) return;
  await run(id, 'cutover', async () => { await invoke('migrate_cutover', { id }); await refreshSites(); }, '已切换到新站，DNS 和证书生效一般要几分钟');
}

export async function rollback(id: string, host: string) {
  if (!await confirmDialog(`把 ${host} 的 DNS 恢复成切换前的记录，打开的又会是旧站；新站和抓好的内容都保留，可以改好后再切换。`, { title: '回退到旧站', kind: 'danger', confirmText: '回退' })) return;
  await run(id, 'rollback', async () => { const m = await invoke<string>('migrate_rollback', { id }); say(m, 'ok'); await refreshSites(); });
}

export const track = (id: string) => run(id, 'track', () => invoke('migrate_track', { id }));

/** 迁移作业结束（或者上线到预览地址结束）就刷新迁移状态。 */
export function initMigrate() {
  $effect(() => {
    const un = listen<{ site_id: string; status: string; kind: string; title: string; error: string }>('site-job', (ev) => {
      const j = ev.payload;
      if (j.status === 'running') return;
      const s = sites.list.find((x) => x.id === j.site_id);
      if (!showMigrateCard(s)) return;
      void loadMigrate(j.site_id);
      if (j.kind.startsWith('migrate:')) say(j.status === 'ok' ? `${j.title}完成` : j.status === 'canceled' ? `已停止${j.title}` : `${j.title}没通过：${j.error.split('\n')[0]}`, j.status === 'ok' ? 'ok' : 'err');
    });
    return () => { void un.then((f) => f()); };
  });
}

/** 切换后的跟踪：打开时超过 12 小时没查过就查一次。 */
export function trackIfStale(id: string) {
  const v = migrate.view[id];
  if (!v || v.stage !== 'track' || migrate.busy[id] || sites.jobs[id]?.status === 'running') return;
  if (Date.now() / 1000 - (v.meta.track.at || 0) > 12 * 3600) void track(id);
}

