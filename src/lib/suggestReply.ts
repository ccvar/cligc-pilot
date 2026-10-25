// 输入框里的「建议回复」：AI 上一条说「解锁后回复我『好了』」这类话时，把那句话放进输入框的占位文字，
// 空着的时候按 Tab 就填进去（不自动发送）。只认 AI 明确让用户回的那句，认不出就不给。

const QUOTE = '[「『“"]([^」』”"\\n]{1,24})[」』”"]';
// 回复（我）/ 回我 / 说（一声）/ 告诉我 / 发 + 引号里的话；也认英文 reply "…"
const ASK = new RegExp(`(?:回复(?:我)?(?:一声|一句)?|回我(?:一声|一句)?|跟我说(?:一声|一句)?|告诉我|说一声|说一句|发(?:一句|一声)?|reply(?: with)?|say)\\s*${QUOTE}`, 'gi');

export function suggestReply(text: string | null | undefined): string {
  if (!text) return '';
  // 只看最后一段：前面列步骤时提到的「好了」不一定是现在要回的
  const tail = text.slice(-600);
  let last = '';
  for (const m of tail.matchAll(ASK)) last = m[1].trim();
  return last;
}
