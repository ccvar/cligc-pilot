// 设置里「连接」各组的折叠状态（记在本机）：条目多的组默认收起，只看组头的数量。
const FOLD_KEY = 'cligc.pilot.connFold';

class FoldState {
  map = $state<Record<string, boolean>>((() => { try { return JSON.parse(localStorage.getItem(FOLD_KEY) || '{}'); } catch { return {}; } })());
}
export const connFold = new FoldState();

export const isFolded = (kind: string, n: number) => connFold.map[kind] ?? n > 4;

export function setFolded(kind: string, folded: boolean) {
  connFold.map = { ...connFold.map, [kind]: folded };
  try { localStorage.setItem(FOLD_KEY, JSON.stringify(connFold.map)); } catch { /* */ }
}
