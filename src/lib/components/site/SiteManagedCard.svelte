<script lang="ts">
  // 概览上的「托管」卡：这个站点在哪个托管方案里、现在托不托管、阶段和定位；没托管就是自己运营，可以加入方案。
  import './site.css';
  import Icon from './Icon.svelte';
  import Dropdown from '$lib/Dropdown.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import type { Site, SiteStageView } from '$lib/types';
  import { say } from '$lib/state/app.svelte.js';
  import { refreshSites } from '$lib/state/sites.svelte.js';
  import { STAGE_LABEL, STAGE_FOCUS } from '$lib/state/ops.svelte.js';
  import { managed, loadManaged, openManaged, editGroup, newGroup, setManagedSites, setSiteActive, AUTONOMY_LABEL } from '$lib/state/managed.svelte.js';
  import { clock } from './fmt';
  import { jev, loadJev } from '$lib/state/typesafe.svelte.js';

  let { site }: { site: Site } = $props();
  $effect(() => { if (!managed.overview) void loadManaged(); });
  const groups = $derived(managed.overview?.groups ?? []);
  const g = $derived(groups.find((x) => x.sites.includes(site.id)));
  const active = $derived(!!g && !g.paused && !g.stopped.includes(site.id));

  // 阶段与定位（只有托管的站点用得上）
  let sv = $state<SiteStageView | null>(null);
  let posDraft = $state('');
  $effect(() => { void loadJev(); });
  let saving = $state(false);
  let editingPos = $state(false);
  async function loadStage(id: string) {
    try { const v = await invoke<SiteStageView>('site_stage_info', { id }); sv = v; posDraft = v.positioning.text; } catch { sv = null; }
  }
  const gid = $derived(g?.id ?? '');
  $effect(() => { if (gid) void loadStage(site.id); }); // 跟着方案 id，不跟着每次刷新都换的对象
  const stage = $derived(site.ops.stage || sv?.auto_stage || 'new');
  async function setStage(v: string) {
    try {
      await invoke('save_site', { site: { ...site, ops: { ...site.ops, stage: v } } });
      await refreshSites(); await loadStage(site.id);
    } catch (e) { say(String(e), 'err'); }
  }
  async function savePositioning(text: string) {
    saving = true;
    try {
      const p = await invoke<SiteStageView['positioning']>('save_site_positioning', { id: site.id, text });
      if (sv) sv.positioning = p;
      posDraft = p.text; editingPos = false;
      say(text.trim() ? '站点定位已保存，AI 之后按它来' : '已交还给 AI，下次制定计划时重新归纳');
    } catch (e) { say(String(e), 'err'); }
    finally { saving = false; }
  }

  let joining = $state(false);
  let joinTo = $state('');
  $effect(() => { if (joining && !joinTo) joinTo = groups[0]?.id ?? '__new'; });
  async function join() {
    if (joinTo === '__new') { joining = false; newGroup([site.id]); openManaged('groups'); return; }
    await setManagedSites([site.id], true, joinTo);
    joining = false;
  }
</script>

