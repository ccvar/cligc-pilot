//! 多站点：全部站点总览、跨站点待确认、置顶 / 标签、数据源全局管理与按域名批量绑定。
//! 站点数量不设上限：总览只读每站的小文件（summary.json、队列、最近一次上线），不读大缓存。

use serde::Serialize;
use std::collections::HashMap;

use crate::site_ops::QueueItem;
use crate::sites::Site;
use crate::AppState;

#[derive(Serialize)]
pub struct SiteRow {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub source: String,
    pub repo: String,
    pub pinned: bool,
    pub tags: Vec<String>,
    pub level: u8,
    /// 所在托管方案名（没托管为空）与自主程度：suggest / review / auto，方案暂停或站点停止时为 stopped。
    pub plan: String,
    pub autonomy: String,
    pub deploy_mode: String,
    pub has_data: bool,
    pub summary: crate::site_data::Summary,
    /// 待你确认 / 失败 / 未处理的条数。
    pub proposed: usize,
    pub failed: usize,
    pub open: usize,
    pub job: String,
    pub last_deploy_ts: u64,
    pub last_deploy_ok: Option<bool>,
    /// 在线监测：首页连续两次打不开、从什么时候开始；证书 / 域名还剩几天（没查到为 None）、证书出错原因
    pub site_down: bool,
    pub down_since: u64,
    pub down_err: String,
    pub cert_days: Option<i64>,
    pub cert_err: String,
    pub domain_days: Option<i64>,
    /// 上次推广链接检查里失效的个数
    pub links_broken: usize,
    /// 分发渠道（站点列表上显示图标）
    pub channels: Vec<crate::syndicate::ChannelBrief>,
    /// 最近活动：会话、上线、站点设置里最晚的那个。
    pub active_at: u64,
    /// 收益（本月 / 上月）和本月 AI 花费（美元，能估出来的部分）
    pub money: crate::revenue::Money,
    pub cost_month_usd: f64,
}

#[tauri::command]
pub fn sites_overview(state: tauri::State<'_, AppState>) -> Vec<SiteRow> {
    let mut last_conv: HashMap<String, u64> = HashMap::new();
    for c in state.convos.list() {
        if let Some(id) = c.conn_id.strip_prefix("site:") {
            let e = last_conv.entry(id.to_string()).or_default();
            *e = (*e).max(c.updated_at);
        }
    }
    let cfg = crate::managed::load(&state.data_dir);
    let ads = crate::revenue::load_cache(&state.data_dir);
    let month = crate::managed_cmds::month_start();
    state
        .sites
        .list()
        .into_iter()
        .map(|s| {
            let group = cfg.group_of(&s.id);
            let plan = group.map(|g| g.name.clone()).unwrap_or_default();
            let autonomy = group.map(|g| if g.active_for(&s.id) { g.autonomy.clone() } else { "stopped".to_string() }).unwrap_or_default();
            let dir = state.sites.data_dir(&s.id);
            if !attended(&cfg, &s.id) {
                sync_unattended(&dir);
            }
            let q = crate::site_ops::load_queue(&dir);
            let count = |st: &str| q.iter().filter(|i| i.status == st).count();
            // 你自己取消的上线不算失败：按最近一次没被取消的那次算
            let last = crate::site_deploy::load_history(&dir).into_iter().find(|d| d.ok || !d.error.contains("已取消"));
            let job = state.site_jobs.snapshot(&s.id).filter(|j| j.status == "running").map(|j| j.title).unwrap_or_default();
            let last_ts = last.as_ref().map(|d| d.ts).unwrap_or(0);
            let mon = crate::monitor::load(&dir);
            SiteRow {
                money: crate::revenue::site_money(&ads, &s, &dir),
                cost_month_usd: crate::usage::site_cost(&dir, month).usd,
                active_at: s.updated_at.max(last_ts).max(last_conv.get(&s.id).copied().unwrap_or(0)),
                has_data: !s.data.gsc_site.is_empty() || !s.data.ga4_property.is_empty() || !s.data.bing_site.is_empty(),
                summary: crate::site_data::summary(&dir),
                proposed: count("proposed"),
                failed: count("failed"),
                open: count("open"),
                job,
                last_deploy_ts: last_ts,
                last_deploy_ok: last.map(|d| d.ok),
                channels: crate::syndicate::channel_briefs(&dir),
                links_broken: crate::link_check::load(&dir).broken(),
                site_down: mon.down(),
                down_since: mon.down_since(),
                down_err: mon.pings.last().map(|p| p.err.clone()).unwrap_or_default(),
                cert_days: mon.cert_days(crate::now_secs()),
                cert_err: mon.cert_err.clone(),
                domain_days: mon.domain_days(crate::now_secs()),
                id: s.id,
                name: s.name,
                domain: s.domain,
                source: s.source,
                repo: s.repo,
                pinned: s.pinned,
                tags: s.tags,
                level: s.ops.level,
                plan,
                autonomy,
                deploy_mode: s.deploy.mode,
            }
        })
        .collect()
}

