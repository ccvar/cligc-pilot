<script lang="ts">
  // 新建 / 编辑托管方案：选站点（一个或多个）+ 这组站点自己的规则。
  import './site.css';
  import Icon from './Icon.svelte';
  import SiteIcon from './SiteIcon.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import { launcher } from '$lib/state/launcher.svelte.js';
  import { sites } from '$lib/state/sites.svelte.js';
  import { managed, saveGroup, deleteGroup, ACTIONS, GUIDELINE_TEMPLATE, GOAL_PRESETS, METRIC_LABEL, suggestPreset } from '$lib/state/managed.svelte.js';
  import { fleet } from '$lib/state/fleet.svelte.js';

  const g = $derived(managed.editor);
  const groups = $derived(managed.overview?.groups ?? []);
  const modelOpts = $derived(launcher.comboOpts.filter((x) => !x.value.startsWith('codex::')).map((x) => ({ value: x.value, label: x.label, sub: x.sub, icon: x.icon, disabled: x.disabled })));
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
  const list = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    const chosen = new Set(g?.sites ?? []);
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
  $effect(() => { const k = g ? g.id || 'new' : ''; if (k !== lastFor) { lastFor = k; step = 1; q = ''; } });
  const siteName = (id: string) => sites.list.find((x) => x.id === id)?.name ?? id;
</script>

