<script lang="ts">
  // 「为什么做这条」：动作类型 + 来源 + 目标查询词，一行小标签。
  import Icon from './Icon.svelte';
  import { CHANGE_LABEL, KIND_LABEL, SOURCE_LABEL } from '$lib/state/ops.svelte.js';

  let { change = '', source = '', kind = '', target = '' }: { change?: string; source?: string; kind?: string; target?: string } = $props();
  const SRC_ICON: Record<string, string> = { data: 'chart', gap: 'search', topic: 'bulb', positioning: 'target', health: 'shield', manual: 'user' };
  // 规则算出来的机会，来源写得更具体一点（「数据 · 点击率偏低」）
  const srcText = $derived(source === 'data' && kind && kind !== 'plan' && KIND_LABEL[kind] ? `数据 · ${KIND_LABEL[kind]}` : SOURCE_LABEL[source] ?? '');
</script>

<span class="why">
  {#if change}<span class="w act"><Icon name="sliders" size={10} />{CHANGE_LABEL[change] ?? change}</span>{/if}
  {#if srcText}<span class="w src {source}" data-tip="这条从哪来"><Icon name={SRC_ICON[source] ?? 'info'} size={10} />{srcText}</span>{/if}
  {#if target}<span class="w tgt" data-tip="目标查询词"><Icon name="search" size={10} />{target}</span>{/if}
</span>

<style>
  .why { display: inline-flex; flex-wrap: wrap; gap: 4px; vertical-align: middle; }
  .w { display: inline-flex; align-items: center; gap: 4px; height: 19px; padding: 0 7px; border-radius: 6px; font-size: 11px; color: var(--dim); background: var(--rail); box-shadow: inset 0 0 0 1px var(--border); white-space: nowrap; max-width: 260px; overflow: hidden; text-overflow: ellipsis; }
  .w :global(.ico) { color: var(--faint); flex: none; }
  .src.topic, .src.positioning { color: #6b4fb5; background: #f4f1fb; box-shadow: inset 0 0 0 1px #e6e0f5; }
  .src.topic :global(.ico), .src.positioning :global(.ico) { color: #8b72cc; }
  .src.health { color: var(--su-warn-ink); background: var(--su-warn-soft); box-shadow: none; }
  .src.gap { color: var(--su-run); background: var(--su-run-soft); box-shadow: none; }
</style>
