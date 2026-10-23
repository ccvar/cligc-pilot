import type { ApiProfile } from '$lib/apiProfiles';
import type { Brain } from '$lib/types';
import { app } from './app.svelte.js';
import { launcher } from './launcher.svelte.js';

export function apiProfile(brain: string): ApiProfile | undefined {
  return brain.startsWith('api:') ? app.apiProfiles.find((p) => p.id === brain.slice(4)) : undefined;
}
export function modelIcon(brain: string): string {
  const p = apiProfile(brain);
  return p ? (p.protocol === 'anthropic_messages' ? 'claude' : p.protocol === 'openai_responses' ? 'openai' : 'api') : brain;
}
/** 第一个可用（已装已登录）的厂商；全不可用时回退 claude（保持旧行为）。 */
export function firstUsableBrain(): Brain { return app.ALL_BRAINS.find((b) => brainUsable(b)) ?? 'claude'; }
const FAST_MODELS = ['claude-opus-5-5', 'claude-opus-5', 'claude-opus-4-8'];
export const fastable = (m: string) => FAST_MODELS.includes(m);
const permOpts = [
  { value: 'plan', label: '计划', sub: '只读 · 只给方案' },
  { value: 'ask', label: '询问', sub: '每步都要你批准' },
  { value: 'auto', label: '自动', sub: '仅危险动作确认' },
  { value: 'full', label: '全自动', sub: '全程不拦' },
];
// Codex + 远程连接（用户拍板放开）：能用，但闸是**打折**的 —— 走 AI 桥的远程命令照样逐条弹卡
// （闸在 bridge.rs，与厂商无关），可它无头下没有逐工具闸（claude 有 PreToolUse 钩子、grok 有 ACP
// 权限回调），拦不住它绕开桥、直接用系统 ssh + 用户自己的 ~/.ssh 密钥打同一台机器。
// 所以这里不置灰、只挂这句注记 —— 完整说法在权限档位的 tip 里（permTipFor）。
// ★ 这句话必须**自足**：它长在模型下拉里，别再写「见下方警告」——下面那个红框已经撤了，
//   指过去就是指向空气（这坑踩过一次）。
export const CODEX_SSH_NOTE = '拦不住它绕开 Pilot 的确认卡';
/** GitHub 仓库会话里用 Codex：它没有逐命令确认，推送 / 提 PR 等对外操作会直接执行。 */
export const CODEX_GITHUB_NOTE = 'Codex 不支持逐条确认：在这个仓库里推送分支、提 PR 等操作不会弹窗，会直接执行。要每一步都先问你，请改用 Claude 或 Grok。';
export function isCodexSsh(b: string, ssh: boolean): boolean { return ssh && b === 'codex'; }

