<script lang="ts">
  import { escClose } from '$lib/escClose';
  // 制作站点，两种填法：
  // 快速制作（默认）：一句话说清是个什么站，名称、域名可空，其余交给 AI 判断，点开始就在新站点的对话里开工；
  // 详细设置：两步。① 定位（是什么站、给谁看、想靠它做什么）② 技术栈、风格、首批页面数、存放位置。
  // 两种都在新站点里开第一版对话，内容标准和视觉标准都内置在第一条消息里（site_make::first_message）。
  import './site.css';
  import Dropdown from '$lib/Dropdown.svelte';
  import ModelFx from '$lib/ModelFx.svelte';
  import SiteMark from '$lib/components/icons/SiteMark.svelte';
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { app } from '$lib/state/app.svelte.js';
  import { launcher, pickCombo, persistLauncherEffort, setFast, STYLE_OPTS } from '$lib/state/launcher.svelte.js';
  import { brainUsable, brainSetupHint } from '$lib/state/models';
  import { siteCreate as f, closeSiteCreate, setCreateMode, toBuildStep, createSite, pickParent, folderName, recommendedStack, SITE_KINDS, STACKS, LANGUAGES, GOALS, PAGE_COUNTS } from '$lib/state/siteCreate.svelte.js';

  const rec = $derived(recommendedStack(f.kind, f.about));
  const sep = $derived(f.parent.includes('\\') && !f.parent.includes('/') ? '\\' : '/');
  const target = $derived(f.parent ? `${f.parent.replace(/[\\/]+$/, '')}${sep}${folderName(f.domain, f.name)}` : '');
  const quick = $derived(f.mode === 'quick');
  const quickStack = $derived(recommendedStack('', f.about));
  const quickTarget = $derived(f.parent ? `${f.parent.replace(/[\\/]+$/, '')}${sep}${folderName(f.domain, f.name)}` : '');
  const canQuick = $derived(!!f.about.trim() && !!f.parent && !f.busy);
  // 内容 / 视觉：内置标准不改，可以另加这个站点的补充要求；展开时顺便能看内置标准全文
  const STD = {
    content: { label: '内容', sum: '按读者真实的搜索问法写，信息讲透不注水，不编造数据，配真实截图', ph: '比如：每篇文末列参考资料；评测写清测试条件和日期；不写医疗建议' },
    visual: { label: '视觉', sum: '按获奖网站的品质做原创设计，不套模板、不用米黄纸色底，各种屏幕宽度都适配', ph: '比如：主色用品牌绿 #1f8a4c；参考 linear.app 的克制感；要有深色模式' },
  } as const;
  type StdKey = keyof typeof STD;
  let editing = $state<StdKey | ''>('');
  // 组件一直挂着：每次重新打开都从收起的状态开始
  $effect(() => { if (!f.open) editing = ''; });
  let full = $state<{ content: string; visual: string } | null>(null);
  function extraOf(k: StdKey) { return k === 'content' ? f.contentExtra : f.visualExtra; }
  function setExtra(k: StdKey, v: string) { if (k === 'content') f.contentExtra = v; else f.visualExtra = v; }
  function editStd(k: StdKey) {
    editing = editing === k ? '' : k;
    if (!full) Promise.resolve().then(() => invoke<{ content: string; visual: string }>('site_standards')).then((r) => (full = r)).catch(() => {});
  }
  function quickKey(e: KeyboardEvent) { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && canQuick) { e.preventDefault(); void createSite(); } }
  function toggleGoal(g: string) { f.goals = f.goals.includes(g) ? f.goals.filter((x) => x !== g) : [...f.goals, g]; }
</script>

