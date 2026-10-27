<script lang="ts">
  // 「后台运行与备份」弹窗（bg.open）：开机启动、不让电脑睡眠、最近停过的时段；Pilot 数据备份到 GitHub 私有仓库、从备份恢复。
  // 从托管运营页右上角的菜单、站点列表上的「后台停了」提醒打开。
  import './site.css';
  import Icon from './Icon.svelte';
  import { escClose } from '$lib/escClose';
  import { app } from '$lib/state/app.svelte.js';
  import { bg, setBackground, setBackup, backupNow, restoreBackup, span, gapWhy, gapTime } from '$lib/state/background.svelte.js';
  import { ago, full } from './fmt';
  import Dropdown from '$lib/Dropdown.svelte';
  // 下拉选项前的 GitHub 小图标（Dropdown 的 svg 选项要整段内联 SVG）
  const GH_SVG = '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>';

  const st = $derived(bg.status);
  const bk = $derived(bg.backup);
  const ghConns = $derived(app.conns.filter((c) => c.kind === 'github'));
  const conn = $derived(ghConns.find((c) => c.id === bk?.settings.conn_id));
  // 还没选过连接：默认第一个 GitHub 连接（点开关或备份时才存）
  const connId = $derived(bk?.settings.conn_id || ghConns[0]?.id || '');
  const repoLabel = $derived(bk?.settings.repo || `${(conn ?? ghConns[0])?.login ?? '你的账号'}/cligc-pilot-backup`);
  let allGaps = $state(false);
  const gaps = $derived(allGaps ? st?.gaps ?? [] : (st?.gaps ?? []).slice(0, 4));
  const kb = (b: number) => (b >= 1024 * 1024 ? `${(b / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`);
  const dev = $derived(!!st?.exe && /[/\\]target[/\\]debug[/\\]/.test(st.exe));
  const close = () => (bg.open = false);
</script>

