<script lang="ts">
  // 站点「数据」里的竞品观察：每周在 Google 搜一遍主要关键词（Search Console 展现最多的 10 个 + 你加的），
  // 看自己排第几、和上周比、前 3 名是谁、谁新冲进前 10；经常排在你前面的网站列成竞品。
  // 自己排 4–20、前 3 都是别人的词会变成「对比竞品补内容」机会。搜索结果来自 Serper（设置里填 Key）。
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Dropdown from '$lib/Dropdown.svelte';
  import { say } from '$lib/state/app.svelte.js';
  import { openConnectionSetup } from '$lib/state/settings.svelte.js';
  import { ago, full } from './fmt';
  import type { Site, SerpView, SerpKeyword } from '$lib/types';

  let { site }: { site: Site } = $props();
  let v = $state<SerpView | null>(null);
  let busy = $state(false);
  let editing = $state(false);
  let draft = $state({ off: false, region: '', words: '' });
  let showAll = $state(false);

  const REGIONS = [
    { value: '', label: '自动（按搜索词猜）' },
    { value: 'us|en', label: '美国 · 英文' },
    { value: 'gb|en', label: '英国 · 英文' },
    { value: 'hk|zh-cn', label: '香港 · 简体中文' },
    { value: 'tw|zh-tw', label: '台湾 · 繁体中文' },
    { value: 'sg|zh-cn', label: '新加坡 · 简体中文' },
    { value: 'jp|ja', label: '日本 · 日文' },
    { value: 'kr|ko', label: '韩国 · 韩文' },
  ];
  const regionLabel = (gl: string, hl: string) => REGIONS.find((r) => r.value === `${gl}|${hl}`)?.label ?? `${gl} · ${hl}`;

  $effect(() => { const id = site.id; editing = false; invoke<SerpView>('site_serp', { id }).then((r) => (v = r)).catch(() => (v = null)); });
  async function check() {
    if (busy) return;
    busy = true;
    try {
      v = await invoke<SerpView>('check_site_serp', { id: site.id });
      say(`查了 ${v.report.keywords.length} 个关键词`);
    } catch (e) { say(String(e), 'err'); }
    finally { busy = false; }
  }
  function edit() {
    if (!v) return;
    draft = { off: v.config.off, region: v.config.gl ? `${v.config.gl}|${v.config.hl}` : '', words: v.config.keywords.join('\n') };
    editing = true;
  }
  async function save() {
    const [gl, hl] = draft.region ? draft.region.split('|') : ['', ''];
    try {
      v = await invoke<SerpView>('set_site_serp', { id: site.id, config: { off: draft.off, gl, hl, keywords: draft.words.split(/\n|,|，/).map((w) => w.trim()).filter(Boolean) } });
      editing = false;
    } catch (e) { say(String(e), 'err'); }
  }
  const r = $derived(v?.report);
  const kws = $derived((r?.keywords ?? []).filter((k) => k.q));
  const shown = $derived(showAll ? kws : kws.slice(0, 10));
  function move(k: SerpKeyword) {
    if (!k.prev_checked) return null;
    if (k.ours == null && k.prev == null) return null;
    if (k.ours == null) return { t: '掉出前 10', c: 'down' };
    if (k.prev == null) return { t: '新进前 10', c: 'up' };
    const d = k.prev - k.ours;
    return d === 0 ? null : { t: d > 0 ? `↑${d}` : `↓${-d}`, c: d > 0 ? 'up' : 'down' };
  }
  const err = $derived(kws.length === 0 && (r?.keywords ?? []).find((k) => k.error)?.error);
</script>

