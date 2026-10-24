import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { TurnEvent, Message, Conversation, TaskType } from '$lib/types';
import { savePrefs } from '$lib/defaults';
import { app, say, refreshConvos } from './app.svelte.js';
import { brainUsable } from './models';
import { panes, rememberSplit, finishSplitQueue, restoreSplitQueue } from './panes.svelte.js';
import { scrollSoon, thread } from './thread.svelte.js';
import { viewAfterConvGone } from './conversations';
import { composer, queuedPayloadText } from './composer.svelte.js';
import { launcher, STYLE_DIRECTIVES, workspaceFolderName } from './launcher.svelte.js';
import { mdRender, ATT_MARKER } from './messages.svelte.js';
import { cf, checkCfReady } from './cloudflare.svelte.js';
import { workbench, setWbFlags } from './workbench.svelte.js';
import { sites, SITE_PREFIX } from './sites.svelte.js';
import { selectConn } from './connections.svelte.js';

class TurnsState {
  autoRetried = $state<Record<string, boolean>>({}); // convId → 本轮已自动重试过（每个用户轮只自动重试一次）
  retryTimers: Record<string, ReturnType<typeof setTimeout>> = {}; // 待触发的自动重连定时器（新一轮开始时取消，防陈旧触发）
  // 智能升级：手动重试后若以同样的错误再次失败，说明 resume 救不了 → 之后只显示「重建继续」。
  retryExhausted = $state<Record<string, boolean>>({});
  // 重试上一轮：去掉失败的助手消息、用最后一条用户消息 resume 再跑（不新增用户气泡）。manual=用户手点（重置自动预算）。
  // ---------- 额度耗尽卡：倒计时 + 到点自动续跑 ----------
  limitAuto = $state<Record<string, number>>({}); // convId → 触发时刻 ms
  nowTick = $state(Date.now());
}

export const turns = new TurnsState();

// ---------- 运行一轮 ----------
export function makeChannel(convId: string): Channel<TurnEvent> {
  const ch = new Channel<TurnEvent>();
  ch.onmessage = (ev) => applyTurnEvent(convId, ev);
  return ch;
}

/** 无人值守的运营会话：后端把实时事件转成 ops-turn 广播；收到时建流式缓冲，打开会话就和普通会话一样实时显示。 */
export function initOpsLive() {
  $effect(() => {
    let done: Record<string, ReturnType<typeof setTimeout>> = {};
    const un = listen<{ conv_id: string; event: TurnEvent }>('ops-turn', (e) => {
      const { conv_id: id, event: ev } = e.payload;
      if (!app.lives[id]) {
        const now = Date.now();
        app.lives[id] = { text: '', renderedText: '', tools: [], activities: {}, lastActivityAt: now, error: '', failed: false, startedAt: now };
        app.running[id] = app.convos.find((c) => c.id === id)?.conn_id ?? 'ops';
      }
      applyTurnEvent(id, ev);
      if (ev.type === 'done') {
        clearTimeout(done[id]);
        // 后端跑完才落库最终消息；稍等一下再刷新，然后撤掉流式缓冲
        done[id] = setTimeout(async () => {
          await refreshConvos();
          if (app.activeConvId === id) app.activeConv = app.convos.find((c) => c.id === id) ?? app.activeConv;
          delete app.lives[id]; delete app.running[id];
        }, 1500);
      }
    });
    return () => { void un.then((f) => f()); Object.values(done).forEach(clearTimeout); done = {}; };
  });
}

