// 设置里「连接」各组的折叠状态（记在本机）：默认都收起，只看组头的数量；展开过的记住。
// （v2：原来是条目多的组才默认收起，换了键让大家都从收起开始）
const FOLD_KEY = 'cligc.pilot.connFold.v2';

class FoldState {
  map = $state<Record<string, boolean>>((() => { try { const v = JSON.parse(localStorage.getItem(FOLD_KEY) || '{}'); return v && typeof v === 'object' && !Array.isArray(v) ? v : {}; } catch { return {}; } })());
}
export const connFold = new FoldState();

export const isFolded = (kind: string) => connFold.map[kind] ?? true;

export function setFolded(kind: string, folded: boolean) {
  connFold.map = { ...connFold.map, [kind]: folded };
  try { localStorage.setItem(FOLD_KEY, JSON.stringify(connFold.map)); } catch { /* */ }
}