#[derive(Serialize)]
pub struct PendingItem {
    pub site_id: String,
    pub site_name: String,
    pub item: QueueItem,
}

/// 所有站点里等你处理的条目（待确认、失败、上线后待核对），按时间倒序。
#[tauri::command]
pub fn pending_across_sites(state: tauri::State<'_, AppState>) -> Vec<PendingItem> {
    let mut out: Vec<PendingItem> = Vec::new();
    for s in state.sites.list() {
        for item in crate::site_ops::load_queue(&state.sites.data_dir(&s.id)) {
            if crate::site_ops::needs_you(&item) {
                out.push(PendingItem { site_id: s.id.clone(), site_name: s.name.clone(), item });
            }
        }
    }
    out.sort_by(|a, b| b.item.updated_at.cmp(&a.item.updated_at));
    out
}

/// 没人自动处理的站点（自己运营的，或托管方案暂停 / 单独停止的）：每天同步数据时算出的机会，
/// 还没进待处理队列的补进去（和扫描入队同一套去重和 21 天冷却：驳回过的不会再冒出来）。只在有新机会时才写文件。
fn sync_unattended(site_dir: &std::path::Path) {
    let cached: Vec<crate::site_ops::Opportunity> = crate::site_data::read_json(&crate::site_data::cache_dir(site_dir), "opportunities.json");
    if cached.is_empty() {
        return;
    }
    let now = crate::now_secs();
    let q = crate::site_ops::load_queue(site_dir);
    let fresh: Vec<_> = cached.into_iter().filter(|o| crate::site_ops::should_enqueue(&q, &o.key, now, crate::site_runner::COOLDOWN)).collect();
    if !fresh.is_empty() {
        let _ = crate::site_ops::enqueue(site_dir, &fresh, now, crate::site_runner::COOLDOWN);
    }
}

/// AI 会不会自动处理这个站点的机会：在托管方案里、方案和总开关都没暂停、站点没单独停止
fn attended(cfg: &crate::managed::ManagedConfig, site_id: &str) -> bool {
    !cfg.paused && cfg.group_of(site_id).is_some_and(|g| g.active_for(site_id))
}

/// 运营机会的优先级：urgent 正在伤害站点的问题、value 收益大的、easy 顺手做的小改动。
pub fn tier_of(o: &crate::site_ops::Opportunity) -> &'static str {
    match o.kind.as_str() {
        "crawl_issue" | "sitemap_error" | "slow" => "urgent",
        // 点击掉了四成以上
        "declining" if o.prev_clicks > 0.0 && o.clicks < o.prev_clicks * 0.6 => "urgent",
        "img_audit" if o.title.contains("不存在") => "urgent",
        "stale_year" | "stale" | "schema" | "img_audit" => "easy",
        _ if matches!(o.change.as_str(), "internal_links" | "meta_description" | "title") => "easy",
        _ => "value",
    }
}