{#if g}
  <div class="su-mask" role="presentation" onclick={close}></div>
  <div class="su modal ge" role="dialog" aria-modal="true" aria-label={g.id ? '编辑托管方案' : '新建托管方案'}>
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
        <span class="lbl">站点 <span class="faint cnt">已选 {g.sites.length} 个</span></span>
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
      <div class="field"><span class="lbl">目标预设 <span class="faint cnt">点选会填好目标、勾上推荐的动作、定好衡量标准，都可以再改；可以多选</span></span>
        <div class="presets">
          {#each GOAL_PRESETS as p (p.key)}
            <button class="preset" class:on={picked.includes(p.key)} onclick={() => togglePreset(p.key)} aria-pressed={picked.includes(p.key)}>
              <span class="pr-top"><Icon name={p.icon} size={13} /><b>{p.name}</b>{#if p.key === suggested && !picked.length}<span class="rec">推荐</span>{/if}{#if picked.includes(p.key)}<Icon name="check" size={12} />{/if}</span>
              <small>{p.fits} · 看{METRIC_LABEL[p.metric]}</small>
            </button>
          {/each}
        </div>
      </div>
      <div class="field"><label for="ge-goal">运营目标</label>
        <textarea id="ge-goal" class="tin plain" rows="2" bind:value={g.goal} maxlength="500" placeholder="例如：提升中文页的 Google 自然流量，重点是费率、邀请和下载相关的查询词"></textarea>
        <small>一两句话。每次制定计划、每个改动都会围绕它。</small>
      </div>
      <div class="field"><span class="lbl">衡量标准 <span class="faint cnt">效果复盘按它判断改动有没有用；点击跌 15% 以上始终算负面</span></span>
        <div class="seg metric">{#each Object.entries(METRIC_LABEL) as [k, l] (k)}<button class:on={g.metric === k} onclick={() => g && (g.metric = k)}>{l}</button>{/each}</div>
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
      <div class="field"><label for="ge-topics">选题清单 <span class="faint cnt">{topicCount ? `${topicCount} 个` : '可选'}</span></label>
        <textarea id="ge-topics" class="tin plain" rows="4" bind:value={g.topics} placeholder={'一行一个关键词或主题，例如：\ngate kyc 认证教程\ngate 手续费对比'}></textarea>
        <small>写新页面时优先从这里选（已有页面覆盖的会跳过）。还没有数据的新站主要靠它。</small>
      </div>
      <div class="field">
        <div class="lbl-row"><label for="ge-guide">内容准则</label><span class="grow"></span>
          {#if g.guidelines !== GUIDELINE_TEMPLATE}<button class="link-btn sm" onclick={() => g && (g.guidelines = GUIDELINE_TEMPLATE)}>恢复默认模板</button>{/if}
          {#if g.guidelines.trim()}<button class="link-btn sm" onclick={() => g && (g.guidelines = '')}>清空</button>{/if}
        </div>
        <textarea id="ge-guide" class="tin guide" rows="12" bind:value={g.guidelines} placeholder="写给 AI 的写作规则：读者、语气、结构、长度、事实要求、禁区……留空就不额外约束。"></textarea>
        <small>原样交给 AI，写新页面和修改页面都必须遵守。默认模板只是起点，按你的站点改。</small>
      </div>
     {:else}
      <div class="chosen">
        <div class="ch-top"><span class="lbl">「{g.name || '未命名'}」托管 {g.sites.length} 个站点</span><button class="link-btn" onclick={() => (step = 1)}>修改</button></div>
        <div class="chips">{#each g.sites as id (id)}<span class="pill line"><SiteIcon {id} size={12} />{siteName(id)}</span>{/each}</div>
        <div class="ch-top"><span class="ch-sub">{g.goal.trim() || '没有写运营目标'} · 看{METRIC_LABEL[g.metric] ?? '点击'} · 允许 {g.actions.length} 种动作{topicCount ? ` · ${topicCount} 个选题` : ''}{g.guidelines.trim() ? ' · 有内容准则' : ''}</span><button class="link-btn" onclick={() => (step = 2)}>修改</button></div>
      </div>
      <div class="sec"><Icon name="shield" size={13} />发布规则</div>
      <div class="field"><span class="lbl">新页面（整篇新内容）</span>
        <div class="seg"><button class:on={g.new_content === 'auto'} onclick={() => g && (g.new_content = 'auto')}>自动发布</button><button class:on={g.new_content === 'review'} onclick={() => g && (g.new_content = 'review')}>人审人发</button></div>
        <small>{g.new_content === 'auto' ? 'AI 写好的新页面检查通过就直接上线。' : 'AI 写好的新页面放进「需要你」，你看过点上线才发布。'}标题、描述、内链和已有页面的改动，检查通过就自动上线；依赖、配置、代码、推荐 / 邀请相关内容、超过 400 行的改动永远不自动上线。</small>
      </div>
      <div class="grid2">
        <div class="field"><label for="ge-daily">每站每天自动上线上限（条）</label><input id="ge-daily" class="tin" type="number" min="0" max="50" bind:value={g.daily_changes} />
          <small>超出的改动转进「需要你」等你决定；0＝全部人审人发。</small></div>
        <div class="field"><label for="ge-tok">每站每周模型用量上限（token）</label><input id="ge-tok" class="tin" type="number" min="0" step="100000" bind:value={g.tokens_per_week} />
          <small>用完就暂停这个站点到下周并通知你。0＝不限。</small></div>
      </div>

      <div class="sec"><Icon name="cpu" size={13} />模型</div>
      <div class="grid2">
        <div class="field"><span class="lbl">主模型</span><Dropdown bind:value={() => primary, (v) => pick(v, 'primary')} options={modelOpts} /></div>
        <div class="field"><span class="lbl">备用模型</span><Dropdown bind:value={() => fallback, (v) => pick(v, 'fallback')} options={[{ value: '', label: '不用备用模型' }, ...modelOpts]} /></div>
      </div>
      <small class="faint">主模型这一轮失败（额度用完、限流、不可用）时自动换备用模型重跑一次。Codex 不能用于托管。</small>
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
        <button class="btn small primary" onclick={saveGroup} disabled={managed.saving || !g.name.trim() || !g.actions.length}>{#if managed.saving}<span class="spin"></span>保存中{:else}{g.id ? '保存' : `开始托管 ${g.sites.length} 个站点`}{/if}</button>
      {/if}
    </footer>
  </div>
{/if}

<style>
  .ge { width: min(640px, calc(100vw - 32px)); }
  .cnt { font-weight: 400; font-size: 11.5px; margin-left: 4px; }
  .picker { border: 1px solid var(--border2); border-radius: 10px; overflow: hidden; }
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
  .presets { display: grid; grid-template-columns: repeat(auto-fill, minmax(176px, 1fr)); gap: 6px; }
  .preset { text-align: left; display: flex; flex-direction: column; gap: 3px; padding: 8px 10px; border: 1px solid var(--border2); border-radius: 9px; background: #fff; font: inherit; color: var(--text); cursor: pointer; transition: border-color .12s, background-color .12s; }
  .preset:hover:not(.on) { background: var(--su-hover); }
  .preset.on { border-color: var(--accent); background: #fcfbf8; box-shadow: inset 0 0 0 .5px var(--accent); }
  .pr-top { display: flex; align-items: center; gap: 6px; }
  .pr-top b { font-size: 12.5px; font-weight: 600; flex: 1; }
  .pr-top :global(.ico) { color: var(--dim); }
  .preset small { font-size: 11px; color: var(--faint); }
  .rec { font-size: 10px; font-weight: 600; color: var(--ok); background: var(--su-ok-soft); border-radius: 999px; padding: 1px 6px; }
  .seg.metric { flex-wrap: wrap; }
  .acts { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 6px; }
  .act { display: flex; align-items: flex-start; gap: 9px; padding: 8px 10px; border: 1px solid var(--border2); border-radius: 9px; cursor: pointer; transition: border-color .12s, background-color .12s; }
  .act:hover { background: var(--su-hover); }
  .act.on { border-color: var(--accent); background: #fcfbf8; }
  .act input { margin: 3px 0 0; accent-color: var(--accent); }
  .act span { display: flex; flex-direction: column; min-width: 0; }
  .act b { font-size: 12.5px; font-weight: 600; }
  .act small { font-size: 11px; color: var(--faint); line-height: 1.45; }
  .chosen + .sec { border-top: none; padding-top: 4px; }
</style>
