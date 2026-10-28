<script lang="ts">
  import { escClose } from '$lib/escClose';
  // 新建 / 编辑托管方案：选站点（一个或多个）+ 这组站点自己的规则。
  import './site.css';
  import { untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import SiteIcon from './SiteIcon.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import { launcher } from '$lib/state/launcher.svelte.js';
  import { sites } from '$lib/state/sites.svelte.js';
  import { managed, cadenceLabel, perWeek, saveGroup, deleteGroup, ACTIONS, GUIDELINE_TEMPLATE, GOAL_PRESETS, AUTONOMY, AUTONOMY_LABEL, suggestPreset } from '$lib/state/managed.svelte.js';
  import { fleet } from '$lib/state/fleet.svelte.js';
  import { jev, loadJev } from '$lib/state/typesafe.svelte.js';
  import { openConnectionSetup } from '$lib/state/settings.svelte.js';
  import TypeSafeMark from '$lib/components/icons/TypeSafeMark.svelte';
  import InfoTip from '$lib/components/common/InfoTip.svelte';

  const g = $derived(managed.editor);
  const groups = $derived(managed.overview?.groups ?? []);
  const modelOpts = $derived(launcher.comboOpts.map((x) => ({ value: x.value, label: x.label, sub: x.value.startsWith('codex::') ? `${x.sub} · 托管时断网运行` : x.sub, icon: x.icon, disabled: x.disabled })));
  // Codex 没有逐条确认的钩子：托管时 Pilot 把它关在工作文件夹里并断网运行，选它要先看清楚代价
  const usesCodex = $derived(!!g && (g.brain === 'codex' || g.fallback_brain === 'codex'));
  const primary = $derived(g ? `${g.brain}::${g.model}` : '');
  // 主模型没选（或选的已不可用）：默认第一个可用的
  $effect(() => {
    if (!g || !modelOpts.length) return;
    if (!modelOpts.some((o) => o.value === primary && !o.disabled)) {
      const first = modelOpts.find((o) => !o.disabled);
      if (first) pick(first.value, 'primary');
    }
  });
  const fallback = $derived(g && g.fallback_brain ? `${g.fallback_brain}::${g.fallback_model}` : '');
  function pick(v: string, which: 'primary' | 'fallback') {
    if (!g) return;
    const i = v.indexOf('::');
    const [b, m] = i < 0 ? ['', ''] : [v.slice(0, i), v.slice(i + 2)];
    if (which === 'primary') { g.brain = b; g.model = m; } else { g.fallback_brain = b; g.fallback_model = m; }
  }

  let q = $state('');
  const otherOf = (id: string) => groups.find((x) => x.id !== g?.id && x.sites.includes(id));
  // 排序按打开时的选中状态算一次：勾选时那一行不会跳到最上面（选了哪些看上面的标签）
  let chosenAtOpen = $state<Set<string>>(new Set());
  let openedFor = '';
  $effect(() => { const k = `${g?.id ?? ''}:${managed.editor ? 1 : 0}`; if (k !== openedFor) { openedFor = k; chosenAtOpen = new Set(untrack(() => g?.sites ?? [])); } });
  let chosenOpen = $state(false);
  const list = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    const chosen = chosenAtOpen;
    return [...sites.list]
      .filter((s) => !needle || `${s.name} ${s.domain} ${s.tags.map((t) => `#${t}`).join(' ')}`.toLowerCase().includes(needle))
      // 已选的排前面，其次是还没进任何方案的
      .sort((a, b) => Number(chosen.has(b.id)) - Number(chosen.has(a.id)) || Number(!!otherOf(a.id)) - Number(!!otherOf(b.id)) || a.name.localeCompare(b.name, 'zh'));
  });
  function toggle(id: string) {
    if (!g) return;
    g.sites = g.sites.includes(id) ? g.sites.filter((x) => x !== id) : [...g.sites, id];
  }
  const moving = $derived((g?.sites ?? []).filter((id) => otherOf(id)).length);
  function close() { if (!managed.saving) managed.editor = null; }
  const isNew = $derived(!g?.id);
  // 三步：选站点 → 运营方式 → 发布与模型；新建时只有最后一步才能「开始托管」，编辑时可以随便切
  let step = $state<1 | 2 | 3>(1);
  const STEPS = ['选择站点', '运营方式', '发布与模型'];
  $effect(() => { void loadJev(); });
  function toggleAction(k: string) {
    if (!g) return;
    g.actions = g.actions.includes(k) ? g.actions.filter((x) => x !== k) : [...g.actions, k];
  }
  // 目标预设：可以多选，目标文字合并、动作取并集；衡量标准用第一个选中的
  // 站点阶段：和后端 site_stage 同一套阈值（近 28 天展现 < 200 新站，点击 < 200 成长期）
  const stageOf = (id: string) => {
    const r = fleet.rows.find((x) => x.id === id);
    if (!r || !r.has_data || !r.summary.fetched_at || r.summary.gsc_impressions < 200) return 'new';
    return r.summary.gsc_clicks < 200 ? 'growing' : 'established';
  };
  const suggested = $derived(suggestPreset((g?.sites ?? []).map(stageOf)));
  const picked = $derived(GOAL_PRESETS.filter((p) => !!g?.goal && g.goal.includes(p.goal)).map((p) => p.key));
  function togglePreset(key: string) {
    if (!g) return;
    const p = GOAL_PRESETS.find((x) => x.key === key);
    if (!p) return;
    const on = picked.includes(key);
    const keys = on ? picked.filter((k) => k !== key) : [...picked, key];
    const chosen = GOAL_PRESETS.filter((x) => keys.includes(x.key));
    if (on) {
      g.goal = g.goal.replace(p.goal, '').replace(/\n{2,}/g, '\n').trim();
    } else {
      g.goal = g.goal.trim() ? `${g.goal.trim()}\n${p.goal}` : p.goal;
    }
    if (chosen.length) {
      g.actions = ACTIONS.map((a) => a.key).filter((k) => chosen.some((c) => c.actions.includes(k)));
      g.metric = chosen[0].metric;
    }
  }
  const topicCount = $derived((g?.topics ?? '').split('\n').filter((l) => l.trim()).length);
  const canNext = $derived(!!g && !!g.name.trim() && (!isNew || g.sites.length > 0) && (step !== 2 || g.actions.length > 0));
  let lastFor = '';
  $effect(() => {
    const k = g ? g.id || 'new' : '';
    if (k === lastFor) return;
    lastFor = k; q = '';
    // 从方案卡片点「内容准则」进来：直接到第 2 步、光标放进准则框
    const focus = untrack(() => managed.editorFocus);
    step = focus === 'guidelines' ? 2 : 1;
    if (focus) {
      untrack(() => (managed.editorFocus = ''));
      if (focus === 'guidelines') requestAnimationFrame(() => { const el = document.getElementById('ge-guide') as HTMLTextAreaElement | null; el?.focus(); el?.scrollIntoView({ block: 'center' }); });
    }
  });
  const siteName = (id: string) => sites.list.find((x) => x.id === id)?.name ?? id;
  const WEEKDAYS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日'].map((l, n) => ({ value: String(n + 1), label: l }));
