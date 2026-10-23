<script lang="ts">
  import { cf, installWrangler } from '$lib/state/cloudflare.svelte.js';
  import { elapsedLabel } from '$lib/state/messages.svelte.js';
</script>

<div class="wr-note"><svg class="wr-ic" width="15" height="15" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.6" stroke="currentColor" stroke-width="1.3" /><path d="M8 4.8v4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /><circle cx="8" cy="11.1" r="0.6" fill="currentColor" /></svg><span>{cf.wrInstalling ? '正在从 npm 装 wrangler，首次约 1–2 分钟，别关窗…' : '还没装 wrangler，预览 / 部署要用'}</span><button class="wr-btn" onclick={installWrangler} disabled={cf.wrInstalling}>{#if cf.wrInstalling}<span class="wr-spin"></span>安装中 {elapsedLabel(cf.wrElapsed)}{:else}一键安装{/if}</button></div>

<style>
  .wr-note { display: flex; align-items: center; gap: 7px; width: fit-content; max-width: 100%; margin-top: 12px; font-size: 12px; color: #8a5a08; }
  .wr-note .wr-ic { flex: none; color: #d9a400; }
  .wr-note span { color: #8a5a08; }
  .wr-btn { flex: none; border: none; background: #d9a400; color: #fff; font-size: 11px; font-weight: 500; padding: 3px 9px; border-radius: 6px; cursor: pointer; }
  .wr-btn:hover { background: #c08f00; }
  .wr-btn:disabled { opacity: .85; cursor: default; }
  .wr-spin { display: inline-block; width: 10px; height: 10px; border: 1.6px solid #fff; border-right-color: transparent; border-radius: 50%; animation: rspin .7s linear infinite; margin-right: 5px; vertical-align: -1px; }
  @keyframes rspin { to { transform: rotate(360deg); } }
</style>
