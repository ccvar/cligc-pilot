<script lang="ts">
  // 站点概览里的「在线情况」：首页每小时打开一次（最近 7 天的可用率、响应时间、每次检查一格），
  // HTTPS 证书每天看一次、域名每周查一次到期日，快到期标黄 / 红。
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { say } from '$lib/state/app.svelte.js';
  import { ago, full } from './fmt';
  import type { Site, MonitorView } from '$lib/types';

  let { site }: { site: Site } = $props();
  let m = $state<MonitorView | null>(null);
  let busy = $state(false);

  async function load(id: string) {
    try { m = await invoke<MonitorView>('site_monitor', { id }); } catch { m = null; }
  }
  $effect(() => { void load(site.id); });
  async function check() {
    if (busy) return;
    busy = true;
    try { m = await invoke<MonitorView>('check_site_monitor', { id: site.id }); say(m.down ? '首页还是打不开' : m.last?.ok ? '首页、证书、域名都查过了' : '首页这次没打开，下次再看'); }
    catch (e) { say(String(e), 'err'); }
    finally { busy = false; }
  }

  const now = () => Date.now() / 1000;
  const days = (t: number) => Math.floor((t - now()) / 86400);
  const date = (t: number) => { const d = new Date(t * 1000); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`; };
  function expiry(t: number, err: string, warn: number) {
    if (err) return { t: err, c: 'err' };
    if (!t) return { t: '还没查', c: 'faint' };
    const d = days(t);
    return { t: d < 0 ? `${date(t)}（已过期）` : `${date(t)}（还剩 ${d} 天）`, c: d <= 3 ? 'err' : d <= warn ? 'warn' : '' };
  }
  const cert = $derived(m ? expiry(m.cert_expires, m.cert_err, 14) : null);
  const dom = $derived(m ? expiry(m.domain_expires, m.domain_err, 30) : null);
</script>

<div class="card mon">
  <h3><Icon name="activity" />在线情况
    <span class="sub">{#if !m?.checks}每小时自动打开一次首页{:else if m.down}<span class="err">打不开</span> · 从 {ago(m.down_since)}开始{:else}<span class="ok">正常</span>{#if m.last} · <span data-tip={full(m.last.ts)}>{ago(m.last.ts)}检查</span>{/if}{/if}</span>
    <span class="grow"></span>
    <button class="btn tiny" onclick={check} disabled={busy} data-tip="现在就打开首页、看证书、查域名到期">{#if busy}<span class="spin"></span>检查中…{:else}<Icon name="refresh" size={11} />立即检查{/if}</button>
  </h3>
  {#if m}
    <dl class="kv">
      <dt>近 7 天</dt>
      <dd>{#if m.uptime < 0}<span class="faint">还没检查过</span>{:else}可用 <b class:err={m.uptime < 0.99}>{(m.uptime * 100).toFixed(m.uptime === 1 ? 0 : 1)}%</b>{#if m.avg_ms} · 平均 {m.avg_ms} 毫秒打开{/if} · 查了 {m.checks} 次{#if m.down && m.last?.err}<div class="err">{m.last.err}</div>{/if}{/if}</dd>
      <dt>HTTPS 证书</dt><dd class={cert?.c} data-tip={m.cert_checked ? `${ago(m.cert_checked)}查的；14 天内到期会提醒` : null}>{cert?.t}</dd>
      <dt>域名到期</dt><dd class={dom?.c} data-tip={m.domain_checked ? `${ago(m.domain_checked)}查的（公开注册信息 RDAP）；30 天内到期会提醒` : null}>{dom?.t}</dd>
    </dl>
  {/if}
</div>

<style>
  .mon .sub { font-size: 12px; color: var(--faint); font-weight: 400; }
  .ok { color: var(--ok); }
  .err { color: var(--err); }
  .warn { color: var(--warn); }
  .faint { color: var(--faint); }
</style>
