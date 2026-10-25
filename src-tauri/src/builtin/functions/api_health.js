// GET /api/health：Cligc Pilot 核对和跟踪时用来确认动态功能在线、数据库连得上。
import { json } from '../_lib.js';

export async function onRequestGet({ env }) {
  let db = false;
  try { db = !!(env.DB && (await env.DB.prepare('SELECT 1 AS ok').first())); } catch { db = false; }
  return json({ ok: db, db, files: !!env.FILES, turnstile: !!env.TURNSTILE_SECRET_KEY }, db ? 200 : 503);
}
