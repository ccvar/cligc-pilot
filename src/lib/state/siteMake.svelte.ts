// 站点制作的后半程：第一版进度、上线前检查、让 AI 修问题，以及给老站点补一份站点约定。
// 进度和检查都由后端看文件得出（site_make.rs）：AI 说做完了不算，文件在、构建过、提交了才算。
import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import type { Conversation, MakeCheck, MakeState, Site, SiteTodo } from '$lib/types';
import { app, say, confirmDialog, refreshConvos } from './app.svelte.js';
import { SITE_PREFIX, refreshSites, checkSite } from './sites.svelte.js';
import { startSiteChat, MAKING_PERM } from './siteCreate.svelte.js';
import { openConv } from './conversations.js';
import { persistThreadPerm } from './thread.svelte.js';
import { composer } from './composer.svelte.js';
import { send, startBackgroundChat, continueInBackground } from './turns.svelte.js';
import { brainUsable } from './models';
import { launcher } from './launcher.svelte.js';

const HIDE_KEY = 'cligc.pilot.conventionsHintHidden';
function loadHidden(): string[] { try { return JSON.parse(localStorage.getItem(HIDE_KEY) || '[]'); } catch { return []; } }

export const siteMake = $state({
  state: {} as Record<string, MakeState>,
  loading: {} as Record<string, boolean>,
  hidden: loadHidden(),
  /** 制作卡里手动展开过的部分（全部完成后默认收起；展开了这次打开期间保持展开） */
  expanded: {} as Record<string, { steps?: boolean; checks?: boolean }>,
  /** GA4 接入的结果（这次打开期间）：读到 / 建好的衡量 ID、存进哪个变量、代码里接了没有、出错原因、自动读取试过没有 */
  ga: {} as Record<string, { mid?: string; key?: string; declared?: boolean; error?: string; tried?: boolean; working?: boolean; wired?: boolean; rebuilt?: boolean }>,
});

/** full=false 只问站点约定写了没有（站点看板每次打开都会问，便宜）。 */
export async function loadMake(id: string, full = true) {
  // 会在 $effect 里调用：读写 loading 别被追踪成依赖，不然自己触发自己
  if (untrack(() => siteMake.loading[id])) return;
  untrack(() => { siteMake.loading[id] = true; });
  try {
    const got = await invoke<MakeState>('site_make_state', { id, full });
    // 轻量读取不扫构建产物：已有完整结果时只更新约定 / 待办 / 配置，别把进度和检查清空
    const prev = untrack(() => siteMake.state[id]);
    siteMake.state[id] = !full && prev?.steps.length ? { ...prev, conventions: got.conventions, todos: got.todos, config: got.config, built_at: got.built_at } : got;
    // 快速制作的站点：AI 写回了站点资料，名称可能换成了 AI 起的
    if (got.profile) void refreshSites();
  }
  catch { /* 站点文件夹不在了之类：卡片不出现就行 */ }
  finally { siteMake.loading[id] = false; }
}

/** 制作对话：最近一条「从零搭建 / 继续制作 / 迁移重做」的对话；都没有就用这个站点最早的那条。 */
export function makingConv(siteId: string): Conversation | undefined {
  const all = app.convos.filter((c) => c.conn_id === SITE_PREFIX + siteId).sort((a, b) => a.created_at - b.created_at);
  const mk = all.filter((c) => /^(请从零搭建|继续制作|请迁移重做|继续迁移重做)/.test(c.title));
  return mk[mk.length - 1] ?? all[0];
}

/** 第一版没做完 AI 就停了：带上最新标准，让它先看现状再接着做。
 *  接在原来那条制作对话里（同一个站点的制作是一条线，前面做到哪也在上面）；找不到才开新的。 */
export async function continueMaking(site: Site, issues: MakeCheck[] = []) {
  try {
    let text = await invoke<string>('site_continue_message', { id: site.id });
    // 上次构建检查出的问题一起带上（第一版没做完时不另起一轮「修复」）
    const bad = issues.filter((c) => c.level !== 'ok' && c.key !== 'todo');
    if (bad.length) {
      const lines = bad.map((c) => `- ${c.label}：${c.detail}${c.items.length ? '\n' + c.items.slice(0, 10).map((i) => `  - ${i}`).join('\n') + (c.items.length > 10 ? `\n  - ……还有 ${c.items.length - 10} 处` : '') : ''}`);
      text += `\n\n【上次构建的检查结果】下面这些问题要在这一轮里一起处理（缺的页面补上后，指向它们的断链自然就好了）：\n${lines.join('\n')}`;
    }
    await sendToMaking(site, text);
  }
  catch (e) { say(String(e), 'err'); }
}

