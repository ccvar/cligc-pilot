import { invoke } from '@tauri-apps/api/core';
import type { Conversation, Brain } from '$lib/types';
import type { ModelOption } from '$lib/modelSelection';
import { tick } from 'svelte';
import { app, say, confirmDialog } from './app.svelte.js';
import { brainUsable, isCodexSsh, CODEX_SSH_NOTE, brainAvailability, launcherModelOpts, modelIcon, apiProfile, brainLabel } from './models';
import { conversationConnection, panes } from './panes.svelte.js';
import { rebuildSession } from './turns.svelte.js';

class ThreadState {
  // 会话可切换模型；threadModel 跟随活动会话、改动即持久化。
  threadModel = $state('');
  threadComboOpts = $derived(conversationModelOptions(app.activeConv));
  // 进行中会话可切换权限档位：claude 的钩子/参数和 codex 的 sandbox 都是每轮下发的，
  // 改完从下一轮（含排队消息）生效；正在跑的那轮已带旧档位启动，改动拦不住它（想拦先停止）。
  threadPerm = $state('');
  threadEffort = $state('');
  threadEl = $state<HTMLDivElement | null>(null);
  threadInnerEl = $state<HTMLDivElement | null>(null);
  // 是否应随新消息贴住底部。用户主动向上翻后会关闭，回到底部后自动恢复。
  threadStickBottom = true;
  threadScrollQueued = false;
  threadScrollForce = false;
  threadSecondRaf = 0;

  viewBusy = $derived(!!app.running[app.activeConvId]); // 当前查看的对话是否在跑
  liveView = $derived(app.lives[app.activeConvId]); // 当前对话的流式缓冲（可能 undefined）

  // 加载中的"耗时"计时：仅在当前对话跑着时开一个轻量心跳刷新 nowMs，停了自动清掉。
  nowMs = $state(0);
  shownMessages = $derived((app.activeConv?.messages ?? []).filter((m) => !m.hidden));
}

export const thread = new ThreadState();

async function persistThreadModel(m: string) {
  if (!app.activeConv || m === app.activeConv.model) return;
  const resetsSession = app.activeConv.brain === 'claude' || app.activeConv.brain.startsWith('api:');
  try {
    const u = await invoke<Conversation | null>('set_conversation_model', { convId: app.activeConv.id, model: m });
    if (u && app.activeConvId === u.id) app.activeConv = u;
    // claude 的 --resume 无视 --model（后端因此清了 session）：如实告知生效方式。
    if (resetsSession) say('模型已切换：下一条消息将以历史摘要重建会话后用新模型跑');
  } catch (e) { say(String(e), 'err'); }
}
// 会话内模型面板：三家厂商的模型都列出（值编码 "<brain>::<model>"，同启动器 comboOpts；行首图标区分）。
// 当前厂商永远可选（保持原行为）；其他厂商未就绪（未装/未登录）整组置灰。当前厂商排前面。
export function conversationModelOptions(c:Conversation|null) {
  const cur = (c?.brain ?? 'claude') as Brain;
  const order: Brain[] = [cur, ...app.ALL_BRAINS.filter((b) => b !== cur)];
  const out: ModelOption[] = [];
  for (const b of order) {
    // 当前厂商永远可选（会话已经在用它了，灰掉只会让触发器显示成一个选不中的项）。
    // 远程连接下的 codex 不置灰、只挂注记：它能用，只是那道闸是打折的（完整说法见 permTipFor）。
    const usable = b.startsWith('api:') ? brainUsable(b) : b === cur || brainUsable(b);
    const note = isCodexSsh(b, conversationConnection(c)?.kind==='ssh') ? CODEX_SSH_NOTE : brainAvailability(b);
    for (const m of launcherModelOpts(b)) {
      out.push({ value: `${b}::${m.value}`, label: m.label, sub: note || m.sub, icon: modelIcon(b), disabled: !usable, apiProtocol: apiProfile(b)?.protocol });
    }
  }
  // 会话当前模型不在清单里（比如自定义 ID 事后被删）：补一条，触发器别显示成裸编码值。
  const curVal = `${cur}::${c?.model}`;
  if (!out.some((o) => o.value === curVal)) out.unshift({ value: curVal, label: c?.model || '模型', sub: brainAvailability(cur) || '当前会话', icon: modelIcon(cur), disabled: cur.startsWith('api:') && !brainUsable(cur), apiProtocol: apiProfile(cur)?.protocol });
  return out;
}
// 会话内选模型：同厂商＝只改存储、下一轮生效（原行为不变）；跨厂商＝底层 session 换不了家，
// 确认后更新会话 brain/model（后端顺带清 session_ref；非 ssh 的 codex 下 ask/auto 落 full，
// ssh 下**保持原档**——桥那道卡是 codex 仅剩的闸，见 lib.rs::apply_brain_switch），
// 再走「重建继续」：全新底层会话 + 历史摘要，自动重跑最近一条用户请求。
export async function pickThreadCombo(v: string) {
  const conv = app.activeConv;
  if (!conv || app.running[conv.id]) return;
  const i = v.indexOf('::');
  if (i < 0) return;
  const brain = v.slice(0, i) as Brain;
  const model = v.slice(i + 2);
  if (brain === conv.brain) { thread.threadModel = model; void persistThreadModel(model); return; }
  const label = brainLabel(brain);
  // ★ 这个框就是「在真机上自动重跑」的同意点：切完立刻 rebuildSession 重跑最近一条请求。
  // 切到 codex 的风险必须**当场**说，不能只靠权限档位 tip 那种要悬停才出的——用户点完就跑了。
  const risk = isCodexSsh(brain, app.activeConvIsSsh)
    ? `\n\n注意：Codex 无头模式没有逐命令闸。经 Pilot 跑的远程命令仍会弹卡${thread.threadPerm === 'full' ? '（但「全自动」档下这些卡也全关了）' : ''}，但它可以绕开 Pilot、直接用你本机的 ssh 和 ~/.ssh 密钥连这台机器——那条路一张卡都不弹。`
    : '';
  const yes = await confirmDialog(
    `切到 ${label} 后，这条对话将以历史摘要重建一个全新底层会话继续（自动重跑最近一条请求）。项目文件不受影响；很长的对话会有一定上下文损耗。${risk}`,
    { title: `切换到 ${label}`, kind: 'warning' },
  );
  if (!yes) return;
  try {
    const u = await invoke<Conversation | null>('set_conversation_brain_model', { convId: conv.id, brain, model });
    if (u && app.activeConvId === u.id) { app.activeConv = u; thread.threadModel = u.model; thread.threadPerm = u.perm_mode || 'full'; thread.threadEffort = u.effort || ''; }
    void rebuildSession(conv.id);
  } catch (e) { say(String(e), 'err'); }
}
export async function persistThreadEffort(v: string) {
  if (!app.activeConv || v === (app.activeConv.effort || '')) return;
  try {
    const u = await invoke<Conversation | null>('set_conversation_effort', { convId: app.activeConv.id, effort: v });
    if (u && app.activeConvId === u.id) { app.activeConv = u; thread.threadEffort = u.effort || ''; }
    if (thread.viewBusy) say('推理强度已调整，从下一轮开始生效');
  } catch (e) { say(String(e), 'err'); }
}
export async function persistThreadFast(v: boolean) {
  if (!app.activeConv || v === (app.activeConv.fast ?? false)) return;
  try {
    const u = await invoke<Conversation | null>('set_conversation_fast', { convId: app.activeConv.id, fast: v });
    if (u && app.activeConvId === u.id) app.activeConv = u;
    if (thread.viewBusy) say('Fast 模式已调整，从下一轮开始生效');
  } catch (e) { say(String(e), 'err'); }
}
export async function persistThreadPerm(p: string) {
  if (!app.activeConv || p === (app.activeConv.perm_mode || 'full')) return;
  try {
    const u = await invoke<Conversation | null>('set_conversation_perm_mode', { convId: app.activeConv.id, permMode: p });
    // 回写 threadPerm：若这段 RPC 期间轮次刚好收尾，endTurn 可能已把下拉打回旧值，这里以落库值自愈。
    if (u && app.activeConvId === u.id) { app.activeConv = u; thread.threadPerm = u.perm_mode || 'full'; }
    // 放宽到全自动＝实时生效（后端把本轮已弹/将弹的批准卡全部自动放行）；
    // 收紧方向做不到——本轮是带旧档启动的，钩子/参数拦不回来，只能下一轮。
    if (thread.viewBusy) say(p === 'full' ? '已切全自动：本轮剩余操作不再弹卡' : '权限档位已收紧，从下一轮开始生效（正在跑的这轮拦不回来）');
  } catch (e) { say(String(e), 'err'); }
}

