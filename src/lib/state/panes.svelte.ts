import { sites, siteIdOf, SITE_PREFIX } from './sites.svelte.js';
import { SITES_CONN_ID } from './env';
import { selectConn } from './connections.svelte.js';
import { type Layout, type Edge, leaves, pane, replacePane, removePane, outerDropEdge, dropEdge, splitPane } from '$lib/conversationLayout';
import { onDestroy, tick, untrack } from 'svelte';
import type { Conversation, Brain } from '$lib/types';
import { invoke } from '@tauri-apps/api/core';
import { app, say, refreshConvos, confirmDialog } from './app.svelte.js';
import { brainUsable, isCodexSsh, CODEX_SSH_NOTE, brainLabel } from './models';
import { thread, conversationModelOptions } from './thread.svelte.js';
import { turns, beginTurn, optimisticUser, makeChannel, endTurn, maybeAutoRetry, failTurn, rebuildSession } from './turns.svelte.js';
import { composer, autogrow, attachmentFilename } from './composer.svelte.js';
import type { Attach } from './composer.svelte.js';
import { detectSiteUrl, conversationProject, ATT_MARKER } from './messages.svelte.js';

type PaneDraft = {text:string;files:Attach[];scroll:number;sticky:boolean};

class PanesState {
  // Layout stores only conversation references. Execution and drafts remain keyed by conversation,
  // so moving/closing a pane never cancels a turn or redirects an in-flight operation.
  conversationLayout = $state<Layout | null>(null);
  conversationLayoutWorkspace = $state('');
  openConvSeq = 0;
  splitFocused = $state('');
  splitDragId = $state('');
  splitDragPoint = $state<{x:number;y:number}|null>(null);
  singleDropEdge = $state<Edge | null>(null);
  splitPointerPreview = $state<{id:string;edge:Edge}|null>(null);
  cancelConversationPointerDrag: ()=>void = ()=>{};
  splitSnapshots = $state<Record<string, Conversation>>({});
  splitDrafts = $state<Record<string, PaneDraft>>({});
  splitErrors = $state<Record<string, string>>({});
  splitQueued = $state<Record<string, {text:string;files:Attach[];startedAt:number}>>({});
  /** 主区现在显示的东西（对应分屏里的哪一格）；空＝显示的是不能进分屏的页面（模板库、远程工作台……）。 */
  mainRef = $derived.by(():string=>{
    if(app.view==='thread')return app.activeConvId;
    if(app.view==='launcher')return app.activeConn && !app.isSshConn ? 'page:launcher' : '';
    if(app.isSitesHub && (app.view==='sites'||app.view==='making'||app.view==='managed'))return `page:${app.view}`;
    if(app.view==='site' && app.isSiteConn)return `board:${siteIdOf(app.activeConnId)}`;
    return '';
  });
  // 对话哪怕只有一格也走画布（那就是普通的对话页）；页面只有一格时按普通页面显示，两格以上才进画布。
  splitVisible = $derived.by(()=>{
    const ref=this.mainRef, all=leaves(this.conversationLayout);
    return !!ref && all.some(p=>p.conversation===ref) && (all.length>1 || !isPageRef(ref));
  });
  paneCfReady = $state<Record<string,boolean>>({});
  paneUploading = $state<Record<string,number>>({});
}

export const panes = new PanesState();

export function splitConversation(id:string):Conversation | null { return panes.splitSnapshots[id] ?? app.convos.find(c=>c.id===id) ?? null; }
/** 分屏格子里能放的东西：对话（会话 id 原样）、页面（`page:launcher` 新对话 / `page:sites` 全部站点 /
 *  `page:making` 站点制作 / `page:managed` 托管运营）、某个站点的看板（`board:<站点 id>`）。 */
export function isPageRef(ref:string):boolean { return ref.startsWith('page:') || ref.startsWith('board:'); }

/** 分屏按「工作区」分组：所有站点的对话算同一个工作区（可以把两个站点的对话拉进同一个分屏、在站点之间切换不清空分屏），
 *  其它连接各自一个。每个分屏里的对话本来就带着自己的连接，发消息、传附件都按对话自己的连接走。 */
