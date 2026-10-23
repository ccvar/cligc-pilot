import { getVersion } from '@tauri-apps/api/app';
import { check as checkUpdate } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { isMainWindow } from './env';
import { say, confirmDialog } from './app.svelte.js';
import { settings } from './settings.svelte.js';

// 应用版本 + 在线检查更新（Tauri updater：拉 release 仓 latest.json，ed25519 验签后下载安装再重启）。
type PilotUpdateStatus = 'idle' | 'checking' | 'available' | 'current' | 'unreachable' | 'updating' | 'waiting' | 'done' | 'failed';

class UpdaterState {
  appVersion = $state('');
  updBusy = $state(false);
  updMsg = $state('');
  updPct = $state(-1); // 下载进度 0-100；-1 = 不确定（无 content-length）
  updAvail = $state(''); // 非空 = 静默检查到的新版本号，驱动工具栏「待更新」图标
  pilotUpdateStatus = $state<PilotUpdateStatus>('idle');
  pilotUpdateError = $state('');
  pilotReleaseNotes = $state('');
  pilotPublishedAt = $state('');
  pilotUpdateReceipt = $state('');
  updateCenterOpen = $state(false);
  updateCheckedAt = $state(0);
  topUpdateVisible = $derived(isMainWindow && (!!this.updAvail || this.updateCenterOpen || this.updBusy));
}

export const updater = new UpdaterState();

const PILOT_UPDATE_RECEIPT_KEY = 'cligc.pilot.updateReceipt';
function sameUpdateVersion(a: string, b: string): boolean {
  const norm = (v: string) => v.trim().replace(/^v/i, '');
  return norm(a) === norm(b);
}

// 只检查并释放 updater 资源句柄；主窗口统一调度，连接子窗口不重复请求更新源。
export async function checkPilotUpdate(silent = true) {
  if (!isMainWindow || updater.updBusy || updater.pilotUpdateStatus === 'checking') return;
  updater.pilotUpdateStatus = 'checking';
  updater.pilotUpdateError = '';
  updater.updAvail = '';
  try {
    const upd = await checkUpdate();
    updater.updateCheckedAt = Date.now();
    if (!upd) {
      updater.pilotReleaseNotes = '';
      updater.pilotPublishedAt = '';
      updater.pilotUpdateStatus = 'current';
      return;
    }
    updater.updAvail = upd.version;
    updater.pilotReleaseNotes = upd.body || '';
    updater.pilotPublishedAt = upd.date || '';
    updater.pilotUpdateStatus = 'available';
    try { await upd.close(); } catch { /* */ }
  } catch (e) {
    updater.updateCheckedAt = Date.now();
    updater.pilotUpdateError = String(e);
    updater.pilotUpdateStatus = 'unreachable';
    if (!silent) say(`检查 Pilot 更新失败：${updater.pilotUpdateError}`, 'err');
  }
}