function threadNearBottom(el: HTMLDivElement): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < 160;
}
export function onThreadScroll(e: Event) {
  thread.threadStickBottom = threadNearBottom(e.currentTarget as HTMLDivElement);
}
export async function scrollSoon(force = false) {
  // 关键是先沿用内容增长前的贴底意图，不能等 DOM 变高后再判断距离；大段增量会被误判为用户上翻。
  if (force) thread.threadStickBottom = true;
  if (!force && !thread.threadStickBottom) return;
  thread.threadScrollForce ||= force;
  if (thread.threadScrollQueued) return;
  thread.threadScrollQueued = true;
  await tick();
  requestAnimationFrame(() => {
    const forced = thread.threadScrollForce;
    thread.threadScrollForce = false;
    thread.threadScrollQueued = false;
    const el = thread.threadEl;
    if (!el || (!forced && !thread.threadStickBottom)) return;
    el.scrollTop = el.scrollHeight;
    // WebView2 的 markdown/字体布局可能比首帧晚一拍，再校准一次消掉最后几行的缺口。
    cancelAnimationFrame(thread.threadSecondRaf);
    thread.threadSecondRaf = requestAnimationFrame(() => {
      const latest = thread.threadEl;
      if (latest && (forced || thread.threadStickBottom)) latest.scrollTop = latest.scrollHeight;
    });
  });
}

export function initThread() {
  $effect(() => {
    if (!thread.viewBusy && !(panes.splitVisible && Object.keys(app.running).length)) return;
    thread.nowMs = Date.now();
    const t = setInterval(() => { thread.nowMs = Date.now(); }, 500);
    return () => clearInterval(t);
  });
  // 图片加载、markdown 重排、工具卡展开也会改变正文高度；只有仍贴底时才跟随。
  $effect(() => {
    const inner = thread.threadInnerEl;
    if (!inner) return;
    const ro = new ResizeObserver(() => { if (thread.threadStickBottom) void scrollSoon(); });
    ro.observe(inner);
    return () => { ro.disconnect(); cancelAnimationFrame(thread.threadSecondRaf); };
  });
}
