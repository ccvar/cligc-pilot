<script lang="ts">
  // 迁移已有站点：填域名和存放位置。建好后自动盘点旧站，进度和后面几步都在站点概览的「迁移进度」卡片里。
  import './site.css';
  import Icon from './Icon.svelte';
  import { migrate, closeMigrate, createMigration, createMigrations, pickMigrateParent } from '$lib/state/migrate.svelte.js';
  import { folderName } from '$lib/state/siteCreate.svelte.js';

  const d = $derived(migrate.dialog);
  const sep = $derived(d.parent.includes('\\') && !d.parent.includes('/') ? '\\' : '/');
  const target = $derived(d.parent && d.domain.trim() && !d.multi ? `${d.parent.replace(/[\\/]+$/, '')}${sep}${folderName(d.domain, '')}` : '');
  const count = $derived(d.domains.split(/[\s,，;；]+/).filter((x) => x.includes('.')).length);
  const ready = $derived(!!d.parent && (d.multi ? count > 0 : !!d.domain.trim()));
</script>

<svelte:window onkeydown={(e) => { if (d.open && e.key === 'Escape') closeMigrate(); }} />
{#if d.open}
  <div class="su-mask" role="presentation" onclick={closeMigrate}></div>
  <div class="su modal mgd" role="dialog" aria-modal="true" aria-label="迁移已有站点">
    <header class="sheet-head">
      <b><Icon name="route" size={15} /> 迁移已有站点</b>
      <button class="x" aria-label="关闭" data-tip="关闭" onclick={closeMigrate} disabled={d.busy}>×</button>
    </header>
    <div class="sheet-body">
      <p class="hint">把线上的站点搬进 Pilot 重做：原来的 URL 和正文都保留，换成新的视觉，部署到 Cloudflare 或你自己的服务器。搬完以后由托管运营接着更新内容。</p>
      <ol class="flow">
        <li><b>盘点</b>找全旧站的页面，判断适合放在哪</li>
        <li><b>确认方案</b>选放在哪、新站的样子</li>
        <li><b>抓取</b>把文字、图片和附件搬下来，地址照旧</li>
        <li><b>重做</b>先挑一个设计方向，AI 照着重做全站，文字一字不改</li>
        <li><b>核对</b>每个旧地址在新站上实际打开一遍，有问题 AI 自己修</li>
        <li><b>正式上线</b>确认是你的站，让域名打开新站，随时能退回</li>
        <li><b>上线后观察</b>盯 4 周：旧地址、搜索流量</li>
      </ol>
      <p class="hint">确认方案以后，抓取、重做、核对都会自动接着做，只在需要你挑设计方向和正式上线时停下来。</p>
      <div class="seg" role="tablist">
        <button class:on={!d.multi} onclick={() => (d.multi = false)} disabled={d.busy}>一个站点</button>
        <button class:on={d.multi} onclick={() => (d.multi = true)} disabled={d.busy}>批量迁移</button>
      </div>
      {#if !d.multi}
        <div class="field"><label for="mg-domain">旧站域名</label>
          <input id="mg-domain" class="tin" bind:value={d.domain} placeholder="example.com" spellcheck="false" autocapitalize="off" onkeydown={(e) => { if (e.key === 'Enter' && ready) void createMigration(); }} />
          <small>只看公开的页面，慢慢抓、不给旧站添负担。正式上线前会确认这个站是你的。</small>
        </div>
      {:else}
        <div class="field"><label for="mg-domains">旧站域名 <small class="opt">每行一个，最多 50 个</small></label>
          <textarea id="mg-domains" class="tin plain" rows="6" bind:value={d.domains} placeholder={'example.com\nblog.example.org\nshop.example.net'} spellcheck="false"></textarea>
          <small>每个站建一个文件夹，排队盘点（同时最多 2 个在抓）。盘点好以后可以逐个确认方案，也可以「按默认方案确认」，再打开「自动推进」。正式上线始终一个一个确认。</small>
        </div>
        {#if d.results.some((r) => r.error)}
          <ul class="res">{#each d.results as r (r.domain)}<li class:bad={!!r.error}><b>{r.domain}</b>{r.error ? `：${r.error}` : '：已建好，排队盘点'}</li>{/each}</ul>
        {/if}
      {/if}
      <div class="field"><span class="lbl">存放位置</span>
        <div class="row">
          <button class="btn small" onclick={pickMigrateParent} disabled={d.busy}>选择文件夹…</button>
          <span class="mono path" title={target}>{target || (d.parent ? d.parent : '还没选')}</span>
        </div>
        <small>会在这里新建一个文件夹放新站的代码，只在你的电脑上。</small>
      </div>
      {#if d.err}<div class="err-note">{d.err}</div>{/if}
    </div>
    <footer class="sheet-foot">
      <span class="grow"></span>
      <button class="btn small ghost" onclick={closeMigrate} disabled={d.busy}>取消</button>
      <button class="btn small primary" onclick={() => (d.multi ? createMigrations() : createMigration())} disabled={d.busy || !ready}>{#if d.busy}<span class="spin"></span>{d.multi ? '正在逐个连接…' : '正在连接旧站…'}{:else}{d.multi ? `开始盘点${count ? ` ${count} 个站` : ''}` : '开始盘点'}{/if}</button>
    </footer>
  </div>
{/if}

<style>
  .mgd { width: min(560px, calc(100vw - 32px)); }
  .flow { list-style: none; margin: 4px 0 6px; padding: 10px 12px; border-radius: 10px; background: var(--rail); display: flex; flex-direction: column; gap: 5px; counter-reset: f; }
  .flow li { counter-increment: f; display: flex; gap: 8px; font-size: 12px; color: var(--dim); line-height: 1.5; }
  .flow li::before { content: counter(f); flex: none; width: 16px; height: 16px; margin-top: 1px; border-radius: 50%; background: #fff; box-shadow: inset 0 0 0 1px var(--border2); font-size: 10px; font-weight: 600; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .flow b { flex: none; font-weight: 600; color: var(--text); width: 66px; white-space: nowrap; }
  .path { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: var(--dim); }
  .grow { flex: 1; }
  .opt { font-weight: 400; color: var(--faint); font-size: 11px; margin-left: 4px; }
  .su textarea.plain { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12.5px; }
  .res { flex: none; list-style: none; margin: 0; padding: 8px 10px; border-radius: 8px; background: var(--rail); font-size: 12px; display: flex; flex-direction: column; gap: 3px; max-height: 160px; overflow-y: auto; }
  .res li.bad { color: var(--err); }
  .sheet-foot { display: flex; align-items: center; gap: 8px; }
</style>
