<script lang="ts">
  // 改版进度卡：盘点 → 确认方案 → 抓取 → 重做 → 核对 → 切换 → 跟踪。
  // 一步一个面板：步骤条可以点，默认停在当前这一步；每个面板固定是「这一步是什么 → 现在的状态 → 一个主操作 + 次要操作 → 细节」。
  // 这一步的后台任务（盘点、抓取、核对、配置服务器、上线……）的进度直接显示在面板里，不再另起任务卡片。
  // 迁移期间站点的 domain 是空的：预览部署只建 Pages 项目、不绑域名，切换时才把域名指过来。
  import './site.css';
  import { untrack } from 'svelte';
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
  import { humanTool } from '$lib/state/messages.svelte.js';
  import { makingConv, siteMake, loadMake, migrateTodos, previewSite } from '$lib/state/siteMake.svelte.js';
  import { migrate, STAGES, loadMigrate, resurvey, extract, startRebuild, verify, liveCheck, setupPreviewProject, defaultProject, batch, setAuto, directions, loadDirections, askDirections, pickDirection, openDirection, setFormal, hostUse, useText, oldRedirect, oldRedirectUndo, stagingUp, stagingRemove, stagingCheck, stagingStatus, prepareDnsQuiet, checkOwnership, prepareDns, addToCloudflare, fixDns, cutover, rollback, track, trackIfStale, setupDynamic, setupVps, vpsDns, checkVpsDns } from '$lib/state/migrate.svelte.js';
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
  // 换了正式域名：看看它现在有没有在用
  let formalUse = $state<import('$lib/state/migrate.svelte.js').HostUse | null>(null);
  $effect(() => {
    const h = moved ? host : '';
    void v?.meta.dns.zone_status; // 接进 Cloudflare、生效以后重新查
    formalUse = null;
    if (h) untrack(() => void hostUse(site.id, h).then((u) => { if (host === h) formalUse = u; }));
  });
  // 测试域名是可选的：没绑过就先收起来
  let showStaging = $state(false);
  const stg = $derived(v?.meta.staging);
  const apexOf = (h: string) => h.replace(/^www\./, '');
  const cfConns = $derived(app.conns.filter((c) => c.kind === 'cloudflare'));
  let cfConn = $state('');
  let project = $state('');
  $effect(() => { if (!cfConn && cfConns.length) cfConn = site.deploy.cf_conn_id || cfConns[0].id; });
  $effect(() => { if (!project) project = defaultProject(site); });
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
  const setupPreview = () => setupPreviewProject(site, cfConn, project);
  const isVps = $derived(v?.plan?.deploy === 'vps');
  const sshConns = $derived(app.conns.filter((c) => c.kind === 'ssh'));
  let sshConn = $state('');
  $effect(() => { if (!sshConn && sshConns.length) sshConn = site.deploy.ssh_conn_id || sshConns[0].id; });
  const vpsSetting = $derived(busy === 'vps' || (jobRunning && job.kind === 'vps:setup'));
  const vd = $derived(vpsDns[site.id]);
  // ── 切换：能自动的都自动 ──
  // 进到这一步就查一次 DNS（不用点「检查 DNS」）
  let dnsAuto = '';
  $effect(() => { if (v?.stage === 'cutover' && !isVps && !v.meta.dns.at && dnsAuto !== site.id) { dnsAuto = site.id; untrack(() => void prepareDnsQuiet(site.id)); } });
  // 旧站的所有权：进到这一步还没确认的，自动用连接的 Cloudflare / Google 账号查一遍
  let ownAuto = '';
  $effect(() => { if (v?.stage === 'cutover' && v.survey && !v.survey.ownership.verified && ownAuto !== site.id) { ownAuto = site.id; untrack(() => void checkOwnership(site.id)); } });
  // 等你改名称服务器：每分钟自动查一次生效没有
  $effect(() => {
    if (v?.stage !== 'cutover' || isVps || !v.meta.dns.zone_id || v.meta.dns.zone_status === 'active') return;
    const t = setInterval(() => untrack(() => void prepareDnsQuiet(site.id)), 60000);
    return () => clearInterval(t);
  });
  // 自己的服务器：等你改 A 记录，每分钟查一次
  $effect(() => {
    if (v?.stage !== 'cutover' || !isVps || !vd || vd.points || vd.cf_auto) return;
    const t = setInterval(() => untrack(() => void checkVpsDns(site.id)), 60000);
    return () => clearInterval(t);
  });
  // 测试域名在生效中（Cloudflare 验证、签证书）：隔几秒问一次，生效了后台自动实测
  $effect(() => {
    const st = v?.meta.staging;
    if (!st?.host || st.status === 'active') return;
    untrack(() => void stagingStatus(site.id));
    const t = setInterval(() => untrack(() => void stagingStatus(site.id)), 8000);
    return () => clearInterval(t);
  });
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
  // 已经正式上线：前面几步只给你看，不再给「重新抓取 / 修改方案 / 重新部署」这类会动到线上站的按钮
  const launched = $derived(!!v && (v.stage === 'track' || v.stage === 'done'));
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
  const dirs = $derived(directions[site.id] ?? []);
  let dirFeedback = $state('');
  $effect(() => { if (v?.meta.directions_at) untrack(() => void loadDirections(site.id)); });
  // AI 现在在做什么：最近一步工具调用（读文件、改文件、跑命令、截图……）
  const aiLive = $derived(conv ? app.lives[conv.id] : undefined);
  const aiSteps = $derived(aiLive?.tools?.length ?? 0);
  const aiDoing = $derived.by(() => {
    const t = aiLive?.tools?.[aiLive.tools.length - 1];
    if (!t) return '';
    const h = humanTool(t);
    return `${h.verb} ${h.target}`.trim().slice(0, 120);
  });
  const aiMins = $derived(aiStarted ? Math.max(1, Math.round((Date.now() - aiStarted) / 60000)) : 0);
  // 「是你的」看要上线的那个域名：旧站域名看所有权确认；换了新域名，看它在不在你的 Cloudflare 账号里
  const oldOwned = $derived(!!own?.verified);
  // 上线会替换掉的东西：旧站域名上是旧站；新域名在用就是那里原来的内容；没在用就没什么可退的
  const goneText = $derived(!moved ? '；原来的设置存着，随时能退回旧站' : formalUse?.records.length && !formalUse.ours ? '；那里原来的内容会打不开（设置存着，能退回）' : '');
  // 新域名在不在你的 Cloudflare 账号里：第 2 条（域名交给 Cloudflare 管理）查到生效了就算，和它用同一份结果
  const formalInCf = $derived(!!formalUse?.in_cf || (moved && dns?.zone_status === 'active'));
  const mine = $derived(moved ? formalInCf : oldOwned);
  // 切换这一步现在该做什么：只剩一个动作，写在最上面
  const cutNext = $derived.by<{ text: string; spin?: boolean; ready?: boolean; act?: 'addcf' | 'fix' | 'own' } | null>(() => {
    if (!v || v.stage !== 'cutover') return null;
    const apex = host.replace(/^www\./, '');
    if (moved && (!formalUse || (!formalInCf && !dns?.at))) return { text: `正在查 ${host} 是不是你的…`, spin: true };
    // 已经加进 Cloudflare、在等名称服务器生效的，交给下面 DNS 那几种情况说
    if (moved && !mine && !dns?.zone_id) return { text: `${host} 不在你的 Cloudflare 账号里。是你的域名就按下面第 2 条接进来；不是你的没法上线到它，点上面「修改」换一个。`, act: isVps ? undefined : 'addcf' };
    if (!moved && !mine) return { text: `先确认 ${oldHost.replace(/^www\./, '')} 是你的：按下面第 1 条加一条 TXT 记录，或者在设置里接上 Search Console`, act: 'own' };
    if (isVps) {
      if (!vd) return { text: `正在检查 ${host} 指向哪里…`, spin: true };
      if (!(vd.points || vd.cf_auto)) return { text: `只差一步要你自己做：到域名的 DNS 服务商把 ${host} 指向服务器 ${vd.ip}（见下面第 2 条），Pilot 会自动检查` };
      return { text: `都准备好了。点「正式上线」，${host} 就会打开新站${goneText}。`, ready: true };
    }
    if (!dns?.at || busy === 'dns') return { text: `正在检查 ${apex} 的 DNS…`, spin: true };
    if (!dns.zone_id) return { text: `${apex} 还不在你的 Cloudflare 账号里。点「接进 Cloudflare」：Pilot 自动加进去、照原样补齐现有记录（邮件等），之后只剩改名称服务器要你做。`, act: 'addcf' };
    if (dns.missing.length) return { text: `原有的 ${dns.missing.length} 条记录还没进 Cloudflare，改名称服务器前要补上（见下面第 2 条）。`, act: 'fix' };
    if (dns.zone_status !== 'active') return { text: `只差一步要你自己做：去域名注册商把名称服务器改成下面第 2 条里的两个。Pilot 每分钟自动检查一次，生效了这里会变。` };
    const tip = stg?.host && stg.status !== 'active' ? `测试域名 ${stg.host} 还在生效中，可以等它好了先看看。` : '';
    return { text: `都准备好了。点「正式上线」，${host} 就会打开新站${goneText}。${tip}`, ready: true };
  });
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
    extract: '把旧站的文字、图片和附件搬下来，地址照旧',
    rebuild: 'AI 用 Astro 重做全站：URL 一个不变、正文一字不改',
    verify: '新站逐条对照旧站，再把每个旧地址实际打开一遍',
    cutover: '让正式域名打开新站，随时能退回旧站',
    track: '上线后盯一段时间：旧地址、搜索流量',
  };
