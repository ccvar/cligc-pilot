<script lang="ts">
  // 设置里的「后台运行与备份」：开机启动、不让电脑睡眠、最近停过的时段；Pilot 数据备份到 GitHub 私有仓库、从备份恢复。
  // 说明文字都收进 ⓘ 悬停提示。「后台停了」提醒上的按钮会打开设置并滚到这里（openBackground）。
  import '$lib/components/site/site.css';
  import Icon from '$lib/components/site/Icon.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { bg, loadBackground, loadBackup, setBackground, setBackup, backupNow, restoreBackup, span, gapWhy, gapTime } from '$lib/state/background.svelte.js';
  import { ago, full } from '$lib/components/site/fmt';
  import Dropdown from '$lib/Dropdown.svelte';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  // 下拉选项前的 GitHub 小图标（Dropdown 的 svg 选项要整段内联 SVG）
  const GH_SVG = '<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>';

  const st = $derived(bg.status);
  const bk = $derived(bg.backup);
  const ghConns = $derived(app.conns.filter((c) => c.kind === 'github'));
  const conn = $derived(ghConns.find((c) => c.id === bk?.settings.conn_id));
  // 还没选过连接：默认第一个 GitHub 连接（点开关或备份时才存）
  const connId = $derived(bk?.settings.conn_id || ghConns[0]?.id || '');
  const repoLabel = $derived(bk?.settings.repo || `${(conn ?? ghConns[0])?.login ?? '你的账号'}/cligc-pilot-backup`);
  const kb = (b: number) => (b >= 1024 * 1024 ? `${(b / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`);
  const dev = $derived(!!st?.exe && /[/\\]target[/\\]debug[/\\]/.test(st.exe));
  let gapsOpen = $state(false);
  // 上次备份的说明：「已备份到 x/y：34 个站点、74 个文件」→ 只留冒号后面
  const lastDetail = $derived((bk?.settings.last_msg ?? '').split('：').slice(1).join('：') || bk?.settings.last_msg || '');
  // 打开设置时读一次最新状态
  $effect(() => { void loadBackground(); void loadBackup(); });
</script>