// 权限档位按风险给下拉文字上色：自动=警告色、全自动(全程不拦)=危险色，计划/询问保持中性。
export function permTone(v: string): string { return v === 'full' ? 'danger' : v === 'auto' ? 'warn' : ''; }
// Codex 没有逐命令确认（询问/自动的弹卡是 Claude 的 PreToolUse 钩子）——这两档对它无意义，
// 下拉里直接置灰，省掉那行说明。Codex 实际就两档能用：计划(只读) / 全自动(建站要联网，跑得动)。
// **远程连接例外**：那里的命令闸在 Pilot 里（AI 桥，见 bridge.rs），不依赖厂商的钩子能力，
// 所以 codex 一样能逐条确认远程命令 —— 这两档对它有意义，不置灰。
// 远程连接下每个档位的长说明（挂 data-tip，Dropdown 会把它交给全局 .ui-tip；\n 会保留）。
// ★ 这些话原来是 composer 下面的常驻框，现在长在档位本身上 —— 那才是**决策点**：
// 原来的写法全都 reactive 于 perm==='full'，等于用户已经把闸拨到底了才弹出来说「这样很危险」，
// 亡羊补牢。挂在选项上，用户在**拨之前**就读得到；挂在触发器上，选完悬停还能复查。
export function permTipFor(brain: string, perm: string, ssh = false): string {
  if (!ssh) return '';
  const codex = brain === 'codex';
  if (perm === 'full') {
    return codex
      ? 'Codex ＋「全自动」＝ 没有任何确认。\n「全自动」档下 Pilot 的确认卡本来就全关了，而 Codex 无头模式自己也没有逐命令闸——它在这台机器上删数据、改配置、重启都不会问你。\n它还能绕开 Pilot，直接用你本机的 ssh 和 ~/.ssh 密钥连上来。\n除非你非常清楚要这样，否则退回「自动」。'
      : '「全自动」档下，AI 在这台机器上跑命令不会问你——包括删数据、改配置、重启。\n除非你清楚要这样，否则用「自动」。';
  }
  if (perm === 'ask' || perm === 'auto') {
    return codex
      ? 'Codex 的确认卡是打折的。\n它经 Pilot 跑的每条远程命令仍会弹卡等你确认；但 Codex 无头模式没有逐命令闸，它可以绕开 Pilot、直接用你本机的 ssh 和 ~/.ssh 密钥连这台机器——那条路一张卡都不弹。\n要每条命令都真正过你的眼，用 Claude 或 Grok。'
      : '远程命令的闸在 Pilot 里（与厂商无关）：每条要在这台机器上跑的命令都会弹卡等你确认。';
  }
  // plan 档在 ssh 下：bridge.rs 的 gate() 对 Plan 一律 Err —— 连只读命令都不跑，别写「只读远端」。
  if (perm === 'plan') return '只出方案、不碰这台机器：计划档下 AI 一条远程命令都不会跑（连只读的也不跑）。';
  return '';
}
export function permOptsFor(brain: string, ssh = false) {
  if (ssh) {
    // 远程连接下「询问/自动」对三家**都有意义**：远程命令的闸在 Pilot 的 AI 桥里（bridge.rs），
    // 与厂商无关 —— 所以这里不像下面那样把 codex 的这两档灰掉。
    // 但对 codex 要说实话：它能绕开桥（见 permTipFor），所以副标题不能许「每条都确认」。
    const codex = brain === 'codex';
    return permOpts.map((o) => {
      const tip = permTipFor(brain, o.value, true);
      return o.value === 'auto' ? { ...o, tip, sub: codex ? '经 Pilot 的远程命令要确认' : '本地放行 · 每条远程命令都确认' }
      : o.value === 'ask' ? { ...o, tip, sub: codex ? '经 Pilot 的远程命令要确认' : o.sub }
      // ★ 全自动的副标题就是那句警告的**短版**（红字），长版在 tip 里。codex 这档一道闸都没有。
      : o.value === 'full' ? { ...o, tip, tone: 'danger', sub: codex ? 'Codex：一张确认卡都没有' : '连远程命令也不拦' }
      : { ...o, tip };
    });
  }
  if (brain !== 'codex') return permOpts;
  return permOpts.map((o) => (o.value === 'ask' || o.value === 'auto') ? { ...o, disabled: true, sub: 'Codex 不支持逐命令确认' } : o);
}
export function customsOf(b: string): string[] { return b.startsWith('api:') ? [] : (b === 'codex' ? launcher.prefs.customCodexIds : b === 'grok' ? launcher.prefs.customGrokIds : launcher.prefs.customClaudeIds) ?? []; }
// 会话大小/用量：上下文按「厂商 + 模型」估算——Codex 暂以缓存默认窗口 272k 估算，
// 并非模型容量或当前会话确认值（Astra 缓存另有 max_context_window=872000）；Grok 4.5 / 4.6 500k
//（ACP initialize 的 totalContextTokens 实测值）。Claude 当前 Sonnet / Opus / Fable 是 1M；
// Haiku 4.5 仍是 200k。旧版或无法识别的自定义 Claude ID 保守按 200k。
//
// ★ 加新模型时**必须同时改这里**，否则它会掉进最后那个 200k 兜底 —— 用量百分比、
//   「快满了」提示、自动压缩时机全部按错误的窗口算，而且不报错、只是数字悄悄不对。
//   `-5` 这一支（sonnet-5 / opus-5 / fable-5）没有第三段版本号，别套 `-4-[6-9]` 那种写法。
function ctxLimit(brain: string, model = ''): number {
  if (brain.startsWith('api:')) return 0; // Capability is not inferable from a custom model ID.
  if (brain === 'codex') return 272000;
  if (brain === 'grok') return 500000;
  const id = (model.trim() || 'sonnet').toLowerCase();
  if (id === 'sonnet' || id === 'opus' || id.includes('fable') || id.includes('mythos')) return 1_000_000;
  if (/claude-(?:sonnet-5|opus-5|sonnet-4-[6-9]|opus-4-[6-9])(?:$|-)/.test(id)) return 1_000_000;
  return 200_000;
}
// 实测自适应升档（证据驱动，只升不降）：实测 ctx 已超过假定上限的 95%，说明真实窗口更大
//（如 Max 订阅跑 Sonnet 实际 1M），按下一档 1M 计；展示百分比由调用方钳 100%。
export function ctxLimitAdaptive(brain: string, model: string, ctx: number): number {
  const base = ctxLimit(brain, model);
  // Codex/Grok 的占用接近默认值，不足以证明窗口已扩展到 1M。
  if (brain !== 'claude' || model.toLowerCase().includes('haiku')) return base;
  return base < 1_000_000 && ctx > base * 0.95 ? 1_000_000 : base;
}
export function brainUsable(b: Brain): boolean {
  if (b.startsWith('api:')) { const p = apiProfile(b); return !!p?.enabled && p.key_configured && p.models.length > 0; }
  const s = b === 'claude' ? app.brains?.claude : b === 'grok' ? app.brains?.grok : app.brains?.codex;
  return !!s && s.found && s.logged_in !== false;
}
export function brainAvailability(b: string): string {
  if (b.startsWith('api:')) {
    const p = apiProfile(b);
    return !app.apiProfilesLoaded ? '正在读取 API 连接' : !p ? 'API 连接已移除' : !p.enabled ? 'API 连接已停用' : !p.key_configured ? '未配置 API Key' : !p.models.length ? '尚未添加 API 模型' : `API · ${p.name}`;
  }
  const status = b === 'claude' ? app.brains?.claude : b === 'grok' ? app.brains?.grok : app.brains?.codex;
  return !status?.found ? '未安装' : status.logged_in === false ? '未登录' : '本地客户端';
}
export function brainSetupHint(b: Brain): string {
  return b.startsWith('api:') ? `${brainAvailability(b)}，请在设置的「API 服务」中检查配置。` : '所选厂商未就绪，点左下角设置里「去授权」。';
}
export function brainTitle(b?: string): string {
  return b === 'codex' ? 'Codex' : b === 'grok' ? 'Grok' : b === 'claude' ? 'Claude' : '模型';
}
export function brainLabel(b: string): string { return b.startsWith('api:') ? `${apiProfile(b)?.name || '已移除的连接'} · API` : b === 'codex' ? 'Codex' : b === 'grok' ? 'Grok' : 'Claude'; }

