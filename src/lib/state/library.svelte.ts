import { invoke } from '@tauri-apps/api/core';
import { type Prompt, loadUserPrompts, PRESET_PROMPTS, newPromptId, saveUserPrompts } from '$lib/prompts';
import { say, app, confirmDialog } from './app.svelte.js';
import { selectConn } from './connections.svelte.js';
import { launcher } from './launcher.svelte.js';
import { portOf } from './cloudflare.svelte.js';

// ---------- 模板库 ----------
// builtin＝Pilot 随附的起始模板：删不掉（后端也拒），created_at 恒 0（列表里自然沉底）。
export type Template = { slug: string; name: string; desc: string; category: string; pages: number; created_at: number; builtin: boolean };

class LibraryState {
  templates = $state<Template[]>([]);
  templatesLoading = $state(false);
  tmplHtml = $state<Record<string, string>>({}); // 每个模板的入口 HTML，做真实缩略图
  tmplCat = $state('全部');
  tmplCats = $derived.by(() => {
    const has = new Set(this.templates.map(catOf));
    // 「自建」紧跟「全部」：它俩是「看什么范围」，后面那些才是「按页型筛」——
    // 同一类的摆一起，别让自建漂在页型清单的尾巴上。
    const out = has.has(CAT_MINE) ? [CAT_MINE] : [];
    out.push(...CAT_ORDER.filter((c) => has.has(c)));
    // 后端将来加了新分类，前端不用跟着改也不会把它漏掉
    for (const c of has) if (c !== CAT_MINE && !out.includes(c)) out.push(c);
    return out;
  });
  // 视图里只按分类筛。**按关键词找模板走全局搜索**（那里能一处搜到会话/任务/托管/模板，
  // 而不是每个视图各摆一个搜索框）。
  tmplShown = $derived(this.templates.filter((t) => this.tmplCat === '全部' || catOf(t) === this.tmplCat));
  // 存为模板（从 CF 会话）
  saveTmplOpen = $state(false);
  saveTmplName = $state('');
  saveTmplDesc = $state('');
  saveTmplBusy = $state(false);
  saveTmplErr = $state('');
  // 用模板建站
  useTmplOpen = $state(false);
  useTmplSlug = $state('');
  useTmplName = $state('');
  useTmplProject = $state('');
  useTmplBusy = $state(false);
  useTmplErr = $state('');
  // ---------- 提示词库 ----------
  // 预设（内置只读）+ 用户自建（存 localStorage）。点一下把内容填进启动页对话框开始建站。
  userPrompts = $state<Prompt[]>(loadUserPrompts());
  allPrompts = $derived<Prompt[]>([...PRESET_PROMPTS, ...this.userPrompts]);
  // 新增 / 编辑（只对用户自建的）
  promptEditOpen = $state(false);
  promptEditId = $state('');   // '' = 新增
  promptEditTitle = $state('');
  promptEditBody = $state('');
}

export const library = new LibraryState();

// 分类检索。随附模板的 category 由后端给；用户自己沉淀的没有分类（存模板时不问）——
// 归到「自建」而不是硬塞进某一档，这既是实话，也正好是他们最想单独看的一组。
export const CAT_MINE = '自建';
// 固定顺序：按出现顺序排的话，删一个模板就可能让整排筹码跳位。
const CAT_ORDER = ['落地页', '内容', '作品集', '电商', '企业', '活动'];
export const catOf = (t: Template) => (t.builtin ? t.category || '其他' : CAT_MINE);
/** 模板文本索引：全局搜索按它匹配（名称 / 描述 / slug / 分类）。 */
export const tmplHay = (t: Template) => `${t.name} ${t.desc} ${t.slug} ${catOf(t)}`.toLowerCase();
export async function loadTemplates() {
  library.templatesLoading = true;
  try {
    library.templates = await invoke<Template[]>('list_templates');
    const map: Record<string, string> = {};
    await Promise.all(library.templates.map(async (t) => {
      try { map[t.slug] = await invoke<string>('template_index_html', { slug: t.slug }); } catch { map[t.slug] = ''; }
    }));
    library.tmplHtml = map;
  } catch (e) { say(String(e), 'err'); }
  finally { library.templatesLoading = false; }
}
export function openTemplates() { app.view = 'templates'; app.activeConvId = ''; app.activeConv = null; loadTemplates(); }
/** 全局搜索要搜模板，可 templates 本来只在模板库视图里才加载。这里**只补元数据**：
    loadTemplates 还会把 12 个模板的入口 HTML 全拉一遍（12 次 IPC × 几十 KB，用来做缩略图），
    为了搜索付这个代价不值当——缩略图等真进视图再说。 */
export async function ensureTemplateMeta() {
  if (app.activeConn?.kind !== 'cloudflare' || library.templates.length) return;
  try { library.templates = await invoke<Template[]>('list_templates'); } catch { /* 搜不到模板而已，别让它把搜索也带崩 */ }
}
/** 从全局搜索跳到某个模板卡：等它渲染出来再滚过去。
    openTemplates 里的 loadTemplates 是异步的，写死延时就是在赌它已经回来了——列表一慢就滚了个空。 */
