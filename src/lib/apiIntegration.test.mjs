import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { appSource, stateFunctions, withStores } from './testing/appSource.mjs';

const page = appSource();
const functions = ['apiProfile', 'modelIcon', 'brainAvailability', 'brainUsable', 'modelOptsFor',
  'launcherModelOpts', 'defaultModelFor', 'isLauncherModel', 'customsOf', 'ctxLimit', 'ctxLimitAdaptive', 'newChat', 'maybeAutoRetry', 'brainSetupHint'];
const source = stateFunctions(functions);
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
const ready = { found: true, logged_in: true };
function context(profiles = []) {
  const ctx = { apiProfiles: profiles, apiProfilesLoaded: true, brains: { claude: ready, codex: ready, grok: ready },
    prefs: { customClaudeIds: ['local-only'], customCodexIds: [], customGrokIds: [] },
    CLAUDE_MODELS: [{ value: 'claude-sonnet-5', label: 'Sonnet' }], CODEX_MODELS: [], GROK_MODELS: [] };
  vm.createContext(withStores(ctx)); vm.runInContext(compiled, ctx); return ctx;
}
function profile(id, overrides = {}) {
  return { id, name: `Account ${id}`, protocol: 'openai_responses', enabled: true, key_configured: true,
    models: ['shared-model'], default_model: 'shared-model', ...overrides };
}

test('same vendor/model across two API accounts retains independent identity and status', () => {
  const ctx = context([profile('a'), profile('b', { enabled: false })]);
  assert.equal(ctx.defaultModelFor('api:a'), 'shared-model');
  assert.equal(ctx.brainUsable('api:a'), true);
  assert.equal(ctx.brainUsable('api:b'), false);
  assert.equal(ctx.brainAvailability('api:a'), 'API · Account a');
  assert.match(ctx.brainAvailability('api:b'), /停用/);
  assert.equal(ctx.launcherModelOpts('api:a').length, 1);
  assert.equal(ctx.customsOf('api:a').length, 0);
  assert.equal(ctx.modelIcon('api:a'), 'openai');
});

test('missing, disabled, keyless and model-less API profiles never borrow CLI readiness', () => {
  const ctx = context([profile('keyless', { key_configured: false }), profile('empty', { models: [] })]);
  for (const brain of ['api:missing', 'api:keyless', 'api:empty']) assert.equal(ctx.brainUsable(brain), false);
  assert.equal(ctx.brainUsable('claude'), true);
  assert.match(ctx.brainAvailability('api:missing'), /已移除/);
  assert.match(ctx.brainAvailability('api:keyless'), /API Key/);
});

test('API startup preserves saved model until metadata is loaded, does not guess context capacity', () => {
  const ctx = context(); ctx.apiProfilesLoaded = false;
  assert.equal(ctx.isLauncherModel('api:a', 'custom-model'), true);
  assert.equal(ctx.ctxLimitAdaptive('api:a', 'gpt-any', 900000), 0);
  assert.equal(ctx.defaultModelFor('claude'), 'claude-sonnet-5');
  assert.match(page, /if \(!launcher\.lBrain\.startsWith\('api:'\) && !brainUsable\(launcher\.lBrain\)\)/);
});

test('new conversation never silently replaces a missing API default', () => {
  const ctx = context();
  const notices = [];
  Object.assign(ctx, { openConvSeq: 0, activeConvId: '', activeConv: null, lDraft: '', draft: '', attachments: [],
    lSite: '', lBrain: 'api:removed', isSshConn: false, isWorkspaceChat: true, lPerm: 'auto', viewAfterConvGone() {},
    firstUsableBrain() { return 'claude'; }, say(message) { notices.push(message); } });
  ctx.newChat();
  assert.equal(ctx.lBrain, 'api:removed');
  assert.equal(ctx.lPerm, 'full');
  assert.match(notices[0], /API 连接已移除/);
});

test('API recovery never triggers the generic automatic CLI retry and points to API settings', () => {
  const ctx = context();
  Object.assign(ctx, { activeConvId: 'c', activeConv: { id: 'c', brain: 'api:a' } });
  ctx.maybeAutoRetry('c', true, 'network timeout'); // no retry timers or CLI state should be accessed
  assert.match(ctx.brainSetupHint('api:missing'), /API 服务/);
  assert.doesNotMatch(ctx.brainSetupHint('api:missing'), /去授权/);
});

test('API selection is shared by the launcher and existing conversations', () => {
  assert.match(page, /ALL_BRAINS = \$derived<Brain\[\]>/);
  assert.match(page, /apiProfiles\.map\(\(p\): Brain => `api:\$\{p\.id\}`\)/);
  assert.match(page, /<ApiServices profiles=\{app\.apiProfiles\}/);
  assert.match(page, /threadComboOpts = \$derived\(conversationModelOptions/);
  assert.match(page, /<ModelFx options=\{launcher\.comboOpts\} value=\{`\$\{launcher\.lBrain\}::\$\{launcher\.lModel\}`\}/);
  const fx = fs.readFileSync(new URL('./ModelFx.svelte', import.meta.url), 'utf8');
  assert.match(fx, /const fastable = \$derived\(!value\.startsWith\('api:'\)/);
  assert.match(fx, /\{#if !apiModel \|\| apiLevels\.length\}/);
  assert.match(fx, /apiReasoningLevels\(apiProtocol, model\)/);
});