/// AI 能直接处理、不用你拿主意的：改标题和描述、标题年份、抓取和 sitemap 报错（sitemap 先由程序检查）、图片、结构化数据。
/// 扩写内容、新写页面、合并页面、对比竞品这类要你判断值不值得做，不算。
pub fn ai_ready(o: &crate::site_ops::Opportunity) -> bool {
    matches!(o.kind.as_str(), "low_ctr" | "stale_year" | "sitemap_error" | "crawl_issue" | "img_audit" | "schema")
}

/// score 是「每月大约能多拿到的点击」的那几类机会
fn gain_of(o: &crate::site_ops::Opportunity) -> f64 {
    if matches!(o.kind.as_str(), "low_ctr" | "striking" | "declining" | "content_gap" | "cannibal" | "serp_gap") {
        o.score
    } else {
        0.0
    }
}

#[derive(Serialize)]
pub struct OppItem {
    pub site_id: String,
    pub site_name: String,
    pub domain: String,
    pub item: QueueItem,
    pub tier: &'static str,
    /// 每月大约能多拿到的点击（算不出为 0）
    pub gain: f64,
    /// AI 能直接处理、不用你拿主意
    pub ai: bool,
}

#[derive(Serialize)]
pub struct OppOverview {
    pub items: Vec<OppItem>,
    /// 托管站点里排着的机会（AI 会自动处理，不列出来）
    pub managed_open: usize,
    /// 正在跑作业的站点
    pub running: Vec<String>,
    /// 跨站点批量里还排着、没开始的站点数
    pub waiting: usize,
}

/// 没人自动处理的站点（自己运营的、托管停着的）里还没处理的运营机会，按优先级、预计收益排。
/// 托管且在运行的站点由 AI 自动处理，只给个数。
#[tauri::command]
pub fn opportunities_across_sites(state: tauri::State<'_, AppState>) -> OppOverview {
    let cfg = crate::managed::load(&state.data_dir);
    let mut items = Vec::new();
    let mut managed_open = 0;
    let mut running = Vec::new();
    for s in state.sites.list() {
        if state.site_jobs.running(&s.id) {
            running.push(s.id.clone());
        }
        let dir = state.sites.data_dir(&s.id);
        let on = attended(&cfg, &s.id);
        if !on {
            sync_unattended(&dir);
        }
        let open: Vec<QueueItem> = crate::site_ops::load_queue(&dir).into_iter().filter(|i| i.status == "open").collect();
        if on {
            managed_open += open.len();
            continue;
        }
        for item in open {
            items.push(OppItem { site_id: s.id.clone(), site_name: s.name.clone(), domain: s.domain.clone(), tier: tier_of(&item.opp), gain: gain_of(&item.opp), ai: ai_ready(&item.opp), item });
        }
    }
    let rank = |t: &str| match t { "urgent" => 0, "value" => 1, _ => 2 };
    items.sort_by(|a, b| rank(a.tier).cmp(&rank(b.tier)).then(b.item.opp.score.total_cmp(&a.item.opp.score)));
    OppOverview { items, managed_open, running, waiting: ACROSS.load(std::sync::atomic::Ordering::SeqCst) }
}

static ACROSS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 跨站点批量让 AI 改：每个站点一个作业（里面按顺序一条一条做），同时最多跑 3 个作业，其余排着等。
/// 立即返回；作业开始、结束会照常发 site-job / site-queue 事件。
#[tauri::command]
pub fn work_across_sites(app: tauri::AppHandle, batches: Vec<(String, Vec<String>)>) -> Result<usize, String> {
    use tauri::Manager;
    let batches: Vec<(String, Vec<String>)> = batches.into_iter().filter(|(_, ids)| !ids.is_empty()).collect();
    if batches.iter().any(|(_, ids)| ids.iter().any(|x| x.contains(','))) {
        return Err("条目 id 不合法".into());
    }
    let n = batches.len();
    ACROSS.fetch_add(n, std::sync::atomic::Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        let st = app.state::<AppState>();
        let deadline = crate::now_secs() + 6 * 3600;
        let mut left = batches;
        // 每 10 秒看一圈：有空位、站点自己没在跑作业的就开始（某个站点忙着不耽误后面的）
        while !left.is_empty() {
            left.retain(|(site_id, ids)| {
                let gone = st.sites.get(site_id).is_err();
                let started = !gone && st.site_jobs.running_count() < crate::site_runner::MAX_PARALLEL_JOBS && !st.site_jobs.running(site_id) && {
                    let ctx = crate::site_runner::Ctx { app: app.clone() };
                    let r = if ids.len() == 1 { crate::site_runner::start(ctx, site_id, "work", "manual", Some(ids[0].clone())) } else { crate::site_runner::start(ctx, site_id, "batch", "manual", Some(ids.join(","))) };
                    r.is_ok()
                };
                let done = gone || started || crate::now_secs() > deadline;
                if done {
                    ACROSS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                }
                !done
            });
            if !left.is_empty() {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        }
    });
    Ok(n)
}