</script>

<div class="card mg">
  <h3>
    <Icon name="route" />改版进度<span class="sub">{launched && v ? `原站 ${v.domain} → 新站 ${v.meta.cutover.host}` : `原站 ${v?.domain ?? site.migrate_from ?? ''}`}</span>
    <span class="grow"></span>
    <button class="ic-btn ic-sm" onclick={() => loadMigrate(site.id)} disabled={loading} aria-label="刷新" data-tip="刷新改版进度"><RefreshIcon spinning={loading} /></button>
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

  {#if v?.plan && ['extract', 'rebuild', 'verify'].includes(v.stage)}
    <div class="autobar" class:on={v.plan.auto}>
      {#if v.plan.auto}
        <span class="dot" class:wait={!!batch.waiting[site.id]}></span>
        <span class="grow">{batch.waiting[site.id] ? `自动推进停在这里：${batch.waiting[site.id]}` : '自动推进中：每一步做完会自己接着下一步，到挑设计方向、正式上线时停下来等你'}</span>
        <button class="link-btn" onclick={() => setAuto(site.id, false)}>改成手动</button>
      {:else}
        <span class="grow">现在是手动：每一步要你点。</span>
        <button class="link-btn" onclick={() => setAuto(site.id, true)}>打开自动推进</button>
      {/if}
    </div>
  {/if}

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
            {#if !v.survey.gsc_used && !launched}<span class="grow hint">没接 Search Console：接上以后重新盘点，有排名的页面一个都不会漏。<button class="link-btn" onclick={() => (sites.tab = 'settings')}>去接入</button></span>{:else}<span class="grow"></span>{/if}
            {#if !launched}<button class="btn tiny" onclick={() => redo('重新盘点', () => resurvey(site.id))} disabled={!!busy || jobRunning}>重新盘点</button>{/if}
          </div>
        {/if}

      {:else if shown === 'plan'}
        {#if isCurrent || editPlan}
          {#if !isCurrent}<p class="hint warn-text">改了方案要重新抓取，后面的步骤也要重来。</p>{/if}
          <MigrateSurvey id={site.id} {v} editable={!jobRunning} oncancel={!isCurrent ? () => (editPlan = false) : undefined} />
        {:else if v.plan}
          {@const p = v.plan}
          <dl class="kv">
            <dt>部署到</dt><dd>{p.deploy === 'vps' ? '自己的服务器（Caddy + Go + SQLite）' : (v.hosting?.label ?? 'Cloudflare')}</dd>
            <dt>新站的样子</dt><dd>{p.visual === 'fresh' ? '全新视觉' : '保留品牌，升级设计'}{p.style ? ` · ${p.style}` : ''}</dd>
            <dt>不迁的页面组</dt><dd>{p.drop_groups.length ? p.drop_groups.join('、') : '没有，全部迁'}</dd>
            {#if Object.keys(p.features ?? {}).length}<dt>动态功能</dt><dd>{Object.entries(p.features).map(([k, x]) => `${v.survey?.features.find((f) => f.key === k)?.label ?? k}：${x === 'drop' ? '去掉' : '保留'}`).join('；')}</dd>{/if}
            <dt>确认于</dt><dd><span data-tip={full(p.confirmed_at)}>{ago(p.confirmed_at)}</span></dd>
          </dl>
          {#if !launched}<div class="act sub"><span class="grow"></span><button class="ic-btn ic-sm" onclick={() => (editPlan = true)} disabled={jobRunning} aria-label="修改方案" data-tip="修改方案（改了要重新抓取）"><Icon name="pencil" size={14} /></button></div>{/if}
        {/if}

      {:else if shown === 'extract'}
        {#if isCurrent && !runHere && !v.meta.extracted_at}
          <div class="act"><span class="grow">{failHere ? '可以再试一次。' : '方案已确认，接下来把旧站的内容抓下来。'}</span><button class="btn small primary" onclick={() => extract(site.id)} disabled={!!busy || jobRunning}><Icon name="play" size={12} />开始抓取</button></div>
        {/if}
        {#if v.meta.extracted_at && !runHere}
          {@const ex = v.meta.extract}
          <div class="stats">
            <div class="stat"><small>内容页</small><b>{ex.pages}</b></div>
            <div class="stat"><small>列表页和首页</small><b>{ex.lists}</b></div>
            <div class="stat"><small>图片和附件</small><b>{ex.assets}</b>{#if ex.large?.length}<em>{ex.large.length} 个大文件另放</em>{/if}</div>
            {#if ex.comments}<div class="stat"><small>旧评论</small><b>{ex.comments}</b></div>{/if}
          </div>
          {#if ex.features?.length}<p class="hint">动态功能：{featNames(ex.features)}（{isVps ? '在你的服务器上' : '在 Cloudflare 上'}实现）</p>{/if}
          {#if ex.assets_failed.length}
            <ul class="mini"><li><span class="lv warn">!</span><span>{ex.assets_failed.length} 个图片 / 附件没下载下来 <button class="link-btn" onclick={() => toggleItem('af')}>{openItems.has('af') ? '收起' : '查看'}</button></span></li></ul>
            {#if openItems.has('af')}<ul class="items">{#each ex.assets_failed as l (l)}<li class="mono">{l}</li>{/each}</ul>{/if}
          {/if}
          <div class="act sub"><small class="grow faint">抓取于 <span data-tip={full(v.meta.extracted_at)}>{ago(v.meta.extracted_at)}</span></small>{#if !launched}<button class="btn tiny" onclick={() => redo('重新抓取', () => extract(site.id))} disabled={!!busy || jobRunning} data-tip="按确认的方案重新抓一遍（会覆盖抓下来的内容文件）">重新抓取</button>{/if}</div>
        {/if}

      {:else if shown === 'rebuild'}
        {@const ex = v.meta.extract}
        {#if isCurrent}
          {#if !v.meta.rebuild_at && !v.plan?.direction}
            <!-- 重做前先挑设计方向：AI 画 3 个首页小样，挑一个再开工 -->
            {#if aiRunning}
              <div class="running"><span class="spin"></span><span class="rt"><b>AI 正在画 3 个设计方向</b>{#if aiMins}<small>已 {aiMins} 分钟，一般 5–10 分钟</small>{/if}</span>{#if conv}<button class="btn small" onclick={() => openConv(conv.id)}><Icon name="chat" size={12} />看它在做什么</button>{/if}</div>
              {#if aiDoing}<p class="hint doing">现在：{aiDoing}</p>{/if}
            {:else if !v.meta.directions_at || !dirs.length}
              <div class="act"><span class="grow">{v.meta.directions_at && !dirs.length ? 'AI 这一轮没画出设计方向，可以再让它画一次。' : '重做前先挑设计方向：AI 先画 3 个首页小样（大约 5–10 分钟），你挑一个，它再照着重做全站。'}</span>
                <button class="link-btn" onclick={() => pickDirection(site, 'auto')} data-tip="不挑了，让 AI 自己定设计方向直接开始重做">跳过，直接重做</button>
                <button class="btn small primary" onclick={() => askDirections(site)}><Icon name="wand" size={12} />让 AI 画 3 个方向</button>
              </div>
            {:else}
              <div class="act"><span class="grow"><b>挑一个设计方向</b>点「打开看看」在预览窗里看完整的首页，选定后 AI 照着它重做全站。</span></div>
              <div class="dirs">
                {#each dirs as d (d.id)}
                  <div class="dir">
                    <button class="shot" onclick={() => openDirection(site.id, d.id)} data-tip="在预览窗里打开这个方向的首页小样">
                      {#if d.shot}<img src={d.shot} alt={d.name} />{:else}<span class="noshot">{d.name}</span>{/if}
                    </button>
                    <div class="dmeta">
                      <b>{d.name}</b>
                      <small>{d.idea}</small>
                      {#if d.palette.length}<span class="swatches">{#each d.palette.slice(0, 5) as c (c)}<i style="background:{c}" title={c}></i>{/each}</span>{/if}
                      {#if d.fonts}<small class="faint">{d.fonts}</small>{/if}
                    </div>
                    <div class="dacts">
                      <button class="btn tiny" onclick={() => openDirection(site.id, d.id)}><Icon name="external" size={11} />打开看看</button>
                      <button class="btn tiny primary" onclick={() => pickDirection(site, d.id)}>用这个</button>
                    </div>
                  </div>
                {/each}
              </div>
              <div class="redo-dirs">
                <input class="tin" bind:value={dirFeedback} placeholder="都不满意？说说哪里不对，比如「太花了，想要更干净的」" />
                <button class="btn small" onclick={() => { void askDirections(site, dirFeedback); dirFeedback = ''; }} disabled={!dirFeedback.trim()}>重新画</button>
                <button class="link-btn" onclick={() => pickDirection(site, 'auto')}>跳过，让 AI 自己定</button>
              </div>
            {/if}
          {:else if !v.meta.rebuild_at}
            <div class="act"><span class="grow">{v.plan?.direction && v.plan.direction !== 'auto' ? `设计方向选好了（${dirs.find((d) => d.id === v.plan?.direction)?.name ?? v.plan.direction}）。` : ''}交给 AI 用 Astro 重做全站，文字一字不改、地址一个不变。</span><button class="btn small primary" onclick={() => startRebuild(site)}><Icon name="wand" size={12} />开始重做</button></div>
          {:else if aiRunning}
            <div class="running"><span class="spin"></span><span class="rt"><b>AI 正在重做</b>{#if aiMins}<small>已 {aiMins} 分钟{aiSteps ? ` · 做了 ${aiSteps} 步` : ''}</small>{/if}</span>{#if conv}<button class="btn small" onclick={() => openConv(conv.id)}><Icon name="chat" size={12} />看它在做什么</button>{/if}</div>
            {#if aiDoing}<p class="hint doing">现在：{aiDoing}</p>{/if}
            <p class="hint">整站重做一般要半小时到一两个小时，{v.plan?.auto ? '做完会自动核对。' : '做完后这里会出现「核对」。'}可以先去忙别的。</p>
          {:else}
            {#if aiStop}<div class="failed"><span class="lv block">!</span><span class="rt"><b>上一轮停下了</b><small>{aiStop}</small></span></div>{/if}
            {#if aiStop}
              <div class="act"><span class="grow">点「继续重做」会接着原来的对话做，已经做过的不会重来。</span>
                {#if conv}<button class="btn small" onclick={() => openConv(conv.id)}><Icon name="chat" size={12} />打开对话</button>{/if}
                <button class="btn small" onclick={() => verify(site.id)} disabled={jobRunning || !!busy}><Icon name="checks" size={12} />先核对看看</button>
                <button class="btn small primary" onclick={() => startRebuild(site)}><Icon name="play" size={12} />继续重做</button>
              </div>
            {:else}
              <div class="act"><span class="grow">AI 做完了{todos.length ? `，还留了 ${todos.length} 件事给你（见下面，不处理也能先核对）` : ''}。下一步核对：新站逐条对照旧站，有问题会列出来，可以再让 AI 修。{#if conv} <button class="link-btn" onclick={() => openConv(conv.id)}>看 AI 做了什么</button>{/if}</span>
                {#if built}<button class="btn small" onclick={() => previewSite(site)} data-tip="在预览窗里打开新站（本机，不用上线）"><Icon name="external" size={12} />本地预览</button>{/if}
                <button class="btn small" onclick={() => startRebuild(site)} data-tip="没做完、或者想再改点什么：接着原来的对话做"><Icon name="play" size={12} />让 AI 接着做</button>
                <button class="btn small primary" onclick={() => verify(site.id)} disabled={jobRunning || !!busy}><Icon name="checks" size={12} />核对</button>
              </div>
            {/if}
          {/if}
        {:else if conv}
          <div class="act sub"><span class="grow faint">重做对话里有 AI 做了什么、改了哪些文件。</span><button class="btn tiny" onclick={() => openConv(conv.id)}><Icon name="chat" size={11} />打开重做对话</button></div>
        {/if}
        {#if ex.low_coverage.length && isCurrent}<ul class="mini"><li><span class="lv warn">!</span><span>{ex.low_coverage.length} 页正文可能没抓全，AI 会对照旧页面补齐 <button class="link-btn" onclick={() => toggleItem('cov')}>{openItems.has('cov') ? '收起' : '查看'}</button></span></li></ul>{/if}
        {#if openItems.has('cov')}<ul class="items">{#each ex.low_coverage as l (l)}<li class="mono">{l}</li>{/each}</ul>{/if}
        <details class="req">
          <summary>交给 AI 的要求</summary>
          <ul class="mini">
            <li><span class="lv info">i</span><span>{ex.pages} 篇内容、{ex.lists} 个列表页和首页，每个旧地址原样生成，正文一字不改</span></li>
            {#if ex.features?.length}<li><span class="lv info">i</span><span>动态功能：{featNames(ex.features)}，按 Pilot 给的接口接到页面上</span></li>{/if}
            <li><span class="lv info">i</span><span>{v.plan?.visual === 'fresh' ? '全新视觉' : '保留品牌（logo、站名、主色），升级设计'}{v.plan?.direction && v.plan.direction !== 'auto' ? `，照你挑的设计方向做` : ''}；不用米黄纸色这类套路</span></li>
          </ul>
        </details>

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
                {#if !launched}<button class="btn small" class:primary={vNext === 'parity'} onclick={() => verify(site.id)} disabled={jobRunning || !!busy || aiRunning}><Icon name="play" size={12} />{v.meta.parity_at ? '重新核对' : '核对'}</button>{/if}
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
                  <small class="desc">选一台你的服务器（在 Pilot 的连接里加过的远程服务器）。Pilot 会装好网页服务和免费 HTTPS 证书{dynNeed ? '，再装一个处理表单和评论的小服务' : ''}，不用你懂运维。要能用管理员身份登录（root 或免密 sudo）；服务器上已经有别的网站占着时不会动它。</small>
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
                <span><b>准备预览地址</b>{#if v.deploy_ready}<small>{site.deploy.project}.pages.dev{dynNeed ? (dynReady ? ' · 动态功能已配置' : '') : ''}</small>{/if}</span>
                {#if !v.deploy_ready}
                  <small class="desc">在你的 Cloudflare 账号里建一个预览用的项目，新站先放在那里给你看，不影响现在的网站。</small>
                  {#if cfConns.length}
                    <div class="row wrap">
                      {#if cfConns.length > 1}<div class="dd"><Dropdown bind:value={cfConn} options={cfConns.map((c) => ({ value: c.id, label: c.name, sub: c.login || c.account_id }))} /></div>{/if}
                      <input class="tin proj" bind:value={project} placeholder="项目名" spellcheck="false" data-tip="预览地址会是 项目名.pages.dev" />
                      <button class="btn small" class:primary={vNext === 'deploy'} onclick={setupPreview} disabled={!!busy || !cfConn || !project.trim()}>{busy === 'pages' ? '创建中…' : '建好预览项目'}</button>
                    </div>
                  {:else}<small class="desc warn-text">还没有 Cloudflare 连接：先在设置里连接 Cloudflare。</small>{/if}
                {:else if dynNeed && !dynReady}
                  <small class="desc">保留了{featNames(v.dynamic?.features ?? [])}：还要在 Cloudflare 上建好存提交内容的数据库{v.dynamic?.features.includes('form') || v.dynamic?.features.includes('comments') ? '和防垃圾的人机验证' : ''}{v.dynamic?.large.length ? `，把 ${v.dynamic.large.length} 个大文件放到云存储` : ''}，Pilot 一键建好。（Cloudflare 令牌要有 D1、Turnstile{v.dynamic?.large.length ? '、R2' : ''} 的编辑权限）</small>
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
              <span><b>{isVps ? '发布到服务器，实测每个旧地址' : '部署到预览地址，实测每个旧地址'}</b>
                {#if v.meta.live.at}<small>{v.meta.live.bad.length ? `${v.meta.live.checked} 个里 ${v.meta.live.bad.length} 个不对` : `${v.meta.live.checked} 个旧地址都能正常打开或跳转`}</small>{/if}
              </span>
              {#if !v.meta.live.at && v.deploy_ready}<small class="desc">{deployedOk ? `已${isVps ? '发布' : '部署'}，还没实测：点「实测每个旧地址」（不用重新构建，一两分钟），都通过就到「正式上线」。` : isVps ? '发布到服务器后，Pilot 会在服务器上把每个旧地址打开一遍，不用先改域名的指向。' : `部署到 ${v.preview.replace(/^https?:\/\//, '')}（Cloudflare 自带的地址，只给你看），成功后 Pilot 会接着把每个旧地址实际打开一遍。`}{blocks.length ? '先把上面必须修的问题修好。' : ''}</small>{/if}
              {#if v.meta.live.bad.length}
                <button class="link-btn sm al" onclick={() => toggleItem('live')}>{openItems.has('live') ? '收起' : '查看不对的地址'}</button>
                {#if openItems.has('live')}<ul class="items">{#each v.meta.live.bad.slice(0, 80) as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
              {/if}
              {#if v.deploy_ready && (dynReady || isVps)}
                <div class="row">
                  {#if deployedOk && !v.meta.live.at}<button class="btn small" class:primary={vNext === 'live'} onclick={() => liveCheck(site.id)} disabled={jobRunning || !!busy}><Icon name="checks" size={12} />{runHere && job.kind === 'migrate:live' ? '实测中…' : '实测每个旧地址'}</button>{/if}
                  {#if !launched}<button class="btn small" class:primary={vNext === 'live' && !deployedOk} onclick={() => deploySite(site.id)} disabled={jobRunning || !!blocks.length}><Icon name="rocket" size={12} />{deploying ? (isVps ? '发布中…' : '部署中…') : deployedOk ? (isVps ? '重新发布' : '重新部署') : (isVps ? '发布到服务器' : '部署到预览地址')}</button>{/if}
                  {#if !isVps && (v.meta.live.base || lastUrl)}<button class="btn tiny" onclick={() => openUrl(v.meta.live.base || lastUrl)}><Icon name="external" size={11} />打开预览</button>{/if}
                </div>
              {/if}
            </div>
          </li>
        </ol>
        {#if launched && (warns.length || blocks.length)}
          <div class="act"><span class="grow">还有 {warns.length + blocks.length} 项提醒（上面列着）。可以交给 AI 改，改完用右上角的「构建并上线」更新线上的新站。</span><button class="btn small" onclick={() => startRebuild(site)} disabled={aiRunning}><Icon name="wand" size={12} />{aiRunning ? 'AI 正在改…' : '交给 AI 改'}</button></div>
        {:else if launched}
          <p class="hint">新站已经正式上线，以后改了内容用右上角的「构建并上线」更新。</p>
        {:else if vNext === 'fix'}
          <div class="act"><span class="grow">有问题要修：交给 AI，按上面的结果改完再点「重新核对」。</span><button class="btn small primary" onclick={() => startRebuild(site)} disabled={aiRunning}><Icon name="wand" size={12} />{aiRunning ? 'AI 正在修…' : '让 AI 修这些问题'}</button></div>
        {:else if vNext === 'done' && warns.length && isCurrent}
          <div class="act sub"><span class="grow faint">还有 {warns.length} 项提醒，不影响上线。</span><button class="btn tiny" onclick={() => startRebuild(site)} disabled={aiRunning}><Icon name="wand" size={11} />让 AI 也处理掉</button></div>
        {/if}

      {:else if shown === 'cutover'}
        {#if cutNext && isCurrent}
          <div class="act next" class:ready={cutNext.ready}>
            {#if cutNext.spin}<span class="spin"></span>{/if}
            <span class="grow"><b>{cutNext.ready ? '可以正式上线了' : '下一步'}</b>{cutNext.text}</span>
            {#if cutNext.ready}<button class="btn small primary" onclick={() => cutover(site.id, host, isVps, !!vd?.cf_auto, oldHost, oldOwned)} disabled={!!busy || jobRunning}><Icon name="rocket" size={12} />{busy === 'cutover' ? '上线中…' : '正式上线'}</button>
            {:else if cutNext.act === 'addcf'}<button class="btn small primary" onclick={() => addToCloudflare(site.id, host.replace(/^www\./, ''))} disabled={!!busy || !site.deploy.cf_conn_id}>接进 Cloudflare</button>
            {:else if cutNext.act === 'fix' && dns}<button class="btn small primary" onclick={() => fixDns(site.id, dns.missing.length)} disabled={!!busy}>补上 {dns.missing.length} 条</button>
            {:else if cutNext.act === 'own'}<button class="btn small" onclick={() => checkOwnership(site.id)} disabled={!!busy}>{busy === 'own' ? '检查中…' : '重新检查'}</button>{/if}
          </div>
        {/if}
        <div class="domains">
          <div class="dm">
            <span class="dk">正式域名</span>
            {#if editFormal}
              <small class="desc warn-text formal-warn">换正式域名等于整站搬家：新站会上到新域名，搜索引擎也会把它当成正式站。只是想先看看效果的话，不用换，用下面的预览地址或「在自己的域名下先看看」。</small>
              <input class="tin" bind:value={formalIn} placeholder={oldHost} spellcheck="false" autocapitalize="off" onkeydown={(e) => { if (e.key === 'Enter') void setFormal(site.id, formalIn, oldHost, oldOwned).then((ok) => { if (ok) editFormal = false; }); }} />
              <button class="btn small primary" onclick={() => setFormal(site.id, formalIn, oldHost, oldOwned).then((ok) => { if (ok) editFormal = false; })} disabled={!!busy}>保存</button>
              <button class="btn small quiet" onclick={() => (editFormal = false)}>取消</button>
            {:else}
              <b class="mono">{launched ? v.meta.cutover.host : host}</b>{#if !moved}<small class="faint">和旧站一样</small>{/if}
              {#if launched}<small class="faint">已经正式上线，要换域名先「撤下新站」</small>
              {:else if !isVps && !moved}<button class="link-btn" onclick={() => { formalIn = ''; editFormal = true; }}>要换成新域名？</button>{:else if !isVps}<button class="link-btn" onclick={() => { formalIn = host; editFormal = true; }}>修改</button>{/if}
              {#if moved && oldOwned && !launched}<button class="link-btn" onclick={() => setFormal(site.id, '', oldHost)}>改回 {oldHost}</button>{/if}
            {/if}
          </div>
          {#if moved && !launched}
            {#if !formalUse}<small class="desc">正在查 {host} 是不是你的、现在有没有在用…</small>
            {:else if !formalInCf && !dns?.at}<small class="desc">正在查 {host} 在不在你的 Cloudflare 账号里…</small>
            {:else if !formalInCf}<small class="desc warn-text">{host} 不在你的 Cloudflare 账号里：是你的域名就按下面第 2 条接进来；不是你的没法上线到它，换一个。</small>
            {:else}<small class="desc" class:warn-text={formalUse.records.length > 0 && !formalUse.ours}>{host} 在你的 Cloudflare 账号里 · {useText(formalUse).replace(`${host} `, '')}{formalUse.records.length && !formalUse.ours ? '：正式上线时会改为打开新站，那里原来的内容就打不开了' : ''}。</small>{/if}
            {#if oldOwned}<small class="desc warn-text">旧域名 {oldHost} 也是你的：新站稳定后，记得让它跳转到 {host}，原来的搜索排名才能带过来。</small>
            {:else}<small class="desc warn-text">旧站 {oldHost} 不在你的账号里，没法让它跳转过来：搜索排名要在 {host} 上重新积累；旧站要是一直开着，两边内容一样，搜索引擎可能只收录其中一个。</small>{/if}
          {/if}
          {#if !isVps && (v.meta.live.base || lastUrl)}
            <div class="dm"><span class="dk">预览地址</span><button class="link-btn mono" onclick={() => openUrl(v.meta.live.base || lastUrl)}>{(v.meta.live.base || lastUrl).replace(/^https?:\/\//, '').replace(/\/$/, '')}</button><small class="faint">Cloudflare 自带的地址，现在就能打开看新站 · 不让搜索引擎收录</small></div>
          {/if}
          {#if !launched && !isVps && !stg?.host && !showStaging}
            <div class="dm"><button class="link-btn" onclick={() => (showStaging = true)}>想在自己的域名下先看看？</button><small class="faint">可选：比如广告、统计、第三方嵌入只认你的域名时用</small></div>
          {:else if !isVps && (!launched || stg?.host)}
            <div class="dm">
              <span class="dk">测试域名<em>可选</em></span>
              {#if stg?.host}
                <button class="link-btn mono" onclick={() => openUrl(`https://${stg.host}`)}>{stg.host}</button>
                <small class="faint">{stg.status !== 'active' ? '正在生效（Cloudflare 验证域名、签证书，一般几分钟，好了自动实测）' : stg.live.at ? (stg.live.bad.length ? `${stg.live.checked} 个里 ${stg.live.bad.length} 个不对` : `已生效 · ${stg.live.checked} 个旧地址都正常 · 不让搜索引擎收录`) : '已生效 · 正在实测…'}</small>
                <span class="grow"></span>
                {#if stg.status === 'active'}<button class="btn tiny" onclick={() => stagingCheck(site.id)} disabled={!!busy || jobRunning}>{runHere && job.kind === 'migrate:staging' ? '实测中…' : '重新实测'}</button>{:else}<span class="spin"></span>{/if}
                <button class="btn tiny quiet" onclick={() => stagingRemove(site.id, stg.host)} disabled={!!busy || jobRunning}>删除</button>
              {:else}
                <input class="tin" bind:value={stagingIn} placeholder={`new.${host.replace(/^www\./, '')}`} spellcheck="false" autocapitalize="off" />
                <button class="btn small" onclick={() => stagingUp(site.id, stagingIn || `new.${host.replace(/^www\./, '')}`)} disabled={!!busy || jobRunning}>{busy === 'staging' ? '部署中…' : '部署到测试域名'}</button>
                <button class="btn small quiet" onclick={() => (showStaging = false)} disabled={busy === 'staging'}>收起</button>
              {/if}
            </div>
            {#if !stg?.host}<small class="desc">在一个没在用的子域名上打开新站，旧站不受影响，页面不让搜索引擎收录。</small>{/if}
            {#if stg?.live.bad.length}
              <button class="link-btn sm al" onclick={() => toggleItem('stg')}>{openItems.has('stg') ? '收起' : '查看不对的地址'}</button>
              {#if openItems.has('stg')}<ul class="items">{#each stg.live.bad.slice(0, 80) as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
            {/if}
          {/if}
        </div>
        <ol class="cut">
          {#if moved}
          <li class:ok={mine}>
            <span class="lv">{mine ? '✓' : '1'}</span>
            <div class="cb">
              <span><b>确认 {host} 是你的</b>{#if mine}<small>在你的 Cloudflare 账号里</small>{/if}</span>
              {#if !mine}<small class="desc">能接进你的 Cloudflare 账号，就说明域名是你的（下面第 2 条会带你做）。不是你的域名没法上线到它。</small>{/if}
            </div>
          </li>
          {:else}
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
          {/if}
          {#if isVps}
          <li class:ok={!!vd?.points}>
            <span class="lv">{vd?.points ? '✓' : '2'}</span>
            <div class="cb">
              <span><b>域名指向服务器 {v.vps?.ip}</b>{#if vd?.points}<small>已指向</small>{:else if vd?.cf_auto}<small>域名在你的 Cloudflare 账号里，正式上线时 Pilot 自动改</small>{/if}</span>
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
              <span><b>域名交给 Cloudflare 管理</b>{#if dns?.zone_status === 'active'}<small>{apexOf(host)} 已经在你的 Cloudflare 账号里</small>{:else if dns?.zone_id}<small>已加进 Cloudflare，等你改名称服务器</small>{/if}</span>
              {#if dns?.zone_status !== 'active'}<small class="desc">正式上线就是在 Cloudflare 里把 {host} 改成指向新站，所以这个域名得由你的 Cloudflare 账号管理。</small>{/if}
              {#if !dns?.at}
                <div class="row"><span class="spin"></span><small class="faint">正在查 {apexOf(host)} 现在由谁管理，顺便记下现有的设置（邮件等），上线时不会弄丢</small></div>
              {:else if !dns.zone_id}
                <small class="desc">{apexOf(host)} 现在不在你的 Cloudflare 账号里。点「接进 Cloudflare」，Pilot 会把它加进去，并把现有的设置（邮件等）原样搬过去；之后只剩一件事要你做：去买域名的地方改两项「名称服务器」。</small>
                <div class="row"><button class="btn tiny" onclick={() => addToCloudflare(site.id, apexOf(host))} disabled={!!busy || !site.deploy.cf_conn_id}>{busy === 'dns' ? '处理中…' : '接进 Cloudflare'}</button><button class="btn tiny quiet" onclick={() => prepareDns(site.id)} disabled={!!busy}>重新检查</button></div>
              {:else}
                {#if dns.missing.length}
                  <small class="desc warn-text">还有 {dns.missing.length} 项现有设置没搬进 Cloudflare，不补上的话{dns.missing.some((r) => r.kind === 'MX') ? '邮件会收不到' : '这些子域名会打不开'}：</small>
                  <ul class="items">{#each dns.missing as r (r.kind + r.name + r.content)}<li class="mono">{r.kind} {r.name} → {r.priority != null ? `${r.priority} ` : ''}{r.content}</li>{/each}</ul>
                  <div class="row"><button class="btn tiny" onclick={() => fixDns(site.id, dns.missing.length)} disabled={!!busy}>补上 {dns.missing.length} 项</button></div>
                {/if}
                {#if dns.zone_status !== 'active'}
                  <small class="desc">去买域名的地方（域名注册商的后台，比如阿里云、GoDaddy、Namecheap），找到「名称服务器 / DNS 服务器 / Nameservers」，改成下面两个：</small>
                  <div class="ns">{#each dns.ns as n (n)}<code>{n}</code>{/each}</div>
                  <small class="faint">{dns.current_ns.length ? `现在是：${dns.current_ns.join('、')}。` : ''}改完一般几分钟到几小时生效，期间网站和邮件照常可用。Pilot 每分钟自动查一次，不用守着。</small>
                {/if}
              {/if}
            </div>
          </li>
          {/if}
          <li class:ok={launched}>
            <span class="lv">{launched ? '✓' : '3'}</span>
            <div class="cb">
              {#if launched}<span><b>正式上线</b><small>{ago(v.meta.cutover.at)}上线到 {v.meta.cutover.host}</small></span>{:else}
              <span><b>正式上线</b><small>{isVps ? `${host} 指向你的服务器，自动申请免费的 HTTPS 证书` : `${host} 打开新站`}{goneText}{!moved ? '。旧站的服务器先别关，新站稳定后再停' : ''}</small></span>
              {#if !cutNext?.ready}<small class="desc">前面两条都好了，最上面会出现「正式上线」。</small>{/if}{/if}
            </div>
          </li>
        </ol>

      {:else if shown === 'track'}
        {@const t = v.meta.track}
        {@const liveHost = v.meta.cutover.host}
        {@const movedLive = liveHost.replace(/^www\./, '') !== oldHost.replace(/^www\./, '')}
        {@const rd = v.meta.old_redirect}
        <div class="act"><span class="grow">{v.stage === 'done' ? '观察满 4 周了，改版完成。' : `${ago(v.meta.cutover.at) === '刚刚' ? '新站刚上线' : `新站已经上线 ${ago(v.meta.cutover.at).replace(/前$/, '')}`}。Pilot 会盯 4 周：旧地址在新站上能不能打开、搜索流量有没有掉；打开站点时超过 12 小时没查过会自动查一次。`}{#if t.at}<small class="faint"> 上次检查 <span data-tip={full(t.at)}>{ago(t.at)}</span></small>{/if}</span>
          <button class="btn small" onclick={() => track(site.id)} disabled={jobRunning || !!busy} data-tip="马上把旧地址在新站上打开一遍，再看一下搜索流量"><Icon name="play" size={12} />{runHere && job.kind === 'migrate:track' ? '检查中…' : '现在查一次'}</button>
        </div>
        {#if movedLive && oldOwned}
          <div class="act" class:ok-bar={rd.at > 0 && rd.verified}>
            {#if rd.at}
              <span class="grow"><b>{oldHost.replace(/^www\./, '')} 已经跳到 {liveHost}</b>{rd.verified ? '：打开旧域名的任何地址，都会带到新站的同一地址' : `：还在生效中${rd.note ? `（${rd.note}）` : ''}`}</span>
              <button class="link-btn" onclick={() => oldRedirectUndo(site.id, oldHost.replace(/^www\./, ''))} disabled={!!busy}>取消跳转</button>
            {:else}
              <span class="grow"><b>旧域名 {oldHost.replace(/^www\./, '')} 还在打开旧站</b>新站稳定了，就让它跳到 {liveHost}：原来的搜索排名才能带过来，访客也不会看到两个站。</span>
              <button class="btn small" onclick={() => oldRedirect(site.id, oldHost.replace(/^www\./, ''), liveHost)} disabled={!!busy || jobRunning}>{busy === 'redirect' ? '设置中…' : `让 ${oldHost.replace(/^www\./, '')} 跳到新站`}</button>
            {/if}
          </div>
        {/if}
        <ul class="mini">
          {#each v.meta.cutover.notes.filter((n) => !(movedLive && oldOwned && /^旧域名 /.test(n))) as n (n)}<li><span class="lv info">i</span><span>{n}</span></li>{/each}
          {#if t.at}
            <li><span class="lv {t.bad.length ? 'block' : 'ok'}">{t.bad.length ? '✕' : '✓'}</span><span>{t.bad.length ? `${t.checked} 个旧地址里 ${t.bad.length} 个在正式域名上不对` : `${t.checked} 个旧地址在正式域名上都正常`}{#if t.bad.length} <button class="link-btn" onclick={() => toggleItem('tb')}>{openItems.has('tb') ? '收起' : '查看'}</button>{/if}</span></li>
            {#if openItems.has('tb')}<ul class="items">{#each t.bad.slice(0, 80) as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
            {#if t.after_days}<li><span class="lv {t.base_clicks > 5 && t.after_clicks < t.base_clicks * 0.7 ? 'warn' : 'ok'}">{t.base_clicks > 5 && t.after_clicks < t.base_clicks * 0.7 ? '!' : '✓'}</span><span>搜索点击：上线前每天 {t.base_clicks.toFixed(1)} 次，上线后每天 {t.after_clicks.toFixed(1)} 次（{t.after_days} 天）</span></li>{/if}
            <li><span class="lv {t.sitemap_submitted && t.indexnow_submitted ? 'ok' : 'info'}">{t.sitemap_submitted && t.indexnow_submitted ? '✓' : 'i'}</span><span>{t.sitemap_submitted && t.indexnow_submitted ? '已经通知搜索引擎来收录新站（Google、Bing 都提交了）' : `通知搜索引擎：Google ${t.sitemap_submitted ? '已提交' : '还没提交'} · Bing ${t.indexnow_submitted ? '已提交' : '等验证生效后提交'}`}</span></li>
            {#each t.notes as n (n)}<li><span class="lv warn">!</span><span>{n}</span></li>{/each}
          {/if}
        </ul>
        <p class="hint">接下来的内容更新可以交给托管运营：在下面的「托管」里加入一个托管方案。</p>
        <div class="act sub danger-row"><span class="grow faint">{movedLive ? `新站有问题？可以先撤下，${liveHost} 恢复成上线前的样子。` : `新站有问题？可以退回旧站，${liveHost} 马上恢复成上线前的设置。`}</span>
          <button class="link-btn danger-link" onclick={() => rollback(site.id, liveHost, movedLive)} disabled={jobRunning || !!busy}><Icon name="undo" size={11} />{movedLive ? '撤下新站' : '退回旧站'}</button>
        </div>
      {/if}

      {#if todos.length && !aiRunning && isCurrent && ['rebuild', 'verify', 'cutover'].includes(shown)}
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
  .req summary { cursor: pointer; font-size: 12px; color: var(--faint); }
  .req summary:hover { color: var(--text); }
  .req[open] summary { margin-bottom: 6px; }
  .domains { display: flex; flex-direction: column; gap: 8px; padding: 12px 14px; border-radius: 10px; background: var(--rail); }
  .dm { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; font-size: 12.5px; min-height: 28px; }
  .dm .dk { flex: none; width: 64px; color: var(--dim); }
  .formal-warn { flex-basis: 100%; order: -1; margin-bottom: 2px; }
  .dm .dk em { display: block; font-style: normal; font-size: 10.5px; color: var(--faint); }
  .act.next { background: var(--rail); }
  .act.next b { margin-right: 8px; color: var(--text); font-weight: 600; }
  .act.next.ready { background: var(--su-ok-soft); }
  .act.next .spin { flex: none; }
  .dm b { font-weight: 600; font-size: 13px; }
  .dm .tin { flex: 1; min-width: 180px; max-width: 320px; height: 26px; font-size: 12.5px; }
  .dm .grow { flex: 1; }
  .domains .desc { font-size: 12px; color: var(--faint); line-height: 1.55; }
  .warn-text, .domains .desc.warn-text, .cb .desc.warn-text { color: var(--su-warn-ink, #8a5a08); }
  .act.ok-bar { background: var(--su-ok-soft); }
  .act b { margin-right: 6px; color: var(--text); font-weight: 600; }
  .danger-row { margin-top: 2px; }
  .danger-link { color: var(--err); font-size: 12px; display: inline-flex; align-items: center; gap: 4px; }
  .autobar { display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--faint); margin: -4px 0 2px; }
  .autobar.on { color: var(--dim); }
  .autobar .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 0 3px var(--su-ok-soft); flex: none; }
  .autobar .dot.wait { background: var(--warn, #c98a1b); box-shadow: 0 0 0 3px #faf1d8; }
  .dirs { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; }
  .dir { display: flex; flex-direction: column; gap: 8px; border: 1px solid var(--border); border-radius: 12px; padding: 8px; background: var(--card, #fff); }
  .dir .shot { display: block; padding: 0; border: none; border-radius: 8px; overflow: hidden; aspect-ratio: 16 / 10; background: var(--rail); cursor: zoom-in; }
  .dir .shot img { width: 100%; height: 100%; object-fit: cover; object-position: top; display: block; }
  .dir .noshot { display: flex; height: 100%; align-items: center; justify-content: center; font-size: 13px; color: var(--faint); }
  .dmeta { display: flex; flex-direction: column; gap: 3px; padding: 0 4px; }
  .dmeta b { font-size: 13px; }
  .dmeta small { font-size: 12px; color: var(--dim); line-height: 1.5; }
  .swatches { display: flex; gap: 4px; margin-top: 2px; }
  .swatches i { width: 16px; height: 16px; border-radius: 4px; box-shadow: inset 0 0 0 1px rgba(0, 0, 0, .1); }
  .dacts { display: flex; gap: 6px; justify-content: flex-end; padding: 0 4px 2px; }
  .redo-dirs { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .redo-dirs .tin { flex: 1; min-width: 220px; height: 26px; font-size: 12.5px; }
  .hint.doing { margin-top: -4px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .todo-box { display: flex; flex-direction: column; gap: 8px; border-top: 1px solid var(--border); padding-top: 12px; }
  .todo-box > b { font-size: 12.5px; font-weight: 600; }
  .todo-box > b small { margin-left: 6px; font-weight: 400; color: var(--faint); }
  .link-btn.sm { font-size: 12px; }
  .link-btn.al { align-self: flex-start; }
  .kv { font-size: 12.5px; }
  .mg h3 .sub { font-weight: 400; font-size: 12px; color: var(--faint); }
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
  .proj { width: 180px !important; height: 26px !important; }
  .cut > li { padding: 10px 0; border-top: 1px solid var(--border); }
  .cut > li:first-child { border-top: none; padding-top: 2px; }
  .txt { display: flex; flex-direction: column; gap: 3px; padding: 7px 10px; border-radius: 8px; background: var(--rail); }
  .txt code { font-size: 12px; user-select: all; }
  .ns { display: flex; gap: 8px; flex-wrap: wrap; }
  .ns code { font-size: 12px; padding: 2px 6px; user-select: all; }
  .bars { display: flex; flex-direction: column; gap: 8px; }
  .bars i { display: block; height: 10px; border-radius: 6px; background: #f1efe9; }
</style>