{#if bg.open}
  <div class="su-mask" role="presentation" onclick={close}></div>
  <div class="su modal bgd" use:escClose={close} role="dialog" aria-modal="true" aria-label="后台运行与备份">
    <header class="sheet-head"><b><Icon name="clock" size={15} />后台运行与备份</b><button class="x" aria-label="关闭" data-tip="关闭" onclick={close}>×</button></header>
    <div class="sheet-body">
      <section>
        <h4>后台运行</h4>
        <p class="hint">托管、在线监测、推广链接检查和备份，都只在 Pilot 开着的时候跑。关掉窗口后它会留在菜单栏继续跑；退出或电脑睡着了就停了。</p>
        {#if st}
          <div class="opt">
            <span class="ot"><b>开机自动启动</b><small>开机后留在菜单栏，不弹窗口，后台照常运营{#if dev}<br /><span class="warn-text">这是开发版，开机启动会指向调试程序；正式版装好后再打开</span>{/if}</small></span>
            <button class="switch" class:on={st.launch_at_login} role="switch" aria-checked={st.launch_at_login} aria-label="开机自动启动" onclick={() => setBackground({ launch_at_login: !st.launch_at_login })}><i></i></button>
          </div>
          <div class="opt" class:dim={!st.can_keep_awake}>
            <span class="ot"><b>托管期间不让电脑自动睡眠</b><small>{st.can_keep_awake ? '只挡放着不动时的自动睡眠，屏幕照常会关；合上笔记本盖子还是会睡。运营任务在跑的时候总会挡着，不用打开这项也不会跑一半睡过去' : '这个系统上不支持'}</small></span>
            <button class="switch" class:on={st.keep_awake} role="switch" aria-checked={st.keep_awake} aria-label="托管期间不让电脑自动睡眠" disabled={!st.can_keep_awake} onclick={() => setBackground({ keep_awake: !st.keep_awake })}><i></i></button>
          </div>
          <div class="gaps">
            <div class="gh">{st.stopped_week ? `最近 7 天后台一共停了 ${span(st.stopped_week)}` : '最近 7 天后台没有停过'}{#if st.awake_now}<span class="pill ok" data-tip="正在挡着电脑自动睡眠">防睡眠中</span>{/if}</div>
            {#if gaps.length}
              <ul>
                {#each gaps as g (g.from)}
                  <li><span class="mono">{gapTime(g.from)} – {gapTime(g.to)}</span><span class="d">{span(g.to - g.from)}</span><span class="faint">{gapWhy(g.kind)}</span></li>
                {/each}
              </ul>
              {#if (st.gaps.length ?? 0) > 4}<button class="link-btn" onclick={() => (allGaps = !allGaps)}>{allGaps ? '收起' : `显示全部 ${st.gaps.length} 段`}</button>{/if}
              <p class="hint">停着的时候到期的任务，下次开着时每样补跑一次（同时最多 3 个站点），不会一下子全部挤在一起。</p>
            {/if}
          </div>
        {/if}
      </section>

      <section>
        <h4>数据备份</h4>
        <p class="hint">站点设置、托管方案、计划、任务队列、运营记录和 AI 用量只在这台电脑上{#if bk}（现在 {bk.files} 个文件、{kb(bk.bytes)}）{/if}。每周推一份到你 GitHub 账号下的私有仓库，换电脑或硬盘坏了可以恢复。不含任何密钥和 Token、对话记录和数据缓存。</p>
        {#if !ghConns.length}
          <p class="hint warn-text">先在「连接与模型」里连上 GitHub，才能备份。</p>
        {:else if bk}
          {#if ghConns.length > 1}
            <div class="opt">
              <span class="ot"><b>用哪个 GitHub 账号</b></span>
              <div class="acct"><Dropdown compact menuCompact value={connId} options={ghConns.map((c) => ({ value: c.id, label: c.login || c.name, svg: GH_SVG }))} ariaLabel="备份用的 GitHub 账号" onchange={(v: string) => setBackup(bk.settings.enabled, v)} /></div>
            </div>
          {/if}
          <div class="opt">
            <span class="ot"><b>每周自动备份</b><small>备份到 <span class="mono">{repoLabel}</span>{bk.settings.repo ? '' : '（第一次备份时自动建，私有）'}</small></span>
            <button class="switch" class:on={bk.settings.enabled} role="switch" aria-checked={bk.settings.enabled} aria-label="每周自动备份" onclick={() => setBackup(!bk.settings.enabled, connId)}><i></i></button>
          </div>
          {#if bk.settings.last_try}
            <p class="last" class:err-text={!bk.settings.last_ok}>
              {#if bk.settings.last_ok}<Icon name="check" size={12} />上次备份 <span data-tip={full(bk.settings.last_at)}>{ago(bk.settings.last_at)}</span>：{bk.settings.last_msg}
              {:else}<Icon name="x" size={12} />上次备份失败（<span data-tip={full(bk.settings.last_try)}>{ago(bk.settings.last_try)}</span>）：{bk.settings.last_msg}{/if}
            </p>
          {:else}<p class="last faint">还没有备份过</p>{/if}
          <div class="row">
            <button class="btn small primary" disabled={!!bg.busy || bk.running} onclick={async () => { if (!bk.settings.conn_id) await setBackup(bk.settings.enabled, connId); await backupNow(); }}>
              {#if bg.busy === 'backup' || bk.running}<span class="spin"></span>正在备份…{:else}<Icon name="upload" size={12} />立即备份{/if}
            </button>
            <button class="btn small ghost" disabled={!!bg.busy || bk.running} onclick={() => restoreBackup(connId, bk.settings.repo)} data-tip="先看备份里有什么，确认后才恢复">
              {#if bg.busy === 'restore'}<span class="spin"></span>读取备份…{:else}<Icon name="undo" size={12} />从备份恢复…{/if}
            </button>
          </div>
        {/if}
      </section>
    </div>
  </div>
{/if}

<style>
  .bgd { width: min(580px, calc(100vw - 32px)); }
  section { display: flex; flex-direction: column; gap: 10px; }
  section + section { padding-top: 14px; border-top: 1px solid var(--border); }
  h4 { margin: 0; font-size: 13px; font-weight: 600; }
  .hint { margin: 0; }
  .opt { display: flex; align-items: center; gap: 14px; justify-content: space-between; }
  .opt.dim { opacity: .55; }
  .ot { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .ot b { font-size: 12.5px; font-weight: 550; }
  .ot small { font-size: 11.5px; line-height: 1.55; color: var(--faint); }
  .acct { flex: none; display: flex; justify-content: flex-end; }
  .switch { position: relative; flex: none; width: 32px; height: 18px; border: none; border-radius: 999px; background: var(--border2); cursor: pointer; padding: 0; transition: background-color .18s; }
  .switch i { position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; box-shadow: 0 1px 2px rgba(0, 0, 0, .2); transition: transform .18s var(--su-ease); }
  .switch.on { background: var(--ok); }
  .switch.on i { transform: translateX(14px); }
  .switch:disabled { cursor: default; }
  .switch:focus-visible { box-shadow: var(--su-ring); }
  .gaps { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; padding: 10px 12px; border: 1px solid var(--border); border-radius: 10px; }
  .gh { display: flex; align-items: center; gap: 8px; font-size: 12.5px; }
  .gaps ul { list-style: none; margin: 0; padding: 0; width: 100%; display: flex; flex-direction: column; gap: 3px; }
  .gaps li { display: flex; gap: 12px; font-size: 12px; color: var(--dim); }
  .gaps li .mono { font-size: 11.5px; }
  .gaps li .d { width: 96px; font-variant-numeric: tabular-nums; }
  .last { margin: 0; display: flex; align-items: center; gap: 5px; font-size: 12px; color: var(--dim); }
  .last.err-text { color: var(--err); }
  .mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
</style>