/// 「最近上线」的一条：改动本身，加上这个页面现在近 28 天的搜索数据（和上线前那 28 天对比看效果）。
#[derive(Serialize)]
pub struct ShipItem {
    pub site_id: String,
    pub site_name: String,
    pub item: QueueItem,
    /// 这个页面现在（最近一次拉数据）的近 28 天 Google 数据；没有展示就是 None
    pub page_now: Option<crate::site_data::Row>,
    /// 现在这份数据截止到哪天（GSC 有 2–3 天延迟）
    pub data_end: String,
}

/// 托管站点最近 days 天上线过的改动（含之后被回滚的），新的在前：托管页「最近上线」用。
#[tauri::command]
pub fn recent_ships(state: tauri::State<'_, AppState>, days: Option<u64>) -> Vec<ShipItem> {
    let since = crate::now_secs().saturating_sub(days.unwrap_or(30).min(90) * 86_400);
    let cfg = crate::managed::load(&state.data_dir);
    let mut out: Vec<ShipItem> = Vec::new();
    for s in state.sites.list().into_iter().filter(|s| cfg.group_of(&s.id).is_some()) {
        let dir = state.sites.data_dir(&s.id);
        let items: Vec<QueueItem> = crate::site_ops::load_queue(&dir).into_iter().filter(|i| i.deployed_at >= since && matches!(i.status.as_str(), "deployed" | "rolled_back" | "done")).collect();
        if items.is_empty() {
            continue;
        }
        let cache = crate::site_data::cache_dir(&dir);
        let pages: Vec<crate::site_data::Row> = crate::site_data::read_json(&cache, "gsc_pages.json");
        let data_end = crate::site_data::summary(&dir).data_end;
        for item in items {
            let page = if item.opp.page.starts_with("https://") { item.opp.page.clone() } else { item.new_pages.first().cloned().unwrap_or_default() };
            let page_now = pages.iter().find(|r| r.key == page).cloned();
            out.push(ShipItem { site_id: s.id.clone(), site_name: s.name.clone(), item, page_now, data_end: data_end.clone() });
        }
    }
    out.sort_by(|a, b| b.item.deployed_at.cmp(&a.item.deployed_at));
    out
}

/// 这个站点上次推广链接检查的结果（没查过是空的）。
#[tauri::command]
pub fn site_links(state: tauri::State<'_, AppState>, id: String) -> Result<crate::link_check::LinkReport, String> {
    state.sites.get(&id)?;
    Ok(crate::link_check::load(&state.sites.data_dir(&id)))
}

