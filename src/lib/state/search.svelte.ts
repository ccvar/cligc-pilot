import type { Conversation, Connection } from '$lib/types';
import { app } from './app.svelte.js';
import { connHay, selectConn } from './connections.svelte.js';
import { openConv } from './conversations';
import { ensureTemplateMeta, library, tmplHay, openTemplates, openPrompts, focusTemplate } from './library.svelte.js';
import type { Template } from './library.svelte.js';

type SearchEntry =
  | { kind: 'view'; view: 'templates' | 'prompts'; label: string }
  | { kind: 'conv'; c: Conversation }
  | { kind: 'template'; t: Template }
  | { kind: 'conn'; c: Connection };

class SearchState {
  // 全局搜索（分区）：视图导航 / 连接 / 模板 / 会话。
  searchOpen = $state(false);
  searchQ = $state('');
  searchInput = $state<HTMLInputElement | null>(null);
  // 会话匹配：按标题 / 项目名 / 工作目录。
  searchResults = $derived.by(() => {
    const q = this.searchQ.trim().toLowerCase();
    const list = app.convos.filter((c) => {
      // 每条会话固定在创建它的连接里；本地对话同样只在本地工作区里搜。
      if (c.conn_id !== app.activeConnId) return false;
      if (!q) return true;
      return (c.title || '').toLowerCase().includes(q)
        || (c.site_name || '').toLowerCase().includes(q)
        || (c.site_slug || '').toLowerCase().includes(q)
        || (c.workspace_dir || '').toLowerCase().includes(q);
    });
    return list.slice(0, 60);
  });
  /** 分区结果（按匹配数动态显隐）：空查询只给视图导航项。 */
  searchSections = $derived.by(() => {
    const q = this.searchQ.trim().toLowerCase();
    const out: { title: string; entries: SearchEntry[] }[] = [];
    // 模板库 / 提示词只有 Cloudflare 连接有；别的连接下选中它只会被弹回启动页。
    const views = (app.activeConn?.kind === 'cloudflare' ? SEARCH_VIEWS : [])
      .filter((v) => !q || v.label.includes(q))
      .map((v) => ({ kind: 'view' as const, ...v }));
    if (views.length) out.push({ title: '视图', entries: views });
    if (q) {
      // 连接：名称 / IP / user@host / 端口 / 系统。
      // ★ 这一段**不按 activeConnId 过滤** —— 别的分区搜的是「当前连接内部的东西」，
      //   而连接本身是**跨连接**的：机器一多，全局搜索就是最快的切换方式（记不住哪台是哪个 IP）。
      const connHits = app.conns.filter((c) => connHay(c).includes(q)).slice(0, 8);
      if (connHits.length) out.push({ title: '连接', entries: connHits.map((c) => ({ kind: 'conn' as const, c })) });
      // 模板：名称 / 描述 / slug / 分类（模板库视图里因此不再摆搜索框）
      if (app.activeConn?.kind === 'cloudflare') {
        const tplHits = library.templates.filter((t) => tmplHay(t).includes(q)).slice(0, 8);
        if (tplHits.length) out.push({ title: '模板', entries: tplHits.map((t) => ({ kind: 'template' as const, t })) });
      }
      if (this.searchResults.length) out.push({ title: '会话', entries: this.searchResults.map((c) => ({ kind: 'conv' as const, c })) });
    }
    return out;
  });
  searchFlat = $derived(this.searchSections.flatMap((s) => s.entries));
  searchIdx = $state(0);
}

export const search = new SearchState();

export function openSearch() { search.searchOpen = true; search.searchQ = ''; search.searchIdx = 0; void ensureTemplateMeta(); requestAnimationFrame(() => search.searchInput?.focus()); }
const SEARCH_VIEWS: { view: 'templates' | 'prompts'; label: string }[] = [
  { view: 'templates', label: '模板库' },
  { view: 'prompts', label: '提示词' },
];
function searchNudge(delta: number) {
  if (!search.searchFlat.length) return;
  search.searchIdx = Math.max(0, Math.min(search.searchFlat.length - 1, search.searchIdx + delta));
  requestAnimationFrame(() => document.querySelector('.search-item.on')?.scrollIntoView({ block: 'nearest' }));
}
export function pickEntry(it: SearchEntry) {
  search.searchOpen = false;
  if (it.kind === 'view') {
    if (it.view === 'templates') openTemplates();
    else openPrompts();
  } else if (it.kind === 'template') {
    // 分类筛选可能正好把它挡在外面——搜到了、跳过去却看不见，比搜不到还让人懵
    library.tmplCat = '全部';
    openTemplates();
    void focusTemplate(it.t.slug);
  } else if (it.kind === 'conn') {
    // 走和切换器同一条路：连接切换牵着视图守卫、SSH 探系统、终端重建，别在这儿另起一套
    void selectConn(it.c.id);
  } else {
    openConv(it.c.id);
  }
}

export function initSearch() {
  $effect(() => { void search.searchQ; search.searchIdx = 0; }); // 换关键词回到第一项
  $effect(() => {
    if (!search.searchOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') search.searchOpen = false;
      else if (e.key === 'ArrowDown') { e.preventDefault(); searchNudge(1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); searchNudge(-1); }
      else if (e.key === 'Enter') { const it = search.searchFlat[search.searchIdx]; if (it) pickEntry(it); }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  });
}
