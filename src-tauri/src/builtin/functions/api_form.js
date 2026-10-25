// POST /api/form：收表单（留言、联系、订阅……）。字段随意，以下划线开头的是控制字段：
// _form 表单名（默认 contact）、_page 来源页面、_gotcha 隐藏的防机器人字段（有值就当机器人，假装成功）。
// 存进 D1 的 submissions 表，在 Cligc Pilot 的站点页「表单和评论」里看。
import { reply, readBody, ipHash, turnstileOk, tooMany, sameOrigin, clip, pagePath } from '../_lib.js';

export async function onRequestPost({ request, env }) {
  if (!sameOrigin(request)) return reply(request, false, { error: 'forbidden' }, 403);
  let body;
  try { body = await readBody(request); } catch { return reply(request, false, { error: 'bad_request' }); }
  if (body._gotcha) return reply(request, true, {});
  const ip = await ipHash(request, env);
  if (await tooMany(env, 'submissions', ip, 5, 10)) return reply(request, false, { error: 'rate_limited' }, 429);
  if (!(await turnstileOk(env, body, request))) return reply(request, false, { error: 'captcha' });
  const fields = {};
  for (const [k, v] of Object.entries(body)) {
    if (k.startsWith('_') || k === 'cf-turnstile-response') continue;
    const val = clip(v, 5000);
    if (val) fields[k.slice(0, 60)] = val;
  }
  if (!Object.keys(fields).length) return reply(request, false, { error: 'empty' });
  if (Object.keys(fields).length > 30) return reply(request, false, { error: 'too_many_fields' });
  await env.DB.prepare('INSERT INTO submissions (form, page, fields, ip, ua, created_at) VALUES (?, ?, ?, ?, ?, ?)')
    .bind(clip(body._form || 'contact', 40), pagePath(body, request), JSON.stringify(fields), ip, clip(request.headers.get('user-agent'), 200), Date.now())
    .run();
  return reply(request, true, {});
}