function applyTurnEvent(convId: string, ev: TurnEvent) {
  {
    const buf = app.lives[convId];
    if (!buf) return; // 该对话已结束/被清（切走后仍可能收到尾包）
    if (ev.type === 'delta' || ev.type === 'text_reset') {
      // 恢复一次被截断的响应时，只撤回该响应的文字；此前的正文、工具和活动保留。
      buf.text = ev.type === 'text_reset' ? ev.text : buf.text + ev.text;
      // Markdown 的 {@html} 会替换其整棵子树。若直接在模板中调用 mdRender，
      // 生图/推理等 activity 更新也可能让正文跟着反复失效，形成可见闪空。
      // 把渲染结果缓存到正文事件上，任务卡更新时该字段保持引用和值都不变。
      buf.renderedText = mdRender(buf.text);
      buf.lastActivityAt = Date.now();
      if (app.activeConvId === convId) scrollSoon();
    }
    else if (ev.type === 'tool') {
      buf.tools = [...buf.tools, { label: ev.label, detail: ev.detail, at: buf.text.length }];
      buf.lastActivityAt = Date.now();
      if (app.activeConvId === convId) scrollSoon();
    }
    else if (ev.type === 'activity') {
      const now = Date.now();
      const previous = buf.activities[ev.activity_id];
      buf.activities = {
        ...buf.activities,
        [ev.activity_id]: {
          id: ev.activity_id,
          label: ev.label || previous?.label || '',
          detail: ev.detail || previous?.detail || '',
          status: ev.status,
          error: ev.error || previous?.error || '',
          kind: ev.kind || previous?.kind,
          phase: ev.phase || previous?.phase,
          current: ev.current ?? previous?.current,
          total: ev.total ?? previous?.total,
          startedAt: previous?.startedAt ?? now,
          updatedAt: now,
        },
      };
      buf.lastActivityAt = now;
      if (app.activeConvId === convId) scrollSoon();
    }
    else if (ev.type === 'context_compacted') {
      buf.compactedAt = Date.now();
      buf.compactedPreTokens = ev.pre_tokens;
      if (app.activeConvId === convId) {
        say('Claude 已自动整理上下文，对话记录保持不变');
        scrollSoon();
      }
    }
    else if (ev.type === 'done') { if (!ev.ok) { buf.error = ev.error; buf.failed = true; } }
  }
}
export function optimisticUser(text: string): Message {
  return { role: 'user', text, tools: [], ts: Math.floor(Date.now() / 1000), hidden: false, error: false };
}
// 首轮也能流式渲染 + 停止（cancel 键对准真正的注册表 key）。
export function beginTurn(convId: string, optimistic: Conversation) {
  if (turns.retryTimers[convId]) { clearTimeout(turns.retryTimers[convId]); delete turns.retryTimers[convId]; } // 任何新一轮开始都取消该会话待触发的自动重连
  const startedAt = Date.now();
  app.lives[convId] = {
    text: '',
    renderedText: '',
    tools: [],
    activities: {},
    lastActivityAt: startedAt,
    error: '',
    failed: false,
    startedAt,
  };
  app.running[convId] = optimistic.conn_id;
  if(panes.splitSnapshots[convId]) {
    rememberSplit(optimistic);
    app.convos=app.convos.map(c=>c.id===convId?optimistic:c);
    if(panes.conversationLayout || app.activeConvId!==convId) return;
  }
  app.activeConv = optimistic; app.activeConvId = convId; thread.threadModel = optimistic.model; thread.threadPerm = optimistic.perm_mode || 'full'; thread.threadEffort = optimistic.effort || '';
  cf.cfReady = false; // 本轮跑完再重新判定是否已建出内容
  // 远程连接：对话就在工作台底部面板里跑，别跳去独立对话页——那样终端就从眼前消失了。
  // （openConv/newChat/selectConn 同理，见各自注释。这条是「发消息新建对话」这一路。）
  if (app.conns.find((c) => c.id === optimistic.conn_id)?.kind === 'ssh') {
    app.view = 'remote';
    if (!workbench.wb.chat) setWbFlags({ chat: true });
  } else {
    app.view = 'thread';
  }
  scrollSoon(true);
}
export function endTurn(conv: Conversation | null, convId: string) {
  const failed = app.lives[convId]?.failed ?? false; // 删缓冲前先取，供等待消息门控用
  const completedTurnStartedAt = app.lives[convId]?.startedAt ?? 0;
  // 仅当用户仍停留在这条会话时用权威结果覆盖，避免打断已切到别的会话的用户。
  // 覆盖源按新鲜度择优：运行中改的档位/模型晚于轮末快照落库，刷新过的列表条目更全；
  // 但 refreshConvos 失败时列表是旧值（甚至 startChat 的乐观条目），无脑取列表会把
  // 刚跑完的整轮消息打回去——updated_at 更旧就回退用快照。
  if (conv) {
    const inList = app.convos.find((x) => x.id === convId);
    // 时间戳精度是秒：列表旧快照与轮末结果可能 updated_at 相同。相同时消息更多的才是真正的新版本。
    const fresh = !inList ? conv
      : inList.updated_at > conv.updated_at ? inList
        : inList.updated_at < conv.updated_at ? conv
          : inList.messages.length > conv.messages.length ? inList : conv;
    if (app.activeConvId === convId) { app.activeConv = fresh; thread.threadModel = fresh.model; thread.threadPerm = fresh.perm_mode || 'full'; thread.threadEffort = fresh.effort || ''; }
    if(panes.splitSnapshots[convId]) rememberSplit(fresh);
    app.convos = app.convos.some((item) => item.id === fresh.id)
      ? app.convos.map((item) => item.id === fresh.id ? fresh : item)
      : [fresh, ...app.convos];
  }
  delete app.running[convId];
  delete app.lives[convId];
  finishSplitQueue(convId, conv, failed, completedTurnStartedAt);
  if (app.activeConvId === convId) {
    checkCfReady(); // 这轮可能写了文件，重新判定预览/部署是否可用
    scrollSoon(); // 流式缓冲换成权威消息后高度还会变，贴底状态要延续。
  }
  // 等待消息：这轮**成功**结束、用户还在本会话，才把排队的那条发出去（失败/要重连时不发，避免连环失败）。
  if (!failed && composer.queued && composer.queued.convId === convId && app.activeConvId === convId) {
    const q = composer.queued;
    const lastUserText = [...(conv?.messages ?? app.activeConv?.messages ?? [])]
      .reverse()
      .find((message) => message.role === 'user')?.text.trim() ?? '';
    const staleQueue = !completedTurnStartedAt || q.turnStartedAt !== completedTurnStartedAt;
    const duplicateQueue = queuedPayloadText(q).trim() === lastUserText;
    composer.queued = null;
    if (staleQueue || duplicateQueue) {
      // 排队项必须来自刚结束的这一轮，并且不能与该轮原始请求完全相同。
      // 这同时挡住 HMR/旧状态残留和完成后的原消息重放。
      if (app.activeConvId === convId) say('已拦截重复的等待消息，未再次执行');
    } else {
      composer.draft = q.text; composer.attachments = q.atts;
      queueMicrotask(() => { void send(); });
    }
  }
  // 不在接近上限时自动 rebuild：rebuild 会创建全新的底层 Claude session，只能靠摘要
  // 衔接，等同于主动丢弃原生会话上下文。Claude Code 会自行管理长会话；若底层真的返回
  // 上下文超限，保留完整界面历史并让用户从错误卡明确选择「重建继续」。
}
export async function failTurn(e: unknown, convId: string) {
  if(panes.splitSnapshots[convId]) {panes.splitErrors[convId]=String(e);restoreSplitQueue(convId);}
  delete app.running[convId];
  delete app.lives[convId];
  // 这轮失败：不自动发排队消息，把它放回输入框让用户决定。
  if (composer.queued && composer.queued.convId === convId && app.activeConvId === convId) { composer.draft = composer.queued.text; composer.attachments = composer.queued.atts; composer.queued = null; }
  // 仅当用户仍在看这条会话时才弹错误提示——后台会话的失败不打断前台（成功也不弹，见 endTurn），
  // 失败态已落库，切回该会话即可看到。
  if (app.activeConvId === convId) say(String(e), 'err');
  await refreshConvos();
  const reloaded = await invoke<Conversation | null>('get_conversation', { id: convId });
  if(reloaded && panes.splitSnapshots[convId])rememberSplit(reloaded);
  if (reloaded) { if (app.activeConvId === convId) app.activeConv = reloaded; }
  else if (app.activeConvId === convId) { app.activeConv = null; viewAfterConvGone(); }
  if (app.activeConvId === convId) scrollSoon();
}

