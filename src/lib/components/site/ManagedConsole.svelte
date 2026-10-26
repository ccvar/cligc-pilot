<script lang="ts">
  import './site.css';
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { app, confirmDialog } from '$lib/state/app.svelte.js';
  import { sites, loadSiteStatus } from '$lib/state/sites.svelte.js';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import { mdRender } from '$lib/state/messages.svelte.js';
  import { launcher } from '$lib/state/launcher.svelte.js';
  import { openSite } from '$lib/state/sites.svelte.js';
  import { fleet, loadPending } from '$lib/state/fleet.svelte.js';
  import { runTask, sourceOf, CHANGE_LABEL, STATUS_LABEL, STAGE_LABEL } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';
  import { ago, full } from './fmt';
  import ManagedGroupEditor from './ManagedGroupEditor.svelte';
  import WhyChips from './WhyChips.svelte';
  import { managed, ACTIONS, AUTONOMY_LABEL, loadManaged, loadPlans, loadDigests, openDigest, digestNow, saveManaged, newGroup, editGroup, setSitesActive, setSiteActive, setManagedSites, kTok, budgetFull, CODE_LABEL } from '$lib/state/managed.svelte.js';

  const o = $derived(managed.overview);
  const d = $derived(managed.draft);
  const paused = $derived(!!o?.config.paused);
  const managedIds = $derived(new Set((o?.sites ?? []).map((s) => s.id)));
  const needs = $derived(fleet.pending.filter((p) => managedIds.has(p.site_id)));
  const TABS = [
    { k: 'groups' as const, l: '托管方案', i: 'sliders' },
    { k: 'plans' as const, l: '计划', i: 'checks' },
    { k: 'needs' as const, l: '需要你', i: 'bell' },
    { k: 'digests' as const, l: '日报', i: 'news' },
  ];
  $effect(() => {
    void loadManaged();
    if (managed.tab === 'plans') void loadPlans();
    if (managed.tab === 'needs') void loadPending();
    if (managed.tab === 'digests') void loadDigests();
  });

  // 规则摘要里显示模型名：沿用启动页的「服务 + 模型」选项
  const modelLabel = (b: string, m: string) => launcher.comboOpts.find((x) => x.value === `${b}::${m}`)?.label ?? (m || b);
  const siteName = (id: string) => sitesById.get(id)?.name ?? id;
  const sitesById = $derived(new Map((o?.sites ?? []).map((s) => [s.id, s])));
  const groups = $derived(o?.groups ?? []);
  // 立即执行的过程：作业步骤和日志（site-job 事件实时更新 sites.jobs），以及 AI 的那条会话
  const progress = $derived(managed.progress);
  function toggleProgress(id: string) {
    const n = new Set(managed.progress);
    if (n.has(id)) n.delete(id); else { n.add(id); if (!sites.jobs[id]) void loadSiteStatus(id); }
    managed.progress = n;
  }
  const opsConvOf = (id: string, since: number) =>
    app.convos.filter((c) => c.conn_id === `site:${id}` && c.origin === 'ops' && c.updated_at >= since - 5).sort((a, b) => b.updated_at - a.updated_at)[0];
  // 托管方案页打开时取一下各站点正在跑的作业（之后由 site-job 事件实时更新）
  let jobsFor = '';
  $effect(() => {
    if (managed.tab !== 'groups') return;
    const ids = groups.flatMap((g) => g.sites);
    const key = ids.join(',');
    if (key === jobsFor) return;
    jobsFor = key;
    for (const id of ids) if (!sites.jobs[id]) void loadSiteStatus(id);
  });
  let logOpen = $state(false);
  async function moveSite(id: string, from: string, to: string) {
    if (to === from) return;
    if (to === '__new') { newGroup([id]); return; }
    if (to === '__out') { await setManagedSites([id], false); return; }
    await setManagedSites([id], true, to);
  }
  async function runNow(id: string) {
    const g = groups.find((x) => x.sites.includes(id));
    const name = siteName(id);
    const full = budgetFull(sitesById.get(id)?.budget);
    if (full) {
      if (g && (await confirmDialog(`${name} ${full}，这一轮开始不了，计划里的任务会一直排在「待处理」。调高方案里的「每站每周 token」后再执行，或等用量滚出最近 7 天。`, { title: '本周额度已满', kind: 'warning', confirmText: '调整方案' }))) editGroup(g);
      return;
    }
    const what = g?.autonomy === 'auto'
      ? `现在为 ${name} 执行一整轮：本周还没有计划就先让 AI 制定（已有就沿用），然后按方案改排在前面的几条（每次最多 3 条）、跑构建和检查，符合发布规则的改动会直接上线到 ${name}；新页面${g.new_content === 'auto' ? '也会自动发布' : '会留给你确认'}，其余的放进「需要你」。继续吗？`
      : g?.autonomy === 'review'
        ? `现在为 ${name} 执行一整轮：本周还没有计划就先让 AI 制定（已有就沿用），然后改排在前面的几条（每次最多 3 条）、跑构建和检查，改好的都留给你确认，不会自动上线。继续吗？`
        : `现在为 ${name} 制定本周计划（只出建议，不改代码）。继续吗？`;
    const note = g?.stopped.includes(id) ? '\n\n（这个站点的托管暂停着：这一次是你手动执行，执行完照样保持暂停。）' : '';
    if (!(await confirmDialog(what + note, { title: '立即执行', kind: g?.autonomy === 'auto' ? 'warning' : 'info', confirmText: g?.autonomy === 'auto' ? '执行并自动上线' : '执行' }))) return;
    await runTask(id, 'cycle');
    managed.progress = new Set([...managed.progress, id]);
    void loadManaged();
  }
  // 排着等 AI、但现在不会被处理的站点（托管暂停、方案暂停、站点停止、额度满了）：在「需要你」里说一声
  const stuck = $derived((o?.sites ?? []).filter((x) => x.queued > 0).map((x) => {
    const g = groups.find((y) => y.sites.includes(x.id));
    const full = budgetFull(x.budget);
    const why = o?.config.paused ? '托管已全部暂停' : g?.paused ? `方案「${g.name}」已暂停` : g?.stopped.includes(x.id) ? '这个站点的托管暂停着' : full ? full : '';
    const fix = o?.config.paused ? '点右上角「恢复托管」后会接着做。' : g?.paused ? '恢复这个方案后会接着做。' : g?.stopped.includes(x.id) ? '把它的托管开关打开后会接着做。' : full ? '调高方案里的「每站每周 token」，或等用量滚出最近 7 天。' : '';
    return { id: x.id, name: x.name, queued: x.queued, why, fix };
  }).filter((x) => x.why));
  // 暂停只看站点自己的开关：所有站点都关着＝全部已暂停
  const allOff = $derived(groups.some((g) => g.sites.length) && groups.every((g) => g.sites.every((x) => g.stopped.includes(x))));
  let goalOpen = $state<Set<string>>(new Set());
  function toggleGoal(id: string) { const n = new Set(goalOpen); if (n.has(id)) n.delete(id); else n.add(id); goalOpen = n; }
  const ACT_LABEL = Object.fromEntries(ACTIONS.map((a) => [a.key, a.label]));
  // 日报设置（全局）：只比较这两项
  const dirty = $derived(!!d && !!o && (d.digest !== o.config.digest || d.digest_hour !== o.config.digest_hour));

  const KIND: Record<string, { l: string; c: string; i: string }> = {
    shipped: { l: '已上线', c: 'ok', i: 'rocket' }, planned: { l: '计划', c: 'run', i: 'checks' }, proposed: { l: '待你确认', c: 'warn', i: 'bell' }, rolled_back: { l: '已回滚', c: 'err', i: 'undo' },
    failed: { l: '失败', c: 'err', i: 'x' }, watch_ok: { l: '验收通过', c: '', i: 'check' }, watch_failed: { l: '验收没通过', c: 'err', i: 'x' }, attention: { l: '待核对', c: 'warn', i: 'search' }, downgraded: { l: '降档', c: 'warn', i: 'sliders' }, fallback: { l: '备用模型', c: 'warn', i: 'cpu' },
    budget: { l: '预算', c: 'warn', i: 'gauge' }, digest: { l: '日报', c: '', i: 'news' }, paused: { l: '暂停', c: 'warn', i: 'pause' }, resumed: { l: '恢复', c: 'ok', i: 'play' }, joined: { l: '开始托管', c: 'ok', i: 'plus' }, left: { l: '退出托管', c: '', i: 'x' },
  };
  const when = ago;
  const ratio = (a: number, b: number) => (b > 0 ? Math.min(1, a / b) : 0);
  const md = (iso: string) => { const m = /^\d{4}-(\d{2})-(\d{2})$/.exec(iso); return m ? `${+m[1]}月${+m[2]}日` : iso; };
