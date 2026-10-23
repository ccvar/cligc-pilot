#!/usr/bin/env node
// site-data.js — Cligc Pilot 随附的站点数据查询工具（只读本地缓存，没有任何凭据）。
// 用法: node site-data.js <缓存目录> <命令> [选项]
// 命令: summary | queries | pages | query-pages | events | daily | opportunities | sitemaps
"use strict";
const fs = require("fs");
const path = require("path");

function out(v) { process.stdout.write(JSON.stringify(v, null, 1) + "\n"); }
function fail(msg) { out({ ok: false, error: msg }); process.exitCode = 1; }

function main() {
const argv = process.argv.slice(2);
const dir = argv[0];
const cmd = argv[1];
if (!dir || !cmd) {
  fail("用法: node site-data.js <缓存目录> <summary|queries|pages|query-pages|events|daily|opportunities|sitemaps> [--source gsc|bing|ga4] [--min-impressions N] [--max-ctr X] [--min-position N] [--max-position N] [--page 片段] [--query 片段] [--sort clicks|impressions|ctr|position|views|sessions] [--limit N] [--prev] [--days N]");
  return;
}
const opt = {};
for (let i = 2; i < argv.length; i++) {
  const a = argv[i];
  if (!a.startsWith("--")) continue;
  const k = a.slice(2);
  if (k === "prev") { opt.prev = true; continue; }
  opt[k] = argv[++i];
}
function load(name) {
  const p = path.join(dir, name);
  // 只读缓存目录里的文件，不接受路径穿越
  if (path.dirname(p) !== path.resolve(dir)) return null;
  try { return JSON.parse(fs.readFileSync(p, "utf8")); } catch { return null; }
}
const meta = load("meta.json") || {};
const num = (k, d) => (opt[k] === undefined ? d : Number(opt[k]));
const limit = Math.max(1, Math.min(500, num("limit", 50)));
const has = (s, sub) => !sub || String(s || "").toLowerCase().includes(String(sub).toLowerCase());
const round = (r) => {
  const o = { ...r };
  for (const k of ["ctr"]) if (typeof o[k] === "number") o[k] = Math.round(o[k] * 10000) / 10000;
  for (const k of ["position"]) if (typeof o[k] === "number") o[k] = Math.round(o[k] * 10) / 10;
  return o;
};
function filterRows(rows, keyField) {
  const minI = num("min-impressions", 0), maxCtr = num("max-ctr", Infinity);
  const minP = num("min-position", 0), maxP = num("max-position", Infinity);
  let v = (rows || []).filter((r) =>
    (r.impressions ?? Infinity) >= minI && (r.ctr ?? 0) <= maxCtr &&
    (r.position ?? 0) >= minP && (r.position ?? 0) <= maxP);
  if (opt.page) v = v.filter((r) => has(keyField === "page" ? r.key : r.key2, opt.page));
  if (opt.query) v = v.filter((r) => has(r.key, opt.query));
  const s = opt.sort || "clicks";
  const asc = s === "position";
  v.sort((a, b) => asc ? (a[s] ?? 0) - (b[s] ?? 0) : (b[s] ?? 0) - (a[s] ?? 0) || (b.impressions ?? 0) - (a.impressions ?? 0));
  return v.slice(0, limit).map(round);
}
const src = opt.source || "gsc";
const window = (m) => (m && m.fetched_at ? { start: m.start, end: m.end } : null);

switch (cmd) {
  case "summary": {
    const sum = (rows, f) => (rows || []).reduce((a, r) => a + (r[f] || 0), 0);
    const gq = load("gsc_queries.json"), gqp = load("gsc_queries_prev.json");
    const ga = load("ga4_pages.json"), gap = load("ga4_pages_prev.json");
    const bq = load("bing_queries.json");
    const opps = load("opportunities.json") || [];
    out({
      ok: true,
      windows: { gsc: window(meta.gsc), ga4: window(meta.ga4), bing: window(meta.bing) },
      errors: { gsc: meta.gsc?.error || "", ga4: meta.ga4?.error || "", bing: meta.bing?.error || "" },
      gsc: gq ? { clicks: sum(gq, "clicks"), impressions: sum(gq, "impressions"), prev_clicks: sum(gqp, "clicks"), prev_impressions: sum(gqp, "impressions"), queries: gq.length } : null,
      ga4: ga ? { views: sum(ga, "views"), sessions: sum(ga, "sessions"), prev_views: sum(gap, "views"), pages: ga.length } : null,
      bing: bq ? { clicks: sum(bq, "clicks"), impressions: sum(bq, "impressions"), queries: bq.length } : null,
      opportunities: opps.length,
      top_opportunities: opps.slice(0, 5).map((o) => ({ kind: o.kind, title: o.title, page: o.page })),
    });
    break;
  }
  case "queries": {
    const file = src === "bing" ? "bing_queries.json" : opt.prev ? "gsc_queries_prev.json" : "gsc_queries.json";
    const rows = load(file);
    if (!rows) { fail(`没有 ${src} 查询词数据`); break; }
    out({ ok: true, source: src, window: window(meta[src === "bing" ? "bing" : "gsc"]), prev: !!opt.prev, rows: filterRows(rows, "query") });
    break;
  }
  case "pages": {
    if (src === "ga4") {
      const rows = load(opt.prev ? "ga4_pages_prev.json" : "ga4_pages.json");
      if (!rows) { fail("没有 GA4 页面数据"); break; }
      let v = rows.filter((r) => has(r.key, opt.page));
      const s = opt.sort || "views";
      v.sort((a, b) => (b[s] ?? 0) - (a[s] ?? 0));
      out({ ok: true, source: "ga4", window: window(meta.ga4), rows: v.slice(0, limit) });
      break;
    }
    const file = src === "bing" ? "bing_pages.json" : opt.prev ? "gsc_pages_prev.json" : "gsc_pages.json";
    const rows = load(file);
    if (!rows) { fail(`没有 ${src} 页面数据`); break; }
    out({ ok: true, source: src, window: window(meta[src === "bing" ? "bing" : "gsc"]), rows: filterRows(rows, "page") });
    break;
  }
  case "query-pages": {
    const rows = load("gsc_query_page.json");
    if (!rows) { fail("没有查询词×页面数据"); break; }
    out({ ok: true, source: "gsc", window: window(meta.gsc), rows: filterRows(rows, "query").map((r) => ({ query: r.key, page: r.key2, clicks: r.clicks, impressions: r.impressions, ctr: r.ctr, position: r.position })) });
    break;
  }
  case "events": {
    const rows = load("ga4_events.json");
    if (!rows) { fail("没有 GA4 事件数据"); break; }
    out({ ok: true, source: "ga4", window: window(meta.ga4), rows: rows.slice(0, limit) });
    break;
  }
  case "daily": {
    const file = src === "ga4" ? "ga4_daily.json" : src === "bing" ? "bing_daily.json" : "gsc_daily.json";
    const rows = load(file);
    if (!rows) { fail(`没有 ${src} 按天数据`); break; }
    const days = Math.max(1, Math.min(120, num("days", 28)));
    out({ ok: true, source: src, rows: rows.slice(-days).map(round) });
    break;
  }
  case "opportunities": {
    const rows = load("opportunities.json") || [];
    out({ ok: true, rows: rows.filter((o) => !opt.kind || o.kind === opt.kind).slice(0, limit) });
    break;
  }
  case "sitemaps": {
    out({ ok: true, gsc: load("gsc_sitemaps.json"), bing_crawl_issues: (load("bing_crawl.json") || []).slice(0, limit) });
    break;
  }
  default:
    fail("未知命令：" + cmd);
}
}
main();
