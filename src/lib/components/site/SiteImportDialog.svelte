<script lang="ts">
  import { escClose } from '$lib/escClose';
  import './site.css';
  import Dropdown from '$lib/Dropdown.svelte';
  import SiteMark from '$lib/components/icons/SiteMark.svelte';
  import GhMark from '$lib/components/icons/GhMark.svelte';
  import { app } from '$lib/state/app.svelte.js';
  import { sites, closeSiteImport, pickImportFolder, detectImport, confirmImport, confirmBatchImport, importPickedRepos, loadImportRepos, loadImportProjects } from '$lib/state/sites.svelte.js';
  import Icon from './Icon.svelte';
  import { openGhConnect } from '$lib/state/github.svelte.js';
  import { openCfConnect } from '$lib/state/cloudflare.svelte.js';

  const f = $derived(sites.imp);
  let pathIn = $state('');
  const ghConns = $derived(app.conns.filter((c) => c.kind === 'github'));
  const cfConns = $derived(app.conns.filter((c) => c.kind === 'cloudflare'));
  const d = $derived(f.preview?.detected);
  // GitHub：可以勾多个仓库
  let repoQ = $state('');
  // 组件一直挂着：关掉再打开不该还留着上次的路径和搜索词
  $effect(() => { if (!f.open) { pathIn = ''; repoQ = ''; } });
  const shownRepos = $derived(f.repos.filter((r) => !repoQ.trim() || r.full_name.toLowerCase().includes(repoQ.trim().toLowerCase())));
  function toggleRepo(name: string) { f.repoPicks = f.repoPicks.includes(name) ? f.repoPicks.filter((x) => x !== name) : [...f.repoPicks, name]; }
  const imported = $derived(new Set(sites.list.filter((s) => s.source === 'github').map((s) => s.repo)));
  // 批量清单
  const detecting = $derived(f.batch.filter((it) => it.status === 'detecting').length);
  const picked = $derived(f.batch.filter((it) => it.pick && it.status === 'ready').length);
  const DEPLOY_SHORT: Record<string, string> = { direct: '构建后上传到 Pages', workers: '构建后部署到 Workers', vps: 'VPS 发布脚本', pages_git: 'Pages 绑定 GitHub', none: '暂不部署' };
  const modeOpts = $derived([
    { value: 'direct', label: 'Pilot 构建后上传到 Pages', sub: '构建、检查、wrangler pages deploy 都由 Pilot 执行' },
    { value: 'workers', label: 'Pilot 构建后部署到 Workers', sub: '按仓库里的 wrangler 配置执行 wrangler deploy（静态资源 + Worker）' },
    { value: 'vps', label: 'VPS（项目自带的发布脚本）', sub: '检查通过后执行发布脚本；SSH 用 Pilot 的部署密钥，不用输密码' },
    ...(f.source === 'github' ? [{ value: 'pages_git', label: 'Pages 绑定 GitHub', sub: '合并到生产分支后 Cloudflare 自动构建' }] : []),
    { value: 'none', label: '暂不部署', sub: '只管代码与数据，以后再配' },
  ]);
</script>

