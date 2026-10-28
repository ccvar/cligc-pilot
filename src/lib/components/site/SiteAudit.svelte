<script lang="ts">
  // 站点概览里的「页面体检」：每周把构建产物过一遍——图片（缺替代文字、文件不存在、太大）和结构化数据（写坏了、文章页缺 Article、缺字段），
  // 再告诉你 llms.txt 是站点自己的还是上线时 Pilot 生成的。问题多的会变成机会交给 AI 修。
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { say } from '$lib/state/app.svelte.js';
  import { ago, full } from './fmt';
  import type { Site, AuditReport } from '$lib/types';

  let { site }: { site: Site } = $props();
  let rep = $state<AuditReport | null>(null);
  let busy = $state(false);
  let open = $state('');

  $effect(() => { const id = site.id; open = ''; invoke<AuditReport>('site_audit', { id }).then((r) => (rep = r)).catch(() => (rep = null)); });
  async function check() {
    if (busy) return;
    busy = true;
    try {
      rep = await invoke<AuditReport>('check_site_audit', { id: site.id });
      const n = rep.no_alt.length + rep.broken.length + rep.large.length + rep.schema.length;
      say(n ? `查了 ${rep.pages} 个页面，发现 ${n} 个问题` : `查了 ${rep.pages} 个页面，没发现问题`);
    } catch (e) { say(String(e), 'err'); }
    finally { busy = false; }
  }
  const kb = (b: number) => (b >= 1024 * 1024 ? `${(b / 1024 / 1024).toFixed(1)} MB` : `${Math.round(b / 1024)} KB`);
  const groups = $derived(rep?.checked_at ? [
    { k: 'broken', label: '图片不存在', tone: 'err', items: rep.broken.map((i) => ({ a: i.src, b: i.page })) },
    { k: 'large', label: '图片太大', tone: 'warn', items: rep.large.map((i) => ({ a: `${i.src}（${kb(i.bytes)}）`, b: i.page })) },
    { k: 'no_alt', label: '图片缺替代文字', tone: 'warn', items: rep.no_alt.map((i) => ({ a: i.src || '（没有 src）', b: i.page })) },
    { k: 'schema', label: '结构化数据问题', tone: 'warn', items: rep.schema.map((s) => ({ a: s.problem, b: s.page })) },
  ].filter((g) => g.items.length) : []);
  const more = (n: number) => (n >= 200 ? '200+' : String(n));
</script>

<div class="card aud">
  <h3><Icon name="checks" />页面体检
    <span class="sub">{#if !rep?.checked_at}每周检查一次构建产物{:else if !rep.pages}还没有构建产物 · {ago(rep.checked_at)}看过{:else}{rep.pages} 个页面 · {rep.images} 张图片 · <span data-tip={full(rep.checked_at)}>{ago(rep.checked_at)}检查</span>{/if}</span>
    <span class="grow"></span>
    <button class="btn tiny" onclick={check} disabled={busy} data-tip="把构建产物里的页面过一遍，只读本地文件">{#if busy}<span class="spin"></span>检查中…{:else}<Icon name="refresh" size={11} />立即检查{/if}</button>
  </h3>
  {#if !rep?.checked_at || !rep.pages}
    <p class="hint">查图片（缺替代文字、文件不存在、超过 300 KB）和结构化数据（写坏了、文章页缺 Article、缺标题 / 日期 / 作者 / 图片）。这些页面照样能打开，但搜索引擎和 AI 搜索更难读懂。</p>
  {:else}
    <ul class="rows">
      {#each groups as g (g.k)}
        <li>
          <span class="pill {g.tone}">{more(g.items.length)}</span>
          <span class="lb">{g.label}</span>
          <span class="grow"></span>
          <button class="link-btn" onclick={() => (open = open === g.k ? '' : g.k)}>{open === g.k ? '收起' : '看清单'}</button>
          {#if open === g.k}
            <ul class="ilist">{#each g.items.slice(0, 12) as it, n (n)}<li><span class="mono a" title={it.a}>{it.a}</span><span class="faint b" title={it.b}>{it.b}</span></li>{/each}
              {#if g.items.length > 12}<li class="faint">…还有 {g.items.length - 12} 条</li>{/if}</ul>
          {/if}
        </li>
      {:else}
        <li class="okl"><Icon name="check" size={12} />图片和结构化数据都没发现问题{#if rep.article_pages} · {rep.article_pages} 个文章页{/if}</li>
      {/each}
      <li class="llms" data-tip={rep.llms === 'own' ? '站点自己有 llms.txt，Pilot 不动它' : '站点没有 llms.txt：上线时 Pilot 按页面标题和描述生成一份放进构建产物，给 AI 搜索看的站点目录'}>
        <span class="faint">llms.txt</span><span>{rep.llms === 'own' ? '站点自己的' : '上线时由 Pilot 生成'}</span>
      </li>
    </ul>
  {/if}
</div>

<style>
  .aud .sub { font-size: 12px; color: var(--faint); font-weight: 400; }
  .rows { list-style: none; margin: 0 -18px -16px; padding: 0; }
  .rows > li { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 10px; padding: 9px 18px; border-top: 1px solid var(--border); font-size: 12.5px; }
  .rows > li .pill { min-width: 22px; justify-content: center; font-variant-numeric: tabular-nums; }
  .grow { flex: 1; }
  .link-btn { font-size: 12px; }
  .okl { color: var(--ok); gap: 6px; }
  .llms { font-size: 12px; gap: 8px; }
  .ilist { flex-basis: 100%; list-style: none; margin: 2px 0 0; padding: 0; display: flex; flex-direction: column; gap: 3px; }
  .ilist li { display: flex; gap: 10px; font-size: 11.5px; min-width: 0; }
  .ilist .a { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11.5px; }
  .ilist .b { flex: none; max-width: 45%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
</style>
