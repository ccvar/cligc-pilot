<script lang="ts">
  import SiteFav from '$lib/SiteFav.svelte';
  import BrainIcon from '$lib/BrainIcon.svelte';
  import TemplateIcon from '$lib/components/icons/TemplateIcon.svelte';
  import PromptIcon from '$lib/components/icons/PromptIcon.svelte';
  import WorkspaceMark from '$lib/components/icons/WorkspaceMark.svelte';
  import SshMark from '$lib/components/icons/SshMark.svelte';
  import ConnectionMark from '$lib/components/icons/ConnectionMark.svelte';
  import { search, pickEntry } from '$lib/state/search.svelte.js';
  import { workspaceFolderName } from '$lib/state/launcher.svelte.js';
  import { brainLabel, modelIcon } from '$lib/state/models';
  import { catOf } from '$lib/state/library.svelte.js';
  import { app } from '$lib/state/app.svelte.js';
</script>

<!-- 会话搜索（Claude Code 风格：顶部圆角搜索框 + 结果列表） -->
{#if search.searchOpen}
  <div class="mask" role="presentation" onclick={() => (search.searchOpen = false)}></div>
  <div class="search-box" role="dialog" aria-modal="true" aria-label="搜索会话">
      <div class="search-head">
        <svg class="si-ic" width="16" height="16" viewBox="0 0 18 18" fill="none">
          <circle cx="8" cy="8" r="5" stroke="currentColor" stroke-width="1.4" />
          <path d="M11.7 11.7L15 15" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
        <input bind:this={search.searchInput} bind:value={search.searchQ} placeholder="搜索会话、连接（名称/IP）、模板…" spellcheck="false" autocapitalize="off" autocorrect="off" />
        <kbd>esc</kbd>
      </div>
      <div class="search-list">
        {#if search.searchFlat.length === 0}
          <p class="search-empty">没有匹配结果</p>
        {:else}
          {#each search.searchSections as sec, si (sec.title)}
            {@const base = search.searchSections.slice(0, si).reduce((n, s) => n + s.entries.length, 0)}
            <div class="search-sec">{sec.title}</div>
            {#each sec.entries as it, i (`${sec.title}-${i}`)}
              <button class="search-item {base + i === search.searchIdx ? 'on' : ''}" onclick={() => pickEntry(it)} onmouseenter={() => (search.searchIdx = base + i)}>
                {#if it.kind === 'view'}
                  {#if it.view === 'templates'}<TemplateIcon size={15} />{:else}<PromptIcon size={15} />{/if}
                  <span class="si-main"><b>{it.label}</b><small>切换到{it.label}视图</small></span>
                {:else if it.kind === 'conv'}
                  {#if it.c.task_type === 'workspace'}
                    <WorkspaceMark size={15} />
                  {:else if it.c.task_type === 'remote'}
                    <SshMark size={15} />
                  {:else}
                    <SiteFav src="" label={it.c.site_slug} size={15} />
                  {/if}
                  <span class="si-main"><b>{it.c.title || '未命名会话'}</b><small>{#if it.c.task_type === 'workspace'}{it.c.workspace_dir ? workspaceFolderName(it.c.workspace_dir) : '纯对话'}{:else}{it.c.site_name || it.c.site_slug}{/if}</small></span>
                  {@render brainTag(it.c.brain, brainLabel(it.c.brain))}
                {:else if it.kind === 'template'}
                  <!-- 与左栏「模板库」同一个图标：搜到的东西该看得出是同一样东西 -->
                  <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="2.5" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="2.5" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /><rect x="9" y="9" width="4.5" height="4.5" rx="1" stroke="currentColor" stroke-width="1.3" /></svg>
                  <span class="si-main"><b>{it.t.name}</b><small>{catOf(it.t)}{it.t.desc ? ` · ${it.t.desc}` : ''}</small></span>
                  {#if it.t.builtin}<span class="si-tag">内置</span>{/if}
                {:else}
                  <!-- 与连接切换器同一个标 -->
                  <ConnectionMark kind={it.c.kind} size={15} />
                  <span class="si-main"><b>{it.c.name}</b><small>{#if it.c.kind === 'ssh'}{it.c.ssh_user}@{it.c.ssh_host}{it.c.ssh_port && it.c.ssh_port !== 22 ? ':' + it.c.ssh_port : ''}{it.c.ssh_os ? ` · ${it.c.ssh_os}` : ''}{:else if it.c.kind === 'cloudflare'}Cloudflare · {it.c.remark?.trim() || it.c.key_prefix}{/if}</small></span>
                  {#if app.activeConnId === it.c.id}<span class="si-tag">当前</span>{/if}
                {/if}
              </button>
            {/each}
          {/each}
        {/if}
      </div>
  </div>
{/if}

{#snippet brainTag(brain: string, label: string)}<span class="btag"><BrainIcon brain={modelIcon(brain)} size={12} />{label}</span>{/snippet}

<style>
  /* 会话搜索（命令面板式；.mask 提供遮罩，box 居中于顶部） */
  .search-box { position: fixed; z-index: 61; top: 12vh; left: 50%; transform: translateX(-50%); width: min(560px, 92vw); background: #fff; border: 1px solid var(--border); border-radius: 14px; box-shadow: 0 24px 60px rgba(20, 15, 8, .28); overflow: hidden; display: flex; flex-direction: column; max-height: 66vh; }
  .search-head { display: flex; align-items: center; gap: 10px; padding: 12px 15px; border-bottom: 1px solid var(--border); }
  .search-head .si-ic { color: var(--faint); flex: none; }
  .search-head input { flex: 1; min-width: 0; border: none; outline: none; background: none; font: inherit; font-size: 15px; color: var(--text); }
  .search-head kbd { font: inherit; font-size: 10.5px; color: var(--faint); border: 1px solid var(--border2); border-radius: 5px; padding: 1px 6px; background: var(--rail); flex: none; }
  .search-list { min-width: 0; overflow-x: hidden; overflow-y: auto; padding: 6px; }
  .search-sec { padding: 8px 10px 3px; font-size: 11px; font-weight: 600; letter-spacing: .04em; color: var(--faint); }
  .search-empty { text-align: center; color: var(--faint); font-size: 13px; padding: 26px 12px; margin: 0; }
  .search-item { width: 100%; display: flex; align-items: center; gap: 10px; background: none; border: none; border-radius: 9px; padding: 9px 10px; cursor: pointer; text-align: left; color: var(--text); }
  .search-item:hover, .search-item.on { background: #f4f3ef; }
  .si-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .si-main b { font-weight: 500; font-size: 13.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .si-main small { color: var(--dim); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .si-tag { flex: none; font-size: 10px; color: var(--faint); border: 1px solid var(--border2); border-radius: 4px; padding: 0 4px; line-height: 15px; }
  .btag { display: inline-flex; align-items: center; gap: 4px; }
  .mask { position: fixed; inset: 0; background: rgba(25,20,10,.28); z-index: 50; }
</style>
