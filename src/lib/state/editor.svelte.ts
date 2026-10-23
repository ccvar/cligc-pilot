import { inspectEditorText, serializeEditorText } from '$lib/fileEditor';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { pilotWindowLabel } from './env';
import { confirmDialog, say, app } from './app.svelte.js';
import { b64ToBytes, strToB64 } from './workbench.svelte.js';
import { sftp, loadDir, parentOf } from './sftp.svelte.js';

class EditorState {
  // 在线编辑器
  edOpen = $state(false);
  edPath = $state('');
  edText = $state('');
  edLoading = $state(false);
  edSaving = $state(false);
  edErr = $state('');
  edReadOnly = $state(false);
  edConnId = '';
  edSession = 0;
  edOriginalB64 = '';
  edOriginal = $state('');
  edFormat = $state(inspectEditorText(''));
  edFull = $state(false);
  edFormatting = $state(false);
  edClosing = $state(false);
  edSaved = $state(false);
  edDirty = $derived(this.edText !== this.edOriginal);
}

export const editor = new EditorState();

export async function closeEdit(): Promise<boolean> {
  if (editor.edSaving || editor.edFormatting || editor.edClosing) return false;
  editor.edClosing = true;
  try {
    if (editor.edDirty && !await confirmDialog('这个文件有尚未保存的修改。放弃后无法恢复，远端文件不会改变。', {
      title: '放弃文件修改？', confirmText: '放弃修改', cancelText: '继续编辑',
    })) return false;
    editor.edOpen = false; ++editor.edSession;
    return true;
  } finally { editor.edClosing = false; }
}

export async function sftpEdit(path: string) {
  const connId = sftp.sftpConnId;
  if (!connId || (editor.edOpen && !await closeEdit())) return;
  const session = ++editor.edSession;
  editor.edConnId = connId; editor.edOriginal = ''; editor.edOriginalB64 = ''; editor.edFull = false; editor.edSaved = false;
  editor.edFormatting = false;
  editor.edOpen = true; editor.edPath = path; editor.edText = ''; editor.edErr = ''; editor.edReadOnly = false; editor.edLoading = true;
  try {
    const b64 = await invoke<string>('sftp_read', { connId, path });
    if (session !== editor.edSession) return;
    // fatal 模式：不是合法 UTF-8（二进制）直接抛 TypeError → 只给下载不给编辑
    const raw = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(b64ToBytes(b64));
    if (raw.includes('\0')) throw new TypeError('Binary file');
    editor.edOriginalB64 = b64;
    editor.edFormat = inspectEditorText(raw);
    editor.edText = editor.edOriginal = editor.edFormat.text;
  } catch (e) {
    if (session !== editor.edSession) return;
    editor.edReadOnly = true;
    editor.edErr = e instanceof TypeError ? '这是二进制文件，不能在线编辑；右键「下载」可以取回本地。' : String(e);
  } finally { if (session === editor.edSession) editor.edLoading = false; }
}
export async function saveEdit() {
  if (!editor.edOpen || editor.edLoading || editor.edSaving || editor.edFormatting || editor.edClosing || editor.edReadOnly || !editor.edDirty) return;
  editor.edSaving = true; editor.edErr = '';
  const connId = editor.edConnId, path = editor.edPath, text = editor.edText;
  try {
    if (editor.edFormat.mixed && !await confirmDialog(`这个文件混用了多种换行符。保存会统一为 ${editor.edFormat.eol === '\r\n' ? 'CRLF' : editor.edFormat.eol === '\r' ? 'CR' : 'LF'}，是否继续？`, {
      title: '确认换行方式', confirmText: '统一并保存', cancelText: '返回编辑',
    })) return;
    const current = await invoke<string>('sftp_read', { connId, path });
    if (current !== editor.edOriginalB64) throw new Error('远端文件已被其他操作修改，已停止保存以免覆盖。请先复制保留本次修改，再关闭并重新打开文件进行核对。');
    const b64 = strToB64(serializeEditorText(text, editor.edFormat));
    await invoke('sftp_write', { connId, path, b64 });
    editor.edOriginal = text; editor.edOriginalB64 = b64; editor.edFormat.mixed = false; editor.edSaved = true;
    say(`已保存 ${path}`);
    if (sftp.sftpConnId === connId) void loadDir(parentOf(path), true);
  } catch (e) { editor.edErr = String(e); }
  finally { editor.edSaving = false; }
}
export function onEditKey(e: KeyboardEvent) {
  if (e.defaultPrevented || app.confirmState) return;
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault();
    void saveEdit();
  } else if (e.key === 'Escape') {
    e.preventDefault();
    if (editor.edFull) editor.edFull = false;
    else void closeEdit();
  }
}

export function initEditor() {
  $effect(() => {
    if (!editor.edOpen) return;
    const unload = (event: BeforeUnloadEvent) => {
      if (editor.edOpen && (editor.edDirty || editor.edSaving || editor.edFormatting)) { event.preventDefault(); event.returnValue = ''; }
    };
    window.addEventListener('beforeunload', unload);
    // The main window closes to the tray (and keeps edits). Guard only windows
    // that actually close; otherwise the native tray handler hides the prompt.
    const unlisten = pilotWindowLabel !== 'main' ? getCurrentWindow().onCloseRequested(async event => {
      if (!editor.edOpen || (!editor.edDirty && !editor.edSaving && !editor.edFormatting)) return;
      event.preventDefault();
      if (await closeEdit()) await getCurrentWindow().close();
    }).catch(() => () => {}) : Promise.resolve(() => {});
    return () => { window.removeEventListener('beforeunload', unload); void unlisten.then(dispose => dispose()); };
  });
}
