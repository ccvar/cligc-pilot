// GET  /api/comments?path=/xxx/ ：这一页审核通过的评论（迁移来的旧评论在页面里静态渲染，不在这里）。
// POST /api/comments ：发评论（path、name、content，可选 email、url、parent_id）。默认待审核，在 Cligc Pilot 里通过；
//      COMMENTS_AUTO_APPROVE=1 时直接通过。
import { json, reply, readBody, ipHash, turnstileOk, tooMany, sameOrigin, clip } from '../_lib.js';

export async function onRequestGet({ request, env }) {
  const path = new URL(request.url).searchParams.get('path') || '';
  if (!path.startsWith('/')) return json({ ok: false, error: 'path' }, 400);
  const { results } = await env.DB.prepare(
    "SELECT id, parent_id, name, url, content, created_at FROM comments WHERE path = ? AND status = 'approved' ORDER BY created_at ASC LIMIT 500",
  ).bind(path.slice(0, 300)).all();
  return json({ ok: true, comments: results || [] }, 200, { 'cache-control': 'public, max-age=30' });
}

export async function onRequestPost({ request, env }) {
  if (!sameOrigin(request)) return reply(request, false, { error: 'forbidden' }, 403);
  let body;
  try { body = await readBody(request); } catch { return reply(request, false, { error: 'bad_request' }); }
  if (body._gotcha) return reply(request, true, { pending: true });
  const path = clip(body.path, 300);
  const name = clip(body.name, 60);
  const content = clip(body.content, 5000);
  if (!path.startsWith('/') || !name || content.length < 2) return reply(request, false, { error: 'invalid' });
  const ip = await ipHash(request, env);
  if (await tooMany(env, 'comments', ip, 3, 10)) return reply(request, false, { error: 'rate_limited' }, 429);
  if (!(await turnstileOk(env, body, request))) return reply(request, false, { error: 'captcha' });
  const url = clip(body.url, 200);
  const parent = Number.parseInt(body.parent_id, 10);
  const status = env.COMMENTS_AUTO_APPROVE === '1' ? 'approved' : 'pending';
  await env.DB.prepare('INSERT INTO comments (path, parent_id, name, email, url, content, status, ip, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)')
    .bind(path, Number.isFinite(parent) ? parent : null, name, clip(body.email, 120), /^https?:\/\//.test(url) ? url : '', content, status, ip, Date.now())
    .run();
  return reply(request, true, { pending: status === 'pending' });
}
