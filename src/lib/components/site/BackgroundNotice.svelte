<script lang="ts">
  // 「后台停了 N 小时」提醒：最近 24 小时里有一段停了 1 小时以上（客户端没开 / 电脑睡着了）就显示，关掉后同一段不再提醒。
  // 站点列表、托管运营页顶上共用；顺手给出对应的解决办法（开机自动启动 / 不让电脑睡眠）。
  import Icon from './Icon.svelte';
  import { bg, setBackground, dismissGapNotice, openBackground, span, gapWhy, gapTime } from '$lib/state/background.svelte.js';

  const st = $derived(bg.status);
  const g = $derived(st?.notice ?? null);
</script>

{#if st && g}
  <div class="bgn" role="status">
    <Icon name="clock" size={14} />
    <span class="t">后台停了 <b>{span(g.to - g.from)}</b>（{gapTime(g.from)} – {gapTime(g.to)}，{gapWhy(g.kind)}），这段时间到期的任务正在一个个补跑。</span>
    <span class="grow"></span>
    {#if g.kind === 'closed' && !st.launch_at_login}
      <button class="btn tiny" onclick={() => setBackground({ launch_at_login: true })} data-tip="开机后留在菜单栏，不弹窗口，后台照常运营">开机自动启动</button>
    {:else if g.kind === 'asleep' && st.can_keep_awake && !st.keep_awake}
      <button class="btn tiny" onclick={() => setBackground({ keep_awake: true })} data-tip="托管期间不让电脑放着不动时自动睡眠（屏幕照常会关）">不让电脑自动睡眠</button>
    {/if}
    <button class="btn tiny ghost" onclick={openBackground}>后台运行设置</button>
    <button class="ic-btn ic-sm" onclick={dismissGapNotice} aria-label="知道了" data-tip="知道了，这段不再提醒"><Icon name="x" size={13} /></button>
  </div>
{/if}

<style>
  .bgn { display: flex; align-items: center; gap: 8px; margin-bottom: 12px; padding: 8px 10px 8px 14px; border: 1px solid color-mix(in srgb, var(--warn) 35%, var(--border)); border-radius: 10px; background: color-mix(in srgb, var(--warn) 7%, var(--bg)); color: var(--warn); }
  .t { min-width: 0; font-size: 12.5px; color: var(--text); }
  .t b { font-weight: 600; }
  .grow { flex: 1; }
  .btn, .ic-btn { flex: none; }
</style>
