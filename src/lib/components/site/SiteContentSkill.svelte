<script lang="ts">
  // 站点设置里的「内容技能」：站点仓库自带的技能（skills/*/SKILL.md）说正文在线上数据库里时，
  // Pilot 识别出取文档 / 试运行 / 写入三条命令，你确认一次，运营任务就按它改（见后端 content_skill）。
  // 没有技能的站点不显示这张卡。
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { say, confirmDialog } from '$lib/state/app.svelte.js';
  import { ago } from './fmt';
  import type { ContentSkill, Site } from '$lib/types';

  let { site }: { site: Site } = $props();
  let cs = $state<ContentSkill | null>(null);
  let busy = $state(false);
  let loadedFor = '';

  $effect(() => {
    const id = site.id;
    if (id === loadedFor) return;
    loadedFor = id;
    cs = null;
    invoke<ContentSkill | null>('site_content_skill', { id }).then((v) => { if (site.id === id) cs = v; }).catch(() => {});
  });

  async function detect() {
    busy = true;
    try { cs = await invoke<ContentSkill | null>('site_content_skill_detect', { id: site.id }); say(cs ? '识别完了' : '仓库里没有内容技能'); }
    catch (e) { say(String(e), 'err'); }
    finally { busy = false; }
  }
  async function confirm(on: boolean) {
    if (!cs) return;
    if (on && !(await confirmDialog(`以后这个站点的运营任务按技能 ${cs.skill} 改正文：\n\n· Pilot 在你的站点文件夹里用「取文档」把要改的文章取出来，交给 AI 改；\n· 改好的文档在卡片上先「试运行」，你点「写入线上」才会写进线上数据库；\n· 只运行站点文件夹里本来就有的技能脚本，技能的密钥不交给 AI。\n\n三条命令在这张卡上，确认它们是对的再继续。`, { title: '按内容技能改正文', kind: 'info', confirmText: '确认使用' }))) return;
    busy = true;
    try { cs = await invoke<ContentSkill>('site_content_skill_confirm', { id: site.id, on }); say(on ? '确认了：运营任务按内容技能改正文' : '不再按内容技能改正文'); }
    catch (e) { say(String(e), 'err'); }
    finally { busy = false; }
  }
</script>

{#if site.source === 'local' && cs}
  <!-- 自己占满设置页网格的一整行（没有技能时什么都不渲染，不留空行） -->
  <div class="card csk">
    <h3><Icon name="file" />内容技能<span class="sub">{cs.skill}</span>
      <span class="grow"></span>
      {#if cs.detected_at && cs.external && !cs.error}<span class="pill" class:ok={cs.confirmed} class:warn={!cs.confirmed}>{cs.confirmed ? '在用' : '待你确认'}</span>{/if}
    </h3>
    {#if !cs.detected_at}
      <p class="hint">站点仓库里有这个技能，还没识别过：点「识别」让模型读一遍技能说明，看正文是不是在线上数据库里、要用哪几条命令改。之后每次体检会自动看技能有没有变。</p>
    {:else if cs.error}
      <p class="hint err">{cs.error}</p>
    {:else if !cs.external}
      <p class="hint">这个技能说正文就在仓库文件里：运营任务照常改文件。</p>
    {:else}
      <p class="hint">{cs.summary || '正文在线上数据库里，要用技能的命令读写。'}</p>
      <dl class="kv">
        <dt>取文档</dt><dd class="mono small">{cs.get}</dd>
        <dt>试运行</dt><dd class="mono small">{cs.check}</dd>
        <dt>写入线上</dt><dd class="mono small">{cs.apply}</dd>
      </dl>
      <p class="hint small">在你的站点文件夹里运行：<code>{'{path}'}</code> 是页面路径，<code>{'{file}'}</code> 是改好的文档。识别于 {ago(cs.detected_at)}。</p>
    {/if}
    <div class="row">
      {#if cs.external && !cs.error}
        {#if cs.confirmed}<button class="btn tiny" onclick={() => confirm(false)} disabled={busy}>停用</button>
        {:else}<button class="btn tiny primary" onclick={() => confirm(true)} disabled={busy}><Icon name="check" size={11} />确认使用</button>{/if}
      {/if}
      <button class="btn tiny" onclick={detect} disabled={busy} data-tip="技能改过了、或者识别得不对：让模型重新读一遍技能说明">{busy ? '识别中…' : cs.detected_at ? '重新识别' : '识别'}</button>
    </div>
  </div>
{/if}

<style>
  .csk { grid-column: 1 / -1; margin: 0; }
  .csk h3 { display: flex; align-items: center; gap: 6px; }
  .csk .sub { font-size: 12px; font-weight: 400; color: var(--faint); font-family: var(--mono, ui-monospace, monospace); }
  .csk .grow { flex: 1; }
  .csk .pill { font-size: 11px; font-weight: 500; padding: 1px 8px; border-radius: 999px; background: var(--su-hover); color: var(--dim); }
  .csk .pill.ok { background: #e7f6ec; color: #18794e; }
  .csk .pill.warn { background: #fdf3e3; color: #a35a00; }
  .csk .err { color: var(--err); }
  .csk .small { font-size: 12px; }
  .csk dd { word-break: break-all; }
  .csk .row { display: flex; gap: 6px; margin-top: 10px; }
</style>
