// 站点制作的后半程：第一版进度、上线前检查、让 AI 修问题，以及给老站点补一份站点约定。
// 进度和检查都由后端看文件得出（site_make.rs）：AI 说做完了不算，文件在、构建过、提交了才算。
import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import type { Conversation, MakeCheck, MakeState, Site, SiteTodo } from '$lib/types';
import { app, say, confirmDialog, refreshConvos } from './app.svelte.js';
import { SITE_PREFIX, refreshSites } from './sites.svelte.js';
import { startSiteChat, MAKING_PERM } from './siteCreate.svelte.js';
import { openConv } from './conversations.js';
import { persistThreadPerm } from './thread.svelte.js';
import { composer } from './composer.svelte.js';
import { send } from './turns.svelte.js';

const HIDE_KEY = 'cligc.pilot.conventionsHintHidden';
function loadHidden(): string[] { try { return JSON.parse(localStorage.getItem(HIDE_KEY) || '[]'); } catch { return []; } }

export const siteMake = $state({
  state: {} as Record<string, MakeState>,
  loading: {} as Record<string, boolean>,
  hidden: loadHidden(),
});

/** full=false 只问站点约定写了没有（站点看板每次打开都会问，便宜）。 */
export async function loadMake(id: string, full = true) {
  // 会在 $effect 里调用：读写 loading 别被追踪成依赖，不然自己触发自己
  if (untrack(() => siteMake.loading[id])) return;
  untrack(() => { siteMake.loading[id] = true; });
  try { siteMake.state[id] = await invoke<MakeState>('site_make_state', { id, full }); }
  catch { /* 站点文件夹不在了之类：卡片不出现就行 */ }
  finally { siteMake.loading[id] = false; }
}

/** 制作对话：最近一条「从零搭建 / 继续制作」的对话；都没有就用这个站点最早的那条。 */
export function makingConv(siteId: string): Conversation | undefined {
  const all = app.convos.filter((c) => c.conn_id === SITE_PREFIX + siteId).sort((a, b) => a.created_at - b.created_at);
  const mk = all.filter((c) => /^(请从零搭建|继续制作)/.test(c.title));
  return mk[mk.length - 1] ?? all[0];
}

/** 第一版没做完 AI 就停了：带上最新标准，让它先看现状再接着做。
 *  接在原来那条制作对话里（同一个站点的制作是一条线，前面做到哪也在上面）；找不到才开新的。 */
export async function continueMaking(site: Site) {
  try { await sendToMaking(site, await invoke<string>('site_continue_message', { id: site.id })); }
  catch (e) { say(String(e), 'err'); }
}

/** 发到这个站点的制作对话里（全自动）；找不到或正在跑就开一条新的。 */
async function sendToMaking(site: Site, text: string) {
  const conv = makingConv(site.id);
  if (!conv || conv.status === 'running' || app.running[conv.id]) {
    await startSiteChat(site.id, text, undefined, MAKING_PERM);
    return;
  }
  await openConv(conv.id);
  if (app.activeConvId !== conv.id) { await startSiteChat(site.id, text, undefined, MAKING_PERM); return; }
  await persistThreadPerm(MAKING_PERM);
  composer.draft = text; composer.attachments = [];
  await send();
}

// ───────── 需要我处理的事（AI 写在 .pilot/todo.json 里） ─────────

const doneLine = (t: SiteTodo) => `做完重新构建确认通过并提交，然后把 \`.pilot/todo.json\` 里 id 为 \`${t.id}\` 的条目删掉。`;

/** 我填好了值：交给 AI 按站点约定写进去。正式域名同时存进 Pilot 的站点设置。 */
export async function submitTodo(site: Site, t: SiteTodo, values: Record<string, string>) {
  const filled = t.inputs.filter((i) => values[i.key]?.trim());
  if (!filled.length) return;
  try {
    if (t.id === 'domain' && values.domain?.trim()) {
      await invoke('save_site', { site: { ...site, domain: values.domain.trim() } });
      await refreshSites();
    }
    const lines = filled.map((i) => `- ${i.label || i.key}：${values[i.key].trim()}`).join('\n');
    const where = t.where ? `（${t.where}）` : '';
    const extra = t.id === 'domain' ? '站点代码里用到站点地址的地方（canonical、sitemap、OG 之类）都改成这个域名。' : '';
    await sendToMaking(site, `我补充了「${t.title}」：\n${lines}\n\n请按站点约定写进站点${where}。${extra}${doneLine(t)}`);
  } catch (e) { say(String(e), 'err'); }
}