export async function startChat() {
  // 全部站点 / 托管运营里开的新对话：先进入所选站点，再按站点会话开始
  if (app.isSitesHub) {
    const sid = launcher.lSite;
    if (!sid || !sites.list.some((s) => s.id === sid) || !launcher.lDraft.trim()) return;
    await selectConn(SITE_PREFIX + sid);
    if (!app.isSiteConn) return;
  }
  const workspace = app.isWorkspaceChat;
  const site = app.isSiteConn ? sites.active : null;
  const targetReady = app.isSshConn || workspace || !!site || !!launcher.lSite.trim();
  const textReady = !!launcher.lDraft.trim() || (app.isSshConn && composer.attachments.length > 0);
  if (!app.activeConn || !targetReady || !textReady || !brainUsable(launcher.lBrain)) return;
  const taskType: TaskType = site ? 'site' : app.isSshConn ? 'remote' : app.isCfConn ? 'sitebuild' : app.isGhConn ? 'repo' : 'workspace';
  launcher.prefs.brain = launcher.lBrain; launcher.prefs.model = launcher.lModel; launcher.prefs.perm = launcher.lPerm; launcher.prefs.effort = launcher.lEffort; savePrefs(launcher.prefs);
  const model = launcher.lModel;
  // CF 建站：把所选视觉风格的 tokens 指令拼进首条消息（可见、可追溯）。
  const styleDir = app.isCfConn && launcher.lStyle ? STYLE_DIRECTIVES[launcher.lStyle] : '';
  let text = launcher.lDraft.trim() + (styleDir ? `\n\n【视觉风格】${styleDir}` : '');
  if (composer.attachments.length) {
    text += (text ? '\n\n' : '') + ATT_MARKER + '\n' + composer.attachments.map((a) => `- ${a.path}`).join('\n');
  }
  const id = crypto.randomUUID();
  const now = Math.floor(Date.now() / 1000);
  // 远程连接没有项目：slug 留空，侧栏分组名用 user@host（那就是这台机器的身份）。
  const mSlug = site ? site.id : workspace || app.isSshConn ? '' : launcher.lSite;
  const mName = site ? site.name : app.isSshConn ? `${app.activeConn.ssh_user}@${app.activeConn.ssh_host}` : workspace ? (launcher.workspaceDir ? workspaceFolderName(launcher.workspaceDir) : '自由对话') : launcher.lSite;
  const conversationConnId = app.activeConnId;
  const optimistic: Conversation = {
    id, conn_id: conversationConnId, conn_name: app.activeConn.name, site_slug: mSlug, site_name: mName,
    workspace_dir: workspace ? launcher.workspaceDir : '',
    task_type: taskType, brain: launcher.lBrain, model, perm_mode: launcher.lPerm, effort: launcher.lEffort, session_ref: '',
    title: text.slice(0, 30), messages: [optimisticUser(text)], status: 'running', created_at: now, updated_at: now,
  };
  // 立刻塞进侧栏，这样即便用户随后切走/新开对话，这条新会话也带着 running 圈可见，不必等这一轮跑完才出现。
  app.convos = [optimistic, ...app.convos];
  delete turns.autoRetried[id];
  beginTurn(id, optimistic);
  launcher.lDraft = ''; composer.attachments = [];
  try {
    const conv = await invoke<Conversation>('start_conversation', {
      convId: id, connId: conversationConnId, siteSlug: mSlug, siteName: mName,
      taskType, brain: launcher.lBrain, model, permMode: launcher.lPerm, effort: launcher.lEffort, fast: launcher.lFast,
      workspaceDir: workspace ? launcher.workspaceDir : '',
      message: text, onEvent: makeChannel(id),
    });
    await refreshConvos();
    const failed = app.lives[id]?.failed ?? false; const errText = app.lives[id]?.error ?? '';
    endTurn(conv, id);
    if (!failed && workspace) launcher.workspaceDir = '';
    maybeAutoRetry(id, failed, errText);
  } catch (e) { await failTurn(e, id); }
}