{#snippet stdRow(k: StdKey)}
  {@const extra = extraOf(k).trim()}
  <div class="std" class:open={editing === k}><dt>{STD[k].label}</dt><dd>
    <div class="std-line">
      <span class="std-sum"><em>内置标准</em>{STD[k].sum}</span>
      <button class="ic-btn ic-sm" class:on={editing === k} onclick={() => editStd(k)} disabled={f.busy} title={extra ? `改${STD[k].label}的补充要求` : `加${STD[k].label}的补充要求（内置标准照常带上）`} aria-label={`${STD[k].label}的补充要求`}><Icon name="pencil" size={13} /></button>
    </div>
    {#if editing === k}
      <!-- svelte-ignore a11y_autofocus -->
      <textarea class="tin" rows="3" value={extraOf(k)} oninput={(e) => setExtra(k, e.currentTarget.value)} placeholder={STD[k].ph} autofocus></textarea>
      <small class="std-note">补充要求接在内置标准后面一起交给 AI，冲突时以你写的为准。</small>
      <details class="std-full"><summary>看内置的{STD[k].label}标准全文</summary><pre>{full ? full[k] : '读取中…'}</pre></details>
    {:else if extra}
      <div class="std-extra" title={extra}><b>另加</b>{extra}</div>
    {/if}
  </dd></div>
{/snippet}

{#if f.open}
  <div class="su-mask" role="presentation" onclick={closeSiteCreate}></div>
  <div class="su modal sc" use:escClose={closeSiteCreate} role="dialog" aria-modal="true" aria-label="制作站点">
    <header class="sheet-head">
      <b><SiteMark size={15} /> 制作站点</b>
      <div class="seg" role="tablist" aria-label="填法">
        <button role="tab" aria-selected={quick} class:on={quick} onclick={() => setCreateMode('quick')} disabled={f.busy} title="一句话说清是个什么站，其余交给 AI">快速制作</button>
        <button role="tab" aria-selected={!quick} class:on={!quick} onclick={() => setCreateMode('full')} disabled={f.busy} title="自己定类型、读者、技术栈、风格和页数">详细设置</button>
      </div>
      {#if !quick}
        <ol class="stepper" aria-label="步骤">
          <li class:on={f.step === 'brief'} class:done={f.step === 'build'}><span class="n">{f.step === 'build' ? '✓' : 1}</span>定位</li>
          <li class="bar" aria-hidden="true"></li>
          <li class:on={f.step === 'build'}><span class="n">2</span>技术栈与风格</li>
        </ol>
      {/if}
      <button class="x" aria-label="关闭" data-tip="关闭" onclick={closeSiteCreate} disabled={f.busy}>×</button>
    </header>
    <div class="sheet-body">
      {#if quick}
        <p class="hint">说一句这是个什么站，剩下的交给 AI：站点类型、语言、风格由它判断，写在规划里，做完你都能看到、能改。</p>
        <div class="field"><label for="sc-q-about">这是个什么站</label>
          <!-- svelte-ignore a11y_autofocus -->
          <textarea id="sc-q-about" class="tin" rows="4" bind:value={f.about} onkeydown={quickKey} autofocus placeholder="写什么内容、给谁看、帮他们解决什么问题。例如：面向刚入门的咖啡爱好者，讲清楚手冲器具怎么选、豆子怎么挑、每种冲法的参数和常见问题。"></textarea>
        </div>
        <div class="grid2">
          <div class="field"><label for="sc-q-name">站点名称 <small class="opt">不填就让 AI 起</small></label><input id="sc-q-name" class="tin" bind:value={f.name} placeholder="如 手冲咖啡笔记" /></div>
          <div class="field"><label for="sc-q-domain">主域名 <small class="opt">可以先空着</small></label><input id="sc-q-domain" class="tin" bind:value={f.domain} placeholder="example.com" spellcheck="false" autocapitalize="off" /></div>
        </div>
        <dl class="plan">
          <div><dt>技术栈</dt><dd>{STACKS.find((x) => x.value === quickStack)?.label}<small>按描述推荐</small></dd></div>
          <div><dt>第一版</dt><dd>首页 + 8 个核心页面 + 关于、联系、隐私政策</dd></div>
          {@render stdRow('content')}
          {@render stdRow('visual')}
          <div><dt>存放位置</dt><dd class="loc"><span class="mono path" title={quickTarget}>{quickTarget || '还没选'}</span><button class="ic-btn ic-sm" onclick={pickParent} disabled={f.busy} title="换个存放位置" aria-label="换个存放位置"><Icon name="pencil" size={13} /></button></dd></div>
        </dl>
        <small class="more">想自己定类型、读者、技术栈和风格？切到<button class="link" onclick={() => setCreateMode('full')} disabled={f.busy}>详细设置</button></small>
        {#if !brainUsable(launcher.lBrain)}<p class="hint warn-text">{brainSetupHint(launcher.lBrain)}</p>{/if}
      {:else if f.step === 'brief'}
        <p class="hint">说清楚这是个什么站，AI 会据此规划栏目、写出第一批页面。这段定位也会存下来，以后托管运营挑选题时照着它来。</p>
        <div class="grid2">
          <div class="field"><label for="sc-name">站点名称</label><input id="sc-name" class="tin" bind:value={f.name} placeholder="如 手冲咖啡笔记" /></div>
          <div class="field"><label for="sc-domain">主域名 <small class="opt">可以先空着</small></label><input id="sc-domain" class="tin" bind:value={f.domain} placeholder="example.com" spellcheck="false" autocapitalize="off" /></div>
        </div>
        <div class="field"><label for="sc-about">这是个什么站</label>
          <textarea id="sc-about" class="tin" rows="3" bind:value={f.about} placeholder="一两句话：写什么内容、给谁看、帮他们解决什么问题。例如：面向刚入门的咖啡爱好者，讲清楚手冲器具怎么选、豆子怎么挑、每种冲法的参数和常见问题。"></textarea>
        </div>
        <div class="field"><span class="lbl">站点类型</span>
          <div class="kinds">
            {#each SITE_KINDS as k (k.value)}
              <button type="button" class="kind" class:ai={!k.value} class:on={f.kind === k.value} onclick={() => (f.kind = k.value)}>
                {#if !k.value}<Icon name="sparkles" size={14} />{/if}<b>{k.label}</b><small>{k.sub}</small>
              </button>
            {/each}
          </div>
        </div>
        <div class="grid2">
          <div class="field"><label for="sc-aud">目标读者 <small class="opt">选填</small></label><input id="sc-aud" class="tin" bind:value={f.audience} placeholder="如 刚入门的咖啡爱好者" /></div>
          <div class="field"><span class="lbl">页面语言</span><Dropdown bind:value={f.language} options={LANGUAGES} /></div>
        </div>
        <div class="field"><span class="lbl">想靠它做什么 <small class="opt">可多选</small></span>
          <div class="chips">{#each GOALS as g (g)}<button type="button" class="chip" class:on={f.goals.includes(g)} aria-pressed={f.goals.includes(g)} onclick={() => toggleGoal(g)}>{g}</button>{/each}</div>
        </div>
      {:else}
        <div class="field"><span class="lbl">技术栈</span>
          <div class="stacks" role="radiogroup">
            {#each STACKS as s (s.value)}
              <button type="button" class="stack" class:on={f.stack === s.value} role="radio" aria-checked={f.stack === s.value} onclick={() => { f.stack = s.value; f.stackTouched = true; }}>
                <span class="dot"></span>
                <span class="st-main"><b>{s.label}{#if s.value === rec}<em>推荐</em>{/if}</b><small>{s.sub}</small></span>
              </button>
            {/each}
          </div>
          <small>不管选哪个，都会生成一份站点约定（<code>.pilot/site.md</code>）：内容放哪、新增一页要改哪些地方。以后加内容、改页面都按它来。</small>
        </div>
        <div class="grid2">
          <div class="field"><span class="lbl">视觉风格</span><Dropdown bind:value={f.style} options={STYLE_OPTS} /></div>
          <div class="field"><span class="lbl">首批核心页面</span>
            <Dropdown value={String(f.pages)} onchange={(v: string) => (f.pages = Number(v))} options={PAGE_COUNTS.map((n) => ({ value: String(n), label: `${n} 页`, sub: '另加首页、关于、联系、隐私政策' }))} />
          </div>
        </div>
        <div class="field"><span class="lbl">内容与视觉 <small class="opt">选哪种风格都照常带上内置标准，可以另加要求</small></span>
          <dl class="plan">{@render stdRow('content')}{@render stdRow('visual')}</dl>
        </div>
        <div class="field"><span class="lbl">存放位置</span>
          <div class="row">
            <button class="btn small" onclick={pickParent} disabled={f.busy}>选择文件夹…</button>
            <span class="mono path" title={target}>{target || '还没选'}</span>
          </div>
          <small>会在这里建一个站点文件夹并 git init；代码只在你本机，Pilot 不会替你推送。</small>
        </div>
        {#if !brainUsable(launcher.lBrain)}<p class="hint warn-text">{brainSetupHint(launcher.lBrain)}</p>{/if}
      {/if}
      {#if f.err}<div class="err-note">{f.err}</div>{/if}
    </div>
    <footer class="sheet-foot">
      {#if quick}
        <span class="grow"></span>
        <ModelFx options={launcher.comboOpts} value={`${launcher.lBrain}::${launcher.lModel}`} effort={launcher.lEffort} fast={launcher.lFast} fastOk={app.brains?.claude_fast_ok ?? false} onpick={pickCombo} oneffort={persistLauncherEffort} onfast={setFast} />
        <button class="btn small primary" onclick={createSite} disabled={!canQuick} title="⌘ Enter">{f.busy ? '正在建站点…' : '开始制作'}</button>
      {:else if f.step === 'brief'}
        <button class="btn small primary" onclick={toBuildStep} disabled={!(f.name.trim() || f.domain.trim()) || !f.about.trim()}>下一步</button>
      {:else}
        <button class="btn small ghost" onclick={() => (f.step = 'brief')} disabled={f.busy}>上一步</button>
        <span class="grow"></span>
        <ModelFx options={launcher.comboOpts} value={`${launcher.lBrain}::${launcher.lModel}`} effort={launcher.lEffort} fast={launcher.lFast} fastOk={app.brains?.claude_fast_ok ?? false} onpick={pickCombo} oneffort={persistLauncherEffort} onfast={setFast} />
        <button class="btn small primary" onclick={createSite} disabled={f.busy || !f.stack || !f.parent}>{f.busy ? '正在建站点…' : '开始制作'}</button>
      {/if}
    </footer>
  </div>
{/if}

<style>
  .sc { width: min(640px, calc(100vw - 32px)); }
  /* 步骤条紧跟标题，× 留在最右 */
  .sc .sheet-head { justify-content: flex-start; gap: 16px; }
  .sc .sheet-head .x { margin-left: auto; }
  .stepper { list-style: none; margin: 0; padding: 0; display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--faint); }
  .stepper li { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; }
  .stepper li.on { color: var(--text); font-weight: 600; }
  .stepper li.done { color: var(--dim); }
  .stepper .n { width: 18px; height: 18px; border-radius: 50%; border: 1px solid var(--border2); box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; font-size: 10.5px; font-weight: 600; }
  .stepper li.on .n { background: var(--text); border-color: var(--text); color: #fff; }
  .stepper li.done .n { background: var(--su-ok-soft, #e6f4ea); border-color: transparent; color: var(--ok); }
  .stepper .bar { width: 22px; height: 1px; background: var(--border2); }
  .opt { font-weight: 400; color: var(--faint); font-size: 11px; margin-left: 4px; }
  .kinds { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .kind { display: flex; flex-direction: column; gap: 2px; align-items: flex-start; text-align: left; padding: 9px 11px; border: 1px solid var(--border); border-radius: 10px; background: none; font: inherit; color: var(--text); cursor: pointer; transition: border-color .12s, background-color .12s; }
  .kind:hover { border-color: var(--border2); }
  .kind.on { border-color: var(--text); background: #f7f6f2; }
  .kind b { font-size: 13px; font-weight: 600; }
  .kind small { font-size: 11.5px; color: var(--faint); line-height: 1.4; }
  /* 「让 AI 判断」占一整行，横排：图标 + 名称 + 说明 */
  .kind.ai { grid-column: 1 / -1; flex-direction: row; align-items: center; gap: 8px; padding: 8px 11px; }
  .kind.ai > :global(svg) { flex: none; color: var(--accent, #b4541a); }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .chip { height: 28px; padding: 0 12px; border: 1px solid var(--border); border-radius: 999px; background: none; font: inherit; font-size: 12.5px; color: var(--dim); cursor: pointer; transition: border-color .12s, color .12s, background-color .12s; }
  .chip:hover { color: var(--text); border-color: var(--border2); }
  .chip.on { color: var(--text); border-color: var(--text); background: #f7f6f2; }
  .stacks { display: flex; flex-direction: column; border: 1px solid var(--border); border-radius: 10px; overflow: hidden; margin-bottom: 4px; }
  .stack { display: flex; align-items: center; gap: 10px; padding: 8px 12px; border: none; border-top: 1px solid var(--border); background: none; font: inherit; color: var(--text); text-align: left; cursor: pointer; transition: background-color .12s; }
  .stack:first-child { border-top: none; }
  .stack:hover { background: #faf9f6; }
  .stack.on { background: #f4f3ef; }
  .dot { flex: none; width: 14px; height: 14px; border-radius: 50%; border: 1.5px solid var(--border2); box-sizing: border-box; }
  .stack.on .dot { border: 4px solid var(--text); }
  .st-main { display: flex; flex-direction: column; min-width: 0; }
  .st-main b { font-size: 13px; font-weight: 600; display: inline-flex; align-items: center; gap: 6px; }
  .st-main em { font-style: normal; font-size: 10.5px; font-weight: 500; padding: 1px 6px; border-radius: 999px; background: var(--accent-soft, #f3e8dd); color: var(--accent, #b4541a); }
  .st-main small { font-size: 11.5px; color: var(--faint); }
  .path { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: var(--dim); }
  .grow { flex: 1; }
  .seg { display: inline-flex; padding: 2px; border-radius: 8px; background: var(--su-soft, #f1f0ec); }
  .seg button { height: 24px; padding: 0 10px; border: none; border-radius: 6px; background: none; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; white-space: nowrap; }
  .seg button.on { background: #fff; color: var(--text); font-weight: 600; box-shadow: 0 1px 2px rgba(0, 0, 0, .08); }
  .plan { margin: 2px 0 0; border: 1px solid var(--border); border-radius: 10px; padding: 4px 12px; }
  .plan > div { display: flex; gap: 12px; padding: 7px 0; border-top: 1px solid var(--border); font-size: 12.5px; line-height: 1.5; }
  .plan > div:first-child { border-top: none; }
  .plan dt { flex: none; width: 56px; color: var(--faint); }
  .plan dd { margin: 0; min-width: 0; flex: 1; color: var(--text); }
  .plan dd small { margin-left: 6px; color: var(--faint); font-size: 11.5px; }
  .plan dd em { font-style: normal; font-size: 10.5px; font-weight: 500; padding: 1px 6px; margin-right: 6px; border-radius: 999px; background: var(--su-ok-soft, #e6f4ea); color: var(--ok); }
  .plan .loc { display: flex; align-items: center; gap: 6px; }
  .std-line { display: flex; align-items: flex-start; gap: 6px; }
  .std-sum { flex: 1; min-width: 0; }
  .std-line .ic-btn { margin-top: -3px; }
  .std-line .ic-btn.on { background: var(--su-hover); color: var(--text); }
  .std textarea { margin-top: 6px; width: 100%; box-sizing: border-box; font-size: 12.5px; }
  .std-note { display: block; margin-top: 4px; font-size: 11.5px; color: var(--faint); }
  .std-extra { margin-top: 4px; font-size: 12px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .std-extra b { font-weight: 500; color: var(--faint); margin-right: 6px; }
  .std-full { margin-top: 4px; font-size: 11.5px; color: var(--faint); }
  .std-full summary { cursor: pointer; width: fit-content; }
  .std-full summary:hover { color: var(--text); }
  .std-full pre { margin: 6px 0 0; max-height: 180px; overflow: auto; padding: 8px 10px; border-radius: 8px; background: var(--su-soft, #f6f6f4); white-space: pre-wrap; font: inherit; font-size: 11.5px; line-height: 1.6; color: var(--dim); }
  .more { display: block; margin-top: 8px; font-size: 11.5px; color: var(--faint); }
  .link { border: none; background: none; padding: 0 2px; font: inherit; color: var(--accent, #b4541a); cursor: pointer; }
  .link:hover { text-decoration: underline; }
  .sheet-foot { display: flex; align-items: center; gap: 8px; }
  @media (max-width: 520px) { .kinds { grid-template-columns: 1fr; } }
</style>