async function installPilotUpdate() {
  if (!isMainWindow) { say('请在 Pilot 主窗口中更新客户端', 'err'); return; }
  if (updater.updBusy) return;
  updater.updBusy = true; updater.updMsg = '检查中…'; updater.updPct = -1;
  updater.pilotUpdateStatus = 'checking'; updater.pilotUpdateError = '';
  let installed = false;
  let upd: Awaited<ReturnType<typeof checkUpdate>> = null;
  try {
    upd = await checkUpdate();
    updater.updateCheckedAt = Date.now();
    if (!upd) {
      updater.updAvail = ''; updater.updMsg = '已是最新版本'; updater.pilotUpdateStatus = 'current';
      return;
    }
    updater.updAvail = upd.version;
    updater.pilotReleaseNotes = upd.body || '';
    updater.pilotPublishedAt = upd.date || '';
    updater.pilotUpdateStatus = 'available';
    const ok = await confirmDialog(`发现新版本 ${upd.version}，现在下载更新并重启？`, { title: '有可用更新', kind: 'info' });
    if (!ok) {
      updater.updMsg = '';
      try { await upd.close(); } catch { /* */ }
      return;
    }
    let total = 0, got = 0;
    updater.updMsg = '准备下载…'; updater.updPct = 0;
    updater.pilotUpdateStatus = 'updating';
    await upd.downloadAndInstall((ev) => {
      if (ev.event === 'Started') { total = ev.data.contentLength ?? 0; got = 0; updater.updPct = 0; updater.updMsg = '下载更新…'; }
      else if (ev.event === 'Progress') { got += ev.data.chunkLength; updater.updPct = total > 0 ? Math.min(100, Math.round((got / total) * 100)) : -1; updater.updMsg = total > 0 ? `下载更新… ${updater.updPct}%` : '下载更新…'; }
      else if (ev.event === 'Finished') { updater.updPct = 100; updater.updMsg = '安装中…'; }
    });
    installed = true;
    try {
      localStorage.setItem(PILOT_UPDATE_RECEIPT_KEY, JSON.stringify({ target: upd.version, installedAt: Date.now() }));
    } catch { /* */ }
    updater.updMsg = '即将重启…'; updater.updPct = 100;
    updater.pilotUpdateStatus = 'done';
    await relaunch();
  } catch (e) {
    const detail = String(e);
    updater.updPct = -1;
    if (!upd) updater.updAvail = '';
    if (installed) {
      updater.updMsg = `已安装，等待手动重启：${detail}`;
      updater.pilotUpdateStatus = 'waiting';
    } else {
      updater.updMsg = `更新失败：${detail}`;
      updater.pilotUpdateStatus = 'failed';
    }
    updater.pilotUpdateError = detail;
    say(updater.updMsg, 'err');
    try { await upd?.close(); } catch { /* */ }
  }
  finally { updater.updBusy = false; }
}
export async function runUpdate() { await installPilotUpdate(); }
export function pilotUpdateLabel(status: PilotUpdateStatus): string {
  switch (status) {
    case 'checking': return '检查中';
    case 'available': return '可更新';
    case 'current': return '已是最新';
    case 'unreachable': return '无法检查';
    case 'updating': return '更新中';
    case 'waiting': return '等待重启';
    case 'done': return '已完成';
    case 'failed': return '失败';
    default: return '尚未检查';
  }
}
export function pilotUpdateMessage(): string {
  if (updater.updBusy) return updater.updMsg || '正在更新';
  if (updater.pilotUpdateStatus === 'unreachable' || updater.pilotUpdateStatus === 'failed') return updater.pilotUpdateError || updater.updMsg || '无法完成更新检查';
  if (updater.updAvail) return `${updater.appVersion || '当前版本'} → ${updater.updAvail}`;
  if (updater.pilotUpdateStatus === 'current') return updater.appVersion ? `当前 v${updater.appVersion}，已是最新版` : '当前已是最新版';
  return updater.pilotUpdateStatus === 'checking' ? '正在检查客户端更新…' : '尚未检查客户端更新';
}
export function openUpdateCenter() {
  if (!isMainWindow) { say('请在 Pilot 主窗口中打开更新中心', 'err'); return; }
  settings.setupOpen = false;
  updater.updateCenterOpen = true;
  if (!updater.updateCheckedAt || Date.now() - updater.updateCheckedAt > 10 * 60 * 1000) void checkPilotUpdate(false);
}
export function closeUpdateCenter() {
  if (updater.updBusy) return;
  updater.updateCenterOpen = false;
}

export function initUpdater() {
  getVersion().then((v) => {
    updater.appVersion = v;
    try {
      const receipt = JSON.parse(localStorage.getItem(PILOT_UPDATE_RECEIPT_KEY) || 'null') as { target?: string } | null;
      if (receipt?.target && sameUpdateVersion(v, receipt.target)) {
        updater.pilotUpdateReceipt = `Pilot 已更新到 ${v}`;
        localStorage.removeItem(PILOT_UPDATE_RECEIPT_KEY);
      }
    } catch { /* 忽略损坏的回执 */ }
  }).catch(() => { /* */ });

  // 启动 5 秒后检查一次，之后每 6 小时静默检查。
  $effect(() => {
    if (!isMainWindow) return;
    const initial = setTimeout(() => { void checkPilotUpdate(true); }, 5000);
    const periodic = setInterval(() => { void checkPilotUpdate(true); }, 6 * 60 * 60 * 1000);
    return () => { clearTimeout(initial); clearInterval(periodic); };
  });
}
