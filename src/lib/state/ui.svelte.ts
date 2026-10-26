import { getCurrentWindow } from '@tauri-apps/api/window';
import { hideTip, showTipFor } from '$lib/tip';
import { invoke } from '@tauri-apps/api/core';
import { editMenuTarget } from '$lib/editMenu';
import { app } from './app.svelte.js';
import { IMG_EXT_RE, badImgUrls, codeImgUrl, relWorkPath, workAbsPath, isImgPath, thumbKey, media, loadWorkdirImg, conversationProject } from './messages.svelte.js';
import { workbench, copyTermSelection } from './workbench.svelte.js';

class UiState {
  // 全屏时无红绿灯，顶部工具按钮改与左栏菜单左对齐。
  isFullscreen = $state(false);

  // 侧栏折叠 + 可拖拽宽度（持久化）。
  railCollapsed = $state(loadRailFlag());
  /** 会话列表已滚动（不在顶部）——导航与列表之间显示分界线。 */
  convosScrolled = $state(false);
  railWidth = $state(loadRailWidth());
  // 点缩略图看大图（点任意处/Esc 关闭）
  lightbox = $state('');

  // ---------- 全局自定义 tips（替换原生 title 系统提示） ----------
  // 原生 title 出现慢、样式与应用割裂。首次悬停时把 title 搬进 data-tip（保留 aria-label
  // 供无障碍），再交给共享定位器按真实尺寸避让视口边缘。
  // disabled 按钮不触发鼠标事件 → 需要 tip 的禁用按钮外面包 .tipwrap 承载 data-tip。
  imgTip = $state<{ x: number; y: number; url: string; below: boolean } | null>(null);
  imgTipReady = $state(false); // 图片 onload 之前浮层不可见，避免空白小块→大图的闪变
  hoverWant = ''; // 正在等加载的悬停目标（工作目录图片异步读完时校验还悬停着才弹）
}

export const ui = new UiState();

// 单行省略文案只有真的被截断时才挂全局 tooltip；宽度够时保持安静。
function overflowTip(node: HTMLElement, text: string) {
  let current = text;
  let frame = 0;
  const sync = () => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const clipped = node.scrollWidth > node.clientWidth + 1 || node.scrollHeight > node.clientHeight + 1;
      if (clipped && current) node.dataset.tip = current;
      else delete node.dataset.tip;
    });
  };
  const observer = new ResizeObserver(sync);
  observer.observe(node);
  sync();
  return {
    update(value: string) { current = value; sync(); },
    destroy() { cancelAnimationFrame(frame); observer.disconnect(); delete node.dataset.tip; },
  };
}

