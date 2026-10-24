<script lang="ts">
  // 「站点制作」：制作中的站点（在 Pilot 里从零做、还没配置上线的）都在这里，不进全部站点。
  // 配好部署或手动标记做完，就移到全部站点。
  import './site.css';
  import Icon from './Icon.svelte';
  import SiteIcon from './SiteIcon.svelte';
  import type { Site } from '$lib/types';
  import { ago } from './fmt';
  import { app } from '$lib/state/app.svelte.js';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { openSite } from '$lib/state/sites.svelte.js';
  import { openConv } from '$lib/state/conversations.js';
  import { makingSites, openSiteCreate, finishMaking, STACKS } from '$lib/state/siteCreate.svelte.js';
  import { siteMake, loadMake, makingConv } from '$lib/state/siteMake.svelte.js';

  const list = $derived(makingSites().sort((a, b) => b.updated_at - a.updated_at));
  $effect(() => { for (const s of list) void loadMake(s.id); });

  function status(s: Site): { text: string; tone: '' | 'live' | 'warn' | 'ok' } {
    if (app.convos.some((c) => c.conn_id === `site:${s.id}` && c.status === 'running')) return { text: 'AI 正在制作…', tone: 'live' };
    const ms = siteMake.state[s.id];
    if (!ms) return { text: '读取中…', tone: '' };
    const next = ms.steps.find((x) => !x.done);
    if (next) return { text: `进行到：${next.label}`, tone: '' };
    const fails = ms.checks.filter((c) => c.level === 'fail').length;
    if (!ms.built_at) return { text: '等待构建并检查', tone: '' };
    if (fails) return { text: `上线前检查有 ${fails} 项要修`, tone: 'warn' };
    return { text: '可以上线了', tone: 'ok' };
  }
  const stackName = (id: string) => STACKS.find((x) => x.value === id)?.label ?? id;
</script>

<div class="su page mkl">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="page-head" data-tauri-drag-region onmousedown={startDrag}>
    <div class="ttl">
      <h1>站点制作</h1>
      <p class="hint">{list.length ? `${list.length} 个制作中` : '从零做一个新站点'}</p>
    </div>
    <div class="row">
      <button class="btn small primary" onclick={() => openSiteCreate()}><Icon name="wand" size={13} />制作站点</button>
    </div>
  </header>

  <div class="page-body">
    <div class="pane">
      <p class="hint lead">说清楚要做什么站，AI 从空仓库做出第一版：规划栏目、写首批内容页、配好 SEO、按获奖级的视觉标准自检。做好并配置上线后，站点会移到「全部站点」。</p>
      {#if !list.length}
        <div class="card empty">
          <Icon name="sparkles" size={20} />
          <b>还没有制作中的站点</b>
          <small>填一段定位、选好技术栈，剩下的交给 AI。</small>
          <button class="btn small primary" onclick={() => openSiteCreate()}><Icon name="wand" size={13} />制作站点</button>
        </div>
      {:else}
        <div class="grid">
          {#each list as s (s.id)}
            {@const ms = siteMake.state[s.id]}
            {@const st = status(s)}
            {@const conv = makingConv(s.id)}
            {@const done = ms?.steps.filter((x) => x.done).length ?? 0}
            <div class="card item">
              <button class="top" onclick={() => openSite(s.id, 'overview')}>
                <SiteIcon id={s.id} size={18} />
                <span class="nm"><b>{s.name}</b><small>{s.domain || '还没填域名'} · {stackName(s.framework)}</small></span>
              </button>
              <div class="prog">
                <span class="meter"><i style="width:{ms ? (done / Math.max(ms.steps.length, 1)) * 100 : 0}%"></i></span>
                <span class="n">{ms ? `${done}/${ms.steps.length}` : ''}</span>
              </div>
              <div class="st {st.tone}">{#if st.tone === 'live'}<span class="spin"></span>{/if}{st.text}</div>
              <div class="acts">
                <button class="btn tiny" onclick={() => openSite(s.id, 'overview')}><Icon name="checks" size={11} />查看进度</button>
                {#if conv}<button class="btn tiny" onclick={() => openConv(conv.id)}><Icon name="chat" size={11} />制作对话</button>{/if}
                <span class="grow"></span>
                <button class="link-btn" onclick={() => finishMaking(s)} data-tip="不等配置部署，直接移到全部站点">标记做完</button>
              </div>
              <small class="when">更新于 {ago(s.updated_at)}</small>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .lead { margin: 0 0 2px; font-size: 12.5px; line-height: 1.6; max-width: 720px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 12px; }
  .item { display: flex; flex-direction: column; gap: 10px; padding: 14px 16px; }
  .top { display: flex; align-items: center; gap: 10px; border: none; background: none; padding: 0; font: inherit; color: var(--text); text-align: left; cursor: pointer; min-width: 0; }
  .nm { display: flex; flex-direction: column; min-width: 0; }
  .nm b { font-size: 13.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nm small { font-size: 11.5px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .top:hover .nm b { text-decoration: underline; text-underline-offset: 2px; }
  .prog { display: flex; align-items: center; gap: 8px; }
  .meter { flex: 1; height: 4px; border-radius: 99px; background: var(--border); overflow: hidden; }
  .meter i { display: block; height: 100%; background: var(--ok); border-radius: inherit; transition: width .3s; }
  .prog .n { font-size: 11px; color: var(--faint); min-width: 24px; text-align: right; }
  .st { display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--dim); }
  .st.live { color: var(--text); }
  .st.warn { color: #8a5a08; }
  .st.ok { color: var(--ok); }
  .spin { width: 10px; height: 10px; border: 1.5px solid var(--border2); border-top-color: var(--dim); border-radius: 50%; animation: mkl-spin .7s linear infinite; }
  @keyframes mkl-spin { to { transform: rotate(360deg); } }
  .acts { display: flex; align-items: center; gap: 6px; }
  .acts .link-btn { font-size: 12px; color: var(--faint); }
  .acts .link-btn:hover { color: var(--text); }
  .grow { flex: 1; }
  .when { font-size: 11px; color: var(--faint); }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 36px 16px; text-align: center; }
  .empty > :global(svg) { color: var(--accent, #b4541a); margin-bottom: 4px; }
  .empty b { font-size: 14px; font-weight: 600; }
  .empty small { font-size: 12px; color: var(--faint); margin-bottom: 8px; }
  @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
</style>
