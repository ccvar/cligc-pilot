// 远程连接分屏：从左栏把机器拖进主区，像自由对话那样上下左右分屏，每个窗格一台机器。
// 布局复用对话分屏那套（叶子里放连接 id），画布也是同一个组件。
// 聚焦的窗格就是「当前机器」：它显示完整工作台（终端 + 按需开的对话 / 文件）；其它窗格只显示各自的终端。
// 每台机器的终端常驻（见 workbench.svelte.ts 的终端登记表），换焦点、关窗格都不断开。
import { untrack } from 'svelte';
import { type Layout, type Edge, leaves, pane, replacePane, removePane, splitPane, outerDropEdge, dropEdge } from '$lib/conversationLayout';
import { app } from './app.svelte.js';
import { pilotWindowLabel } from './env';
import { selectConn } from './connections.svelte.js';

const KEY = `cligc.pilot.remoteLayout.${pilotWindowLabel}`;

function load(): Layout | null {
  try { const v = JSON.parse(localStorage.getItem(KEY) || 'null'); return v && typeof v === 'object' ? v : null; } catch { return null; }
}

class RemotePanesState {
  layout = $state<Layout | null>(load());
  focused = $state('');
  /** 拖动中的机器（连接 id） */
  dragId = $state('');
  /** 已经分屏时：落点窗格 + 边 */
  preview = $state<{ id: string; edge: Edge } | null>(null);
  /** 还没分屏时：落在主区的哪条边 */
  singleEdge = $state<Edge | null>(null);
  visible = $derived.by(() => !!this.layout && app.view === 'remote' && app.isSshConn
    && leaves(this.layout).length > 1 && leaves(this.layout).some((p) => p.conversation === app.activeConnId));
}

export const remotePanes = new RemotePanesState();

function save() {
  try {
    if (remotePanes.layout && leaves(remotePanes.layout).length > 1) localStorage.setItem(KEY, JSON.stringify(remotePanes.layout));
    else localStorage.removeItem(KEY);
  } catch { /* */ }
}
function setLayout(l: Layout | null) {
  remotePanes.layout = l && leaves(l).length ? l : null;
  save();
}

/** 点窗格：切到那台机器（左栏高亮、负载、对话和文件都跟着它）。 */
export function focusRemotePane(paneId: string) {
  const leaf = leaves(remotePanes.layout).find((p) => p.id === paneId);
  if (!leaf) return;
  remotePanes.focused = paneId;
  if (leaf.conversation !== app.activeConnId) void selectConn(leaf.conversation);
}

/** 把机器放进分屏：target 不给就切开当前聚焦的窗格。 */
export function addRemotePane(connId: string, edge: Edge, target?: string) {
  if (!app.conns.some((c) => c.id === connId && c.kind === 'ssh')) return;
  let l = remotePanes.layout;
  // 还没分屏：先把当前机器放进一个窗格
  if (!l || leaves(l).length < 2 || !leaves(l).some((p) => p.conversation === app.activeConnId)) {
    if (!app.isSshConn || connId === app.activeConnId) return;
    l = pane(app.activeConnId);
    target = l.id;
  }
  const at = target ?? leaves(l).find((p) => p.id === remotePanes.focused)?.id ?? leaves(l)[0].id;
  const next = splitPane(l, at, connId, edge);
  if (next === l) return;
  setLayout(next);
  const leaf = leaves(next).find((p) => p.conversation === connId);
  if (leaf) focusRemotePane(leaf.id);
}

/** 关掉一个窗格（终端只是挪去后台，不断开）。只剩一个时回到普通的单机工作台。 */
export function closeRemotePane(paneId: string) {
  if (!remotePanes.layout) return;
  const rest = removePane(remotePanes.layout, paneId);
  if (!rest) return;
  const left = leaves(rest);
  if (left.length < 2) {
    setLayout(null);
    if (left[0] && left[0].conversation !== app.activeConnId) void selectConn(left[0].conversation);
    return;
  }
  setLayout(rest);
  if (remotePanes.focused === paneId) focusRemotePane(left[0].id);
}

export function resizeRemoteLayout(l: Layout) { setLayout(l); }

export function endRemoteDrag() { remotePanes.dragId = ''; remotePanes.preview = null; remotePanes.singleEdge = null; }

