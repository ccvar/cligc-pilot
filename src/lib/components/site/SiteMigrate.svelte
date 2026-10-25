<script lang="ts">
  // 迁移进度卡：盘点 → 确认方案 → 抓取 → 重做 → 核对 → 切换 → 跟踪。每一步的结果和下一步的按钮都在这里。
  // 迁移期间站点的 domain 是空的：预览部署只建 Pages 项目、不绑域名，切换时才把域名指过来。
  import './site.css';
  import Icon from './Icon.svelte';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import MigrateSurvey from './MigrateSurvey.svelte';
  import type { Site } from '$lib/types';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { ago, full } from './fmt';
  import { app, say } from '$lib/state/app.svelte.js';
  import { sites, deploySite, refreshSites } from '$lib/state/sites.svelte.js';
  import { openConv } from '$lib/state/conversations.js';
  import { makingConv } from '$lib/state/siteMake.svelte.js';
  import { migrate, STAGES, loadMigrate, resurvey, extract, startRebuild, verify, checkOwnership, prepareDns, addToCloudflare, fixDns, cutover, rollback, track, trackIfStale, setupDynamic, setupVps, vpsDns, checkVpsDns } from '$lib/state/migrate.svelte.js';
  import { FEATURE_LABEL } from '$lib/state/dynamic.svelte.js';

  let { site }: { site: Site } = $props();
  const v = $derived(migrate.view[site.id]);
  const loading = $derived(!!migrate.loading[site.id]);
  const busy = $derived(migrate.busy[site.id] ?? '');
  const job = $derived(sites.jobs[site.id]);
  const jobRunning = $derived(job?.status === 'running');
  const migJob = $derived(jobRunning && job.kind.startsWith('migrate:') ? job : null);
  const deploying = $derived(jobRunning && job.kind === 'deploy');
  const conv = $derived(makingConv(site.id));
  const aiRunning = $derived(app.convos.some((c) => c.conn_id === `site:${site.id}` && c.status === 'running'));
  const stageIdx = $derived(v ? Math.max(0, STAGES.findIndex((s) => s.key === (v.stage === 'done' ? 'track' : v.stage))) : 0);
  const cur = $derived(migJob?.steps.find((s) => s.status === 'running'));
  const blocks = $derived(v?.meta.parity.filter((c) => c.level === 'block') ?? []);
  const warns = $derived(v?.meta.parity.filter((c) => c.level === 'warn') ?? []);
  const own = $derived(v?.survey?.ownership);
  const dns = $derived(v?.meta.dns);
  const host = $derived(v?.survey ? v.survey.origin.replace(/^https?:\/\//, '') : v?.domain ?? '');
  const cfConns = $derived(app.conns.filter((c) => c.kind === 'cloudflare'));
  let cfConn = $state('');
  let project = $state('');
  $effect(() => { if (!cfConn && cfConns.length) cfConn = site.deploy.cf_conn_id || cfConns[0].id; });
  $effect(() => { if (!project) project = (site.deploy.project || (site.migrate_from ?? site.name).replace(/^www\./, '').split('.')[0]).toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-+|-+$/g, '') + '-new'; });
  let showSurvey = $state(false);
  let openItems = $state<Set<string>>(new Set());
  function toggleItem(k: string) { const n = new Set(openItems); if (n.has(k)) n.delete(k); else n.add(k); openItems = n; }

  $effect(() => { void loadMigrate(site.id); });
  $effect(() => { if (v?.stage === 'track') trackIfStale(site.id); });
  // AI 这一轮做完：刷新一下（重做阶段的按钮跟着变）
  let wasRunning = false;
  $effect(() => { const r = aiRunning; if (wasRunning && !r) void loadMigrate(site.id); wasRunning = r; });

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
</script>

<div class="card mg">
  <h3>
    <Icon name="route" />迁移进度<span class="sub">{v?.domain ?? site.migrate_from ?? ''}</span>
    <span class="grow"></span>
    {#if aiRunning}<span class="live"><span class="spin"></span>AI 正在重做…</span>{/if}
    {#if conv && v && stageIdx >= 3}<button class="btn tiny" onclick={() => openConv(conv.id)}><Icon name="chat" size={12} />重做对话</button>{/if}
    <button class="ic-btn" onclick={() => loadMigrate(site.id)} disabled={loading} aria-label="刷新" data-tip="刷新迁移状态"><RefreshIcon spinning={loading} /></button>
  </h3>

  <ol class="stages">
    {#each STAGES as s, i (s.key)}
      <li class:done={i < stageIdx || v?.stage === 'done'} class:on={i === stageIdx && v?.stage !== 'done'}><span class="n">{i < stageIdx || v?.stage === 'done' ? '✓' : i + 1}</span>{s.label}</li>
    {/each}
  </ol>

  {#if migJob}
    <div class="running"><span class="spin"></span><span><b>正在{migJob.title}</b>{#if cur} · {cur.label}{#if cur.detail}<small>{cur.detail}</small>{/if}{/if}</span></div>
  {/if}

  {#if !v}
    <div class="bars"><i style="width:58%"></i><i style="width:34%"></i></div>
  {:else if v.stage === 'survey'}
    {#if !migJob}
      <div class="resume"><span>盘点还没完成{job?.kind === 'migrate:survey' && job.status === 'failed' ? `：${job.error.split('\n')[0]}` : ''}。</span><button class="btn small primary" onclick={() => resurvey(site.id)} disabled={!!busy || jobRunning}><Icon name="play" size={12} />开始盘点</button></div>
    {/if}
  {:else if v.stage === 'plan' && v.survey}
    <p class="lead">盘点好了。看一下报告，勾掉不想迁的页面组，选好新站的样子，确认后开始抓取内容。</p>
    <MigrateSurvey id={site.id} {v} editable={!jobRunning} />
  {:else}
    {#if v.survey}
      <div class="sec first"><b>盘点报告</b><small>{v.survey.pages} 个网页 · {v.plan?.deploy === 'vps' ? '部署到自己的服务器' : (v.hosting ?? v.survey.hosting).label} · <span data-tip={full(v.survey.finished_at)}>{ago(v.survey.finished_at)}</span></small><span class="grow"></span><button class="fold" onclick={() => (showSurvey = !showSurvey)}>{showSurvey ? '收起' : '展开'}</button></div>
      {#if showSurvey}<MigrateSurvey id={site.id} {v} editable={false} />{/if}
    {/if}

    {#if v.stage === 'extract' && !migJob}
      <div class="resume"><span>方案已确认，还没抓取内容{job?.kind === 'migrate:extract' && job.status === 'failed' ? `（上次失败：${job.error.split('\n')[0]}）` : ''}。</span><button class="btn small primary" onclick={() => extract(site.id)} disabled={!!busy || jobRunning}><Icon name="play" size={12} />开始抓取</button></div>
    {/if}

    {#if v.meta.extracted_at}
      {@const ex = v.meta.extract}
      <div class="sec"><b>抓取</b><small>{ex.pages} 篇内容 · {ex.lists} 个列表页和首页 · {ex.assets} 个图片和附件{ex.comments ? ` · ${ex.comments} 条旧评论` : ''}{ex.large?.length ? ` · ${ex.large.length} 个大文件放 R2` : ''}{ex.features?.length ? ` · 动态功能：${featNames(ex.features)}` : ''} · <span data-tip={full(v.meta.extracted_at)}>{ago(v.meta.extracted_at)}</span></small><span class="grow"></span>{#if v.stage === 'rebuild' || v.stage === 'verify'}<button class="btn tiny quiet" onclick={() => extract(site.id)} disabled={!!busy || jobRunning} data-tip="按确认的方案重新抓一遍（会覆盖抓下来的内容文件）">重新抓取</button>{/if}</div>
      <!-- 抓取时的问题只在重做、核对阶段有用；核对通过以后不再提 -->
      {#if (v.stage === 'rebuild' || v.stage === 'verify') && (ex.low_coverage.length || ex.assets_failed.length)}
        <ul class="mini">
          {#if ex.low_coverage.length}<li><span class="lv warn">!</span><span>{ex.low_coverage.length} 页正文可能没抓全，已列给 AI 对照旧页面补齐 <button class="link-btn" onclick={() => toggleItem('cov')}>{openItems.has('cov') ? '收起' : '查看'}</button></span></li>{#if openItems.has('cov')}<ul class="items">{#each ex.low_coverage as l (l)}<li class="mono">{l}</li>{/each}</ul>{/if}{/if}
          {#if ex.assets_failed.length}<li><span class="lv warn">!</span><span>{ex.assets_failed.length} 个图片 / 附件没下载下来 <button class="link-btn" onclick={() => toggleItem('af')}>{openItems.has('af') ? '收起' : '查看'}</button></span></li>{#if openItems.has('af')}<ul class="items">{#each ex.assets_failed as l (l)}<li class="mono">{l}</li>{/each}</ul>{/if}{/if}
        </ul>
      {/if}
    {/if}

    {#if v.stage === 'rebuild'}
      <div class="next">
        <span>{v.meta.rebuild_at ? (aiRunning ? 'AI 正在重做，做完后点「核对」：构建新站，逐条对照旧站。' : 'AI 做完了就点「核对」：构建新站，逐条对照旧站的地址、标题、正文。') : '内容都抓好了。下一步交给 AI 用 Astro 重做全站：URL 一个不变、正文一字不改，只换外观。'}</span>
        {#if !v.meta.rebuild_at}<button class="btn small primary" onclick={() => startRebuild(site)}><Icon name="wand" size={12} />开始重做</button>
        {:else}
          {#if !aiRunning}<button class="btn small" onclick={() => startRebuild(site)}><Icon name="play" size={12} />继续重做</button>{/if}
          <button class="btn small primary" onclick={() => verify(site.id)} disabled={aiRunning || jobRunning || !!busy}><Icon name="checks" size={12} />核对</button>
        {/if}
      </div>
    {/if}

    {#if v.meta.parity_at && (v.stage === 'verify' || v.stage === 'cutover')}
      <div class="sec"><b>核对</b>
        {#if !blocks.length && !warns.length}<span class="pass"><Icon name="check" size={11} />对照旧站全部通过</span>{:else}<small>{blocks.length} 项必须修 · {warns.length} 项提醒</small>{/if}
        <small data-tip={full(v.meta.parity_at)}>{ago(v.meta.parity_at)}</small>
        <span class="grow"></span>
        <button class="btn tiny" onclick={() => verify(site.id)} disabled={jobRunning || !!busy || aiRunning}><Icon name="play" size={11} />重新核对</button>
      </div>
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

      <!-- 预览地址实测 -->
      <div class="sub-sec">
        <b>{isVps ? '服务器上实测' : '预览地址实测'}</b>
        {#if isVps}
          {#if !v.vps?.setup_at}
            <p class="hint">选一台服务器（SSH 连接）。Pilot 会装 Caddy（网页和自动 HTTPS）{dynNeed ? '，再装一个 Go 写的小服务处理表单和评论（数据存 SQLite）' : ''}。需要这个账号是 root 或能免密 sudo；80 / 443 端口被别的网站服务占着时不会去动它。</p>
            {#if sshConns.length}
              <div class="row wrap">
                <div class="dd"><Dropdown bind:value={sshConn} options={sshConns.map((c) => ({ value: c.id, label: c.name, sub: `${c.ssh_user}@${c.ssh_host}` }))} /></div>
                <button class="btn small primary" onclick={() => setupVps(site.id, sshConn)} disabled={!!busy || jobRunning || !sshConn}>{vpsSetting ? '配置中…' : '配置服务器'}</button>
              </div>
            {:else}<p class="hint warn-text">还没有远程连接：先在设置里新建一条 SSH 连接。</p>{/if}
          {:else if !v.meta.live.at}
            <p class="dyn-ok"><Icon name="check" size={12} />服务器已配置 · {v.vps.ip} · {v.vps.os}{v.vps.api_port ? ' · 表单和评论服务' : ''} <button class="link-btn" onclick={() => setupVps(site.id, site.deploy.ssh_conn_id)} disabled={jobRunning || !!busy}>重新配置</button></p>
            {#each v.vps.notes as n (n)}<p class="hint warn-text">{n}</p>{/each}
            <div class="row"><span class="hint grow">发布到服务器，然后点「重新核对」：Pilot 在服务器上带域名请求每个旧地址，不用先改 DNS。</span><button class="btn small" onclick={() => deploySite(site.id)} disabled={jobRunning || !!blocks.length}><Icon name="rocket" size={12} />{deploying ? '发布中…' : '发布到服务器'}</button></div>
            {#if blocks.length}<p class="hint">先把上面必须修的问题修好再发布。</p>{/if}
          {:else}
            <div class="row">
              {#if v.meta.live.bad.length}<span class="lv block">✕</span><span class="grow">{v.meta.live.checked} 个地址里 {v.meta.live.bad.length} 个不对 <button class="link-btn" onclick={() => toggleItem('live')}>{openItems.has('live') ? '收起' : '查看'}</button></span>
              {:else}<span class="lv ok">✓</span><span class="grow">{v.meta.live.checked} 个旧地址在服务器上都能正常打开或跳转</span>{/if}
              <button class="btn tiny" onclick={() => deploySite(site.id)} disabled={jobRunning}><Icon name="rocket" size={11} />{deploying ? '发布中…' : '重新发布'}</button>
            </div>
            {#if openItems.has('live')}<ul class="items">{#each v.meta.live.bad.slice(0, 80) as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
          {/if}
        {:else if !v.deploy_ready}
          <p class="hint">还要在预览地址上把每个旧地址实际打开一遍（Cloudflare 的跳转规则只有上线后才生效）。先建一个 Pages 项目——只用来预览，不会动线上域名。</p>
          {#if cfConns.length}
            <div class="row wrap">
              <div class="dd"><Dropdown bind:value={cfConn} options={cfConns.map((c) => ({ value: c.id, label: c.name, sub: c.login || c.account_id }))} /></div>
              <input class="tin proj" bind:value={project} placeholder="Pages 项目名" spellcheck="false" />
              <button class="btn small" onclick={setupPreview} disabled={!!busy || !cfConn || !project.trim()}>{busy === 'pages' ? '创建中…' : '创建 Pages 项目'}</button>
            </div>
          {:else}<p class="hint warn-text">还没有 Cloudflare 连接：先在设置里连接 Cloudflare。</p>{/if}
        {:else if dynNeed && !dynReady && !isVps}
          <p class="hint">保留了{featNames(v.dynamic?.features ?? [])}：还要在 Cloudflare 上建数据库（D1）{v.dynamic?.features.includes('form') || v.dynamic?.features.includes('comments') ? '和人机验证（Turnstile）' : ''}{v.dynamic?.large.length ? `，把 ${v.dynamic.large.length} 个大文件传到 R2` : ''}，绑到这个 Pages 项目。</p>
          <div class="row"><button class="btn small primary" onclick={() => setupDynamic(site.id)} disabled={jobRunning || !!busy}>{dynSetting ? '配置中…' : '配置动态功能'}</button><small class="faint">令牌要有 D1、Turnstile{v.dynamic?.large.length ? '、R2' : ''} 的编辑权限</small></div>
        {:else if !v.meta.live.at}
          {#if dynNeed}<p class="dyn-ok"><Icon name="check" size={12} />动态功能已配置{v.dynamic?.turnstile_sitekey ? '' : '（没开人机验证）'} <button class="link-btn" onclick={() => setupDynamic(site.id)} disabled={jobRunning || !!busy}>重新配置</button></p>{/if}
          {#each v.dynamic?.notes ?? [] as n (n)}<p class="hint">{n}</p>{/each}
          <div class="row"><span class="hint grow">上线到 <code>{v.preview.replace(/^https?:\/\//, '')}</code>，然后点「重新核对」。</span><button class="btn small" onclick={() => deploySite(site.id)} disabled={jobRunning || !!blocks.length}><Icon name="rocket" size={12} />{deploying ? '上线中…' : '上线到预览地址'}</button></div>
          {#if blocks.length}<p class="hint">先把上面必须修的问题修好再上线。</p>{/if}
        {:else}
          <div class="row">
            {#if v.meta.live.bad.length}<span class="lv block">✕</span><span class="grow">{v.meta.live.checked} 个地址里 {v.meta.live.bad.length} 个不对 <button class="link-btn" onclick={() => toggleItem('live')}>{openItems.has('live') ? '收起' : '查看'}</button></span>
            {:else}<span class="lv ok">✓</span><span class="grow">{v.meta.live.checked} 个旧地址在预览地址上都能正常打开或跳转</span>{/if}
            <button class="btn tiny" onclick={() => openUrl(v.meta.live.base)}><Icon name="external" size={11} />打开预览</button>
            <button class="btn tiny" onclick={() => deploySite(site.id)} disabled={jobRunning}><Icon name="rocket" size={11} />{deploying ? '上线中…' : '重新上线'}</button>
          </div>
          {#if openItems.has('live')}<ul class="items">{#each v.meta.live.bad.slice(0, 80) as b (b)}<li class="mono">{b}</li>{/each}</ul>{/if}
        {/if}
      </div>

      {#if v.stage === 'verify' && (blocks.length || warns.length || v.meta.live.bad.length)}
        <div class="foot"><button class="btn small primary" onclick={() => startRebuild(site)} disabled={aiRunning}><Icon name="wand" size={12} />让 AI 修这些问题</button><small class="faint">修完再点「重新核对」</small></div>
      {/if}
    {/if}

    {#if v.stage === 'cutover'}
      <div class="sec"><b>切换</b><small>把 {host} 指向新站</small></div>
      <ol class="cut">
        <li class:ok={own?.verified}>
          <span class="lv">{own?.verified ? '✓' : '1'}</span>
          <div class="cb">
            <span><b>确认这个站是你的</b>{#if own?.verified}<small>{OWN_BY[own.by] ?? '已确认'}</small>{/if}</span>
            {#if !own?.verified}
              <small class="desc">满足一条即可：域名在你的 Cloudflare 账号里；或者 Search Console 里验证过这个站；或者在域名的 DNS 里加一条 TXT 记录：</small>
              <div class="txt"><code>{own?.txt_token}</code><small>主机 <code>{host.replace(/^www\./, '')}</code>（根域）· 类型 TXT</small></div>
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
              <div class="row"><button class="btn tiny primary" onclick={() => addToCloudflare(site.id, host.replace(/^www\./, ''))} disabled={!!busy || !site.deploy.cf_conn_id}>{busy === 'dns' ? '处理中…' : '接进 Cloudflare'}</button><button class="btn tiny" onclick={() => prepareDns(site.id)} disabled={!!busy}>重新检查</button></div>
            {:else}
              {#if dns.missing.length}
                <small class="desc warn-text">原来的 {dns.missing.length} 条记录 Cloudflare 里还没有，改名称服务器前要补上，不然{dns.missing.some((r) => r.kind === 'MX') ? '邮件会收不到' : '这些子域名会打不开'}：</small>
                <ul class="items">{#each dns.missing as r (r.kind + r.name + r.content)}<li class="mono">{r.kind} {r.name} → {r.priority != null ? `${r.priority} ` : ''}{r.content}</li>{/each}</ul>
                <div class="row"><button class="btn tiny primary" onclick={() => fixDns(site.id, dns.missing.length)} disabled={!!busy}>补上 {dns.missing.length} 条</button></div>
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
    {/if}

    {#if v.stage === 'track' || v.stage === 'done'}
      {@const t = v.meta.track}
      <div class="sec"><b>{v.stage === 'done' ? '迁移完成' : '切换后跟踪'}</b><small>切换于 <span data-tip={full(v.meta.cutover.at)}>{ago(v.meta.cutover.at)}</span>{t.at ? ` · 上次检查 ${ago(t.at)}` : ''}</small><span class="grow"></span>
        <button class="btn tiny" onclick={() => track(site.id)} disabled={jobRunning || !!busy}><Icon name="play" size={11} />立即检查</button>
        <button class="btn tiny danger" onclick={() => rollback(site.id, v.meta.cutover.host)} disabled={jobRunning || !!busy}><Icon name="undo" size={11} />回退到旧站</button>
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
      {#if v.stage === 'done'}<p class="hint">跟踪满 {28} 天了。以后的内容更新交给托管运营；旧主机确认不用了就可以停掉。</p>{/if}
    {/if}
  {/if}
</div>

<style>
  .mg h3 .sub { font-weight: 400; font-size: 12px; color: var(--faint); }
  .live { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 500; color: var(--dim); }
  .ic-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border: none; background: none; border-radius: 6px; color: var(--faint); cursor: pointer; }
  .ic-btn:hover { background: #f1efe9; color: var(--text); }
  .stages { list-style: none; margin: 0 0 14px; padding: 0; display: flex; flex-wrap: wrap; gap: 6px 14px; font-size: 12px; color: var(--faint); }
  .stages li { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; }
  .stages li.on { color: var(--text); font-weight: 600; }
  .stages li.done { color: var(--dim); }
  .stages .n { width: 18px; height: 18px; border-radius: 50%; border: 1px solid var(--border2); box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; font-size: 10.5px; font-weight: 600; }
  .stages li.on .n { background: var(--text); border-color: var(--text); color: #fff; }
  .stages li.done .n { background: var(--su-ok-soft); border-color: transparent; color: var(--ok); }
  .running { display: flex; align-items: center; gap: 10px; padding: 9px 12px; margin-bottom: 12px; border-radius: 10px; background: var(--su-run-soft); color: var(--su-run); font-size: 12.5px; }
  .running span:last-child { min-width: 0; }
  .running small { margin-left: 6px; color: var(--dim); }
  .lead { margin: 0 0 12px; font-size: 12.5px; color: var(--dim); line-height: 1.6; }
  .resume { display: flex; align-items: center; gap: 12px; padding: 10px 12px; border-radius: 10px; background: #faf6ec; border: 1px solid #f0e4c4; font-size: 12.5px; color: #6b4e12; }
  .resume span { flex: 1; min-width: 0; line-height: 1.5; }
  .next { display: flex; align-items: center; gap: 10px; margin-top: 12px; padding: 8px 8px 8px 14px; border-radius: 10px; background: var(--su-ok-soft); border: 1px solid var(--su-ok-line); font-size: 12.5px; color: #1f5a37; }
  .next span { flex: 1; min-width: 0; line-height: 1.5; }
  .next .btn { flex: none; }
  .sec { display: flex; align-items: center; gap: 8px; margin: 16px 0 8px; padding-top: 14px; border-top: 1px solid var(--border); flex-wrap: wrap; }
  .sec.first { margin-top: 0; padding-top: 0; border-top: none; }
  .sec b { font-size: 13px; font-weight: 600; }
  .sec small { font-size: 11.5px; color: var(--faint); }
  .grow { flex: 1; }
  .fold { border: none; background: none; padding: 2px 4px; border-radius: 6px; font: inherit; font-size: 12px; color: var(--faint); cursor: pointer; }
  .fold:hover { color: var(--text); background: #f1efe9; }
  .pass { display: inline-flex; align-items: center; gap: 4px; font-size: 12px; color: var(--ok); }
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
  .folded small { color: var(--faint); }
  .items { margin: 2px 0; padding: 6px 10px; list-style: none; background: var(--rail); border-radius: 8px; display: flex; flex-direction: column; gap: 2px; max-height: 180px; overflow-y: auto; }
  .items li { font-size: 11.5px; color: var(--dim); overflow-wrap: anywhere; }
  .sub-sec { margin-top: 12px; padding: 10px 12px; border-radius: 10px; background: var(--rail); display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; }
  .sub-sec > b { font-size: 12.5px; font-weight: 600; }
  .sub-sec .row { gap: 8px; }
  .dd { width: 200px; }
  .proj { width: 180px !important; }
  .foot { display: flex; align-items: center; gap: 10px; margin-top: 12px; }
  .cut > li { padding: 10px 0; border-top: 1px solid var(--border); }
  .cut > li:first-child { border-top: none; padding-top: 2px; }
  .txt { display: flex; flex-direction: column; gap: 3px; padding: 7px 10px; border-radius: 8px; background: var(--rail); }
  .txt code { font-size: 12px; user-select: all; }
  .ns { display: flex; gap: 8px; flex-wrap: wrap; }
  .ns code { font-size: 12px; padding: 2px 6px; user-select: all; }
  .dyn-ok { display: flex; align-items: center; gap: 6px; margin: 0; font-size: 12px; color: var(--ok); }
  .bars { display: flex; flex-direction: column; gap: 8px; }
  .bars i { display: block; height: 10px; border-radius: 6px; background: #f1efe9; }
</style>
