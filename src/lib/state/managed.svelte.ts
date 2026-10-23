import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ManagedConfig, ManagedOverview, ManagedActivity, SitePlan } from '$lib/types';
import { app, say, confirmDialog } from './app.svelte.js';
import { refreshSites } from './sites.svelte.js';
import { loadFleet, loadPending } from './fleet.svelte.js';
import { selectConn } from './connections.svelte.js';
import { SITES_CONN_ID } from './env';

export type ManagedTab = 'activity' | 'plans' | 'needs' | 'digests' | 'settings';

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
}

export const managed = new ManagedState();

export function openManaged(tab?: ManagedTab) {
  if (tab) managed.tab = tab;
  void selectConn(SITES_CONN_ID).then(() => { app.view = 'managed'; });
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
    say('托管设置已保存');
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
export async function setManagedSites(ids: string[], on: boolean) {
  if (!ids.length) return;
  if (on && !(await confirmDialog(`把 ${ids.length} 个站点交给 AI 全托管？AI 会每周制定计划、自动修改并上线页面内容（依赖、配置、代码、推荐 / 邀请相关内容永远不会自动改），上线后自动检查，变差就回滚。随时可以退出或暂停。`, { title: '开始托管', kind: 'info', confirmText: '开始托管' }))) return;
  try {
    await invoke<number>('set_sites_managed', { ids, managed: on });
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
