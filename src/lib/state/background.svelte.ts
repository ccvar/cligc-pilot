// 后台运行（开机启动、不让电脑睡眠、停过的时段）和 Pilot 数据备份。界面在 BackgroundDialog / BackgroundNotice。
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { relaunch } from '@tauri-apps/plugin-process';
import { say, confirmDialog } from './app.svelte.js';
import { openConnectionSetup } from './settings.svelte.js';
import { openUpdateCenter } from './updater.svelte.js';
import { isMainWindow } from './env';
import type { BackgroundStatus, BackupStatus, BackupManifest } from '$lib/types';

class BgState {
  open = $state(false);
  status = $state<BackgroundStatus | null>(null);
  backup = $state<BackupStatus | null>(null);
  busy = $state('');
}
export const bg = new BgState();

export async function loadBackground() {
  try { bg.status = await invoke<BackgroundStatus>('background_status'); } catch { /* 读不到不打扰 */ }
}
export async function loadBackup() {
  try { bg.backup = await invoke<BackupStatus>('backup_status'); } catch { /* 读不到不打扰 */ }
}
export function openBackground() {
  bg.open = true;
  void loadBackground();
  void loadBackup();
}

export async function setBackground(p: { launch_at_login?: boolean; keep_awake?: boolean }) {
  try { bg.status = await invoke<BackgroundStatus>('background_set', { launchAtLogin: p.launch_at_login ?? null, keepAwake: p.keep_awake ?? null }); }
  catch (e) { say(String(e), 'err'); }
}
export async function dismissGapNotice() {
  if (bg.status) bg.status = { ...bg.status, notice: null };
  try { await invoke('background_dismiss_notice'); } catch { /* 下次还提醒也无妨 */ }
}

export async function setBackup(enabled: boolean, connId: string) {
  try { bg.backup = await invoke<BackupStatus>('backup_set', { enabled, connId }); }
  catch (e) { say(String(e), 'err'); }
}
export async function backupNow() {
  bg.busy = 'backup';
  try { say(await invoke<string>('backup_now')); }
  catch (e) { say(String(e), 'err'); }
  finally { bg.busy = ''; void loadBackup(); }
}

const when = (s: number) => new Date(s * 1000).toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' });
/** 先看备份里有什么，再二次确认，恢复后重启 Pilot */
export async function restoreBackup(connId: string, repo: string) {
  bg.busy = 'restore';
  try {
    const m = await invoke<BackupManifest>('backup_preview', { connId, repo });
    const ok = await confirmDialog(
      `备份时间：${when(m.at)}${m.host ? `（来自 ${m.host}）` : ''}\n里面有 ${m.sites} 个站点、${m.files} 个文件。\n\n恢复会用备份里的站点设置、托管方案、计划、任务队列和运营记录覆盖这台电脑上的同名文件；这台电脑上有、备份里没有的连接和站点会保留。被覆盖的文件先留一份在数据目录的 before-restore 文件夹里。\n\n恢复完 Pilot 会自动重启。密钥不在备份里，换了电脑要在「连接与模型」里重新填一下 Token 和 API Key。`,
      { title: '从备份恢复？', kind: 'warning', confirmText: '恢复并重启' },
    );
    if (!ok) return;
    await invoke<string>('backup_restore', { connId, repo });
    await relaunch();
  } catch (e) { say(String(e), 'err'); }
  finally { bg.busy = ''; }
}

export function initBackground() {
  $effect(() => {
    void loadBackground();
    // 调度每 10 分钟心跳一次：跟着刷新，停过的提醒能及时出现
    const t = setInterval(() => void loadBackground(), 10 * 60_000);
    const un = listen('backup-done', () => void loadBackup());
    return () => { clearInterval(t); void un.then((f) => f()); };
  });
}

/** macOS 菜单栏里「检查更新… / 设置… / 后台运行与备份…」（Rust 端发到主窗口的 app-menu 事件） */
export function initAppMenu() {
  $effect(() => {
    if (!isMainWindow) return;
    const un = listen<string>('app-menu', (e) => {
      if (e.payload === 'update') openUpdateCenter();
      else if (e.payload === 'settings') openConnectionSetup();
      else if (e.payload === 'background') openBackground();
    });
    return () => void un.then((f) => f());
  });
}

/** 时长：3 小时 20 分 / 2 天 5 小时 */
export function span(sec: number): string {
  const m = Math.round(sec / 60);
  if (m < 60) return `${m} 分钟`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时${m % 60 >= 10 ? ` ${m % 60} 分` : ''}`;
  return `${Math.floor(h / 24)} 天${h % 24 ? ` ${h % 24} 小时` : ''}`;
}
export const gapWhy = (kind: string) => (kind === 'asleep' ? '电脑睡着了' : '客户端没开');
export { when as gapTime };