/// 现在就检查这个站点的推广链接。
#[tauri::command]
pub async fn check_site_links(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<crate::link_check::LinkReport, String> {
    let site = state.sites.get(&id)?;
    let out = crate::syndicate::built_out(&state, &site).ok();
    let r = crate::link_check::check_site(&site, &state.sites.data_dir(&id), out).await;
    // 全部站点列表上的「推广链接失效」跟着更新
    use tauri::Emitter;
    let _ = app.emit("links-checked", ());
    r
}

/// 站点概览「在线情况」卡片：最近的检查、7 天可用率、证书 / 域名到期。
#[derive(Serialize)]
pub struct MonitorView {
    pub last: Option<crate::monitor::Ping>,
    pub down: bool,
    pub down_since: u64,
    /// 最近 7 天能打开的比例（0–1），没检查过为 -1
    pub uptime: f64,
    pub avg_ms: u32,
    pub checks: usize,
    pub cert_expires: u64,
    pub cert_err: String,
    pub cert_checked: u64,
    pub domain_expires: u64,
    pub domain_err: String,
    pub domain_checked: u64,
}

fn monitor_view(m: crate::monitor::Monitor) -> MonitorView {
    let ok: Vec<&crate::monitor::Ping> = m.pings.iter().filter(|p| p.ok).collect();
    MonitorView {
        last: m.pings.last().cloned(),
        down: m.down(),
        down_since: m.down_since(),
        uptime: if m.pings.is_empty() { -1.0 } else { ok.len() as f64 / m.pings.len() as f64 },
        avg_ms: if ok.is_empty() { 0 } else { (ok.iter().map(|p| p.ms as u64).sum::<u64>() / ok.len() as u64) as u32 },
        checks: m.pings.len(),
        cert_expires: m.cert_expires,
        cert_err: m.cert_err,
        cert_checked: m.cert_checked,
        domain_expires: m.domain_expires,
        domain_err: m.domain_err,
        domain_checked: m.domain_checked,
    }
}

#[tauri::command]
pub fn site_monitor(state: tauri::State<'_, AppState>, id: String) -> Result<MonitorView, String> {
    state.sites.get(&id)?;
    Ok(monitor_view(crate::monitor::load(&state.sites.data_dir(&id))))
}

/// 现在就检查首页、证书和域名。
#[tauri::command]
pub async fn check_site_monitor(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<MonitorView, String> {
    let site = state.sites.get(&id)?;
    if site.domain.trim().is_empty() {
        return Err("这个站点还没有设置域名".into());
    }
    let (m, _) = crate::monitor::check_site(&site, &state.sites.data_dir(&id), true).await;
    use tauri::Emitter;
    let _ = app.emit("monitor-checked", ());
    Ok(monitor_view(m))
}

#[tauri::command]
pub fn set_site_flags(state: tauri::State<'_, AppState>, id: String, pinned: Option<bool>, tags: Option<Vec<String>>) -> Result<Site, String> {
    let tags = tags.map(|t| crate::sites::normalize_tags(&t)).transpose()?;
    state.sites.update(&id, crate::now_secs(), |s| {
        if let Some(p) = pinned {
            s.pinned = p;
        }
        if let Some(t) = tags {
            s.tags = t;
        }
        Ok(())
    })
}

#[derive(Serialize)]
pub struct DataSourceRow {
    pub conn: crate::conns::Connection,
    /// 用着这个数据源的站点名。
    pub sites: Vec<String>,
}

#[tauri::command]
pub fn data_sources_overview(state: tauri::State<'_, AppState>) -> Vec<DataSourceRow> {
    let sites = state.sites.list();
    state
        .conns
        .list()
        .into_iter()
        .filter(|c| matches!(c.kind.as_str(), "google" | "bing"))
        .map(|c| {
            let used = sites
                .iter()
                .filter(|s| s.data.google_conn_id == c.id || s.data.gsc_conn() == c.id || s.data.bing_conn_id == c.id)
                .map(|s| s.name.clone())
                .collect();
            DataSourceRow { conn: c, sites: used }
        })
        .collect()
}

#[derive(Serialize, Default)]
pub struct BindReport {
    /// （站点名，绑上的内容）
    pub bound: Vec<(String, String)>,
    /// 没找到对应资源的站点名
    pub unmatched: Vec<String>,
    /// 已经绑好、没动的站点数
    pub kept: usize,
    pub errors: Vec<String>,
    /// 绑上的站点 id（和 bound 一一对应；自动匹配完要去拉这些站点的数据）
    #[serde(skip)]
    pub bound_ids: Vec<String>,
}

/// GA4 媒体资源的数据流主机名缓存（自动匹配用：打开站点总览、每天同步都会跑，不必每次都把每个媒体资源查一遍）。
static STREAM_HOSTS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, (std::time::Instant, Vec<String>)>>> =
    std::sync::LazyLock::new(Default::default);
const STREAM_HOSTS_TTL: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

async fn stream_hosts(token: &str, property: &str, fresh: bool) -> Vec<String> {
    if !fresh {
        if let Some((at, h)) = STREAM_HOSTS.lock().ok().and_then(|m| m.get(property).cloned()) {
            if at.elapsed() < STREAM_HOSTS_TTL {
                return h;
            }
        }
    }
    let Ok(h) = crate::google::ga4_stream_hosts(token, property).await else { return Vec::new() };
    if let Ok(mut m) = STREAM_HOSTS.lock() {
        m.insert(property.to_string(), (std::time::Instant::now(), h.clone()));
    }
    h
}

/// 按域名把一个数据源批量绑到所有站点：GA4 按网页数据流的网址匹配，Search Console 优先网域资源，
/// Bing 按站点网址的主机名。已经绑好的不动（`overwrite` 为真时才重绑）；没有域名的站点跳过。
#[tauri::command]
pub async fn auto_bind_sites(state: tauri::State<'_, AppState>, conn_id: String, overwrite: bool) -> Result<BindReport, String> {
    bind_conn(&state, &conn_id, overwrite, true).await
}

/// `auto_bind_sites` 的本体；自动匹配（`data_cmds::auto_bind_data`）也用它。
/// `fresh` 为假时数据流主机名可以用 6 小时内的缓存。
pub async fn bind_conn(state: &AppState, conn_id: &str, overwrite: bool, fresh: bool) -> Result<BindReport, String> {
    let conn = state.conns.get(conn_id)?;
    let sites: Vec<Site> = state.sites.list().into_iter().filter(|s| !s.domain.is_empty()).collect();
    let mut report = BindReport::default();
    match conn.kind.as_str() {
        "google" => {
            let raw = crate::keychain::get_key(&conn.id)?;
            let cred = crate::google::parse_credential(&conn.key_kind, &raw)?;
            let token = crate::google::access_token(&conn.id, &cred, &crate::google::read_scopes()).await?;
            let props = crate::google::list_ga4_properties(&token).await.unwrap_or_else(|e| {
                report.errors.push(format!("GA4：{e}"));
                Vec::new()
            });
            let gsc = crate::google::list_gsc_sites(&token).await.unwrap_or_else(|e| {
                report.errors.push(format!("Search Console：{e}"));
                Vec::new()
            });
            // 每个媒体资源的数据流主机名（只在有站点需要 GA4 时才去查）
            let need_ga4 = sites.iter().any(|s| overwrite || s.data.ga4_property.is_empty());
            let mut hosts: Vec<(crate::google::Ga4Property, Vec<String>)> = Vec::new();
            if need_ga4 {
                for p in props {
                    let h = stream_hosts(&token, &p.property, fresh).await;
                    hosts.push((p, h));
                }
            }
            for s in &sites {
                let mut what = Vec::new();
                let mut ga4: Option<(String, String)> = None;
                let mut gsc_hit: Option<String> = None;
                // GA4 / Search Console 各自独立：已经绑在别的账号下的那一项不动（除非要求全部重新绑定）
                if overwrite || s.data.ga4_property.is_empty() {
                    if let Some((p, _)) = hosts.iter().find(|(_, hs)| hs.iter().any(|h| crate::google::same_host(h, &s.domain))) {
                        ga4 = Some((p.property.clone(), p.name.clone()));
                        what.push(format!("GA4 {}", p.name));
                    }
                }
                if overwrite || s.data.gsc_site.is_empty() {
                    if let Some(g) = crate::google::match_gsc_site(&gsc, &s.domain) {
                        gsc_hit = Some(g.url.clone());
                        what.push(format!("Search Console {}", g.url));
                    }
                }
                if ga4.is_none() && gsc_hit.is_none() {
                    if !s.data.ga4_property.is_empty() || !s.data.gsc_site.is_empty() {
                        report.kept += 1;
                    } else {
                        report.unmatched.push(s.name.clone());
                    }
                    continue;
                }
                let cid = conn.id.clone();
                state.sites.update(&s.id, crate::now_secs(), |x| {
                    if let Some((p, n)) = ga4 {
                        // 换 GA4 账号时，Search Console 原本跟着旧账号走的，要把旧账号记成它自己的
                        if x.data.gsc_conn_id.is_empty() && !x.data.gsc_site.is_empty() && x.data.google_conn_id != cid {
                            x.data.gsc_conn_id = x.data.google_conn_id.clone();
                        }
                        x.data.google_conn_id = cid.clone();
                        x.data.ga4_property = p;
                        x.data.ga4_property_name = n;
                    }
                    if let Some(g) = gsc_hit {
                        if x.data.google_conn_id.is_empty() { x.data.google_conn_id = cid.clone(); }
                        x.data.gsc_conn_id = if x.data.google_conn_id == cid { String::new() } else { cid.clone() };
                        x.data.gsc_site = g;
                    }
                    Ok(())
                })?;
                report.bound.push((s.name.clone(), what.join("、")));
                report.bound_ids.push(s.id.clone());
            }
        }
        "bing" => {
            let key = crate::keychain::get_key(&conn.id)?;
            let list = crate::bing::list_sites(&key).await?;
            for s in &sites {
                if !overwrite && !s.data.bing_site.is_empty() {
                    report.kept += 1;
                    continue;
                }
                let hit = list.iter().find(|b| b.verified && crate::sites::normalize_domain(&b.url).is_ok_and(|h| crate::google::same_host(&h, &s.domain)));
                let Some(b) = hit else {
                    report.unmatched.push(s.name.clone());
                    continue;
                };
                let (cid, url) = (conn.id.clone(), b.url.clone());
                state.sites.update(&s.id, crate::now_secs(), |x| {
                    x.data.bing_conn_id = cid;
                    x.data.bing_site = url;
                    Ok(())
                })?;
                report.bound.push((s.name.clone(), format!("Bing {}", b.url)));
                report.bound_ids.push(s.id.clone());
            }
        }
        _ => return Err("这个连接不是数据源".into()),
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::{ai_ready, tier_of};
    use crate::site_ops::Opportunity;

    fn opp(kind: &str, change: &str) -> Opportunity {
        Opportunity { kind: kind.into(), change: change.into(), ..Default::default() }
    }

    #[test]
    fn tiers_put_site_harm_first_and_small_edits_last() {
        assert_eq!(tier_of(&opp("crawl_issue", "technical")), "urgent");
        assert_eq!(tier_of(&opp("slow", "technical")), "urgent");
        assert_eq!(tier_of(&Opportunity { clicks: 50.0, prev_clicks: 100.0, ..opp("declining", "content") }), "urgent");
        assert_eq!(tier_of(&Opportunity { clicks: 80.0, prev_clicks: 100.0, ..opp("declining", "content") }), "value");
        assert_eq!(tier_of(&opp("low_ctr", "meta")), "value");
        assert_eq!(tier_of(&opp("serp_gap", "content")), "value");
        assert_eq!(tier_of(&opp("stale_year", "meta")), "easy");
        assert_eq!(tier_of(&Opportunity { title: "图片问题：1张不存在".into(), ..opp("img_audit", "technical") }), "urgent");
        assert_eq!(tier_of(&opp("img_audit", "technical")), "easy");
        assert_eq!(tier_of(&opp("plan", "internal_links")), "easy");
    }

    #[test]
    fn ai_handles_edits_and_fixes_but_not_judgment_calls() {
        for k in ["low_ctr", "sitemap_error", "img_audit", "stale_year"] {
            assert!(ai_ready(&opp(k, "meta")), "{k}");
        }
        for k in ["striking", "content_gap", "cannibal", "serp_gap", "low_rpm"] {
            assert!(!ai_ready(&opp(k, "content")), "{k}");
        }
    }
}