<div class="card mcard">
  <h3><Icon name="sparkles" />托管
    {#if g}
      <span class="st" class:on={active}>{g.paused ? '方案已暂停' : active ? '托管中' : '已停止'}</span>
    {:else}
      <span class="st">自己运营</span>
    {/if}
    <span class="grow"></span>
    {#if g}
      {#if !g.paused}<button class="btn tiny" onclick={() => setSiteActive(site.id, !active)}><Icon name={active ? 'pause' : 'play'} size={11} />{active ? '停止托管' : '恢复托管'}</button>{/if}
      <button class="btn tiny" onclick={() => { openManaged('groups'); editGroup(g); }}><Icon name="sliders" size={11} />方案设置</button>
    {:else if !joining}
      <button class="btn tiny" onclick={() => (joining = true)}><Icon name="plus" size={11} />加入托管方案</button>
    {/if}
  </h3>

  {#if g}
    <div class="row-plan">
      <span>方案 <b>「{g.name}」</b></span>
      <span class="pill line">{AUTONOMY_LABEL[g.autonomy] ?? g.autonomy}</span>
      <span class="faint">{g.autonomy === 'suggest' ? 'AI 每周出计划和建议，不改代码；你可以在「待处理」里挑一条让 AI 改。' : g.autonomy === 'review' ? 'AI 改好、跑完检查后等你确认再上线。' : 'AI 按方案规则改好并自动上线，上线后检查，变差自动回滚。'}</span>
    </div>
    <div class="grid">
      <div class="field"><span class="lbl">站点阶段</span>
        <Dropdown value={site.ops.stage ?? ''} onchange={setStage} options={[
          { value: '', label: `自动判断（当前：${STAGE_LABEL[sv?.auto_stage ?? 'new']}）`, sub: sv?.reason },
          { value: 'new', label: '新站', sub: '没有或很少搜索数据' },
          { value: 'growing', label: '成长期', sub: '有展现、点击还少' },
          { value: 'established', label: '成熟期', sub: '已有稳定流量' },
        ]} />
        <small>{STAGE_FOCUS[stage]}</small>
      </div>
      <div class="field">
        <div class="pos-head"><span class="lbl">站点定位</span>
          {#if sv?.positioning.text}<span class="faint by">{sv.positioning.by === 'user' ? '你改过 · AI 照用' : sv.positioning.updated ? `AI 归纳 · ${clock(sv.positioning.updated)}` : 'AI 归纳'}</span>{/if}
          <span class="grow"></span>
          {#if !editingPos}<button class="link-btn sm" onclick={() => { editingPos = true; posDraft = sv?.positioning.text ?? ''; }}>{sv?.positioning.text ? '修改' : '现在写'}</button>{/if}
          {#if sv?.positioning.by === 'user' && !editingPos}<button class="link-btn sm" onclick={() => savePositioning('')} disabled={saving}>交还给 AI</button>{/if}
        </div>
        {#if editingPos}
          <textarea class="tin plain" rows="4" bind:value={posDraft} placeholder="主题、目标读者、语言 / 地区、已有栏目和内容类型、商业目标"></textarea>
          <div class="row-end"><button class="btn small ghost" onclick={() => (editingPos = false)}>取消</button><button class="btn small primary" onclick={() => savePositioning(posDraft)} disabled={saving || !posDraft.trim()}>保存定位</button></div>
        {:else}
          <p class="pos">{sv?.positioning.text || '还没有。AI 下次制定计划时会先读站点归纳出来；选题不确定时，AI 从这里推导增量内容。'}</p>
          {#if jev.configured}<small class="faint">{sv?.positioning.text ? '运营机会按这段定位排相关度（判断模型），和它无关的搜索词排到后面。' : '写好定位以后，运营机会会按相关度排序（判断模型），和站点无关的搜索词排到后面。'}</small>{/if}
        {/if}
      </div>
    </div>
  {:else if joining}
    <div class="join">
      <span>加入</span>
      <div class="dd"><Dropdown compact bind:value={joinTo} options={[...groups.map((x) => ({ value: x.id, label: x.name, sub: `${AUTONOMY_LABEL[x.autonomy] ?? ''} · ${x.sites.length} 个站点` })), { value: '__new', label: '新建托管方案…' }]} /></div>
      <span class="grow"></span>
      <button class="btn small ghost" onclick={() => (joining = false)}>取消</button>
      <button class="btn small primary" onclick={join}>{joinTo === '__new' ? '去新建' : '加入'}</button>
    </div>
  {:else}
    <p class="hint">Pilot 照常同步数据、找运营机会、定期体检；改代码只在你让 AI 做的时候发生。想让 AI 按规则持续运营，就加入一个托管方案。</p>
  {/if}
</div>

<style>
  .st { font-size: 11.5px; font-weight: 500; color: var(--faint); }
  .st.on { color: var(--ok); }
  .row-plan { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 10px; font-size: 12.5px; margin: -2px 0 14px; }
  .row-plan b { font-weight: 600; }
  .row-plan .faint { font-size: 12px; }
  .grid { display: grid; grid-template-columns: minmax(220px, 300px) 1fr; gap: 14px 24px; }
  @container page (max-width: 700px) { .grid { grid-template-columns: 1fr; } }
  .field small { line-height: 1.6; }
  .pos-head { display: flex; align-items: baseline; gap: 10px; }
  .pos-head .lbl { font-size: 12px; font-weight: 550; }
  .pos-head .grow { flex: 1; }
  .by { font-size: 11.5px; }
  .link-btn.sm { font-size: 12px; }
  .pos { margin: 2px 0 0; font-size: 12.5px; color: var(--dim); line-height: 1.65; }
  .tin.plain { font-family: inherit; font-size: 13px; line-height: 1.6; }
  .join { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 12.5px; color: var(--dim); }
  .join .dd { min-width: 200px; }
  .join .grow { flex: 1; }
</style>
