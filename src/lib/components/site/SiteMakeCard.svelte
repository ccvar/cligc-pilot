<script lang="ts">
  // 制作站点的「第一版制作」卡：进度清单（看文件判断）+ 上线前检查（扫构建产物）+ 让 AI 修问题。
  // 只在 Pilot 里新建、还没配部署的站点上出现；配好部署后交给下面的上线向导。
  import './site.css';
  import Icon from './Icon.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import type { Site } from '$lib/types';
  import { ago } from './fmt';
  import { app } from '$lib/state/app.svelte.js';
  import { sites, checkSite } from '$lib/state/sites.svelte.js';
  import { openConv } from '$lib/state/conversations.js';
  import { siteMake, loadMake, makingConv, fixWithAi, continueMaking, previewSite, askTodos } from '$lib/state/siteMake.svelte.js';
  import SiteTodos from './SiteTodos.svelte';

  let { site }: { site: Site } = $props();
  const ms = $derived(siteMake.state[site.id]);
  const loading = $derived(!!siteMake.loading[site.id]);
  const job = $derived(sites.jobs[site.id]);
  const building = $derived(job?.status === 'running' && job.kind === 'check');
  const conv = $derived(makingConv(site.id));
  const aiRunning = $derived(conv?.status === 'running' || app.convos.some((c) => c.conn_id === `site:${site.id}` && c.status === 'running'));
  const doneN = $derived(ms?.steps.filter((s) => s.done).length ?? 0);
  // 「需要你处理的事」那一项要你补信息，AI 修不了，不算进「让 AI 修复」
  const bad = $derived(ms?.checks.filter((c) => c.level !== 'ok' && c.key !== 'todo') ?? []);
  const fails = $derived(bad.filter((c) => c.level === 'fail').length);
  const pending = $derived(ms?.steps.filter((s) => !s.done).map((s) => s.label) ?? []);
  const goalTodos = $derived(ms?.todos?.filter((t) => t.affects_goal).length ?? 0);
  const ready = $derived(!!ms && ms.built_at > 0 && fails === 0);
  let openItems = $state<Set<string>>(new Set());
  function toggle(k: string) { const n = new Set(openItems); if (n.has(k)) n.delete(k); else n.add(k); openItems = n; }

  $effect(() => { void loadMake(site.id); });
  // 构建预检跑完、AI 这一轮做完：重新看一遍文件
  let lastJob = '';
  $effect(() => { const k = job ? `${job.id}:${job.status}` : ''; if (k !== lastJob) { const was = lastJob; lastJob = k; if (was && job && job.status !== 'running') void loadMake(site.id); } });
  let wasRunning = false;
  $effect(() => { const r = aiRunning; if (wasRunning && !r) void loadMake(site.id); wasRunning = r; });
</script>

