import { invoke } from '@tauri-apps/api/core';
import { app, say } from './app.svelte.js';
import { scrollSoon } from './thread.svelte.js';

// 待批工具调用：询问/自动档下钩子把请求写在后端 pending 目录，UI 轮询渲染批准卡。
type Permit = { id: string; conv: string; tool: string; cmd: string; desc: string; arg: string; dangerous: boolean; mode: string; ts: number };

class PermitsState {
  // 命令一律默认收起（说明才是给人看的重点），点「查看具体命令」再展开。
  permitCmdOpen = $state(new Set<string>());
  pendingPermits = $state<Permit[]>([]);
  activePermits = $derived(this.pendingPermits.filter((p) => p.conv === app.activeConvId));
  permConvs = $derived(new Set(this.pendingPermits.map((p) => p.conv))); // 哪些对话有待批（含后台的，侧栏标出来）
}

export const permits = new PermitsState();

// 批准卡标题：优先用模型自己写的操作说明（Bash 的 description），没有就按工具合成一句人话。
export function permitDesc(p: Permit): string {
  if (p.desc) return p.desc;
  if (p.tool === 'WebFetch') return p.arg ? `抓取网页：${p.arg}` : '抓取一个网页';
  if (p.tool === 'WebSearch') return p.arg ? `联网搜索：${p.arg}` : '联网搜索';
  if (p.tool === 'Write' || p.tool === 'Edit' || p.tool === 'NotebookEdit') return p.arg ? `写入文件：${p.arg}` : '写入文件';
  if (p.tool === 'SSH') return p.arg ? `在 ${p.arg} 上执行命令` : '在远程服务器上执行命令';
  if (p.tool === 'Bash') return '运行一条命令';
  return `使用工具 ${p.tool}`;
}
export function togglePermitCmd(id: string) { const s = new Set(permits.permitCmdOpen); if (!s.delete(id)) s.add(id); permits.permitCmdOpen = s; }
// 已应答过的请求 id：应答后钩子最多要 300ms 才删掉 .req.json，而轮询每 600ms 重扫目录，
// 扫描落进窗口会让刚点掉的卡"复活"一次（看起来像连续弹两个）。id 不复用，永久过滤即可。
const respondedPermits = new Set<string>();
export async function respondPermit(id: string, allow: boolean) {
  respondedPermits.add(id);
  permits.pendingPermits = permits.pendingPermits.filter((p) => p.id !== id); // 乐观移除
  try { await invoke('respond_permit', { id, allow }); } catch (e) { say(String(e), 'err'); }
}

export function initPermits() {
  $effect(() => { if (permits.activePermits.length) scrollSoon(true); }); // 待批卡一出现就滚到它，不用手动翻
  $effect(() => {
    const t = setInterval(async () => {
      if (!Object.keys(app.running).length) { if (permits.pendingPermits.length) permits.pendingPermits = []; return; }
      try { permits.pendingPermits = (await invoke<Permit[]>('list_pending_permits')).filter((p) => !respondedPermits.has(p.id)); } catch { /* */ }
    }, 600);
    return () => clearInterval(t);
  });
}
