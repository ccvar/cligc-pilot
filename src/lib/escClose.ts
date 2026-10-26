import { app } from '$lib/state/app.svelte.js';
import { ui } from '$lib/state/ui.svelte.js';

// 挂在对话框根节点上的 Svelte action：按 Esc 关掉它。
// 只有最后挂上的（叠在最上面的）那个对话框响应；确认框开着、看大图时交给它们自己处理；
// 下拉菜单等自己消费了 Esc（preventDefault / stopPropagation）的也不管。
// close 里自己判断 busy——和右上角 × 的 disabled 用同一个条件。
const stack: HTMLElement[] = [];

export function escClose(node: HTMLElement, close: () => void) {
  let fn = close;
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== 'Escape' || e.defaultPrevented || app.confirmState || ui.lightbox) return;
    if (stack[stack.length - 1] !== node) return;
    e.preventDefault();
    fn();
  };
  stack.push(node);
  window.addEventListener('keydown', onKey);
  return {
    update(c: () => void) { fn = c; },
    destroy() {
      const i = stack.lastIndexOf(node);
      if (i >= 0) stack.splice(i, 1);
      window.removeEventListener('keydown', onKey);
    },
  };
}
