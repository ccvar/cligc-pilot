import { invoke } from '@tauri-apps/api/core';
import { app, say } from './app.svelte.js';
import { splitConversation, splitUpload, panes } from './panes.svelte.js';
import { send } from './turns.svelte.js';
import { conversationProject, isImgPath, ATT_MARKER } from './messages.svelte.js';
import { cf } from './cloudflare.svelte.js';

// ---------- 附件：粘贴/拖拽文件到输入框，存进项目目录供 AI 读取 ----------
export type Attach = { name: string; path: string; preview: string };

// 等待消息：这轮还在跑时先把下一条排进队列，等回合结束自动发；发出前可编辑/清除。绑定 convId，切走即清。
export type QueuedMessage = {
  convId: string;
  text: string;
  atts: Attach[];
  /** 排队动作所属的运行轮；轮末只消费同一轮创建的消息。 */
  turnStartedAt: number;
};

class ComposerState {
  draftEl = $state<HTMLTextAreaElement | undefined>();

  // ---------- composer / live turn ----------
  draft = $state('');
  attachments = $state<Attach[]>([]);
  attaching = $state(false);
  attachingCount = 0;
  composerFileOver = $state(false);
  queued = $state<QueuedMessage | null>(null);
  // 输入法组合中（候选框开着）按回车是确认候选词，不能当发送。isComposing 在部分 webview 不可靠，
  // 叠加手动组合标记 + keyCode===229（IME 处理中）双保险。
  composing = false;
}

export const composer = new ComposerState();

