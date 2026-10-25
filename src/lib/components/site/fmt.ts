// 站点界面共用的数字 / 时间格式：中文习惯、同一处同一种写法。

const pad = (v: number) => String(v).padStart(2, '0');
const hm = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
const dayKey = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;

/** 绝对时间：今天 14:05 / 昨天 14:05 / 明天 14:05 / 10月22日 14:05 / 2025年10月22日。秒级时间戳。 */
export function clock(ts: number, withTime = true): string {
  if (!ts) return '—';
  const d = new Date(ts * 1000);
  const now = new Date();
  const off = (n: number) => { const x = new Date(now); x.setDate(x.getDate() + n); return dayKey(x); };
  const k = dayKey(d);
  const t = withTime ? ` ${hm(d)}` : '';
  if (k === off(0)) return withTime ? `今天 ${hm(d)}` : '今天';
  if (k === off(-1)) return `昨天${t}`;
  if (k === off(1)) return `明天${t}`;
  if (d.getFullYear() === now.getFullYear()) return `${d.getMonth() + 1}月${d.getDate()}日${t}`;
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`;
}

/** 相对时间（过去）：刚刚 / 5 分钟前 / 3 小时前 / 昨天 / 3 天前 / 2 周前 / 4 个月前 / 1 年前。具体时间放进悬停提示（full）。 */
export function ago(ts: number): string {
  if (!ts) return '—';
  const s = Date.now() / 1000 - ts;
  if (s < 60) return '刚刚';
  if (s < 3600) return `${Math.floor(s / 60)} 分钟前`;
  if (s < 86400) return `${Math.floor(s / 3600)} 小时前`;
  // 超过一天按日历天数算：昨天 23 点和今天 1 点隔的是「1 天」
  const t0 = new Date(); t0.setHours(0, 0, 0, 0);
  const d0 = new Date(ts * 1000); d0.setHours(0, 0, 0, 0);
  const n = Math.max(1, Math.round((t0.getTime() - d0.getTime()) / 864e5));
  if (n === 1) return '昨天';
  if (n < 7) return `${n} 天前`;
  if (n < 30) return `${Math.floor(n / 7)} 周前`;
  if (n < 365) return `${Math.floor(n / 30)} 个月前`;
  return `${Math.floor(n / 365)} 年前`;
}

/** 完整时间，给悬停提示用：2026年10月23日 19:07。 */
export function full(ts: number): string {
  if (!ts) return '';
  const d = new Date(ts * 1000);
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 ${hm(d)}`;
}

/** ISO 字符串版本的 clock。 */
export const clockIso = (iso: string) => (iso ? clock(Date.parse(iso) / 1000) : '—');

export const num = (v: number) => Math.round(v || 0).toLocaleString('en-US');
export const pct = (v: number) => `${(v * 100).toFixed(1)}%`;

/** 涨跌：{ text: '↑21%', dir: 1 }；没有可比数据时 dir=0、text=''。 */
export function delta(cur: number, prev: number): { text: string; dir: -1 | 0 | 1 } {
  if (!(prev > 0)) return { text: '', dir: 0 };
  const r = (cur - prev) / prev;
  const p = Math.round(Math.abs(r) * 100);
  if (p === 0) return { text: '持平', dir: 0 };
  return { text: `${r > 0 ? '↑' : '↓'}${p}%`, dir: r > 0 ? 1 : -1 };
}
