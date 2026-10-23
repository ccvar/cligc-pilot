// xterm 自带样式：必须从 JS 侧引入。写进组件样式块的 @import 会被 Svelte 作用域化，
// 而 xterm 的 DOM 是运行时建的、静态分析看不到 → 26 条 unused selector 警告。
import '@xterm/xterm/css/xterm.css';
import { invoke, Channel } from '@tauri-apps/api/core';
import { isMac, isWindows } from './env';
import { app, say } from './app.svelte.js';
import { openSshCredentialRepair } from './ssh.svelte.js';

// ---------- 远程工作台：终端为主区，底部对话 / 右侧文件按需开（VS Code 那套） ----------
// 两个面板可以同时开，各自可拖改大小；开关与尺寸都记住。
// 面板布局**按对话独立**：同一台机器上不同对话的干活姿势不一样 ——
// 一条在装东西要盯着终端，另一条只是问问题、想让对话占满。全局一份的话，
// 在这条里关掉命令行，切过去那条也没了。
// 存储只留一份「默认」：新对话（和还没碰过的旧对话）用它开局，你一拨就成为新的默认。
// 各对话的当前布局只放内存 —— 否则 localStorage 会随对话数无限长，删了对话还留一堆孤儿键。
// 面板布局**按对话独立**：会话1 只留对话框、会话2 只留命令行 —— 各是各的，互不影响。
//
// ★ 上一版错在「拨一下顺便记成全局默认」：于是在会话1关掉命令行就改写了默认，
//   会话2（还没碰过）一开局就继承成会话1的样子 —— 看起来就是「共享」。**开关不许写全局**。
//
// 开关（term/chat/files）：每条对话一份，按对话 id 存盘；新对话用固定默认（只开命令行，对话框按需打开）。
// 尺寸（chatH/filesW）：全局偏好 —— 「面板多高」是手感，不是某条对话的属性，
//   每开一条新对话就要重新拖一遍才叫烦人。
type WbFlags = { term: boolean; chat: boolean; files: boolean };
type WbLayout = WbFlags & { chatH: number; filesW: number };

// ---------- 远端负载（顶栏的 CPU / 内存 / 磁盘） ----------
// CPU 那两个字段是 /proc/stat 的**累计计数**，不是百分比：单次读没意义，得跟上一次求差。
// 所以这里按连接留一份上次的样本；第一次只能显示「—」，第二次（5 秒后）才有 CPU%。
type SshStats = { cpu_total: number; cpu_idle: number; mem_total_kb: number; mem_avail_kb: number; disk_total_kb: number; disk_used_kb: number };

const WB_FLAGS_DEF: WbFlags = { term: true, chat: false, files: false };
const WB_BY_CONV = 'cligc.pilot.wb.byConv';

class WorkbenchState {
  wbStartEl = $state<HTMLTextAreaElement | null>(null); // 起点输入框（新对话后把光标送进去）
  wbByConv: Record<string, WbFlags> = loadWbByConv(); // 对话 id → 开关；'new:<连接>' = 该连接还没有对话时的起点
  wb = $state<WbLayout>({
    ...WB_FLAGS_DEF,
    chatH: loadWbSize('chatH', 260, 140, 900),
    filesW: loadWbSize('filesW', 380, 240, 900),
  });
  wbKeyCur = '';
  // 文件栏单开：终端和对话都关了，只剩文件 —— 这时让它撑满主区（.wb-main 空了就收起来）。
  wbFilesSolo = $derived(this.wb.files && !this.wb.term && !this.wb.chat);
  statPrev: Record<string, SshStats> = {};
  stat = $state<null | { cpu: number | null; memPct: number; memText: string; diskPct: number; diskText: string }>(null);
  // ---------- 远程终端（xterm + PTY 流） ----------
  termEl = $state<HTMLDivElement | null>(null);
  termOn = $state(false);
  termBusy = $state(false); // 建连中：刷新键转起来 + 禁重复点
  termErr = $state('');
  term: import('@xterm/xterm').Terminal | null = null;
  termFit: import('@xterm/addon-fit').FitAddon | null = null;
  termConnId = '';
}

