// 「分发」的全局状态：各站点等你确认发布的篇数（站点「待处理」上的数字），后台自动处理完会发 site-syndication 事件。
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

class SyndState {
  /** 站点 id → 摘要写好、等你确认发布的篇数 */
  pending = $state<Record<string, number>>({});
  /** 后台自动处理过一次就加一：开着的「分发」分页据此重新读取 */
  ver = $state<Record<string, number>>({});
}
export const synd = new SyndState();

export async function loadSyndPending(siteId: string) {
  try { synd.pending = { ...synd.pending, [siteId]: await invoke<number>('syndication_pending', { siteId }) }; } catch { /* 读不到就不显示 */ }
}

let listening = false;
export function listenSyndication() {
  if (listening) return;
  listening = true;
  void listen<{ site_id: string }>('site-syndication', (e) => {
    const id = e.payload.site_id;
    synd.ver = { ...synd.ver, [id]: (synd.ver[id] ?? 0) + 1 };
    void loadSyndPending(id);
  });
}
