<script lang="ts">
  // 正文在线上数据库里的改动：最近一次试运行 / 写入的结果，每份文档一行，点开看技能脚本的原样输出。
  import type { ContentRun, OpsItem } from '$lib/types';
  import { ago } from './fmt';

  let { item }: { item: OpsItem } = $props();
  let open = $state(new Set<string>());
  const runs = $derived(item.content_runs ?? []);
  const stale = $derived(runs.some((r) => r.kind === 'check' && r.head && r.head !== item.head_sha));
  const key = (r: ContentRun) => `${r.kind}:${r.file}:${r.at}`;
  function toggle(k: string) {
    const s = new Set(open);
    if (s.has(k)) s.delete(k); else s.add(k);
    open = s;
  }
</script>

{#if runs.length}
  <div class="cr">
    {#if stale}<p class="cr-note">改动在试运行之后又变过，写入前会按最新的再试运行一遍。</p>{/if}
    {#each runs as r (key(r))}
      {@const k = key(r)}
      <div class="cr-row" class:bad={!r.ok}>
        <button class="cr-head" onclick={() => toggle(k)} aria-expanded={open.has(k)} data-tip={open.has(k) ? null : '看技能脚本的输出'}>
          <span class="cr-st">{r.ok ? '✓' : '✗'}</span>
          <span class="cr-kind">{r.kind === 'apply' ? '写入' : '试运行'}</span>
          <span class="mono cr-file">{r.file.replace(/^\.pilot\/content\//, '')}</span>
          <span class="cr-res">{r.ok ? (r.kind === 'apply' ? '已写入线上' : '通过') : '没通过'}</span>
          <span class="grow"></span>
          <span class="faint">{ago(r.at)}</span>
          <svg class="chev" class:on={open.has(k)} width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5 6 7.5 9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
        </button>
        {#if open.has(k)}<pre class="cr-out">{r.output || '（没有输出）'}</pre>{/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .cr { display: flex; flex-direction: column; gap: 2px; margin: 6px 0 8px; }
  .cr-note { margin: 0 0 4px; font-size: 12px; color: var(--dim); }
  .cr-row { border-radius: 8px; background: var(--rail, #f7f6f3); }
  .cr-head { width: 100%; display: flex; align-items: center; gap: 8px; padding: 5px 8px; border: none; background: none; font: inherit; font-size: 12px; color: var(--text); text-align: left; cursor: pointer; }
  .cr-st { width: 12px; color: var(--ok, #18a058); }
  .cr-row.bad .cr-st, .cr-row.bad .cr-res { color: var(--err); }
  .cr-kind { color: var(--dim); }
  .cr-file { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .cr-res { color: var(--dim); white-space: nowrap; }
  .grow { flex: 1; }
  .faint { color: var(--faint); white-space: nowrap; }
  .chev { flex: none; color: var(--faint); transition: transform .16s; }
  .chev.on { transform: rotate(180deg); }
  .cr-out { margin: 0; padding: 6px 10px 9px 28px; max-height: 260px; overflow: auto; font-size: 11.5px; line-height: 1.5; white-space: pre-wrap; word-break: break-word; color: var(--dim); }
</style>