{#if v}
  <div class="card serp">
    <h3><Icon name="eye" />竞品观察
      <span class="sub">{#if !v.has_key}要在设置里填 Serper 的 Key{:else if v.config.off}这个站点不查{:else if r?.checked_at}{regionLabel(r.gl, r.hl)} · <span data-tip={full(r.checked_at)}>{ago(r.checked_at)}查</span>{:else}每周在 Google 上查一次{/if}</span>
      <span class="grow"></span>
      {#if v.has_key}
        <button class="ic-btn ic-sm" onclick={() => (editing ? (editing = false) : edit())} aria-label="竞品观察设置" data-tip="地区、自己加的关键词"><Icon name="sliders" size={13} /></button>
        <button class="btn tiny" onclick={check} disabled={busy || v.config.off} data-tip="现在就在 Google 上搜一遍（每个词扣一次 Serper 额度）">{#if busy}<span class="spin"></span>查询中…{:else}<Icon name="refresh" size={11} />立即查{/if}</button>
      {/if}
    </h3>
    {#if !v.has_key}
      <p class="hint">每周在 Google 上搜一遍主要关键词：自己排第几、前面是谁、谁新冲进前 10。搜索结果来自 Serper（按次付费，1000 次约 1 美元），在设置里填它的 API Key 后开始。
        <button class="link-btn" onclick={() => openConnectionSetup()}>去设置</button></p>
    {:else}
      {#if editing}
        <div class="cfg">
          <label class="row"><span class="k">地区和语言</span><Dropdown compact menuCompact value={draft.region} options={REGIONS} ariaLabel="搜索的地区和语言" onchange={(x: string) => (draft.region = x)} /></label>
          <label class="row top"><span class="k">自己加的关键词</span><textarea class="tin" rows="3" placeholder="一行一个；和 Search Console 里展现最多的 10 个词一起查，合计最多 15 个" bind:value={draft.words}></textarea></label>
          <label class="row"><span class="k">这个站点不查</span><input type="checkbox" bind:checked={draft.off} /></label>
          <div class="acts"><button class="btn tiny ghost" onclick={() => (editing = false)}>取消</button><button class="btn tiny" onclick={save}>保存</button></div>
        </div>
      {/if}
      {#if err}
        <p class="hint err-t">{err}</p>
      {:else if !kws.length}
        <p class="hint">{v.planned.length ? `会查这些词：${v.planned.slice(0, 8).join('、')}${v.planned.length > 8 ? ` 等 ${v.planned.length} 个` : ''}。` : '还没有要查的词：等 Search Console 有了搜索词，或者点右上角的设置自己加几个。'}</p>
      {:else}
        {#if r && r.competitors.length}
          <div class="comp">
            <span class="faint">常排在你前面：</span>
            {#each r.competitors.filter((c) => c.above_us > 0).slice(0, 6) as c (c.domain)}
              <span class="pill" data-tip={`在 ${c.keywords} 个词的前 10 里，${c.above_us} 次排在你前面，平均第 ${c.avg_pos.toFixed(1)} 名`}>{c.domain} · {c.above_us}</span>
            {/each}
          </div>
        {/if}
        <table class="tbl kw">
          <thead><tr><th class="c-q">关键词</th><th class="num c-pos">你的名次</th><th>前 3 名</th></tr></thead>
          <tbody>
            {#each shown as k (k.q)}
              {@const mv = move(k)}
              <tr>
                <td class="trunc" title={k.q}>{k.q}{#if k.manual}<span class="faint mine" data-tip="你自己加的词">·自加</span>{/if}</td>
                <td class="num">{#if k.error}<span class="faint" data-tip={k.error}>没查到</span>{:else if k.ours != null}<button class="lk" onclick={() => openUrl(k.our_url)} data-tip={k.our_url}>第 {k.ours}</button>{:else}<span class="faint">前 10 外</span>{/if}{#if mv}<small class="mv {mv.c}">{mv.t}</small>{/if}</td>
                <td class="top">{#each k.top.slice(0, 3) as h (h.link)}<button class="dm" class:me={k.ours === h.pos} class:new={k.new_domains.includes(h.domain)} onclick={() => openUrl(h.link)} data-tip={`第 ${h.pos}：${h.title}${k.new_domains.includes(h.domain) ? '\n这周新进前 10' : ''}`}>{h.domain}</button>{/each}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if kws.length > 10}<button class="link-btn more" onclick={() => (showAll = !showAll)}>{showAll ? '收起' : `还有 ${kws.length - 10} 个词`}</button>{/if}
      {/if}
    {/if}
  </div>
{/if}

<style>
  .serp .sub { font-size: 12px; color: var(--faint); font-weight: 400; }
  .err-t { color: var(--err); }
  .comp { display: flex; flex-wrap: wrap; align-items: center; gap: 5px; margin-bottom: 10px; font-size: 12px; }
  /* 固定列宽：关键词、名次定宽，剩下的都给「前 3 名」（域名多了省略） */
  .kw { table-layout: fixed; width: 100%; }
  .kw .c-q { width: 34%; }
  .kw .c-pos { width: 150px; }
  .mine { font-size: 11px; margin-left: 4px; }
  .lk { border: 0; background: none; padding: 0; font: inherit; color: var(--text); cursor: pointer; }
  .lk:hover { text-decoration: underline; text-underline-offset: 3px; }
  .mv { font-size: 10.5px; margin-left: 4px; }
  .mv.up { color: var(--ok); }
  .mv.down { color: var(--err); }
  .top { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .dm { border: 0; background: none; padding: 0 8px 0 0; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; }
  .dm:hover { color: var(--text); text-decoration: underline; text-underline-offset: 3px; }
  .dm.me { color: var(--ok); font-weight: 600; }
  .dm.new::after { content: '新'; margin-left: 3px; font-size: 10px; color: var(--warn); }
  .more { display: block; margin: 8px auto 0; font-size: 12px; }
  .cfg { display: flex; flex-direction: column; gap: 8px; padding: 10px 12px; margin-bottom: 12px; border: 1px solid var(--border); border-radius: 10px; }
  .cfg .row { display: flex; align-items: center; gap: 12px; font-size: 12.5px; }
  .cfg .row.top { align-items: flex-start; }
  .cfg .k { width: 96px; flex: none; color: var(--dim); }
  .cfg textarea { flex: 1; font-size: 12.5px; resize: vertical; }
  .cfg input[type='checkbox'] { margin: 0; accent-color: var(--accent); }
  .cfg .acts { display: flex; justify-content: flex-end; gap: 6px; }
</style>