export const workbench = new WorkbenchState();

function loadWbSize(k: string, def: number, lo: number, hi: number): number {
  try { const n = parseInt(localStorage.getItem('cligc.pilot.wb.' + k) || ''); return n >= lo && n <= hi ? n : def; } catch { return def; }
}
function loadWbByConv(): Record<string, WbFlags> {
  try { const o = JSON.parse(localStorage.getItem(WB_BY_CONV) || '{}'); return o && typeof o === 'object' ? o : {}; } catch { return {}; }
}
function wbKeyOf(): string { return app.activeConvId || (app.isSshConn ? 'new:' + app.activeConnId : ''); }
/** 存盘：顺手按「现存对话」清一遍，免得删了对话还留一堆孤儿键把 localStorage 撑大。 */
function saveWbByConv() {
  try {
    // ★ convos 还没加载完（开机那会儿是空的）时**绝不能清**：那会把所有对话的布局
    //   一把全删光 —— 看起来就是「配置没保住」。空表说明「还不知道有哪些对话」，
    //   不是「一条都没有」。
    if (app.convos.length) {
      const live = new Set(app.convos.map((c) => c.id));
      const keep: Record<string, WbFlags> = {};
      for (const [k, v] of Object.entries(workbench.wbByConv)) {
        if (k.startsWith('new:') || live.has(k)) keep[k] = v;
      }
      workbench.wbByConv = keep;
    }
    localStorage.setItem(WB_BY_CONV, JSON.stringify(workbench.wbByConv));
  } catch { /* 配额满/隐私模式：布局丢了就丢了，不值得打断用户 */ }
}
function saveWbSize(k: 'chatH' | 'filesW', v: number) { try { localStorage.setItem('cligc.pilot.wb.' + k, String(v)); } catch { /* */ } }
/** 拨一下：只改**当前这条对话**的开关，绝不碰别的对话、也不写全局默认。 */
export function setWbFlags(patch: Partial<WbFlags>) {
  workbench.wb = { ...workbench.wb, ...patch };
  const key = workbench.wbKeyCur || wbKeyOf();
  if (key) {
    workbench.wbByConv[key] = { term: workbench.wb.term, chat: workbench.wb.chat, files: workbench.wb.files };
    saveWbByConv();
  }
}
/** 三个面板至少留一个开着：全关＝主区一整块空白。
 *  以前更严——终端/对话必须留一个（那会儿文件栏只是右侧窄条、不占主区，关掉这俩主区就空了）。
 *  现在放开：文件栏单开时让它吃满整宽（见 wbFilesSolo + .wb-main.collapsed），
 *  所以「只留文件」是合法布局。每个开关只在「关掉后一块都不剩」时才顶个默认（终端）上来。 */
