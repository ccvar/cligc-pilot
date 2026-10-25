<script lang="ts">
  // 设置里的「服务器」（Pilot 托管的 VPS：Caddy + Go + SQLite）：服务器状态、配置服务器、回退到上一个版本。
  import './site.css';
  import Icon from './Icon.svelte';
  import type { Site } from '$lib/types';
  import { invoke } from '@tauri-apps/api/core';
  import { ago, full } from './fmt';
  import { say, confirmDialog } from '$lib/state/app.svelte.js';
  import { sites } from '$lib/state/sites.svelte.js';

  interface Vps { slug: string; root: string; host: string; alias: string; ip: string; os: string; arch: string; api_port: number; https: boolean; setup_at: number; https_at: number; releases: string[]; current: string; notes: string[] }
  let { site }: { site: Site } = $props();
  let v = $state<Vps | null>(null);
  let busy = $state('');
  const job = $derived(sites.jobs[site.id]);
  const setting = $derived(job?.status === 'running' && job.kind === 'vps:setup');
  async function load() { try { v = await invoke<Vps | null>('site_vps_state', { id: site.id }); } catch { v = null; } }
  $effect(() => { void site.id; void load(); });
  let was = false;
  $effect(() => { const r = setting; if (was && !r) void load(); was = r; });
  async function setup() {
    busy = 'setup';
    try { await invoke('site_vps_setup', { id: site.id, sshConnId: site.deploy.ssh_conn_id }); }
    catch (e) { say(String(e), 'err'); }
    finally { busy = ''; }
  }
  const prev = $derived.by(() => { if (!v) return ''; const i = v.releases.indexOf(v.current); return i > 0 ? v.releases[i - 1] : ''; });
  const fmtRel = (r: string) => r.replace(/^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})$/, '$2-$3 $4:$5');
  async function rollback() {
    if (!prev) return;
    if (!await confirmDialog(`把线上切回 ${fmtRel(prev)} 发布的版本？当前版本保留，之后可以再切回来或重新上线。`, { title: '回退到上一个版本', kind: 'warning', confirmText: '回退' })) return;
    busy = 'rollback';
    try { const r = await invoke<string>('site_vps_activate', { id: site.id, release: prev }); say(`已切回 ${fmtRel(r)} 的版本`, 'ok'); await load(); }
    catch (e) { say(String(e), 'err'); }
    finally { busy = ''; }
  }
</script>

<div class="card">
  <h3><Icon name="server" />服务器<span class="sub">Caddy + Go + SQLite，Pilot 托管</span></h3>
  {#if v?.setup_at}
    <dl class="kv">
      <dt>地址</dt><dd>{v.ip || '—'} · {v.os}{v.arch ? ` · ${v.arch}` : ''}</dd>
      <dt>站点目录</dt><dd class="mono">{v.root}</dd>
      <dt>HTTPS</dt><dd>{v.https ? (v.https_at ? '已开启（Caddy 自动申请和续期）' : '已开启，证书申请中') : '还没开（切换域名后自动开启）'}</dd>
      <dt>表单和评论</dt><dd>{v.api_port ? `pilot-site 服务，端口 ${v.api_port}` : '没开'}</dd>
      <dt>当前版本</dt><dd>{v.current ? fmtRel(v.current) : '还没发布'}{v.releases.length > 1 ? ` · 服务器上留着 ${v.releases.length} 个版本` : ''}</dd>
      <dt>配置于</dt><dd><span data-tip={full(v.setup_at)}>{ago(v.setup_at)}</span></dd>
    </dl>
    {#each v.notes as n (n)}<p class="hint warn-text">{n}</p>{/each}
  {:else}
    <p class="hint">发布命令留空时，由 Pilot 自己配置服务器：装 Caddy（自动 HTTPS）、建站点目录；有表单和评论时再装一个 Go 写的小服务（数据存 SQLite）。需要这个 SSH 账号是 root 或者能免密 sudo；80 / 443 端口被别的网站服务占着时不会去动它。</p>
  {/if}
  <div class="row">
    <button class="btn small" class:primary={!v?.setup_at} onclick={setup} disabled={!!busy || setting || !site.deploy.ssh_conn_id}>{setting ? '配置中…' : v?.setup_at ? '重新配置服务器' : '配置服务器'}</button>
    {#if prev}<button class="btn small" onclick={rollback} disabled={!!busy}><Icon name="undo" size={12} />回退到上一个版本</button>{/if}
  </div>
</div>