export function workspaceOf(connId:string):string {
  return connId===SITES_CONN_ID || connId.startsWith('site:') ? 'site:*' : connId;
}
/** 一格内容属于哪个分屏组：站点相关的页面都在站点组；新对话跟着当前连接。 */
function refWorkspace(ref:string):string {
  if(ref==='page:launcher')return workspaceOf(app.activeConnId);
  if(isPageRef(ref))return 'site:*';
  const c=splitConversation(ref);
  return c?conversationWorkspace(c):'';
}
/** 分屏里一格的名字（拖动时的卡片、格子顶部的标题）。 */
export function paneRefLabel(ref:string):string {
  if(ref==='page:launcher')return '新对话';
  if(ref==='page:sites')return '全部站点';
  if(ref==='page:making')return '站点制作';
  if(ref==='page:managed')return '托管运营';
  if(ref.startsWith('board:')){const s=sites.list.find(x=>x.id===ref.slice(6));return s?`${s.name} · 站点看板`:'站点看板';}
  return splitConversation(ref)?.title??'';
}
export function conversationWorkspace(c:Conversation):string {
  return workspaceOf(c.conn_id);
}
function resetConversationLayout(workspace:string) {
  if(panes.conversationLayoutWorkspace===workspace)return;
  ++panes.openConvSeq; // Invalidate pending opens from the workspace being left.
  panes.cancelConversationPointerDrag();
  endConversationDrag();
  panes.conversationLayout=null;
  panes.splitFocused='';
  panes.conversationLayoutWorkspace=workspace;
}
export function switchConversationWorkspace(workspace:string) {
  if(panes.conversationLayoutWorkspace===workspace)return;
  // Keep drafts/queued turns bound to their conversation, never the next workspace.
  if(app.activeConv && !panes.splitVisible) {
    panes.splitDrafts[app.activeConv.id]={text:composer.draft,files:composer.attachments,scroll:thread.threadEl?.scrollTop??0,sticky:thread.threadStickBottom};
    if(composer.queued?.convId===app.activeConv.id) {
      panes.splitQueued[app.activeConv.id]={text:composer.queued.text,files:composer.queued.atts,startedAt:composer.queued.turnStartedAt};
    }
  }
  resetConversationLayout(workspace);
  app.activeConv=null;app.activeConvId='';composer.draft='';composer.attachments=[];composer.queued=null;
}
const paneInputs = new Map<string,HTMLTextAreaElement>();
const paneScrollers = new Map<string,HTMLDivElement>();
const paneMetadataLoading = new Set<string>();
export function conversationConnection(c:Conversation|null) {
  return c && c.task_type!=='workspace' ? app.conns.find(x=>x.id===c.conn_id)??null : null;
}
export function conversationPresentation(c:Conversation|null) {
  if(c?.task_type==='site'){const s=sites.list.find(x=>x.id===c.site_slug);return {conn:null,url:s?.domain?`https://${s.domain}`:''};}
  const conn=conversationConnection(c);
  if(conn?.kind==='github'&&c?.site_slug)return {conn,url:`https://github.com/${c.site_slug}`};
  return {conn,url:conn?.kind==='cloudflare'&&c ? detectSiteUrl([c]) : ''};
}
export async function loadPaneMetadata(c:Conversation) {
  const conn=conversationConnection(c);
  if(conn?.kind!=='cloudflare' || paneMetadataLoading.has(c.id))return;
  paneMetadataLoading.add(c.id);
  try {
    panes.paneCfReady[c.id]=await invoke<boolean>('cf_project_ready',{connId:c.conn_id,project:c.site_slug});
  }catch{/* Metadata failure must not prevent opening a conversation. */}
  finally{paneMetadataLoading.delete(c.id);}
}
export function setPaneText(id:string,text:string) {
  if(panes.splitDrafts[id])panes.splitDrafts[id].text=text;
  if(app.activeConvId===id)composer.draft=text;
  const input=paneInputs.get(id) as (HTMLTextAreaElement & {__autogrow?:()=>void})|undefined;
  void tick().then(()=>input?.__autogrow?.());
}
export function paneScroll(node:HTMLDivElement, id:string) {
  if(!id){thread.threadEl=node;thread.threadInnerEl=node.firstElementChild as HTMLDivElement;return;}
  paneScrollers.set(id,node);
  const state=panes.splitDrafts[id];
  const restore=()=>{node.scrollTop=state.sticky?node.scrollHeight:state.scroll;};
  const scroll=()=>{state.scroll=node.scrollTop;state.sticky=node.scrollHeight-node.scrollTop-node.clientHeight<100;};
  const observer=new ResizeObserver(()=>{if(state.sticky)node.scrollTop=node.scrollHeight;});
  if(node.firstElementChild)observer.observe(node.firstElementChild);
  const frame=requestAnimationFrame(restore);node.addEventListener('scroll',scroll);
  return {destroy(){cancelAnimationFrame(frame);observer.disconnect();node.removeEventListener('scroll',scroll);paneScrollers.delete(id);}};
}
export function paneInput(node:HTMLTextAreaElement, id:string) {
  if(id)paneInputs.set(id,node);
  if(!id||app.activeConvId===id)composer.draftEl=node;
  const resize=autogrow(node);
  return {destroy(){resize.destroy();if(id)paneInputs.delete(id);}};
}
export async function uploadPaneFiles(id:string,files:FileList|null) {
  const c=splitConversation(id);if(!c||!files)return;
  for(const file of Array.from(files))try{await splitUpload(c,file);}catch(e){panes.splitErrors[id]=String(e);}
}
export function panePrefill(id:string, action:()=>void) {
  const leaf=leaves(panes.conversationLayout).find(p=>p.conversation===id);
  if(leaf)focusSplit(leaf.id);
  action();setPaneText(id,composer.draft);paneInputs.get(id)?.focus();
}
export function rememberSplit(c:Conversation) {
  panes.splitSnapshots[c.id]=c;
  panes.splitDrafts[c.id] ??= {text:'',files:[],scroll:0,sticky:true};
  if(app.activeConvId===c.id) {app.activeConv=c;thread.threadModel=c.model;thread.threadPerm=c.perm_mode||'full';thread.threadEffort=c.effort||'';}
}
export function focusSplit(id:string) {
  const leaf=leaves(panes.conversationLayout).find(p=>p.id===id);
  if(leaf && isPageRef(leaf.conversation)) {
    if(refWorkspace(leaf.conversation)!==panes.conversationLayoutWorkspace)return;
    panes.splitFocused=id;
    void showPage(leaf.conversation);
    return;
  }
  const c=leaf && splitConversation(leaf.conversation);
  if(!c || conversationWorkspace(c)!==panes.conversationLayoutWorkspace)return;
  if(panes.splitFocused===id && app.activeConvId===c.id && app.view==='thread')return;
  panes.splitFocused=id;
  app.view='thread';
  app.activeConvId=c.id; app.activeConv=c;thread.threadModel=c.model;thread.threadPerm=c.perm_mode||'full';thread.threadEffort=c.effort||'';
  composer.draft=panes.splitDrafts[c.id]?.text??'';
  composer.draftEl=paneInputs.get(c.id);
  // Connection discovery is not switched here: all pane actions carry their own target.
}
/** 让主区显示某个页面（点了分屏里那一格）。左栏高亮、新对话的目标站点都跟着它走。 */
export async function showPage(ref:string) {
  if(panes.mainRef===ref)return;
  ++panes.openConvSeq; // 还在路上的「打开对话」不能再把焦点抢回去
  if(ref==='page:launcher') {
    app.activeConvId='';app.activeConv=null;app.view='launcher';
    return;
  }
  if(ref.startsWith('board:')) {
    const id=ref.slice(6);
    // 每个看板记着自己停在哪个页签：切回来时恢复（不然会跟着上一个看板的页签走）
    sites.tab=sites.paneTabs[id]??'overview';
    if(app.activeConnId!==SITE_PREFIX+id)await selectConn(SITE_PREFIX+id);
    app.activeConvId='';app.activeConv=null;app.view='site';
    return;
  }
  if(app.activeConnId!==SITES_CONN_ID)await selectConn(SITES_CONN_ID);
  app.activeConvId='';app.activeConv=null;app.view=ref.slice(5) as typeof app.view;
}
/** 主区现在显示的东西要占一格（还没分屏时先把它放进去，再往旁边加）。返回 false＝它不能进分屏。 */
function ensureMainPane():boolean {
  const ref=panes.mainRef;
  if(!ref)return false;
  if(!isPageRef(ref)){ensureConversationPane();return true;}
  resetConversationLayout(refWorkspace(ref));
  const all=leaves(panes.conversationLayout);
  if(!panes.conversationLayout)panes.conversationLayout=pane(ref);
  else if(!all.some(p=>p.conversation===ref)) {
    const target=all.find(p=>p.id===panes.splitFocused)??all[0];
    panes.conversationLayout=replacePane(panes.conversationLayout,target.id,ref);
  }
  panes.splitFocused=leaves(panes.conversationLayout).find(p=>p.conversation===ref)!.id;
  return true;
}
function ensureConversationPane() {
  if(!app.activeConv)return;
  resetConversationLayout(conversationWorkspace(app.activeConv));
  const savedDraft=panes.splitDrafts[app.activeConv.id];
  rememberSplit(app.activeConv);
  panes.splitDrafts[app.activeConv.id]=savedDraft??{text:composer.draft,files:composer.attachments,scroll:thread.threadEl?.scrollTop??0,sticky:thread.threadStickBottom};
  composer.draft=panes.splitDrafts[app.activeConv.id].text;
  if(composer.queued?.convId===app.activeConv.id) {
    panes.splitQueued[app.activeConv.id]={text:composer.queued.text,files:composer.queued.atts,startedAt:composer.queued.turnStartedAt}; composer.queued=null;
  }
  composer.attachments=[];
  if(!panes.conversationLayout)panes.conversationLayout=pane(app.activeConvId);
  else if(!leaves(panes.conversationLayout).some(p=>p.conversation===app.activeConvId)) {
    const target=leaves(panes.conversationLayout).find(p=>p.id===panes.splitFocused)??leaves(panes.conversationLayout)[0];
    panes.conversationLayout=replacePane(panes.conversationLayout,target.id,app.activeConvId);
  }
  panes.splitFocused=leaves(panes.conversationLayout).find(p=>p.conversation===app.activeConvId)!.id;
  app.view='thread';
  void loadPaneMetadata(app.activeConv);
}
export function closeSplit(id:string) {
  if(!panes.conversationLayout)return;
  const remaining=removePane(panes.conversationLayout,id);
  if(!remaining) return; // The last pane is the ordinary conversation, not a separate mode.
  panes.conversationLayout=remaining;
  if(panes.splitFocused===id)focusSplit(leaves(remaining)[0].id);
}
// 拖动从哪儿都能起（左栏的菜单项、对话行本身就是按钮），只有输入框和标了 data-no-drag 的（删除 ×、展开箭头）不行
const NO_DRAG='input,textarea,select,[data-no-drag]';
export function startConversationDrag(e:DragEvent,id:string) {
  if((e.target as HTMLElement).closest(NO_DRAG)){e.preventDefault();return;}
  panes.splitDragId=id;
  e.dataTransfer?.setData('application/x-pilot-conversation',id);
  if(e.dataTransfer)e.dataTransfer.effectAllowed='move';
}
export function endConversationDrag() {panes.splitDragId='';panes.splitDragPoint=null;panes.singleDropEdge=null;panes.splitPointerPreview=null;}
export function startConversationPointerDrag(e:PointerEvent,id:string) {
  if(e.button!==0||e.isPrimary===false||(e.target as HTMLElement).closest(NO_DRAG))return;
  panes.cancelConversationPointerDrag();
  // Pointer-driven dragging is consistent in WebView2 and WKWebView; native HTML drag
  // remains a fallback. Do not start a move until the user crosses the drag threshold.
  e.preventDefault();
  const startX=e.clientX,startY=e.clientY;
  let moved=false;
  const move=(event:PointerEvent)=>{
    if(!moved&&Math.hypot(event.clientX-startX,event.clientY-startY)<6)return;
    moved=true;panes.splitDragId=id;panes.singleDropEdge=null;panes.splitPointerPreview=null;
    panes.splitDragPoint={x:event.clientX,y:event.clientY};
    const element=document.elementFromPoint(event.clientX,event.clientY);
    const target=element?.closest<HTMLElement>('[data-pilot-pane]');
    if(panes.splitVisible&&target){
      const r=target.getBoundingClientRect(),canvas=target.closest('[data-pilot-canvas]')?.getBoundingClientRect();
      const outer=canvas?outerDropEdge(event.clientX-canvas.left,event.clientY-canvas.top,canvas.width,canvas.height):null;
      panes.splitPointerPreview=outer&&panes.conversationLayout?.kind==='split'
        ?{id:panes.conversationLayout.id,edge:outer}
        :{id:target.dataset.pilotPane!,edge:dropEdge(event.clientX-r.left,event.clientY-r.top,r.width,r.height)};
      if(panes.splitPointerPreview.id===leaves(panes.conversationLayout).find(p=>p.conversation===id)?.id)panes.splitPointerPreview=null;
    }
    else if(!panes.splitVisible&&canStartSplit(id)) {
      const area=element?.closest<HTMLElement>('[data-pilot-workarea]');
      if(area){const r=area.getBoundingClientRect();panes.singleDropEdge=dropEdge(event.clientX-r.left,event.clientY-r.top,r.width,r.height);}
    }
  };
  const cleanup=()=>{document.removeEventListener('pointermove',move);document.removeEventListener('pointerup',up);document.removeEventListener('pointercancel',cancel);document.removeEventListener('keydown',key);window.removeEventListener('blur',cancel);panes.cancelConversationPointerDrag=()=>{};};
  const cancel=()=>{cleanup();endConversationDrag();};
  const key=(event:KeyboardEvent)=>{if(event.key==='Escape')cancel();};
  const up=()=>{
    const dest=panes.splitPointerPreview,edge=panes.singleDropEdge;
    cleanup();endConversationDrag();
    if(!moved)return;
    // A drag must not also run the sidebar's ordinary click-to-replace action.
    const block=(event:MouseEvent)=>{event.preventDefault();event.stopImmediatePropagation();};
    document.addEventListener('click',block,{capture:true,once:true});
    setTimeout(()=>document.removeEventListener('click',block,true),0);
    if(dest)void addSplit(id,dest.edge,dest.id);else if(edge)void addSplit(id,edge);
  };
  panes.cancelConversationPointerDrag=cancel;
  document.addEventListener('pointermove',move);document.addEventListener('pointerup',up);document.addEventListener('pointercancel',cancel);document.addEventListener('keydown',key);window.addEventListener('blur',cancel);
}
/** 还没分屏时，把 ref 拖到主区边上能不能分出一格：主区得是能进分屏的东西，而且不是它自己、同一个分屏组。 */
function canStartSplit(ref:string):boolean {
  if(panes.mainRef)return panes.mainRef!==ref && refWorkspace(ref)===refWorkspace(panes.mainRef);
  return !isPageRef(ref) && !!app.activeConv && app.view==='remote';
}
async function addPageSplit(ref:string,edge:Edge,target?:string) {
  try {
    if(!panes.splitVisible && !ensureMainPane())return;
    if(!panes.conversationLayout || refWorkspace(ref)!==panes.conversationLayoutWorkspace)return;
    const leaf=target??panes.splitFocused;
    if(leaf!==panes.conversationLayout.id&&!leaves(panes.conversationLayout).some(p=>p.id===leaf))return;
    panes.conversationLayout=splitPane(panes.conversationLayout,leaf,ref,edge);
    focusSplit(leaves(panes.conversationLayout).find(p=>p.conversation===ref)!.id);
  } finally {endConversationDrag();}
}
export async function addSplit(id:string,edge:Edge,target?:string) {
  if(isPageRef(id))return addPageSplit(id,edge,target);
  if(!app.convos.some(c=>c.id===id)&&!panes.splitSnapshots[id])return;
  const existing=splitConversation(id);
  const workspace=panes.conversationLayoutWorkspace, navigation=panes.openConvSeq;
  if(!existing || conversationWorkspace(existing)!==workspace)return;
  try {
    const c=app.running[id]&&existing ? existing : await invoke<Conversation|null>('get_conversation',{id});
    if(!c || navigation!==panes.openConvSeq || workspace!==panes.conversationLayoutWorkspace
      || conversationWorkspace(c)!==workspace)return;
    // 主区是页面时先把页面放进一格；远程工作台里开着的对话走原来的路
    if(!panes.splitVisible && !ensureMainPane())ensureConversationPane();
    if(!panes.conversationLayout)return;
    const leaf=target??panes.splitFocused;
    if(leaf!==panes.conversationLayout.id&&!leaves(panes.conversationLayout).some(p=>p.id===leaf))return;
    rememberSplit(c);
    void loadPaneMetadata(c);
    panes.conversationLayout=splitPane(panes.conversationLayout,leaf,id,edge);
    focusSplit(leaves(panes.conversationLayout).find(p=>p.conversation===id)!.id);
  } catch(e) {say(String(e),'err');}
  finally {endConversationDrag();}
}
export function singleConversationDragOver(e:DragEvent) {
  if(!panes.splitDragId||panes.splitVisible||!canStartSplit(panes.splitDragId))return;
  e.preventDefault();
  const r=(e.currentTarget as HTMLElement).getBoundingClientRect();
  panes.singleDropEdge=dropEdge(e.clientX-r.left,e.clientY-r.top,r.width,r.height);
}
export async function splitUpload(c:Conversation,file:File) {
  if(file.size>25*1024*1024)throw new Error('文件太大（上限 25MB）');
  panes.paneUploading[c.id]=(panes.paneUploading[c.id]??0)+1;
  try {
    const path=await invoke<string>('save_attachment',{connId:c.conn_id,project:conversationProject(c),filename:attachmentFilename(file),data:Array.from(new Uint8Array(await file.arrayBuffer()))});
    let preview='';
    if(file.type?.startsWith('image/'))preview=await new Promise<string>(resolve=>{const reader=new FileReader();reader.onload=()=>resolve(String(reader.result));reader.onerror=()=>resolve('');reader.readAsDataURL(file);});
    const state=panes.splitDrafts[c.id];
    if(state)state.files=[...state.files,{name:file.name||path.split('/').pop()||'附件',path,preview}];
  }finally{panes.paneUploading[c.id]--;}
}
export function restoreSplitQueue(id:string) {
  const q=panes.splitQueued[id], state=panes.splitDrafts[id];
  if(!q||!state)return;
  setPaneText(id,[q.text,state.text].filter(Boolean).join('\n\n'));state.files=[...q.files,...state.files];delete panes.splitQueued[id];
}
export async function splitSubmit(id:string) {
  const c=splitConversation(id), state=panes.splitDrafts[id];
  if(!c||!state||(!state.text.trim()&&!state.files.length)||panes.paneUploading[id]||(panes.splitQueued[id]&&!app.running[id]))return;
  if(app.running[id]) {
    const prior=panes.splitQueued[id];
    panes.splitQueued[id]={text:[prior?.text,state.text].filter(Boolean).join('\n'),files:[...(prior?.files??[]),...state.files],startedAt:app.lives[id]?.startedAt??0};setPaneText(id,'');state.files=[];return;
  }
  const text=state.text.trim()+(state.files.length ? (state.text.trim()?'\n\n':'')+ATT_MARKER+'\n'+state.files.map(f=>`- ${f.path}`).join('\n'):'');
  setPaneText(id,'');state.files=[];state.sticky=true;delete panes.splitErrors[id];delete turns.autoRetried[id];delete turns.retryExhausted[id];
  beginTurn(id,{...c,messages:[...c.messages,optimisticUser(text)],status:'running'});
  try {
    const result=await invoke<Conversation>('send_message',{convId:id,message:text,onEvent:makeChannel(id)});
    const failed=app.lives[id]?.failed??false, err=app.lives[id]?.error??'';
    await refreshConvos();endTurn(result,id);maybeAutoRetry(id,failed,err);
  }catch(e){panes.splitErrors[id]=String(e);await failTurn(e,id);}
}
export async function splitStop(id:string) {
  restoreSplitQueue(id);
  if(app.running[id])try{await invoke('cancel_turn',{convId:id});}catch(e){panes.splitErrors[id]=String(e);}
}
export function finishSplitQueue(id:string, c:Conversation|null, failed:boolean, startedAt:number) {
  const q=panes.splitQueued[id], state=panes.splitDrafts[id];
  if(!q||!state)return;
  const last=[...(c?.messages??[])].reverse().find(m=>m.role==='user')?.text.trim();
  const payload=q.text.trim()+(q.files.length?(q.text.trim()?'\n\n':'')+ATT_MARKER+'\n'+q.files.map(f=>`- ${f.path}`).join('\n'):'');
  if(failed||!c||!startedAt||q.startedAt!==startedAt||payload===last){restoreSplitQueue(id);return;}
  delete panes.splitQueued[id];
  const newer={text:state.text,files:state.files};
  state.text=q.text;state.files=q.files;
  void splitSubmit(id); // captures this conversation synchronously, before the first await
  setPaneText(id,newer.text);state.files=newer.files;
}
export function splitModelOptions(c:Conversation) { return conversationModelOptions(c); }
export async function splitSetting(c:Conversation,kind:'model'|'effort'|'fast'|'perm',value:string|boolean) {
  const id=c.id;
  try {
    let result:Conversation|null=null;
    if(kind==='model') {
      if(app.running[id])return;
      const [brain,...parts]=String(value).split('::'),model=parts.join('::');
      if(!app.ALL_BRAINS.includes(brain as Brain)||(!brainUsable(brain as Brain)&&brain!==c.brain))return;
      if(brain!==c.brain) {
        const warning=isCodexSsh(brain,app.conns.some(x=>x.id===c.conn_id&&x.kind==='ssh'))?`\n\n${CODEX_SSH_NOTE}`:'';
        if(!await confirmDialog(`切换后将以历史摘要重建对话，自动重跑最近一条请求。${warning}`,{title:`切换到 ${brainLabel(brain)}`,kind:'warning'}))return;
        if(app.running[id])return;
        result=await invoke('set_conversation_brain_model',{convId:id,brain,model});
        if(result){rememberSplit(result);app.convos=app.convos.map(x=>x.id===id?result!:x);void rebuildSession(id);}return;
      }
      result=await invoke('set_conversation_model',{convId:id,model});
    }else if(kind==='effort')result=await invoke('set_conversation_effort',{convId:id,effort:value});
    else if(kind==='fast')result=await invoke('set_conversation_fast',{convId:id,fast:value});
    else result=await invoke('set_conversation_perm_mode',{convId:id,permMode:value});
    if(result){rememberSplit(result);app.convos=app.convos.map(x=>x.id===id?result!:x);}
  }catch(e){panes.splitErrors[id]=String(e);}
}

