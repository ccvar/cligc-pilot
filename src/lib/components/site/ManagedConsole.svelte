<script lang="ts">
  import { turnErrorHint } from '$lib/friendlyError';
  import './site.css';
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { app, confirmDialog, choiceDialog } from '$lib/state/app.svelte.js';
  import { jev, loadJev } from '$lib/state/typesafe.svelte.js';
  import TypeSafeMark from '$lib/components/icons/TypeSafeMark.svelte';
  import { sites, loadSiteStatus } from '$lib/state/sites.svelte.js';
  import SiteJobPanel from './SiteJobPanel.svelte';
  import { mdRender } from '$lib/state/messages.svelte.js';
  import { launcher } from '$lib/state/launcher.svelte.js';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import { modelIcon } from '$lib/state/models';
  import { openSite } from '$lib/state/sites.svelte.js';
  import DigestCalendar from './DigestCalendar.svelte';
  import PlansView from './PlansView.svelte';
  import RunChart from './RunChart.svelte';
  import PlanSpark from './PlanSpark.svelte';
  import RecentShips from './RecentShips.svelte';
  import NeedsView from './NeedsView.svelte';
  import DigestView from './DigestView.svelte';
  import { usd } from '$lib/state/growth.svelte.js';
  import BackgroundNotice from './BackgroundNotice.svelte';
  import { fleet, loadPending } from '$lib/state/fleet.svelte.js';
  import { runTask, STAGE_LABEL } from '$lib/state/ops.svelte.js';
  import { openConv } from '$lib/state/conversations';
  import { ago, full } from './fmt';
  import ManagedGroupEditor from './ManagedGroupEditor.svelte';
  import WhyChips from './WhyChips.svelte';
  import { managed, cadenceLabel, perWeek, ACTIONS, AUTONOMY_LABEL, loadManaged, loadPlans, loadDigests, openDigest, digestNow, saveManaged, newGroup, editGroup, setSitesActive, setSiteActive, setManagedSites, kTok, budgetFull, costText, costTip, CODE_LABEL, setGroupRule, GOAL_PRESETS } from '$lib/state/managed.svelte.js';
  import type { ManagedGroup, DayCount, AiCost } from '$lib/types';

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
    if (managed.tab === 'groups') void loadDigests();
    if (managed.tab === 'plans') void loadPlans();
    if (managed.tab === 'needs') void loadPending();
    if (managed.tab === 'digests') void loadDigests().then(() => { if (!managed.digest && managed.digests[0]) void openDigest(managed.digests[0]); });
  });

  // 规则摘要里显示模型名：沿用启动页的「服务 + 模型」选项
  // 内容准则的悬停预览：去掉 Markdown 标题符号和空行，取前几行
  const guidePreview = (t: string) => {
    const lines = t.split('\n').map((l) => l.replace(/^#+\s*/, '').trim()).filter(Boolean);
    return lines.slice(0, 6).join('\n') + (lines.length > 6 ? '\n…' : '');
  };
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
  // 记录卡片：最近上线 / 操作日志。记住上次看的 tab；没记过的话有上线记录先看上线，没有就看日志
  const HIST_KEY = 'pilot.managed.histTab';
  let histPick = $state<'' | 'ships' | 'log'>((() => { try { const v = localStorage.getItem(HIST_KEY); return v === 'ships' || v === 'log' ? v : ''; } catch { return ''; } })());
  const histTab = $derived(histPick || (managed.ships.length ? 'ships' : 'log'));
  function setHist(t: 'ships' | 'log') { histPick = t; try { localStorage.setItem(HIST_KEY, t); } catch { /* 存不了就只在这次有效 */ } }
  const LOG_LIMIT = 20;
  let logAll = $state(false);
  $effect(() => { void loadJev(); });
  // 日志按「时间 + 站点 + 条目 + 类型」作 key；同一秒批量暂停 / 退出多个站点时会撞，重复的加序号（撞 key 会让整个列表渲染失败）
  const activityKeys = $derived.by(() => {
    const seen = new Map<string, number>();
    return managed.activity.map((a) => {
      const k = `${a.ts}:${a.site_id}:${a.item_id}:${a.kind}`;
      const n = seen.get(k) ?? 0;
      seen.set(k, n + 1);
      return n ? `${k}:${n}` : k;
    });
  });
  // 点执行记录图上没有日报的那天：操作日志只看那一天
  let logDay = $state('');
  let logEl = $state<HTMLElement | null>(null);
  const dayOf = (ts: number) => { const d = new Date(ts * 1000); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`; };
  const feed = $derived(managed.activity.map((a, i) => ({ a, k: activityKeys[i] })).filter((x) => !logDay || dayOf(x.a.ts) === logDay));
  function pickDay(date: string, report: boolean) {
    if (report) { managed.tab = 'digests'; void openDigest(date); return; }
    logDay = date; histPick = 'log';
    requestAnimationFrame(() => logEl?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
  }
  async function moveSite(id: string, from: string, to: string) {
    if (to === from) return;
    if (to === '__new') { newGroup([id]); return; }
    if (to === '__out') {
      if (!(await confirmDialog(`${siteName(id)} 退出托管后，计划里的任务不再自动执行，改为你自己运营。`, { title: '退出托管', kind: 'warning', confirmText: '退出托管' }))) return;
      await setManagedSites([id], false);
      return;
    }
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
      ? `现在为 ${name} 执行一整轮：本周还没有计划就先让 AI 制定（已有就沿用），然后按方案改排在前面的 ${g.per_round} 个、跑构建和检查。\n\n「执行并自动上线」：符合发布规则的改动直接上线到 ${name}，新页面${g.new_content === 'auto' ? '也会自动发布' : '留给你确认'}，其余的放进「需要你」。\n「执行但不上线」：改好、检查通过的也都放进「需要你」，你看过再上线。`
      : g?.autonomy === 'review'
        ? `现在为 ${name} 执行一整轮：本周还没有计划就先让 AI 制定（已有就沿用），然后改排在前面的 ${g.per_round} 个、跑构建和检查，改好的都留给你确认，不会自动上线。继续吗？`
        : `现在为 ${name} 制定本周计划（只出建议，不改代码）。继续吗？`;
    const note = g?.stopped.includes(id) ? '\n\n（这个站点的托管暂停着：这一次是你手动执行，执行完照样保持暂停。）' : '';
    let hold = false;
    if (g?.autonomy === 'auto') {
      const c = await choiceDialog(what + note, { title: '立即执行', kind: 'warning', confirmText: '执行并自动上线', altText: '执行但不上线' });
      if (!c) return;
      hold = c === 'alt';
    } else if (!(await confirmDialog(what + note, { title: '立即执行', kind: 'info', confirmText: '执行' }))) return;
    await runTask(id, 'cycle', hold);
    managed.progress = new Set([...managed.progress, id]);
    void loadManaged();
  }
  // 排着等 AI、但现在不会被处理的站点（托管暂停、方案暂停、站点停止、额度满了）：在「需要你」里说一声
  const stuck = $derived((o?.sites ?? []).filter((x) => x.queued > 0).map((x) => {
    const g = groups.find((y) => y.sites.includes(x.id));
    const full = budgetFull(x.budget);
    const why = o?.config.paused ? '托管已全部暂停' : g?.paused ? `方案「${g.name}」已暂停` : g?.stopped.includes(x.id) ? '这个站点的托管暂停着' : full ? full : '';
    const fix = o?.config.paused ? '点右上角「恢复托管」后会接着做。' : g?.paused ? '恢复这个方案后会接着做。' : g?.stopped.includes(x.id) ? '把它的托管开关打开后会接着做。' : full ? '调高方案里的「每站每周 token」，或等用量滚出最近 7 天。' : '';
    // 能就地处理的：站点自己的开关关着 → 打开托管；额度满了 → 编辑方案。有计划的都可以清除计划
    const kind = o?.config.paused || g?.paused ? 'paused' : g?.stopped.includes(x.id) ? 'site' : full ? 'budget' : '';
    return { id: x.id, name: x.name, queued: x.queued, why, fix, kind, group: g?.id ?? '', hasPlan: x.plan_count > 0 };
  }).filter((x) => x.why));
  // 暂停只看站点自己的开关：所有站点都关着＝全部已暂停
  const allOff = $derived(groups.some((g) => g.sites.length) && groups.every((g) => g.sites.every((x) => g.stopped.includes(x))));
  let moreOpen = $state(false);
  let planMenu = $state('');
  // 弹出菜单：点外面关（遮罩），按 Esc 也关
  $effect(() => {
    if (!moreOpen && !planMenu) return;
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { moreOpen = false; planMenu = ''; } };
    document.addEventListener('keydown', esc);
    return () => document.removeEventListener('keydown', esc);
  });
  // 方案卡片展开 / 收起：点过的记下来；没点过的，暂停的方案收起、其余展开
  let grpOpen = $state<Record<string, boolean>>({});
  // 站点多的方案先显示 SITE_LIMIT 个，要你处理的、失败的、正在跑的排前面
  const SITE_LIMIT = 6;
  let showAll = $state<Set<string>>(new Set());
  function toggleShowAll(id: string) { const n = new Set(showAll); if (n.has(id)) n.delete(id); else n.add(id); showAll = n; }
  let siteMenu = $state('');
  // 方案里的站点搜索（每个方案各自一个）
  let siteQ = $state<Record<string, string>>({});
  const matchSite = (id: string, q: string) => { const k = q.toLowerCase(); const s = sitesById.get(id); return siteName(id).toLowerCase().includes(k) || !!s?.domain?.toLowerCase().includes(k); };
  function orderSites(g: ManagedGroup) {
    const rank = (id: string) => {
      const s = sitesById.get(id);
      if (s && (s.waiting || budgetFull(s.budget))) return 0;
      if (s?.job || sites.jobs[id]?.status === 'running') return 1;
      return g.stopped.includes(id) ? 3 : 2;
    };
    return [...g.sites].map((id, i) => ({ id, i, r: rank(id) })).sort((a, b) => a.r - b.r || a.i - b.i).map((x) => x.id);
  }
  /** 方案里各站点最近 14 天的计数按天加起来（站点按现在所在的方案算） */
  function groupDays(g: ManagedGroup) {
    const m = new Map<string, DayCount>();
    for (const id of g.sites) for (const d of managed.siteDaily[id] ?? []) {
      const e = m.get(d.date) ?? { date: d.date, shipped: 0, planned: 0, proposed: 0, broken: 0, ran: 0 };
      e.shipped += d.shipped; e.planned += d.planned; e.proposed += d.proposed; e.broken += d.broken; e.ran += d.ran;
      m.set(d.date, e);
    }
    return [...m.values()];
  }
  function groupSummary(g: ManagedGroup) {
    let shipped = 0, need = 0, failed = 0, next = 0, nextSite = '', shippedMonth = 0;
    const cost: AiCost = { tokens: 0, usd: 0, sub_tokens: 0, unpriced_tokens: 0, rough: false, turns: 0 };
    for (const id of g.sites) {
      const s = sitesById.get(id); if (!s) continue;
      shipped += s.shipped_week; failed += s.failed; need += s.waiting - s.failed;
      if (s.next_at && (!next || s.next_at < next)) { next = s.next_at; nextSite = s.name; }
      if (s.cost) {
        cost.tokens += s.cost.tokens; cost.usd += s.cost.usd; cost.sub_tokens += s.cost.sub_tokens; cost.unpriced_tokens += s.cost.unpriced_tokens; cost.turns += s.cost.turns; cost.rough ||= s.cost.rough;
        shippedMonth += s.shipped_month;
      }
    }
    return { shipped, need, failed, next, nextSite, cost, shippedMonth };
  }
  // 下一次自动运行：到点时间写成「马上 / 今天 15:00 / 明天 14:36 / 周五 16:27 / 11月3日 09:00」
  const NEXT_TASK: Record<string, string> = { scan: '扫描', health: '体检', plan: '制定计划', report: '周报', review: '复盘', watch: '上线后检查' };
  function whenNext(ts: number) {
    if (ts * 1000 <= Date.now() + 60_000) return '马上';
    const d = new Date(ts * 1000), t = new Date(); t.setHours(0, 0, 0, 0);
    const days = Math.floor((new Date(d).setHours(0, 0, 0, 0) - t.getTime()) / 86_400_000);
    const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
    if (days === 0) return `今天 ${hm}`;
    if (days === 1) return `明天 ${hm}`;
    if (days < 7) return `${'周' + '日一二三四五六'[d.getDay()]} ${hm}`;
    return `${d.getMonth() + 1}月${d.getDate()}日 ${hm}`;
  }
  const NEXT_TIP = '按上次运行时间推算的到点时刻；客户端要开着（托盘也行），每 10 分钟检查一次，实际会晚几分钟';
  /** 没放在卡片上的那些规则（「还有 N 项」的悬停说明） */
  function hiddenRules(g: ManagedGroup) {
    const acts = (g.actions ?? []).length === 5 ? '全部（标题描述、内容、内链、新页面、技术问题）' : (g.actions ?? []).map((a) => ACT_LABEL[a] ?? a).join('、') || '无';
    const lines = [`可以做的动作：${acts}`, `新页面：${g.new_content === 'auto' ? `检查通过直接上线${g.min_page_chars ? `，正文少于 ${g.min_page_chars} 字的留给你确认` : ''}` : '等你看过再上线'}`];
    if (g.fallback_brain) lines.push(`备用模型：${modelLabel(g.fallback_brain, g.fallback_model)}`);
    if (g.guidelines?.trim()) lines.push('有内容准则');
    if (g.topics?.trim()) lines.push(`${g.topics.split('\n').filter((l) => l.trim()).length} 个选题`);
    return lines;
  }
  let goalOpen = $state<Set<string>>(new Set());
  function toggleGoal(id: string) { const n = new Set(goalOpen); if (n.has(id)) n.delete(id); else n.add(id); goalOpen = n; }
  // 运营目标是编辑时勾的几个预设拼起来的（再加自己写的话）：拆回预设，卡片上显示成一排名称，
  // 两个方案开头一样时也一眼分得清；自己写的部分接在后面
  function goalParts(goal: string) {
    let rest = goal;
    const presets = GOAL_PRESETS.filter((p) => goal.includes(p.goal));
    for (const p of presets) rest = rest.replace(p.goal, '');
    return { presets, rest: rest.replace(/\s+/g, ' ').trim() };
  }
  const ACT_LABEL = Object.fromEntries(ACTIONS.map((a) => [a.key, a.label]));
  // 日报设置（全局）：只比较这两项
  let digestSetOpen = $state(false);
  const dirty = $derived(!!d && !!o && (d.digest !== o.config.digest || d.digest_hour !== o.config.digest_hour));

  const KIND: Record<string, { l: string; c: string; i: string }> = {
    shipped: { l: '已上线', c: 'ok', i: 'rocket' }, planned: { l: '计划', c: 'run', i: 'checks' }, proposed: { l: '待你确认', c: 'warn', i: 'bell' }, rolled_back: { l: '已回滚', c: 'err', i: 'undo' },
    failed: { l: '失败', c: 'err', i: 'x' }, fixed: { l: '已处理', c: 'ok', i: 'check' }, watch_ok: { l: '验收通过', c: '', i: 'check' }, watch_failed: { l: '验收没通过', c: 'err', i: 'x' }, attention: { l: '待核对', c: 'warn', i: 'search' }, downgraded: { l: '降档', c: 'warn', i: 'sliders' }, fallback: { l: '备用模型', c: 'warn', i: 'cpu' },
    budget: { l: '预算', c: 'warn', i: 'gauge' }, digest: { l: '日报', c: '', i: 'news' }, paused: { l: '暂停', c: 'warn', i: 'pause' }, resumed: { l: '恢复', c: 'ok', i: 'play' }, joined: { l: '开始托管', c: 'ok', i: 'plus' }, left: { l: '退出托管', c: '', i: 'x' }, plan_cleared: { l: '清除计划', c: '', i: 'x' },
  };
  const when = ago;
  // 「失败 N」的悬停说明：最近几条的标题和原因；余额不够、没登录这类要你先处理的，补一句下一步
  function failTip(s: { failed: number; failed_why?: [string, string][] }): string {
    const rows = (s.failed_why ?? []).map(([t, why]) => `· ${t}\n  ${why || '没有记下原因'}${turnErrorHint(why) ? `\n  → ${turnErrorHint(why)}` : ''}`);
    const more = s.failed > rows.length ? `\n还有 ${s.failed - rows.length} 条` : '';
    return `${rows.join('\n')}${more}\n\n点开去处理（重新检查、再交给 AI 或驳回）`;
  }
  const ratio = (a: number, b: number) => (b > 0 ? Math.min(1, a / b) : 0);
</script>

{#snippet jevRule(g: ManagedGroup, rule: 'jev_gate' | 'skip_offtopic', label: string, what: string)}
  {@const on = g[rule] !== false}
  <button type="button" class="rule-tg" class:off={!on} role="switch" aria-checked={on} aria-label={`${label}：${on ? '开' : '关'}`}
    data-tip={`${what}\n现在${on ? '开着，点一下关掉' : '关着，点一下打开'}`} onclick={() => setGroupRule(g, rule, !on)}>
    <TypeSafeMark size={12} />{label}<span class="tg" aria-hidden="true"><i></i></span>
  </button>
{/snippet}


<ManagedGroupEditor />
<div class="su page mc">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="page-head" data-tauri-drag-region onmousedown={startDrag}>
    <div class="ttl">
      <h1>托管运营<InfoTip text={'每个方案管一个或多个站点，各有自己的自主程度和规则；一个站点只属于一个方案，不在方案里的站点由你自己运营。\n方案和站点都能单独暂停。'} label="托管方案说明" /> {#if allOff}<span class="pill warn">全部已暂停</span>{/if}</h1>
      <p class="hint">{o ? `托管 ${o.sites.length} 个站点 · 今天 AI 自动上线 ${o.shipped_today} 项 · ${o.waiting} 项需要你` : '读取中…'}{#if o && costText(o.cost_month)}{' · '}<span class="cost-h" data-tip={costTip(o.cost_month, o.shipped_month, '所有托管站点')}>本月 AI {costText(o.cost_month)}</span>{/if}{#if o?.revenue_tracked}{' · '}<span class="cost-h" data-tip="所有托管站点这个月的收益（AdSense + 手动记的，美元）">本月收益 {usd(o.revenue_month ?? 0)}</span>{/if}</p>
    </div>
    <div class="row">
      <button class="btn small primary" onclick={() => newGroup()}><Icon name="plus" size={13} />新增方案</button>
      <!-- 不常用的整体操作收进一个菜单：生成今日日报、暂停 / 恢复全部 -->
      {#if o?.sites.length}
      <div class="more-wrap">
        <button class="ic-btn" class:on={moreOpen} onclick={() => (moreOpen = !moreOpen)} aria-label="托管运营的整体操作" aria-haspopup="menu" aria-expanded={moreOpen} data-tip={moreOpen ? null : '生成日报、暂停 / 恢复全部'}><Icon name="sliders" size={15} /></button>
        {#if moreOpen}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="more-mask" onclick={() => (moreOpen = false)}></div>
          <div class="more-pop" role="menu">
              <button role="menuitem" onclick={() => { moreOpen = false; void digestNow(); }}><Icon name="news" size={14} /><span><b>生成今日日报</b><small>把今天各站点做了什么汇总成一份</small></span></button>
              <button role="menuitem" onclick={() => { moreOpen = false; void setSitesActive(groups, allOff, '全部托管站点'); }}><Icon name={allOff ? 'play' : 'pause'} size={14} /><span><b>{allOff ? '恢复全部站点的托管' : '暂停全部站点的托管'}</b><small>{allOff ? '把所有站点的托管开关都打开' : '把所有站点的托管开关都关掉，之后可以一个个恢复'}</small></span></button>
          </div>
        {/if}
      </div>
      {/if}
    </div>
  </header>
  <nav class="tabs">
    {#each TABS as t (t.k)}<button class:on={managed.tab === t.k} onclick={() => (managed.tab = t.k)}><Icon name={t.i} size={13} />{t.l}{#if t.k === 'needs' && o?.waiting}<span class="cnt">{o.waiting}</span>{:else if t.k === 'needs' && stuck.length}<i class="stuck-dot" data-tip="{stuck.reduce((n, x) => n + x.queued, 0)} 条任务因为{stuck[0].why}排着没动，不用你确认"></i>{/if}</button>{/each}
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
      <PlansView sites={o?.sites ?? []} plans={managed.plans} />
    {:else if managed.tab === 'needs'}
      <NeedsView {needs} {stuck} onturnon={(id) => setSiteActive(id, true)} onbudget={(gid) => { const g = groups.find((x) => x.id === gid); if (g) editGroup(g); }} />
    {:else if managed.tab === 'digests'}
      <div class="card">
        <h3><Icon name="news" />日报 <span class="grow"></span><button class="btn tiny" onclick={digestNow}><Icon name="news" size={11} />生成今日日报</button></h3>
        {#if !managed.digests.length}<p class="hint">还没有日报。每天{o?.config.digest_hour ?? 9} 点后自动生成一份，并发系统通知。</p>{/if}
        {#if managed.digests.length}<DigestCalendar digests={managed.digests} daily={managed.daily} selected={managed.digest?.name ?? null} onpick={openDigest} />{/if}
        {#if managed.digest?.data}<DigestView data={managed.digest.data} />{:else if managed.digest}<div class="md">{@html mdRender(managed.digest.text)}</div>{/if}
      </div>
      {#if d}
        {@const setOpen = digestSetOpen || dirty}
        <div class="card">
          <h3 style:margin-bottom={setOpen ? null : '0'}><button class="set-toggle" onclick={() => (digestSetOpen = !setOpen)} aria-expanded={setOpen}><Icon name="calendar" />日报设置 <span class="sub">{o?.config.digest ? `每天 ${o.config.digest_hour} 点后 · ` : '已关闭 · '}所有托管方案共用</span>
            <svg class="chev" class:up={setOpen} width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button></h3>
          {#if setOpen}
          <div class="row">
            <label class="check"><input type="checkbox" bind:checked={d.digest} /><span>每天发一份托管日报（系统通知 + 报告）</span></label>
            {#if d.digest}<span class="faint" style="font-size:12px">时间</span><input class="tin" type="number" min="0" max="23" style="width:70px" bind:value={d.digest_hour} /><span class="faint" style="font-size:12px">点之后</span>{/if}
          </div>
          <p class="hint">托管需要客户端在运行（关掉窗口后托盘常驻即可）；电脑关机期间不会运营，开机后补跑一次。</p>
          {/if}
        </div>
        {#if dirty}<div class="savebar"><span>日报设置有未保存的修改</span><span class="grow"></span><button class="btn small ghost" onclick={() => o && (managed.draft = JSON.parse(JSON.stringify(o.config)))}>撤销</button><button class="btn small primary" onclick={saveManaged} disabled={managed.saving}>保存</button></div>{/if}
      {/if}
    {:else}
      <BackgroundNotice />
      {#if groups.length}<RunChart daily={managed.daily} digests={managed.digests} onpick={pickDay} />{/if}
      {#each groups as g (g.id)}
        {@const gOff = g.sites.length > 0 && g.sites.every((x) => g.stopped.includes(x))}
        {@const gOpen = grpOpen[g.id] ?? !gOff}
        {@const gq = (siteQ[g.id] ?? '').trim()}
        {@const sum = groupSummary(g)}
        {@const ordered = gq ? orderSites(g).filter((id) => matchSite(id, gq)) : orderSites(g)}
        {@const visible = showAll.has(g.id) || gq ? ordered : ordered.slice(0, SITE_LIMIT)}
        <div class="card grp" class:off={gOff} class:shut={!gOpen}>
          <h3><button class="grp-tg" onclick={() => (grpOpen[g.id] = !gOpen)} aria-expanded={gOpen} data-tip={gOpen ? '收起' : '展开'}><Icon name="sparkles" />{g.name}<span class="pill line">{AUTONOMY_LABEL[g.autonomy] ?? '全自动'}</span>{#if gOff}<span class="pill warn">已暂停</span>{/if}
            <span class="sub gsum">{g.sites.length} 个站点{#if sum.shipped}{' · '}本周上线 {sum.shipped}{/if}{#if sum.need}{' · '}<span class="warn-text">{sum.need} 项要你处理</span>{/if}{#if sum.failed}{' · '}<span class="err-text">{sum.failed} 项失败</span>{/if}{#if costText(sum.cost)}{' · '}<span data-tip={costTip(sum.cost, sum.shippedMonth, '这个方案')}>本月 AI {costText(sum.cost)}</span>{/if}</span>
            {#if g.sites.length}<PlanSpark days={groupDays(g)} />{/if}
            {#if sum.next}<span class="next-chip" data-tip={`${sum.nextSite} 最先：${NEXT_TIP}`}><Icon name="clock" size={11} />下次 {whenNext(sum.next)}</span>{/if}
            <svg class="chev" class:up={gOpen} width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
            <!-- 方案的操作收进一个菜单：编辑、暂停 / 恢复 -->
            <span class="more-wrap">
              <button class="ic-btn ic-sm" class:on={planMenu === g.id} onclick={() => (planMenu = planMenu === g.id ? '' : g.id)} aria-label="这个方案的操作" aria-haspopup="menu" aria-expanded={planMenu === g.id} data-tip={planMenu === g.id ? null : '编辑、暂停这个方案'}><Icon name="more" size={16} /></button>
              {#if planMenu === g.id}
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <span class="more-mask" onclick={() => (planMenu = '')}></span>
                <span class="more-pop" role="menu">
                  <button role="menuitem" onclick={() => { planMenu = ''; editGroup(g); }}><Icon name="pencil" size={14} /><span><b>编辑方案</b><small>目标、自主程度、规则和站点</small></span></button>
                  {#if g.sites.length}<button role="menuitem" onclick={() => { planMenu = ''; void setSitesActive([g], gOff, `「${g.name}」`); }}><Icon name={gOff ? 'play' : 'pause'} size={14} /><span><b>{gOff ? '恢复方案' : '暂停方案'}</b><small>{gOff ? '把这个方案里的站点都打开' : '把这个方案里的站点都暂停'}</small></span></button>{/if}
                </span>
              {/if}
            </span>
          </h3>
          {#if gOpen}
          {#if g.goal?.trim()}
            {@const gp = goalParts(g.goal)}
            <!-- 运营目标：预设显示成小标签（悬停看完整说明），自己写的话接在后面、默认一行，点开看全文 -->
            <div class="goal" class:open={goalOpen.has(g.id)}>
              <span class="goal-lbl"><Icon name="target" size={12} />目标</span>
              {#each gp.presets as p (p.key)}<span class="goal-pill" data-tip={p.goal}>{p.name}</span>{/each}
              {#if gp.rest}
                <button class="goal-t" onclick={() => toggleGoal(g.id)} data-tip={goalOpen.has(g.id) ? '收起' : `点开看全文：${gp.rest}`}>{gp.rest}</button>
              {/if}
            </div>
          {/if}
          <!-- 规则只留一行最要紧的：模型、怎么上线、每天几条、每周用量、两个判断模型开关；其余（动作、内容准则、新页规则、备用模型）在「规则详情」里 -->
          <div class="rules">
            <span class="model" data-tip={g.fallback_brain ? `备用：${modelLabel(g.fallback_brain, g.fallback_model)}（主模型不可用时换它）` : null}><BrainIcon brain={modelIcon(g.brain)} size={12} /><span class="trunc">{modelLabel(g.brain, g.model)}</span>{#if g.brain === 'codex' || g.fallback_brain === 'codex'}<span class="codex-off" data-tip="Codex 改代码时不联网（没法逐条确认它的操作，所以只让它在单独的文件夹里改）；推送和上线由 Pilot 来做">不联网改</span>{/if}</span>
            {#if g.autonomy === 'auto'}<span data-tip={CODE_LABEL[g.code_changes ?? 'all']}><Icon name="code" size={12} />{({ review: '人审上线', small: '小改自动上线', all: '自动上线' } as Record<string, string>)[g.code_changes ?? 'all']}</span>{/if}
            <span data-tip={`运营节奏：${cadenceLabel(g)}跑一轮，每次挑最值得做的 ${g.per_round} 个机会交给 AI（平均每周最多 ${perWeek(g)} 个）；该自动上线的改好就上线`}><Icon name="rocket" size={12} />{cadenceLabel(g)} · 每轮 {g.per_round} 个</span>
            <span data-tip="每个站点每周的模型用量上限，用完暂停到下周"><Icon name="gauge" size={12} />每周 {g.tokens_per_week ? `${kTok(g.tokens_per_week)} token` : '不限'}</span>
            {#if jev.configured}
              <!-- 判断模型的两条规则：点一下就开 / 关，不用进编辑器 -->
              {#if g.autonomy === 'auto'}{@render jevRule(g, 'jev_gate', '上线前质检', `自动上线前让判断模型（Jev）检查新页面和大改的页面，${g.jev_rewrite !== false ? '不过先让 AI 按原因重写一次，还不过才留给你确认' : '不过就留给你确认'}`)}{/if}
              {@render jevRule(g, 'skip_offtopic', '跳过无关搜索词', '判断模型（Jev）认为和站点定位基本无关的搜索词不交给 AI 处理')}
            {/if}
            <button type="button" class="rule-more" onclick={() => editGroup(g)} data-tip={`${hiddenRules(g).join('\n')}\n点一下查看或修改`}>还有 {hiddenRules(g).length} 项<svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
          </div>
          {#if g.sites.length > 5}
            <!-- 站点多了不好找：在这个方案里按站点名 / 域名筛 -->
            <div class="gq-bar">
              <label class="gq"><Icon name="search" size={13} /><input type="search" bind:value={siteQ[g.id]} placeholder="搜索这个方案的站点" spellcheck="false" autocomplete="off" onkeydown={(e) => { if (e.key === 'Escape') siteQ[g.id] = ''; }} /></label>
              {#if gq}<span class="faint gq-n">{ordered.length ? `找到 ${ordered.length} 个` : '没有匹配的站点'}</span>{/if}
            </div>
          {/if}
          {#if g.sites.length}
            <div class="list gsites">
              {#each visible as id (id)}
                {@const s = sitesById.get(id)}
                {@const on = !g.stopped.includes(id)}
                {@const need = s ? s.waiting - s.failed : 0}
                <div class="li gs" class:stopped={!on}>
                  <button class="gs-name" onclick={() => openSite(id)}><SiteIcon {id} size={15} /><span>{siteName(id)}</span></button>
                  {#if s}<span class="stg {s.stage}" data-tip={s.stage_manual ? '手动指定的阶段' : '按 Search Console 数据自动判断'}>{STAGE_LABEL[s.stage] ?? s.stage}</span>{/if}
                  {#if s}
                    <span class="gs-meta">
                      {#if s.shipped_week}<button class="ship-l" onclick={() => openSite(id, 'queue', 'live')} data-tip="看这个站点的上线记录（任务队列 · 已上线）">本周上线 {s.shipped_week}</button>{/if}
                      {#if need > 0}<button class="ship-l warn-text" onclick={() => openSite(id, 'queue', 'todo')} data-tip="打开这个站点的「需要你处理」：确认上线、驳回或重新检查">待你处理 {need}</button>{/if}
                      {#if s.failed}<button class="ship-l err-text" onclick={() => openSite(id, 'queue', 'todo')} data-tip={failTip(s)}>失败 {s.failed}</button>{/if}
                      {#if s.queued && on}<span class="faint">排队 {s.queued}</span>{/if}
                      {#if s.money?.tracked}<span class="faint cost" data-tip={`本月收益（AdSense + 手动记的）；上月 ${usd(s.money.last_month)}`}>收益 {usd(s.money.month)}</span>{/if}
                      {#if costText(s.cost)}<span class="faint cost" data-tip={costTip(s.cost, s.shipped_month)}>{s.money?.tracked ? 'AI' : '本月'} {costText(s.cost)}</span>{/if}
                      {#if budgetFull(s.budget)}<span class="warn-text" data-tip="{budgetFull(s.budget)}。额度满了 AI 不再处理这个站点的任务，调高方案里的「每站每周 token」或等用量滚出最近 7 天">额度已满</span>{/if}
                    </span>
                  {/if}
                  {#if s?.job || sites.jobs[id]?.status === 'running'}
                    <button class="pill run gs-job" onclick={() => toggleProgress(id)} aria-expanded={progress.has(id)} data-tip="看进度"><span class="spin"></span>{s?.job || sites.jobs[id]?.title}中<Icon name={progress.has(id) ? 'x' : 'arrow'} size={10} /></button>
                  {:else if progress.has(id) && sites.jobs[id]}
                    <button class="btn tiny" onclick={() => toggleProgress(id)}>收起结果</button>
                  {/if}
                  <span class="grow"></span>
                  {#if on && s?.next_at}<span class="next-t" data-tip={NEXT_TIP}><Icon name="clock" size={11} />下次 {whenNext(s.next_at)} {NEXT_TASK[s.next_task] ?? s.next_task}</span>{/if}
                  <!-- 不常用的：立即执行、移到别的方案、退出托管，收进悬停才出现的菜单 -->
                  <span class="more-wrap gs-more" class:on={siteMenu === id}>
                    <button class="ic-btn ic-sm" onclick={() => (siteMenu = siteMenu === id ? '' : id)} aria-label="这个站点的操作" aria-haspopup="menu" aria-expanded={siteMenu === id} data-tip={siteMenu === id ? null : '立即执行、移到别的方案'}><Icon name="more" size={15} /></button>
                    {#if siteMenu === id}
                      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                      <span class="more-mask" onclick={() => (siteMenu = '')}></span>
                      <span class="more-pop" role="menu">
                        <button role="menuitem" disabled={!!s?.job} onclick={() => { siteMenu = ''; void runNow(id); }}><Icon name="play" size={14} /><span><b>立即执行</b><small>沿用或制定本周计划，再按方案改排在前面的几条</small></span></button>
                        {#each groups.filter((x) => x.id !== g.id) as x (x.id)}
                          <button role="menuitem" onclick={() => { siteMenu = ''; void moveSite(id, g.id, x.id); }}><Icon name="arrow" size={14} /><span><b>移到「{x.name}」</b><small>{AUTONOMY_LABEL[x.autonomy] ?? ''}</small></span></button>
                        {/each}
                        <button role="menuitem" onclick={() => { siteMenu = ''; void moveSite(id, g.id, '__new'); }}><Icon name="plus" size={14} /><span><b>新建方案…</b><small>用这个站点新建一个方案</small></span></button>
                        <button role="menuitem" onclick={() => { siteMenu = ''; void moveSite(id, g.id, '__out'); }}><Icon name="x" size={14} /><span><b>退出托管</b><small>改为自己运营</small></span></button>
                      </span>
                    {/if}
                  </span>
                  <button class="switch" class:on role="switch" aria-checked={on} aria-label="{on ? '停止' : '启用'}托管 {siteName(id)}" data-tip={on ? '托管中，点一下暂停这个站点（规则保留）' : '已暂停，点一下恢复托管'} onclick={() => setSiteActive(id, !on)}><i></i></button>
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
              {#if ordered.length > SITE_LIMIT && !gq}
                <button class="li gs-all" onclick={() => toggleShowAll(g.id)}>{showAll.has(g.id) ? '收起' : `显示全部 ${ordered.length} 个站点`}</button>
              {/if}
            </div>
          {:else}<p class="hint">这个方案还没有站点。<button class="link-btn" onclick={() => editGroup(g)}>选择站点</button></p>{/if}
          {/if}
        </div>
      {/each}

      {#if groups.length || managed.activity.length}
        <!-- 记录：最近上线 / 操作日志 两个 tab，同一张卡片 -->
        <div class="card hist" bind:this={logEl}>
          <div class="hist-h">
            <div class="seg" role="tablist" aria-label="记录">
              <button role="tab" class:on={histTab === 'ships'} aria-selected={histTab === 'ships'} onclick={() => setHist('ships')}><Icon name="rocket" size={12} />最近上线<span class="hc">{managed.ships.length}</span></button>
              <button role="tab" class:on={histTab === 'log'} aria-selected={histTab === 'log'} onclick={() => setHist('log')}><Icon name="activity" size={12} />操作日志<span class="hc">{managed.activity.length}</span></button>
            </div>
            {#if histTab === 'log' && logDay}<span class="day-chip">只看 {+logDay.slice(5, 7)}月{+logDay.slice(8, 10)}日 · {feed.length} 条<button class="x" aria-label="看全部" data-tip="看全部" onclick={() => (logDay = '')}>×</button></span>{/if}
            <span class="grow"></span>
            <span class="sub">{histTab === 'ships' ? '近 30 天，所有托管站点' : managed.activity.length ? `最近 ${when(managed.activity[0].ts)}` : ''}</span>
          </div>
          {#if histTab === 'ships'}
            <RecentShips items={managed.ships} />
          {:else}
            {#if !managed.activity.length}<p class="hint">托管站点的计划、上线、检查、回滚、暂停都会记在这里。</p>{:else if !feed.length}<p class="hint">这一天没有操作记录。</p>{/if}
            <div class="list feed">
              {#each feed.slice(0, logAll ? feed.length : LOG_LIMIT) as { a, k } (k)}
                <div class="li ev">
                  <span class="pill {KIND[a.kind]?.c ?? ''}"><Icon name={KIND[a.kind]?.i ?? 'activity'} size={11} />{KIND[a.kind]?.l ?? a.kind}</span>
                  <div class="ev-main">
                    <b>{a.title}</b>
                    {#if a.change || a.source}<div class="ev-why"><WhyChips change={a.change} source={a.source} /></div>{/if}
                    <small>{#if a.site_name}<button class="link-btn plain site-l" onclick={() => openSite(a.site_id)}><SiteIcon id={a.site_id} size={11} />{a.site_name}</button>{/if}{#if a.site_name && a.detail}<span class="sep">·</span>{/if}{a.detail}</small>
                  </div>
                  <span class="ev-r">
                    <span class="faint t" data-tip={full(a.ts)}>{when(a.ts)}</span>
                    {#if a.item_id && a.site_id}<button class="link-btn plain see" onclick={() => openSite(a.site_id, 'queue', '', a.item_id)}>看详情</button>{/if}
                  </span>
                </div>
              {/each}
              {#if feed.length > LOG_LIMIT}<button class="li log-more" onclick={() => (logAll = !logAll)}>{logAll ? '收起' : `显示更多（共 ${feed.length} 条）`}</button>{/if}
            </div>
          {/if}
        </div>
      {/if}
    {/if}
   </div>
   {/key}
  </div>
</div>

<style>
  .bad { color: var(--err); font-weight: 600; }
  .hist-h { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; min-width: 0; }
  .hist-h .seg button { display: inline-flex; align-items: center; gap: 5px; }
  .hist-h .hc { font-size: 11px; color: var(--faint); font-weight: 400; font-variant-numeric: tabular-nums; }
  .hist-h .sub { font-size: 12px; color: var(--faint); white-space: nowrap; }
  .hist-h .grow { flex: 1; }
  .day-chip .x { border: 0; background: none; padding: 0; font: inherit; color: inherit; }
  .log-more { display: block; width: 100%; border: 0; background: none; font: inherit; font-size: 12px; color: var(--dim); text-align: center; cursor: pointer; }
  .log-more:hover { color: var(--text); }
  .day-chip { display: inline-flex; align-items: center; gap: 4px; height: 20px; padding: 0 4px 0 8px; border-radius: 999px; background: var(--accent-soft); font-size: 11px; font-weight: 400; color: var(--dim); }
  .day-chip .x { display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 16px; border-radius: 50%; cursor: pointer; font-size: 13px; line-height: 1; }
  .day-chip .x:hover { background: var(--su-hover); color: var(--text); }
  .grp.off { background: #fcfbf9; }
  .more-wrap { position: relative; display: inline-flex; font-weight: 400; }
  .ic-btn.on { background: var(--su-hover); color: var(--text); }
  .more-mask { position: fixed; inset: 0; z-index: 30; }
  .more-pop { position: absolute; right: 0; top: calc(100% + 6px); z-index: 31; width: max-content; min-width: 180px; max-width: 240px; padding: 4px; border: 1px solid var(--border); border-radius: 12px; background: var(--card, #fff); box-shadow: 0 12px 32px rgba(0, 0, 0, .12); display: flex; flex-direction: column; gap: 2px; }
  .more-pop button { display: flex; align-items: flex-start; gap: 8px; padding: 7px 10px 7px 8px; border: none; border-radius: 8px; background: none; font: inherit; text-align: left; color: var(--text); cursor: pointer; }
  .more-pop button:hover { background: var(--su-hover); }
  .more-pop button :global(svg) { flex: none; margin-top: 2px; color: var(--dim); }
  .more-pop span { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .more-pop b { font-size: 13px; font-weight: 500; }
  .more-pop small { font-size: 11px; color: var(--faint); line-height: 1.4; }
  /* 运营目标：方案里最重要的一条，比下面的规则深一级；预设是胶囊，自己写的话一行省略、点开看全文 */
  .goal { display: flex; align-items: center; gap: 5px; min-width: 0; margin: -4px 0 10px; font-size: 12.5px; color: var(--text); }
  .goal.open { flex-wrap: wrap; }
  .goal-lbl { flex: none; display: inline-flex; align-items: center; gap: 5px; margin-right: 4px; color: var(--dim); font-size: 12px; }
  .goal-pill { flex: none; display: inline-flex; align-items: center; height: 20px; padding: 0 8px; border-radius: 999px; background: var(--accent-soft); font-size: 11.5px; color: var(--dim); white-space: nowrap; cursor: default; }
  .goal-t { flex: 1; min-width: 0; margin-left: 4px; padding: 0; border: 0; background: none; font: inherit; font-size: 12.5px; color: var(--dim); text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  .goal.open .goal-t { white-space: normal; flex-basis: 100%; margin-left: 0; color: var(--text); line-height: 1.6; }
  .goal-t:hover { color: var(--text); }
  /* 一行放下：每项不折行，模型名和动作列表太长时省略号收起；窄到放不下时才换行 */
  .rules { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 14px; font-size: 12px; color: var(--dim); margin: -4px 0 10px; min-width: 0; }
  .rules > span { display: inline-flex; align-items: center; gap: 5px; flex: none; white-space: nowrap; }
  .rules > span.model { flex: 0 1 auto; min-width: 0; }
  /* 「还有 N 项」：不是一条规则，是看其余规则的入口，所以不带图标、颜色最淡 */
  .rules > .rule-more { flex: none; display: inline-flex; align-items: center; gap: 2px; border: 0; background: none; padding: 0; font: inherit; font-size: 12px; color: var(--faint); white-space: nowrap; cursor: pointer; transition: color .12s; }
  .rules > .rule-more:hover { color: var(--text); }
  .rules .trunc { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .rules :global(svg) { flex: none; }
  @container page (max-width: 760px) { .rules { flex-wrap: wrap; } }
  .rules :global(.ico) { color: var(--faint); }
  .codex-off { margin-left: 4px; font-size: 11px; color: var(--su-warn-ink); background: var(--su-warn-soft); border-radius: 999px; padding: 0 6px; }
  /* 判断模型规则的小开关：标签本身可点，右边一个迷你开关表示开 / 关 */
  .rule-tg { display: inline-flex; align-items: center; gap: 5px; border: 0; background: none; padding: 1px 0; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; }
  .rule-tg:hover { color: var(--text); }
  .rule-tg.off { color: var(--faint); }
  .rule-tg.off :global(img) { filter: grayscale(1); opacity: .5; }
  .rule-tg .tg { position: relative; width: 22px; height: 12px; border-radius: 999px; background: var(--ok, #12805c); transition: background-color .15s; margin-left: 2px; }
  .rule-tg .tg i { position: absolute; top: 2px; left: 12px; width: 8px; height: 8px; border-radius: 50%; background: #fff; transition: left .15s; }
  .rule-tg.off .tg { background: var(--border2, #d6d3cb); }
  .rule-tg.off .tg i { left: 2px; }
  .gsites { margin-bottom: -8px; }
  .gs { display: flex; align-items: center; gap: 10px; padding-top: 8px; padding-bottom: 8px; }
  .gs-name { display: inline-flex; align-items: center; gap: 8px; border: none; background: none; padding: 0; font: inherit; font-size: 13px; font-weight: 550; color: var(--text); cursor: pointer; min-width: 0; }
  .gs-name span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .gs-name:hover span { text-decoration: underline; text-underline-offset: 3px; text-decoration-color: var(--border2); }
  .stg { flex: none; display: inline-flex; align-items: center; height: 18px; padding: 0 7px; border-radius: 999px; font-size: 10.5px; font-weight: 600; background: var(--su-warn-soft); color: var(--su-warn-ink); }
  .stg.growing { background: var(--su-run-soft); color: var(--su-run); }
  .stg.established { background: var(--su-ok-soft); color: var(--ok); }
  .gs-job { flex: none; border: none; cursor: pointer; gap: 5px; }
  .gs-prog { padding-top: 4px; padding-bottom: 14px; background: #fcfbf9; }
  .gs-prog :global(pre.log) { max-height: 200px; }
  .gp-acts { margin-top: 10px; gap: 6px; }
  .gs-meta { display: inline-flex; align-items: center; gap: 10px; font-size: 11.5px; white-space: nowrap; }
  .su .err-text { color: var(--err); }
  .gsum .warn-text, .gsum .err-text { font-weight: 400; }
  .gs.stopped .gs-name { color: var(--dim); font-weight: 500; }
  /* 站点的「…」菜单：平时隐藏，悬停这一行或打开时才出现 */
  .gs-more { opacity: 0; transition: opacity .12s; }
  .gs:hover .gs-more, .gs-more:focus-within, .gs-more.on { opacity: 1; }
  .gs-all { display: block; width: 100%; padding: 9px 18px; border: 0; border-top: 1px solid var(--border); background: none; font: inherit; font-size: 12px; color: var(--dim); text-align: center; cursor: pointer; }
  .gs-all:hover { color: var(--text); background: #fcfbf9; }
  .grp.shut > h3 { margin-bottom: 0; }
  /* 方案标题整行（名称、标签、概况、空白处）都能点开收起，箭头在右边 */
  .gq-bar { display: flex; align-items: center; gap: 12px; margin: 2px 0 10px; }
  .gq { position: relative; display: flex; align-items: center; width: min(220px, 100%); color: var(--faint); }
  .gq :global(svg) { position: absolute; left: 9px; pointer-events: none; }
  .gq input { width: 100%; height: 30px; padding: 0 10px 0 28px; border: 1px solid var(--border2); border-radius: 8px; background: var(--card); font: inherit; font-size: 12.5px; color: var(--text); outline: none; }
  .gq input:focus { border-color: #b7b2a6; }
  .gq-n { font-size: 12px; }
  /* 下一次运行：放在右边（方案标题上是箭头左边的绿色小胶囊，站点行在「…」和开关左边），一列对齐好扫 */
  .next-chip { flex: none; margin-left: auto; display: inline-flex; align-items: center; gap: 4px; height: 20px; padding: 0 8px; border-radius: 999px; background: var(--su-ok-soft); color: var(--ok); font-size: 11.5px; font-weight: 550; white-space: nowrap; }
  .next-t { flex: none; display: inline-flex; align-items: center; gap: 4px; font-size: 11.5px; color: var(--dim); white-space: nowrap; }
  .next-t :global(svg) { color: var(--ok); }
  .ship-l { border: 0; background: none; padding: 0; font: inherit; color: var(--faint); cursor: pointer; text-decoration: underline dotted var(--border2); text-underline-offset: 3px; }
  .ship-l:hover { color: var(--text); text-decoration-color: currentColor; }
  .ship-l.err-text, .ship-l.err-text:hover { color: var(--err); }
  .ship-l.warn-text, .ship-l.warn-text:hover { color: var(--warn); }
  .stuck-dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--faint); margin-left: 2px; }
  .grp-tg { flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; border: 0; background: none; padding: 0; font: inherit; color: inherit; cursor: pointer; text-align: left; }
  .grp-tg > :global(.ico) { color: var(--dim); margin-right: -1px; flex: none; }
  .grp-tg .chev { margin-left: auto; flex: none; color: var(--faint); transition: transform .16s; }
  .grp-tg .chev.up { transform: rotate(180deg); }
  .grp-tg:hover .chev { color: var(--dim); }
  /* 胶囊右边和下面站点行「下次…」的右边对齐：站点行右边是 10 + 「…」20 + 10 + 开关 32 = 72px，标题这边是 箭头 12 + 8 + 「…」20 = 40px，差的 32px 放在胶囊和箭头之间；实测还差 5px，按截图校正成 27px */
  .grp-tg .next-chip + .chev { margin-left: 27px; }
  .gs .grow { flex: 1; }
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
  .link-btn.plain { text-decoration: none; color: var(--dim); font-size: inherit; }
  .link-btn.plain:hover { color: var(--text); text-decoration: underline; }
  .x.mini { width: 16px; height: 16px; font-size: 13px; margin-right: -4px; }
  .md { margin-top: 14px; font-size: 13px; line-height: 1.75; border-top: 1px solid var(--border); padding-top: 12px; }
  .md :global(h1) { font-size: 16px; margin: 0 0 8px; }
  .md :global(h2) { font-size: 14px; margin: 16px 0 6px; }
  .md :global(ul) { padding-left: 18px; margin: 6px 0; }
  /* 和旁边「新增方案」小按钮一样高 */
  .page-head .more-wrap > .ic-btn { width: 26px; height: 26px; border-radius: 7px; }
  .set-toggle { flex: 1; width: 100%; display: flex; align-items: center; gap: 8px; border: 0; background: none; padding: 0; font: inherit; color: inherit; cursor: pointer; text-align: left; }
  .set-toggle .chev { margin-left: auto; flex: none; color: var(--faint); transition: transform .16s; }
  .set-toggle .chev.up { transform: rotate(180deg); }
  .set-toggle:hover .chev, .set-toggle:hover .sub { color: var(--dim); }
</style>
