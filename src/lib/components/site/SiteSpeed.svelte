<script lang="ts">
  // 站点概览里的「页面速度」：每周用 PageSpeed Insights 测首页和搜索流量最多的几个页面（手机版）。
  // 每个页面一行：性能分、LCP；点开看主要问题。慢的页面会变成「页面太慢」机会。
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { say } from '$lib/state/app.svelte.js';
  import { ago, full } from './fmt';
  import type { Site, SpeedReport, PageSpeed } from '$lib/types';

  let { site }: { site: Site } = $props();
  let rep = $state<SpeedReport | null>(null);
  let busy = $state(false);
  let open = $state('');

  $effect(() => { const id = site.id; open = ''; invoke<SpeedReport>('site_speed', { id }).then((r) => (rep = r)).catch(() => (rep = null)); });
  async function check() {
    if (busy) return;
    busy = true;
    try {
      rep = await invoke<SpeedReport>('check_site_speed', { id: site.id });
      const slow = rep.pages.filter(isSlow).length;
      const err = rep.pages.find((p) => p.error)?.error;
      say(err && rep.pages.every((p) => p.error) ? err : slow ? `测了 ${rep.pages.length} 个页面，${slow} 个偏慢` : `测了 ${rep.pages.length} 个页面`, err && rep.pages.every((p) => p.error) ? 'err' : undefined);
    } catch (e) { say(String(e), 'err'); }
    finally { busy = false; }
  }
  const isSlow = (p: PageSpeed) => !p.error && ((p.score > 0 && p.score < 50) || p.field === 'SLOW');
  const tone = (s: number) => (s >= 90 ? 'ok' : s >= 50 ? 'warn' : 'err');
  const path = (u: string) => { try { return new URL(u).pathname; } catch { return u; } };
  const sec = (ms: number) => `${(ms / 1000).toFixed(1)} 秒`;
  const FIELD: Record<string, string> = { FAST: '快', AVERAGE: '一般', SLOW: '慢' };
  const avg = $derived.by(() => { const ok = (rep?.pages ?? []).filter((p) => !p.error && p.score > 0); return ok.length ? Math.round(ok.reduce((a, p) => a + p.score, 0) / ok.length) : 0; });
</script>

<div class="card spd">
  <h3><Icon name="gauge" />页面速度
    <span class="sub">{#if !rep?.checked_at}每周用 PageSpeed 测一次（手机版）{:else}{#if avg}平均 <b class={tone(avg)}>{avg}</b> 分 · {/if}<span data-tip={full(rep.checked_at)}>{ago(rep.checked_at)}测</span>{/if}</span>
    <span class="grow"></span>
    <button class="btn tiny" onclick={check} disabled={busy} data-tip="用 Google PageSpeed Insights 测首页和搜索流量最多的几个页面，一个页面要半分钟左右">{#if busy}<span class="spin"></span>测速中…{:else}<Icon name="refresh" size={11} />立即测速{/if}</button>
  </h3>
  {#if !rep?.checked_at}
    <p class="hint">测首页和 Search Console 里点击最多的 3 个页面：性能分、最大内容渲染（LCP）和 PageSpeed 列出的主要问题。低于 50 分的会变成「页面太慢」机会交给 AI 优化。</p>
  {:else}
    <ul class="rows">
      {#each rep.pages as p (p.url)}
        <li>
          {#if p.error}
            <span class="pill">没测出来</span>
            <button class="pth mono" onclick={() => openUrl(p.url)} data-tip={p.url}>{path(p.url)}</button>
            <p class="note">{p.error}</p>
          {:else}
            <span class="score {tone(p.score)}" data-tip="PageSpeed 性能分（手机版，满分 100）">{p.score}</span>
            <button class="pth mono" onclick={() => openUrl(`https://pagespeed.web.dev/analysis?url=${encodeURIComponent(p.url)}`)} data-tip="在 PageSpeed Insights 里看完整报告">{path(p.url)}</button>
            <span class="grow"></span>
            <span class="m faint" data-tip="最大内容渲染（LCP）：页面主要内容显示出来用的时间，2.5 秒内算好">LCP {sec(p.lcp_ms)}</span>
            {#if p.field}<span class="pill {p.field === 'FAST' ? 'ok' : p.field === 'SLOW' ? 'err' : 'warn'}" data-tip={`真实用户（近 28 天）：LCP ${sec(p.field_lcp_ms)}、交互响应 ${Math.round(p.field_inp_ms)} 毫秒`}>真实用户{FIELD[p.field] ?? p.field}</span>{/if}
            {#if p.issues.length}<button class="link-btn" onclick={() => (open = open === p.url ? '' : p.url)}>{open === p.url ? '收起' : `${p.issues.length} 个问题`}</button>{/if}
            {#if open === p.url}
              <ul class="iss">{#each p.issues as i (i.title)}<li>{i.title}<span class="faint">约省 {sec(i.savings_ms)}</span></li>{/each}</ul>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .spd .sub { font-size: 12px; color: var(--faint); font-weight: 400; }
  .rows { list-style: none; margin: 0 -18px -16px; padding: 0; }
  .rows > li { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 10px; padding: 9px 18px; border-top: 1px solid var(--border); font-size: 12.5px; }
  .score { flex: none; width: 30px; height: 22px; border-radius: 6px; display: inline-flex; align-items: center; justify-content: center; font-size: 12px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .score.ok, b.ok { color: var(--ok); }
  .score.warn, b.warn { color: var(--warn); }
  .score.err, b.err { color: var(--err); }
  .score.ok { background: color-mix(in srgb, var(--ok) 12%, transparent); }
  .score.warn { background: color-mix(in srgb, var(--warn) 12%, transparent); }
  .score.err { background: color-mix(in srgb, var(--err) 12%, transparent); }
  .pth { border: 0; background: none; padding: 0; font-size: 12px; color: var(--text); cursor: pointer; max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pth:hover { text-decoration: underline; text-underline-offset: 3px; }
  .m { font-size: 11.5px; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .grow { flex: 1; }
  .link-btn { font-size: 12px; }
  .note { flex-basis: 100%; margin: 0; font-size: 11.5px; color: var(--dim); }
  .iss { flex-basis: 100%; list-style: none; margin: 2px 0 0; padding: 0 0 0 40px; display: flex; flex-direction: column; gap: 3px; font-size: 12px; color: var(--dim); }
  .iss li { display: flex; gap: 8px; justify-content: space-between; }
  .iss .faint { flex: none; font-size: 11.5px; }
</style>
