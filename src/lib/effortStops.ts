// 推理强度的档位：不同执行器、不同模型支持的档位不一样，对话框的模型面板（ModelFx）和站点运营的模型菜单（ModelMenu）共用这一份。
import { apiReasoningLevels } from './apiReasoning';
import type { ApiProtocol } from './apiProfiles';

export type Stop = { v: string; l: string; d: string };
export const BASE_STOPS: Stop[] = [
  { v: '', l: '自动', d: '跟随模型默认' },
  { v: 'low', l: '低', d: '更快更省' },
  { v: 'medium', l: '中', d: '均衡' },
  { v: 'high', l: '高', d: '深度推理' },
];
/**
 * 不同执行器的原生 effort 档位并不相同，按当前模型动态展示：
 * Claude Code: low / medium / high / xhigh / max
 * Grok CLI: none / minimal / low / medium / high / xhigh / max
 * Codex 常规模型: low / medium / high
 * GPT-6 Astra / Sol（Codex 通道）: low / medium / high / xhigh / max / ultra
 * GPT-6 Luna（Codex 通道）: low / medium / high / xhigh / max
 */
export const CLAUDE_STOPS: Stop[] = [
  ...BASE_STOPS,
  { v: 'xhigh', l: '极高', d: '扩展推理' },
  { v: 'max', l: '最高', d: '最大推理 · 最慢' },
];
export const GROK_STOPS: Stop[] = [
  { v: '', l: '自动', d: '跟随模型默认' },
  { v: 'none', l: '无', d: '不推理' },
  { v: 'minimal', l: '极低', d: '最少推理' },
  { v: 'low', l: '低', d: '更快更省' },
  { v: 'medium', l: '中', d: '均衡' },
  { v: 'high', l: '高', d: '深度推理' },
  { v: 'xhigh', l: '极高', d: '扩展推理' },
  { v: 'max', l: '最高', d: '最大推理 · 最慢' },
];
export const CODEX_ASTRA_STOPS: Stop[] = [
  ...BASE_STOPS,
  { v: 'xhigh', l: '极高', d: '扩展推理' },
  { v: 'max', l: '最高', d: '最大推理' },
  { v: 'ultra', l: '极致', d: '自动委派 · 最深推理' },
];
// Codex subscription-channel capabilities (models_cache), not Responses API tiers.
export const CODEX_LUNA_STOPS = CODEX_ASTRA_STOPS.filter((s) => s.v !== 'ultra');

/** API 模型的档位：按服务商协议和模型查（第一档是「默认」＝跟随服务商默认） */
export function apiStops(protocol: ApiProtocol | undefined, model: string): Stop[] {
  return [
    { v: '', l: '默认', d: '跟随服务商默认' },
    ...apiReasoningLevels(protocol, model).map((v) => v === 'max'
      ? { v, l: '最大', d: '最大推理' }
      : GROK_STOPS.find((stop) => stop.v === v) ?? { v, l: v, d: '' }),
  ];
}

/** "brain::model" 这个模型能选哪些档位 */
export function stopsFor(brain: string, model: string, protocol?: ApiProtocol): Stop[] {
  if (brain.startsWith('api:')) return apiStops(protocol, model);
  if (brain === 'claude') return CLAUDE_STOPS;
  if (brain === 'grok') return GROK_STOPS;
  if (brain === 'codex' && (model === 'gpt-6-astra' || model === 'gpt-6-sol')) return CODEX_ASTRA_STOPS;
  if (brain === 'codex' && model === 'gpt-6-luna') return CODEX_LUNA_STOPS;
  return BASE_STOPS;
}

/** 档位存的值：API 模型加 api: 前缀（和对话里一样，换回本地客户端时不会误用），其它原样 */
export function effortValue(brain: string, stop: string): string {
  return brain.startsWith('api:') && stop ? `api:${stop}` : stop;
}

/** 选中的档位名字（「高」「极高」…）；跟随默认返回空，按钮上就不写 */
export function effortLabel(value: string, effort: string, protocol?: ApiProtocol): string {
  if (!effort) return '';
  const [brain, ...m] = value.split('::');
  const v = brain.startsWith('api:') ? effort.replace(/^api:/, '') : effort;
  return stopsFor(brain, m.join('::'), protocol).find((s) => s.v === v && s.v)?.l ?? '';
}
