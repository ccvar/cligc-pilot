// 后端报错多是英文或技术话（HTTP 状态码、os error、git 的 stderr…），直接弹给用户看不懂。
// 这里统一翻译成「怎么回事 + 下一步怎么做」；认不出的照原样。原文另留一份，界面上可以放在悬停或小字里。

export interface Explained {
  /** 给用户看的话；认不出时就是原文 */
  text: string;
  /** 后端原文（去掉了 "Error: " 前缀） */
  raw: string;
  /** 是否翻译过（false = text 就是原文） */
  known: boolean;
}

/** 消息里提到的服务商，用来把话说具体 */
function providerOf(m: string): string {
  if (/cloudflare|pages\.dev|wrangler/i.test(m)) return 'Cloudflare';
  if (/github/i.test(m)) return 'GitHub';
  if (/google|gmail|search console|gsc/i.test(m)) return 'Google';
  if (/bing/i.test(m)) return 'Bing';
  if (/wordpress/i.test(m)) return 'WordPress.com';
  if (/模型服务|API HTTP|openai|anthropic|模型/i.test(m)) return '模型服务';
  return '';
}

/** 从 "Cloudflare 403: …" / "GitHub 返回 404" / "HTTP 502" / "（HTTP 429）" 里取状态码 */
function statusOf(m: string): number {
  const hit = m.match(/(?:HTTP\s*|返回\s*|Cloudflare\s*|GitHub\s*|Google\s*|Bing\s*|WordPress\.com\s*|status(?: code)?\s*)(\d{3})\b/i) ?? m.match(/\b(401|403|404|429|5\d\d)\b(?=\s*[:：(（]|\s*$)/);
  return hit ? Number(hit[1]) : 0;
}

const WHERE_TOKEN = '到 Pilot 的「连接」里';

function authText(p: string): string {
  if (p === 'Cloudflare') return `Cloudflare 令牌的权限不够或已失效，这一步做不了。到 Cloudflare 后台（我的个人资料 → API 令牌）给这个令牌加上对应权限，或者${WHERE_TOKEN}换一个权限够的令牌。`;
  if (p === 'GitHub') return `GitHub 不认这个令牌（没权限或已过期）。${WHERE_TOKEN}重新填一个有 repo 权限的令牌。`;
  if (p === 'Google') return `Google 的授权失效了。${WHERE_TOKEN}重新授权一次就好。`;
  if (p === 'Bing') return `Bing 不认这个 API 密钥。到 Bing Webmaster 后台重新生成一个，再${WHERE_TOKEN}换上。`;
  if (p === 'WordPress.com') return `WordPress.com 的授权失效了。${WHERE_TOKEN}重新授权一次就好。`;
  if (p === '模型服务') return `模型服务不认这个 API 密钥（填错、过期或没余额）。到「设置」里检查这个服务的密钥。`;
  return `对方不认这个凭据（没权限或已过期）。${WHERE_TOKEN}重新填一下。`;
}

/** 翻译规则：按顺序试，先匹配的生效 */
const RULES: { test: (m: string) => boolean; text: (m: string) => string }[] = [
  // 自己的 Chinese 提示已经说清楚了的，不动
  { test: (m) => /没有找到 wrangler|请先安装 Node\.js/.test(m), text: (m) => m },
  // wrangler / node 没装
  {
    test: (m) => /wrangler/i.test(m) && /(not found|找不到|No such file|ENOENT|command not found|不是内部或外部命令)/i.test(m),
    text: () => '还没装 wrangler（Cloudflare 的命令行工具）。先安装 Node.js（nodejs.org），再在终端执行 npm i -g wrangler，装好后重试。',
  },
  // git 身份验证
  {
    test: (m) => /git|push|pull|clone|fetch|remote:/i.test(m) && /(Authentication failed|could not read Username|Permission denied \(publickey\)|Invalid username or (token|password)|403 Forbidden|Repository not found|Could not resolve host)/i.test(m),
    text: (m) => /Could not resolve host/i.test(m)
      ? '连不上代码仓库所在的服务器，可能是网络或代理的问题。检查网络后再试一次。'
      : /Repository not found/i.test(m)
        ? '找不到这个代码仓库：可能已被删除、改名，或者这个令牌看不到它。检查仓库地址和 GitHub 连接的权限。'
        : `推送 / 拉取代码时身份验证没通过。${WHERE_TOKEN}检查 GitHub 的令牌（要有 repo 权限）或 SSH 密钥，再试一次。`,
  },
  // 钥匙串
  {
    test: (m) => /keychain|钥匙串|keyring|secret|凭据存储/i.test(m) && /(not found|找不到|没有这个|无法读取|初始化.*失败|entry)/i.test(m),
    text: (m) => /无法读取|初始化|授权/.test(m)
      ? '读不到系统钥匙串里的密钥。系统弹出钥匙串授权时请点「始终允许」，然后重试。'
      : `这个连接的密钥找不到了（可能换了电脑或清过钥匙串）。${WHERE_TOKEN}重新填一下。`,
  },
  // 网络
  {
    test: (m) => /error sending request|timed out|timeout|dns error|connection (refused|reset|closed)|ECONNREFUSED|ECONNRESET|ENOTFOUND|EAI_AGAIN|ETIMEDOUT|Network is unreachable|network error|failed to lookup address|连不上|请求 [\w.]+ 失败|无法连接|operation timed out/i.test(m),
    text: (m) => {
      const p = providerOf(m);
      if (/timed out|timeout|ETIMEDOUT|超时/i.test(m)) return `连${p ? ` ${p}` : '服务器'}等了太久没回应，可能是网络慢或对方暂时忙。稍等一下再试；一直这样就检查网络或代理。`;
      return `连不上${p ? ` ${p}` : '服务器'}，可能是网络或代理的问题。检查网络后再试一次。`;
    },
  },
  // HTTP 状态码（含「Cloudflare 403: …」「GitHub 返回 404」「（HTTP 429）」）
  {
    test: (m) => {
      const s = statusOf(m);
      return s === 401 || s === 403 || s === 404 || s === 429 || (s >= 500 && s <= 599);
    },
    text: (m) => {
      const s = statusOf(m), p = providerOf(m), who = p || '对方服务';
      if (s === 401 || s === 403) return authText(p);
      if (s === 404) return `${who}上找不到要操作的东西（可能已被删掉、改过名，或者填的名字不对）。核对一下再试。`;
      if (s === 429) return `${who}说请求太频繁了，暂时限流。等一两分钟再试。`;
      return `${who}那边暂时出了问题（${s}），不是你操作有误。稍后再试一次。`;
    },
  },
  // 没有状态码但有鉴权字眼
  {
    test: (m) => /Authentication error|Unauthorized|Forbidden|invalid[_ ]token|token (is )?(invalid|expired)|bad credentials|invalid api key|incorrect api key|令牌(无效|过期|不对)/i.test(m) && !/令牌缺|令牌不能为空/.test(m),
    text: (m) => authText(providerOf(m)),
  },
  // 文件不存在
  {
    test: (m) => /No such file or directory|os error 2\b|ENOENT|文件夹不存在|目录不存在/i.test(m),
    text: () => '找不到这个文件或文件夹，可能已被移走、改名或删掉。检查路径后再试。',
  },
  // 没权限
  {
    test: (m) => /Permission denied|os error 13\b|EACCES|EPERM|Operation not permitted|权限不足|没有权限/i.test(m),
    text: () => '没有权限读写这个位置。检查文件夹的权限（或者它是不是被别的程序锁住了），再试一次。',
  },
  // 磁盘满
  {
    test: (m) => /No space left|os error 28\b|ENOSPC|磁盘空间不足/i.test(m),
    text: () => '磁盘满了，写不进去。清出一些空间再试。',
  },
];

/** 把原文拆成「中文前缀：具体报错」，翻译只针对具体报错那部分，前缀保留（「读取仓库失败：…」） */
function splitPrefix(raw: string): [string, string] {
  const m = raw.match(/^([^：:\n]{1,24}(?:失败|出错|错误|不了|没通过|没成功))[：:]\s*([\s\S]+)$/);
  return m ? [m[1], m[2]] : ['', raw];
}

/** 翻译后端报错；认不出时 text 就是原文、known=false */
export function explainError(e: unknown): Explained {
  const raw = String(e instanceof Error ? e.message : e ?? '').replace(/^Error:\s*/, '').trim();
  if (!raw) return { text: '', raw: '', known: false };
  const [prefix, body] = splitPrefix(raw);
  const rule = RULES.find((r) => r.test(body)) ?? RULES.find((r) => r.test(raw));
  if (!rule) return { text: raw, raw, known: false };
  const explained = rule.text(body);
  if (explained === body || explained === raw) return { text: raw, raw, known: false };
  return { text: prefix ? `${prefix}：${explained}` : explained, raw, known: true };
}

/** 对话里这一轮失败的报错：点「重试」解决不了、要用户先做点什么的，给一句下一步；其它返回空串。
 *  只补一行提示，不替换原文（原文还在错误卡上，方便对照服务商的说明）。 */
const TURN_HINTS: { test: RegExp; text: string }[] = [
  { test: /prompt is too long|context(_length)?[ _-]?(window|length).*(exceed|too)|maximum context length|too many (input )?tokens|上下文超(限|长)/i,
    text: '对话太长，超出了模型一次能记住的范围。点「重试」会换成新会话接着做（带上之前的摘要）。' },
  { test: /credit balance is too low|insufficient[_ ](quota|balance|funds)|余额不足|账户余额|payment required|billing|\b402\b/i,
    text: '模型服务的余额不够了：到服务商后台充值后点「重试」，或者在下面换个模型。' },
  { test: /not logged in|please run \/login|claude login|未登录/i,
    text: '本机的客户端还没登录：到「设置」里登录后点「重试」，或者在下面换个模型。' },
  { test: /invalid[ _-]?(x-)?api[ _-]?key|incorrect api key|authentication[_ ]error|unauthorized|\b401\b|密钥(无效|错误)/i,
    text: '模型服务不认这个 API 密钥（填错或过期了）：到「设置」里检查这个服务的密钥，改好后点「重试」。' },
  { test: /model.{0,40}(not found|does not exist|not available|unsupported)|unknown model|模型不存在/i,
    text: '这个模型用不了（名字不对，或者账号没开通）：在下面换个模型再试。' },
  { test: /rate[ _-]?limit|too many requests|\b429\b|usage limit|quota (exceeded|reached)|limit reached|额度(用完|不足)/i,
    text: '请求太频繁或额度用完了：等一会儿再点「重试」，或者在下面换个模型。' },
  { test: /overloaded|\b529\b|\b50[0234]\b|internal server error|bad gateway|service unavailable/i,
    text: '服务商那边暂时出了问题：稍等片刻再点「重试」。' },
];
export function turnErrorHint(text: string): string {
  return TURN_HINTS.find((h) => h.test.test(text))?.text ?? '';
}

/** 原来的底层会话已经接不下去了（上下文超长、会话记录找不到 / 损坏）：重试要换新会话（带历史摘要），原地 resume 只会再报一样的错 */
export function needsFreshSession(text: string): boolean {
  return /prompt is too long|maximum context length|context(_length)?[ _-]?(window|length).*(exceed|too)|too many (input )?tokens|上下文超(限|长)|no conversation found|session.{0,30}(not found|corrupt|invalid|expired)|会话.{0,10}(找不到|损坏|不存在)|tool_use.{0,60}tool_result|unexpected `?tool_use_id/i.test(text);
}

/** 常见的报错翻译成「怎么回事 + 下一步怎么做」，附一行原文；认不出的照原样 */
export function friendly(e: unknown): string {
  const x = explainError(e);
  return x.known ? `${x.text}\n（原文：${x.raw}）` : x.raw;
}
