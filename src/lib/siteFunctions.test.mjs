import { test } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import os from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { DatabaseSync } from 'node:sqlite';

// Cligc Pilot 给迁移站点生成的 Cloudflare Pages Functions（src-tauri/src/builtin/functions）：
// 按 Pages 的目录结构摆好，用内存 SQLite 模拟 D1、假的 R2，逐个场景跑一遍。
const src = fileURLToPath(new URL('../../src-tauri/src/builtin/functions/', import.meta.url));
const dir = fs.mkdtempSync(join(os.tmpdir(), 'pilot-fn-'));
fs.mkdirSync(join(dir, 'functions/api'), { recursive: true });
fs.writeFileSync(join(dir, 'package.json'), '{"type":"module"}');
for (const [from, to] of [['_lib.js', 'functions/_lib.js'], ['api_form.js', 'functions/api/form.js'], ['api_comments.js', 'functions/api/comments.js'], ['api_health.js', 'functions/api/health.js'], ['files.js', 'functions/[[path]].js']]) {
  fs.copyFileSync(join(src, from), join(dir, to));
}
const db = new DatabaseSync(':memory:');
db.exec(fs.readFileSync(join(src, 'schema.sql'), 'utf8'));
const DB = { prepare(sql) { const st = db.prepare(sql); let args = []; const o = { bind(...a) { args = a; return o; }, first: async () => st.get(...args) ?? null, all: async () => ({ results: st.all(...args) }), run: async () => st.run(...args) }; return o; } };

test('site functions: form, comments, health, R2 files', async () => {
  const form = await import(pathToFileURL(join(dir, 'functions/api/form.js')).href);
  const comments = await import(pathToFileURL(join(dir, 'functions/api/comments.js')).href);
  const health = await import(pathToFileURL(join(dir, 'functions/api/health.js')).href);
  const files = await import(pathToFileURL(join(dir, 'functions/[[path]].js')).href);
  const env = { DB };
  const req = (url, body, h = {}) => new Request('https://site.pages.dev' + url, { method: 'POST', headers: { 'cf-connecting-ip': '1.2.3.4', origin: 'https://site.pages.dev', ...h }, body });
  // 1. JSON 提交
  let r = await form.onRequestPost({ request: req('/api/form', JSON.stringify({ name: '张三', email: 'a@b.c', message: '你好', _form: 'contact', _page: '/contact/' }), { 'content-type': 'application/json' }), env });
  assert.equal(r.status, 200); assert.deepEqual(await r.json(), { ok: true });
  let row = db.prepare('SELECT * FROM submissions').get();
  assert.equal(row.form, 'contact'); assert.equal(row.page, '/contact/'); assert.deepEqual(JSON.parse(row.fields), { name: '张三', email: 'a@b.c', message: '你好' });
  assert.ok(row.ip.length === 24 && !row.ip.includes('1.2.3.4'));
  // 2. 普通表单提交 → 303 跳回来源页
  const fd = new FormData(); fd.set('email', 'x@y.z'); fd.set('_form', 'subscribe');
  r = await form.onRequestPost({ request: req('/api/form', fd, { accept: 'text/html', referer: 'https://site.pages.dev/news/' }), env });
  assert.equal(r.status, 303); assert.equal(r.headers.get('location'), 'https://site.pages.dev/news/?sent=1');
  // 3. 隐藏字段有值 → 假装成功、不入库
  r = await form.onRequestPost({ request: req('/api/form', JSON.stringify({ email: 'bot', _gotcha: 'x' }), { 'content-type': 'application/json' }), env });
  assert.equal(r.status, 200); assert.equal(db.prepare('SELECT COUNT(*) n FROM submissions').get().n, 2);
  // 4. 别的网站发来 → 403
  r = await form.onRequestPost({ request: req('/api/form', JSON.stringify({ a: 1 }), { 'content-type': 'application/json', origin: 'https://evil.com' }), env });
  assert.equal(r.status, 403);
  // 5. 限频：同一 IP 10 分钟最多 5 条
  for (let i = 0; i < 3; i++) await form.onRequestPost({ request: req('/api/form', JSON.stringify({ m: 'x' + i }), { 'content-type': 'application/json' }), env });
  r = await form.onRequestPost({ request: req('/api/form', JSON.stringify({ m: 'y' }), { 'content-type': 'application/json' }), env });
  assert.equal(r.status, 429);
  // 6. 空表单
  r = await form.onRequestPost({ request: req('/api/form', JSON.stringify({ _form: 'x' }), { 'content-type': 'application/json', 'cf-connecting-ip': '9.9.9.9' }), env });
  assert.equal(r.status, 400);
  // 7. Turnstile：配了密钥但没带令牌 → 拒绝
  r = await form.onRequestPost({ request: req('/api/form', JSON.stringify({ m: 'z' }), { 'content-type': 'application/json', 'cf-connecting-ip': '8.8.8.8' }), env: { ...env, TURNSTILE_SECRET_KEY: 's' } });
  assert.equal(r.status, 400); assert.equal((await r.json()).error, 'captcha');
  // 8. 评论：发 → 待审核 → GET 看不到 → 通过后看得到
  r = await comments.onRequestPost({ request: req('/api/comments', JSON.stringify({ path: '/a/', name: '李四', content: '写得好', url: 'javascript:alert(1)' }), { 'content-type': 'application/json' }), env });
  assert.deepEqual(await r.json(), { ok: true, pending: true });
  const get = (p) => comments.onRequestGet({ request: new Request('https://site.pages.dev/api/comments?path=' + encodeURIComponent(p)), env });
  assert.deepEqual((await (await get('/a/')).json()).comments, []);
  assert.equal(db.prepare('SELECT url FROM comments').get().url, '');
  db.exec("UPDATE comments SET status='approved'");
  const list = (await (await get('/a/')).json()).comments;
  assert.equal(list.length, 1); assert.equal(list[0].name, '李四'); assert.ok(!('email' in list[0]) && !('ip' in list[0]));
  r = await comments.onRequestPost({ request: req('/api/comments', JSON.stringify({ path: 'a', name: 'x', content: 'yy' }), { 'content-type': 'application/json' }), env });
  assert.equal(r.status, 400);
  r = await comments.onRequestPost({ request: req('/api/comments', JSON.stringify({ path: '/a/', name: 'x', content: 'auto' }), { 'content-type': 'application/json', 'cf-connecting-ip': '5.5.5.5' }), env: { ...env, COMMENTS_AUTO_APPROVE: '1' } });
  assert.deepEqual(await r.json(), { ok: true, pending: false });
  // 9. health
  r = await health.onRequestGet({ env }); assert.equal(r.status, 200); assert.equal((await r.json()).db, true);
  r = await health.onRequestGet({ env: {} }); assert.equal(r.status, 503);
  // 10. R2 大文件
  const store = { 'video/big.mp4': 'BYTES' };
  const FILES = { get: async (k) => k in store ? { body: store[k], size: 5, httpEtag: '"e"', writeHttpMetadata: (h) => h.set('content-type', 'video/mp4') } : null };
  let nexted = false;
  r = await files.onRequest({ request: new Request('https://site.pages.dev/video/big.mp4'), env: { FILES }, next: () => { nexted = true; return new Response('static'); } });
  assert.equal(await r.text(), 'BYTES'); assert.equal(r.headers.get('content-type'), 'video/mp4');
  r = await files.onRequest({ request: new Request('https://site.pages.dev/other.pdf'), env: { FILES }, next: () => new Response('static') });
  assert.equal(await r.text(), 'static');
});
