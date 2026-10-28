// 设置里的「手机通知」（Telegram 机器人）和「收益与外部数据」（AdSense、PageSpeed、Serper 的 Key、导入收益 CSV）；
// 站点页里收益、速度、体检、竞品卡片共用的金额格式。
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';
import { say } from './app.svelte.js';
import { loadManaged } from './managed.svelte.js';
import type { PushStatus, RevenueStatus, ApiKeys, RevenueImport } from '$lib/types';

class GrowthState {
  push = $state<PushStatus | null>(null);
  revenue = $state<RevenueStatus | null>(null);
  keys = $state<ApiKeys | null>(null);
  busy = $state('');
}
export const growth = new GrowthState();

export async function loadGrowth() {
  const [p, r, k] = await Promise.all([
    invoke<PushStatus>('push_status').catch(() => null),
    invoke<RevenueStatus>('revenue_status').catch(() => null),
    invoke<ApiKeys>('api_keys_status').catch(() => null),
  ]);
  growth.push = p;
  growth.revenue = r;
  growth.keys = k;
}

async function run<T>(tag: string, f: () => Promise<T>): Promise<T | undefined> {
  growth.busy = tag;
  try { return await f(); }
  catch (e) { say(String(e), 'err'); return undefined; }
  finally { growth.busy = ''; }
}

// ── Telegram ──
export async function setPushToken(token: string) {
  const s = await run('tg-token', () => invoke<PushStatus>('push_set_token', { token }));
  if (s) { growth.push = s; await startPairing(); }
  return !!s;
}
/** 给一个 10 分钟有效的配对码，并打开 Telegram 里和机器人的对话（带上 /start 配对码） */
export async function startPairing() {
  const s = await run('tg-pair', () => invoke<PushStatus>('push_pair'));
  if (!s) return;
  growth.push = s;
  if (s.bot_name) void openUrl(`https://t.me/${s.bot_name}?start=${s.pair_code}`).catch(() => {});
}
export async function setPush(p: { enabled?: boolean; allow_approve?: boolean }) {
  const s = await run('tg-set', () => invoke<PushStatus>('push_set', { enabled: p.enabled ?? null, allowApprove: p.allow_approve ?? null }));
  if (s) growth.push = s;
}
export async function testPush() {
  const ok = await run('tg-test', () => invoke('push_test'));
  if (ok !== undefined) say('测试消息已发出，看看手机');
}
export async function forgetPush() {
  const s = await run('tg-forget', () => invoke<PushStatus>('push_forget'));
  if (s) growth.push = s;
}

// ── AdSense ──
export async function connectAdsense() {
  const s = await run('ads', () => invoke<RevenueStatus>('adsense_connect'));
  if (s) { growth.revenue = s; say(s.error ? `已连接，但报表没拉下来：${s.error}` : '已连接 AdSense，收益会每天更新一次', s.error ? 'err' : undefined); }
}
export async function refreshAdsense() {
  const s = await run('ads-refresh', () => invoke<RevenueStatus>('adsense_refresh'));
  if (s) { growth.revenue = s; say('AdSense 收益已更新'); }
}
export async function disconnectAdsense() {
  const s = await run('ads-off', () => invoke<RevenueStatus>('adsense_disconnect'));
  if (s) growth.revenue = s;
}
/** 选一个 CSV 文件导入（每行：月份,域名,金额[,来源[,备注]]） */
export async function importRevenue() {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const path = await open({ title: '选择收益 CSV', multiple: false, filters: [{ name: 'CSV', extensions: ['csv', 'tsv', 'txt'] }] });
  if (typeof path !== 'string') return;
  const r = await run('csv', () => invoke<RevenueImport>('revenue_import', { path }));
  if (!r) return;
  const parts = [`导入了 ${r.added} 笔`];
  if (r.unknown.length) parts.push(`${r.unknown.length} 个域名对不上站点（${r.unknown.slice(0, 3).join('、')}${r.unknown.length > 3 ? ' 等' : ''}）`);
  if (r.bad_lines.length) parts.push(`第 ${r.bad_lines.slice(0, 5).join('、')} 行看不懂`);
  say(parts.join('；'), r.added ? undefined : 'err');
}

// ── 外部服务的 Key ──
export async function setApiKey(name: 'pagespeed' | 'serper', value: string) {
  const k = await run(`key-${name}`, () => invoke<ApiKeys>('api_key_set', { name, value }));
  if (k) { growth.keys = k; say(name === 'serper' ? 'Serper 的 Key 能用，每周会查一遍竞品' : 'PageSpeed 的 Key 能用'); }
  return !!k;
}
export async function removeApiKey(name: 'pagespeed' | 'serper') {
  const k = await run(`key-${name}`, () => invoke<ApiKeys>('api_key_remove', { name }));
  if (k) growth.keys = k;
}

/** 配对成功（机器人收到配对码）时刷新 */
export function initGrowth() {
  $effect(() => {
    const un = listen('push-changed', () => void loadGrowth());
    const un2 = listen('revenue-changed', () => void loadGrowth());
    // 在手机上发了 /pause、/resume：托管页跟着刷新
    const un3 = listen('managed-changed', () => void loadManaged());
    return () => { for (const u of [un, un2, un3]) void u.then((f) => f()); };
  });
}

/** 金额：$1,234 / $12.30 / $0.45 */
export function usd(v: number): string {
  if (!Number.isFinite(v)) return '—';
  const a = Math.abs(v);
  const s = a >= 1000 ? Math.round(a).toLocaleString('en-US') : a >= 100 ? a.toFixed(0) : a.toFixed(2);
  return `${v < 0 ? '−' : ''}$${s}`;
}
