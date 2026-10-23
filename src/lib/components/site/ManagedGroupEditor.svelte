<script lang="ts">
  // 新建 / 编辑托管方案：选站点（一个或多个）+ 这组站点自己的规则。
  import './site.css';
  import Icon from './Icon.svelte';
  import SiteIcon from './SiteIcon.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import { launcher } from '$lib/state/launcher.svelte.js';
  import { sites } from '$lib/state/sites.svelte.js';
  import { managed, saveGroup, deleteGroup } from '$lib/state/managed.svelte.js';

  const g = $derived(managed.editor);
  const groups = $derived(managed.overview?.groups ?? []);
  const modelOpts = $derived(launcher.comboOpts.filter((x) => !x.value.startsWith('codex::')).map((x) => ({ value: x.value, label: x.label, sub: x.sub, icon: x.icon, disabled: x.disabled })));
  const primary = $derived(g ? `${g.brain}::${g.model}` : '');
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
  // 两步：先选站点，再看规则；新建时只有第二步才能「开始托管」，编辑时两步可以随便切
  let step = $state<1 | 2>(1);
  let lastFor = '';
  $effect(() => { const k = g ? g.id || 'new' : ''; if (k !== lastFor) { lastFor = k; step = 1; q = ''; } });
  const isNew = $derived(!g?.id);
  const siteName = (id: string) => sites.list.find((x) => x.id === id)?.name ?? id;
</script>

{#if g}
  <div class="su-mask" role="presentation" onclick={close}></div>
  <div class="su modal ge" role="dialog" aria-modal="true" aria-label={g.id ? '编辑托管方案' : '新建托管方案'}>
    <header class="sheet-head">
      <b><Icon name="sparkles" />{g.id ? '编辑托管方案' : '新建托管方案'}</b>
      <nav class="stepper" aria-label="步骤">
        <button class:on={step === 1} class:done={step === 2} onclick={() => (step = 1)}><span class="sn">{step === 2 ? '✓' : 1}</span>选择站点</button>
        <span class="sl"></span>
        <button class:on={step === 2} onclick={() => (step = 2)} disabled={isNew && !g.sites.length}><span class="sn">2</span>托管规则</button>
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

     {:else}
      <div class="chosen">
        <div class="ch-top"><span class="lbl">「{g.name || '未命名'}」托管 {g.sites.length} 个站点</span><button class="link-btn" onclick={() => (step = 1)}>修改</button></div>
        <div class="chips">{#each g.sites as id (id)}<span class="pill line"><SiteIcon {id} size={12} />{siteName(id)}</span>{/each}</div>
      </div>
      <div class="sec"><Icon name="shield" size={13} />发布规则</div>
      <div class="field"><span class="lbl">新页面（整篇新内容）</span>
        <div class="seg"><button class:on={g.new_content === 'auto'} onclick={() => g && (g.new_content = 'auto')}>自动发布</button><button class:on={g.new_content === 'review'} onclick={() => g && (g.new_content = 'review')}>运营人员点头后发布</button></div>
        <small>标题、描述、内链和已有页面的内容改动，检查通过就自动上线。依赖、配置、代码、推荐 / 邀请相关内容、超过 400 行的改动永远不自动上线。</small>
      </div>
      <div class="grid2">
        <div class="field"><label for="ge-daily">每个站点每天最多自动上线</label><input id="ge-daily" class="tin" type="number" min="0" max="50" bind:value={g.daily_changes} /></div>
        <div class="field"><label for="ge-tok">每个站点每周模型用量（token，0＝不限）</label><input id="ge-tok" class="tin" type="number" min="0" step="100000" bind:value={g.tokens_per_week} /></div>
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
        {#if !isNew}<button class="btn small ghost" onclick={saveGroup} disabled={managed.saving || !g.name.trim()}>保存</button>{/if}
        <button class="btn small primary" onclick={() => (step = 2)} disabled={!g.name.trim() || (isNew && !g.sites.length)}>下一步：托管规则<Icon name="arrow" size={12} /></button>
      {:else}
        <button class="btn small ghost" onclick={() => (step = 1)} disabled={managed.saving}>上一步</button>
        <button class="btn small primary" onclick={saveGroup} disabled={managed.saving || !g.name.trim()}>{#if managed.saving}<span class="spin"></span>保存中{:else}{g.id ? '保存' : `开始托管 ${g.sites.length} 个站点`}{/if}</button>
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
  .chosen + .sec { border-top: none; padding-top: 4px; }
</style>
