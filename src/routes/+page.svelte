<script lang="ts">
  import ConversationCanvas from '$lib/ConversationCanvas.svelte';
  import SettingsIcon from '$lib/SettingsIcon.svelte';
  import SftpTransfers from '$lib/SftpTransfers.svelte';
  import { SFTP_TRANSFER_HISTORY_KEY, createTransfer, addTransfer, updateTransfer, finishTransfer, readTransferHistory, serializeTransferHistory, isTransferActive, type TransferRecord, type TransferUpdate, type TransferFinalStatus } from '$lib/sftpTransfers';
  import SidebarConversationList from '$lib/SidebarConversationList.svelte';
  import { pane, leaves, splitPane, removePane, replacePane, dropEdge, outerDropEdge, type Layout, type Edge } from '$lib/conversationLayout';
  import { onDestroy, tick, untrack } from 'svelte';
  import { invoke, Channel } from '@tauri-apps/api/core';
  // xterm 自带样式：必须从 JS 侧引入。写进组件样式块的 @import 会被 Svelte 作用域化，
  // 而 xterm 的 DOM 是运行时建的、静态分析看不到 → 26 条 unused selector 警告。
  import '@xterm/xterm/css/xterm.css';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { getVersion } from '@tauri-apps/api/app';
  import { open, save as saveDialog } from '@tauri-apps/plugin-dialog';
  import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
  import { check as checkUpdate } from '@tauri-apps/plugin-updater';
  import { relaunch } from '@tauri-apps/plugin-process';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import ApiServices from '$lib/ApiServices.svelte';
  import type { ApiProfile } from '$lib/apiProfiles';
  import SiteFav from '$lib/SiteFav.svelte';
  import ModelFx from '$lib/ModelFx.svelte';
  import { effortAfterModelPick, type ModelOption } from '$lib/modelSelection';
  import UsageRing from '$lib/UsageRing.svelte';
  import { hideTip, showTipFor, tip as tipAction } from '$lib/tip';
  import AppIcon from '$lib/AppIcon.svelte';
  import type { Connection, BrainsInfo, Brain, CliUpdateInfo, Conversation, Message, TaskType, TurnEvent, ToolCall } from '$lib/types';
  import { loadPrefs, savePrefs } from '$lib/defaults';
  import { copyTargetName, fitFileMenu } from '$lib/sftpFiles';
  import { resolveSftpDropTarget, localFileBasename, type SftpDropTarget } from '$lib/sftpDrop';
  import FileCodeEditor from '$lib/FileCodeEditor.svelte';
  import { inspectEditorText, serializeEditorText } from '$lib/fileEditor';
  import { editMenuTarget } from '$lib/editMenu';
  import { PRESET_PROMPTS, loadUserPrompts, saveUserPrompts, newPromptId, type Prompt } from '$lib/prompts';
  import Dropdown from '$lib/Dropdown.svelte';
  import ConfirmDialog from '$lib/ConfirmDialog.svelte';
  import { dialogGeometry, type DialogGeometryOptions } from '$lib/dialogGeometry';
  import { sheetWidth, type SheetWidthOptions } from '$lib/sheetWidth';
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';

  const DIALOG_GEOMETRY = {
    ssh: { id: 'ssh-connection', minWidth: 460, minHeight: 420, draggable: true, resizable: true, rememberPosition: true, rememberSize: true },
    connectionRemark: { id: 'connection-remark', minWidth: 320, minHeight: 160, draggable: true, resizable: false, rememberPosition: true },
    fileOperation: { id: 'file-operation', minWidth: 320, minHeight: 160, draggable: true, resizable: false, rememberPosition: true },
    fileEditor: { id: 'file-editor', storageVersion: 2, minWidth: 560, minHeight: 380, draggable: true, resizable: true, rememberPosition: true, rememberSize: true },
    cloudflare: { id: 'cloudflare-connect', minWidth: 380, minHeight: 360, draggable: true, resizable: true, rememberPosition: true, rememberSize: true },
    templateSave: { id: 'template-save', minWidth: 320, minHeight: 180, draggable: true, resizable: false, rememberPosition: true },
    templateUse: { id: 'template-use', minWidth: 320, minHeight: 180, draggable: true, resizable: false, rememberPosition: true },
    promptEditor: { id: 'prompt-editor', minWidth: 420, minHeight: 320, draggable: true, resizable: true, rememberPosition: true, rememberSize: true },
  } satisfies Record<string, DialogGeometryOptions>;

  function currentPilotWindowLabel(): string {
    try { return getCurrentWindow().label; }
    catch { return 'main'; }
  }
  const pilotWindowLabel = currentPilotWindowLabel();

  const SHEET_WIDTH = {
    setup: { id: 'connection-settings', defaultWidth: 400, minWidth: 360, maxWidth: 720, label: '调整连接与模型面板宽度' },
    updateCenter: { id: 'update-center', defaultWidth: 540, minWidth: 440, maxWidth: 900, label: '调整更新中心面板宽度' },
  } satisfies Record<string, SheetWidthOptions>;

  // ---------- setup ----------
  let conns = $state<Connection[]>([]);
  let activeConnId = $state('');
  let brains = $state<BrainsInfo | null>(null);
  let apiProfiles = $state<ApiProfile[]>([]);
  let apiProfilesLoaded = $state(false);
  function apiProfile(brain: string): ApiProfile | undefined {
    return brain.startsWith('api:') ? apiProfiles.find((p) => p.id === brain.slice(4)) : undefined;
  }
  function modelIcon(brain: string): string {
    const p = apiProfile(brain);
    return p ? (p.protocol === 'anthropic_messages' ? 'claude' : p.protocol === 'openai_responses' ? 'openai' : 'api') : brain;
  }
  async function refreshApiProfiles() {
    try { apiProfiles = await invoke<ApiProfile[]>('list_api_profiles'); }
    catch (e) { say(`读取 API 服务失败：${String(e)}`, 'err'); }
    finally { apiProfilesLoaded = true; }
  }
  let importBusy = $state(false);
  let setupOpen = $state(false);
  let setupAddOpen = $state(false);
  let setupAddEl = $state<HTMLElement | null>(null);
  let switcherOpen = $state(false);
  function connectionDisplayName(connection: Connection): string {
    return connection.name;
  }
  // 连接切换器里每条连接的右键菜单（新窗口打开 / 编辑 / 删除）。
  let connCtx = $state<null | { x: number; y: number; conn: Connection; ready: boolean }>(null);
  let connCtxMenuEl = $state<HTMLDivElement | null>(null);
  // 删除确认期间保留连接切换器与右键菜单，让用户始终看得见操作来源；
  // 取消后仍回到原菜单，确认后才由 removeConn 主动收起右键菜单。
  let connCtxConfirmPinned = $state(false);
  async function settleConnCtxMenu(connId: string, anchor?: { top: number; bottom: number; right: number }) {
    await tick();
    if (!connCtx || connCtx.conn.id !== connId || !connCtxMenuEl) return;
    const menuRect = connCtxMenuEl.getBoundingClientRect();
    const pad = 8;
    const x = anchor
      ? Math.max(pad, Math.min(anchor.right - menuRect.width, window.innerWidth - menuRect.width - pad))
      : Math.max(pad, Math.min(connCtx.x, window.innerWidth - menuRect.width - pad));
    const y = anchor
      ? anchor.bottom + 5 + menuRect.height <= window.innerHeight - pad
        ? anchor.bottom + 5
        : Math.max(pad, anchor.top - menuRect.height - 5)
      : Math.max(pad, Math.min(connCtx.y, window.innerHeight - menuRect.height - pad));
    connCtx = { ...connCtx, x, y, ready: true };
  }
  async function openConnCtx(e: MouseEvent, c: Connection) {
    e.preventDefault();
    e.stopPropagation(); // 别让全局那个输入框右键菜单也跳出来
    hideTip();
    connCtx = { x: e.clientX, y: e.clientY, conn: c, ready: false };
    await settleConnCtxMenu(c.id);
  }
  async function toggleConnCtx(e: MouseEvent, c: Connection) {
    e.preventDefault();
    e.stopPropagation();
    hideTip();
    if (connCtx?.conn.id === c.id) {
      connCtx = null;
      return;
    }
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    connCtx = {
      x: rect.right - 180,
      y: rect.bottom + 5,
      conn: c,
      ready: false,
    };
    await settleConnCtxMenu(c.id, { top: rect.top, bottom: rect.bottom, right: rect.right });
  }
  let connRemarkEdit = $state<null | { id: string; name: string; original: string; value: string; busy: boolean; error: string }>(null);
  function openConnRemark(c: Connection) {
    connCtx = null;
    switcherOpen = false;
    const value = c.remark?.trim() ?? '';
    connRemarkEdit = { id: c.id, name: c.name, original: value, value, busy: false, error: '' };
  }
  async function saveConnRemark() {
    if (!connRemarkEdit || connRemarkEdit.busy) return;
    connRemarkEdit.busy = true;
    connRemarkEdit.error = '';
    const value = connRemarkEdit.value.trim();
    try {
      const updated = await invoke<Connection>('set_connection_remark', { connId: connRemarkEdit.id, remark: value });
      conns = conns.map((c) => (c.id === updated.id ? updated : c));
      connRemarkEdit = null;
      say(value ? '连接备注已保存' : '连接备注已清除');
    } catch (e) {
      if (connRemarkEdit) {
        connRemarkEdit.error = String(e);
        connRemarkEdit.busy = false;
      }
    }
  }
  $effect(() => {
    if (!connCtx) return;
    const close = () => { if (!connCtxConfirmPinned) connCtx = null; };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape' && !connCtxConfirmPinned) connCtx = null; };
    const onScroll = (e: Event) => {
      const target = e.target;
      if (target instanceof Node && connCtxMenuEl?.contains(target)) return;
      close();
    };
    window.addEventListener('click', close);
    window.addEventListener('contextmenu', close);
    window.addEventListener('keydown', onKey);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('contextmenu', close);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', close);
    };
  });
  function openConnectionSetup(event?: MouseEvent) {
    event?.preventDefault();
    event?.stopPropagation();
    setupOpen = true;
    setupAddOpen = false;
    connCtx = null;
    switcherOpen = false;
  }
  async function openConnWindow(id: string) {
    try { await invoke('open_conn_window', { connId: id }); }
    catch (e) { say(String(e), 'err'); }
  }
  let footerEl = $state<HTMLElement | null>(null);
  $effect(() => {
    if (!setupAddOpen) return;
    const onDoc = (e: MouseEvent) => {
      if (setupAddEl && !setupAddEl.contains(e.target as Node)) setupAddOpen = false;
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setupAddOpen = false; };
    document.addEventListener('mousedown', onDoc, true);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDoc, true);
      document.removeEventListener('keydown', onKey);
    };
  });
  $effect(() => {
    if (!switcherOpen) return;
    const onDoc = (e: MouseEvent) => {
      const target = e.target as Element | null;
      if (connCtxConfirmPinned || target?.closest('[data-keep-connection-menu]')) return;
      if (footerEl && !footerEl.contains(e.target as Node)) switcherOpen = false;
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape' && !connCtxConfirmPinned) switcherOpen = false; };
    // 捕获阶段：点空白（含拖拽区）也能关闭切换器。
    document.addEventListener('mousedown', onDoc, true);
    document.addEventListener('keydown', onKey);
    return () => { document.removeEventListener('mousedown', onDoc, true); document.removeEventListener('keydown', onKey); };
  });
  let flash = $state('');
  let flashKind = $state<'ok' | 'err'>('ok');

  type ConfirmKind = 'warning' | 'danger' | 'info';
  let confirmState = $state<null | { message: string; title: string; kind: ConfirmKind; confirmText: string; cancelText: string }>(null);
  let confirmResolve: ((value: boolean) => void) | null = null;

  function confirmDialog(message: string, options: { title?: string; kind?: ConfirmKind; confirmText?: string; cancelText?: string } = {}) {
    if (confirmResolve) confirmResolve(false);
    return new Promise<boolean>((resolve) => {
      confirmResolve = resolve;
      confirmState = {
        message,
        title: options.title || '请确认操作',
        kind: options.kind || 'warning',
        confirmText: options.confirmText || '确认',
        cancelText: options.cancelText || '取消',
      };
    });
  }

  function finishConfirm(value: boolean) {
    const resolve = confirmResolve;
    confirmResolve = null;
    confirmState = null;
    resolve?.(value);
  }

  // 本地对话是一个不落 connections.json 的虚拟连接：没有凭据，只给模型一个隔离的工作目录。
  const WORKSPACE_CONN_ID = '__workspace__';
  const LOCAL_CONN: Connection = { id: WORKSPACE_CONN_ID, name: '本地对话', kind: 'workspace', skill_dir: '', key_prefix: '', key_kind: '', account_id: '', created_at: '' };
  const activeConn = $derived(activeConnId === WORKSPACE_CONN_ID ? LOCAL_CONN : conns.find((c) => c.id === activeConnId) ?? null);
  /** 全部厂商（顺序即下拉展示顺序）。新增厂商在此登记，brainUsable/brainLabel/模型档随之生效。 */
  const ALL_BRAINS = $derived<Brain[]>(['claude', 'codex', 'grok', ...apiProfiles.map((p): Brain => `api:${p.id}`)]);
  /** 第一个可用（已装已登录）的厂商；全不可用时回退 claude（保持旧行为）。 */
  function firstUsableBrain(): Brain { return ALL_BRAINS.find((b) => brainUsable(b)) ?? 'claude'; }

  // ---------- conversations ----------
  let convos = $state<Conversation[]>([]);
  let activeConvId = $state('');
  let activeConv = $state<Conversation | null>(null);
  // Layout stores only conversation references. Execution and drafts remain keyed by conversation,
  // so moving/closing a pane never cancels a turn or redirects an in-flight operation.
  let conversationLayout = $state<Layout | null>(null);
  let conversationLayoutWorkspace = $state('');
  let openConvSeq = 0;
  let splitFocused = $state('');
  type PaneDraft = {text:string;files:Attach[];scroll:number;sticky:boolean};
  let splitDragId = $state('');
  let splitDragPoint = $state<{x:number;y:number}|null>(null);
  let singleDropEdge = $state<Edge | null>(null);
  let splitPointerPreview = $state<{id:string;edge:Edge}|null>(null);
  let cancelConversationPointerDrag:()=>void=()=>{};
  onDestroy(()=>cancelConversationPointerDrag());
  let splitSnapshots = $state<Record<string, Conversation>>({});
  let splitDrafts = $state<Record<string, PaneDraft>>({});
  let splitErrors = $state<Record<string, string>>({});
  let splitQueued = $state<Record<string, {text:string;files:Attach[];startedAt:number}>>({});
  const splitVisible = $derived.by(()=>!!conversationLayout && view === 'thread'
    && leaves(conversationLayout).some(p=>p.conversation===activeConvId));
  function splitConversation(id:string):Conversation | null { return splitSnapshots[id] ?? convos.find(c=>c.id===id) ?? null; }
  function conversationWorkspace(c:Conversation):string {
    return c.conn_id;
  }
  function resetConversationLayout(workspace:string) {
    if(conversationLayoutWorkspace===workspace)return;
    ++openConvSeq; // Invalidate pending opens from the workspace being left.
    cancelConversationPointerDrag();
    endConversationDrag();
    conversationLayout=null;
    splitFocused='';
    conversationLayoutWorkspace=workspace;
  }
  function switchConversationWorkspace(workspace:string) {
    if(conversationLayoutWorkspace===workspace)return;
    // Keep drafts/queued turns bound to their conversation, never the next workspace.
    if(activeConv && !splitVisible) {
      splitDrafts[activeConv.id]={text:draft,files:attachments,scroll:threadEl?.scrollTop??0,sticky:threadStickBottom};
      if(queued?.convId===activeConv.id) {
        splitQueued[activeConv.id]={text:queued.text,files:queued.atts,startedAt:queued.turnStartedAt};
      }
    }
    resetConversationLayout(workspace);
    activeConv=null;activeConvId='';draft='';attachments=[];queued=null;
  }
  const paneInputs = new Map<string,HTMLTextAreaElement>();
  const paneScrollers = new Map<string,HTMLDivElement>();
  let paneCfReady = $state<Record<string,boolean>>({});
  let paneUploading = $state<Record<string,number>>({});
  const paneMetadataLoading = new Set<string>();
  function conversationConnection(c:Conversation|null) {
    return c && c.task_type!=='workspace' ? conns.find(x=>x.id===c.conn_id)??null : null;
  }
  function conversationPresentation(c:Conversation|null) {
    const conn=conversationConnection(c);
    return {conn,url:conn?.kind==='cloudflare'&&c ? detectSiteUrl([c]) : ''};
  }
  async function loadPaneMetadata(c:Conversation) {
    const conn=conversationConnection(c);
    if(conn?.kind!=='cloudflare' || paneMetadataLoading.has(c.id))return;
    paneMetadataLoading.add(c.id);
    try {
      paneCfReady[c.id]=await invoke<boolean>('cf_project_ready',{connId:c.conn_id,project:c.site_slug});
    }catch{/* Metadata failure must not prevent opening a conversation. */}
    finally{paneMetadataLoading.delete(c.id);}
  }
  function setPaneText(id:string,text:string) {
    if(splitDrafts[id])splitDrafts[id].text=text;
    if(activeConvId===id)draft=text;
    const input=paneInputs.get(id) as (HTMLTextAreaElement & {__autogrow?:()=>void})|undefined;
    void tick().then(()=>input?.__autogrow?.());
  }
  function paneScroll(node:HTMLDivElement, id:string) {
    if(!id){threadEl=node;threadInnerEl=node.firstElementChild as HTMLDivElement;return;}
    paneScrollers.set(id,node);
    const state=splitDrafts[id];
    const restore=()=>{node.scrollTop=state.sticky?node.scrollHeight:state.scroll;};
    const scroll=()=>{state.scroll=node.scrollTop;state.sticky=node.scrollHeight-node.scrollTop-node.clientHeight<100;};
    const observer=new ResizeObserver(()=>{if(state.sticky)node.scrollTop=node.scrollHeight;});
    if(node.firstElementChild)observer.observe(node.firstElementChild);
    const frame=requestAnimationFrame(restore);node.addEventListener('scroll',scroll);
    return {destroy(){cancelAnimationFrame(frame);observer.disconnect();node.removeEventListener('scroll',scroll);paneScrollers.delete(id);}};
  }
  function paneInput(node:HTMLTextAreaElement, id:string) {
    if(id)paneInputs.set(id,node);
    if(!id||activeConvId===id)draftEl=node;
    const resize=autogrow(node);
    return {destroy(){resize.destroy();if(id)paneInputs.delete(id);}};
  }
  async function uploadPaneFiles(id:string,files:FileList|null) {
    const c=splitConversation(id);if(!c||!files)return;
    for(const file of Array.from(files))try{await splitUpload(c,file);}catch(e){splitErrors[id]=String(e);}
  }
  function panePrefill(id:string, action:()=>void) {
    const leaf=leaves(conversationLayout).find(p=>p.conversation===id);
    if(leaf)focusSplit(leaf.id);
    action();setPaneText(id,draft);paneInputs.get(id)?.focus();
  }
  function rememberSplit(c:Conversation) {
    splitSnapshots[c.id]=c;
    splitDrafts[c.id] ??= {text:'',files:[],scroll:0,sticky:true};
    if(activeConvId===c.id) {activeConv=c;threadModel=c.model;threadPerm=c.perm_mode||'full';threadEffort=c.effort||'';}
  }
  function focusSplit(id:string) {
    const leaf=leaves(conversationLayout).find(p=>p.id===id);
    const c=leaf && splitConversation(leaf.conversation);
    if(!c || conversationWorkspace(c)!==conversationLayoutWorkspace)return;
    if(splitFocused===id && activeConvId===c.id)return;
    splitFocused=id;
    activeConvId=c.id; activeConv=c;threadModel=c.model;threadPerm=c.perm_mode||'full';threadEffort=c.effort||'';
    draft=splitDrafts[c.id]?.text??'';
    draftEl=paneInputs.get(c.id);
    // Connection discovery is not switched here: all pane actions carry their own target.
  }
  function ensureConversationPane() {
    if(!activeConv)return;
    resetConversationLayout(conversationWorkspace(activeConv));
    const savedDraft=splitDrafts[activeConv.id];
    rememberSplit(activeConv);
    splitDrafts[activeConv.id]=savedDraft??{text:draft,files:attachments,scroll:threadEl?.scrollTop??0,sticky:threadStickBottom};
    draft=splitDrafts[activeConv.id].text;
    if(queued?.convId===activeConv.id) {
      splitQueued[activeConv.id]={text:queued.text,files:queued.atts,startedAt:queued.turnStartedAt}; queued=null;
    }
    attachments=[];
    if(!conversationLayout)conversationLayout=pane(activeConvId);
    else if(!leaves(conversationLayout).some(p=>p.conversation===activeConvId)) {
      const target=leaves(conversationLayout).find(p=>p.id===splitFocused)??leaves(conversationLayout)[0];
      conversationLayout=replacePane(conversationLayout,target.id,activeConvId);
    }
    splitFocused=leaves(conversationLayout).find(p=>p.conversation===activeConvId)!.id;
    view='thread';
    void loadPaneMetadata(activeConv);
  }
  function closeSplit(id:string) {
    if(!conversationLayout)return;
    const remaining=removePane(conversationLayout,id);
    if(!remaining) return; // The last pane is the ordinary conversation, not a separate mode.
    conversationLayout=remaining;
    if(splitFocused===id)focusSplit(leaves(remaining)[0].id);
  }
  function startConversationDrag(e:DragEvent,id:string) {
    if((e.target as HTMLElement).closest('button')){e.preventDefault();return;}
    splitDragId=id;
    e.dataTransfer?.setData('application/x-pilot-conversation',id);
    if(e.dataTransfer)e.dataTransfer.effectAllowed='move';
  }
  function endConversationDrag() {splitDragId='';splitDragPoint=null;singleDropEdge=null;splitPointerPreview=null;}
  function startConversationPointerDrag(e:PointerEvent,id:string) {
    if(e.button!==0||e.isPrimary===false||(e.target as HTMLElement).closest('button,input,textarea'))return;
    cancelConversationPointerDrag();
    // Pointer-driven dragging is consistent in WebView2 and WKWebView; native HTML drag
    // remains a fallback. Do not start a move until the user crosses the drag threshold.
    e.preventDefault();
    const startX=e.clientX,startY=e.clientY;
    let moved=false;
    const move=(event:PointerEvent)=>{
      if(!moved&&Math.hypot(event.clientX-startX,event.clientY-startY)<6)return;
      moved=true;splitDragId=id;singleDropEdge=null;splitPointerPreview=null;
      splitDragPoint={x:event.clientX,y:event.clientY};
      const element=document.elementFromPoint(event.clientX,event.clientY);
      const target=element?.closest<HTMLElement>('[data-pilot-pane]');
      if(splitVisible&&target){
        const r=target.getBoundingClientRect(),canvas=target.closest('[data-pilot-canvas]')?.getBoundingClientRect();
        const outer=canvas?outerDropEdge(event.clientX-canvas.left,event.clientY-canvas.top,canvas.width,canvas.height):null;
        splitPointerPreview=outer&&conversationLayout?.kind==='split'
          ?{id:conversationLayout.id,edge:outer}
          :{id:target.dataset.pilotPane!,edge:dropEdge(event.clientX-r.left,event.clientY-r.top,r.width,r.height)};
        if(splitPointerPreview.id===leaves(conversationLayout).find(p=>p.conversation===id)?.id)splitPointerPreview=null;
      }
      else if(!splitVisible&&activeConv&&(view==='thread'||view==='remote')) {
        const area=element?.closest<HTMLElement>('[data-pilot-workarea]');
        if(area){const r=area.getBoundingClientRect();singleDropEdge=dropEdge(event.clientX-r.left,event.clientY-r.top,r.width,r.height);}
      }
    };
    const cleanup=()=>{document.removeEventListener('pointermove',move);document.removeEventListener('pointerup',up);document.removeEventListener('pointercancel',cancel);document.removeEventListener('keydown',key);window.removeEventListener('blur',cancel);cancelConversationPointerDrag=()=>{};};
    const cancel=()=>{cleanup();endConversationDrag();};
    const key=(event:KeyboardEvent)=>{if(event.key==='Escape')cancel();};
    const up=()=>{
      const dest=splitPointerPreview,edge=singleDropEdge;
      cleanup();endConversationDrag();
      if(!moved)return;
      // A drag must not also run the sidebar's ordinary click-to-replace action.
      const block=(event:MouseEvent)=>{event.preventDefault();event.stopImmediatePropagation();};
      document.addEventListener('click',block,{capture:true,once:true});
      setTimeout(()=>document.removeEventListener('click',block,true),0);
      if(dest)void addSplit(id,dest.edge,dest.id);else if(edge)void addSplit(id,edge);
    };
    cancelConversationPointerDrag=cancel;
    document.addEventListener('pointermove',move);document.addEventListener('pointerup',up);document.addEventListener('pointercancel',cancel);document.addEventListener('keydown',key);window.addEventListener('blur',cancel);
  }
  async function addSplit(id:string,edge:Edge,target?:string) {
    if(!convos.some(c=>c.id===id)&&!splitSnapshots[id])return;
    const existing=splitConversation(id);
    const workspace=conversationLayoutWorkspace, navigation=openConvSeq;
    if(!existing || conversationWorkspace(existing)!==workspace)return;
    try {
      const c=running[id]&&existing ? existing : await invoke<Conversation|null>('get_conversation',{id});
      if(!c || navigation!==openConvSeq || workspace!==conversationLayoutWorkspace
        || conversationWorkspace(c)!==workspace)return;
      if(!splitVisible)ensureConversationPane();
      if(!conversationLayout)return;
      const leaf=target??splitFocused;
      if(leaf!==conversationLayout.id&&!leaves(conversationLayout).some(p=>p.id===leaf))return;
      rememberSplit(c);
      void loadPaneMetadata(c);
      conversationLayout=splitPane(conversationLayout,leaf,id,edge);
      focusSplit(leaves(conversationLayout).find(p=>p.conversation===id)!.id);
    } catch(e) {say(String(e),'err');}
    finally {endConversationDrag();}
  }
  function singleConversationDragOver(e:DragEvent) {
    if(!splitDragId||splitVisible||!activeConv||(view!=='thread'&&view!=='remote'))return;
    e.preventDefault();
    const r=(e.currentTarget as HTMLElement).getBoundingClientRect();
    singleDropEdge=dropEdge(e.clientX-r.left,e.clientY-r.top,r.width,r.height);
  }
  async function splitUpload(c:Conversation,file:File) {
    if(file.size>25*1024*1024)throw new Error('文件太大（上限 25MB）');
    paneUploading[c.id]=(paneUploading[c.id]??0)+1;
    try {
      const path=await invoke<string>('save_attachment',{connId:c.conn_id,project:conversationProject(c),filename:attachmentFilename(file),data:Array.from(new Uint8Array(await file.arrayBuffer()))});
      let preview='';
      if(file.type?.startsWith('image/'))preview=await new Promise<string>(resolve=>{const reader=new FileReader();reader.onload=()=>resolve(String(reader.result));reader.onerror=()=>resolve('');reader.readAsDataURL(file);});
      const state=splitDrafts[c.id];
      if(state)state.files=[...state.files,{name:file.name||path.split('/').pop()||'附件',path,preview}];
    }finally{paneUploading[c.id]--;}
  }
  function restoreSplitQueue(id:string) {
    const q=splitQueued[id], state=splitDrafts[id];
    if(!q||!state)return;
    setPaneText(id,[q.text,state.text].filter(Boolean).join('\n\n'));state.files=[...q.files,...state.files];delete splitQueued[id];
  }
  async function splitSubmit(id:string) {
    const c=splitConversation(id), state=splitDrafts[id];
    if(!c||!state||(!state.text.trim()&&!state.files.length)||paneUploading[id]||(splitQueued[id]&&!running[id]))return;
    if(running[id]) {
      const prior=splitQueued[id];
      splitQueued[id]={text:[prior?.text,state.text].filter(Boolean).join('\n'),files:[...(prior?.files??[]),...state.files],startedAt:lives[id]?.startedAt??0};setPaneText(id,'');state.files=[];return;
    }
    const text=state.text.trim()+(state.files.length ? (state.text.trim()?'\n\n':'')+ATT_MARKER+'\n'+state.files.map(f=>`- ${f.path}`).join('\n'):'');
    setPaneText(id,'');state.files=[];state.sticky=true;delete splitErrors[id];delete autoRetried[id];delete retryExhausted[id];
    beginTurn(id,{...c,messages:[...c.messages,optimisticUser(text)],status:'running'});
    try {
      const result=await invoke<Conversation>('send_message',{convId:id,message:text,onEvent:makeChannel(id)});
      const failed=lives[id]?.failed??false, err=lives[id]?.error??'';
      await refreshConvos();endTurn(result,id);maybeAutoRetry(id,failed,err);
    }catch(e){splitErrors[id]=String(e);await failTurn(e,id);}
  }
  async function splitStop(id:string) {
    restoreSplitQueue(id);
    if(running[id])try{await invoke('cancel_turn',{convId:id});}catch(e){splitErrors[id]=String(e);}
  }
  function finishSplitQueue(id:string, c:Conversation|null, failed:boolean, startedAt:number) {
    const q=splitQueued[id], state=splitDrafts[id];
    if(!q||!state)return;
    const last=[...(c?.messages??[])].reverse().find(m=>m.role==='user')?.text.trim();
    const payload=q.text.trim()+(q.files.length?(q.text.trim()?'\n\n':'')+ATT_MARKER+'\n'+q.files.map(f=>`- ${f.path}`).join('\n'):'');
    if(failed||!c||!startedAt||q.startedAt!==startedAt||payload===last){restoreSplitQueue(id);return;}
    delete splitQueued[id];
    const newer={text:state.text,files:state.files};
    state.text=q.text;state.files=q.files;
    void splitSubmit(id); // captures this conversation synchronously, before the first await
    setPaneText(id,newer.text);state.files=newer.files;
  }
  function splitModelOptions(c:Conversation) { return conversationModelOptions(c); }
  async function splitSetting(c:Conversation,kind:'model'|'effort'|'fast'|'perm',value:string|boolean) {
    const id=c.id;
    try {
      let result:Conversation|null=null;
      if(kind==='model') {
        if(running[id])return;
        const [brain,...parts]=String(value).split('::'),model=parts.join('::');
        if(!ALL_BRAINS.includes(brain as Brain)||(!brainUsable(brain as Brain)&&brain!==c.brain))return;
        if(brain!==c.brain) {
          const warning=isCodexSsh(brain,conns.some(x=>x.id===c.conn_id&&x.kind==='ssh'))?`\n\n${CODEX_SSH_NOTE}`:'';
          if(!await confirmDialog(`切换后将以历史摘要重建对话，自动重跑最近一条请求。${warning}`,{title:`切换到 ${brainLabel(brain)}`,kind:'warning'}))return;
          if(running[id])return;
          result=await invoke('set_conversation_brain_model',{convId:id,brain,model});
          if(result){rememberSplit(result);convos=convos.map(x=>x.id===id?result!:x);void rebuildSession(id);}return;
        }
        result=await invoke('set_conversation_model',{convId:id,model});
      }else if(kind==='effort')result=await invoke('set_conversation_effort',{convId:id,effort:value});
      else if(kind==='fast')result=await invoke('set_conversation_fast',{convId:id,fast:value});
      else result=await invoke('set_conversation_perm_mode',{convId:id,permMode:value});
      if(result){rememberSplit(result);convos=convos.map(x=>x.id===id?result!:x);}
    }catch(e){splitErrors[id]=String(e);}
  }
  // 会话可切换模型；threadModel 跟随活动会话、改动即持久化。
  let threadModel = $state('');
  async function persistThreadModel(m: string) {
    if (!activeConv || m === activeConv.model) return;
    const resetsSession = activeConv.brain === 'claude' || activeConv.brain.startsWith('api:');
    try {
      const u = await invoke<Conversation | null>('set_conversation_model', { convId: activeConv.id, model: m });
      if (u && activeConvId === u.id) activeConv = u;
      // claude 的 --resume 无视 --model（后端因此清了 session）：如实告知生效方式。
      if (resetsSession) say('模型已切换：下一条消息将以历史摘要重建会话后用新模型跑');
    } catch (e) { say(String(e), 'err'); }
  }
  // 会话内模型面板：三家厂商的模型都列出（值编码 "<brain>::<model>"，同启动器 comboOpts；行首图标区分）。
  // 当前厂商永远可选（保持原行为）；其他厂商未就绪（未装/未登录）整组置灰。当前厂商排前面。
  function conversationModelOptions(c:Conversation|null) {
    const cur = (c?.brain ?? 'claude') as Brain;
    const order: Brain[] = [cur, ...ALL_BRAINS.filter((b) => b !== cur)];
    const out: ModelOption[] = [];
    for (const b of order) {
      // 当前厂商永远可选（会话已经在用它了，灰掉只会让触发器显示成一个选不中的项）。
      // 远程连接下的 codex 不置灰、只挂注记：它能用，只是那道闸是打折的（完整说法见 permTipFor）。
      const usable = b.startsWith('api:') ? brainUsable(b) : b === cur || brainUsable(b);
      const note = isCodexSsh(b, conversationConnection(c)?.kind==='ssh') ? CODEX_SSH_NOTE : brainAvailability(b);
      for (const m of launcherModelOpts(b)) {
        out.push({ value: `${b}::${m.value}`, label: m.label, sub: note || m.sub, icon: modelIcon(b), disabled: !usable, apiProtocol: apiProfile(b)?.protocol });
      }
    }
    // 会话当前模型不在清单里（比如自定义 ID 事后被删）：补一条，触发器别显示成裸编码值。
    const curVal = `${cur}::${c?.model}`;
    if (!out.some((o) => o.value === curVal)) out.unshift({ value: curVal, label: c?.model || '模型', sub: brainAvailability(cur) || '当前会话', icon: modelIcon(cur), disabled: cur.startsWith('api:') && !brainUsable(cur), apiProtocol: apiProfile(cur)?.protocol });
    return out;
  }
  const threadComboOpts = $derived(conversationModelOptions(activeConv));
  // 会话内选模型：同厂商＝只改存储、下一轮生效（原行为不变）；跨厂商＝底层 session 换不了家，
  // 确认后更新会话 brain/model（后端顺带清 session_ref；非 ssh 的 codex 下 ask/auto 落 full，
  // ssh 下**保持原档**——桥那道卡是 codex 仅剩的闸，见 lib.rs::apply_brain_switch），
  // 再走「重建继续」：全新底层会话 + 历史摘要，自动重跑最近一条用户请求。
  async function pickThreadCombo(v: string) {
    const conv = activeConv;
    if (!conv || running[conv.id]) return;
    const i = v.indexOf('::');
    if (i < 0) return;
    const brain = v.slice(0, i) as Brain;
    const model = v.slice(i + 2);
    if (brain === conv.brain) { threadModel = model; void persistThreadModel(model); return; }
    const label = brainLabel(brain);
    // ★ 这个框就是「在真机上自动重跑」的同意点：切完立刻 rebuildSession 重跑最近一条请求。
    // 切到 codex 的风险必须**当场**说，不能只靠权限档位 tip 那种要悬停才出的——用户点完就跑了。
    const risk = isCodexSsh(brain, activeConvIsSsh)
      ? `\n\n注意：Codex 无头模式没有逐命令闸。经 Pilot 跑的远程命令仍会弹卡${threadPerm === 'full' ? '（但「全自动」档下这些卡也全关了）' : ''}，但它可以绕开 Pilot、直接用你本机的 ssh 和 ~/.ssh 密钥连这台机器——那条路一张卡都不弹。`
      : '';
    const yes = await confirmDialog(
      `切到 ${label} 后，这条对话将以历史摘要重建一个全新底层会话继续（自动重跑最近一条请求）。项目文件不受影响；很长的对话会有一定上下文损耗。${risk}`,
      { title: `切换到 ${label}`, kind: 'warning' },
    );
    if (!yes) return;
    try {
      const u = await invoke<Conversation | null>('set_conversation_brain_model', { convId: conv.id, brain, model });
      if (u && activeConvId === u.id) { activeConv = u; threadModel = u.model; threadPerm = u.perm_mode || 'full'; threadEffort = u.effort || ''; }
      void rebuildSession(conv.id);
    } catch (e) { say(String(e), 'err'); }
  }
  // 进行中会话可切换权限档位：claude 的钩子/参数和 codex 的 sandbox 都是每轮下发的，
  // 改完从下一轮（含排队消息）生效；正在跑的那轮已带旧档位启动，改动拦不住它（想拦先停止）。
  let threadPerm = $state('');
  let threadEffort = $state('');
  async function persistThreadEffort(v: string) {
    if (!activeConv || v === (activeConv.effort || '')) return;
    try {
      const u = await invoke<Conversation | null>('set_conversation_effort', { convId: activeConv.id, effort: v });
      if (u && activeConvId === u.id) { activeConv = u; threadEffort = u.effort || ''; }
      if (viewBusy) say('推理强度已调整，从下一轮开始生效');
    } catch (e) { say(String(e), 'err'); }
  }
  async function persistThreadFast(v: boolean) {
    if (!activeConv || v === (activeConv.fast ?? false)) return;
    try {
      const u = await invoke<Conversation | null>('set_conversation_fast', { convId: activeConv.id, fast: v });
      if (u && activeConvId === u.id) activeConv = u;
      if (viewBusy) say('Fast 模式已调整，从下一轮开始生效');
    } catch (e) { say(String(e), 'err'); }
  }
  async function persistThreadPerm(p: string) {
    if (!activeConv || p === (activeConv.perm_mode || 'full')) return;
    try {
      const u = await invoke<Conversation | null>('set_conversation_perm_mode', { convId: activeConv.id, permMode: p });
      // 回写 threadPerm：若这段 RPC 期间轮次刚好收尾，endTurn 可能已把下拉打回旧值，这里以落库值自愈。
      if (u && activeConvId === u.id) { activeConv = u; threadPerm = u.perm_mode || 'full'; }
      // 放宽到全自动＝实时生效（后端把本轮已弹/将弹的批准卡全部自动放行）；
      // 收紧方向做不到——本轮是带旧档启动的，钩子/参数拦不回来，只能下一轮。
      if (viewBusy) say(p === 'full' ? '已切全自动：本轮剩余操作不再弹卡' : '权限档位已收紧，从下一轮开始生效（正在跑的这轮拦不回来）');
    } catch (e) { say(String(e), 'err'); }
  }
  let view = $state<'launcher' | 'thread' | 'sites' | 'schedule' | 'tasks' | 'managed' | 'templates' | 'prompts' | 'remote' | 'skills' | 'skillchat'>('launcher');

  // ---------- launcher form ----------
  let prefs = $state(loadPrefs());
  let lSite = $state('');
  // Cloudflare 建站：连接是 CF 时，lSite 复用为「项目名」，站点选择器换成项目输入。
  const isCfConn = $derived(activeConn?.kind === 'cloudflare');
  let cfProjects = $state<string[]>([]);
  // 自由对话的 conn_id 只表示侧栏创建位置，绝不能把该连接当成运行上下文。
  const activeConvIsCf = $derived(!!activeConv && activeConv.task_type !== 'workspace' && conns.find((c) => c.id === activeConv!.conn_id)?.kind === 'cloudflare');
  const activeConvConn = $derived(activeConv && activeConv.task_type !== 'workspace' ? (conns.find((c) => c.id === activeConv!.conn_id) ?? null) : null);
  const activeConvIsSsh = $derived(activeConvConn?.kind === 'ssh');
  const activeConvIsWorkspace = $derived(activeConv?.task_type === 'workspace');
  let previewBusy = $state(false);
  let cfReady = $state(false); // 项目是否已建出可预览/部署的内容（否则预览/部署置灰）
  async function checkCfReady() {
    if (!activeConvIsCf || !activeConv) { cfReady = false; return; }
    const c=activeConv;
    try {
      const ready=await invoke<boolean>('cf_project_ready', { connId:c.conn_id, project:c.site_slug });
      paneCfReady[c.id]=ready;
      if(activeConvId===c.id)cfReady=ready;
    }
    catch { paneCfReady[c.id]=false;if(activeConvId===c.id)cfReady=false; }
  }
  /** 从预览 URL 里取 ":端口" 给提示用（端口是后端挑的，不再恒等于 8788）。 */
  function portOf(url: string): string {
    try { return ':' + new URL(url).port; } catch { return url; }
  }
  async function startPreview() {
    if (!activeConv || previewBusy) return;
    previewBusy = true;
    say('正在启动本地预览…（起来就开窗）');
    try {
      // 端口是后端挑的空端口（8788 被别的程序占着时会自动往上让），所以别写死——用它返回的真 URL。
      const u = await invoke<string>('cf_preview_start', { connId: activeConv.conn_id, project: activeConv.site_slug });
      say(`本地预览已打开（${portOf(u)}）·关掉预览窗即停止`);
    } catch (e) { say(String(e), 'err'); }
    finally { previewBusy = false; }
  }
  let wrInstalling = $state(false);
  let wrElapsed = $state(0); // 安装 wrangler 已用时（毫秒），给个"在动"的实时反馈
  let wrTimer: ReturnType<typeof setInterval> | undefined;
  async function installWrangler() {
    if (wrInstalling) return;
    wrInstalling = true; wrElapsed = 0;
    const t0 = Date.now();
    wrTimer = setInterval(() => { wrElapsed = Date.now() - t0; }, 500);
    try { const m = await invoke<string>('install_wrangler'); say(m); await refreshBrainsManual(); }
    catch (e) { say(String(e), 'err'); }
    finally { wrInstalling = false; if (wrTimer) { clearInterval(wrTimer); wrTimer = undefined; } }
  }
  // Claude Code 用官方原生安装器（独立二进制，不需要 Node/npm）——npm 命令对新手是三重门槛。
  // 注意：不能引用 isWindows（声明在后面，TDZ），这里自足判断。
  const CLAUDE_INSTALL_CMD = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)
    ? 'irm https://claude.ai/install.ps1 | iex'
    : 'curl -fsSL https://claude.ai/install.sh | sh';
  // Grok 官方脚本（独立二进制，不需要 Node）；Windows 官方要求 Git-Bash/MSYS2 跑同一条命令。
  const GROK_INSTALL_CMD = 'curl -fsSL https://x.ai/cli/install.sh | bash';
  let claudeInstalling = $state(false);
  let claudeElapsed = $state(0);
  let claudeTimer: ReturnType<typeof setInterval> | undefined;
  let codexInstalling = $state(false);
  let codexElapsed = $state(0);
  let codexTimer: ReturnType<typeof setInterval> | undefined;
  async function installCodex() {
    if (codexInstalling) return;
    codexInstalling = true; codexElapsed = 0;
    const t0 = Date.now();
    codexTimer = setInterval(() => { codexElapsed = Date.now() - t0; }, 500);
    try { const m = await invoke<string>('install_codex'); say(m); await refreshBrainsManual(); }
    catch (e) { say(String(e), 'err', 20000); }
    finally { codexInstalling = false; nodeBoot = ''; if (codexTimer) { clearInterval(codexTimer); codexTimer = undefined; } }
  }
  let grokInstalling = $state(false);
  let grokElapsed = $state(0);
  let grokTimer: ReturnType<typeof setInterval> | undefined;
  async function installGrok() {
    if (grokInstalling) return;
    grokInstalling = true; grokElapsed = 0;
    const t0 = Date.now();
    grokTimer = setInterval(() => { grokElapsed = Date.now() - t0; }, 500);
    try { const m = await invoke<string>('install_grok'); say(m); await refreshBrainsManual(); }
    catch (e) { say(String(e), 'err', 20000); }
    finally { grokInstalling = false; nodeBoot = ''; if (grokTimer) { clearInterval(grokTimer); grokTimer = undefined; } }
  }
  // 托管 Node 自举进度（后端 "node-boot" 事件）：一键安装按钮上分步显示。
  let nodeBoot = $state('');
  $effect(() => {
    const un = getCurrentWindow().listen<{ phase: string; pct: number }>('node-boot', (e) => {
      const p = e.payload;
      nodeBoot = p.phase === 'download' ? `下载 Node ${p.pct}%`
        : p.phase === 'extract' ? '解压 Node…'
        : p.phase === 'verify' ? '校验 Node…'
        : p.phase === 'grok-download' ? `下载 Grok ${p.pct}%`
        : p.phase === 'grok-verify' ? '校验 Grok…'
        : '安装 CLI…';
    });
    return () => { void un.then((f) => f()); };
  });
  async function installClaude() {
    if (claudeInstalling) return;
    claudeInstalling = true; claudeElapsed = 0;
    const t0 = Date.now();
    claudeTimer = setInterval(() => { claudeElapsed = Date.now() - t0; }, 500);
    try { const m = await invoke<string>('install_claude'); say(m); await refreshBrainsManual(); }
    catch (e) { say(String(e), 'err', 20000); } // 详情含输出末尾，多留时间读/滚动
    finally { claudeInstalling = false; nodeBoot = ''; if (claudeTimer) { clearInterval(claudeTimer); claudeTimer = undefined; } }
  }
  function fillDeploy() {
    if (viewBusy) return;
    // 预填一条部署指令，用户可补上域名后发送；真正的 wrangler deploy 会经权限确认。
    draft = '把当前项目构建并部署到 Cloudflare Pages。若要绑定自定义域名，用这个域名：';
  }
  let lBrain = $state<Brain>(prefs.brain);
  let lModel = $state(prefs.model);
  // 本地对话默认全自动；Cloudflare / SSH 在 selectConn 里把「全自动」降为「自动」。已有会话继续读取各自保存的 perm_mode。
  let lPerm = $state<string>(prefs.perm ?? 'full');
  let lEffort = $state<string>(prefs.effort ?? '');
  /** fast 模式。只有 Opus 5.5 / Opus 5 / Opus 4.8 支持 —— 换到别的模型必须自动落下，
   *  否则会留下一个「显示开着、对该模型其实无效」的状态，比关着更误导。 */
  let lFast = $state<boolean>(prefs.fast ?? false);
  const FAST_MODELS = ['claude-opus-5-5', 'claude-opus-5', 'claude-opus-4-8'];
  const fastable = (m: string) => FAST_MODELS.includes(m);
  $effect(() => { if (lFast && !fastable(lModel)) { lFast = false; prefs.fast = false; savePrefs(prefs); } });
  function setFast(v: boolean) { lFast = v; prefs.fast = v; savePrefs(prefs); }
  let workspaceDir = $state('');
  function workspaceFolderName(path: string): string {
    const parts = path.replace(/[\\/]+$/, '').split(/[\\/]/).filter(Boolean);
    return parts[parts.length - 1] || path;
  }
  async function chooseWorkspaceFolder() {
    const picked = await open({ title: '选择自由对话使用的文件夹', directory: true, multiple: false });
    if (typeof picked === 'string' && picked.trim()) workspaceDir = picked;
  }
  const permOpts = [
    { value: 'plan', label: '计划', sub: '只读 · 只给方案' },
    { value: 'ask', label: '询问', sub: '每步都要你批准' },
    { value: 'auto', label: '自动', sub: '仅危险动作确认' },
    { value: 'full', label: '全自动', sub: '全程不拦' },
  ];
  // CF 建站「视觉风格」预设：每档是一组具体 design tokens 指令，拼进首条消息，
  // 把"语言欠定视觉"变成"点一下锁定方向"。空 = 让 AI 按内容自行判断。
  const STYLE_OPTS = [
    { value: '', label: '风格：让 AI 定', sub: '按站点内容自行判断' },
    { value: 'minimal', label: '极简留白', sub: '黑白灰 · 大留白' },
    { value: 'editorial', label: '杂志编辑', sub: '衬线标题 · 窄栏长文' },
    { value: 'dark-tech', label: '深色科技', sub: '近黑底 · 霓虹点缀' },
    { value: 'warm-craft', label: '暖色手作', sub: '奶油底 · 陶土强调' },
    { value: 'saas', label: '企业 SaaS', sub: '浅底 · 卡片层次' },
  ];
  const STYLE_DIRECTIVES: Record<string, string> = {
    minimal: '极简留白：纯白底、近黑文字，仅一个低饱和强调色；无衬线字体；区块间大留白、细分割线；圆角 8px；无花哨动效。',
    editorial: '杂志编辑风：米白底；衬线大标题 + 无衬线正文；正文窄栏（约 720px）居中；标题字号大胆、段落节奏松；用细横线分节。',
    'dark-tech': '深色科技：近黑背景（如 #0b0e14）、高对比浅色文字；一个霓虹强调色（青或紫）；数据/代码用等宽字体点缀；小圆角或直角；hover 微发光。',
    'warm-craft': '暖色手作：奶油白/米色底、深棕文字、陶土或暖橙强调色；12px 圆润圆角；人文气质字体；阴影温和；整体温暖安静。',
    saas: '企业 SaaS：浅灰白底、品牌蓝或紫强调；卡片 + 轻阴影分层；间距体系化、信息密度适中；CTA 按钮醒目；配 FAQ/价格等标准分区样式。',
  };
  let lStyle = $state('');

  // 启动页「厂商+模型」合并为一个下拉：行带厂商图标，值编码为 "<brain>::<model>"。
  // 厂商未就绪时整组置灰（原厂商下拉的禁用语义保留）。
  const comboOpts = $derived.by(() => {
    const out: ModelOption[] = [];
    for (const b of ALL_BRAINS) {
      const usable = brainUsable(b);
      const note = isCodexSsh(b, isSshConn) ? CODEX_SSH_NOTE : brainAvailability(b);
      for (const m of launcherModelOpts(b)) {
        out.push({ value: `${b}::${m.value}`, label: m.label, sub: note || m.sub, icon: modelIcon(b), disabled: !usable, apiProtocol: apiProfile(b)?.protocol });
      }
    }
    return out;
  });
  // Codex + 远程连接（用户拍板放开）：能用，但闸是**打折**的 —— 走 AI 桥的远程命令照样逐条弹卡
  // （闸在 bridge.rs，与厂商无关），可它无头下没有逐工具闸（claude 有 PreToolUse 钩子、grok 有 ACP
  // 权限回调），拦不住它绕开桥、直接用系统 ssh + 用户自己的 ~/.ssh 密钥打同一台机器。
  // 所以这里不置灰、只挂这句注记 —— 完整说法在权限档位的 tip 里（permTipFor）。
  // ★ 这句话必须**自足**：它长在模型下拉里，别再写「见下方警告」——下面那个红框已经撤了，
  //   指过去就是指向空气（这坑踩过一次）。
  const CODEX_SSH_NOTE = '拦不住它绕开 Pilot 的确认卡';
  function isCodexSsh(b: string, ssh: boolean): boolean { return ssh && b === 'codex'; }
  function pickCombo(v: string) {
    const i = v.indexOf('::');
    if (i < 0) return;
    lEffort = effortAfterModelPick(`${lBrain}::${lModel}`, v, lEffort, comboOpts);
    lBrain = v.slice(0, i) as Brain;
    lModel = v.slice(i + 2);
    if (lBrain.startsWith('api:')) lFast = false;
    persistLauncherEffort(lEffort);
  }
  function persistLauncherEffort(v: string) {
    lEffort = v;
    // Keep the model identity and its protocol-specific effort in one snapshot,
    // even before a first message is sent or the managed wizard is opened.
    prefs.brain = lBrain; prefs.model = lModel; prefs.effort = lEffort; prefs.fast = lFast;
    savePrefs(prefs);
  }

  // 权限档位按风险给下拉文字上色：自动=警告色、全自动(全程不拦)=危险色，计划/询问保持中性。
  function permTone(v: string): string { return v === 'full' ? 'danger' : v === 'auto' ? 'warn' : ''; }
  // Codex 没有逐命令确认（询问/自动的弹卡是 Claude 的 PreToolUse 钩子）——这两档对它无意义，
  // 下拉里直接置灰，省掉那行说明。Codex 实际就两档能用：计划(只读) / 全自动(建站要联网，跑得动)。
  // **远程连接例外**：那里的命令闸在 Pilot 里（AI 桥，见 bridge.rs），不依赖厂商的钩子能力，
  // 所以 codex 一样能逐条确认远程命令 —— 这两档对它有意义，不置灰。
  // 远程连接下每个档位的长说明（挂 data-tip，Dropdown 会把它交给全局 .ui-tip；\n 会保留）。
  // ★ 这些话原来是 composer 下面的常驻框，现在长在档位本身上 —— 那才是**决策点**：
  // 原来的写法全都 reactive 于 perm==='full'，等于用户已经把闸拨到底了才弹出来说「这样很危险」，
  // 亡羊补牢。挂在选项上，用户在**拨之前**就读得到；挂在触发器上，选完悬停还能复查。
  function permTipFor(brain: string, perm: string, ssh = false): string {
    if (!ssh) return '';
    const codex = brain === 'codex';
    if (perm === 'full') {
      return codex
        ? 'Codex ＋「全自动」＝ 没有任何确认。\n「全自动」档下 Pilot 的确认卡本来就全关了，而 Codex 无头模式自己也没有逐命令闸——它在这台机器上删数据、改配置、重启都不会问你。\n它还能绕开 Pilot，直接用你本机的 ssh 和 ~/.ssh 密钥连上来。\n除非你非常清楚要这样，否则退回「自动」。'
        : '「全自动」档下，AI 在这台机器上跑命令不会问你——包括删数据、改配置、重启。\n除非你清楚要这样，否则用「自动」。';
    }
    if (perm === 'ask' || perm === 'auto') {
      return codex
        ? 'Codex 的确认卡是打折的。\n它经 Pilot 跑的每条远程命令仍会弹卡等你确认；但 Codex 无头模式没有逐命令闸，它可以绕开 Pilot、直接用你本机的 ssh 和 ~/.ssh 密钥连这台机器——那条路一张卡都不弹。\n要每条命令都真正过你的眼，用 Claude 或 Grok。'
        : '远程命令的闸在 Pilot 里（与厂商无关）：每条要在这台机器上跑的命令都会弹卡等你确认。';
    }
    // plan 档在 ssh 下：bridge.rs 的 gate() 对 Plan 一律 Err —— 连只读命令都不跑，别写「只读远端」。
    if (perm === 'plan') return '只出方案、不碰这台机器：计划档下 AI 一条远程命令都不会跑（连只读的也不跑）。';
    return '';
  }
  function permOptsFor(brain: string, ssh = false) {
    if (ssh) {
      // 远程连接下「询问/自动」对三家**都有意义**：远程命令的闸在 Pilot 的 AI 桥里（bridge.rs），
      // 与厂商无关 —— 所以这里不像下面那样把 codex 的这两档灰掉。
      // 但对 codex 要说实话：它能绕开桥（见 permTipFor），所以副标题不能许「每条都确认」。
      const codex = brain === 'codex';
      return permOpts.map((o) => {
        const tip = permTipFor(brain, o.value, true);
        return o.value === 'auto' ? { ...o, tip, sub: codex ? '经 Pilot 的远程命令要确认' : '本地放行 · 每条远程命令都确认' }
        : o.value === 'ask' ? { ...o, tip, sub: codex ? '经 Pilot 的远程命令要确认' : o.sub }
        // ★ 全自动的副标题就是那句警告的**短版**（红字），长版在 tip 里。codex 这档一道闸都没有。
        : o.value === 'full' ? { ...o, tip, tone: 'danger', sub: codex ? 'Codex：一张确认卡都没有' : '连远程命令也不拦' }
        : { ...o, tip };
      });
    }
    if (brain !== 'codex') return permOpts;
    return permOpts.map((o) => (o.value === 'ask' || o.value === 'auto') ? { ...o, disabled: true, sub: 'Codex 不支持逐命令确认' } : o);
  }
  // 选中的档位若在 Codex 下不可用，落到「全自动」（保持已选值始终合法，不显示一个灰着的当前项）。
  // 远程连接不做这个降级：见上，那里 codex 的询问/自动是真能用的（降级会把真机推进无人确认的全自动）。
  $effect(() => { if (lBrain === 'codex' && !isSshConn && (lPerm === 'ask' || lPerm === 'auto')) lPerm = 'full'; });
  // 待批工具调用：询问/自动档下钩子把请求写在后端 pending 目录，UI 轮询渲染批准卡。
  type Permit = { id: string; conv: string; tool: string; cmd: string; desc: string; arg: string; dangerous: boolean; mode: string; ts: number };
  // 批准卡标题：优先用模型自己写的操作说明（Bash 的 description），没有就按工具合成一句人话。
  function permitDesc(p: Permit): string {
    if (p.desc) return p.desc;
    if (p.tool === 'WebFetch') return p.arg ? `抓取网页：${p.arg}` : '抓取一个网页';
    if (p.tool === 'WebSearch') return p.arg ? `联网搜索：${p.arg}` : '联网搜索';
    if (p.tool === 'Write' || p.tool === 'Edit' || p.tool === 'NotebookEdit') return p.arg ? `写入文件：${p.arg}` : '写入文件';
    if (p.tool === 'SSH') return p.arg ? `在 ${p.arg} 上执行命令` : '在远程服务器上执行命令';
    if (p.tool === 'Bash') return '运行一条命令';
    return `使用工具 ${p.tool}`;
  }
  // 命令一律默认收起（说明才是给人看的重点），点「查看具体命令」再展开。
  let permitCmdOpen = $state(new Set<string>());
  function togglePermitCmd(id: string) { const s = new Set(permitCmdOpen); if (!s.delete(id)) s.add(id); permitCmdOpen = s; }
  // 已应答过的请求 id：应答后钩子最多要 300ms 才删掉 .req.json，而轮询每 600ms 重扫目录，
  // 扫描落进窗口会让刚点掉的卡"复活"一次（看起来像连续弹两个）。id 不复用，永久过滤即可。
  const respondedPermits = new Set<string>();
  let pendingPermits = $state<Permit[]>([]);
  const activePermits = $derived(pendingPermits.filter((p) => p.conv === activeConvId));
  const permConvs = $derived(new Set(pendingPermits.map((p) => p.conv))); // 哪些对话有待批（含后台的，侧栏标出来）
  $effect(() => { if (activePermits.length) scrollSoon(true); }); // 待批卡一出现就滚到它，不用手动翻
  async function respondPermit(id: string, allow: boolean) {
    respondedPermits.add(id);
    pendingPermits = pendingPermits.filter((p) => p.id !== id); // 乐观移除
    try { await invoke('respond_permit', { id, allow }); } catch (e) { say(String(e), 'err'); }
  }
  $effect(() => {
    const t = setInterval(async () => {
      if (!Object.keys(running).length) { if (pendingPermits.length) pendingPermits = []; return; }
      try { pendingPermits = (await invoke<Permit[]>('list_pending_permits')).filter((p) => !respondedPermits.has(p.id)); } catch { /* */ }
    }, 600);
    return () => clearInterval(t);
  });
  let lDraft = $state('');
  // 全局自定义模型 ID（按厂商，可多个，在「连接与模型」里增删）；作为该厂商模型下拉的附加档位。
  let customDraft = $state<Record<string, string>>({ claude: '', codex: '', grok: '' });
  let customOpen = $state<Record<string, boolean>>({ claude: false, codex: false, grok: false });
  /** 账号行（邮箱+重新绑定）默认收起：多数时候不需要看，展开才见。 */
  let acctOpen = $state<Record<string, boolean>>({ claude: false, codex: false, grok: false });
  function customsOf(b: string): string[] { return b.startsWith('api:') ? [] : (b === 'codex' ? prefs.customCodexIds : b === 'grok' ? prefs.customGrokIds : prefs.customClaudeIds) ?? []; }
  function addCustom(b: string) {
    const v = (customDraft[b] ?? '').trim();
    if (!v) return;
    const arr = customsOf(b);
    if (!arr.includes(v)) { arr.push(v); savePrefs(prefs); }
    customDraft[b] = '';
  }
  function removeCustom(b: string, id: string) {
    const arr = customsOf(b);
    const i = arr.indexOf(id);
    if (i < 0) return;
    arr.splice(i, 1);
    savePrefs(prefs);
    if (lBrain === b && lModel === id) lModel = defaultModelFor(b);
  }
  // 切换引擎后，若当前档位不属于该引擎（launcher 含自定义），重置为该引擎默认档位（避免下拉空选/发错模型）。
  $effect(() => { if (!isLauncherModel(lBrain, lModel)) lModel = defaultModelFor(lBrain); });
  // 首次识别出本地 CLI 后：若默认厂商不可用（只装/登录了另一个），把 composer 默认厂商切到可用的那个；之后尊重手动选择。
  let brainAutoSet = false;
  $effect(() => {
    if (!brains || !apiProfilesLoaded || brainAutoSet) return;
    brainAutoSet = true;
    if (!lBrain.startsWith('api:') && !brainUsable(lBrain)) lBrain = brains && ALL_BRAINS.some((b) => brainUsable(b)) ? firstUsableBrain() : lBrain;
  });
  // 自定义模型输入框的占位示例，按当前引擎给不同提示。
  // ---------- composer 自动撑高 ----------
  // 有内容随行数长高，到上限后固定内部滚动；清空/发送后回到初始高度（对标 Claude Code）。
  function autogrow(node: HTMLTextAreaElement, max = 220) {
    const min = node.offsetHeight; // 以挂载时的初始高度为下限（启动器 3 行、会话 1 行各自保形）
    const resize = () => {
      node.style.height = 'auto';
      const h = Math.max(min, Math.min(node.scrollHeight, max));
      node.style.height = h + 'px';
      node.style.overflowY = node.scrollHeight > max ? 'auto' : 'hidden';
    };
    node.addEventListener('input', resize);
    requestAnimationFrame(resize);
    (node as unknown as { __autogrow?: () => void }).__autogrow = resize;
    return { destroy: () => node.removeEventListener('input', resize) };
  }
  // 单行省略文案只有真的被截断时才挂全局 tooltip；宽度够时保持安静。
  function overflowTip(node: HTMLElement, text: string) {
    let current = text;
    let frame = 0;
    const sync = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const clipped = node.scrollWidth > node.clientWidth + 1 || node.scrollHeight > node.clientHeight + 1;
        if (clipped && current) node.dataset.tip = current;
        else delete node.dataset.tip;
      });
    };
    const observer = new ResizeObserver(sync);
    observer.observe(node);
    sync();
    return {
      update(value: string) { current = value; sync(); },
      destroy() { cancelAnimationFrame(frame); observer.disconnect(); delete node.dataset.tip; },
    };
  }
  let draftEl = $state<HTMLTextAreaElement | undefined>();
  let lDraftEl = $state<HTMLTextAreaElement | undefined>();
  // 程序化清空（发送/排队/切会话）不触发 input 事件，这里补一脚
  $effect(() => { void draft; const el = draftEl as unknown as { __autogrow?: () => void } | undefined; el?.__autogrow?.(); });
  $effect(() => { void lDraft; const el = lDraftEl as unknown as { __autogrow?: () => void } | undefined; el?.__autogrow?.(); });

  // ---------- composer / live turn ----------
  let draft = $state('');
  function legacyComposer(){return {text:draft,files:attachments,queued,cfReady};}
  function setLegacyDraft(text:string){draft=text;}
  // A conversation always occupies a leaf; splitting changes only its geometry.
  $effect(() => {
    const c=activeConv, currentView=view;
    if(!c || currentView!=='thread')return;
    untrack(() => {
      if(!conversationLayout || !leaves(conversationLayout).some(p=>p.conversation===c.id))ensureConversationPane();
      else splitSnapshots[c.id]=c;
    });
  });
  // Existing actions which prefill the active composer (deploy/readiness/etc.) use draft.
  $effect(() => {
    const text=draft, id=activeConvId;
    untrack(() => {if(splitVisible && splitDrafts[id])splitDrafts[id].text=text;});
  });
  type ExecutionActivity = {
    id: string;
    label: string;
    detail: string;
    error: string;
    status: 'running' | 'completed' | 'failed' | 'canceled';
    kind?: string;
    phase?: string;
    current?: number;
    total?: number;
    startedAt: number;
    updatedAt: number;
  };
  type LiveTurn = {
    text: string;
    /** 只在正文 delta 到达时更新；任务活动变化不能重建正文 DOM。 */
    renderedText: string;
    tools: ToolCall[];
    activities: Record<string, ExecutionActivity>;
    lastActivityAt: number;
    error: string;
    failed: boolean;
    startedAt: number;
    compactedAt?: number;
    compactedPreTokens?: number;
  };
  // 并发对话：running = convId → connId（在跑的对话；带 connId 便于删连接时可靠判定）；lives = 每个对话的流式缓冲。
  let running = $state<Record<string, string>>({});
  let lives = $state<Record<string, LiveTurn>>({});
  let autoRetried = $state<Record<string, boolean>>({}); // convId → 本轮已自动重试过（每个用户轮只自动重试一次）
  let retryTimers: Record<string, ReturnType<typeof setTimeout>> = {}; // 待触发的自动重连定时器（新一轮开始时取消，防陈旧触发）
  // 智能升级：手动重试后若以同样的错误再次失败，说明 resume 救不了 → 之后只显示「重建继续」。
  let retryExhausted = $state<Record<string, boolean>>({});
  let threadEl = $state<HTMLDivElement | null>(null);
  let threadInnerEl = $state<HTMLDivElement | null>(null);
  // 是否应随新消息贴住底部。用户主动向上翻后会关闭，回到底部后自动恢复。
  let threadStickBottom = true;
  let threadScrollQueued = false;
  let threadScrollForce = false;
  let threadSecondRaf = 0;

  const viewBusy = $derived(!!running[activeConvId]); // 当前查看的对话是否在跑
  const liveView = $derived(lives[activeConvId]); // 当前对话的流式缓冲（可能 undefined）

  // 加载中的"耗时"计时：仅在当前对话跑着时开一个轻量心跳刷新 nowMs，停了自动清掉。
  let nowMs = $state(0);
  $effect(() => {
    if (!viewBusy && !(splitVisible && Object.keys(running).length)) return;
    nowMs = Date.now();
    const t = setInterval(() => { nowMs = Date.now(); }, 500);
    return () => clearInterval(t);
  });
  function elapsedLabel(ms: number): string {
    const s = Math.max(0, Math.floor(ms / 1000));
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${String(s % 60).padStart(2, '0')}s`;
  }
  function activityLabel(activity: ExecutionActivity): string {
    const phase = activity.phase?.trim() ?? '';
    if (phase) {
      // 原生活动在开始与结束时复用同一 phase。按真实状态改写，否则已经完成的
      // 上传仍会显示“正在上传”，旧客户端留下的“正在执行命令”也会继续暴露技术概念。
      if (phase === '正在执行命令' || phase === '正在处理任务步骤') {
        return activity.status === 'failed' ? '任务步骤未成功'
          : activity.status === 'completed' ? '任务步骤已完成'
          : '正在处理任务步骤';
      }
      if (phase === '正在检查图片文件') {
        return activity.status === 'failed' ? '图片检查未成功'
          : activity.status === 'completed' ? '图片检查完成'
          : phase;
      }
      return phase;
    }
    if (activity.kind === 'image_generation') return activity.status === 'completed' ? '图片已生成' : '正在生成图片';
    if (activity.kind === 'image_review') return activity.status === 'completed' ? '图片检查完成' : '正在检查图片';
    if (activity.kind === 'reasoning') return '正在分析与规划';
    if (activity.kind === 'skill') return '正在使用技能';
    const label = activity.label.trim();
    const key = label.toLowerCase();
    if (key.includes('get_command_or_subagent_output')) return '等待后台任务返回';
    if (key.includes('kill_command_or_subagent')) return '正在停止无响应的后台任务';
    if (key === 'todo_write' || key.includes('update_plan')) return '正在更新执行计划';
    if (key.includes('subagent') || key.includes('spawn_agent')) return '后台代理正在处理';
    if (activity.kind === 'command' || key === 'exec' || key === 'bash' || key.includes('terminal')) {
      const detail = activity.detail.trim().toLowerCase();
      const completed = activity.status === 'completed';
      const failed = activity.status === 'failed';
      if (/\b(npm|cargo|svelte-check|pytest|go test)\b/.test(detail)) {
        return failed ? '项目检查未通过' : completed ? '项目检查完成' : '正在检查项目';
      }
      return failed ? '任务步骤未成功' : completed ? '任务步骤已完成' : '正在执行任务步骤';
    }
    if (key === 'read') return '正在读取资料';
    if (key === 'edit') return '正在修改文件';
    if (key === 'search' || key === 'grep') return '正在查找信息';
    if (key === 'fetch') return '正在获取网页';
    return activity.detail.trim() || label || '正在执行下一步';
  }
  function executionProgress(live: LiveTurn, now: number) {
    const activities = Object.values(live.activities).sort((a, b) => a.startedAt - b.startedAt);
    const runningActivities = activities.filter((activity) => activity.status === 'running');
    const failedActivities = activities.filter((activity) => activity.status === 'failed');
    const latest = runningActivities.at(-1) ?? activities.at(-1);
    const finished = activities.filter((activity) => activity.status === 'completed').length;
    const canceled = activities.filter((activity) => activity.status === 'canceled').length;
    const quietFor = Math.max(0, now - live.lastActivityAt);
    const quiet = activities.length > 0 && quietFor >= 45_000;
    // CLI 子进程启动后，到首个 JSON 状态事件返回前没有更细的真实阶段。
    // 与 Codex 原生界面一致立即显示“正在思考”，不把这段空窗误称为恢复历史会话。
    const waitingForFirstActivity = activities.length === 0 && !live.text;
    let title = latest ? activityLabel(latest) : '正在思考下一步';
    if (waitingForFirstActivity) title = '正在思考';
    else if (quiet) title = '后台任务仍在运行';
    else if (!runningActivities.length && activities.length) title = live.text ? '正在整理并继续处理' : '正在等待下一步';
    const parts: string[] = [];
    const generated = activities.filter((activity) => activity.kind === 'image_generation' && activity.status === 'completed').length;
    const skills = activities.filter((activity) => activity.kind === 'skill' && activity.status === 'completed').length;
    // 这是工具真实返回的“调用完成次数”，可能包含重做，不能冒充业务目标张数。
    if (generated) parts.push(`图片生成完成 ${generated} 次`);
    if (skills) parts.push(`已完成 ${skills} 次技能调用`);
    if (!generated && !skills && finished) parts.push(`${finished} 个步骤已完成`);
    if (canceled) parts.push(`${canceled} 项已停止`);
    if (waitingForFirstActivity && quietFor >= 60_000) parts.push('Codex 暂未返回新进展');
    if (quiet) parts.push(`最近活动在 ${elapsedLabel(quietFor)} 前`);
    parts.push(`已运行 ${elapsedLabel(now - live.startedAt)}`);
    const trueProgress = [...activities]
      .reverse()
      .find((activity) => activity.total != null && activity.total > 0 && activity.current != null && activity.current >= 0 && activity.current <= activity.total);
    const latestFailureAt = failedActivities.reduce((latestAt, activity) => Math.max(latestAt, activity.updatedAt), 0);
    const continuedAfterFailure = latestFailureAt > 0
      && activities.some((activity) => activity.status === 'completed' && activity.updatedAt > latestFailureAt);
    return {
      hasActivity: activities.length > 0 || waitingForFirstActivity,
      thinking: waitingForFirstActivity,
      title,
      meta: parts.join(' · '),
      quiet,
      failed: failedActivities.length > 0,
      continuedAfterFailure,
      failedActivities,
      activities: [...activities].reverse().slice(0, 10),
      trueProgress,
    };
  }
  function activityFailureReason(activity: ExecutionActivity): string {
    return activity.error.trim() || '执行端未返回具体错误原因';
  }
  function activityStatusLabel(activity: ExecutionActivity): string {
    if (activity.status === 'completed') return '已完成';
    if (activity.status === 'failed') return '未成功';
    if (activity.status === 'canceled') return '已停止';
    return '进行中';
  }
  function activityDetail(activity: ExecutionActivity): string {
    // 原始 shell 命令、绝对路径和参数不适合直接暴露给普通用户。完整命令已经保留在
    // 上方可折叠的“执行记录”中；任务卡只呈现语义状态和真实终态。
    if (activity.kind === 'command') return '';
    return activity.detail.trim();
  }
  function compactToolRows(tools: ToolCall[]): Array<ToolCall & { count: number }> {
    const rows: Array<ToolCall & { count: number }> = [];
    const indexes = new Map<string, number>();
    for (const tool of tools) {
      const key = `${tool.label}\u0000${tool.detail}`;
      const index = indexes.get(key);
      if (index == null) {
        indexes.set(key, rows.length);
        rows.push({ ...tool, count: 1 });
      } else {
        rows[index].count += 1;
      }
    }
    return rows;
  }
  // 会话大小/用量：上下文按「厂商 + 模型」估算——Codex 暂以缓存默认窗口 272k 估算，
  // 并非模型容量或当前会话确认值（Astra 缓存另有 max_context_window=872000）；Grok 4.5 / 4.6 500k
  //（ACP initialize 的 totalContextTokens 实测值）。Claude 当前 Sonnet / Opus / Fable 是 1M；
  // Haiku 4.5 仍是 200k。旧版或无法识别的自定义 Claude ID 保守按 200k。
  //
  // ★ 加新模型时**必须同时改这里**，否则它会掉进最后那个 200k 兜底 —— 用量百分比、
  //   「快满了」提示、自动压缩时机全部按错误的窗口算，而且不报错、只是数字悄悄不对。
  //   `-5` 这一支（sonnet-5 / opus-5 / fable-5）没有第三段版本号，别套 `-4-[6-9]` 那种写法。
  function ctxLimit(brain: string, model = ''): number {
    if (brain.startsWith('api:')) return 0; // Capability is not inferable from a custom model ID.
    if (brain === 'codex') return 272000;
    if (brain === 'grok') return 500000;
    const id = (model.trim() || 'sonnet').toLowerCase();
    if (id === 'sonnet' || id === 'opus' || id.includes('fable') || id.includes('mythos')) return 1_000_000;
    if (/claude-(?:sonnet-5|opus-5|sonnet-4-[6-9]|opus-4-[6-9])(?:$|-)/.test(id)) return 1_000_000;
    return 200_000;
  }
  // 实测自适应升档（证据驱动，只升不降）：实测 ctx 已超过假定上限的 95%，说明真实窗口更大
  //（如 Max 订阅跑 Sonnet 实际 1M），按下一档 1M 计；展示百分比由调用方钳 100%。
  function ctxLimitAdaptive(brain: string, model: string, ctx: number): number {
    const base = ctxLimit(brain, model);
    // Codex/Grok 的占用接近默认值，不足以证明窗口已扩展到 1M。
    if (brain !== 'claude' || model.toLowerCase().includes('haiku')) return base;
    return base < 1_000_000 && ctx > base * 0.95 ? 1_000_000 : base;
  }
  // 拖动窗口：忽略交互元素（按钮/输入等），否则点它们会误触发拖动。
  /** 页头「抬升」：内容真滑到它底下时才淡入阴影、把那条线淡掉（见 .thread-head 的注释）。
   *
   *  ★ 一处监听管全部视图：scroll 事件**不冒泡、但会捕获**，所以在 document 上抓一次就能收到
   *  任意视图里 .thread 的滚动 —— 不必逐视图挂钩子（7 个页头 / 6 个 .thread 分散在同一条
   *  {#if} 链里，同一时刻只挂一个）。判定靠**结构约定**（.thread 的前一个兄弟是 .thread-head），
   *  所以新增视图照现有写法写就自动继承，不用记得注册。
   *  写歪了（比如把 .thread 包了一层）→ 取不到页头 → 直接 no-op，退回今天的样子：
   *  是「没变好」，不是「坏了」—— 这个退化方向是选它而不选纯 CSS 方案的关键。
   *
   *  迟滞（4 上 / 1 下）不是洁癖：惯性滚动和触控板会在 0~1px 抖，用 >0 判定会让阴影反复
   *  淡入淡出，看着就是闪。 */
  $effect(() => {
    const onScroll = (e: Event) => {
      const el = e.target as HTMLElement | null;
      if (!el?.classList?.contains('thread')) return;
      const head = el.previousElementSibling; // 注释节点不算元素，convPane 那种 {@render} 锚点跳得过去
      if (!head?.classList.contains('thread-head')) return;
      const up = head.classList.contains('elevated');
      head.classList.toggle('elevated', el.scrollTop > (up ? 1 : 4));
    };
    document.addEventListener('scroll', onScroll, true); // capture：scroll 不冒泡
    return () => document.removeEventListener('scroll', onScroll, true);
  });

  function startDrag(e: MouseEvent) {
    if (e.button !== 0) return;
    const t = e.target as HTMLElement;
    if (t.closest('button, a, input, textarea, select, [role="button"], [data-no-drag]')) return;
    getCurrentWindow().startDragging().catch(() => {});
  }

  // Windows 无 macOS 红绿灯（窗口控件在右侧），顶部工具按钮改靠左对齐，别飘在中间。
  const isWindows = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
  const isMac = typeof navigator !== 'undefined' && /Macintosh|Mac OS X/i.test(navigator.userAgent);
  /** 密钥存储的平台名称（文案用）：mac=钥匙串，win=凭据管理器。 */
  const keystoreName = isWindows ? 'Windows 凭据管理器' : 'macOS 钥匙串';
  // 全屏时无红绿灯，顶部工具按钮改与左栏菜单左对齐。
  let isFullscreen = $state(false);
  $effect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    const sync = async () => { try { isFullscreen = await win.isFullscreen(); } catch { /* */ } };
    sync();
    win.onResized(sync).then((u) => (unlisten = u)).catch(() => {});
    return () => unlisten?.();
  });

  // 侧栏折叠 + 可拖拽宽度（持久化）。
  let railCollapsed = $state(loadRailFlag());
  /** 会话列表已滚动（不在顶部）——导航与列表之间显示分界线。 */
  let convosScrolled = $state(false);
  let railWidth = $state(loadRailWidth());
  function loadRailFlag(): boolean { try { return localStorage.getItem('cligc.pilot.railCollapsed') === '1'; } catch { return false; } }
  function loadRailWidth(): number { try { const n = parseInt(localStorage.getItem('cligc.pilot.railWidth') || ''); return n >= 190 && n <= 460 ? n : 240; } catch { return 240; } }
  function toggleRail() { railCollapsed = !railCollapsed; try { localStorage.setItem('cligc.pilot.railCollapsed', railCollapsed ? '1' : '0'); } catch { /* */ } }
  function startResize(e: MouseEvent) {
    e.preventDefault();
    const onMove = (ev: MouseEvent) => { railWidth = Math.min(460, Math.max(190, Math.round(ev.clientX))); };
    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.body.style.cursor = '';
      try { localStorage.setItem('cligc.pilot.railWidth', String(railWidth)); } catch { /* */ }
    };
    document.body.style.cursor = 'col-resize';
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }

  // 全局搜索（分区）：视图导航 / 连接 / 模板 / 会话。
  let searchOpen = $state(false);
  let searchQ = $state('');
  let searchInput = $state<HTMLInputElement | null>(null);
  function openSearch() { searchOpen = true; searchQ = ''; searchIdx = 0; void ensureTemplateMeta(); requestAnimationFrame(() => searchInput?.focus()); }
  // 会话匹配：按标题 / 项目名 / 工作目录。
  const searchResults = $derived.by(() => {
    const q = searchQ.trim().toLowerCase();
    const list = convos.filter((c) => {
      // 每条会话固定在创建它的连接里；本地对话同样只在本地工作区里搜。
      if (c.conn_id !== activeConnId) return false;
      if (!q) return true;
      return (c.title || '').toLowerCase().includes(q)
        || (c.site_name || '').toLowerCase().includes(q)
        || (c.site_slug || '').toLowerCase().includes(q)
        || (c.workspace_dir || '').toLowerCase().includes(q);
    });
    return list.slice(0, 60);
  });
  type SearchEntry =
    | { kind: 'view'; view: 'templates' | 'prompts'; label: string }
    | { kind: 'conv'; c: Conversation }
    | { kind: 'template'; t: Template }
    | { kind: 'conn'; c: Connection };
  const SEARCH_VIEWS: { view: 'templates' | 'prompts'; label: string }[] = [
    { view: 'templates', label: '模板库' },
    { view: 'prompts', label: '提示词' },
  ];
  /** 分区结果（按匹配数动态显隐）：空查询只给视图导航项。 */
  const searchSections = $derived.by(() => {
    const q = searchQ.trim().toLowerCase();
    const out: { title: string; entries: SearchEntry[] }[] = [];
    // 模板库 / 提示词只有 Cloudflare 连接有；别的连接下选中它只会被弹回启动页。
    const views = (activeConn?.kind === 'cloudflare' ? SEARCH_VIEWS : [])
      .filter((v) => !q || v.label.includes(q))
      .map((v) => ({ kind: 'view' as const, ...v }));
    if (views.length) out.push({ title: '视图', entries: views });
    if (q) {
      // 连接：名称 / IP / user@host / 端口 / 系统。
      // ★ 这一段**不按 activeConnId 过滤** —— 别的分区搜的是「当前连接内部的东西」，
      //   而连接本身是**跨连接**的：机器一多，全局搜索就是最快的切换方式（记不住哪台是哪个 IP）。
      const connHits = conns.filter((c) => connHay(c).includes(q)).slice(0, 8);
      if (connHits.length) out.push({ title: '连接', entries: connHits.map((c) => ({ kind: 'conn' as const, c })) });
      // 模板：名称 / 描述 / slug / 分类（模板库视图里因此不再摆搜索框）
      if (activeConn?.kind === 'cloudflare') {
        const tplHits = templates.filter((t) => tmplHay(t).includes(q)).slice(0, 8);
        if (tplHits.length) out.push({ title: '模板', entries: tplHits.map((t) => ({ kind: 'template' as const, t })) });
      }
      if (searchResults.length) out.push({ title: '会话', entries: searchResults.map((c) => ({ kind: 'conv' as const, c })) });
    }
    return out;
  });
  const searchFlat = $derived(searchSections.flatMap((s) => s.entries));
  let searchIdx = $state(0);
  $effect(() => { void searchQ; searchIdx = 0; }); // 换关键词回到第一项
  function searchNudge(delta: number) {
    if (!searchFlat.length) return;
    searchIdx = Math.max(0, Math.min(searchFlat.length - 1, searchIdx + delta));
    requestAnimationFrame(() => document.querySelector('.search-item.on')?.scrollIntoView({ block: 'nearest' }));
  }
  function pickEntry(it: SearchEntry) {
    searchOpen = false;
    if (it.kind === 'view') {
      if (it.view === 'templates') openTemplates();
      else openPrompts();
    } else if (it.kind === 'template') {
      // 分类筛选可能正好把它挡在外面——搜到了、跳过去却看不见，比搜不到还让人懵
      tmplCat = '全部';
      openTemplates();
      void focusTemplate(it.t.slug);
    } else if (it.kind === 'conn') {
      // 走和切换器同一条路：连接切换牵着视图守卫、SSH 探系统、终端重建，别在这儿另起一套
      void selectConn(it.c.id);
    } else {
      openConv(it.c.id);
    }
  }
  $effect(() => {
    if (!searchOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') searchOpen = false;
      else if (e.key === 'ArrowDown') { e.preventDefault(); searchNudge(1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); searchNudge(-1); }
      else if (e.key === 'Enter') { const it = searchFlat[searchIdx]; if (it) pickEntry(it); }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  });
  function say(m: string, k: 'ok' | 'err' = 'ok', ms = 0) { flash = m; flashKind = k; setTimeout(() => (flash = ''), ms || (k === 'err' ? 8000 : 4000)); }
  function brainUsable(b: Brain): boolean {
    if (b.startsWith('api:')) { const p = apiProfile(b); return !!p?.enabled && p.key_configured && p.models.length > 0; }
    const s = b === 'claude' ? brains?.claude : b === 'grok' ? brains?.grok : brains?.codex;
    return !!s && s.found && s.logged_in !== false;
  }
  function brainAvailability(b: string): string {
    if (b.startsWith('api:')) {
      const p = apiProfile(b);
      return !apiProfilesLoaded ? '正在读取 API 连接' : !p ? 'API 连接已移除' : !p.enabled ? 'API 连接已停用' : !p.key_configured ? '未配置 API Key' : !p.models.length ? '尚未添加 API 模型' : `API · ${p.name}`;
    }
    const status = b === 'claude' ? brains?.claude : b === 'grok' ? brains?.grok : brains?.codex;
    return !status?.found ? '未安装' : status.logged_in === false ? '未登录' : '本地客户端';
  }
  function hostOf(u: string): string { try { return new URL(u).host; } catch { return u; } }
  function brainSetupHint(b: Brain): string {
    return b.startsWith('api:') ? `${brainAvailability(b)}，请在设置的「API 服务」中检查配置。` : '所选厂商未就绪，点左下角设置里「去授权」。';
  }

  // 从对话里认出这个项目已部署的线上地址（AI 部署完都会说「访问 https://xxx」）。
  // 优先自定义域名，其次 *.pages.dev；忽略 localhost / CDN / 文档等无关链接。
  function detectSiteUrl(convs: (Conversation | null)[]): string {
    const re = /https?:\/\/[a-z0-9.-]+\.[a-z]{2,}[^\s`"'）)，。；]*/gi;
    let custom = '', pages = '';
    for (const c of convs) {
      if (!c) continue;
      for (const m of c.messages) {
        if (m.role !== 'assistant' || m.hidden) continue;
        const found = m.text.match(re);
        if (!found) continue;
        for (let u of found) {
          u = u.replace(/[.,;)]+$/, '');
          const host = u.replace(/^https?:\/\//i, '').split('/')[0].toLowerCase();
          if (host.includes('localhost') || host.startsWith('127.') || host.includes('cloudflare.com')
            || host.includes('developers.') || host.includes('tailwindcss.com') || host.includes('github')
            || host.includes('npmjs') || host.includes('unpkg') || host.includes('jsdelivr')) continue;
          if (host.endsWith('.pages.dev')) pages = u; else custom = u;
        }
      }
    }
    return custom || pages;
  }
  // 当前会话站点的公开地址：CF 从对话里探测部署地址；其它会话没有站点。
  const activeSiteUrl = $derived.by(() => {
    const c = activeConv;
    if (!c || !activeConvIsCf) return '';
    return detectSiteUrl([c]);
  });
  // 站点公开地址猜标准 favicon 位置（SiteFav 加载失败再退首字母）。
  function faviconGuess(url?: string): string { const u = (url ?? '').trim(); return u ? u.replace(/\/+$/, '') + '/favicon.ico' : ''; }

  // 「新窗口打开」开出来的窗口，label 是 `conn-<id>`：这一份前端就固定开在那个连接上。
  // （连接/对话/SSH 会话都在 Rust 侧同一份 AppState 里，多个窗口看到的是同一份数据。）
  // 用 label 而不是 URL 参数：带 query 的 WebviewUrl::App 在 dev 下会指到 /index.html，
  // 而 SvelteKit 只认 /，开出来是白屏。详见 lib.rs::open_conn_window。
  const CONN_WIN_PREFIX = 'conn-';
  const LAST_CONN_KEY = 'cligc.pilot.lastConn';
  function bootConnId(): string {
    if (pilotWindowLabel.startsWith(CONN_WIN_PREFIX)) return pilotWindowLabel.slice(CONN_WIN_PREFIX.length);
    try { return localStorage.getItem(LAST_CONN_KEY) || ''; } catch { return ''; }
  }
  async function refreshConns() {
    try {
      conns = await invoke<Connection[]>('list_connections');
      if (activeConnId && activeConnId !== WORKSPACE_CONN_ID && !conns.some((c) => c.id === activeConnId)) {
        // 当前连接被删掉了：退回本地对话，不留一个指向空连接的界面。
        void selectConn(WORKSPACE_CONN_ID);
      } else if (!activeConnId) {
        const want = bootConnId();
        void selectConn(want && conns.some((c) => c.id === want) ? want : WORKSPACE_CONN_ID);
      }
    } catch (e) {
      say(String(e), 'err');
    }
  }
  async function refreshBrains() { try { brains = await invoke('detect_brains'); } catch (e) { say(String(e), 'err'); } }
  let copiedCmd = $state('');
  async function copyCmd(cmd: string) {
    try { await navigator.clipboard.writeText(cmd); copiedCmd = cmd; setTimeout(() => { if (copiedCmd === cmd) copiedCmd = ''; }, 1500); }
    catch { say('复制失败，请手动选中命令复制', 'err'); }
  }
  let brainsBusy = $state(false);
  // 手动重新检测：走 redetect_brains（后端先重读登录 shell 的 PATH，再探测）——治「装/登录完仍显示未安装/未登录」。
  async function refreshBrainsManual() { brainsBusy = true; try { brains = await invoke('redetect_brains'); } catch (e) { say(String(e), 'err'); } finally { brainsBusy = false; } void verifyClaude(true); }
  const anyBrainUsable = $derived(ALL_BRAINS.some((b) => brainUsable(b)));

  // 应用版本 + 在线检查更新（Tauri updater：拉 release 仓 latest.json，ed25519 验签后下载安装再重启）。
  type PilotUpdateStatus = 'idle' | 'checking' | 'available' | 'current' | 'unreachable' | 'updating' | 'waiting' | 'done' | 'failed';
  const isMainWindow = pilotWindowLabel === 'main';
  const PILOT_UPDATE_RECEIPT_KEY = 'cligc.pilot.updateReceipt';
  let appVersion = $state('');
  let updBusy = $state(false);
  let updMsg = $state('');
  let updPct = $state(-1); // 下载进度 0-100；-1 = 不确定（无 content-length）
  let updAvail = $state(''); // 非空 = 静默检查到的新版本号，驱动工具栏「待更新」图标
  let pilotUpdateStatus = $state<PilotUpdateStatus>('idle');
  let pilotUpdateError = $state('');
  let pilotReleaseNotes = $state('');
  let pilotPublishedAt = $state('');
  let pilotUpdateReceipt = $state('');
  let updateCenterOpen = $state(false);
  let updateCheckedAt = $state(0);
  const topUpdateVisible = $derived(isMainWindow && (!!updAvail || updateCenterOpen || updBusy));
  function sameUpdateVersion(a: string, b: string): boolean {
    const norm = (v: string) => v.trim().replace(/^v/i, '');
    return norm(a) === norm(b);
  }
  getVersion().then((v) => {
    appVersion = v;
    try {
      const receipt = JSON.parse(localStorage.getItem(PILOT_UPDATE_RECEIPT_KEY) || 'null') as { target?: string } | null;
      if (receipt?.target && sameUpdateVersion(v, receipt.target)) {
        pilotUpdateReceipt = `Pilot 已更新到 ${v}`;
        localStorage.removeItem(PILOT_UPDATE_RECEIPT_KEY);
      }
    } catch { /* 忽略损坏的回执 */ }
  }).catch(() => { /* */ });

  // 只检查并释放 updater 资源句柄；主窗口统一调度，连接子窗口不重复请求更新源。
  async function checkPilotUpdate(silent = true) {
    if (!isMainWindow || updBusy || pilotUpdateStatus === 'checking') return;
    pilotUpdateStatus = 'checking';
    pilotUpdateError = '';
    updAvail = '';
    try {
      const upd = await checkUpdate();
      updateCheckedAt = Date.now();
      if (!upd) {
        pilotReleaseNotes = '';
        pilotPublishedAt = '';
        pilotUpdateStatus = 'current';
        return;
      }
      updAvail = upd.version;
      pilotReleaseNotes = upd.body || '';
      pilotPublishedAt = upd.date || '';
      pilotUpdateStatus = 'available';
      try { await upd.close(); } catch { /* */ }
    } catch (e) {
      updateCheckedAt = Date.now();
      pilotUpdateError = String(e);
      pilotUpdateStatus = 'unreachable';
      if (!silent) say(`检查 Pilot 更新失败：${pilotUpdateError}`, 'err');
    }
  }

  async function installPilotUpdate() {
    if (!isMainWindow) { say('请在 Pilot 主窗口中更新客户端', 'err'); return; }
    if (updBusy) return;
    updBusy = true; updMsg = '检查中…'; updPct = -1;
    pilotUpdateStatus = 'checking'; pilotUpdateError = '';
    let installed = false;
    let upd: Awaited<ReturnType<typeof checkUpdate>> = null;
    try {
      upd = await checkUpdate();
      updateCheckedAt = Date.now();
      if (!upd) {
        updAvail = ''; updMsg = '已是最新版本'; pilotUpdateStatus = 'current';
        return;
      }
      updAvail = upd.version;
      pilotReleaseNotes = upd.body || '';
      pilotPublishedAt = upd.date || '';
      pilotUpdateStatus = 'available';
      const ok = await confirmDialog(`发现新版本 ${upd.version}，现在下载更新并重启？`, { title: '有可用更新', kind: 'info' });
      if (!ok) {
        updMsg = '';
        try { await upd.close(); } catch { /* */ }
        return;
      }
      let total = 0, got = 0;
      updMsg = '准备下载…'; updPct = 0;
      pilotUpdateStatus = 'updating';
      await upd.downloadAndInstall((ev) => {
        if (ev.event === 'Started') { total = ev.data.contentLength ?? 0; got = 0; updPct = 0; updMsg = '下载更新…'; }
        else if (ev.event === 'Progress') { got += ev.data.chunkLength; updPct = total > 0 ? Math.min(100, Math.round((got / total) * 100)) : -1; updMsg = total > 0 ? `下载更新… ${updPct}%` : '下载更新…'; }
        else if (ev.event === 'Finished') { updPct = 100; updMsg = '安装中…'; }
      });
      installed = true;
      try {
        localStorage.setItem(PILOT_UPDATE_RECEIPT_KEY, JSON.stringify({ target: upd.version, installedAt: Date.now() }));
      } catch { /* */ }
      updMsg = '即将重启…'; updPct = 100;
      pilotUpdateStatus = 'done';
      await relaunch();
    } catch (e) {
      const detail = String(e);
      updPct = -1;
      if (!upd) updAvail = '';
      if (installed) {
        updMsg = `已安装，等待手动重启：${detail}`;
        pilotUpdateStatus = 'waiting';
      } else {
        updMsg = `更新失败：${detail}`;
        pilotUpdateStatus = 'failed';
      }
      pilotUpdateError = detail;
      say(updMsg, 'err');
      try { await upd?.close(); } catch { /* */ }
    }
    finally { updBusy = false; }
  }
  async function runUpdate() { await installPilotUpdate(); }
  function pilotUpdateLabel(status: PilotUpdateStatus): string {
    switch (status) {
      case 'checking': return '检查中';
      case 'available': return '可更新';
      case 'current': return '已是最新';
      case 'unreachable': return '无法检查';
      case 'updating': return '更新中';
      case 'waiting': return '等待重启';
      case 'done': return '已完成';
      case 'failed': return '失败';
      default: return '尚未检查';
    }
  }
  function pilotUpdateMessage(): string {
    if (updBusy) return updMsg || '正在更新';
    if (pilotUpdateStatus === 'unreachable' || pilotUpdateStatus === 'failed') return pilotUpdateError || updMsg || '无法完成更新检查';
    if (updAvail) return `${appVersion || '当前版本'} → ${updAvail}`;
    if (pilotUpdateStatus === 'current') return appVersion ? `当前 v${appVersion}，已是最新版` : '当前已是最新版';
    return pilotUpdateStatus === 'checking' ? '正在检查客户端更新…' : '尚未检查客户端更新';
  }
  function openUpdateCenter() {
    if (!isMainWindow) { say('请在 Pilot 主窗口中打开更新中心', 'err'); return; }
    setupOpen = false;
    updateCenterOpen = true;
    if (!updateCheckedAt || Date.now() - updateCheckedAt > 10 * 60 * 1000) void checkPilotUpdate(false);
  }
  function closeUpdateCenter() {
    if (updBusy) return;
    updateCenterOpen = false;
  }

  // 启动 5 秒后检查一次，之后每 6 小时静默检查。
  $effect(() => {
    if (!isMainWindow) return;
    const initial = setTimeout(() => { void checkPilotUpdate(true); }, 5000);
    const periodic = setInterval(() => { void checkPilotUpdate(true); }, 6 * 60 * 60 * 1000);
    return () => { clearTimeout(initial); clearInterval(periodic); };
  });
  async function refreshConvos() { try { convos = await invoke('list_conversations'); } catch (e) { say(String(e), 'err'); } }

  let selSeq = 0;
  async function selectConn(id: string) {
    const switching = id !== activeConnId;
    const conn = conns.find((c) => c.id === id);
    switchConversationWorkspace(id);
    const seq = ++selSeq; activeConnId = id;
    if (isMainWindow) { try { localStorage.setItem(LAST_CONN_KEY, id); } catch { /* */ } }
    if (switching) {
      // 换连接＝换工作区：关掉上个连接的对话视图，别串场。模板库 / 提示词库只在 CF 连接下有。
      activeConvId = ''; activeConv = null;
      if (view === 'thread' || view === 'remote'
        || ((view === 'templates' || view === 'prompts') && conn?.kind !== 'cloudflare')) view = 'launcher';
    }
    if (conn?.kind === 'ssh') {
      // SSH 连接没有项目可发现，也没有独立的启动页/对话页——**一切都在远程工作台里**
      //（终端为主，底部对话、右侧文件按需开）。
      void probeOs(id); // 系统版本：没探过才连，探到就存着，之后一直是本地读
      view = 'remote';
      if (switching) {
        disposeTerm();
        // 远程连接绝不默认「全自动」：那是别人的真机，默认得让每条命令都过一次你的眼。
        if (lPerm === 'full') lPerm = 'auto';
      }
      return;
    }
    if (conn?.kind === 'cloudflare') {
      // 只在真正换连接时清输入 + 拨默认档；同连接重选（如刷新）不动用户已填的项目名。
      if (switching) {
        cfProjects = []; lSite = ''; lStyle = '';
        if (lPerm === 'full') lPerm = 'auto'; // CF 默认「自动」——部署/改 DNS 弹确认，防手滑
      }
      try { const ps = await invoke<string[]>('list_cf_projects', { connId: id }); if (seq === selSeq) cfProjects = ps; }
      catch (e) { if (seq === selSeq) say(String(e), 'err'); }
      return;
    }
    // 本地对话：没有项目可选，进启动页即可。
    if (switching) { lSite = ''; view = 'launcher'; }
  }
  function openLocalWorkspace() {
    switcherOpen = false;
    void selectConn(WORKSPACE_CONN_ID);
  }

  $effect(() => {
    void refreshConns();
    void refreshBrains();
    void refreshApiProfiles();
    void refreshConvos();
  });
  $effect(() => {
    const need = !!brains && ((brains.claude.found && brains.claude.logged_in === false) || (brains.codex.found && brains.codex.logged_in === false) || (brains.grok.found && brains.grok.logged_in === false));
    if (!need) return;
    const t = setInterval(() => { if (!document.hidden) refreshBrains(); }, 6000);
    return () => clearInterval(t);
  });

  // ---------- 新建 / 编辑远程连接（SSH） ----------
  // 安全：密码/私钥口令只在这个表单的内存里过一下，随 connect_ssh/update_ssh 进钥匙串，绝不落盘。
  // 主机指纹走 TOFU：必须先「测试连接」拿到指纹、由你确认后才能保存（后端无指纹一律拒）。
  let sshOpen = $state(false);
  let sshTesting = $state(false);
  let sshConnecting = $state(false);
  let sshErr = $state('');
  let sshFp = $state(''); // 试连拿到的主机指纹；非空 = 认证已通过、待你确认指纹
  let sshEditId = $state(''); // 非空 = 编辑已有连接
  let sshCredentialRepair = $state(false); // 从“凭据缺失”入口进入：必须补填，不能空保存
  let sshOldFp = $state(''); // 编辑时：连接里存着的旧指纹，用来发现「机器的指纹变了」
  const SSH_BLANK = { name: '', host: '', port: 22, user: '', auth: 'password', password: '', keyPath: '', keyPass: '' };
  let sshF = $state({ ...SSH_BLANK });
  let sshF0 = $state({ ...SSH_BLANK }); // 编辑时的初始值：用来判断「有没有动过连接相关的字段」
  const sshAuthOpts = [
    { value: 'password', label: '密码', sub: '用账号密码登录' },
    { value: 'key', label: '密钥', sub: '用私钥文件登录（更安全）' },
  ];
  function openSshConnect() {
    sshOpen = true; sshErr = ''; sshFp = ''; sshEditId = ''; sshCredentialRepair = false; sshOldFp = '';
    sshF = { ...SSH_BLANK };
    sshF0 = { ...SSH_BLANK };
  }
  function openSshEdit(c: Connection) {
    sshOpen = true; sshErr = ''; sshFp = ''; sshEditId = c.id; sshCredentialRepair = false; sshOldFp = c.ssh_fingerprint || '';
    // 密码/口令不回显（我们自己也读不到明文以外的用途）——留空即保持钥匙串里那条不变。
    sshF = {
      name: c.name, host: c.ssh_host ?? '', port: c.ssh_port || 22, user: c.ssh_user ?? '',
      auth: c.ssh_auth || 'password', password: '', keyPath: c.ssh_key_path ?? '', keyPass: '',
    };
    sshF0 = { ...sshF };
  }
  function openSshCredentialRepair(c: Connection) {
    openSshEdit(c);
    sshCredentialRepair = true;
  }
  // 改了连接相关的字段（地址/端口/用户/认证方式/密钥路径/新密码）→ 必须重新试连拿指纹再存。
  // 只改名字则不必：那和「能不能连上」无关，别为了改个名逼用户去连一次机器。
  const sshDirty = $derived(
    sshF.host.trim() !== sshF0.host || Number(sshF.port) !== Number(sshF0.port) ||
    sshF.user.trim() !== sshF0.user || sshF.auth !== sshF0.auth ||
    sshF.keyPath.trim() !== sshF0.keyPath || !!sshF.password || !!sshF.keyPass
  );
  const sshRenameOnly = $derived(!!sshEditId && !sshDirty && !sshCredentialRepair);
  // 试连看到的指纹和存着的不一样 = 这台机器换了主机密钥（重装？也可能是中间人）——必须让人看见。
  const sshFpChanged = $derived(!!sshEditId && !!sshFp && !!sshOldFp && sshFp !== sshOldFp);
  async function saveSshEdit() {
    if (sshConnecting) return;
    if (sshCredentialRepair && sshF.auth === 'password' && !sshF.password) {
      sshErr = '请输入这条连接的密码，并先测试连接。';
      return;
    }
    sshConnecting = true; sshErr = '';
    try {
      const conn = await invoke<Connection>('update_ssh', {
        connId: sshEditId,
        name: sshF.name.trim(), host: sshF.host.trim(), port: Number(sshF.port) || 22,
        user: sshF.user.trim(), auth: sshF.auth, keyPath: sshF.keyPath,
        // 留空＝不动钥匙串（只改了名字/地址时不必重填密码）
        secret: sshF.auth === 'key' ? sshF.keyPass : sshF.password,
        // 只改名字时没试连，沿用原指纹
        fingerprint: sshFp || sshOldFp,
      });
      sshOpen = false; say(`已更新「${conn.name}」`);
      await refreshConns();
      if (activeConnId === conn.id) {
        disposeTerm();
        void probeOs(conn.id);
        // 补录凭据后原地恢复工作台，不要求用户再点终端和文件区的刷新按钮。
        await tick();
        if (view === 'remote' && wb.term && termEl) void startTerm(conn.id, termEl);
        if (view === 'remote' && wb.files && sftpConnId === conn.id) void sftpGo(sftpPath);
      }
    } catch (e) { sshErr = String(e); }
    finally { sshConnecting = false; }
  }
  async function pickSshKey() {
    try {
      const p = await open({ multiple: false, title: '选择私钥文件（如 ~/.ssh/id_ed25519）' });
      if (typeof p === 'string') sshF.keyPath = p;
    } catch (e) { sshErr = String(e); }
  }
  // 编辑时密码可以留空（＝沿用钥匙串里那条），新建时必须填。
  const sshCanTest = $derived(
    !!sshF.host.trim() && !!sshF.user.trim() &&
    (sshF.auth === 'key' ? !!sshF.keyPath.trim() : (!!sshF.password || (!!sshEditId && !sshCredentialRepair && sshF0.auth === 'password')))
  );
  async function testSsh() {
    if (sshTesting || !sshCanTest) return;
    sshTesting = true; sshErr = ''; sshFp = '';
    try {
      const p = await invoke<{ fingerprint: string; auth_ok: boolean; error: string }>('verify_ssh', {
        host: sshF.host.trim(), port: Number(sshF.port) || 22, user: sshF.user.trim(), auth: sshF.auth,
        password: sshF.password, keyPath: sshF.keyPath, keyPass: sshF.keyPass, expectFingerprint: '',
        // 编辑时密码/口令留空 = 用钥匙串里存着的那条来试（后端取，前端永远看不到明文）
        connId: sshEditId,
      });
      sshFp = p.fingerprint;
    } catch (e) { sshErr = String(e); }
    finally { sshTesting = false; }
  }
  async function confirmSsh() {
    if (sshConnecting || !sshFp) return;
    sshConnecting = true; sshErr = '';
    try {
      const conn = await invoke<Connection>('connect_ssh', {
        name: sshF.name.trim(), host: sshF.host.trim(), port: Number(sshF.port) || 22,
        user: sshF.user.trim(), auth: sshF.auth, keyPath: sshF.keyPath,
        secret: sshF.auth === 'key' ? sshF.keyPass : sshF.password,
        fingerprint: sshFp,
      });
      sshOpen = false; say(`已连接「${conn.name}」`);
      await refreshConns(); await selectConn(conn.id);
      view = 'remote';
    } catch (e) { sshErr = String(e); }
    finally { sshConnecting = false; }
  }

  // ---------- 远程连接：系统信息 ----------
  // os-release 的 ID 归一化到我们画得出图标的那几家；认不出的返回 '' 走通用形。
  function distroOf(c: Connection | undefined | null): string {
    const id = (c?.ssh_os_id || '').toLowerCase();
    if (['ubuntu', 'debian', 'alpine', 'fedora', 'arch', 'centos', 'rocky', 'almalinux', 'rhel'].includes(id)) return id;
    if (id === 'raspbian' || id === 'linuxmint' || id === 'pop' || id === 'kali') return 'debian'; // 都是 debian 系
    if (id === 'arch' || id === 'manjaro' || id === 'endeavouros') return 'arch';
    if (id.startsWith('opensuse') || id === 'sles') return ''; // 没画 suse，走通用形
    return '';
  }
  // 页脚/下拉的副标题：探到了就显示系统，没探到就显示 user@host（总得有东西）。
  /** 远程连接的搜索索引：名称 / IP / user@host / 端口 / 系统都能命中。
   *  机器一多就记不住「哪台是哪个 IP」——所以 IP 必须能搜，这正是用户要它的原因。 */
  const connHay = (c: Connection) =>
    `${c.name} ${c.ssh_user ?? ''}@${c.ssh_host ?? ''} ${c.ssh_host ?? ''}:${c.ssh_port ?? ''} ${c.ssh_os ?? ''} ${c.ssh_os_id ?? ''}`.toLowerCase();
  function sshSub(c: Connection | undefined | null): string {
    if (!c) return '';
    return c.ssh_os || `${c.ssh_user}@${c.ssh_host}${c.ssh_port && c.ssh_port !== 22 ? ':' + c.ssh_port : ''}`;
  }
  let osProbing = $state<Record<string, boolean>>({});
  /** 探一次远端系统版本并存进连接。已探过就不再连（除非 force）。 */
  async function probeOs(id: string, force = false) {
    const c = conns.find((x) => x.id === id);
    if (!c || c.kind !== 'ssh' || osProbing[id]) return;
    if (c.ssh_os && !force) return;
    osProbing = { ...osProbing, [id]: true };
    try {
      const u = await invoke<Connection>('ssh_os_probe', { connId: id });
      conns = conns.map((x) => (x.id === u.id ? u : x));
    } catch (e) {
      // 机器关着/网络不通都很正常：静默（真要用连接时自然会报错），只在手动点刷新时说话。
      if (force) say(String(e), 'err');
    } finally {
      osProbing = { ...osProbing, [id]: false };
    }
  }

  // ---------- 远程工作台：终端为主区，底部对话 / 右侧文件按需开（VS Code 那套） ----------
  // 两个面板可以同时开，各自可拖改大小；开关与尺寸都记住。
  // 面板布局**按对话独立**：同一台机器上不同对话的干活姿势不一样 ——
  // 一条在装东西要盯着终端，另一条只是问问题、想让对话占满。全局一份的话，
  // 在这条里关掉命令行，切过去那条也没了。
  // 存储只留一份「默认」：新对话（和还没碰过的旧对话）用它开局，你一拨就成为新的默认。
  // 各对话的当前布局只放内存 —— 否则 localStorage 会随对话数无限长，删了对话还留一堆孤儿键。
  // 面板布局**按对话独立**：会话1 只留对话框、会话2 只留命令行 —— 各是各的，互不影响。
  //
  // ★ 上一版错在「拨一下顺便记成全局默认」：于是在会话1关掉命令行就改写了默认，
  //   会话2（还没碰过）一开局就继承成会话1的样子 —— 看起来就是「共享」。**开关不许写全局**。
  //
  // 开关（term/chat/files）：每条对话一份，按对话 id 存盘；新对话用固定默认（命令行+对话框）。
  // 尺寸（chatH/filesW）：全局偏好 —— 「面板多高」是手感，不是某条对话的属性，
  //   每开一条新对话就要重新拖一遍才叫烦人。
  type WbFlags = { term: boolean; chat: boolean; files: boolean };
  type WbLayout = WbFlags & { chatH: number; filesW: number };
  const WB_FLAGS_DEF: WbFlags = { term: true, chat: true, files: false };
  const WB_BY_CONV = 'cligc.pilot.wb.byConv';
  let wbStartEl = $state<HTMLTextAreaElement | null>(null); // 起点输入框（新对话后把光标送进去）
  function loadWbSize(k: string, def: number, lo: number, hi: number): number {
    try { const n = parseInt(localStorage.getItem('cligc.pilot.wb.' + k) || ''); return n >= lo && n <= hi ? n : def; } catch { return def; }
  }
  function loadWbByConv(): Record<string, WbFlags> {
    try { const o = JSON.parse(localStorage.getItem(WB_BY_CONV) || '{}'); return o && typeof o === 'object' ? o : {}; } catch { return {}; }
  }
  let wbByConv: Record<string, WbFlags> = loadWbByConv(); // 对话 id → 开关；'new:<连接>' = 该连接还没有对话时的起点
  let wb = $state<WbLayout>({
    ...WB_FLAGS_DEF,
    chatH: loadWbSize('chatH', 260, 140, 900),
    filesW: loadWbSize('filesW', 380, 240, 900),
  });
  let wbKeyCur = '';
  function wbKeyOf(): string { return activeConvId || (isSshConn ? 'new:' + activeConnId : ''); }
  /** 存盘：顺手按「现存对话」清一遍，免得删了对话还留一堆孤儿键把 localStorage 撑大。 */
  function saveWbByConv() {
    try {
      // ★ convos 还没加载完（开机那会儿是空的）时**绝不能清**：那会把所有对话的布局
      //   一把全删光 —— 看起来就是「配置没保住」。空表说明「还不知道有哪些对话」，
      //   不是「一条都没有」。
      if (convos.length) {
        const live = new Set(convos.map((c) => c.id));
        const keep: Record<string, WbFlags> = {};
        for (const [k, v] of Object.entries(wbByConv)) {
          if (k.startsWith('new:') || live.has(k)) keep[k] = v;
        }
        wbByConv = keep;
      }
      localStorage.setItem(WB_BY_CONV, JSON.stringify(wbByConv));
    } catch { /* 配额满/隐私模式：布局丢了就丢了，不值得打断用户 */ }
  }
  // 切对话/切连接 → 取回目标那份（没碰过的用固定默认，**不继承别的对话**）。
  // 不必在这里存：setWbFlags 每次拨动就已经存过了。
  $effect(() => {
    const key = wbKeyOf();
    if (key === wbKeyCur) return;
    const prev = wbKeyCur;
    wbKeyCur = key;
    let f = wbByConv[key];
    // ★ 起点 → 新对话：在起点输入框里拨好的布局要跟过去。这条对话**就是**那个起点变的，
    //   不是「继承别的对话」。漏了它就会：你在起点拨好面板，一发消息全弹回默认。
    if (!f && key && !key.startsWith('new:') && prev.startsWith('new:')) {
      f = wbByConv[prev];
      if (f) { wbByConv[key] = { ...f }; saveWbByConv(); }
    }
    f = f ?? WB_FLAGS_DEF;
    wb = { ...wb, term: f.term, chat: f.chat, files: f.files };
  });
  function saveWbSize(k: 'chatH' | 'filesW', v: number) { try { localStorage.setItem('cligc.pilot.wb.' + k, String(v)); } catch { /* */ } }
  /** 拨一下：只改**当前这条对话**的开关，绝不碰别的对话、也不写全局默认。 */
  function setWbFlags(patch: Partial<WbFlags>) {
    wb = { ...wb, ...patch };
    const key = wbKeyCur || wbKeyOf();
    if (key) {
      wbByConv[key] = { term: wb.term, chat: wb.chat, files: wb.files };
      saveWbByConv();
    }
  }
  /** 三个面板至少留一个开着：全关＝主区一整块空白。
   *  以前更严——终端/对话必须留一个（那会儿文件栏只是右侧窄条、不占主区，关掉这俩主区就空了）。
   *  现在放开：文件栏单开时让它吃满整宽（见 wbFilesSolo + .wb-main.collapsed），
   *  所以「只留文件」是合法布局。每个开关只在「关掉后一块都不剩」时才顶个默认（终端）上来。 */
  function toggleWbTerm() { setWbFlags(!wb.term ? { term: true } : wb.chat || wb.files ? { term: false } : { term: false, chat: true }); }
  function toggleWbChat() { setWbFlags(!wb.chat ? { chat: true } : wb.term || wb.files ? { chat: false } : { chat: false, term: true }); }
  function toggleWbFiles() { setWbFlags(!wb.files ? { files: true } : wb.term || wb.chat ? { files: false } : { files: false, term: true }); }
  // 文件栏单开：终端和对话都关了，只剩文件 —— 这时让它撑满主区（.wb-main 空了就收起来）。
  const wbFilesSolo = $derived(wb.files && !wb.term && !wb.chat);
  /** 面板拖拽。dir=h：拖对话面板的上沿（往上拖＝更高）；dir=v：拖文件面板的左沿（往左拖＝更宽）。 */
  function startWbResize(e: MouseEvent, dir: 'h' | 'v') {
    e.preventDefault();
    const x0 = e.clientX, y0 = e.clientY;
    const h0 = wb.chatH, w0 = wb.filesW;
    const onMove = (ev: MouseEvent) => {
      if (dir === 'h') wb = { ...wb, chatH: Math.min(900, Math.max(140, Math.round(h0 - (ev.clientY - y0)))) };
      else wb = { ...wb, filesW: Math.min(900, Math.max(240, Math.round(w0 - (ev.clientX - x0)))) };
    };
    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.body.style.cursor = '';
      saveWbSize(dir === 'h' ? 'chatH' : 'filesW', dir === 'h' ? wb.chatH : wb.filesW); // 落定才存
    };
    document.body.style.cursor = dir === 'h' ? 'row-resize' : 'col-resize';
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }

  // ---------- 远端负载（顶栏的 CPU / 内存 / 磁盘） ----------
  // CPU 那两个字段是 /proc/stat 的**累计计数**，不是百分比：单次读没意义，得跟上一次求差。
  // 所以这里按连接留一份上次的样本；第一次只能显示「—」，第二次（5 秒后）才有 CPU%。
  type SshStats = { cpu_total: number; cpu_idle: number; mem_total_kb: number; mem_avail_kb: number; disk_total_kb: number; disk_used_kb: number };
  const STAT_EVERY_MS = 5000;
  let statPrev: Record<string, SshStats> = {};
  let stat = $state<null | { cpu: number | null; memPct: number; memText: string; diskPct: number; diskText: string }>(null);
  function fmtKb(kb: number): string {
    const u = ['KB', 'MB', 'GB', 'TB'];
    let v = kb, i = 0;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
  }
  async function pollStats(id: string) {
    try {
      const s = await invoke<SshStats>('ssh_stats', { connId: id });
      const p = statPrev[id];
      statPrev[id] = s;
      // 两次采样之间的差：分母是这段时间里所有时间片的增量，分子是其中非空闲的部分。
      let cpu: number | null = null;
      if (p) {
        const dt = s.cpu_total - p.cpu_total;
        const di = s.cpu_idle - p.cpu_idle;
        // 机器重启过（计数器归零）→ dt<=0，这一轮跳过，下一轮就正常了
        if (dt > 0) cpu = Math.min(100, Math.max(0, Math.round((1 - di / dt) * 100)));
      }
      const memUsed = s.mem_total_kb - s.mem_avail_kb;
      stat = {
        cpu,
        memPct: s.mem_total_kb ? Math.round((memUsed / s.mem_total_kb) * 100) : 0,
        memText: `${fmtKb(memUsed)} / ${fmtKb(s.mem_total_kb)}`,
        diskPct: s.disk_total_kb ? Math.round((s.disk_used_kb / s.disk_total_kb) * 100) : 0,
        diskText: `${fmtKb(s.disk_used_kb)} / ${fmtKb(s.disk_total_kb)}`,
      };
    } catch {
      // 没连上 / 不是 Linux / 命令被拒 —— 都不吭声，顶栏空着就是了（这不是用户要办的事）
      stat = null;
    }
  }
  // 只在看得见工作台时才轮询；换连接清掉上一台的数（否则会拿别的机器的样本算差）。
  $effect(() => {
    const id = activeConnId;
    if (view !== 'remote' || !isSshConn || !id) { stat = null; return; }
    stat = null;
    delete statPrev[id];
    void pollStats(id);
    const t = setInterval(() => { if (!document.hidden) void pollStats(id); }, STAT_EVERY_MS);
    return () => clearInterval(t);
  });

  // ---------- 远程终端（xterm + PTY 流） ----------
  const isSshConn = $derived(activeConn?.kind === 'ssh');
  const isWorkspaceChat = $derived(activeConn?.kind === 'workspace');
  let termEl = $state<HTMLDivElement | null>(null);
  let termOn = $state(false);
  let termBusy = $state(false); // 建连中：刷新键转起来 + 禁重复点
  let termErr = $state('');
  let term: import('@xterm/xterm').Terminal | null = null;
  let termFit: import('@xterm/addon-fit').FitAddon | null = null;
  let termConnId = '';
  async function copyTermSelection(t: import('@xterm/xterm').Terminal) {
    const text = t.getSelection();
    if (!text) return;
    try { await navigator.clipboard.writeText(text); }
    catch { say('复制失败，请检查系统剪贴板权限', 'err'); }
  }
  async function pasteIntoTerm(t: import('@xterm/xterm').Terminal) {
    try {
      const text = await navigator.clipboard.readText();
      // 读剪贴板是异步的；期间用户可能已经切换连接/拆掉旧终端。
      if (term === t && text) t.paste(text);
    } catch {
      say('读取剪贴板失败，可在终端中右键选择“粘贴”', 'err');
    }
  }
  function handleTermKey(t: import('@xterm/xterm').Terminal, e: KeyboardEvent): boolean {
    if (e.type !== 'keydown') return true;
    const key = e.key.toLowerCase();
    // Windows 兼容两套习惯：有选区时 Ctrl+C 复制，无选区时仍把 ^C 发给远端；
    // Ctrl+Shift+C 始终按终端快捷键处理。macOS 则保持 Cmd+C，Linux 用 Ctrl+Shift+C。
    const copy = (isMac && e.metaKey && key === 'c')
      || (!isMac && e.ctrlKey && e.shiftKey && key === 'c')
      || (isWindows && e.ctrlKey && key === 'c' && t.hasSelection());
    if (copy) {
      e.preventDefault(); e.stopPropagation();
      void copyTermSelection(t);
      return false;
    }

    // macOS 的 Cmd+V 交还给系统/WKWebView，避免 navigator.clipboard.readText 触发
    // 系统二次 Paste 授权；Windows 显式读取，绕开 WebView2 偶发不派发 paste 事件的问题。
    const paste = (isWindows && e.ctrlKey && key === 'v')
      || (!isMac && !isWindows && e.ctrlKey && e.shiftKey && key === 'v')
      || (!isMac && e.shiftKey && e.key === 'Insert');
    if (paste) {
      e.preventDefault(); e.stopPropagation();
      void pasteIntoTerm(t);
      return false;
    }

    // Tab 在 WebView2 / WKWebView 中可能先被宿主当成「切换页面焦点」，xterm 收不到，
    // Shell 就无法做命令与路径补全。终端聚焦时显式走用户输入通道；Shift+Tab
    // 保留终端通用的反向制表序列。带 Ctrl/Alt/Meta 的组合键仍交给 xterm/宿主。
    if (e.key === 'Tab' && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault(); e.stopPropagation();
      t.input(e.shiftKey ? '\x1b[Z' : '\t', true);
      return false;
    }

    // WebView2 会把 Escape 留给宿主 WebView，xterm 收不到时 vi 就无法退出插入模式。
    // 这里直接走 xterm 的用户输入通道，macOS 也统一经过同一路径，且不会发送两次。
    if (e.key === 'Escape') {
      e.preventDefault(); e.stopPropagation();
      t.input('\x1b', true);
      return false;
    }
    return true;
  }
  // 注意：**不清 activeConv** —— 工作台里终端一直在，没有「切到终端」这回事；
  // 清掉对话只会把用户正在聊的东西关了（那正是侧栏「远程终端」入口被删掉的原因）。
  function openRemote() { view = 'remote'; }
  function b64ToBytes(b64: string): Uint8Array {
    const s = atob(b64); const a = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i);
    return a;
  }
  function bytesToB64(bytes: Uint8Array): string {
    let bin = '';
    // 避免把大数组一次性 spread 进 fromCharCode，超过 JS 参数上限会直接抛错。
    for (let start = 0; start < bytes.length; start += 0x8000) {
      const end = Math.min(start + 0x8000, bytes.length);
      for (let i = start; i < end; i++) bin += String.fromCharCode(bytes[i]);
    }
    return btoa(bin);
  }
  function strToB64(s: string): string {
    let bin = '';
    for (const b of new TextEncoder().encode(s)) bin += String.fromCharCode(b);
    return btoa(bin);
  }
  function disposeTerm() {
    if (term) { try { term.dispose(); } catch { /* */ } }
    term = null; termFit = null; termOn = false; termBusy = false; termErr = ''; termConnId = '';
  }
  // 终端容器一变大小就重排：开关面板、拖面板边、拉窗口——一个 ResizeObserver 全包了，
  // 不用在每个改布局的地方各记一次 fit()（漏一个就是花屏）。
  $effect(() => {
    const el = termEl;
    if (!el) return;
    let raf = 0;
    const ro = new ResizeObserver(() => {
      // 合并同一帧里的连续变化（拖拽时每像素都会触发）
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => { try { termFit?.fit(); } catch { /* 终端已拆 */ } });
    });
    ro.observe(el);
    return () => { cancelAnimationFrame(raf); ro.disconnect(); };
  });
  // WKWebView 有时会在 xterm 的自定义按键处理器之前吞掉 Escape。直接在终端容器的
  // capture 阶段截获并写入 PTY，保证 vi/vim 的“退出插入模式”不依赖 WebView 默认行为。
  $effect(() => {
    const el = termEl;
    if (!el) return;
    const onEscape = (event: KeyboardEvent) => {
      if ((event.key !== 'Escape' && event.key !== 'Esc' && event.keyCode !== 27) || !termOn || !termConnId) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      void invoke('ssh_input', { connId: termConnId, b64: strToB64('\x1b') });
    };
    el.addEventListener('keydown', onEscape, true);
    return () => el.removeEventListener('keydown', onEscape, true);
  });
  // 进 remote 视图且容器就位 → 起终端；切走/换连接由 $effect 的清理拆掉。
  $effect(() => {
    const id = activeConnId;
    const el = termEl;
    if (view !== 'remote' || !isSshConn || !el || !id) return;
    if (termConnId === id && term) return; // 同一连接已在跑
    void startTerm(id, el);
  });
  async function startTerm(id: string, el: HTMLDivElement) {
    disposeTerm();
    termConnId = id;
    termBusy = true;
    termErr = '';
    // xterm 要 DOM，动态 import（SSR 关着，但仍别在模块顶层拉进来）。
    const [{ Terminal }, { FitAddon }] = await Promise.all([
      import('@xterm/xterm'),
      import('@xterm/addon-fit'),
    ]);
    // macOS 的 SF Mono/Menlo 在 Retina 下 12px 已经很清楚；Windows WebView2 在 100% DPI
    // 下同字号会显得细小、行距又紧。优先使用 Windows Terminal 同款 Cascadia Mono，
    // 未安装时回退 Consolas，并单独放大一档；FitAddon 会据此重新计算行列，不会裁字。
    const terminalFontFamily = isWindows
      ? '"Cascadia Mono", "Cascadia Code", Consolas, "Lucida Console", monospace'
      : 'ui-monospace, SFMono-Regular, Menlo, monospace';
    const t = new Terminal({
      fontSize: isWindows ? 14 : 12,
      fontFamily: terminalFontFamily,
      lineHeight: isWindows ? 1.15 : 1,
      letterSpacing: isWindows ? 0.2 : 0,
      fontWeight: isWindows ? '400' : 'normal',
      fontWeightBold: isWindows ? '600' : 'bold',
      cursorBlink: true,
      // ★ 滚动条宽度只能从这儿改：xterm 6 不再用 viewport 的原生滚动条，改成了自绘的
      // （.xterm-scrollable-element > .scrollbar，从 VS Code 移植），宽度在 JS 里取
      // `overviewRuler?.width || 14` —— CSS 的 ::-webkit-scrollbar 对它完全无效。
      // 副作用：这个选项**身兼两职**，一设就同时把「装饰栏」渲染出来（文档原话：this must be
      // set in order to see the overview ruler），它会在自己左沿画一条贯穿全高的 1px 竖线
      // （_renderRulerOutline，用 theme.overviewRulerBorder，默认深色）——就是那条碍眼的线。
      // 我们压根不用 decoration，那个 canvas 纯属负担 → CSS 里直接 display:none（见 .term-wrap 那段）。
      overviewRuler: { width: 7 },
      theme: {
        background: '#1c1917',
        foreground: '#e8e4dc',
        cursor: '#e8e4dc',
        // 自绘滚动条的三档颜色（xterm 6 的 theme 项）：平时很淡，摸上去才明显。
        scrollbarSliderBackground: '#4b464033',
        scrollbarSliderHoverBackground: '#4b4640aa',
        scrollbarSliderActiveBackground: '#635d55cc',
      },
    });
    const f = new FitAddon();
    t.loadAddon(f); t.open(el); f.fit();
    term = t; termFit = f;
    t.attachCustomKeyEventHandler((e) => handleTermKey(t, e));
    // GPU 渲染：默认的 DOM 渲染器在 Retina 上敲字会发涩（每个字符都是 DOM 活儿）。
    // 必须 open() 之后再挂，且要能回退 —— WebView 里 WebGL 不一定可用，
    // 上下文丢失（切显卡/系统回收）也要退回 DOM，否则整个终端会变黑。
    try {
      const { WebglAddon } = await import('@xterm/addon-webgl');
      const gl = new WebglAddon();
      gl.onContextLoss(() => { try { gl.dispose(); } catch { /* 退回 DOM 渲染 */ } });
      t.loadAddon(gl);
    } catch { /* 没 WebGL 就用默认的 DOM 渲染器，功能不受影响 */ }
    const ch = new Channel<{ type: string; b64?: string; error?: string }>();
    ch.onmessage = (ev) => {
      if (term !== t) return; // 已被拆掉/换连接，丢弃尾包
      if (ev.type === 'data' && ev.b64) t.write(b64ToBytes(ev.b64));
      else if (ev.type === 'closed') {
        termOn = false;
        t.write(`\r\n\x1b[90m—— 连接已关闭${ev.error ? '：' + ev.error : ''}，按回车重新连接 ——\x1b[0m\r\n`);
      }
    };
    t.onData((d) => {
      // 关闭态：回车＝就地重连（iTerm/VS Code 的肌肉记忆），其余按键不再灌进死管道；
      // 连接中（termBusy）也别触发，免得连点回车叠一队重连。
      if (!termOn) {
        if (d.includes('\r') && !termBusy) void reconnectTerm();
        return;
      }
      void invoke('ssh_input', { connId: id, b64: strToB64(d) });
    });
    t.onResize(({ cols, rows }) => { void invoke('ssh_resize', { connId: id, cols, rows }); });
    // 「正在连接」用浮层（见 .term-connecting），**不往终端里写**：
    // 写进去就是终端缓冲的一部分，连上之后擦不干净（服务器的 banner 可能已经跟着来了，
    // 光标位置不由我们说了算）—— 那行字就会一直赖在最上面。浮层随 termBusy 自动收。
    try {
      await invoke('ssh_open_shell', { connId: id, cols: t.cols, rows: t.rows, onEvent: ch });
      termOn = true;
    } catch (e) {
      termErr = String(e);
      t.write(`\r\n\x1b[31m${termErr}\x1b[0m\r\n`);
    } finally {
      if (term === t) termBusy = false; // 已被换掉的旧终端别去动当前状态
    }
  }
  async function reconnectTerm() {
    const id = activeConnId; const el = termEl;
    if (!id || !el || termBusy) return;
    termBusy = true; // 立刻转起来：ssh_close 也要等一下，别让这段时间看着像没反应
    try { await invoke('ssh_close', { connId: id }); } catch { /* 本来就没连上，照样重来 */ }
    await startTerm(id, el);
  }

  // ---------- 远程文件（SFTP，走终端同一条 SSH 会话） ----------
  // 视图＝可展开的树：sftpPath 是树根，每个目录的孩子按需加载后缓存在 sftpKids 里。
  type SftpEntry = { name: string; dir: boolean; link: boolean; perms: string; size: number; mtime: number };
  type SftpRow = SftpEntry & { path: string; depth: number };
  let sftpConnId = $state(''); // 已加载文件列表所属的连接；换连接才重置，切页签不重置
  let sftpPath = $state('');
  let sftpPathDraft = $state('');
  let sftpKids = $state<Record<string, SftpEntry[]>>({}); // 目录路径 → 它的孩子
  let sftpOpenDirs = $state(new Set<string>());
  let sftpLoading = $state<Record<string, boolean>>({});
  let sftpSel = $state('');
  let sftpSort = $state<{ key: 'name' | 'size' | 'mtime'; asc: boolean }>({ key: 'name', asc: true });
  let sftpBusy = $state(false);
  let sftpErr = $state('');
  function missingSshCredential(error: string): boolean {
    return error.includes('钥匙串里没有这个连接的密码');
  }
  function repairActiveSshCredential() {
    if (activeConn?.kind === 'ssh') openSshCredentialRepair(activeConn);
  }
  function retryOrRepairSftp() {
    if (missingSshCredential(sftpErr)) repairActiveSshCredential();
    else void sftpGo(sftpPath);
  }
  let sftpXfer = $state(''); // 上传/下载进行中的提示文案
  type DownloadProgress = { phase: string; bytes: number; total_bytes: number; files: number; total_files: number; skipped: number };
  type DownloadResult = { path: string; drag_id: string | null; skipped: number };
  let sftpDownloadTask = $state<null | { id: string; name: string; drag: boolean; started: boolean; cancelling: boolean; progress: DownloadProgress | null }>(null);
  let sftpUploadTask = $state<null | { id: string; started: boolean; cancelling: boolean }>(null);
  let sftpNativeDragging = $state(false);
  type SftpDragEvent = { phase: 'dragging' | 'dropped' | 'completed' | 'cancelled' | 'error'; path?: string; error?: string };
  const sftpHistoryKey = `${SFTP_TRANSFER_HISTORY_KEY}:${pilotWindowLabel}`;
  let sftpTransfers = $state<TransferRecord[]>(loadSftpTransfers());
  let sftpHistoryTimer: ReturnType<typeof setTimeout> | undefined;
  const sftpCancellableIds = $derived([sftpDownloadTask, sftpUploadTask].filter(task => task?.started).map(task => task!.id));
  function loadSftpTransfers(): TransferRecord[] {
    try { return readTransferHistory(localStorage.getItem(sftpHistoryKey)); } catch { return []; }
  }
  function persistSftpTransfers(immediate = false) {
    const save = () => {
      sftpHistoryTimer = undefined;
      try { localStorage.setItem(sftpHistoryKey, serializeTransferHistory(sftpTransfers)); } catch { /* Storage may be unavailable or full. */ }
    };
    if (immediate) { clearTimeout(sftpHistoryTimer); save(); }
    else if (!sftpHistoryTimer) sftpHistoryTimer = setTimeout(save, 250);
  }
  onDestroy(() => { clearTimeout(sftpHistoryTimer); persistSftpTransfers(true); });
  function addSftpTransfer(id: string, name: string, direction: 'upload' | 'download', connId: string, remotePath: string, localPath?: string, phase = 'connecting') {
    const conn = conns.find(c => c.id === connId);
    sftpTransfers = addTransfer(sftpTransfers, createTransfer({ id, name, direction, connId, connectionName: conn?.remark?.trim() || conn?.name || '远程连接', remotePath, localPath, phase, status: phase === 'dragging' ? 'queued' : 'running' }));
    persistSftpTransfers(true);
  }
  function updateSftpTransfer(id: string, patch: TransferUpdate) {
    sftpTransfers = updateTransfer(sftpTransfers, id, patch);
    persistSftpTransfers();
  }
  function progressSftpTransfer(id: string, p: DownloadProgress) {
    updateSftpTransfer(id, { status: 'running', phase: p.phase, bytes: p.bytes, totalBytes: p.total_bytes, files: p.files, totalFiles: p.total_files });
  }
  function finishSftpTransfer(id: string, status: TransferFinalStatus, patch: TransferUpdate = {}) {
    sftpTransfers = finishTransfer(sftpTransfers, id, status, patch);
    persistSftpTransfers(true);
  }
  function clearSftpTransfers() { sftpTransfers = sftpTransfers.filter(isTransferActive); persistSftpTransfers(true); }
  async function revealSftpTransfer(record: TransferRecord) {
    if (!record.localPath || record.status !== 'completed' || record.direction !== 'download') return;
    try { await revealItemInDir(record.localPath); } catch (e) { say(String(e), 'err'); }
  }
  async function cancelSftpTransfer(id: string) {
    if (sftpDownloadTask?.id === id) return cancelSftpDownload();
    if (sftpUploadTask?.id !== id || !sftpUploadTask.started || sftpUploadTask.cancelling) return;
    sftpUploadTask = { ...sftpUploadTask, cancelling: true };
    updateSftpTransfer(id, { cancelling: true });
    try { await invoke('sftp_cancel_download', { transferId: id }); }
    catch (e) {
      if (sftpUploadTask?.id === id) sftpUploadTask = { ...sftpUploadTask, cancelling: false };
      updateSftpTransfer(id, { cancelling: false });
      say(String(e), 'err');
    }
  }
  let sftpFileOver = $state(false);
  let sftpDropTarget = $state<SftpDropTarget | null>(null);
  type LocalDropEntry = { name: string; size: number; mtime: number; directory: boolean | null };
  type LocalDropCapture = { token: string; count: number };
  type LocalDropProbe = { capture: Promise<LocalDropCapture | null>; entries: LocalDropEntry[] };
  let sftpDropCapture: Promise<LocalDropCapture | null> | null = null;
  function beginSftpLocalDrop() {
    if (!isMac || sftpDropCapture) return;
    // Native code binds this one-use token to a real drag sequence and window,
    // never to a stale drag pasteboard or the general clipboard.
    const capture = invoke<LocalDropCapture>('sftp_capture_local_drop');
    // A hover can be abandoned without a drop. Suppress an unhandled rejection
    // here, but retain the rejection so an accepted drop fails closed.
    void capture.catch(() => {});
    sftpDropCapture = capture;
  }
  function discardSftpLocalDrop() {
    const capture = sftpDropCapture;
    sftpDropCapture = null;
    if (capture) void capture.then(result => {
      if (result) void invoke('sftp_discard_local_drop', { token: result.token }).catch(() => {});
    }).catch(() => {});
  }
  $effect(() => {
    // Invalidate a hover when its panel, connection or directory goes away.
    void view; void activeConnId; void wb.files; void sftpPath;
    return () => {
      sftpFileOver = false; sftpDropTarget = null;
      discardSftpLocalDrop();
    };
  });
  function sftpDropTargetAt(e: DragEvent): SftpDropTarget | null {
    const element = e.target instanceof Element ? e.target : null;
    const path = element?.closest<HTMLElement>('[data-sftp-drop-path]')?.dataset.sftpDropPath;
    const hoveredRow = path ? sftpRows.find(row => row.path === path) : undefined;
    const dir = hoveredRow?.dir && !hoveredRow.link ? hoveredRow.path : sftpPath;
    return resolveSftpDropTarget({ connId: sftpConnId, remoteDir: sftpPath, busy: sftpBusy,
      transferring: !!sftpXfer, nativeDragging: sftpNativeDragging,
      insidePanel: !!element && (e.currentTarget as HTMLElement).contains(element), hoveredRow,
      targetDeleting: isSftpDeleting(dir) });
  }
  function localDropEntries(data: DataTransfer, files: File[]): LocalDropEntry[] {
    // Read protected drag items synchronously, before any IPC/await clears them.
    const items = Array.from(data.items).filter(item => item.kind === 'file');
    return files.map((file, index) => {
      let directory: boolean | null = null;
      try { const entry = items[index]?.webkitGetAsEntry?.(); if (entry) directory = entry.isDirectory; } catch { /* Non-filesystem drag. */ }
      return { name: file.name, size: file.size, mtime: file.lastModified, directory };
    });
  }
  let sftpClipboard = $state<null | { connId: string; path: string; name: string; dir: boolean }>(null);
  // connId + path：切换服务器时，旧服务器仍在进行的删除不会误伤当前列表状态。
  let sftpDeleting = $state(new Set<string>());
  function sftpDeletingKey(connId: string, path: string): string { return `${connId}\n${path}`; }
  function isSftpDeleteTarget(path: string): boolean { return sftpDeleting.has(sftpDeletingKey(sftpConnId, path)); }
  function isSftpDeleting(path: string): boolean {
    const prefix = `${sftpConnId}\n`;
    for (const key of sftpDeleting) {
      if (!key.startsWith(prefix)) continue;
      const target = key.slice(prefix.length).replace(/\/+$/, '');
      if (path === target || path.startsWith(`${target}/`)) return true;
    }
    return false;
  }
  // 首次开文件面板或换了连接 → 从 home 起步；关了再开则保留原目录。
  $effect(() => {
    const id = activeConnId;
    if (view !== 'remote' || !isSshConn || !wb.files || !id) return;
    if (sftpConnId === id) return;
    sftpConnId = id; sftpPath = ''; sftpPathDraft = ''; sftpErr = '';
    sftpKids = {}; sftpOpenDirs = new Set(); sftpSel = '';
    void sftpGo('');
  });
  function sftpJoin(dir: string, name: string): string { return dir === '/' ? '/' + name : dir + '/' + name; }
  function parentOf(p: string): string { return p.replace(/\/[^/]+\/?$/, '') || '/'; }
  function baseOf(p: string): string { return p.split('/').filter(Boolean).pop() || p; }
  function sortRows(list: SftpEntry[]): SftpEntry[] {
    const { key, asc } = sftpSort;
    const sign = asc ? 1 : -1;
    return [...list].sort((a, b) => {
      if (key === 'size') return sign * (a.size - b.size);
      if (key === 'mtime') return sign * (a.mtime - b.mtime);
      return sign * a.name.localeCompare(b.name, 'zh-Hans-CN', { numeric: true });
    });
  }
  function setSort(key: 'name' | 'size' | 'mtime') {
    sftpSort = sftpSort.key === key ? { key, asc: !sftpSort.asc } : { key, asc: true };
  }
  // ---- 列宽可拖（名称是弹性列，吃掉剩下的宽度）----
  // 手柄在每个定宽列的**左边缘**上：拖它就是拖这一列的左边，往右拖＝这列变窄，名称随之变宽。
  // 这样手柄始终跟着鼠标走，三根手柄的行为也一致。
  type ColKey = 'perm' | 'size' | 'date';
  const COL_MIN: Record<ColKey, number> = { perm: 62, size: 48, date: 92 };
  const COL_MAX: Record<ColKey, number> = { perm: 200, size: 160, date: 240 };
  const COL_DEF: Record<ColKey, number> = { perm: 96, size: 76, date: 128 };
  let colW = $state(loadColW());
  function loadColW(): Record<ColKey, number> {
    try {
      const raw = JSON.parse(localStorage.getItem('cligc.pilot.sftpCols') || '{}');
      const pick = (k: ColKey) => {
        const n = Number(raw[k]);
        return Number.isFinite(n) && n >= COL_MIN[k] && n <= COL_MAX[k] ? n : COL_DEF[k];
      };
      return { perm: pick('perm'), size: pick('size'), date: pick('date') };
    } catch { return { ...COL_DEF }; }
  }
  function saveColW() { try { localStorage.setItem('cligc.pilot.sftpCols', JSON.stringify(colW)); } catch { /* */ } }
  function setColW(k: ColKey, w: number) {
    colW = { ...colW, [k]: Math.min(COL_MAX[k], Math.max(COL_MIN[k], Math.round(w))) };
  }
  function startColDrag(e: MouseEvent, k: ColKey) {
    e.preventDefault();
    e.stopPropagation(); // 别让这一下变成「点了列头去排序」
    const x0 = e.clientX;
    const w0 = colW[k];
    const onMove = (ev: MouseEvent) => setColW(k, w0 - (ev.clientX - x0));
    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.body.style.cursor = '';
      saveColW();
    };
    document.body.style.cursor = 'col-resize';
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }
  // 键盘也能调（手柄是个 button，Tab 得到）
  function onColKey(e: KeyboardEvent, k: ColKey) {
    const d = e.key === 'ArrowLeft' ? 8 : e.key === 'ArrowRight' ? -8 : 0;
    if (!d) return;
    e.preventDefault();
    setColW(k, colW[k] + d);
    saveColW();
  }
  // 树 → 扁平行（只把展开着的目录的孩子摊进来）。
  const sftpRows = $derived.by(() => {
    const out: SftpRow[] = [];
    const walk = (dir: string, depth: number) => {
      for (const e of sortRows(sftpKids[dir] ?? [])) {
        const p = sftpJoin(dir, e.name);
        out.push({ ...e, path: p, depth });
        if (e.dir && !e.link && sftpOpenDirs.has(p)) walk(p, depth + 1);
      }
    };
    if (sftpPath) walk(sftpPath, 0);
    return out;
  });
  const sftpSelectedRow = $derived(sftpRows.find((row) => row.path === sftpSel));
  /** 读一个目录的孩子进缓存。force=true 时无视缓存重读（改动后刷新用）。 */
  async function loadDir(path: string, force = false): Promise<boolean> {
    const id = sftpConnId;
    if (!force && sftpKids[path]) return true;
    sftpLoading = { ...sftpLoading, [path]: true };
    try {
      const list = await invoke<SftpEntry[]>('sftp_list', { connId: id, path });
      if (sftpConnId !== id) return false; // 加载途中换了连接，丢弃
      sftpKids = { ...sftpKids, [path]: list };
      return true;
    } catch (e) {
      if (path === sftpPath) sftpErr = String(e); else say(String(e), 'err');
      return false;
    } finally {
      sftpLoading = { ...sftpLoading, [path]: false };
    }
  }
  /** 换树根（path 为空＝去 home）。 */
  async function sftpGo(path: string) {
    const id = sftpConnId;
    sftpBusy = true; sftpErr = '';
    try {
      const p = path || await invoke<string>('sftp_home', { connId: id });
      if (sftpConnId !== id) return;
      sftpPath = p; sftpPathDraft = p; sftpSel = '';
      sftpOpenDirs = new Set();
      await loadDir(p, true);
    } catch (e) { sftpErr = String(e); }
    finally { sftpBusy = false; }
  }
  function sftpUp() {
    if (!sftpPath || sftpPath === '/') return;
    void sftpGo(parentOf(sftpPath));
  }
  // ---- 路径栏：面包屑（可点逐级跳）／点空白处切成输入框直接敲路径 ----
  const sftpCrumbs = $derived.by(() => {
    const out = [{ name: '/', path: '/' }];
    let acc = '';
    for (const p of sftpPath.split('/').filter(Boolean)) { acc += '/' + p; out.push({ name: p, path: acc }); }
    return out;
  });
  let sftpPathEdit = $state(false);
  let sftpPathEl = $state<HTMLInputElement | null>(null);
  let crumbEl = $state<HTMLElement | null>(null);
  function startPathEdit() { sftpPathDraft = sftpPath; sftpPathEdit = true; }
  $effect(() => { if (sftpPathEdit && sftpPathEl) { sftpPathEl.focus(); sftpPathEl.select(); } });
  // 路径深了就把尾巴（当前目录）滚进视野——面包屑是从左往右长的。
  $effect(() => {
    void sftpCrumbs;
    if (crumbEl) crumbEl.scrollLeft = crumbEl.scrollWidth;
  });
  function commitPath() {
    const p = sftpPathDraft.trim();
    sftpPathEdit = false;
    if (p && p !== sftpPath) void sftpGo(p);
  }
  /** 刷新：重读当前展开着的每一层，**保留展开状态**（sftpGo 是「换根」，会把整棵树收起来）。 */
  async function sftpRefresh() {
    if (!sftpPath || sftpBusy) return;
    sftpBusy = true; sftpErr = '';
    try {
      const dirs = [sftpPath, ...sftpOpenDirs];
      const ok = await Promise.all(dirs.map((d) => loadDir(d, true)));
      // 远端已经不在了的目录：连同展开态一起丢掉，别在树里留幽灵行
      const gone = dirs.filter((_, i) => !ok[i] && dirs[i] !== sftpPath);
      if (gone.length) {
        const n = new Set(sftpOpenDirs);
        const kids = { ...sftpKids };
        for (const d of gone) { n.delete(d); delete kids[d]; }
        sftpOpenDirs = n; sftpKids = kids;
      }
    } finally { sftpBusy = false; }
  }
  async function toggleDir(r: SftpRow) {
    if (isSftpDeleting(r.path)) return;
    const n = new Set(sftpOpenDirs);
    if (n.has(r.path)) { n.delete(r.path); sftpOpenDirs = n; return; }
    if (!(await loadDir(r.path))) return;
    n.add(r.path); sftpOpenDirs = n;
  }
  /** 试着把它当目录读一下。不是目录就 false —— 调用方退回按文件处理，不报错。 */
  async function isRemoteDir(path: string): Promise<boolean> {
    try { await invoke<SftpEntry[]>('sftp_list', { connId: sftpConnId, path }); return true; }
    catch { return false; }
  }
  /** 打开（双击 / 回车 / 右键「打开」）：目录＝**进到里面去**（换根，不是就地展开——展开是前面那个小箭头干的事）；
   *  软链＝先当目录试（sftp 的 readdir 给的是 lstat，指向目录的软链这里 dir=false）；文件＝在线编辑。 */
  async function sftpOpenRow(r: SftpRow) {
    if (isSftpDeleting(r.path)) return;
    if (r.dir && !r.link) { await sftpGo(r.path); return; }
    if (r.link && await isRemoteDir(r.path)) { await sftpGo(r.path); return; }
    sftpEdit(r.path);
  }
  function fmtSize(n: number): string {
    if (n < 1024) return `${n} B`;
    const u = ['KB', 'MB', 'GB', 'TB']; let v = n / 1024; let i = 0;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
  }
  function fmtStamp(secs: number): string {
    if (!secs) return '';
    const d = new Date(secs * 1000);
    const p = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()} ${p(d.getHours())}:${p(d.getMinutes())}`;
  }
  // ---- 右键菜单 ----
  let fctxMenu = $state<null | { x: number; y: number; row: SftpRow | null }>(null);
  let fctxMenuEl = $state<HTMLDivElement | null>(null);
  $effect(() => {
    if (!fctxMenu || !fctxMenuEl) return;
    const { x, y } = fitFileMenu(fctxMenu.x, fctxMenu.y, fctxMenuEl.offsetWidth, fctxMenuEl.offsetHeight, window.innerWidth, window.innerHeight);
    if (x !== fctxMenu.x || y !== fctxMenu.y) fctxMenu = { ...fctxMenu, x, y };
  });
  function openFctx(e: MouseEvent, row: SftpRow | null) {
    e.preventDefault();
    e.stopPropagation(); // 空白处也不能冒泡到 window 的菜单关闭监听。
    if (!sftpPath || sftpBusy) return;
    if (row && isSftpDeleting(row.path)) return;
    if (row) sftpSel = row.path;
    // 贴边时往回收，别让菜单跑出窗口
    fctxMenu = {
      x: Math.max(8, Math.min(e.clientX, window.innerWidth - 190)),
      y: Math.max(8, Math.min(e.clientY, window.innerHeight - 320)),
      row,
    };
  }
  $effect(() => {
    if (!fctxMenu) return;
    const close = () => (fctxMenu = null);
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') fctxMenu = null; };
    // 冒泡阶段：菜单项自己的 onclick 先跑，再由这里收摊。
    window.addEventListener('click', close);
    window.addEventListener('contextmenu', close);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('contextmenu', close);
      window.removeEventListener('keydown', onKey);
    };
  });
  async function copyPath(p: string) {
    try { await navigator.clipboard.writeText(p); say('已复制路径'); }
    catch { say('复制失败，请手动选择复制', 'err'); }
  }
  function copySftpEntry(r: SftpRow) {
    sftpClipboard = { connId: sftpConnId, path: r.path, name: r.name, dir: r.dir && !r.link };
    say(`已复制「${r.name}」，在目标文件夹右键粘贴`);
  }
  async function pasteSftpEntry(dir = sftpPath, copied = sftpClipboard) {
    if (!copied || copied.connId !== sftpConnId || !dir || sftpXfer) return;
    const connId = copied.connId;
    sftpXfer = `复制 ${copied.name}…`;
    try {
      // 粘贴前重新读取目标目录，避免缓存落后于服务器时误判重名并让 cp 失败。
      const kids = await invoke<SftpEntry[]>('sftp_list', { connId, path: dir });
      const name = copyTargetName(copied.name, copied.dir, new Set(kids.map((item) => item.name)));
      const target = sftpJoin(dir, name);
      await invoke('sftp_copy', { connId, from: copied.path, to: target });
      say(name === copied.name ? `已粘贴「${name}」` : `已粘贴为「${name}」`);
      if (sftpConnId === connId) await loadDir(dir, true);
    } catch (e) { say(String(e), 'err'); }
    finally { sftpXfer = ''; }
  }
  function duplicateSftpEntry(r: SftpRow) {
    return pasteSftpEntry(sftpPath, { connId: sftpConnId, path: r.path, name: r.name, dir: r.dir && !r.link });
  }
  // 新建文件夹 / 文件使用弹窗；重命名单独在列表中原地编辑。
  let fsAsk = $state<null | { mode: 'mkdir' | 'create'; connId: string; dir: string; from: string; value: string; busy: boolean; err: string }>(null);
  function openMkdir(dir = sftpPath) { fsAsk = { mode: 'mkdir', connId: sftpConnId, dir, from: '', value: '', busy: false, err: '' }; }
  function openNewFile(dir = sftpPath) { fsAsk = { mode: 'create', connId: sftpConnId, dir, from: '', value: '', busy: false, err: '' }; }
  let fileRename = $state<null | { connId: string; path: string; name: string; directory: boolean; value: string; busy: boolean; err: string }>(null);
  function openRename(r: SftpRow) {
    fctxMenu = null;
    if (fileRename?.busy || isSftpDeleting(r.path)) return;
    sftpSel = r.path;
    fileRename = { connId: sftpConnId, path: r.path, name: r.name, directory: r.dir, value: r.name, busy: false, err: '' };
  }
  function focusFileRename(node: HTMLInputElement) {
    // Wait until bind:value has populated the input before selecting the stem.
    queueMicrotask(() => {
      if (!node.isConnected) return;
      node.focus();
      const dot = node.value.lastIndexOf('.');
      node.setSelectionRange(0, !fileRename?.directory && dot > 0 ? dot : node.value.length);
    });
  }
  async function saveFileRename() {
    const edit = fileRename;
    if (!edit || edit.busy || edit.connId !== sftpConnId) return;
    const name = edit.value.trim();
    if (!name || name === '.' || name === '..' || /[/\0]/.test(name)) {
      edit.err = '名字不能为空，也不能包含 /、空字符或使用 .、..'; return;
    }
    if (name === edit.name) { fileRename = null; return; }
    edit.busy = true; edit.err = '';
    const dir = parentOf(edit.path);
    const target = sftpJoin(dir, name);
    try {
      const entries = await invoke<SftpEntry[]>('sftp_list', { connId: edit.connId, path: dir });
      if (entries.some(entry => entry.name === name)) throw new Error('同名文件或文件夹已存在，请换一个名字');
      await invoke('sftp_rename', { connId: edit.connId, from: edit.path, to: target });
      if (fileRename === edit) fileRename = null;
      if (sftpConnId === edit.connId) {
        if (sftpSel === edit.path) sftpSel = target;
        const prefix = `${edit.path}/`;
        sftpOpenDirs = new Set([...sftpOpenDirs].filter(path => path !== edit.path && !path.startsWith(prefix)));
        await loadDir(dir, true);
      }
    } catch (e) {
      if (fileRename === edit) { edit.err = String(e); edit.busy = false; }
      else say(String(e), 'err');
    }
  }
  $effect(() => {
    if (fileRename && fileRename.connId !== sftpConnId) fileRename = null;
  });
  async function confirmFsAsk() {
    if (!fsAsk || fsAsk.busy) return;
    const v = fsAsk.value.trim();
    if (!v) return;
    if (v.includes('/') || v.includes('\0') || v === '.' || v === '..') { fsAsk.err = '请输入有效名字，不能包含 /、空字符或使用 .、..'; return; }
    fsAsk.busy = true; fsAsk.err = '';
    const dir = fsAsk.dir;
    const connId = fsAsk.connId;
    try {
      if (fsAsk.mode === 'mkdir') await invoke('sftp_mkdir', { connId, path: sftpJoin(dir, v) });
      else await invoke('sftp_create_file', { connId, path: sftpJoin(dir, v) });
      fsAsk = null;
      if (sftpConnId === connId) await loadDir(dir, true);
    } catch (e) { if (fsAsk) { fsAsk.err = String(e); fsAsk.busy = false; } }
  }
  async function sftpDelete(r: SftpRow) {
    if (isSftpDeleting(r.path)) return;
    const connId = sftpConnId;
    const isDir = r.dir && !r.link;
    const yes = await confirmDialog(
      isDir ? `删除文件夹「${r.name}」及其中的全部内容？此操作不可撤销。` : `删除「${r.name}」？此操作不可撤销。`,
      { title: isDir ? '删除文件夹' : '删除文件', kind: 'danger', confirmText: isDir ? '删除全部' : '删除' },
    );
    if (!yes) return;
    const key = sftpDeletingKey(connId, r.path);
    sftpDeleting = new Set(sftpDeleting).add(key);
    try {
      await invoke('sftp_remove', { connId, path: r.path, dir: isDir });
      if (sftpConnId === connId) {
        const prefix = `${r.path.replace(/\/+$/, '')}/`;
        sftpOpenDirs = new Set([...sftpOpenDirs].filter((path) => path !== r.path && !path.startsWith(prefix)));
        const kids = { ...sftpKids };
        for (const path of Object.keys(kids)) if (path === r.path || path.startsWith(prefix)) delete kids[path];
        sftpKids = kids;
        await loadDir(parentOf(r.path), true);
      }
    } catch (e) { say(String(e), 'err'); }
    finally {
      const next = new Set(sftpDeleting);
      next.delete(key);
      sftpDeleting = next;
    }
  }
  function sftpDownloadSelected() {
    if (sftpBusy || sftpXfer || sftpNativeDragging) return;
    const row = sftpRows.find((row) => row.path === sftpSel);
    if (row && !isSftpDeleting(row.path)) void sftpDownload(row);
  }
  async function sftpDownload(r: SftpRow, archive = false) {
    if (sftpBusy || sftpXfer || sftpNativeDragging || isSftpDeleting(r.path)) return;
    const connId = sftpConnId;
    const directory = r.dir && !r.link;
    fctxMenu = null;
    sftpSel = r.path;
    sftpXfer = `选择 ${r.name} 的保存位置…`;
    let transferId = '';
    try {
      let local: string | null = null;
      if (directory && !archive) {
        const picked = await open({ directory: true, multiple: false, title: `选择「${r.name}」的本地保存目录` });
        local = typeof picked === 'string' ? picked : null;
      } else {
        local = await saveDialog({ defaultPath: archive ? `${r.name}.tar.gz` : r.name, title: `下载 ${r.name}（同名文件不会覆盖）` });
      }
      if (!local) return;
      const id = crypto.randomUUID();
      transferId = id;
      addSftpTransfer(id, r.name, 'download', connId, r.path);
      sftpDownloadTask = { id, name: r.name, drag: false, started: false, cancelling: false, progress: null };
      sftpXfer = `下载 ${r.name}…`;
      const onProgress = new Channel<DownloadProgress>();
      onProgress.onmessage = (progress) => {
        if (sftpDownloadTask?.id === id) sftpDownloadTask = { ...sftpDownloadTask, started: true, progress };
        progressSftpTransfer(id, progress);
      };
      const result = await invoke<DownloadResult>('sftp_download_entry', {
        connId, remote: r.path, local, directory, archive, prepareDrag: false, transferId: id, onProgress,
      });
      finishSftpTransfer(id, 'completed', { localPath: result.path, ...(result.skipped ? { error: `跳过 ${result.skipped} 个软链接或特殊文件；如需保留请使用打包下载` } : {}) });
      say(result.skipped ? `已下载到 ${result.path}；跳过 ${result.skipped} 个软链接或特殊文件，如需保留请使用打包下载` : `已下载到 ${result.path}`);
    }
    catch (e) {
      const cancelled = String(e).startsWith('下载已取消');
      if (transferId) finishSftpTransfer(transferId, cancelled ? 'cancelled' : 'failed', { error: String(e) });
      if (cancelled) say('已取消下载'); else say(String(e), 'err');
    }
    finally { sftpXfer = ''; sftpDownloadTask = null; }
  }
  async function cancelSftpDownload() {
    const task = sftpDownloadTask;
    if (!task?.started || task.cancelling) return;
    sftpDownloadTask = { ...task, cancelling: true };
    updateSftpTransfer(task.id, { cancelling: true });
    try { await invoke('sftp_cancel_download', { transferId: task.id }); }
    catch (e) {
      if (sftpDownloadTask?.id === task.id) sftpDownloadTask = { ...sftpDownloadTask, cancelling: false };
      updateSftpTransfer(task.id, { cancelling: false });
      say(String(e), 'err');
    }
  }
  async function onSftpDragStart(e: DragEvent, r: SftpRow) {
    e.preventDefault(); e.stopPropagation();
    if (sftpBusy || isSftpDeleting(r.path) || sftpXfer || sftpNativeDragging) return;
    const connId = sftpConnId;
    const id = crypto.randomUUID();
    sftpSel = r.path;
    addSftpTransfer(id, r.name, 'download', connId, r.path, undefined, 'dragging');
    sftpDownloadTask = { id, name: r.name, drag: true, started: false, cancelling: false, progress: null };
    sftpXfer = `拖到本机 · ${r.name}`;
    sftpNativeDragging = true;
    fctxMenu = null;
    const onProgress = new Channel<DownloadProgress>();
    onProgress.onmessage = (progress) => {
      if (sftpDownloadTask?.id === id) sftpDownloadTask = { ...sftpDownloadTask, started: true, progress };
      progressSftpTransfer(id, progress);
    };
    const onEvent = new Channel<SftpDragEvent>();
    onEvent.onmessage = (event) => {
      if (sftpDownloadTask?.id !== id) return;
      if (event.phase === 'dragging') return;
      sftpNativeDragging = false;
      if (event.phase === 'dropped') {
        sftpDownloadTask = { ...sftpDownloadTask, started: true };
        updateSftpTransfer(id, { status: 'running', phase: 'connecting' });
        sftpXfer = `下载 ${r.name}…`;
        return;
      }
      const cancelling = sftpDownloadTask.cancelling;
      sftpXfer = ''; sftpDownloadTask = null;
      finishSftpTransfer(id, event.phase === 'completed' ? 'completed' : event.phase === 'error' ? 'failed' : 'cancelled', { localPath: event.path, error: event.error });
      if (event.phase === 'completed') say(`已下载到 ${event.path || '目标文件夹'}`);
      else if (event.phase === 'error') say(event.error || '下载失败，请重试', 'err');
      else if (cancelling) say('已取消下载');
    };
    try {
      await invoke('sftp_drag_download', { connId, remote: r.path, directory: r.dir && !r.link, transferId: id, onProgress, onEvent });
    } catch (err) {
      if (sftpDownloadTask?.id !== id) return;
      sftpNativeDragging = false; sftpXfer = ''; sftpDownloadTask = null;
      finishSftpTransfer(id, 'failed', { error: String(err) });
      say(String(err), 'err');
    }
  }
  async function sftpUpload(dir = sftpPath) {
    if (sftpBusy || sftpXfer || sftpNativeDragging || !dir) return;
    const connId = sftpConnId;
    fctxMenu = null;
    sftpXfer = '选择要上传的文件…';
    let uploaded = 0;
    let failed = false;
    try {
      const picked = await open({ multiple: true, title: '选择要上传的文件' });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      for (const local of paths) {
        const name = localFileBasename(local);
        if (!name) throw new Error('本地文件名无效，未上传');
        await uploadSftpFile(connId, dir, name, local);
        uploaded++;
      }
    } catch (e) { failed = true; say(String(e), String(e).startsWith('上传已取消') ? undefined : 'err'); }
    finally {
      sftpXfer = '';
      if (uploaded && !failed) say(uploaded > 1 ? `已上传 ${uploaded} 个文件` : '已上传');
      if (sftpConnId === connId) await loadDir(dir, true);
    }
  }
  async function uploadSftpFile(connId: string, dir: string, name: string, local?: string, file?: File) {
    const id = crypto.randomUUID(), remote = sftpJoin(dir, name);
    addSftpTransfer(id, name, 'upload', connId, remote, local);
    sftpUploadTask = { id, started: false, cancelling: false };
    sftpXfer = `上传 ${name}…`;
    try {
      if (local) {
        const onProgress = new Channel<DownloadProgress>();
        onProgress.onmessage = progress => {
          if (sftpUploadTask?.id === id) sftpUploadTask = { ...sftpUploadTask, started: true };
          progressSftpTransfer(id, progress);
        };
        const result = await invoke<{ path: string; bytes: number; files: number }>('sftp_upload_entry', { connId, local, remote, transferId: id, onProgress });
        finishSftpTransfer(id, 'completed', { bytes: result.bytes, totalBytes: result.bytes, files: result.files, totalFiles: result.files });
      } else {
        if (!file) throw new Error('未取得本地文件');
        if (file.size > 25 * 1024 * 1024) throw new Error('此拖入方式上限为 25MB；请在「传输」中选择上传文件');
        updateSftpTransfer(id, { phase: 'uploading', totalFiles: 1 });
        const bytes = new Uint8Array(await file.arrayBuffer());
        await invoke('sftp_write', { connId, path: remote, b64: bytesToB64(bytes) });
        // WebView byte uploads do not expose per-chunk progress or cancellation.
        // Only record bytes once the backend confirms a completed write.
        finishSftpTransfer(id, 'completed', { bytes: file.size, totalBytes: file.size, files: 1, totalFiles: 1 });
      }
    } catch (e) {
      finishSftpTransfer(id, String(e).startsWith('上传已取消') ? 'cancelled' : 'failed', { error: String(e) });
      throw e;
    } finally { if (sftpUploadTask?.id === id) sftpUploadTask = null; }
  }
  /** Native local drops stream from verified paths; platforms without the bridge keep the byte fallback. */
  async function sftpUploadDropped(files: File[], dir = sftpPath, connId = sftpConnId, probe?: LocalDropProbe) {
    if (!files.length || !dir || sftpXfer || sftpBusy || sftpNativeDragging) return;
    let uploaded = 0;
    let failed = false;
    let attempted = false;
    sftpXfer = '准备上传…';
    try {
      let verifiedPaths: string[] | undefined;
      if (probe) {
        const captured = await probe.capture;
        if (captured) {
          if (captured.count !== files.length || probe.entries.length !== files.length) {
            await invoke('sftp_discard_local_drop', { token: captured.token }).catch(() => {});
            throw new Error('本地拖入项目不匹配，未上传');
          }
          const result = await invoke<{ paths: string[] }>('sftp_consume_local_drop', { token: captured.token, entries: probe.entries });
          if (result.paths.length !== files.length || result.paths.some((path, index) => localFileBasename(path) !== files[index].name)) {
            throw new Error('本地拖入项目不匹配，未上传');
          }
          verifiedPaths = result.paths;
        } else if (probe.entries.some(entry => entry.directory)) {
          throw new Error('无法读取此次拖入的本地文件夹，未上传任何文件');
        }
      }
      for (const [index, file] of files.entries()) {
        const local = verifiedPaths?.[index] ?? (probe ? undefined : (file as File & { path?: string }).path);
        const name = local ? localFileBasename(local) : file.name;
        if (!name || name === '.' || name === '..' || name.includes('/') || name.includes('\0')) throw new Error('本地文件名无效，未继续上传');
        attempted = true;
        await uploadSftpFile(connId, dir, name, local, local ? undefined : file);
        uploaded++;
      }
    } catch (e) {
      failed = true;
      say(String(e), String(e).startsWith('上传已取消') ? undefined : 'err');
    } finally {
      sftpXfer = '';
      if (uploaded && !failed) say(uploaded > 1 ? `已上传 ${uploaded} 个项目` : '已上传');
      // A cancelled upload can retain partial remote data. Refresh only its
      // original connection, even if the user switched servers during capture.
      if (attempted && sftpConnId === connId) await loadDir(dir, true);
    }
  }
  function onSftpDragOver(e: DragEvent) {
    if (!e.dataTransfer?.types?.includes('Files') || e.dataTransfer.types.includes('application/x-pilot-conversation')) return;
    e.preventDefault();
    const target = sftpDropTargetAt(e);
    e.dataTransfer.dropEffect = target ? 'copy' : 'none';
    sftpDropTarget = target;
    sftpFileOver = !!target;
    if (target) beginSftpLocalDrop();
    else discardSftpLocalDrop();
  }
  function onSftpDragLeave(e: DragEvent) {
    const next = e.relatedTarget;
    if (!(next instanceof Node) || !(e.currentTarget as HTMLElement).contains(next)) {
      sftpFileOver = false; sftpDropTarget = null;
      discardSftpLocalDrop();
    }
  }
  function onSftpDrop(e: DragEvent) {
    const target = sftpDropTargetAt(e);
    const files = Array.from(e.dataTransfer?.files ?? []);
    sftpFileOver = false; sftpDropTarget = null;
    if (!e.dataTransfer?.types.includes('Files') || e.dataTransfer.types.includes('application/x-pilot-conversation')) {
      discardSftpLocalDrop(); return;
    }
    e.preventDefault(); e.stopPropagation();
    if (!target || !files.length) { discardSftpLocalDrop(); return; }
    const entries = localDropEntries(e.dataTransfer, files);
    beginSftpLocalDrop(); // Also covers a very quick enter/drop without a dragover.
    const capture = sftpDropCapture ?? Promise.resolve(null);
    sftpDropCapture = null; // The accepted drop owns this token until consume.
    void sftpUploadDropped(files, target.remoteDir, target.connId, { capture, entries });
  }
  // 在线编辑器
  let edOpen = $state(false);
  let edPath = $state('');
  let edText = $state('');
  let edLoading = $state(false);
  let edSaving = $state(false);
  let edErr = $state('');
  let edReadOnly = $state(false);
  let edConnId = '';
  let edSession = 0;
  let edOriginalB64 = '';
  let edOriginal = $state('');
  let edFormat = $state(inspectEditorText(''));
  let edFull = $state(false);
  let edFormatting = $state(false);
  let edClosing = $state(false);
  let edSaved = $state(false);
  const edDirty = $derived(edText !== edOriginal);

  async function closeEdit(): Promise<boolean> {
    if (edSaving || edFormatting || edClosing) return false;
    edClosing = true;
    try {
      if (edDirty && !await confirmDialog('这个文件有尚未保存的修改。放弃后无法恢复，远端文件不会改变。', {
        title: '放弃文件修改？', confirmText: '放弃修改', cancelText: '继续编辑',
      })) return false;
      edOpen = false; ++edSession;
      return true;
    } finally { edClosing = false; }
  }

  async function sftpEdit(path: string) {
    const connId = sftpConnId;
    if (!connId || (edOpen && !await closeEdit())) return;
    const session = ++edSession;
    edConnId = connId; edOriginal = ''; edOriginalB64 = ''; edFull = false; edSaved = false;
    edFormatting = false;
    edOpen = true; edPath = path; edText = ''; edErr = ''; edReadOnly = false; edLoading = true;
    try {
      const b64 = await invoke<string>('sftp_read', { connId, path });
      if (session !== edSession) return;
      // fatal 模式：不是合法 UTF-8（二进制）直接抛 TypeError → 只给下载不给编辑
      const raw = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(b64ToBytes(b64));
      if (raw.includes('\0')) throw new TypeError('Binary file');
      edOriginalB64 = b64;
      edFormat = inspectEditorText(raw);
      edText = edOriginal = edFormat.text;
    } catch (e) {
      if (session !== edSession) return;
      edReadOnly = true;
      edErr = e instanceof TypeError ? '这是二进制文件，不能在线编辑；右键「下载」可以取回本地。' : String(e);
    } finally { if (session === edSession) edLoading = false; }
  }
  async function saveEdit() {
    if (!edOpen || edLoading || edSaving || edFormatting || edClosing || edReadOnly || !edDirty) return;
    edSaving = true; edErr = '';
    const connId = edConnId, path = edPath, text = edText;
    try {
      if (edFormat.mixed && !await confirmDialog(`这个文件混用了多种换行符。保存会统一为 ${edFormat.eol === '\r\n' ? 'CRLF' : edFormat.eol === '\r' ? 'CR' : 'LF'}，是否继续？`, {
        title: '确认换行方式', confirmText: '统一并保存', cancelText: '返回编辑',
      })) return;
      const current = await invoke<string>('sftp_read', { connId, path });
      if (current !== edOriginalB64) throw new Error('远端文件已被其他操作修改，已停止保存以免覆盖。请先复制保留本次修改，再关闭并重新打开文件进行核对。');
      const b64 = strToB64(serializeEditorText(text, edFormat));
      await invoke('sftp_write', { connId, path, b64 });
      edOriginal = text; edOriginalB64 = b64; edFormat.mixed = false; edSaved = true;
      say(`已保存 ${path}`);
      if (sftpConnId === connId) void loadDir(parentOf(path), true);
    } catch (e) { edErr = String(e); }
    finally { edSaving = false; }
  }
  function onEditKey(e: KeyboardEvent) {
    if (e.defaultPrevented || confirmState) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      void saveEdit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      if (edFull) edFull = false;
      else void closeEdit();
    }
  }

  $effect(() => {
    if (!edOpen) return;
    const unload = (event: BeforeUnloadEvent) => {
      if (edOpen && (edDirty || edSaving || edFormatting)) { event.preventDefault(); event.returnValue = ''; }
    };
    window.addEventListener('beforeunload', unload);
    // The main window closes to the tray (and keeps edits). Guard only windows
    // that actually close; otherwise the native tray handler hides the prompt.
    const unlisten = pilotWindowLabel !== 'main' ? getCurrentWindow().onCloseRequested(async event => {
      if (!edOpen || (!edDirty && !edSaving && !edFormatting)) return;
      event.preventDefault();
      if (await closeEdit()) await getCurrentWindow().close();
    }).catch(() => () => {}) : Promise.resolve(() => {});
    return () => { window.removeEventListener('beforeunload', unload); void unlisten.then(dispose => dispose()); };
  });

  // ---------- 连接 Cloudflare ----------
  const CF_TOKEN_URL = 'https://dash.cloudflare.com/profile/api-tokens';
  const CF_TOKEN_PERMISSIONS = [
    { key: 'page', type: 'edit' },
    { key: 'workers_scripts', type: 'edit' },
    { key: 'd1', type: 'edit' },
    { key: 'dns', type: 'edit' },
    { key: 'zone', type: 'read' },
  ] as const;
  const CF_DASH_URL = 'https://dash.cloudflare.com/';
  let cfOpen = $state(false);
  let cfToken = $state('');
  let cfVerifying = $state(false);
  let cfConnecting = $state(false);
  let cfErr = $state('');
  let cfAccounts = $state<{ id: string; name: string }[]>([]);
  let cfZones = $state<{ id: string; name: string }[]>([]);
  let cfAccountId = $state('');
  let cfName = $state('');
  let cfPermsOpen = $state(false);
  let cfUpdateId = $state('');
  function cfUpdateConnection(): Connection | null {
    return conns.find((connection) => connection.id === cfUpdateId) ?? null;
  }
  function cfTokenTemplateUrl(): string {
    const url = new URL(CF_TOKEN_URL);
    url.searchParams.set('permissionGroupKeys', JSON.stringify(CF_TOKEN_PERMISSIONS));
    url.searchParams.set('accountId', cfUpdateConnection()?.account_id || '*');
    url.searchParams.set('zoneId', 'all');
    url.searchParams.set('name', 'Cligc Pilot');
    return url.toString();
  }
  function resetCfConnect(updateId = '') {
    const updating = conns.find((connection) => connection.id === updateId);
    cfUpdateId = updating?.id ?? '';
    cfOpen = true; cfToken = ''; cfErr = ''; cfAccounts = []; cfZones = [];
    cfAccountId = updating?.account_id ?? '';
    cfName = updating?.name ?? '';
    cfPermsOpen = !!updating;
  }
  function openCfConnect() { switcherOpen = false; resetCfConnect(); }
  function openCfUpdate(c: Connection) { connCtx = null; switcherOpen = false; resetCfConnect(c.id); }
  function closeCfConnect() {
    if (cfConnecting || cfVerifying) return;
    cfOpen = false;
    cfUpdateId = '';
  }
  async function verifyCf() {
    if (!cfToken.trim()) return;
    cfVerifying = true; cfErr = '';
    try {
      const v = await invoke<{ token_status: string; accounts: { id: string; name: string }[]; zones: { id: string; name: string }[] }>('verify_cf_token', { token: cfToken.trim() });
      cfAccounts = v.accounts; cfZones = v.zones;
      const updating = cfUpdateConnection();
      if (updating) {
        cfAccountId = updating.account_id;
        cfName = updating.name;
        if (!v.accounts.some((account) => account.id === updating.account_id)) {
          cfErr = `Token 有效，但无法访问原账号 ${updating.account_id}。请为这条连接创建属于同一账号的 Token。`;
        }
      } else {
        if (v.accounts.length === 1) cfAccountId = v.accounts[0].id;
        if (!cfName.trim() && v.accounts.length) cfName = (v.accounts.find((a) => a.id === cfAccountId) ?? v.accounts[0]).name;
        if (!v.accounts.length) cfErr = 'Token 有效，但没有可用账号（需要 Account 级权限）。';
      }
    } catch (e) { cfErr = String(e); }
    finally { cfVerifying = false; }
  }
  function resetCfVerification() {
    cfAccounts = [];
    cfZones = [];
    cfErr = '';
    if (!cfUpdateId) cfAccountId = '';
  }
  async function confirmCf() {
    if (!cfAccountId) { cfErr = '请先选择一个账号'; return; }
    if (!cfAccounts.some((account) => account.id === cfAccountId)) { cfErr = '请先验证这枚 Token'; return; }
    cfConnecting = true; cfErr = '';
    try {
      const updating = cfUpdateConnection();
      const conn = updating
        ? await invoke<Connection>('update_cloudflare_token', { connId: updating.id, token: cfToken.trim() })
        : await invoke<Connection>('connect_cloudflare', { name: cfName.trim(), token: cfToken.trim(), accountId: cfAccountId });
      cfOpen = false; cfUpdateId = '';
      say(updating ? `已更新 Cloudflare 连接「${updating.remark?.trim() || conn.name}」` : `已连接 Cloudflare「${conn.name}」`);
      await refreshConns();
      await selectConn(conn.id);
    } catch (e) { cfErr = String(e); }
    finally { cfConnecting = false; }
  }

  // ---------- 模板库 ----------
  // builtin＝Pilot 随附的起始模板：删不掉（后端也拒），created_at 恒 0（列表里自然沉底）。
  type Template = { slug: string; name: string; desc: string; category: string; pages: number; created_at: number; builtin: boolean };
  let templates = $state<Template[]>([]);
  let templatesLoading = $state(false);
  let tmplHtml = $state<Record<string, string>>({}); // 每个模板的入口 HTML，做真实缩略图
  // 分类检索。随附模板的 category 由后端给；用户自己沉淀的没有分类（存模板时不问）——
  // 归到「自建」而不是硬塞进某一档，这既是实话，也正好是他们最想单独看的一组。
  const CAT_MINE = '自建';
  // 固定顺序：按出现顺序排的话，删一个模板就可能让整排筹码跳位。
  const CAT_ORDER = ['落地页', '内容', '作品集', '电商', '企业', '活动'];
  let tmplCat = $state('全部');
  const catOf = (t: Template) => (t.builtin ? t.category || '其他' : CAT_MINE);
  const tmplCats = $derived.by(() => {
    const has = new Set(templates.map(catOf));
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
  const tmplShown = $derived(templates.filter((t) => tmplCat === '全部' || catOf(t) === tmplCat));
  /** 模板文本索引：全局搜索按它匹配（名称 / 描述 / slug / 分类）。 */
  const tmplHay = (t: Template) => `${t.name} ${t.desc} ${t.slug} ${catOf(t)}`.toLowerCase();
  // 选中的分类可能整个消失（删掉最后一个自建模板）→ 那会停在一个永远空的筛选上，像坏了。
  $effect(() => {
    if (tmplCat !== '全部' && !tmplCats.includes(tmplCat)) tmplCat = '全部';
  });
  async function loadTemplates() {
    templatesLoading = true;
    try {
      templates = await invoke<Template[]>('list_templates');
      const map: Record<string, string> = {};
      await Promise.all(templates.map(async (t) => {
        try { map[t.slug] = await invoke<string>('template_index_html', { slug: t.slug }); } catch { map[t.slug] = ''; }
      }));
      tmplHtml = map;
    } catch (e) { say(String(e), 'err'); }
    finally { templatesLoading = false; }
  }
  function openTemplates() { view = 'templates'; activeConvId = ''; activeConv = null; loadTemplates(); }
  /** 全局搜索要搜模板，可 templates 本来只在模板库视图里才加载。这里**只补元数据**：
      loadTemplates 还会把 12 个模板的入口 HTML 全拉一遍（12 次 IPC × 几十 KB，用来做缩略图），
      为了搜索付这个代价不值当——缩略图等真进视图再说。 */
  async function ensureTemplateMeta() {
    if (activeConn?.kind !== 'cloudflare' || templates.length) return;
    try { templates = await invoke<Template[]>('list_templates'); } catch { /* 搜不到模板而已，别让它把搜索也带崩 */ }
  }
  /** 从全局搜索跳到某个模板卡：等它渲染出来再滚过去。
      openTemplates 里的 loadTemplates 是异步的，写死延时就是在赌它已经回来了——列表一慢就滚了个空。 */
  async function focusTemplate(slug: string) {
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
  async function delTemplate(t: Template) {
    const yes = await confirmDialog(`删除模板「${t.name}」？`, { title: '删除模板', kind: 'danger' });
    if (!yes) return;
    try { await invoke('delete_template', { slug: t.slug }); await loadTemplates(); } catch (e) { say(String(e), 'err'); }
  }
  async function previewTemplate(t: Template) {
    say('正在启动模板预览…（约 2 秒后打开预览窗）');
    try { const u = await invoke<string>('cf_preview_template', { slug: t.slug }); say(`模板预览已打开（${portOf(u)}）·关掉预览窗即停`); } catch (e) { say(String(e), 'err'); }
  }
  // 存为模板（从 CF 会话）
  let saveTmplOpen = $state(false); let saveTmplName = $state(''); let saveTmplDesc = $state(''); let saveTmplBusy = $state(false); let saveTmplErr = $state('');
  function openSaveTmpl() { if (!activeConv) return; saveTmplName = activeConv.site_slug; saveTmplDesc = ''; saveTmplErr = ''; saveTmplOpen = true; }
  async function confirmSaveTmpl() {
    if (!activeConv || !saveTmplName.trim()) return;
    saveTmplBusy = true; saveTmplErr = '';
    try {
      await invoke('save_as_template', { connId: activeConv.conn_id, project: activeConv.site_slug, name: saveTmplName.trim(), desc: saveTmplDesc.trim() });
      saveTmplOpen = false; say('已存为模板');
    } catch (e) { saveTmplErr = String(e); }
    finally { saveTmplBusy = false; }
  }
  // 用模板建站
  let useTmplOpen = $state(false); let useTmplSlug = $state(''); let useTmplName = $state(''); let useTmplProject = $state(''); let useTmplBusy = $state(false); let useTmplErr = $state('');
  function openUseTmpl(t: Template) { useTmplSlug = t.slug; useTmplName = t.name; useTmplProject = ''; useTmplErr = ''; useTmplOpen = true; }
  async function confirmUseTmpl() {
    if (!useTmplProject.trim()) { useTmplErr = '给新项目起个名'; return; }
    if (!isCfConn) { useTmplErr = '请先在左下角切到一个 Cloudflare 连接'; return; }
    useTmplBusy = true; useTmplErr = '';
    try {
      const cid = activeConnId;
      const proj = useTmplProject.trim();
      await invoke('use_template', { slug: useTmplSlug, connId: cid, project: proj });
      useTmplOpen = false;
      say(`已用模板「${useTmplName}」建好项目，接着描述你要的定制吧`);
      activeConvId = ''; activeConv = null; view = 'launcher';
      await selectConn(cid); // 刷新项目列表（会清空 lSite）
      lSite = proj;
    } catch (e) { useTmplErr = String(e); }
    finally { useTmplBusy = false; }
  }
  // ---------- 提示词库 ----------
  // 预设（内置只读）+ 用户自建（存 localStorage）。点一下把内容填进启动页对话框开始建站。
  let userPrompts = $state<Prompt[]>(loadUserPrompts());
  let allPrompts = $derived<Prompt[]>([...PRESET_PROMPTS, ...userPrompts]);
  function openPrompts() { view = 'prompts'; activeConvId = ''; activeConv = null; }
  function usePrompt(p: Prompt) {
    lDraft = p.body;
    view = 'launcher';
    activeConvId = ''; activeConv = null;
    say(isCfConn ? '已填入建站需求，起个项目名就能开始' : '已填入需求，选好站点就能开始');
  }
  async function copyPrompt(p: Prompt) {
    try { await navigator.clipboard.writeText(p.body); say('已复制提示词'); }
    catch { say('复制失败，请手动选择复制', 'err'); }
  }
  // 新增 / 编辑（只对用户自建的）
  let promptEditOpen = $state(false);
  let promptEditId = $state('');   // '' = 新增
  let promptEditTitle = $state('');
  let promptEditBody = $state('');
  function openNewPrompt() { promptEditId = ''; promptEditTitle = ''; promptEditBody = ''; promptEditOpen = true; }
  function openEditPrompt(p: Prompt) { promptEditId = p.id; promptEditTitle = p.title; promptEditBody = p.body; promptEditOpen = true; }
  function savePrompt() {
    const title = promptEditTitle.trim(); const body = promptEditBody.trim();
    if (!title || !body) return;
    if (promptEditId) {
      userPrompts = userPrompts.map((p) => (p.id === promptEditId ? { ...p, title, body } : p));
    } else {
      userPrompts = [...userPrompts, { id: newPromptId(), title, body, created_at: Date.now() }];
    }
    if (!saveUserPrompts(userPrompts)) { say('保存失败：本地存储写入异常', 'err'); return; } // 存不下就别假装成功、保持弹窗开着
    promptEditOpen = false;
  }
  async function deletePrompt(p: Prompt) {
    const yes = await confirmDialog(`删除提示词「${p.title}」？`, { title: '删除提示词', kind: 'danger' });
    if (!yes) return;
    userPrompts = userPrompts.filter((x) => x.id !== p.id);
    if (!saveUserPrompts(userPrompts)) say('删除已生效，但本地存储写入异常，重启后可能恢复', 'err');
  }

  async function removeConn(id: string, preserveMenu = false) {
    // 该连接下有对话正在跑一轮时不删：否则删掉会话行会让在途回合回写失败（「会话丢失」），子进程还在用已删的密钥/目录。
    const runningUnderConn = Object.values(running).includes(id);
    if (runningUnderConn || convos.some((c) => c.conn_id === id && c.status === 'running')) {
      say('该连接下有对话正在运行，请先点停止结束这一轮，再删除连接。', 'err');
      return;
    }
    if (preserveMenu) connCtxConfirmPinned = true;
    const yes = await confirmDialog('删除这个连接？连接工作区目录、钥匙串里的密钥，以及该连接下的所有对话都会一并删除。', { title: '删除连接', kind: 'danger' });
    if (preserveMenu) connCtxConfirmPinned = false;
    if (!yes) return;
    if (preserveMenu) connCtx = null;
    try {
      await invoke('remove_connection', { id });
      if (activeConv?.conn_id === id) { activeConv = null; activeConvId = ''; view = 'launcher'; }
      await refreshConns();
      await refreshConvos();
    } catch (e) { say(String(e), 'err'); }
  }
  // 一键授权：开系统终端跑官方 CLI 登录命令（浏览器 OAuth 在终端流程里完成，Pilot 绝不代输凭据），
  // 随后进入「等待授权…」：每 4s 静默重检 brains，logged_in 翻 true 即复位绿灯；120s 超时自动复位。
  let authWaiting = $state<Brain | ''>('');
  let authTimer: ReturnType<typeof setInterval> | undefined;
  let claudeTokenOpen = $state(false);
  let claudeTokenDraft = $state('');
  let claudeTokenSaving = $state(false);
  function stopAuthWait() { authWaiting = ''; if (authTimer) { clearInterval(authTimer); authTimer = undefined; } }
  async function authorize(b: Brain) {
    try {
      await invoke('open_brain_login', { brain: b });
      say('已打开终端，请在终端里完成登录——完成后这里自动变绿');
      authWaiting = b;
      const deadline = Date.now() + 120_000;
      if (authTimer) clearInterval(authTimer);
      authTimer = setInterval(() => {
        if (Date.now() > deadline) { stopAuthWait(); return; }
        void refreshBrains();
      }, 4000);
    } catch (e) { say(String(e), 'err'); }
  }
  $effect(() => {
    if (!authWaiting || !brains) return;
    const st = authWaiting === 'claude' ? brains.claude : authWaiting === 'codex' ? brains.codex : brains.grok;
    if (st.found && st.logged_in) {
      const wasClaude = authWaiting === 'claude';
      stopAuthWait(); say('授权完成，已就绪');
      if (wasClaude) void verifyClaude(true); // 新登录立即复验，别让旧的「失效」标记赖着
    }
  });
  async function saveClaudeToken() {
    const token = claudeTokenDraft.trim();
    if (!token || claudeTokenSaving) return;
    claudeTokenSaving = true;
    try {
      await invoke('save_claude_oauth_token', { token });
      claudeTokenDraft = '';
      claudeTokenOpen = false;
      // 后端已经用候选 Token 做过一次真实最小调用；不要在这里再计费验证第二次。
      claudeVerify = 'ok';
      claudeVerifyAt = Date.now();
      await refreshBrains();
      if (!brains?.claude.logged_in) {
        claudeTokenOpen = true;
        say('Claude CLI 未接受这个 Token，请确认由 claude setup-token 生成且复制完整', 'err');
        return;
      }
      say('Claude 订阅 Token 已验证并安全保存');
    } catch (e) {
      say(String(e), 'err');
    } finally {
      claudeTokenSaving = false;
    }
  }
  function closeClaudeToken() {
    if (claudeTokenSaving) return;
    claudeTokenDraft = '';
    claudeTokenOpen = false;
  }
  /** Claude 凭据深度验证：auth status 只读本地记录会说谎（桌面端换号后旧 token 已废
   *  它照样报已登录）——打开设置时真跑一次最小无头调用（30 分钟缓存；跑一次是一次
   *  最小 haiku 计费，所以只在设置面板/手动刷新触发，不进 4 秒轮询）。 */
  let claudeVerify = $state<'unknown' | 'checking' | 'ok' | 'bad'>('unknown');
  let claudeVerifyAt = 0;
  async function verifyClaude(force = false) {
    if (!brains?.claude.found || !brains.claude.logged_in) { claudeVerify = 'unknown'; return claudeVerify; }
    if (claudeVerify === 'checking') return claudeVerify;
    if (!force && Date.now() - claudeVerifyAt < 30 * 60_000) return claudeVerify;
    claudeVerify = 'checking';
    try {
      const ok = await invoke<boolean>('brain_verify_claude');
      claudeVerify = ok ? 'ok' : 'bad';
    } catch {
      claudeVerify = 'unknown'; // 网络/档位类异常：不冤枉凭据，也不背书
    }
    claudeVerifyAt = Date.now();
    return claudeVerify;
  }
  $effect(() => { if (setupOpen) void verifyClaude(); });

  // ---------- Claude CLI 版本提示 + 一键升级 ----------
  // 只在设置面板打开时查一次（要一次 npm registry 请求），不在启动路径上加网络往返。
  let cliUpd = $state<CliUpdateInfo | null>(null);
  let cliUpdBusy = $state(false);
  let cliUpgrading = $state(false);
  async function checkCliUpdate() {
    if (cliUpdBusy) return;
    cliUpdBusy = true;
    try { cliUpd = await invoke<CliUpdateInfo>('check_claude_update'); }
    catch { cliUpd = null; } // 查不到（离线/无 npm）就当没有提示，别拿红字骚扰
    finally { cliUpdBusy = false; }
  }
  $effect(() => { if (setupOpen && brains?.claude.found && !cliUpd) void checkCliUpdate(); });
  async function upgradeCli() {
    if (!cliUpd || cliUpgrading) return;
    // 会改本机环境 → 必须先确认，并把「升的是哪一个」摆明：多安装时只有 PATH 第一个生效。
    const multi = cliUpd.all.length > 1
      ? `\n\n注意：PATH 上有 ${cliUpd.all.length} 份 claude，只有第一个会被真正调用：\n${cliUpd.all.map((p, i) => `${i === 0 ? '→ ' : '   '}${p}`).join('\n')}`
      : '';
    const yes = await confirmDialog(
      `将对 ${cliUpd.target} 执行 claude update，从 ${cliUpd.installed || '未知'} 升到 ${cliUpd.latest}。${multi}`,
      { title: '升级 Claude Code CLI', kind: 'warning' },
    );
    if (!yes) return;
    cliUpgrading = true;
    try {
      const out = await invoke<string>('upgrade_claude_cli');
      say(out.trim() ? out.trim().split('\n').slice(-2).join(' · ') : '升级完成');
      cliUpd = null;
      await refreshBrainsManual(); // 重探版本，UI 立刻反映新版本与 fast 可用性
      void checkCliUpdate();
    } catch (e) { say(String(e), 'err'); }
    finally { cliUpgrading = false; }
  }
  /** 重新绑定：先 headless 退出当前账号，再走原有授权终端登录新账号（等待轮询复用 authorize）。 */
  async function rebind(b: Brain) {
    try {
      await invoke('brain_logout', { brain: b });
    } catch (e) { say(String(e), 'err'); return; }
    say('已退出当前账号');
    void refreshBrains();
    await authorize(b);
  }

  // ---------- 对话导航 ----------
  function newChat() {
    ++openConvSeq;
    activeConvId = ''; activeConv = null; lDraft = '';
    draft='';attachments=[];
    if (isWorkspaceChat) lPerm = 'full';
    // 远程连接没有启动页：新对话＝把底部对话面板腾空（工作台留在原地，终端不断）。
    // 它不换「页面」，所以必须给点看得见的反馈：把面板打开并把光标放进输入框。
    viewAfterConvGone();
    if (isSshConn) {
      if (!wb.chat) setWbFlags({ chat: true });
      requestAnimationFrame(() => wbStartEl?.focus());
    }
    if (!lBrain.startsWith('api:') && !brainUsable(lBrain)) lBrain = firstUsableBrain();
    if (lBrain.startsWith('api:') && !brainUsable(lBrain)) say(`${brainAvailability(lBrain)}，请在设置中检查 API 服务或手动选择其他模型。`, 'err');
  }
  async function openConv(id: string) {
    const request=++openConvSeq;
    const c = await invoke<Conversation | null>('get_conversation', { id });
    if(request!==openConvSeq)return;
    if (!c) { await refreshConvos(); return; }
    // Resolve the workspace BEFORE taking the split-pane fast path. Otherwise an old
    // pane survives when a skill/search result opens in a different workspace.
    switchConversationWorkspace(conversationWorkspace(c));
    // 打开的对话可能属于别的连接（从搜索进来）——先切到它所属的连接。
    if (c.conn_id !== activeConnId) void selectConn(c.conn_id);
    if (conversationLayout) {
      rememberSplit(running[id] ? splitConversation(id) ?? c : c);
      void loadPaneMetadata(c);
      const existing=leaves(conversationLayout).find(p=>p.conversation===id);
      if(existing)focusSplit(existing.id);
      else {
        const target=leaves(conversationLayout).find(p=>p.id===splitFocused)??leaves(conversationLayout)[0];
        conversationLayout=replacePane(conversationLayout,target.id,id);focusSplit(target.id);
      }
      view='thread';
      return;
    }
    activeConv = c; activeConvId = id; threadModel = c.model; threadPerm = c.perm_mode || 'full'; threadEffort = c.effort || '';
    // 远程连接的对话在工作台里开，不跳去独立对话页。
    // 命令行开着就让它开着 —— 面板开关是用户自己拨的，翻个旧对话不该替他改布局。
    if (conns.find((x) => x.id === c.conn_id)?.kind === 'ssh') {
      view = 'remote';
      if (!wb.chat) setWbFlags({ chat: true });
    } else view = 'thread';
    attachments = []; queued = null; // 换会话清掉未发送的附件 / 等待消息
    expandSite(conversationGroupKey(c));
    checkCfReady();
    scrollSoon(true);
  }

  async function deleteConv(id: string) {
    // 对话进行中不删：否则删掉会话行会孤儿掉后台在跑的 CLI 子进程 + 触发「会话丢失」。先停止再删。
    if (running[id]) { say('对话进行中，请先点停止再删除。', 'err'); return; }
    const c = convos.find((x) => x.id === id);
    const label = c?.title?.trim() ? `「${c.title.trim()}」` : '这条对话';
    const yes = await confirmDialog(`删除对话${label}？聊天记录不可恢复。`, { title: '删除对话', kind: 'danger' });
    if (!yes) return;
    try {
      await invoke('delete_conversation', { id });
      const leaf=leaves(conversationLayout).find(p=>p.conversation===id);
      if(leaf)closeSplit(leaf.id);
      delete splitSnapshots[id];delete splitDrafts[id];delete splitQueued[id];delete splitErrors[id];
      if (activeConvId === id) { activeConvId = ''; activeConv = null; viewAfterConvGone(); }
      await refreshConvos();
    } catch (e) { say(String(e), 'err'); }
  }

  // ---------- 运行一轮 ----------
  function makeChannel(convId: string): Channel<TurnEvent> {
    const ch = new Channel<TurnEvent>();
    ch.onmessage = (ev) => {
      const buf = lives[convId];
      if (!buf) return; // 该对话已结束/被清（切走后仍可能收到尾包）
      if (ev.type === 'delta' || ev.type === 'text_reset') {
        // 恢复一次被截断的响应时，只撤回该响应的文字；此前的正文、工具和活动保留。
        buf.text = ev.type === 'text_reset' ? ev.text : buf.text + ev.text;
        // Markdown 的 {@html} 会替换其整棵子树。若直接在模板中调用 mdRender，
        // 生图/推理等 activity 更新也可能让正文跟着反复失效，形成可见闪空。
        // 把渲染结果缓存到正文事件上，任务卡更新时该字段保持引用和值都不变。
        buf.renderedText = mdRender(buf.text);
        buf.lastActivityAt = Date.now();
        if (activeConvId === convId) scrollSoon();
      }
      else if (ev.type === 'tool') {
        buf.tools = [...buf.tools, { label: ev.label, detail: ev.detail }];
        buf.lastActivityAt = Date.now();
        if (activeConvId === convId) scrollSoon();
      }
      else if (ev.type === 'activity') {
        const now = Date.now();
        const previous = buf.activities[ev.activity_id];
        buf.activities = {
          ...buf.activities,
          [ev.activity_id]: {
            id: ev.activity_id,
            label: ev.label || previous?.label || '',
            detail: ev.detail || previous?.detail || '',
            status: ev.status,
            error: ev.error || previous?.error || '',
            kind: ev.kind || previous?.kind,
            phase: ev.phase || previous?.phase,
            current: ev.current ?? previous?.current,
            total: ev.total ?? previous?.total,
            startedAt: previous?.startedAt ?? now,
            updatedAt: now,
          },
        };
        buf.lastActivityAt = now;
        if (activeConvId === convId) scrollSoon();
      }
      else if (ev.type === 'context_compacted') {
        buf.compactedAt = Date.now();
        buf.compactedPreTokens = ev.pre_tokens;
        if (activeConvId === convId) {
          say('Claude 已自动整理上下文，对话记录保持不变');
          scrollSoon();
        }
      }
      else if (ev.type === 'done') { if (!ev.ok) { buf.error = ev.error; buf.failed = true; } }
    };
    return ch;
  }
  function optimisticUser(text: string): Message {
    return { role: 'user', text, tools: [], ts: Math.floor(Date.now() / 1000), hidden: false, error: false };
  }
  // 乐观：立刻把用户消息塞进当前/合成会话，activeConvId 立即等于 convId，
  /** 当前对话没了（删掉/丢失/新建）之后该落到哪儿。
   *  ★ 远程连接永远留在工作台（底部面板自己退回起点输入框）——**别在各处硬写 view='launcher'**，
   *  那会把人踢出工作台、终端从眼前消失。ssh 连接压根没有启动页。 */
  function viewAfterConvGone() { view = isSshConn ? 'remote' : 'launcher'; }
  // 首轮也能流式渲染 + 停止（cancel 键对准真正的注册表 key）。
  function beginTurn(convId: string, optimistic: Conversation) {
    if (retryTimers[convId]) { clearTimeout(retryTimers[convId]); delete retryTimers[convId]; } // 任何新一轮开始都取消该会话待触发的自动重连
    const startedAt = Date.now();
    lives[convId] = {
      text: '',
      renderedText: '',
      tools: [],
      activities: {},
      lastActivityAt: startedAt,
      error: '',
      failed: false,
      startedAt,
    };
    running[convId] = optimistic.conn_id;
    if(splitSnapshots[convId]) {
      rememberSplit(optimistic);
      convos=convos.map(c=>c.id===convId?optimistic:c);
      if(conversationLayout || activeConvId!==convId) return;
    }
    activeConv = optimistic; activeConvId = convId; threadModel = optimistic.model; threadPerm = optimistic.perm_mode || 'full'; threadEffort = optimistic.effort || '';
    cfReady = false; // 本轮跑完再重新判定是否已建出内容
    // 远程连接：对话就在工作台底部面板里跑，别跳去独立对话页——那样终端就从眼前消失了。
    // （openConv/newChat/selectConn 同理，见各自注释。这条是「发消息新建对话」这一路。）
    if (conns.find((c) => c.id === optimistic.conn_id)?.kind === 'ssh') {
      view = 'remote';
      if (!wb.chat) setWbFlags({ chat: true });
    } else {
      view = 'thread';
    }
    scrollSoon(true);
  }
  function endTurn(conv: Conversation | null, convId: string) {
    const failed = lives[convId]?.failed ?? false; // 删缓冲前先取，供等待消息门控用
    const completedTurnStartedAt = lives[convId]?.startedAt ?? 0;
    // 仅当用户仍停留在这条会话时用权威结果覆盖，避免打断已切到别的会话的用户。
    // 覆盖源按新鲜度择优：运行中改的档位/模型晚于轮末快照落库，刷新过的列表条目更全；
    // 但 refreshConvos 失败时列表是旧值（甚至 startChat 的乐观条目），无脑取列表会把
    // 刚跑完的整轮消息打回去——updated_at 更旧就回退用快照。
    if (conv) {
      const inList = convos.find((x) => x.id === convId);
      // 时间戳精度是秒：列表旧快照与轮末结果可能 updated_at 相同。相同时消息更多的才是真正的新版本。
      const fresh = !inList ? conv
        : inList.updated_at > conv.updated_at ? inList
          : inList.updated_at < conv.updated_at ? conv
            : inList.messages.length > conv.messages.length ? inList : conv;
      if (activeConvId === convId) { activeConv = fresh; threadModel = fresh.model; threadPerm = fresh.perm_mode || 'full'; threadEffort = fresh.effort || ''; }
      if(splitSnapshots[convId]) rememberSplit(fresh);
      convos = convos.some((item) => item.id === fresh.id)
        ? convos.map((item) => item.id === fresh.id ? fresh : item)
        : [fresh, ...convos];
    }
    delete running[convId];
    delete lives[convId];
    finishSplitQueue(convId, conv, failed, completedTurnStartedAt);
    if (activeConvId === convId) {
      checkCfReady(); // 这轮可能写了文件，重新判定预览/部署是否可用
      scrollSoon(); // 流式缓冲换成权威消息后高度还会变，贴底状态要延续。
    }
    // 等待消息：这轮**成功**结束、用户还在本会话，才把排队的那条发出去（失败/要重连时不发，避免连环失败）。
    if (!failed && queued && queued.convId === convId && activeConvId === convId) {
      const q = queued;
      const lastUserText = [...(conv?.messages ?? activeConv?.messages ?? [])]
        .reverse()
        .find((message) => message.role === 'user')?.text.trim() ?? '';
      const staleQueue = !completedTurnStartedAt || q.turnStartedAt !== completedTurnStartedAt;
      const duplicateQueue = queuedPayloadText(q).trim() === lastUserText;
      queued = null;
      if (staleQueue || duplicateQueue) {
        // 排队项必须来自刚结束的这一轮，并且不能与该轮原始请求完全相同。
        // 这同时挡住 HMR/旧状态残留和完成后的原消息重放。
        if (activeConvId === convId) say('已拦截重复的等待消息，未再次执行');
      } else {
        draft = q.text; attachments = q.atts;
        queueMicrotask(() => { void send(); });
      }
    }
    // 不在接近上限时自动 rebuild：rebuild 会创建全新的底层 Claude session，只能靠摘要
    // 衔接，等同于主动丢弃原生会话上下文。Claude Code 会自行管理长会话；若底层真的返回
    // 上下文超限，保留完整界面历史并让用户从错误卡明确选择「重建继续」。
  }
  async function failTurn(e: unknown, convId: string) {
    if(splitSnapshots[convId]) {splitErrors[convId]=String(e);restoreSplitQueue(convId);}
    delete running[convId];
    delete lives[convId];
    // 这轮失败：不自动发排队消息，把它放回输入框让用户决定。
    if (queued && queued.convId === convId && activeConvId === convId) { draft = queued.text; attachments = queued.atts; queued = null; }
    // 仅当用户仍在看这条会话时才弹错误提示——后台会话的失败不打断前台（成功也不弹，见 endTurn），
    // 失败态已落库，切回该会话即可看到。
    if (activeConvId === convId) say(String(e), 'err');
    await refreshConvos();
    const reloaded = await invoke<Conversation | null>('get_conversation', { id: convId });
    if(reloaded && splitSnapshots[convId])rememberSplit(reloaded);
    if (reloaded) { if (activeConvId === convId) activeConv = reloaded; }
    else if (activeConvId === convId) { activeConv = null; viewAfterConvGone(); }
    if (activeConvId === convId) scrollSoon();
  }

  async function startChat() {
    const workspace = isWorkspaceChat;
    const targetReady = isSshConn || workspace || !!lSite.trim();
    const textReady = !!lDraft.trim() || (isSshConn && attachments.length > 0);
    if (!activeConn || !targetReady || !textReady || !brainUsable(lBrain)) return;
    const taskType: TaskType = isSshConn ? 'remote' : isCfConn ? 'sitebuild' : 'workspace';
    prefs.brain = lBrain; prefs.model = lModel; prefs.perm = lPerm; prefs.effort = lEffort; savePrefs(prefs);
    const model = lModel;
    // CF 建站：把所选视觉风格的 tokens 指令拼进首条消息（可见、可追溯）。
    const styleDir = isCfConn && lStyle ? STYLE_DIRECTIVES[lStyle] : '';
    let text = lDraft.trim() + (styleDir ? `\n\n【视觉风格】${styleDir}` : '');
    if (attachments.length) {
      text += (text ? '\n\n' : '') + ATT_MARKER + '\n' + attachments.map((a) => `- ${a.path}`).join('\n');
    }
    const id = crypto.randomUUID();
    const now = Math.floor(Date.now() / 1000);
    // 远程连接没有项目：slug 留空，侧栏分组名用 user@host（那就是这台机器的身份）。
    const mSlug = workspace || isSshConn ? '' : lSite;
    const mName = isSshConn ? `${activeConn.ssh_user}@${activeConn.ssh_host}` : workspace ? (workspaceDir ? workspaceFolderName(workspaceDir) : '自由对话') : lSite;
    const conversationConnId = activeConnId;
    const optimistic: Conversation = {
      id, conn_id: conversationConnId, conn_name: activeConn.name, site_slug: mSlug, site_name: mName,
      workspace_dir: workspace ? workspaceDir : '',
      task_type: taskType, brain: lBrain, model, perm_mode: lPerm, effort: lEffort, session_ref: '',
      title: text.slice(0, 30), messages: [optimisticUser(text)], status: 'running', created_at: now, updated_at: now,
    };
    // 立刻塞进侧栏，这样即便用户随后切走/新开对话，这条新会话也带着 running 圈可见，不必等这一轮跑完才出现。
    convos = [optimistic, ...convos];
    delete autoRetried[id];
    beginTurn(id, optimistic);
    lDraft = ''; attachments = [];
    try {
      const conv = await invoke<Conversation>('start_conversation', {
        convId: id, connId: conversationConnId, siteSlug: mSlug, siteName: mName,
        taskType, brain: lBrain, model, permMode: lPerm, effort: lEffort, fast: lFast,
        workspaceDir: workspace ? workspaceDir : '',
        message: text, onEvent: makeChannel(id),
      });
      await refreshConvos();
      const failed = lives[id]?.failed ?? false; const errText = lives[id]?.error ?? '';
      endTurn(conv, id);
      if (!failed && workspace) workspaceDir = '';
      maybeAutoRetry(id, failed, errText);
    } catch (e) { await failTurn(e, id); }
  }

  // ---------- 附件：粘贴/拖拽文件到输入框，存进项目目录供 AI 读取 ----------
  type Attach = { name: string; path: string; preview: string };
  // 发消息时把这段追加到正文末尾让 AI 拿到路径；渲染时再从正文里拆出来单独做成附件卡（见 splitAttachments）。
  const ATT_MARKER = '附件（已存在项目里，可直接读取使用）：';
  // 助手消息里由后端补的 codex 生图产物清单（agent.rs append_generated_images），渲染成缩略图。
  const GEN_MARKER = '生成的图片：';
  // 从消息正文里拆出「真正的文字」和「路径列表」——存储的是完整正文，气泡里显示干净文字 + 图卡。
  function splitMarked(text: string, marker: string): { body: string; atts: string[] } {
    let cut = text.indexOf('\n\n' + marker);
    let sep = 2;
    if (cut < 0) {
      if (text.startsWith(marker)) { cut = 0; sep = 0; }
      else return { body: text, atts: [] };
    }
    const body = text.slice(0, cut);
    const atts = text.slice(cut + sep).split('\n').slice(1).map((l) => l.replace(/^-\s*/, '').trim()).filter(Boolean);
    return { body, atts };
  }
  function splitAttachments(text: string) { return splitMarked(text, ATT_MARKER); }
  function splitGenImages(text: string) { return splitMarked(text, GEN_MARKER); }
  function isImgPath(p: string): boolean { return /\.(png|jpe?g|gif|webp|svg|bmp|avif)$/i.test(p); }
  // 图片附件缩略图：消息里存的是相对工作目录的 uploads/ 路径，webview 读不了本地文件，
  // 走后端按「连接+项目」解析后读成 data URI；缓存键带上二者，避免不同会话同名文件串图。
  // data URI 可观（≤8MB 原图 → ~10MB 字符串），切会话时把其他会话的条目逐出，防止只增不减；
  // 顺带让上次读失败（''）的条目在重进会话时有重试机会。
  let thumbs = $state<Record<string, string>>({});
  const thumbJobs = new Map<string, Promise<string>>();
  function conversationProject(c: Conversation): string { return c.task_type === 'workspace' ? c.id : c.site_slug; }
  function thumbKey(p: string, c:Conversation|null=activeConv): string { return c ? c.conn_id + '|' + conversationProject(c) + '|' + p : '||' + p; }
  // 读工作目录内图片 → data URI（并发去重；失败缓存空串＝退回文件卡样式）。
  function loadWorkdirImg(connId: string, project: string, p: string): Promise<string> {
    const k = connId + '|' + project + '|' + p;
    if (thumbs[k] !== undefined) return Promise.resolve(thumbs[k]);
    let j = thumbJobs.get(k);
    if (!j) {
      j = invoke<string>('read_workdir_image', { connId, project, path: p })
        .then((d) => { thumbs = pruneThumbs({ ...thumbs, [k]: d }); return d; })
        .catch(() => { thumbs = { ...thumbs, [k]: '' }; return ''; })
        .finally(() => { thumbJobs.delete(k); });
      thumbJobs.set(k, j);
    }
    return j;
  }
  // 悬停/放大会把任意工作目录图片读进缓存：软上限 80 条，超了丢最早的（对象键序＝插入序），
  // 防止单会话里长期翻聊天记录把 data URI 越积越多。
  function pruneThumbs(t: Record<string, string>): Record<string, string> {
    const keys = Object.keys(t);
    if (keys.length <= 80) return t;
    for (const kk of keys.slice(0, keys.length - 80)) delete t[kk];
    return t;
  }
  function ensureThumb(connId: string, project: string, p: string) { void loadWorkdirImg(connId, project, p); }
  $effect(() => {
    const contexts = splitVisible
      ? leaves(conversationLayout).map(p=>splitConversation(p.conversation)).filter((c):c is Conversation=>!!c)
      : activeConv ? [activeConv] : [];
    const prefixes=contexts.map(c=>c.conn_id+'|'+conversationProject(c)+'|');
    const stale = Object.keys(thumbs).filter((k) => !prefixes.some(prefix=>k.startsWith(prefix)));
    if (stale.length) { const keep = { ...thumbs }; for (const k of stale) delete keep[k]; thumbs = keep; }
    const images=contexts.flatMap(c=>c.messages.flatMap(m=> {
      const atts=m.role==='user'?splitAttachments(m.text).atts:splitGenImages(m.text).atts;
      return atts.filter(isImgPath).map(p=>({c,p}));
    })).slice(-80);
    for(const {c,p} of images)ensureThumb(c.conn_id,conversationProject(c),p);
  });
  // 点缩略图看大图（点任意处/Esc 关闭）
  let lightbox = $state('');
  let attachments = $state<Attach[]>([]);
  let attaching = $state(false);
  let attachingCount = 0;
  let composerFileOver = $state(false);
  function attachmentTarget(): { connId: string; project: string } | null {
    // 远程工作台的首条消息还没有 Conversation，但连接本身已经有本地工作目录。
    // 进入已有对话后仍以该对话为准；避免把另一个连接残留的 activeConv 当成当前目标。
    if (activeConv && (view !== 'remote' || activeConv.conn_id === activeConnId)) {
      return { connId: activeConv.conn_id, project: conversationProject(activeConv) };
    }
    if (view === 'remote' && isSshConn && activeConnId) return { connId: activeConnId, project: '' };
    return null;
  }
  function attachmentFilename(f: File): string {
    if (f.name.trim()) return f.name.trim();
    const ext = ({ 'image/png': 'png', 'image/jpeg': 'jpg', 'image/gif': 'gif', 'image/webp': 'webp' } as Record<string, string>)[f.type] || 'bin';
    return `clipboard-${Date.now()}.${ext}`;
  }
  async function attachFile(f: File) {
    const target = attachmentTarget();
    if (!target) { say('先进入一个对话再添加文件', 'err'); return; }
    if (f.size > 25 * 1024 * 1024) { say('文件太大（上限 25MB）', 'err'); return; }
    const filename = attachmentFilename(f);
    attachingCount++;
    attaching = true;
    try {
      const buf = new Uint8Array(await f.arrayBuffer());
      const path = await invoke<string>('save_attachment', { connId: target.connId, project: target.project, filename, data: Array.from(buf) });
      let preview = '';
      if (f.type.startsWith('image/') || isImgPath(filename)) {
        preview = await new Promise<string>((res) => { const r = new FileReader(); r.onload = () => res(String(r.result)); r.onerror = () => res(''); r.readAsDataURL(f); });
      }
      attachments = [...attachments, { name: path.split('/').pop() || filename, path, preview }];
    } catch (e) { say(String(e), 'err'); }
    finally { attachingCount--; attaching = attachingCount > 0; }
  }
  function removeAttachment(i: number) { attachments = attachments.filter((_, idx) => idx !== i); }

  // 等待消息：这轮还在跑时先把下一条排进队列，等回合结束自动发；发出前可编辑/清除。绑定 convId，切走即清。
  type QueuedMessage = {
    convId: string;
    text: string;
    atts: Attach[];
    /** 排队动作所属的运行轮；轮末只消费同一轮创建的消息。 */
    turnStartedAt: number;
  };
  let queued = $state<QueuedMessage | null>(null);
  function queuedPayloadText(message: QueuedMessage): string {
    if (!message.atts.length) return message.text;
    return message.text
      + (message.text ? '\n\n' : '')
      + ATT_MARKER + '\n'
      + message.atts.map((attachment) => `- ${attachment.path}`).join('\n');
  }
  function queueMessage() {
    if (!activeConv) return;
    if (!draft.trim() && !attachments.length) return;
    if (!running[activeConv.id]) { void send(); return; } // 其实没在跑就直接发
    const t = draft.trim();
    const turnStartedAt = lives[activeConv.id]?.startedAt ?? Date.now();
    // 已有排队则合并进同一条（不覆盖丢字）；否则新建。
    queued = queued && queued.convId === activeConv.id
      ? {
          convId: activeConv.id,
          text: [queued.text, t].filter(Boolean).join('\n'),
          atts: [...queued.atts, ...attachments],
          turnStartedAt,
        }
      : { convId: activeConv.id, text: t, atts: attachments, turnStartedAt };
    draft = ''; attachments = [];
  }
  function editQueued() {
    if (!queued) return;
    draft = queued.text; attachments = queued.atts; queued = null;
  }
  function clearQueued() { queued = null; }
  function onComposerPaste(e: ClipboardEvent, paneId='') {
    const data = e.clipboardData;
    if (!data) return;
    const files = Array.from(data.files);
    if (!files.length) {
      for (const item of Array.from(data.items)) {
        if (item.kind !== 'file') continue;
        const file = item.getAsFile();
        if (file) files.push(file);
      }
    }
    const handled = files.length > 0;
    const c=paneId?splitConversation(paneId):null;
    for (const file of files) {
      if(c)void splitUpload(c,file).catch(error=>{splitErrors[c.id]=String(error);});
      else void attachFile(file);
    }
    if (handled) e.preventDefault();
  }
  function onComposerDrop(e: DragEvent) {
    composerFileOver = false;
    const files = e.dataTransfer?.files;
    if (files && files.length) { e.preventDefault(); for (const f of Array.from(files)) void attachFile(f); }
  }
  function onComposerDragOver(e: DragEvent) {
    if (!e.dataTransfer?.types?.includes('Files')) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
    composerFileOver = true;
  }
  function onComposerDragLeave(e: DragEvent) {
    const next = e.relatedTarget;
    if (!(next instanceof Node) || !(e.currentTarget as HTMLElement).contains(next)) composerFileOver = false;
  }

  // ---------- 重试 / 自动重连 ----------
  // 错误文本像不像"瞬时问题"（网络/限流/过载/5xx）——只对这类做自动重试，避免对密钥错误/取消等硬错误反复重跑。
  function isTransient(err: string): boolean {
    if (!err || /已停止/.test(err)) return false;
    return /(网络|连接|超时|无法访问|timed?\s*out|timeout|overload|rate.?limit|too many requests|\b429\b|\b5\d\d\b|ECONN| ENET|EAI_AGAIN|socket hang|connection (reset|refused|closed|error)|temporar|try again|network error|fetch failed)/i.test(err);
  }
  // 重试上一轮：去掉失败的助手消息、用最后一条用户消息 resume 再跑（不新增用户气泡）。manual=用户手点（重置自动预算）。
  // ---------- 额度耗尽卡：倒计时 + 到点自动续跑 ----------
  let limitAuto = $state<Record<string, number>>({}); // convId → 触发时刻 ms
  let nowTick = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => {
      nowTick = Date.now();
      for (const [id, at] of Object.entries(limitAuto)) {
        if (nowTick >= at && !running[id]) {
          const rest = { ...limitAuto };
          delete rest[id];
          limitAuto = rest;
          void retry(id, false);
        }
      }
    }, 1000);
    return () => clearInterval(t);
  });
  function armLimitAuto(convId: string, resetSec: number) {
    limitAuto = { ...limitAuto, [convId]: resetSec * 1000 + 90_000 }; // 官方重置后再缓 90 秒
  }
  function disarmLimitAuto(convId: string) {
    const rest = { ...limitAuto };
    delete rest[convId];
    limitAuto = rest;
  }
  function fmtClock(sec: number): string {
    const d = new Date(sec * 1000);
    const hm = String(d.getHours()).padStart(2, '0') + ':' + String(d.getMinutes()).padStart(2, '0');
    return new Date().toDateString() === d.toDateString() ? hm : `${d.getMonth() + 1}/${d.getDate()} ${hm}`;
  }
  function fmtRemain(ms: number): string {
    const m = Math.ceil(ms / 60000);
    if (m >= 60) return `${Math.floor(m / 60)} 小时 ${m % 60} 分`;
    if (m > 1) return `${m} 分钟`;
    return '不到 1 分钟';
  }
  function brainTitle(b?: string): string {
    return b === 'codex' ? 'Codex' : b === 'grok' ? 'Grok' : b === 'claude' ? 'Claude' : '模型';
  }

  async function retry(convId: string, manual = false) {
    if (running[convId]) return;
    const base = activeConvId === convId ? activeConv : convos.find((c) => c.id === convId);
    if (!base) return;
    if (!base.session_ref?.trim()) { if (manual) say('这个会话首轮就没建起来，无法重试；请回启动页重新发起。', 'err'); return; }
    if (manual) delete autoRetried[convId];
    const msgs = base.messages.slice();
    const prevErr = msgs.length && msgs[msgs.length - 1].role === 'assistant' && msgs[msgs.length - 1].error ? msgs[msgs.length - 1].text.trim() : '';
    if (msgs.length && msgs[msgs.length - 1].role === 'assistant') msgs.pop(); // 去掉失败/部分的那条
    beginTurn(convId, { ...base, messages: msgs, status: 'running' });
    try {
      const conv = await invoke<Conversation>('retry_turn', { convId, onEvent: makeChannel(convId) });
      await refreshConvos();
      const failed = lives[convId]?.failed ?? false; const errText = lives[convId]?.error ?? '';
      endTurn(conv, convId);
      // 手动重试后同错再败 → resume 无效，收起「重试」只留「重建继续」；成功则解除。
      if (manual && failed && prevErr && errText.trim() === prevErr) retryExhausted[convId] = true;
      if (!failed) delete retryExhausted[convId];
      maybeAutoRetry(convId, failed, errText);
    } catch (e) { await failTurn(e, convId); }
  }
  // 重建会话续跑：底层会话状态损坏、重试无效时——换全新会话（带历史摘要）原地继续这条对话。
  async function rebuildSession(convId: string) {
    if (running[convId]) return;
    const base = activeConvId === convId ? activeConv : convos.find((c) => c.id === convId);
    if (!base) return;
    delete autoRetried[convId];
    delete retryExhausted[convId];
    const msgs = base.messages.slice();
    if (msgs.length && msgs[msgs.length - 1].role === 'assistant') msgs.pop();
    beginTurn(convId, { ...base, messages: msgs, status: 'running' });
    try {
      const conv = await invoke<Conversation>('rebuild_session', { convId, onEvent: makeChannel(convId) });
      await refreshConvos();
      endTurn(conv, convId); // 重建后不做自动重试，避免坏环境下打转
    } catch (e) { await failTurn(e, convId); }
  }
  // 这轮若因瞬时问题失败且本轮还没自动重试过，隔几秒自动重连一次；再不行就停手（留手动「重试」）。
  function maybeAutoRetry(convId: string, failed: boolean, errText: string) {
    const c = activeConvId === convId ? activeConv : convos.find((item) => item.id === convId);
    // Native API requests are independently billed and may have executed tools.
    // Keep retries explicit; the executor also refuses interrupted transcripts.
    if (c?.brain.startsWith('api:')) return;
    if (!failed || autoRetried[convId] || !isTransient(errText)) return;
    autoRetried[convId] = true;
    if (activeConvId === convId) say('网络/接口异常，正在自动重连重试…', 'err');
    // 存句柄，2.5s 后仅当没在跑且用户还停在本会话才自动重连（切走/又发新消息都不触发，避免抢焦点/陈旧重试）。
    retryTimers[convId] = setTimeout(() => {
      delete retryTimers[convId];
      if (!running[convId] && activeConvId === convId) void retry(convId, false);
    }, 2500);
  }

  async function send() {
    if (!activeConv || running[activeConv.id]) return;
    const atts = attachments;
    if (!draft.trim() && !atts.length) return;
    let text = draft.trim();
    if (atts.length) {
      text += (text ? '\n\n' : '') + ATT_MARKER + '\n' + atts.map((a) => `- ${a.path}`).join('\n');
    }
    draft = ''; attachments = [];
    const id = activeConv.id;
    delete autoRetried[id]; // 新的用户轮：重置自动重试预算
    delete retryExhausted[id];
    beginTurn(id, { ...activeConv, messages: [...activeConv.messages, optimisticUser(text)], status: 'running' });
    try {
      const conv = await invoke<Conversation>('send_message', { convId: id, message: text, onEvent: makeChannel(id) });
      await refreshConvos();
      const failed = lives[id]?.failed ?? false; const errText = lives[id]?.error ?? '';
      endTurn(conv, id);
      maybeAutoRetry(id, failed, errText);
    } catch (e) { await failTurn(e, id); }
  }

  async function stop() { if (running[activeConvId]) { try { await invoke('cancel_turn', { convId: activeConvId }); } catch { /* */ } } }

  function threadNearBottom(el: HTMLDivElement): boolean {
    return el.scrollHeight - el.scrollTop - el.clientHeight < 160;
  }
  function onThreadScroll(e: Event) {
    threadStickBottom = threadNearBottom(e.currentTarget as HTMLDivElement);
  }
  async function scrollSoon(force = false) {
    // 关键是先沿用内容增长前的贴底意图，不能等 DOM 变高后再判断距离；大段增量会被误判为用户上翻。
    if (force) threadStickBottom = true;
    if (!force && !threadStickBottom) return;
    threadScrollForce ||= force;
    if (threadScrollQueued) return;
    threadScrollQueued = true;
    await tick();
    requestAnimationFrame(() => {
      const forced = threadScrollForce;
      threadScrollForce = false;
      threadScrollQueued = false;
      const el = threadEl;
      if (!el || (!forced && !threadStickBottom)) return;
      el.scrollTop = el.scrollHeight;
      // WebView2 的 markdown/字体布局可能比首帧晚一拍，再校准一次消掉最后几行的缺口。
      cancelAnimationFrame(threadSecondRaf);
      threadSecondRaf = requestAnimationFrame(() => {
        const latest = threadEl;
        if (latest && (forced || threadStickBottom)) latest.scrollTop = latest.scrollHeight;
      });
    });
  }
  // 图片加载、markdown 重排、工具卡展开也会改变正文高度；只有仍贴底时才跟随。
  $effect(() => {
    const inner = threadInnerEl;
    if (!inner) return;
    const ro = new ResizeObserver(() => { if (threadStickBottom) void scrollSoon(); });
    ro.observe(inner);
    return () => { ro.disconnect(); cancelAnimationFrame(threadSecondRaf); };
  });
  // 输入法组合中（候选框开着）按回车是确认候选词，不能当发送。isComposing 在部分 webview 不可靠，
  // 叠加手动组合标记 + keyCode===229（IME 处理中）双保险。
  let composing = false;
  function onComposerKey(e: KeyboardEvent, fn: () => void) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing && !composing && e.keyCode !== 229) { e.preventDefault(); fn(); }
  }

  // ---------- 全局自定义 tips（替换原生 title 系统提示） ----------
  // 原生 title 出现慢、样式与应用割裂。首次悬停时把 title 搬进 data-tip（保留 aria-label
  // 供无障碍），再交给共享定位器按真实尺寸避让视口边缘。
  // disabled 按钮不触发鼠标事件 → 需要 tip 的禁用按钮外面包 .tipwrap 承载 data-tip。
  let imgTip = $state<{ x: number; y: number; url: string; below: boolean } | null>(null);
  let imgTipReady = $state(false); // 图片 onload 之前浮层不可见，避免空白小块→大图的闪变
  let hoverWant = ''; // 正在等加载的悬停目标（工作目录图片异步读完时校验还悬停着才弹）
  function onTipHover(e: MouseEvent) {
    const tgt = e.target as HTMLElement;
    // 助手消息里的图片（行内代码路径或链接）→ 悬停缩略图浮层（点击见 mdClick）
    const holder = tgt.closest?.('a, code') as HTMLElement | null;
    if (holder && holder.closest('.text.md') && !holder.closest('pre')) {
      const isLink = holder.tagName === 'A';
      const s = isLink ? (holder.getAttribute('href') ?? '') : (holder.textContent ?? '').trim();
      // 远程图片：完整 URL（链接/代码都认），或行内代码里的 /uploads/ 站点路径
      let u = '';
      if (/^https?:/i.test(s)) {
        if (IMG_EXT_RE.test(s.replace(/[?#].*$/, '')) && !badImgUrls.has(s)) u = s;
      } else if (!isLink) {
        u = codeImgUrl(s);
      }
      // 工作目录内图片（相对/绝对/file: 都认）→ data URI（负缓存空串＝确认不是可预览图片，当普通元素处理）
      let rel = '';
      if (!u) { const r = relWorkPath(s) || workAbsPath(s); if (r && isImgPath(r)) rel = r; }
      const k = rel ? thumbKey(rel) : '';
      const known = rel ? thumbs[k] : undefined;
      if (u || (rel && known !== '')) {
        const r0 = holder.getBoundingClientRect();
        const below = r0.top < 230; // 上方放不下缩略图就翻到下方
        const x = Math.min(Math.max(r0.left + r0.width / 2, 150), window.innerWidth - 150);
        const place = { x, y: below ? r0.bottom + 8 : r0.top - 8, below };
        holder.style.cursor = 'zoom-in';
        hideTip();
        if (u || known) {
          hoverWant = ''; // 立即可显示：作废先前挂着的异步加载，防止旧图旧坐标顶掉当前浮层
          const src = u || (known as string);
          if (!imgTip || imgTip.url !== src) imgTipReady = false;
          imgTip = { ...place, url: src };
        } else {
          const c = activeConv;
          imgTip = null;
          if (c) {
            hoverWant = k;
            loadWorkdirImg(c.conn_id, conversationProject(c), rel).then((d) => {
              if (d && hoverWant === k) { imgTipReady = false; imgTip = { ...place, url: d }; }
            });
          }
        }
        return;
      }
      holder.style.cursor = ''; // 负缓存命中/失效时把之前设过的手势复位
    }
    hoverWant = '';
    imgTip = null;
    const el = tgt.closest?.('[title], [data-tip]') as HTMLElement | null;
    if (!el) { hideTip(); return; }
    const t = el.getAttribute('title');
    if (t) {
      el.removeAttribute('title');
      el.setAttribute('data-tip', t);
      if (!el.getAttribute('aria-label') && !el.textContent?.trim()) el.setAttribute('aria-label', t);
    }
    const text = el.getAttribute('data-tip') ?? '';
    if (!text) { hideTip(); return; }
    showTipFor(el, text, { align: el.dataset.tipAlign === 'end' ? 'end' : 'center' });
  }

  // ---------- 右键菜单（原生） ----------
  // 替换 WKWebView 默认英文菜单（含 Inspect Element / AutoFill 等）。
  //
  // ★ 这里**必须交给原生菜单**，不能前端自己画：自己画就得用 navigator.clipboard.readText()
  // 去粘贴，而 macOS 14 起「程序读剪贴板」会弹一个系统「Paste」确认按钮 —— 用户点完我们的
  // 「粘贴」还得再点一次系统的，看着就像坏了，而且 JS 绕不过去（系统只看到有人要读剪贴板，
  // 看不到用户刚点过菜单，那正是它要防的）。见 lib.rs::show_edit_menu。
  // 启用态（没选中就灰掉复制等）也由系统自己管，不用我们猜。
  function onCtxMenu(e: MouseEvent) {
    if (e.defaultPrevented) return; // Child-owned menus (files/connections) take precedence.
    e.preventDefault();
    const t = e.target instanceof Element ? e.target : e.target instanceof Node ? e.target.parentElement : null;
    if (!t) return;
    const xterm = t.closest('.xterm');
    if (xterm && term) {
      // xterm 的选区画在 canvas 上，不属于 window.getSelection()：有选区时右键直接复制；
      // 无选区时把隐藏 textarea 设为第一响应者，再交给原生菜单完成安全粘贴。
      if (term.hasSelection()) {
        void copyTermSelection(term);
      } else {
        (xterm.querySelector('textarea') as HTMLTextAreaElement | null)?.focus();
        void invoke('show_edit_menu', { editable: true }).catch(() => {});
      }
      return;
    }
    const target = editMenuTarget(t);
    const sel = window.getSelection()?.toString() ?? '';
    if (!target && !sel) return; // 空白处不出菜单
    // 先聚焦：原生「剪切/复制/粘贴」作用在**当前第一响应者**上，不聚焦就打空
    // Focus the editing host without resetting its selection or scrolling the modal.
    if (target && document.activeElement !== target.element) target.element.focus({ preventScroll: true });
    void invoke('show_edit_menu', { editable: target?.editable ?? false }).catch(() => {});
  }

  // ---------- 助手消息 Markdown 渲染 ----------
  // 模型输出的 **粗体**/列表/表格/代码块 之前是裸文本，读感远不如 Claude 客户端。
  // marked(GFM+breaks) 渲染 → DOMPurify 消毒（模型会读网页/站点内容，注入面必须过滤）→ {@html}。
  marked.setOptions({ gfm: true, breaks: true });
  // 在 DOMPurify 默认协议白名单上放行 file:/sandbox:（AI 常用它们链接本地产物；默认会把
  // href 整个剥掉，链接看着能点实际是死的）。javascript: 等仍被拦。
  const MD_URI_RE = /^(?:(?:(?:f|ht)tps?|mailto|tel|callto|sms|cid|xmpp|file|sandbox):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i;
  // file: 只为 <a> 放行（点击全被 mdClick 代理接管）；<img> src 收紧回 http(s)/data:image，
  // 防 file: 子资源直连本地文件系统。
  DOMPurify.addHook('afterSanitizeAttributes', (n) => {
    if (n.tagName === 'IMG') {
      const src = n.getAttribute('src') || '';
      if (!/^(https?:|data:image\/)/i.test(src)) n.removeAttribute('src');
    }
  });
  function mdRender(text: string): string {
    return DOMPurify.sanitize(marked.parse(text, { async: false }) as string, { ALLOWED_URI_REGEXP: MD_URI_RE });
  }
  // Markdown 里 <a> 的点击代理：拦下 webview 内导航，改用系统浏览器打开。
  // 行内代码若是图片路径（如 `/uploads/xx.svg`）：点击也用浏览器打开完整地址。
  function mdClick(e: MouseEvent, context:Conversation|null=activeConv) {
    const t = e.target as HTMLElement;
    // 链接分支不看选区且永远 preventDefault——点链接是明确意图，而且 WebKit 里点 <a> 不清除
    // 页面残留选区，若因选区早退会放任 webview 自导航（本函数存在的意义就是拦它）。
    const a = t.closest('a');
    if (a) {
      e.preventDefault();
      const raw = a.getAttribute('href') ?? '';
      if (/^https?:/i.test(raw)) { openUrl(raw); return; }
      if (/^(mailto|tel):/i.test(raw)) { openUrl(raw); return; } // gfm 会把裸邮箱自动变 mailto 链接
      // 工作目录内的路径（相对 / 绝对 / file:）：图片→应用内放大；其他文件→文件管理器里定位
      const p = relWorkPath(raw) || workAbsPath(raw);
      if (p && isImgPath(p)) { openWorkdirLightbox(p, context); return; }
      if (p) { void revealWorkdir(p, context); return; }
      if (raw && raw !== '#') say('这个链接打不开（不是网址，也不是工作目录里的文件）', 'err');
      return;
    }
    const code = t.closest('code');
    if (code && !code.closest('pre')) {
      if (window.getSelection()?.toString()) return; // 用户在拖选复制路径文本：不劫持成打开动作
      const s = (code.textContent ?? '').trim();
      const u = codeImgUrl(s);
      if (u) { e.preventDefault(); openUrl(u); return; }
      const rel = relWorkPath(s) || workAbsPath(s);
      if (rel && isImgPath(rel)) { e.preventDefault(); openWorkdirLightbox(rel, context); return; }
      // 非图片的行内代码：必须带路径分隔符才当文件处理（`package.json`、`v1.2.3` 这类
      // 纯提及不劫持——否则点一下就弹「没找到文件」的噪音）。
      if (rel && rel.includes('/') && /\.[a-z0-9]{1,8}$/i.test(rel)) { e.preventDefault(); void revealWorkdir(rel, context); }
    }
  }

  // 工作目录相对路径（消息里提到的项目内文件）：无协议、不以 / 开头、无空白、不出目录。
  function relWorkPath(s: string): string {
    const v = s.trim().replace(/^\.\//, '');
    if (!v || /\s/.test(v) || /^[a-z][a-z0-9+.-]*:/i.test(v) || v.startsWith('/') || v.startsWith('#') || v.includes('..')) return '';
    return v;
  }
  // 本地绝对路径 / file: / sandbox: 链接 → 绝对路径字符串。只做形态识别；
  // 「必须在工作目录内」的边界由后端 canonicalize 后强制，越界一律拒绝。
  function workAbsPath(s: string): string {
    let v = s.trim();
    if (/^(file|sandbox):/i.test(v)) {
      try { v = decodeURIComponent(v.replace(/^file:\/\/(localhost)?/i, '').replace(/^sandbox:(\/\/)?/i, '')); }
      catch { return ''; }
    }
    if (!v || v.length > 1024) return '';
    if (!(v.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(v))) return '';
    return v;
  }
  function openWorkdirLightbox(p: string, c:Conversation|null=activeConv) {
    if (!c) return;
    // 点击是明确意图：之前失败过（负缓存空串）的条目给重试机会——文件可能刚被生成出来
    const k = thumbKey(p,c);
    if (thumbs[k] === '') { const t2 = { ...thumbs }; delete t2[k]; thumbs = t2; }
    loadWorkdirImg(c.conn_id, conversationProject(c), p).then((d) => {
      if (d) lightbox = d;
      else say('预览失败：文件不存在、超过 8MB，或在工作目录之外', 'err');
    });
  }
  async function revealWorkdir(p: string, c:Conversation|null=activeConv) {
    if (!c) return;
    try {
      const abs = await invoke<string>('resolve_workdir_file', { connId: c.conn_id, project: conversationProject(c), path: p });
      await revealItemInDir(abs);
    } catch { say('打不开：文件不存在（可能还没生成或已移动），或在工作目录之外', 'err'); }
  }

  // ---------- 行内代码图片预览 ----------
  // 助手常回报「已上传 → /uploads/xx.svg」这类路径；把它变成可看的：悬停缩略图、点击打开。
  // 只认站点约定的 /uploads/ 路径且要求已知站点公开地址（服务端对拼不出公开地址的站点刻意
  // 返回空 url，前端不猜接口域名）；完整 http(s) 图片 URL 也认。其余（本地绝对路径、构建
  // 产物路径）一律不当图片，避免拼出必 404 的链接。加载失败的 URL 进负缓存不再反复尝试。
  const IMG_EXT_RE = /\.(png|jpe?g|gif|webp|svg|bmp|avif|ico)$/i;
  const badImgUrls = new Set<string>();
  function codeImgUrl(t: string): string {
    const s = t.trim();
    if (!s || /\s/.test(s) || !IMG_EXT_RE.test(s)) return '';
    let u = '';
    if (/^https?:\/\//i.test(s)) u = s;
    else if (s.startsWith('/uploads/')) {
      const base = (activeSiteUrl || '').replace(/\/+$/, '');
      if (base) u = base + s;
    }
    return u && !badImgUrls.has(u) ? u : '';
  }

  // linkify + 段落（用户消息/错误消息仍走纯文本路径）
  const urlRe = /(https?:\/\/[^\s)"'`」』】>，。；：！？、（）《》]+)/g;
  function segs(text: string): { t: string; link: boolean }[] {
    const out: { t: string; link: boolean }[] = [];
    let last = 0; let m: RegExpExecArray | null; urlRe.lastIndex = 0;
    while ((m = urlRe.exec(text))) {
      if (m.index > last) out.push({ t: text.slice(last, m.index), link: false });
      out.push({ t: m[0], link: true }); last = m.index + m[0].length;
    }
    if (last < text.length) out.push({ t: text.slice(last), link: false });
    return out;
  }

  function taskLabel(t: string): string {
    return t === 'sitebuild' ? '建站' : t === 'remote' ? '远程运维' : '自由对话';
  }
  function conversationTaskLabel(c: Conversation | null | undefined): string {
    return taskLabel(c?.task_type ?? '');
  }
  // 侧栏分组键：CF 按项目；SSH 一台机器一组；本地对话按接入的文件夹（未接文件夹的归「自由对话」）。
  function conversationGroupKey(c: Conversation): string {
    if (c.task_type === 'workspace') return c.workspace_dir?.trim() || WORKSPACE_CONN_ID;
    if (c.task_type === 'remote') return '__remote__';
    return c.site_slug || '(未命名项目)';
  }
  function brainLabel(b: string): string { return b.startsWith('api:') ? `${apiProfile(b)?.name || '已移除的连接'} · API` : b === 'codex' ? 'Codex' : b === 'grok' ? 'Grok' : 'Claude'; }
  function fmt(secs: number): string { return new Date(secs * 1000).toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }
  const shownMessages = $derived((activeConv?.messages ?? []).filter((m) => !m.hidden));

  // 下拉选项
  // Sonnet / Haiku 用别名（--model sonnet/haiku），永远指向该档「当前最新」，厂商发新版自动
  // 跟随、无需更新客户端；sub 里的版本号只是提示，可能滞后。
  // Fable 各版本与 Opus 两档例外，走全 ID（原因见下），出新版需回来改这里。
  //
  // ★ Opus 两档都**钉全 ID**，不用 opus 别名：
  //   - 别名不会指到 Opus 5（那是它必须写全 ID 的原因）；
  //   - 且别名此刻也不指 4.8 —— 让它自报模型返回的是 claude-opus-4-6。自报版本本来就不可靠，
  //     不足以断定别名真指 4.6；但既然拿不准，就别用一个标着「4.8」的选项去赌别名解析到哪。
  //   钉住 claude-opus-4-8 后，标签写的就是实际发出去的，用户点 4.8 一定得到 4.8。
  //   代价与 Fable 相同：厂商出新版不会自动跟随，得回来改这里。
  //
  // ★ 测「某个 --model ID 存不存在」的前提：**CLI 凭据必须是有效的**。
  //   凭据过期时，claude CLI 对**任何** --model 值都静默返回空 + exit 0 —— 连凭空编的
  //   claude-definitely-not-real-9 也一样，此时「没报错」完全不等于「ID 有效」。
  //   凭据有效后才有判别力：真 ID 返回内容，假 ID 明确回
  //   「There's an issue with the selected model (…). It may not exist or you may not have access to it.」
  //   claude-opus-5 与 claude-opus-4-8 都是这么验的（真回话 + 同轮假 ID 被拒）。
  const CLAUDE_MODELS = [
    // ★ 必须用显式 ID，不能用别名 'sonnet'：别名由 CLI 解析，2.1.96 把它解析成
    //   claude-sonnet-4-6 —— 标签写着 Sonnet 5、实际跑的是 4.6（用户实测抓到）。
    { value: 'claude-sonnet-5', label: 'Sonnet', sub: '性价比 · Sonnet 5 · 1M 上下文' },
    { value: 'claude-opus-5-5', label: 'Opus 5.5', sub: '最新 Opus · 1M 上下文' },
    { value: 'claude-opus-5', label: 'Opus 5', sub: '主力 · Opus 5 · 1M 上下文' },
    { value: 'claude-opus-4-8', label: 'Opus 4.8', sub: '上一代 Opus · 1M 上下文' },
    { value: 'claude-fable-5-1', label: 'Fable 5.1', sub: '旗舰 · 最新 Fable · 1M 上下文' },
    { value: 'claude-fable-5', label: 'Fable 5', sub: '上一代 Fable · 1M 上下文' },
    { value: 'haiku', label: 'Haiku', sub: '最快 · 当前 Haiku 4.5 · 200k 上下文' },
  ];
  // Codex 档位 = 具体模型 ID（-c model=）。首项「跟随 Codex 默认」= 不覆盖本地 codex 配置。
  // 型号取自本机 codex 模型清单，会随厂商更新；要用别的新模型走下方「自定义模型 ID」。
  const CODEX_MODELS = [
    { value: '', label: '跟随 Codex 默认', sub: '用本地 codex 配置' },
    // 菜单展示官方模型容量；不要把 Codex 缓存默认窗口当成模型上限。
    // https://developers.openai.com/api/docs/models/gpt-6-astra
    { value: 'gpt-6-astra', label: 'GPT-6 Astra', sub: '最新旗舰 · 1.05M 模型上下文' },
    { value: 'gpt-6-sol', label: 'GPT-6 Sol', sub: '编程与复杂任务 · 1.05M 模型上下文' },
    { value: 'gpt-6-luna', label: 'GPT-6 Luna', sub: '快速轻量 · 1.05M 模型上下文' },
    // GPT-5.6 三分档（2026-07 GA）：裸 "gpt-5.6" 在 ChatGPT 订阅通道会被拒，必须用分档 ID；
    // 且要求 codex CLI ≥0.144（旧版报「requires a newer version of Codex」，升级 CLI 即可）。
    { value: 'gpt-5.6-sol', label: 'GPT-5.6 Sol', sub: '最强 · 细节打磨' },
    { value: 'gpt-5.6-terra', label: 'GPT-5.6 Terra', sub: '日常主力' },
    { value: 'gpt-5.6-luna', label: 'GPT-5.6 Luna', sub: '最快最省' },
    { value: 'gpt-5.5', label: 'GPT-5.5', sub: '上一代' },
    { value: 'gpt-5.4', label: 'GPT-5.4', sub: '上一代' },
    { value: 'gpt-5.4-mini', label: 'GPT-5.4-Mini', sub: '上一代 · 最省' },
  ];
  // Grok（订阅通道）使用官方模型 ID；首项「跟随默认」
  // 不传 --model，厂商换默认时自动跟随。显式档位方便会话固定模型与回退上一代。
  const GROK_MODELS = [
    { value: '', label: '跟随 Grok 默认', sub: '使用 CLI 当前默认模型' },
    { value: 'grok-4.7', label: 'Grok 4.7', sub: '最新 · 500k 上下文' },
    { value: 'grok-4.6', label: 'Grok 4.6', sub: '上一代 · 500k 上下文' },
    { value: 'grok-4.5', label: 'Grok 4.5', sub: '上一代 · 500k 上下文' },
  ];
  function modelOptsFor(b: string) {
    if (b.startsWith('api:')) return (apiProfile(b)?.models ?? []).map((id) => ({ value: id, label: id, sub: brainAvailability(b) }));
    return b === 'codex' ? CODEX_MODELS : b === 'grok' ? GROK_MODELS : CLAUDE_MODELS;
  }
  // launcher / 会话里可选：预设档位 + 该厂商的全局自定义模型 ID（定时任务表单仍只用预设 + 自己的 modelCustom）。
  function launcherModelOpts(b: string) { return [...modelOptsFor(b), ...customsOf(b).map((id) => ({ value: id, label: id, sub: '自定义' }))]; }
  function defaultModelFor(b: string): string {
    if (b.startsWith('api:')) return apiProfile(b)?.default_model ?? '';
    return b === 'claude' ? 'claude-sonnet-5' : '';
  }
  function isLauncherModel(b: string, m: string): boolean { return b.startsWith('api:') && !apiProfilesLoaded || launcherModelOpts(b).some((o) => o.value === m); }

  // 会话按「分组 → 任务类型」两级分组：分组按最近活动倒序；任务类型固定顺序，只留有会话的。
  const TASK_ORDER = ['sitebuild', 'remote', 'workspace'];
  const grouped = $derived.by(() => {
    const bySite = new Map<string, { slug: string; name: string; recent: number; convs: Conversation[] }>();
    for (const c of convos) {
      // 侧栏位置必须稳定：所有会话都只显示在创建它的连接下。
      if (c.conn_id !== activeConnId) continue;
      const key = conversationGroupKey(c);
      let g = bySite.get(key);
      const groupName = key === WORKSPACE_CONN_ID ? '自由对话'
        : c.task_type === 'workspace' ? workspaceFolderName(key)
          : c.site_name || c.site_slug || key;
      if (!g) { g = { slug: key, name: groupName, recent: 0, convs: [] }; bySite.set(key, g); }
      g.convs.push(c);
      if (c.updated_at > g.recent) g.recent = c.updated_at;
    }
    const groups = [...bySite.values()].sort((a, b) => b.recent - a.recent);
    return groups.map((s) => {
      const subs: { type: string; label: string; items: Conversation[] }[] = [];
      for (const t of TASK_ORDER) {
        const items = s.convs.filter((c) => c.task_type === t).sort((a, b) => b.updated_at - a.updated_at);
        if (items.length) subs.push({ type: t, label: taskLabel(t), items });
      }
      // 分组头的域名：CF 从对话里探测部署地址。
      return { slug: s.slug, name: s.name, count: s.convs.length, subs, url: isCfConn ? detectSiteUrl(s.convs) : '' };
    });
  });
  // CF 页脚：有域名（已部署）的项目算「站点」。
  const cfSiteCount = $derived(grouped.filter((g) => g.url).length);
  function faviconOf(u: string): string { try { return new URL(u).origin + '/favicon.ico'; } catch { return ''; } }
  // 站点分组折叠态（持久化）；默认全展开，打开某会话时自动展开其站点。
  let collapsedSites = $state(loadCollapsedSites());
  function loadCollapsedSites(): Set<string> { try { return new Set<string>(JSON.parse(localStorage.getItem('cligc.pilot.collapsedSites') || '[]')); } catch { return new Set(); } }
  function persistCollapsedSites() { try { localStorage.setItem('cligc.pilot.collapsedSites', JSON.stringify([...collapsedSites])); } catch { /* */ } }
  function toggleSite(slug: string) { const n = new Set(collapsedSites); if (n.has(slug)) n.delete(slug); else n.add(slug); collapsedSites = n; persistCollapsedSites(); }
  function expandSite(slug: string) { if (collapsedSites.has(slug)) { const n = new Set(collapsedSites); n.delete(slug); collapsedSites = n; persistCollapsedSites(); } }
  // 侧栏会话时间标签：今天 / 昨天 / N 天前 / 更早日期。
  function relTime(secs: number): string {
    const t0 = new Date(); t0.setHours(0, 0, 0, 0);
    const day = t0.getTime(); const ms = secs * 1000;
    if (ms >= day) return '今天';
    if (ms >= day - 864e5) return '昨天';
    const n = Math.floor((day - ms) / 864e5) + 1;
    if (n <= 7) return `${n} 天前`;
    const d = new Date(ms); return `${d.getMonth() + 1}/${d.getDate()}`;
  }
</script>

<svelte:window
  oncontextmenu={onCtxMenu}
  onmouseover={onTipHover}
  onscrollcapture={() => { hideTip(); imgTip = null; hoverWant = ''; }}
  onresize={() => { hideTip(); imgTip = null; hoverWant = ''; }}
  onkeydown={(e) => { if (edOpen) { onEditKey(e); return; } if (e.key === 'Escape') { if (lightbox) lightbox = ''; } }}
/>
<main class="app" class:win={isWindows} class:fs={isFullscreen} class:rail-collapsed={railCollapsed} class:top-update-visible={topUpdateVisible}>
  <!-- 融合式标题栏：透明拖拽条铺满顶部，红绿灯与工具按钮浮在其上（macOS Overlay） -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="titlebar" data-tauri-drag-region aria-hidden="true" onmousedown={startDrag} style="width:{railCollapsed ? (topUpdateVisible ? 167 : 140) : railWidth}px"></div>

  <!-- 顶部工具：折叠侧栏 + 搜索会话（窗口模式紧挨红绿灯右侧；全屏时无红绿灯，与左栏菜单左对齐） -->
  <div class="win-tools" class:fs={isFullscreen} class:win={isWindows}>
    <button class="wt" onclick={toggleRail} title={railCollapsed ? '展开侧栏' : '折叠侧栏'} aria-label="折叠侧栏">
      <svg width="16" height="16" viewBox="0 0 18 18" fill="none">
        <rect x="2.5" y="3.5" width="13" height="11" rx="2" stroke="currentColor" stroke-width="1.3" />
        <path d="M7 3.7v10.6" stroke="currentColor" stroke-width="1.3" />
        <rect x="3.4" y="4.4" width="2.7" height="9.2" rx="1" fill="currentColor" opacity=".28" />
      </svg>
    </button>
    <button class="wt" onclick={openSearch} disabled={!activeConn} title="搜索会话" aria-label="搜索会话">
      <svg width="16" height="16" viewBox="0 0 18 18" fill="none">
        <circle cx="8" cy="8" r="5" stroke="currentColor" stroke-width="1.4" />
        <path d="M11.7 11.7L15 15" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
      </svg>
    </button>
    {#if topUpdateVisible}
      <button class="wt upd" class:active={updateCenterOpen} onclick={openUpdateCenter}
        title={pilotUpdateStatus === 'checking' ? '正在检查更新' : updAvail ? `发现新版本 ${updAvail}，打开更新中心` : '打开更新中心'} aria-label="打开更新中心">
        {#if updBusy || pilotUpdateStatus === 'checking'}
          <svg class="upd-spin" width="15" height="15" viewBox="0 0 18 18" fill="none"><path d="M15.5 9a6.5 6.5 0 1 1-2-4.72" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>
        {:else}
          <svg width="15" height="15" viewBox="0 0 18 18" fill="none">
            <path d="M9 3v7.4M5.8 7.2 9 10.4l3.2-3.2" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
            <path d="M4.2 14.3h9.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
          </svg>
          {#if updAvail}<span class="upd-count">1</span>{/if}
        {/if}
      </button>
    {/if}
  </div>

  {#if updBusy}
    <div class="upd-toast" role="status">
      <svg class="upd-spin" width="13" height="13" viewBox="0 0 18 18" fill="none"><path d="M15.5 9a6.5 6.5 0 1 1-2-4.72" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg>
      <span class="upd-toast-msg">{updMsg}</span>
      {#if updBusy && updPct >= 0}<span class="upd-toast-bar"><span style="width:{updPct}%"></span></span>{/if}
    </div>
  {/if}

  <!-- 全局操作反馈挂在应用根层：无论当前打开抽屉、弹窗还是确认框，都应位于最外层。 -->
  {#if flash}<div class="flash {flashKind}" role="status" aria-live="polite">{flash}</div>{/if}

  <!-- 左栏 -->
  <aside class="rail" class:collapsed={railCollapsed} style="width:{railWidth}px">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="rail-resize" title="拖动调整宽度" onmousedown={startResize} role="separator" aria-orientation="vertical"></div>
    <div class="rail-head">
      <button class="newchat" onclick={newChat} disabled={!activeConn} title="新对话">
        <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
          <path d="M11.5 2.5l2 2L6 12l-2.5.5L4 10l7.5-7.5z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
          <path d="M2.5 13.5h11" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
        </svg>
        新对话
      </button>
      {#if isCfConn}
      <button class="railnav {view === 'templates' ? 'on' : ''}" onclick={openTemplates}>
        {@render templateIcon(14)}
        模板库
      </button>
      <button class="railnav {view === 'prompts' ? 'on' : ''}" onclick={openPrompts}>
        {@render promptIcon(14)}
        提示词
      </button>
      {/if}
    </div>

    <div class="convos" class:scrolled={convosScrolled} onscroll={(e) => (convosScrolled = (e.currentTarget as HTMLElement).scrollTop > 2)}>
      {#if grouped.length === 0}
        <p class="rail-empty">还没有对话。<br />{isSshConn ? '在下方面板里直接说你想让这台机器做什么。' : isCfConn ? '给项目起个名，直接说你想要什么样的网站。' : '选好模型，直接说你想做什么。'}</p>
      {/if}
      {#each grouped as g (JSON.stringify([activeConnId, g.slug]))}
        <button class="site-grp" onclick={() => toggleSite(g.slug)} title={g.name}>
          <svg class="site-grp-chev" class:collapsed={collapsedSites.has(g.slug)} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
          {#if isSshConn}
            {@render sshMark(13)}
          {:else if isCfConn}
            {#if g.url}<img class="site-grp-fav" src={faviconOf(g.url)} alt="" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.display = 'none')} />{/if}
          {:else}
            {@render workspaceMark(14)}
          {/if}
          <span class="site-grp-name">{g.name}</span>
          {#if g.url}<span class="site-grp-host" title={hostOf(g.url)}>{hostOf(g.url)}</span>{/if}
        </button>
        {#if !collapsedSites.has(g.slug)}
          <SidebarConversationList items={g.subs.flatMap(sub => sub.items)} label={g.name}>
            {#snippet row(c)}
              <div class="convo {activeConvId === c.id ? 'on' : ''}" onpointerdown={(e)=>startConversationPointerDrag(e,c.id)} draggable="true" ondragstart={(e)=>startConversationDrag(e,c.id)} ondragend={endConversationDrag} role="button" tabindex="0"
                onclick={() => openConv(c.id)} onkeydown={(e) => e.key === 'Enter' && openConv(c.id)}>
                <div class="convo-body">
                  <span class="convo-bi" title={brainLabel(c.brain)}><BrainIcon brain={modelIcon(c.brain)} size={11} /></span>
                  <span class="convo-title">{c.title}</span>
                  {#if c.status === 'running' || running[c.id]}<span class="mini-run"></span>{/if}
                  {#if permConvs.has(c.id)}<span class="mini-permit" title="有操作等你批准">待批</span>{/if}
                  <span class="convo-when">{relTime(c.updated_at)}</span>
                </div>
                <button class="convo-x" title="删除对话" onclick={(e) => { e.stopPropagation(); deleteConv(c.id); }}>×</button>
              </div>
            {/snippet}
          </SidebarConversationList>
        {/if}
      {/each}
    </div>

    <div class="foot-wrap" bind:this={footerEl}>
      {#if switcherOpen}
        <div class="conn-switch" role="menu" aria-label="切换连接">
          <div class="cs-scroll" aria-label="连接列表">
            <button class="cs-item {activeConnId === WORKSPACE_CONN_ID ? 'on' : ''}" role="menuitemradio" aria-checked={activeConnId === WORKSPACE_CONN_ID} onclick={openLocalWorkspace}>
              {@render workspaceMark(18)}
              <span class="cs-main"><b>本地对话</b><small>不连接任何服务，可接入本地文件夹</small></span>
              {#if activeConnId === WORKSPACE_CONN_ID}<span class="cs-check">✓</span>{/if}
            </button>
            {#each conns as c (c.id)}
              <button class="cs-item {activeConnId === c.id ? 'on' : ''}" role="menuitemradio" aria-checked={activeConnId === c.id} onclick={() => { selectConn(c.id); switcherOpen = false; }}
                oncontextmenu={(e) => openConnCtx(e, c)}>
                {#if c.kind === 'ssh'}{@render sshMark(18)}{:else}{@render cfMark(18)}{/if}
                <span class="cs-main"><b>{connectionDisplayName(c)}</b>
                  {#if c.kind === 'ssh'}
                    <small class="cs-os">{@render distroMark(distroOf(c), 12)}<span class="cs-os-label">{sshSub(c)}</span></small>
                  {:else}
                    <small><span class:conn-remark={!!c.remark?.trim()} data-tip={c.remark?.trim() ? `脱敏 Key：${c.key_prefix}` : undefined}>{c.remark?.trim() || c.key_prefix}</span> · Cloudflare</small>
                  {/if}</span>
                {#if activeConnId === c.id}<span class="cs-check">✓</span>{/if}
              </button>
            {/each}
          </div>
          <div class="cs-actions">
            <div class="cs-div"></div>
            <button class="cs-act" role="menuitem" onclick={openCfConnect}>{@render cfMark(15)}连接 Cloudflare</button>
            <button class="cs-act" role="menuitem" onclick={() => { switcherOpen = false; openSshConnect(); }}>{@render sshMark(15)}新建远程连接</button>
            <button type="button" class="cs-act" role="menuitem" onclick={openConnectionSetup}>{@render gearIcon()}连接与模型设置…</button>
          </div>
        </div>
      {/if}
    <button class="rail-foot" class:open={switcherOpen} onclick={() => { switcherOpen = !switcherOpen; }}>
      {#if isCfConn}{@render cfMark(18)}{:else if isSshConn}{@render sshMark(18)}{:else if activeConn}{@render workspaceMark(18)}{:else}<AppIcon size={18} />{/if}
      <span class="foot-main">
        <b>{activeConn ? connectionDisplayName(activeConn) : '未连接'}</b>
        {#if activeConn}<small>{#if isCfConn}{cfProjects.length} 个项目{cfSiteCount ? ` · ${cfSiteCount} 个站点` : ''}{:else if isSshConn}<!--
          远程连接：这里放远端系统版本（点一下重探，比如刚 do-release-upgrade 过）。
        --><span class="foot-os" role="button" tabindex="-1" title="远端系统 · 点击重新检测"
          onclick={(e) => { e.stopPropagation(); probeOs(activeConnId, true); }}
          onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); probeOs(activeConnId, true); } }}
          >{@render distroMark(distroOf(activeConn), 12)}{osProbing[activeConnId] ? '检测系统…' : sshSub(activeConn)}</span>{:else}本地 · 不连接任何服务{/if}</small>{/if}
      </span>
      <!-- 永远是展开箭头：这是个切换器，不是设置快捷键。 -->
      <svg class="foot-chev" class:up={switcherOpen} width="13" height="13" viewBox="0 0 12 12" fill="none">
        <path d="M2.75 7.5L6 4.25L9.25 7.5" stroke="currentColor" stroke-width="1.15" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
    </div>
  </aside>

  {#if splitDragId && splitDragPoint}
    {@const dragConversation=splitConversation(splitDragId)}
    {#if dragConversation}
      <div class="conversation-drag-ghost" style={`--drag-x:${splitDragPoint.x}px;--drag-y:${splitDragPoint.y}px`} aria-hidden="true">
        <span class="conversation-drag-icon"><BrainIcon brain={modelIcon(dragConversation.brain)} size={12} /></span>
        <span class="conversation-drag-title">{dragConversation.title}</span>
      </div>
    {/if}
  {/if}

  <!-- 主区 -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section class="main" aria-label="工作区" data-pilot-workarea ondragover={singleConversationDragOver}
    ondragleave={(e)=>{if(!(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node))singleDropEdge=null;}}
    ondrop={(e)=>{if(splitDragId&&singleDropEdge){e.preventDefault();const id=splitDragId,edge=singleDropEdge;endConversationDrag();void addSplit(id,edge);}}}>
    {#if singleDropEdge && !splitVisible}<div class={`single-split-preview ${singleDropEdge}`} aria-hidden="true"><span>分屏视图</span></div>{/if}
    {#if imgTip}
      {@const iu = imgTip.url}
      <div class="imgtip" class:ready={imgTipReady} class:below={imgTip.below} style="left:{imgTip.x}px; top:{imgTip.y}px">
        <img src={iu} alt="" onload={() => { if (imgTip?.url === iu) imgTipReady = true; }} onerror={() => { badImgUrls.add(iu); if (imgTip?.url === iu) imgTip = null; }} />
      </div>
    {/if}
    {#if lightbox}
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
      <div class="lightbox" onclick={() => (lightbox = '')}><img src={lightbox} alt="附件大图" /></div>
    {/if}

    {#if splitVisible && conversationLayout}
      <ConversationCanvas layout={conversationLayout} focused={splitFocused} dragging={!!splitDragId} dragConversation={splitDragId} preview={splitPointerPreview} content={splitContent}
        onfocus={focusSplit} ondrop={(target,edge)=>{const id=splitDragId;endConversationDrag();void addSplit(id,edge,target);}} onresize={(layout)=>conversationLayout=layout}/>
    {:else if !activeConn}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="center" data-tauri-drag-region onmousedown={startDrag}>
        <div class="hero-card">
          <div class="hero-mark"><AppIcon size={54} /></div>
          <h1>开始之前，先选一个工作方式</h1>
          <p>直接和本地 Claude / Codex / Grok 或 API 模型对话；连接 Cloudflare 让它帮你建站并部署；或者连上一台远程服务器，让它帮你运维。</p>
          <div class="hero-btns">
            <button class="btn soft hero-import" onclick={openLocalWorkspace}>{@render workspaceMark(15)}本地对话</button>
            <button class="btn soft hero-import" onclick={openCfConnect}>{@render cfMark(15)}连接 Cloudflare</button>
            <button class="btn soft hero-import" onclick={openSshConnect}>{@render sshMark(15)}新建远程连接</button>
          </div>
          {@render cliGuide()}
        </div>
      </div>

    {:else if view === 'launcher'}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="center launch-center" data-tauri-drag-region onmousedown={startDrag}>
        <div class="launcher">
          {#if isCfConn}
            <h1>让 AI 帮你建个站</h1>
            <p class="sub">给项目起个名，描述你想要的网站——边聊边建，随时本地预览，满意了再部署到 Cloudflare。</p>
          {:else if isSshConn}
            <h1>想让它在这台机器上做点什么？</h1>
            <p class="sub">它通过 Pilot 在 {activeConn?.ssh_user}@{activeConn?.ssh_host} 上执行命令；每条命令按你选的权限档位确认，凭据始终不交给模型。</p>
          {:else}
            <h1>想让它帮你做点什么？</h1>
            <p class="sub">像聊天一样把需求说清楚；需要处理文件时，可以接入一个本地文件夹。</p>
          {/if}

            <div class="composer big">
                <textarea bind:value={lDraft} bind:this={lDraftEl} use:autogrow rows="3"
                  placeholder={isSshConn ? '例如：看看这台机器的配置和已装的东西，然后帮我装上 Docker' : isCfConn ? '例如：做个卖手冲咖啡的落地页，深色调，留个邮箱订阅表单存到 D1，先给我方案' : isWorkspaceChat ? '任意聊点什么；需要处理本地文件时，可在左下角添加一个文件夹' : '例如：分析这个站点最近的内容表现，并给出下一步运营计划'}
                  oncompositionstart={() => (composing = true)} oncompositionend={() => (composing = false)}
                  onkeydown={(e) => onComposerKey(e, startChat)}></textarea>
              <div class="composer-bar">
                <div class="cb-left">
                  {#if isSshConn}
                    <!-- 远程连接没有站点/项目可选：对象就是那台机器 -->
                    <span class="ssh-target">{@render sshMark(13)}{activeConn?.ssh_user}@{activeConn?.ssh_host}:{activeConn?.ssh_port}</span>
                  {:else if isCfConn}
                    <input class="cf-proj-in" bind:value={lSite} placeholder="项目名，如 coffee-landing" spellcheck="false" autocapitalize="off" autocorrect="off" />
                    <Dropdown compact bind:value={lStyle} options={STYLE_OPTS} />
                  {:else}
                    <div class="workspace-picker">
                      <button type="button" class="workspace-folder" class:selected={!!workspaceDir} data-tip={workspaceDir || '选择一个本地文件夹作为本次自由对话的工作目录'} onclick={chooseWorkspaceFolder}>
                        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M1.8 4.3a1.5 1.5 0 0 1 1.5-1.5h3l1.4 1.6h5a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5H3.3a1.5 1.5 0 0 1-1.5-1.5V4.3Z" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round" /></svg>
                        <span>{workspaceDir ? workspaceFolderName(workspaceDir) : '添加文件夹'}</span>
                      </button>
                      {#if workspaceDir}<button type="button" class="workspace-clear" aria-label="移除文件夹" data-tip="移除文件夹，改为纯对话" onclick={() => (workspaceDir = '')}>×</button>{/if}
                    </div>
                  {/if}
                </div>
                <div class="cb-right">
                  <Dropdown compact bind:value={lPerm} options={permOptsFor(lBrain, isSshConn)} tone={permTone(lPerm)} tip={permTipFor(lBrain, lPerm, isSshConn)} />
                  <ModelFx options={comboOpts} value={`${lBrain}::${lModel}`} effort={lEffort} fast={lFast} fastOk={brains?.claude_fast_ok ?? false} onpick={pickCombo} oneffort={persistLauncherEffort} onfast={setFast} />
                  <UsageRing />
                  <button class="send" onclick={startChat} disabled={(isCfConn && !lSite.trim()) || !lDraft.trim() || !brainUsable(lBrain)} title="发送（Enter）">↑</button>
                </div>
              </div>
            </div>
          {#if !brainUsable(lBrain)}<p class="hint warn-text">{brainSetupHint(lBrain)}</p>{/if}
          {#if isWorkspaceChat && brains && !anyBrainUsable}{@render cliGuide()}{/if}
          {#if isCfConn && brains?.wrangler && !brains.wrangler.found}{@render wrNote()}{/if}
        </div>
      </div>

    {:else if view === 'templates'}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <header class="thread-head" data-tauri-drag-region onmousedown={startDrag}>
        <div class="th-info">
          <div class="th-line"><b class="th-title">{@render templateIcon(16)}<span>模板库</span></b><small class="tmpl-hint">把做好的站点存成模板，之后引用它快速起新项目</small></div>
          {#if templates.length}
            <div class="tmpl-chips">
              <button class="tmpl-chip {tmplCat === '全部' ? 'on' : ''}" onclick={() => (tmplCat = '全部')}>{@render catIcon('全部')}全部<span class="tc-n">{templates.length}</span></button>
              {#each tmplCats as c (c)}
                <button class="tmpl-chip {tmplCat === c ? 'on' : ''}" onclick={() => (tmplCat = c)}>{@render catIcon(c)}{c}<span class="tc-n">{templates.filter((t) => catOf(t) === c).length}</span></button>
              {/each}
            </div>
          {/if}
        </div>
        <button class="icon-btn" onclick={loadTemplates} disabled={templatesLoading} title="刷新">{@render refreshIcon(templatesLoading)}</button>
      </header>
      <div class="thread">
        <div class="tmpl-inner">
          {#if templates.length === 0}
            <div class="sched-empty">
              <div class="cal-mark">🧩</div>
              <b>还没有模板</b>
              <p>在一个 Cloudflare 站点项目对话里点「存模板」，做得好的站就能沉淀下来复用。</p>
            </div>
          {:else}
            <div class="tmpl-grid">
              {#each tmplShown as t (t.slug)}
                <div class="tmpl-card" id="tmpl-{t.slug}">
                  <div class="tmpl-thumb">
                    {#if tmplHtml[t.slug]}
                      <iframe class="tmpl-frame" srcdoc={tmplHtml[t.slug]} sandbox="allow-scripts" title={t.name} tabindex="-1" scrolling="no"></iframe>
                    {:else}
                      <span class="tmpl-letter">{t.name.slice(0, 1)}</span>
                    {/if}
                    <div class="tmpl-hover">
                      <button class="tmpl-act" aria-label="预览真实页面" onclick={() => previewTemplate(t)} data-tip="预览真实页面"><svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M1.6 8s2.4-4.4 6.4-4.4S14.4 8 14.4 8s-2.4 4.4-6.4 4.4S1.6 8 1.6 8Z" stroke="currentColor" stroke-width="1.2" /><circle cx="8" cy="8" r="1.9" stroke="currentColor" stroke-width="1.2" /></svg></button>
                      <button class="tmpl-act primary" aria-label="用它建新站" onclick={() => openUseTmpl(t)} data-tip="用它建新站"><svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M8 3.2v9.6M3.2 8h9.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg></button>
                      <!-- 随附模板不给删（后端也拒）：按钮摆着只会让人点了吃一个错 -->
                      {#if !t.builtin}
                        <button class="tmpl-act" aria-label="删除模板" onclick={() => delTemplate(t)} data-tip="删除模板"><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M3 4.6h10M6.4 4.6V3.3h3.2V4.6M4.6 4.6l.6 8.1h5.6l.6-8.1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
                      {/if}
                    </div>
                  </div>
                  <div class="tmpl-body">
                    <b>{t.name}</b>
                    <!-- 随附模板 created_at 恒 0，relTime 会渲染成 1970 的「1/1」——改显「内置」 -->
                    <!-- 「N 页」只在多页时挂：模板的单位是「一个站」，几页是**引用前就该知道**的事 -->
                    <div class="tmpl-sub"><span class="tmpl-desc">{t.desc || ''}</span><span class="tmpl-meta">{#if t.pages > 1}<span class="tmpl-pg">{t.pages} 页</span>{/if}{t.builtin ? '内置' : relTime(t.created_at)}</span></div>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      </div>

    {:else if view === 'remote'}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- 工作台头部只留一行（机器地址 + 状态）：标题「远程连接」是废话——侧栏已经写着你在哪台机器上。 -->
      <header class="thread-head slim" data-tauri-drag-region onmousedown={startDrag}>
        <div class="th-info"><small class="rhead-line">{activeConn?.ssh_user}@{activeConn?.ssh_host}:{activeConn?.ssh_port} · {termOn ? '已连接' : '未连接'}
          <button class="th-rfz" data-tip={termBusy ? '连接中…' : '重新连接'} aria-label="重新连接" disabled={termBusy} onclick={reconnectTerm}>{@render refreshIcon(termBusy)}</button>
          {#if stat}
            <span class="hstats">
              {@render loadStat('CPU', stat.cpu, stat.cpu === null ? '正在取样…（要两次采样才算得出占用）' : `CPU 占用 ${stat.cpu}%`)}
              {@render loadStat('内存', stat.memPct, `内存 ${stat.memText}`)}
              {@render loadStat('磁盘', stat.diskPct, `根分区 ${stat.diskText}`)}
            </span>
          {/if}</small></div>
        <div class="rhead-acts">
          <!-- 三个面板开关（VS Code 那套）：命令行 / 底部对话 / 右侧文件，各自可开可关、可同时开。 -->
          <button class="wb-tg" class:on={wb.term} aria-pressed={wb.term} data-tip="命令行窗口" aria-label="命令行窗口" onclick={toggleWbTerm}>
            {@render sshMark(15)}
          </button>
          <button class="wb-tg" class:on={wb.chat} aria-pressed={wb.chat} data-tip="底部对话框" aria-label="底部对话框" onclick={toggleWbChat}>
            <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="1.8" y="2.6" width="12.4" height="10.8" rx="2" stroke="currentColor" stroke-width="1.3" /><path d="M1.8 9.8h12.4" stroke="currentColor" stroke-width="1.3" /><rect x="1.8" y="9.8" width="12.4" height="3.6" fill="currentColor" opacity=".9" /></svg>
          </button>
          <button class="wb-tg" class:on={wb.files} aria-pressed={wb.files} data-tip="右侧文件" aria-label="右侧文件" onclick={toggleWbFiles}>
            <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="1.8" y="2.6" width="12.4" height="10.8" rx="2" stroke="currentColor" stroke-width="1.3" /><path d="M9.6 2.6v10.8" stroke="currentColor" stroke-width="1.3" /><path d="M9.6 2.6h2.6a2 2 0 0 1 2 2v6.8a2 2 0 0 1-2 2H9.6z" fill="currentColor" opacity=".9" /></svg>
          </button>
        </div>
      </header>
      <!-- 工作台：终端占主区，底部对话 / 右侧文件按需开，两条分隔线可拖。
           终端的尺寸变化交给 ResizeObserver 自动 fit（见 startTerm 上面那个 $effect）。 -->
      <div class="wb">
        <!-- 文件栏单开时把主区收起来（display:none）：终端就在里头，只藏不拆，xterm 和 SSH 会话都不掉。 -->
        <div class="wb-main" class:collapsed={wbFilesSolo}>
          <!-- 终端自己管滚动：别套 .thread（它的 overflow-y:auto 会和 xterm 打架）。
               ★ 关掉命令行只是 display:none **藏起来**，绝不拆 DOM —— 拆了 xterm 就没了，
               回来还得重连一次（连接是真的，掉一次要重登服务器）。 -->
          <div class="term-wrap" class:hid={!wb.term} class:with-chat={wb.chat}>
            <div class="term" bind:this={termEl}></div>
            {#if termBusy}
              <!-- 建连要走网络+认证，一两秒是常事，别让黑框干愣着。浮层而非写进终端：见 startTerm。 -->
              <div class="term-connecting"><span class="tc-spin"></span>正在连接 {activeConn?.ssh_user}@{activeConn?.ssh_host}…</div>
            {:else if termErr && missingSshCredential(termErr)}
              <button type="button" class="term-credential-repair" onclick={repairActiveSshCredential}>连接凭据缺失 · 补录密码</button>
            {/if}
          </div>
          {#if wb.chat}
            {#if wb.term}
              <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
              <div class="wb-split h" title="拖动调整高度" role="separator" aria-orientation="horizontal" onmousedown={(e) => startWbResize(e, 'h')}></div>
            {/if}
            <!-- 命令行关着时对话吃满整个主区（固定高度只在两者共存时才有意义） -->
            <div class="wb-chat" class:solo={!wb.term} style={wb.term ? `height:${wb.chatH}px` : ''}>
              {#if activeConv && activeConv.conn_id === activeConnId}
                {@render convPane(activeConv)}
              {:else}
                <!-- 还没有对话：这里就是起点（发出去就新建一条，走 startChat 那套）。
                     外层必须是 .composer-wrap —— 输入框的样式挂在 `.composer.big, .composer-wrap .composer` 上，
                     裸 .composer 一条都套不上（就是那次「样式缺失」）。 -->
                <div class="wb-start composer-wrap">
                  <div class="composer" class:fileover={composerFileOver} role="group" aria-label="远程消息输入"
                    ondragover={onComposerDragOver} ondragleave={onComposerDragLeave} ondrop={onComposerDrop}>
                    {#if attachments.length || attaching}
                      <div class="attach-row">
                        {#each attachments as a, i (a.path)}
                          <div class="attach-chip">
                            {#if a.preview}<img class="attach-img" src={a.preview} alt="" />{:else}<span class="attach-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M9 1.8H4.5A1.3 1.3 0 0 0 3.2 3.1v9.8a1.3 1.3 0 0 0 1.3 1.3h7a1.3 1.3 0 0 0 1.3-1.3V5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9 1.8v3.7h3.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg></span>{/if}
                            <span class="attach-name" title={a.name}>{a.name}</span>
                            <button class="attach-x" aria-label="移除附件" onclick={() => removeAttachment(i)}>×</button>
                          </div>
                        {/each}
                        {#if attaching}<span class="attach-loading">上传中…</span>{/if}
                      </div>
                    {/if}
                    <textarea bind:this={wbStartEl} bind:value={lDraft} use:autogrow rows="2" placeholder="让 AI 在这台机器上做点什么…例如：看看装了什么，帮我装上 Docker"
                      oncompositionstart={() => (composing = true)} oncompositionend={() => (composing = false)}
                      onpaste={onComposerPaste}
                      onkeydown={(e) => onComposerKey(e, startChat)}></textarea>
                    <div class="composer-bar">
                      <div class="cb-left"><span class="ssh-target" title={activeConn?.ssh_os || '远端系统未知'}>{@render distroMark(distroOf(activeConn), 13)}{activeConn?.ssh_user}@{activeConn?.ssh_host}</span></div>
                      <div class="cb-right">
                        <Dropdown compact bind:value={lPerm} options={permOptsFor(lBrain, true)} tone={permTone(lPerm)} tip={permTipFor(lBrain, lPerm, true)} />
                        <ModelFx options={comboOpts} value={`${lBrain}::${lModel}`} effort={lEffort} fast={lFast} fastOk={brains?.claude_fast_ok ?? false} onpick={pickCombo} oneffort={persistLauncherEffort} onfast={setFast} />
                        <button class="send" onclick={startChat} disabled={(!lDraft.trim() && !attachments.length) || attaching || !brainUsable(lBrain)} title="发送（Enter）">↑</button>
                      </div>
                    </div>
                  </div>
                  <!-- 档位警告见权限下拉的 tip（permTipFor）；这里只留「厂商没就绪」这种当下堵路的事。 -->
                  {#if !brainUsable(lBrain)}
                    <p class="hint warn-text">{brainSetupHint(lBrain)}</p>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        </div>
        {#if wb.files}
          <!-- 文件栏单开时没有可拖的对象（主区已收起），分隔线也就不出 -->
          {#if !wbFilesSolo}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <div class="wb-split v" title="拖动调整宽度" role="separator" aria-orientation="vertical" onmousedown={(e) => startWbResize(e, 'v')}></div>
          {/if}
          <aside class="files-wrap" class:solo={wbFilesSolo} class:fileover={sftpFileOver}
            style="{wbFilesSolo ? '' : `width:${wb.filesW}px;`} --fw-perm:{colW.perm}px; --fw-size:{colW.size}px; --fw-date:{colW.date}px"
            ondragenter={onSftpDragOver} ondragover={onSftpDragOver} ondragleave={onSftpDragLeave} ondrop={onSftpDrop}>
          {#if sftpFileOver}
            <div class="files-drop-overlay"><b>松开上传</b><small title={sftpDropTarget?.remoteDir}>到 {sftpDropTarget?.remoteDir || sftpPath || '当前目录'}</small></div>
          {/if}
          <div class="files-bar">
            <button class="fbtn" aria-label="上一级" data-tip="上一级" onclick={sftpUp} disabled={sftpBusy || !sftpPath || sftpPath === '/'}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M8 12.5v-9M4.2 7.3 8 3.5l3.8 3.8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
            {#if sftpPathEdit}
              <input class="tin fpath" bind:this={sftpPathEl} bind:value={sftpPathDraft} spellcheck="false" autocapitalize="off" autocorrect="off" placeholder="/"
                onblur={() => (sftpPathEdit = false)}
                onkeydown={(e) => { if (e.key === 'Enter') commitPath(); else if (e.key === 'Escape') sftpPathEdit = false; }} />
            {:else}
              <!-- 面包屑：点哪一段跳哪一层。尾巴上那块空白本身是个按钮 ——
                   点它＝切成输入框直接敲路径（资源管理器那套），键盘也 Tab 得到。 -->
              <div class="fcrumbs" bind:this={crumbEl}>
                {#each sftpCrumbs as c, i (c.path)}
                  {#if i > 0}<span class="fcrumb-sep">›</span>{/if}
                  <button class="fcrumb" class:cur={c.path === sftpPath} disabled={sftpBusy} onclick={() => sftpGo(c.path)}>{c.name}</button>
                {/each}
                <button class="fcrumb-blank" aria-label="直接输入路径" data-tip="点这里可直接输入路径" onclick={startPathEdit}></button>
              </div>
            {/if}
            <button class="fbtn" aria-label="刷新" data-tip="刷新" onclick={sftpRefresh} disabled={sftpBusy}>{@render refreshIcon(sftpBusy)}</button>
            <button class="fbtn" aria-label="新建文件夹" data-tip="新建文件夹" onclick={() => openMkdir()} disabled={sftpBusy}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M1.8 4.2A1.4 1.4 0 0 1 3.2 2.8h3l1.4 1.6h5.2a1.4 1.4 0 0 1 1.4 1.4v6a1.4 1.4 0 0 1-1.4 1.4H3.2a1.4 1.4 0 0 1-1.4-1.4v-7.6z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M8 7.4v3.4M6.3 9.1h3.4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg></button>
            <SftpTransfers records={sftpTransfers} busy={sftpBusy || !!sftpXfer || sftpNativeDragging}
              canDownload={!fileRename && !!sftpSelectedRow && !isSftpDeleting(sftpSelectedRow.path)} selectedName={sftpSelectedRow?.name}
              cancellableIds={sftpCancellableIds} onupload={() => sftpUpload()} ondownload={sftpDownloadSelected}
              oncancel={cancelSftpTransfer} onclear={clearSftpTransfers} onreveal={revealSftpTransfer} />
          </div>
          <!-- 出错就地给个重试：首次加载失败时 sftpPath 还是空的，刷新键（要有当前目录）帮不上忙 -->
          {#if sftpErr}<button type="button" class="err-note files-err" onclick={retryOrRepairSftp} title={missingSshCredential(sftpErr) ? '打开连接设置并补录密码' : '点击重试'}>{sftpErr} · {missingSshCredential(sftpErr) ? '补录密码' : '点此重试'}</button>{/if}
          <div class="fhead">
            <button class="fh fh-name" onclick={() => setSort('name')}>名称{#if sftpSort.key === 'name'}<span class="fh-ar" class:desc={!sftpSort.asc}>^</span>{/if}</button>
            <span class="fh fh-perm">{@render colGrip('perm')}权限</span>
            <button class="fh fh-size" onclick={() => setSort('size')}>{@render colGrip('size')}大小{#if sftpSort.key === 'size'}<span class="fh-ar" class:desc={!sftpSort.asc}>^</span>{/if}</button>
            <button class="fh fh-date" onclick={() => setSort('mtime')}>{@render colGrip('date')}日期{#if sftpSort.key === 'mtime'}<span class="fh-ar" class:desc={!sftpSort.asc}>^</span>{/if}</button>
          </div>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="files-list" oncontextmenu={(e) => openFctx(e, null)}>
            {#if sftpBusy && !sftpRows.length}
              <div class="files-empty">读取中…</div>
            {:else if !sftpRows.length}
              <div class="files-empty">{sftpErr ? '' : '空目录'}</div>
            {:else}
              {#each sftpRows as r (r.path)}
                {@const deleting = isSftpDeleting(r.path)}
                {@const deleteTarget = isSftpDeleteTarget(r.path)}
                {@const renaming = fileRename?.connId === sftpConnId && fileRename?.path === r.path}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class="frow" class:on={sftpSel === r.path} class:deleting class:drop-target={sftpFileOver && sftpDropTarget?.remoteDir === r.path && r.dir && !r.link} data-sftp-drop-path={r.path} role="row" tabindex="-1" aria-busy={deleting}
                  draggable={isMac && !deleting && !renaming && !sftpBusy && !sftpXfer && !sftpNativeDragging} ondragstart={(e) => onSftpDragStart(e, r)}
                  onclick={() => { if (!deleting) sftpSel = r.path; }}
                  ondblclick={() => { if (!deleting && !renaming) sftpOpenRow(r); }}
                  oncontextmenu={(e) => { e.stopPropagation(); if (!deleting) openFctx(e, r); }}
                  onkeydown={(e) => { if (!deleting && !renaming && e.key === 'Enter') sftpOpenRow(r); }}>
                  <span class="fname" style="padding-left:{r.depth * 15}px">
                    {#if r.dir && !r.link}
                      <button class="fchev" class:open={sftpOpenDirs.has(r.path)} aria-label="展开" disabled={deleting} onclick={(e) => { e.stopPropagation(); if (!deleting) toggleDir(r); }}>
                        {#if sftpLoading[r.path]}<span class="fspin"></span>{:else}<svg width="9" height="9" viewBox="0 0 12 12" fill="none"><path d="M4.5 2.5 8 6l-3.5 3.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
                      </button>
                    {:else}
                      <span class="fchev-sp"></span>
                    {/if}
                    {#if r.link}
                      <!-- 软链：sftp 的 readdir 是 lstat 语义，指向目录的软链也归这里（点开时才知道） -->
                      <svg class="fic link" width="14" height="14" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" fill="currentColor" opacity=".14" /><path d="M5.6 10.4 10.4 5.6M6.6 5.6h3.8v3.8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                    {:else if r.dir}
                      <svg class="fic dir" width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M1.8 4.2A1.4 1.4 0 0 1 3.2 2.8h3l1.4 1.6h5.2a1.4 1.4 0 0 1 1.4 1.4v6a1.4 1.4 0 0 1-1.4 1.4H3.2a1.4 1.4 0 0 1-1.4-1.4v-7.6z" fill="currentColor" opacity=".16" /><path d="M1.8 4.2A1.4 1.4 0 0 1 3.2 2.8h3l1.4 1.6h5.2a1.4 1.4 0 0 1 1.4 1.4v6a1.4 1.4 0 0 1-1.4 1.4H3.2a1.4 1.4 0 0 1-1.4-1.4v-7.6z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg>
                    {:else}
                      <svg class="fic" width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M4 1.8h5.2L12.8 5.4v8.8H4V1.8z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9.2 1.8v3.6h3.6" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg>
                    {/if}
                    {#if renaming && fileRename}
                      <!-- svelte-ignore a11y_no_static_element_interactions -->
                      <span class="file-rename" ondblclick={(e) => e.stopPropagation()} oncontextmenu={(e) => e.stopPropagation()}>
                        <input use:focusFileRename bind:value={fileRename.value} aria-label={`重命名 ${r.name}`}
                          aria-invalid={!!fileRename.err} readonly={fileRename.busy} spellcheck="false" autocapitalize="off" autocorrect="off"
                          oninput={() => { if (fileRename) fileRename.err = ''; }}
                          onblur={() => { if (!fileRename?.err) void saveFileRename(); }}
                          onkeydown={(e) => {
                            e.stopPropagation();
                            if (e.isComposing) return;
                            if (e.key === 'Enter') { e.preventDefault(); void saveFileRename(); }
                            if (e.key === 'Escape' && !fileRename?.busy) { e.preventDefault(); fileRename = null; }
                          }} />
                        {#if fileRename.err}<small role="alert">{fileRename.err}</small>{/if}
                      </span>
                    {:else}
                      <button type="button" class="fname-t" title="双击名称重命名" disabled={deleting}
                        ondblclick={(e) => { e.preventDefault(); e.stopPropagation(); openRename(r); }}>{r.name}</button>
                    {/if}
                    {#if deleteTarget}<span class="fdelete-state"><span class="fdelete-spin" aria-hidden="true"></span>删除中…</span>{/if}
                  </span>
                  <span class="fperm">{r.perms}</span>
                  <span class="fsize">{r.dir && !r.link ? '·' : fmtSize(r.size)}</span>
                  <span class="fdate">{fmtStamp(r.mtime)}</span>
                </div>
              {/each}
            {/if}
          </div>
        </aside>
        {/if}
      </div>

    {:else if view === 'prompts'}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <header class="thread-head" data-tauri-drag-region onmousedown={startDrag}>
        <div class="th-info"><b class="th-title">{@render promptIcon(16)}<span>提示词库</span></b><small>常用建站需求，点「用它建站」填进对话框就能开始；也能存自己的。</small></div>
        <button class="btn soft bare" onclick={openNewPrompt}>{@render plusIcon()}新增提示词</button>
      </header>
      <div class="thread">
        <div class="sched-inner">
          <div class="prompt-grid">
            {#each allPrompts as p (p.id)}
              <div class="prompt-card">
                <div class="prompt-top">
                  <b class="prompt-title">{p.title}</b>
                  {#if p.builtin}<span class="prompt-tag">预设</span>{/if}
                </div>
                <p class="prompt-body">{p.body}</p>
                <div class="prompt-acts">
                  <button class="prompt-act" aria-label="用它建站" data-tip="用它建站" onclick={() => usePrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M8 3.2v9.6M3.2 8h9.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg></button>
                  <button class="prompt-act" aria-label="复制提示词" data-tip="复制" onclick={() => copyPrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><rect x="5.5" y="5.5" width="7.6" height="8" rx="1.4" stroke="currentColor" stroke-width="1.2" /><path d="M10.6 5.5V4.2A1.4 1.4 0 0 0 9.2 2.8H4A1.4 1.4 0 0 0 2.6 4.2v5.2A1.4 1.4 0 0 0 4 10.8h1.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg></button>
                  {#if !p.builtin}
                    <button class="prompt-act" aria-label="编辑提示词" data-tip="编辑" onclick={() => openEditPrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M10.8 2.9l2.3 2.3M11.4 2.3l2.3 2.3-8 8-3 .7.7-3 8-8z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg></button>
                    <button class="prompt-act" aria-label="删除提示词" data-tip="删除" onclick={() => deletePrompt(p)}><svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M3 4.6h10M6.4 4.6V3.3h3.2V4.6M4.6 4.6l.6 8.1h5.6l.6-8.1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        </div>
      </div>

    {:else}
      {@render conversationHeader(activeConv)}
      {@render convPane(activeConv)}
    {/if}
  </section>
</main>

<!-- 会话搜索（Claude Code 风格：顶部圆角搜索框 + 结果列表） -->
{#if searchOpen}
  <div class="mask" role="presentation" onclick={() => (searchOpen = false)}></div>
  <div class="search-box" role="dialog" aria-modal="true" aria-label="搜索会话">
      <div class="search-head">
        <svg class="si-ic" width="16" height="16" viewBox="0 0 18 18" fill="none">
          <circle cx="8" cy="8" r="5" stroke="currentColor" stroke-width="1.4" />
          <path d="M11.7 11.7L15 15" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
        <input bind:this={searchInput} bind:value={searchQ} placeholder="搜索会话、连接（名称/IP）、模板…" spellcheck="false" autocapitalize="off" autocorrect="off" />
        <kbd>esc</kbd>
      </div>
      <div class="search-list">
        {#if searchFlat.length === 0}
          <p class="search-empty">没有匹配结果</p>
        {:else}
          {#each searchSections as sec, si (sec.title)}
            {@const base = searchSections.slice(0, si).reduce((n, s) => n + s.entries.length, 0)}
            <div class="search-sec">{sec.title}</div>
            {#each sec.entries as it, i (`${sec.title}-${i}`)}
              <button class="search-item {base + i === searchIdx ? 'on' : ''}" onclick={() => pickEntry(it)} onmouseenter={() => (searchIdx = base + i)}>
                {#if it.kind === 'view'}
                  {#if it.view === 'templates'}{@render templateIcon(15)}{:else}{@render promptIcon(15)}{/if}
                  <span class="si-main"><b>{it.label}</b><small>切换到{it.label}视图</small></span>
                {:else if it.kind === 'conv'}
                  {#if it.c.task_type === 'workspace'}
                    {@render workspaceMark(15)}
                  {:else if it.c.task_type === 'remote'}
                    {@render sshMark(15)}
                  {:else}
                    <SiteFav src="" label={it.c.site_slug} size={15} />
                  {/if}
                  <span class="si-main"><b>{it.c.title || '未命名会话'}</b><small>{#if it.c.task_type === 'workspace'}{it.c.workspace_dir ? workspaceFolderName(it.c.workspace_dir) : '纯对话'}{:else}{it.c.site_name || it.c.site_slug}{/if}</small></span>
                  {@render brainTag(it.c.brain, brainLabel(it.c.brain))}
                {:else if it.kind === 'template'}
                  <!-- 与左栏「模板库」同一个图标：搜到的东西该看得出是同一样东西 -->
                  <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="2.5" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /></svg>
                  <span class="si-main"><b>{it.t.name}</b><small>{catOf(it.t)}{it.t.desc ? ` · ${it.t.desc}` : ''}</small></span>
                  {#if it.t.builtin}<span class="si-tag">内置</span>{/if}
                {:else}
                  <!-- 与连接切换器同一个标 -->
                  {#if it.c.kind === 'ssh'}{@render sshMark(15)}{:else}{@render cfMark(15)}{/if}
                  <span class="si-main"><b>{it.c.name}</b><small>{#if it.c.kind === 'ssh'}{it.c.ssh_user}@{it.c.ssh_host}{it.c.ssh_port && it.c.ssh_port !== 22 ? ':' + it.c.ssh_port : ''}{it.c.ssh_os ? ` · ${it.c.ssh_os}` : ''}{:else}Cloudflare · {it.c.remark?.trim() || it.c.key_prefix}{/if}</small></span>
                  {#if activeConnId === it.c.id}<span class="si-tag">当前</span>{/if}
                {/if}
              </button>
            {/each}
          {/each}
        {/if}
      </div>
  </div>
{/if}

<!-- 消息气泡片段 -->
{#snippet bubble(m: Message, isLast: boolean, context:Conversation|null=activeConv)}
  {@const bubbleConv=context}
  {@const bubbleId=context?.id??''}
  {@const bubbleBusy=!!running[bubbleId]}
  {#if m.role === 'user'}
    {@const ua = splitAttachments(m.text)}
    <div class="msg user"><div class="ubody">
      {#if ua.body.trim()}<span class="ub-text">{@render richText(ua.body)}</span>{/if}
      {#if ua.atts.length}
        <div class="ub-atts" class:only={!ua.body.trim()}>
          {#each ua.atts as p (p)}
            {#if isImgPath(p) && thumbs[thumbKey(p,context)] !== ''}
              {#if thumbs[thumbKey(p,context)]}
                <button class="ub-att-img" data-tip={p.split('/').pop()} onclick={() => (lightbox = thumbs[thumbKey(p,context)])}><img src={thumbs[thumbKey(p,context)]} alt={p.split('/').pop()} onerror={() => (thumbs = { ...thumbs, [thumbKey(p,context)]: '' })} /></button>
              {:else}
                <!-- 数据未到：同尺寸占位，图片就位零布局跳动 -->
                <span class="ub-att-img ph" data-tip={p.split('/').pop()}></span>
              {/if}
            {:else}
              <span class="ub-att">
                <span class="ub-att-ic">
                  {#if isImgPath(p)}<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="2.2" y="2.8" width="11.6" height="10.4" rx="1.6" stroke="currentColor" stroke-width="1.2" /><circle cx="5.7" cy="6.2" r="1.1" fill="currentColor" /><path d="M3 12.4l3.3-3.1 2.1 1.9 2.4-2.6 2.2 2.3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  {:else}<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M9 1.8H4.5A1.3 1.3 0 0 0 3.2 3.1v9.8a1.3 1.3 0 0 0 1.3 1.3h7a1.3 1.3 0 0 0 1.3-1.3V5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9 1.8v3.7h3.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg>{/if}
                </span>
                <span class="ub-att-n" title={p}>{p.split('/').pop()}</span>
              </span>
            {/if}
          {/each}
        </div>
      {/if}
    </div></div>
  {:else}
    <div class="msg assistant">
      <div class="body">
        {#if m.tools.length}{@render cmds(m.tools)}{/if}
        {#if m.error && m.limit_reset != null}
          <div class="limit-card">
            <div class="limit-head">⏳ {brainTitle(bubbleConv?.brain)} 额度已用完</div>
            {#if (m.limit_reset ?? 0) > 0}
              <div class="limit-sub">预计 {fmtClock(m.limit_reset ?? 0)} 恢复{#if (m.limit_reset ?? 0) * 1000 > nowTick} · 还有 {fmtRemain((m.limit_reset ?? 0) * 1000 - nowTick)}{/if}</div>
            {:else}
              <div class="limit-sub">订阅套餐的时间窗限额已触顶，稍后会自动恢复；也可以稍等片刻手动重试。</div>
            {/if}
            {#if isLast && !bubbleBusy}
              <div class="limit-actions">
                {#if limitAuto[bubbleId]}
                  <button class="retry-btn is-armed" onclick={() => disarmLimitAuto(bubbleId)}>✓ 已排队，到点自动续跑 · 点击取消</button>
                {:else if (m.limit_reset ?? 0) * 1000 > nowTick}
                  <button class="retry-btn" onclick={() => armLimitAuto(bubbleId, m.limit_reset ?? 0)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="5.6" stroke="currentColor" stroke-width="1.4" /><path d="M8 5.2V8l2 1.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>到点自动续跑</button>
                {/if}
                {#if bubbleConv?.session_ref}<button class="retry-btn" onclick={() => retry(bubbleId, true)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M13 8a5 5 0 1 1-1.5-3.6M13 2v3h-3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>立即重试</button>{/if}
              </div>
              <div class="limit-hint">也可以在右下角把模型切到另一家继续。</div>
            {/if}
          </div>
        {:else if m.error}
          <div class="text is-err">{@render richText(m.text)}{#if isLast && !bubbleBusy}{#if bubbleConv?.session_ref && !retryExhausted[bubbleId]}<button class="retry-btn" onclick={() => retry(bubbleId, true)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M13 8a5 5 0 1 1-1.5-3.6M13 2v3h-3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>重试</button>{/if}<button class="retry-btn" title="换一个全新会话原地续跑（自动带上历史摘要）——用于会话状态损坏、重试无效时" onclick={() => rebuildSession(bubbleId)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M2.6 8a5.4 5.4 0 0 1 9.3-3.7M13.4 8a5.4 5.4 0 0 1-9.3 3.7" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><path d="M11.6 1.6v2.9h2.9M4.4 14.4v-2.9H1.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>重建继续</button>{/if}</div>
        {:else}
          {@const ga = splitGenImages(m.text)}
          <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
          <div class="text md" onclick={(e)=>mdClick(e,context)} onauxclick={(e)=>mdClick(e,context)}>{@html mdRender(ga.body)}</div>
          {#if ga.atts.length}
            <!-- codex 生图产物：缩略图（点击放大）；读不到的退回文件名卡（点击访达定位） -->
            <div class="ub-atts gen-imgs">
              {#each ga.atts as p (p)}
                {#if isImgPath(p) && thumbs[thumbKey(p,context)] !== ''}
                  {#if thumbs[thumbKey(p,context)]}
                    <button class="ub-att-img" data-tip={p.split(/[\\/]/).pop()} onclick={() => (lightbox = thumbs[thumbKey(p,context)])}><img src={thumbs[thumbKey(p,context)]} alt={p.split(/[\\/]/).pop()} onerror={() => (thumbs = { ...thumbs, [thumbKey(p,context)]: '' })} /></button>
                  {:else}
                    <span class="ub-att-img ph" data-tip={p.split(/[\\/]/).pop()}></span>
                  {/if}
                {:else}
                  <button class="ub-att as-btn" title={p} onclick={() => void revealWorkdir(p,context)}>
                    <span class="ub-att-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="2.2" y="2.8" width="11.6" height="10.4" rx="1.6" stroke="currentColor" stroke-width="1.2" /><circle cx="5.7" cy="6.2" r="1.1" fill="currentColor" /><path d="M3 12.4l3.3-3.1 2.1 1.9 2.4-2.6 2.2 2.3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg></span>
                    <span class="ub-att-n">{p.split(/[\\/]/).pop()}</span>
                  </button>
                {/if}
              {/each}
            </div>
          {/if}
        {/if}
      </div>
    </div>
  {/if}
{/snippet}

{#snippet richText(text: string)}{#each text.split('\n') as line, i (i)}{#if i > 0}<br />{/if}{#each segs(line) as s, si (si)}{#if s.link}<button class="inlink" onclick={() => openUrl(s.t)}>{s.t}</button>{:else}{s.t}{/if}{/each}{/each}{/snippet}

{#snippet toolChip(t: ToolCall & { count?: number })}
  <div class="tool"><span class="tcode">{t.label}</span><span class="tdetail">{t.detail}</span>{#if (t.count ?? 0) > 1}<span class="tool-count">×{t.count}</span>{/if}</div>
{/snippet}

{#snippet brainTag(brain: string, label: string)}<span class="btag"><BrainIcon brain={modelIcon(brain)} size={12} />{label}</span>{/snippet}

<!-- 只有「全部」「自建」带图标：这两个是**看什么范围**，后面那排是**按页型筛**。
     给每个都配图标反而把这层区别抹平了。 -->
{#snippet catIcon(c: string)}
  {#if c === '全部'}
    <svg class="tc-ic" width="11" height="11" viewBox="0 0 16 16" aria-hidden="true"><rect x="1.5" y="1.5" width="5.6" height="5.6" rx="1.3" fill="currentColor" /><rect x="8.9" y="1.5" width="5.6" height="5.6" rx="1.3" fill="currentColor" /><rect x="1.5" y="8.9" width="5.6" height="5.6" rx="1.3" fill="currentColor" /><rect x="8.9" y="8.9" width="5.6" height="5.6" rx="1.3" fill="currentColor" /></svg>
  {:else if c === CAT_MINE}
    <svg class="tc-ic" width="11" height="11" viewBox="0 0 16 16" fill="none" aria-hidden="true"><circle cx="8" cy="5" r="2.7" stroke="currentColor" stroke-width="1.5" /><path d="M2.9 14c0-2.7 2.3-4.4 5.1-4.4s5.1 1.7 5.1 4.4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
  {/if}
{/snippet}
{#snippet refreshIcon(spinning: boolean)}
  <svg class="rfz {spinning ? 'spin' : ''}" width="15" height="15" viewBox="0 0 24 24" fill="none">
    <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" />
    <path d="M21 3v5h-5" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{/snippet}
{#snippet backChevronIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="m9.8 3.5-4.5 4.5 4.5 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}

{#snippet plusIcon()}<svg class="plus-ic" width="13" height="13" viewBox="0 0 14 14" fill="none"><path d="M7 2.4v9.2M2.4 7h9.2" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg>{/snippet}
{#snippet templateIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="2.5" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /></svg>{/snippet}
{#snippet promptIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3 3h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H6.5L4 13.2V11H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /><path d="M5.2 6.2h5.6M5.2 8.4h3.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /></svg>{/snippet}

{#snippet gearIcon()}<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M2 5.3h3.5M9.3 5.3H14M2 10.7h5.1M10.9 10.7H14" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /><circle cx="7.4" cy="5.3" r="1.6" stroke="currentColor" stroke-width="1.3" /><circle cx="9" cy="10.7" r="1.6" stroke="currentColor" stroke-width="1.3" /></svg>{/snippet}
{#snippet settingsIcon()}<SettingsIcon />{/snippet}
{#snippet updateIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 2.2v7.2M5.3 6.8 8 9.5l2.7-2.7M3 11.2v1.3c0 .7.6 1.3 1.3 1.3h7.4c.7 0 1.3-.6 1.3-1.3v-1.3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}
{#snippet externalLinkIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M6 3.5h6.5V10M12.2 3.8 3.8 12.2" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}
{#snippet trashIcon(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3.4 4.5h9.2M6 2.5h4l.6 2M5 6.5v6M8 6.5v6M11 6.5v6M4.2 4.5l.6 9h6.4l.6-9" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}
{#snippet cfMark(size: number)}<svg class="cf-mark" width={size} height={size} viewBox="0 0 128 128"><path fill="#fff" d="m115.679 69.288l-15.591-8.94l-2.689-1.163l-63.781.436v32.381h82.061z" /><path fill="#f38020" d="M87.295 89.022c.763-2.617.472-5.015-.8-6.796c-1.163-1.635-3.125-2.58-5.488-2.689l-44.737-.581c-.291 0-.545-.145-.691-.363s-.182-.509-.109-.8c.145-.436.581-.763 1.054-.8l45.137-.581c5.342-.254 11.157-4.579 13.192-9.885l2.58-6.723c.109-.291.145-.581.073-.872c-2.906-13.158-14.644-22.97-28.672-22.97c-12.938 0-23.913 8.359-27.838 19.952a13.35 13.35 0 0 0-9.267-2.58c-6.215.618-11.193 5.597-11.811 11.811c-.145 1.599-.036 3.162.327 4.615C10.104 70.051 2 78.337 2 88.549c0 .909.073 1.817.182 2.726a.895.895 0 0 0 .872.763h82.57c.472 0 .909-.327 1.054-.8z" /><path fill="#faae40" d="M101.542 60.275c-.4 0-.836 0-1.236.036c-.291 0-.545.218-.654.509l-1.744 6.069c-.763 2.617-.472 5.015.8 6.796c1.163 1.635 3.125 2.58 5.488 2.689l9.522.581c.291 0 .545.145.691.363s.182.545.109.8c-.145.436-.581.763-1.054.8l-9.924.582c-5.379.254-11.157 4.579-13.192 9.885l-.727 1.853c-.145.363.109.727.509.727h34.089c.4 0 .763-.254.872-.654c.581-2.108.909-4.325.909-6.614c0-13.447-10.975-24.422-24.458-24.422" /></svg>{/snippet}
<!-- 单会话、多个窗格和远程工作台共用原始对话组件。
     运行状态按 context.id 查找；输入、滚动和操作由所属窗格绑定，不能读取其他窗格的焦点状态。 -->
{#snippet conversationActivity(context:Conversation)}
  {@const activeConv=context}
  {@const viewBusy=!!running[context.id]}
  {@const liveView=lives[context.id]}
  {@const activePermits=pendingPermits.filter(p=>p.conv===context.id)}
      {#if viewBusy && liveView}
        {@const progress = executionProgress(liveView, nowMs)}
        <div class="msg assistant">
          <div class="body">
            {#if liveView.tools.length}{@render cmds(liveView.tools)}{/if}
            {#if liveView.compactedAt}
              <div class="context-compact-note">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3 5.2h10M5 2.8l-2 2.4 2 2.4M13 10.8H3M11 8.4l2 2.4-2 2.4" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" /></svg>
                <span>Claude 已自动整理上下文</span>
                <small>同一会话继续，聊天记录保持不变</small>
              </div>
            {/if}
            <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
            {#if liveView.renderedText}<div class="text md live-text" onclick={(e)=>mdClick(e,activeConv)} onauxclick={(e)=>mdClick(e,activeConv)}>{@html liveView.renderedText}</div>{/if}
            {#if progress.thinking}
              <div class="codex-thinking" role="status" aria-live="polite">
                <span>正在思考</span>
              </div>
            {:else if progress.hasActivity}
              <div class="execution-progress-stack">
                <details
                  class="execution-progress execution-progress-details is-running"
                  class:quiet={progress.quiet}
                  class:has-failure={progress.failed}
                  class:recovered-failure={progress.continuedAfterFailure}
                >
                  <summary>
                    <span class="execution-progress-mark" aria-hidden="true">
                      <span></span>
                    </span>
                    <span class="execution-progress-copy">
                      <b>{progress.title}</b>
                      <small>{progress.meta}</small>
                    </span>
                    <span class="execution-failure-summary">
                      {#if progress.failedActivities.length && !progress.continuedAfterFailure}
                        <b>{progress.failedActivities.length} 项异常</b>
                      {/if}
                      <small>查看详情</small>
                      <svg width="11" height="11" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5 6 7.5l3-3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                    </span>
                  </summary>
                  <div class="execution-activity-list">
                    {#if progress.trueProgress}
                      <div class="execution-real-progress">
                        <div>
                          <b>{progress.trueProgress.current} / {progress.trueProgress.total}</b>
                          <span>{activityLabel(progress.trueProgress)}</span>
                        </div>
                        <span><i style={`width:${Math.round(((progress.trueProgress.current ?? 0) / (progress.trueProgress.total ?? 1)) * 100)}%`}></i></span>
                      </div>
                    {/if}
                    {#each progress.activities as activity (activity.id)}
                      {@const detail = activityDetail(activity)}
                      <div class="execution-activity-item" class:is-failed={activity.status === 'failed'}>
                        <span class={`execution-activity-state ${activity.status}`}></span>
                        <div>
                          <b>{activityLabel(activity)}</b>
                          {#if activity.status === 'failed'}
                            <p>{activityFailureReason(activity)}</p>
                          {:else if detail}
                            <p>{detail}</p>
                          {/if}
                        </div>
                        <small>{activityStatusLabel(activity)}</small>
                      </div>
                    {/each}
                    {#if progress.failedActivities.length}
                      <p class="execution-failure-impact">
                        {progress.continuedAfterFailure
                          ? '后续步骤已继续执行；最终答复会说明该异常是否影响结果。'
                          : '本轮仍在继续；最终答复会说明该异常是否影响结果。'}
                      </p>
                    {/if}
                  </div>
                </details>
              </div>
            {/if}
            {#if liveView.error}<div class="err-note">{liveView.error}</div>
            {:else if !progress.thinking}
              <div class="working" aria-label="思考中">
                <svg class="wl" viewBox="0 0 64 64" width="17" height="17" aria-hidden="true">
                  <path class="wl-trace" d="M44 24a14 14 0 1 0 0 16" fill="none" stroke="#a03c2b" stroke-width="7" stroke-linecap="round" pathLength="100" stroke-dasharray="100" />
                  <circle class="wl-dot" cx="45.5" cy="42" r="4.6" fill="#a03c2b" />
                </svg>
                <span class="working-t">{elapsedLabel(nowMs - liveView.startedAt)}</span>
              </div>
            {/if}
          </div>
        </div>
      {/if}
      {#each activePermits as p (p.id)}
        <div class="msg assistant">
          <div class="permit-card" class:danger={p.dangerous}>
            <div class="permit-head"><span class="permit-dot"></span>需要你确认这个操作{#if p.dangerous}<span class="permit-tag">危险 · 对线上生效</span>{/if}</div>
            <div class="permit-desc">{permitDesc(p)}</div>
            <div class="permit-meta">{p.tool === 'SSH' ? '远程命令' : p.tool}{#if p.tool === 'SSH' && p.arg} · {p.arg}{/if}{#if p.mode === 'ask'} · 询问档{/if}</div>
            {#if p.cmd}
              {#if p.tool === 'SSH'}
                <!-- 远程命令不给收起：它要在用户的真机上跑，不看着它就没法做决定。 -->
                <code class="permit-cmd">{p.cmd}</code>
              {:else if permitCmdOpen.has(p.id)}
                <button class="permit-more" onclick={() => togglePermitCmd(p.id)}>收起命令 ▴</button>
                <code class="permit-cmd">{p.cmd}</code>
              {:else}
                <button class="permit-more" onclick={() => togglePermitCmd(p.id)}>查看具体命令 ▸</button>
              {/if}
            {/if}
            <div class="permit-act">
              <button class="btn sm" onclick={() => respondPermit(p.id, false)}>拒绝</button>
              <button class="btn sm primary" onclick={() => respondPermit(p.id, true)}>批准</button>
            </div>
          </div>
        </div>
      {/each}

{/snippet}

{#snippet conversationHeader(context:Conversation|null, paneId='')}
  {@const meta=conversationPresentation(context)}
  {@const activeConv=context}
  {@const activeSiteUrl=meta.url}
  {@const headerDomain=activeSiteUrl?hostOf(activeSiteUrl):''}
  {@const headerSiteIcon=headerDomain?faviconGuess(`https://${headerDomain}`):''}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <header class="thread-head conversation-thread-head" class:multi={leaves(conversationLayout).length>1} onmousedown={(e)=>{if(!paneId)startDrag(e);}}>
        <div class="th-info" role="button" tabindex="0" aria-label={`拖动对话 ${context?.title} 以分屏`} onpointerdown={(e)=>{if(context)startConversationPointerDrag(e,context.id);}} onkeydown={(e)=>{if(e.key==='Escape')endConversationDrag();}}>
          <b class="conversation-head-title" data-tip={activeConv?.title}>
            <span class="conversation-head-model" data-tip={brainLabel(activeConv?.brain??'claude')}><BrainIcon brain={modelIcon(activeConv?.brain??'claude')} size={14} /></span>
            <span>{activeConv?.title}</span>
          </b>
        </div>
        {#if activeSiteUrl}
          <button class="th-open conversation-site-link" onclick={() => openUrl(activeSiteUrl)} title="打开 {activeSiteUrl}"><SiteFav src={headerSiteIcon} label={headerDomain} size={14} /><span>{headerDomain}</span><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M6 3.5h6.5V10M12.2 3.8 3.8 12.2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
        {:else if headerDomain}
          <span class="th-open conversation-site-link static" title={headerDomain}><SiteFav src={headerSiteIcon} label={headerDomain} size={14} /><span>{headerDomain}</span></span>
        {/if}
        {#if paneId && leaves(conversationLayout).length>1}<button class="x pane-close" title="关闭窗格，不停止任务" aria-label="关闭窗格" onclick={()=>closeSplit(paneId)}>×</button>{/if}
      </header>

{/snippet}

{#snippet splitContent(id:string, paneId:string)}
  {@const c=splitConversation(id)}
  {#if c && splitDrafts[id]}
    {#key c.id}
      {@render conversationHeader(c,paneId)}
      {@render convPane(c,paneId)}
    {/key}
  {/if}
{/snippet}

{#snippet convPane(context:Conversation|null, paneId='')}
  {@const meta=conversationPresentation(context)}
  {@const activeConv=context}
  {@const activeConvConn=meta.conn}
  {@const activeConvIsCf=meta.conn?.kind==='cloudflare'}
  {@const activeConvIsWorkspace=context?.task_type==='workspace'}
  {@const scoped=!!paneId && !!context}
  {@const id=context?.id??''}
  {@const activeConvId=id}
  {@const activeConvIsSsh=meta.conn?.kind==='ssh'}
  {@const viewBusy=!!running[id]}
  {@const paneState=scoped?splitDrafts[id]:null}
  {@const draft=paneState?paneState.text:legacyComposer().text}
  {@const attachments=paneState?paneState.files:legacyComposer().files}
  {@const queued=scoped?(splitQueued[id]?{convId:id,text:splitQueued[id].text,atts:splitQueued[id].files}:null):legacyComposer().queued}
  {@const cfReady=scoped?paneCfReady[id]??false:legacyComposer().cfReady}
  {@const uploading=scoped?!!paneUploading[id]:attaching}
  {@const messages=context?.messages.filter(m=>!m.hidden)??[]}
  <div class="thread" use:paneScroll={scoped?id:''} onscroll={(e)=>{if(!scoped)onThreadScroll(e);}}>
    <div class="thread-inner" >
      {#each messages as m, i (i)}
        {@render bubble(m, i === messages.length - 1,context)}
      {/each}
      {#if activeConv}{@render conversationActivity(activeConv)}{/if}
    </div>
  </div>

  <div class="composer-wrap">
    {#if activeConvIsCf}
      <div class="cf-bar">
        <span class="tipwrap" data-tip={cfReady ? '在本机跑起来看效果（关预览窗即停）' : '先让 AI 建出页面再预览'}><button class="cb-prev" onclick={()=>scoped?panePrefill(id,()=>void startPreview()):void startPreview()} disabled={previewBusy || !cfReady}><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M1.6 8s2.4-4.4 6.4-4.4S14.4 8 14.4 8s-2.4 4.4-6.4 4.4S1.6 8 1.6 8Z" stroke="currentColor" stroke-width="1.2" /><circle cx="8" cy="8" r="1.9" stroke="currentColor" stroke-width="1.2" /></svg>{previewBusy ? '启动中…' : '预览'}</button></span>
        <span class="tipwrap" data-tip={cfReady ? '发布到 Cloudflare 上线' : '先让 AI 建出页面再部署'}><button class="cb-prev" onclick={()=>scoped?panePrefill(id,fillDeploy):fillDeploy()} disabled={viewBusy || !cfReady}><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M8 12.5V4M4.5 7 8 3.5 11.5 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>部署</button></span>
        <span class="tipwrap" data-tip={cfReady ? '存成模板，以后一键复用' : '先让 AI 建出页面再存'}><button class="cb-prev dim" onclick={()=>scoped?panePrefill(id,openSaveTmpl):openSaveTmpl()} disabled={!cfReady}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><rect x="2.6" y="2.6" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9.1" y="2.6" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="2.6" y="9.1" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9.1" y="9.1" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /></svg>存模板</button></span>
      </div>
    {/if}
    <div class="composer" class:fileover={!scoped&&composerFileOver} role="group" aria-label="消息输入"
      ondragover={(e)=>{if(scoped){if(e.dataTransfer?.types.includes('Files'))e.preventDefault();}else onComposerDragOver(e);}}
      ondragleave={(e)=>{if(!scoped)onComposerDragLeave(e);}}
      ondrop={(e)=>{if(scoped&&e.dataTransfer?.files.length){e.preventDefault();e.stopPropagation();void uploadPaneFiles(id,e.dataTransfer.files);}else if(!scoped)onComposerDrop(e);}}>
      {#if scoped && splitErrors[id]}<p class="pane-error" role="alert">{splitErrors[id]}</p>{/if}
      {#if queued && queued.convId === activeConvId}
        <div class="queued-row">
          <span class="queued-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.3" /><path d="M8 4.7V8l2.2 1.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg></span>
          <span class="queued-t">等这轮结束后发送：{queued.text || '（仅附件）'}{#if queued.atts.length} · {queued.atts.length} 个文件{/if}</span>
          <button class="queued-btn" onclick={()=>scoped?restoreSplitQueue(id):editQueued()}>编辑</button>
          <button class="queued-x" aria-label="清除等待消息" onclick={()=>{if(scoped)delete splitQueued[id];else clearQueued();}}>×</button>
        </div>
      {/if}
      {#if attachments.length}
        <div class="attach-row">
          {#each attachments as a, i (a.path)}
            <div class="attach-chip">
              {#if a.preview}<img class="attach-img" src={a.preview} alt="" />{:else}<span class="attach-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M9 1.8H4.5A1.3 1.3 0 0 0 3.2 3.1v9.8a1.3 1.3 0 0 0 1.3 1.3h7a1.3 1.3 0 0 0 1.3-1.3V5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9 1.8v3.7h3.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg></span>{/if}
              <span class="attach-name" title={a.name}>{a.name}</span>
              <button class="attach-x" aria-label="移除附件" onclick={() => {if(paneState)paneState.files=paneState.files.filter((_,n)=>n!==i);else removeAttachment(i);}}>×</button>
            </div>
          {/each}
        </div>
      {/if}
      {#if uploading}<span class="attach-loading">上传中…</span>{/if}
      <textarea bind:value={()=>draft,(v)=>scoped?setPaneText(id,v):setLegacyDraft(v)} use:paneInput={scoped?id:''} aria-label={`给 ${context?.title||'当前对话'} 发消息`} rows="1" placeholder={viewBusy ? '输入下一条，回车排队，等这轮结束自动发送' : '继续说…（Enter 发送，Shift+Enter 换行，可粘贴/拖入文件）'}
        oncompositionstart={() => (composing = true)} oncompositionend={() => (composing = false)}
        onpaste={(e)=>onComposerPaste(e,scoped?id:'')}
        onkeydown={(e) => onComposerKey(e, scoped ? ()=>void splitSubmit(id) : viewBusy ? queueMessage : send)}></textarea>
      <div class="composer-bar">
        <div class="cb-left">
          {#if activeConvIsCf}
            <span class="cb-ro" title="项目已固定"><span class="cb-ro-t">{activeConv?.site_slug}</span></span>
          {:else if activeConvIsSsh}
            <!-- 远程对话：这里放远端系统的图标 + 机器地址。 -->
            <span class="cb-ro" title={activeConvConn?.ssh_os || '远端系统未知'}>{@render distroMark(distroOf(activeConvConn), 15)}<span class="cb-ro-t">{activeConv?.site_name}</span></span>
          {:else}
            <span class="cb-ro" title={activeConv?.workspace_dir || '本会话未关联文件夹'}>{@render workspaceMark(15)}<span class="cb-ro-t">{activeConv?.workspace_dir ? workspaceFolderName(activeConv.workspace_dir) : '纯对话'}</span></span>
          {/if}
        </div>
        <div class="cb-right">
          <Dropdown compact value={context?.perm_mode||'full'} options={permOptsFor(activeConv?.brain ?? 'claude', activeConvIsSsh)} tone={permTone(context?.perm_mode||'full')} tip={permTipFor(activeConv?.brain ?? 'claude', context?.perm_mode||'full', activeConvIsSsh)} onchange={(v)=>scoped&&context?void splitSetting(context,'perm',v):persistThreadPerm(v)} />
          <!-- 模型下拉列全部厂商（图标区分）：同厂商下一轮生效；跨厂商确认后以历史摘要重建续跑 -->
          <ModelFx options={scoped&&context?splitModelOptions(context):threadComboOpts} value={`${activeConv?.brain ?? 'claude'}::${context?.model??''}`} effort={context?.effort??''} fast={activeConv?.fast ?? false} fastOk={brains?.claude_fast_ok ?? false} lockModel={viewBusy} onpick={(v: string) => { if(scoped&&context)void splitSetting(context,'model',v);else void pickThreadCombo(v); }} oneffort={(v:string)=>scoped&&context?void splitSetting(context,'effort',v):persistThreadEffort(v)} onfast={(v: boolean) => { if(scoped&&context)void splitSetting(context,'fast',v);else void persistThreadFast(v); }} />
          <UsageRing ctx={activeConv?.ctx_tokens ?? 0} limit={ctxLimitAdaptive(activeConv?.brain ?? 'claude', activeConv?.model ?? '', activeConv?.ctx_tokens ?? 0)} total={activeConv?.total_tokens ?? 0} />
          {#if viewBusy}
            {#if draft.trim() || attachments.length}
              <button class="send queue" disabled={uploading} onclick={()=>scoped?void splitSubmit(id):queueMessage()} title="排队：等这轮结束后自动发送">↑</button>
            {/if}
            <button class="send stop" onclick={()=>scoped?void splitStop(id):stop()} title="停止">■</button>
          {:else}
            <button class="send" onclick={()=>scoped?void splitSubmit(id):send()} disabled={uploading || (!draft.trim() && !attachments.length)} title="发送（Enter）">↑</button>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/snippet}

<!-- 顶栏的一个负载读数：标签 + 百分比 + 一条细底纹。
     pct=null＝还没算出来（CPU 要两次采样求差，见 pollStats）。 -->
{#snippet loadStat(label: string, pct: number | null, tip: string)}
  <span class="hstat" class:warn={pct !== null && pct >= 75} class:hot={pct !== null && pct >= 90} data-tip={tip}>
    <span class="hstat-l">{label}</span>
    <span class="hstat-v">{pct === null ? '—' : pct + '%'}</span>
  </span>
{/snippet}

<!-- 列宽手柄：坐在定宽列的左边缘上（就是列头之间那根分隔线）。做成 button 是为了键盘也够得着
     （←/→ 微调），顺带免掉给 div 挂事件的一堆 a11y 警告。 -->
{#snippet colGrip(k: 'perm' | 'size' | 'date')}
  <button class="fh-grip" aria-label="调整列宽（左右方向键微调）" data-tip="拖动调整列宽"
    onmousedown={(e) => startColDrag(e, k)}
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => onColKey(e, k)}></button>
{/snippet}

{#snippet sshMark(size: number)}<svg width={size} height={size} viewBox="0 0 16 16" fill="none"><rect x="1.6" y="2.6" width="12.8" height="10.8" rx="2" stroke="currentColor" stroke-width="1.3" /><path d="M4.4 6.2 6.6 8l-2.2 1.8M8.2 10.2h3.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg>{/snippet}

<!-- 发行版图标：按 os-release 的 ID 选（distroOf 做归一化 + ID_LIKE 兜底）。
     都是简化几何形，认得出即可；认不出的发行版走最后那个通用「服务器」形。 -->
{#snippet distroMark(id: string, size: number)}
  {#if id === 'ubuntu'}
    <!-- Ubuntu：三点「朋友圈」 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="5.6" stroke="#E95420" stroke-width="1.5" />
      <circle cx="8" cy="2.9" r="1.75" fill="#E95420" /><circle cx="3.6" cy="10.6" r="1.75" fill="#E95420" /><circle cx="12.4" cy="10.6" r="1.75" fill="#E95420" />
    </svg>
  {:else if id === 'debian'}
    <!-- Debian：开口螺旋 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path d="M11.6 4.2A5.2 5.2 0 1 0 12 11" stroke="#A80030" stroke-width="1.5" stroke-linecap="round" />
      <path d="M9.9 6.1a3 3 0 1 0 .5 3.6" stroke="#A80030" stroke-width="1.4" stroke-linecap="round" />
    </svg>
  {:else if id === 'alpine'}
    <!-- Alpine：山 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="6.2" fill="#0D597F" />
      <path d="M4 10.6 6.3 6.6l1.5 2.6M7.4 10.6l2.4-4.2 2.4 4.2z" stroke="#fff" stroke-width="1.1" stroke-linejoin="round" />
    </svg>
  {:else if id === 'fedora'}
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="6.2" fill="#51A2DA" />
      <path d="M9.9 4.6h-.8a1.9 1.9 0 0 0-1.9 1.9v4a1.5 1.5 0 1 1-1.5-1.5h3.4" stroke="#fff" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  {:else if id === 'arch'}
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path d="M8 1.8 14 13.6c-1.6-.9-3-1.5-3.6-1.7L8 7.2l-2.4 4.7c-.6.2-2 .8-3.6 1.7L8 1.8z" fill="#1793D1" />
    </svg>
  {:else if id === 'centos' || id === 'rhel' || id === 'rocky' || id === 'almalinux'}
    <!-- RHEL 系：四色方块 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <rect x="1.8" y="1.8" width="5.4" height="5.4" rx="1" fill="#932279" /><rect x="8.8" y="1.8" width="5.4" height="5.4" rx="1" fill="#EFA724" />
      <rect x="1.8" y="8.8" width="5.4" height="5.4" rx="1" fill="#79BCE8" /><rect x="8.8" y="8.8" width="5.4" height="5.4" rx="1" fill="#9CCD2A" />
    </svg>
  {:else}
    <!-- 认不出的（含没探到系统信息时）：中性服务器形 -->
    <svg class="distro" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <rect x="2" y="2.6" width="12" height="4.6" rx="1.2" stroke="currentColor" stroke-width="1.2" />
      <rect x="2" y="8.8" width="12" height="4.6" rx="1.2" stroke="currentColor" stroke-width="1.2" />
      <circle cx="4.6" cy="4.9" r=".8" fill="currentColor" /><circle cx="4.6" cy="11.1" r=".8" fill="currentColor" />
    </svg>
  {/if}
{/snippet}

{#snippet wrNote()}<div class="wr-note"><svg class="wr-ic" width="15" height="15" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.6" stroke="currentColor" stroke-width="1.3" /><path d="M8 4.8v4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /><circle cx="8" cy="11.1" r="0.6" fill="currentColor" /></svg><span>{wrInstalling ? '正在从 npm 装 wrangler，首次约 1–2 分钟，别关窗…' : '还没装 wrangler，预览 / 部署要用'}</span><button class="wr-btn" onclick={installWrangler} disabled={wrInstalling}>{#if wrInstalling}<span class="wr-spin"></span>安装中 {elapsedLabel(wrElapsed)}{:else}一键安装{/if}</button></div>{/snippet}

{#snippet cliGuide()}
  {#if brains}
    <div class="cli-guide" data-no-drag>
      <div class="cli-guide-h"><span>模型准备（本地客户端或设置中的 API 服务）</span><button class="cli-redetect" onclick={refreshBrainsManual} disabled={brainsBusy} title="装好 / 登录完点这里重新检测（会重新读取 PATH）">{@render refreshIcon(brainsBusy)}<span>重新检测</span></button></div>
      {#each [{ b: 'claude' as Brain, st: brains.claude, name: 'Claude Code', cmd: CLAUDE_INSTALL_CMD }, { b: 'codex' as Brain, st: brains.codex, name: 'Codex', cmd: 'npm i -g @openai/codex' }, { b: 'grok' as Brain, st: brains.grok, name: 'Grok', cmd: GROK_INSTALL_CMD }] as r (r.b)}
        <div class="cli-row">
          <BrainIcon brain={r.b} size={16} />
          <span class="cli-name">{r.name}</span>
          {#if !r.st.found}
            <span class="cli-tag bad">未安装</span>
            {#if r.b === 'claude'}
              <button class="wr-btn" use:tipAction={'安装失败排障：VPN/代理需覆盖 claude.ai 与下载域名 storage.googleapis.com（规则模式加进规则或临时切全局）；也可复制右侧命令到终端手动执行看完整输出。'} onclick={installClaude} disabled={claudeInstalling}>{#if claudeInstalling}<span class="wr-spin"></span>{nodeBoot || `安装中 ${elapsedLabel(claudeElapsed)}`}{:else}一键安装{/if}</button>
            {:else if r.b === 'grok'}
              <button class="wr-btn" onclick={installGrok} disabled={grokInstalling}>{#if grokInstalling}<span class="wr-spin"></span>{nodeBoot || `安装中 ${elapsedLabel(grokElapsed)}`}{:else}一键安装{/if}</button>
            {:else if r.b === 'codex'}
              <button class="wr-btn" onclick={installCodex} disabled={codexInstalling}>{#if codexInstalling}<span class="wr-spin"></span>{nodeBoot || `安装中 ${elapsedLabel(codexElapsed)}`}{:else}一键安装{/if}</button>
            {/if}
            <code class="cli-cmd" title={r.cmd}>{r.cmd}</code>
            <button class="cli-copy" title="复制命令" aria-label="复制安装命令" onclick={() => copyCmd(r.cmd)}>
              {#if copiedCmd === r.cmd}
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M3.5 8.5l3 3 6-7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" /></svg>
              {:else}
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="5.4" y="5.4" width="8.2" height="8.2" rx="1.6" stroke="currentColor" stroke-width="1.3" /><path d="M10.6 5.4V4.1A1.6 1.6 0 0 0 9 2.5H4.1A1.6 1.6 0 0 0 2.5 4.1V9a1.6 1.6 0 0 0 1.6 1.6h1.3" stroke="currentColor" stroke-width="1.3" /></svg>
              {/if}
            </button>
          {:else if r.st.logged_in === false}
            <span class="cli-tag warn">未登录</span><button class="authbtn" onclick={() => authorize(r.b)} disabled={authWaiting === r.b}>{#if authWaiting === r.b}<span class="wr-spin"></span>等待授权…{:else}去授权 ↗{/if}</button>
          {:else}
            <span class="cli-tag ok">✓ {r.st.version || '已就绪'}</span>
          {/if}
        </div>
      {/each}
      <p class="cli-note"><svg class="cli-note-ic" width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.3" /><path d="M8 7.3v3.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><circle cx="8" cy="4.8" r="0.95" fill="currentColor" /></svg><span>安装/登录后状态灯自动变绿；密钥只进 {keystoreName}，绝不落盘。</span></p>
      <p class="cli-note"><svg class="cli-note-ic" width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.3" /><path d="M8 7.3v3.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><circle cx="8" cy="4.8" r="0.95" fill="currentColor" /></svg><span>没有 Node 的机器，一键安装会自动下载托管版 Node(v22 LTS) 到应用数据目录，并写入用户级 PATH（只追加、绝不覆盖或截断；写入失败会提示手动配置）。</span></p>
      {#if !brains.browser.found}
        <p class="cli-note"><svg class="cli-note-ic" width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.3" /><path d="M1.9 8h12.2M8 1.6c-4.4 4.2-4.4 8.6 0 12.8 4.4-4.2 4.4-8.6 0-12.8Z" stroke="currentColor" stroke-width="1.1" /></svg><span>未检测到 Chrome / Edge——「AI 网页截图配图」不可用（可选功能，装个 Chrome 即启用）。</span></p>
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet workspaceMark(size: number)}
  <svg width={size} height={size} viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M4 6.6A2.6 2.6 0 0 1 6.6 4h6.8A2.6 2.6 0 0 1 16 6.6v3.9a2.6 2.6 0 0 1-2.6 2.6H8.2l-3.1 2.7a.42.42 0 0 1-.7-.32V6.6Z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" /></svg>
{/snippet}

{#snippet cmds(tools: ToolCall[])}
  <details class="cmds">
    <summary>
      <svg class="cmd-ic" width="13" height="13" viewBox="0 0 16 16" fill="none">
        <rect x="1.5" y="2.5" width="13" height="11" rx="2" stroke="currentColor" stroke-width="1.3" />
        <path d="M4.5 6l2 2-2 2M8.5 10h3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <span>{tools.length} 条执行记录</span>
      <svg class="cmd-chev" width="11" height="11" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </summary>
    <div class="tools">{#each compactToolRows(tools) as t, i (i)}{@render toolChip(t)}{/each}</div>
  </details>
{/snippet}

<!-- 设置弹窗 -->
{#if setupOpen}
  <div class="mask" role="presentation" onclick={() => { setupAddOpen = false; setupOpen = false; }}></div>
  <div class="sheet" use:sheetWidth={SHEET_WIDTH.setup} role="dialog" aria-modal="true" aria-label="连接与模型">
    <header class="sheet-head"><b>连接与模型</b><button class="x" onclick={() => { setupAddOpen = false; setupOpen = false; }}>×</button></header>
    <div class="sheet-body">
      <div class="sec-head">
        <span>连接</span>
        <div class="setup-add" bind:this={setupAddEl}>
          <button
            class="icon-btn setup-add-trigger"
            class:open={setupAddOpen}
            aria-label="添加连接"
            aria-haspopup="menu"
            aria-expanded={setupAddOpen}
            data-tip={setupAddOpen ? undefined : '添加连接'}
            onclick={() => (setupAddOpen = !setupAddOpen)}
          >{@render plusIcon()}</button>
          {#if setupAddOpen}
            <div class="setup-add-menu" role="menu">
              <button role="menuitem" onclick={() => { setupAddOpen = false; openCfConnect(); }}>
                {@render cfMark(15)}<span>连接 Cloudflare</span>
              </button>
              <button role="menuitem" onclick={() => { setupAddOpen = false; openSshConnect(); }}>
                {@render sshMark(15)}<span>新建远程连接</span>
              </button>
            </div>
          {/if}
        </div>
      </div>
      {#if conns.length === 0}<p class="hint">还没有连接。可以连接 Cloudflare 建站部署，或新建一个 SSH 远程连接。本地对话不需要连接。</p>{/if}
      <div class="conn-list">
        {#each conns as c (c.id)}
          <div class="conn-row {activeConnId === c.id ? 'on' : ''}" role="button" tabindex="0"
            onclick={() => selectConn(c.id)}
            onkeydown={(e) => e.key === 'Enter' && e.currentTarget === e.target && selectConn(c.id)}
            oncontextmenu={(e) => openConnCtx(e, c)}>
            {#if c.kind === 'ssh'}{@render sshMark(22)}{:else}{@render cfMark(22)}{/if}
            <span class="conn-main"><b>{connectionDisplayName(c)}</b>
              {#if c.kind === 'ssh'}
                <small class="cs-os">{@render distroMark(distroOf(c), 12)}<span class="cs-os-label">{sshSub(c)}</span></small>
              {:else}
                <small><span class:conn-remark={!!c.remark?.trim()} data-tip={c.remark?.trim() ? `脱敏 Key：${c.key_prefix}` : undefined}>{c.remark?.trim() || c.key_prefix}</span> · Cloudflare</small>
              {/if}</span>
            <button
              class="icon-btn sm"
              class:active={connCtx?.conn.id === c.id}
              title="连接设置"
              aria-label={`打开 ${connectionDisplayName(c)} 的连接设置`}
              aria-haspopup="menu"
              aria-expanded={connCtx?.conn.id === c.id}
              onclick={(e) => toggleConnCtx(e, c)}
            >{@render settingsIcon()}</button>
          </div>
        {/each}
      </div>

      <div class="sec-head mt"><span>模型服务 · 本地客户端</span><button class="icon-btn" onclick={refreshBrainsManual} title="刷新本地客户端">{@render refreshIcon(brainsBusy)}</button></div>
      {#if brains}
        <div class="brains-list">
        {#each [{ b: 'claude' as Brain, st: brains.claude, name: 'Claude Code', cmd: CLAUDE_INSTALL_CMD }, { b: 'codex' as Brain, st: brains.codex, name: 'Codex', cmd: 'npm i -g @openai/codex' }, { b: 'grok' as Brain, st: brains.grok, name: 'Grok', cmd: GROK_INSTALL_CMD }] as r (r.b)}
          <div class="brain-block">
          <div class="brain-row">
            <span class="brain-ic"><BrainIcon brain={r.b} size={18} /></span>
            <span class="brain-main"><b>{r.name}</b>
              <small>{#if !r.st.found}未安装{:else if r.st.logged_in === false}未登录{:else}{r.st.version || '已就绪'}{#if r.b === 'claude' && cliUpd?.has_update} · <span class="cli-new">新版 {cliUpd.latest}</span>{/if}{/if}</small></span>
            <span class="brain-dot"><span class="dot {r.st.found && r.st.logged_in && !(r.b === 'claude' && claudeVerify === 'bad') ? 'ok' : r.st.found ? 'warn' : 'off'}"></span></span>
            {#if r.st.found && r.st.logged_in === false}
              {#if r.b === 'claude'}
                <span class="brain-auth-actions">
                  <button
                    class="claude-token-trigger"
                    class:active={claudeTokenOpen}
                    type="button"
                    onclick={() => (claudeTokenOpen = !claudeTokenOpen)}
                    aria-label="粘贴 Claude 订阅 Token"
                    aria-expanded={claudeTokenOpen}
                    data-tip="粘贴订阅 Token&#10;使用 claude setup-token 生成"
                  ><svg width="15" height="15" viewBox="0 0 18 18" fill="none" aria-hidden="true"><circle cx="5.5" cy="12.5" r="3.25" stroke="currentColor" stroke-width="1.45"/><path d="m7.85 10.15 6.55-6.55M11.45 6.55l1.7 1.7M13.15 4.85l1.35 1.35" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
                  <button class="authbtn" onclick={() => authorize(r.b)} disabled={authWaiting === r.b}>{#if authWaiting === r.b}<span class="wr-spin"></span>等待授权…{:else}去授权 ↗{/if}</button>
                </span>
              {:else}
                <button class="authbtn" onclick={() => authorize(r.b)} disabled={authWaiting === r.b}>{#if authWaiting === r.b}<span class="wr-spin"></span>等待授权…{:else}去授权 ↗{/if}</button>
              {/if}
            {:else if r.b === 'claude' && r.st.logged_in && claudeVerify === 'bad'}<button class="authbtn" onclick={() => rebind('claude')} disabled={authWaiting === 'claude'}>{#if authWaiting === 'claude'}<span class="wr-spin"></span>等待授权…{:else}重新绑定 ↗{/if}</button>
            {:else if r.b === 'claude' && cliUpd?.has_update}
              <!-- 图标 + tips：这一行右侧本来是「去授权 / 重新绑定」那类**必须做**的动作，
                   升级是可选的，用文字按钮会跟它们抢同一级视觉重量。tips 里写全「升到哪个版本、
                   会动哪个路径」——多安装时只有 PATH 第一个生效，这信息不能只藏在确认弹窗里。
                   升级中不另写 tip：disabled 的按钮在 Chrome 下根本不派发 mouseover，气泡永远弹不出来，
                   状态只能靠 spinner 表达。 -->
              <button class="cli-upg" onclick={() => void upgradeCli()} disabled={cliUpgrading}
                aria-label={`升级 Claude Code 到 ${cliUpd.latest}`}
                data-tip={`升级到 ${cliUpd.latest}\n${cliUpd.target}${cliUpd.all.length > 1 ? `\nPATH 上有 ${cliUpd.all.length} 份，只有这个生效` : ''}`}>
                {#if cliUpgrading}<span class="wr-spin"></span>{:else}{@render updateIcon(14)}{/if}
              </button>{/if}
          </div>
          {#if r.b === 'claude' && claudeTokenOpen}
            <div class="claude-token-form">
              <div class="claude-token-copy">
                <b>Claude Code 订阅 Token</b>
                <small>在终端运行 <code>claude setup-token</code>，完成授权后复制生成的长期 Token。仅支持 Pro、Max、Team 或 Enterprise 订阅。</small>
              </div>
              <div class="claude-token-entry">
                <input
                  type="password"
                  bind:value={claudeTokenDraft}
                  placeholder="粘贴 Token"
                  autocomplete="off"
                  spellcheck="false"
                  autocapitalize="off"
                  autocorrect="off"
                  onkeydown={(event) => event.key === 'Enter' && void saveClaudeToken()}
                />
                <button class="btn sm" type="button" onclick={() => void saveClaudeToken()} disabled={!claudeTokenDraft.trim() || claudeTokenSaving}>{claudeTokenSaving ? '验证中…' : '保存并验证'}</button>
                <button class="btn sm ghost" type="button" onclick={closeClaudeToken} disabled={claudeTokenSaving}>取消</button>
              </div>
              <small class="claude-token-safe">Token 只保存到 {keystoreName}，不会写入 Pilot 配置、导出文件或对话内容。</small>
            </div>
          {/if}
          {#if !r.st.found}
            <!-- 与主界面同款一键安装（同 invoke/进度态/错误处理），放在「未安装」状态文字下方、小号靠左；
                 手动命令降级为同行的复制小图标 -->
            <div class="brain-install">
              <button class="wr-btn" onclick={r.b === 'claude' ? installClaude : r.b === 'codex' ? installCodex : installGrok}
                disabled={r.b === 'claude' ? claudeInstalling : r.b === 'codex' ? codexInstalling : grokInstalling}>
                {#if (r.b === 'claude' && claudeInstalling) || (r.b === 'codex' && codexInstalling) || (r.b === 'grok' && grokInstalling)}
                  <span class="wr-spin"></span>{nodeBoot || `安装中 ${elapsedLabel(r.b === 'claude' ? claudeElapsed : r.b === 'codex' ? codexElapsed : grokElapsed)}`}
                {:else}一键安装{/if}
              </button>
              <button class="cli-copy" title={`手动安装命令：${r.cmd}（点击复制）`} aria-label="复制手动安装命令" onclick={() => copyCmd(r.cmd)}>
                {#if copiedCmd === r.cmd}
                  <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M3.5 8.5l3 3 6-7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" /></svg>
                {:else}
                  <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="5.4" y="5.4" width="8.2" height="8.2" rx="1.6" stroke="currentColor" stroke-width="1.3" /><path d="M10.6 5.4V4.1A1.6 1.6 0 0 0 9 2.5H4.1A1.6 1.6 0 0 0 2.5 4.1V9a1.6 1.6 0 0 0 1.6 1.6h1.3" stroke="currentColor" stroke-width="1.3" /></svg>
                {/if}
              </button>
            </div>
          {/if}
          {#if r.st.found}
            <div class="cust">
              <!-- 自定义模型 与 账号 两个折叠头并排一行（默认都收起），展开体各自出现在行下方。
                   账号从各 CLI 本地凭据只读提取（brains.rs）；重新绑定 = headless logout
                   + 原有授权终端流程登录新账号 -->
              <div class="cust-heads">
                <button class="cust-head" type="button" onclick={() => (customOpen[r.b] = !customOpen[r.b])}>
                  <svg class="cust-chev" class:open={customOpen[r.b]} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  自定义模型{#if customsOf(r.b).length}<span class="cust-n">{customsOf(r.b).length}</span>{/if}
                </button>
                {#if r.st.logged_in}
                  <button class="cust-head" type="button" onclick={() => (acctOpen[r.b] = !acctOpen[r.b])}>
                    <svg class="cust-chev" class:open={acctOpen[r.b]} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
                    账号
                    {#if r.b === 'claude' && claudeVerify === 'bad'}
                      <span class="verify-bad">凭据已失效</span>
                    {:else if r.b === 'claude' && claudeVerify === 'checking'}
                      <span class="verify-checking">验证中…</span>
                    {/if}
                  </button>
                {/if}
              </div>
              {#if customOpen[r.b]}
                <div class="cust-body">
                  {#each customsOf(r.b) as id (id)}
                    <div class="cust-chip"><span class="cust-id">{id}</span><button class="cust-x" title="删除" onclick={() => removeCustom(r.b, id)}>×</button></div>
                  {/each}
                  <div class="cust-add">
                    <input class="tin" bind:value={customDraft[r.b]} placeholder={r.b === 'codex' ? '如 gpt-5.5 / o3' : r.b === 'grok' ? '如 grok-4.6' : '如 claude-opus-4-8'}
                      spellcheck="false" autocapitalize="off" autocorrect="off" onkeydown={(e) => e.key === 'Enter' && addCustom(r.b)} />
                    <button class="btn sm" onclick={() => addCustom(r.b)} disabled={!(customDraft[r.b] ?? '').trim()}>添加</button>
                  </div>
                </div>
              {/if}
              {#if acctOpen[r.b] && r.st.logged_in}
                <div class="brain-auth">
                  {#if r.st.account}<span class="brain-acct" title={`当前账号：${r.st.account}`}>{r.st.account}</span>{/if}
                  {#if r.st.account}<span class="brain-auth-sep" aria-hidden="true">·</span>{/if}
                  {#if r.b === 'claude'}
                    <button
                      class="brain-auth-icon"
                      class:active={claudeTokenOpen}
                      type="button"
                      aria-label="更换 Claude 订阅 Token"
                      aria-expanded={claudeTokenOpen}
                      data-tip="更换订阅 Token&#10;粘贴 claude setup-token 生成的长期授权 Token"
                      onclick={() => (claudeTokenOpen = !claudeTokenOpen)}
                    ><svg width="14" height="14" viewBox="0 0 18 18" fill="none" aria-hidden="true"><circle cx="5.5" cy="12.5" r="3.25" stroke="currentColor" stroke-width="1.45"/><path d="m7.85 10.15 6.55-6.55M11.45 6.55l1.7 1.7M13.15 4.85l1.35 1.35" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
                  {/if}
                  <button
                    class="brain-auth-icon"
                    type="button"
                    aria-label={authWaiting === r.b ? '正在重新绑定账号' : '重新绑定账号'}
                    data-tip={authWaiting === r.b ? '正在等待账号授权…' : '退出当前账号并重新登录'}
                    onclick={() => rebind(r.b)}
                    disabled={authWaiting === r.b}
                  >{#if authWaiting === r.b}<span class="wr-spin"></span>{:else}{@render refreshIcon(false)}{/if}</button>
                </div>
              {/if}
            </div>
          {/if}
          </div>
        {/each}
        </div>
      {/if}
      <p class="hint tos">自定义模型 ID 会作为该厂商模型下拉里的附加档位（可加多个）；仅限本人订阅账户驱动本地官方 CLI，密钥存 {keystoreName}。</p>
      <ApiServices profiles={apiProfiles} onchange={(profiles: ApiProfile[]) => { apiProfiles = profiles; }} />

      <div class="sec-head mt"><span>关于</span></div>
      <div class="brain-row">
        <span class="brain-ic"><AppIcon size={20} /></span>
        <span class="brain-main"><b>Cligc Pilot</b><small>{appVersion ? `v${appVersion}` : ''}{updMsg ? ` · ${updMsg}` : ''}</small></span>
        <button class="btn small ghost bare about-update-entry" onclick={openUpdateCenter} disabled={!isMainWindow}>
          {@render updateIcon(14)}
          <span>{updAvail ? `新版本 ${updAvail}` : '检查更新'}</span>
        </button>
      </div>
    </div>
  </div>
{/if}

{#if updateCenterOpen}
  <div class="mask update-center-mask" role="presentation" onclick={closeUpdateCenter}></div>
  <div class="sheet update-center-sheet" use:sheetWidth={SHEET_WIDTH.updateCenter} role="dialog" aria-modal="true" aria-label="更新中心">
    <header class="sheet-head update-center-head">
      <div class="update-center-title">
        <span class="update-center-title-icon" aria-hidden="true">{@render updateIcon(16)}</span>
        <span class="update-center-title-copy">
          <b>更新中心</b>
          <small>{updateCheckedAt ? `上次检查 ${new Date(updateCheckedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}` : '尚未检查'}</small>
        </span>
      </div>
      <div class="update-center-head-actions">
        <button
          type="button"
          class="icon-btn"
          aria-label={pilotUpdateStatus === 'checking' ? '正在重新检查更新' : '重新检查更新'}
          data-tip={pilotUpdateStatus === 'checking' ? '正在重新检查更新…' : '重新检查更新'}
          onclick={() => void checkPilotUpdate(false)}
          disabled={pilotUpdateStatus === 'checking' || updBusy}
        >{@render refreshIcon(pilotUpdateStatus === 'checking')}</button>
        <button class="x" onclick={closeUpdateCenter} disabled={updBusy}>×</button>
      </div>
    </header>
    <div class="sheet-body update-center-body">
      {#if pilotUpdateReceipt}<div class="update-center-notice success">{pilotUpdateReceipt}</div>{/if}
      <section class="update-section">
        <div class="update-item pilot">
          <span class="update-item-icon"><AppIcon size={20} /></span>
          <span class="update-item-copy">
            <b>Cligc Pilot</b>
            <small>{pilotUpdateMessage()}</small>
            {#if pilotReleaseNotes && pilotUpdateStatus === 'available'}<i>{pilotReleaseNotes}</i>{/if}
          </span>
          <span class="update-state {pilotUpdateStatus}" title={pilotUpdateMessage()}>{pilotUpdateLabel(pilotUpdateStatus)}</span>
          {#if pilotUpdateStatus === 'available'}
            <button class="btn small ghost" onclick={() => void runUpdate()} disabled={updBusy}>更新</button>
          {/if}
        </div>
      </section>
      <p class="update-center-footnote">更新包经 ed25519 签名校验后安装，安装完成会自动重启客户端；正在进行的对话会在重启前保存。</p>
    </div>
    <footer class="update-center-footer">
      <button class="btn ghost" onclick={closeUpdateCenter} disabled={updBusy}>关闭</button>
      <button class="btn primary" onclick={() => void runUpdate()} disabled={updBusy || pilotUpdateStatus !== 'available'}>
        {updBusy ? '正在更新…' : updAvail ? `更新到 ${updAvail}` : '当前无需更新'}
      </button>
    </footer>
  </div>
{/if}

<!-- 连接 Cloudflare -->
{#if sshOpen}
  <div class="mask" role="presentation" onclick={() => (sshOpen = false)}></div>
  <div class="modal wide ssh-modal" use:dialogGeometry={DIALOG_GEOMETRY.ssh} role="dialog" aria-modal="true" aria-label={sshCredentialRepair ? '补录远程连接密码' : sshEditId ? '编辑远程连接' : '新建远程连接'}>
    <header class="sheet-head"><b>{sshCredentialRepair ? '补录远程连接密码' : sshEditId ? '编辑远程连接' : '新建远程连接'}</b><button class="x" onclick={() => (sshOpen = false)}>×</button></header>
    <div class="sheet-body">
      <p class="hint">{#if sshCredentialRepair}这条连接的密码已不在 {keystoreName} 中。输入一次密码并测试连接，保存后会自动恢复终端和文件列表。{:else if sshEditId}改完地址 / 用户 / 认证方式后要重新「测试连接」核对指纹再保存；只改名字可以直接保存。密码 / 私钥口令留空＝沿用 {keystoreName} 里已存的那条。{:else}SSH 到一台远程机器：能开终端、管文件、让 AI 帮你干活。密码 / 私钥口令只进 {keystoreName}，绝不落盘。{/if}</p>
      <div class="trow">
        <div class="tfield" style="flex:2"><span>主机</span><input class="tin" bind:value={sshF.host} placeholder="例如 1.2.3.4 或 server.example.com" spellcheck="false" autocapitalize="off" autocorrect="off" /></div>
        <div class="tfield" style="flex:1"><span>端口</span><input class="tin" type="number" bind:value={sshF.port} placeholder="22" /></div>
      </div>
      <div class="trow">
        <div class="tfield"><span>用户名</span><input class="tin" bind:value={sshF.user} placeholder="root" spellcheck="false" autocapitalize="off" autocorrect="off" /></div>
        <div class="tfield"><span>认证方式</span><Dropdown bind:value={sshF.auth} options={sshAuthOpts} /></div>
      </div>
      {#if sshF.auth === 'password'}
        <div class="tfield"><span>密码{#if sshEditId && sshF0.auth === 'password' && !sshCredentialRepair}（留空＝不改）{/if}</span><input class="tin" type="password" bind:value={sshF.password} autocomplete="off" placeholder={sshCredentialRepair ? '请输入这条连接的密码' : sshEditId && sshF0.auth === 'password' ? '不改就别填' : ''} /></div>
      {:else}
        <div class="tfield"><span>私钥文件（只记住路径，不会拷贝你的私钥）</span>
          <div class="ssh-key-row">
            <input class="tin" bind:value={sshF.keyPath} placeholder="~/.ssh/id_ed25519" spellcheck="false" autocapitalize="off" autocorrect="off" />
            <button class="btn sm" onclick={pickSshKey}>选择…</button>
          </div>
        </div>
        <div class="tfield"><span>私钥口令（没有就留空{#if sshEditId && sshF0.auth === 'key'}；不改也留空{/if}）</span><input class="tin" type="password" bind:value={sshF.keyPass} autocomplete="off" /></div>
      {/if}
      <div class="tfield"><span>名称（可选）</span><input class="tin" bind:value={sshF.name} placeholder={sshF.user && sshF.host ? `${sshF.user}@${sshF.host}` : '留空自动用 user@host'} /></div>

      {#if sshErr}<div class="err-note">{sshErr}</div>{/if}

      {#if sshFp}
        <!-- TOFU：指纹必须由人确认过才保存（后端无指纹直接拒）。 -->
        <div class="ssh-fp" class:alarm={sshFpChanged}>
          {#if sshFpChanged}
            <!-- 这台机器的主机密钥变了。可能是重装/换机，也可能是有人在中间——不能轻轻放过。 -->
            <b class="fp-alarm">⚠ 这台机器的指纹变了！</b>
            <small>之前记住的是：</small>
            <code class="fp-old">{sshOldFp}</code>
            <small>现在连上看到的是：</small>
            <code>{sshFp}</code>
            <small>服务器重装 / 换机器会这样，<b>被中间人劫持也会这样</b>。请到服务器上执行
              <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code> 亲自核对；对不上就别保存。</small>
          {:else}
            <b>认证通过。请核对主机指纹：</b>
            <code>{sshFp}</code>
            <small>请确认它和你在服务器上执行 <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code> 看到的一致 —— 对不上可能是中间人。确认后此指纹会被记住，以后变了会直接拒连。</small>
          {/if}
        </div>
      {/if}

      <div class="row-end">
        <button class="btn ghost" onclick={() => (sshOpen = false)}>取消</button>
        {#if sshRenameOnly}
          <!-- 只改了名字：和「能不能连上」无关，不必为此去连一次机器 -->
          <button class="btn primary" onclick={saveSshEdit} disabled={sshConnecting}>{sshConnecting ? '保存中…' : '保存'}</button>
        {:else if !sshFp}
          <button class="btn primary" onclick={testSsh} disabled={sshTesting || !sshCanTest}>{sshTesting ? '连接中…' : '测试连接'}</button>
        {:else if sshEditId}
          <button class="btn primary" class:danger={sshFpChanged} onclick={saveSshEdit} disabled={sshConnecting}>{sshConnecting ? '保存中…' : sshFpChanged ? '我已核对，接受新指纹' : '指纹无误，保存'}</button>
        {:else}
          <button class="btn primary" onclick={confirmSsh} disabled={sshConnecting}>{sshConnecting ? '添加中…' : '指纹无误，添加'}</button>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if connCtx}
  <!-- 连接切换器的右键菜单。菜单项按连接类型给：能做的才列，做不了的不摆在那儿灰着。 -->
  {@const c = connCtx.conn}
  <div bind:this={connCtxMenuEl} class="ctx-menu fctx conn-ctx-menu" class:ready={connCtx.ready} style="left:{connCtx.x}px; top:{connCtx.y}px" role="menu" tabindex="-1">
    <button class="ctx-item" role="menuitem" onclick={() => openConnWindow(c.id)}>新窗口打开</button>
    {#if c.kind === 'ssh'}
      <button class="ctx-item" role="menuitem" onclick={() => { switcherOpen = false; selectConn(c.id); openRemote(); }}>远程终端</button>
      <div class="ctx-div"></div>
      <button class="ctx-item" role="menuitem" onclick={() => { switcherOpen = false; openSshEdit(c); }}>编辑连接…</button>
    {:else}
      <div class="ctx-div"></div>
      <button class="ctx-item" role="menuitem" onclick={() => openConnRemark(c)}>{c.remark?.trim() ? '修改备注…' : '设置备注…'}</button>
      <button class="ctx-item" role="menuitem" onclick={() => openCfUpdate(c)}>更新 Token…</button>
    {/if}
    <div class="ctx-div"></div>
    <button
      class="ctx-item danger"
      role="menuitem"
      data-keep-connection-menu
      onclick={(e) => { e.stopPropagation(); void removeConn(c.id, true); }}
    >删除连接…</button>
  </div>
{/if}

{#if connRemarkEdit}
  <div class="mask" role="presentation" onclick={() => !connRemarkEdit?.busy && (connRemarkEdit = null)}></div>
  <div class="modal conn-remark-modal" use:dialogGeometry={DIALOG_GEOMETRY.connectionRemark} role="dialog" aria-modal="true" aria-label="设置连接备注">
    <header class="sheet-head"><div><b>设置备注</b><small class="dim">{connRemarkEdit.name}</small></div><button class="x" onclick={() => (connRemarkEdit = null)} disabled={connRemarkEdit.busy}>×</button></header>
    <div class="sheet-body">
      <p class="hint">备注会替代列表里的脱敏 Key；鼠标悬停仍可查看原来的脱敏 Key。留空保存即可清除。</p>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="tin" bind:value={connRemarkEdit.value} maxlength="60" placeholder="例如 cms.ccvar.com" spellcheck="false" autocapitalize="off" autocorrect="off" autofocus disabled={connRemarkEdit.busy} onkeydown={(e) => e.key === 'Enter' && void saveConnRemark()} />
      {#if connRemarkEdit.error}<div class="err-note">{connRemarkEdit.error}</div>{/if}
      <div class="row-end">
        <button class="btn ghost" onclick={() => (connRemarkEdit = null)} disabled={connRemarkEdit.busy}>取消</button>
        <button class="btn primary" onclick={() => void saveConnRemark()} disabled={connRemarkEdit.busy || connRemarkEdit.value.trim() === connRemarkEdit.original}>{connRemarkEdit.busy ? '保存中…' : '保存'}</button>
      </div>
    </div>
  </div>
{/if}

{#if fctxMenu}
  <!-- 远程文件右键菜单。挂最外层：列表自己是滚动容器，菜单放里面会被裁掉。 -->
  <div bind:this={fctxMenuEl} class="ctx-menu fctx" style="left:{fctxMenu.x}px; top:{fctxMenu.y}px" role="menu" tabindex="-1">
    {#if fctxMenu.row}
      {@const r = fctxMenu.row}
      <button class="ctx-item" role="menuitem" onclick={() => sftpOpenRow(r)}>{r.dir && !r.link ? '打开' : r.link ? '打开（跟随链接）' : '打开（编辑）'}</button>
      <button class="ctx-item" role="menuitem" onclick={() => sftpDownload(r)} disabled={!!sftpXfer}>下载…</button>
      {#if r.dir && !r.link}<button class="ctx-item" role="menuitem" onclick={() => sftpDownload(r, true)} disabled={!!sftpXfer}>打包下载（.tar.gz）…</button>{/if}
      {#if r.dir && !r.link}
        <button class="ctx-item" role="menuitem" onclick={() => sftpUpload(r.path)} disabled={!!sftpXfer}>上传到这里…</button>
        <button class="ctx-item" role="menuitem" onclick={() => openMkdir(r.path)}>在这里新建文件夹</button>
      {/if}
      <div class="ctx-div"></div>
      <button class="ctx-item" role="menuitem" onclick={() => copySftpEntry(r)}>复制</button>
      <button class="ctx-item" role="menuitem" onclick={() => duplicateSftpEntry(r)} disabled={!!sftpXfer}>复制到当前目录</button>
      {#if r.dir && !r.link && sftpClipboard}
        <button class="ctx-item" role="menuitem" onclick={() => pasteSftpEntry(r.path)} disabled={!!sftpXfer || sftpClipboard.connId !== sftpConnId}>粘贴到这里</button>
      {/if}
      <button class="ctx-item" role="menuitem" onclick={() => copyPath(r.path)}>复制路径</button>
      <button class="ctx-item" role="menuitem" onclick={() => openRename(r)}>重命名</button>
      <div class="ctx-div"></div>
      <button class="ctx-item danger" role="menuitem" onclick={() => sftpDelete(r)}>删除…</button>
    {:else}
      <button class="ctx-item" role="menuitem" onclick={() => openMkdir()}>新建文件夹</button>
      <button class="ctx-item" role="menuitem" onclick={() => openNewFile()}>新建文件</button>
      <button class="ctx-item" role="menuitem" onclick={() => sftpUpload()} disabled={!!sftpXfer}>上传文件…</button>
      <div class="ctx-div"></div>
      {#if sftpClipboard}
        <button class="ctx-item" role="menuitem" onclick={() => pasteSftpEntry()} disabled={!!sftpXfer || sftpClipboard.connId !== sftpConnId}>粘贴</button>
      {/if}
      <button class="ctx-item" role="menuitem" onclick={() => copyPath(sftpPath)}>复制当前路径</button>
      <button class="ctx-item" role="menuitem" onclick={sftpRefresh}>刷新</button>
    {/if}
  </div>
{/if}

{#if fsAsk}
  <div class="mask" role="presentation" onclick={() => !fsAsk?.busy && (fsAsk = null)}></div>
  <div class="modal" use:dialogGeometry={DIALOG_GEOMETRY.fileOperation} role="dialog" aria-modal="true" aria-label={fsAsk.mode === 'mkdir' ? '新建文件夹' : '新建文件'}>
    <header class="sheet-head"><b>{fsAsk.mode === 'mkdir' ? '新建文件夹' : '新建文件'}</b><button class="x" onclick={() => (fsAsk = null)} disabled={fsAsk.busy}>×</button></header>
    <div class="sheet-body">
      <p class="hint">位置：<code>{fsAsk.dir}</code></p>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="tin" bind:value={fsAsk.value} placeholder={fsAsk.mode === 'mkdir' ? '文件夹名' : '文件名（例如 notes.txt）'} spellcheck="false" autocapitalize="off" autocorrect="off" autofocus disabled={fsAsk.busy} onkeydown={(e) => e.key === 'Enter' && confirmFsAsk()} />
      {#if fsAsk.err}<div class="err-note">{fsAsk.err}</div>{/if}
      <div class="row-end">
        <button class="btn ghost" onclick={() => (fsAsk = null)} disabled={fsAsk.busy}>取消</button>
        <button class="btn primary" onclick={confirmFsAsk} disabled={fsAsk.busy || !fsAsk.value.trim()}>{fsAsk.busy ? '处理中…' : '确定'}</button>
      </div>
    </div>
  </div>
{/if}

{#if edOpen}
  <div class="mask" role="presentation" onclick={() => void closeEdit()}></div>
  <div class="modal ed-modal" class:ed-full={edFull} use:dialogGeometry={{ ...DIALOG_GEOMETRY.fileEditor, enabled: !edFull }} role="dialog" aria-modal="true" aria-label={`编辑文件 ${edPath}`} aria-busy={edSaving}>
    <header class="sheet-head ed-head">
      <div class="ed-heading"><div><b>编辑文件</b><span class="ed-badge" class:dirty={edDirty}>{edDirty ? '未保存' : edSaved ? '已保存' : '远程文件'}</span></div><small class="dim ed-path" title={edPath}>{edPath}</small></div>
      <button type="button" class="ed-expand" title={edFull ? '退出全屏' : '全屏编辑'} aria-label={edFull ? '退出全屏' : '全屏编辑'} aria-pressed={edFull} onclick={() => { edFull = !edFull; }}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          {#if edFull}
            <path d="M3 8h5V3M16 3v5h5M21 16h-5v5M8 21v-5H3" />
          {:else}
            <path d="M8 3H3v5M16 3h5v5M21 16v5h-5M8 21H3v-5" />
          {/if}
        </svg>
      </button>
      <button class="x" aria-label="关闭文件编辑器" onclick={() => void closeEdit()} disabled={edSaving || edFormatting}>×</button>
    </header>
    <div class="sheet-body ed-body">
      {#if edLoading}
        <div class="files-empty">读取中…</div>
      {:else if edErr && edReadOnly}
        <div class="err-note">{edErr}</div>
      {:else}
        <FileCodeEditor bind:value={edText} path={edPath} indent={edFormat.indent} readonly={edSaving || edClosing} bind:busy={edFormatting}
          eol={`UTF-8${edFormat.bom ? ' BOM' : ''} · ${edFormat.mixed ? '混合换行' : edFormat.eol === '\r\n' ? 'CRLF' : edFormat.eol === '\r' ? 'CR' : 'LF'}`} onsave={() => void saveEdit()} />
        {#if edErr}<div class="err-note">{edErr}</div>{/if}
      {/if}
    </div>
    <div class="ed-footer">
      <span class="ed-save-note">{edSaving ? '正在保存到服务器…' : '仅在点击保存时写入远端；不会自动格式化。'}</span>
      <button type="button" class="btn ghost" onclick={() => void closeEdit()} disabled={edSaving || edFormatting}>关闭</button>
      {#if !edReadOnly && !edLoading}
        <button type="button" class="btn primary" onclick={saveEdit} disabled={edSaving || edFormatting || edClosing || !edDirty}>{edSaving ? '保存中…' : '保存'}</button>
      {/if}
    </div>
  </div>
{/if}

{#if cfOpen}
  {@const updatingCf = cfUpdateConnection()}
  <div class="mask cf-mask" role="presentation" onclick={closeCfConnect}></div>
  <div class="modal cf-modal" use:dialogGeometry={DIALOG_GEOMETRY.cloudflare} role="dialog" aria-modal="true" aria-label={updatingCf ? '更新 Cloudflare 连接' : '连接 Cloudflare'}>
    <header class="sheet-head"><div><b>{updatingCf ? '更新' : '连接'} {@render cfMark(15)} Cloudflare</b><small class="dim" style="margin-left:10px">token 只进钥匙串，绝不落盘</small></div><button class="x" onclick={closeCfConnect} disabled={cfConnecting || cfVerifying}>×</button></header>
    <div class="sheet-body">
      {#if updatingCf}
        <div class="cf-update-target">
          <span>{@render cfMark(17)}</span>
          <div><b>{updatingCf.remark?.trim() || updatingCf.name}</b><small>{updatingCf.remark?.trim() ? `${updatingCf.name} · ` : ''}{updatingCf.key_prefix} · 账号 {updatingCf.account_id}</small></div>
        </div>
        <p class="hint">不用在已有的很多 Token 中猜哪一个需要修改。建议直接为这条 Pilot 连接新建一枚权限完整的 Token；验证成功后只替换这一条连接，其他连接不受影响。</p>
      {/if}
      <div class="cf-step">
        <p class="cf-step-t">① {updatingCf ? '为这条连接新建一枚 API Token' : '在 Cloudflare 建一个 API Token'}（下列权限已预选）</p>
        <button class="cf-perms-toggle" type="button" onclick={() => (cfPermsOpen = !cfPermsOpen)}>
          <svg class="cf-chev" class:open={cfPermsOpen} width="10" height="10" viewBox="0 0 12 12" fill="none"><path d="M3 4.5 6 7.5 9 4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
          所需权限（5 项）
        </button>
        {#if cfPermsOpen}
          <ul class="cf-perms">
            <li>Account · Cloudflare Pages · <b>Edit</b></li>
            <li>Account · Workers Scripts · <b>Edit</b></li>
            <li>Account · D1 · <b>Edit</b></li>
            <li>Zone · DNS · <b>Edit</b>（绑定自定义域名用）</li>
            <li>Zone · Zone · <b>Read</b></li>
          </ul>
        {/if}
        <button class="btn soft" onclick={() => openUrl(cfTokenTemplateUrl())}>{updatingCf ? '为此连接新建' : '打开'} {@render cfMark(14)} Cloudflare Token ↗</button>
      </div>
      <div class="cf-step">
        <p class="cf-step-t">② 把{updatingCf ? '新的' : '生成的'} Token 粘到这里：</p>
        <div class="cf-row">
          <input class="tin" type="password" bind:value={cfToken} placeholder="粘贴 API Token" autocomplete="off" spellcheck="false" oninput={resetCfVerification} onkeydown={(e) => e.key === 'Enter' && verifyCf()} />
          <button class="btn small primary" onclick={verifyCf} disabled={cfVerifying || !cfToken.trim()}>{cfVerifying ? '验证中…' : '验证'}</button>
        </div>
      </div>
      {#if cfAccounts.length}
        {#if updatingCf}
          <div class="cf-update-verified" class:ok={cfAccounts.some((account) => account.id === updatingCf.account_id)}>
            <span>{cfAccounts.some((account) => account.id === updatingCf.account_id) ? '✓' : '!'}</span>
            <div><b>{cfAccounts.some((account) => account.id === updatingCf.account_id) ? '已确认属于原 Cloudflare 账号' : '不是原 Cloudflare 账号'}</b><small>保存后连接名称和已有设置保持不变。</small></div>
          </div>
        {:else}
          <div class="cf-step">
            <p class="cf-step-t">③ 选择账号 · 给连接起个名：</p>
            <Dropdown bind:value={cfAccountId} options={cfAccounts.map((a) => ({ value: a.id, label: a.name, sub: a.id }))} placeholder="选择账号" />
            <input class="tin" style="margin-top:7px" bind:value={cfName} placeholder="连接名称（可留默认）" />
            {#if cfZones.length}<p class="hint">可管理域名：{cfZones.slice(0, 6).map((z) => z.name).join('、')}{cfZones.length > 6 ? ` 等 ${cfZones.length} 个` : ''}</p>{:else}<p class="hint">没检测到可管理域名——绑定自定义域名需 Zone 权限，可稍后补 token。</p>{/if}
          </div>
        {/if}
      {/if}
      {#if brains?.wrangler && !brains.wrangler.found}{@render wrNote()}{/if}
      {#if cfErr}<div class="err-note">{cfErr}</div>{/if}
      <div class="row-end">
        <button class="btn small ghost" onclick={closeCfConnect} disabled={cfConnecting || cfVerifying}>取消</button>
        <button class="btn small primary" onclick={confirmCf} disabled={cfConnecting || !cfAccountId || !cfAccounts.some((account) => account.id === cfAccountId)}>{cfConnecting ? (updatingCf ? '更新中…' : '连接中…') : updatingCf ? '更新此连接' : '连接'}</button>
      </div>
    </div>
  </div>
{/if}

<!-- 存为模板 -->
{#if saveTmplOpen}
  <div class="mask" role="presentation" onclick={() => !saveTmplBusy && (saveTmplOpen = false)}></div>
  <div class="modal" use:dialogGeometry={DIALOG_GEOMETRY.templateSave} role="dialog" aria-modal="true" aria-label="存为模板">
    <header class="sheet-head"><b>存为模板</b><button class="x" onclick={() => (saveTmplOpen = false)} disabled={saveTmplBusy}>×</button></header>
    <div class="sheet-body">
      <p class="hint">当前项目会被复制成模板（自动去掉密钥、依赖、构建产物），之后可引用它快速建站。</p>
      <input class="tin" bind:value={saveTmplName} placeholder="模板名，如 minimal-landing" spellcheck="false" />
      <input class="tin" style="margin-top:7px" bind:value={saveTmplDesc} placeholder="一句话描述（可选）" />
      {#if saveTmplErr}<div class="err-note">{saveTmplErr}</div>{/if}
      <div class="row-end">
        <button class="btn ghost" onclick={() => (saveTmplOpen = false)} disabled={saveTmplBusy}>取消</button>
        <button class="btn primary" onclick={confirmSaveTmpl} disabled={saveTmplBusy || !saveTmplName.trim()}>{saveTmplBusy ? '保存中…' : '存为模板'}</button>
      </div>
    </div>
  </div>
{/if}

<!-- 用模板建站 -->
{#if useTmplOpen}
  <div class="mask" role="presentation" onclick={() => !useTmplBusy && (useTmplOpen = false)}></div>
  <div class="modal" use:dialogGeometry={DIALOG_GEOMETRY.templateUse} role="dialog" aria-modal="true" aria-label="用模板建站">
    <header class="sheet-head"><div><b>用模板建站</b><small class="dim" style="margin-left:10px">{useTmplName}</small></div><button class="x" onclick={() => (useTmplOpen = false)} disabled={useTmplBusy}>×</button></header>
    <div class="sheet-body">
      {#if !isCfConn}<p class="hint warn-text">请先在左下角切到一个 Cloudflare 连接，再用模板建站。</p>{/if}
      <p class="hint">给新项目起个名，模板文件会拷进去，然后在对话里描述你的定制。</p>
      <input class="tin" bind:value={useTmplProject} placeholder="新项目名，如 my-landing" spellcheck="false" onkeydown={(e) => e.key === 'Enter' && confirmUseTmpl()} />
      {#if useTmplErr}<div class="err-note">{useTmplErr}</div>{/if}
      <div class="row-end">
        <button class="btn ghost" onclick={() => (useTmplOpen = false)} disabled={useTmplBusy}>取消</button>
        <button class="btn primary" onclick={confirmUseTmpl} disabled={useTmplBusy || !useTmplProject.trim() || !isCfConn}>{useTmplBusy ? '创建中…' : '创建项目'}</button>
      </div>
    </div>
  </div>
{/if}

<!-- 提示词 新建/编辑 -->
{#if promptEditOpen}
  <div class="mask" role="presentation" onclick={() => (promptEditOpen = false)}></div>
  <div class="modal wide" use:dialogGeometry={DIALOG_GEOMETRY.promptEditor} role="dialog" aria-modal="true" aria-label={promptEditId ? '编辑提示词' : '新增提示词'}>
    <header class="sheet-head"><b>{promptEditId ? '编辑提示词' : '新增提示词'}</b><button class="x" onclick={() => (promptEditOpen = false)}>×</button></header>
    <div class="sheet-body">
      <input class="tin" bind:value={promptEditTitle} placeholder="标题，如 手冲咖啡落地页" spellcheck="false" />
      <textarea class="tin" style="margin-top:7px; min-height:150px; line-height:1.5" bind:value={promptEditBody} placeholder="写清楚你要什么样的网站：给谁看、要哪些区块、风格方向，合适的话让它把表单存进 D1，结尾让它先给方案。"></textarea>
      <div class="row-end">
        <button class="btn ghost" onclick={() => (promptEditOpen = false)}>取消</button>
        <button class="btn primary" onclick={savePrompt} disabled={!promptEditTitle.trim() || !promptEditBody.trim()}>保存</button>
      </div>
    </div>
  </div>
{/if}


{#if confirmState}
  <ConfirmDialog
    title={confirmState.title}
    message={confirmState.message}
    kind={confirmState.kind}
    confirmText={confirmState.confirmText}
    cancelText={confirmState.cancelText}
    onConfirm={() => finishConfirm(true)}
    onCancel={() => finishConfirm(false)}
  />
{/if}

<style>
  .pane-close{position:static;flex:none;align-self:center}
  .thread-head.conversation-thread-head{align-items:center}
  .thread-head.multi{padding:7px 16px;min-height:38px;align-items:center}
  .thread-head.multi .th-info{display:block;min-width:0}
  .conversation-head-title{display:flex!important;align-items:center;gap:7px;width:100%;min-width:0}
  .conversation-head-title>span:last-child{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .conversation-head-model{display:inline-flex;align-items:center;flex:none;line-height:0}
  .conversation-head-model :global(.bi){display:block}
  .conversation-site-link{max-width:52%;min-width:0;flex:none}
  .conversation-site-link>span{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .conversation-site-link.static{cursor:default;color:var(--dim)}
  .thread-head .th-info[role="button"]{cursor:grab}
  .pane-error{color:var(--danger,#ad3228);font-size:13px;margin:8px 16px;overflow-wrap:anywhere}
  :global(.pane) .thread{min-height:0;overscroll-behavior:contain}
  :global(.canvas.multi .pane) .thread-inner{padding:18px 20px 8px}
  :global(.canvas.multi .pane) .composer-wrap{padding:8px 16px 14px;max-height:55%;overflow-y:auto}
  :global(.canvas.multi .pane) .composer-bar{flex-wrap:wrap}
  :global(.canvas.multi .pane) .cb-right{margin-left:auto;max-width:100%;flex-wrap:wrap}
  .conversation-drag-ghost{position:fixed;z-index:1000;left:clamp(12px,calc(var(--drag-x) + 14px),calc(100vw - 312px))!important;top:clamp(12px,calc(var(--drag-y) + 12px),calc(100vh - 50px))!important;width:300px;max-width:calc(100vw - 24px);height:38px;box-sizing:border-box;display:flex;align-items:center;gap:8px;padding:0 12px;background:color-mix(in srgb,var(--bg,#fff) 96%,transparent);border:1px solid var(--border,#e7e5df);border-radius:8px;box-shadow:0 8px 24px #2d29231c;pointer-events:none;color:var(--fg,#282723)}
  .conversation-drag-icon{display:inline-flex;align-items:center;line-height:0;flex:none}
  .conversation-drag-title{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px;font-weight:500}
  .single-split-preview{position:absolute;z-index:100;pointer-events:none;box-sizing:border-box;display:grid;place-items:center;background:color-mix(in srgb,var(--bg,#fff) 60%,transparent);backdrop-filter:blur(4px);-webkit-backdrop-filter:blur(4px);border:2px solid #2f80ed;border-radius:8px;box-shadow:0 0 0 1px #2f80ed24}
  .single-split-preview.left{inset:0 50% 0 0}.single-split-preview.right{inset:0 0 0 50%}.single-split-preview.top{inset:0 0 50%}.single-split-preview.bottom{inset:50% 0 0}
  .single-split-preview span{padding:6px 12px;border-radius:18px;background:#2f80ed;color:#fff;font-size:12px;font-weight:500;box-shadow:0 4px 14px #1a5ca733}

  /* 远程工作台：终端主区 + 底部对话 + 右侧文件（VS Code 那套）。
     每层都 min-*:0 —— flex 子项默认 min-size:auto，不清零的话终端撑着不缩、面板挤不出来。 */
  .wb { flex: 1; min-height: 0; display: flex; }
  .wb-main { flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  /* 文件栏单开：主区收起（终端只藏不拆，DOM 还在），让文件栏独占整宽。 */
  .wb-main.collapsed { display: none; }
  /* 终端与对话的接缝不再用 border-top 那条黑白硬边，改由上面 .term-wrap.with-chat 的圆底+柔和阴影收口 */
  .wb-chat { flex: none; min-height: 0; display: flex; flex-direction: column; background: var(--bg); }
  .wb-chat.solo { flex: 1; }
  /* 起点输入框：贴着面板底（justify-content:flex-end），面板矮时也不顶头。
     ★ .composer 必须显式 width:100% —— 它自带 `margin:0 auto`，而这里是 flex 列容器，
     交叉轴上的 auto margin 会**压制 stretch**，不给宽度它就缩成 textarea 的默认宽（~192px）。
     对话页那边的 .composer-wrap 是普通块容器，没这问题。 */
  .wb-start { flex: 1; min-height: 0; overflow-y: auto; display: flex; flex-direction: column; justify-content: flex-end; gap: 6px; }
  .wb-start .composer { width: 100%; }
  .wb-start .hint { width: 100%; max-width: 760px; margin: 0 auto; }
  /* 分隔线：命中区比看到的粗，好抓 */
  .wb-split { flex: none; background: transparent; z-index: 2; }
  .wb-split.h { height: 5px; margin-bottom: -5px; cursor: row-resize; }
  .wb-split.v { width: 5px; margin-right: -5px; cursor: col-resize; }
  .wb-split:hover { background: var(--accent-soft); }
  /* 面板开关（头部两枚 VS Code 式图标）：不给底，开关状态只用图标本身的深浅表示 */
  .wb-tg { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: 0; background: transparent; color: var(--faint); cursor: pointer; -webkit-app-region: no-drag; }
  .wb-tg:hover { color: var(--dim); }
  .wb-tg.on { color: var(--text); }
  .wb-tg.on:hover { color: var(--accent-h); }
  /* 工作台头部：单行，比常规 thread-head 矮一截。
     ★ margin-top:0 是必须的 —— `.th-info small` 那 2px 是给它上面那行标题留的空隙，
     而这里标题已经去掉了，留着就是净偏移，文字会比右边的图标低 2px（对不上一条水平线）。 */
  /* 高度对齐左上角那排工具图标：`.win-tools` 是 fixed/top:0/height:30px，中心在 15px。
     这里最高的子元素是 24px 的面板开关 → 上下各留 3px 正好 30px，中心也落在 15px，
     于是「地址行 / 刷新键 / 三枚开关 / 折叠侧栏 / 搜索」全在同一条水平线上。 */
  /* 这条**必须自己写回 align-items:center**：上面 .thread-head 改成了 flex-start（为了让标题行
     对齐 15px 的图标带），而 slim 是靠 center 把「地址行 / 刷新键 / 三枚开关」这些不等高的子元素
     一起压到 15px 那条线上的 —— 被 flex-start 带走就全顶到 3px 去了。 */
  .thread-head.slim { padding-top: 3px; padding-bottom: 3px; align-items: center; container-type: inline-size; }
  /* 选择器要带上 .th-info small，否则压不过 `.th-info small` 那条（它带元素选择器，分更高）——
     实测：只写 .rhead-line 的话 margin-top 仍是 2px，文字比图标低 1px。 */
  .th-info small.rhead-line { display: flex; align-items: center; gap: 5px; margin-top: 0; }
  /* 终端：自己管滚动，容器不能给 overflow-y:auto（会和 xterm 打架）。 */
  .term-wrap { flex: 1; min-height: 0; overflow: hidden; background: #1c1917; padding: 8px 10px; }
  /* 关掉命令行＝只藏不拆：DOM 留着，xterm 和 SSH 会话都不掉，开回来立刻还在。 */
  .term-wrap.hid { display: none; }
  /* 「正在连接」浮层：盖在终端上，连上即消失，不进终端缓冲（所以不会留下擦不掉的一行）。 */
  .term-wrap { position: relative; }
  /* 终端**在对话框上方**时（with-chat）把接缝做软：一道朝下的柔和阴影落在对话面板上，
     替掉原来黑白直接相撞的硬边（不收圆角——直角保持齐整）。阴影几乎只朝下（负 spread 收掉左右溢出，
     正 offset 让顶边不糊）。z-index 让阴影压在对话面板之上（对话是后面的兄弟，默认会盖住它）。
     终端独占/无对话时不加，保持齐边。 */
  .term-wrap.with-chat { z-index: 1; box-shadow: 0 6px 12px -6px rgba(20, 16, 13, .4); }
  .term-connecting { position: absolute; left: 14px; top: 12px; display: flex; align-items: center; gap: 7px; color: #8b857c; font-size: 12px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; pointer-events: none; }
  .term-credential-repair { position: absolute; left: 14px; top: 12px; border: 1px solid rgba(248, 113, 113, .35); border-radius: 7px; background: rgba(127, 29, 29, .3); color: #fca5a5; padding: 6px 9px; font: 12px ui-monospace, SFMono-Regular, Menlo, monospace; cursor: pointer; }
  .term-credential-repair:hover { background: rgba(153, 27, 27, .42); color: #fecaca; }
  .tc-spin { width: 9px; height: 9px; border: 1.5px solid #4b4640; border-top-color: #8b857c; border-radius: 50%; animation: spin .7s linear infinite; }
  .term { width: 100%; height: 100%; }
  /* 远程视图头部：终端/文件 页签 */
  .rhead-acts { display: flex; align-items: center; gap: 10px; -webkit-app-region: no-drag; }
  /* 远程文件面板（工作台右栏；宽度由 wb.filesW 内联给） */
  .files-wrap { position: relative; flex: none; min-height: 0; display: flex; flex-direction: column; border-left: 1px solid var(--border); background: var(--bg); }
  .files-drop-overlay { position: absolute; inset: auto 8px 8px; z-index: 40; display: flex; align-items: center; gap: 8px; padding: 8px 10px; border: 1px solid var(--border2); border-radius: 6px; background: var(--bg); color: var(--text); pointer-events: none; box-shadow: 0 2px 8px #0001; }
  .files-drop-overlay b { flex: none; font-size: 12px; font-weight: 500; }
  .files-drop-overlay small { min-width: 0; overflow: hidden; color: var(--dim); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  /* 单开时吃满整宽（撤掉固定宽 flex:none）；左边没东西了，左边框也去掉。 */
  .files-wrap.solo { flex: 1; min-width: 0; border-left: 0; }
  .files-bar { flex: none; display: flex; align-items: center; gap: 6px; padding: 10px 16px; border-bottom: 1px solid var(--border); }
  .fpath { flex: 1; min-width: 0; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; }
  /* 面包屑：不画框（它是一行路径，不是输入框——要输入点空白处会切成真的 input），
     撑满路径栏，路径深了横向滚动 */
  .fcrumbs { flex: 1; min-width: 0; display: flex; align-items: center; gap: 1px; overflow-x: auto; overflow-y: hidden; scrollbar-width: none; cursor: text; padding: 4px 2px; }
  .fcrumbs::-webkit-scrollbar { display: none; }
  .fcrumb { flex: none; border: 0; background: transparent; color: var(--dim); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; padding: 2px 5px; border-radius: 5px; cursor: pointer; white-space: nowrap; }
  .fcrumb:hover:not(:disabled) { background: var(--accent-soft); color: var(--text); }
  .fcrumb.cur { color: var(--text); font-weight: 500; }
  .fcrumb-sep { flex: none; color: var(--faint); font-size: 11px; user-select: none; }
  /* 面包屑尾巴的空白＝一个撑满剩余宽度的按钮（点它进输入模式）。min-width 保证路径很长时也还点得着。 */
  .fcrumb-blank { flex: 1; min-width: 28px; align-self: stretch; border: 0; background: transparent; cursor: text; border-radius: 5px; }
  .fcrumb-blank:focus-visible { outline: 2px solid var(--accent-soft); outline-offset: -2px; }
  .fbtn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; flex: none; border: 1px solid var(--border); background: var(--bg); color: var(--dim); border-radius: 6px; cursor: pointer; }
  .fbtn :global(svg) { width: 13px; height: 13px; }
  .fbtn:hover:not(:disabled) { color: var(--text); border-color: var(--border2); }
  .fbtn:disabled { opacity: .45; cursor: default; }
  .fbtn.sm { width: 22px; height: 22px; border: 0; border-radius: 5px; }
  .fbtn.sm:hover:not(:disabled) { background: var(--accent-soft); }
  .fbtn.danger:hover:not(:disabled) { color: var(--err); background: var(--err-soft); }
  /* 现在是个重试按钮：清掉按钮默认样式，只留 err-note 的红底红字 + 左对齐 + 手型 */
  .files-err { flex: none; margin: 10px 16px 0; display: block; width: calc(100% - 32px); text-align: left; font: inherit; font-size: 13px; cursor: pointer; }
  .files-err:hover { background: var(--err-border); }
  /* 列表＝四列表格（名称/权限/大小/日期），行内缩进出树形。列宽用同一套变量对齐表头与行。
     ★ 窄面板里按容器宽度砍列：右栏默认才 380px，四列定宽合计就 300px，名称会被挤成 0
     （文件名全没了，只剩权限/大小/日期——那就本末倒置了）。名称永远优先。
     用容器查询而不是媒体查询：这里要看的是**面板自己的宽度**，不是窗口宽度（面板还能拖）。 */
  .files-wrap { --fw-perm: 96px; --fw-size: 76px; --fw-date: 128px; container-type: inline-size; }
  .fhead { flex: none; display: flex; align-items: stretch; gap: 0; padding: 0 14px; border-bottom: 1px solid var(--border); background: var(--rail); }
  .fh { position: relative; display: flex; align-items: center; gap: 3px; border: 0; background: transparent; color: var(--dim); font-size: 11.5px; padding: 5px 6px; text-align: left; }
  .fhead button.fh { cursor: pointer; }
  .fhead button.fh:hover { color: var(--text); }
  .fh + .fh { border-left: 1px solid var(--border); }
  /* 列宽手柄：骑在列头分隔线上，比线宽一点好抓 */
  .fh-grip { position: absolute; left: -4px; top: 0; bottom: 0; width: 8px; padding: 0; border: 0; background: transparent; cursor: col-resize; z-index: 1; }
  .fh-grip:hover { background: linear-gradient(to right, transparent 2px, var(--border2) 2px, var(--border2) 6px, transparent 6px); }
  .fh-grip:focus-visible { outline: 2px solid var(--accent-soft); outline-offset: -2px; }
  .fh-name { flex: 1; min-width: 0; }
  .fh-perm { width: var(--fw-perm); flex: none; }
  .fh-size { width: var(--fw-size); flex: none; justify-content: flex-end; }
  .fh-date { width: var(--fw-date); flex: none; }
  .fh-ar { font-size: 10px; line-height: 1; }
  .fh-ar.desc { transform: rotate(180deg); }
  .files-list { flex: 1; min-height: 0; overflow-y: auto; padding: 2px 6px 14px; }
  .files-empty { padding: 28px 0; text-align: center; color: var(--faint); font-size: 13px; }
  .frow { display: flex; align-items: center; padding: 0 8px; border-radius: 6px; cursor: default; user-select: none; }
  .frow:hover { background: var(--rail); }
  .frow.on { background: var(--accent-soft); }
  .frow.drop-target { background: var(--accent-soft); outline: 1px solid var(--border2); outline-offset: -1px; }
  .frow.deleting, .frow.deleting:hover, .frow.deleting.on { background: var(--rail); opacity: .52; cursor: progress; }
  .fname { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; color: var(--text); font-size: 13px; padding: 5px 0; }
  .fname-t { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; border: 0; padding: 0; background: transparent; color: inherit; font: inherit; text-align: left; cursor: default; }
  .file-rename { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 3px; }
  .file-rename input { min-width: 0; width: 100%; box-sizing: border-box; border: 1px solid var(--border2); border-radius: 3px; padding: 1px 3px; background: var(--bg, white); color: var(--text); font: inherit; }
  .file-rename input:focus { outline: 1px solid var(--dim); }
  .file-rename small { color: #c53b32; font-size: 11px; white-space: normal; }
  .fdelete-state { flex: none; display: inline-flex; align-items: center; gap: 4px; color: var(--dim); font-size: 11.5px; white-space: nowrap; }
  .fdelete-spin { width: 9px; height: 9px; border: 1.4px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: rspin .7s linear infinite; }
  .fchev, .fchev-sp { width: 14px; height: 14px; flex: none; }
  .fchev { display: inline-flex; align-items: center; justify-content: center; border: 0; background: transparent; color: var(--faint); border-radius: 4px; cursor: pointer; padding: 0; transition: transform .12s; }
  .fchev:hover { color: var(--text); }
  .fchev.open { transform: rotate(90deg); }
  .fspin { width: 8px; height: 8px; border: 1.4px solid var(--border2); border-top-color: var(--dim); border-radius: 50%; animation: spin .7s linear infinite; }
  .fic { flex: none; color: var(--faint); }
  .fic.dir { color: #4a90d9; }
  .fic.link { color: #d98a2b; }
  .fperm { width: var(--fw-perm); flex: none; padding: 0 6px; color: var(--dim); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; }
  .fsize { width: var(--fw-size); flex: none; padding: 0 6px; text-align: right; color: var(--dim); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .fdate { width: var(--fw-date); flex: none; padding: 0 6px; color: var(--dim); font-size: 11.5px; font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  /* ★ 砍列规则必须放在列样式**之后**：@container 不加优先级，和 `.fh{display:flex}` 同分，
     写在前面会被它盖掉（实测过：380px 下权限列照样 96px）。 */
  @container (max-width: 560px) { .fh-perm, .fperm { display: none; } }
  @container (max-width: 400px) { .fh-date, .fdate { display: none; } }
  @container (max-width: 300px) { .fh-size, .fsize { display: none; } }
  /* 远程文件右键菜单：复用全局 .ctx-menu/.ctx-item 的样式，只补两处差异 */
  .fctx { min-width: 172px; }
  .fctx .ctx-item { justify-content: flex-start; }
  .ctx-item.danger { color: var(--err); }
  .ctx-item.danger:hover:not(:disabled) { background: var(--err-soft); }
  /* 在线编辑器 */
  .modal.ed-modal { width: min(1080px, 94vw); height: min(760px, 88vh); }
  .ed-modal.ed-full { inset: 38px 8px 8px; width: auto; height: auto; max-width: none; max-height: none; margin: 0; }
  .ed-head { gap: 10px; }
  .ed-heading { min-width: 0; flex: 1; display: grid; gap: 4px; }
  .ed-heading > div { display: flex; align-items: center; gap: 10px; }
  .ed-badge { color: var(--dim); background: var(--rail); border-radius: 5px; padding: 2px 6px; font-size: 10px; }
  .ed-badge.dirty { color: var(--accent); background: var(--accent-soft); }
  .ed-expand { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 30px; height: 30px; padding: 0; border: 0; border-radius: 5px; background: transparent; box-shadow: none; color: var(--dim); cursor: pointer; }
  .ed-expand:hover { background: var(--rail); color: var(--text); }
  .ed-expand:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .ed-path { display: block; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sheet-body.ed-body { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; gap: 0; overflow: hidden; padding: 0; }
  .ed-body > .err-note { margin: 10px 14px; max-height: 100px; overflow: auto; flex: none; }
  .ed-save-note { margin-right: auto; color: var(--dim); font-size: 11px; }
  .ed-footer { position: relative; z-index: 1; flex: none; display: flex; justify-content: flex-end; align-items: center; gap: 7px; padding: 10px 18px 14px; border-top: 1px solid var(--border); background: var(--bg); }
  /* 远端系统信息（页脚可点＝重新检测；下拉/设置里只读） */
  .foot-os, .cs-os { display: inline-flex; align-items: center; gap: 4px; min-width: 0; }
  .foot-os { cursor: pointer; border-radius: 5px; padding: 0 3px; margin-left: -3px; }
  .foot-os:hover { background: var(--accent-soft); color: var(--text); }
  .foot-os :global(svg.distro) { transform: translateY(1px); }
  .cs-os { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* ★ 上移 1px 不是玄学：align-items:center 对齐的是**盒子**，而文字行盒底下有一截没人用的
     降部空间（Ubuntu/22.04 这类没有 g/p/y），于是「看得见的字」整体偏上、图标按盒居中就显得低。
     发行版图标是纯圆/纯环，没有基线可言，只能按视觉中线手动补这 1px。 */
  :global(svg.distro) { flex: none; transform: translateY(-1px); }
  /* 头部的「重新连接」：只留图标，紧跟在连接状态后面 */
  .th-rfz { display: inline-flex; align-items: center; justify-content: center; width: 15px; height: 15px; border: 0; background: transparent; color: var(--faint); cursor: pointer; padding: 0; -webkit-app-region: no-drag; }
  .th-rfz :global(svg) { width: 12px; height: 12px; }
  .th-rfz:disabled { cursor: default; color: var(--dim); }
  /* 顶栏的远端负载：三个小读数。
     ★ 做得很紧（不画迷你条）是有原因的：应用最小窗口 820px、侧栏 240 → 头部只剩 532px，
     地址+刷新(215) + 三枚开关(84) + 间距(20) 已占 319，留给负载的只有 213px。
     带条的版本要 290px —— 于是在最小窗口下永远塞不下。 */
  /* 分隔线用伪元素画：直接给容器 border-left 会顶满整行高，太抢眼；这里只画 11px 的一小截。 */
  .hstats { position: relative; display: flex; align-items: center; gap: 9px; margin-left: 5px; padding-left: 10px; }
  .hstats::before { content: ''; position: absolute; left: 0; top: 50%; transform: translateY(-50%); width: 1px; height: 11px; background: var(--border2); }
  .hstat { display: inline-flex; align-items: center; gap: 3px; font-size: 11px; color: var(--dim); white-space: nowrap; cursor: default; }
  .hstat-l { color: var(--faint); }
  .hstat-v { font-variant-numeric: tabular-nums; }
  .hstat.warn { color: var(--warn); font-weight: 500; }
  .hstat.hot { color: var(--err); font-weight: 500; }
  /* 真挤不下了才收起（比如侧栏被拖到很宽）。看的是**头部自己的宽度**，不是窗口宽 ——
     侧栏可拖 190~460，窗口宽说明不了头部还剩多少。
     ★ 必须写在 .hstats 之后：@container 不加优先级，同分靠后取胜（上次在文件列表踩过）。 */
  @container (max-width: 470px) { .hstats { display: none; } }
  .th-rfz:hover { color: var(--text); }
  /* 启动页：远程连接的目标机器（占站点选择器的位子——那台机器就是本次对话的对象） */
  .ssh-target { display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px; border: 1px solid var(--border); border-radius: 999px; background: var(--rail); color: var(--dim); font-size: 12px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  .ssh-target :global(svg) { color: var(--faint); flex: none; }
  .ssh-target :global(svg.distro) { transform: translateY(1px); }
  .workspace-picker { min-width: 0; display: inline-flex; align-items: center; gap: 2px; }
  .workspace-folder { min-width: 0; max-width: 210px; height: 27px; display: inline-flex; align-items: center; gap: 6px; padding: 0 9px; border: 1px solid var(--border); border-radius: 999px; background: var(--rail); color: var(--dim); font: inherit; font-size: 11.5px; cursor: pointer; }
  .workspace-folder:hover { background: #f4f2ed; color: var(--text); }
  .workspace-folder.selected { color: var(--text); background: #f7f6f2; }
  .workspace-folder svg { flex: none; color: var(--faint); }
  .workspace-folder span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .workspace-clear { width: 23px; height: 23px; padding: 0; border: 0; border-radius: 50%; background: transparent; color: var(--faint); font-size: 15px; line-height: 1; cursor: pointer; }
  .workspace-clear:hover { background: #eeeae3; color: var(--text); }
  .ssh-key-row { display: flex; gap: 6px; align-items: center; }
  .ssh-key-row .tin { flex: 1; min-width: 0; }
  .ssh-fp { display: flex; flex-direction: column; gap: 5px; padding: 9px 11px; border: 1px solid #cfc9ec; background: #f7f6ff; border-radius: 10px; }
  .ssh-fp b { font-size: 12.5px; }
  .ssh-fp > code { font-family: ui-monospace, monospace; font-size: 11.5px; color: var(--text); background: #fff; border: 1px solid var(--border); border-radius: 6px; padding: 4px 7px; overflow-wrap: anywhere; user-select: text; }
  .ssh-fp small { color: var(--dim); font-size: 11px; line-height: 1.5; }
  .ssh-fp small code { font-family: ui-monospace, monospace; font-size: 10.5px; background: #fff; border-radius: 4px; padding: 1px 4px; user-select: text; }
  :global(:root) {
    --bg: #ffffff; --rail: #faf9f7; --card: #ffffff;
    /* 浮层实底（更新吐司/附件 chip 等用）：此前未定义导致 var(--panel) 落空、浮层透明。 */
    --panel: #ffffff;
    --border: #ecebe6; --border2: #e1dfd8;
    --text: #26241f; --dim: #6f6b62; --faint: #a29d93;
    /* 强调色走暖黑/灰（Codex 式安静路线），不用蓝紫；风险色（红/琥珀）另有专用变量。 */
    --accent: #33302a; --accent-h: #1d1b17; --accent-soft: #edece7;
    --user-bg: #f3f1ec;
    --ok: #12805c; --warn: #b45309; --err: #dc2626;
    --err-soft: #fef2f2; --err-border: #f4cccc;
    --shadow-sm: 0 1px 2px rgba(30,25,15,.05);
    --shadow: 0 4px 16px rgba(30,25,15,.08);
    --shadow-lg: 0 16px 48px rgba(30,25,15,.16);
    /* 单行表单控件统一高度 token：.tin(单行)/.dd-trigger/.tfield-fx 壳全部吃它，别再靠 padding 推算。 */
    --ctl-h: 30px;
    /* composer 底栏 chip 统一高度 token：站点/权限（Dropdown compact）、模型（ModelFx 触发器）、
       用量圆环按钮全部定高——chip 内容各异（17px favicon/「低」徽章/纯文字），靠 padding 撑高必然参差。 */
    --chip-h: 24px;
  }
  :global(html, body) { margin: 0; height: 100%; background: var(--bg); color: var(--text);
    font: 15px/1.65 -apple-system, 'PingFang SC', 'Segoe UI', 'Helvetica Neue', sans-serif; -webkit-font-smoothing: antialiased; }
  :global(*) { box-sizing: border-box; }
  /* 去掉原生按钮外观（避免 WKWebView 给 <button> 画默认边框/底纹，即那个「黑框」）。 */
  :global(button) { -webkit-appearance: none; appearance: none; font: inherit; }
  /* 去掉点击时 WKWebView 的黑色焦点框；键盘可达性用 hover/active 态足够（桌面鼠标为主）。 */
  :global(button:focus), :global(button:focus-visible), :global([role='button']:focus),
  :global([role='button']:focus-visible), :global(summary:focus), :global(a:focus) { outline: none; box-shadow: none; }
  .app { display: flex; height: 100vh; overflow: hidden; }
  /* Windows：主内容左上角轻微圆角，跟原生标题栏之间更柔和（角落露出 rail 色）。macOS 无原生标题栏不需要。
     侧栏分隔线在 Windows 去掉——通高的 border-right 会在圆角上方留一截两侧同色的「悬空线头」；
     边界靠 rail/main 底色对比 + 圆角呈现（Codex 客户端同款做法）。 */
  .app.win { background: var(--rail); }
  /* 圆角回归：当初去掉它是因为页头有暖底、和 DWM 窗框几乎同色，圆角单独露成一道台阶；
     暖底已拿掉（0.2.22），.main 的白又直接顶到窗沿——12px 圆角恢复「白面板嵌在 rail 色
     窗框里」的收边（用户实测：背景拿掉后这里正常了）。
     顶栏左缘那条竖线也随之取消：白/rail 色差已经把边界撑起来了，再画线反而多余。 */
  .app.win .main { background: var(--bg); border-top-left-radius: 12px; }
  .app.win .rail { border-right: none; }

  /* 细滚动条（macOS overlay 风格）：细、圆角、透明轨道，thumb 用 padding-box 内缩显得更细。 */
  :global(::-webkit-scrollbar) { width: 10px; height: 10px; }
  :global(::-webkit-scrollbar-track) { background: transparent; }
  :global(::-webkit-scrollbar-thumb) { background-color: rgba(60, 54, 44, .22); border-radius: 8px; border: 3px solid transparent; background-clip: padding-box; }
  :global(::-webkit-scrollbar-thumb:hover) { background-color: rgba(60, 54, 44, .4); border: 2px solid transparent; background-clip: padding-box; }
  :global(::-webkit-scrollbar-corner) { background: transparent; }

  /* 融合标题栏：全宽透明拖拽条，红绿灯浮在其上，两列各自的底色透出来 */
  /* 拖拽条只盖住左栏顶部（红绿灯+工具区）；主区域顶部改由各页头/启动页自身承担拖拽，
     这样内容能贴近顶部又不被拖拽条挡住按钮。 */
  .titlebar { position: fixed; top: 0; left: 0; height: 30px; z-index: 6; }
  /* 顶部工具按钮：浮在拖拽条之上、紧挨红绿灯右侧。 */
  .win-tools { position: fixed; top: 0; left: 80px; height: 30px; display: flex; align-items: center; gap: 1px; z-index: 8; }
  /* 全屏无红绿灯：左移到与左栏菜单图标同一左边界（rail-head 8px + 按钮 padding 让开）。 */
  .win-tools.fs { left: 12px; }
  /* Windows 无红绿灯：靠左边缘，不飘在中间。 */
  .win-tools.win { left: 12px; }
  .wt { display: inline-flex; align-items: center; justify-content: center; width: 27px; height: 24px; border: none; background: none; border-radius: 6px; color: var(--dim); cursor: pointer; -webkit-app-region: no-drag; }
  .wt:hover { background: rgba(0, 0, 0, .06); color: var(--text); }
  .wt:disabled { opacity: .4; cursor: default; }
  .wt:disabled:hover { background: none; color: var(--dim); }
  /* 待更新图标：锚在侧栏右上角（跟随 rail 右边界），静默检查发现新版时出现。 */
  /* 待更新指示：并入 win-tools，样式同折叠/搜索（无底、灰色 + 悬停高亮），仅右上角一个柔和强调色小点。 */
  .wt.upd { position: relative; }
  .wt.upd.active { background: rgba(0, 0, 0, .07); color: var(--text); }
  .wt.upd:disabled { opacity: .55; }
  .upd-count { position: absolute; top: -1px; right: -3px; min-width: 12px; height: 12px; padding: 0 3px; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; border: 1.5px solid var(--rail); border-radius: 999px; background: var(--accent); color: #fff; font-size: 7.5px; font-weight: 750; line-height: 1; }
  .upd-spin { animation: upd-spin .7s linear infinite; transform-origin: center; }
  @keyframes upd-spin { to { transform: rotate(360deg); } }
  /* 更新进度小药丸：点更新后顶部居中显示 状态文字 + 进度条，直到重启。 */
  .upd-toast { position: fixed; top: 38px; left: 50%; transform: translateX(-50%); z-index: 200; display: flex; align-items: center; gap: 8px; padding: 5px 12px; background: var(--panel); border: 1px solid var(--border); border-radius: 999px; box-shadow: var(--shadow-lg); font-size: 12px; color: var(--text); -webkit-app-region: no-drag; }
  .upd-toast .upd-spin { color: var(--accent); flex: none; }
  .upd-toast-msg { white-space: nowrap; }
  /* 下载进度条：实底浅灰轨道 + 主题色填充（容器 --panel 已实底，文字对比度由 --text/白底保证） */
  .upd-toast-bar { width: 84px; height: 5px; border-radius: 3px; background: var(--border2); overflow: hidden; flex: none; }
  .upd-toast-bar > span { display: block; height: 100%; background: var(--accent); border-radius: 3px; transition: width .2s ease; }

  /* ---- 左栏 ---- */
  .rail { position: relative; width: 240px; flex: none; display: flex; flex-direction: column; background: var(--rail); border-right: 1px solid var(--border); padding-top: 30px; }
  .rail.collapsed { display: none; }
  /* 右缘拖拽把手：改侧栏宽度。 */
  /* 只用 col-resize 光标提示，不画任何指示线（避免拖动时多出一条竖线）。 */
  .rail-resize { position: absolute; top: 30px; right: -3px; bottom: 0; width: 7px; cursor: col-resize; z-index: 5; }
  .rail-head { padding: 8px 8px 2px; display: flex; flex-direction: column; gap: 2px; }
  .newchat { display: flex; align-items: center; gap: 8px; width: 100%; padding: 7px 10px;
    background: none; color: var(--text); border: none; border-radius: 9px; font-size: 13.5px; font-weight: 550; cursor: pointer; text-align: left; }
  .newchat:hover { background: #f1efe9; }
  .newchat:disabled { opacity: .5; cursor: default; }
  .newchat svg { flex: none; color: var(--accent); }
  .railnav { display: flex; align-items: center; gap: 8px; width: 100%; padding: 5px 10px; background: none;
    border: none; border-radius: 9px; font-size: 13px; color: var(--dim); cursor: pointer; text-align: left; margin-top: -4px; }
  .railnav:hover { background: #f1efe9; color: var(--text); }
  .railnav.on { background: #e9e7e0; color: var(--text); font-weight: 550; }
  .railnav:disabled { opacity: .45; cursor: default; }
  .railnav svg { flex: none; }
  .convos { flex: 1; overflow-y: auto; padding: 2px 8px 8px; display: flex; flex-direction: column; gap: 1px; border-top: 1px solid transparent; transition: border-color .15s; }
  .convos > button.site-grp:first-of-type { padding-top: 3px; margin-top: 0; }
  .convos.scrolled { border-top-color: rgba(30, 25, 15, 0.07); }
  .rail-empty { color: var(--faint); font-size: 12px; padding: 10px 8px; line-height: 1.7; }
  /* 站点分组头（可折叠） */
  .site-grp { width: 100%; display: flex; align-items: center; gap: 6px; padding: 9px 8px 4px 10px; margin-top: 2px; background: none; border: none; cursor: pointer; text-align: left; -webkit-appearance: none; appearance: none; }
  .site-grp:first-child { margin-top: 0; }
  .site-grp:hover .site-grp-name { color: var(--accent); }
  .site-grp-chev { color: var(--faint); flex: none; transition: transform .12s; }
  .site-grp-chev.collapsed { transform: rotate(-90deg); }
  .site-grp-name { flex: 0 1 auto; max-width: 58%; min-width: 0; font-size: 12.5px; font-weight: 600; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .site-grp-host { flex: 0 1 auto; min-width: 0; font-size: 10.5px; color: var(--accent); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* 线程头右上角：站点域名 + ↗，点开站点 */
  .th-open { flex: none; display: inline-flex; align-items: center; gap: 4px; background: none; border: none; padding: 4px 6px; border-radius: 7px; color: var(--accent); font: inherit; font-size: 12.5px; cursor: pointer; }
  .th-open:hover { background: #f1efe9; }
  .th-open:disabled { opacity: .5; cursor: wait; }
  .th-open svg { flex: none; }
  /* 任务类型子组标签 */
  /* padding-left 19：让对话行的厂商图标中心对齐分组头里的站点图标中心
     （分组头：10 左内边距 + 10 chevron + 6 gap，14px 图标中心落在 33px；
      对话行：8 外边距 + 19 左内边距，12px 图标中心也落在 33px）。顺带标题也更贴近站点名。 */
  .convo { position: relative; display: flex; align-items: center; gap: 6px; border-radius: 8px; margin: 0 8px; padding: 1px 6px 1px 19px; cursor: pointer; }
  .convo:hover { background: #f1efe9; }
  .convo.on { background: #e9e7e0; }
  .convo-body { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; }
  .convo-bi { flex: none; display: inline-flex; align-items: center; }
  /* 对话标题：比分组头（.site-grp-name 12.5px/var(--text)）小半号、淡一档——让站点头当主、对话当项 */
  .convo-title { flex: 1; min-width: 0; font-size: 12px; font-weight: 500; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .convo-when { flex: none; font-size: 11px; color: var(--dim); }
  .mini-run { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: pulse 1.1s infinite; flex: none; }
  .mini-permit { font-size: 10px; font-weight: 600; color: #b45309; background: #faf1d8; padding: 0 5px; border-radius: 4px; flex: none; }
  .convo-x { display: none; background: none; border: none; color: var(--faint); font-size: 16px; line-height: 1; padding: 1px 2px 1px 4px; border-radius: 5px; cursor: pointer; flex: none; }
  .convo:hover .convo-x, .convo:focus-within .convo-x { display: block; }
  .convo:hover .convo-when, .convo:focus-within .convo-when { display: none; }
  .convo-x:hover { color: var(--dim); }

  .rail-foot { width: 100%; display: flex; align-items: center; gap: 8px; padding: 8px 12px; border: none; border-top: 1px solid var(--border); background: none; cursor: pointer; text-align: left; -webkit-appearance: none; appearance: none; box-shadow: none; }
  .rail-foot:focus, .rail-foot:active { outline: none; box-shadow: none; }
  .rail-foot:hover { background: #f1efe9; }
  .rail-foot.open { background: #f1efe9; border-top-color: transparent; }
  .foot-main { flex: 1; min-width: 0; }
  .foot-main b { display: block; font-size: 12.5px; line-height: 1.15; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .foot-main small { display: block; color: var(--dim); font-size: 10.5px; line-height: 1.1; }
  .foot-chev { color: var(--faint); flex: none; transition: transform .15s; }
  .foot-chev.up { transform: rotate(180deg); }

  .foot-wrap { position: relative; }
  .conn-switch {
    position: absolute; left: 8px; right: 8px; bottom: calc(100% + 4px); z-index: 40;
    background: #fff; border: 1px solid var(--border2); border-radius: 12px;
    box-shadow: 0 10px 30px rgba(30,25,15,.15); padding: 5px;
    display: flex; flex-direction: column; max-height: min(640px, calc(100vh - 86px)); overflow: hidden;
    animation: pop .1s ease-out;
  }
  .cs-scroll {
    min-height: 0; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable;
  }
  .cs-actions { flex: none; }
  .cs-item {
    width: 100%; display: flex; align-items: center; gap: 9px;
    background: none; border: none; border-radius: 8px; padding: 5.5px 9px; cursor: pointer; text-align: left; font: inherit;
    color: var(--text);
  }
  .cs-item:hover { background: #f4f3ef; }
  .cs-item.on { background: #efeee9; }
  .cs-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .cs-main b { font-weight: 500; font-size: 13px; line-height: 1.2; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* 连接副标题（Key 前缀 · Cloudflare / Ubuntu … LTS 之类）：更小更淡，让连接名更突出 */
  .cs-main small {
    display: block;
    width: 100%;
    min-width: 0;
    color: var(--faint);
    font-size: 10px;
    line-height: 1.25;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cs-main small.cs-os {
    display: flex;
    align-items: center;
  }
  .cs-main small.cs-os :global(svg.distro) {
    transform: translateY(1px);
  }
  .cs-os-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cs-check { color: var(--accent); flex: none; font-size: 13px; }
  .cs-div { height: 1px; background: var(--border); margin: 3.5px 4px; }
  /* 动作行是单行：比原来(7px)收，但别收到发挤——padding 5.5px + line-height 1.3 落在约 28px，
     比原来 37px 紧一截、又留了呼吸感（23px 太挤，实测反馈）。连接行是两行，另按 .cs-item 定高。 */
  .cs-act {
    width: 100%; display: flex; align-items: center; gap: 8px;
    background: none; border: none; border-radius: 8px; padding: 5.5px 9px; cursor: pointer; text-align: left;
    font: inherit; font-size: 12.5px; line-height: 1.3; color: var(--dim);
  }
  .cs-act:hover { background: #f4f3ef; color: var(--text); }
  .cs-act.on { background: #efeee9; color: var(--text); font-weight: 550; }
  .cs-act :global(svg) { color: var(--faint); flex: none; }
  .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; }
  .dot.ok { background: #16a34a; } .dot.warn { background: #d97706; } .dot.off { background: #cfccc4; }

  /* ---- 主区 ---- */
  .main { flex: 1; position: relative; display: flex; flex-direction: column; min-width: 0; padding-top: 0; }
  /* 吐司：pre-wrap 让「——详情——」多行错误可读；err 形态限高可滚动（安装失败输出末尾等） */
  .flash { position: fixed; top: 40px; left: 50%; transform: translateX(-50%); z-index: 1200; background: #14231a; color: #fff; padding: 9px 16px; border-radius: 10px; font-size: 13px; box-shadow: var(--shadow); max-width: 70%; white-space: pre-wrap; overflow-wrap: anywhere; -webkit-app-region: no-drag; }
  .flash.err { max-height: 42vh; overflow-x: hidden; overflow-y: auto; }
  /* 自定义右键菜单（替换 WKWebView 默认英文菜单） */
  .ctx-menu { position: fixed; z-index: 120; min-width: 148px; max-width: calc(100vw - 16px); max-height: calc(100vh - 16px); overflow-x: hidden; overflow-y: auto; background: #fff; border: 1px solid var(--border); border-radius: 11px; box-shadow: 0 12px 32px rgba(30,25,15,.16); padding: 5px; animation: pop .1s ease-out; }
  .conn-ctx-menu { visibility: hidden; }
  .conn-ctx-menu.ready { visibility: visible; }
  .ctx-item { width: 100%; display: flex; align-items: center; justify-content: space-between; gap: 18px; background: none; border: none; border-radius: 7px; padding: 6px 10px; font: inherit; font-size: 13px; color: var(--text); cursor: pointer; text-align: left; }
  .ctx-item:hover:not(:disabled) { background: #f1efe9; }
  .ctx-item:disabled { color: var(--faint); cursor: default; }
  .ctx-div { height: 1px; background: var(--border); margin: 4px 6px; }
  .imgtip { position: fixed; z-index: 130; transform: translate(-50%, -100%); padding: 5px; border-radius: 10px; background: #fff; border: 1px solid rgba(0, 0, 0, .1); box-shadow: 0 8px 24px rgba(0, 0, 0, .18); pointer-events: none; visibility: hidden; }
  .imgtip.ready { visibility: visible; animation: tipin .12s ease-out both; }
  .imgtip.below { transform: translate(-50%, 0); }
  /* min-* + contain：极端长宽比（整页截图/超宽横幅）装进保底盒子里，不再缩成一条细缝 */
  .imgtip img { display: block; max-width: 320px; max-height: 240px; min-width: 140px; min-height: 90px; object-fit: contain; border-radius: 6px; background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 14px 14px; }
  .lightbox { position: fixed; inset: 0; z-index: 140; background: rgba(24, 22, 18, .72); display: flex; align-items: center; justify-content: center; cursor: zoom-out; }
  .lightbox img { max-width: 86vw; max-height: 86vh; border-radius: 12px; background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 16px 16px; box-shadow: 0 24px 64px rgba(0, 0, 0, .4); }
  @keyframes tipin { from { opacity: 0; } to { opacity: 1; } }
  .tipwrap { display: inline-flex; }
  .flash.err { background: var(--err); }

  /* safe center：内容比可视区高时退回顶对齐可滚动，避免居中把顶部裁掉。 */
  .center { flex: 1; display: flex; align-items: safe center; justify-content: center; overflow-y: auto; padding: 24px; }
  /* 启动页：大屏不无限拉宽，入口区与下方的新站引导共用同一条 920px 内容基线。 */
  .center.launch-center { align-items: safe center; justify-content: flex-start; padding: 24px 40px; }
  .center.launch-center .launcher { width: min(920px, 100%); margin-inline: auto; }
  .hero-card { text-align: center; max-width: 460px; }
  .hero-mark { display: flex; justify-content: center; margin-bottom: 6px; }
  .hero-card h1 { font-size: 22px; margin: 12px 0 8px; }
  .hero-card p { color: var(--dim); margin: 0 0 18px; }
  .cli-guide { margin-top: 22px; text-align: left; border-top: 1px solid var(--border); padding-top: 14px; }
  .cli-guide-h { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 11px; letter-spacing: .03em; text-transform: uppercase; color: var(--faint); font-weight: 600; margin-bottom: 6px; }
  .cli-redetect { display: inline-flex; align-items: center; gap: 4px; flex: none; background: none; border: none; padding: 2px 6px; border-radius: 6px; cursor: pointer; color: var(--dim); font: inherit; font-size: 11px; font-weight: 500; letter-spacing: 0; text-transform: none; }
  .cli-redetect:hover { background: rgba(0, 0, 0, .06); color: var(--text); }
  .cli-redetect:disabled { opacity: .55; cursor: default; }
  .cli-redetect :global(svg) { width: 12px; height: 12px; }
  .cli-row { display: flex; align-items: center; gap: 8px; padding: 3px 0; font-size: 13px; }
  .cli-name { width: 100px; flex: none; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .cli-copy { flex: none; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 5px; color: var(--faint); cursor: pointer; }
  .cli-copy:hover { background: rgba(0, 0, 0, .06); color: var(--text); }
  .cli-tag { flex: none; font-size: 11px; padding: 1px 7px; border-radius: 6px; font-weight: 600; }
  .cli-tag.ok { color: #1a7f4b; background: #e7f4ec; }
  .cli-tag.warn { color: #9a6a00; background: #f7efd9; }
  .cli-tag.bad { color: #9a3b2f; background: #f6e7e3; }
  .cli-cmd { flex: 0 1 auto; min-width: 0; font-family: ui-monospace, monospace; font-size: 10.5px; color: var(--faint); background: #f6f5f1; padding: 3px 8px; border-radius: 5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; user-select: text; cursor: text; }
  /* 提高优先级压过 .hero-card p（后者 margin/color 更 specific，会把 top margin 顶成 0）。 */
  .cli-guide .cli-note { display: flex; align-items: center; gap: 5px; font-size: 11.5px; line-height: 1.5; color: var(--faint); margin: 12px 0 0; }
  .cli-note-ic { flex: none; color: var(--faint); }

  .launcher { width: min(680px, 100%); }
  .launcher h1 { font-size: 26px; margin: 0 0 6px; letter-spacing: -.01em; }
  .launcher .sub { color: var(--dim); margin: 0 0 22px; }








  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  .tin, textarea { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1.5px solid var(--border2); border-radius: 8px; padding: 5px 10px; }
  /* 单行输入（text/password/number/datetime-local）：显式统一高 --ctl-h，内容垂直居中；
     去掉竖向 padding 防 macOS 原生步进器/日期控件把高度顶开。textarea 不定高，不吃这条。 */
  input.tin { height: var(--ctl-h); box-sizing: border-box; padding-top: 0; padding-bottom: 0; line-height: normal; }
  .tin:focus, textarea:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }

  /* 会话搜索（命令面板式；.mask 提供遮罩，box 居中于顶部） */
  .search-box { position: fixed; z-index: 61; top: 12vh; left: 50%; transform: translateX(-50%); width: min(560px, 92vw); background: #fff; border: 1px solid var(--border); border-radius: 14px; box-shadow: 0 24px 60px rgba(20, 15, 8, .28); overflow: hidden; display: flex; flex-direction: column; max-height: 66vh; }
  .search-head { display: flex; align-items: center; gap: 10px; padding: 12px 15px; border-bottom: 1px solid var(--border); }
  .search-head .si-ic { color: var(--faint); flex: none; }
  .search-head input { flex: 1; min-width: 0; border: none; outline: none; background: none; font: inherit; font-size: 15px; color: var(--text); }
  .search-head kbd { font: inherit; font-size: 10.5px; color: var(--faint); border: 1px solid var(--border2); border-radius: 5px; padding: 1px 6px; background: var(--rail); flex: none; }
  .search-list { min-width: 0; overflow-x: hidden; overflow-y: auto; padding: 6px; }
  .search-sec { padding: 8px 10px 3px; font-size: 11px; font-weight: 600; letter-spacing: .04em; color: var(--faint); }
  .search-empty { text-align: center; color: var(--faint); font-size: 13px; padding: 26px 12px; margin: 0; }
  .search-item { width: 100%; display: flex; align-items: center; gap: 10px; background: none; border: none; border-radius: 9px; padding: 9px 10px; cursor: pointer; text-align: left; color: var(--text); }
  .search-item:hover, .search-item.on { background: #f4f3ef; }
  .si-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .si-main b { font-weight: 500; font-size: 13.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .si-main small { color: var(--dim); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .si-tag { flex: none; font-size: 10px; color: var(--faint); border: 1px solid var(--border2); border-radius: 4px; padding: 0 4px; line-height: 15px; }

  /* 输入框（仿 Claude Code：整块圆角边框，聚焦时高亮，发送按钮嵌在框内） */
  .composer.big, .composer-wrap .composer { position: relative; background: #fff; border: 1px solid var(--border2); border-radius: 12px; box-shadow: none; transition: border-color .12s, box-shadow .12s; }
  .composer.big:focus-within, .composer-wrap .composer:focus-within { border-color: #b7b2a6; box-shadow: none; }
  .composer-wrap .composer.fileover { border-color: var(--accent); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 16%, transparent); }
  .composer-wrap .composer.fileover::after {
    content: '松开即可添加文件'; position: absolute; inset: 4px; z-index: 8; display: flex; align-items: center; justify-content: center;
    border: 1px dashed color-mix(in srgb, var(--accent) 58%, transparent); border-radius: 9px;
    background: color-mix(in srgb, #fff 92%, var(--accent)); color: var(--accent); font-size: 13px; font-weight: 600; pointer-events: none;
  }
  .composer.big textarea, .composer-wrap textarea { width: 100%; resize: none; border: none; background: none; box-shadow: none; padding: 14px 52px 14px 17px; font-size: 14px; line-height: 1.6; max-height: 200px; display: block; }
  .composer.big textarea:focus, .composer-wrap textarea:focus { outline: none; box-shadow: none; border: none; }
  .composer .send { position: absolute; right: 9px; bottom: 9px; width: 32px; height: 32px; border-radius: 50%; border: none; background: var(--accent); color: #fff; font-size: 16px; cursor: pointer; display: flex; align-items: center; justify-content: center; transition: background .12s, transform .08s; }
  .composer .send:hover { background: var(--accent-h); }
  .composer .send:active { transform: scale(.92); }
  .composer .send:disabled { background: #dcdad2; cursor: default; transform: none; }
  .composer .send.stop { background: var(--text); }
  /* composer：竖排 textarea + 底栏（启动页站点在左；会话页只读厂商在左，模型/发送在右）。 */
  .composer.big, .composer-wrap .composer { display: flex; flex-direction: column; }
  .composer.big textarea, .composer-wrap textarea { padding: 12px 16px 6px 16px; }
  .composer-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 8px 8px; }
  .cb-left { display: flex; align-items: center; gap: 1px; flex: 1; min-width: 0; }
  .cb-right { display: flex; align-items: center; gap: 3px; flex: none; }
  .composer-bar .send { position: static; width: 30px; height: 30px; margin-left: 3px; font-size: 15px; }
  /* 会话页只读徽标（站点/厂商，不可改）。 */
  .cb-ro { display: inline-flex; align-items: center; gap: 6px; padding: 4px 8px; color: var(--dim); font-size: 13px; opacity: .9; min-width: 0; }
  .cb-ro-t { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  /* ---- 线程 ---- */
  /* ★ 标题要和左上角那排工具图标在同一条水平线上。
     `.win-tools` 是 fixed / top:0 / height:30px + align-items:center → **图标中心恒在 15px**
     （和 .thread-head.slim 那条注释是同一个基准）。标题行高 20.3 → 上内边距取 5px，
     标题中心落在 15.15，对上。原来 13px 时中心在 23.1 —— **每个页面都低 8px**。
     ★ 上边距被「对齐那条 fixed 图标带」锁死＝5px，所以**下边距只能跟着收成 5px**，
     不然上下就不对称（原来 5/13，下面明显空一截）。头因此比以前矮 8px，这是这条对齐的代价。
     ★ align-items 必须 flex-start，不能用 center：th-info 比右侧按钮（28px）矮时
     （例如只有标题、没有副标题的头），center 会让 th-info 跟着按钮居中，标题又掉到 26.8。 */
  /* ★ 和内容区之间那条缝：原来是一条 1px 实线直接把两块切开，太硬。**两个状态分开治**：
     ① 静止态（治「硬」）：页头与内容**同底不加色**——曾试过一层极淡暖底做色阶差，
        但那条色带在页面顶上像贴了块补丁（用户点名拿掉）；现在静止态只留几乎看不见的
        底线。**刻意不上 backdrop-filter**：实测它只会把卡片的红字糊成粉影漂在标题背后
        （是噪点不是层次），还要赔上脱流 + ResizeObserver + 三处 scrollIntoView 的静默回归。
     ② 滚动态（治「切」）：见 .elevated —— 内容真滑到底下时才淡入柔和阴影、把线淡掉。
        滚到顶时页头底下是**空的**，那时打阴影＝给不存在的遮挡关系编故事
        （同 .term-wrap.with-chat 那条「独占/无对话时不加，保持齐边」的分寸）。
     z-index：阴影得压在 .thread 之上 —— .thread 是后面的兄弟，默认会盖住它。 */
  .thread-head { flex: none; padding: 5px 24px; border-bottom: 1px solid rgba(30, 25, 15, .05); position: relative; z-index: 1; transition: border-bottom-color .18s ease; display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  /* ★ 阴影用伪元素而不是 box-shadow：box-shadow 天然四周都有 —— `0 6px 12px -6px` 水平方向是
     「收缩 6 再模糊 6」，**正好到边**，于是左右两端各留一段 6px 渐弱，看着就是两个圆头戳在窗沿上。
     负 spread 再加也没用：加到不露头，朝下的阴影也就没了。
     伪元素 left:0/right:0 精确铺满、只朝下，左右**零溢出、零圆头**。 */
  .thread-head::after {
    content: ''; position: absolute; left: 0; right: 0; top: 100%; height: 6px;
    background: linear-gradient(rgba(20, 16, 13, .03), rgba(20, 16, 13, 0)); /* 原 .13 → .05 → .03：用户连续点名太重，压到将将可感；再嫌重就整个去掉 */
    opacity: 0; transition: opacity .18s ease; pointer-events: none;
  }
  /* 只在内容真滑到页头底下时才抬升。border 只改 color 不改宽度 —— 改宽度会让下面整块跳 1px。 */
  .thread-head.elevated { border-bottom-color: transparent; }
  .thread-head.elevated::after { opacity: 1; }
  /* 侧栏收起：红绿灯 + 悬浮的折叠/搜索钮压在内容区左上，页头统一左让位（全部视图受益）。
     发现更新时顶部临时增加更新入口，页头再让出一个按钮宽度，避免压住页面标题图标。 */
  .app.rail-collapsed .thread-head { padding-left: 140px; }
  .app.rail-collapsed.fs .thread-head, .app.rail-collapsed.win .thread-head { padding-left: 100px; }
  .app.rail-collapsed.top-update-visible .thread-head { padding-left: 167px; }
  .app.rail-collapsed.top-update-visible.fs .thread-head,
  .app.rail-collapsed.top-update-visible.win .thread-head { padding-left: 127px; }
  /* 会话头同时承担应用标题栏：展开、折叠与分屏都固定成与 win-tools 相同的 30px，
     避免域名按钮的上下 padding 把标题整行撑低。 */
  .thread-head.conversation-thread-head {
    height: 30px; min-height: 30px; padding-top: 0; padding-bottom: 0; align-items: center;
  }
  .th-info { min-width: 0; flex: 1 1 auto; overflow: hidden; }
  .th-info b { display: block; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 15px; line-height: 1.35; }
  .th-info b.th-title { display: inline-flex; align-items: center; gap: 7px; }
  .th-title :global(svg) { flex: none; color: var(--dim); }
  .th-info small { display: block; min-width: 0; overflow: hidden; color: var(--dim); font-size: 12px; margin-top: 2px; text-overflow: ellipsis; white-space: nowrap; }
  .th-info small :global(svg), .th-info small :global(img) { vertical-align: -2px; }
  .btag { display: inline-flex; align-items: center; gap: 4px; }
  .thread { flex: 1; overflow-y: auto; }
  .thread-inner { max-width: 760px; margin: 0 auto; padding: 22px 24px 8px; display: flex; flex-direction: column; gap: 20px; }

  .msg { display: flex; gap: 12px; }
  .msg.user { justify-content: flex-end; }
  .ubody { background: var(--user-bg); border-radius: 16px 16px 5px 16px; padding: 10px 14px; max-width: 78%; word-break: break-word; }
  .ub-text { white-space: pre-wrap; }
  .msg.assistant .body { flex: 1; min-width: 0; }
  .text { white-space: pre-wrap; word-break: break-word; }
  /* 助手消息的 Markdown 阅读版式（渲染出的 HTML 不在 Svelte 作用域内，须 :global） */
  .text.md { white-space: normal; font-size: 14px; line-height: 1.7; }
  .text.md :global(p) { margin: 0 0 10px; }
  .text.md :global(p:last-child), .text.md :global(ul:last-child), .text.md :global(ol:last-child),
  .text.md :global(pre:last-child), .text.md :global(table:last-child) { margin-bottom: 0; }
  .text.md :global(strong) { font-weight: 650; }
  .text.md :global(ul), .text.md :global(ol) { margin: 0 0 10px; padding-left: 22px; }
  .text.md :global(li) { margin: 3px 0; }
  .text.md :global(li::marker) { color: var(--faint); }
  .text.md :global(h1), .text.md :global(h2) { font-size: 16px; font-weight: 650; margin: 16px 0 6px; }
  .text.md :global(h3), .text.md :global(h4) { font-size: 14.5px; font-weight: 650; margin: 14px 0 5px; }
  .text.md :global(h1:first-child), .text.md :global(h2:first-child),
  .text.md :global(h3:first-child), .text.md :global(h4:first-child) { margin-top: 0; }
  .text.md :global(code) { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: .88em; background: #f1efe9; padding: 1px 5px; border-radius: 5px; }
  .text.md :global(pre) { background: #f6f5f1; border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; overflow-x: auto; margin: 0 0 10px; }
  .text.md :global(pre code) { background: none; padding: 0; font-size: 12.5px; line-height: 1.6; }
  .text.md :global(a) { color: var(--accent); text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
  .text.md :global(table) { border-collapse: collapse; margin: 0 0 10px; font-size: 13px; display: block; overflow-x: auto; max-width: 100%; }
  .text.md :global(th), .text.md :global(td) { border: 1px solid var(--border2); padding: 5px 10px; text-align: left; }
  .text.md :global(th) { background: #f6f5f1; font-weight: 600; }
  .text.md :global(blockquote) { margin: 0 0 10px; padding: 2px 12px; border-left: 3px solid var(--border2); color: var(--dim); }
  .text.md :global(hr) { border: none; border-top: 1px solid var(--border); margin: 12px 0; }
  .text.md :global(img) { max-width: 100%; border-radius: 8px; }
  .text.is-err { color: var(--err); }
  /* 重试：无边框，内联跟在红色错误文字后面 */
  .retry-btn { display: inline-flex; align-items: center; gap: 4px; margin-left: 10px; padding: 0; background: none; border: none; color: var(--accent); font: inherit; font-size: 12.5px; cursor: pointer; vertical-align: baseline; }
  .retry-btn:hover { text-decoration: underline; }
  .retry-btn svg { flex: none; }
  /* 额度耗尽卡：明确状态 + 倒计时 + 到点自动续跑 */
  .limit-card { border: 1px solid var(--border2, #e2dfd7); border-left: 3px solid #c98d70; border-radius: 10px; padding: 10px 13px; background: #fbf9f5; max-width: 520px; }
  .limit-head { font-size: 13.5px; font-weight: 600; color: var(--text, #26241f); }
  .limit-sub { margin-top: 3px; font-size: 12.5px; color: var(--dim, #6b675f); }
  .limit-actions { margin-top: 8px; display: flex; flex-wrap: wrap; gap: 4px 8px; }
  .limit-actions .retry-btn { margin-left: 0; }
  .limit-actions .retry-btn.is-armed { color: #3e7a4e; }
  .limit-hint { margin-top: 6px; font-size: 11.5px; color: var(--faint, #9b968c); }
  /* 运行中活动：只展示协议里真实收到的工具生命周期，不把轮询次数冒充业务完成度。 */
  .codex-thinking {
    width: max-content;
    margin: 13px 0 11px;
    font-size: 15px;
    font-weight: 500;
    line-height: 1.5;
  }
  .codex-thinking > span {
    color: transparent;
    background: linear-gradient(90deg, #aaa7a0 20%, #dedbd4 48%, #aaa7a0 76%);
    background-size: 220% 100%;
    background-position: 100% 50%;
    background-clip: text;
    -webkit-background-clip: text;
    animation: codexThinkingShimmer 2.1s ease-in-out infinite;
  }
  @keyframes codexThinkingShimmer {
    0%, 100% { background-position: 100% 50%; opacity: .62; }
    50% { background-position: 0 50%; opacity: 1; }
  }
  .execution-progress-stack { width: min(620px, 100%); margin: 7px 0 5px; }
  .execution-progress {
    width: 100%; box-sizing: border-box; margin: 0; border: 1px solid var(--border); border-radius: 9px;
    background: #faf9f6;
  }
  .execution-progress-mark { width: 16px; height: 16px; flex: none; display: grid; place-items: center; border-radius: 5px; background: #eeeae3; }
  .execution-progress-mark > span { width: 5px; height: 5px; border-radius: 50%; background: var(--accent); }
  .execution-progress-copy { min-width: 0; display: grid; gap: 0; }
  .execution-progress-copy b, .execution-progress-copy small { min-width: 0; overflow-wrap: anywhere; white-space: normal; }
  .execution-progress-copy b { font-size: 11.5px; line-height: 1.42; font-weight: 600; color: var(--text); }
  .execution-progress-copy small { margin-top: 1px; font-size: 10px; line-height: 1.4; color: var(--faint); font-variant-numeric: tabular-nums; }
  .execution-progress.quiet { border-color: #ead8b7; background: #fcf8ef; }
  .execution-progress.quiet .execution-progress-mark { background: #f4ead6; }
  .execution-progress.quiet .execution-progress-mark > span { background: #b87920; }
  .execution-progress.has-failure:not(.quiet) { border-color: #ead8b7; background: #fcf8ef; }
  .execution-progress.has-failure:not(.quiet) .execution-progress-mark { background: #f4ead6; }
  .execution-progress.has-failure:not(.quiet) .execution-progress-mark > span { background: #b87920; }
  .execution-progress.recovered-failure:not(.quiet) { border-color: var(--border); background: #faf9f6; }
  .execution-progress.recovered-failure:not(.quiet) .execution-progress-mark { background: #eeeae3; }
  .execution-progress.recovered-failure:not(.quiet) .execution-progress-mark > span { background: var(--accent); }
  .execution-progress-details {
    display: block; padding: 0; overflow: hidden;
  }
  .execution-progress-details > summary {
    display: grid; grid-template-columns: 16px minmax(0, 1fr) auto; align-items: start; gap: 7px; min-width: 0; padding: 6px 8px;
    cursor: pointer; list-style: none; user-select: none;
  }
  .execution-progress-details > summary::-webkit-details-marker { display: none; }
  .execution-progress-details .execution-progress-copy { flex: 1 1 auto; }
  .execution-failure-summary {
    display: flex; align-items: center; gap: 4px; flex: none; margin: 1px 0 0 auto;
    color: #8a5d1b; white-space: nowrap;
  }
  .execution-failure-summary b { font-size: 10px; font-weight: 650; }
  .execution-failure-summary small { color: var(--faint); font-size: 10px; font-weight: 500; }
  .execution-failure-summary svg { flex: none; transition: transform .15s ease; }
  .execution-progress-details[open] .execution-failure-summary svg { transform: rotate(180deg); }
  .execution-activity-list { max-height: min(300px, 42vh); overflow: auto; padding: 5px 8px 7px; border-top: 1px solid rgba(128,122,110,.16); }
  .execution-activity-item { display: grid; grid-template-columns: 7px minmax(0,1fr) auto; gap: 7px; align-items: start; padding: 6px 0; border-bottom: 1px solid rgba(128,122,110,.12); }
  .execution-activity-item > div { min-width: 0; }
  .execution-activity-item b { display: block; font-size: 11.5px; font-weight: 650; line-height: 1.4; overflow-wrap: anywhere; }
  .execution-activity-item p { margin: 2px 0 0; color: var(--dim); font-size: 10.5px; line-height: 1.45; overflow-wrap: anywhere; }
  .execution-activity-item > small { color: var(--faint); font-size: 10px; white-space: nowrap; }
  .execution-activity-item.is-failed > small { color: #9a6420; }
  .execution-activity-state { width: 6px; height: 6px; margin-top: 5px; border-radius: 50%; background: #aaa59a; }
  .execution-activity-state.running { background: var(--accent); box-shadow: 0 0 0 3px rgba(160,60,43,.09); }
  .execution-activity-state.completed { background: #4e9861; }
  .execution-activity-state.failed { background: #b87920; }
  .execution-real-progress { padding: 5px 0 8px; }
  .execution-real-progress > div { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; font-size: 10.5px; color: var(--dim); }
  .execution-real-progress > div b { color: var(--text); font-size: 11px; }
  .execution-real-progress > span { display: block; height: 3px; margin-top: 5px; overflow: hidden; border-radius: 9px; background: #e8e4dc; }
  .execution-real-progress > span i { display: block; height: 100%; border-radius: inherit; background: var(--accent); }
  .execution-progress.is-running .execution-progress-mark > span { animation: executionPulse 1.6s ease-in-out infinite; }
  @keyframes executionPulse {
    0%, 100% { transform: scale(.82); opacity: .72; box-shadow: 0 0 0 0 rgba(160,60,43,0); }
    50% { transform: scale(1.08); opacity: 1; box-shadow: 0 0 0 4px rgba(160,60,43,.11); }
  }
  .execution-failure-impact { margin: 8px 0 0; color: #8a6d43; font-size: 10.5px; line-height: 1.55; }
  @media (max-width: 620px) {
    .execution-progress-stack { width: 100%; }
    .execution-progress-details > summary { padding: 6px 8px; }
    .execution-failure-summary small { display: none; }
    .execution-failure-summary b { max-width: 76px; overflow: hidden; text-overflow: ellipsis; }
  }
  @media (prefers-reduced-motion: reduce) {
    .codex-thinking > span {
      color: var(--faint);
      background: none;
      animation: none;
      opacity: 1;
    }
    .execution-progress.is-running .execution-progress-mark > span { animation: none; }
  }
  /* 命令列表：默认收起，点击展开 */
  .cmds { margin-bottom: 9px; }
  .cmds summary { display: inline-flex; align-items: center; gap: 6px; cursor: pointer; list-style: none; width: fit-content;
    font-size: 12px; color: var(--dim); border-radius: 8px; padding: 4px 8px; margin-left: -8px; user-select: none; }
  .cmds summary::-webkit-details-marker { display: none; }
  .cmds summary:hover { background: #f1efe9; }
  .cmds .cmd-ic { color: var(--accent); flex: none; }
  .cmds .cmd-chev { flex: none; transition: transform .15s; }
  .cmds[open] .cmd-chev { transform: rotate(180deg); }
  .cmds .tools { margin: 7px 0 0; }
  .tools { display: flex; flex-direction: column; gap: 5px; margin-bottom: 8px; }
  .tool { display: flex; gap: 8px; align-items: baseline; background: #f6f5f1; border: 1px solid var(--border); border-radius: 8px; padding: 5px 10px; font-size: 12px; }
  .tcode { color: var(--accent); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; flex: none; font-weight: 600; }
  .tdetail { color: var(--dim); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tool-count { flex: none; margin-left: auto; color: var(--faint); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .inlink { background: none; border: none; color: var(--accent); text-decoration: underline; cursor: pointer; padding: 0; font: inherit; word-break: break-all; }
  .err-note { color: var(--err); background: var(--err-soft); border: 1px solid var(--err-border); border-radius: 8px; padding: 8px 10px; font-size: 13px; }
  /* 加载：logo 的 C 一笔描出、圆点落定，循环；后面带本轮耗时。尊重"减弱动态"。 */
  .working { display: flex; align-items: center; gap: 7px; padding: 3px 0; }
  .working:not(:first-child) { margin-top: 12px; }
  .wl { flex: none; }
  .working-t { font-size: 12px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .context-compact-note { display: flex; align-items: center; gap: 6px; width: fit-content; margin: 2px 0 9px; padding: 5px 8px; border-radius: 8px; color: #267457; background: #edf7f1; font-size: 12px; }
  .context-compact-note small { color: #6e8d7d; font-size: 11px; }
  .wl-trace { animation: wldraw 1.5s ease-in-out infinite; }
  @keyframes wldraw { 0% { stroke-dashoffset: 100; opacity: 1; } 55% { stroke-dashoffset: 0; opacity: 1; } 82% { stroke-dashoffset: 0; opacity: 1; } 100% { stroke-dashoffset: 0; opacity: 0; } }
  .wl-dot { transform-box: fill-box; transform-origin: center; animation: wldot 1.5s ease-in-out infinite; }
  @keyframes wldot { 0%, 45% { opacity: 0; transform: scale(.2); } 62% { opacity: 1; transform: scale(1); } 82% { opacity: 1; } 100% { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .wl-trace, .wl-dot { animation: none; } .wl-trace { stroke-dashoffset: 0; } .wl-dot { opacity: 1; } }
  @keyframes pulse { 50% { opacity: .35; } }


  @keyframes gsc-shimmer { to { background-position-x: -220%; } }


  @keyframes theme-thumb-shimmer {
    0% { transform: translateX(-42%); }
    68%, 100% { transform: translateX(42%); }
  }


  /* 排期视图 */
  .sched-inner { max-width: 720px; margin: 0 auto; padding: 18px 24px 24px; }
 
 
  .sched-empty { text-align: center; color: var(--dim); padding: 12vh 24px; }
  .sched-empty .cal-mark { font-size: 34px; }
  .sched-empty b { display: block; margin: 12px 0 6px; color: var(--text); font-size: 16px; }
  .sched-empty p { margin: 0 auto; max-width: 380px; font-size: 13px; }


  .switch { width: 34px; height: 20px; border-radius: 12px; border: none; background: #d3d0c8; cursor: pointer; padding: 0; position: relative; transition: background .15s; }
  .switch.on { background: var(--accent); }
  .switch span { position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: #fff; transition: transform .15s; box-shadow: var(--shadow-sm); }
  .switch.on span { transform: translateX(14px); }
 

  .modal.wide { width: min(520px, 94vw); }
  .modal.ssh-modal .trow,
  .modal.ssh-modal .tfield,
  .modal.ssh-modal .ssh-key-row { max-width: 100%; }
  .trow { display: flex; gap: 12px; }
  .tfield { display: flex; flex-direction: column; gap: 5px; flex: 1; min-width: 0; }
  .tfield > span { font-size: 12px; color: var(--dim); }

  .composer-wrap { flex: none; padding: 10px 24px 20px; }
  .composer-wrap .composer { max-width: 760px; margin: 0 auto; }

  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1.5px solid var(--border2); border-radius: 8px; padding: 6px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
  .btn:hover { background: #f6f5f1; } .btn:disabled { opacity: .5; cursor: default; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; } .btn.primary:hover { background: var(--accent-h); }
  .btn.ghost { border-color: var(--border); }
  .btn.small { padding: 4px 10px; font-size: 12px; }
  .btn.sm { padding: 5px 12px; font-size: 12px; border-radius: 8px; }
  .btn.soft { display: inline-flex; align-items: center; gap: 6px; padding: 3px 12px; background: #fff; color: var(--text); border: 1px solid var(--border2); border-radius: 9px; font-weight: 500; }
  .btn.soft:hover { background: var(--rail); border-color: #cfccc2; }
  .btn.soft .plus-ic { color: var(--dim); }
  /* 无边框动作按钮（头部次要动作）：去边框去底，仅 hover 有淡底 */
  .btn.bare { border: none; background: none; }
  .btn.bare:hover { background: var(--rail); border-color: transparent; }
  .authbtn { display: inline-flex; align-items: center; gap: 3px; background: none; border: none; padding: 1px 2px; color: var(--accent); font-size: 12px; font-weight: 550; line-height: 1; cursor: pointer; }
  .authbtn:hover { color: var(--accent-h); }
  .permit-card { border: 1px solid var(--border2); border-radius: 11px; overflow: hidden; background: #fff; max-width: 480px; }
  .permit-card.danger { border-color: #f0d9a8; }
  .permit-head { display: flex; align-items: center; gap: 7px; padding: 8px 12px; font-size: 12px; font-weight: 600; color: var(--text); background: #f6f5f1; border-bottom: 0.5px solid var(--border); }
  .permit-card.danger .permit-head { color: #8a5a08; background: #fdf6e6; border-bottom-color: #f0e3c4; }
  .permit-dot { width: 7px; height: 7px; border-radius: 50%; background: #b45309; flex: none; }
  .permit-tag { margin-left: auto; font-size: 10px; font-weight: 600; color: #b45309; background: #faf1d8; padding: 1px 7px; border-radius: 5px; }
  .permit-desc { padding: 10px 12px 0; font-size: 13px; font-weight: 550; color: var(--text); line-height: 1.55; word-break: break-word; }
  .permit-meta { padding: 3px 12px 0; font-size: 11px; color: var(--dim); }
  .permit-more { display: block; margin: 7px 12px 0; padding: 0; border: 0; background: none; font-size: 11px; color: var(--dim); cursor: pointer; text-align: left; }
  .permit-more:hover { color: var(--text); }
  .permit-cmd { display: block; margin: 6px 12px 0; font-size: 12px; background: #17130f; color: #e8dcc8; padding: 8px 10px; border-radius: 7px; white-space: pre-wrap; word-break: break-all; max-height: 180px; overflow-y: auto; }
  .permit-act { display: flex; gap: 8px; padding: 10px 12px 12px; }
  .permit-act .btn { flex: 1; }
  .cf-mark { flex: none; display: inline-block; vertical-align: middle; }
  .hero-btns { display: flex; gap: 10px; flex-wrap: wrap; justify-content: center; }
  .setup-add { position: relative; display: inline-flex; }
  .setup-add-trigger { width: 27px; height: 27px; padding: 0; }
  .setup-add-trigger.open { color: var(--text); background: #f1efe9; }
  .setup-add-menu {
    position: absolute; z-index: 8; top: calc(100% + 5px); right: 0; width: 216px; padding: 5px;
    background: var(--panel); border: 1px solid var(--border); border-radius: 10px;
    box-shadow: 0 10px 28px rgba(30, 25, 15, .15); animation: pop .1s ease-out;
  }
  .setup-add-menu button {
    width: 100%; min-height: 34px; padding: 7px 9px; border: 0; border-radius: 7px; background: none;
    display: flex; align-items: center; gap: 9px; color: var(--text); cursor: pointer; text-align: left;
    font: inherit; font-size: 12.5px; font-weight: 500; letter-spacing: 0; text-transform: none;
  }
  .setup-add-menu button:hover { background: #f4f3ef; }
  .setup-add-menu button:disabled { opacity: .5; cursor: default; }
  .setup-add-menu button :global(svg) { flex: none; color: var(--dim); }
  .mask.cf-mask { z-index: 70; }
  .modal.cf-modal { z-index: 80; width: min(440px, 94vw); max-height: 88vh; }
  .cf-step { border-top: 0.5px solid var(--border); padding-top: 12px; margin-top: 6px; }
  .cf-step:first-child { border-top: none; padding-top: 0; margin-top: 0; }
  .cf-step-t { font-size: 13px; color: var(--text); margin: 0 0 7px; line-height: 1.5; }
  .cf-update-target { display: flex; align-items: center; gap: 9px; margin-bottom: 9px; padding: 9px 10px; border: 1px solid #dfe7e2; border-radius: 9px; background: #f4f8f5; }
  .cf-update-target > span { width: 28px; height: 28px; border-radius: 8px; background: #fff; display: inline-flex; align-items: center; justify-content: center; flex: none; }
  .cf-update-target > div { min-width: 0; }
  .cf-update-target b, .cf-update-target small { display: block; }
  .cf-update-target b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .cf-update-target small { margin-top: 2px; color: var(--dim); font-size: 10.5px; overflow-wrap: anywhere; }
  .cf-update-verified { display: flex; align-items: center; gap: 8px; margin-top: 8px; padding: 8px 9px; border-radius: 9px; background: #fbf3e7; color: #8a5a08; }
  .cf-update-verified.ok { background: #edf7f0; color: #287248; }
  .cf-update-verified > span { width: 22px; height: 22px; border-radius: 50%; background: rgb(255 255 255 / .75); display: inline-flex; align-items: center; justify-content: center; flex: none; font-weight: 700; }
  .cf-update-verified b, .cf-update-verified small { display: block; }
  .cf-update-verified b { font-size: 11.5px; }
  .cf-update-verified small { margin-top: 1px; color: var(--dim); font-size: 10px; }
  .cf-perms { margin: 0 0 9px; padding-left: 0; list-style: none; display: flex; flex-direction: column; gap: 3px; }
  .cf-perms li { font-size: 12px; color: var(--dim); padding-left: 13px; position: relative; }
  .cf-perms li::before { content: '·'; position: absolute; left: 4px; color: #f6821f; font-weight: 700; }
  .cf-perms li b { color: var(--text); font-weight: 550; }
  .cf-row { display: flex; gap: 8px; }
  .cf-row .tin { flex: 1; }
  .wr-note { display: flex; align-items: center; gap: 7px; width: fit-content; max-width: 100%; margin-top: 12px; font-size: 12px; color: #8a5a08; }
  .wr-note .wr-ic { flex: none; color: #d9a400; }
  .wr-note span { color: #8a5a08; }
  .wr-btn { flex: none; border: none; background: #d9a400; color: #fff; font-size: 11px; font-weight: 500; padding: 3px 9px; border-radius: 6px; cursor: pointer; }
  .wr-btn:hover { background: #c08f00; }
  .wr-btn:disabled { opacity: .85; cursor: default; }
  .wr-spin { display: inline-block; width: 10px; height: 10px; border: 1.6px solid #fff; border-right-color: transparent; border-radius: 50%; animation: rspin .7s linear infinite; margin-right: 5px; vertical-align: -1px; }
  .cf-perms-toggle { display: inline-flex; align-items: center; gap: 5px; background: none; border: none; padding: 0 0 8px; font-size: 12px; color: var(--dim); cursor: pointer; }
  .cf-perms-toggle:hover { color: var(--text); }
  .cf-chev { flex: none; transition: transform .12s; }
  .cf-chev.open { transform: rotate(180deg); }
  /* 启动页底栏空间不足时先收项目名，避免它把后面的风格/权限控件挤到一起。 */
  .cf-proj-in {
    flex: 0 1 190px; width: 190px; min-width: 72px; max-width: 190px;
    border: none; background: none; font-size: 13px; color: var(--text); padding: 3px 6px; outline: none;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .cf-proj-in::placeholder { color: #b3ada2; }
  .cb-prev { position: relative; display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; font-size: 12px; padding: 3px 8px; border: none; background: none; color: var(--accent); cursor: pointer; font-weight: 500; border-radius: 6px; }
  .cb-prev:hover:not(:disabled) { background: var(--rail); }
  .cb-prev:disabled { opacity: .45; cursor: default; }
  .cb-prev.dim { color: var(--dim); font-weight: 400; }
  .cb-prev svg { flex: none; }
  /* 自定义悬停提示（不用系统 title） */
  .cf-bar { max-width: 760px; margin: 0 auto 6px; display: flex; align-items: center; gap: 2px; flex-wrap: wrap; }
  /* 等待消息（排队）条：琥珀色调，区别于普通输入 */
  .queued-row { display: flex; align-items: center; gap: 8px; margin: 10px 12px 0; padding: 6px 8px 6px 11px; background: #fbf6ea; border: 1px solid #f0e2c0; border-radius: 9px; font-size: 12.5px; }
  .queued-ic { flex: none; color: #b45309; display: inline-flex; }
  .queued-t { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--dim); }
  .queued-btn { flex: none; border: none; background: none; color: var(--accent); font-size: 12px; cursor: pointer; padding: 2px 4px; }
  .queued-btn:hover { text-decoration: underline; }
  .queued-x { flex: none; border: none; background: none; color: var(--faint); font-size: 15px; line-height: 1; cursor: pointer; padding: 0 3px; }
  .queued-x:hover { color: var(--err); }
  .send.queue { background: var(--accent); color: #fff; }
  .attach-row { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding: 10px 14px 2px; }
  .attach-chip { display: inline-flex; align-items: center; gap: 6px; max-width: 190px; padding: 3px 4px 3px 6px; background: var(--panel); border: 1px solid var(--border2); border-radius: 8px; font-size: 12px; }
  .attach-img { width: 24px; height: 24px; object-fit: cover; border-radius: 4px; flex: none; }
  .attach-ic { color: var(--dim); flex: none; display: inline-flex; }
  .attach-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .attach-x { flex: none; border: none; background: none; color: var(--dim); cursor: pointer; font-size: 14px; line-height: 1; padding: 0 2px; }
  /* 用户气泡里的附件卡（只读，白底衬在浅色气泡上） */
  .ub-atts { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .ub-atts.only { margin-top: 0; }
  .ub-att { display: inline-flex; align-items: center; gap: 6px; max-width: 220px; padding: 4px 10px 4px 8px; background: #fff; border: 1px solid var(--border2); border-radius: 8px; font-size: 12px; }
  .ub-att-img { padding: 0; border: 1px solid var(--border2); border-radius: 10px; overflow: hidden; cursor: zoom-in; line-height: 0; background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 14px 14px; display: inline-flex; align-items: center; justify-content: center; min-width: 44px; min-height: 34px; }
  /* 按图片原始比例呈现（纯像素上限、不裁切），宽 logo/竖图都能看到全貌；点击仍可放大。
     注意不能用 max-width:min(…,100%)：容器 shrink-to-fit，百分比循环引用会让整条声明失效。 */
  .ub-att-img img { display: block; max-width: 260px; max-height: 180px; width: auto; height: auto; }
  .ub-att-img.ph { display: inline-block; width: 128px; height: 96px; cursor: default; animation: phpulse 1.2s ease-in-out infinite; }
  /* 助手消息里的生图产物：与用户附件同款卡片，气泡外基底微调 */
  .gen-imgs { margin-top: 6px; }
  .ub-att.as-btn { cursor: pointer; font: inherit; font-size: 12px; color: inherit; }
  .ub-att.as-btn:hover { border-color: var(--border3, var(--border2)); }
  @keyframes phpulse { 0%, 100% { opacity: .55; } 50% { opacity: .9; } }
  .ub-att-ic { flex: none; display: inline-flex; color: var(--dim); }
  .ub-att-n { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .attach-x:hover { color: var(--err); }
  .attach-loading { font-size: 11px; color: var(--dim); }
  /* 提示词库 */
  .prompt-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(272px, 1fr)); gap: 12px; }
  .prompt-card { display: flex; flex-direction: column; background: var(--card); border: 1px solid var(--border2); border-radius: 12px; padding: 13px 14px 11px; }
  .prompt-top { display: flex; align-items: center; gap: 7px; margin-bottom: 6px; }
  .prompt-title { font-size: 13.5px; font-weight: 600; color: var(--text); flex: 1; min-width: 0; }
  .prompt-tag { flex: none; font-size: 10px; font-weight: 600; color: var(--dim); background: #e7e4dd; padding: 1px 6px; border-radius: 5px; }
  .prompt-body { margin: 0 0 11px; font-size: 12.5px; line-height: 1.5; color: var(--dim); display: -webkit-box; -webkit-line-clamp: 5; line-clamp: 5; -webkit-box-orient: vertical; overflow: hidden; }
  .prompt-acts { display: flex; align-items: center; gap: 4px; margin-top: auto; }
  .prompt-act { position: relative; width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: none; border-radius: 8px; background: none; color: var(--dim); cursor: pointer; }
  .prompt-act:hover { color: var(--text); background: var(--rail); }
  .prompt-act svg { flex: none; }
  /* 模板库单独一个更宽的容器：grid 本来就是 auto-fill（会自己长列），真正卡住它的是
     .sched-inner 的 720px —— 三列要 3*272+2*12+48 = 876px，720 差得远，所以窗口再宽也只有两列。
     日程页还得用窄栏（那是读列表的），所以不改 .sched-inner，另起一个。 */
  .tmpl-inner { max-width: 1180px; margin: 0 auto; padding: 18px 24px 24px; }
  .th-line { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  /* 提示挪到标题**同一行**了。`.th-info small` 那 2px 上间距是给「标题下面那行」准备的，
     这里得清掉；选择器带上 .tmpl-hint 才压得过它（它带元素选择器，分更高）。 */
  .th-info small.tmpl-hint { margin-top: 0; color: var(--faint); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* 负边距抵掉筹码自己的内边距 —— 那 8px/3px 是**点击热区**，不该算进视觉间距：
     - 左：不抵的话整排比上面的「模板库」缩进 8px，看着就是没对齐。
     - 下：不抵的话筹码文字离底边比标题离顶边多出 3px 的死空（上 6 / 下 10.5），上下就不一致。 */
  .tmpl-chips { display: flex; flex-wrap: wrap; gap: 2px; margin-top: 5px; margin-left: -8px; margin-bottom: -3px; }
  /* 无外框、无底：选中只靠**颜色加重**（--dim → --text）。
     ★ 故意不动 font-weight：字重一变宽度就变，点一下整排筹码会跟着挪位。 */
  .tmpl-chip { display: inline-flex; align-items: center; gap: 3px; padding: 3px 8px; border: 0; border-radius: 999px; background: transparent; color: var(--dim); font-size: 12px; cursor: pointer; }
  .tmpl-chip:hover { color: var(--text); }
  .tmpl-chip:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .tmpl-chip.on { color: var(--text); }
  .tc-ic { flex: none; margin-right: 1px; opacity: 0.7; }
  .tc-n { font-size: 10px; opacity: 0.55; font-variant-numeric: tabular-nums; }
  /* ★ 240 是**默认窗口能放下两列**倒推出来的，别再往上调：
     默认 820 宽、左栏 240 → 内容 580，减 48 内边距 = 532。两列 = 2*240+12 = 492 ✓。
     上一版写 272 要 556 > 532，就差这一点，默认窗口直接掉成**一列**。
     再宽才 3 列（≈1050）、4 列（≈1300，到 1180 封顶就不再加）。 */
  .tmpl-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; }
  /* 从全局搜索跳过来时闪一下：一屏十几张卡，跳到了也认不出是哪张 */
  @keyframes tmplFlash { from { box-shadow: 0 0 0 2px var(--accent); } to { box-shadow: 0 0 0 2px transparent; } }
  .tmpl-card.tmpl-flash { animation: tmplFlash 1.4s ease-out; }
  .tmpl-card { display: flex; flex-direction: column; background: var(--card); border: 1px solid var(--border2); border-radius: 12px; overflow: hidden; }
  .tmpl-thumb { position: relative; height: 158px; overflow: hidden; background: #fff; display: flex; align-items: center; justify-content: center; border-bottom: 1px solid var(--border); }
  .tmpl-frame { position: absolute; top: 0; left: 0; width: 1280px; height: 860px; border: 0; transform: scale(0.23); transform-origin: top left; pointer-events: none; background: #fff; }
  .tmpl-letter { font-size: 40px; font-weight: 600; color: #a03c2b; }
  .tmpl-hover { position: absolute; inset: 0; display: flex; align-items: flex-end; justify-content: center; gap: 8px; padding: 10px; background: linear-gradient(to top, rgba(0, 0, 0, .4), rgba(0, 0, 0, .05) 45%, transparent 70%); opacity: 0; transition: opacity .12s; }
  .tmpl-card:hover .tmpl-hover { opacity: 1; }
  .tmpl-act { position: relative; width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center; border: none; border-radius: 7px; background: rgba(255, 255, 255, .95); color: var(--text); cursor: pointer; box-shadow: 0 2px 8px rgba(0, 0, 0, .15); }
  .tmpl-act.primary { background: var(--accent); color: #fff; }
  .tmpl-act:hover { transform: translateY(-1px); }
  .tmpl-act svg { flex: none; width: 13px; height: 13px; }
  .tmpl-body { padding: 10px 12px 12px; flex: 1; }
  .tmpl-body b { font-size: 13px; }
  .tmpl-sub { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; margin-top: 4px; }
  .tmpl-desc { flex: 1; min-width: 0; font-size: 12px; color: var(--dim); line-height: 1.4; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tmpl-meta { flex: none; display: inline-flex; align-items: center; gap: 6px; font-size: 11px; color: #b3ada2; }
  .tmpl-pg { color: var(--dim); background: var(--border); border-radius: 4px; padding: 0 4px; line-height: 15px; }
  .site-grp-fav { width: 14px; height: 14px; border-radius: 3px; flex: none; object-fit: cover; }
  .plus-ic { flex: none; }
  .icon-btn { background: none; border: none; cursor: pointer; padding: 6px; border-radius: 8px; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .icon-btn:hover { background: #f1efe9; color: var(--text); }
  .icon-btn:disabled { opacity: .55; cursor: default; }
  .rfz { display: block; }
  .rfz.spin { animation: rspin .8s linear infinite; }
  @keyframes rspin { to { transform: rotate(360deg); } }
  @keyframes pop { from { opacity: 0; transform: translateY(4px); } }
  .x { background: none; border: none; color: var(--faint); font-size: 20px; cursor: pointer; line-height: 1; }
  .x:hover { color: var(--err); } .x.sm { font-size: 15px; }

  .mask { position: fixed; inset: 0; background: rgba(25,20,10,.28); z-index: 50; }
  .sheet, .modal { position: fixed; z-index: 60; background: var(--bg); border: 1px solid var(--border); box-shadow: var(--shadow-lg); display: flex; flex-direction: column; }
  .sheet { top: 0; right: 0; bottom: 0; width: min(400px, 92vw); border-radius: 0; }
  /* margin:auto 居中而非 transform——transform 会给内部 position:fixed 的下拉菜单
     制造 containing block，菜单被钳进弹窗坐标系再被 overflow:hidden 裁掉（遮挡）。 */
  .modal { inset: 0; margin: auto; height: fit-content; max-height: 88vh; width: min(440px, 92vw); border-radius: 14px; overflow: hidden; }
  .sheet-head { box-sizing: border-box; min-height: 48px; display: flex; justify-content: space-between; align-items: center; padding: 7px 14px; border-bottom: 1px solid var(--border); }
  .sheet-head > b { min-width: 0; overflow: hidden; font-size: 13.5px; line-height: 1.2; text-overflow: ellipsis; white-space: nowrap; }
  /* 弹窗正文统一只允许纵向滚动。只声明 overflow-y:auto 会按 CSS 规则把横向轴
     隐式计算成 auto；贴边 footer 的负边距和 WebView 的浮点取整都会因此冒出横向条。 */
  .sheet-body { min-width: 0; padding: 16px 18px; overflow-x: hidden; overflow-y: auto; display: flex; flex-direction: column; gap: 7px; }
  :global(.modal[data-dialog-geometry="true"] > .sheet-head) { flex: none; user-select: none; }
  :global(.modal[data-dialog-geometry="true"] > .sheet-head[data-dialog-dragging="true"]) { cursor: grabbing !important; }
  :global(.modal[data-dialog-resizable="true"] > .sheet-body) { flex: 1 1 auto; min-width: 0; min-height: 0; }
  :global(.modal[data-dialog-resizable="true"] > .sheet-body > textarea.tin) { flex: 1 1 auto; min-height: 120px; resize: none; }
  :global([data-dialog-resize="se"])::after {
    position: absolute;
    right: 3px;
    bottom: 3px;
    width: 6px;
    height: 6px;
    border-right: 1.5px solid var(--faint);
    border-bottom: 1.5px solid var(--faint);
    content: "";
    opacity: .52;
    transition: border-color .15s, opacity .15s;
  }
  :global([data-dialog-resize="se"]:hover)::after,
  :global([data-dialog-resize="se"]:focus-visible)::after { border-color: var(--accent); opacity: .9; }
  :global(.sheet-width-handle:hover .sheet-width-handle-grip),
  :global(.sheet-width-handle:focus-visible .sheet-width-handle-grip) { background: var(--accent) !important; opacity: .82 !important; }
  :global(.sheet-width-handle:focus-visible) { outline: 2px solid var(--accent) !important; outline-offset: -4px !important; }
  .update-center-mask { z-index: 65; }
  .update-center-sheet { z-index: 70; width: min(540px, 96vw); background: #fbfaf7; }
  .about-update-entry { display: inline-flex; align-items: center; gap: 5px; }
  .about-update-entry :global(svg) { flex: none; color: var(--dim); }
  .update-center-title { min-width: 0; display: flex; align-items: center; gap: 7px; }
  .update-center-title-icon { flex: none; width: 26px; height: 26px; display: grid; place-items: center; border-radius: 7px; background: var(--accent-soft); color: var(--accent); }
  .update-center-title-copy { min-width: 0; display: grid; gap: 1px; }
  .update-center-title-copy b { font-size: 13.5px; line-height: 1.2; }
  .update-center-title-copy small { color: var(--faint); font-size: 9.5px; line-height: 1.2; font-weight: 400; }
  .update-center-head-actions { display: flex; align-items: center; gap: 5px; }
  .update-center-body { flex: 1 1 auto; min-height: 0; gap: 13px; padding: 15px 17px 18px; }
  .update-center-notice { padding: 8px 10px; border: 1px solid #e6dfd3; border-radius: 9px; background: #f7f4ee; color: var(--dim); font-size: 11px; line-height: 1.5; }
  .update-center-notice.success { border-color: #cfe5d8; background: #eef8f2; color: var(--ok); }
  .update-center-notice.error { border-color: var(--err-border); background: var(--err-soft); color: var(--err); }
  .update-section { display: grid; gap: 7px; }
  .update-item { min-width: 0; display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 9px; padding: 9px 10px; }
  .update-item.pilot { border: 1px solid var(--border); border-radius: 11px; background: #fff; }
  .update-item-icon { flex: none; width: 28px; height: 28px; display: grid; place-items: center; border-radius: 8px; background: var(--soft); color: var(--dim); }
  .update-item-icon.package { color: var(--accent); }
  .update-item-copy { min-width: 0; display: grid; gap: 2px; }
  .update-item-copy b { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11.5px; }
  .update-item-copy small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--dim); font-size: 10.5px; }
  .update-item-copy i { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--faint); font-size: 9.5px; font-style: normal; }
  .update-state { flex: none; min-width: 48px; padding: 3px 6px; border-radius: 999px; background: var(--soft); color: var(--dim); font-size: 9.5px; font-weight: 650; line-height: 1.25; text-align: center; white-space: nowrap; }
  .update-state.available { background: #fff1dc; color: #a65b00; }
  .update-state.current, .update-state.done { background: #eaf6ee; color: var(--ok); }
  .update-state.checking, .update-state.updating { background: #eceff6; color: #46619b; }
  .update-state.waiting { background: #f3f0e9; color: #826c47; }
  .update-state.unreachable, .update-state.failed { background: var(--err-soft); color: var(--err); }
  .update-item .btn.small { flex: none; min-width: 42px; padding-inline: 7px; }
  .update-center-footnote { margin: 0; padding: 9px 10px; border-radius: 9px; background: #f1efe9; color: var(--dim); font-size: 10px; line-height: 1.55; }
  .update-center-footer { flex: none; display: flex; align-items: center; justify-content: flex-end; gap: 8px; padding: 11px 17px; border-top: 1px solid var(--border); background: #fff; }
  .conn-remark { cursor: help; }
  .conn-remark-modal { width: min(370px, 92vw); }
  .conn-remark-modal .sheet-head > div { min-width: 0; display: flex; align-items: baseline; gap: 8px; }
  .conn-remark-modal .sheet-head b { flex: none; }
  .conn-remark-modal .sheet-head small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sec-head { display: flex; justify-content: space-between; align-items: center; font-size: 11px; letter-spacing: .03em; text-transform: uppercase; color: var(--faint); font-weight: 600; margin-bottom: 1px; }
  .sec-head.mt { margin-top: 12px; }
  .conn-list { display: flex; flex-direction: column; gap: 5px; }
  .conn-row { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; padding: 8px 10px; border: 1px solid var(--border); border-radius: 11px; cursor: pointer; transition: border-color .12s, background .12s; }
  .conn-row:hover { background: #faf9f6; }
  .conn-row.on { border-color: #cfc9ec; background: #f7f6ff; }
  .conn-row :global(.sm) { border-radius: 6px; }
  .conn-main { flex: 1; min-width: 0; } .conn-main b { display: block; font-size: 13.5px; } .conn-main small { display: block; color: var(--dim); font-size: 11px; }
  /* ★ 上面这条 display:block（0,1,1）会压过 .cs-os 的 inline-flex（0,1,0）：设置弹窗的连接行里
     发行版图标于是退回**行内基线对齐**——12px 的图标坐在 11px 文字的基线上，整个上蹿一截。
     这里把 flex 垂直居中赢回来（用 flex 而非 inline-flex：副标题本来就独占一行）。 */
  .conn-main small.cs-os { display: flex; align-items: center; }
  .icon-btn.sm { padding: 4px; border-radius: 7px; }
  .brain-row { display: flex; align-items: center; gap: 11px; padding: 0; }
  .brain-auth-actions { flex: none; display: inline-flex; align-items: center; gap: 8px; }
  .claude-token-trigger { flex: none; width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center; padding: 0; border: 0; border-radius: 7px; background: transparent; color: var(--dim); cursor: pointer; }
  .claude-token-trigger:hover, .claude-token-trigger.active { background: #f1efe9; color: var(--accent); }
  .claude-token-form { margin: 7px 0 2px 33px; padding: 10px; display: grid; gap: 8px; border: 1px solid var(--border); border-radius: 10px; background: #faf9f6; }
  .claude-token-copy { min-width: 0; display: grid; gap: 3px; }
  .claude-token-copy b { font-size: 11.5px; font-weight: 650; }
  .claude-token-copy small, .claude-token-safe { color: var(--dim); font-size: 9.5px; line-height: 1.45; }
  .claude-token-copy code { padding: 1px 4px; border-radius: 4px; background: #efede7; color: var(--text); font: 500 9.5px/1.3 ui-monospace, SFMono-Regular, Menlo, monospace; }
  .claude-token-entry { display: flex; align-items: center; gap: 6px; }
  .claude-token-entry input { min-width: 0; flex: 1; height: 30px; box-sizing: border-box; padding: 0 9px; border: 1px solid var(--border2); border-radius: 7px; outline: none; background: #fff; color: var(--text); font: 11px ui-monospace, SFMono-Regular, Menlo, monospace; }
  .claude-token-entry input:focus { border-color: #c7a599; box-shadow: 0 0 0 2px rgb(160 60 43 / 8%); }
  .claude-token-entry .btn { flex: none; }
  /* 状态点放进与 .icon-btn 同宽(27px)的居中盒，右缘、图标中心都与本地模型区的刷新图标对齐。 */
  .brain-dot { flex: none; width: 27px; display: inline-flex; align-items: center; justify-content: center; }
  /* 「账号 / 自定义模型」两个折叠头并排一行；展开体各自出现在行下 */
  .cust-heads { display: flex; align-items: center; gap: 16px; }
  /* 当前账号 + 紧凑操作：展开体缩进对齐 chevron 后的文字；完整动作通过 tips 说明。 */
  .brain-auth { display: flex; align-items: center; gap: 6px; margin: 3px 0 1px 15px; min-width: 0; }
  .brain-acct { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--dim); font-size: 11px; }
  .brain-auth-sep { color: var(--faint); font-size: 11px; }
  .brain-auth-icon { flex: none; width: 20px; height: 20px; display: inline-flex; align-items: center; justify-content: center; padding: 0; border: none; border-radius: 5px; background: transparent; color: var(--faint); cursor: pointer; }
  .brain-auth-icon:hover, .brain-auth-icon.active { color: var(--accent); background: #f1efe9; }
  .brain-auth-icon:disabled { opacity: .6; cursor: default; }
  .brain-auth-icon :global(.rfz) { width: 12px; height: 12px; }
  .brain-auth-icon .wr-spin { width: 10px; height: 10px; }
  .cli-new { color: var(--ok); }
  .cli-upg { flex: none; width: 24px; height: 24px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 6px; color: var(--ok); cursor: pointer; }
  .cli-upg:hover:not(:disabled) { background: #f1efe9; }
  .cli-upg:disabled { color: var(--faint); cursor: default; }
  .verify-bad { color: #b45309; font-size: 10px; font-weight: 550; white-space: nowrap; }
  .verify-checking { color: var(--faint); font-size: 10px; white-space: nowrap; }
  .brain-ic { flex: none; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; }
  .brain-main { flex: 1; min-width: 0; } .brain-main b { display: block; font-size: 13.5px; line-height: 1.15; } .brain-main small { display: block; color: var(--dim); font-size: 11px; line-height: 1.1; }
  /* 自定义模型：折叠管理器（对齐大脑名，缩进 33px = 图标 22 + gap 11） */
  /* 大脑列表：整列作为一个 sheet-body 子项，块间距自控（不吃 sheet-body 的 7px gap）。 */
  .brains-list { display: flex; flex-direction: column; gap: 14px; }
  /* 每个大脑（行 + 自定义模型）成一块，块内贴紧。 */
  .brain-block { display: flex; flex-direction: column; gap: 0; }
  /* 未安装行：一键安装挪到状态文字下方（缩进对齐 brain-main），复制小图标同行在旁 */
  .brain-install { display: flex; align-items: center; gap: 5px; margin: 3px 0 2px 33px; }
  .cust { margin: 4px 0 0 33px; }
  .cust-head { display: inline-flex; align-items: center; gap: 5px; background: none; border: none; padding: 0; cursor: pointer; font: inherit; font-size: 11.5px; color: var(--dim); -webkit-appearance: none; appearance: none; }
  .cust-head:hover { color: var(--text); }
  .cust-chev { color: var(--faint); flex: none; transition: transform .12s; }
  .cust-chev:not(.open) { transform: rotate(-90deg); }
  .cust-n { min-width: 15px; height: 15px; padding: 0 4px; display: inline-flex; align-items: center; justify-content: center; background: var(--border); color: var(--dim); border-radius: 8px; font-size: 10px; font-weight: 600; }
  .cust-body { display: flex; flex-direction: column; gap: 5px; margin: 4px 0 4px; }
  .cust-chip { display: flex; align-items: center; gap: 6px; background: #f6f5f1; border: 1px solid var(--border); border-radius: 8px; padding: 4px 6px 4px 9px; }
  .cust-id { flex: 1; min-width: 0; font-family: ui-monospace, monospace; font-size: 11.5px; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .cust-x { flex: none; background: none; border: none; color: var(--faint); font-size: 15px; line-height: 1; padding: 0 3px; border-radius: 5px; cursor: pointer; }
  .cust-x:hover { color: var(--err); background: #fff; }
  .cust-add { display: flex; gap: 6px; align-items: center; }
  .cust-add .tin { flex: 1; min-width: 0; width: auto; }
  .cust-add .btn { flex: none; }
  .hint { color: var(--dim); font-size: 12px; margin: 2px 0; line-height: 1.6; }
  .hint.tos { color: var(--faint); margin-top: 12px; }
  .warn-text { color: var(--warn); }
  .dim { color: var(--dim); }
  .tin { width: 100%; }
  .row-end { display: flex; justify-content: flex-end; align-items: center; gap: 7px; margin-top: 2px; }

  /* 全局共享 hover 气泡：挂到 body，真实测量后避让四边；高于弹窗，永远不被局部层级压住。 */
  :global(.ui-tip) { position: fixed; z-index: 1400; box-sizing: border-box; pointer-events: none; visibility: hidden; background: #26241f; color: #fff; font-size: 11px; line-height: 1.5; padding: 5px 9px; border-radius: 7px; width: max-content; max-width: min(320px, calc(100vw - 16px)); white-space: pre-line; overflow-wrap: anywhere; text-align: left; box-shadow: 0 5px 16px rgba(0, 0, 0, .18); animation: tipin .12s ease-out both; }

  /* 通用文字链按钮（任务卡「查看对话/运行记录」、托管卡操作等）——此前从未定义,一直是系统默认灰凸样式 */
  .link { border: none; background: none; padding: 0; font: inherit; font-size: 12.5px; color: var(--accent); cursor: pointer; white-space: nowrap; }
  .link:hover { text-decoration: underline; }

  /* 弹窗底部操作区统一使用紧凑规格。正文末尾的 row-end 也铺到弹窗边缘，
     看起来和独立 footer 是同一套组件，同时不影响表单内部的小型操作行。 */
  .modal > .sheet-body > .row-end:last-child {
    box-sizing: border-box;
    min-height: 40px;
    flex: none;
    margin: 5px -18px -16px;
    padding: 5px 14px 7px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  .update-center-footer,
  .ed-footer {
    box-sizing: border-box;
    min-height: 40px;
    gap: 6px;
    padding: 5px 14px 7px;
  }
  .modal > .sheet-body > .row-end:last-child .btn,
  .update-center-footer .btn,
  .ed-footer .btn {
    box-sizing: border-box;
    min-height: 28px;
    padding: 4px 10px;
    border-radius: 7px;
    font-size: 11.5px;
    line-height: 1.15;
  }
  @media (max-width: 760px) {
    .center.launch-center { padding: 20px 18px; }
  }
</style>