export function startDrag(e: MouseEvent) {
  if (e.button !== 0) return;
  const t = e.target as HTMLElement;
  if (t.closest('button, a, input, textarea, select, [role="button"], [data-no-drag]')) return;
  getCurrentWindow().startDragging().catch(() => {});
}
function loadRailFlag(): boolean { try { return localStorage.getItem('cligc.pilot.railCollapsed') === '1'; } catch { return false; } }
function loadRailWidth(): number { try { const n = parseInt(localStorage.getItem('cligc.pilot.railWidth') || ''); return n >= 190 && n <= 460 ? n : 240; } catch { return 240; } }
export function toggleRail() { ui.railCollapsed = !ui.railCollapsed; try { localStorage.setItem('cligc.pilot.railCollapsed', ui.railCollapsed ? '1' : '0'); } catch { /* */ } }
export function startResize(e: MouseEvent) {
  e.preventDefault();
  const onMove = (ev: MouseEvent) => { ui.railWidth = Math.min(460, Math.max(190, Math.round(ev.clientX))); };
  const onUp = () => {
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
    document.body.style.cursor = '';
    try { localStorage.setItem('cligc.pilot.railWidth', String(ui.railWidth)); } catch { /* */ }
  };
  document.body.style.cursor = 'col-resize';
  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
}
export function onTipHover(e: MouseEvent) {
  const tgt = e.target as HTMLElement;
  // 助手消息里的图片（行内代码路径或链接）→ 悬停缩略图浮层（点击见 mdClick）
  const holder = tgt.closest?.('a, code') as HTMLElement | null;
  if (holder && holder.closest('.text.md') && !holder.closest('pre')) {
    const isLink = holder.tagName === 'A';
    const s = isLink ? (holder.getAttribute('href') ?? '') : (holder.textContent ?? '').trim();
    // 远程图片：完整 URL（链接/代码都认），或行内代码里的 /uploads/ 站点路径
    let u = '';
    if (/^https?:/i.test(s)) {
      if (IMG_EXT_RE.test(s.replace(/[?#].*$/, '')) && !badImgUrls.has(s)) u = s;
    } else if (!isLink) {
      u = codeImgUrl(s);
    }
    // 工作目录内图片（相对/绝对/file: 都认）→ data URI（负缓存空串＝确认不是可预览图片，当普通元素处理）
    let rel = '';
    if (!u) { const r = relWorkPath(s) || workAbsPath(s); if (r && isImgPath(r)) rel = r; }
    const k = rel ? thumbKey(rel) : '';
    const known = rel ? media.thumbs[k] : undefined;
    if (u || (rel && known !== '')) {
      const r0 = holder.getBoundingClientRect();
      const below = r0.top < 230; // 上方放不下缩略图就翻到下方
      const x = Math.min(Math.max(r0.left + r0.width / 2, 150), window.innerWidth - 150);
      const place = { x, y: below ? r0.bottom + 8 : r0.top - 8, below };
      holder.style.cursor = 'zoom-in';
      hideTip();
      if (u || known) {
        ui.hoverWant = ''; // 立即可显示：作废先前挂着的异步加载，防止旧图旧坐标顶掉当前浮层
        const src = u || (known as string);
        if (!ui.imgTip || ui.imgTip.url !== src) ui.imgTipReady = false;
        ui.imgTip = { ...place, url: src };
      } else {
        const c = app.activeConv;
        ui.imgTip = null;
        if (c) {
          ui.hoverWant = k;
          loadWorkdirImg(c.conn_id, conversationProject(c), rel).then((d) => {
            if (d && ui.hoverWant === k) { ui.imgTipReady = false; ui.imgTip = { ...place, url: d }; }
          });
        }
      }
      return;
    }
    holder.style.cursor = ''; // 负缓存命中/失效时把之前设过的手势复位
  }
  ui.hoverWant = '';
  ui.imgTip = null;
  const el = tgt.closest?.('[title], [data-tip]') as HTMLElement | null;
  if (!el) { hideTip(); return; }
  const t = el.getAttribute('title');
  if (t) {
    el.removeAttribute('title');
    el.setAttribute('data-tip', t);
    if (!el.getAttribute('aria-label') && !el.textContent?.trim()) el.setAttribute('aria-label', t);
  }
  const text = el.getAttribute('data-tip') ?? '';
  if (!text) { hideTip(); return; }
  showTipFor(el, text, { align: el.dataset.tipAlign === 'end' ? 'end' : 'center' });
}

// ---------- 右键菜单（原生） ----------
// 替换 WKWebView 默认英文菜单（含 Inspect Element / AutoFill 等）。
//
// ★ 这里**必须交给原生菜单**，不能前端自己画：自己画就得用 navigator.clipboard.readText()
// 去粘贴，而 macOS 14 起「程序读剪贴板」会弹一个系统「Paste」确认按钮 —— 用户点完我们的
// 「粘贴」还得再点一次系统的，看着就像坏了，而且 JS 绕不过去（系统只看到有人要读剪贴板，
// 看不到用户刚点过菜单，那正是它要防的）。见 lib.rs::show_edit_menu。
// 启用态（没选中就灰掉复制等）也由系统自己管，不用我们猜。
export function onCtxMenu(e: MouseEvent) {
  if (e.defaultPrevented) return; // Child-owned menus (files/connections) take precedence.
  e.preventDefault();
  const t = e.target instanceof Element ? e.target : e.target instanceof Node ? e.target.parentElement : null;
  if (!t) return;
  const xterm = t.closest('.xterm');
  if (xterm && workbench.term) {
    // xterm 的选区画在 canvas 上，不属于 window.getSelection()：有选区时右键直接复制；
    // 无选区时把隐藏 textarea 设为第一响应者，再交给原生菜单完成安全粘贴。
    if (workbench.term.hasSelection()) {
      void copyTermSelection(workbench.term);
    } else {
      (xterm.querySelector('textarea') as HTMLTextAreaElement | null)?.focus();
      void invoke('show_edit_menu', { editable: true }).catch(() => {});
    }
    return;
  }
  const target = editMenuTarget(t);
  const sel = window.getSelection()?.toString() ?? '';
  if (!target && !sel) return; // 空白处不出菜单
  // 先聚焦：原生「剪切/复制/粘贴」作用在**当前第一响应者**上，不聚焦就打空
  // Focus the editing host without resetting its selection or scrolling the modal.
  if (target && document.activeElement !== target.element) target.element.focus({ preventScroll: true });
  void invoke('show_edit_menu', { editable: target?.editable ?? false }).catch(() => {});
}

export function initUi() {
  // 拖动窗口：忽略交互元素（按钮/输入等），否则点它们会误触发拖动。
  /** 页头「抬升」：内容真滑到它底下时才淡入阴影、把那条线淡掉（见 .thread-head 的注释）。
   *
   *  ★ 一处监听管全部视图：scroll 事件**不冒泡、但会捕获**，所以在 document 上抓一次就能收到
   *  任意视图里 .thread 的滚动 —— 不必逐视图挂钩子（7 个页头 / 6 个 .thread 分散在同一条
   *  {#if} 链里，同一时刻只挂一个）。判定靠**结构约定**（.thread 的前一个兄弟是 .thread-head），
   *  所以新增视图照现有写法写就自动继承，不用记得注册。
   *  写歪了（比如把 .thread 包了一层）→ 取不到页头 → 直接 no-op，退回今天的样子：
   *  是「没变好」，不是「坏了」—— 这个退化方向是选它而不选纯 CSS 方案的关键。
   *
   *  迟滞（4 上 / 1 下）不是洁癖：惯性滚动和触控板会在 0~1px 抖，用 >0 判定会让阴影反复
   *  淡入淡出，看着就是闪。 */
  $effect(() => {
    const onScroll = (e: Event) => {
      const el = e.target as HTMLElement | null;
      if (!el?.classList?.contains('thread')) return;
      const head = el.previousElementSibling; // 注释节点不算元素，convPane 那种 {@render} 锚点跳得过去
      if (!head?.classList.contains('thread-head')) return;
      const up = head.classList.contains('elevated');
      head.classList.toggle('elevated', el.scrollTop > (up ? 1 : 4));
    };
    document.addEventListener('scroll', onScroll, true); // capture：scroll 不冒泡
    return () => document.removeEventListener('scroll', onScroll, true);
  });
  $effect(() => {
    const win = getCurrentWindow();
    const sync = async () => { try { ui.isFullscreen = await win.isFullscreen(); } catch { /* */ } };
    sync();
    const un = win.onResized(sync).catch(() => undefined);
    return () => { void un.then((f) => f?.()); };
  });
}
