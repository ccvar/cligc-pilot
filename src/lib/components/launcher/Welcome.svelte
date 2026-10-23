<script lang="ts">
  import AppIcon from '$lib/AppIcon.svelte';
  import WorkspaceMark from '$lib/components/icons/WorkspaceMark.svelte';
  import CfMark from '$lib/components/icons/CfMark.svelte';
  import SshMark from '$lib/components/icons/SshMark.svelte';
  import CliGuide from '$lib/components/launcher/CliGuide.svelte';
  import { startDrag } from '$lib/state/ui.svelte.js';
  import { openLocalWorkspace } from '$lib/state/connections.svelte.js';
  import { openCfConnect } from '$lib/state/cloudflare.svelte.js';
  import { openSshConnect } from '$lib/state/ssh.svelte.js';
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="center" data-tauri-drag-region onmousedown={startDrag}>
  <div class="hero-card">
    <div class="hero-mark"><AppIcon size={54} /></div>
    <h1>开始之前，先选一个工作方式</h1>
    <p>直接和本地 Claude / Codex / Grok 或 API 模型对话；连接 Cloudflare 让它帮你建站并部署；或者连上一台远程服务器，让它帮你运维。</p>
    <div class="hero-btns">
      <button class="btn soft hero-import" onclick={openLocalWorkspace}><WorkspaceMark size={15} />本地对话</button>
      <button class="btn soft hero-import" onclick={openCfConnect}><CfMark size={15} />连接 Cloudflare</button>
      <button class="btn soft hero-import" onclick={openSshConnect}><SshMark size={15} />新建远程连接</button>
    </div>
    <CliGuide />
  </div>
</div>

<style>
  /* safe center：内容比可视区高时退回顶对齐可滚动，避免居中把顶部裁掉。 */
  .center { flex: 1; display: flex; align-items: safe center; justify-content: center; overflow-y: auto; padding: 24px; }
  .hero-card { text-align: center; max-width: 460px; }
  .hero-mark { display: flex; justify-content: center; margin-bottom: 6px; }
  .hero-card h1 { font-size: 22px; margin: 12px 0 8px; }
  .hero-card p { color: var(--dim); margin: 0 0 18px; }
  /* ---- 按钮 / 弹窗 ---- */
  /* 动作按钮统一尺度：13px 字 + 6px 14px 内边距，高约 30px（与输入框/下拉同高，主次同高）。 */
  .btn { background: #fff; color: var(--text); border: 1.5px solid var(--border2); border-radius: 8px; padding: 6px 14px; cursor: pointer; font-size: 13px; line-height: 1.25; }
  .btn:hover { background: #f6f5f1; }
  .btn:disabled { opacity: .5; cursor: default; }
  .btn.soft { display: inline-flex; align-items: center; gap: 6px; padding: 3px 12px; background: #fff; color: var(--text); border: 1px solid var(--border2); border-radius: 9px; font-weight: 500; }
  .btn.soft:hover { background: var(--rail); border-color: #cfccc2; }
  .btn.soft :global(.plus-ic) { color: var(--dim); }
  .hero-btns { display: flex; gap: 10px; flex-wrap: wrap; justify-content: center; }
</style>