<div class="card mk">
  <h3>
    <Icon name="hammer" />第一版制作
    {#if ms}<span class="prog"><span class="meter"><i style="width:{(doneN / Math.max(ms.steps.length, 1)) * 100}%"></i></span>{doneN}/{ms.steps.length}</span>{/if}
    <span class="grow"></span>
    {#if aiRunning}<span class="live"><span class="spin"></span>AI 正在制作…</span>{/if}
    {#if ms?.built_at}<button class="btn tiny" onclick={() => previewSite(site)} data-tip="在预览窗里打开构建产物（和上线的是同一份文件）"><Icon name="external" size={12} />本地预览</button>{/if}
    {#if conv}<button class="btn tiny" onclick={() => openConv(conv.id)}><Icon name="chat" size={12} />制作对话</button>{/if}
    <button class="ic-btn" onclick={() => loadMake(site.id)} disabled={loading} aria-label="重新检查" data-tip="重新看一遍站点文件"><RefreshIcon spinning={loading} /></button>
  </h3>

  {#if ms}
    {#if !aiRunning && pending.length}
      <div class="resume">
        <span>第一版还没做完，还差：{pending.join('、')}。AI 停下了（可能是中断、出错或额度用完）。</span>
        <button class="btn small primary" onclick={() => continueMaking(site)}><Icon name="play" size={12} />继续制作</button>
      </div>
    {/if}
    <ol class="mk-steps">
      {#each ms.steps as s (s.key)}
        <li class:done={s.done}><span class="tick">{#if s.done}<Icon name="check" size={11} />{/if}</span><b>{s.label}</b>{#if s.detail}<small>{s.detail}</small>{/if}</li>
      {/each}
    </ol>

    {#if ms.todos?.length}
      <div class="sec"><b>需要你处理</b><small>{ms.todos.length} 项{goalTodos ? ` · ${goalTodos} 项影响站点目标` : ''}</small></div>
      <SiteTodos {site} todos={ms.todos} />
    {:else if ms.todos === null && !pending.length && !aiRunning}
      <div class="sec ask"><b>需要你处理</b><small>AI 还没把要你补的信息、要你做的决定整理到这里（可能只写在对话里）。</small><span class="grow"></span><button class="btn tiny" onclick={() => askTodos(site)}><Icon name="wand" size={11} />让 AI 整理</button></div>
    {/if}

    <div class="sec">
      <b>上线前检查</b>
      <small>{ms.built_at ? `构建于 ${ago(ms.built_at)}` : '还没有构建产物'}</small>
      <span class="grow"></span>
      <button class="btn tiny" onclick={() => checkSite(site.id)} disabled={building || job?.status === 'running'}><Icon name="play" size={11} />{building ? '构建中…' : ms.built_at ? '重新构建并检查' : '构建并检查'}</button>
    </div>
    <ul class="mk-checks">
      {#each ms.checks as c (c.key)}
        <li class={c.level}>
          <span class="lv">{c.level === 'ok' ? '✓' : c.level === 'fail' ? '✕' : '!'}</span>
          <div class="cb">
            <span><b>{c.label}</b><small>{c.detail}</small>{#if c.items.length}<button class="link-btn" onclick={() => toggle(c.key)}>{openItems.has(c.key) ? '收起' : '查看'}</button>{/if}</span>
            {#if openItems.has(c.key)}<ul class="items">{#each c.items.slice(0, 30) as it (it)}<li class="mono">{it}</li>{/each}</ul>{/if}
          </div>
        </li>
      {/each}
    </ul>
    <div class="foot">
      {#if ready}
        {#if goalTodos}
          <p class="hint warn-text">没有必须修的问题，但还有 {goalTodos} 项会影响站点目标（见上面「需要你处理」），最好先处理再上线。</p>
        {:else}
          <p class="ok-note">{bad.length ? `没有必须修的问题（还有 ${bad.length} 项建议）。` : '检查都通过了。'}可以上线了：在下面的上线向导里配置部署和域名。</p>
        {/if}
      {:else if ms.built_at && fails}
        <p class="hint">有 {fails} 项必须修的问题，修好之前不建议上线。</p>
      {/if}
      {#if bad.length && ms.built_at}<button class="btn small primary" onclick={() => fixWithAi(site, ms.checks)} disabled={aiRunning}><Icon name="wand" size={12} />让 AI 修复这 {bad.length} 项</button>{/if}
    </div>
  {:else}
    <div class="bars"><i style="width:58%"></i><i style="width:34%"></i></div>
  {/if}
</div>

<style>
  .mk h3 .prog { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; font-weight: 500; color: var(--faint); }
  .meter { width: 64px; height: 4px; border-radius: 99px; background: var(--border); overflow: hidden; display: inline-block; }
  .meter i { display: block; height: 100%; background: var(--ok); border-radius: inherit; transition: width .3s; }
  .live { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 500; color: var(--dim); }
  .spin { width: 10px; height: 10px; border: 1.5px solid var(--border2); border-top-color: var(--dim); border-radius: 50%; animation: mk-spin .7s linear infinite; }
  @keyframes mk-spin { to { transform: rotate(360deg); } }
  .ic-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border: none; background: none; border-radius: 6px; color: var(--faint); cursor: pointer; }
  .ic-btn:hover { background: #f1efe9; color: var(--text); }
  .resume { display: flex; align-items: center; gap: 12px; padding: 10px 12px; margin-bottom: 14px; border-radius: 10px; background: #faf6ec; border: 1px solid #f0e4c4; font-size: 12.5px; color: #6b4e12; }
  .resume span { flex: 1; min-width: 0; line-height: 1.5; }
  .resume .btn { flex: none; }
  .mk-steps { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 7px; }
  .mk-steps li { display: flex; align-items: center; gap: 9px; font-size: 13px; color: var(--dim); }
  .mk-steps li b { font-weight: 500; }
  .mk-steps li.done { color: var(--text); }
  .mk-steps li small { font-size: 11.5px; color: var(--faint); }
  .tick { flex: none; width: 16px; height: 16px; border-radius: 50%; border: 1.5px solid var(--border2); box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; color: #fff; }
  .mk-steps li.done .tick { background: var(--ok); border-color: var(--ok); }
  .sec { display: flex; align-items: center; gap: 8px; margin: 16px 0 8px; padding-top: 14px; border-top: 1px solid var(--border); }
  .sec b { font-size: 13px; font-weight: 600; }
  .sec small { font-size: 11.5px; color: var(--faint); }
  .sec.ask small { flex: 1; min-width: 0; }
  .mk-checks { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .mk-checks > li { display: flex; align-items: flex-start; gap: 9px; font-size: 12.5px; }
  .lv { flex: none; width: 16px; height: 16px; margin-top: 1px; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; font-size: 10px; font-weight: 700; }
  .ok .lv { background: var(--su-ok-soft, #e6f4ea); color: var(--ok); }
  .warn .lv { background: #faf1d8; color: #8a5a08; }
  .fail .lv { background: var(--err-soft, #fde8e8); color: var(--err); }
  .cb { min-width: 0; flex: 1; }
  .cb > span { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px; }
  .cb b { font-weight: 500; color: var(--text); }
  .cb small { color: var(--faint); font-size: 12px; }
  .cb .link-btn { font-size: 12px; }
  .ok .cb small { color: var(--faint); }
  .items { margin: 4px 0 2px; padding: 6px 10px; list-style: none; background: var(--rail); border-radius: 8px; display: flex; flex-direction: column; gap: 2px; max-height: 180px; overflow-y: auto; }
  .items li { font-size: 11.5px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .foot { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-top: 12px; }
  .foot p { margin: 0; flex: 1; min-width: 200px; }
  .bars { display: flex; flex-direction: column; gap: 8px; }
  .bars i { display: block; height: 10px; border-radius: 6px; background: #f1efe9; }
  @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
</style>
