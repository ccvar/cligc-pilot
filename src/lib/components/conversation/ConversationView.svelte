<script module lang="ts">
  import type { Message, Conversation, ToolCall } from '$lib/types';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { leaves } from '$lib/conversationLayout';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import SiteFav from '$lib/SiteFav.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import ModelFx from '$lib/ModelFx.svelte';
  import UsageRing from '$lib/UsageRing.svelte';
  import DistroMark from '$lib/components/icons/DistroMark.svelte';
  import WorkspaceMark from '$lib/components/icons/WorkspaceMark.svelte';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { splitAttachments, isImgPath, media, thumbKey, fmtClock, fmtRemain, splitGenImages, mdClick, mdRender, revealWorkdir, segs, compactToolRows, humanTool, toolSummary, interleave, mentionedImages, mdRenderCached, opsTask, splitPlanJson, convTitle, executionProgress, activityLabel, activityDetail, activityFailureReason, activityStatusLabel, elapsedLabel, hostOf, faviconGuess } from '$lib/state/messages.svelte.js';
  import { ui, startDrag } from '$lib/state/ui.svelte.js';
  import { brainTitle, brainLabel, modelIcon, permOptsFor, permTone, permTipFor, ctxLimitAdaptive, CODEX_GITHUB_NOTE } from '$lib/state/models';
  import { turns, disarmLimitAuto, armLimitAuto, retry, rebuildSession, send, stop } from '$lib/state/turns.svelte.js';
  import { permits, permitDesc, togglePermitCmd, respondPermit } from '$lib/state/permits.svelte.js';
  import { thread, onThreadScroll, persistThreadPerm, pickThreadCombo, persistThreadEffort, persistThreadFast } from '$lib/state/thread.svelte.js';
  import { conversationPresentation, panes, startConversationPointerDrag, endConversationDrag, closeSplit, splitConversation, paneScroll, panePrefill, uploadPaneFiles, restoreSplitQueue, setPaneText, paneInput, splitSubmit, splitSetting, splitModelOptions, splitStop } from '$lib/state/panes.svelte.js';
  import { legacyComposer, composer, onComposerDragOver, onComposerDragLeave, onComposerDrop, editQueued, clearQueued, removeAttachment, setLegacyDraft, onComposerPaste, onComposerKey, queueMessage } from '$lib/state/composer.svelte.js';
  import { startPreview, cf, fillDeploy } from '$lib/state/cloudflare.svelte.js';
  import { openSaveTmpl } from '$lib/state/library.svelte.js';
  import { importCfProject } from '$lib/state/launch.svelte.js';
  import { distroOf } from '$lib/state/connections.svelte.js';
  import { workspaceFolderName } from '$lib/state/launcher.svelte.js';
  import WhyChips from '$lib/components/site/WhyChips.svelte';

  export { splitContent, convPane };
  // 很长的用户消息（比如 Pilot 发给 AI 的整段指令）默认只露前几行，像 Claude 客户端那样
  let openUser = $state<Set<string>>(new Set());
  function toggleUser(k: string) { const n = new Set(openUser); if (n.has(k)) n.delete(k); else n.add(k); openUser = n; }
</script>

{@render conversationHeader(app.activeConv)}
{@render convPane(app.activeConv)}

