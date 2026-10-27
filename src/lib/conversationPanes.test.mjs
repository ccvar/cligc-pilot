import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import {leaves,pane,removePane,replacePane,splitPane} from './conversationLayout.ts';
import {appSource,sliceSource,withStores} from './testing/appSource.mjs';

const source=appSource();
const canvasSource=fs.readFileSync(new URL('./ConversationCanvas.svelte',import.meta.url),'utf8');
const slice=(from,to)=>{const a=source.indexOf(from),b=source.indexOf(to,a);assert.ok(a>=0&&b>a);return source.slice(a,b);};
const code=ts.transpileModule(sliceSource('lib/state/panes.svelte.ts','export function splitConversation(','export function initPanes('),{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
function harness(){
  const c=id=>({id,conn_id:'conn-a',site_slug:`site-${id}`,model:'test',brain:'codex',messages:[],status:'idle'});
  const a=c('a'),b=c('b'),calls=[],results={};
  const ctx={paneCfReady:{},paneUploading:{},splitSnapshots:{a,b},splitDrafts:{a:{text:'A draft',files:[],sticky:true},b:{text:'B draft',files:[],sticky:true}},splitErrors:{},splitQueued:{},convos:[a,b],activeConv:a,activeConvId:'a',threadModel:'test',threadPerm:'full',threadEffort:'',running:{},lives:{},autoRetried:{},retryExhausted:{},ATT_MARKER:'ATTACHMENTS',
    conversationLayout:splitPane(pane('a','pa'),'pa','b','right'),splitFocused:'pa',leaves,pane,removePane,replacePane,splitPane,
    $state:v=>v,tick:()=>Promise.resolve(),
    conversationLayoutWorkspace:'conn-a',openConvSeq:0,cancelConversationPointerDrag:()=>{},threadEl:null,threadStickBottom:true,
    optimisticUser:text=>({role:'user',text}),makeChannel:id=>({id}),
    beginTurn:(id,c)=>{ctx.running[id]=c.conn_id;ctx.lives[id]={startedAt:42};ctx.splitSnapshots[id]=c;},
    endTurn:(c,id)=>{ctx.splitSnapshots[id]=c;delete ctx.running[id];delete ctx.lives[id];},
    refreshConvos:async()=>{},maybeAutoRetry:()=>{},failTurn:async(e,id)=>{delete ctx.running[id];ctx.restoreSplitQueue(id);},
    invoke:async(command,args)=>{calls.push({command,args});if(command==='send_message')return new Promise((resolve,reject)=>{results[args.convId]={resolve,reject};});return 'saved/file.txt';},
    conversationProject:c=>c.site_slug,attachmentFilename:f=>f.name,ATTACH_RAW_MAX:200*1024*1024,ATTACH_RAW_TOO_BIG:'too big',filePreview:async()=>'',saveAttachmentBytes:(t,filename)=>ctx.invoke('save_attachment',{...t,filename}),openConv:()=>Promise.resolve(),say:()=>{},view:'thread',SITES_CONN_ID:'__sites__',SITE_PREFIX:'site:',paneTabs:{},tab:'overview',list:[],draft:'',attachments:[],queued:null,splitSuspended:false,splitVisible:true,conns:[],Date,Uint8Array,
  };
  vm.createContext(withStores(ctx));vm.runInContext(code,ctx);return{ctx,calls,results};
}
test('simultaneous sends stay bound to their conversations after focus switches',async()=>{
  const {ctx,calls,results}=harness();
  const first=ctx.splitSubmit('a');ctx.activeConvId='b';ctx.activeConv=ctx.splitSnapshots.b;
  const second=ctx.splitSubmit('b');
  assert.deepEqual(calls.map(c=>[c.args.convId,c.args.message]),[['a','A draft'],['b','B draft']]);
  results.a.resolve({...ctx.splitSnapshots.a,messages:[{role:'assistant',text:'A done'}]});await first;
  assert.equal(ctx.activeConvId,'b');assert.equal(ctx.splitSnapshots.a.messages[0].text,'A done');
  results.b.resolve({...ctx.splitSnapshots.b,messages:[{role:'assistant',text:'B done'}]});await second;
  assert.equal(ctx.splitSnapshots.b.messages[0].text,'B done');
});
test('queue is per conversation and preserves a newer unsent draft when it drains',()=>{
  const {ctx,calls}=harness();ctx.running.a='conn-a';ctx.lives.a={startedAt:42};
  ctx.splitSubmit('a');assert.equal(calls.length,0);assert.equal(ctx.splitQueued.a.text,'A draft');
  ctx.splitDrafts.a.text='newer A';ctx.activeConvId='b';delete ctx.running.a;
  ctx.finishSplitQueue('a',{messages:[{role:'user',text:'previous'}]},false,42);
  assert.equal(calls[0].args.convId,'a');assert.equal(calls[0].args.message,'A draft');
  assert.equal(ctx.splitDrafts.a.text,'newer A');assert.equal(ctx.splitDrafts.b.text,'B draft');
});
test('failure, stale turn, duplicate or stopping restores queue without sending',async()=>{
  for(const [failed,turn,text] of [[true,42,'previous'],[false,43,'previous'],[false,42,'A draft']]) {
    const {ctx,calls}=harness();ctx.splitQueued.a={text:'A draft',files:[],startedAt:42};ctx.splitDrafts.a.text='newer';
    ctx.finishSplitQueue('a',{messages:[{role:'user',text}]},failed,turn);
    assert.equal(calls.length,0);assert.equal(ctx.splitDrafts.a.text,'A draft\n\nnewer');
  }
  const {ctx,calls}=harness();ctx.running.a='conn-a';ctx.splitQueued.a={text:'pending',files:[],startedAt:42};
  await ctx.splitStop('a');assert.equal(calls[0].command,'cancel_turn');assert.equal(calls[0].args.convId,'a');assert.equal(ctx.splitQueued.a,undefined);
});
test('closing an active pane neither cancels work nor drops drafts',()=>{
  const {ctx,calls}=harness();ctx.running.a='conn-a';ctx.closeSplit('pa');
  assert.equal(calls.length,0);assert.equal(ctx.running.a,'conn-a');assert.equal(ctx.splitDrafts.a.text,'A draft');
  assert.equal(leaves(ctx.conversationLayout).length,1);assert.equal(ctx.activeConvId,'b');
});
test('last remaining pane stays an ordinary conversation with no alternate view',()=>{
  const {ctx,calls}=harness();ctx.closeSplit('pa');
  const last=leaves(ctx.conversationLayout)[0];ctx.closeSplit(last.id);
  assert.equal(leaves(ctx.conversationLayout).length,1);
  assert.equal(ctx.activeConvId,'b');assert.equal(calls.length,0);
  assert.doesNotMatch(source,/splitSuspended|expandSplit|split-entry|返回分屏|<ConversationPane/);
  assert.match(source,/@render conversationHeader\(c,paneId\)/);
  assert.match(source,/@render convPane\(c,paneId\)/);
  assert.match(source,/@render convPane\(app\.activeConv\)/);
});
test('upload completion appends to the captured conversation, never the focused one',async()=>{
  const {ctx,calls}=harness();let resolve;
  ctx.invoke=(command,args)=>{calls.push({command,args});return new Promise(r=>resolve=r);};
  const uploading=ctx.splitUpload(ctx.splitSnapshots.a,{size:3,name:'test.txt',arrayBuffer:async()=>new ArrayBuffer(3)});
  await new Promise(r=>setImmediate(r));ctx.activeConvId='b';resolve('attachments/test.txt');await uploading;
  assert.equal(calls[0].args.connId,'conn-a');assert.equal(ctx.splitDrafts.a.files.length,1);assert.equal(ctx.splitDrafts.b.files.length,0);
});

test('real turn lifecycle updates background results without stealing focus, even after its pane closes',()=>{
  const {ctx}=harness();
  Object.assign(ctx,{
    retryTimers:{},clearTimeout,
    queued:null,scrollSoon:()=>{},checkCfReady:()=>{},activeConnId:'conn-b',
  });
  const lifecycle=ts.transpileModule(sliceSource('lib/state/turns.svelte.ts','export function beginTurn(','export async function failTurn('),{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
  vm.runInContext(lifecycle,ctx);
  ctx.closeSplit('pa');assert.equal(ctx.activeConvId,'b');
  const updated={...ctx.splitSnapshots.a,messages:[{role:'user',text:'background request'}],updated_at:1};
  ctx.beginTurn('a',updated);
  assert.equal(ctx.activeConvId,'b');assert.equal(ctx.running.a,'conn-a');
  const finished={...updated,messages:[...updated.messages,{role:'assistant',text:'completed in background'}],updated_at:2};
  ctx.endTurn(finished,'a');
  assert.equal(ctx.activeConvId,'b');assert.equal(ctx.splitSnapshots.a.messages.length,2);
  assert.equal(ctx.convos.find(c=>c.id==='a').messages.length,2);assert.equal(ctx.running.a,undefined);
});

test('pane actions render conversation-bound file and approval handlers',()=>{
  assert.match(source,/mdClick\(e,context\)/);
  assert.match(source,/thumbKey\(p,context\)/);
  assert.match(source,/onpointerdown=\{\(e\)=>startConversationPointerDrag\(e,c.id\)\}/);
});

test('conversation header keeps model and site identity on one compact row',()=>{
  assert.match(source,/class="conversation-head-title"[\s\S]*?<BrainIcon brain=\{modelIcon\(activeConv\?\.brain\?\?'claude'\)\}/);
  assert.match(source,/class="th-open conversation-site-link"[\s\S]*?<SiteFav src=\{headerSiteIcon\}/);
  assert.doesNotMatch(slice('{#snippet conversationHeader', '{/snippet}'),/<small/);
  assert.match(source,/\.conversation-head-title>span:last-child\{[^}]*text-overflow:ellipsis;[^}]*white-space:nowrap/);
  assert.match(source,/\.conversation-head-model\{[^}]*line-height:0\}/);
  assert.doesNotMatch(source,/\.conversation-head-model\{[^}]*transform:/);
  assert.match(source,/\.conversation-site-link\{max-width:52%;min-width:0;flex:none\}/);
  assert.match(source,/\.thread-head\.conversation-thread-head\s*\{[^}]*height: 30px; min-height: 30px;[^}]*align-items: center/);
});

test('split canvas always fits its viewport instead of creating page-level scrollbars',()=>{
  assert.doesNotMatch(canvasSource,/style:min-width|style:min-height|minimumSize/);
  assert.match(canvasSource,/\.canvas-scroll\{[^}]*overflow:hidden;[^}]*min-width:0/);
  assert.match(canvasSource,/\.canvas\{[^}]*width:100%;height:100%;min-width:0;min-height:0/);
});