// 下拉选项
// Sonnet / Haiku 用别名（--model sonnet/haiku），永远指向该档「当前最新」，厂商发新版自动
// 跟随、无需更新客户端；sub 里的版本号只是提示，可能滞后。
// Fable 各版本与 Opus 两档例外，走全 ID（原因见下），出新版需回来改这里。
//
// ★ Opus 两档都**钉全 ID**，不用 opus 别名：
//   - 别名不会指到 Opus 5（那是它必须写全 ID 的原因）；
//   - 且别名此刻也不指 4.8 —— 让它自报模型返回的是 claude-opus-4-6。自报版本本来就不可靠，
//     不足以断定别名真指 4.6；但既然拿不准，就别用一个标着「4.8」的选项去赌别名解析到哪。
//   钉住 claude-opus-4-8 后，标签写的就是实际发出去的，用户点 4.8 一定得到 4.8。
//   代价与 Fable 相同：厂商出新版不会自动跟随，得回来改这里。
//
// ★ 测「某个 --model ID 存不存在」的前提：**CLI 凭据必须是有效的**。
//   凭据过期时，claude CLI 对**任何** --model 值都静默返回空 + exit 0 —— 连凭空编的
//   claude-definitely-not-real-9 也一样，此时「没报错」完全不等于「ID 有效」。
//   凭据有效后才有判别力：真 ID 返回内容，假 ID 明确回
//   「There's an issue with the selected model (…). It may not exist or you may not have access to it.」
//   claude-opus-5 与 claude-opus-4-8 都是这么验的（真回话 + 同轮假 ID 被拒）。
const CLAUDE_MODELS = [
  // ★ 必须用显式 ID，不能用别名 'sonnet'：别名由 CLI 解析，2.1.96 把它解析成
  //   claude-sonnet-4-6 —— 标签写着 Sonnet 5、实际跑的是 4.6（用户实测抓到）。
  { value: 'claude-sonnet-5', label: 'Sonnet', sub: '性价比 · Sonnet 5 · 1M 上下文' },
  { value: 'claude-opus-5-5', label: 'Opus 5.5', sub: '最新 Opus · 1M 上下文' },
  { value: 'claude-opus-5', label: 'Opus 5', sub: '主力 · Opus 5 · 1M 上下文' },
  { value: 'claude-opus-4-8', label: 'Opus 4.8', sub: '上一代 Opus · 1M 上下文' },
  { value: 'claude-fable-5-1', label: 'Fable 5.1', sub: '旗舰 · 最新 Fable · 1M 上下文' },
  { value: 'claude-fable-5', label: 'Fable 5', sub: '上一代 Fable · 1M 上下文' },
  { value: 'haiku', label: 'Haiku', sub: '最快 · 当前 Haiku 4.5 · 200k 上下文' },
];
// Codex 档位 = 具体模型 ID（-c model=）。首项「跟随 Codex 默认」= 不覆盖本地 codex 配置。
// 型号取自本机 codex 模型清单，会随厂商更新；要用别的新模型走下方「自定义模型 ID」。
const CODEX_MODELS = [
  { value: '', label: '跟随 Codex 默认', sub: '用本地 codex 配置' },
  // 菜单展示官方模型容量；不要把 Codex 缓存默认窗口当成模型上限。
  // https://developers.openai.com/api/docs/models/gpt-6-astra
  { value: 'gpt-6-astra', label: 'GPT-6 Astra', sub: '最新旗舰 · 1.05M 模型上下文' },
  { value: 'gpt-6-sol', label: 'GPT-6 Sol', sub: '编程与复杂任务 · 1.05M 模型上下文' },
  { value: 'gpt-6-luna', label: 'GPT-6 Luna', sub: '快速轻量 · 1.05M 模型上下文' },
  // GPT-5.6 三分档（2026-07 GA）：裸 "gpt-5.6" 在 ChatGPT 订阅通道会被拒，必须用分档 ID；
  // 且要求 codex CLI ≥0.144（旧版报「requires a newer version of Codex」，升级 CLI 即可）。
  { value: 'gpt-5.6-sol', label: 'GPT-5.6 Sol', sub: '最强 · 细节打磨' },
  { value: 'gpt-5.6-terra', label: 'GPT-5.6 Terra', sub: '日常主力' },
  { value: 'gpt-5.6-luna', label: 'GPT-5.6 Luna', sub: '最快最省' },
  { value: 'gpt-5.5', label: 'GPT-5.5', sub: '上一代' },
  { value: 'gpt-5.4', label: 'GPT-5.4', sub: '上一代' },
  { value: 'gpt-5.4-mini', label: 'GPT-5.4-Mini', sub: '上一代 · 最省' },
];
// Grok（订阅通道）使用官方模型 ID；首项「跟随默认」
// 不传 --model，厂商换默认时自动跟随。显式档位方便会话固定模型与回退上一代。
const GROK_MODELS = [
  { value: '', label: '跟随 Grok 默认', sub: '使用 CLI 当前默认模型' },
  { value: 'grok-4.7', label: 'Grok 4.7', sub: '最新 · 500k 上下文' },
  { value: 'grok-4.6', label: 'Grok 4.6', sub: '上一代 · 500k 上下文' },
  { value: 'grok-4.5', label: 'Grok 4.5', sub: '上一代 · 500k 上下文' },
];
function modelOptsFor(b: string) {
  if (b.startsWith('api:')) return (apiProfile(b)?.models ?? []).map((id) => ({ value: id, label: id, sub: brainAvailability(b) }));
  return b === 'codex' ? CODEX_MODELS : b === 'grok' ? GROK_MODELS : CLAUDE_MODELS;
}
// launcher / 会话里可选：预设档位 + 该厂商的全局自定义模型 ID（定时任务表单仍只用预设 + 自己的 modelCustom）。
export function launcherModelOpts(b: string) { return [...modelOptsFor(b), ...customsOf(b).map((id) => ({ value: id, label: id, sub: '自定义' }))]; }
export function defaultModelFor(b: string): string {
  if (b.startsWith('api:')) return apiProfile(b)?.default_model ?? '';
  return b === 'claude' ? 'claude-sonnet-5' : '';
}
export function isLauncherModel(b: string, m: string): boolean { return b.startsWith('api:') && !app.apiProfilesLoaded || launcherModelOpts(b).some((o) => o.value === m); }
