<script lang="ts">
  // 迁移已有站点：填域名和存放位置。建好后自动盘点旧站，进度和后面几步都在站点概览的「迁移进度」卡片里。
  import './site.css';
  import Icon from './Icon.svelte';
  import { migrate, closeMigrate, createMigration, pickMigrateParent } from '$lib/state/migrate.svelte.js';
  import { folderName } from '$lib/state/siteCreate.svelte.js';

  const d = $derived(migrate.dialog);
  const sep = $derived(d.parent.includes('\\') && !d.parent.includes('/') ? '\\' : '/');
  const target = $derived(d.parent && d.domain.trim() ? `${d.parent.replace(/[\\/]+$/, '')}${sep}${folderName(d.domain, '')}` : '');
</script>

{#if d.open}
  <div class="su-mask" role="presentation" onclick={closeMigrate}></div>
  <div class="su modal mgd" role="dialog" aria-modal="true" aria-label="迁移已有站点">
    <header class="sheet-head">
      <b><Icon name="route" size={15} /> 迁移已有站点</b>
      <button class="x" onclick={closeMigrate} disabled={d.busy}>×</button>
    </header>
    <div class="sheet-body">
      <p class="hint">把一个线上的站点搬进 Pilot 重做：原来的 URL 和正文都保留，换成新的视觉，部署到 Cloudflare。搬完以后由托管运营接着更新内容。</p>
      <ol class="flow">
        <li><b>盘点</b>找全旧站的页面、判断能不能部署到 Cloudflare</li>
        <li><b>抓取</b>正文转成 Markdown，图片和附件放回原路径</li>
        <li><b>重做</b>AI 用 Astro 重做全站，URL 一个不变</li>
        <li><b>核对</b>每个旧地址在预览地址上实测，通过才能切换</li>
        <li><b>切换</b>确认是你的站后再把域名指过来，随时可以回退</li>
      </ol>
      <div class="field"><label for="mg-domain">旧站域名</label>
        <input id="mg-domain" class="tin" bind:value={d.domain} placeholder="example.com" spellcheck="false" autocapitalize="off" onkeydown={(e) => { if (e.key === 'Enter' && d.domain.trim() && d.parent) void createMigration(); }} />
        <small>只抓公开页面，限速、遵守 robots.txt。切换域名前会确认这个站是你的。</small>
      </div>
      <div class="field"><span class="lbl">存放位置</span>
        <div class="row">
          <button class="btn small" onclick={pickMigrateParent} disabled={d.busy}>选择文件夹…</button>
          <span class="mono path" title={target}>{target || (d.parent ? d.parent : '还没选')}</span>
        </div>
        <small>会在这里建一个站点文件夹并 git init；代码只在你本机。</small>
      </div>
      {#if d.err}<div class="err-note">{d.err}</div>{/if}
    </div>
    <footer class="sheet-foot">
      <span class="grow"></span>
      <button class="btn small ghost" onclick={closeMigrate} disabled={d.busy}>取消</button>
      <button class="btn small primary" onclick={createMigration} disabled={d.busy || !d.domain.trim() || !d.parent}>{#if d.busy}<span class="spin"></span>正在连接旧站…{:else}开始盘点{/if}</button>
    </footer>
  </div>
{/if}

<style>
  .mgd { width: min(560px, calc(100vw - 32px)); }
  .flow { list-style: none; margin: 4px 0 6px; padding: 10px 12px; border-radius: 10px; background: var(--rail); display: flex; flex-direction: column; gap: 5px; counter-reset: f; }
  .flow li { counter-increment: f; display: flex; gap: 8px; font-size: 12px; color: var(--dim); line-height: 1.5; }
  .flow li::before { content: counter(f); flex: none; width: 16px; height: 16px; margin-top: 1px; border-radius: 50%; background: #fff; box-shadow: inset 0 0 0 1px var(--border2); font-size: 10px; font-weight: 600; color: var(--dim); display: inline-flex; align-items: center; justify-content: center; }
  .flow b { flex: none; font-weight: 600; color: var(--text); width: 30px; }
  .path { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: var(--dim); }
  .grow { flex: 1; }
  .sheet-foot { display: flex; align-items: center; gap: 8px; }
</style>
