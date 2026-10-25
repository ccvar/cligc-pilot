// 输入框的建议回复（模型猜的那句）：任何一个对话这一轮跑完（running → 停），就请后端用同一家的小模型猜一句。
// AI 原话里明确让用户回的那句（见 suggestReply）优先，认出来就不调模型。
import { invoke } from '@tauri-apps/api/core';
import { untrack } from 'svelte';
import { app } from './app.svelte.js';
import { suggestReply } from '$lib/suggestReply';
import { leaves } from '$lib/conversationLayout';
import { panes } from './panes.svelte.js';
import type { Conversation } from '$lib/types';

type Suggestion = { ts: number; text: string };

class SuggestState {
  /** convId → 这个对话最后一条 AI 回复对应的建议 */
  byConv = $state<Record<string, Suggestion>>({});
}
export const suggest = new SuggestState();

async function fetchSuggestion(convId: string) {
  try {
    const s = await invoke<Suggestion>('suggest_reply', { convId });
    if (s.text) suggest.byConv = { ...suggest.byConv, [convId]: s };
  } catch { /* 猜不到就不显示 */ }
}

/** 这个对话现在该显示的建议：AI 原话里点名的那句 > 模型猜的（还对应最后一条回复才算数）。 */
export function suggestionFor(conv: Conversation | null | undefined, busy: boolean): string {
  if (!conv || busy) return '';
  const msgs = conv.messages.filter((m) => !m.hidden);
  const last = msgs[msgs.length - 1];
  if (!last || last.role !== 'assistant' || last.error) return '';
  const said = suggestReply(last.text);
  if (said) return said;
  const s = suggest.byConv[conv.id];
  return s && s.ts === last.ts ? s.text : '';
}

export function initSuggest() {
  let prev = new Set<string>();
  $effect(() => {
    const now = new Set(Object.keys(app.running));
    untrack(() => {
      // 只给屏幕上开着的对话猜（当前对话和分屏里的）：后台的自动任务跑完不花这一下
      const open = new Set([app.activeConvId, ...leaves(panes.conversationLayout).map((p) => p.conversation)]);
      for (const id of prev) {
        if (now.has(id) || !open.has(id)) continue;
        // 跑完了：原话里已经点名要回什么就不用再猜
        const conv = app.activeConv?.id === id ? app.activeConv : null;
        const last = conv?.messages.filter((m) => !m.hidden).at(-1);
        if (last && last.role === 'assistant' && suggestReply(last.text)) continue;
        void fetchSuggestion(id);
      }
      prev = now;
    });
  });
}