{#if f.open}
  <div class="su-mask" role="presentation" onclick={closeSiteImport}></div>
  <div class="su modal" use:escClose={closeSiteImport} role="dialog" aria-modal="true" aria-label="导入站点">
    <header class="sheet-head">
      <b><SiteMark size={15} /> 导入站点</b>
      <button class="x" aria-label="关闭" data-tip="关闭" onclick={closeSiteImport} disabled={f.busy}>×</button>
    </header>
    <div class="sheet-body">
      {#if f.step === 'source'}
        <p class="hint">把一个用代码管理的静态站接进来：之后可以在站点里开会话改代码、构建上线、接入 GA4 / Search Console / Bing 数据并开启自动化运营。</p>
        <div class="seg" role="tablist">
          <button class:on={f.source === 'local'} onclick={() => (f.source = 'local')}>本地文件夹</button>
          <button class:on={f.source === 'github'} onclick={() => { f.source = 'github'; if (!f.repos.length) void loadImportRepos(); }}>GitHub 仓库</button>
        </div>
        {#if f.source === 'local'}
          <p class="hint">选择站点代码所在的文件夹，按住 ⌘ 可以一次选多个。改动会直接落在这个文件夹里，Pilot 不会替你提交或推送。</p>
          <div class="row">
            <button class="btn primary" onclick={pickImportFolder} disabled={f.busy}>{f.busy ? '正在识别…' : '选择文件夹…'}</button>
            <span class="faint" style="font-size:12px">或</span>
            <input class="tin mono" bind:value={pathIn} placeholder="粘贴文件夹路径，回车识别" spellcheck="false" onkeydown={(e) => { if (e.key === 'Enter' && pathIn.trim() && !f.busy) void detectImport({ path: pathIn.trim() }); }} />
          </div>
        {:else if !ghConns.length}
          <p class="hint">还没有 GitHub 连接。先连接一个 fine-grained Token，再回来选仓库。</p>
          <div class="row"><button class="btn" onclick={() => { closeSiteImport(); openGhConnect(); }}><GhMark size={14} /> 连接 GitHub</button></div>
        {:else}
          <div class="grid2">
            <div class="field"><span class="lbl">GitHub 连接</span>
              <Dropdown bind:value={f.ghConnId} options={ghConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name, sub: c.login }))} onchange={() => { f.repo = ''; f.repoPicks = []; void loadImportRepos(); }} />
            </div>
            <div class="field"><label for="si-repo-q">仓库 <small class="opt">可以勾多个</small></label>
              <input id="si-repo-q" class="tin" bind:value={repoQ} placeholder={f.reposLoading ? '正在读取…' : `搜索 ${f.repos.length} 个仓库`} spellcheck="false" />
            </div>
          </div>
          <div class="repo-list" role="listbox" aria-multiselectable="true">
            {#each shownRepos as r (r.full_name)}
              <label class="repo-row" class:on={f.repoPicks.includes(r.full_name)}>
                <input type="checkbox" checked={f.repoPicks.includes(r.full_name)} onchange={() => toggleRepo(r.full_name)} />
                <span class="mono rn">{r.full_name}</span>
                {#if imported.has(r.full_name)}<span class="tag">已导入</span>{/if}
                <small>{r.private ? '私有' : '公开'}{r.can_push ? '' : ' · 只读'}</small>
              </label>
            {:else}
              <p class="empty">{f.reposLoading ? '正在读取仓库…' : repoQ ? '没有匹配的仓库' : '这个连接下没有仓库'}</p>
            {/each}
          </div>
          <p class="hint">Pilot 会把仓库克隆到这个 GitHub 连接的工作区（与仓库会话共用同一份检出），AI 的改动走分支和 PR。</p>
          <div class="row">
            <button class="btn primary" onclick={importPickedRepos} disabled={f.busy || !f.repoPicks.length}>{f.busy ? '正在克隆并识别…' : f.repoPicks.length > 1 ? `下一步：识别 ${f.repoPicks.length} 个仓库` : '下一步'}</button>
            {#if f.repoPicks.length > 1}<button class="link-btn" onclick={() => (f.repoPicks = [])} disabled={f.busy}>清空</button>{/if}
          </div>
        {/if}
      {:else if f.step === 'batch'}
        <p class="hint">{detecting ? `正在识别 ${f.batch.length - detecting} / ${f.batch.length}…` : `识别了 ${f.batch.length} 个`}。勾选要导入的，名称可以直接改；构建命令和部署方式按识别结果来，导入后在每个站点的设置里都能改。</p>
        <div class="batch">
          {#each f.batch as it (it.key)}
            <div class="b-row" class:off={!it.pick && it.status !== 'done'}>
              <input type="checkbox" bind:checked={it.pick} disabled={it.status !== 'ready' || f.busy} aria-label={`导入 ${it.name || it.key}`} />
              <div class="b-main">
                {#if it.status === 'ready' || it.status === 'done'}
                  <input class="tin b-name" bind:value={it.name} disabled={it.status === 'done' || f.busy} aria-label="站点名称" />
                  <small class="b-meta">{it.preview?.detected.framework}{it.domain ? ` · ${it.domain}` : ''} · {DEPLOY_SHORT[it.mode]}{it.project && it.mode !== 'none' && it.mode !== 'vps' ? `（${it.project}）` : ''}</small>
                {:else}
                  <b class="b-key">{f.source === 'github' ? it.key : it.key.split(/[\\/]/).filter(Boolean).pop()}</b>
                {/if}
                <small class="mono b-path" title={it.key}>{it.key}</small>
                {#if it.status === 'error' || it.err}<small class="b-err">{it.err}</small>{/if}
                {#if it.dup}<small class="b-note">已经导入过（「{it.dup}」），默认不再导入</small>{/if}
                {#if it.note}<small class="b-note">{it.note}</small>{/if}
              </div>
              <span class="b-st">
                {#if it.status === 'detecting'}<span class="faint">识别中…</span>
                {:else if it.status === 'done'}<span class="ok-text"><Icon name="check" size={12} /> 已导入</span>
                {:else if it.status === 'error'}<span class="warn-text">识别失败</span>{/if}
              </span>
            </div>
          {/each}
        </div>
      {:else if d}
        <div class="field"><span class="lbl">代码</span>
          <div class="row"><span class="pill">{f.source === 'github' ? 'GitHub' : '本地'}</span><span class="mono dim" style="font-size:12px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{f.source === 'github' ? f.repo : f.preview?.dir}</span></div>
          <small>识别为 {d.framework}{d.git ? ` · git 分支 ${d.git_branch || '（分离）'}` : ' · 不是 git 仓库'}{d.github_repo && f.source === 'local' ? ` · 远程 ${d.github_repo}` : ''}</small>
        </div>
        <div class="grid2">
          <div class="field"><label for="si-name">站点名称</label><input id="si-name" class="tin" bind:value={f.name} /></div>
          <div class="field"><label for="si-domain">主域名</label><input id="si-domain" class="tin" bind:value={f.domain} placeholder="example.com" spellcheck="false" /></div>
        </div>
        {#if f.root}<p class="hint">站点在仓库的 <code>{f.root}/</code> 子目录里：安装、构建、检查和部署都在那里执行。</p>{/if}
        <div class="grid3">
          <div class="field"><label for="si-install">安装依赖</label><input id="si-install" class="tin mono" bind:value={f.install} placeholder="（不需要）" spellcheck="false" /></div>
          <div class="field"><label for="si-build">构建命令</label><input id="si-build" class="tin mono" bind:value={f.build} placeholder="（纯静态）" spellcheck="false" /></div>
          <div class="field"><label for="si-out">产物目录</label><input id="si-out" class="tin mono" bind:value={f.outDir} spellcheck="false" /></div>
        </div>
        <div class="field"><label for="si-checks">上线前检查（每行一条，任何一条失败都不上传）</label>
          <textarea id="si-checks" class="tin" rows="2" bind:value={f.checks} placeholder="例如 npm run check" spellcheck="false"></textarea>
        </div>
        {#if d.env.length}
          <label class="check"><input type="checkbox" bind:checked={f.keepEnv} /><span>把 deploy 脚本里的构建变量 <code>{d.env.map(([k]) => k).join('、')}</code> 存为站点构建变量（值进钥匙串，只在 Pilot 构建时注入）</span></label>
        {/if}
        {#if f.source === 'github' && d.dotenv_example_keys.length}
          <p class="hint warn-text">仓库里有 .env.example（{d.dotenv_example_keys.join('、')}）。.env 不在仓库里，导入后到站点设置的「构建变量」里补齐。</p>
        {/if}

        <div class="field"><span class="lbl">部署</span>
          <Dropdown bind:value={f.mode} options={modeOpts} />
        </div>
        {#if f.mode === 'vps'}
          <div class="grid2">
            <div class="field"><span class="lbl">服务器（SSH 连接）</span>
              <Dropdown bind:value={f.sshConnId} placeholder="选择远程连接" options={app.conns.filter((c) => c.kind === 'ssh').map((c) => ({ value: c.id, label: c.name, sub: `${c.ssh_user}@${c.ssh_host}` }))} />
            </div>
            <div class="field"><label for="si-cmd">发布命令</label><input id="si-cmd" class="tin mono" bind:value={f.command} spellcheck="false" /></div>
          </div>
          <p class="hint">导入后在站点设置里点「为部署授权」：Pilot 通过这条连接在服务器上加一把专用部署密钥，以后发布不用再输密码。</p>
        {:else if f.mode !== 'none'}
          {#if !cfConns.length}
            <p class="hint">还没有 Cloudflare 连接。<button class="link-btn" onclick={() => { closeSiteImport(); openCfConnect(); }}>连接 Cloudflare</button>，或先选「暂不部署」。</p>
          {:else}
            <div class="grid3">
              <div class="field"><span class="lbl">Cloudflare 连接</span>
                <Dropdown bind:value={f.cfConnId} options={cfConns.map((c) => ({ value: c.id, label: c.remark?.trim() || c.name }))} onchange={() => void loadImportProjects()} />
              </div>
              {#if f.mode === 'workers'}
                <div class="field"><label for="si-proj">Worker 名称</label>
                  <input id="si-proj" class="tin mono" bind:value={f.project} spellcheck="false" />
                  <small>来自 wrangler 配置；路由、绑定和变量也都按那份配置部署。</small>
                </div>
              {:else}
                <div class="field"><label for="si-proj">Pages 项目</label>
                  <input id="si-proj" class="tin mono" bind:value={f.project} list="si-projects" placeholder={f.projectsLoading ? '读取中…' : '项目名'} spellcheck="false" />
                  <datalist id="si-projects">{#each f.projects as p (p.name)}<option value={p.name}>{p.domains.join(', ')}</option>{/each}</datalist>
                  <small>{f.projects.find((p) => p.name === f.project) ? `已存在 · ${f.projects.find((p) => p.name === f.project)?.source_type === 'github' ? '绑定了 GitHub' : '直接上传'}` : f.project ? '不存在，首次上线时自动创建' : ''}</small>
                </div>
                <div class="field"><label for="si-branch">生产分支</label><input id="si-branch" class="tin mono" bind:value={f.branch} spellcheck="false" /></div>
              {/if}
            </div>
          {/if}
        {/if}
        <div class="field"><label for="si-indexnow">IndexNow 密钥</label>
          <input id="si-indexnow" class="tin mono" bind:value={f.indexnowKey} placeholder="（没有可留空，之后在设置里生成）" spellcheck="false" />
          {#if d.indexnow_key}<small>在站点文件里找到了验证文件 {d.indexnow_key}.txt</small>{/if}
        </div>
        {#each d.notes as n (n)}<p class="hint">{n}</p>{/each}
      {/if}
      <div class="err-note slot" class:on={!!f.err} role="alert">{f.err}</div>
    </div>
    {#if f.step === 'batch'}
      <footer class="sheet-foot">
        <button class="btn small ghost" onclick={() => { f.step = 'source'; f.batch = []; }} disabled={f.busy}>上一步</button>
        <button class="btn small primary" onclick={confirmBatchImport} disabled={f.busy || detecting > 0 || !picked}>{f.busy ? '导入中…' : detecting ? '识别中…' : `导入 ${picked} 个站点`}</button>
      </footer>
    {/if}
    {#if f.step === 'review'}
      <footer class="sheet-foot">
        <button class="btn small ghost" onclick={() => (f.step = 'source')} disabled={f.busy}>上一步</button>
        <button class="btn small primary" onclick={confirmImport} disabled={f.busy || !f.name.trim()}>{f.busy ? '导入中…' : '导入站点'}</button>
      </footer>
    {/if}
  </div>
{/if}

<style>
  .opt { font-weight: 400; color: var(--faint); font-size: 11px; margin-left: 4px; }
  .repo-list { max-height: 240px; overflow: auto; border: 1px solid var(--border); border-radius: 10px; margin-top: -4px; }
  .repo-row { display: flex; align-items: center; gap: 8px; padding: 7px 12px; border-top: 1px solid var(--border); cursor: pointer; font-size: 12.5px; }
  .repo-row:first-child { border-top: none; }
  .repo-row:hover { background: var(--su-hover); }
  .repo-row.on { background: var(--su-soft, #f6f6f4); }
  .repo-row .rn { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .repo-row small { color: var(--faint); font-size: 11.5px; flex: none; }
  .repo-row .tag { flex: none; font-size: 10.5px; padding: 1px 6px; border-radius: 999px; background: var(--su-soft, #f1f0ec); color: var(--dim); }
  .empty { margin: 0; padding: 14px 12px; font-size: 12px; color: var(--faint); }
  .batch { border: 1px solid var(--border); border-radius: 10px; max-height: 420px; overflow: auto; }
  .b-row { display: flex; align-items: flex-start; gap: 10px; padding: 10px 12px; border-top: 1px solid var(--border); }
  .b-row:first-child { border-top: none; }
  .b-row > input[type='checkbox'] { margin-top: 8px; flex: none; }
  .b-row.off .b-main { opacity: .6; }
  .b-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .b-name { height: 28px; font-size: 13px; font-weight: 600; max-width: 320px; }
  .b-key { font-size: 13px; line-height: 28px; }
  .b-meta { font-size: 12px; color: var(--dim); }
  .b-path { font-size: 11px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .b-err { font-size: 12px; color: var(--err, #c0392b); }
  .b-note { font-size: 12px; color: var(--faint); }
  .ok-text { color: var(--ok); }
  .b-st { flex: none; font-size: 12px; line-height: 28px; display: inline-flex; align-items: center; gap: 4px; }
</style>