/** 发到这个站点的制作对话里（全自动）；找不到或正在跑就开一条新的。 */
export async function sendToMaking(site: Site, text: string) {
  const conv = makingConv(site.id);
  // 迁移中：和「继续重做」一样在后台接着那条重做对话做，不跳走（进度看迁移卡片的那一步）
  if (site.origin === 'migrating') {
    const reuse = !!conv && conv.status !== 'running' && !app.running[conv.id];
    if (!reuse && !brainUsable(launcher.lBrain)) { await startSiteChat(site.id, text, undefined, MAKING_PERM); return; }
    say('交给 AI 在后台处理了，做完这里会刷新', 'ok');
    const run = reuse ? continueInBackground(conv!.id, text) : startBackgroundChat({ siteId: site.id, siteName: site.name, text, perm: MAKING_PERM });
    void run.finally(() => void loadMake(site.id, false));
    return;
  }
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

/** 迁移中的站点：正式域名就是旧站域名、切换时才指过来，AI 万一还是写了「确认正式域名」也不显示 */
export function migrateTodos(todos: SiteTodo[] | null | undefined): SiteTodo[] {
  return (todos ?? []).filter((t) => t.id !== 'domain');
}


/** 一起交给 AI：看板上填好的值、勾选要 AI 做的事，合成一条消息，AI 一轮处理完。
 *  对应 .env.example 里变量名的值直接存成配置变量（不用 AI）；正式域名同时存进站点设置。 */
export async function submitTodos(site: Site, batch: { todo: SiteTodo; values: Record<string, string> }[]) {
  if (!batch.length) return;
  try {
    const declared = new Set((siteMake.state[site.id]?.config ?? []).filter((v) => v.declared).map((v) => v.key));
    const envSet: [string, string][] = [];
    const handled: string[] = [];
    const filled: { t: SiteTodo; lines: string[] }[] = [];
    const tasks: SiteTodo[] = [];
    const confirms: SiteTodo[] = [];
    for (const { todo: t, values } of batch) {
      const given = t.inputs.filter((i) => values[i.key]?.trim());
      if (t.id === 'domain' && given.length) {
        const domain = (values.domain ?? values[given[0].key]).trim().replace(/^https?:\/\//, '').replace(/\/.*$/, '');
        if (domain) { await invoke('save_site', { site: { ...site, domain } }); }
      }
      if (!given.length) { (t.kind === 'confirm' ? confirms : tasks).push(t); continue; }
      const env = given.filter((i) => declared.has(i.key));
      env.forEach((i) => envSet.push([i.key, values[i.key].trim()]));
      const rest = given.filter((i) => !declared.has(i.key));
      if (rest.length) filled.push({ t, lines: rest.map((i) => `- ${i.label || i.key}：${values[i.key].trim()}`) });
      else handled.push(t.id);
    }
    if (envSet.length) await invoke('set_site_env', { id: site.id, set: envSet, remove: [] });
    for (const id of handled) await invoke('site_todo_handled', { id: site.id, todoId: id });
    await refreshSites();
    await loadMake(site.id, isFull(site.id));
    const forAi = [...filled.map((f) => f.t), ...tasks, ...confirms];
    if (!forAi.length) {
      if (site.origin === 'migrating') { say(`已存成配置变量：${envSet.map(([k]) => k).join('、')}。核对和上线预览时会用上`, 'ok'); return; }
      say(`已存成配置变量：${envSet.map(([k]) => k).join('、')}。正在重新构建，构建好刷新预览就能看到`);
      await checkSite(site.id);
      return;
    }
    let n = 0;
    const parts: string[] = [];
    if (filled.length) parts.push('【我补充的信息】\n' + filled.map((f) => `${++n}. ${f.t.title}${f.t.where ? `（${f.t.where}）` : ''}\n${f.lines.map((l) => '   ' + l).join('\n')}${f.t.id === 'domain' ? '\n   站点代码里用到站点地址的地方（canonical、sitemap、OG 之类）都改成这个域名。' : ''}`).join('\n'));
    if (tasks.length) parts.push('【请你处理】\n' + tasks.map((t) => `${++n}. ${t.title}${t.why ? `（${t.why}）` : ''}${t.where ? ` 涉及：${t.where}` : ''}`).join('\n'));
    if (confirms.length) parts.push('【我确认，按你的建议来】\n' + confirms.map((t) => `${++n}. ${t.title}`).join('\n'));
    const env = envSet.length ? `\n\n（${envSet.map(([k]) => k).join('、')} 我已经在 Pilot 里存成配置变量了，代码从环境变量读就行，不用写进代码。）` : '';
    const ids = forAi.map((t) => `\`${t.id}\``).join('、');
    await sendToMaking(site, `看板上「需要你处理」的几件事，我一起交给你（按站点约定来）：\n\n${parts.join('\n\n')}${env}\n\n全部做完后重新构建确认通过、提交一次，然后把 \`.pilot/todo.json\` 里这些条目删掉：${ids}。`);
  } catch (e) { say(String(e), 'err'); }
}

/** .env.example 里声明的 GA4 变量（和后端 site_config::ga_key 一致）。 */
export function gaKey(site: Site): string {
  const vars = (siteMake.state[site.id]?.config ?? []).filter((v) => v.declared && !v.secret);
  if (vars.some((v) => v.key === 'PUBLIC_GA_ID')) return 'PUBLIC_GA_ID';
  return vars.find((v) => { const u = v.key.toUpperCase(); return (u.includes('GA4') || u.startsWith('GA_') || u.includes('_GA_') || u.endsWith('_GA') || u.includes('GTAG')) && (u.includes('ID') || u.includes('MEASUREMENT')); })?.key ?? '';
}
/** GA4 真的接好了吗：代码里声明了 GA 变量（.env.example），而且 Pilot 里有值。
 *  只存了值、代码还没读这个变量的不算——站点不会加载统计代码。 */
export function gaReady(site: Site): boolean {
  const k = gaKey(site);
  return !!k && !!siteMake.state[site.id]?.config.find((v) => v.key === k)?.set;
}
/** 值已经存了（PUBLIC_GA_ID），但代码还没接：要交给 AI 接上。不依赖这次打开期间的状态，重启后也认得出。 */
export function gaNeedsWiring(site: Site): boolean {
  return !gaKey(site) && site.build.env_keys.includes('PUBLIC_GA_ID');
}

/** 接入 GA4：建（或读出已绑定的、或用我填的）衡量 ID，存进配置变量。
 *  auto：打开看板时自动读已绑定的——只读不写，不自动重新构建、也不自动叫 AI，结果留在 siteMake.ga 里给界面提示。 */
export async function ga4ToConfig(site: Site, connId = '', measurementId = '', auto = false) {
  const g = (siteMake.ga[site.id] = { ...(siteMake.ga[site.id] ?? {}), tried: true, working: true, error: '' });
  try {
    const r = await invoke<{ measurement_id: string; key: string; declared: boolean; created: boolean }>('ga4_to_config', { id: site.id, connId: connId || null, measurementId: measurementId || null });
    siteMake.ga[site.id] = { ...g, working: false, mid: r.measurement_id, key: r.key, declared: r.declared };
    await refreshSites();
    await loadMake(site.id, isFull(site.id));
    if (auto) return;
    if (r.declared) {
      say(`${r.created ? '已建好 GA4 媒体资源，' : ''}衡量 ID ${r.measurement_id} 已存进配置变量 ${r.key}，正在重新构建`);
      await rebuildForGa(site);
    } else {
      await wireGa(site);
    }
  } catch (e) {
    siteMake.ga[site.id] = { ...g, working: false, error: String(e) };
    if (!auto) say(String(e), 'err');
  }
}

/** 老站点：代码还没接 GA 变量，交给 AI 按配置变量的规矩接上。 */
export async function wireGa(site: Site) {
  const key = siteMake.ga[site.id]?.key || 'PUBLIC_GA_ID';
  siteMake.ga[site.id] = { ...(siteMake.ga[site.id] ?? {}), wired: true };
  await sendToMaking(site, `我已经在 Pilot 里把 GA4 衡量 ID 存成了配置变量 \`${key}\`（构建时注入，值不用写进代码）。请按【配置变量】的规矩接上：\n- 在 \`.env.example\` 里声明 \`${key}\`（[public]，GA4 衡量 ID），有值时在所有页面共用的 \`<head>\` 里用官方 gtag.js 加载，没值就不加载；站点里如果已经写死了别的统计代码，先告诉我；\n- 同步更新隐私政策：说明使用 Google Analytics 统计访问、会用到 Cookie、收集哪些数据、怎么拒绝；\n- \`.pilot/todo.json\` 里如果有「要不要加访问统计」之类的条目，接好后删掉。\n改完重新构建确认产物里的页面都带上了统计代码，提交一次。`);
}

export async function rebuildForGa(site: Site) {
  siteMake.ga[site.id] = { ...(siteMake.ga[site.id] ?? {}), rebuilt: true };
  await checkSite(site.id);
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
