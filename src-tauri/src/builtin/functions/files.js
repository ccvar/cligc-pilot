// 超过 25 MB 的文件放在 R2（绑定 FILES），这里按原路径回应；不在 R2 里的交回静态文件。
// 只有 _routes.json 里列出的路径才会进到这里，其它请求不经过 Functions。
export async function onRequest({ request, env, next }) {
  if (!env.FILES || !['GET', 'HEAD'].includes(request.method)) return next();
  const key = decodeURIComponent(new URL(request.url).pathname).replace(/^\/+/, '');
  const obj = await env.FILES.get(key, { range: request.headers, onlyIf: request.headers });
  if (!obj) return next();
  const h = new Headers();
  obj.writeHttpMetadata(h);
  h.set('etag', obj.httpEtag);
  h.set('accept-ranges', 'bytes');
  h.set('cache-control', 'public, max-age=86400');
  if (!('body' in obj)) return new Response(null, { status: 304, headers: h });
  if (obj.range) {
    const { offset = 0, length } = obj.range;
    h.set('content-range', `bytes ${offset}-${offset + length - 1}/${obj.size}`);
    return new Response(request.method === 'HEAD' ? null : obj.body, { status: 206, headers: h });
  }
  return new Response(request.method === 'HEAD' ? null : obj.body, { headers: h });
}