export function initPanes() {
  onDestroy(()=>panes.cancelConversationPointerDrag());
  // A conversation always occupies a leaf; splitting changes only its geometry.
  $effect(() => {
    const c=app.activeConv, currentView=app.view;
    if(!c || currentView!=='thread')return;
    untrack(() => {
      if(!panes.conversationLayout || !leaves(panes.conversationLayout).some(p=>p.conversation===c.id))ensureConversationPane();
      else panes.splitSnapshots[c.id]=c;
    });
  });
  // 分屏时从左栏 / 页面里的链接打开一个页面：放进当前聚焦的那一格（和点开对话一样）；已经在分屏里就聚焦它。
  // 只有一格时不管——那就是普通的页面切换。
  $effect(() => {
    const ref=panes.mainRef;
    if(!ref || !isPageRef(ref))return;
    untrack(() => {
      const layout=panes.conversationLayout;
      if(!layout || panes.conversationLayoutWorkspace!==refWorkspace(ref))return;
      const all=leaves(layout), hit=all.find(p=>p.conversation===ref);
      if(hit){panes.splitFocused=hit.id;return;}
      if(all.length<2)return;
      const target=all.find(p=>p.id===panes.splitFocused)??all[0];
      panes.conversationLayout=replacePane(layout,target.id,ref);
      panes.splitFocused=target.id;
    });
  });
  // Existing actions which prefill the active composer (deploy/readiness/etc.) use draft.
  $effect(() => {
    const text=composer.draft, id=app.activeConvId;
    untrack(() => {if(panes.splitVisible && panes.splitDrafts[id])panes.splitDrafts[id].text=text;});
  });
}
