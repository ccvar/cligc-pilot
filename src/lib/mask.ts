// 列表里的敏感信息脱敏显示：截图、录屏、共享屏幕时不露出完整的邮箱 / IP / 设备序列号。
// 完整内容放进 data-tip，鼠标移上去看（`maskTip`：脱敏了才给提示，没变就不给）。

const EMAIL = /([A-Za-z0-9._%+-]+)@([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,})/g;
const IPV4 = /\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b/g;

/** 邮箱留前两位（短的留一位）和域名；IPv4 留首尾两段。其余原样。 */
export function mask(s: string | null | undefined): string {
  if (!s) return '';
  return s
    .replace(EMAIL, (_, user: string, domain: string) => `${user.length > 3 ? user.slice(0, 2) : user.slice(0, 1)}***@${domain}`)
    .replace(IPV4, (_, a: string, _b: string, _c: string, d: string) => `${a}.*.*.${d}`);
}

/** 设备序列号之类的长 id：留头 4 位和尾 3 位。 */
export function maskId(s: string | null | undefined): string {
  if (!s) return '';
  return s.length > 9 ? `${s.slice(0, 4)}…${s.slice(-3)}` : s;
}

/** 脱敏后和原文不一样时，原文当悬停提示；一样就不给提示。 */
export function maskTip(full: string | null | undefined, shown: string): string | undefined {
  return full && full !== shown ? full : undefined;
}
