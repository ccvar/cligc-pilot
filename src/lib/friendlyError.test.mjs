import { test } from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import ts from 'typescript';
import { read, asScript } from './testing/appSource.mjs';

// 直接跑真实的翻译器：纯函数，没有 Svelte 和 Tauri 依赖
const code = ts.transpileModule(asScript(read("lib/friendlyError.ts")).replace(/^export interface[\s\S]*?^}\n/m, ""), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
const ctx = {};
vm.createContext(ctx); vm.runInContext(code, ctx);
const { explainError, friendly } = ctx;

test('Cloudflare 401/403 → 令牌权限的提示，原文保留', () => {
  const x = explainError('Cloudflare 403: Authentication error');
  assert.ok(x.known);
  assert.match(x.text, /Cloudflare 令牌/);
  assert.equal(x.raw, 'Cloudflare 403: Authentication error');
  assert.match(friendly('Cloudflare 403: Authentication error'), /\n（原文：Cloudflare 403: Authentication error）$/);
});

test('GitHub 404 / 429 / 5xx 各说各的', () => {
  assert.match(explainError('GitHub 返回 404').text, /GitHub上找不到|GitHub 上找不到/);
  assert.match(explainError('Bing 返回 429：Too many requests').text, /太频繁/);
  assert.match(explainError('Google 返回 502：bad gateway').text, /Google那边|Google 那边/);
  assert.match(explainError('模型服务返回 HTTP 401').text, /API 密钥/);
});

test('网络类：连不上 / 超时', () => {
  assert.match(explainError('请求 Cloudflare 失败: error sending request for url (https://api.cloudflare.com/)').text, /连不上/);
  assert.match(explainError('connect ECONNREFUSED 127.0.0.1:8787').text, /连不上/);
  assert.match(explainError('operation timed out').text, /等了太久/);
  assert.match(explainError('dns error: failed to lookup address information').text, /网络或代理/);
});

test('git 身份验证 / 文件不存在 / 没权限 / 钥匙串 / wrangler', () => {
  assert.match(explainError('git push 失败：remote: Invalid username or token. fatal: Authentication failed for https://github.com/x/y').text, /身份验证没通过/);
  assert.match(explainError('No such file or directory (os error 2)').text, /找不到这个文件/);
  assert.match(explainError('Permission denied (os error 13)').text, /没有权限/);
  assert.match(explainError('钥匙串里没有这个连接的密码，请补录密码').text, /密钥找不到了/);
  assert.match(explainError('无法读取系统凭据存储，请检查钥匙串授权后重试').text, /钥匙串授权/);
  assert.match(explainError('spawn wrangler ENOENT').text, /还没装 wrangler/);
});

test('中文前缀保留，只翻译后面的具体报错', () => {
  const x = explainError('读取仓库失败：GitHub 返回 401');
  assert.ok(x.known);
  assert.match(x.text, /^读取仓库失败：GitHub 不认这个令牌/);
});

test('认不出的原样返回；Error 对象取 message；空值不炸', () => {
  assert.equal(JSON.stringify(explainError('上一轮还在进行中，请稍候')), JSON.stringify({ text: '上一轮还在进行中，请稍候', raw: '上一轮还在进行中，请稍候', known: false }));
  assert.equal(friendly(new Error('站点没有填写域名')), '站点没有填写域名');
  assert.equal(friendly('Error: 这个连接不是 Cloudflare'), '这个连接不是 Cloudflare');
  assert.equal(friendly(undefined), '');
  // 后端自己已经写清楚的 wrangler 提示不改写
  assert.equal(explainError('没有找到 wrangler，也没有 Node.js。请先安装 Node.js（nodejs.org），再执行 npm i -g wrangler。').known, false);
});
