<script lang="ts">
  // 制作站点：两步。① 定位（是什么站、给谁看、想靠它做什么）② 技术栈、风格、首批页面数、存放位置。
  // 点「开始制作」后在新站点里开第一版对话，AI 照着需求从空仓库做出能预览的第一版。
  import './site.css';
  import Dropdown from '$lib/Dropdown.svelte';
  import ModelFx from '$lib/ModelFx.svelte';
  import SiteMark from '$lib/components/icons/SiteMark.svelte';
  import Icon from './Icon.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { launcher, pickCombo, persistLauncherEffort, setFast, STYLE_OPTS } from '$lib/state/launcher.svelte.js';
  import { brainUsable, brainSetupHint } from '$lib/state/models';
  import { siteCreate as f, closeSiteCreate, toBuildStep, createSite, pickParent, folderName, recommendedStack, SITE_KINDS, STACKS, LANGUAGES, GOALS, PAGE_COUNTS } from '$lib/state/siteCreate.svelte.js';

  const rec = $derived(recommendedStack(f.kind, f.about));
  const sep = $derived(f.parent.includes('\\') && !f.parent.includes('/') ? '\\' : '/');
  const target = $derived(f.parent ? `${f.parent.replace(/[\\/]+$/, '')}${sep}${folderName(f.domain, f.name)}` : '');
  function toggleGoal(g: string) { f.goals = f.goals.includes(g) ? f.goals.filter((x) => x !== g) : [...f.goals, g]; }
</script>

{#if f.open}
  <div class="su-mask" role="presentation" onclick={closeSiteCreate}></div>
  <div class="su modal sc" role="dialog" aria-modal="true" aria-label="制作站点">
    <header class="sheet-head">
      <b><SiteMark size={15} /> 制作站点</b>
      <ol class="stepper" aria-label="步骤">
        <li class:on={f.step === 'brief'} class:done={f.step === 'build'}><span class="n">{f.step === 'build' ? '✓' : 1}</span>定位</li>
        <li class="bar" aria-hidden="true"></li>
        <li class:on={f.step === 'build'}><span class="n">2</span>技术栈与风格</li>
      </ol>
      <button class="x" onclick={closeSiteCreate} disabled={f.busy}>×</button>
    </header>
    <div class="sheet-body">
      {#if f.step === 'brief'}
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
      {#if f.step === 'brief'}
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
  .sheet-foot { display: flex; align-items: center; gap: 8px; }
  @media (max-width: 520px) { .kinds { grid-template-columns: 1fr; } }
</style>