<div class="su bgs" id="bg-settings" class:flash={bg.flash}>
  <section>
    <h4>后台运行<InfoTip label="后台运行说明" text={'托管、在线监测、推广链接检查和备份，都只在 Pilot 开着的时候跑。\n关掉窗口后它会留在菜单栏继续跑；退出或电脑睡着了就停了。\n停着的时候到期的任务，下次开着时每样补跑一次（同时最多 3 个站点）。'} /></h4>
    {#if st}
      <div class="rows">
        <div class="opt">
          <span class="ot"><b>开机自动启动<InfoTip label="开机自动启动" text="开机后留在菜单栏，不弹窗口，后台照常运营" />{#if dev}<span class="pill warn" data-tip="开机启动会指向调试程序；正式版装好后再打开">开发版</span>{/if}</b></span>
          <button class="switch" class:on={st.launch_at_login} role="switch" aria-checked={st.launch_at_login} aria-label="开机自动启动" onclick={() => setBackground({ launch_at_login: !st.launch_at_login })}><i></i></button>
        </div>
        <div class="opt" class:dim={!st.can_keep_awake}>
          <span class="ot"><b>托管期间不让电脑自动睡眠<InfoTip label="不让电脑自动睡眠" text={st.can_keep_awake ? '只挡放着不动时的自动睡眠，屏幕照常会关；合上笔记本盖子还是会睡。\n运营任务在跑的时候总会挡着，不打开这项也不会跑一半睡过去。' : '这个系统上不支持'} />{#if st.awake_now}<span class="pill ok" data-tip="正在挡着电脑自动睡眠">防睡眠中</span>{/if}</b></span>
          <button class="switch" class:on={st.keep_awake} role="switch" aria-checked={st.keep_awake} aria-label="托管期间不让电脑自动睡眠" disabled={!st.can_keep_awake} onclick={() => setBackground({ keep_awake: !st.keep_awake })}><i></i></button>
        </div>
        <div class="opt">
          <span class="ot"><b>最近 7 天</b></span>
          {#if st.gaps.length}
            <button class="val link" onclick={() => (gapsOpen = !gapsOpen)} aria-expanded={gapsOpen}>{st.stopped_week ? `后台停了 ${span(st.stopped_week)}` : '没有停过'}<span class="faint"> · {gapsOpen ? '收起' : '看时段'}</span></button>
          {:else}<span class="val faint">后台没有停过</span>{/if}
        </div>
        {#if gapsOpen && st.gaps.length}
          <ul class="gaps">
            {#each st.gaps as g (g.from)}
              <li><span class="mono">{gapTime(g.from)} – {gapTime(g.to)}</span><span class="d">{span(g.to - g.from)}</span><span class="faint">{gapWhy(g.kind)}</span></li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </section>

  <section>
    <h4>数据备份<InfoTip label="数据备份说明" text={'站点设置、托管方案、计划、任务队列、运营记录和 AI 用量只在这台电脑上。\n每周推一份到你 GitHub 账号下的私有仓库，换电脑或硬盘坏了可以恢复。\n不含任何密钥和 Token、对话记录和数据缓存。'} />{#if bk}<span class="h-sub">{bk.files} 个文件 · {kb(bk.bytes)}</span>{/if}</h4>
    {#if !ghConns.length}
      <p class="hint warn-text">先在上面的「连接」里连上 GitHub，才能备份。</p>
    {:else if bk}
      <div class="rows">
        {#if ghConns.length > 1}
          <div class="opt">
            <span class="ot"><b>GitHub 账号</b></span>
            <div class="acct"><Dropdown compact menuCompact value={connId} options={ghConns.map((c) => ({ value: c.id, label: c.login || c.name, svg: GH_SVG }))} ariaLabel="备份用的 GitHub 账号" onchange={(v: string) => setBackup(bk.settings.enabled, v)} /></div>
          </div>
        {/if}
        <div class="opt">
          <span class="ot"><b>每周自动备份</b><small class="mono" data-tip={bk.settings.repo ? '备份仓库（私有）' : '第一次备份时自动建，私有'}>{repoLabel}</small></span>
          <button class="switch" class:on={bk.settings.enabled} role="switch" aria-checked={bk.settings.enabled} aria-label="每周自动备份" onclick={() => setBackup(!bk.settings.enabled, connId)}><i></i></button>
        </div>
        <div class="opt">
          {#if !bk.settings.last_try}<span class="last faint">还没有备份过</span>
          {:else if bk.settings.last_ok}<span class="last" data-tip={`${full(bk.settings.last_at)}\n${bk.settings.last_msg}`}><Icon name="check" size={12} />上次备份 {ago(bk.settings.last_at)}{#if lastDetail}<span class="faint"> · {lastDetail}</span>{/if}</span>
          {:else}<span class="last err-text" data-tip={bk.settings.last_msg}><Icon name="x" size={12} />上次备份失败（{ago(bk.settings.last_try)}）：{bk.settings.last_msg}</span>{/if}
          <span class="acts">
            <button class="btn tiny ghost" disabled={!!bg.busy || bk.running} onclick={() => restoreBackup(connId, bk.settings.repo)} data-tip="先看备份里有什么，确认后才恢复">
              {#if bg.busy === 'restore'}<span class="spin"></span>读取…{:else}<Icon name="undo" size={11} />恢复…{/if}
            </button>
            <button class="btn tiny" disabled={!!bg.busy || bk.running} onclick={async () => { if (!bk.settings.conn_id) await setBackup(bk.settings.enabled, connId); await backupNow(); }}>
              {#if bg.busy === 'backup' || bk.running}<span class="spin"></span>备份中…{:else}<Icon name="upload" size={11} />立即备份{/if}
            </button>
          </span>
        </div>
      </div>
    {/if}
  </section>
</div>

<style>
  .bgs { display: flex; flex-direction: column; gap: 12px; margin-top: 12px; border-radius: 12px; transition: box-shadow .4s; }
  /* 从「后台停了」提醒跳过来时闪一下，长长的设置里一眼找到 */
  .bgs.flash { box-shadow: 0 0 0 6px color-mix(in srgb, var(--warn) 22%, transparent); }
  section { display: flex; flex-direction: column; gap: 8px; }
  /* 和设置里其它分组标题（.sec-head）同一个样子 */
  h4 { margin: 0 0 1px; display: flex; align-items: center; gap: 2px; font-size: 11px; font-weight: 600; letter-spacing: .03em; color: var(--faint); }
  .h-sub { margin-left: auto; font-weight: 400; letter-spacing: 0; font-size: 11.5px; }
  .hint { margin: 0; }
  /* 一组设置一张卡片，每项一行，行之间细线 */
  .rows { border: 1px solid var(--border); border-radius: 10px; overflow: hidden; background: #fff; }
  .opt { min-height: 44px; box-sizing: border-box; display: flex; align-items: center; gap: 12px; justify-content: space-between; padding: 7px 14px; }
  .opt + .opt { border-top: 1px solid var(--border); }
  .opt.dim { opacity: .55; }
  .ot { min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .ot b { display: inline-flex; align-items: center; gap: 4px; font-size: 13px; font-weight: 500; }
  .ot b .pill { margin-left: 4px; }
  .ot small { font-size: 11.5px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .val { font-size: 12.5px; }
  .val.link { border: 0; background: none; padding: 0; font: inherit; font-size: 12.5px; color: var(--warn); cursor: pointer; }
  .acct { flex: none; display: flex; justify-content: flex-end; }
  .switch { position: relative; flex: none; width: 32px; height: 18px; border: none; border-radius: 999px; background: var(--border2); cursor: pointer; padding: 0; transition: background-color .18s; }
  .switch i { position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; box-shadow: 0 1px 2px rgba(0, 0, 0, .2); transition: transform .18s var(--su-ease); }
  .switch.on { background: var(--ok); }
  .switch.on i { transform: translateX(14px); }
  .switch:disabled { cursor: default; }
  .switch:focus-visible { box-shadow: var(--su-ring); }
  .gaps { list-style: none; margin: 0; padding: 4px 14px 10px; display: flex; flex-direction: column; gap: 3px; }
  .gaps li { display: flex; gap: 12px; font-size: 12px; color: var(--dim); }
  .gaps li .mono { font-size: 11.5px; }
  .gaps li .d { width: 96px; font-variant-numeric: tabular-nums; }
  .last { min-width: 0; display: inline-flex; align-items: center; gap: 5px; font-size: 12.5px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .last :global(svg) { flex: none; color: var(--ok); }
  .last.err-text { color: var(--err); }
  .last.err-text :global(svg) { color: var(--err); }
  .acts { flex: none; display: inline-flex; gap: 6px; }
  .mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
</style>
