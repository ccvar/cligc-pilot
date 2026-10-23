import type { ToolCall, Conversation } from '$lib/types';
import { invoke } from '@tauri-apps/api/core';
import { leaves } from '$lib/conversationLayout';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
import { app, say } from './app.svelte.js';
import { panes, splitConversation } from './panes.svelte.js';
import { ui } from './ui.svelte.js';

type ExecutionActivity = {
  id: string;
  label: string;
  detail: string;
  error: string;
  status: 'running' | 'completed' | 'failed' | 'canceled';
  kind?: string;
  phase?: string;
  current?: number;
  total?: number;
  startedAt: number;
  updatedAt: number;
};
export type LiveTurn = {
  text: string;
  /** 只在正文 delta 到达时更新；任务活动变化不能重建正文 DOM。 */
  renderedText: string;
  tools: ToolCall[];
  activities: Record<string, ExecutionActivity>;
  lastActivityAt: number;
  error: string;
  failed: boolean;
  startedAt: number;
  compactedAt?: number;
  compactedPreTokens?: number;
};

class MediaState {
  // 图片附件缩略图：消息里存的是相对工作目录的 uploads/ 路径，webview 读不了本地文件，
  // 走后端按「连接+项目」解析后读成 data URI；缓存键带上二者，避免不同会话同名文件串图。
  // data URI 可观（≤8MB 原图 → ~10MB 字符串），切会话时把其他会话的条目逐出，防止只增不减；
  // 顺带让上次读失败（''）的条目在重进会话时有重试机会。
  thumbs = $state<Record<string, string>>({});
}

export const media = new MediaState();

