import { test } from 'node:test';
import assert from 'node:assert/strict';
import { suggestReply } from './suggestReply.ts';

test('picks the phrase the AI asks the user to reply with', () => {
  assert.equal(suggestReply('请你先解锁手机，解锁后回复「好了」，我会从打开 OKX 开始。'), '好了');
  assert.equal(suggestReply('到邮箱输入页后先别填，回复我「好了」。'), '好了');
  assert.equal(suggestReply('确认没问题就跟我说一声“继续”'), '继续');
  assert.equal(suggestReply('When done, reply "ok".'), 'ok');
});

test('takes the last ask, and nothing when there is no ask', () => {
  assert.equal(suggestReply('先回复「好了」……之后改好了再回我「继续」'), '继续');
  assert.equal(suggestReply('已经部署好了，链接是 https://x.com'), '');
  assert.equal(suggestReply('页面上写着「Create account」'), '');
  assert.equal(suggestReply(''), '');
});