export async function focusTemplate(slug: string) {
  for (let i = 0; i < 40; i++) { // 最多等 ~2s
    const el = document.getElementById(`tmpl-${slug}`);
    if (el) {
      el.scrollIntoView({ block: 'center', behavior: 'smooth' });
      el.classList.add('tmpl-flash');
      setTimeout(() => el.classList.remove('tmpl-flash'), 1400);
      return;
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}
export async function delTemplate(t: Template) {
  const yes = await confirmDialog(`删除模板「${t.name}」？`, { title: '删除模板', kind: 'danger' });
  if (!yes) return;
  try { await invoke('delete_template', { slug: t.slug }); await loadTemplates(); } catch (e) { say(String(e), 'err'); }
}
export async function previewTemplate(t: Template) {
  say('正在启动模板预览…（约 2 秒后打开预览窗）');
  try { const u = await invoke<string>('cf_preview_template', { slug: t.slug }); say(`模板预览已打开（${portOf(u)}）·关掉预览窗即停`); } catch (e) { say(String(e), 'err'); }
}
export function openSaveTmpl() { if (!app.activeConv) return; library.saveTmplName = app.activeConv.site_slug; library.saveTmplDesc = ''; library.saveTmplErr = ''; library.saveTmplOpen = true; }
export async function confirmSaveTmpl() {
  if (!app.activeConv || !library.saveTmplName.trim()) return;
  library.saveTmplBusy = true; library.saveTmplErr = '';
  try {
    await invoke('save_as_template', { connId: app.activeConv.conn_id, project: app.activeConv.site_slug, name: library.saveTmplName.trim(), desc: library.saveTmplDesc.trim() });
    library.saveTmplOpen = false; say('已存为模板');
  } catch (e) { library.saveTmplErr = String(e); }
  finally { library.saveTmplBusy = false; }
}
export function openUseTmpl(t: Template) { library.useTmplSlug = t.slug; library.useTmplName = t.name; library.useTmplProject = ''; library.useTmplErr = ''; library.useTmplOpen = true; }
export async function confirmUseTmpl() {
  if (library.useTmplBusy) return; // 输入框里连按回车
  if (!library.useTmplProject.trim()) { library.useTmplErr = '给新项目起个名'; return; }
  if (!app.isCfConn) { library.useTmplErr = '请先在左下角切到一个 Cloudflare 连接'; return; }
  library.useTmplBusy = true; library.useTmplErr = '';
  try {
    const cid = app.activeConnId;
    const proj = library.useTmplProject.trim();
    await invoke('use_template', { slug: library.useTmplSlug, connId: cid, project: proj });
    library.useTmplOpen = false;
    say(`已用模板「${library.useTmplName}」建好项目，接着描述你要的定制吧`);
    app.activeConvId = ''; app.activeConv = null; app.view = 'launcher';
    await selectConn(cid); // 刷新项目列表（会清空 lSite）
    launcher.lSite = proj;
  } catch (e) { library.useTmplErr = String(e); }
  finally { library.useTmplBusy = false; }
}
export function openPrompts() { app.view = 'prompts'; app.activeConvId = ''; app.activeConv = null; }
export function usePrompt(p: Prompt) {
  launcher.lDraft = p.body;
  app.view = 'launcher';
  app.activeConvId = ''; app.activeConv = null;
  say(app.isCfConn ? '已填入建站需求，起个项目名就能开始' : '已填入需求，选好站点就能开始');
}
export async function copyPrompt(p: Prompt) {
  try { await navigator.clipboard.writeText(p.body); say('已复制提示词'); }
  catch { say('复制失败，请手动选择复制', 'err'); }
}
export function openNewPrompt() { library.promptEditId = ''; library.promptEditTitle = ''; library.promptEditBody = ''; library.promptEditOpen = true; }
export function openEditPrompt(p: Prompt) { library.promptEditId = p.id; library.promptEditTitle = p.title; library.promptEditBody = p.body; library.promptEditOpen = true; }
export function savePrompt() {
  const title = library.promptEditTitle.trim(); const body = library.promptEditBody.trim();
  if (!title || !body) return;
  if (library.promptEditId) {
    library.userPrompts = library.userPrompts.map((p) => (p.id === library.promptEditId ? { ...p, title, body } : p));
  } else {
    library.userPrompts = [...library.userPrompts, { id: newPromptId(), title, body, created_at: Date.now() }];
  }
  if (!saveUserPrompts(library.userPrompts)) { say('保存失败：本地存储写入异常', 'err'); return; } // 存不下就别假装成功、保持弹窗开着
  library.promptEditOpen = false;
}
export async function deletePrompt(p: Prompt) {
  const yes = await confirmDialog(`删除提示词「${p.title}」？`, { title: '删除提示词', kind: 'danger' });
  if (!yes) return;
  library.userPrompts = library.userPrompts.filter((x) => x.id !== p.id);
  if (!saveUserPrompts(library.userPrompts)) say('删除已生效，但本地存储写入异常，重启后可能恢复', 'err');
}

export function initLibrary() {
  // 选中的分类可能整个消失（删掉最后一个自建模板）→ 那会停在一个永远空的筛选上，像坏了。
  $effect(() => {
    if (library.tmplCat !== '全部' && !library.tmplCats.includes(library.tmplCat)) library.tmplCat = '全部';
  });
}