export function elapsedLabel(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${String(s % 60).padStart(2, '0')}s`;
}
export function activityLabel(activity: ExecutionActivity): string {
  const phase = activity.phase?.trim() ?? '';
  if (phase) {
    // 原生活动在开始与结束时复用同一 phase。按真实状态改写，否则已经完成的
    // 上传仍会显示“正在上传”，旧客户端留下的“正在执行命令”也会继续暴露技术概念。
    if (phase === '正在执行命令' || phase === '正在处理任务步骤') {
      return activity.status === 'failed' ? '任务步骤未成功'
        : activity.status === 'completed' ? '任务步骤已完成'
        : '正在处理任务步骤';
    }
    if (phase === '正在检查图片文件') {
      return activity.status === 'failed' ? '图片检查未成功'
        : activity.status === 'completed' ? '图片检查完成'
        : phase;
    }
    return phase;
  }
  if (activity.kind === 'image_generation') return activity.status === 'completed' ? '图片已生成' : '正在生成图片';
  if (activity.kind === 'image_review') return activity.status === 'completed' ? '图片检查完成' : '正在检查图片';
  if (activity.kind === 'reasoning') return '正在分析与规划';
  if (activity.kind === 'skill') return '正在使用技能';
  const label = activity.label.trim();
  const key = label.toLowerCase();
  if (key.includes('get_command_or_subagent_output')) return '等待后台任务返回';
  if (key.includes('kill_command_or_subagent')) return '正在停止无响应的后台任务';
  if (key === 'todo_write' || key.includes('update_plan')) return '正在更新执行计划';
  if (key.includes('subagent') || key.includes('spawn_agent')) return '后台代理正在处理';
  if (activity.kind === 'command' || key === 'exec' || key === 'bash' || key.includes('terminal')) {
    const detail = activity.detail.trim().toLowerCase();
    const completed = activity.status === 'completed';
    const failed = activity.status === 'failed';
    if (/\b(npm|cargo|svelte-check|pytest|go test)\b/.test(detail)) {
      return failed ? '项目检查未通过' : completed ? '项目检查完成' : '正在检查项目';
    }
    return failed ? '任务步骤未成功' : completed ? '任务步骤已完成' : '正在执行任务步骤';
  }
  if (key === 'read') return '正在读取资料';
  if (key === 'edit') return '正在修改文件';
  if (key === 'search' || key === 'grep') return '正在查找信息';
  if (key === 'fetch') return '正在获取网页';
  // API 执行器的工具：名字是 read_file / site_data，详情是 JSON 参数，写成「读取 README.md」
  const h = jsonTool(activity);
  if (h) return h.target ? `${h.verb} ${h.target}` : h.verb;
  return activity.detail.trim() || label || '正在执行下一步';
}
function jsonTool(activity: ExecutionActivity): { verb: string; target: string } | null {
  const label = activity.label.trim();
  const detail = activity.detail.trim();
  if (!(label in TOOL_VERB) && !detail.startsWith('{')) return null;
  const h = humanTool({ label, detail } as ToolCall);
  return { verb: h.verb, target: h.target.length > 120 ? `${h.target.slice(0, 120)}…` : h.target };
}
export function executionProgress(live: LiveTurn, now: number) {
  const activities = Object.values(live.activities).sort((a, b) => a.startedAt - b.startedAt);
  const runningActivities = activities.filter((activity) => activity.status === 'running');
  const failedActivities = activities.filter((activity) => activity.status === 'failed');
  const latest = runningActivities.at(-1) ?? activities.at(-1);
  const finished = activities.filter((activity) => activity.status === 'completed').length;
  const canceled = activities.filter((activity) => activity.status === 'canceled').length;
  const quietFor = Math.max(0, now - live.lastActivityAt);
  const quiet = activities.length > 0 && quietFor >= 45_000;
  // CLI 子进程启动后，到首个 JSON 状态事件返回前没有更细的真实阶段。
  // 与 Codex 原生界面一致立即显示“正在思考”，不把这段空窗误称为恢复历史会话。
  const waitingForFirstActivity = activities.length === 0 && !live.text;
  let title = latest ? activityLabel(latest) : '正在思考下一步';
  if (waitingForFirstActivity) title = '正在思考';
  // 没有工具在跑、只是很久没新动作：模型在思考或在写一大段内容（比如整篇新页面），不是「后台任务」
  else if (quiet) title = runningActivities.length ? '这一步还在进行' : 'AI 正在思考或撰写较长的内容';
  else if (!runningActivities.length && activities.length) title = live.text ? '正在整理并继续处理' : '正在等待下一步';
  const parts: string[] = [];
  const generated = activities.filter((activity) => activity.kind === 'image_generation' && activity.status === 'completed').length;
  const skills = activities.filter((activity) => activity.kind === 'skill' && activity.status === 'completed').length;
  // 这是工具真实返回的“调用完成次数”，可能包含重做，不能冒充业务目标张数。
  if (generated) parts.push(`图片生成完成 ${generated} 次`);
  if (skills) parts.push(`已完成 ${skills} 次技能调用`);
  if (!generated && !skills && finished) parts.push(`${finished} 个步骤已完成`);
  if (canceled) parts.push(`${canceled} 项已停止`);
  if (waitingForFirstActivity && quietFor >= 60_000) parts.push('Codex 暂未返回新进展');
  if (quiet) parts.push(`最近活动在 ${elapsedLabel(quietFor)} 前`);
  parts.push(`已运行 ${elapsedLabel(now - live.startedAt)}`);
  const trueProgress = [...activities]
    .reverse()
    .find((activity) => activity.total != null && activity.total > 0 && activity.current != null && activity.current >= 0 && activity.current <= activity.total);
  const latestFailureAt = failedActivities.reduce((latestAt, activity) => Math.max(latestAt, activity.updatedAt), 0);
  const continuedAfterFailure = latestFailureAt > 0
    && activities.some((activity) => activity.status === 'completed' && activity.updatedAt > latestFailureAt);
  return {
    hasActivity: activities.length > 0 || waitingForFirstActivity,
    thinking: waitingForFirstActivity,
    title,
    meta: parts.join(' · '),
    quiet,
    failed: failedActivities.length > 0,
    continuedAfterFailure,
    failedActivities,
    activities: [...activities].reverse().slice(0, 10),
    trueProgress,
  };
}
export function activityFailureReason(activity: ExecutionActivity): string {
  return activity.error.trim() || '执行端未返回具体错误原因';
}
export function activityStatusLabel(activity: ExecutionActivity): string {
  if (activity.status === 'completed') return '已完成';
  if (activity.status === 'failed') return '未成功';
  if (activity.status === 'canceled') return '已停止';
  return '进行中';
}
export function activityDetail(activity: ExecutionActivity): string {
  // 原始 shell 命令、绝对路径和参数不适合直接暴露给普通用户。完整命令已经保留在
  // 上方可折叠的“执行记录”中；任务卡只呈现语义状态和真实终态。
  if (activity.kind === 'command') return '';
  // 标题已经写成「动作 + 对象」，JSON 参数不再重复显示
  if (jsonTool(activity)) return '';
  return activity.detail.trim();
}
export function compactToolRows(tools: ToolCall[]): Array<ToolCall & { count: number }> {
  const rows: Array<ToolCall & { count: number }> = [];
  const indexes = new Map<string, number>();
  for (const tool of tools) {
    const key = `${tool.label}\u0000${tool.detail}`;
    const index = indexes.get(key);
    if (index == null) {
      indexes.set(key, rows.length);
      rows.push({ ...tool, count: 1 });
    } else {
      rows[index].count += 1;
    }
  }
  return rows;
}
// ---------- 执行记录：把工具调用写成人看得懂的步骤（像 Claude 客户端那样「读取 README.md」） ----------
const TOOL_VERB: Record<string, string> = {
  read_file: '读取', Read: '读取', NotebookRead: '读取',
  list_files: '查看目录', LS: '查看目录', Glob: '查找文件',
  write_file: '写入', Write: '写入', Edit: '修改', MultiEdit: '修改', NotebookEdit: '修改',
  run_command: '运行命令', Bash: '运行命令', ssh_command: '远程命令',
  Grep: '搜索代码', WebFetch: '打开网页', WebSearch: '搜索网页', TodoWrite: '更新待办', Task: '子任务',
  site_data: '查询数据',
};
const DATA_CMD: Record<string, string> = {
  summary: '总览', queries: '查询词', pages: '页面', 'query-pages': '查询词 × 页面', events: '转化事件', daily: '按天趋势', opportunities: '运营机会', sitemaps: '站点地图',
};
const DATA_SRC: Record<string, string> = { gsc: 'Google', bing: 'Bing', ga4: 'GA4' };

function dataTarget(a: Record<string, unknown>): string {
  const o = (a.options ?? {}) as Record<string, unknown>;
  const parts = [DATA_SRC[String(o.source ?? '')] ?? '', DATA_CMD[String(a.command ?? '')] ?? String(a.command ?? '')];
  if (o.page) parts.push(`页面含「${o.page}」`);
  if (o.query) parts.push(`查询词含「${o.query}」`);
  if (o['min-position'] || o['max-position']) parts.push(`排名 ${o['min-position'] ?? '1'}–${o['max-position'] ?? ''}`);
  if (o['min-impressions']) parts.push(`展现 ≥ ${o['min-impressions']}`);
  if (o['max-ctr']) parts.push(`点击率 ≤ ${Math.round(Number(o['max-ctr']) * 1000) / 10}%`);
  if (o.prev) parts.push('前 28 天');
  return parts.filter(Boolean).join(' · ');
}

/** 一条工具调用 → { verb: 读取, target: README.md }。认不出的原样显示。 */
export function humanTool(t: ToolCall): { verb: string; target: string; full: string } {
  const verb = TOOL_VERB[t.label] ?? t.label;
  let a: Record<string, unknown> | null = null;
  try { const v = JSON.parse(t.detail); if (v && typeof v === 'object') a = v as Record<string, unknown>; } catch { /* 不是 JSON */ }
  if (!a) return { verb, target: t.detail, full: t.detail };
  if (t.label === 'site_data') return { verb, target: dataTarget(a), full: t.detail };
  const path = String(a.path ?? a.file_path ?? a.notebook_path ?? '');
  const range = a.offset ? `（从第 ${a.offset} 行起）` : '';
  const target = path ? `${path === '.' ? '项目根目录' : path}${range}`
    : String(a.command ?? a.pattern ?? a.url ?? a.query ?? a.description ?? t.detail);
  return { verb, target, full: t.detail };
}

/** 执行记录的折叠摘要：「读取 16 个文件 · 查询数据 11 次 · 查看目录 7 次」。 */
export function toolSummary(tools: ToolCall[]): string {
  const n = new Map<string, number>();
  for (const t of tools) { const v = humanTool(t).verb; n.set(v, (n.get(v) ?? 0) + 1); }
  const top = [...n.entries()].sort((a, b) => b[1] - a[1]);
  const fmt = ([v, c]: [string, number]) => (v === '读取' || v === '写入' || v === '修改' ? `${v} ${c} 个文件` : `${v} ${c} 次`);
  const head = top.slice(0, 3).map(fmt).join(' · ');
  return top.length > 3 ? `${head} 等 ${tools.length} 步` : head || `${tools.length} 步`;
}

// ---------- 托管运营会话：开头那条是 Pilot 交给 AI 的任务，结尾的 JSON 是给 Pilot 读的 ----------
export interface OpsTask { kind: string; site: string; stage: string; title: string }
/** 认出 Pilot 交给 AI 的任务指令（本周计划 / 改动），提取任务名、站点和阶段。 */
export function opsTask(text: string): OpsTask | null {
  const t = text.trim();
  let m = /^【托管运营 · (.+?)】你是「(.+?)」/.exec(t);
  if (m) {
    const stage = /【站点阶段】(\S+?)[（(\s]/.exec(t)?.[1] ?? '';
    return { kind: m[1], site: m[2], stage, title: '' };
  }
  m = /^【运营 · (.+?)】(.+)/.exec(t);
  if (m) return { kind: m[1], site: '', stage: '', title: m[2].trim() };
  return null;
}
export interface OpsPlanTask { title: string; page?: string; change: string; why?: string; source?: string; query?: string; priority?: number; expect?: { metric?: string; query?: string; note?: string } }
export interface OpsPlan { notes: string; positioning: string; tasks: OpsPlanTask[] }
/** 从回复里拆出计划 JSON（最后一个带 tasks 的 ```json 块），返回去掉它之后的正文和解析出的计划。 */
export function splitPlanJson(text: string): { body: string; plan: OpsPlan | null } {
  const re = /```json\s*([\s\S]*?)```/g;
  let hit: { start: number; end: number; plan: OpsPlan } | null = null;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    try {
      const v = JSON.parse(m[1]);
      const tasks = Array.isArray(v) ? v : v?.tasks;
      if (Array.isArray(tasks)) hit = { start: m.index, end: m.index + m[0].length, plan: { notes: String(v?.notes ?? ''), positioning: String(v?.positioning ?? ''), tasks } };
    } catch { /* 不完整的 JSON（还在流式输出）就先原样显示 */ }
  }
  if (!hit) return { body: text, plan: null };
  return { body: (text.slice(0, hit.start) + text.slice(hit.end)).trim(), plan: hit.plan };
}