export function toggleWbTerm() { setWbFlags(!workbench.wb.term ? { term: true } : workbench.wb.chat || workbench.wb.files ? { term: false } : { term: false, chat: true }); }
export function toggleWbChat() { setWbFlags(!workbench.wb.chat ? { chat: true } : workbench.wb.term || workbench.wb.files ? { chat: false } : { chat: false, term: true }); }
export function toggleWbFiles() { setWbFlags(!workbench.wb.files ? { files: true } : workbench.wb.term || workbench.wb.chat ? { files: false } : { files: false, term: true }); }
/** 面板拖拽。dir=h：拖对话面板的上沿（往上拖＝更高）；dir=v：拖文件面板的左沿（往左拖＝更宽）。 */
export function startWbResize(e: MouseEvent, dir: 'h' | 'v') {
  e.preventDefault();
  const x0 = e.clientX, y0 = e.clientY;
  const h0 = workbench.wb.chatH, w0 = workbench.wb.filesW;
  const onMove = (ev: MouseEvent) => {
    if (dir === 'h') workbench.wb = { ...workbench.wb, chatH: Math.min(900, Math.max(140, Math.round(h0 - (ev.clientY - y0)))) };
    else workbench.wb = { ...workbench.wb, filesW: Math.min(900, Math.max(240, Math.round(w0 - (ev.clientX - x0)))) };
  };
  const onUp = () => {
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
    document.body.style.cursor = '';
    saveWbSize(dir === 'h' ? 'chatH' : 'filesW', dir === 'h' ? workbench.wb.chatH : workbench.wb.filesW); // 落定才存
  };
  document.body.style.cursor = dir === 'h' ? 'row-resize' : 'col-resize';
  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
}
const STAT_EVERY_MS = 5000;
function fmtKb(kb: number): string {
  const u = ['KB', 'MB', 'GB', 'TB'];
  let v = kb, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
}
async function pollStats(id: string) {
  try {
    const s = await invoke<SshStats>('ssh_stats', { connId: id });
    const p = workbench.statPrev[id];
    workbench.statPrev[id] = s;
    // 两次采样之间的差：分母是这段时间里所有时间片的增量，分子是其中非空闲的部分。
    let cpu: number | null = null;
    if (p) {
      const dt = s.cpu_total - p.cpu_total;
      const di = s.cpu_idle - p.cpu_idle;
      // 机器重启过（计数器归零）→ dt<=0，这一轮跳过，下一轮就正常了
      if (dt > 0) cpu = Math.min(100, Math.max(0, Math.round((1 - di / dt) * 100)));
    }
    const memUsed = s.mem_total_kb - s.mem_avail_kb;
    workbench.stat = {
      cpu,
      memPct: s.mem_total_kb ? Math.round((memUsed / s.mem_total_kb) * 100) : 0,
      memText: `${fmtKb(memUsed)} / ${fmtKb(s.mem_total_kb)}`,
      diskPct: s.disk_total_kb ? Math.round((s.disk_used_kb / s.disk_total_kb) * 100) : 0,
      diskText: `${fmtKb(s.disk_used_kb)} / ${fmtKb(s.disk_total_kb)}`,
    };
  } catch {
    // 没连上 / 不是 Linux / 命令被拒 —— 都不吭声，顶栏空着就是了（这不是用户要办的事）
    workbench.stat = null;
  }
}
export async function copyTermSelection(t: import('@xterm/xterm').Terminal) {
  const text = t.getSelection();
  if (!text) return;
  try { await navigator.clipboard.writeText(text); }
  catch { say('复制失败，请检查系统剪贴板权限', 'err'); }
}
async function pasteIntoTerm(t: import('@xterm/xterm').Terminal) {
  try {
    const text = await navigator.clipboard.readText();
    // 读剪贴板是异步的；期间用户可能已经切换连接/拆掉旧终端。
    if (workbench.term === t && text) t.paste(text);
  } catch {
    say('读取剪贴板失败，可在终端中右键选择“粘贴”', 'err');
  }
}
function handleTermKey(t: import('@xterm/xterm').Terminal, e: KeyboardEvent): boolean {
  if (e.type !== 'keydown') return true;
  const key = e.key.toLowerCase();
  // Windows 兼容两套习惯：有选区时 Ctrl+C 复制，无选区时仍把 ^C 发给远端；
  // Ctrl+Shift+C 始终按终端快捷键处理。macOS 则保持 Cmd+C，Linux 用 Ctrl+Shift+C。
  const copy = (isMac && e.metaKey && key === 'c')
    || (!isMac && e.ctrlKey && e.shiftKey && key === 'c')
    || (isWindows && e.ctrlKey && key === 'c' && t.hasSelection());
  if (copy) {
    e.preventDefault(); e.stopPropagation();
    void copyTermSelection(t);
    return false;
  }

  // macOS 的 Cmd+V 交还给系统/WKWebView，避免 navigator.clipboard.readText 触发
  // 系统二次 Paste 授权；Windows 显式读取，绕开 WebView2 偶发不派发 paste 事件的问题。
  const paste = (isWindows && e.ctrlKey && key === 'v')
    || (!isMac && !isWindows && e.ctrlKey && e.shiftKey && key === 'v')
    || (!isMac && e.shiftKey && e.key === 'Insert');
  if (paste) {
    e.preventDefault(); e.stopPropagation();
    void pasteIntoTerm(t);
    return false;
  }

  // Tab 在 WebView2 / WKWebView 中可能先被宿主当成「切换页面焦点」，xterm 收不到，
  // Shell 就无法做命令与路径补全。终端聚焦时显式走用户输入通道；Shift+Tab
  // 保留终端通用的反向制表序列。带 Ctrl/Alt/Meta 的组合键仍交给 xterm/宿主。
  if (e.key === 'Tab' && !e.ctrlKey && !e.altKey && !e.metaKey) {
    e.preventDefault(); e.stopPropagation();
    t.input(e.shiftKey ? '\x1b[Z' : '\t', true);
    return false;
  }

  // WebView2 会把 Escape 留给宿主 WebView，xterm 收不到时 vi 就无法退出插入模式。
  // 这里直接走 xterm 的用户输入通道，macOS 也统一经过同一路径，且不会发送两次。
  if (e.key === 'Escape') {
    e.preventDefault(); e.stopPropagation();
    t.input('\x1b', true);
    return false;
  }
  return true;
}
// 注意：**不清 activeConv** —— 工作台里终端一直在，没有「切到终端」这回事；
// 清掉对话只会把用户正在聊的东西关了（那正是侧栏「远程终端」入口被删掉的原因）。
export function openRemote() { app.view = 'remote'; }
export function b64ToBytes(b64: string): Uint8Array {
  const s = atob(b64); const a = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i);
  return a;
}
export function bytesToB64(bytes: Uint8Array): string {
  let bin = '';
  // 避免把大数组一次性 spread 进 fromCharCode，超过 JS 参数上限会直接抛错。
  for (let start = 0; start < bytes.length; start += 0x8000) {
    const end = Math.min(start + 0x8000, bytes.length);
    for (let i = start; i < end; i++) bin += String.fromCharCode(bytes[i]);
  }
  return btoa(bin);
}
export function strToB64(s: string): string {
  let bin = '';
  for (const b of new TextEncoder().encode(s)) bin += String.fromCharCode(b);
  return btoa(bin);
}
export function disposeTerm() {
  if (workbench.term) { try { workbench.term.dispose(); } catch { /* */ } }
  workbench.term = null; workbench.termFit = null; workbench.termOn = false; workbench.termBusy = false; workbench.termErr = ''; workbench.termConnId = '';
}
export async function startTerm(id: string, el: HTMLDivElement) {
  disposeTerm();
  workbench.termConnId = id;
  workbench.termBusy = true;
  workbench.termErr = '';
  // xterm 要 DOM，动态 import（SSR 关着，但仍别在模块顶层拉进来）。
  const [{ Terminal }, { FitAddon }] = await Promise.all([
    import('@xterm/xterm'),
    import('@xterm/addon-fit'),
  ]);
  // macOS 的 SF Mono/Menlo 在 Retina 下 12px 已经很清楚；Windows WebView2 在 100% DPI
  // 下同字号会显得细小、行距又紧。优先使用 Windows Terminal 同款 Cascadia Mono，
  // 未安装时回退 Consolas，并单独放大一档；FitAddon 会据此重新计算行列，不会裁字。
  const terminalFontFamily = isWindows
    ? '"Cascadia Mono", "Cascadia Code", Consolas, "Lucida Console", monospace'
    : 'ui-monospace, SFMono-Regular, Menlo, monospace';
  const t = new Terminal({
    fontSize: isWindows ? 14 : 12,
    fontFamily: terminalFontFamily,
    lineHeight: isWindows ? 1.15 : 1,
    letterSpacing: isWindows ? 0.2 : 0,
    fontWeight: isWindows ? '400' : 'normal',
    fontWeightBold: isWindows ? '600' : 'bold',
    cursorBlink: true,
    // ★ 滚动条宽度只能从这儿改：xterm 6 不再用 viewport 的原生滚动条，改成了自绘的
    // （.xterm-scrollable-element > .scrollbar，从 VS Code 移植），宽度在 JS 里取
    // `overviewRuler?.width || 14` —— CSS 的 ::-webkit-scrollbar 对它完全无效。
    // 副作用：这个选项**身兼两职**，一设就同时把「装饰栏」渲染出来（文档原话：this must be
    // set in order to see the overview ruler），它会在自己左沿画一条贯穿全高的 1px 竖线
    // （_renderRulerOutline，用 theme.overviewRulerBorder，默认深色）——就是那条碍眼的线。
    // 我们压根不用 decoration，那个 canvas 纯属负担 → CSS 里直接 display:none（见 .term-wrap 那段）。
    overviewRuler: { width: 7 },
    theme: {
      background: '#1c1917',
      foreground: '#e8e4dc',
      cursor: '#e8e4dc',
      // 自绘滚动条的三档颜色（xterm 6 的 theme 项）：平时很淡，摸上去才明显。
      scrollbarSliderBackground: '#4b464033',
      scrollbarSliderHoverBackground: '#4b4640aa',
      scrollbarSliderActiveBackground: '#635d55cc',
    },
  });
  const f = new FitAddon();
  t.loadAddon(f); t.open(el); f.fit();
  workbench.term = t; workbench.termFit = f;
  t.attachCustomKeyEventHandler((e) => handleTermKey(t, e));
  // GPU 渲染：默认的 DOM 渲染器在 Retina 上敲字会发涩（每个字符都是 DOM 活儿）。
  // 必须 open() 之后再挂，且要能回退 —— WebView 里 WebGL 不一定可用，
  // 上下文丢失（切显卡/系统回收）也要退回 DOM，否则整个终端会变黑。
  try {
    const { WebglAddon } = await import('@xterm/addon-webgl');
    const gl = new WebglAddon();
    gl.onContextLoss(() => { try { gl.dispose(); } catch { /* 退回 DOM 渲染 */ } });
    t.loadAddon(gl);
  } catch { /* 没 WebGL 就用默认的 DOM 渲染器，功能不受影响 */ }
  const ch = new Channel<{ type: string; b64?: string; error?: string }>();
  ch.onmessage = (ev) => {
    if (workbench.term !== t) return; // 已被拆掉/换连接，丢弃尾包
    if (ev.type === 'data' && ev.b64) t.write(b64ToBytes(ev.b64));
    else if (ev.type === 'closed') {
      workbench.termOn = false;
      t.write(`\r\n\x1b[90m—— 连接已关闭${ev.error ? '：' + ev.error : ''}，按回车重新连接 ——\x1b[0m\r\n`);
    }
  };
  t.onData((d) => {
    // 关闭态：回车＝就地重连（iTerm/VS Code 的肌肉记忆），其余按键不再灌进死管道；
    // 连接中（termBusy）也别触发，免得连点回车叠一队重连。
    if (!workbench.termOn) {
      if (d.includes('\r') && !workbench.termBusy) void reconnectTerm();
      return;
    }
    void invoke('ssh_input', { connId: id, b64: strToB64(d) });
  });
  t.onResize(({ cols, rows }) => { void invoke('ssh_resize', { connId: id, cols, rows }); });
  // 「正在连接」用浮层（见 .term-connecting），**不往终端里写**：
  // 写进去就是终端缓冲的一部分，连上之后擦不干净（服务器的 banner 可能已经跟着来了，
  // 光标位置不由我们说了算）—— 那行字就会一直赖在最上面。浮层随 termBusy 自动收。
  try {
    await invoke('ssh_open_shell', { connId: id, cols: t.cols, rows: t.rows, onEvent: ch });
    workbench.termOn = true;
  } catch (e) {
    workbench.termErr = String(e);
    t.write(`\r\n\x1b[31m${workbench.termErr}\x1b[0m\r\n`);
  } finally {
    if (workbench.term === t) workbench.termBusy = false; // 已被换掉的旧终端别去动当前状态
  }
}
export async function reconnectTerm() {
  const id = app.activeConnId; const el = workbench.termEl;
  if (!id || !el || workbench.termBusy) return;
  workbench.termBusy = true; // 立刻转起来：ssh_close 也要等一下，别让这段时间看着像没反应
  try { await invoke('ssh_close', { connId: id }); } catch { /* 本来就没连上，照样重来 */ }
  await startTerm(id, el);
}
export function missingSshCredential(error: string): boolean {
  return error.includes('钥匙串里没有这个连接的密码');
}
export function repairActiveSshCredential() {
  if (app.activeConn?.kind === 'ssh') openSshCredentialRepair(app.activeConn);
}