/** 交给 AI 做（ai / confirm / suggest）。 */
export async function delegateTodo(site: Site, t: SiteTodo) {
  const head = t.kind === 'confirm' ? `我确认：${t.title}，按你的建议来。` : `请处理这件事：${t.title}。`;
  const why = t.why ? `（${t.why}）` : '';
  const where = t.where ? `涉及：${t.where}。` : '';
  await sendToMaking(site, `${head}${why}${where}${doneLine(t)}`);
}

/** 我自己处理了 / 不需要：只记在 Pilot 里，不改仓库。 */
export async function markTodo(site: Site, t: SiteTodo) {
  try { await invoke('site_todo_handled', { id: site.id, todoId: t.id }); await loadMake(site.id, isFull(site.id)); }
  catch (e) { say(String(e), 'err'); }
}

/** 老一点的站点：AI 只在汇报里写了待办，让它整理成清单。 */
export async function askTodos(site: Site) {
  try { await sendToMaking(site, await invoke<string>('site_todo_request_message')); }
  catch (e) { say(String(e), 'err'); }
}

const isFull = (id: string) => (siteMake.state[id]?.steps.length ?? 0) > 0;

/** 本地预览构建产物（和上线的是同一份文件），在预览窗里打开。 */
export async function previewSite(site: Site) {
  try { await invoke<string>('site_preview_start', { id: site.id }); }
  catch (e) { say(String(e), 'err'); }
}

/** 从 Pilot 移除，文件夹留在原处。 */
export async function removeMade(site: Site) {
  const ok = await confirmDialog(`从 Pilot 移除「${site.name}」？站点的制作记录和对话会一并删除，文件夹 ${site.local_dir} 保留在原处。`, { title: '移除站点', kind: 'danger', confirmText: '移除' });
  if (!ok) return;
  try { await invoke('remove_site', { id: site.id }); await refreshSites(); await refreshConvos(); say(`已移除「${site.name}」`); }
  catch (e) { say(String(e), 'err'); }
}

/** 放弃制作：文件夹移到废纸篓（可以恢复），再从 Pilot 移除。 */
export async function discardMade(site: Site) {
  const ok = await confirmDialog(`删除「${site.name}」？文件夹 ${site.local_dir} 会移到废纸篓（可以从废纸篓恢复），站点的制作记录和对话也会一并删除。`, { title: '删除站点', kind: 'danger', confirmText: '删除' });
  if (!ok) return;
  try { await invoke('discard_made_site', { id: site.id }); await refreshSites(); await refreshConvos(); say(`已删除「${site.name}」，文件夹在废纸篓里`); }
  catch (e) { say(String(e), 'err'); }
}

export function fixWithAi(site: Site, checks: MakeCheck[]) {
  const bad = checks.filter((c) => c.level !== 'ok' && c.key !== 'todo');
  const lines = bad.map((c) => `- ${c.label}：${c.detail}${c.items.length ? '\n' + c.items.slice(0, 15).map((i) => `  - ${i}`).join('\n') + (c.items.length > 15 ? `\n  - ……还有 ${c.items.length - 15} 处` : '') : ''}`);
  const text = `上线前检查发现了下面这些问题，请逐条修复（按站点约定 .pilot/site.md 来）：\n\n${lines.join('\n')}\n\n`
    + '需要真实信息（联系方式、价格、数据）而你不知道的，不要编，写进 `.pilot/todo.json`（Pilot 会显示在看板上让我处理）。修完重新构建确认通过，提交一次，再简要说明改了什么。';
  void startSiteChat(site.id, text, undefined, site.origin === 'created' ? MAKING_PERM : '');
}

export function writeConventions(site: Site) {
  const text = '请读这个站点的代码，写一份站点约定 `.pilot/site.md`，以后加内容、改页面都按它来：\n'
    + '- 技术栈和主要依赖；\n'
    + '- 内容放在哪个目录、文件怎么命名、页面开头的元数据有哪些字段及含义（拿一个现有页面做例子）；\n'
    + '- 新增一页要改哪些地方（列表页、导航、sitemap 是否自动生成、要不要加内链）；\n'
    + '- 图片放哪、怎么引用；\n'
    + '- 安装、构建、本地预览的命令和产物目录；\n'
    + '- 现有的写作和排版惯例（标题层级、常用组件和模块）。\n\n'
    + '只写从代码里确认过的，不确定的标出来问我。只新增这一个文件，不改其它代码。写完列一下要点。';
  void startSiteChat(site.id, text);
}

export function hideConventionsHint(id: string) {
  siteMake.hidden = [...siteMake.hidden, id];
  try { localStorage.setItem(HIDE_KEY, JSON.stringify(siteMake.hidden)); } catch { /* */ }
}