// 运营任务的对话标题是指令的开头（「【托管运营 · 本周计划】你是「x」…」），显示成「本周计划 · x」这样的短标题
export function convTitle(title: string): string {
  const t = title || '';
  const m = t.match(/^【托管运营 · (.+?)】你是「([^」（]+)/);
  if (m) return `${m[1]} · ${m[2]}`;
  const o = t.match(/^【运营 · (.+?)】\s*(.+)/);
  if (o) return `${o[1]} · ${o[2]}`;
  return t;
}

export function hostOf(u: string): string { try { return new URL(u).host; } catch { return u; } }

// 从对话里认出这个项目已部署的线上地址（AI 部署完都会说「访问 https://xxx」）。
// 优先自定义域名，其次 *.pages.dev；忽略 localhost / CDN / 文档等无关链接。
export function detectSiteUrl(convs: (Conversation | null)[]): string {
  const re = /https?:\/\/[a-z0-9.-]+\.[a-z]{2,}[^\s`"'）)，。；]*/gi;
  let custom = '', pages = '';
  for (const c of convs) {
    if (!c) continue;
    for (const m of c.messages) {
      if (m.role !== 'assistant' || m.hidden) continue;
      const found = m.text.match(re);
      if (!found) continue;
      for (let u of found) {
        u = u.replace(/[.,;)]+$/, '');
        const host = u.replace(/^https?:\/\//i, '').split('/')[0].toLowerCase();
        if (host.includes('localhost') || host.startsWith('127.') || host.includes('cloudflare.com')
          || host.includes('developers.') || host.includes('tailwindcss.com') || host.includes('github')
          || host.includes('npmjs') || host.includes('unpkg') || host.includes('jsdelivr')) continue;
        if (host.endsWith('.pages.dev')) pages = u; else custom = u;
      }
    }
  }
  return custom || pages;
}
// 站点公开地址猜标准 favicon 位置（SiteFav 加载失败再退首字母）。
export function faviconGuess(url?: string): string { const u = (url ?? '').trim(); return u ? u.replace(/\/+$/, '') + '/favicon.ico' : ''; }
// 发消息时把这段追加到正文末尾让 AI 拿到路径；渲染时再从正文里拆出来单独做成附件卡（见 splitAttachments）。
export const ATT_MARKER = '附件（已存在项目里，可直接读取使用）：';
// 助手消息里由后端补的 codex 生图产物清单（agent.rs append_generated_images），渲染成缩略图。
const GEN_MARKER = '生成的图片：';
// 从消息正文里拆出「真正的文字」和「路径列表」——存储的是完整正文，气泡里显示干净文字 + 图卡。
function splitMarked(text: string, marker: string): { body: string; atts: string[] } {
  let cut = text.indexOf('\n\n' + marker);
  let sep = 2;
  if (cut < 0) {
    if (text.startsWith(marker)) { cut = 0; sep = 0; }
    else return { body: text, atts: [] };
  }
  const body = text.slice(0, cut);
  const atts = text.slice(cut + sep).split('\n').slice(1).map((l) => l.replace(/^-\s*/, '').trim()).filter(Boolean);
  return { body, atts };
}
export function splitAttachments(text: string) { return splitMarked(text, ATT_MARKER); }
export function splitGenImages(text: string) { return splitMarked(text, GEN_MARKER); }
export function isImgPath(p: string): boolean { return /\.(png|jpe?g|gif|webp|svg|bmp|avif)$/i.test(p); }
const thumbJobs = new Map<string, Promise<string>>();
export function conversationProject(c: Conversation): string { return c.task_type === 'workspace' ? c.id : c.site_slug; }
export function thumbKey(p: string, c:Conversation|null=app.activeConv): string { return c ? c.conn_id + '|' + conversationProject(c) + '|' + p : '||' + p; }
// 读工作目录内图片 → data URI（并发去重；失败缓存空串＝退回文件卡样式）。
export function loadWorkdirImg(connId: string, project: string, p: string): Promise<string> {
  const k = connId + '|' + project + '|' + p;
  if (media.thumbs[k] !== undefined) return Promise.resolve(media.thumbs[k]);
  let j = thumbJobs.get(k);
  if (!j) {
    j = invoke<string>('read_workdir_image', { connId, project, path: p })
      .then((d) => { media.thumbs = pruneThumbs({ ...media.thumbs, [k]: d }); return d; })
      .catch(() => { media.thumbs = { ...media.thumbs, [k]: '' }; return ''; })
      .finally(() => { thumbJobs.delete(k); });
    thumbJobs.set(k, j);
  }
  return j;
}
// 悬停/放大会把任意工作目录图片读进缓存：软上限 80 条，超了丢最早的（对象键序＝插入序），
// 防止单会话里长期翻聊天记录把 data URI 越积越多。
function pruneThumbs(t: Record<string, string>): Record<string, string> {
  const keys = Object.keys(t);
  if (keys.length <= 80) return t;
  for (const kk of keys.slice(0, keys.length - 80)) delete t[kk];
  return t;
}
function ensureThumb(connId: string, project: string, p: string) { void loadWorkdirImg(connId, project, p); }
export function fmtClock(sec: number): string {
  const d = new Date(sec * 1000);
  const hm = String(d.getHours()).padStart(2, '0') + ':' + String(d.getMinutes()).padStart(2, '0');
  return new Date().toDateString() === d.toDateString() ? hm : `${d.getMonth() + 1}/${d.getDate()} ${hm}`;
}
export function fmtRemain(ms: number): string {
  const m = Math.ceil(ms / 60000);
  if (m >= 60) return `${Math.floor(m / 60)} 小时 ${m % 60} 分`;
  if (m > 1) return `${m} 分钟`;
  return '不到 1 分钟';
}

// ---------- 助手消息 Markdown 渲染 ----------
// 模型输出的 **粗体**/列表/表格/代码块 之前是裸文本，读感远不如 Claude 客户端。
// marked(GFM+breaks) 渲染 → DOMPurify 消毒（模型会读网页/站点内容，注入面必须过滤）→ {@html}。
marked.setOptions({ gfm: true, breaks: true });
// 在 DOMPurify 默认协议白名单上放行 file:/sandbox:（AI 常用它们链接本地产物；默认会把
// href 整个剥掉，链接看着能点实际是死的）。javascript: 等仍被拦。
const MD_URI_RE = /^(?:(?:(?:f|ht)tps?|mailto|tel|callto|sms|cid|xmpp|file|sandbox):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i;
// file: 只为 <a> 放行（点击全被 mdClick 代理接管）；<img> src 收紧回 http(s)/data:image，
// 防 file: 子资源直连本地文件系统。
DOMPurify.addHook('afterSanitizeAttributes', (n) => {
  if (n.tagName === 'IMG') {
    const src = n.getAttribute('src') || '';
    if (!/^(https?:|data:image\/)/i.test(src)) n.removeAttribute('src');
  }
});
export function mdRender(text: string): string {
  return DOMPurify.sanitize(marked.parse(text, { async: false }) as string, { ALLOWED_URI_REGEXP: MD_URI_RE });
}
// Markdown 里 <a> 的点击代理：拦下 webview 内导航，改用系统浏览器打开。
// 行内代码若是图片路径（如 `/uploads/xx.svg`）：点击也用浏览器打开完整地址。
export function mdClick(e: MouseEvent, context:Conversation|null=app.activeConv) {
  const t = e.target as HTMLElement;
  // 链接分支不看选区且永远 preventDefault——点链接是明确意图，而且 WebKit 里点 <a> 不清除
  // 页面残留选区，若因选区早退会放任 webview 自导航（本函数存在的意义就是拦它）。
  const a = t.closest('a');
  if (a) {
    e.preventDefault();
    const raw = a.getAttribute('href') ?? '';
    if (/^https?:/i.test(raw)) { openUrl(raw); return; }
    if (/^(mailto|tel):/i.test(raw)) { openUrl(raw); return; } // gfm 会把裸邮箱自动变 mailto 链接
    // 工作目录内的路径（相对 / 绝对 / file:）：图片→应用内放大；其他文件→文件管理器里定位
    const p = relWorkPath(raw) || workAbsPath(raw);
    if (p && isImgPath(p)) { openWorkdirLightbox(p, context); return; }
    if (p) { void revealWorkdir(p, context); return; }
    if (raw && raw !== '#') say('这个链接打不开（不是网址，也不是工作目录里的文件）', 'err');
    return;
  }
  const code = t.closest('code');
  if (code && !code.closest('pre')) {
    if (window.getSelection()?.toString()) return; // 用户在拖选复制路径文本：不劫持成打开动作
    const s = (code.textContent ?? '').trim();
    const u = codeImgUrl(s);
    if (u) { e.preventDefault(); openUrl(u); return; }
    const rel = relWorkPath(s) || workAbsPath(s);
    if (rel && isImgPath(rel)) { e.preventDefault(); openWorkdirLightbox(rel, context); return; }
    // 非图片的行内代码：必须带路径分隔符才当文件处理（`package.json`、`v1.2.3` 这类
    // 纯提及不劫持——否则点一下就弹「没找到文件」的噪音）。
    if (rel && rel.includes('/') && /\.[a-z0-9]{1,8}$/i.test(rel)) { e.preventDefault(); void revealWorkdir(rel, context); }
  }
}

// 工作目录相对路径（消息里提到的项目内文件）：无协议、不以 / 开头、无空白、不出目录。
export function relWorkPath(s: string): string {
  const v = s.trim().replace(/^\.\//, '');
  if (!v || /\s/.test(v) || /^[a-z][a-z0-9+.-]*:/i.test(v) || v.startsWith('/') || v.startsWith('#') || v.includes('..')) return '';
  return v;
}
// 本地绝对路径 / file: / sandbox: 链接 → 绝对路径字符串。只做形态识别；
// 「必须在工作目录内」的边界由后端 canonicalize 后强制，越界一律拒绝。
export function workAbsPath(s: string): string {
  let v = s.trim();
  if (/^(file|sandbox):/i.test(v)) {
    try { v = decodeURIComponent(v.replace(/^file:\/\/(localhost)?/i, '').replace(/^sandbox:(\/\/)?/i, '')); }
    catch { return ''; }
  }
  if (!v || v.length > 1024) return '';
  if (!(v.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(v))) return '';
  return v;
}
function openWorkdirLightbox(p: string, c:Conversation|null=app.activeConv) {
  if (!c) return;
  // 点击是明确意图：之前失败过（负缓存空串）的条目给重试机会——文件可能刚被生成出来
  const k = thumbKey(p,c);
  if (media.thumbs[k] === '') { const t2 = { ...media.thumbs }; delete t2[k]; media.thumbs = t2; }
  loadWorkdirImg(c.conn_id, conversationProject(c), p).then((d) => {
    if (d) ui.lightbox = d;
    else say('预览失败：文件不存在、超过 8MB，或在工作目录之外', 'err');
  });
}
export async function revealWorkdir(p: string, c:Conversation|null=app.activeConv) {
  if (!c) return;
  try {
    const abs = await invoke<string>('resolve_workdir_file', { connId: c.conn_id, project: conversationProject(c), path: p });
    await revealItemInDir(abs);
  } catch { say('打不开：文件不存在（可能还没生成或已移动），或在工作目录之外', 'err'); }
}

// ---------- 行内代码图片预览 ----------
// 助手常回报「已上传 → /uploads/xx.svg」这类路径；把它变成可看的：悬停缩略图、点击打开。
// 只认站点约定的 /uploads/ 路径且要求已知站点公开地址（服务端对拼不出公开地址的站点刻意
// 返回空 url，前端不猜接口域名）；完整 http(s) 图片 URL 也认。其余（本地绝对路径、构建
// 产物路径）一律不当图片，避免拼出必 404 的链接。加载失败的 URL 进负缓存不再反复尝试。
export const IMG_EXT_RE = /\.(png|jpe?g|gif|webp|svg|bmp|avif|ico)$/i;
export const badImgUrls = new Set<string>();
export function codeImgUrl(t: string): string {
  const s = t.trim();
  if (!s || /\s/.test(s) || !IMG_EXT_RE.test(s)) return '';
  let u = '';
  if (/^https?:\/\//i.test(s)) u = s;
  else if (s.startsWith('/uploads/')) {
    const base = (app.activeSiteUrl || '').replace(/\/+$/, '');
    if (base) u = base + s;
  }
  return u && !badImgUrls.has(u) ? u : '';
}

// linkify + 段落（用户消息/错误消息仍走纯文本路径）
const urlRe = /(https?:\/\/[^\s)"'`」』】>，。；：！？、（）《》]+)/g;
export function segs(text: string): { t: string; link: boolean }[] {
  const out: { t: string; link: boolean }[] = [];
  let last = 0; let m: RegExpExecArray | null; urlRe.lastIndex = 0;
  while ((m = urlRe.exec(text))) {
    if (m.index > last) out.push({ t: text.slice(last, m.index), link: false });
    out.push({ t: m[0], link: true }); last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ t: text.slice(last), link: false });
  return out;
}
function fmt(secs: number): string { return new Date(secs * 1000).toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }

export function initMessages() {
  $effect(() => {
    const contexts = panes.splitVisible
      ? leaves(panes.conversationLayout).map(p=>splitConversation(p.conversation)).filter((c):c is Conversation=>!!c)
      : app.activeConv ? [app.activeConv] : [];
    const prefixes=contexts.map(c=>c.conn_id+'|'+conversationProject(c)+'|');
    const stale = Object.keys(media.thumbs).filter((k) => !prefixes.some(prefix=>k.startsWith(prefix)));
    if (stale.length) { const keep = { ...media.thumbs }; for (const k of stale) delete keep[k]; media.thumbs = keep; }
    const images=contexts.flatMap(c=>c.messages.flatMap(m=> {
      const atts=m.role==='user'?splitAttachments(m.text).atts:splitGenImages(m.text).atts;
      return atts.filter(isImgPath).map(p=>({c,p}));
    })).slice(-80);
    for(const {c,p} of images)ensureThumb(c.conn_id,conversationProject(c),p);
  });
}