<!-- 消息气泡片段 -->
{#snippet bubble(m: Message, isLast: boolean, context:Conversation|null=app.activeConv)}
  {@const bubbleConv=context}
  {@const bubbleId=context?.id??''}
  {@const bubbleBusy=!!app.running[bubbleId]}
  {#if m.role === 'user' && context?.origin === 'ops' && opsTask(m.text)}
    {@const task = opsTask(m.text)!}
    <div class="msg user ops-task"><div class="ops-card">
      <div class="ot-head">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z"/></svg>
        <b>托管运营 · {task.kind}</b>
        {#if task.site}<span class="ot-chip">{task.site}</span>{/if}
        {#if task.stage}<span class="ot-chip">{task.stage}</span>{/if}
      </div>
      {#if task.title}<div class="ot-title">{task.title}</div>{/if}
      <div class="ot-sub">Pilot 交给 AI 的任务：读站点和数据{task.kind.includes('计划') ? '、制定本周计划，这一轮不改文件' : '、在独立分支里改好并提交，由 Pilot 检查后决定上线'}。</div>
      <details class="ot-full"><summary>查看完整指令</summary><div class="ot-text">{m.text}</div></details>
    </div></div>
  {:else if m.role === 'user'}
    {@const ua = splitAttachments(m.text)}
    <div class="msg user"><div class="ubody">
      {#if ua.body.trim()}
        {@const ukey = `${bubbleId}:${m.ts}:${ua.body.length}`}
        {@const long = ua.body.length > 400 || ua.body.split('\n').length > 8}
        <span class="ub-text" class:clamp={long && !openUser.has(ukey)}>{@render richText(ua.body)}</span>
        {#if long}<button class="ub-more" onclick={() => toggleUser(ukey)}>{openUser.has(ukey) ? '收起' : `展开全部（${ua.body.length} 字）`}</button>{/if}
      {/if}
      {#if ua.atts.length}
        <div class="ub-atts" class:only={!ua.body.trim()}>
          {#each ua.atts as p (p)}
            {#if isImgPath(p) && media.thumbs[thumbKey(p,context)] !== ''}
              {#if media.thumbs[thumbKey(p,context)]}
                <button class="ub-att-img" data-tip={p.split('/').pop()} onclick={() => (ui.lightbox = media.thumbs[thumbKey(p,context)])}><img src={media.thumbs[thumbKey(p,context)]} alt={p.split('/').pop()} onerror={() => (media.thumbs = { ...media.thumbs, [thumbKey(p,context)]: '' })} /></button>
              {:else}
                <!-- 数据未到：同尺寸占位，图片就位零布局跳动 -->
                <span class="ub-att-img ph" data-tip={p.split('/').pop()}></span>
              {/if}
            {:else}
              <span class="ub-att">
                <span class="ub-att-ic">
                  {#if isImgPath(p)}<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="2.2" y="2.8" width="11.6" height="10.4" rx="1.6" stroke="currentColor" stroke-width="1.2" /><circle cx="5.7" cy="6.2" r="1.1" fill="currentColor" /><path d="M3 12.4l3.3-3.1 2.1 1.9 2.4-2.6 2.2 2.3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
                  {:else}<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M9 1.8H4.5A1.3 1.3 0 0 0 3.2 3.1v9.8a1.3 1.3 0 0 0 1.3 1.3h7a1.3 1.3 0 0 0 1.3-1.3V5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9 1.8v3.7h3.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg>{/if}
                </span>
                <span class="ub-att-n" title={p}>{p.split('/').pop()}</span>
              </span>
            {/if}
          {/each}
        </div>
      {/if}
    </div></div>
  {:else}
    <div class="msg assistant">
      <div class="body">
        {#if m.tools.length && m.error}{@render cmds(m.tools)}{/if}
        {#if m.error && m.limit_reset != null}
          <div class="limit-card">
            <div class="limit-head">⏳ {brainTitle(bubbleConv?.brain)} 额度已用完</div>
            {#if (m.limit_reset ?? 0) > 0}
              <div class="limit-sub">预计 {fmtClock(m.limit_reset ?? 0)} 恢复{#if (m.limit_reset ?? 0) * 1000 > turns.nowTick} · 还有 {fmtRemain((m.limit_reset ?? 0) * 1000 - turns.nowTick)}{/if}</div>
            {:else}
              <div class="limit-sub">订阅套餐的时间窗限额已触顶，稍后会自动恢复；也可以稍等片刻手动重试。</div>
            {/if}
            {#if isLast && !bubbleBusy}
              <div class="limit-actions">
                {#if turns.limitAuto[bubbleId]}
                  <button class="retry-btn is-armed" onclick={() => disarmLimitAuto(bubbleId)}>✓ 已排队，到点自动续跑 · 点击取消</button>
                {:else if (m.limit_reset ?? 0) * 1000 > turns.nowTick}
                  <button class="retry-btn" onclick={() => armLimitAuto(bubbleId, m.limit_reset ?? 0)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="5.6" stroke="currentColor" stroke-width="1.4" /><path d="M8 5.2V8l2 1.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg>到点自动续跑</button>
                {/if}
                {#if bubbleConv?.session_ref}<button class="retry-btn" onclick={() => retry(bubbleId, true)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M13 8a5 5 0 1 1-1.5-3.6M13 2v3h-3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>立即重试</button>{/if}
              </div>
              <div class="limit-hint">也可以在右下角把模型切到另一家继续。</div>
            {/if}
          </div>
        {:else if m.error}
          <div class="text is-err">{@render richText(m.text)}{#if isLast && !bubbleBusy}{#if bubbleConv?.session_ref && !turns.retryExhausted[bubbleId]}<button class="retry-btn" onclick={() => retry(bubbleId, true)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M13 8a5 5 0 1 1-1.5-3.6M13 2v3h-3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>重试</button>{/if}<button class="retry-btn" title="换一个全新会话原地续跑（自动带上历史摘要）——用于会话状态损坏、重试无效时" onclick={() => rebuildSession(bubbleId)}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M2.6 8a5.4 5.4 0 0 1 9.3-3.7M13.4 8a5.4 5.4 0 0 1-9.3 3.7" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><path d="M11.6 1.6v2.9h2.9M4.4 14.4v-2.9H1.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>重建继续</button>{/if}</div>
        {:else}
          {@const ga = splitGenImages(m.text)}
          {@const pj = context?.origin === 'ops' ? splitPlanJson(ga.body) : { body: ga.body, plan: null }}
          <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
          {#each interleave(pj.body, m.tools) as part, pi (pi)}
            {#if part.kind === 'steps'}{@render steps(part.tools)}
            {:else}<div class="text md" onclick={(e)=>mdClick(e,context)} onauxclick={(e)=>mdClick(e,context)}>{@html mdRenderCached(part.text)}</div>{/if}
          {/each}
          {#if pj.plan}{@render planCard(pj.plan)}{/if}
          {@const shown = mentionedImages(pj.body).filter((p) => !ga.atts.includes(p) && media.thumbs[thumbKey(p,context)])}
          {#if shown.length}
            <!-- 回复里提到的图片：读得到的才显示（像 Claude 客户端），点开放大 -->
            <div class="ub-atts gen-imgs ref-imgs">
              {#each shown as p (p)}
                <button class="ub-att-img ref" data-tip={p} onclick={() => (ui.lightbox = media.thumbs[thumbKey(p,context)])}><img src={media.thumbs[thumbKey(p,context)]} alt={p.split(/[\\/]/).pop()} /><span class="ref-n">{p.split(/[\\/]/).pop()}</span></button>
              {/each}
            </div>
          {/if}
          {#if ga.atts.length}
            <!-- codex 生图产物：缩略图（点击放大）；读不到的退回文件名卡（点击访达定位） -->
            <div class="ub-atts gen-imgs">
              {#each ga.atts as p (p)}
                {#if isImgPath(p) && media.thumbs[thumbKey(p,context)] !== ''}
                  {#if media.thumbs[thumbKey(p,context)]}
                    <button class="ub-att-img" data-tip={p.split(/[\\/]/).pop()} onclick={() => (ui.lightbox = media.thumbs[thumbKey(p,context)])}><img src={media.thumbs[thumbKey(p,context)]} alt={p.split(/[\\/]/).pop()} onerror={() => (media.thumbs = { ...media.thumbs, [thumbKey(p,context)]: '' })} /></button>
                  {:else}
                    <span class="ub-att-img ph" data-tip={p.split(/[\\/]/).pop()}></span>
                  {/if}
                {:else}
                  <button class="ub-att as-btn" title={p} onclick={() => void revealWorkdir(p,context)}>
                    <span class="ub-att-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="2.2" y="2.8" width="11.6" height="10.4" rx="1.6" stroke="currentColor" stroke-width="1.2" /><circle cx="5.7" cy="6.2" r="1.1" fill="currentColor" /><path d="M3 12.4l3.3-3.1 2.1 1.9 2.4-2.6 2.2 2.3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg></span>
                    <span class="ub-att-n">{p.split(/[\\/]/).pop()}</span>
                  </button>
                {/if}
              {/each}
            </div>
          {/if}
        {/if}
      </div>
    </div>
  {/if}
{/snippet}

{#snippet richText(text: string)}{#each text.split('\n') as line, i (i)}{#if i > 0}<br />{/if}{#each segs(line) as s, si (si)}{#if s.link}<button class="inlink" onclick={() => openUrl(s.t)}>{s.t}</button>{:else}{s.t}{/if}{/each}{/each}{/snippet}

{#snippet toolChip(t: ToolCall & { count?: number })}
  {@const h = humanTool(t)}
  <div class="tool"><span class="tcode">{h.verb}</span><span class="tdetail" title={h.full}>{h.target}</span>{#if (t.count ?? 0) > 1}<span class="tool-count">×{t.count}</span>{/if}</div>
{/snippet}

{#snippet planCard(plan: { notes: string; positioning: string; tasks: { title: string; page?: string; change: string; why?: string; source?: string; query?: string; expect?: { note?: string } }[] })}
  <div class="su plan-card">
    <div class="pc-head"><b>本周计划 · {plan.tasks.length} 项</b><span>已放进这个站点的「待处理」</span></div>
    {#if plan.notes}<div class="pc-notes"><b>给你的说明</b><div>{plan.notes}</div></div>{/if}
    <ol class="pc-tasks">
      {#each plan.tasks as t, i (i)}
        <li>
          <span class="pc-no">{i + 1}</span>
          <div class="pc-body">
            <b>{t.title}</b>
            <div class="pc-why"><WhyChips change={t.change} source={t.source} target={t.query} /></div>
            {#if t.why}<p>{t.why}</p>{/if}
            {#if t.expect?.note}<p class="pc-expect">预期：{t.expect.note}</p>{/if}
            {#if t.page}<small>{t.page}</small>{/if}
          </div>
        </li>
      {/each}
    </ol>
    {#if plan.positioning}<details class="pc-pos"><summary>AI 归纳的站点定位</summary><p>{plan.positioning}</p></details>{/if}
  </div>
{/snippet}

<!-- 一步一行（像 Claude 客户端）：动作 + 对象，点开看完整参数；连着 4 步以上收成一行摘要。
     running：这一组最后一步还在跑，显示转圈和用时。 -->
{#snippet steps(tools: ToolCall[], running: { ms: number } | null = null)}
  {#if tools.length >= 4}
    <details class="steps-sum">
      <summary><span class="st-t">{toolSummary(tools)}</span><svg class="st-chev" width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></summary>
      <div class="st-list">{#each compactToolRows(tools) as t, i (i)}{@render stepRow(t, null)}{/each}</div>
    </details>
    {#if running}{@render stepRow(tools[tools.length - 1], running)}{/if}
  {:else}
    {#each tools as t, i (i)}{@render stepRow(t, running && i === tools.length - 1 ? running : null)}{/each}
  {/if}
{/snippet}

{#snippet stepRow(t: ToolCall & { count?: number }, running: { ms: number } | null)}
  {@const h = humanTool(t)}
  <details class="step" class:running={!!running}>
    <summary>
      {#if running}<span class="st-dots" aria-hidden="true"><i></i><i></i><i></i><i></i></span>{/if}
      <span class="st-t"><b>{h.verb}</b> {h.target}</span>
      {#if (t.count ?? 0) > 1}<span class="st-n">×{t.count}</span>{/if}
      {#if running}<span class="st-ms">{elapsedLabel(running.ms)}</span>{/if}
      <svg class="st-chev" width="10" height="10" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </summary>
    {#if h.full}<pre class="st-full">{h.full}</pre>{/if}
  </details>
{/snippet}

{#snippet cmds(tools: ToolCall[])}
  <details class="cmds">
    <summary>
      <svg class="cmd-ic" width="13" height="13" viewBox="0 0 16 16" fill="none">
        <rect x="1.5" y="2.5" width="13" height="11" rx="2" stroke="currentColor" stroke-width="1.3" />
        <path d="M4.5 6l2 2-2 2M8.5 10h3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <span>{toolSummary(tools)}</span>
      <svg class="cmd-chev" width="11" height="11" viewBox="0 0 12 12" fill="none"><path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </summary>
    <div class="tools">{#each compactToolRows(tools) as t, i (i)}{@render toolChip(t)}{/each}</div>
  </details>
{/snippet}

<!-- 单会话、多个窗格和远程工作台共用原始对话组件。
     运行状态按 context.id 查找；输入、滚动和操作由所属窗格绑定，不能读取其他窗格的焦点状态。 -->
{#snippet conversationActivity(context:Conversation)}
  {@const activeConv=context}
  {@const viewBusy=!!app.running[context.id]}
  {@const liveView=app.lives[context.id]}
  {@const activePermits=permits.pendingPermits.filter(p=>p.conv===context.id)}
      {#if viewBusy && liveView}
        {@const progress = executionProgress(liveView, thread.nowMs)}
        {@const parts = interleave(liveView.text, liveView.tools)}
        {@const lastSteps = parts.length > 0 && parts[parts.length - 1].kind === 'steps'}
        <div class="msg assistant">
          <div class="body">
            {#if liveView.compactedAt}
              <div class="context-compact-note">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M3 5.2h10M5 2.8l-2 2.4 2 2.4M13 10.8H3M11 8.4l2 2.4-2 2.4" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" /></svg>
                <span>Claude 已自动整理上下文</span>
                <small>同一会话继续，聊天记录保持不变</small>
              </div>
            {/if}
            {#each parts as part, pi (pi)}
              {#if part.kind === 'steps'}{@render steps(part.tools, pi === parts.length - 1 && progress.running ? { ms: thread.nowMs - progress.running.startedAt } : null)}
              {:else}<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="text md live-text" onclick={(e)=>mdClick(e,activeConv)} onauxclick={(e)=>mdClick(e,activeConv)}>{@html mdRenderCached(part.text)}</div>{/if}
            {/each}
            <!-- 正在跑的步骤不是工具（比如生图、思考）或还没出现在步骤里：单独一行 -->
            {#if progress.running && !lastSteps}
              <div class="step running solo"><span class="st-dots" aria-hidden="true"><i></i><i></i><i></i><i></i></span><span class="st-t">{activityLabel(progress.running)}</span><span class="st-ms">{elapsedLabel(thread.nowMs - progress.running.startedAt)}</span></div>
            {/if}
            {#if progress.failed && !progress.continuedAfterFailure}
              <div class="step failed solo"><span class="st-t">{progress.failedActivities.length} 步没成功：{activityFailureReason(progress.failedActivities[progress.failedActivities.length - 1])}</span></div>
            {/if}
            {#if progress.thinking}
              <div class="codex-thinking" role="status" aria-live="polite">
                <span>正在思考</span>
              </div>
            {/if}
            {#if liveView.error}<div class="err-note">{liveView.error}</div>
            {:else if !progress.thinking}
              <div class="working" aria-label="思考中">
                <svg class="wl" viewBox="0 0 64 64" width="17" height="17" aria-hidden="true">
                  <!-- Cligc Pilot 标志：两根斜着的圆头条，轮流画出来 -->
                  <defs><linearGradient id="wl-g" x1="0" y1="10" x2="0" y2="56" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#6fe39c" /><stop offset="1" stop-color="#17a058" /></linearGradient></defs>
                  <path class="wl-a" d="M38 14 19.5 44" fill="none" stroke="url(#wl-g)" stroke-width="12" stroke-linecap="round" pathLength="100" stroke-dasharray="100" />
                  <path class="wl-b" d="M44.5 33.5 33 52.5" fill="none" stroke="url(#wl-g)" stroke-width="12" stroke-linecap="round" pathLength="100" stroke-dasharray="100" />
                </svg>
                <span class="working-t">{elapsedLabel(thread.nowMs - liveView.startedAt)}</span>
              </div>
            {/if}
          </div>
        </div>
      {/if}
      {#each activePermits as p (p.id)}
        <div class="msg assistant">
          <div class="permit-card" class:danger={p.dangerous}>
            <div class="permit-head"><span class="permit-dot"></span>需要你确认这个操作{#if p.dangerous}<span class="permit-tag">危险 · 对线上生效</span>{/if}</div>
            <div class="permit-desc">{permitDesc(p)}</div>
            <div class="permit-meta">{p.tool === 'SSH' ? '远程命令' : p.tool}{#if p.tool === 'SSH' && p.arg} · {p.arg}{/if}{#if p.mode === 'ask'} · 询问档{/if}</div>
            {#if p.cmd}
              {#if p.tool === 'SSH'}
                <!-- 远程命令不给收起：它要在用户的真机上跑，不看着它就没法做决定。 -->
                <code class="permit-cmd">{p.cmd}</code>
              {:else if permits.permitCmdOpen.has(p.id)}
                <button class="permit-more" onclick={() => togglePermitCmd(p.id)}>收起命令 ▴</button>
                <code class="permit-cmd">{p.cmd}</code>
              {:else}
                <button class="permit-more" onclick={() => togglePermitCmd(p.id)}>查看具体命令 ▸</button>
              {/if}
            {/if}
            <div class="permit-act">
              <button class="btn sm" onclick={() => respondPermit(p.id, false)}>拒绝</button>
              <button class="btn sm primary" onclick={() => respondPermit(p.id, true)}>批准</button>
            </div>
          </div>
        </div>
      {/each}

{/snippet}

{#snippet conversationHeader(context:Conversation|null, paneId='')}
  {@const meta=conversationPresentation(context)}
  {@const activeConv=context}
  {@const activeSiteUrl=meta.url}
  {@const headerDomain=activeSiteUrl?hostOf(activeSiteUrl):''}
  {@const headerSiteIcon=headerDomain?faviconGuess(`https://${headerDomain}`):''}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <header class="thread-head conversation-thread-head" class:multi={leaves(panes.conversationLayout).length>1} onmousedown={(e)=>{if(!paneId)startDrag(e);}}>
        <div class="th-info" role="button" tabindex="0" aria-label={`拖动对话 ${context?.title} 以分屏`} onpointerdown={(e)=>{if(context)startConversationPointerDrag(e,context.id);}} onkeydown={(e)=>{if(e.key==='Escape')endConversationDrag();}}>
          <b class="conversation-head-title" data-tip={convTitle(activeConv?.title ?? '')}>
            <span class="conversation-head-model" data-tip={brainLabel(activeConv?.brain??'claude')}><BrainIcon brain={modelIcon(activeConv?.brain??'claude')} size={14} /></span>
            <span>{convTitle(activeConv?.title ?? '')}</span>
          </b>
        </div>
        {#if activeSiteUrl}
          <button class="th-open conversation-site-link" onclick={() => openUrl(activeSiteUrl)} title="打开 {activeSiteUrl}"><SiteFav src={headerSiteIcon} label={headerDomain} size={14} /><span>{headerDomain}</span><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M6 3.5h6.5V10M12.2 3.8 3.8 12.2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg></button>
        {:else if headerDomain}
          <span class="th-open conversation-site-link static" title={headerDomain}><SiteFav src={headerSiteIcon} label={headerDomain} size={14} /><span>{headerDomain}</span></span>
        {/if}
        {#if paneId && leaves(panes.conversationLayout).length>1}<button class="x pane-close" title="关闭窗格，不停止任务" aria-label="关闭窗格" onclick={()=>closeSplit(paneId)}>×</button>{/if}
      </header>

{/snippet}

{#snippet splitContent(id:string, paneId:string)}
  {@const c=splitConversation(id)}
  {#if c && panes.splitDrafts[id]}
    {#key c.id}
      {@render conversationHeader(c,paneId)}
      {@render convPane(c,paneId)}
    {/key}
  {/if}
{/snippet}

{#snippet convPane(context:Conversation|null, paneId='')}
  {@const meta=conversationPresentation(context)}
  {@const activeConv=context}
  {@const activeConvConn=meta.conn}
  {@const activeConvIsCf=meta.conn?.kind==='cloudflare'}
  {@const activeConvIsWorkspace=context?.task_type==='workspace'}
  {@const scoped=!!paneId && !!context}
  {@const id=context?.id??''}
  {@const activeConvId=id}
  {@const activeConvIsSsh=meta.conn?.kind==='ssh'}
  {@const viewBusy=!!app.running[id]}
  {@const paneState=scoped?panes.splitDrafts[id]:null}
  {@const draft=paneState?paneState.text:legacyComposer().text}
  {@const attachments=paneState?paneState.files:legacyComposer().files}
  {@const queued=scoped?(panes.splitQueued[id]?{convId:id,text:panes.splitQueued[id].text,atts:panes.splitQueued[id].files}:null):legacyComposer().queued}
  {@const cfReady=scoped?panes.paneCfReady[id]??false:legacyComposer().cfReady}
  {@const uploading=scoped?!!panes.paneUploading[id]:composer.attaching}
  {@const messages=context?.messages.filter(m=>!m.hidden)??[]}
  <div class="thread" use:paneScroll={scoped?id:''} onscroll={(e)=>{if(!scoped)onThreadScroll(e);}}>
    <div class="thread-inner" >
      {#each messages as m, i (i)}
        {@render bubble(m, i === messages.length - 1,context)}
      {/each}
      {#if activeConv}{@render conversationActivity(activeConv)}{/if}
    </div>
  </div>

  <div class="composer-wrap">
    {#if activeConvIsCf}
      <div class="cf-bar">
        <span class="tipwrap" data-tip={cfReady ? '在本机跑起来看效果（关预览窗即停）' : '先让 AI 建出页面再预览'}><button class="cb-prev" onclick={()=>scoped?panePrefill(id,()=>void startPreview()):void startPreview()} disabled={cf.previewBusy || !cfReady}><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M1.6 8s2.4-4.4 6.4-4.4S14.4 8 14.4 8s-2.4 4.4-6.4 4.4S1.6 8 1.6 8Z" stroke="currentColor" stroke-width="1.2" /><circle cx="8" cy="8" r="1.9" stroke="currentColor" stroke-width="1.2" /></svg>{cf.previewBusy ? '启动中…' : '预览'}</button></span>
        <span class="tipwrap" data-tip={cfReady ? '发布到 Cloudflare 上线' : '先让 AI 建出页面再部署'}><button class="cb-prev" onclick={()=>scoped?panePrefill(id,fillDeploy):fillDeploy()} disabled={viewBusy || !cfReady}><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M8 12.5V4M4.5 7 8 3.5 11.5 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>部署</button></span>
        <span class="tipwrap" data-tip={cfReady ? '存成模板，以后一键复用' : '先让 AI 建出页面再存'}><button class="cb-prev dim" onclick={()=>scoped?panePrefill(id,openSaveTmpl):openSaveTmpl()} disabled={!cfReady}><svg width="12" height="12" viewBox="0 0 16 16" fill="none"><rect x="2.6" y="2.6" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9.1" y="2.6" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="2.6" y="9.1" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9.1" y="9.1" width="4.3" height="4.3" rx="1" stroke="currentColor" stroke-width="1.3" /></svg>存模板</button></span>
        <span class="tipwrap" data-tip={cfReady ? '变成一个站点：接 GitHub、域名、GA4 / Search Console 数据，开始运营' : '先让 AI 建出页面'}><button class="cb-prev dim" onclick={()=>context && void importCfProject(context.conn_id, context.site_slug)} disabled={!cfReady || viewBusy}><svg width="12" height="12" viewBox="0 0 20 20" fill="none"><circle cx="10" cy="10" r="6.6" stroke="currentColor" stroke-width="1.5" /><path d="M3.6 10h12.8M10 3.4c1.9 1.8 2.9 4.1 2.9 6.6s-1 4.8-2.9 6.6M10 3.4C8.1 5.2 7.1 7.5 7.1 10s1 4.8 2.9 6.6" stroke="currentColor" stroke-width="1.3" /></svg>转为站点</button></span>
      </div>
    {/if}
    <div class="composer" class:fileover={!scoped&&composer.composerFileOver} role="group" aria-label="消息输入"
      ondragover={(e)=>{if(scoped){if(e.dataTransfer?.types.includes('Files'))e.preventDefault();}else onComposerDragOver(e);}}
      ondragleave={(e)=>{if(!scoped)onComposerDragLeave(e);}}
      ondrop={(e)=>{if(scoped&&e.dataTransfer?.files.length){e.preventDefault();e.stopPropagation();void uploadPaneFiles(id,e.dataTransfer.files);}else if(!scoped)onComposerDrop(e);}}>
      {#if scoped && panes.splitErrors[id]}<p class="pane-error" role="alert">{panes.splitErrors[id]}</p>{/if}
      {#if queued && queued.convId === activeConvId}
        <div class="queued-row">
          <span class="queued-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.3" /><path d="M8 4.7V8l2.2 1.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg></span>
          <span class="queued-t">等这轮结束后发送：{queued.text || '（仅附件）'}{#if queued.atts.length} · {queued.atts.length} 个文件{/if}</span>
          <button class="queued-btn" onclick={()=>scoped?restoreSplitQueue(id):editQueued()}>编辑</button>
          <button class="queued-x" aria-label="清除等待消息" onclick={()=>{if(scoped)delete panes.splitQueued[id];else clearQueued();}}>×</button>
        </div>
      {/if}
      {#if attachments.length}
        <div class="attach-row">
          {#each attachments as a, i (a.path)}
            <div class="attach-chip">
              {#if a.preview}<img class="attach-img" src={a.preview} alt="" />{:else}<span class="attach-ic"><svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M9 1.8H4.5A1.3 1.3 0 0 0 3.2 3.1v9.8a1.3 1.3 0 0 0 1.3 1.3h7a1.3 1.3 0 0 0 1.3-1.3V5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /><path d="M9 1.8v3.7h3.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg></span>{/if}
              <span class="attach-name" title={a.name}>{a.name}</span>
              <button class="attach-x" aria-label="移除附件" onclick={() => {if(paneState)paneState.files=paneState.files.filter((_,n)=>n!==i);else removeAttachment(i);}}>×</button>
            </div>
          {/each}
        </div>
      {/if}
      {#if uploading}<span class="attach-loading">上传中…</span>{/if}
      <textarea bind:value={()=>draft,(v)=>scoped?setPaneText(id,v):setLegacyDraft(v)} use:paneInput={scoped?id:''} aria-label={`给 ${context?.title||'当前对话'} 发消息`} rows="1" placeholder={viewBusy ? '输入下一条，回车排队，等这轮结束自动发送' : '继续说…（Enter 发送，Shift+Enter 换行，可粘贴/拖入文件）'}
        oncompositionstart={() => (composer.composing = true)} oncompositionend={() => (composer.composing = false)}
        onpaste={(e)=>onComposerPaste(e,scoped?id:'')}
        onkeydown={(e) => onComposerKey(e, scoped ? ()=>void splitSubmit(id) : viewBusy ? queueMessage : send)}></textarea>
      <div class="composer-bar">
        <div class="cb-left">
          {#if activeConvIsCf}
            <span class="cb-ro" title="项目已固定"><span class="cb-ro-t">{activeConv?.site_slug}</span></span>
          {:else if meta.conn?.kind==='github'}
            <span class="cb-ro" title="仓库已固定"><GhMark size={14} /><span class="cb-ro-t">{activeConv?.site_slug}</span></span>
          {:else if activeConvIsSsh}
            <!-- 远程对话：这里放远端系统的图标 + 机器地址。 -->
            <span class="cb-ro" title={activeConvConn?.ssh_os || '远端系统未知'}><DistroMark id={distroOf(activeConvConn)} size={15} /><span class="cb-ro-t">{activeConv?.site_name}</span></span>
          {:else}
            <span class="cb-ro" title={activeConv?.workspace_dir || '本会话未关联文件夹'}><WorkspaceMark size={15} /><span class="cb-ro-t">{activeConv?.workspace_dir ? workspaceFolderName(activeConv.workspace_dir) : '纯对话'}</span></span>
          {/if}
        </div>
        <div class="cb-right">
          <Dropdown compact value={context?.perm_mode||'full'} options={permOptsFor(activeConv?.brain ?? 'claude', activeConvIsSsh)} tone={permTone(context?.perm_mode||'full')} tip={permTipFor(activeConv?.brain ?? 'claude', context?.perm_mode||'full', activeConvIsSsh)} onchange={(v)=>scoped&&context?void splitSetting(context,'perm',v):persistThreadPerm(v)} />
          <!-- 模型下拉列全部厂商（图标区分）：同厂商下一轮生效；跨厂商确认后以历史摘要重建续跑 -->
          <ModelFx options={scoped&&context?splitModelOptions(context):thread.threadComboOpts} value={`${activeConv?.brain ?? 'claude'}::${context?.model??''}`} effort={context?.effort??''} fast={activeConv?.fast ?? false} fastOk={app.brains?.claude_fast_ok ?? false} lockModel={viewBusy} onpick={(v: string) => { if(scoped&&context)void splitSetting(context,'model',v);else void pickThreadCombo(v); }} oneffort={(v:string)=>scoped&&context?void splitSetting(context,'effort',v):persistThreadEffort(v)} onfast={(v: boolean) => { if(scoped&&context)void splitSetting(context,'fast',v);else void persistThreadFast(v); }} />
          <UsageRing ctx={activeConv?.ctx_tokens ?? 0} limit={ctxLimitAdaptive(activeConv?.brain ?? 'claude', activeConv?.model ?? '', activeConv?.ctx_tokens ?? 0)} total={activeConv?.total_tokens ?? 0} />
          {#if viewBusy}
            {#if draft.trim() || attachments.length}
              <button class="send queue" disabled={uploading} onclick={()=>scoped?void splitSubmit(id):queueMessage()} title="排队：等这轮结束后自动发送">↑</button>
            {/if}
            <button class="send stop" onclick={()=>scoped?void splitStop(id):stop()} title="停止">■</button>
          {:else}
            <button class="send" onclick={()=>scoped?void splitSubmit(id):send()} disabled={uploading || (!draft.trim() && !attachments.length)} title="发送（Enter）">↑</button>
          {/if}
        </div>
      </div>
    </div>
    {#if meta.conn?.kind==='github' && activeConv?.brain==='codex'}<p class="gh-codex-note">{CODEX_GITHUB_NOTE}</p>{/if}
  </div>
{/snippet}

<style>
  .gh-codex-note{margin:6px 4px 0;color:var(--warn);font-size:11.5px;line-height:1.5}
  .pane-close{position:static;flex:none;align-self:center}
  .thread-head.conversation-thread-head{align-items:center}
  .thread-head.multi{padding:7px 16px;min-height:38px;align-items:center}
  .thread-head.multi .th-info{display:block;min-width:0}
  .conversation-head-title{display:flex!important;align-items:center;gap:7px;width:100%;min-width:0}
  .conversation-head-title>span:last-child{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .conversation-head-model{display:inline-flex;align-items:center;flex:none;line-height:0}
  .conversation-head-model :global(.bi){display:block}
  .conversation-site-link{max-width:52%;min-width:0;flex:none}
  .conversation-site-link>span{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .conversation-site-link.static{cursor:default;color:var(--dim)}
  .thread-head .th-info[role="button"]{cursor:grab}
  .pane-error{color:var(--danger,#ad3228);font-size:13px;margin:8px 16px;overflow-wrap:anywhere}
  :global(.pane) .thread{min-height:0;overscroll-behavior:contain}
  :global(.canvas.multi .pane) .thread-inner{padding:18px 20px 8px}
  :global(.canvas.multi .pane) .composer-wrap{padding:8px 16px 14px;max-height:55%;overflow-y:auto}
  :global(.canvas.multi .pane) .composer-bar{flex-wrap:wrap}
  :global(.canvas.multi .pane) .cb-right{margin-left:auto;max-width:100%;flex-wrap:wrap}
  /* 线程头右上角：站点域名 + ↗，点开站点 */
  .th-open { flex: none; display: inline-flex; align-items: center; gap: 4px; background: none; border: none; padding: 4px 6px; border-radius: 7px; color: var(--accent); font: inherit; font-size: 12.5px; cursor: pointer; }
  .th-open:hover { background: #f1efe9; }
  .th-open:disabled { opacity: .5; cursor: wait; }
  .th-open svg { flex: none; }
  .tipwrap { display: inline-flex; }
  /* 表单控件统一尺度（桌面紧凑密度）：13px 字 + 5px 10px 内边距 + 8px 圆角，单行高约 30px。
     composer 聊天输入框不吃这套（下方 composer 专属规则钉回 14px 大字）。 */
  textarea { font-family: inherit; font-size: 13px; color: var(--text); background: #fff; border: 1.5px solid var(--border2); border-radius: 8px; padding: 5px 10px; }
  textarea:focus { outline: none; border-color: #b7b2a6; box-shadow: none; }
  /* 输入框（仿 Claude Code：整块圆角边框，聚焦时高亮，发送按钮嵌在框内） */
  .composer-wrap .composer { position: relative; background: #fff; border: 1px solid var(--border2); border-radius: 12px; box-shadow: none; transition: border-color .12s, box-shadow .12s; }
  .composer-wrap .composer:focus-within { border-color: #b7b2a6; box-shadow: none; }
  .composer-wrap .composer.fileover { border-color: var(--accent); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 16%, transparent); }
  .composer-wrap .composer.fileover::after {
    content: '松开即可添加文件'; position: absolute; inset: 4px; z-index: 8; display: flex; align-items: center; justify-content: center;
    border: 1px dashed color-mix(in srgb, var(--accent) 58%, transparent); border-radius: 9px;
    background: color-mix(in srgb, #fff 92%, var(--accent)); color: var(--accent); font-size: 13px; font-weight: 600; pointer-events: none;
  }
  .composer-wrap textarea { width: 100%; resize: none; border: none; background: none; box-shadow: none; padding: 14px 52px 14px 17px; font-size: 14px; line-height: 1.6; max-height: 200px; display: block; }
  .composer-wrap textarea:focus { outline: none; box-shadow: none; border: none; }
  .composer .send { position: absolute; right: 9px; bottom: 9px; width: 32px; height: 32px; border-radius: 50%; border: none; background: var(--accent); color: #fff; font-size: 16px; cursor: pointer; display: flex; align-items: center; justify-content: center; transition: background .12s, transform .08s; }
  .composer .send:hover { background: var(--accent-h); }
  .composer .send:active { transform: scale(.92); }
  .composer .send:disabled { background: #dcdad2; cursor: default; transform: none; }
  .composer .send.stop { background: var(--text); }
  /* composer：竖排 textarea + 底栏（启动页站点在左；会话页只读厂商在左，模型/发送在右）。 */
  .composer-wrap .composer { display: flex; flex-direction: column; }
  .composer-wrap textarea { padding: 12px 16px 6px 16px; }
  .composer-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 8px 8px; }
  .cb-left { display: flex; align-items: center; gap: 1px; flex: 1; min-width: 0; }
  .cb-right { display: flex; align-items: center; gap: 3px; flex: none; }
  .composer-bar .send { position: static; width: 30px; height: 30px; margin-left: 3px; font-size: 15px; }
  /* 会话页只读徽标（站点/厂商，不可改）。 */
  .cb-ro { display: inline-flex; align-items: center; gap: 6px; padding: 4px 8px; color: var(--dim); font-size: 13px; opacity: .9; min-width: 0; }
  .cb-ro-t { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* ---- 线程 ---- */
  /* ★ 标题要和左上角那排工具图标在同一条水平线上。
     `.win-tools` 是 fixed / top:0 / height:30px + align-items:center → **图标中心恒在 15px**
     （和 .thread-head.slim 那条注释是同一个基准）。标题行高 20.3 → 上内边距取 5px，
     标题中心落在 15.15，对上。原来 13px 时中心在 23.1 —— **每个页面都低 8px**。
     ★ 上边距被「对齐那条 fixed 图标带」锁死＝5px，所以**下边距只能跟着收成 5px**，
     不然上下就不对称（原来 5/13，下面明显空一截）。头因此比以前矮 8px，这是这条对齐的代价。
     ★ align-items 必须 flex-start，不能用 center：th-info 比右侧按钮（28px）矮时
     （例如只有标题、没有副标题的头），center 会让 th-info 跟着按钮居中，标题又掉到 26.8。 */
  /* ★ 和内容区之间那条缝：原来是一条 1px 实线直接把两块切开，太硬。**两个状态分开治**：
     ① 静止态（治「硬」）：页头与内容**同底不加色**——曾试过一层极淡暖底做色阶差，
        但那条色带在页面顶上像贴了块补丁（用户点名拿掉）；现在静止态只留几乎看不见的
        底线。**刻意不上 backdrop-filter**：实测它只会把卡片的红字糊成粉影漂在标题背后
        （是噪点不是层次），还要赔上脱流 + ResizeObserver + 三处 scrollIntoView 的静默回归。
     ② 滚动态（治「切」）：见 .elevated —— 内容真滑到底下时才淡入柔和阴影、把线淡掉。
        滚到顶时页头底下是**空的**，那时打阴影＝给不存在的遮挡关系编故事
        （同 .term-wrap.with-chat 那条「独占/无对话时不加，保持齐边」的分寸）。
     z-index：阴影得压在 .thread 之上 —— .thread 是后面的兄弟，默认会盖住它。 */
  .thread-head { flex: none; padding: 5px 24px; border-bottom: 1px solid transparent; position: relative; z-index: 1; transition: border-bottom-color .18s ease; display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  /* ★ 阴影用伪元素而不是 box-shadow：box-shadow 天然四周都有 —— `0 6px 12px -6px` 水平方向是
     「收缩 6 再模糊 6」，**正好到边**，于是左右两端各留一段 6px 渐弱，看着就是两个圆头戳在窗沿上。
     负 spread 再加也没用：加到不露头，朝下的阴影也就没了。
     伪元素 left:0/right:0 精确铺满、只朝下，左右**零溢出、零圆头**。 */
  /* 滚动态改成 Claude 客户端那样的渐隐：内容滑到页头底下时，从页头往下一段由底色渐变到透明，
     文字是慢慢淡出去的，不是被一刀切掉，也不画阴影和线。 */
  .thread-head::after {
    content: ''; position: absolute; left: 0; right: 0; top: 100%; height: 28px;
    background: linear-gradient(var(--bg), transparent);
    opacity: 0; transition: opacity .18s ease; pointer-events: none;
  }
  /* 只在内容真滑到页头底下时才抬升。border 只改 color 不改宽度 —— 改宽度会让下面整块跳 1px。 */
  .thread-head:global(.elevated) { border-bottom-color: transparent; }
  .thread-head:global(.elevated)::after { opacity: 1; }
  /* 侧栏收起：红绿灯 + 悬浮的折叠/搜索钮压在内容区左上，页头统一左让位（全部视图受益）。
     发现更新时顶部临时增加更新入口，页头再让出一个按钮宽度，避免压住页面标题图标。 */
  :global(.app.rail-collapsed) .thread-head { padding-left: 140px; }
  :global(.app.rail-collapsed.fs) .thread-head, :global(.app.rail-collapsed.win) .thread-head { padding-left: 100px; }
  :global(.app.rail-collapsed.top-update-visible) .thread-head { padding-left: 167px; }
  :global(.app.rail-collapsed.top-update-visible.fs) .thread-head,
  :global(.app.rail-collapsed.top-update-visible.win) .thread-head { padding-left: 127px; }
  /* 会话头同时承担应用标题栏：展开、折叠与分屏都固定成与 win-tools 相同的 30px，
     避免域名按钮的上下 padding 把标题整行撑低。 */
  .thread-head.conversation-thread-head {
    height: 30px; min-height: 30px; padding-top: 0; padding-bottom: 0; align-items: center;
  }
  .th-info { min-width: 0; flex: 1 1 auto; overflow: hidden; }
  .th-info b { display: block; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 15px; line-height: 1.35; }
  .th-info small { display: block; min-width: 0; overflow: hidden; color: var(--dim); font-size: 12px; margin-top: 2px; text-overflow: ellipsis; white-space: nowrap; }
  .th-info small :global(svg), .th-info small :global(img) { vertical-align: -2px; }
  .thread { flex: 1; overflow-y: auto; }
  .thread-inner { max-width: 760px; margin: 0 auto; padding: 22px 24px 8px; display: flex; flex-direction: column; gap: 20px; }
  .msg { display: flex; gap: 12px; }
  .msg.user { justify-content: flex-end; }
  .ubody { background: var(--user-bg); border-radius: 14px; padding: 9px 14px; max-width: 80%; word-break: break-word; font-size: 14.5px; line-height: 1.6; }
  .ub-text { white-space: pre-wrap; display: block; }
  .ub-text.clamp { max-height: 8em; overflow: hidden; -webkit-mask-image: linear-gradient(#000 55%, transparent); mask-image: linear-gradient(#000 55%, transparent); }
  .ub-more { display: block; margin-top: 4px; padding: 0; border: none; background: none; font: inherit; font-size: 12.5px; color: var(--dim); cursor: pointer; }
  .ub-more:hover { color: var(--text); }
  .msg.assistant .body { flex: 1; min-width: 0; }
  .text { white-space: pre-wrap; word-break: break-word; }
  /* 助手消息的 Markdown 阅读版式（渲染出的 HTML 不在 Svelte 作用域内，须 :global） */
  .text.md { white-space: normal; font-size: 14px; line-height: 1.7; }
  .text.md :global(p) { margin: 0 0 10px; }
  .text.md :global(p:last-child), .text.md :global(ul:last-child), .text.md :global(ol:last-child),
  .text.md :global(pre:last-child), .text.md :global(table:last-child) { margin-bottom: 0; }
  .text.md :global(strong) { font-weight: 650; }
  .text.md :global(ul), .text.md :global(ol) { margin: 0 0 10px; padding-left: 22px; }
  .text.md :global(li) { margin: 3px 0; }
  .text.md :global(li::marker) { color: var(--faint); }
  .text.md :global(h1), .text.md :global(h2) { font-size: 16px; font-weight: 650; margin: 16px 0 6px; }
  .text.md :global(h3), .text.md :global(h4) { font-size: 14.5px; font-weight: 650; margin: 14px 0 5px; }
  .text.md :global(h1:first-child), .text.md :global(h2:first-child),
  .text.md :global(h3:first-child), .text.md :global(h4:first-child) { margin-top: 0; }
  .text.md :global(code) { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: .88em; background: #f1efe9; padding: 1px 5px; border-radius: 5px; }
  .text.md :global(pre) { background: #f6f5f1; border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; overflow-x: auto; margin: 0 0 10px; }
  .text.md :global(pre code) { background: none; padding: 0; font-size: 12.5px; line-height: 1.6; }
  .text.md :global(a) { color: var(--accent); text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
  .text.md :global(table) { border-collapse: collapse; margin: 0 0 10px; font-size: 13px; display: block; overflow-x: auto; max-width: 100%; }
  .text.md :global(th), .text.md :global(td) { border: 1px solid var(--border2); padding: 5px 10px; text-align: left; }
  .text.md :global(th) { background: #f6f5f1; font-weight: 600; }
  .text.md :global(blockquote) { margin: 0 0 10px; padding: 2px 12px; border-left: 3px solid var(--border2); color: var(--dim); }
  .text.md :global(hr) { border: none; border-top: 1px solid var(--border); margin: 12px 0; }
  .text.md :global(img) { max-width: 100%; border-radius: 8px; }
  .text.is-err { color: var(--err); }
  /* 重试：无边框，内联跟在红色错误文字后面 */
  .retry-btn { display: inline-flex; align-items: center; gap: 4px; margin-left: 10px; padding: 0; background: none; border: none; color: var(--accent); font: inherit; font-size: 12.5px; cursor: pointer; vertical-align: baseline; }
  .retry-btn:hover { text-decoration: underline; }
  .retry-btn svg { flex: none; }
  /* 额度耗尽卡：明确状态 + 倒计时 + 到点自动续跑 */
  .limit-card { border: 1px solid var(--border2, #e2dfd7); border-left: 3px solid #c98d70; border-radius: 10px; padding: 10px 13px; background: #fbf9f5; max-width: 520px; }
  .limit-head { font-size: 13.5px; font-weight: 600; color: var(--text, #26241f); }
  .limit-sub { margin-top: 3px; font-size: 12.5px; color: var(--dim, #6b675f); }
  .limit-actions { margin-top: 8px; display: flex; flex-wrap: wrap; gap: 4px 8px; }
  .limit-actions .retry-btn { margin-left: 0; }
  .limit-actions .retry-btn.is-armed { color: #3e7a4e; }
  .limit-hint { margin-top: 6px; font-size: 11.5px; color: var(--faint, #9b968c); }
  /* 运行中活动：只展示协议里真实收到的工具生命周期，不把轮询次数冒充业务完成度。 */
  .codex-thinking {
    width: max-content;
    margin: 13px 0 11px;
    font-size: 15px;
    font-weight: 500;
    line-height: 1.5;
  }
  .codex-thinking > span {
    color: transparent;
    background: linear-gradient(90deg, #aaa7a0 20%, #dedbd4 48%, #aaa7a0 76%);
    background-size: 220% 100%;
    background-position: 100% 50%;
    background-clip: text;
    -webkit-background-clip: text;
    animation: codexThinkingShimmer 2.1s ease-in-out infinite;
  }
  @keyframes codexThinkingShimmer {
    0%, 100% { background-position: 100% 50%; opacity: .62; }
    50% { background-position: 0 50%; opacity: 1; }
  }
  @media (max-width: 620px) {
  }
  @media (prefers-reduced-motion: reduce) {
    .codex-thinking > span {
      color: var(--faint);
      background: none;
      animation: none;
      opacity: 1;
    }
  }
  /* 命令列表：默认收起，点击展开 */
  .cmds { margin-bottom: 9px; }
  .cmds summary { display: inline-flex; align-items: center; gap: 6px; cursor: pointer; list-style: none; width: fit-content;
    font-size: 12px; color: var(--dim); border-radius: 8px; padding: 4px 8px; margin-left: -8px; user-select: none; }
  .cmds summary::-webkit-details-marker { display: none; }
  .cmds summary:hover { background: #f1efe9; }
  .cmds .cmd-ic { color: var(--accent); flex: none; }
  .cmds .cmd-chev { flex: none; transition: transform .15s; }
  .cmds[open] .cmd-chev { transform: rotate(180deg); }
  .cmds .tools { margin: 6px 0 0 6px; padding-left: 12px; border-left: 1px solid var(--border); gap: 1px; }
  .tools { display: flex; flex-direction: column; gap: 5px; margin-bottom: 8px; }
  .tool { display: flex; gap: 8px; align-items: baseline; min-width: 0; border-radius: 6px; padding: 3px 8px; font-size: 12.5px; line-height: 1.55; }
  .tool:hover { background: var(--hover, rgba(0, 0, 0, .035)); }
  .tcode { color: var(--text); flex: none; font-weight: 550; }
  .tdetail { color: var(--dim); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .tool-count { flex: none; margin-left: auto; color: var(--faint); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .inlink { background: none; border: none; color: var(--accent); text-decoration: underline; cursor: pointer; padding: 0; font: inherit; word-break: break-all; }
  .err-note { color: var(--err); background: var(--err-soft); border: 1px solid var(--err-border); border-radius: 8px; padding: 8px 10px; font-size: 13px; }
  /* 加载：logo 的 C 一笔描出、圆点落定，循环；后面带本轮耗时。尊重"减弱动态"。 */
  .working { display: flex; align-items: center; gap: 7px; padding: 3px 0; }
  .working:not(:first-child) { margin-top: 12px; }
  .wl { flex: none; }
  .working-t { font-size: 12px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .context-compact-note { display: flex; align-items: center; gap: 6px; width: fit-content; margin: 2px 0 9px; padding: 5px 8px; border-radius: 8px; color: #267457; background: #edf7f1; font-size: 12px; }
  .context-compact-note small { color: #6e8d7d; font-size: 11px; }
  .wl-a { animation: wlA 1.6s ease-in-out infinite; }
  .wl-b { animation: wlB 1.6s ease-in-out infinite; }
  @keyframes wlA { 0% { stroke-dashoffset: 100; opacity: 1; } 35%, 75% { stroke-dashoffset: 0; opacity: 1; } 100% { stroke-dashoffset: 0; opacity: .15; } }
  @keyframes wlB { 0%, 25% { stroke-dashoffset: 100; opacity: 1; } 60%, 75% { stroke-dashoffset: 0; opacity: 1; } 100% { stroke-dashoffset: 0; opacity: .15; } }
  @media (prefers-reduced-motion: reduce) { .wl-a, .wl-b { animation: none; stroke-dashoffset: 0; } }
  .composer-wrap { flex: none; padding: 10px 24px 20px; }
  .composer-wrap .composer { max-width: 760px; margin: 0 auto; }
  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1.5px solid var(--border2); border-radius: 8px; padding: 6px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
  .btn:hover { background: #f6f5f1; }
  .btn:disabled { opacity: .5; cursor: default; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .btn.primary:hover { background: var(--accent-h); }
  .btn.sm { padding: 5px 12px; font-size: 12px; border-radius: 8px; }
  .permit-card { border: 1px solid var(--border2); border-radius: 11px; overflow: hidden; background: #fff; max-width: 480px; }
  .permit-card.danger { border-color: #f0d9a8; }
  .permit-head { display: flex; align-items: center; gap: 7px; padding: 8px 12px; font-size: 12px; font-weight: 600; color: var(--text); background: #f6f5f1; border-bottom: 0.5px solid var(--border); }
  .permit-card.danger .permit-head { color: #8a5a08; background: #fdf6e6; border-bottom-color: #f0e3c4; }
  .permit-dot { width: 7px; height: 7px; border-radius: 50%; background: #b45309; flex: none; }
  .permit-tag { margin-left: auto; font-size: 10px; font-weight: 600; color: #b45309; background: #faf1d8; padding: 1px 7px; border-radius: 5px; }
  .permit-desc { padding: 10px 12px 0; font-size: 13px; font-weight: 550; color: var(--text); line-height: 1.55; word-break: break-word; }
  .permit-meta { padding: 3px 12px 0; font-size: 11px; color: var(--dim); }
  .permit-more { display: block; margin: 7px 12px 0; padding: 0; border: 0; background: none; font-size: 11px; color: var(--dim); cursor: pointer; text-align: left; }
  .permit-more:hover { color: var(--text); }
  .permit-cmd { display: block; margin: 6px 12px 0; font-size: 12px; background: #17130f; color: #e8dcc8; padding: 8px 10px; border-radius: 7px; white-space: pre-wrap; word-break: break-all; max-height: 180px; overflow-y: auto; }
  .permit-act { display: flex; gap: 8px; padding: 10px 12px 12px; }
  .permit-act .btn { flex: 1; }
  .cb-prev { position: relative; display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; font-size: 12px; padding: 3px 8px; border: none; background: none; color: var(--accent); cursor: pointer; font-weight: 500; border-radius: 6px; }
  .cb-prev:hover:not(:disabled) { background: var(--rail); }
  .cb-prev:disabled { opacity: .45; cursor: default; }
  .cb-prev.dim { color: var(--dim); font-weight: 400; }
  .cb-prev svg { flex: none; }
  /* 自定义悬停提示（不用系统 title） */
  .cf-bar { max-width: 760px; margin: 0 auto 6px; display: flex; align-items: center; gap: 2px; flex-wrap: wrap; }
  /* 等待消息（排队）条：琥珀色调，区别于普通输入 */
  .queued-row { display: flex; align-items: center; gap: 8px; margin: 10px 12px 0; padding: 6px 8px 6px 11px; background: #fbf6ea; border: 1px solid #f0e2c0; border-radius: 9px; font-size: 12.5px; }
  .queued-ic { flex: none; color: #b45309; display: inline-flex; }
  .queued-t { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--dim); }
  .queued-btn { flex: none; border: none; background: none; color: var(--accent); font-size: 12px; cursor: pointer; padding: 2px 4px; }
  .queued-btn:hover { text-decoration: underline; }
  .queued-x { flex: none; border: none; background: none; color: var(--faint); font-size: 15px; line-height: 1; cursor: pointer; padding: 0 3px; }
  .queued-x:hover { color: var(--err); }
  .send.queue { background: var(--accent); color: #fff; }
  .attach-row { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding: 10px 14px 2px; }
  .attach-chip { display: inline-flex; align-items: center; gap: 6px; max-width: 190px; padding: 3px 4px 3px 6px; background: var(--panel); border: 1px solid var(--border2); border-radius: 8px; font-size: 12px; }
  .attach-img { width: 24px; height: 24px; object-fit: cover; border-radius: 4px; flex: none; }
  .attach-ic { color: var(--dim); flex: none; display: inline-flex; }
  .attach-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .attach-x { flex: none; border: none; background: none; color: var(--dim); cursor: pointer; font-size: 14px; line-height: 1; padding: 0 2px; }
  /* 用户气泡里的附件卡（只读，白底衬在浅色气泡上） */
  .ub-atts { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .ub-atts.only { margin-top: 0; }
  .ub-att { display: inline-flex; align-items: center; gap: 6px; max-width: 220px; padding: 4px 10px 4px 8px; background: #fff; border: 1px solid var(--border2); border-radius: 8px; font-size: 12px; }
  .ub-att-img { padding: 0; border: 1px solid var(--border2); border-radius: 10px; overflow: hidden; cursor: zoom-in; line-height: 0; background: repeating-conic-gradient(#ececea 0 25%, #fff 0 50%) 0 0 / 14px 14px; display: inline-flex; align-items: center; justify-content: center; min-width: 44px; min-height: 34px; }
  /* 按图片原始比例呈现（纯像素上限、不裁切），宽 logo/竖图都能看到全貌；点击仍可放大。
     注意不能用 max-width:min(…,100%)：容器 shrink-to-fit，百分比循环引用会让整条声明失效。 */
  .ub-att-img img { display: block; max-width: 260px; max-height: 180px; width: auto; height: auto; }
  .ub-att-img.ph { display: inline-block; width: 128px; height: 96px; cursor: default; animation: phpulse 1.2s ease-in-out infinite; }
  /* 助手消息里的生图产物：与用户附件同款卡片，气泡外基底微调 */
  .gen-imgs { margin-top: 6px; }
  .ub-att.as-btn { cursor: pointer; font: inherit; font-size: 12px; color: inherit; }
  .ub-att.as-btn:hover { border-color: var(--border3, var(--border2)); }
  @keyframes phpulse { 0%, 100% { opacity: .55; } 50% { opacity: .9; } }
  .ub-att-ic { flex: none; display: inline-flex; color: var(--dim); }
  .ub-att-n { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .attach-x:hover { color: var(--err); }
  .attach-loading { font-size: 11px; color: var(--dim); }
  .x { background: none; border: none; color: var(--faint); font-size: 20px; cursor: pointer; line-height: 1; }
  .x:hover { color: var(--err); }
  .dim { color: var(--dim); }

  /* 托管运营会话：任务卡 + 计划卡 */
  .msg.user.ops-task { justify-content: stretch; }
  .ops-card { width: 100%; box-sizing: border-box; border: 1px solid var(--border); border-radius: 12px; background: var(--rail); padding: 12px 14px; }
  .ot-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; color: var(--text); }
  .ot-head svg { color: var(--dim); flex: none; }
  .ot-head b { font-size: 13.5px; font-weight: 600; }
  .ot-chip { font-size: 11.5px; color: var(--dim); background: var(--bg); box-shadow: inset 0 0 0 1px var(--border); border-radius: 999px; padding: 1px 8px; }
  .ot-title { margin-top: 6px; font-size: 13.5px; font-weight: 550; color: var(--text); }
  .ot-sub { margin-top: 4px; font-size: 12px; color: var(--dim); line-height: 1.6; }
  .ot-full { margin-top: 8px; }
  .ot-full summary { font-size: 12px; color: var(--faint); cursor: pointer; list-style: none; }
  .ot-full summary::-webkit-details-marker { display: none; }
  .ot-full summary:hover { color: var(--text); }
  .ot-full[open] summary { margin-bottom: 6px; }
  .ot-text { max-height: 320px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; line-height: 1.6; color: var(--dim); background: var(--bg); border-radius: 8px; padding: 10px 12px; }
  .plan-card { margin-top: 10px; border: 1px solid var(--border); border-radius: 12px; padding: 12px 14px; background: var(--bg); }
  .pc-head { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  .pc-head b { font-size: 13.5px; font-weight: 600; }
  .pc-head span { font-size: 12px; color: var(--faint); }
  .pc-notes { margin-top: 10px; padding: 9px 12px; border-radius: 9px; background: var(--su-warn-soft); color: var(--su-warn-ink); font-size: 12.5px; line-height: 1.65; white-space: pre-wrap; }
  .pc-notes b { display: block; font-weight: 600; margin-bottom: 2px; }
  .pc-tasks { list-style: none; margin: 10px 0 0; padding: 0; display: flex; flex-direction: column; }
  .pc-tasks li { display: flex; gap: 10px; padding: 10px 0; border-top: 1px solid var(--border); }
  .pc-tasks li:first-child { border-top: none; }
  .pc-no { flex: none; width: 20px; height: 20px; border-radius: 50%; background: var(--accent-soft); color: var(--dim); font-size: 11px; font-weight: 600; display: inline-flex; align-items: center; justify-content: center; margin-top: 1px; }
  .pc-body { min-width: 0; flex: 1; }
  .pc-body b { font-size: 13px; font-weight: 550; line-height: 1.5; }
  .pc-why { margin: 4px 0 2px; }
  .pc-expect { color: var(--dim); font-size: 12.5px; }
  .pc-body p { margin: 3px 0 0; font-size: 12.5px; color: var(--dim); line-height: 1.65; }
  .pc-body small { display: block; margin-top: 2px; font-size: 11px; color: var(--faint); font-family: ui-monospace, SFMono-Regular, Menlo, monospace; overflow-wrap: anywhere; }
  .pc-pos { margin-top: 8px; border-top: 1px solid var(--border); padding-top: 8px; }
  .pc-pos summary { font-size: 12px; color: var(--faint); cursor: pointer; }
  .pc-pos p { margin: 6px 0 0; font-size: 12.5px; color: var(--dim); line-height: 1.65; }

  /* 步骤行：一步一行，灰字，悬停有框，点开看完整参数（像 Claude 客户端） */
  .step, .steps-sum { margin: 2px 0; max-width: 100%; }
  .step > summary, .steps-sum > summary, .step.solo { display: inline-flex; align-items: center; gap: 7px; max-width: 100%; box-sizing: border-box; padding: 3px 0; font-size: 13px; line-height: 1.5; color: var(--dim); cursor: pointer; list-style: none; user-select: none; transition: color .12s; }
  .step > summary::-webkit-details-marker, .steps-sum > summary::-webkit-details-marker { display: none; }
  .step > summary:hover, .steps-sum > summary:hover, .step[open] > summary, .steps-sum[open] > summary { color: var(--text); }
  .step > summary:hover .st-t b, .steps-sum > summary:hover .st-t, .step[open] > summary .st-t b { color: var(--text); }
  .step.solo { cursor: default; }
  .st-t { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st-t b { font-weight: 550; color: var(--dim); margin-right: 2px; }
  .st-n, .st-ms { flex: none; font-size: 12px; color: var(--faint, #9a958c); font-variant-numeric: tabular-nums; }
  .st-chev { flex: none; color: var(--faint, #9a958c); transition: transform .15s; }
  details[open] > summary > .st-chev { transform: rotate(90deg); }
  .st-full { margin: 4px 0 6px 0; padding: 8px 10px; max-height: 220px; overflow: auto; border-radius: 8px; background: var(--code-bg, #f6f5f1); font-size: 11.5px; line-height: 1.5; white-space: pre-wrap; word-break: break-all; color: var(--dim); }
  .st-list { margin: 2px 0 4px 2px; padding-left: 12px; border-left: 1px solid var(--border); display: flex; flex-direction: column; }
  .step.running > summary { color: var(--text); }
  .step.failed { color: #b42318; }
  /* 正在跑：四个小点轮流亮 */
  .st-dots { flex: none; display: inline-grid; grid-template-columns: 4px 4px; gap: 3px; }
  .st-dots i { width: 4px; height: 4px; border-radius: 50%; background: var(--accent, #d97757); animation: stDot 1.2s ease-in-out infinite; }
  .st-dots i:nth-child(2) { animation-delay: .3s; }
  .st-dots i:nth-child(4) { animation-delay: .6s; }
  .st-dots i:nth-child(3) { animation-delay: .9s; }
  @keyframes stDot { 0%, 100% { opacity: .25; } 50% { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .st-dots i { animation: none; opacity: .7; } }
  .msg.assistant .body > .text + .step, .msg.assistant .body > .text + .steps-sum { margin-top: 8px; }
  .msg.assistant .body > .step + .text, .msg.assistant .body > .steps-sum + .text { margin-top: 8px; }
  .ref-imgs { margin-top: 10px; }
  .ub-att-img.ref { position: relative; background: repeating-conic-gradient(#f1efea 0 25%, #fff 0 50%) 0 0 / 14px 14px; }
  .ub-att-img.ref { width: 120px; height: 120px; }
  .ub-att-img.ref img { width: 100%; height: 100%; max-width: none; max-height: none; object-fit: contain; padding: 10px; box-sizing: border-box; }
  .ref-n { position: absolute; left: 0; right: 0; bottom: 0; padding: 2px 6px; font-size: 10.5px; line-height: 1.5; color: #fff; background: linear-gradient(transparent, rgba(0, 0, 0, .55)); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; text-align: left; }
</style>
