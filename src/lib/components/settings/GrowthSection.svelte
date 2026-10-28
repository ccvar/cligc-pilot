<script lang="ts">
  // 设置里的「手机通知」（Telegram 机器人：日报、站点打不开、等你确认上线的改动带按钮）
  // 和「收益与外部数据」（AdSense 授权、导入推广佣金 CSV、PageSpeed / Serper 的 API Key）。
  // 样式和「后台运行」「数据备份」两节一样：一组一张卡片、每项一行，说明收进 ⓘ。
  import '$lib/components/site/site.css';
  import Icon from '$lib/components/site/Icon.svelte';
  import InfoTip from '$lib/components/common/InfoTip.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { confirmDialog } from '$lib/state/app.svelte.js';
  import { ago, full } from '$lib/components/site/fmt';
  import {
    growth, loadGrowth, setPushToken, startPairing, setPush, testPush, forgetPush,
    connectAdsense, refreshAdsense, disconnectAdsense, importRevenue, setApiKey, removeApiKey,
  } from '$lib/state/growth.svelte.js';

  const p = $derived(growth.push);
  const r = $derived(growth.revenue);
  const k = $derived(growth.keys);
  let token = $state('');
  let psKey = $state('');
  let serperKey = $state('');
  $effect(() => { void loadGrowth(); });
  // 配对中：每 3 秒看一眼绑上了没有（机器人收到配对码后 Rust 端也会发 push-changed）
  $effect(() => {
    if (!p?.pair_code) return;
    const t = setInterval(() => void loadGrowth(), 3000);
    return () => clearInterval(t);
  });
  const left = $derived(p?.pair_until ? Math.max(0, Math.round((p.pair_until - Date.now() / 1000) / 60)) : 0);

  async function forget() {
    if (await confirmDialog('断开 Telegram？机器人的 Token 会从钥匙串删掉，手机上不再收到通知，之前发出去的按钮也不能用了。', { title: '断开手机通知', kind: 'danger', confirmText: '断开' })) void forgetPush();
  }
  async function offAds() {
    if (await confirmDialog('断开 AdSense？授权会从钥匙串删掉，各站点的 AdSense 收益不再显示（手动记的不受影响）。', { title: '断开 AdSense', kind: 'danger', confirmText: '断开' })) void disconnectAdsense();
  }
  async function offKey(name: 'pagespeed' | 'serper') {
    const what = name === 'serper' ? 'Serper 的 Key？之后不再查竞品' : 'PageSpeed 的 Key？之后用免费额度测速，偶尔会被限流';
    if (await confirmDialog(`删除 ${what}。`, { title: '删除 API Key', kind: 'danger', confirmText: '删除' })) void removeApiKey(name);
  }
</script>

