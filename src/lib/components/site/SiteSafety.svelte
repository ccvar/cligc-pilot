<script lang="ts">
  // 站点健康里的「安全标记」：有没有被 Google（浏览器红色警告页，俗称飘红）、Cloudflare（钓鱼警告页、安全 DNS）、Quad9 标记为危险网站。
  // 每天自动查一次（safety.rs）。被标记时写清楚访客会看到什么、最近上线了哪些改动（先查这些）、去哪里申诉；这期间自动上线暂停。
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { say } from '$lib/state/app.svelte.js';
  import { openSite } from '$lib/state/sites.svelte.js';
  import { ago, full } from './fmt';
  import type { Site, HealthSum, SafetyView, SafetyCheck, OpsItem } from '$lib/types';

  let { site, embedded = false, onsummary }: { site: Site; embedded?: boolean; onsummary?: (s: HealthSum) => void } = $props();
  let v = $state<SafetyView | null>(null);
  let busy = $state(false);
  let recent = $state<OpsItem[]>([]);

  async function load(id: string) {
    try { v = await invoke<SafetyView>('site_safety', { id }); } catch { v = null; }
  }
  $effect(() => { void load(site.id); });
  export async function check(quiet = false) {
    if (busy) return;
    busy = true;
    try {
      v = await invoke<SafetyView>('check_site_safety', { id: site.id });
      const bad = v.checks.filter((c) => c.state === 'bad');
      if (!quiet || bad.length) say(bad.length ? `被 ${bad.map((c) => LABEL[c.key]).join('、')} 标记为危险网站` : '查过了，没有被标记', bad.length ? 'err' : undefined);
    } catch (e) { say(String(e), 'err'); }
    finally { busy = false; }
  }

  const LABEL: Record<SafetyCheck['key'], string> = { google: 'Google 安全浏览', cf_page: 'Cloudflare 钓鱼警告', cf_dns: 'Cloudflare 安全 DNS', quad9: 'Quad9' };
  const WHAT: Record<SafetyCheck['key'], string> = {
    google: 'Chrome、Safari、Firefox 打开前的红色警告页',
    cf_page: '走 Cloudflare 代理的站，访客先看到的钓鱼警告页',
    cf_dns: '1.1.1.2 / 1.1.1.3，很多家庭和公司用它挡恶意网站',
    quad9: '9.9.9.9，不少路由器和公司用它挡恶意网站',
  };
  const ORDER: SafetyCheck['key'][] = ['google', 'cf_page', 'cf_dns', 'quad9'];
  const domain = $derived(site.domain.trim().replace(/\/+$/, '').toLowerCase());
  const gscRes = $derived(site.data?.gsc_site || `sc-domain:${domain}`);
  /** 每家的查看 / 申诉入口：被标记时是「去申诉」，平时是「查看」 */
  function link(k: SafetyCheck['key'], bad: boolean): { l: string; u: string; tip: string } {
    if (k === 'google') return bad
      ? { l: '去 Search Console 申请复查', u: `https://search.google.com/search-console/security-issues?resource_id=${encodeURIComponent(gscRes)}`, tip: '修好后在 Search Console 的「安全问题」里点「申请审核」，一般几天内处理' }
      : { l: '查看', u: `https://transparencyreport.google.com/safe-browsing/search?url=${domain}`, tip: '在 Google 透明度报告里查这个站' };
    if (k === 'cf_page') return bad
      ? { l: '去 Cloudflare 申诉', u: 'https://dash.cloudflare.com/', tip: '在 Cloudflare 后台申诉，或直接回复 Cloudflare 发给你的那封通知邮件' }
      : { l: '', u: '', tip: '' };
    if (k === 'cf_dns') return { l: bad ? '去 Cloudflare Radar 提交反馈' : '查看', u: `https://radar.cloudflare.com/domains/domain/${domain}`, tip: 'Cloudflare Radar 上这个域名的分类；分错了可以在页面上提交反馈' };
    return { l: bad ? '查看并申诉' : '查看', u: `https://www.quad9.net/result/?url=${domain}`, tip: 'Quad9 上这个域名的拦截结果；误拦可以按页面上的说明申诉' };
  }
  const rows = $derived(ORDER.map((k) => v?.checks.find((c) => c.key === k) ?? { key: k, state: 'unknown' as const, note: '还没查' }));
  const bad = $derived(rows.filter((c) => c.state === 'bad'));
  const unknown = $derived(rows.filter((c) => c.state === 'unknown'));
  const STATE: Record<SafetyCheck['state'], string> = { ok: '没被标记', warn: '有提醒', bad: '被标记', unknown: '没查到' };

  // 被标记了：读出这几天上线的改动，先查这些（AI 加进去的链接、下载、脚本最可疑）
  $effect(() => {
    const since = v?.flagged_since ?? 0;
    if (!since) { recent = []; return; }
    invoke<OpsItem[]>('site_queue', { id: site.id })
      .then((q) => (recent = q.filter((i) => i.status === 'deployed' && i.deployed_at >= since - 7 * 86400).sort((a, b) => b.deployed_at - a.deployed_at).slice(0, 6)))
      .catch(() => (recent = []));
  });

  $effect(() => {
    if (!v) return;
    const warn = rows.some((c) => c.state === 'warn');
    onsummary?.({
      tone: !v.checked ? 'none' : bad.length ? 'err' : warn ? 'warn' : 'ok',
      status: !v.checked ? '还没检查' : bad.length ? '被标记为危险' : warn ? '有提醒' : '没被标记',
      sub: bad.length ? bad.map((c) => LABEL[c.key]).join('、') : !v.checked ? 'Google、Cloudflare、Quad9' : unknown.length ? `${rows.length - unknown.length} 家没标记，${unknown.length} 家没查到` : `${rows.length} 家都没标记`,
      checked: v.checked,
    });
  });
  const day = (t: number) => { const d = new Date(t * 1000); return `${d.getMonth() + 1}月${d.getDate()}日`; };
