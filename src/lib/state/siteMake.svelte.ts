// 站点制作的后半程：第一版进度、上线前检查、让 AI 修问题，以及给老站点补一份站点约定。
// 进度和检查都由后端看文件得出（site_make.rs）：AI 说做完了不算，文件在、构建过、提交了才算。
import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import type { Conversation, MakeCheck, MakeState, Site } from '$lib/types';
import { app } from './app.svelte.js';
import { SITE_PREFIX } from './sites.svelte.js';
import { startSiteChat } from './siteCreate.svelte.js';

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

/** 这个站点最早的那条对话＝制作第一版的对话。 */
export function makingConv(siteId: string): Conversation | undefined {
  return app.convos.filter((c) => c.conn_id === SITE_PREFIX + siteId).sort((a, b) => a.created_at - b.created_at)[0];
}

export function fixWithAi(site: Site, checks: MakeCheck[]) {
  const bad = checks.filter((c) => c.level !== 'ok');
  const lines = bad.map((c) => `- ${c.label}：${c.detail}${c.items.length ? '\n' + c.items.slice(0, 15).map((i) => `  - ${i}`).join('\n') + (c.items.length > 15 ? `\n  - ……还有 ${c.items.length - 15} 处` : '') : ''}`);
  const text = `上线前检查发现了下面这些问题，请逐条修复（按站点约定 .pilot/site.md 来）：\n\n${lines.join('\n')}\n\n`
    + '需要真实信息（联系方式、价格、数据）而你不知道的，不要编，列出来问我。修完重新构建确认通过，提交一次，再简要说明改了什么。';
  void startSiteChat(site.id, text);
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
