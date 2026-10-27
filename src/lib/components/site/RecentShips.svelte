<script lang="ts">
  // 托管页记录卡片里的「最近上线」tab：所有托管站点最近 14 天上线的改动，新的在前（卡片和 tab 在 ManagedConsole）。
  // 每条：站点、标题、动作、页面链接（在浏览器打开线上页面）、现在的状态（验收通过 / 待核对 / 已回滚）、自动还是你确认的；
  // 点开看 AI 的说明和线上验收；悬停可以看改动、回滚、去任务队列。
  import Icon from './Icon.svelte';
  import SiteIcon from '$lib/components/site/SiteIcon.svelte';
  import DiffModal from './DiffModal.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { sites, openSite } from '$lib/state/sites.svelte.js';
  import { showDiff, rollbackItem, CHANGE_LABEL } from '$lib/state/ops.svelte.js';
  import { ago, full } from './fmt';
  import type { PendingItem, OpsItem } from '$lib/types';

  let { items }: { items: PendingItem[] } = $props();
  const LIMIT = 5;
  let all = $state(false);
  let open = $state<Set<string>>(new Set());
  function toggle(id: string) { const n = new Set(open); if (n.has(id)) n.delete(id); else n.add(id); open = n; }
  const shown = $derived(all ? items : items.slice(0, LIMIT));
  const siteOf = (id: string) => sites.list.find((s) => s.id === id);

  function shipState(i: OpsItem) {
    if (i.status === 'rolled_back') return { l: '已回滚', c: 'line' };
    if (i.status === 'done') return { l: '已处理', c: '' };
    if (i.attention) return { l: '上线待核对', c: 'warn' };
    const bad = (i.checks ?? []).filter((c) => !c.ok).length;
    if (i.checks?.length && !bad) return { l: '验收通过', c: 'ok' };
    return { l: '已上线', c: 'ok' };
  }
  /** 页面链接显示成短路径：https://iosur.com/zh/posts/x/ → /zh/posts/x/ */
  const shortPath = (u: string) => { try { const x = new URL(u); return x.pathname + x.search || '/'; } catch { return u; } };
</script>

<div class="rs">
  {#if !items.length}<p class="hint">最近 14 天托管站点没有上线改动。</p>{/if}
  {#if items.length}
    <ol class="rows">
      {#each shown as x (x.item.id)}
        {@const i = x.item}
        {@const st = shipState(i)}
        {@const site = siteOf(x.site_id)}
        {@const isOpen = open.has(i.id)}
        <li class:open={isOpen}>
          <div class="row">
            <button class="main" onclick={() => toggle(i.id)} aria-expanded={isOpen}>
              <span class="sn"><SiteIcon id={x.site_id} size={13} />{x.site_name}</span>
              <span class="tt" class:gone={i.status === 'rolled_back'}>{i.opp.title}</span>
            </button>
            {#if i.opp.page}<button class="url mono" onclick={() => openUrl(i.opp.page)} data-tip={`在浏览器打开：${i.opp.page}`}><span class="up">{shortPath(i.opp.page)}</span><Icon name="external" size={10} /></button>{/if}
            <span class="acts">
              {#if site && i.head_sha}<button class="ic-btn ic-sm" onclick={() => showDiff(site.id, i)} aria-label="看改动" data-tip="看改动"><Icon name="diff" size={13} /></button>{/if}
              {#if site && i.status === 'deployed' && i.merge_sha}<button class="ic-btn ic-sm" onclick={() => rollbackItem(site, i)} aria-label="回滚" data-tip="回滚这次改动（先确认）"><Icon name="undo" size={13} /></button>{/if}
              <button class="ic-btn ic-sm" onclick={() => openSite(x.site_id, 'queue', 'live')} aria-label="在任务队列里看" data-tip="在任务队列里看"><Icon name="inbox" size={13} /></button>
            </span>
            <span class="how faint" data-tip={i.auto ? '检查都通过后自动上线' : '你确认后上线'}>{i.auto ? '自动' : '你确认的'}</span>
            <span class="pill {st.c}">{st.l}</span>
            <span class="t faint" data-tip={full(i.deployed_at)}>{ago(i.deployed_at)}</span>
          </div>
          {#if isOpen}
            <div class="more">
              {#if CHANGE_LABEL[i.opp.change]}<span class="tag">{CHANGE_LABEL[i.opp.change]}</span>{/if}
              {#if i.attention}<p class="warn-text">要你核对：{i.attention}</p>{/if}
              {#if i.ai_summary}<p>{i.ai_summary}</p>{/if}
              {#if i.checks?.length}
                {@const bad = i.checks.filter((c) => !c.ok)}
                <p class="faint">线上验收：{i.checks.length - bad.length} 项通过{#if bad.length}，<span class="warn-text">{bad.length} 项没过（{bad.map((c) => c.label).join('、')}）</span>{/if}</p>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ol>
    {#if items.length > LIMIT}<button class="all" onclick={() => (all = !all)}>{all ? '收起' : `显示全部 ${items.length} 条`}</button>{/if}
  {/if}
</div>
<DiffModal />

<style>
  .rows { list-style: none; margin: 0 -18px; padding: 0; }
  .rows > li { border-top: 1px solid var(--border); }
  .row { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 4px 18px; }
  .rows > li:hover .row { background: #fcfbf9; }
  .main { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; border: 0; background: none; padding: 6px 0; font: inherit; color: inherit; text-align: left; cursor: pointer; }
  .sn { flex: none; display: inline-flex; align-items: center; gap: 5px; max-width: 150px; font-size: 12px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tt { min-width: 0; font-size: 13px; font-weight: 550; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tt.gone { color: var(--faint); text-decoration: line-through; }
  li.open .tt { white-space: normal; }
  /* 路径太长时只截路径文字，后面的 ↗ 图标一直露着 */
  .url { flex: 0 1 auto; min-width: 0; max-width: 260px; display: inline-flex; align-items: center; gap: 3px; border: 0; background: none; padding: 0; font-size: 11px; color: var(--faint); cursor: pointer; white-space: nowrap; }
  .url .up { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .url :global(svg) { flex: none; }
  .url:hover { color: var(--text); text-decoration: underline; text-underline-offset: 3px; }
  .acts { flex: none; display: inline-flex; gap: 2px; opacity: 0; transition: opacity .12s; }
  .rows > li:hover .acts, .acts:focus-within { opacity: 1; }
  .acts .ic-btn { margin: 0; }
  .how { flex: none; font-size: 11px; white-space: nowrap; }
  .t { flex: none; width: 64px; text-align: right; font-size: 11.5px; white-space: nowrap; }
  .more { padding: 0 18px 12px; display: flex; flex-direction: column; align-items: flex-start; gap: 6px; }
  .more p { margin: 0; font-size: 12.5px; line-height: 1.65; color: var(--dim); }
  .tag { display: inline-flex; align-items: center; height: 20px; padding: 0 8px; border-radius: 999px; background: var(--accent-soft); font-size: 11px; color: var(--dim); }
  .all { display: block; width: calc(100% + 36px); margin: 0 -18px -16px; padding: 9px 18px; border: 0; border-top: 1px solid var(--border); border-radius: 0 0 12px 12px; background: none; font: inherit; font-size: 12px; color: var(--dim); cursor: pointer; }
  .all:hover { color: var(--text); background: #fcfbf9; }
  .rows:last-child { margin-bottom: -16px; }
  .rows:last-child > li:last-child .row { border-radius: 0 0 12px 12px; }
</style>