// ---------- 重试 / 自动重连 ----------
// 错误文本像不像"瞬时问题"（网络/限流/过载/5xx）——只对这类做自动重试，避免对密钥错误/取消等硬错误反复重跑。
function isTransient(err: string): boolean {
  if (!err || /已停止/.test(err)) return false;
  return /(网络|连接|超时|无法访问|timed?\s*out|timeout|overload|rate.?limit|too many requests|\b429\b|\b5\d\d\b|ECONN| ENET|EAI_AGAIN|socket hang|connection (reset|refused|closed|error)|temporar|try again|network error|fetch failed)/i.test(err);
}
export function armLimitAuto(convId: string, resetSec: number) {
  turns.limitAuto = { ...turns.limitAuto, [convId]: resetSec * 1000 + 90_000 }; // 官方重置后再缓 90 秒
}
export function disarmLimitAuto(convId: string) {
  const rest = { ...turns.limitAuto };
  delete rest[convId];
  turns.limitAuto = rest;
}

export async function retry(convId: string, manual = false) {
  if (app.running[convId]) return;
  const base = app.activeConvId === convId ? app.activeConv : app.convos.find((c) => c.id === convId);
  if (!base) return;
  if (!base.session_ref?.trim()) { if (manual) say('这个会话首轮就没建起来，无法重试；请回启动页重新发起。', 'err'); return; }
  if (manual) delete turns.autoRetried[convId];
  const msgs = base.messages.slice();
  const prevErr = msgs.length && msgs[msgs.length - 1].role === 'assistant' && msgs[msgs.length - 1].error ? msgs[msgs.length - 1].text.trim() : '';
  if (msgs.length && msgs[msgs.length - 1].role === 'assistant') msgs.pop(); // 去掉失败/部分的那条
  beginTurn(convId, { ...base, messages: msgs, status: 'running' });
  try {
    const conv = await invoke<Conversation>('retry_turn', { convId, onEvent: makeChannel(convId) });
    await refreshConvos();
    const failed = app.lives[convId]?.failed ?? false; const errText = app.lives[convId]?.error ?? '';
    endTurn(conv, convId);
    // 手动重试后同错再败 → resume 无效，收起「重试」只留「重建继续」；成功则解除。
    if (manual && failed && prevErr && errText.trim() === prevErr) turns.retryExhausted[convId] = true;
    if (!failed) delete turns.retryExhausted[convId];
    maybeAutoRetry(convId, failed, errText);
  } catch (e) { await failTurn(e, convId); }
}
// 重建会话续跑：底层会话状态损坏、重试无效时——换全新会话（带历史摘要）原地继续这条对话。
export async function rebuildSession(convId: string) {
  if (app.running[convId]) return;
  const base = app.activeConvId === convId ? app.activeConv : app.convos.find((c) => c.id === convId);
  if (!base) return;
  delete turns.autoRetried[convId];
  delete turns.retryExhausted[convId];
  const msgs = base.messages.slice();
  if (msgs.length && msgs[msgs.length - 1].role === 'assistant') msgs.pop();
  beginTurn(convId, { ...base, messages: msgs, status: 'running' });
  try {
    const conv = await invoke<Conversation>('rebuild_session', { convId, onEvent: makeChannel(convId) });
    await refreshConvos();
    endTurn(conv, convId); // 重建后不做自动重试，避免坏环境下打转
  } catch (e) { await failTurn(e, convId); }
}
// 这轮若因瞬时问题失败且本轮还没自动重试过，隔几秒自动重连一次；再不行就停手（留手动「重试」）。
export function maybeAutoRetry(convId: string, failed: boolean, errText: string) {
  const c = app.activeConvId === convId ? app.activeConv : app.convos.find((item) => item.id === convId);
  // Native API requests are independently billed and may have executed tools.
  // Keep retries explicit; the executor also refuses interrupted transcripts.
  if (c?.brain.startsWith('api:')) return;
  if (!failed || turns.autoRetried[convId] || !isTransient(errText)) return;
  turns.autoRetried[convId] = true;
  if (app.activeConvId === convId) say('网络/接口异常，正在自动重连重试…', 'err');
  // 存句柄，2.5s 后仅当没在跑且用户还停在本会话才自动重连（切走/又发新消息都不触发，避免抢焦点/陈旧重试）。
  turns.retryTimers[convId] = setTimeout(() => {
    delete turns.retryTimers[convId];
    if (!app.running[convId] && app.activeConvId === convId) void retry(convId, false);
  }, 2500);
}