</script>

{#if g}
  <div class="su-mask" role="presentation" onclick={close}></div>
  <div class="su modal ge" use:escClose={close} role="dialog" aria-modal="true" aria-label={g.id ? '编辑托管方案' : '新建托管方案'}>
    <header class="sheet-head">
      <b><Icon name="sparkles" />{g.id ? '编辑托管方案' : '新建托管方案'}</b>
      <nav class="stepper" aria-label="步骤">
        {#each STEPS as label, i (label)}
          {#if i}<span class="sl"></span>{/if}
          <button class:on={step === i + 1} class:done={step > i + 1} onclick={() => (step = (i + 1) as 1 | 2 | 3)} disabled={isNew && i > 0 && (!g.sites.length || (i === 2 && !g.actions.length))}><span class="sn">{step > i + 1 ? '✓' : i + 1}</span>{label}</button>
        {/each}
      </nav>
      <button class="x" onclick={close} aria-label="关闭">×</button>
    </header>
    <div class="sheet-body">
     {#if step === 1}
      <div class="field"><label for="ge-name">方案名称</label><input id="ge-name" class="tin" bind:value={g.name} placeholder="例如：中文站点 · 只改标题描述" maxlength="40" /></div>

      <div class="field">
        <span class="lbl">站点
          <!-- 点「已选 N 个」浮出已选的站点：核对、去掉都在这里，不挤动下面的列表 -->
          <span class="ch-wrap">
            <button class="ch-btn" class:on={chosenOpen} onclick={() => (chosenOpen = !chosenOpen)} disabled={!g.sites.length} aria-haspopup="dialog" aria-expanded={chosenOpen}>已选 {g.sites.length} 个{#if g.sites.length}<svg width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M3 4.5 6 7.5 9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}</button>
            {#if chosenOpen && g.sites.length}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <span class="ch-mask" onclick={() => (chosenOpen = false)}></span>
              <span class="ch-pop" role="dialog" aria-label="已选的站点">
                <span class="ch-list">
                  {#each g.sites as id (id)}
                    {@const st = sites.list.find((x) => x.id === id)}
                    <span class="ch-row"><SiteIcon {id} size={14} /><span class="ch-n">{st?.name ?? id}</span><button class="ch-x" onclick={() => toggle(id)} aria-label="去掉 {st?.name ?? id}" data-tip="不选这个站点">×</button></span>
                  {/each}
                </span>
                <span class="ch-foot"><button class="link-btn" onclick={() => { if (g) g.sites = []; chosenOpen = false; }}>全部清空</button></span>
              </span>
            {/if}
          </span>
        </span>
        <div class="picker">
          <div class="pk-search"><Icon name="search" size={13} /><input bind:value={q} placeholder={`在 ${sites.list.length} 个站点里搜索`} spellcheck="false" /></div>
          <div class="pk-list">
            {#each list as s (s.id)}
              {@const other = otherOf(s.id)}
              <label class="pk-row" class:on={g.sites.includes(s.id)}>
                <input type="checkbox" checked={g.sites.includes(s.id)} onchange={() => toggle(s.id)} />
                <SiteIcon id={s.id} size={15} />
                <span class="pk-name">{s.name}</span>
                {#if other}<span class="pk-tag" data-tip="勾选会把它从「{other.name}」移到这个方案">在「{other.name}」里</span>{/if}
              </label>
            {:else}<p class="hint empty">没有匹配的站点</p>{/each}
          </div>
        </div>
        {#if moving}<small class="warn-text">{moving} 个站点会从原来的方案移到这里，改用这个方案的规则。</small>{/if}
      </div>

     {:else if step === 2}
      <div class="field"><span class="lbl">目标预设<InfoTip label="目标预设" text="点选会填好目标、勾上推荐的动作，都可以再改；可以多选" /></span>
        <div class="presets">
          {#each GOAL_PRESETS as p (p.key)}
            <button class="preset" class:on={picked.includes(p.key)} onclick={() => togglePreset(p.key)} aria-pressed={picked.includes(p.key)}>
              <span class="pr-top"><Icon name={p.icon} size={13} /><b>{p.name}</b>{#if p.key === suggested && !picked.length}<span class="rec">推荐</span>{/if}{#if picked.includes(p.key)}<Icon name="check" size={12} />{/if}</span>
              <small>{p.fits}</small>
            </button>
          {/each}
        </div>
      </div>
      <div class="field"><label for="ge-goal">运营目标<InfoTip label="运营目标" text="一两句话。每次制定计划、每个改动都会围绕它。" /></label>
        <textarea id="ge-goal" class="tin plain" rows="2" bind:value={g.goal} maxlength="500" placeholder="例如：提升中文页的 Google 自然流量，重点是费率、邀请和下载相关的查询词"></textarea>
      </div>
      <div class="field"><span class="lbl">允许的动作 <span class="faint cnt">健康巡查始终开启</span></span>
        <div class="acts">
          {#each ACTIONS as a (a.key)}
            <label class="act" class:on={g.actions.includes(a.key)}>
              <input type="checkbox" checked={g.actions.includes(a.key)} onchange={() => toggleAction(a.key)} />
              <span><b>{a.label}</b><small>{a.sub}</small></span>
            </label>
          {/each}
        </div>
        {#if !g.actions.length}<small class="warn-text">至少选一种，否则托管什么也不会做。</small>{/if}
      </div>
      <div class="field"><label for="ge-topics">选题清单<InfoTip label="选题清单" text="写新页面时优先从这里选（已有页面覆盖的会跳过）。还没有数据的新站主要靠它。" /> <span class="faint cnt">{topicCount ? `${topicCount} 个` : '可选'}</span></label>
        <textarea id="ge-topics" class="tin plain" rows="4" bind:value={g.topics} placeholder={'一行一个关键词或主题，例如：\ngate kyc 认证教程\ngate 手续费对比'}></textarea>
      </div>
      <div class="field">
        <div class="lbl-row"><label for="ge-guide">内容准则<InfoTip label="内容准则" text="原样交给 AI，写新页面和修改页面都必须遵守。默认模板只是起点，按你的站点改。" /></label><span class="grow"></span>
          {#if g.guidelines !== GUIDELINE_TEMPLATE}<button class="link-btn sm" onclick={() => g && (g.guidelines = GUIDELINE_TEMPLATE)}>恢复默认模板</button>{/if}
          {#if g.guidelines.trim()}<button class="link-btn sm" onclick={() => g && (g.guidelines = '')}>清空</button>{/if}
        </div>
        <textarea id="ge-guide" class="tin guide" rows="12" bind:value={g.guidelines} placeholder="写给 AI 的写作规则：读者、语气、结构、长度、事实要求、禁区……留空就不额外约束。"></textarea>
      </div>
     {:else}
      <div class="chosen">
        <div class="ch-top"><span class="lbl">「{g.name || '未命名'}」托管 {g.sites.length} 个站点</span><button class="ic-btn ic-sm" onclick={() => (step = 1)} aria-label="回去修改" data-tip="回去修改托管哪些站点"><Icon name="pencil" size={13} /></button></div>
        <div class="chips">{#each g.sites as id (id)}<span class="pill line"><SiteIcon {id} size={12} />{siteName(id)}</span>{/each}</div>
        <div class="ch-top"><span class="ch-sub">{g.goal.trim() || '没有写运营目标'} · 允许 {g.actions.length} 种动作{topicCount ? ` · ${topicCount} 个选题` : ''}{g.guidelines.trim() ? ' · 有内容准则' : ''}</span><button class="link-btn" onclick={() => (step = 2)}>修改</button></div>
      </div>
      <div class="sec"><Icon name="gauge" size={13} />AI 自主程度</div>
      <div class="auto">
        {#each AUTONOMY as a (a.key)}
          <button class="au" class:on={g.autonomy === a.key} onclick={() => g && (g.autonomy = a.key)} aria-pressed={g.autonomy === a.key} data-tip={a.sub}>
            <span class="au-top"><b>{a.label}</b>{#if g.autonomy === a.key}<Icon name="check" size={12} />{/if}</span>
            <small>{a.short}</small>
          </button>
        {/each}
      </div>
      {#if g.autonomy === 'auto'}
      <div class="sec"><Icon name="shield" size={13} />发布规则</div>
      <div class="field"><span class="lbl">新页面<InfoTip label="新页面" text={`整篇新内容。${g.new_content === 'auto' ? 'AI 写好的新页面检查通过就直接上线。' : 'AI 写好的新页面放进「需要你」，你看过点上线才发布。'}\n标题、描述、内链和已有页面的改动，检查通过就自动上线。`} /></span>
        <div class="seg"><button class:on={g.new_content === 'auto'} onclick={() => g && (g.new_content = 'auto')}>自动发布</button><button class:on={g.new_content === 'review'} onclick={() => g && (g.new_content = 'review')}>人审人发</button></div>
      </div>
      <div class="field"><span class="lbl">代码改动<InfoTip label="代码改动" text={`文章、页面写在代码里的站点会用到。\n${g.code_changes === 'review' ? '改到模板、组件、样式或站点脚本的，都放进「需要你」等你看过再上线。' : (g.code_changes ?? 'all') === 'all' ? '改模板、组件、样式和站点脚本，构建、检查和上线后验收都通过就自动上线；单次改动超过 400 行的除外。' : '代码部分合计不超过 80 行、5 个文件（文章正文这类数据不算）的自动上线，更大的留给你确认。'}\n不管选哪一档，依赖、构建 / 部署配置、Worker 和服务端代码、读环境变量或调外部服务、引入新的外部包、推荐 / 邀请相关内容，都不会自动上线；上线后验收没过会自动回滚。`} /></span>
        <div class="seg">
          <button class:on={(g.code_changes ?? 'all') === 'all'} onclick={() => g && (g.code_changes = 'all')}>都自动上线</button>
          <button class:on={g.code_changes === 'small'} onclick={() => g && (g.code_changes = 'small')}>小改动自动上线</button>
          <button class:on={g.code_changes === 'review'} onclick={() => g && (g.code_changes = 'review')}>留给你确认</button>
        </div>
      </div>
      <div class="field"><span class="lbl">运营节奏<InfoTip label="运营节奏" text={'到点跑一轮：拉数据 → 从这个站点没处理的机会里挑最值得做的几个交给 AI → 检查通过的按上面的规则上线（该自动上线的改好就上线，要你确认的留在「需要你」）。\n体检不花 AI 的钱，照样每天做；你手动交给 AI 的不受这个数限制。'} /></span>
        <div class="cad">
          <span class="rl">频率</span>
          <label class="pr" for="ge-every">每<input id="ge-every" class="tin" type="number" min="1" max={g.cadence === 'weekly' ? 8 : 30} bind:value={g.cadence_every} /></label>
          <div class="seg"><button class:on={(g.cadence ?? 'daily') === 'daily'} onclick={() => g && (g.cadence = 'daily')}>天</button><button class:on={g.cadence === 'weekly'} onclick={() => g && (g.cadence = 'weekly')}>周</button></div>
          {#if g.cadence === 'weekly'}<Dropdown compact menuCompact value={String(g.cadence_day ?? 1)} options={WEEKDAYS} ariaLabel="每周哪天跑" onchange={(v: string) => g && (g.cadence_day = Number(v))} />{/if}
          <span class="pr">跑一轮</span>
        </div>
        <div class="cad">
          <span class="rl">每轮</span>
          <label class="pr" for="ge-round">最多处理<input id="ge-round" class="tin" type="number" min="1" max="20" bind:value={g.per_round} />个机会</label>
          <InfoTip label="一个机会是什么" text={'一个机会＝一处具体的改进，例如：\n· 改一个页面的标题和描述\n· 扩写或更新一个已有页面\n· 写一篇新页面\n· 修一个技术问题（死链、sitemap、抓取错误）\n就是站点「待处理 → 未处理的机会」里的一条。'} />
        </div>
        <small>{cadenceLabel(g)}跑一轮，每次挑最值得做的 {g.per_round || 0} 个交给 AI；平均每周最多 {perWeek(g)} 个。</small>
      </div>
      <div class="grid2">
        <div class="field"><label for="ge-minp">新页面正文下限（字）<InfoTip label="新页面正文下限" text="低于它的新页面不自动上线，转进「需要你」由你判断（术语解释这类短页可以直接放行）。0＝不检查。" /></label><input id="ge-minp" class="tin" type="number" min="0" max="10000" step="100" bind:value={g.min_page_chars} /></div>
        <div class="field"><label for="ge-tok">每站每周模型用量上限（token）<InfoTip label="模型用量上限" text="用完就暂停这个站点到下周并通知你。0＝不限。" /></label><input id="ge-tok" class="tin" type="number" min="0" step="100000" bind:value={g.tokens_per_week} /></div>
      </div>
      <div class="field"><span class="lbl"><TypeSafeMark size={13} />上线前内容质检<InfoTip label="上线前内容质检" text={'判断模型（TypeSafe）。规则都过了、马上要自动上线时，再看一眼新页面和大改的页面：要讲清楚目标搜索问题、信息够具体、和站点定位相关、没有没来源的数字和遗留的说明文字。\n有一项不过：选「先让 AI 重写一次」就把原因交回 AI 改一遍、重新构建和质检，过了照常自动上线，还不过才留给你确认；选「直接留给你」就不重写。分数记在待处理条目上。'} /></span>
        <div class="seg"><button class:on={g.jev_gate !== false} onclick={() => g && (g.jev_gate = true)} disabled={!jev.configured}>做质检</button><button class:on={g.jev_gate === false} onclick={() => g && (g.jev_gate = false)} disabled={!jev.configured}>不做</button></div>
        {#if jev.configured && g.jev_gate !== false}
          <div class="sub-opt"><span>质检没过时</span>
            <div class="seg"><button class:on={g.jev_rewrite !== false} onclick={() => g && (g.jev_rewrite = true)}>先让 AI 重写一次</button><button class:on={g.jev_rewrite === false} onclick={() => g && (g.jev_rewrite = false)}>直接留给你</button></div>
          </div>
        {/if}
        {#if !jev.configured}<small>要先在<button class="link-btn" onclick={(e) => openConnectionSetup(e)}>设置 → 判断模型</button>里接上 TypeSafe，接上以后默认开着。</small>{/if}
      </div>

      {/if}
      <div class="sec"><Icon name="bulb" size={13} />运营机会</div>
      <div class="field"><span class="lbl"><TypeSafeMark size={13} />和站点定位无关的搜索词<InfoTip label="无关的搜索词" text="判断模型（TypeSafe）按站点定位给每条机会的搜索词打相关度，基本无关的（比如同名词误撞、别的领域）扫描时不入队，省下改动额度；看板里照样列出来、标着「相关度低」。站点要先写好定位。" /></span>
        <div class="seg"><button class:on={g.skip_offtopic !== false} onclick={() => g && (g.skip_offtopic = true)} disabled={!jev.configured}>跳过，不交给 AI</button><button class:on={g.skip_offtopic === false} onclick={() => g && (g.skip_offtopic = false)} disabled={!jev.configured}>照常处理</button></div>
        {#if !jev.configured}<small>要先在<button class="link-btn" onclick={(e) => openConnectionSetup(e)}>设置 → 判断模型</button>里接上 TypeSafe，接上以后默认跳过。</small>{/if}
      </div>
      <div class="sec"><Icon name="cpu" size={13} />模型</div>
      <div class="grid2">
        <div class="field"><span class="lbl">主模型</span><Dropdown bind:value={() => primary, (v) => pick(v, 'primary')} options={modelOpts} /></div>
        <div class="field"><span class="lbl">备用模型<InfoTip label="备用模型" text="主模型这一轮失败（额度用完、限流、不可用）时自动换备用模型重跑一次。" /></span><Dropdown bind:value={() => fallback, (v) => pick(v, 'fallback')} options={[{ value: '', label: '不用备用模型' }, ...modelOpts]} /></div>
      </div>
      {#if usesCodex}
        <div class="codex-warn" role="note">
          <b><Icon name="shield" size={13} />Codex（GPT 本地客户端）托管时会断网运行</b>
          <p>Codex 没有逐条确认的钩子，Pilot 拦不住它执行的命令。所以托管时把它关在这次改动的工作文件夹里、断开网络：它改不到别的文件，推送不了代码、上不了线，也传不出东西；推送和上线照常由 Pilot 在检查通过后执行。</p>
          <p>代价是它看不了外部网页、截不了图、装不了新依赖，写需要查资料的新页面效果会差一些。想要 GPT 又不受这些限制，可以在<button class="link-btn" onclick={(e) => openConnectionSetup(e)}>设置 → API 服务</button>里接入 OpenAI，用「API · OpenAI」的模型。</p>
        </div>
      {/if}
     {/if}
    </div>
    <footer class="sheet-foot">
      {#if g.id}<button class="btn small danger" onclick={() => g && deleteGroup(g)} disabled={managed.saving}><Icon name="trash" size={12} />删除方案</button>{/if}
      <span class="grow"></span>
      {#if step === 1}
        <button class="btn small ghost" onclick={close} disabled={managed.saving}>取消</button>
      {:else}
        <button class="btn small ghost" onclick={() => (step = (step - 1) as 1 | 2)} disabled={managed.saving}>上一步</button>
      {/if}
      {#if !isNew && step < 3}<button class="btn small ghost" onclick={saveGroup} disabled={managed.saving || !g.name.trim() || !g.actions.length}>保存</button>{/if}
      {#if step < 3}
        <button class="btn small primary" onclick={() => (step = (step + 1) as 2 | 3)} disabled={!canNext}>下一步：{STEPS[step]}<Icon name="arrow" size={12} /></button>
      {:else}
        <button class="btn small primary" onclick={saveGroup} disabled={managed.saving || !g.name.trim() || !g.actions.length}>{#if managed.saving}<span class="spin"></span>保存中{:else}{g.id ? '保存' : `开始托管 ${g.sites.length} 个站点（${AUTONOMY_LABEL[g.autonomy]}）`}{/if}</button>
      {/if}
    </footer>
  </div>
{/if}

<style>
  .cad { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; }
  .cad .pr { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--dim); }
  /* 这一行的输入框和天 / 周切换一样高（30px），读起来是一句话 */
  .cad .pr .tin { width: 60px; height: 30px; padding-top: 0; padding-bottom: 0; }
  .cad .seg { align-self: center; height: 30px; box-sizing: border-box; }
  .cad .seg button { padding-top: 0; padding-bottom: 0; }
  .cad :global(.dd-trigger.compact) { height: 30px; border-color: var(--border2); background: #fff; }
  .cad + .cad { margin-top: 8px; }
  .cad .rl { width: 32px; flex: none; font-size: 12.5px; color: var(--faint); }
  .ge { width: min(640px, calc(100vw - 32px)); }
  .cnt { font-weight: 400; font-size: 11.5px; margin-left: 4px; }
  .picker { border: 1px solid var(--border2); border-radius: 10px; overflow: hidden; }
  .ch-wrap { position: relative; display: inline-flex; margin-left: 6px; font-weight: 400; }
  .ch-btn { display: inline-flex; align-items: center; gap: 4px; border: none; background: none; padding: 1px 6px; margin: -1px 0; border-radius: 6px; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; }
  .ch-btn:hover:not(:disabled), .ch-btn.on { background: var(--su-hover); color: var(--text); }
  .ch-btn:disabled { color: var(--faint); cursor: default; }
  .ch-mask { position: fixed; inset: 0; z-index: 40; }
  .ch-pop { position: absolute; left: 0; top: calc(100% + 6px); z-index: 41; width: 260px; padding: 4px; border: 1px solid var(--border); border-radius: 10px; background: var(--card, #fff); box-shadow: 0 12px 32px rgba(0, 0, 0, .12); display: flex; flex-direction: column; }
  .ch-list { display: flex; flex-direction: column; max-height: 240px; overflow-y: auto; }
  .ch-row { display: flex; align-items: center; gap: 8px; padding: 5px 6px 5px 8px; border-radius: 6px; font-size: 12.5px; color: var(--text); }
  .ch-row:hover { background: var(--su-hover); }
  .ch-n { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ch-x { width: 20px; height: 20px; display: inline-flex; align-items: center; justify-content: center; border: none; border-radius: 5px; background: none; color: var(--faint); font-size: 15px; line-height: 1; cursor: pointer; }
  .ch-x:hover { background: var(--card, #fff); color: var(--text); }
  .ch-foot { display: flex; justify-content: flex-end; padding: 6px 8px 4px; margin-top: 2px; border-top: 1px solid var(--border); }
  .ch-foot .link-btn { font-size: 12px; color: var(--faint); }
  .ch-foot .link-btn:hover { color: var(--err); }
  .pk-search { display: flex; align-items: center; gap: 8px; padding: 7px 10px; border-bottom: 1px solid var(--border); color: var(--faint); }
  .pk-search input { flex: 1; min-width: 0; border: none; outline: none; background: none; font: inherit; font-size: 13px; color: var(--text); }
  .pk-list { max-height: min(340px, 44vh); overflow-y: auto; padding: 4px; }
  .pk-row { display: flex; align-items: center; gap: 9px; padding: 5px 8px; border-radius: 7px; cursor: pointer; font-size: 13px; transition: background-color .12s; }
  .pk-row:hover { background: var(--su-hover); }
  .pk-row input { accent-color: var(--accent); margin: 0; }
  .pk-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pk-row.on .pk-name { font-weight: 550; }
  .pk-tag { flex: none; font-size: 11px; color: var(--faint); }
  .empty { text-align: center; margin: 12px 0; }
  .sec { display: flex; align-items: center; gap: 6px; margin-top: 6px; padding-top: 12px; border-top: 1px solid var(--border); font-size: 12.5px; font-weight: 600; color: var(--text); }
  .sec :global(.ico) { color: var(--dim); }
  .sheet-foot .grow { flex: 1; }
  .stepper { display: flex; align-items: center; gap: 6px; margin-left: auto; margin-right: 10px; }
  .stepper button { display: inline-flex; align-items: center; gap: 6px; border: none; background: none; padding: 3px 6px; border-radius: 7px; font: inherit; font-size: 12.5px; color: var(--faint); cursor: pointer; }
  .stepper button:hover:not(:disabled) { color: var(--text); background: var(--su-hover); }
  .stepper button:disabled { cursor: default; opacity: .6; }
  .stepper button.on { color: var(--text); font-weight: 600; }
  .stepper .sn { width: 18px; height: 18px; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; font-size: 10.5px; font-weight: 700; background: var(--accent-soft); color: var(--dim); }
  .stepper button.on .sn { background: var(--accent); color: #fff; }
  .stepper button.done .sn { background: var(--ok); color: #fff; }
  .stepper .sl { width: 18px; height: 1px; background: var(--border2); }
  .chosen { display: flex; flex-direction: column; gap: 6px; padding: 10px 12px; border-radius: 10px; background: var(--rail); border: 1px solid var(--border); }
  .chosen .lbl { font-size: 12.5px; font-weight: 600; }
  .chips { display: flex; flex-wrap: wrap; gap: 5px; }
  .chips .pill { gap: 5px; }
  .ch-top { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .chosen .link-btn { font-size: 12px; }
  .ch-top .link-btn { flex: none; white-space: nowrap; }
  .ch-sub { font-size: 12px; color: var(--dim); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tin.plain { font-family: inherit; font-size: 13px; line-height: 1.6; }
  .tin.guide { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; line-height: 1.6; min-height: 200px; }
  .lbl-row { display: flex; align-items: baseline; gap: 10px; }
  .lbl-row label { font-size: 12px; font-weight: 550; }
  .lbl-row .grow { flex: 1; }
  .link-btn.sm { font-size: 12px; }
  .auto { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 6px; }
  @container page (max-width: 560px) { .auto { grid-template-columns: 1fr; } }
  .au { text-align: left; display: flex; flex-direction: column; gap: 4px; padding: 9px 11px; border: 1px solid var(--border2); border-radius: 9px; background: #fff; font: inherit; color: var(--text); cursor: pointer; transition: border-color .12s, background-color .12s; }
  .au:hover:not(.on) { background: var(--su-hover); }
  .au.on { border-color: var(--accent); background: #fcfbf8; box-shadow: inset 0 0 0 .5px var(--accent); }
  .au-top { display: flex; align-items: center; justify-content: space-between; }
  .au b { font-size: 13px; font-weight: 600; }
  .au small { font-size: 11.5px; color: var(--dim); line-height: 1.5; }
  .presets { display: grid; grid-template-columns: repeat(auto-fill, minmax(176px, 1fr)); gap: 6px; }
  .preset { text-align: left; display: flex; flex-direction: column; gap: 3px; padding: 8px 10px; border: 1px solid var(--border2); border-radius: 9px; background: #fff; font: inherit; color: var(--text); cursor: pointer; transition: border-color .12s, background-color .12s; }
  .preset:hover:not(.on) { background: var(--su-hover); }
  .preset.on { border-color: var(--accent); background: #fcfbf8; box-shadow: inset 0 0 0 .5px var(--accent); }
  .pr-top { display: flex; align-items: center; gap: 6px; }
  .pr-top b { font-size: 12.5px; font-weight: 600; flex: 1; }
  .pr-top :global(.ico) { color: var(--dim); }
  .preset small { font-size: 11px; color: var(--faint); }
  .rec { font-size: 10px; font-weight: 600; color: var(--ok); background: var(--su-ok-soft); border-radius: 999px; padding: 1px 6px; }
  .acts { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 6px; }
  .act { display: flex; align-items: flex-start; gap: 9px; padding: 8px 10px; border: 1px solid var(--border2); border-radius: 9px; cursor: pointer; transition: border-color .12s, background-color .12s; }
  .act:hover { background: var(--su-hover); }
  .act.on { border-color: var(--accent); background: #fcfbf8; }
  .act input { margin: 3px 0 0; accent-color: var(--accent); }
  .act span { display: flex; flex-direction: column; min-width: 0; }
  .act b { font-size: 12.5px; font-weight: 600; }
  .act small { font-size: 11px; color: var(--faint); line-height: 1.45; }
  .chosen + .sec { border-top: none; padding-top: 4px; }
  .lbl :global(img) { margin-right: 5px; }
  .codex-warn { margin-top: 8px; padding: 9px 11px; border-radius: 8px; background: var(--su-warn-soft); color: var(--su-warn-ink); font-size: 12px; line-height: 1.6; }
  .codex-warn b { display: inline-flex; align-items: center; gap: 5px; font-weight: 600; }
  .codex-warn p { margin: 4px 0 0; }
  .sub-opt { display: flex; align-items: center; gap: 8px; margin-top: 6px; font-size: 12px; color: var(--dim); }
</style>
