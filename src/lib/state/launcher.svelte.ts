import { loadPrefs, savePrefs } from '$lib/defaults';
import type { Brain } from '$lib/types';
import { open } from '@tauri-apps/plugin-dialog';
import { type ModelOption, effortAfterModelPick } from '$lib/modelSelection';
import { app } from './app.svelte.js';
import { fastable, brainUsable, isCodexSsh, CODEX_SSH_NOTE, brainAvailability, launcherModelOpts, modelIcon, apiProfile, isLauncherModel, defaultModelFor, firstUsableBrain } from './models';

class LauncherState {
  // ---------- launcher form ----------
  prefs = $state(loadPrefs());
  lSite = $state('');
  lBrain = $state<Brain>(this.prefs.brain);
  lModel = $state(this.prefs.model);
  // 本地对话默认全自动；Cloudflare / SSH 在 selectConn 里把「全自动」降为「自动」。已有会话继续读取各自保存的 perm_mode。
  lPerm = $state<string>(this.prefs.perm ?? 'full');
  lEffort = $state<string>(this.prefs.effort ?? '');
  /** fast 模式。只有 Opus 5.5 / Opus 5 / Opus 4.8 支持 —— 换到别的模型必须自动落下，
   *  否则会留下一个「显示开着、对该模型其实无效」的状态，比关着更误导。 */
  lFast = $state<boolean>(this.prefs.fast ?? false);
  workspaceDir = $state('');
  lStyle = $state('');

  // 启动页「厂商+模型」合并为一个下拉：行带厂商图标，值编码为 "<brain>::<model>"。
  // 厂商未就绪时整组置灰（原厂商下拉的禁用语义保留）。
  comboOpts = $derived.by(() => {
    const out: ModelOption[] = [];
    for (const b of app.ALL_BRAINS) {
      const usable = brainUsable(b);
      const note = isCodexSsh(b, app.isSshConn) ? CODEX_SSH_NOTE : brainAvailability(b);
      for (const m of launcherModelOpts(b)) {
        out.push({ value: `${b}::${m.value}`, label: m.label, sub: note || m.sub, icon: modelIcon(b), disabled: !usable, apiProtocol: apiProfile(b)?.protocol });
      }
    }
    return out;
  });
  lDraft = $state('');
  // 首次识别出本地 CLI 后：若默认厂商不可用（只装/登录了另一个），把 composer 默认厂商切到可用的那个；之后尊重手动选择。
  brainAutoSet = false;
  lDraftEl = $state<HTMLTextAreaElement | undefined>();
}

export const launcher = new LauncherState();

export function setFast(v: boolean) { launcher.lFast = v; launcher.prefs.fast = v; savePrefs(launcher.prefs); }
export function workspaceFolderName(path: string): string {
  const parts = path.replace(/[\\/]+$/, '').split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}
export async function chooseWorkspaceFolder() {
  const picked = await open({ title: '选择自由对话使用的文件夹', directory: true, multiple: false });
  if (typeof picked === 'string' && picked.trim()) launcher.workspaceDir = picked;
}
// CF 建站「视觉风格」预设：每档是一组具体 design tokens 指令，拼进首条消息，
// 把"语言欠定视觉"变成"点一下锁定方向"。空 = 让 AI 按内容自行判断。
export const STYLE_OPTS = [
  { value: '', label: '风格：让 AI 定', sub: '按站点内容自行判断' },
  { value: 'minimal', label: '极简留白', sub: '黑白灰 · 大留白' },
  { value: 'editorial', label: '杂志编辑', sub: '衬线标题 · 窄栏长文' },
  { value: 'dark-tech', label: '深色科技', sub: '近黑底 · 霓虹点缀' },
  { value: 'warm-craft', label: '暖色手作', sub: '奶油底 · 陶土强调' },
  { value: 'saas', label: '企业 SaaS', sub: '浅底 · 卡片层次' },
];
export const STYLE_DIRECTIVES: Record<string, string> = {
  minimal: '极简留白：纯白底、近黑文字，仅一个低饱和强调色；无衬线字体；区块间大留白、细分割线；圆角 8px；无花哨动效。',
  editorial: '杂志编辑风：米白底；衬线大标题 + 无衬线正文；正文窄栏（约 720px）居中；标题字号大胆、段落节奏松；用细横线分节。',
  'dark-tech': '深色科技：近黑背景（如 #0b0e14）、高对比浅色文字；一个霓虹强调色（青或紫）；数据/代码用等宽字体点缀；小圆角或直角；hover 微发光。',
  'warm-craft': '暖色手作：奶油白/米色底、深棕文字、陶土或暖橙强调色；12px 圆润圆角；人文气质字体；阴影温和；整体温暖安静。',
  saas: '企业 SaaS：浅灰白底、品牌蓝或紫强调；卡片 + 轻阴影分层；间距体系化、信息密度适中；CTA 按钮醒目；配 FAQ/价格等标准分区样式。',
};
export function pickCombo(v: string) {
  const i = v.indexOf('::');
  if (i < 0) return;
  launcher.lEffort = effortAfterModelPick(`${launcher.lBrain}::${launcher.lModel}`, v, launcher.lEffort, launcher.comboOpts);
  launcher.lBrain = v.slice(0, i) as Brain;
  launcher.lModel = v.slice(i + 2);
  if (launcher.lBrain.startsWith('api:')) launcher.lFast = false;
  persistLauncherEffort(launcher.lEffort);
}
export function persistLauncherEffort(v: string) {
  launcher.lEffort = v;
  // Keep the model identity and its protocol-specific effort in one snapshot,
  // even before a first message is sent or the managed wizard is opened.
  launcher.prefs.brain = launcher.lBrain; launcher.prefs.model = launcher.lModel; launcher.prefs.effort = launcher.lEffort; launcher.prefs.fast = launcher.lFast;
  savePrefs(launcher.prefs);
}

export function initLauncher() {
  $effect(() => { if (launcher.lFast && !fastable(launcher.lModel)) { launcher.lFast = false; launcher.prefs.fast = false; savePrefs(launcher.prefs); } });
  // 选中的档位若在 Codex 下不可用，落到「全自动」（保持已选值始终合法，不显示一个灰着的当前项）。
  // 远程连接不做这个降级：见上，那里 codex 的询问/自动是真能用的（降级会把真机推进无人确认的全自动）。
  $effect(() => { if (launcher.lBrain === 'codex' && !app.isSshConn && (launcher.lPerm === 'ask' || launcher.lPerm === 'auto')) launcher.lPerm = 'full'; });
  // 切换引擎后，若当前档位不属于该引擎（launcher 含自定义），重置为该引擎默认档位（避免下拉空选/发错模型）。
  $effect(() => { if (!isLauncherModel(launcher.lBrain, launcher.lModel)) launcher.lModel = defaultModelFor(launcher.lBrain); });
  $effect(() => {
    if (!app.brains || !app.apiProfilesLoaded || launcher.brainAutoSet) return;
    launcher.brainAutoSet = true;
    if (!launcher.lBrain.startsWith('api:') && !brainUsable(launcher.lBrain)) launcher.lBrain = app.brains && app.ALL_BRAINS.some((b) => brainUsable(b)) ? firstUsableBrain() : launcher.lBrain;
  });
  $effect(() => { void launcher.lDraft; const el = launcher.lDraftEl as unknown as { __autogrow?: () => void } | undefined; el?.__autogrow?.(); });
}