/** 从左栏拖一台机器（按住拖动，超过 6px 才算拖；Esc 取消）。 */
export function startRemoteDrag(e: PointerEvent, connId: string) {
  if (e.button !== 0 || e.isPrimary === false) return;
  const startX = e.clientX, startY = e.clientY;
  let moved = false;
  const move = (ev: PointerEvent) => {
    if (!moved && Math.hypot(ev.clientX - startX, ev.clientY - startY) < 6) return;
    moved = true;
    remotePanes.dragId = connId; remotePanes.preview = null; remotePanes.singleEdge = null;
    const el = document.elementFromPoint(ev.clientX, ev.clientY);
    const target = el?.closest<HTMLElement>('[data-pilot-pane]');
    if (remotePanes.visible && target && remotePanes.layout) {
      const r = target.getBoundingClientRect();
      const canvas = target.closest('[data-pilot-canvas]')?.getBoundingClientRect();
      const outer = canvas ? outerDropEdge(ev.clientX - canvas.left, ev.clientY - canvas.top, canvas.width, canvas.height) : null;
      remotePanes.preview = outer && remotePanes.layout.kind === 'split'
        ? { id: remotePanes.layout.id, edge: outer }
        : { id: target.dataset.pilotPane!, edge: dropEdge(ev.clientX - r.left, ev.clientY - r.top, r.width, r.height) };
      // 拖到自己所在的窗格上：不算
      if (remotePanes.preview.id === leaves(remotePanes.layout).find((p) => p.conversation === connId)?.id) remotePanes.preview = null;
    } else if (!remotePanes.visible && app.view === 'remote' && app.isSshConn && connId !== app.activeConnId) {
      const area = el?.closest<HTMLElement>('[data-pilot-workarea]');
      if (area) { const r = area.getBoundingClientRect(); remotePanes.singleEdge = dropEdge(ev.clientX - r.left, ev.clientY - r.top, r.width, r.height); }
    }
  };
  const cleanup = () => {
    document.removeEventListener('pointermove', move); document.removeEventListener('pointerup', up);
    document.removeEventListener('pointercancel', cancel); document.removeEventListener('keydown', key); window.removeEventListener('blur', cancel);
  };
  const cancel = () => { cleanup(); endRemoteDrag(); };
  const key = (ev: KeyboardEvent) => { if (ev.key === 'Escape') cancel(); };
  const up = () => {
    const dest = remotePanes.preview, edge = remotePanes.singleEdge;
    cleanup(); endRemoteDrag();
    if (!moved) return;
    // 拖完不要再触发左栏那一下「点击切换机器」
    const block = (ev: MouseEvent) => { ev.preventDefault(); ev.stopImmediatePropagation(); };
    document.addEventListener('click', block, { capture: true, once: true });
    setTimeout(() => document.removeEventListener('click', block, true), 0);
    if (dest) addRemotePane(connId, dest.edge, dest.id);
    else if (edge) addRemotePane(connId, edge);
  };
  document.addEventListener('pointermove', move); document.addEventListener('pointerup', up);
  document.addEventListener('pointercancel', cancel); document.addEventListener('keydown', key); window.addEventListener('blur', cancel);
}

export function initRemotePanes() {
  // 左栏点了一台机器（或别处切换了连接）：已在分屏里就聚焦它的窗格，不在就换掉当前聚焦的那格
  $effect(() => {
    const id = app.activeConnId;
    if (!app.isSshConn) return;
    untrack(() => {
      const l = remotePanes.layout;
      if (!l || leaves(l).length < 2) return;
      const hit = leaves(l).find((p) => p.conversation === id);
      if (hit) { remotePanes.focused = hit.id; return; }
      const at = leaves(l).find((p) => p.id === remotePanes.focused) ?? leaves(l)[0];
      setLayout(replacePane(l, at.id, id));
      remotePanes.focused = at.id;
    });
  });
  // 连接被删了：从分屏里拿掉
  $effect(() => {
    const ids = new Set(app.conns.filter((c) => c.kind === 'ssh').map((c) => c.id));
    if (!app.conns.length) return; // 还没加载
    untrack(() => {
      let l = remotePanes.layout;
      if (!l) return;
      for (const p of leaves(l)) if (!ids.has(p.conversation)) l = l ? removePane(l, p.id) : null;
      if (l !== remotePanes.layout) setLayout(l && leaves(l).length > 1 ? l : null);
    });
  });
}
