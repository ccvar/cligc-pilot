<script lang="ts">
  import type { Brain } from '$lib/types';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import { tip as tipAction } from '$lib/tip';
  import RefreshIcon from '$lib/components/icons/RefreshIcon.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { refreshBrainsManual, settings, installClaude, installGrok, installCodex, copyCmd, authorize } from '$lib/state/settings.svelte.js';
  import { CLAUDE_INSTALL_CMD, GROK_INSTALL_CMD, keystoreName } from '$lib/state/env';
  import { elapsedLabel } from '$lib/state/messages.svelte.js';
</script>

{#if app.brains}
    <div class="cli-guide" data-no-drag>
      <div class="cli-guide-h"><span>模型准备（本地客户端或设置中的 API 服务）</span><button class="cli-redetect" onclick={refreshBrainsManual} disabled={settings.brainsBusy} title="装好 / 登录完点这里重新检测（会重新读取 PATH）"><RefreshIcon spinning={settings.brainsBusy} /><span>重新检测</span></button></div>
      {#each [{ b: 'claude' as Brain, st: app.brains.claude, name: 'Claude Code', cmd: CLAUDE_INSTALL_CMD }, { b: 'codex' as Brain, st: app.brains.codex, name: 'Codex', cmd: 'npm i -g @openai/codex' }, { b: 'grok' as Brain, st: app.brains.grok, name: 'Grok', cmd: GROK_INSTALL_CMD }] as r (r.b)}
        <div class="cli-row">
          <BrainIcon brain={r.b} size={16} />
          <span class="cli-name">{r.name}</span>
          {#if !r.st.found}
            <span class="cli-tag bad">未安装</span>
            {#if r.b === 'claude'}
              <button class="wr-btn" use:tipAction={'安装失败排障：VPN/代理需覆盖 claude.ai 与下载域名 storage.googleapis.com（规则模式加进规则或临时切全局）；也可复制右侧命令到终端手动执行看完整输出。'} onclick={installClaude} disabled={settings.claudeInstalling}>{#if settings.claudeInstalling}<span class="wr-spin"></span>{settings.nodeBoot || `安装中 ${elapsedLabel(settings.claudeElapsed)}`}{:else}一键安装{/if}</button>
            {:else if r.b === 'grok'}
              <button class="wr-btn" onclick={installGrok} disabled={settings.grokInstalling}>{#if settings.grokInstalling}<span class="wr-spin"></span>{settings.nodeBoot || `安装中 ${elapsedLabel(settings.grokElapsed)}`}{:else}一键安装{/if}</button>
            {:else if r.b === 'codex'}
              <button class="wr-btn" onclick={installCodex} disabled={settings.codexInstalling}>{#if settings.codexInstalling}<span class="wr-spin"></span>{settings.nodeBoot || `安装中 ${elapsedLabel(settings.codexElapsed)}`}{:else}一键安装{/if}</button>
            {/if}
            <code class="cli-cmd" title={r.cmd}>{r.cmd}</code>
            <button class="cli-copy" title="复制命令" aria-label="复制安装命令" onclick={() => copyCmd(r.cmd)}>
              {#if settings.copiedCmd === r.cmd}
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M3.5 8.5l3 3 6-7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" /></svg>
              {:else}
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><rect x="5.4" y="5.4" width="8.2" height="8.2" rx="1.6" stroke="currentColor" stroke-width="1.3" /><path d="M10.6 5.4V4.1A1.6 1.6 0 0 0 9 2.5H4.1A1.6 1.6 0 0 0 2.5 4.1V9a1.6 1.6 0 0 0 1.6 1.6h1.3" stroke="currentColor" stroke-width="1.3" /></svg>
              {/if}
            </button>
          {:else if r.st.logged_in === false}
            <span class="cli-tag warn">未登录</span><button class="authbtn" onclick={() => authorize(r.b)} disabled={settings.authWaiting === r.b}>{#if settings.authWaiting === r.b}<span class="wr-spin"></span>等待授权…{:else}去授权 ↗{/if}</button>
          {:else}
            <span class="cli-tag ok">✓ {r.st.version || '已就绪'}</span>
          {/if}
        </div>
      {/each}
      <p class="cli-note"><svg class="cli-note-ic" width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.3" /><path d="M8 7.3v3.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><circle cx="8" cy="4.8" r="0.95" fill="currentColor" /></svg><span>安装/登录后状态灯自动变绿；密钥只进 {keystoreName}，绝不落盘。</span></p>
      <p class="cli-note"><svg class="cli-note-ic" width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.3" /><path d="M8 7.3v3.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /><circle cx="8" cy="4.8" r="0.95" fill="currentColor" /></svg><span>没有 Node 的机器，一键安装会自动下载托管版 Node(v22 LTS) 到应用数据目录，并写入用户级 PATH（只追加、绝不覆盖或截断；写入失败会提示手动配置）。</span></p>
      {#if !app.brains.browser.found}
        <p class="cli-note"><svg class="cli-note-ic" width="13" height="13" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.3" /><path d="M1.9 8h12.2M8 1.6c-4.4 4.2-4.4 8.6 0 12.8 4.4-4.2 4.4-8.6 0-12.8Z" stroke="currentColor" stroke-width="1.1" /></svg><span>未检测到 Chrome / Edge——「AI 网页截图配图」不可用（可选功能，装个 Chrome 即启用）。</span></p>
      {/if}
    </div>
  {/if}

<style>
  .cli-guide { margin-top: 22px; text-align: left; border-top: 1px solid var(--border); padding-top: 14px; }
  .cli-guide-h { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 11px; letter-spacing: .03em; text-transform: uppercase; color: var(--faint); font-weight: 600; margin-bottom: 6px; }
  .cli-redetect { display: inline-flex; align-items: center; gap: 4px; flex: none; background: none; border: none; padding: 2px 6px; border-radius: 6px; cursor: pointer; color: var(--dim); font: inherit; font-size: 11px; font-weight: 500; letter-spacing: 0; text-transform: none; }
  .cli-redetect:hover { background: rgba(0, 0, 0, .06); color: var(--text); }
  .cli-redetect:disabled { opacity: .55; cursor: default; }
  .cli-redetect :global(svg) { width: 12px; height: 12px; }
  .cli-row { display: flex; align-items: center; gap: 8px; padding: 3px 0; font-size: 13px; }
  .cli-name { width: 100px; flex: none; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .cli-copy { flex: none; width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; border: none; background: none; border-radius: 5px; color: var(--faint); cursor: pointer; }
  .cli-copy:hover { background: rgba(0, 0, 0, .06); color: var(--text); }
  .cli-tag { flex: none; font-size: 11px; padding: 1px 7px; border-radius: 6px; font-weight: 600; }
  .cli-tag.ok { color: #1a7f4b; background: #e7f4ec; }
  .cli-tag.warn { color: #9a6a00; background: #f7efd9; }
  .cli-tag.bad { color: #9a3b2f; background: #f6e7e3; }
  .cli-cmd { flex: 0 1 auto; min-width: 0; font-family: ui-monospace, monospace; font-size: 10.5px; color: var(--faint); background: #f6f5f1; padding: 3px 8px; border-radius: 5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; user-select: text; cursor: text; }
  /* 提高优先级压过 .hero-card p（后者 margin/color 更 specific，会把 top margin 顶成 0）。 */
  .cli-guide .cli-note { display: flex; align-items: center; gap: 5px; font-size: 11.5px; line-height: 1.5; color: var(--faint); margin: 12px 0 0; }
  .cli-note-ic { flex: none; color: var(--faint); }
  .authbtn { display: inline-flex; align-items: center; gap: 3px; background: none; border: none; padding: 1px 2px; color: var(--accent); font-size: 12px; font-weight: 550; line-height: 1; cursor: pointer; }
  .authbtn:hover { color: var(--accent-h); }
  .wr-btn { flex: none; border: none; background: #d9a400; color: #fff; font-size: 11px; font-weight: 500; padding: 3px 9px; border-radius: 6px; cursor: pointer; }
  .wr-btn:hover { background: #c08f00; }
  .wr-btn:disabled { opacity: .85; cursor: default; }
  .wr-spin { display: inline-block; width: 10px; height: 10px; border: 1.6px solid #fff; border-right-color: transparent; border-radius: 50%; animation: rspin .7s linear infinite; margin-right: 5px; vertical-align: -1px; }
  @keyframes rspin { to { transform: rotate(360deg); } }
</style>
