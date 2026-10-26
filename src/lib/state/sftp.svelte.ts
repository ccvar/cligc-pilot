import { SFTP_TRANSFER_HISTORY_KEY, type TransferRecord, readTransferHistory, serializeTransferHistory, addTransfer, createTransfer, type TransferUpdate, updateTransfer, type TransferFinalStatus, finishTransfer, isTransferActive } from '$lib/sftpTransfers';
import { onDestroy } from 'svelte';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { invoke, Channel } from '@tauri-apps/api/core';
import { type SftpDropTarget, resolveSftpDropTarget, localFileBasename } from '$lib/sftpDrop';
import { fitFileMenu, copyTargetName } from '$lib/sftpFiles';
import { open, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { pilotWindowLabel, isMac } from './env';
import { app, say, confirmDialog } from './app.svelte.js';
import { friendly } from '$lib/friendlyError';
import { missingSshCredential, repairActiveSshCredential, workbench, bytesToB64 } from './workbench.svelte.js';
import { sftpEdit } from './editor.svelte.js';

// ---------- 远程文件（SFTP，走终端同一条 SSH 会话） ----------
// 视图＝可展开的树：sftpPath 是树根，每个目录的孩子按需加载后缓存在 sftpKids 里。
type SftpEntry = { name: string; dir: boolean; link: boolean; perms: string; size: number; mtime: number };
type SftpRow = SftpEntry & { path: string; depth: number };
type DownloadProgress = { phase: string; bytes: number; total_bytes: number; files: number; total_files: number; skipped: number };
type DownloadResult = { path: string; drag_id: string | null; skipped: number };
type SftpDragEvent = { phase: 'dragging' | 'dropped' | 'completed' | 'cancelled' | 'error'; path?: string; error?: string };
type LocalDropEntry = { name: string; size: number; mtime: number; directory: boolean | null };
type LocalDropCapture = { token: string; count: number };
type LocalDropProbe = { capture: Promise<LocalDropCapture | null>; entries: LocalDropEntry[] };
// ---- 列宽可拖（名称是弹性列，吃掉剩下的宽度）----
// 手柄在每个定宽列的**左边缘**上：拖它就是拖这一列的左边，往右拖＝这列变窄，名称随之变宽。
// 这样手柄始终跟着鼠标走，三根手柄的行为也一致。
type ColKey = 'perm' | 'size' | 'date';

const sftpHistoryKey = `${SFTP_TRANSFER_HISTORY_KEY}:${pilotWindowLabel}`;
const COL_MIN: Record<ColKey, number> = { perm: 62, size: 48, date: 92 };
const COL_MAX: Record<ColKey, number> = { perm: 200, size: 160, date: 240 };
const COL_DEF: Record<ColKey, number> = { perm: 96, size: 76, date: 128 };

class SftpState {
  sftpConnId = $state(''); // 已加载文件列表所属的连接；换连接才重置，切页签不重置
  sftpPath = $state('');
  sftpPathDraft = $state('');
  sftpKids = $state<Record<string, SftpEntry[]>>({}); // 目录路径 → 它的孩子
  sftpOpenDirs = $state(new Set<string>());
  sftpLoading = $state<Record<string, boolean>>({});
  sftpSel = $state('');
  sftpSort = $state<{ key: 'name' | 'size' | 'mtime'; asc: boolean }>({ key: 'name', asc: true });
  sftpBusy = $state(false);
  sftpErr = $state('');
  sftpXfer = $state(''); // 上传/下载进行中的提示文案
  sftpDownloadTask = $state<null | { id: string; name: string; drag: boolean; started: boolean; cancelling: boolean; progress: DownloadProgress | null }>(null);
  sftpUploadTask = $state<null | { id: string; started: boolean; cancelling: boolean }>(null);
  sftpNativeDragging = $state(false);
  sftpTransfers = $state<TransferRecord[]>(loadSftpTransfers());
  sftpHistoryTimer: ReturnType<typeof setTimeout> | undefined;
  sftpCancellableIds = $derived([this.sftpDownloadTask, this.sftpUploadTask].filter(task => task?.started).map(task => task!.id));
  sftpFileOver = $state(false);
  sftpDropTarget = $state<SftpDropTarget | null>(null);
  sftpDropCapture: Promise<LocalDropCapture | null> | null = null;
  sftpClipboard = $state<null | { connId: string; path: string; name: string; dir: boolean }>(null);
  // connId + path：切换服务器时，旧服务器仍在进行的删除不会误伤当前列表状态。
  sftpDeleting = $state(new Set<string>());
  colW = $state(loadColW());
  // 树 → 扁平行（只把展开着的目录的孩子摊进来）。
  sftpRows = $derived.by(() => {
    const out: SftpRow[] = [];
    const walk = (dir: string, depth: number) => {
      for (const e of sortRows(this.sftpKids[dir] ?? [])) {
        const p = sftpJoin(dir, e.name);
        out.push({ ...e, path: p, depth });
        if (e.dir && !e.link && this.sftpOpenDirs.has(p)) walk(p, depth + 1);
      }
    };
    if (this.sftpPath) walk(this.sftpPath, 0);
    return out;
  });
  sftpSelectedRow = $derived(this.sftpRows.find((row) => row.path === this.sftpSel));
  // ---- 路径栏：面包屑（可点逐级跳）／点空白处切成输入框直接敲路径 ----
  sftpCrumbs = $derived.by(() => {
    const out = [{ name: '/', path: '/' }];
    let acc = '';
    for (const p of this.sftpPath.split('/').filter(Boolean)) { acc += '/' + p; out.push({ name: p, path: acc }); }
    return out;
  });
  sftpPathEdit = $state(false);
  sftpPathEl = $state<HTMLInputElement | null>(null);
  crumbEl = $state<HTMLElement | null>(null);
  // ---- 右键菜单 ----
  fctxMenu = $state<null | { x: number; y: number; row: SftpRow | null }>(null);
  fctxMenuEl = $state<HTMLDivElement | null>(null);
  // 新建文件夹 / 文件使用弹窗；重命名单独在列表中原地编辑。
  fsAsk = $state<null | { mode: 'mkdir' | 'create'; connId: string; dir: string; from: string; value: string; busy: boolean; err: string }>(null);
  fileRename = $state<null | { connId: string; path: string; name: string; directory: boolean; value: string; busy: boolean; err: string }>(null);
}

export const sftp = new SftpState();

export function retryOrRepairSftp() {
  if (missingSshCredential(sftp.sftpErr)) repairActiveSshCredential();
  else void sftpGo(sftp.sftpPath);
}
function loadSftpTransfers(): TransferRecord[] {
  try { return readTransferHistory(localStorage.getItem(sftpHistoryKey)); } catch { return []; }
}
function persistSftpTransfers(immediate = false) {
  const save = () => {
    sftp.sftpHistoryTimer = undefined;
    try { localStorage.setItem(sftpHistoryKey, serializeTransferHistory(sftp.sftpTransfers)); } catch { /* Storage may be unavailable or full. */ }
  };
  if (immediate) { clearTimeout(sftp.sftpHistoryTimer); save(); }
  else if (!sftp.sftpHistoryTimer) sftp.sftpHistoryTimer = setTimeout(save, 250);
}
function addSftpTransfer(id: string, name: string, direction: 'upload' | 'download', connId: string, remotePath: string, localPath?: string, phase = 'connecting') {
  const conn = app.conns.find(c => c.id === connId);
  sftp.sftpTransfers = addTransfer(sftp.sftpTransfers, createTransfer({ id, name, direction, connId, connectionName: conn?.remark?.trim() || conn?.name || '远程连接', remotePath, localPath, phase, status: phase === 'dragging' ? 'queued' : 'running' }));
  persistSftpTransfers(true);
}
function updateSftpTransfer(id: string, patch: TransferUpdate) {
  sftp.sftpTransfers = updateTransfer(sftp.sftpTransfers, id, patch);
  persistSftpTransfers();
}
function progressSftpTransfer(id: string, p: DownloadProgress) {
  updateSftpTransfer(id, { status: 'running', phase: p.phase, bytes: p.bytes, totalBytes: p.total_bytes, files: p.files, totalFiles: p.total_files });
}
function finishSftpTransfer(id: string, status: TransferFinalStatus, patch: TransferUpdate = {}) {
  sftp.sftpTransfers = finishTransfer(sftp.sftpTransfers, id, status, patch);
  persistSftpTransfers(true);
}
export function clearSftpTransfers() { sftp.sftpTransfers = sftp.sftpTransfers.filter(isTransferActive); persistSftpTransfers(true); }
export async function revealSftpTransfer(record: TransferRecord) {
  if (!record.localPath || record.status !== 'completed' || record.direction !== 'download') return;
  try { await revealItemInDir(record.localPath); } catch (e) { say(String(e), 'err'); }
}
export async function cancelSftpTransfer(id: string) {
  if (sftp.sftpDownloadTask?.id === id) return cancelSftpDownload();
  if (sftp.sftpUploadTask?.id !== id || !sftp.sftpUploadTask.started || sftp.sftpUploadTask.cancelling) return;
  sftp.sftpUploadTask = { ...sftp.sftpUploadTask, cancelling: true };
  updateSftpTransfer(id, { cancelling: true });
  try { await invoke('sftp_cancel_download', { transferId: id }); }
  catch (e) {
    if (sftp.sftpUploadTask?.id === id) sftp.sftpUploadTask = { ...sftp.sftpUploadTask, cancelling: false };
    updateSftpTransfer(id, { cancelling: false });
    say(String(e), 'err');
  }
}
function beginSftpLocalDrop() {
  if (!isMac || sftp.sftpDropCapture) return;
  // Native code binds this one-use token to a real drag sequence and window,
  // never to a stale drag pasteboard or the general clipboard.
  const capture = invoke<LocalDropCapture>('sftp_capture_local_drop');
  // A hover can be abandoned without a drop. Suppress an unhandled rejection
  // here, but retain the rejection so an accepted drop fails closed.
  void capture.catch(() => {});
  sftp.sftpDropCapture = capture;
}
function discardSftpLocalDrop() {
  const capture = sftp.sftpDropCapture;
  sftp.sftpDropCapture = null;
  if (capture) void capture.then(result => {
    if (result) void invoke('sftp_discard_local_drop', { token: result.token }).catch(() => {});
  }).catch(() => {});
}
function sftpDropTargetAt(e: DragEvent): SftpDropTarget | null {
  const element = e.target instanceof Element ? e.target : null;
  const path = element?.closest<HTMLElement>('[data-sftp-drop-path]')?.dataset.sftpDropPath;
  const hoveredRow = path ? sftp.sftpRows.find(row => row.path === path) : undefined;
  const dir = hoveredRow?.dir && !hoveredRow.link ? hoveredRow.path : sftp.sftpPath;
  return resolveSftpDropTarget({ connId: sftp.sftpConnId, remoteDir: sftp.sftpPath, busy: sftp.sftpBusy,
    transferring: !!sftp.sftpXfer, nativeDragging: sftp.sftpNativeDragging,
    insidePanel: !!element && (e.currentTarget as HTMLElement).contains(element), hoveredRow,
    targetDeleting: isSftpDeleting(dir) });
}
function localDropEntries(data: DataTransfer, files: File[]): LocalDropEntry[] {
  // Read protected drag items synchronously, before any IPC/await clears them.
  const items = Array.from(data.items).filter(item => item.kind === 'file');
  return files.map((file, index) => {
    let directory: boolean | null = null;
    try { const entry = items[index]?.webkitGetAsEntry?.(); if (entry) directory = entry.isDirectory; } catch { /* Non-filesystem drag. */ }
    return { name: file.name, size: file.size, mtime: file.lastModified, directory };
  });
}
function sftpDeletingKey(connId: string, path: string): string { return `${connId}\n${path}`; }
export function isSftpDeleteTarget(path: string): boolean { return sftp.sftpDeleting.has(sftpDeletingKey(sftp.sftpConnId, path)); }
export function isSftpDeleting(path: string): boolean {
  const prefix = `${sftp.sftpConnId}\n`;
  for (const key of sftp.sftpDeleting) {
    if (!key.startsWith(prefix)) continue;
    const target = key.slice(prefix.length).replace(/\/+$/, '');
    if (path === target || path.startsWith(`${target}/`)) return true;
  }
  return false;
}
function sftpJoin(dir: string, name: string): string { return dir === '/' ? '/' + name : dir + '/' + name; }
export function parentOf(p: string): string { return p.replace(/\/[^/]+\/?$/, '') || '/'; }
function baseOf(p: string): string { return p.split('/').filter(Boolean).pop() || p; }
function sortRows(list: SftpEntry[]): SftpEntry[] {
  const { key, asc } = sftp.sftpSort;
  const sign = asc ? 1 : -1;
  return [...list].sort((a, b) => {
    if (key === 'size') return sign * (a.size - b.size);
    if (key === 'mtime') return sign * (a.mtime - b.mtime);
    return sign * a.name.localeCompare(b.name, 'zh-Hans-CN', { numeric: true });
  });
}
export function setSort(key: 'name' | 'size' | 'mtime') {
  sftp.sftpSort = sftp.sftpSort.key === key ? { key, asc: !sftp.sftpSort.asc } : { key, asc: true };
}
function loadColW(): Record<ColKey, number> {
  try {
    const raw = JSON.parse(localStorage.getItem('cligc.pilot.sftpCols') || '{}');
    const pick = (k: ColKey) => {
      const n = Number(raw[k]);
      return Number.isFinite(n) && n >= COL_MIN[k] && n <= COL_MAX[k] ? n : COL_DEF[k];
    };
    return { perm: pick('perm'), size: pick('size'), date: pick('date') };
  } catch { return { ...COL_DEF }; }
}
function saveColW() { try { localStorage.setItem('cligc.pilot.sftpCols', JSON.stringify(sftp.colW)); } catch { /* */ } }
function setColW(k: ColKey, w: number) {
  sftp.colW = { ...sftp.colW, [k]: Math.min(COL_MAX[k], Math.max(COL_MIN[k], Math.round(w))) };
}
export function startColDrag(e: MouseEvent, k: ColKey) {
  e.preventDefault();
  e.stopPropagation(); // 别让这一下变成「点了列头去排序」
  const x0 = e.clientX;
  const w0 = sftp.colW[k];
  const onMove = (ev: MouseEvent) => setColW(k, w0 - (ev.clientX - x0));
  const onUp = () => {
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
    document.body.style.cursor = '';
    saveColW();
  };
  document.body.style.cursor = 'col-resize';
  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
}
// 键盘也能调（手柄是个 button，Tab 得到）
export function onColKey(e: KeyboardEvent, k: ColKey) {
  const d = e.key === 'ArrowLeft' ? 8 : e.key === 'ArrowRight' ? -8 : 0;
  if (!d) return;
  e.preventDefault();
  setColW(k, sftp.colW[k] + d);
  saveColW();
}
/** 读一个目录的孩子进缓存。force=true 时无视缓存重读（改动后刷新用）。 */
export async function loadDir(path: string, force = false, quiet = false): Promise<boolean> {
  const id = sftp.sftpConnId;
  if (!force && sftp.sftpKids[path]) return true;
  sftp.sftpLoading = { ...sftp.sftpLoading, [path]: true };
  try {
    const list = await invoke<SftpEntry[]>('sftp_list', { connId: id, path });
    if (sftp.sftpConnId !== id) return false; // 加载途中换了连接，丢弃
    sftp.sftpKids = { ...sftp.sftpKids, [path]: list };
    return true;
  } catch (e) {
    if (path === sftp.sftpPath) sftp.sftpErr = String(e); else if (!quiet) say(String(e), 'err');
    return false;
  } finally {
    sftp.sftpLoading = { ...sftp.sftpLoading, [path]: false };
  }
}
/** 换树根（path 为空＝去 home）。 */
export async function sftpGo(path: string) {
  const id = sftp.sftpConnId;
  sftp.sftpBusy = true; sftp.sftpErr = '';
  try {
    const p = path || await invoke<string>('sftp_home', { connId: id });
    if (sftp.sftpConnId !== id) return;
    sftp.sftpPath = p; sftp.sftpPathDraft = p; sftp.sftpSel = '';
    sftp.sftpOpenDirs = new Set();
    await loadDir(p, true);
  } catch (e) { sftp.sftpErr = friendly(e); }
  finally { sftp.sftpBusy = false; }
}
export function sftpUp() {
  if (!sftp.sftpPath || sftp.sftpPath === '/') return;
  void sftpGo(parentOf(sftp.sftpPath));
}
export function startPathEdit() { sftp.sftpPathDraft = sftp.sftpPath; sftp.sftpPathEdit = true; }
export function commitPath() {
  const p = sftp.sftpPathDraft.trim();
  sftp.sftpPathEdit = false;
  if (p && p !== sftp.sftpPath) void sftpGo(p);
}
/** 刷新：重读当前展开着的每一层，**保留展开状态**（sftpGo 是「换根」，会把整棵树收起来）。 */
export async function sftpRefresh() {
  if (!sftp.sftpPath || sftp.sftpBusy) return;
  sftp.sftpBusy = true; sftp.sftpErr = '';
  try {
    const dirs = [sftp.sftpPath, ...sftp.sftpOpenDirs];
    const ok = await Promise.all(dirs.map((d) => loadDir(d, true, true))); // 没了的目录下面统一收掉，不用一个个弹错
    // 远端已经不在了的目录：连同展开态一起丢掉，别在树里留幽灵行
    const gone = dirs.filter((_, i) => !ok[i] && dirs[i] !== sftp.sftpPath);
    if (gone.length) {
      const n = new Set(sftp.sftpOpenDirs);
      const kids = { ...sftp.sftpKids };
      for (const d of gone) { n.delete(d); delete kids[d]; }
      sftp.sftpOpenDirs = n; sftp.sftpKids = kids;
    }
  } finally { sftp.sftpBusy = false; }
}
export async function toggleDir(r: SftpRow) {
  if (isSftpDeleting(r.path)) return;
  const n = new Set(sftp.sftpOpenDirs);
  if (n.has(r.path)) { n.delete(r.path); sftp.sftpOpenDirs = n; return; }
  if (!(await loadDir(r.path))) return;
  n.add(r.path); sftp.sftpOpenDirs = n;
}
/** 试着把它当目录读一下。不是目录就 false —— 调用方退回按文件处理，不报错。 */
async function isRemoteDir(path: string): Promise<boolean> {
  try { await invoke<SftpEntry[]>('sftp_list', { connId: sftp.sftpConnId, path }); return true; }
  catch { return false; }
}
/** 打开（双击 / 回车 / 右键「打开」）：目录＝**进到里面去**（换根，不是就地展开——展开是前面那个小箭头干的事）；
 *  软链＝先当目录试（sftp 的 readdir 给的是 lstat，指向目录的软链这里 dir=false）；文件＝在线编辑。 */
export async function sftpOpenRow(r: SftpRow) {
  if (isSftpDeleting(r.path)) return;
  if (r.dir && !r.link) { await sftpGo(r.path); return; }
  if (r.link && await isRemoteDir(r.path)) { await sftpGo(r.path); return; }
  sftpEdit(r.path);
}
export function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`;
  const u = ['KB', 'MB', 'GB', 'TB']; let v = n / 1024; let i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
}
export function fmtStamp(secs: number): string {
  if (!secs) return '';
  const d = new Date(secs * 1000);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
export function openFctx(e: MouseEvent, row: SftpRow | null) {
  e.preventDefault();
  e.stopPropagation(); // 空白处也不能冒泡到 window 的菜单关闭监听。
  if (!sftp.sftpPath || sftp.sftpBusy) return;
  if (row && isSftpDeleting(row.path)) return;
  if (row) sftp.sftpSel = row.path;
  // 贴边时往回收，别让菜单跑出窗口
  sftp.fctxMenu = {
    x: Math.max(8, Math.min(e.clientX, window.innerWidth - 190)),
    y: Math.max(8, Math.min(e.clientY, window.innerHeight - 320)),
    row,
  };
}
export async function copyPath(p: string) {
  try { await navigator.clipboard.writeText(p); say('已复制路径'); }
  catch { say('复制失败，请手动选择复制', 'err'); }
}
export function copySftpEntry(r: SftpRow) {
  sftp.sftpClipboard = { connId: sftp.sftpConnId, path: r.path, name: r.name, dir: r.dir && !r.link };
  say(`已复制「${r.name}」，在目标文件夹右键粘贴`);
}
export async function pasteSftpEntry(dir = sftp.sftpPath, copied = sftp.sftpClipboard) {
  if (!copied || copied.connId !== sftp.sftpConnId || !dir || sftp.sftpXfer) return;
  const connId = copied.connId;
  sftp.sftpXfer = `复制 ${copied.name}…`;
  try {
    // 粘贴前重新读取目标目录，避免缓存落后于服务器时误判重名并让 cp 失败。
    const kids = await invoke<SftpEntry[]>('sftp_list', { connId, path: dir });
    const name = copyTargetName(copied.name, copied.dir, new Set(kids.map((item) => item.name)));
    const target = sftpJoin(dir, name);
    await invoke('sftp_copy', { connId, from: copied.path, to: target });
    say(name === copied.name ? `已粘贴「${name}」` : `已粘贴为「${name}」`);
    if (sftp.sftpConnId === connId) await loadDir(dir, true);
  } catch (e) { say(String(e), 'err'); }
  finally { sftp.sftpXfer = ''; }
}
export function duplicateSftpEntry(r: SftpRow) {
  return pasteSftpEntry(sftp.sftpPath, { connId: sftp.sftpConnId, path: r.path, name: r.name, dir: r.dir && !r.link });
}
export function openMkdir(dir = sftp.sftpPath) { sftp.fsAsk = { mode: 'mkdir', connId: sftp.sftpConnId, dir, from: '', value: '', busy: false, err: '' }; }
export function openNewFile(dir = sftp.sftpPath) { sftp.fsAsk = { mode: 'create', connId: sftp.sftpConnId, dir, from: '', value: '', busy: false, err: '' }; }
export function openRename(r: SftpRow) {
  sftp.fctxMenu = null;
  if (sftp.fileRename?.busy || isSftpDeleting(r.path)) return;
  sftp.sftpSel = r.path;
  sftp.fileRename = { connId: sftp.sftpConnId, path: r.path, name: r.name, directory: r.dir, value: r.name, busy: false, err: '' };
}
export function focusFileRename(node: HTMLInputElement) {
  // Wait until bind:value has populated the input before selecting the stem.
  queueMicrotask(() => {
    if (!node.isConnected) return;
    node.focus();
    const dot = node.value.lastIndexOf('.');
    node.setSelectionRange(0, !sftp.fileRename?.directory && dot > 0 ? dot : node.value.length);
  });
}
export async function saveFileRename() {
  const edit = sftp.fileRename;
  if (!edit || edit.busy || edit.connId !== sftp.sftpConnId) return;
  const name = edit.value.trim();
  if (!name || name === '.' || name === '..' || /[/\0]/.test(name)) {
    edit.err = '名字不能为空，也不能包含 /、空字符或使用 .、..'; return;
  }
  if (name === edit.name) { sftp.fileRename = null; return; }
  edit.busy = true; edit.err = '';
  const dir = parentOf(edit.path);
  const target = sftpJoin(dir, name);
  try {
    const entries = await invoke<SftpEntry[]>('sftp_list', { connId: edit.connId, path: dir });
    if (entries.some(entry => entry.name === name)) throw new Error('同名文件或文件夹已存在，请换一个名字');
    await invoke('sftp_rename', { connId: edit.connId, from: edit.path, to: target });
    if (sftp.fileRename === edit) sftp.fileRename = null;
    if (sftp.sftpConnId === edit.connId) {
      if (sftp.sftpSel === edit.path) sftp.sftpSel = target;
      const prefix = `${edit.path}/`;
      sftp.sftpOpenDirs = new Set([...sftp.sftpOpenDirs].filter(path => path !== edit.path && !path.startsWith(prefix)));
      await loadDir(dir, true);
    }
  } catch (e) {
    if (sftp.fileRename === edit) { edit.err = String(e); edit.busy = false; }
    else say(String(e), 'err');
  }
}
export async function confirmFsAsk() {
  if (!sftp.fsAsk || sftp.fsAsk.busy) return;
  const v = sftp.fsAsk.value.trim();
  if (!v) return;
  if (v.includes('/') || v.includes('\0') || v === '.' || v === '..') { sftp.fsAsk.err = '请输入有效名字，不能包含 /、空字符或使用 .、..'; return; }
  sftp.fsAsk.busy = true; sftp.fsAsk.err = '';
  const dir = sftp.fsAsk.dir;
  const connId = sftp.fsAsk.connId;
  try {
    if (sftp.fsAsk.mode === 'mkdir') await invoke('sftp_mkdir', { connId, path: sftpJoin(dir, v) });
    else await invoke('sftp_create_file', { connId, path: sftpJoin(dir, v) });
    sftp.fsAsk = null;
    if (sftp.sftpConnId === connId) await loadDir(dir, true);
  } catch (e) { if (sftp.fsAsk) { sftp.fsAsk.err = String(e); sftp.fsAsk.busy = false; } }
}
export async function sftpDelete(r: SftpRow) {
  if (isSftpDeleting(r.path)) return;
  const connId = sftp.sftpConnId;
  const isDir = r.dir && !r.link;
  const yes = await confirmDialog(
    isDir ? `删除文件夹「${r.name}」及其中的全部内容？此操作不可撤销。` : `删除「${r.name}」？此操作不可撤销。`,
    { title: isDir ? '删除文件夹' : '删除文件', kind: 'danger', confirmText: isDir ? '删除全部' : '删除' },
  );
  if (!yes) return;
  const key = sftpDeletingKey(connId, r.path);
  sftp.sftpDeleting = new Set(sftp.sftpDeleting).add(key);
  try {
    await invoke('sftp_remove', { connId, path: r.path, dir: isDir });
    if (sftp.sftpConnId === connId) {
      const prefix = `${r.path.replace(/\/+$/, '')}/`;
      sftp.sftpOpenDirs = new Set([...sftp.sftpOpenDirs].filter((path) => path !== r.path && !path.startsWith(prefix)));
      const kids = { ...sftp.sftpKids };
      for (const path of Object.keys(kids)) if (path === r.path || path.startsWith(prefix)) delete kids[path];
      sftp.sftpKids = kids;
      await loadDir(parentOf(r.path), true);
    }
  } catch (e) { say(String(e), 'err'); }
  finally {
    const next = new Set(sftp.sftpDeleting);
    next.delete(key);
    sftp.sftpDeleting = next;
  }
}
export function sftpDownloadSelected() {
  if (sftp.sftpBusy || sftp.sftpXfer || sftp.sftpNativeDragging) return;
  const row = sftp.sftpRows.find((row) => row.path === sftp.sftpSel);
  if (row && !isSftpDeleting(row.path)) void sftpDownload(row);
}
export async function sftpDownload(r: SftpRow, archive = false) {
  if (sftp.sftpBusy || sftp.sftpXfer || sftp.sftpNativeDragging || isSftpDeleting(r.path)) return;
  const connId = sftp.sftpConnId;
  const directory = r.dir && !r.link;
  sftp.fctxMenu = null;
  sftp.sftpSel = r.path;
  sftp.sftpXfer = `选择 ${r.name} 的保存位置…`;
  let transferId = '';
  try {
    let local: string | null = null;
    if (directory && !archive) {
      const picked = await open({ directory: true, multiple: false, title: `选择「${r.name}」的本地保存目录` });
      local = typeof picked === 'string' ? picked : null;
    } else {
      local = await saveDialog({ defaultPath: archive ? `${r.name}.tar.gz` : r.name, title: `下载 ${r.name}（同名文件不会覆盖）` });
    }
    if (!local) return;
    const id = crypto.randomUUID();
    transferId = id;
    addSftpTransfer(id, r.name, 'download', connId, r.path);
    sftp.sftpDownloadTask = { id, name: r.name, drag: false, started: false, cancelling: false, progress: null };
    sftp.sftpXfer = `下载 ${r.name}…`;
    const onProgress = new Channel<DownloadProgress>();
    onProgress.onmessage = (progress) => {
      if (sftp.sftpDownloadTask?.id === id) sftp.sftpDownloadTask = { ...sftp.sftpDownloadTask, started: true, progress };
      progressSftpTransfer(id, progress);
    };
    const result = await invoke<DownloadResult>('sftp_download_entry', {
      connId, remote: r.path, local, directory, archive, prepareDrag: false, transferId: id, onProgress,
    });
    finishSftpTransfer(id, 'completed', { localPath: result.path, ...(result.skipped ? { error: `跳过 ${result.skipped} 个软链接或特殊文件；如需保留请使用打包下载` } : {}) });
    say(result.skipped ? `已下载到 ${result.path}；跳过 ${result.skipped} 个软链接或特殊文件，如需保留请使用打包下载` : `已下载到 ${result.path}`);
  }
  catch (e) {
    const cancelled = String(e).startsWith('下载已取消');
    if (transferId) finishSftpTransfer(transferId, cancelled ? 'cancelled' : 'failed', { error: String(e) });
    if (cancelled) say('已取消下载'); else say(String(e), 'err');
  }
  finally { sftp.sftpXfer = ''; sftp.sftpDownloadTask = null; }
}
async function cancelSftpDownload() {
  const task = sftp.sftpDownloadTask;
  if (!task?.started || task.cancelling) return;
  sftp.sftpDownloadTask = { ...task, cancelling: true };
  updateSftpTransfer(task.id, { cancelling: true });
  try { await invoke('sftp_cancel_download', { transferId: task.id }); }
  catch (e) {
    if (sftp.sftpDownloadTask?.id === task.id) sftp.sftpDownloadTask = { ...sftp.sftpDownloadTask, cancelling: false };
    updateSftpTransfer(task.id, { cancelling: false });
    say(String(e), 'err');
  }
}
export async function onSftpDragStart(e: DragEvent, r: SftpRow) {
  e.preventDefault(); e.stopPropagation();
  if (sftp.sftpBusy || isSftpDeleting(r.path) || sftp.sftpXfer || sftp.sftpNativeDragging) return;
  const connId = sftp.sftpConnId;
  const id = crypto.randomUUID();
  sftp.sftpSel = r.path;
  addSftpTransfer(id, r.name, 'download', connId, r.path, undefined, 'dragging');
  sftp.sftpDownloadTask = { id, name: r.name, drag: true, started: false, cancelling: false, progress: null };
  sftp.sftpXfer = `拖到本机 · ${r.name}`;
  sftp.sftpNativeDragging = true;
  sftp.fctxMenu = null;
  const onProgress = new Channel<DownloadProgress>();
  onProgress.onmessage = (progress) => {
    if (sftp.sftpDownloadTask?.id === id) sftp.sftpDownloadTask = { ...sftp.sftpDownloadTask, started: true, progress };
    progressSftpTransfer(id, progress);
  };
  const onEvent = new Channel<SftpDragEvent>();
  onEvent.onmessage = (event) => {
    if (sftp.sftpDownloadTask?.id !== id) return;
    if (event.phase === 'dragging') return;
    sftp.sftpNativeDragging = false;
    if (event.phase === 'dropped') {
      sftp.sftpDownloadTask = { ...sftp.sftpDownloadTask, started: true };
      updateSftpTransfer(id, { status: 'running', phase: 'connecting' });
      sftp.sftpXfer = `下载 ${r.name}…`;
      return;
    }
    const cancelling = sftp.sftpDownloadTask.cancelling;
    sftp.sftpXfer = ''; sftp.sftpDownloadTask = null;
    finishSftpTransfer(id, event.phase === 'completed' ? 'completed' : event.phase === 'error' ? 'failed' : 'cancelled', { localPath: event.path, error: event.error });
    if (event.phase === 'completed') say(`已下载到 ${event.path || '目标文件夹'}`);
    else if (event.phase === 'error') say(event.error || '下载失败，请重试', 'err');
    else if (cancelling) say('已取消下载');
  };
  try {
    await invoke('sftp_drag_download', { connId, remote: r.path, directory: r.dir && !r.link, transferId: id, onProgress, onEvent });
  } catch (err) {
    if (sftp.sftpDownloadTask?.id !== id) return;
    sftp.sftpNativeDragging = false; sftp.sftpXfer = ''; sftp.sftpDownloadTask = null;
    finishSftpTransfer(id, 'failed', { error: String(err) });
    say(String(err), 'err');
  }
}
export async function sftpUpload(dir = sftp.sftpPath) {
  if (sftp.sftpBusy || sftp.sftpXfer || sftp.sftpNativeDragging || !dir) return;
  const connId = sftp.sftpConnId;
  sftp.fctxMenu = null;
  sftp.sftpXfer = '选择要上传的文件…';
  let uploaded = 0;
  let failed = false;
  try {
    const picked = await open({ multiple: true, title: '选择要上传的文件' });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    for (const local of paths) {
      const name = localFileBasename(local);
      if (!name) throw new Error('本地文件名无效，未上传');
      await uploadSftpFile(connId, dir, name, local);
      uploaded++;
    }
  } catch (e) { failed = true; say(String(e), String(e).startsWith('上传已取消') ? undefined : 'err'); }
  finally {
    sftp.sftpXfer = '';
    if (uploaded && !failed) say(uploaded > 1 ? `已上传 ${uploaded} 个文件` : '已上传');
    if (sftp.sftpConnId === connId) await loadDir(dir, true);
  }
}
async function uploadSftpFile(connId: string, dir: string, name: string, local?: string, file?: File) {
  const id = crypto.randomUUID(), remote = sftpJoin(dir, name);
  addSftpTransfer(id, name, 'upload', connId, remote, local);
  sftp.sftpUploadTask = { id, started: false, cancelling: false };
  sftp.sftpXfer = `上传 ${name}…`;
  try {
    if (local) {
      const onProgress = new Channel<DownloadProgress>();
      onProgress.onmessage = progress => {
        if (sftp.sftpUploadTask?.id === id) sftp.sftpUploadTask = { ...sftp.sftpUploadTask, started: true };
        progressSftpTransfer(id, progress);
      };
      const result = await invoke<{ path: string; bytes: number; files: number }>('sftp_upload_entry', { connId, local, remote, transferId: id, onProgress });
      finishSftpTransfer(id, 'completed', { bytes: result.bytes, totalBytes: result.bytes, files: result.files, totalFiles: result.files });
    } else {
      if (!file) throw new Error('未取得本地文件');
      if (file.size > 25 * 1024 * 1024) throw new Error('此拖入方式上限为 25MB；请在「传输」中选择上传文件');
      updateSftpTransfer(id, { phase: 'uploading', totalFiles: 1 });
      const bytes = new Uint8Array(await file.arrayBuffer());
      await invoke('sftp_write', { connId, path: remote, b64: bytesToB64(bytes) });
      // WebView byte uploads do not expose per-chunk progress or cancellation.
      // Only record bytes once the backend confirms a completed write.
      finishSftpTransfer(id, 'completed', { bytes: file.size, totalBytes: file.size, files: 1, totalFiles: 1 });
    }
  } catch (e) {
    finishSftpTransfer(id, String(e).startsWith('上传已取消') ? 'cancelled' : 'failed', { error: String(e) });
    throw e;
  } finally { if (sftp.sftpUploadTask?.id === id) sftp.sftpUploadTask = null; }
}
/** Native local drops stream from verified paths; platforms without the bridge keep the byte fallback. */
async function sftpUploadDropped(files: File[], dir = sftp.sftpPath, connId = sftp.sftpConnId, probe?: LocalDropProbe) {
  if (!files.length || !dir || sftp.sftpXfer || sftp.sftpBusy || sftp.sftpNativeDragging) return;
  let uploaded = 0;
  let failed = false;
  let attempted = false;
  sftp.sftpXfer = '准备上传…';
  try {
    let verifiedPaths: string[] | undefined;
    if (probe) {
      const captured = await probe.capture;
      if (captured) {
        if (captured.count !== files.length || probe.entries.length !== files.length) {
          await invoke('sftp_discard_local_drop', { token: captured.token }).catch(() => {});
          throw new Error('本地拖入项目不匹配，未上传');
        }
        const result = await invoke<{ paths: string[] }>('sftp_consume_local_drop', { token: captured.token, entries: probe.entries });
        if (result.paths.length !== files.length || result.paths.some((path, index) => localFileBasename(path) !== files[index].name)) {
          throw new Error('本地拖入项目不匹配，未上传');
        }
        verifiedPaths = result.paths;
      } else if (probe.entries.some(entry => entry.directory)) {
        throw new Error('无法读取此次拖入的本地文件夹，未上传任何文件');
      }
    }
    for (const [index, file] of files.entries()) {
      const local = verifiedPaths?.[index] ?? (probe ? undefined : (file as File & { path?: string }).path);
      const name = local ? localFileBasename(local) : file.name;
      if (!name || name === '.' || name === '..' || name.includes('/') || name.includes('\0')) throw new Error('本地文件名无效，未继续上传');
      attempted = true;
      await uploadSftpFile(connId, dir, name, local, local ? undefined : file);
      uploaded++;
    }
  } catch (e) {
    failed = true;
    say(String(e), String(e).startsWith('上传已取消') ? undefined : 'err');
  } finally {
    sftp.sftpXfer = '';
    if (uploaded && !failed) say(uploaded > 1 ? `已上传 ${uploaded} 个项目` : '已上传');
    // A cancelled upload can retain partial remote data. Refresh only its
    // original connection, even if the user switched servers during capture.
    if (attempted && sftp.sftpConnId === connId) await loadDir(dir, true);
  }
}
export function onSftpDragOver(e: DragEvent) {
  if (!e.dataTransfer?.types?.includes('Files') || e.dataTransfer.types.includes('application/x-pilot-conversation')) return;
  e.preventDefault();
  const target = sftpDropTargetAt(e);
  e.dataTransfer.dropEffect = target ? 'copy' : 'none';
  sftp.sftpDropTarget = target;
  sftp.sftpFileOver = !!target;
  if (target) beginSftpLocalDrop();
  else discardSftpLocalDrop();
}
export function onSftpDragLeave(e: DragEvent) {
  const next = e.relatedTarget;
  if (!(next instanceof Node) || !(e.currentTarget as HTMLElement).contains(next)) {
    sftp.sftpFileOver = false; sftp.sftpDropTarget = null;
    discardSftpLocalDrop();
  }
}
export function onSftpDrop(e: DragEvent) {
  const target = sftpDropTargetAt(e);
  const files = Array.from(e.dataTransfer?.files ?? []);
  sftp.sftpFileOver = false; sftp.sftpDropTarget = null;
  if (!e.dataTransfer?.types.includes('Files') || e.dataTransfer.types.includes('application/x-pilot-conversation')) {
    discardSftpLocalDrop(); return;
  }
  e.preventDefault(); e.stopPropagation();
  if (!target || !files.length) { discardSftpLocalDrop(); return; }
  const entries = localDropEntries(e.dataTransfer, files);
  beginSftpLocalDrop(); // Also covers a very quick enter/drop without a dragover.
  const capture = sftp.sftpDropCapture ?? Promise.resolve(null);
  sftp.sftpDropCapture = null; // The accepted drop owns this token until consume.
  void sftpUploadDropped(files, target.remoteDir, target.connId, { capture, entries });
}

export function initSftp() {
  onDestroy(() => { clearTimeout(sftp.sftpHistoryTimer); persistSftpTransfers(true); });
  $effect(() => {
    // Invalidate a hover when its panel, connection or directory goes away.
    void app.view; void app.activeConnId; void workbench.wb.files; void sftp.sftpPath;
    return () => {
      sftp.sftpFileOver = false; sftp.sftpDropTarget = null;
      discardSftpLocalDrop();
    };
  });
  // 首次开文件面板或换了连接 → 从 home 起步；关了再开则保留原目录。
  $effect(() => {
    const id = app.activeConnId;
    if (app.view !== 'remote' || !app.isSshConn || !workbench.wb.files || !id) return;
    if (sftp.sftpConnId === id) return;
    sftp.sftpConnId = id; sftp.sftpPath = ''; sftp.sftpPathDraft = ''; sftp.sftpErr = '';
    sftp.sftpKids = {}; sftp.sftpOpenDirs = new Set(); sftp.sftpSel = '';
    void sftpGo('');
  });
  $effect(() => { if (sftp.sftpPathEdit && sftp.sftpPathEl) { sftp.sftpPathEl.focus(); sftp.sftpPathEl.select(); } });
  // 路径深了就把尾巴（当前目录）滚进视野——面包屑是从左往右长的。
  $effect(() => {
    void sftp.sftpCrumbs;
    if (sftp.crumbEl) sftp.crumbEl.scrollLeft = sftp.crumbEl.scrollWidth;
  });
  $effect(() => {
    if (!sftp.fctxMenu || !sftp.fctxMenuEl) return;
    const { x, y } = fitFileMenu(sftp.fctxMenu.x, sftp.fctxMenu.y, sftp.fctxMenuEl.offsetWidth, sftp.fctxMenuEl.offsetHeight, window.innerWidth, window.innerHeight);
    if (x !== sftp.fctxMenu.x || y !== sftp.fctxMenu.y) sftp.fctxMenu = { ...sftp.fctxMenu, x, y };
  });
  $effect(() => {
    if (!sftp.fctxMenu) return;
    const close = () => (sftp.fctxMenu = null);
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') sftp.fctxMenu = null; };
    // 冒泡阶段：菜单项自己的 onclick 先跑，再由这里收摊。
    window.addEventListener('click', close);
    window.addEventListener('contextmenu', close);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('contextmenu', close);
      window.removeEventListener('keydown', onKey);
    };
  });
  $effect(() => {
    if (sftp.fileRename && sftp.fileRename.connId !== sftp.sftpConnId) sftp.fileRename = null;
  });
}
