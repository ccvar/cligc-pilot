// 左上角的「后退 / 前进」：像浏览器一样记下去过的页面（哪个连接、哪个主视图、哪条会话、站点 / 全部站点 / 托管页里的哪个 tab），
// 点后退回到上一个、点前进再回来。只记页面位置，不记输入框里的草稿、滚动位置这些。
// 一次点击常常连着改好几个状态（先换连接、再换视图），所以停 150 毫秒再记一笔，中间那些半成品不进历史。
import { app } from './app.svelte.js';
import { sites } from './sites.svelte.js';
import { fleet } from './fleet.svelte.js';
import { managed } from './managed.svelte.js';
import { selectConn } from './connections.svelte.js';
import { openConv } from './conversations';

type Loc = { view: typeof app.view; conn: string; conv: string; siteTab: typeof sites.tab; queueFilter: typeof sites.queueFilter; fleetTab: typeof fleet.tab; managedTab: typeof managed.tab };

class NavState {
  back = $state<Loc[]>([]);
  fwd = $state<Loc[]>([]);
}
export const nav = new NavState();

const LIMIT = 50;
let cur: Loc | null = null;
let restoring = false;
// 正在切回某一页（切连接、打开会话要等一会儿）：这期间的状态变化都不记，切完再一次记好
let inFlight = false;
let timer: ReturnType<typeof setTimeout> | undefined;

function here(): Loc {
  return { view: app.view, conn: app.activeConnId, conv: app.view === 'thread' ? app.activeConvId : '', siteTab: sites.tab, queueFilter: sites.queueFilter, fleetTab: fleet.tab, managedTab: managed.tab };
}
/** 只比较这个页面用得上的部分：比如在会话里，站点页的 tab 变了不算换页面 */
function key(l: Loc): string {
  const extra = l.view === 'site' ? `${l.siteTab}:${l.queueFilter}` : l.view === 'sites' ? l.fleetTab : l.view === 'managed' ? l.managedTab : l.view === 'thread' ? l.conv : '';
  return `${l.conn}|${l.view}|${extra}`;
}

function commit() {
  timer = undefined;
  if (inFlight) return;
  const l = here();
  if (restoring) { cur = l; restoring = false; return; }
  if (cur && key(cur) === key(l)) { cur = l; return; }
  if (cur) nav.back = [...nav.back, cur].slice(-LIMIT);
  nav.fwd = [];
  cur = l;
}

export function initNav() {
  $effect(() => {
    void key(here());
    clearTimeout(timer);
    timer = setTimeout(commit, 150);
  });
  // ⌘[ / ⌘]（Windows 上 Ctrl）：和浏览器、访达的后退前进一样
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey) return;
      if (e.key === '[') { e.preventDefault(); void goBack(); }
      else if (e.key === ']') { e.preventDefault(); void goForward(); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
}

async function restore(l: Loc) {
  inFlight = true;
  clearTimeout(timer);
  try {
    if (l.conn && l.conn !== app.activeConnId) await selectConn(l.conn);
    sites.tab = l.siteTab;
    sites.queueFilter = l.queueFilter;
    fleet.tab = l.fleetTab;
    managed.tab = l.managedTab;
    if (l.view === 'thread' && l.conv) {
      await openConv(l.conv);
    } else {
      app.activeConvId = ''; app.activeConv = null;
      app.view = l.view;
    }
  } catch { /* 那条会话删掉了之类：停在现在的地方 */ } finally {
    inFlight = false;
    restoring = true;
    clearTimeout(timer);
    timer = setTimeout(commit, 150);
  }
}

export async function goBack() {
  const l = nav.back.at(-1);
  if (!l || inFlight) return;
  nav.back = nav.back.slice(0, -1);
  if (cur) nav.fwd = [...nav.fwd, cur];
  await restore(l);
}

export async function goForward() {
  const l = nav.fwd.at(-1);
  if (!l || inFlight) return;
  nav.fwd = nav.fwd.slice(0, -1);
  if (cur) nav.back = [...nav.back, cur];
  await restore(l);
}