export async function send() {
  if (!app.activeConv || app.running[app.activeConv.id]) return;
  const atts = composer.attachments;
  if (!composer.draft.trim() && !atts.length) return;
  let text = composer.draft.trim();
  if (atts.length) {
    text += (text ? '\n\n' : '') + ATT_MARKER + '\n' + atts.map((a) => `- ${a.path}`).join('\n');
  }
  composer.draft = ''; composer.attachments = [];
  const id = app.activeConv.id;
  delete turns.autoRetried[id]; // 新的用户轮：重置自动重试预算
  delete turns.retryExhausted[id];
  beginTurn(id, { ...app.activeConv, messages: [...app.activeConv.messages, optimisticUser(text)], status: 'running' });
  try {
    const conv = await invoke<Conversation>('send_message', { convId: id, message: text, onEvent: makeChannel(id) });
    await refreshConvos();
    const failed = app.lives[id]?.failed ?? false; const errText = app.lives[id]?.error ?? '';
    endTurn(conv, id);
    maybeAutoRetry(id, failed, errText);
  } catch (e) { await failTurn(e, id); }
}

export async function stop() { if (app.running[app.activeConvId]) { try { await invoke('cancel_turn', { convId: app.activeConvId }); } catch { /* */ } } }

export function initTurns() {
  $effect(() => {
    const t = setInterval(() => {
      turns.nowTick = Date.now();
      for (const [id, at] of Object.entries(turns.limitAuto)) {
        if (turns.nowTick >= at && !app.running[id]) {
          const rest = { ...turns.limitAuto };
          delete rest[id];
          turns.limitAuto = rest;
          void retry(id, false);
        }
      }
    }, 1000);
    return () => clearInterval(t);
  });
}
