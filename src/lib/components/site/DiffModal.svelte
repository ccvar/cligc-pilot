<script lang="ts">
  // 改动差异弹窗（ops.diff 有值时显示）：需要你页、最近上线等不在任务队列里的地方共用。
  import { escClose } from '$lib/escClose';
  import { ops } from '$lib/state/ops.svelte.js';
</script>

{#if ops.diff}
  <div class="su-mask" role="presentation" onclick={() => (ops.diff = null)}></div>
  <div class="su modal diff-modal" use:escClose={() => (ops.diff = null)} role="dialog" aria-modal="true" aria-label="改动差异">
    <header class="sheet-head"><b>{ops.diff.item.opp.title}</b><button class="x" aria-label="关闭" data-tip="关闭" onclick={() => (ops.diff = null)}>×</button></header>
    <div class="sheet-body"><pre class="diff">{#each ops.diff.text.split('\n') as l, n (n)}<span class={l.startsWith('+') && !l.startsWith('+++') ? 'add' : l.startsWith('-') && !l.startsWith('---') ? 'del' : l.startsWith('@@') ? 'hunk' : ''}>{l}{'\n'}</span>{/each}</pre></div>
  </div>
{/if}

<style>
  .diff-modal { width: min(900px, calc(100vw - 32px)); }
  pre.diff { margin: 0; font-size: 11.5px; line-height: 1.5; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
  pre.diff .add { background: #e8f5ec; color: #135c34; }
  pre.diff .del { background: #fdecec; color: #8f1d1d; }
  pre.diff .hunk { color: #2f5fb8; }
</style>
