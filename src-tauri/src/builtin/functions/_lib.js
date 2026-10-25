// Pilot 生成的 Cloudflare Pages Functions 公共代码（表单、评论共用）。由 Cligc Pilot 维护，改动会在下次配置动态功能时被覆盖。
// 绑定：DB（D1）。可选环境变量：TURNSTILE_SECRET_KEY（有就校验 Turnstile）、IP_SALT、COMMENTS_AUTO_APPROVE=1。

export function json(data, status = 200, extra = {}) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store', ...extra },
  });
}

/** 表单或 JSON 都收；同名字段合成数组。 */
export async function readBody(request) {
  const ct = request.headers.get('content-type') || '';
  if (ct.includes('application/json')) {
    const v = await request.json();
    return v && typeof v === 'object' && !Array.isArray(v) ? v : {};
  }
  const f = await request.formData();
  const o = {};
  for (const [k, v] of f.entries()) {
    if (typeof v !== 'string') continue;
    o[k] = k in o ? [].concat(o[k], v) : v;
  }
  return o;
}

/** 访客 IP 只存哈希（限频用），不存原始 IP。 */
export async function ipHash(request, env) {
  const ip = request.headers.get('cf-connecting-ip') || '';
  const d = await crypto.subtle.digest('SHA-256', new TextEncoder().encode((env.IP_SALT || 'cligc-pilot') + ip));
  return [...new Uint8Array(d)].slice(0, 12).map((b) => b.toString(16).padStart(2, '0')).join('');
}

/** 配了 TURNSTILE_SECRET_KEY 就校验人机验证；没配只靠隐藏字段和限频。 */
export async function turnstileOk(env, body, request) {
  if (!env.TURNSTILE_SECRET_KEY) return true;
  const token = body['cf-turnstile-response'];
  if (!token || typeof token !== 'string') return false;
  const r = await fetch('https://challenges.cloudflare.com/turnstile/v0/siteverify', {
    method: 'POST',
    body: new URLSearchParams({ secret: env.TURNSTILE_SECRET_KEY, response: token, remoteip: request.headers.get('cf-connecting-ip') || '' }),
  });
  const j = await r.json().catch(() => ({}));
  return !!j.success;
}

export async function tooMany(env, table, ip, limit, minutes) {
  const r = await env.DB.prepare(`SELECT COUNT(*) AS n FROM ${table} WHERE ip = ? AND created_at > ?`).bind(ip, Date.now() - minutes * 60000).first();
  return ((r && r.n) || 0) >= limit;
}

/** 只收本站页面发来的请求（浏览器会带 Origin）。 */
export function sameOrigin(request) {
  const o = request.headers.get('origin');
  if (!o) return true;
  try { return new URL(o).host === new URL(request.url).host; } catch { return false; }
}

/** 普通表单提交（不是 fetch）：跳回原页面并带上结果，页面不用 JS 也能用。 */
export function reply(request, ok, data, status) {
  const accept = request.headers.get('accept') || '';
  const ct = request.headers.get('content-type') || '';
  if (accept.includes('text/html') && !ct.includes('application/json')) {
    const back = request.headers.get('referer') || new URL(request.url).origin + '/';
    const u = new URL(back);
    u.searchParams.set(ok ? 'sent' : 'error', ok ? '1' : (data && data.error) || 'failed');
    return Response.redirect(u.toString(), 303);
  }
  return json({ ok, ...data }, status || (ok ? 200 : 400));
}

export function clip(v, n) {
  return String(Array.isArray(v) ? v.join(', ') : v == null ? '' : v).trim().slice(0, n);
}

export function pagePath(body, request) {
  const raw = body._page || request.headers.get('referer') || '';
  try { return new URL(String(raw), new URL(request.url).origin).pathname.slice(0, 300); } catch { return ''; }
}
