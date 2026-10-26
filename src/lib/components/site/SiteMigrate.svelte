<script lang="ts">
  // 迁移进度卡：盘点 → 确认方案 → 抓取 → 重做 → 核对 → 切换 → 跟踪。
  // 一步一个面板：步骤条可以点，默认停在当前这一步；每个面板固定是「这一步是什么 → 现在的状态 → 一个主操作 + 次要操作 → 细节」。
  // 这一步的后台任务（盘点、抓取、核对、配置服务器、上线……）的进度直接显示在面板里，不再另起任务卡片。
  // 迁移期间站点的 domain 是空的：预览部署只建 Pages 项目、不绑域名，切换时才把域名指过来。
  import './site.css';
  import Icon from './Icon.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import MigrateSurvey from './MigrateSurvey.svelte';
  import SiteTodos from './SiteTodos.svelte';
  import type { Site } from '$lib/types';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { ago, full } from './fmt';
  import { app, say, confirmDialog } from '$lib/state/app.svelte.js';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import type { MStage } from '$lib/state/migrate.svelte.js';
  import { sites, deploySite, refreshSites } from '$lib/state/sites.svelte.js';
  import { openConv } from '$lib/state/conversations.js';
  import { makingConv, siteMake, loadMake, migrateTodos, previewSite } from '$lib/state/siteMake.svelte.js';
  import { migrate, STAGES, loadMigrate, resurvey, extract, startRebuild, verify, liveCheck, setFormal, stagingUp, stagingRemove, stagingCheck, checkOwnership, prepareDns, addToCloudflare, fixDns, cutover, rollback, track, trackIfStale, setupDynamic, setupVps, vpsDns, checkVpsDns } from '$lib/state/migrate.svelte.js';
  import { FEATURE_LABEL } from '$lib/state/dynamic.svelte.js';

  let { site }: { site: Site } = $props();
  const v = $derived(migrate.view[site.id]);
  const loading = $derived(!!migrate.loading[site.id]);
  const busy = $derived(migrate.busy[site.id] ?? '');
  const job = $derived(sites.jobs[site.id]);
  const jobRunning = $derived(job?.status === 'running');
  const deploying = $derived(jobRunning && job.kind === 'deploy');
  const conv = $derived(makingConv(site.id));
  const aiRunning = $derived(app.convos.some((c) => c.conn_id === `site:${site.id}` && c.status === 'running'));
  const stageIdx = $derived(v ? Math.max(0, STAGES.findIndex((s) => s.key === (v.stage === 'done' ? 'track' : v.stage))) : 0);
  const blocks = $derived(v?.meta.parity.filter((c) => c.level === 'block') ?? []);
  const warns = $derived(v?.meta.parity.filter((c) => c.level === 'warn') ?? []);
  const own = $derived(v?.survey?.ownership);
  const dns = $derived(v?.meta.dns);
  // 切换要指过去的正式域名（默认就是旧站的）；所有权确认看的是旧站的域名
  const oldHost = $derived(v?.origin_host || (v?.survey ? v.survey.origin.replace(/^https?:\/\//, '').replace(/\/.*$/, '') : v?.domain ?? ''));
  const host = $derived(v?.formal || oldHost);
  const moved = $derived(!!v?.meta.formal);
  let editFormal = $state(false);
  let formalIn = $state('');
  let stagingIn = $state('');
  const stg = $derived(v?.meta.staging);
  const cfConns = $derived(app.conns.filter((c) => c.kind === 'cloudflare'));
  let cfConn = $state('');
  let project = $state('');
  $effect(() => { if (!cfConn && cfConns.length) cfConn = site.deploy.cf_conn_id || cfConns[0].id; });
  $effect(() => { if (!project) project = (site.deploy.project || (site.migrate_from ?? site.name).replace(/^www\./, '').split('.')[0]).toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-+|-+$/g, '') + '-new'; });
  let editPlan = $state(false);
  let openItems = $state<Set<string>>(new Set());
  function toggleItem(k: string) { const n = new Set(openItems); if (n.has(k)) n.delete(k); else n.add(k); openItems = n; }

  $effect(() => { void loadMigrate(site.id); });
  $effect(() => { if (v?.stage === 'track') trackIfStale(site.id); });
  // AI 这一轮做完：刷新一下（重做阶段的按钮跟着变）
  let wasRunning = false;
  $effect(() => { const r = aiRunning; if (wasRunning && !r) { void loadMigrate(site.id); void loadMake(site.id, false); } wasRunning = r; });
  // AI 重做时写进 .pilot/todo.json 的「需要我处理的事」：迁移期间看板别的卡片都不显示，就放在这里
  $effect(() => { void loadMake(site.id, false); });
  const todos = $derived(migrateTodos(siteMake.state[site.id]?.todos));
  // 有构建产物就能在本机预览（AI 自己构建过、或者核对构建过）：不用先上线
  const built = $derived(!!siteMake.state[site.id]?.built_at);
  $effect(() => { if (v?.meta.parity_at) void loadMake(site.id, false); });

  /** 只建 Pages 项目、不绑域名（站点的 domain 迁移期间是空的） */
  async function setupPreview() {
    migrate.busy[site.id] = 'pages';
    try {
      await invoke('save_site', { site: { ...site, deploy: { ...site.deploy, mode: 'direct', cf_conn_id: cfConn, project: project.trim(), branch: site.deploy.branch || 'main' } } });
      await refreshSites();
      await invoke('site_setup_pages', { id: site.id });
      say(`已建好 Pages 项目 ${project.trim()}，可以上线到预览地址了`, 'ok');
    } catch (e) { say(String(e), 'err'); }
    finally { migrate.busy[site.id] = ''; void loadMigrate(site.id); }
  }
  const isVps = $derived(v?.plan?.deploy === 'vps');
  const sshConns = $derived(app.conns.filter((c) => c.kind === 'ssh'));
  let sshConn = $state('');
  $effect(() => { if (!sshConn && sshConns.length) sshConn = site.deploy.ssh_conn_id || sshConns[0].id; });
  const vpsSetting = $derived(busy === 'vps' || (jobRunning && job.kind === 'vps:setup'));
  const vd = $derived(vpsDns[site.id]);
  $effect(() => { if (isVps && v?.stage === 'cutover' && !vd) void checkVpsDns(site.id); });
  const featNames = (fs: string[]) => ['form', 'comments', 'files'].filter((f) => fs.includes(f)).map((f) => FEATURE_LABEL[f]).join('、');
  const dynNeed = $derived(!!v?.dynamic && v.dynamic.features.length > 0);
  const dynReady = $derived(!dynNeed || !!v?.dynamic?.setup_at);
  const dynSetting = $derived(busy === 'dynamic' || (jobRunning && job.kind === 'dynamic:setup'));
  const OWN_BY: Record<string, string> = { cloudflare: '域名在你的 Cloudflare 账号里', gsc: 'Search Console 里已验证这个站', txt: 'DNS 里有验证记录' };

  // ── 一步一个面板 ──
  const current = $derived<MStage>(v ? (v.stage === 'done' ? 'track' : v.stage) : 'survey');
  let picked = $state<MStage | ''>('');
  let lastStage = '';
  // 走到下一步时自动跟过去
  $effect(() => { const c = current; if (c !== lastStage) { lastStage = c; picked = ''; editPlan = false; } });
  const shown = $derived<MStage>(picked || current);
  const shownIdx = $derived(STAGES.findIndex((s) => s.key === shown));
  const isCurrent = $derived(shown === current && v?.stage !== 'done');
  // 后台任务属于哪一步
  const JOB_STEP: Record<string, MStage> = { 'migrate:survey': 'survey', 'migrate:extract': 'extract',  'migrate:verify': 'verify', 'migrate:live': 'verify', 'migrate:staging': 'cutover', 'vps:setup': 'verify', 'dynamic:setup': 'verify', deploy: 'verify', 'migrate:track': 'track' };
  const jobStep = $derived.by<MStage | ''>(() => {
    if (!job) return '';
    const st = JOB_STEP[job.kind] ?? '';
    if (job.kind === 'deploy' || job.kind === 'migrate:live') return ['track', 'done'].includes(v?.stage ?? '') ? 'track' : v?.stage === 'cutover' ? 'cutover' : st;
    return st;
  });
  const recentFail = $derived(!!job && job.status === 'failed' && job.finished_at > Date.now() / 1000 - 3600);
  const runHere = $derived(jobRunning && jobStep === shown);
  const failHere = $derived(recentFail && jobStep === shown);
  const runningCur = $derived(job?.steps.find((s) => s.status === 'running'));
  let showLog = $state(false);
  const aiStarted = $derived(conv ? app.lives[conv.id]?.startedAt ?? 0 : 0);
  // 上一轮是不是出错停下的（卡住、连续失败、网络……）：直接告诉用户原因
  const aiStop = $derived.by(() => {
    const last = conv?.messages?.[conv.messages.length - 1];
    return last && last.role !== 'user' && last.error ? last.text.split('\n')[0].slice(0, 160) : '';
  });
  const aiMins = $derived(aiStarted ? Math.max(1, Math.round((Date.now() - aiStarted) / 60000)) : 0);
  // 上线 / 发布成功过：实测不用重新构建，直接测
  const deployedOk = $derived(!!sites.status[site.id]?.last_deploy?.ok);
  const lastUrl = $derived(sites.status[site.id]?.last_deploy?.url ?? '');
  // 核对这一步接下来该做什么（只有它是主按钮）
  const vNext = $derived.by(() => {
    if (!v) return '';
    if (!v.meta.parity_at) return 'parity';
    if (blocks.length) return 'fix';
    if (!v.deploy_ready) return 'deploy';
    if (dynNeed && !dynReady && !isVps) return 'dynamic';
    if (!v.meta.live.at) return 'live';
    if (v.meta.live.bad.length) return 'fix';
    return 'done';
  });
  async function redo(what: string, f: () => unknown) {
    if (!await confirmDialog(`${what}之后，后面的步骤要重新来。确定吗？`, { title: what, kind: 'warning', confirmText: what })) return;
    picked = '';
    await f();
  }
  const STEP_DESC: Record<string, string> = {
    survey: '找全旧站的页面，判断适合部署到哪',
    plan: '决定迁哪些页面、新站的样子、部署到哪',
    extract: '把正文转成 Markdown，图片和附件放回原路径',
    rebuild: 'AI 用 Astro 重做全站：URL 一个不变、正文一字不改',
    verify: '新站逐条对照旧站，再把每个旧地址实际打开一遍',
    cutover: '把域名指向新站，可以随时回退',
    track: '切换后盯一段时间：旧地址、搜索流量',
  };
</script>

<div class="card mg">
  <h3>
    <Icon name="route" />迁移进度<span class="sub">{v?.domain ?? site.migrate_from ?? ''}</span>
    <span class="grow"></span>
    <button class="ic-btn" onclick={() => loadMigrate(site.id)} disabled={loading} aria-label="刷新" data-tip="刷新迁移状态"><RefreshIcon spinning={loading} /></button>
  </h3>

  <ol class="stages" role="tablist">
    {#each STAGES as s, i (s.key)}
      {@const done = i < stageIdx || v?.stage === 'done'}
      {@const now = i === stageIdx && v?.stage !== 'done'}
      <li>
        <button role="tab" aria-selected={shown === s.key} class:done class:now class:sel={shown === s.key} disabled={!done && !now} onclick={() => { picked = s.key === current ? '' : s.key; showLog = false; editPlan = false; }}>
          <span class="n">{#if jobRunning && jobStep === s.key}<span class="spin"></span>{:else}{done ? '✓' : i + 1}{/if}</span>{s.label}
        </button>
      </li>
    {/each}
  </ol>

  {#if !v}
    <div class="bars"><i style="width:58%"></i><i style="width:34%"></i></div>
  {:else}
    <div class="panel">
      <div class="ph">
        <b>第 {shownIdx + 1} 步 · {STAGES[shownIdx]?.label}</b>
        <small>{STEP_DESC[shown]}</small>
        <span class="grow"></span>
        {#if !isCurrent && v.stage !== 'done'}<span class="pill ok">已完成</span><button class="link-btn sm" onclick={() => (picked = '')}>回到当前步骤</button>{/if}
      </div>

      <!-- 这一步的后台任务 -->
      {#if runHere && job}
        <div class="running"><span class="spin"></span><span class="rt"><b>正在{job.title}</b>{#if runningCur} · {runningCur.label}{#if runningCur.detail}<small>{runningCur.detail}</small>{/if}{/if}</span><button class="link-btn sm" onclick={() => (showLog = !showLog)}>{showLog ? '收起日志' : '日志'}</button></div>
      {:else if failHere && job}
        <div class="failed"><span class="lv block">✕</span><span class="rt"><b>{job.title}没成功</b><small>{job.error.split('\n')[0]}</small></span><button class="link-btn sm" onclick={() => (showLog = !showLog)}>{showLog ? '收起日志' : '日志'}</button></div>
      {/if}
      {#if showLog && job && jobStep === shown}<div class="log"><SiteJobPanel {job} bare logOpen /></div>{/if}

      {#if shown === 'survey'}
        {#if isCurrent}
          {#if !runHere}
            <div class="act"><span class="grow">{failHere ? '盘点没完成，可以再试一次。' : '还没盘点。'}</span><button class="btn small primary" onclick={() => resurvey(site.id)} disabled={!!busy || jobRunning}><Icon name="play" size={12} />开始盘点</button></div>
          {/if}
        {:else if v.survey}
          <MigrateSurvey id={site.id} {v} editable={false} />
          <div class="act sub">
            {#if !v.survey.gsc_used}<span class="grow hint">没接 Search Console：接上以后重新盘点，有排名的页面一个都不会漏。<button class="link-btn" onclick={() => (sites.tab = 'settings')}>去接入</button></span>{:else}<span class="grow"></span>{/if}
            <button class="btn tiny" onclick={() => redo('重新盘点', () => resurvey(site.id))} disabled={!!busy || jobRunning}>重新盘点</button>
          </div>
        {/if}

      {:else if shown === 'plan'}
        {#if isCurrent || editPlan}
          {#if !isCurrent}<p class="hint warn-text">改了方案要重新抓取，后面的步骤也要重来。</p>{/if}
          <MigrateSurvey id={site.id} {v} editable={!jobRunning} />
        {:else if v.plan}
          {@const p = v.plan}
          <dl class="kv">
            <dt>部署到</dt><dd>{p.deploy === 'vps' ? '自己的服务器（Caddy + Go + SQLite）' : (v.hosting?.label ?? 'Cloudflare')}</dd>
            <dt>新站的样子</dt><dd>{p.visual === 'fresh' ? '全新视觉' : '保留品牌，升级设计'}{p.style ? ` · ${p.style}` : ''}</dd>
            <dt>不迁的页面组</dt><dd>{p.drop_groups.length ? p.drop_groups.join('、') : '没有，全部迁'}</dd>
            {#if Object.keys(p.features ?? {}).length}<dt>动态功能</dt><dd>{Object.entries(p.features).map(([k, x]) => `${v.survey?.features.find((f) => f.key === k)?.label ?? k}：${x === 'drop' ? '去掉' : '保留'}`).join('；')}</dd>{/if}
            <dt>确认于</dt><dd><span data-tip={full(p.confirmed_at)}>{ago(p.confirmed_at)}</span></dd>
          </dl>
          <div class="act sub"><span class="grow"></span><button class="btn tiny" onclick={() => (editPlan = true)} disabled={jobRunning}>修改方案</button></div>
        {/if}

      {:else if shown === 'extract'}
        {#if isCurrent && !runHere && !v.meta.extracted_at}
          <div class="act"><span class="grow">{failHere ? '可以再试一次。' : '方案已确认，接下来把旧站的内容抓下来。'}</span><button class="btn small primary" onclick={() => extract(site.id)} disabled={!!busy || jobRunning}><Icon name="play" size={12} />开始抓取</button></div>
        {/if}
        {#if v.meta.extracted_at && !runHere}
          {@const ex = v.meta.extract}
          <div class="stats">
            <div class="stat"><small>内容</small><b>{ex.pages}</b><em>篇</em></div>
            <div class="stat"><small>列表页和首页</small><b>{ex.lists}</b></div>
            <div class="stat"><small>图片和附件</small><b>{ex.assets}</b>{#if ex.large?.length}<em>{ex.large.length} 个大文件另放</em>{/if}</div>
            {#if ex.comments}<div class="stat"><small>旧评论</small><b>{ex.comments}</b></div>{/if}
          </div>
          {#if ex.features?.length}<p class="hint">动态功能：{featNames(ex.features)}（{isVps ? '在你的服务器上' : '在 Cloudflare 上'}实现）</p>{/if}
          {#if ex.assets_failed.length}
            <ul class="mini"><li><span class="lv warn">!</span><span>{ex.assets_failed.length} 个图片 / 附件没下载下来 <button class="link-btn" onclick={() => toggleItem('af')}>{openItems.has('af') ? '收起' : '查看'}</button></span></li></ul>
            {#if openItems.has('af')}<ul class="items">{#each ex.assets_failed as l (l)}<li class="mono">{l}</li>{/each}</ul>{/if}
          {/if}
          <div class="act sub"><small class="grow faint">抓取于 <span data-tip={full(v.meta.extracted_at)}>{ago(v.meta.extracted_at)}</span></small><button class="btn tiny" onclick={() => redo('重新抓取', () => extract(site.id))} disabled={!!busy || jobRunning} data-tip="按确认的方案重新抓一遍（会覆盖抓下来的内容文件）">重新抓取</button></div>
        {/if}

      {:else if shown === 'rebuild'}
        {@const ex = v.meta.extract}
        {#if isCurrent}
          {#if !v.meta.rebuild_at}
            <div class="act"><span class="grow">内容都抓好了。交给 AI 用 Astro 重做全站，只换外观。</span><button class="btn small primary" onclick={() => startRebuild(site)}><Icon name="wand" size={12} />开始重做</button></div>
          {:else if aiRunning}
            <div class="running"><span class="spin"></span><span class="rt"><b>AI 正在重做</b>{#if aiMins}<small>已 {aiMins} 分钟</small>{/if}</span>{#if conv}<button class="btn small primary" onclick={() => openConv(conv.id)}><Icon name="chat" size={12} />打开重做对话</button>{/if}</div>
            <p class="hint">做完后这里会出现「核对」。可以先去忙别的。</p>
          {:else}
            {#if aiStop}<div class="failed"><span class="lv block">!</span><span class="rt"><b>上一轮停下了</b><small>{aiStop}</small></span></div>{/if}
            {#if aiStop}
              <div class="act"><span class="grow">点「继续重做」会接着原来的对话做，已经做过的不会重来。</span>
                {#if conv}<button class="btn small" onclick={() => openConv(conv.id)}><Icon name="chat" size={12} />打开对话</button>{/if}
                <button class="btn small" onclick={() => verify(site.id)} disabled={jobRunning || !!busy}><Icon name="checks" size={12} />先核对看看</button>
                <button class="btn small primary" onclick={() => startRebuild(site)}><Icon name="play" size={12} />继续重做</button>
              </div>
            {:else}
              <div class="act"><span class="grow">AI 做完了{todos.length ? `，还留了 ${todos.length} 件事给你（见下面，不处理也能先核对）` : ''}。下一步核对：新站逐条对照旧站，有问题会列出来，可以再让 AI 修。</span>
                {#if conv}<button class="btn small" onclick={() => openConv(conv.id)}><Icon name="chat" size={12} />打开对话</button>{/if}
                {#if built}<button class="btn small" onclick={() => previewSite(site)} data-tip="在预览窗里打开新站（本机，不用上线）"><Icon name="external" size={12} />本地预览</button>{/if}
                <button class="btn small" onclick={() => startRebuild(site)} data-tip="没做完、或者想再改点什么：接着原来的对话做"><Icon name="play" size={12} />让 AI 接着做</button>
                <button class="btn small primary" onclick={() => verify(site.id)} disabled={jobRunning || !!busy}><Icon name="checks" size={12} />核对</button>
              </div>
            {/if}
          {/if}
        {:else if conv}
          <div class="act sub"><span class="grow faint">重做对话里有 AI 做了什么、改了哪些文件。</span><button class="btn tiny" onclick={() => openConv(conv.id)}><Icon name="chat" size={11} />打开重做对话</button></div>
        {/if}
        <div class="req">
          <b>交给 AI 的要求</b>
          <ul class="mini">
            <li><span class="lv info">i</span><span>{ex.pages} 篇内容、{ex.lists} 个列表页和首页，每个旧地址原样生成，正文一字不改</span></li>
            {#if ex.features?.length}<li><span class="lv info">i</span><span>动态功能：{featNames(ex.features)}，按 Pilot 给的接口接到页面上</span></li>{/if}
            {#if ex.low_coverage.length}<li><span class="lv warn">!</span><span>{ex.low_coverage.length} 页正文可能没抓全，要对照旧页面补齐 <button class="link-btn" onclick={() => toggleItem('cov')}>{openItems.has('cov') ? '收起' : '查看'}</button></span></li>{/if}
          </ul>
          {#if openItems.has('cov')}<ul class="items">{#each ex.low_coverage as l (l)}<li class="mono">{l}</li>{/each}</ul>{/if}
        </div>

      {:else if shown === 'verify'}
        <ol class="cut">
          <!-- ① 对照旧站 -->
          <li class={!v.meta.parity_at ? '' : blocks.length ? 'block' : 'ok'}>
            <span class="lv">{!v.meta.parity_at ? '1' : blocks.length ? '✕' : '✓'}</span>
            <div class="cb">
              <span><b>构建新站，对照旧站</b>
                {#if v.meta.parity_at}<small>{blocks.length ? `${blocks.length} 项必须修` : '全部通过'}{warns.length ? ` · ${warns.length} 项提醒` : ''} · <span data-tip={full(v.meta.parity_at)}>{ago(v.meta.parity_at)}</span></small>{/if}
              </span>
              {#if v.meta.parity_at}
                <ul class="checks">
                  {#each v.meta.parity.filter((c) => c.level !== 'ok' || openItems.has('allok')) as c (c.key)}
                    <li class={c.level}>
                      <span class="lv">{c.level === 'ok' ? '✓' : c.level === 'block' ? '✕' : '!'}</span>
                      <div class="cb"><span><b>{c.label}</b><small>{c.detail}</small>{#if c.items.length}<button class="link-btn" onclick={() => toggleItem(c.key)}>{openItems.has(c.key) ? '收起' : '查看'}</button>{/if}</span>
                        {#if openItems.has(c.key)}<ul class="items">{#each c.items.slice(0, 50) as it (it)}<li class="mono">{it}</li>{/each}</ul>{/if}
                      </div>
                    </li>
                  {/each}
                  {#if v.meta.parity.some((c) => c.level === 'ok')}<li class="ok folded"><span class="lv">✓</span><div class="cb"><span><small>{openItems.has('allok') ? '' : blocks.length || warns.length ? `其余 ${v.meta.parity.filter((c) => c.level === 'ok').length} 项已通过` : `${v.meta.parity.length} 项`}</small><button class="link-btn" onclick={() => toggleItem('allok')}>{openItems.has('allok') ? '只看有问题的' : '展开'}</button></span></div></li>{/if}
                </ul>
              {/if}
              <div class="row">
                <button class="btn small" class:primary={vNext === 'parity'} onclick={() => verify(site.id)} disabled={jobRunning || !!busy || aiRunning}><Icon name="play" size={12} />{v.meta.parity_at ? '重新核对' : '核对'}</button>
                {#if built}<button class="btn small" onclick={() => previewSite(site)} data-tip="在预览窗里打开刚构建好的新站（本机，不用上线）"><Icon name="external" size={12} />本地预览</button>{/if}
              </div>
            </div>
          </li>

          <!-- ② 部署到预览 / 服务器 -->
          <li class={v.deploy_ready && (dynReady || isVps) ? 'ok' : ''}>
            <span class="lv">{v.deploy_ready && (dynReady || isVps) ? '✓' : '2'}</span>
            <div class="cb">
              {#if isVps}
                <span><b>配置服务器</b>{#if v.vps?.setup_at}<small>{v.vps.ip} · {v.vps.os}{v.vps.api_port ? ' · 表单和评论服务' : ''}</small>{/if}</span>
                {#if !v.vps?.setup_at}
                  <small class="desc">选一台服务器（SSH 连接）。Pilot 会装 Caddy（网页和自动 HTTPS）{dynNeed ? '，再装一个 Go 写的小服务处理表单和评论（数据存 SQLite）' : ''}。需要 root 或免密 sudo；80 / 443 端口被别的网站服务占着时不会去动它。</small>
                  {#if sshConns.length}
                    <div class="row wrap">
                      <div class="dd"><Dropdown bind:value={sshConn} options={sshConns.map((c) => ({ value: c.id, label: c.name, sub: `${c.ssh_user}@${c.ssh_host}` }))} /></div>
                      <button class="btn small" class:primary={vNext === 'deploy'} onclick={() => setupVps(site.id, sshConn)} disabled={!!busy || jobRunning || !sshConn}>{vpsSetting ? '配置中…' : '配置服务器'}</button>
                    </div>
                  {:else}<small class="desc warn-text">还没有远程连接：先在设置里新建一条 SSH 连接。</small>{/if}
                {:else}
                  {#each v.vps.notes as n (n)}<small class="desc warn-text">{n}</small>{/each}
                  <div class="row"><button class="btn tiny" onclick={() => setupVps(site.id, site.deploy.ssh_conn_id)} disabled={jobRunning || !!busy}>重新配置</button></div>
                {/if}
              {:else}
                <span><b>准备预览地址</b>{#if v.deploy_ready}<small>Pages 项目 {site.deploy.project}{dynNeed ? (dynReady ? ' · 动态功能已配置' : '') : ''}</small>{/if}</span>
                {#if !v.deploy_ready}
                  <small class="desc">建一个 Cloudflare Pages 项目，只用来预览，不会动线上域名（Cloudflare 的跳转规则要上线后才生效，所以要实际上线测）。</small>
                  {#if cfConns.length}
                    <div class="row wrap">
                      <div class="dd"><Dropdown bind:value={cfConn} options={cfConns.map((c) => ({ value: c.id, label: c.name, sub: c.login || c.account_id }))} /></div>
                      <input class="tin proj" bind:value={project} placeholder="Pages 项目名" spellcheck="false" />
                      <button class="btn small" class:primary={vNext === 'deploy'} onclick={setupPreview} disabled={!!busy || !cfConn || !project.trim()}>{busy === 'pages' ? '创建中…' : '创建 Pages 项目'}</button>
                    </div>
                  {:else}<small class="desc warn-text">还没有 Cloudflare 连接：先在设置里连接 Cloudflare。</small>{/if}
                {:else if dynNeed && !dynReady}
                  <small class="desc">保留了{featNames(v.dynamic?.features ?? [])}：还要在 Cloudflare 上建数据库（D1）{v.dynamic?.features.includes('form') || v.dynamic?.features.includes('comments') ? '和人机验证（Turnstile）' : ''}{v.dynamic?.large.length ? `，把 ${v.dynamic.large.length} 个大文件传到 R2` : ''}，绑到这个 Pages 项目。令牌要有 D1、Turnstile{v.dynamic?.large.length ? '、R2' : ''} 的编辑权限。</small>
                  <div class="row"><button class="btn small" class:primary={vNext === 'dynamic'} onclick={() => setupDynamic(site.id)} disabled={jobRunning || !!busy}>{dynSetting ? '配置中…' : '配置动态功能'}</button></div>
                {:else if dynNeed}
                  {#each v.dynamic?.notes ?? [] as n (n)}<small class="desc">{n}</small>{/each}
                  <div class="row"><button class="btn tiny" onclick={() => setupDynamic(site.id)} disabled={jobRunning || !!busy}>重新配置动态功能</button></div>
                {/if}
              {/if}
            </div>
          </li>

          <!-- ③ 实测 -->
          <li class={!v.meta.live.at ? '' : v.meta.live.bad.length ? 'block' : 'ok'}>
            <span class="lv">{!v.meta.live.at ? '3' : v.meta.live.bad.length ? '✕' : '✓'}</span>
            <div class="cb">
              <span><b>{isVps ? '发布到服务器，实测每个旧地址' : '上线到预览地址，实测每个旧地址'}</b>
                {#if v.meta.live.at}<small>{v.meta.live.bad.length ? `${v.meta.live.checked} 个里 ${v.meta.live.bad.length} 个不对` : `${v.meta.live.checked} 个旧地址都能正常打开或跳转`}</small>{/if}
              </span>
              {#if !v.meta.live.at && v.deploy_ready}<small class="desc">{deployedOk ? `已${isVps ? '发布' : '上线'}，还没实测：点「实测每个旧地址」（不用重新构建，一两分钟），都通过就进入「切换」。` : isVps ? '发布成功后 Pilot 会在服务器上带域名请求每个旧地址，不用先改 DNS。' : `上线到 ${v.preview.replace(/^https?:\/\//, '')}，成功后 Pilot 会接着把每个旧地址实际打开一遍。`}{blocks.length ? '先把上面必须修的问题修好。' : ''}</small>{/if}
              {#if v.meta.live.bad.length}
                <button class="link-btn sm al" onclick={() => toggleItem('live')}>{openItems.has('live') ? '收起' : '查看不对的地址'}</button>
                {#if openItems.has('live')}<ul class="items">{#each v.meta.live.bad.slice(0, 80) as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
              {/if}
              {#if v.deploy_ready && (dynReady || isVps)}
                <div class="row">
                  {#if deployedOk && !v.meta.live.at}<button class="btn small" class:primary={vNext === 'live'} onclick={() => liveCheck(site.id)} disabled={jobRunning || !!busy}><Icon name="checks" size={12} />{runHere && job.kind === 'migrate:live' ? '实测中…' : '实测每个旧地址'}</button>{/if}
                  <button class="btn small" class:primary={vNext === 'live' && !deployedOk} onclick={() => deploySite(site.id)} disabled={jobRunning || !!blocks.length}><Icon name="rocket" size={12} />{deploying ? (isVps ? '发布中…' : '上线中…') : deployedOk ? (isVps ? '重新发布' : '重新上线') : (isVps ? '发布到服务器' : '上线到预览地址')}</button>
                  {#if !isVps && (v.meta.live.base || lastUrl)}<button class="btn tiny" onclick={() => openUrl(v.meta.live.base || lastUrl)}><Icon name="external" size={11} />打开预览</button>{/if}
                </div>
              {/if}
            </div>
          </li>
        </ol>
        {#if vNext === 'fix'}
          <div class="act"><span class="grow">有问题要修：交给 AI，按上面的结果改完再点「重新核对」。</span><button class="btn small primary" onclick={() => startRebuild(site)} disabled={aiRunning}><Icon name="wand" size={12} />{aiRunning ? 'AI 正在修…' : '让 AI 修这些问题'}</button></div>
        {:else if vNext === 'done' && warns.length && isCurrent}
          <div class="act sub"><span class="grow faint">还有 {warns.length} 项提醒，不影响切换。</span><button class="btn tiny" onclick={() => startRebuild(site)} disabled={aiRunning}><Icon name="wand" size={11} />让 AI 也处理掉</button></div>
        {/if}

      {:else if shown === 'cutover'}
        <div class="domains">
          <div class="dm">
            <span class="dk">正式域名</span>
            {#if editFormal}
              <input class="tin" bind:value={formalIn} placeholder={oldHost} spellcheck="false" autocapitalize="off" onkeydown={(e) => { if (e.key === 'Enter') void setFormal(site.id, formalIn, oldHost).then((ok) => { if (ok) editFormal = false; }); }} />
              <button class="btn tiny primary" onclick={() => setFormal(site.id, formalIn, oldHost).then((ok) => { if (ok) editFormal = false; })} disabled={!!busy}>保存</button>
              <button class="btn tiny quiet" onclick={() => (editFormal = false)}>取消</button>
            {:else}
              <b class="mono">{host}</b>{#if !moved}<small class="faint">和旧站一样</small>{/if}
              {#if !isVps}<button class="link-btn" onclick={() => { formalIn = host; editFormal = true; }}>修改</button>{/if}
              {#if moved}<button class="link-btn" onclick={() => setFormal(site.id, '', oldHost)}>恢复成 {oldHost}</button>{/if}
            {/if}
          </div>
          {#if moved}<small class="desc warn-text">换了域名：切换后 canonical 和 sitemap 用 {host}；旧域名 {oldHost} 的每个地址要另外做 301 跳到 {host} 的同一路径，不然搜索排名会丢。</small>{/if}
          {#if !isVps}
            <div class="dm">
              <span class="dk">测试域名</span>
              {#if stg?.host}
                <button class="link-btn mono" onclick={() => openUrl(`https://${stg.host}`)}>{stg.host}</button>
                <small class="faint">{stg.live.at ? (stg.live.bad.length ? `${stg.live.checked} 个里 ${stg.live.bad.length} 个不对` : `${stg.live.checked} 个旧地址都正常`) : '已绑定 · 不让搜索引擎收录'}</small>
                <span class="grow"></span>
                <button class="btn tiny" onclick={() => stagingCheck(site.id)} disabled={!!busy || jobRunning}>{runHere && job.kind === 'migrate:staging' ? '实测中…' : '实测测试域名'}</button>
                <button class="btn tiny quiet" onclick={() => stagingRemove(site.id, stg.host)} disabled={!!busy || jobRunning}>删除</button>
              {:else}
                <input class="tin" bind:value={stagingIn} placeholder={`new.${host.replace(/^www\./, '')}`} spellcheck="false" autocapitalize="off" />
                <button class="btn tiny" onclick={() => stagingUp(site.id, stagingIn || `new.${host.replace(/^www\./, '')}`)} disabled={!!busy || jobRunning}>{busy === 'staging' ? '上线中…' : '上线到测试域名'}</button>
              {/if}
            </div>
            {#if !stg?.host}<small class="desc">可选：先在一个测试域名上看新站（旧站不受影响，页面不让搜索引擎收录），没问题再切正式域名。要用 Cloudflare 里已经生效的域名，没在用的子域名最合适。</small>{/if}
            {#if stg?.live.bad.length}
              <button class="link-btn sm al" onclick={() => toggleItem('stg')}>{openItems.has('stg') ? '收起' : '查看不对的地址'}</button>
              {#if openItems.has('stg')}<ul class="items">{#each stg.live.bad.slice(0, 80) as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
            {/if}
          {/if}
        </div>
        <ol class="cut">
          <li class:ok={own?.verified}>
            <span class="lv">{own?.verified ? '✓' : '1'}</span>
            <div class="cb">
              <span><b>确认这个站是你的</b>{#if own?.verified}<small>{OWN_BY[own.by] ?? '已确认'}</small>{/if}</span>
              {#if !own?.verified}
                <small class="desc">满足一条即可：域名在你的 Cloudflare 账号里；或者 Search Console 里验证过这个站；或者在域名的 DNS 里加一条 TXT 记录：</small>
                <div class="txt"><code>{own?.txt_token}</code><small>主机 <code>{oldHost.replace(/^www\./, '')}</code>（根域）· 类型 TXT</small></div>
                <div class="row"><button class="btn tiny" onclick={() => checkOwnership(site.id)} disabled={!!busy}>{busy === 'own' ? '检查中…' : '重新检查'}</button></div>
              {/if}
            </div>
          </li>
          {#if isVps}
          <li class:ok={!!vd?.points}>
            <span class="lv">{vd?.points ? '✓' : '2'}</span>
            <div class="cb">
              <span><b>域名指向服务器 {v.vps?.ip}</b>{#if vd?.points}<small>已指向</small>{:else if vd?.cf_auto}<small>域名在你的 Cloudflare 账号里，切换时 Pilot 自动改</small>{/if}</span>
              {#if vd && !vd.points && !vd.cf_auto}
                <small class="desc">到域名的 DNS 服务商，把这两条 A 记录改成服务器的 IP（改之前访客照常看到旧站；改完一般几分钟到几小时生效）：</small>
                <div class="ns"><code>{vd.host} → {vd.ip}</code>{#if vd.alias}<code>{vd.alias} → {vd.ip}</code>{/if}</div>
                <small class="faint">现在：{vd.host} → {vd.host_ips.join('、') || '没有记录'}{vd.alias ? `；${vd.alias} → ${vd.alias_ips.join('、') || '没有记录'}` : ''}</small>
              {/if}
              <div class="row"><button class="btn tiny" onclick={() => checkVpsDns(site.id)} disabled={!!busy}>{busy === 'vpsdns' ? '检查中…' : '重新检查'}</button></div>
            </div>
          </li>
          {:else}
          <li class:ok={dns?.zone_status === 'active'}>
            <span class="lv">{dns?.zone_status === 'active' ? '✓' : '2'}</span>
            <div class="cb">
              <span><b>域名的 DNS 在 Cloudflare 里生效</b>{#if dns?.zone_status === 'active'}<small>已生效</small>{:else if dns?.zone_id}<small>已加进 Cloudflare，等你改名称服务器（{dns.zone_status || '待生效'}）</small>{/if}</span>
              {#if !dns?.at}
                <div class="row"><button class="btn tiny" onclick={() => prepareDns(site.id)} disabled={!!busy}>{busy === 'dns' ? '检查中…' : '检查 DNS'}</button><small class="faint">先存一份现在的 DNS 记录（邮件等），再看域名在不在 Cloudflare</small></div>
              {:else if !dns.zone_id}
                <small class="desc">{host.replace(/^www\./, '')} 还不在你的 Cloudflare 账号里。加进去以后 Cloudflare 会导入现有记录，Pilot 再对照一遍，缺的（尤其是邮件记录）补上，然后你去域名注册商改名称服务器。</small>
                <div class="row"><button class="btn tiny" onclick={() => addToCloudflare(site.id, host.replace(/^www\./, ''))} disabled={!!busy || !site.deploy.cf_conn_id}>{busy === 'dns' ? '处理中…' : '接进 Cloudflare'}</button><button class="btn tiny quiet" onclick={() => prepareDns(site.id)} disabled={!!busy}>重新检查</button></div>
              {:else}
                {#if dns.missing.length}
                  <small class="desc warn-text">原来的 {dns.missing.length} 条记录 Cloudflare 里还没有，改名称服务器前要补上，不然{dns.missing.some((r) => r.kind === 'MX') ? '邮件会收不到' : '这些子域名会打不开'}：</small>
                  <ul class="items">{#each dns.missing as r (r.kind + r.name + r.content)}<li class="mono">{r.kind} {r.name} → {r.priority != null ? `${r.priority} ` : ''}{r.content}</li>{/each}</ul>
                  <div class="row"><button class="btn tiny" onclick={() => fixDns(site.id, dns.missing.length)} disabled={!!busy}>补上 {dns.missing.length} 条</button></div>
                {/if}
                {#if dns.zone_status !== 'active'}
                  <small class="desc">到域名注册商（买域名的地方）把名称服务器改成：</small>
                  <div class="ns">{#each dns.ns as n (n)}<code>{n}</code>{/each}</div>
                  {#if dns.current_ns.length}<small class="faint">现在是：{dns.current_ns.join('、')}。改完一般几分钟到几小时生效，网站和邮件照常可用。</small>{/if}
                  <div class="row"><button class="btn tiny" onclick={() => prepareDns(site.id)} disabled={!!busy}>{busy === 'dns' ? '检查中…' : '检查是否生效'}</button></div>
                {/if}
              {/if}
            </div>
          </li>
          {/if}
          <li>
            <span class="lv">3</span>
            <div class="cb">
              <span><b>切换到新站</b><small>{isVps ? `${host} 指向服务器后，Caddy 自动申请 HTTPS 证书；原来的 DNS 记录存下来，可以回退。旧主机先别停。` : `${host} 改为指向新站；原来的记录存下来，可以一键回退。旧主机先别停。`}</small></span>
              <div class="row"><button class="btn small primary" onclick={() => cutover(site.id, host, isVps, !!vd?.cf_auto)} disabled={!!busy || jobRunning || !own?.verified || (isVps ? !(vd?.points || vd?.cf_auto) : dns?.zone_status !== 'active' || !!dns?.missing.length)}><Icon name="rocket" size={12} />{busy === 'cutover' ? '切换中…' : '切换到新站'}</button></div>
            </div>
          </li>
        </ol>

      {:else if shown === 'track'}
        {@const t = v.meta.track}
        <div class="act"><span class="grow">{v.stage === 'done' ? '跟踪满 28 天了，迁移完成。' : '切换后会定期检查；打开站点时超过 12 小时没查过也会自动查一次。'}<small class="faint"> 切换于 <span data-tip={full(v.meta.cutover.at)}>{ago(v.meta.cutover.at)}</span>{t.at ? ` · 上次检查 ${ago(t.at)}` : ''}</small></span>
          <button class="btn small primary" onclick={() => track(site.id)} disabled={jobRunning || !!busy}><Icon name="play" size={12} />立即检查</button>
          <button class="btn small danger" onclick={() => rollback(site.id, v.meta.cutover.host)} disabled={jobRunning || !!busy}><Icon name="undo" size={12} />回退到旧站</button>
        </div>
        <ul class="mini">
          {#each v.meta.cutover.notes as n (n)}<li><span class="lv info">i</span><span>{n}</span></li>{/each}
          {#if t.at}
            <li><span class="lv {t.bad.length ? 'block' : 'ok'}">{t.bad.length ? '✕' : '✓'}</span><span>{t.bad.length ? `${t.checked} 个旧地址里 ${t.bad.length} 个在正式域名上不对` : `${t.checked} 个旧地址在正式域名上都正常`}{#if t.bad.length} <button class="link-btn" onclick={() => toggleItem('tb')}>{openItems.has('tb') ? '收起' : '查看'}</button>{/if}</span></li>
            {#if openItems.has('tb')}<ul class="items">{#each t.bad.slice(0, 80) as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
            {#if t.after_days}<li><span class="lv {t.base_clicks > 5 && t.after_clicks < t.base_clicks * 0.7 ? 'warn' : 'ok'}">{t.base_clicks > 5 && t.after_clicks < t.base_clicks * 0.7 ? '!' : '✓'}</span><span>搜索点击：切换前日均 {t.base_clicks.toFixed(1)}，切换后 {t.after_clicks.toFixed(1)}（{t.after_days} 天）</span></li>{/if}
            <li><span class="lv {t.sitemap_submitted && t.indexnow_submitted ? 'ok' : 'info'}">{t.sitemap_submitted && t.indexnow_submitted ? '✓' : 'i'}</span><span>sitemap {t.sitemap_submitted ? '已提交到 Search Console' : '还没提交'} · IndexNow {t.indexnow_submitted ? '已推送全部页面' : '等验证文件生效后推送'}</span></li>
            {#each t.notes as n (n)}<li><span class="lv warn">!</span><span>{n}</span></li>{/each}
          {/if}
        </ul>
        <p class="hint">接下来的内容更新可以交给托管运营：在下面的「托管」里加入一个托管方案。</p>
      {/if}

      {#if todos.length && !aiRunning && ['rebuild', 'verify', 'cutover'].includes(shown)}
        <div class="todo-box">
          <b>需要你处理 <small>{todos.length} 件 · AI 重做时留下的</small></b>
          <SiteTodos {site} {todos} />
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .stages button { display: inline-flex; align-items: center; gap: 6px; border: none; background: none; padding: 4px 8px; margin: 0 -2px; border-radius: 8px; font: inherit; font-size: 12px; color: var(--faint); cursor: pointer; white-space: nowrap; transition: background-color .12s, color .12s; }
  .stages button:disabled { cursor: default; }
  .stages button:not(:disabled):hover { background: #f1efe9; color: var(--text); }
  .stages button.done { color: var(--dim); }
  .stages button.now { color: var(--text); font-weight: 600; }
  .stages button.sel { background: #f1efe9; color: var(--text); }
  .stages button .n .spin { width: 9px; height: 9px; }
  .stages button.now .n { background: var(--text); border-color: var(--text); color: #fff; }
  .stages button.done .n { background: var(--su-ok-soft); border-color: transparent; color: var(--ok); }
  .panel { border-top: 1px solid var(--border); padding-top: 14px; display: flex; flex-direction: column; gap: 12px; }
  .ph { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  .ph b { font-size: 13.5px; font-weight: 600; }
  .ph small { font-size: 12px; color: var(--faint); }
  .rt { flex: 1; min-width: 0; }
  .rt small { margin-left: 6px; color: var(--dim); }
  .failed { display: flex; align-items: center; gap: 10px; padding: 9px 12px; border-radius: 10px; background: var(--err-soft); color: var(--err); font-size: 12.5px; }
  .failed .rt small { color: var(--err); opacity: .85; }
  .log { margin-top: -4px; }
  .act { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; padding: 9px 9px 9px 14px; border-radius: 10px; background: var(--rail); font-size: 12.5px; color: var(--dim); }
  .act .grow { line-height: 1.55; }
  .act.sub { padding: 0; background: none; }
  .req { display: flex; flex-direction: column; gap: 6px; }
  .domains { display: flex; flex-direction: column; gap: 8px; padding: 12px 14px; border-radius: 10px; background: var(--rail); }
  .dm { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; font-size: 12.5px; min-height: 28px; }
  .dm .dk { flex: none; width: 64px; color: var(--dim); }
  .dm b { font-weight: 600; font-size: 13px; }
  .dm .tin { flex: 1; min-width: 180px; max-width: 320px; height: 28px; font-size: 12.5px; }
  .dm .grow { flex: 1; }
  .domains .desc { font-size: 12px; color: var(--faint); line-height: 1.55; }
  .warn-text, .domains .desc.warn-text, .cb .desc.warn-text { color: var(--su-warn-ink, #8a5a08); }
  .todo-box { display: flex; flex-direction: column; gap: 8px; border-top: 1px solid var(--border); padding-top: 12px; }
  .todo-box > b { font-size: 12.5px; font-weight: 600; }
  .todo-box > b small { margin-left: 6px; font-weight: 400; color: var(--faint); }
  .req > b { font-size: 12.5px; font-weight: 600; }
  .link-btn.sm { font-size: 12px; }
  .link-btn.al { align-self: flex-start; }
  .kv { font-size: 12.5px; }
  .mg h3 .sub { font-weight: 400; font-size: 12px; color: var(--faint); }
  .ic-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border: none; background: none; border-radius: 6px; color: var(--faint); cursor: pointer; }
  .ic-btn:hover { background: #f1efe9; color: var(--text); }
  .stages { list-style: none; margin: 0 0 14px; padding: 0; display: flex; flex-wrap: wrap; gap: 6px 14px; font-size: 12px; color: var(--faint); }
  .stages .n { width: 18px; height: 18px; border-radius: 50%; border: 1px solid var(--border2); box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; font-size: 10.5px; font-weight: 600; }
  .running { display: flex; align-items: center; gap: 10px; padding: 9px 12px; margin-bottom: 12px; border-radius: 10px; background: var(--su-run-soft); color: var(--su-run); font-size: 12.5px; }
  .running span:last-child { min-width: 0; }
  .running small { margin-left: 6px; color: var(--dim); }
  .grow { flex: 1; }
  .checks, .mini, .cut { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 7px; }
  .checks > li, .mini > li, .cut > li { display: flex; align-items: flex-start; gap: 9px; font-size: 12.5px; }
  .mini > li > span:last-child { min-width: 0; line-height: 1.5; color: var(--dim); }
  .lv { flex: none; width: 16px; height: 16px; margin-top: 1px; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; font-size: 10px; font-weight: 700; background: var(--accent-soft); color: var(--dim); }
  .ok > .lv, .lv.ok { background: var(--su-ok-soft); color: var(--ok); }
  .warn > .lv, .lv.warn { background: #faf1d8; color: #8a5a08; }
  .block > .lv, .lv.block { background: var(--err-soft); color: var(--err); }
  .lv.info { background: var(--su-run-soft); color: var(--su-run); font-style: italic; }
  .cb { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 5px; }
  .cb > span { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px; }
  .cb b { font-weight: 500; }
  .cb small { color: var(--faint); font-size: 12px; }
  .cb .desc { color: var(--dim); line-height: 1.55; }
  .items { margin: 2px 0; padding: 6px 10px; list-style: none; background: var(--rail); border-radius: 8px; display: flex; flex-direction: column; gap: 2px; max-height: 180px; overflow-y: auto; }
  .items li { font-size: 11.5px; color: var(--dim); overflow-wrap: anywhere; }
  .dd { width: 200px; }
  .proj { width: 180px !important; }
  .cut > li { padding: 10px 0; border-top: 1px solid var(--border); }
  .cut > li:first-child { border-top: none; padding-top: 2px; }
  .txt { display: flex; flex-direction: column; gap: 3px; padding: 7px 10px; border-radius: 8px; background: var(--rail); }
  .txt code { font-size: 12px; user-select: all; }
  .ns { display: flex; gap: 8px; flex-wrap: wrap; }
  .ns code { font-size: 12px; padding: 2px 6px; user-select: all; }
  .bars { display: flex; flex-direction: column; gap: 8px; }
  .bars i { display: block; height: 10px; border-radius: 6px; background: #f1efe9; }
</style>