<div class="su gs">
  <section>
    <h4>手机通知<InfoTip label="手机通知说明" text={'人不在电脑前也知道站点怎么样：托管日报、站点打不开、推广链接失效、自动上线和回滚，同时发到你的 Telegram。\nAI 改好、等你确认的改动带「上线 / 驳回」按钮，点一下就行。\n机器人是你自己在 @BotFather 建的，Token 只存在钥匙串；只认绑定的那个人。\nPilot 要开着才能收发（关窗口没事，留在菜单栏就行）。'} /></h4>
    {#if p}
      <div class="rows">
        {#if !p.has_token}
          <div class="opt">
            <span class="ot"><b>机器人 Token<InfoTip label="怎么拿 Token" text={'在 Telegram 里搜 @BotFather，发 /newbot，按提示起个名字，\n它会给一串 123456789:AA… 的 Token，复制过来。'} /></b><small>还没连接</small></span>
            <span class="acts">
              <input class="tin mono" type="password" placeholder="123456789:AA…" bind:value={token} aria-label="机器人 Token" />
              <button class="btn tiny" disabled={!token.trim() || !!growth.busy} onclick={async () => { if (await setPushToken(token)) token = ''; }}>{#if growth.busy === 'tg-token'}<span class="spin"></span>验证中…{:else}连接{/if}</button>
            </span>
          </div>
        {:else if !p.bound}
          <div class="opt">
            <span class="ot"><b>@{p.bot_name}</b>
              {#if p.pair_code}<small class="wrap">在 Telegram 里给机器人发 <code>/start {p.pair_code}</code>（{left} 分钟内有效），绑好这里会自动更新</small>
              {:else}<small>还没绑定你的 Telegram</small>{/if}
            </span>
            <span class="acts">
              {#if p.pair_code}
                <button class="btn tiny ghost" onclick={() => openUrl(`https://t.me/${p.bot_name}?start=${p.pair_code}`)}>打开 Telegram</button>
                <button class="btn tiny ghost" onclick={startPairing} data-tip="换一个配对码">换一个码</button>
              {:else}
                <button class="btn tiny" disabled={!!growth.busy} onclick={startPairing}>绑定手机</button>
              {/if}
              <button class="ic-btn ic-sm" onclick={forget} aria-label="断开" data-tip="断开（删掉 Token）"><Icon name="x" size={13} /></button>
            </span>
          </div>
        {:else}
          <div class="opt">
            <span class="ot"><b>发通知到 Telegram</b><small>@{p.bot_name} → {p.chat_name || '你'}</small></span>
            <button class="switch" class:on={p.enabled} role="switch" aria-checked={p.enabled} aria-label="发通知到 Telegram" onclick={() => setPush({ enabled: !p.enabled })}><i></i></button>
          </div>
          <div class="opt" class:dim={!p.enabled}>
            <span class="ot"><b>在手机上确认上线<InfoTip label="在手机上确认上线" text={'等你确认的改动消息里带「上线」按钮，点了就合并并上线（和在电脑上点「确认上线」一样）。\n关掉的话只发通知、只能驳回，上线到电脑上点。'} /></b></span>
            <button class="switch" class:on={p.allow_approve} role="switch" aria-checked={p.allow_approve} aria-label="在手机上确认上线" disabled={!p.enabled} onclick={() => setPush({ allow_approve: !p.allow_approve })}><i></i></button>
          </div>
          <div class="opt">
            {#if p.last_err}<span class="last err-text" data-tip={p.last_err}><Icon name="x" size={12} />{p.last_err}</span>{:else}<span class="last faint">可以给机器人发 /status、/pending、/pause</span>{/if}
            <span class="acts">
              <button class="btn tiny ghost" disabled={!!growth.busy} onclick={forget}>断开</button>
              <button class="btn tiny" disabled={!!growth.busy || !p.enabled} onclick={testPush}>{#if growth.busy === 'tg-test'}<span class="spin"></span>发送中…{:else}发测试消息{/if}</button>
            </span>
          </div>
        {/if}
      </div>
    {/if}
  </section>

  <section>
    <h4>收益与外部数据<InfoTip label="收益与外部数据说明" text={'AdSense 收益每天更新一次，和 AI 花费一起显示在站点列表、托管页和站点的「数据」里。\n推广佣金、赞助没有统一接口：在站点里手动记，或者导入 CSV。\nPageSpeed 用来每周测页面速度，Serper 用来每周查竞品排名。Key 都只存在钥匙串。'} /></h4>
    {#if r && k}
      <div class="rows">
        <div class="opt">
          {#if r.connected}
            <span class="ot"><b>AdSense</b><small data-tip={r.fetched_at ? `上次更新：${full(r.fetched_at)}` : null}>{[r.account_name || r.account.replace('accounts/', ''), r.email].filter(Boolean).join(' · ')}{r.fetched_at ? ` · ${ago(r.fetched_at)}更新` : ''}</small>{#if r.error}<small class="err-text" data-tip={r.error}>{r.error}</small>{/if}</span>
            <span class="acts">
              <button class="btn tiny ghost" disabled={!!growth.busy} onclick={offAds}>断开</button>
              <button class="btn tiny" disabled={!!growth.busy} onclick={refreshAdsense}>{#if growth.busy === 'ads-refresh'}<span class="spin"></span>更新中…{:else}<Icon name="refresh" size={11} />立即更新{/if}</button>
            </span>
          {:else}
            <span class="ot"><b>AdSense<InfoTip label="连接 AdSense" text={'在浏览器里用有 AdSense 的 Google 账号授权「查看 AdSense 数据」（只读）。\n用的是「数据源」里同一个 Google OAuth 客户端；它所在的 Google Cloud 项目要先启用 AdSense Management API。\nAdSense 里「网站」列表的域名和站点域名对上了，就算到那个站点。'} /></b><small>{r.has_client ? '还没连接' : '先在上面「数据源」里配好 Google OAuth 客户端'}</small></span>
            <button class="btn tiny" disabled={!r.has_client || !!growth.busy} onclick={connectAdsense}>{#if growth.busy === 'ads'}<span class="spin"></span>等浏览器授权…{:else}在浏览器里授权{/if}</button>
          {/if}
        </div>
        <div class="opt">
          <span class="ot"><b>导入推广佣金<InfoTip label="CSV 格式" text={'每行：月份,域名,金额,来源,备注（后两列可以不写）\n例：2026-10,example.com,128.50,affiliate,OKX 返佣\n金额按美元；来源写 affiliate / sponsor / other；第一行是表头会自动跳过。'} /></b><small>也可以在站点的「数据」里一笔一笔记</small></span>
          <button class="btn tiny ghost" disabled={!!growth.busy} onclick={importRevenue}>{#if growth.busy === 'csv'}<span class="spin"></span>导入中…{:else}<Icon name="upload" size={11} />选择 CSV…{/if}</button>
        </div>
        {@render keyRow('pagespeed', 'PageSpeed API Key', '可以不填：不填用免费额度，偶尔被限流、下次再测。\n要填的话在 Google Cloud 控制台「API 和服务 → 凭据」建一个 API Key，并启用 PageSpeed Insights API。', k.pagespeed, '不填也能测速')}
        {@render keyRow('serper', 'Serper API Key', '竞品观察用：在 serper.dev 注册拿 API Key，每次搜索扣一次额度（1000 次约 1 美元）。\n每个站点每周最多查 15 个词。不填就不查竞品。', k.serper, '填了才会查竞品')}
      </div>
    {/if}
  </section>
</div>

{#snippet keyRow(name: 'pagespeed' | 'serper', label: string, tip: string, has: boolean, empty: string)}
  <div class="opt">
    <span class="ot"><b>{label}<InfoTip {label} text={tip} /></b><small>{has ? '已填（存在钥匙串里）' : empty}</small></span>
    {#if has}
      <button class="btn tiny ghost" disabled={!!growth.busy} onclick={() => offKey(name)}>删除</button>
    {:else}
      <span class="acts">
        {#if name === 'pagespeed'}
          <input class="tin mono" type="password" placeholder="AIza…" bind:value={psKey} aria-label={label} />
          <button class="btn tiny" disabled={!psKey.trim() || !!growth.busy} onclick={async () => { if (await setApiKey(name, psKey)) psKey = ''; }}>{#if growth.busy === `key-${name}`}<span class="spin"></span>验证中…{:else}保存{/if}</button>
        {:else}
          <input class="tin mono" type="password" placeholder="粘贴 API Key" bind:value={serperKey} aria-label={label} />
          <button class="btn tiny" disabled={!serperKey.trim() || !!growth.busy} onclick={async () => { if (await setApiKey(name, serperKey)) serperKey = ''; }}>{#if growth.busy === `key-${name}`}<span class="spin"></span>验证中…{:else}保存{/if}</button>
        {/if}
      </span>
    {/if}
  </div>
{/snippet}

<style>
  .gs { display: flex; flex-direction: column; gap: 24px; margin-top: 24px; }
  section { display: flex; flex-direction: column; gap: 8px; }
  h4 { min-height: 24px; margin: 0; display: flex; align-items: center; gap: 2px; font-size: 11px; font-weight: 600; letter-spacing: .03em; color: var(--faint); }
  .rows { border: 1px solid var(--border); border-radius: 11px; overflow: hidden; background: #fff; }
  .opt { min-height: 44px; box-sizing: border-box; display: flex; align-items: center; gap: 12px; justify-content: space-between; padding: 7px 12px; }
  .opt + .opt { border-top: 1px solid var(--border); }
  .opt.dim { opacity: .55; }
  .ot { min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .ot b { display: inline-flex; align-items: center; gap: 4px; font-size: 13px; font-weight: 500; }
  .ot small { font-size: 11.5px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ot small.err-text { color: var(--err); }
  .ot small.wrap { white-space: normal; }
  .ot code { font-size: 11px; }
  .acts { flex: none; display: inline-flex; align-items: center; gap: 6px; }
  .acts .tin { width: 200px; padding: 4.5px 9px; font-size: 12px; }
  .switch { position: relative; flex: none; width: 32px; height: 18px; border: none; border-radius: 999px; background: var(--border2); cursor: pointer; padding: 0; transition: background-color .18s; }
  .switch i { position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; box-shadow: 0 1px 2px rgba(0, 0, 0, .2); transition: transform .18s var(--su-ease); }
  .switch.on { background: var(--ok); }
  .switch.on i { transform: translateX(14px); }
  .switch:disabled { cursor: default; }
  .switch:focus-visible { box-shadow: var(--su-ring); }
  .last { min-width: 0; display: inline-flex; align-items: center; gap: 5px; font-size: 12.5px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .last.err-text { color: var(--err); }
  .mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
</style>