export function initWorkbench() {
  // 切对话/切连接 → 取回目标那份（没碰过的用固定默认，**不继承别的对话**）。
  // 不必在这里存：setWbFlags 每次拨动就已经存过了。
  $effect(() => {
    const key = wbKeyOf();
    if (key === workbench.wbKeyCur) return;
    const prev = workbench.wbKeyCur;
    workbench.wbKeyCur = key;
    let f = workbench.wbByConv[key];
    // ★ 起点 → 新对话：在起点输入框里拨好的布局要跟过去。这条对话**就是**那个起点变的，
    //   不是「继承别的对话」。漏了它就会：你在起点拨好面板，一发消息全弹回默认。
    if (!f && key && !key.startsWith('new:') && prev.startsWith('new:')) {
      f = workbench.wbByConv[prev];
      if (f) { workbench.wbByConv[key] = { ...f }; saveWbByConv(); }
    }
    f = f ?? WB_FLAGS_DEF;
    workbench.wb = { ...workbench.wb, term: f.term, chat: f.chat, files: f.files };
  });
  // 只在看得见工作台时才轮询；换连接清掉上一台的数（否则会拿别的机器的样本算差）。
  $effect(() => {
    const id = app.activeConnId;
    if (app.view !== 'remote' || !app.isSshConn || !id) { workbench.stat = null; return; }
    workbench.stat = null;
    delete workbench.statPrev[id];
    void pollStats(id);
    const t = setInterval(() => { if (!document.hidden) void pollStats(id); }, STAT_EVERY_MS);
    return () => clearInterval(t);
  });
  // 终端容器一变大小就重排：开关面板、拖面板边、拉窗口——一个 ResizeObserver 全包了，
  // 不用在每个改布局的地方各记一次 fit()（漏一个就是花屏）。
  $effect(() => {
    const el = workbench.termEl;
    if (!el) return;
    let raf = 0;
    const ro = new ResizeObserver(() => {
      // 合并同一帧里的连续变化（拖拽时每像素都会触发）
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => { try { workbench.termFit?.fit(); } catch { /* 终端已拆 */ } });
    });
    ro.observe(el);
    return () => { cancelAnimationFrame(raf); ro.disconnect(); };
  });
  // WKWebView 有时会在 xterm 的自定义按键处理器之前吞掉 Escape。直接在终端容器的
  // capture 阶段截获并写入 PTY，保证 vi/vim 的“退出插入模式”不依赖 WebView 默认行为。
  $effect(() => {
    const el = workbench.termEl;
    if (!el) return;
    const onEscape = (event: KeyboardEvent) => {
      if ((event.key !== 'Escape' && event.key !== 'Esc' && event.keyCode !== 27) || !workbench.termOn || !workbench.termConnId) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      void invoke('ssh_input', { connId: workbench.termConnId, b64: strToB64('\x1b') });
    };
    el.addEventListener('keydown', onEscape, true);
    return () => el.removeEventListener('keydown', onEscape, true);
  });
  // 进 remote 视图且容器就位 → 起终端；切走/换连接由 $effect 的清理拆掉。
  $effect(() => {
    const id = app.activeConnId;
    const el = workbench.termEl;
    if (app.view !== 'remote' || !app.isSshConn || !el || !id) return;
    if (workbench.termConnId === id && workbench.term) return; // 同一连接已在跑
    void startTerm(id, el);
  });
}