</script>

<ManagedGroupEditor />
<div class="su page mc">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="page-head" data-tauri-drag-region onmousedown={startDrag}>
    <div class="ttl">
      <h1>托管运营 {#if allOff}<span class="pill warn">全部已暂停</span>{/if}</h1>
      <p class="hint">{o ? `托管 ${o.sites.length} 个站点 · 今天 AI 自动上线 ${o.shipped_today} 项 · ${o.waiting} 项需要你` : '读取中…'}</p>
    </div>
    <div class="row">
      <button class="ic-btn" onclick={() => newGroup()} aria-label="新增托管方案" data-tip="新增托管方案"><Icon name="plus" size={15} /></button>
      {#if o?.sites.length}<button class="btn small ghost" onclick={digestNow}><Icon name="news" size={13} />生成今日日报</button>{/if}
      {#if o && groups.some((g) => g.sites.length)}
        <button class="ic-btn" onclick={() => setSitesActive(groups, allOff, '全部托管站点')} aria-label={allOff ? '恢复全部站点的托管' : '暂停全部站点的托管'} data-tip={allOff ? '恢复全部站点的托管' : '暂停全部站点的托管（之后可以一个个恢复）'}><Icon name={allOff ? 'play' : 'pause'} size={15} /></button>
      {/if}
    </div>
  </header>
  <nav class="tabs">
    {#each TABS as t (t.k)}<button class:on={managed.tab === t.k} onclick={() => (managed.tab = t.k)}><Icon name={t.i} size={13} />{t.l}{#if t.k === 'needs' && o?.waiting}<span class="cnt">{o.waiting}</span>{:else if t.k === 'needs' && stuck.length}<span class="cnt warn" data-tip="{stuck.reduce((n, x) => n + x.queued, 0)} 条任务排着没动（{stuck[0].why}）">{stuck.reduce((n, x) => n + x.queued, 0)}</span>{/if}</button>{/each}
  </nav>

  <div class="page-body">
   {#key managed.tab}
   <div class="pane">
    {#if o && !groups.length && managed.tab !== 'digests'}
      <div class="card intro">
        <h3><Icon name="sparkles" />把站点交给 AI 运营</h3>
        <p class="hint">托管后，AI 每周读数据和代码给每个站点定计划，然后在独立分支里改好、跑构建和检查、自动上线；上线 1 小时后检查页面，效果复盘变差就自动回滚。你只需要处理「需要你」里的例外。</p>
        <ul>
          <li>依赖、构建 / 部署配置、Worker 代码、推荐与邀请相关的内容，永远不会自动改。</li>
          <li>新页面可以自动发布，也可以设成人审人发。</li>
          <li>主模型额度不够时自动换备用模型；每站每天、每周都有上限，随时可以一键暂停。</li>
        </ul>
        <ul>
          <li>可以建多个托管方案：每个方案管一个或多个站点，各有自己的模型、发布方式和上限；方案里的每个站点也能单独停止。</li>
        </ul>
        <div class="row"><button class="btn small primary" onclick={() => newGroup()}><Icon name="plus" size={12} />新增托管方案</button></div>
      </div>
    {:else if managed.tab === 'plans'}
      {#each o?.sites ?? [] as s (s.id)}
        {@const p = managed.plans.find((x) => x.site_id === s.id)}
        <div class="card">
          <h3><SiteIcon id={s.id} size={15} />{s.name}<span class="stg {s.stage}">{STAGE_LABEL[s.stage] ?? s.stage}</span><span class="sub">{p ? `${md(p.plan.date)}制定` : '还没有计划'}</span><span class="grow"></span>
            {#if p?.plan.conv_id}<button class="btn tiny" onclick={() => openConv(p.plan.conv_id)}><Icon name="sparkles" size={11} />看 AI 的分析</button>{/if}
            <button class="btn tiny" onclick={() => runTask(s.id, 'plan')} disabled={!!s.job}><Icon name="refresh" size={11} />{p ? '重新制定' : '现在制定'}</button>
          </h3>
          {#if p?.plan.notes}
            <div class="pnote"><Icon name="info" size={12} /><span><b>AI 给你的说明</b>{p.plan.notes}</span></div>
          {/if}
          {#if p?.positioning.text}
            <div class="pos">
              <div class="pos-t"><Icon name="target" size={12} />站点定位<span class="faint">{p.positioning.by === 'user' ? '运营人员确认' : 'AI 归纳'}</span><span class="grow"></span><button class="ic-btn ic-sm" onclick={() => openSite(s.id, 'overview')} aria-label="修改站点定位" data-tip="修改站点定位（在站点概览里）"><Icon name="pencil" size={13} /></button></div>
              <p>{p.positioning.text}</p>
            </div>
          {/if}
          {#if p}
            <ol class="plan">
              {#each p.plan.tasks as t, i (i)}
                <li><span class="no">{i + 1}</span><div><div class="pt"><b>{t.title}</b></div><div class="pw"><WhyChips change={t.change} source={t.source || (t.change === 'new_content' ? 'positioning' : t.change === 'technical' ? 'health' : 'data')} target={t.query} /></div>{#if t.why}<small>{t.why}</small>{/if}{#if t.expect?.note}<small class="expect"><Icon name="target" size={11} />预期：{t.expect.note}</small>{/if}{#if t.page}<small class="mono pg">{t.page}</small>{/if}</div></li>
              {/each}
            </ol>
          {:else}<p class="hint">每周自动制定一次；也可以现在就让 AI 先做一份。</p>{/if}
        </div>
      {/each}
    {:else if managed.tab === 'needs'}
      <div class="card">
        <h3><Icon name="bell" />需要你</h3>
        <p class="hint">只放要你动手的：判定不能自动上线的改动（新页面待审、改到推荐 / 邀请内容、超出每日上限）、失败的任务，以及上线后线上验收有问题要你核对的。排着等 AI 处理的不在这里。</p>
        {#if !needs.length}<p class="hint">没有需要你处理的事。</p>{/if}
        {#each stuck as x (x.id)}
          <div class="stuck"><Icon name="pause" size={12} /><span><b>{x.name}</b> 有 {x.queued} 条任务排着等 AI 处理，{x.why}，所以一直没动。{x.fix}</span><button class="link-btn" onclick={() => openSite(x.id, 'queue', 'open')}>去看看</button></div>
        {/each}
        <table class="tbl">
          <tbody>
            {#each needs as n (n.item.id)}
              <tr class="clickable" onclick={() => openSite(n.site_id, 'queue')}>
                <td class="nsite"><span><SiteIcon id={n.site_id} size={13} />{n.site_name}</span></td>
                <td class="trunc">{n.item.opp.title}<div class="nw"><WhyChips change={n.item.opp.change} source={sourceOf(n.item.opp)} kind={n.item.opp.kind} target={n.item.opp.target} /></div><div class="faint" style="font-size:11.5px">{n.item.status === 'deployed' ? n.item.attention : n.item.verdict && n.item.verdict !== 'auto' ? n.item.verdict : n.item.note}</div></td>
                <td style="width:90px"><span class="pill {n.item.status === 'failed' ? 'err' : 'warn'}">{n.item.status === 'deployed' ? '待核对' : STATUS_LABEL[n.item.status]}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if managed.tab === 'digests'}
      <div class="card">
        <h3><Icon name="news" />日报 <span class="grow"></span><button class="btn tiny" onclick={digestNow}><Icon name="news" size={11} />生成今日日报</button></h3>
        {#if !managed.digests.length}<p class="hint">还没有日报。每天{o?.config.digest_hour ?? 9} 点后自动生成一份，并发系统通知。</p>{/if}
        <div class="seg digs">{#each managed.digests as n (n)}<button class:on={managed.digest?.name === n} onclick={() => openDigest(n)}>{md(n)}</button>{/each}</div>
        {#if managed.digest}<div class="md">{@html mdRender(managed.digest.text)}</div>{/if}
      </div>
      {#if d}
        <div class="card">
          <h3><Icon name="calendar" />日报设置 <span class="sub">所有托管方案共用</span></h3>
          <div class="row">
            <label class="check"><input type="checkbox" bind:checked={d.digest} /><span>每天发一份托管日报（系统通知 + 报告）</span></label>
            {#if d.digest}<span class="faint" style="font-size:12px">时间</span><input class="tin" type="number" min="0" max="23" style="width:70px" bind:value={d.digest_hour} /><span class="faint" style="font-size:12px">点之后</span>{/if}
          </div>
          <p class="hint">托管需要客户端在运行（关掉窗口后托盘常驻即可）；电脑关机期间不会运营，开机后补跑一次。</p>
        </div>
        {#if dirty}<div class="savebar"><span>日报设置有未保存的修改</span><span class="grow"></span><button class="btn small ghost" onclick={() => o && (managed.draft = JSON.parse(JSON.stringify(o.config)))}>撤销</button><button class="btn small primary" onclick={saveManaged} disabled={managed.saving}>保存</button></div>{/if}
      {/if}
    {:else}
      <div class="gbar">
        <p class="hint">每个方案管一个或多个站点，各有自己的自主程度和规则；一个站点只属于一个方案，不在方案里的站点由你自己运营。方案和站点都能单独暂停。</p>
      </div>
      {#each groups as g (g.id)}
        {@const gOff = g.sites.length > 0 && g.sites.every((x) => g.stopped.includes(x))}
        <div class="card grp" class:off={gOff}>
          <h3><Icon name="sparkles" />{g.name}<span class="pill line">{AUTONOMY_LABEL[g.autonomy] ?? '全自动'}</span>{#if gOff}<span class="pill warn">已暂停</span>{/if}<span class="sub">{g.sites.length} 个站点</span><span class="grow"></span>
            {#if g.sites.length}<button class="btn tiny" onclick={() => setSitesActive([g], gOff, `「${g.name}」`)} data-tip={gOff ? '把这个方案里的站点都打开' : '把这个方案里的站点都暂停'}><Icon name={gOff ? 'play' : 'pause'} size={11} />{gOff ? '恢复方案' : '暂停方案'}</button>{/if}
            <button class="ic-btn ic-sm" onclick={() => editGroup(g)} aria-label="编辑托管方案" data-tip="编辑托管方案"><Icon name="pencil" size={14} /></button>
          </h3>
          {#if g.goal?.trim()}
            <!-- 运营目标默认只显示一行，点开看全文 -->
            <button class="goal" class:open={goalOpen.has(g.id)} onclick={() => toggleGoal(g.id)} data-tip={goalOpen.has(g.id) ? null : '点开看完整的运营目标'}><span class="goal-t">{g.goal}</span><span class="goal-more">{goalOpen.has(g.id) ? '收起' : '展开'}</span></button>
          {/if}
          <div class="rules">
            <span><Icon name="checks" size={12} />{(g.actions ?? []).length === 5 ? '全部动作' : (g.actions ?? []).map((a) => ACT_LABEL[a] ?? a).join('、')}</span>
            {#if g.guidelines?.trim()}<span><Icon name="doc" size={12} />有内容准则</span>{/if}
            {#if g.topics?.trim()}<span><Icon name="bulb" size={12} />{g.topics.split('\n').filter((l) => l.trim()).length} 个选题</span>{/if}
            <span><Icon name="cpu" size={12} />{modelLabel(g.brain, g.model)}{#if g.fallback_brain}<span class="faint"> · 备用 {modelLabel(g.fallback_brain, g.fallback_model)}</span>{/if}</span>
            <span><Icon name="file" size={12} />新页面{g.new_content === 'auto' ? '自动发布' : '人审人发'}{g.min_page_chars ? ` · ≥ ${g.min_page_chars} 字` : ''}</span>
            {#if g.autonomy === 'auto'}<span><Icon name="code" size={12} />{CODE_LABEL[g.code_changes ?? 'all']}</span>{/if}
            <span><Icon name="rocket" size={12} />每站每天 ≤ {g.daily_changes} 条</span>
            <span><Icon name="gauge" size={12} />每站每周 {g.tokens_per_week ? `${kTok(g.tokens_per_week)} token` : '不限 token'}</span>
          </div>
          {#if g.sites.length}
            <div class="list gsites">
              {#each g.sites as id (id)}
                {@const s = sitesById.get(id)}
                {@const on = !g.stopped.includes(id)}
                <div class="li gs">
                  <button class="gs-name" onclick={() => openSite(id)}><SiteIcon {id} size={15} /><span>{siteName(id)}</span></button>
                  {#if s}<span class="stg {s.stage}" data-tip={s.stage_manual ? '手动指定的阶段' : '按 Search Console 数据自动判断'}>{STAGE_LABEL[s.stage] ?? s.stage}</span>{/if}
                  {#if s}<span class="gs-meta faint">本周上线 {s.shipped_week}{#if s.waiting}{' · '}<span class="warn-text">需要你 {s.waiting}</span>{/if}{#if budgetFull(s.budget)}{' · '}<span class="warn-text" data-tip="{budgetFull(s.budget)}。额度满了 AI 不再处理这个站点的任务，调高方案里的「每站每周 token」或等用量滚出最近 7 天">额度已满</span>{/if}</span>{/if}
                  {#if s?.job || sites.jobs[id]?.status === 'running'}
                    <button class="pill run gs-job" onclick={() => toggleProgress(id)} aria-expanded={progress.has(id)} data-tip="看进度"><span class="spin"></span>{s?.job || sites.jobs[id]?.title}中<Icon name={progress.has(id) ? 'x' : 'arrow'} size={10} /></button>
                  {:else if progress.has(id) && sites.jobs[id]}
                    <button class="btn tiny gs-run" onclick={() => toggleProgress(id)}>收起结果</button>
                  {:else}
                    <button class="btn tiny gs-run" onclick={() => runNow(id)} data-tip="现在执行一整轮：沿用或制定本周计划，再按方案改排在前面的几条（全自动的会自动上线）"><Icon name="play" size={10} />立即执行</button>
                  {/if}
                  <span class="grow"></span>
                  <div class="gs-move" data-tip="把这个站点移到别的方案，或退出托管">
                    <Dropdown compact value={g.id} onchange={(v) => moveSite(id, g.id, v)} options={[
                      ...groups.map((x) => ({ value: x.id, label: x.name, sub: AUTONOMY_LABEL[x.autonomy] ?? '' })),
                      { value: '__new', label: '新建方案…', sub: '用这个站点新建一个方案' },
                      { value: '__out', label: '退出托管', sub: '改为自己运营' },
                    ]} />
                  </div>
                  <span class="gs-state" class:on >{on ? '托管中' : '已暂停'}</span>
                  <button class="switch" class:on role="switch" aria-checked={on} aria-label="{on ? '停止' : '启用'}托管 {siteName(id)}" data-tip={on ? '暂停这个站点的托管（规则保留）' : '恢复这个站点的托管'} onclick={() => setSiteActive(id, !on)}><i></i></button>
                </div>
                {#if progress.has(id) && sites.jobs[id]}
                  {@const conv = opsConvOf(id, sites.jobs[id].started_at)}
                  <div class="li gs-prog">
                    <SiteJobPanel job={sites.jobs[id]} logOpen />
                    <div class="row gp-acts">
                      {#if conv}<button class="btn tiny" onclick={() => openConv(conv.id)}><Icon name="chat" size={11} />{conv.status === 'running' ? '看 AI 正在做什么' : '打开 AI 的会话'}</button>{/if}
                      {#if sites.jobs[id].status !== 'running'}<button class="btn tiny" onclick={() => { managed.tab = 'plans'; }}><Icon name="checks" size={11} />看计划</button><button class="btn tiny" onclick={() => openSite(id, 'queue')}><Icon name="inbox" size={11} />看待处理</button>{/if}
                    </div>
                  </div>
                {/if}
              {/each}
            </div>
          {:else}<p class="hint">这个方案还没有站点。<button class="link-btn" onclick={() => editGroup(g)}>选择站点</button></p>{/if}
        </div>
      {/each}

      <div class="card oplog" class:open={logOpen}>
        <button class="log-h" onclick={() => (logOpen = !logOpen)} aria-expanded={logOpen}>
          <svg class="chev" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
          <Icon name="activity" />操作日志
          <span class="sub">{managed.activity.length ? `${managed.activity.length} 条 · 最近 ${when(managed.activity[0].ts)}` : '还没有记录'}</span>
        </button>
        {#if logOpen}
          {#if !managed.activity.length}<p class="hint">托管站点的计划、上线、检查、回滚、暂停都会记在这里。</p>{/if}
        <div class="list feed">
          {#each managed.activity as a, i (i)}
            <div class="li ev">
              <span class="pill {KIND[a.kind]?.c ?? ''}"><Icon name={KIND[a.kind]?.i ?? 'activity'} size={11} />{KIND[a.kind]?.l ?? a.kind}</span>
              <div class="ev-main">
                <b>{a.title}</b>
                {#if a.change || a.source}<div class="ev-why"><WhyChips change={a.change} source={a.source} /></div>{/if}
                <small>{#if a.site_name}<button class="link-btn plain site-l" onclick={() => openSite(a.site_id)}><SiteIcon id={a.site_id} size={11} />{a.site_name}</button>{/if}{#if a.site_name && a.detail}<span class="sep">·</span>{/if}{a.detail}</small>
              </div>
              <span class="ev-r">
                <span class="faint t" data-tip={full(a.ts)}>{when(a.ts)}</span>
                {#if a.item_id && a.site_id}<button class="link-btn plain see" onclick={() => openSite(a.site_id, 'queue')}>看详情</button>{/if}
              </span>
            </div>
          {/each}
        </div>
        {/if}
      </div>
    {/if}
   </div>
   {/key}
  </div>
</div>

<style>
  .expect { display: inline-flex; align-items: center; gap: 4px; color: var(--dim); }
  .nsite { width: 150px; min-width: 150px; max-width: 150px; vertical-align: top; }
  .nsite span { display: inline-flex; align-items: center; gap: 6px; max-width: 100%; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-size: 12.5px; }
  .stuck { display: flex; align-items: flex-start; gap: 8px; margin-top: 10px; padding: 9px 12px; border-radius: 9px; background: var(--su-warn-soft); color: var(--su-warn-ink); font-size: 12.5px; line-height: 1.6; }
  .stuck :global(svg) { flex: none; margin-top: 4px; }
  .stuck span { flex: 1; }
  .stuck b { font-weight: 600; }
  .stuck .link-btn { flex: none; font-size: 12.5px; }
  .bad { color: var(--err); font-weight: 600; }
  .oplog { padding-top: 12px; padding-bottom: 12px; }
  .log-h { width: 100%; display: flex; align-items: center; gap: 8px; border: none; background: none; padding: 2px 0; font: inherit; color: var(--text); cursor: pointer; text-align: left; }
  .log-h :global(.ico) { color: var(--dim); }
  .log-h .sub { font-size: 12px; color: var(--faint); font-weight: 400; }
  .log-h .chev { color: var(--faint); transition: transform .16s var(--su-ease); }
  .oplog.open .log-h .chev { transform: rotate(90deg); }
  .oplog.open .log-h { margin-bottom: 8px; font-weight: 600; }
  .ic-btn { width: 30px; height: 30px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 8px; color: var(--dim); cursor: pointer; transition: background-color .12s, color .12s; }
  .ic-btn:hover { background: var(--su-hover); color: var(--text); }
  .gbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  .gbar .hint { margin: 0; }
  .grp.off { background: #fcfbf9; }
  .goal { display: flex; align-items: baseline; gap: 8px; width: 100%; margin: -6px 0 10px; padding: 0; border: none; background: none; font: inherit; font-size: 12.5px; color: var(--dim); line-height: 1.6; text-align: left; cursor: pointer; }
  .goal-t { flex: 1; min-width: 0; max-width: 90ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .goal.open .goal-t { white-space: normal; color: var(--text); }
  .goal-more { flex: none; font-size: 12px; color: var(--faint); }
  .goal:hover .goal-more { color: var(--text); text-decoration: underline; }
  .rules { display: flex; flex-wrap: wrap; gap: 6px 16px; font-size: 12px; color: var(--dim); margin: -4px 0 10px; }
  .rules > span { display: inline-flex; align-items: center; gap: 5px; }
  .rules :global(.ico) { color: var(--faint); }
  .gsites { margin-bottom: -8px; }
  .gs { display: flex; align-items: center; gap: 10px; padding-top: 8px; padding-bottom: 8px; }
  .gs-name { display: inline-flex; align-items: center; gap: 8px; border: none; background: none; padding: 0; font: inherit; font-size: 13px; font-weight: 550; color: var(--text); cursor: pointer; min-width: 0; }
  .gs-name span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .gs-name:hover span { text-decoration: underline; text-underline-offset: 3px; text-decoration-color: var(--border2); }
  .stg { flex: none; display: inline-flex; align-items: center; height: 18px; padding: 0 7px; border-radius: 999px; font-size: 10.5px; font-weight: 600; background: var(--su-warn-soft); color: var(--su-warn-ink); }
  .stg.growing { background: var(--su-run-soft); color: var(--su-run); }
  .stg.established { background: var(--su-ok-soft); color: var(--ok); }
  .pnote { display: flex; gap: 8px; margin: -2px 0 12px; padding: 9px 12px; border-radius: 10px; background: var(--su-warn-soft); color: var(--su-warn-ink); font-size: 12.5px; line-height: 1.6; }
  .pnote :global(.ico) { flex: none; margin-top: 3px; }
  .pnote b { display: block; font-weight: 600; }
  .pos { margin: -2px 0 14px; padding: 10px 12px; border-radius: 10px; background: var(--rail); border: 1px solid var(--border); }
  .pos-t { display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: var(--text); }
  .pos-t .faint { font-weight: 400; font-size: 11.5px; }
  .pos-t .grow { flex: 1; }
  .pos p { margin: 4px 0 0; font-size: 12.5px; color: var(--dim); line-height: 1.65; }
  .gs-run { opacity: .55; transition: opacity .12s; }
  .gs:hover .gs-run, .gs-run:focus-visible { opacity: 1; }
  .gs-job { flex: none; border: none; cursor: pointer; gap: 5px; }
  .gs-prog { padding-top: 4px; padding-bottom: 14px; background: #fcfbf9; }
  .gs-prog :global(pre.log) { max-height: 200px; }
  .gp-acts { margin-top: 10px; gap: 6px; }
  .gs-move { flex: none; }
  .gs-move :global(.dd-trigger) { max-width: 180px; font-size: 12px; color: var(--dim); }
  .gs-meta { font-size: 11.5px; white-space: nowrap; }
  .gs .grow { flex: 1; }
  .gs-state { font-size: 11.5px; color: var(--faint); white-space: nowrap; }
  .gs-state.on { color: var(--ok); }
  .switch { position: relative; flex: none; width: 32px; height: 18px; border: none; border-radius: 999px; background: var(--border2); cursor: pointer; padding: 0; transition: background-color .18s; }
  .switch i { position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; box-shadow: 0 1px 2px rgba(0, 0, 0, .2); transition: transform .18s var(--su-ease); }
  .switch.on { background: var(--ok); }
  .switch.on i { transform: translateX(14px); }
  .switch.dim { opacity: .45; }
  .switch:focus-visible { box-shadow: var(--su-ring); }
  .intro ul { margin: 6px 0 14px; padding-left: 18px; font-size: 12.5px; line-height: 1.8; color: var(--dim); }
  .ev { display: flex; align-items: flex-start; gap: 12px; }
  .ev .pill { flex: none; min-width: 76px; justify-content: flex-start; margin-top: 1px; }
  .ev-main { flex: 1; min-width: 0; }
  .ev-main b { display: block; font-size: 13px; font-weight: 550; line-height: 1.5; }
  .ev-main small { display: block; font-size: 11.5px; color: var(--dim); overflow-wrap: anywhere; line-height: 1.55; }
  .site-l { display: inline-flex; align-items: center; gap: 5px; vertical-align: -1px; }
  .sep { margin: 0 6px; color: var(--border2); }
  .ev .t { flex: none; font-size: 11.5px; white-space: nowrap; }
  .ev-r { flex: none; display: flex; flex-direction: column; align-items: flex-end; gap: 2px; margin-top: 2px; }
  .see { font-size: 11.5px; opacity: 0; transition: opacity .12s; }
  .ev:hover .see { opacity: 1; }
  .ev-why { margin: 3px 0 2px; }
  .pw { margin: 4px 0 2px; }
  .nw { margin: 3px 0; }
  .link-btn.plain { text-decoration: none; color: var(--dim); font-size: inherit; }
  .link-btn.plain:hover { color: var(--text); text-decoration: underline; }
  .plan { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 12px; font-size: 13px; }
  .plan li { display: flex; gap: 10px; align-items: flex-start; }
  .plan .no { flex: none; width: 20px; height: 20px; border-radius: 50%; background: var(--accent-soft); color: var(--dim); font-size: 11px; font-weight: 600; display: inline-flex; align-items: center; justify-content: center; margin-top: 1px; font-variant-numeric: tabular-nums; }
  .plan .pt { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .plan b { font-weight: 550; }
  .plan small { display: block; color: var(--dim); font-size: 12px; margin-top: 2px; }
  .plan small.pg { color: var(--faint); font-size: 11px; }
  .x.mini { width: 16px; height: 16px; font-size: 13px; margin-right: -4px; }
  .digs { flex-wrap: wrap; margin-bottom: 4px; }
  tr.clickable { cursor: pointer; }
  .md { margin-top: 14px; font-size: 13px; line-height: 1.75; border-top: 1px solid var(--border); padding-top: 12px; }
  .md :global(h1) { font-size: 16px; margin: 0 0 8px; }
  .md :global(h2) { font-size: 14px; margin: 16px 0 6px; }
  .md :global(ul) { padding-left: 18px; margin: 6px 0; }
</style>
