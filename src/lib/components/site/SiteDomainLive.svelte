<script lang="ts">
  // 部署卡片里的「域名」一行：上线成功只说明传到了 pages.dev，域名打开是不是这一版要另外看。
  // 自动比一次域名和 pages.dev 的页面；不一致就是「待生效」，并查出 Cloudflare 里抢着回应这个域名的东西，给出修复按钮。
  import { untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import { launch, checkDomain, fixDomain } from '$lib/state/launch.svelte.js';

  let { site, deployedAt }: { site: Site; deployedAt: number } = $props();
  const dc = $derived(launch.domainCheck[site.id]);
  const checking = $derived(!!launch.checkingDomain[site.id]);
  const busy = $derived(launch.busy[site.id] === 'domain');
  const note = $derived(launch.notes[site.id]?.domain ?? '');
  // 打开站点、或者又上线了一次：重新比一次
  $effect(() => { void site.id; void deployedAt; untrack(() => void checkDomain(site, true)); });
  const pending = $derived(!!dc && dc.compared && !dc.same);
</script>

<dt>域名</dt>
<dd class="dl">
  {#if !dc}
    <span class="faint">{checking ? `正在看 ${site.domain} 打开是不是这一版…` : site.domain}</span>
  {:else if dc.compared && dc.same}
    <span class="pill ok">已生效</span> {dc.domain} 打开就是这次上线的版本
  {:else if pending}
    <span class="pill warn">待生效</span> {dc.domain} 打开还是旧内容
    <div class="faint">现在显示「{dc.live_title || '没有标题'}」，这次上线的是「{dc.pages_title || '没有标题'}」</div>
  {:else}
    <span class="faint">没能比对：{dc.notes[0] ?? '打不开页面'}</span>
  {/if}
</dd>
<!-- 原因和修复按钮占整行（部署卡片很窄时也不挤） -->
{#if dc}
  <div class="dfull">
    {#if pending}
      {#if checking && !dc.scanned}
        <div class="faint">正在 Cloudflare 里查原因…</div>
      {:else if dc.issues.length}
        <ul class="dissues">
          {#each dc.issues as is, i (i)}
            <li><span>{is.text}</span>{#if is.fix}<button class="btn tiny" onclick={() => fixDomain(site, is)} disabled={busy || checking}>{busy ? '处理中…' : is.fix}</button>{/if}</li>
          {/each}
        </ul>
      {:else if dc.scanned}
        <div class="faint">没查到别的项目占用这个域名。DNS 刚改过要等几分钟；也可能是浏览器缓存，换个无痕窗口打开看看。</div>
      {/if}
      {#if dc.scanned}{#each dc.notes as n, i (i)}<div class="faint small">{n}</div>{/each}{/if}
    {/if}
    {#if note}<div class="small" class:err={note.startsWith('失败')}>{note}</div>{/if}
    {#if pending || !dc.compared}<div><button class="link-btn small" onclick={() => checkDomain(site, true)} disabled={checking}>{checking ? '检查中…' : '重新检查'}</button></div>{/if}
  </div>
{/if}

<style>
  .dl { min-width: 0; }
  .small { font-size: 11.5px; }
  .err { color: var(--err); }
  .dfull { grid-column: 1 / -1; display: flex; flex-direction: column; gap: 4px; min-width: 0; margin-top: -2px; }
  .dfull:empty { display: none; }
  .dissues { list-style: none; margin: 2px 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .dissues li { display: flex; align-items: flex-start; gap: 10px; padding: 7px 10px; border-radius: 8px; background: #fdf8ec; border: 1px solid #f0e4c4; font-size: 12px; line-height: 1.55; color: #6b4a10; }
  .dissues li span { flex: 1; min-width: 0; }
  .dissues li .btn { flex: none; }
</style>