// 自定义模型输入框的占位示例，按当前引擎给不同提示。
// ---------- composer 自动撑高 ----------
// 有内容随行数长高，到上限后固定内部滚动；清空/发送后回到初始高度（对标 Claude Code）。
export function autogrow(node: HTMLTextAreaElement, max = 220) {
  const min = node.offsetHeight; // 以挂载时的初始高度为下限（启动器 3 行、会话 1 行各自保形）
  const resize = () => {
    node.style.height = 'auto';
    const h = Math.max(min, Math.min(node.scrollHeight, max));
    node.style.height = h + 'px';
    node.style.overflowY = node.scrollHeight > max ? 'auto' : 'hidden';
  };
  node.addEventListener('input', resize);
  requestAnimationFrame(resize);
  (node as unknown as { __autogrow?: () => void }).__autogrow = resize;
  return { destroy: () => node.removeEventListener('input', resize) };
}
export function legacyComposer(){return {text:composer.draft,files:composer.attachments,queued: composer.queued,cfReady: cf.cfReady};}
export function setLegacyDraft(text:string){composer.draft=text;}
export function attachmentTarget(): { connId: string; project: string } | null {
  // 远程工作台的首条消息还没有 Conversation，但连接本身已经有本地工作目录。
  // 进入已有对话后仍以该对话为准；避免把另一个连接残留的 activeConv 当成当前目标。
  if (app.activeConv && (app.view !== 'remote' || app.activeConv.conn_id === app.activeConnId)) {
    return { connId: app.activeConv.conn_id, project: conversationProject(app.activeConv) };
  }
  if (app.view === 'remote' && app.isSshConn && app.activeConnId) return { connId: app.activeConnId, project: '' };
  return null;
}
export function attachmentFilename(f: File): string {
  if (f.name.trim()) return f.name.trim();
  const ext = ({ 'image/png': 'png', 'image/jpeg': 'jpg', 'image/gif': 'gif', 'image/webp': 'webp' } as Record<string, string>)[f.type] || 'bin';
  return `clipboard-${Date.now()}.${ext}`;
}
export async function attachFile(f: File) {
  const target = attachmentTarget();
  if (!target) { say('先进入一个对话再添加文件', 'err'); return; }
  if (f.size > 25 * 1024 * 1024) { say('文件太大（上限 25MB）', 'err'); return; }
  const filename = attachmentFilename(f);
  composer.attachingCount++;
  composer.attaching = true;
  try {
    const buf = new Uint8Array(await f.arrayBuffer());
    const path = await invoke<string>('save_attachment', { connId: target.connId, project: target.project, filename, data: Array.from(buf) });
    let preview = '';
    if (f.type.startsWith('image/') || isImgPath(filename)) {
      preview = await new Promise<string>((res) => { const r = new FileReader(); r.onload = () => res(String(r.result)); r.onerror = () => res(''); r.readAsDataURL(f); });
    }
    composer.attachments = [...composer.attachments, { name: path.split('/').pop() || filename, path, preview }];
  } catch (e) { say(String(e), 'err'); }
  finally { composer.attachingCount--; composer.attaching = composer.attachingCount > 0; }
}
export function removeAttachment(i: number) { composer.attachments = composer.attachments.filter((_, idx) => idx !== i); }
export function queuedPayloadText(message: QueuedMessage): string {
  if (!message.atts.length) return message.text;
  return message.text
    + (message.text ? '\n\n' : '')
    + ATT_MARKER + '\n'
    + message.atts.map((attachment) => `- ${attachment.path}`).join('\n');
}
export function queueMessage() {
  if (!app.activeConv) return;
  if (!composer.draft.trim() && !composer.attachments.length) return;
  if (!app.running[app.activeConv.id]) { void send(); return; } // 其实没在跑就直接发
  const t = composer.draft.trim();
  const turnStartedAt = app.lives[app.activeConv.id]?.startedAt ?? Date.now();
  // 已有排队则合并进同一条（不覆盖丢字）；否则新建。
  composer.queued = composer.queued && composer.queued.convId === app.activeConv.id
    ? {
        convId: app.activeConv.id,
        text: [composer.queued.text, t].filter(Boolean).join('\n'),
        atts: [...composer.queued.atts, ...composer.attachments],
        turnStartedAt,
      }
    : { convId: app.activeConv.id, text: t, atts: composer.attachments, turnStartedAt };
  composer.draft = ''; composer.attachments = [];
}
export function editQueued() {
  if (!composer.queued) return;
  composer.draft = composer.queued.text; composer.attachments = composer.queued.atts; composer.queued = null;
}
export function clearQueued() { composer.queued = null; }
export function onComposerPaste(e: ClipboardEvent, paneId='') {
  const data = e.clipboardData;
  if (!data) return;
  const files = Array.from(data.files);
  if (!files.length) {
    for (const item of Array.from(data.items)) {
      if (item.kind !== 'file') continue;
      const file = item.getAsFile();
      if (file) files.push(file);
    }
  }
  const handled = files.length > 0;
  const c=paneId?splitConversation(paneId):null;
  for (const file of files) {
    if(c)void splitUpload(c,file).catch(error=>{panes.splitErrors[c.id]=String(error);});
    else void attachFile(file);
  }
  if (handled) e.preventDefault();
}
export function onComposerDrop(e: DragEvent) {
  composer.composerFileOver = false;
  const files = e.dataTransfer?.files;
  if (files && files.length) { e.preventDefault(); for (const f of Array.from(files)) void attachFile(f); }
}
export function onComposerDragOver(e: DragEvent) {
  if (!e.dataTransfer?.types?.includes('Files')) return;
  e.preventDefault();
  e.dataTransfer.dropEffect = 'copy';
  composer.composerFileOver = true;
}
export function onComposerDragLeave(e: DragEvent) {
  const next = e.relatedTarget;
  if (!(next instanceof Node) || !(e.currentTarget as HTMLElement).contains(next)) composer.composerFileOver = false;
}
export function onComposerKey(e: KeyboardEvent, fn: () => void) {
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing && !composer.composing && e.keyCode !== 229) { e.preventDefault(); fn(); }
}

export function initComposer() {
  // 程序化清空（发送/排队/切会话）不触发 input 事件，这里补一脚
  $effect(() => { void composer.draft; const el = composer.draftEl as unknown as { __autogrow?: () => void } | undefined; el?.__autogrow?.(); });
}
