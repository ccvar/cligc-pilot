<script lang="ts">
  import { untrack } from 'svelte';
  // 「站点制作」：制作中的站点（在 Pilot 里从零做、还没配置上线的）都在这里，不进全部站点。
  // 配好部署或手动标记做完，就移到全部站点。
  import './site.css';
  import Icon from './Icon.svelte';
  import SiteIcon from './SiteIcon.svelte';
  import type { Site } from '$lib/types';
  import { ago, full } from './fmt';
  import { app } from '$lib/state/app.svelte.js';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { openSite } from '$lib/state/sites.svelte.js';
  import { openConv } from '$lib/state/conversations.js';
  import { makingSites, openSiteCreate, finishMaking, isMaking, STACKS } from '$lib/state/siteCreate.svelte.js';
  import { sites } from '$lib/state/sites.svelte.js';
  import { fleet, loadFleet } from '$lib/state/fleet.svelte.js';
  import { launch, checkDomain } from '$lib/state/launch.svelte.js';
  import { siteMake, loadMake, makingConv, continueMaking, previewSite, removeMade, discardMade } from '$lib/state/siteMake.svelte.js';

  // 卡片右上角「…」菜单
  let menu = $state('');
  $effect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => { if (!(e.target as HTMLElement).closest('.more')) menu = ''; };
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') menu = ''; };
    document.addEventListener('mousedown', close, true); document.addEventListener('keydown', esc);
    return () => { document.removeEventListener('mousedown', close, true); document.removeEventListener('keydown', esc); };
  });
  const running = (id: string) => app.convos.some((c) => c.conn_id === `site:${id}` && c.status === 'running');

  const list = $derived(makingSites().sort((a, b) => b.updated_at - a.updated_at));
  // 在这里做出来、已经配好上线（或手动标记做完）的站点
  // 上线记录在站点总表里（没加载过就加载一次）
  if (!fleet.rows.length) void loadFleet();
  // 制作完成的站点「上线」到什么程度：配了部署 ≠ 上线了；Pages 站点还要看域名打开是不是这一版
  const pagesMode = (s: Site) => !['none', 'workers', 'vps', ''].includes(s.deploy.mode);
  function liveState(s: Site): { text: string; tone: string; icon: string } {
    if (!s.deploy.mode || s.deploy.mode === 'none') return { text: '还没配置上线', tone: '', icon: 'check' };
    const row = fleet.rows.find((r) => r.id === s.id);
    if (!row?.last_deploy_ts) return { text: '还没上线', tone: '', icon: 'rocket' };
    if (row.last_deploy_ok === false) return { text: '上次上线失败', tone: 'err', icon: 'rocket' };
    if (!s.domain) return { text: '已上传，还没绑域名', tone: '', icon: 'rocket' };
    if (!pagesMode(s)) return { text: '已上线', tone: 'ok', icon: 'rocket' };
    const dc = launch.domainCheck[s.id];
    if (dc?.compared) return dc.same ? { text: '已上线', tone: 'ok', icon: 'rocket' } : { text: '域名待生效', tone: 'warn', icon: 'rocket' };
    return { text: launch.checkingDomain[s.id] ? '检查域名…' : '已上传', tone: '', icon: 'rocket' };
  }
  const done = $derived(sites.list.filter((s) => (s.origin === 'created' || s.origin === 'made') && !isMaking(s.id)).sort((a, b) => b.updated_at - a.updated_at));
  $effect(() => {
    const ids = done.filter((s) => pagesMode(s) && s.domain && fleet.rows.find((r) => r.id === s.id)?.last_deploy_ok).map((s) => s.id);
    untrack(() => { for (const id of ids) { const s = done.find((x) => x.id === id); if (s && !launch.domainCheck[id]) void checkDomain(s, true); } });
  });
  $effect(() => { for (const s of list) void loadMake(s.id); });

  function status(s: Site): { text: string; tone: '' | 'live' | 'warn' | 'ok' } {
    if (app.convos.some((c) => c.conn_id === `site:${s.id}` && c.status === 'running')) return { text: 'AI 正在制作…', tone: 'live' };
    const ms = siteMake.state[s.id];
    if (!ms) return { text: '读取中…', tone: '' };
    const left = ms.steps.filter((x) => !x.done);
    if (left.length && left.every((x) => x.key === 'build')) return { text: '只差构建一次', tone: '' };
    if (left.length) return { text: `进行到：${left[0].label}`, tone: '' };
    const fails = ms.checks.filter((c) => c.level === 'fail').length;
    if (!ms.built_at) return { text: '等待构建并检查', tone: '' };
    if (fails) return { text: `上线前检查有 ${fails} 项要修`, tone: 'warn' };
    const goal = ms.todos?.filter((t) => t.affects_goal).length ?? 0;
    if (goal) return { text: `还有 ${goal} 项要你处理（影响站点目标）`, tone: 'warn' };
    if (ms.todos?.length) return { text: `可以上线了 · 还有 ${ms.todos.length} 项建议`, tone: 'ok' };
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
            {@const pending = !!ms && ms.steps.some((x) => !x.done) && !running(s.id)}
            <div class="card item">
              <div class="top-row">
                <button class="top" onclick={() => openSite(s.id, 'overview')}>
                  <SiteIcon id={s.id} size={18} />
                  <span class="nm"><b>{s.name}</b><small>{s.domain || '还没填域名'} · {stackName(s.framework)}</small></span>
                </button>
                <div class="more">
                  <button class="more-btn" class:on={menu === s.id} aria-label="更多操作" aria-haspopup="menu" aria-expanded={menu === s.id} onclick={() => (menu = menu === s.id ? '' : s.id)}>
                    <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><circle cx="3.5" cy="8" r="1.3" /><circle cx="8" cy="8" r="1.3" /><circle cx="12.5" cy="8" r="1.3" /></svg>
                  </button>
                  {#if menu === s.id}
                    <div class="menu" role="menu">
                      {#if ms?.built_at}<button role="menuitem" onclick={() => { menu = ''; void previewSite(s); }}><Icon name="external" size={13} />本地预览</button>{/if}
                      {#if pending}<button role="menuitem" onclick={() => { menu = ''; void continueMaking(s); }}><Icon name="play" size={13} />继续制作</button>{/if}
                      <button role="menuitem" onclick={() => { menu = ''; void finishMaking(s); }}><Icon name="check" size={13} />标记做完，移到全部站点</button>
                      <div class="sep"></div>
                      <button role="menuitem" onclick={() => { menu = ''; void removeMade(s); }}><Icon name="x" size={13} />从 Pilot 移除（保留文件夹）</button>
                      <button role="menuitem" class="danger" onclick={() => { menu = ''; void discardMade(s); }}><Icon name="trash" size={13} />删除站点和文件夹…</button>
                    </div>
                  {/if}
                </div>
              </div>
              <div class="prog">
                <span class="meter"><i style="width:{ms ? (done / Math.max(ms.steps.length, 1)) * 100 : 0}%"></i></span>
                <span class="n">{ms ? `${done}/${ms.steps.length}` : ''}</span>
              </div>
              <div class="st {st.tone}">{#if st.tone === 'live'}<span class="spin"></span>{/if}{st.text}</div>
              <div class="acts">
                {#if pending}<button class="btn tiny primary" onclick={() => continueMaking(s)}><Icon name="play" size={11} />继续制作</button>{/if}
                <button class="btn tiny" onclick={() => openSite(s.id, 'overview')}><Icon name="checks" size={11} />查看进度</button>
                {#if conv}<button class="btn tiny" onclick={() => openConv(conv.id)}><Icon name="chat" size={11} />制作对话</button>{/if}
                {#if ms?.built_at}<button class="btn tiny" onclick={() => previewSite(s)}><Icon name="external" size={11} />预览</button>{/if}
              </div>
              <small class="when" data-tip={full(s.updated_at)}>更新于 {ago(s.updated_at)}</small>
            </div>
          {/each}
        </div>
      {/if}

      {#if done.length}
        <div class="done-head"><b>制作完成</b><small>{done.length} 个 · 已经移到「全部站点」</small></div>
        <ul class="done">
          {#each done as s (s.id)}
            <li>
              <button class="row-btn" onclick={() => openSite(s.id, 'overview')}>
                <SiteIcon id={s.id} size={16} />
                <span class="nm"><b>{s.name}</b><small>{s.domain || '还没填域名'} · {stackName(s.framework)}</small></span>
                <span class="state {liveState(s).tone}"><Icon name={liveState(s).icon} size={11} />{liveState(s).text}</span>
                <span class="when" data-tip={full(s.updated_at)}>{ago(s.updated_at)}</span>
                <svg class="chev" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</div>

<style>
  .lead { margin: 0 0 2px; font-size: 12.5px; line-height: 1.6; max-width: 720px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 12px; }
  .item { display: flex; flex-direction: column; gap: 10px; padding: 14px 16px; }
  .top-row { display: flex; align-items: flex-start; gap: 8px; }
  .top-row .top { flex: 1; }
  .more { position: relative; flex: none; }
  .more-btn { width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 7px; color: var(--faint); cursor: pointer; transition: background-color .12s, color .12s; }
  .more-btn:hover, .more-btn.on { background: #f1efe9; color: var(--text); }
  .menu { position: absolute; right: 0; top: calc(100% + 4px); z-index: 30; min-width: 200px; padding: 5px; background: #fff; border: 1px solid var(--border); border-radius: 11px; box-shadow: 0 12px 32px rgba(30, 25, 15, .16); animation: mkl-pop .12s ease-out backwards; }
  @keyframes mkl-pop { from { opacity: 0; transform: translateY(-3px); } }
  .menu button { width: 100%; display: flex; align-items: center; gap: 8px; padding: 6px 10px; border: none; background: none; border-radius: 7px; font: inherit; font-size: 12.5px; color: var(--text); text-align: left; cursor: pointer; white-space: nowrap; }
  .menu button:hover { background: #f1efe9; }
  .menu button :global(svg) { color: var(--faint); flex: none; }
  .menu button.danger { color: var(--err); }
  .menu button.danger :global(svg) { color: var(--err); }
  .menu button.danger:hover { background: var(--err-soft, #fde8e8); }
  .sep { height: 1px; background: var(--border); margin: 4px 2px; }
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
  .acts { flex-wrap: wrap; }
  .when { font-size: 11px; color: var(--faint); }
  .done-head { display: flex; align-items: baseline; gap: 10px; margin: 18px 0 2px; }
  .done-head b { font-size: 13.5px; font-weight: 600; }
  .done-head small { font-size: 12px; color: var(--faint); }
  .done { list-style: none; margin: 0; padding: 0; border: 1px solid var(--border); border-radius: 12px; background: var(--card, #fff); overflow: hidden; }
  .done > li + li { border-top: 1px solid var(--border); }
  .row-btn { width: 100%; display: flex; align-items: center; gap: 10px; padding: 10px 14px; border: none; background: none; font: inherit; color: var(--text); text-align: left; cursor: pointer; transition: background-color .12s; }
  .row-btn:hover { background: #faf9f6; }
  .row-btn .nm { flex: 1; }
  .row-btn .nm b { font-size: 13px; }
  .state { flex: none; display: inline-flex; align-items: center; gap: 4px; font-size: 12px; color: var(--faint); }
  .state.ok { color: var(--ok); }
  .state.warn { color: var(--su-warn-ink, #8a5a08); }
  .state.err { color: var(--err); }
  .row-btn .when { flex: none; width: 76px; text-align: right; font-size: 11.5px; color: var(--faint); }
  .row-btn .chev { flex: none; color: var(--faint); }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 36px 16px; text-align: center; }
  .empty > :global(svg) { color: var(--accent, #b4541a); margin-bottom: 4px; }
  .empty b { font-size: 14px; font-weight: 600; }
  .empty small { font-size: 12px; color: var(--faint); margin-bottom: 8px; }
  @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
</style>
