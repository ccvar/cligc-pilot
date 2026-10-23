import { invoke } from '@tauri-apps/api/core';
import type { Conversation } from '$lib/types';
import { leaves, replacePane } from '$lib/conversationLayout';
import { app, say, refreshConvos, confirmDialog } from './app.svelte.js';
import { brainUsable, firstUsableBrain, brainAvailability } from './models';
import { selectConn } from './connections.svelte.js';
import { panes, switchConversationWorkspace, conversationWorkspace, rememberSplit, splitConversation, loadPaneMetadata, focusSplit, closeSplit } from './panes.svelte.js';
import { thread, scrollSoon } from './thread.svelte.js';
import { composer } from './composer.svelte.js';
import { launcher } from './launcher.svelte.js';
import { checkCfReady } from './cloudflare.svelte.js';
import { workbench, setWbFlags } from './workbench.svelte.js';
import { expandSite, conversationGroupKey } from './sidebar.svelte.js';
import { sites } from './sites.svelte.js';

// ---------- 对话导航 ----------
export function newChat() {
  ++panes.openConvSeq;
  app.activeConvId = ''; app.activeConv = null; launcher.lDraft = '';
  composer.draft='';composer.attachments=[];
  if (app.isWorkspaceChat) launcher.lPerm = 'full';
  if (app.isSitesHub) {
    // 还没进具体站点：默认选最近用过的站点（其次置顶的、第一个），左下角可换
    const ids = new Set(sites.list.map((s) => s.id));
    if (!ids.has(launcher.lSite)) launcher.lSite = sites.recent.find((id) => ids.has(id)) ?? sites.list.find((s) => s.pinned)?.id ?? sites.list[0]?.id ?? '';
    if (launcher.lPerm === 'full') launcher.lPerm = 'auto';
  }
  // 远程连接没有启动页：新对话＝把底部对话面板腾空（工作台留在原地，终端不断）。
  // 它不换「页面」，所以必须给点看得见的反馈：把面板打开并把光标放进输入框。
  viewAfterConvGone();
  if (app.isSshConn) {
    if (!workbench.wb.chat) setWbFlags({ chat: true });
    requestAnimationFrame(() => workbench.wbStartEl?.focus());
  }
  if (!launcher.lBrain.startsWith('api:') && !brainUsable(launcher.lBrain)) launcher.lBrain = firstUsableBrain();
  if (launcher.lBrain.startsWith('api:') && !brainUsable(launcher.lBrain)) say(`${brainAvailability(launcher.lBrain)}，请在设置中检查 API 服务或手动选择其他模型。`, 'err');
}
export async function openConv(id: string) {
  const request=++panes.openConvSeq;
  const c = await invoke<Conversation | null>('get_conversation', { id });
  if(request!==panes.openConvSeq)return;
  if (!c) { await refreshConvos(); return; }
  // Resolve the workspace BEFORE taking the split-pane fast path. Otherwise an old
  // pane survives when a skill/search result opens in a different workspace.
  switchConversationWorkspace(conversationWorkspace(c));
  // 打开的对话可能属于别的连接（从搜索进来）——先切到它所属的连接。
  if (c.conn_id !== app.activeConnId) void selectConn(c.conn_id);
  if (panes.conversationLayout) {
    rememberSplit(app.running[id] ? splitConversation(id) ?? c : c);
    void loadPaneMetadata(c);
    const existing=leaves(panes.conversationLayout).find(p=>p.conversation===id);
    if(existing)focusSplit(existing.id);
    else {
      const target=leaves(panes.conversationLayout).find(p=>p.id===panes.splitFocused)??leaves(panes.conversationLayout)[0];
      panes.conversationLayout=replacePane(panes.conversationLayout,target.id,id);focusSplit(target.id);
    }
    app.view='thread';
    return;
  }
  app.activeConv = c; app.activeConvId = id; thread.threadModel = c.model; thread.threadPerm = c.perm_mode || 'full'; thread.threadEffort = c.effort || '';
  // 远程连接的对话在工作台里开，不跳去独立对话页。
  // 命令行开着就让它开着 —— 面板开关是用户自己拨的，翻个旧对话不该替他改布局。
  if (app.conns.find((x) => x.id === c.conn_id)?.kind === 'ssh') {
    app.view = 'remote';
    if (!workbench.wb.chat) setWbFlags({ chat: true });
  } else app.view = 'thread';
  composer.attachments = []; composer.queued = null; // 换会话清掉未发送的附件 / 等待消息
  expandSite(conversationGroupKey(c));
  checkCfReady();
  scrollSoon(true);
}

export async function deleteConv(id: string) {
  // 对话进行中不删：否则删掉会话行会孤儿掉后台在跑的 CLI 子进程 + 触发「会话丢失」。先停止再删。
  if (app.running[id]) { say('对话进行中，请先点停止再删除。', 'err'); return; }
  const c = app.convos.find((x) => x.id === id);
  const label = c?.title?.trim() ? `「${c.title.trim()}」` : '这条对话';
  const yes = await confirmDialog(`删除对话${label}？聊天记录不可恢复。`, { title: '删除对话', kind: 'danger' });
  if (!yes) return;
  try {
    await invoke('delete_conversation', { id });
    const leaf=leaves(panes.conversationLayout).find(p=>p.conversation===id);
    if(leaf)closeSplit(leaf.id);
    delete panes.splitSnapshots[id];delete panes.splitDrafts[id];delete panes.splitQueued[id];delete panes.splitErrors[id];
    if (app.activeConvId === id) { app.activeConvId = ''; app.activeConv = null; viewAfterConvGone(); }
    await refreshConvos();
  } catch (e) { say(String(e), 'err'); }
}
// 乐观：立刻把用户消息塞进当前/合成会话，activeConvId 立即等于 convId，
/** 当前对话没了（删掉/丢失/新建）之后该落到哪儿。
 *  ★ 远程连接永远留在工作台（底部面板自己退回起点输入框）——**别在各处硬写 view='launcher'**，
 *  那会把人踢出工作台、终端从眼前消失。ssh 连接压根没有启动页。 */
export function viewAfterConvGone() { app.view = app.isSshConn ? 'remote' : 'launcher'; }