test('drag feedback keeps the conversation ghost and restores the blurred split target',()=>{
  assert.match(source,/class="conversation-drag-ghost"[\s\S]*?<BrainIcon brain=\{modelIcon\(dragConversation\.brain\)\}/);
  assert.match(source,/splitDragPoint=\{x:event\.clientX,y:event\.clientY\}/);
  assert.match(canvasSource,/const next=splitPane\(layout,destination\.id,dragConversation,destination\.edge/);
  assert.match(canvasSource,/class="drop-preview"[\s\S]*?<span>分屏视图<\/span>/);
  assert.match(canvasSource,/backdrop-filter:blur\(4px\)/);
  assert.match(source,/class=\{`single-split-preview \$\{panes\.singleDropEdge\}`\}[\s\S]*?<span>分屏视图<\/span>/);
});

test('dropping outward from a boundary pane appends to the full canvas',()=>{
  assert.match(canvasSource,/local==='right'&&Math\.abs\(r\.right-bounds\.right\)<2/);
  assert.match(canvasSource,/hover=promoted\?\{id:layout\.id,edge:promoted\}:\{id,edge:local\}/);
});

test('settings replies update the originating conversation after focus changes',async()=>{
  const {ctx,calls}=harness();let finish;
  ctx.invoke=(command,args)=>{calls.push({command,args});return new Promise(resolve=>finish=resolve);};
  const setting=ctx.splitSetting(ctx.splitSnapshots.a,'effort','high');
  ctx.activeConvId='b';ctx.activeConv=ctx.splitSnapshots.b;
  finish({...ctx.splitSnapshots.a,effort:'high'});await setting;
  assert.equal(calls[0].args.convId,'a');assert.equal(ctx.splitSnapshots.a.effort,'high');
  assert.equal(ctx.activeConv.id,'b');assert.equal(ctx.splitSnapshots.b.effort,undefined);
});

test('workspace switch removes every old pane without canceling work or losing pane drafts',()=>{
  const {ctx,calls}=harness();ctx.running.a='conn-a';
  ctx.splitQueued.a={text:'next',files:[],startedAt:42};
  ctx.switchConversationWorkspace('__workspace__');
  assert.equal(ctx.conversationLayout,null);assert.equal(ctx.splitFocused,'');
  assert.equal(ctx.activeConv,null);assert.equal(ctx.activeConvId,'');
  assert.equal(ctx.conversationLayoutWorkspace,'__workspace__');
  assert.equal(ctx.splitDrafts.a.text,'A draft');assert.equal(ctx.splitQueued.a.text,'next');
  assert.equal(ctx.running.a,'conn-a');assert.equal(calls.length,0);
});

test('same workspace refresh preserves its split layout and focus',()=>{
  const {ctx}=harness();const before=ctx.conversationLayout;
  ctx.switchConversationWorkspace('conn-a');
  assert.equal(ctx.conversationLayout,before);assert.equal(ctx.activeConvId,'a');
});

test('connection workspaces are isolated from one another and from local chats',()=>{
  const {ctx}=harness();ctx.switchConversationWorkspace('conn-b');
  assert.equal(ctx.conversationLayout,null);assert.equal(ctx.conversationLayoutWorkspace,'conn-b');
  assert.equal(ctx.conversationWorkspace({conn_id:'conn-a',task_type:'sitebuild'}),'conn-a');
  assert.equal(ctx.conversationWorkspace({conn_id:'__workspace__',task_type:'workspace'}),'__workspace__');
});

test('cross-workspace drag is rejected before fetching the conversation',async()=>{
  const {ctx,calls}=harness();ctx.splitSnapshots.b={...ctx.splitSnapshots.b,conn_id:'__workspace__',task_type:'workspace'};
  await ctx.addSplit('b','right','pa');
  assert.equal(calls.length,0);assert.equal(ctx.activeConvId,'a');
});

test('a delayed drop cannot recreate old panes after switching workspace',async()=>{
  const {ctx}=harness();let finish;
  ctx.invoke=()=>new Promise(resolve=>finish=resolve);
  const pending=ctx.addSplit('b','right','pa');
  ctx.switchConversationWorkspace('__workspace__');finish(ctx.splitSnapshots.b);await pending;
  assert.equal(ctx.conversationLayout,null);assert.equal(ctx.activeConvId,'');
});

function navigationHarness(){
  const h=harness(),{ctx}=h;
  Object.assign(ctx,{activeConnId:'conn-a',expandSite:()=>{},
    conversationGroupKey:c=>c.site_slug,scrollSoon:()=>{},checkCfReady:()=>{},
    selectConn:async id=>{ctx.switchConversationWorkspace(id);ctx.activeConnId=id;},
    crypto:globalThis.crypto});
  vm.runInContext(ts.transpileModule(sliceSource('lib/state/conversations.ts','export async function openConv(','export async function deleteConv('),
    {compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText,ctx);
  return h;
}

test('opening a local chat from search resets the old layout before the split fast path',async()=>{
  const {ctx}=navigationHarness();
  ctx.invoke=async()=>({id:'local-c',conn_id:'__workspace__',task_type:'workspace',model:'test'});
  await ctx.openConv('local-c');ctx.ensureConversationPane();
  assert.deepEqual(leaves(ctx.conversationLayout).map(p=>p.conversation),['local-c']);
  assert.equal(ctx.conversationLayoutWorkspace,'__workspace__');
});

test('a late open response cannot steal focus after a workspace switch',async()=>{
  const {ctx}=navigationHarness();let finish;
  ctx.invoke=()=>new Promise(resolve=>finish=resolve);
  const pending=ctx.openConv('b');ctx.switchConversationWorkspace('__workspace__');
  finish(ctx.splitSnapshots.b);await pending;
  assert.equal(ctx.activeConvId,'');assert.equal(ctx.conversationLayout,null);
});

test('returning to a connection opens only that connection and restores its draft',async()=>{
  const {ctx}=navigationHarness();ctx.switchConversationWorkspace('__workspace__');
  ctx.activeConnId='__workspace__';ctx.invoke=async()=>ctx.splitSnapshots.a;
  await ctx.openConv('a');ctx.ensureConversationPane();
  assert.equal(ctx.conversationLayoutWorkspace,'conn-a');
  assert.deepEqual(leaves(ctx.conversationLayout).map(p=>p.conversation),['a']);
  assert.equal(ctx.draft,'A draft');
});

test('workspace entry points all reset the conversation layout scope',()=>{
  assert.match(sliceSource('lib/state/connections.svelte.ts','export async function selectConn(','export function openLocalWorkspace('),/switchConversationWorkspace\(workspaceOf\(id\)\)/);
  assert.match(sliceSource('lib/state/connections.svelte.ts','export function openLocalWorkspace(','export function distroOf('),/selectConn\(WORKSPACE_CONN_ID\)/);
});

// ---------- 页面进分屏（新对话 / 全部站点 / 站点制作 / 托管运营 / 站点看板） ----------
const leafOf=(ctx,ref)=>leaves(ctx.conversationLayout).find(p=>p.conversation===ref).id;
function sitesHarness(){
  const h=harness(),{ctx}=h;
  for(const id of ['a','b'])ctx.splitSnapshots[id]={...ctx.splitSnapshots[id],conn_id:'site:s1',task_type:'site'};
  ctx.convos=[ctx.splitSnapshots.a,ctx.splitSnapshots.b];ctx.activeConv=ctx.splitSnapshots.a;
  ctx.conversationLayoutWorkspace='site:*';ctx.activeConnId='site:s1';
  ctx.selectConn=async id=>{ctx.selects=(ctx.selects??[]).concat(id);ctx.switchConversationWorkspace(ctx.workspaceOf(id));ctx.activeConnId=id;};
  return h;
}
test('the sites hub and every site are one split group, other connections stay separate',()=>{
  const {ctx}=harness();
  assert.equal(ctx.workspaceOf('__sites__'),'site:*');
  assert.equal(ctx.workspaceOf('site:abc'),'site:*');
  assert.equal(ctx.workspaceOf('conn-a'),'conn-a');
});
test('dragging a page next to conversations adds one pane and shows that page without touching running work',async()=>{
  const {ctx,calls}=sitesHarness();ctx.running.a='site:s1';
  await ctx.addSplit('page:managed','right',leafOf(ctx,'b'));
  assert.deepEqual(leaves(ctx.conversationLayout).map(p=>p.conversation),['a','b','page:managed']);
  await new Promise(r=>setTimeout(r,0));
  assert.deepEqual(ctx.selects,['__sites__']);assert.equal(ctx.view,'managed');assert.equal(ctx.activeConvId,'');
  assert.equal(ctx.running.a,'site:s1');assert.equal(ctx.splitDrafts.a.text,'A draft');assert.equal(calls.length,0);
});
test('the same page dragged again moves instead of opening a second copy',async()=>{
  const {ctx}=sitesHarness();
  await ctx.addSplit('page:sites','right',leafOf(ctx,'b'));
  await ctx.addSplit('page:sites','left','pa');
  const refs=leaves(ctx.conversationLayout).map(p=>p.conversation);
  assert.equal(refs.filter(r=>r==='page:sites').length,1);assert.equal(refs[0],'page:sites');
});
test('pages from another split group are refused',async()=>{
  const {ctx}=harness();const before=ctx.conversationLayout;
  await ctx.addSplit('page:managed','right',leafOf(ctx,'b'));
  assert.equal(ctx.conversationLayout,before);
});
test('a site board remembers its own tab when it gets focus back',async()=>{
  const {ctx}=sitesHarness();ctx.paneTabs={s2:'queue'};ctx.tab='settings';
  await ctx.addSplit('board:s2','right',leafOf(ctx,'b'));await new Promise(r=>setTimeout(r,0));
  assert.equal(ctx.tab,'queue');assert.equal(ctx.activeConnId,'site:s2');assert.equal(ctx.view,'site');
});
test('closing the focused page pane goes back to a conversation pane',async()=>{
  const {ctx}=sitesHarness();
  await ctx.addSplit('page:making','right',leafOf(ctx,'b'));await new Promise(r=>setTimeout(r,0));
  const page=leaves(ctx.conversationLayout).find(p=>p.conversation==='page:making');
  ctx.closeSplit(page.id);
  assert.deepEqual(leaves(ctx.conversationLayout).map(p=>p.conversation),['a','b']);
  assert.equal(ctx.view,'thread');assert.equal(ctx.activeConvId,'a');
});
test('sidebar menu items, site rows and the new-chat button can all start a split drag',()=>{
  assert.match(source,/startConversationPointerDrag\(e, 'page:launcher'\)/);
  assert.match(source,/startConversationPointerDrag\(e, 'page:sites'\)/);
  assert.match(source,/startConversationPointerDrag\(e, 'page:making'\)/);
  assert.match(source,/startConversationPointerDrag\(e, 'page:managed'\)/);
  assert.match(source,/startConversationPointerDrag\(e, `board:\$\{s\.id\}`\)/);
  // 对话行本身是按钮：拖动不能因为按下的是按钮就不起；只有删除 × 这类标了 data-no-drag 的不起
  assert.match(source,/class="convo-x"[^>]*data-no-drag/);
});
