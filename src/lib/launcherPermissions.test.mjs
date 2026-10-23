import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const page = fs.readFileSync(new URL('../routes/+page.svelte', import.meta.url), 'utf8');
function section(start, end) {
  const a = page.indexOf(start), b = page.indexOf(end, a);
  assert.ok(a >= 0 && b > a, `${start} … ${end}`);
  return ts.transpileModule(page.slice(a, b), {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText;
}
const initial = section('  let lPerm =', '  let lEffort =');
const select = section('  async function selectConn(', '  function openLocalWorkspace(');

test('the launcher restores the saved permission default, full when nothing is saved', () => {
  for (const perm of [undefined, 'plan', 'ask', 'auto', 'full']) {
    const ctx = vm.createContext({ prefs: { brain: 'claude', perm }, $state: v => v });
    vm.runInContext(initial, ctx);
    assert.equal(vm.runInContext('lPerm', ctx), perm ?? 'full');
  }
});

test('new local chats reset the launcher default, without changing stored chats or other workspaces', () => {
  const code = section('  function newChat()', '  async function openConv(');
  for (const isWorkspaceChat of [true, false]) {
    const existing = { perm_mode: 'ask' };
    const ctx = vm.createContext({
      openConvSeq: 0, activeConvId: 'existing', activeConv: existing,
      lDraft: '', draft: '', attachments: [], lPerm: 'auto',
      isWorkspaceChat, isSshConn: false, lSite: '', lBrain: 'api:test',
      viewAfterConvGone: () => {}, brainUsable: () => true,
    });
    vm.runInContext(code, ctx);
    ctx.newChat();
    assert.equal(ctx.lPerm, isWorkspaceChat ? 'full' : 'auto');
    assert.equal(existing.perm_mode, 'ask');
    assert.equal(ctx.activeConv, null);
  }
});

function selectContext(conns, overrides = {}) {
  const ctx = vm.createContext({
    conns, activeConnId: '__workspace__', selSeq: 0, isMainWindow: false, view: 'launcher',
    activeConvId: '', activeConv: null, lPerm: 'full', lSite: 'old', lStyle: 'x', cfProjects: ['old'],
    switchConversationWorkspace() {}, probeOs() {}, disposeTerm() {}, say() {},
    invoke: async () => ['site-a'], localStorage: { setItem() {} },
    ...overrides,
  });
  vm.runInContext(select, ctx);
  return ctx;
}

test('switching to Cloudflare or SSH never keeps full automation; existing chats keep their own mode', async () => {
  for (const kind of ['cloudflare', 'ssh']) {
    for (const perm of ['plan', 'ask', 'auto', 'full']) {
      const existing = { perm_mode: perm };
      const ctx = selectContext([{ id: 'c1', kind }], { lPerm: perm, activeConv: existing });
      await ctx.selectConn('c1');
      assert.equal(ctx.lPerm, perm === 'full' ? 'auto' : perm, `${kind}/${perm}`);
      assert.equal(existing.perm_mode, perm);
      assert.equal(ctx.view, kind === 'ssh' ? 'remote' : 'launcher');
    }
  }
});

test('switching back to the local workspace clears the Cloudflare project and returns to the launcher', async () => {
  const ctx = selectContext([{ id: 'c1', kind: 'cloudflare' }], { activeConnId: 'c1', view: 'templates' });
  await ctx.selectConn('__workspace__');
  assert.equal(ctx.activeConnId, '__workspace__');
  assert.equal(ctx.lSite, '');
  assert.equal(ctx.view, 'launcher');
});
