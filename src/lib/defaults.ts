import type { Brain } from './types';

export interface Prefs {
  brain: Brain;
  model: string;
  /** 全局自定义模型 ID 列表（按厂商分开）；作为该厂商模型下拉里的附加档位。在「连接与模型」里增删。 */
  customClaudeIds: string[];
  customCodexIds: string[];
  customGrokIds: string[];
  /** 权限档位默认值：plan | ask | auto | full。本地对话默认 full；Cloudflare / SSH 会话会降到 auto。 */
  perm: string;
  /** 思考等级（推理强度）：'' 默认 | low | medium | high */
  effort?: string;
  /** fast 模式（仅 Opus 5 / Opus 4.8）。 */
  fast?: boolean;
}

const KEY = 'cligc.pilot.prefs';

export const DEFAULT_PREFS: Prefs = { brain: 'claude', model: 'sonnet', customClaudeIds: [], customCodexIds: [], customGrokIds: [], perm: 'full', effort: '' };

export function loadPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const p = { ...DEFAULT_PREFS, ...JSON.parse(raw) } as Prefs;
      // 始终用全新数组（避免共享 DEFAULT_PREFS 的引用被后续 push/splice 污染）。
      p.customClaudeIds = Array.isArray(p.customClaudeIds) ? [...p.customClaudeIds] : [];
      p.customCodexIds = Array.isArray(p.customCodexIds) ? [...p.customCodexIds] : [];
      p.customGrokIds = Array.isArray(p.customGrokIds) ? [...p.customGrokIds] : [];
      return p;
    }
  } catch {
    /* ignore */
  }
  return { ...DEFAULT_PREFS, customClaudeIds: [], customCodexIds: [], customGrokIds: [] };
}

export function savePrefs(p: Prefs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {
    /* ignore */
  }
}