</script>

<div class="sf" class:card={!embedded}>
  {#if embedded}<div class="ph">{@render head()}</div>
  {:else}<h3><Icon name="flag" />安全标记{@render head()}</h3>{/if}
  {#if bad.length}
    <div class="alert">
      <b>访客现在会看到警告页或打不开这个站。</b>这个站的自动上线已经暂停，标记解除后自动恢复。
      <ol>
        <li>先找原因：{#if recent.length}被标记前后上线了这些改动，先看里面有没有可疑的外链、下载或脚本：
          <ul class="rc">{#each recent as i (i.id)}<li><span class="faint">{day(i.deployed_at)}</span> {i.opp.title}</li>{/each}</ul>
          <button class="lnk" onclick={() => openSite(site.id, 'queue', 'live')}>去「待处理 → 已上线」看详情 / 回滚</button>
        {:else}看看最近有没有加外链（尤其是推广链接指向的网站）、可下载的文件、来路不明的脚本；服务器被入侵也会这样。{/if}</li>
        <li>修好后，点下面每一家后面的链接申请复查（Google 一般几天，其他家时间不定）。</li>
      </ol>
    </div>
  {/if}
  <ul class="src">
    {#each rows as c (c.key)}
      {@const lk = link(c.key, c.state === 'bad')}
      <li>
        <span class="nm" data-tip={WHAT[c.key]}>{LABEL[c.key]}</span>
        <span class="st {c.state}">{STATE[c.state]}</span>
        <span class="note">{c.note}</span>
        {#if lk.u}<button class="lnk" onclick={() => openUrl(lk.u)} data-tip={lk.tip}>{lk.l} ↗</button>{/if}
      </li>
    {/each}
  </ul>
  <p class="foot">每天自动查一次，不用密钥。Search Console 里的安全问题和人工处置、微软 SmartScreen 没有公开接口，查不到——
    <button class="lnk" onclick={() => openUrl(`https://search.google.com/search-console/security-issues?resource_id=${encodeURIComponent(gscRes)}`)}>去 Search Console 看安全问题 ↗</button></p>
</div>

{#snippet head()}
  <span class="sub">{#if !v?.checked}每天自动查一次{:else if bad.length}<span class="err">被标记</span>{#if v.flagged_since} · 从 {ago(v.flagged_since)}开始{/if}{:else}<span class="ok">没被标记</span> · <span data-tip={full(v.checked)}>{ago(v.checked)}检查</span>{#if unknown.length}<span class="faint">（{unknown.map((c) => LABEL[c.key]).join('、')} 这次没查到）</span>{/if}{/if}</span>
  <span class="grow"></span>
  <button class="btn tiny" onclick={() => check()} disabled={busy} data-tip="现在就查 Google、Cloudflare、Quad9 有没有标记这个站">{#if busy}<span class="spin"></span>检查中…{:else}<Icon name="refresh" size={11} />立即检查{/if}</button>
{/snippet}

<style>
  .ph { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; min-height: 26px; }
  .ph .sub, .sf .sub { font-size: 12px; color: var(--faint); font-weight: 400; }
  .ph .grow { flex: 1; }
  .alert { margin: 0 0 12px; padding: 10px 12px; border-radius: 8px; background: color-mix(in srgb, var(--err) 8%, transparent); border: 1px solid color-mix(in srgb, var(--err) 25%, transparent); font-size: 12.5px; line-height: 1.6; }
  .alert b { color: var(--err); }
  .alert ol { margin: 6px 0 0; padding-left: 18px; }
  .rc { margin: 4px 0; padding-left: 16px; }
  .src { list-style: none; margin: 0; padding: 0; }
  .src li { display: grid; grid-template-columns: 150px 64px minmax(0, 1fr) auto; align-items: baseline; gap: 10px; padding: 7px 0; border-top: 1px solid var(--border); font-size: 12.5px; }
  .src li:first-child { border-top: none; }
  .nm { color: var(--dim); }
  .st { font-weight: 600; }
  .st.ok { color: var(--ok); }
  .st.warn { color: var(--warn); }
  .st.bad { color: var(--err); }
  .st.unknown { color: var(--faint); font-weight: 500; }
  .note { color: var(--dim); min-width: 0; }
  .lnk { border: 0; background: none; padding: 0; font: inherit; font-size: 12px; color: var(--accent, var(--text)); cursor: pointer; white-space: nowrap; }
  .lnk:hover { text-decoration: underline; }
  .foot { margin: 10px 0 0; font-size: 11.5px; color: var(--faint); line-height: 1.6; }
  .ok { color: var(--ok); }
  .err { color: var(--err); }
  .faint { color: var(--faint); }
</style>
