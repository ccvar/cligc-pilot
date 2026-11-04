//! 站点数据：GA4 / Search Console / Bing 的拉取、本地缓存与看板汇总。
//! 缓存放在 `<站点数据>/cache/`（JSON，不进代码仓库）。模型通过随附的 site-data.js 只读查询这些文件，
//! 拿不到任何数据源凭据。Search Console 有 2–3 天延迟，统一以「最近有数据的那天」为截止日。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::conns::ConnStore;
use crate::google::{self, SearchRow};
use crate::sites::{Site, SiteStore};

/// 两次自动拉取的最小间隔（打开看板 / 运营任务开始前会按需拉）。
pub const STALE_SECS: u64 = 6 * 3600;

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct SourceMeta {
    pub fetched_at: u64,
    /// 数据覆盖的区间（YYYY-MM-DD）。
    pub start: String,
    pub end: String,
    pub error: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct CacheMeta {
    #[serde(default)]
    pub ga4: SourceMeta,
    #[serde(default)]
    pub gsc: SourceMeta,
    #[serde(default)]
    pub bing: SourceMeta,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct Row {
    /// 查询词 / 页面网址 / 日期 / 来源…
    pub key: String,
    /// 第二维（查询词×页面时的页面）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key2: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct GaRow {
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key2: String,
    pub sessions: f64,
    pub users: f64,
    pub views: f64,
    pub engaged: f64,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct EventRow {
    pub event: String,
    /// 拆分维度名（空＝不拆分）与取值。
    #[serde(default)]
    pub dim: String,
    #[serde(default)]
    pub value: String,
    pub count: f64,
    pub users: f64,
}

pub fn cache_dir(site_dir: &Path) -> PathBuf {
    site_dir.join("cache")
}

fn write_json<T: Serialize>(dir: &Path, name: &str, v: &T) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_vec(v).map_err(|e| e.to_string())?;
    let p = dir.join(name);
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

pub fn read_json<T: for<'de> Deserialize<'de> + Default>(dir: &Path, name: &str) -> T {
    std::fs::read(dir.join(name))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn load_meta(site_dir: &Path) -> CacheMeta {
    read_json(&cache_dir(site_dir), "meta.json")
}

fn day(offset_days: i64) -> String {
    (chrono::Utc::now().date_naive() - chrono::Duration::days(offset_days))
        .format("%Y-%m-%d")
        .to_string()
}

fn now() -> u64 {
    crate::now_secs()
}

fn search_rows(rows: Vec<SearchRow>) -> Vec<Row> {
    rows.into_iter()
        .map(|r| Row {
            key: r.keys.first().cloned().unwrap_or_default(),
            key2: r.keys.get(1).cloned().unwrap_or_default(),
            clicks: r.clicks,
            impressions: r.impressions,
            ctr: r.ctr,
            position: r.position,
        })
        .collect()
}

/// 连接里存的 Google 凭据。
fn google_cred(conns: &ConnStore, conn_id: &str) -> Result<(String, google::Credential), String> {
    let c = conns.get(conn_id).map_err(|_| "站点的 Google 连接已被删除".to_string())?;
    if c.kind != "google" {
        return Err("数据源连接不是 Google".into());
    }
    let raw = crate::keychain::get_key(&c.id)?;
    Ok((c.id.clone(), google::parse_credential(&c.key_kind, &raw)?))
}

pub async fn google_token(conns: &ConnStore, conn_id: &str, scopes: &[&str]) -> Result<String, String> {
    let (id, cred) = google_cred(conns, conn_id)?;
    google::access_token(&id, &cred, scopes).await
}

/// 算「AI 带来的访问」的来源（GA4 的 sessionSource，部分匹配、不分大小写）
pub const AI_SOURCES: &str = "chatgpt|openai|perplexity|gemini\\.google|bard\\.google|copilot|claude\\.ai|deepseek|kimi|moonshot|doubao|yuanbao|tongyi|qianwen|metaso|you\\.com|phind|poe\\.com|meta\\.ai|grok|mistral";

/// GA4：近 180 天按天（全部站点列表要算近 90 天和前 90 天）、近 28 天（和前 28 天）按页面、来源、事件。
async fn fetch_ga4(token: &str, property: &str, dir: &Path) -> Result<(String, String), String> {
    let (start90, end) = (day(90), day(1));
    let start180 = day(180);
    let (start28, prev_start, prev_end) = (day(28), day(56), day(29));
    let gm = ["sessions", "totalUsers", "screenPageViews", "engagedSessions"];
    let to_ga = |rows: Vec<(Vec<String>, Vec<f64>)>| -> Vec<GaRow> {
        rows.into_iter()
            .map(|(d, m)| GaRow {
                key: d.first().cloned().unwrap_or_default(),
                key2: d.get(1).cloned().unwrap_or_default(),
                sessions: m.first().copied().unwrap_or(0.0),
                users: m.get(1).copied().unwrap_or(0.0),
                views: m.get(2).copied().unwrap_or(0.0),
                engaged: m.get(3).copied().unwrap_or(0.0),
            })
            .collect()
    };
    let mut daily = to_ga(google::ga4_report(token, property, &start180, &end, &["date"], &gm, 400, None).await?);
    for r in daily.iter_mut() {
        // GA4 的 date 是 YYYYMMDD
        if r.key.len() == 8 {
            r.key = format!("{}-{}-{}", &r.key[..4], &r.key[4..6], &r.key[6..]);
        }
    }
    write_json(dir, "ga4_daily.json", &daily)?;
    let pages = to_ga(google::ga4_report(token, property, &start28, &end, &["pagePath"], &["screenPageViews", "totalUsers", "sessions", "engagedSessions"], 1000, None).await?)
        .into_iter()
        .map(|r| GaRow { views: r.sessions, sessions: r.views, ..r })
        .collect::<Vec<_>>();
    write_json(dir, "ga4_pages.json", &pages)?;
    let prev = to_ga(google::ga4_report(token, property, &prev_start, &prev_end, &["pagePath"], &["screenPageViews", "totalUsers", "sessions", "engagedSessions"], 1000, None).await?)
        .into_iter()
        .map(|r| GaRow { views: r.sessions, sessions: r.views, ..r })
        .collect::<Vec<_>>();
    write_json(dir, "ga4_pages_prev.json", &prev)?;
    let sources = to_ga(google::ga4_report(token, property, &start28, &end, &["sessionSourceMedium"], &gm, 50, None).await?);
    write_json(dir, "ga4_sources.json", &sources)?;
    // AI 搜索 / 聊天带来的访问（ChatGPT、Perplexity、Gemini…）：按天、按来源，近 90 天。拉不到不影响别的
    let ai_filter = json!({ "filter": { "fieldName": "sessionSource", "stringFilter": { "matchType": "PARTIAL_REGEXP", "value": AI_SOURCES, "caseSensitive": false } } });
    if let Ok(rows) = google::ga4_report(token, property, &start90, &end, &["date", "sessionSource"], &["sessions", "totalUsers", "screenPageViews", "engagedSessions"], 2000, Some(ai_filter)).await {
        let mut ai = to_ga(rows);
        for r in ai.iter_mut() {
            if r.key.len() == 8 {
                r.key = format!("{}-{}-{}", &r.key[..4], &r.key[4..6], &r.key[6..]);
            }
        }
        write_json(dir, "ga4_ai.json", &ai)?;
    }
    // 事件：先按事件名，再按注册过的事件级自定义维度拆分（如 exchange / placement）。
    let standard = ["page_view", "session_start", "first_visit", "user_engagement", "scroll"];
    let mut events: Vec<EventRow> = google::ga4_report(token, property, &start28, &end, &["eventName"], &["eventCount", "totalUsers"], 100, None)
        .await?
        .into_iter()
        .map(|(d, m)| EventRow {
            event: d.first().cloned().unwrap_or_default(),
            dim: String::new(),
            value: String::new(),
            count: m.first().copied().unwrap_or(0.0),
            users: m.get(1).copied().unwrap_or(0.0),
        })
        .collect();
    let custom: Vec<&str> = events.iter().map(|e| e.event.as_str()).filter(|e| !standard.contains(e)).collect();
    if !custom.is_empty() {
        let filter = json!({ "notExpression": { "filter": { "fieldName": "eventName", "inListFilter": { "values": standard } } } });
        if let Ok(dims) = google::ga4_event_dimensions(token, property).await {
            for d in dims.iter().take(4) {
                let name = format!("customEvent:{d}");
                if let Ok(rows) = google::ga4_report(token, property, &start28, &end, &["eventName", &name], &["eventCount", "totalUsers"], 200, Some(filter.clone())).await {
                    events.extend(rows.into_iter().filter(|(dv, _)| dv.get(1).is_some_and(|v| !v.is_empty() && v != "(not set)")).map(|(dv, m)| EventRow {
                        event: dv[0].clone(),
                        dim: d.clone(),
                        value: dv[1].clone(),
                        count: m.first().copied().unwrap_or(0.0),
                        users: m.get(1).copied().unwrap_or(0.0),
                    }));
                }
            }
        }
    }
    write_json(dir, "ga4_events.json", &events)?;
    Ok((start90, end))
}

/// Search Console：近 180 天按天（全部站点列表要算近 90 天和前 90 天）；近 28 天与前 28 天的查询词、页面；查询词×页面；sitemap。
async fn fetch_gsc(token: &str, site: &str, dir: &Path) -> Result<(String, String), String> {
    let daily = search_rows(google::gsc_query(token, site, &day(183), &day(1), &["date"], 1000).await?);
    // 以最近一天有数据的日期为截止日。查询用的是 dataState=all，最近两天的数据还不完整：往前退两天，
    // 不然「近 28 天」末尾少算、「前 28 天」是完整的，页面会系统性地被判成下滑
    let end = daily.iter().map(|r| r.key.clone()).max().unwrap_or_else(|| day(3));
    let end_d = chrono::NaiveDate::parse_from_str(&end, "%Y-%m-%d").unwrap_or_else(|_| chrono::Utc::now().date_naive()) - chrono::Duration::days(2);
    let fmt = |d: chrono::NaiveDate| d.format("%Y-%m-%d").to_string();
    let start = fmt(end_d - chrono::Duration::days(27));
    let prev_end = fmt(end_d - chrono::Duration::days(28));
    let prev_start = fmt(end_d - chrono::Duration::days(55));
    write_json(dir, "gsc_daily.json", &daily)?;
    write_json(dir, "gsc_queries.json", &search_rows(google::gsc_query(token, site, &start, &end, &["query"], 5000).await?))?;
    write_json(dir, "gsc_queries_prev.json", &search_rows(google::gsc_query(token, site, &prev_start, &prev_end, &["query"], 5000).await?))?;
    write_json(dir, "gsc_pages.json", &search_rows(google::gsc_query(token, site, &start, &end, &["page"], 5000).await?))?;
    write_json(dir, "gsc_pages_prev.json", &search_rows(google::gsc_query(token, site, &prev_start, &prev_end, &["page"], 5000).await?))?;
    write_json(dir, "gsc_query_page.json", &search_rows(google::gsc_query(token, site, &start, &end, &["query", "page"], 5000).await?))?;
    if let Ok(sm) = google::gsc_sitemaps(token, site).await {
        write_json(dir, "gsc_sitemaps.json", &sm)?;
    }
    Ok((start, end))
}

/// Bing 的统计是按周给的行，按「最近 28 天」聚合（排名按展现量加权）。
fn aggregate_bing(rows: &[crate::bing::BingRow], from: &str) -> Vec<Row> {
    let mut m: HashMap<String, (f64, f64, f64)> = HashMap::new();
    for r in rows.iter().filter(|r| r.day.as_str() >= from && !r.key.is_empty()) {
        let e = m.entry(r.key.clone()).or_default();
        e.0 += r.clicks;
        e.1 += r.impressions;
        e.2 += r.position * r.impressions;
    }
    let mut out: Vec<Row> = m
        .into_iter()
        .map(|(key, (c, i, p))| Row {
            key,
            key2: String::new(),
            clicks: c,
            impressions: i,
            ctr: if i > 0.0 { c / i } else { 0.0 },
            position: if i > 0.0 { p / i } else { 0.0 },
        })
        .collect();
    out.sort_by(|a, b| b.impressions.total_cmp(&a.impressions));
    out
}

async fn fetch_bing(key: &str, site: &str, dir: &Path) -> Result<(String, String), String> {
    let traffic = crate::bing::traffic_stats(key, site).await?;
    let latest = traffic.iter().map(|r| r.day.clone()).max().unwrap_or_else(|| day(1));
    let latest_d = chrono::NaiveDate::parse_from_str(&latest, "%Y-%m-%d").unwrap_or_else(|_| chrono::Utc::now().date_naive());
    let from = (latest_d - chrono::Duration::days(27)).format("%Y-%m-%d").to_string();
    let daily: Vec<Row> = traffic
        .iter()
        .map(|r| Row { key: r.day.clone(), key2: String::new(), clicks: r.clicks, impressions: r.impressions, ctr: if r.impressions > 0.0 { r.clicks / r.impressions } else { 0.0 }, position: 0.0 })
        .collect();
    write_json(dir, "bing_daily.json", &daily)?;
    let q = crate::bing::query_stats(key, site).await?;
    write_json(dir, "bing_queries.json", &aggregate_bing(&q, &from))?;
    let p = crate::bing::page_stats(key, site).await?;
    write_json(dir, "bing_pages.json", &aggregate_bing(&p, &from))?;
    if let Ok(issues) = crate::bing::crawl_issues(key, site).await {
        write_json(dir, "bing_crawl.json", &issues)?;
    }
    if let Ok(n) = crate::bing::index_count(key, site).await {
        write_json(dir, "bing_index.json", &serde_json::json!({ "in_index": n }))?;
    }
    Ok((from, latest))
}

/// 这一类机会依赖的数据源这次拉成功了没有（没拉成功的，不凭旧数据判断问题好没好）
pub fn source_ok(kind: &str, meta: &CacheMeta) -> bool {
    let ok = |m: &SourceMeta| m.fetched_at > 0 && m.error.is_empty();
    match kind {
        "sitemap_error" | "low_ctr" | "striking" | "declining" | "content_gap" | "cannibal" | "stale" | "stale_year" => ok(&meta.gsc),
        "crawl_issue" => ok(&meta.bing),
        "low_rpm" => ok(&meta.ga4),
        _ => true,
    }
}

/// 现在还成立的机会：自动算出来的 + 速度、体检报告里还有问题的（手动交给 AI 的不受「低于 50 分 / 问题够多」的门槛限制）
pub fn live_opportunities(opps: &[crate::site_ops::Opportunity], site_dir: &Path) -> Vec<crate::site_ops::Opportunity> {
    let mut live = opps.to_vec();
    for key in crate::speed::live_keys(site_dir).into_iter().chain(crate::page_audit::tasks(site_dir).into_iter().map(|o| o.key)) {
        live.push(crate::site_ops::Opportunity { key, ..Default::default() });
    }
    live
}

/// 正在拉数据和排队等着拉的站点、批量健康检查在不在跑（全部站点列表上显示「同步中」，变了发 site-data-sync 事件）。
#[derive(Serialize, Clone, Default, Debug)]
pub struct SyncState {
    pub running: Vec<String>,
    pub queued: Vec<String>,
    pub health: bool,
}

static SYNC: std::sync::Mutex<SyncState> = std::sync::Mutex::new(SyncState { running: Vec::new(), queued: Vec::new(), health: false });

pub fn sync_state() -> SyncState {
    SYNC.lock().map(|s| s.clone()).unwrap_or_default()
}

fn update_sync(f: impl FnOnce(&mut SyncState)) {
    let snap = match SYNC.lock() {
        Ok(mut s) => {
            f(&mut s);
            s.clone()
        }
        Err(_) => return,
    };
    if let Some(app) = crate::APP.get() {
        use tauri::Emitter;
        let _ = app.emit("site-data-sync", snap);
    }
}

/// 批量同步开始前把要拉的站点都标成「排队中」
pub fn mark_queued(ids: &[String]) {
    update_sync(|s| {
        for id in ids {
            if !s.queued.contains(id) && !s.running.contains(id) {
                s.queued.push(id.clone());
            }
        }
    });
}

/// 排了队但最后没拉（站点被删了之类）
pub fn unqueue(id: &str) {
    update_sync(|s| s.queued.retain(|x| x != id));
}

pub fn set_health_running(on: bool) {
    update_sync(|s| s.health = on);
}

/// 拉数据期间站点算「同步中」，拉完（包括出错、中途返回）自动去掉
struct Syncing(String);
impl Syncing {
    fn start(id: &str) -> Self {
        update_sync(|s| {
            s.queued.retain(|x| x != id);
            s.running.push(id.to_string());
        });
        Syncing(id.to_string())
    }
}
impl Drop for Syncing {
    fn drop(&mut self) {
        let id = self.0.clone();
        update_sync(|s| {
            if let Some(i) = s.running.iter().position(|x| *x == id) {
                s.running.remove(i);
            }
        });
    }
}

async fn ga4_via(conns: &ConnStore, conn: &str, property: &str, dir: &Path) -> Result<(String, String), String> {
    let token = google_token(conns, conn, &google::read_scopes()).await?;
    fetch_ga4(&token, property, dir).await
}

async fn gsc_via(conns: &ConnStore, conn: &str, url: &str, dir: &Path) -> Result<(String, String), String> {
    let token = google_token(conns, conn, &google::read_scopes()).await?;
    fetch_gsc(&token, url, dir).await
}

/// Google 说这个账号没有这个资源的权限（`google::api` 的 403）：绑定可能已经过期，值得按域名重新找。
fn no_access(e: &str) -> bool {
    e.starts_with("没有权限")
}

/// 所有 Google 账号，站点现在用的排最前。
fn google_conns(conns: &ConnStore, first: &str) -> Vec<crate::conns::Connection> {
    let mut list: Vec<_> = conns.list().into_iter().filter(|c| c.kind == "google").collect();
    list.sort_by_key(|c| c.id != first);
    list
}

/// 按域名挑 Search Console 资源，跳过刚报没权限的那个。
fn pick_gsc(sites: &[google::GscSite], domain: &str, skip: Option<&str>) -> Option<String> {
    let rest: Vec<google::GscSite> = sites.iter().filter(|s| Some(s.url.as_str()) != skip).cloned().collect();
    google::match_gsc_site(&rest, domain).map(|g| g.url.clone())
}

/// 绑定的资源没权限了（换了账号验证、改成了网域资源、账号被移除…）：到所有 Google 账号里按域名重新找。
async fn find_gsc_binding(conns: &ConnStore, domain: &str, cur_conn: &str, cur_url: &str) -> Option<(String, String)> {
    if domain.is_empty() {
        return None;
    }
    for c in google_conns(conns, cur_conn) {
        let Ok(token) = google_token(conns, &c.id, &google::read_scopes()).await else { continue };
        let Ok(sites) = google::list_gsc_sites(&token).await else { continue };
        if let Some(url) = pick_gsc(&sites, domain, (c.id == cur_conn).then_some(cur_url)) {
            return Some((c.id, url));
        }
    }
    None
}

/// 同上，GA4：按媒体资源里网页数据流的网址匹配域名，名字里带域名的媒体资源先查。
async fn find_ga4_binding(conns: &ConnStore, domain: &str, cur_conn: &str, cur_prop: &str) -> Option<(String, String, String)> {
    if domain.is_empty() {
        return None;
    }
    let label = domain.trim_start_matches("www.").split('.').next().unwrap_or_default().to_lowercase();
    for c in google_conns(conns, cur_conn) {
        let Ok(token) = google_token(conns, &c.id, &google::read_scopes()).await else { continue };
        let Ok(mut props) = google::list_ga4_properties(&token).await else { continue };
        props.sort_by_key(|p| !p.name.to_lowercase().contains(&label));
        for p in props {
            if c.id == cur_conn && p.property == cur_prop {
                continue;
            }
            let hosts = google::ga4_stream_hosts(&token, &p.property).await.unwrap_or_default();
            if hosts.iter().any(|h| google::same_host(h, domain)) {
                return Some((c.id, p.property, p.name));
            }
        }
    }
    None
}

/// 拉取站点配置的全部数据源（各自独立，一个失败不影响其它）。`force=false` 时跳过 6 小时内拉过的源。
pub async fn refresh(site: &Site, conns: &ConnStore, store: &SiteStore, site_dir: &Path, force: bool) -> CacheMeta {
    let dir = cache_dir(site_dir);
    let mut meta = load_meta(site_dir);
    let due = |m: &SourceMeta| force || !m.error.is_empty() || now().saturating_sub(m.fetched_at) > STALE_SECS;
    let d = &site.data;
    // 真要联网拉才算「同步中」（缓存还新的话一闪而过，不显示）
    let any_due = (!d.ga4_property.is_empty() && due(&meta.ga4)) || (!d.gsc_site.is_empty() && due(&meta.gsc)) || (!d.bing_site.is_empty() && due(&meta.bing));
    let _syncing = any_due.then(|| Syncing::start(&site.id));
    if !any_due {
        unqueue(&site.id);
    }
    // GA4 和 Search Console 各用各的连接（可能是两个 Google 账号）；同一条连接时令牌有缓存，不会重复换取。
    if !d.google_conn_id.is_empty() && !d.ga4_property.is_empty() && due(&meta.ga4) {
        let mut res = ga4_via(conns, &d.google_conn_id, &d.ga4_property, &dir).await;
        if res.as_ref().is_err_and(|e| no_access(e)) {
            if let Some((cid, prop, name)) = find_ga4_binding(conns, &site.domain, &d.google_conn_id, &d.ga4_property).await {
                res = ga4_via(conns, &cid, &prop, &dir).await;
                if res.is_ok() {
                    let _ = store.update(&site.id, now(), |x| {
                        // 换 GA4 账号时，原来跟着旧账号的 Search Console 改记成它自己的账号
                        if x.data.gsc_conn_id.is_empty() && !x.data.gsc_site.is_empty() && x.data.google_conn_id != cid {
                            x.data.gsc_conn_id = x.data.google_conn_id.clone();
                        }
                        x.data.google_conn_id = cid;
                        x.data.ga4_property = prop;
                        x.data.ga4_property_name = name;
                        Ok(())
                    });
                }
            }
        }
        meta.ga4 = match res {
            Ok((start, end)) => SourceMeta { fetched_at: now(), start, end, error: String::new() },
            Err(e) => SourceMeta { error: e, ..meta.ga4.clone() },
        };
    }
    if !d.gsc_conn().is_empty() && !d.gsc_site.is_empty() && due(&meta.gsc) {
        let mut res = gsc_via(conns, d.gsc_conn(), &d.gsc_site, &dir).await;
        if res.as_ref().is_err_and(|e| no_access(e)) {
            if let Some((cid, url)) = find_gsc_binding(conns, &site.domain, d.gsc_conn(), &d.gsc_site).await {
                res = gsc_via(conns, &cid, &url, &dir).await;
                if res.is_ok() {
                    let _ = store.update(&site.id, now(), |x| {
                        if x.data.google_conn_id.is_empty() { x.data.google_conn_id = cid.clone(); }
                        x.data.gsc_conn_id = if x.data.google_conn_id == cid { String::new() } else { cid };
                        x.data.gsc_site = url;
                        Ok(())
                    });
                }
            }
        }
        meta.gsc = match res {
            Ok((start, end)) => SourceMeta { fetched_at: now(), start, end, error: String::new() },
            Err(e) => SourceMeta { error: e, ..meta.gsc.clone() },
        };
    }
    if !d.bing_conn_id.is_empty() && !d.bing_site.is_empty() && due(&meta.bing) {
        meta.bing = match conns.get(&d.bing_conn_id).and_then(|c| crate::keychain::get_key(&c.id)) {
            Ok(key) => match fetch_bing(&key, &d.bing_site, &dir).await {
                Ok((start, end)) => SourceMeta { fetched_at: now(), start, end, error: String::new() },
                Err(e) => SourceMeta { error: e, ..meta.bing.clone() },
            },
            Err(_) => SourceMeta { error: "站点的 Bing 连接已被删除".into(), ..meta.bing.clone() },
        };
    }
    let _ = write_json(&dir, "meta.json", &meta);
    // 机会列表跟着数据一起重算，模型的查询工具和看板读的是同一份。
    let opps = ranked_opportunities(site, site_dir).await;
    let _ = write_json(&dir, "opportunities.json", &opps);
    // 已经好了的问题（sitemap 不报错了、页面不慢了、点击率回来了…）还排在待处理里：自动关掉；这次没拉成功的数据源不算数
    let _ = crate::site_ops::close_resolved(site_dir, &live_opportunities(&opps, site_dir), now(), |kind| source_ok(kind, &meta));
    let _ = write_json(&dir, "summary.json", &compute_summary(site_dir));
    meta
}

/// 按规则算出机会，接上了判断模型（Jev）就再按「和站点定位的相关度」调整排序（打过的有缓存，不重复问）。
/// 只调顺序、标出相关度，不改变交不交给 AI——那是托管方案的规则（skip_offtopic），见 site_runner::scan。
pub async fn ranked_opportunities(site: &Site, site_dir: &Path) -> Vec<crate::site_ops::Opportunity> {
    let mut opps = crate::site_ops::opportunities(site, site_dir);
    if let Some(jev) = crate::typesafe::client() {
        crate::jev_checks::rank_opportunities(&jev, site, site_dir, &mut opps).await;
    }
    opps
}

/// 全部站点总览用的一行摘要（每次拉数据后写一份小文件，站点再多也只读小文件）。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Summary {
    pub gsc_clicks: f64,
    pub gsc_clicks_prev: f64,
    pub gsc_impressions: f64,
    pub gsc_position: f64,
    pub ga4_views: f64,
    pub ga4_views_prev: f64,
    pub bing_clicks: f64,
    pub opportunities: usize,
    pub data_end: String,
    pub fetched_at: u64,
    pub errors: Vec<String>,
    /// 摘要格式版本：旧版缓存（没有下面这些字段）读到时重算一次
    #[serde(default)]
    pub v: u32,
    /// GA4 近 28 天用户（按天加总）和前 28 天
    #[serde(default)]
    pub users: f64,
    #[serde(default)]
    pub users_prev: f64,
    /// 近 28 天每天的会话数（趋势小图用，旧的在前）
    #[serde(default)]
    pub sessions_trend: Vec<f64>,
    /// 搜索：Google + Bing 近 28 天点击、前 28 天点击、展示
    #[serde(default)]
    pub search_clicks: f64,
    #[serde(default)]
    pub search_clicks_prev: f64,
    #[serde(default)]
    pub search_impressions: f64,
    /// Google 近 28 天有展示的页面数（Search Console 的站点地图接口不再给收录数，用它近似「已编入」）
    #[serde(default)]
    pub google_pages: usize,
    /// 站点地图里提交的网址数（不算站点地图索引文件本身）
    #[serde(default)]
    pub sitemap_urls: f64,
    /// Bing 索引里的网址数（GetCrawlStats 最新一天的 InIndex）
    #[serde(default)]
    pub bing_indexed: f64,
    /// AI 搜索 / 聊天带来的会话：近 28 天、前 28 天
    #[serde(default)]
    pub ai_sessions: f64,
    #[serde(default)]
    pub ai_sessions_prev: f64,
    /// 近 7 / 14 / 28 / 90 天（和前一段同样长的时间比）：全部站点列表可以切换统计时间段
    #[serde(default)]
    pub wins: std::collections::BTreeMap<u32, Win>,
    /// 各数据源按天数据的最新一天（YYYY-MM-DD；近 N 天就从这天往前算）
    #[serde(default)]
    pub ga4_end: String,
    #[serde(default)]
    pub gsc_end: String,
    #[serde(default)]
    pub bing_end: String,
    /// Search Console 有几天的数据（新验证的站要等几天才有）
    #[serde(default)]
    pub gsc_days: usize,
}

/// 一个时间段的用户、搜索点击、展示、每天会话（和前一段同样长的时间比；按天数据不够覆盖前一段时前一段记 0，界面上就不显示涨跌）
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Win {
    pub users: f64,
    pub users_prev: f64,
    pub clicks: f64,
    pub clicks_prev: f64,
    /// 算涨跌用的本段点击：只算前一段也有数据的来源（Bing 只给一个多月的按天数据，比 28 / 90 天时只拿 Google 比）
    #[serde(default)]
    pub clicks_base: f64,
    pub impressions: f64,
    #[serde(default)]
    pub trend: Vec<f64>,
}

/// 全部站点列表能选的统计时间段（天）
pub const WIN_DAYS: [u32; 4] = [7, 14, 28, 90];

pub const SUMMARY_V: u32 = 6;

/// 按天数据够不够覆盖「前一段」：最早一天要早于最新一天往前 2×days−1 天
fn covers<T>(rows: &[T], date: impl Fn(&T) -> &str, days: i64) -> bool {
    let ds: Vec<_> = rows.iter().filter_map(|r| chrono::NaiveDate::parse_from_str(date(r), "%Y-%m-%d").ok()).collect();
    match (ds.iter().min(), ds.iter().max()) {
        (Some(a), Some(b)) => (*b - *a).num_days() >= days * 2 - 1,
        _ => false,
    }
}

/// 按日期的行里，以最新一天为终点的近 days 天和再往前 days 天的合计
fn windows<T>(rows: &[T], date: impl Fn(&T) -> &str, val: impl Fn(&T) -> f64, days: i64) -> (f64, f64, Vec<(String, f64)>) {
    let Some(end) = rows.iter().map(|r| date(r)).max().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) else { return (0.0, 0.0, Vec::new()) };
    let (mut cur, mut prev, mut series) = (0.0, 0.0, Vec::new());
    for r in rows {
        let Ok(d) = chrono::NaiveDate::parse_from_str(date(r), "%Y-%m-%d") else { continue };
        let ago = (end - d).num_days();
        if (0..days).contains(&ago) {
            cur += val(r);
            series.push((date(r).to_string(), val(r)));
        } else if (days..days * 2).contains(&ago) {
            prev += val(r);
        }
    }
    series.sort_by(|a, b| a.0.cmp(&b.0));
    (cur, prev, series)
}

pub fn compute_summary(site_dir: &Path) -> Summary {
    let dir = cache_dir(site_dir);
    let meta = load_meta(site_dir);
    let gq: Vec<Row> = read_json(&dir, "gsc_queries.json");
    let gqp: Vec<Row> = read_json(&dir, "gsc_queries_prev.json");
    let ga: Vec<GaRow> = read_json(&dir, "ga4_pages.json");
    let gap: Vec<GaRow> = read_json(&dir, "ga4_pages_prev.json");
    let bq: Vec<Row> = read_json(&dir, "bing_queries.json");
    let opps: Vec<crate::site_ops::Opportunity> = read_json(&dir, "opportunities.json");
    let t = totals(&gq);
    let gad: Vec<GaRow> = read_json(&dir, "ga4_daily.json");
    let gsd: Vec<Row> = read_json(&dir, "gsc_daily.json");
    let bsd: Vec<Row> = read_json(&dir, "bing_daily.json");
    let gp: Vec<Row> = read_json(&dir, "gsc_pages.json");
    let sitemaps: Vec<serde_json::Value> = read_json(&dir, "gsc_sitemaps.json");
    let users = windows(&gad, |r| &r.key, |r| r.users, 28);
    let ai_rows: Vec<GaRow> = read_json(&dir, "ga4_ai.json");
    let ai = windows(&ai_rows, |r| &r.key, |r| r.sessions, 28);
    let sessions = windows(&gad, |r| &r.key, |r| r.sessions, 28);
    let g = windows(&gsd, |r| &r.key, |r| r.clicks, 28);
    let b = windows(&bsd, |r| &r.key, |r| r.clicks, 28);
    let gi = windows(&gsd, |r| &r.key, |r| r.impressions, 28);
    let bi = windows(&bsd, |r| &r.key, |r| r.impressions, 28);
    let errors = [("GA4", &meta.ga4.error), ("Search Console", &meta.gsc.error), ("Bing", &meta.bing.error)]
        .iter()
        .filter(|(_, e)| !e.is_empty())
        .map(|(l, e)| format!("{l}：{e}"))
        .collect();
    Summary {
        gsc_clicks: t.clicks,
        gsc_clicks_prev: totals(&gqp).clicks,
        gsc_impressions: t.impressions,
        gsc_position: t.position,
        ga4_views: ga.iter().map(|r| r.views).sum(),
        ga4_views_prev: gap.iter().map(|r| r.views).sum(),
        bing_clicks: bq.iter().map(|r| r.clicks).sum(),
        opportunities: opps.len(),
        data_end: if meta.gsc.end.is_empty() { meta.ga4.end.clone() } else { meta.gsc.end.clone() },
        fetched_at: meta.gsc.fetched_at.max(meta.ga4.fetched_at).max(meta.bing.fetched_at),
        errors,
        v: SUMMARY_V,
        users: users.0,
        users_prev: users.1,
        sessions_trend: sessions.2.into_iter().map(|(_, v)| v).collect(),
        search_clicks: g.0 + b.0,
        search_clicks_prev: g.1 + b.1,
        search_impressions: gi.0 + bi.0,
        google_pages: gp.iter().filter(|r| r.impressions > 0.0).count(),
        sitemap_urls: sitemaps
            .iter()
            .filter(|m| !m["isSitemapsIndex"].as_bool().unwrap_or(false))
            .flat_map(|m| m["contents"].as_array().cloned().unwrap_or_default())
            .map(|c| c["submitted"].as_str().and_then(|n| n.parse::<f64>().ok()).or_else(|| c["submitted"].as_f64()).unwrap_or(0.0))
            .sum(),
        bing_indexed: read_json::<serde_json::Value>(&dir, "bing_index.json")["in_index"].as_f64().unwrap_or(0.0),
        ai_sessions: ai.0,
        ai_sessions_prev: ai.1,
        wins: WIN_DAYS
            .iter()
            .map(|&n| {
                let d = n as i64;
                let u = windows(&gad, |r| &r.key, |r| r.users, d);
                let s = windows(&gad, |r| &r.key, |r| r.sessions, d);
                let (g, b) = (windows(&gsd, |r| &r.key, |r| r.clicks, d), windows(&bsd, |r| &r.key, |r| r.clicks, d));
                let (gi, bi) = (windows(&gsd, |r| &r.key, |r| r.impressions, d), windows(&bsd, |r| &r.key, |r| r.impressions, d));
                // 前一段没被按天数据盖住的来源不参与比较（刚改成拉 180 天、Bing 只给一个多月），免得拿半截数据算出假涨跌
                let (gc, bc) = (covers(&gsd, |r| &r.key, d), covers(&bsd, |r| &r.key, d));
                let (base, prev) = (if gc { g.0 } else { 0.0 } + if bc { b.0 } else { 0.0 }, if gc { g.1 } else { 0.0 } + if bc { b.1 } else { 0.0 });
                let w = Win {
                    users: u.0,
                    users_prev: if covers(&gad, |r| &r.key, d) { u.1 } else { 0.0 },
                    clicks: g.0 + b.0,
                    clicks_prev: prev,
                    clicks_base: base,
                    impressions: gi.0 + bi.0,
                    trend: s.2.into_iter().map(|(_, v)| v).collect(),
                };
                (n, w)
            })
            .collect(),
        ga4_end: gad.iter().map(|r| r.key.clone()).max().unwrap_or_default(),
        gsc_end: gsd.iter().map(|r| r.key.clone()).max().unwrap_or_default(),
        bing_end: bsd.iter().map(|r| r.key.clone()).max().unwrap_or_default(),
        gsc_days: gsd.len(),
    }
}

/// 读摘要；还没有（旧缓存）就现算一份存下。
pub fn summary(site_dir: &Path) -> Summary {
    let dir = cache_dir(site_dir);
    if dir.join("summary.json").exists() {
        let s: Summary = read_json(&dir, "summary.json");
        // 旧版摘要缺新字段：有数据就按缓存重算一份（不联网）
        if s.v >= SUMMARY_V || !has_any_data(site_dir) {
            return s;
        }
        let s = compute_summary(site_dir);
        let _ = write_json(&dir, "summary.json", &s);
        return s;
    }
    if !has_any_data(site_dir) {
        return Summary::default();
    }
    let s = compute_summary(site_dir);
    let _ = write_json(&dir, "summary.json", &s);
    s
}

pub fn has_any_data(site_dir: &Path) -> bool {
    let m = load_meta(site_dir);
    m.ga4.fetched_at > 0 || m.gsc.fetched_at > 0 || m.bing.fetched_at > 0
}

// ---------------------------------------------------------------------------
// 看板汇总
// ---------------------------------------------------------------------------

#[derive(Serialize, Default, Clone, Debug, PartialEq)]
pub struct Totals {
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

fn totals(rows: &[Row]) -> Totals {
    let c: f64 = rows.iter().map(|r| r.clicks).sum();
    let i: f64 = rows.iter().map(|r| r.impressions).sum();
    let p: f64 = rows.iter().map(|r| r.position * r.impressions).sum();
    Totals { clicks: c, impressions: i, ctr: if i > 0.0 { c / i } else { 0.0 }, position: if i > 0.0 { p / i } else { 0.0 } }
}

#[derive(Serialize, Default)]
pub struct GaTotals {
    pub sessions: f64,
    pub users: f64,
    pub views: f64,
    pub engaged: f64,
}

fn ga_totals(rows: &[GaRow]) -> GaTotals {
    GaTotals {
        sessions: rows.iter().map(|r| r.sessions).sum(),
        users: rows.iter().map(|r| r.users).sum(),
        views: rows.iter().map(|r| r.views).sum(),
        engaged: rows.iter().map(|r| r.engaged).sum(),
    }
}

/// 页面的前后对比（查询词、页面表格里的「变化」列用）。
#[derive(Serialize, Default, Clone)]
pub struct Compared {
    #[serde(flatten)]
    pub row: Row,
    pub prev_clicks: f64,
    pub prev_impressions: f64,
    pub prev_position: f64,
}

fn compare(cur: Vec<Row>, prev: &[Row], limit: usize) -> Vec<Compared> {
    let pm: HashMap<&str, &Row> = prev.iter().map(|r| (r.key.as_str(), r)).collect();
    cur.into_iter()
        .take(limit)
        .map(|r| {
            let p = pm.get(r.key.as_str());
            Compared {
                prev_clicks: p.map(|p| p.clicks).unwrap_or(0.0),
                prev_impressions: p.map(|p| p.impressions).unwrap_or(0.0),
                prev_position: p.map(|p| p.position).unwrap_or(0.0),
                row: r,
            }
        })
        .collect()
}

#[derive(Serialize, Default)]
pub struct Dashboard {
    pub meta: CacheMeta,
    pub gsc_daily: Vec<Row>,
    pub gsc_totals: Totals,
    pub gsc_prev_totals: Totals,
    pub gsc_queries: Vec<Compared>,
    pub gsc_pages: Vec<Compared>,
    pub gsc_sitemaps: Value,
    pub ga4_daily: Vec<GaRow>,
    pub ga4_totals: GaTotals,
    pub ga4_prev_views: f64,
    pub ga4_pages: Vec<GaRow>,
    pub ga4_sources: Vec<GaRow>,
    pub ga4_events: Vec<EventRow>,
    pub bing_daily: Vec<Row>,
    pub bing_totals: Totals,
    pub bing_queries: Vec<Row>,
    pub bing_pages: Vec<Row>,
    pub bing_crawl: Value,
    pub opportunities: Vec<crate::site_ops::Opportunity>,
    /// AI 带来的访问：按天 × 来源（近 90 天）
    pub ga4_ai: Vec<GaRow>,
}

fn sort_rows(mut v: Vec<Row>) -> Vec<Row> {
    v.sort_by(|a, b| b.clicks.total_cmp(&a.clicks).then(b.impressions.total_cmp(&a.impressions)));
    v
}

/// 站点数据页的图表只看近 90 天（按天数据存了 180 天，给全部站点列表算前 90 天用）
fn last_days<T>(rows: Vec<T>, date: impl Fn(&T) -> &str, days: i64) -> Vec<T> {
    let Some(end) = rows.iter().filter_map(|r| chrono::NaiveDate::parse_from_str(date(r), "%Y-%m-%d").ok()).max() else { return rows };
    rows.into_iter().filter(|r| chrono::NaiveDate::parse_from_str(date(r), "%Y-%m-%d").map(|d| (end - d).num_days() < days).unwrap_or(true)).collect()
}

pub fn dashboard(site_dir: &Path) -> Dashboard {
    let dir = cache_dir(site_dir);
    let gq: Vec<Row> = read_json(&dir, "gsc_queries.json");
    let gqp: Vec<Row> = read_json(&dir, "gsc_queries_prev.json");
    let gp: Vec<Row> = read_json(&dir, "gsc_pages.json");
    let gpp: Vec<Row> = read_json(&dir, "gsc_pages_prev.json");
    let ga_pages: Vec<GaRow> = read_json(&dir, "ga4_pages.json");
    let ga_prev: Vec<GaRow> = read_json(&dir, "ga4_pages_prev.json");
    let bq: Vec<Row> = read_json(&dir, "bing_queries.json");
    let bp: Vec<Row> = read_json(&dir, "bing_pages.json");
    let mut ga_pages_sorted = ga_pages.clone();
    ga_pages_sorted.sort_by(|a, b| b.views.total_cmp(&a.views));
    Dashboard {
        meta: load_meta(site_dir),
        gsc_daily: last_days(read_json(&dir, "gsc_daily.json"), |r: &Row| &r.key, 93),
        gsc_totals: totals(&gq),
        gsc_prev_totals: totals(&gqp),
        gsc_queries: compare(sort_rows(gq), &gqp, 50),
        gsc_pages: compare(sort_rows(gp), &gpp, 50),
        gsc_sitemaps: read_json(&dir, "gsc_sitemaps.json"),
        ga4_daily: last_days(read_json(&dir, "ga4_daily.json"), |r: &GaRow| &r.key, 90),
        ga4_totals: ga_totals(&ga_pages),
        ga4_prev_views: ga_prev.iter().map(|r| r.views).sum(),
        ga4_pages: ga_pages_sorted.into_iter().take(50).collect(),
        ga4_sources: read_json(&dir, "ga4_sources.json"),
        ga4_events: read_json(&dir, "ga4_events.json"),
        bing_daily: read_json(&dir, "bing_daily.json"),
        bing_totals: totals(&bq),
        bing_queries: sort_rows(bq).into_iter().take(50).collect(),
        bing_pages: sort_rows(bp).into_iter().take(50).collect(),
        bing_crawl: read_json(&dir, "bing_crawl.json"),
        opportunities: read_json(&dir, "opportunities.json"),
        ga4_ai: read_json(&dir, "ga4_ai.json"),
    }
}

/// 站点会话里「数据查询工具」的说明；站点还没有任何缓存数据时返回空串（不白占提示词）。
pub fn tool_prompt(data_dir: &Path, site_dir: &Path) -> String {
    if !has_any_data(site_dir) {
        return String::new();
    }
    let js = crate::tools::site_data_js_path(data_dir);
    let cache = cache_dir(site_dir);
    let m = load_meta(site_dir);
    let mut have = Vec::new();
    if m.gsc.fetched_at > 0 {
        have.push(format!("Search Console（{} ~ {}）", m.gsc.start, m.gsc.end));
    }
    if m.ga4.fetched_at > 0 {
        have.push(format!("GA4（截至 {}）", m.ga4.end));
    }
    if m.bing.fetched_at > 0 {
        have.push(format!("Bing（截至 {}）", m.bing.end));
    }
    format!(
        "【站点数据（只读）】Pilot 已把这个站点的数据拉到本地：{have}。用随附工具查询，输出 JSON：\n\
`node \"{js}\" \"{cache}\" <命令> [选项]`\n\
命令：`summary`（总览）、`queries`（查询词）、`pages`（页面）、`query-pages`（查询词×页面）、`events`（GA4 事件）、`daily`（按天趋势）、`opportunities`（Pilot 按规则算出的机会）、`sitemaps`。\n\
常用选项：`--source gsc|bing|ga4`、`--min-impressions N`、`--max-ctr 0.02`、`--min-position 8 --max-position 20`、`--page <网址片段>`、`--query <词片段>`、`--sort clicks|impressions|ctr|position`、`--limit 50`、`--prev`（前 28 天，用来对比）。\n\
例：最近 28 天展现超过 500、点击率低于 2% 的查询词：`queries --min-impressions 500 --max-ctr 0.02`。\n\
如果你是 Pilot 的 API 执行器（有 site_data 工具），直接调用 site_data：cache 填 `{cache}`，command 填上面的命令，选项写进 options（如 {{\"min-impressions\":500,\"max-ctr\":0.02}}）；它只读、任何权限档位都能用，不需要 run_command。\n\
数据只用来判断改什么；不要把具体流量数字写进代码、提交说明或 PR 描述。",
        have = have.join("、"),
        js = js.to_string_lossy(),
        cache = cache.to_string_lossy(),
    )
}

#[cfg(test)]
mod rebind_tests {
    use super::*;
    use crate::google::GscSite;
    #[test]
    fn a_stale_binding_is_replaced_by_what_the_account_can_really_see() {
        let g = |u: &str| GscSite { url: u.into(), permission: "siteOwner".into() };
        // 还是旧地址：不算找到（不然会一直拿同一个资源重试）
        assert_eq!(pick_gsc(&[g("https://bgvar.com/")], "bgvar.com", Some("https://bgvar.com/")), None);
        // 同账号改成了网域资源
        let both = [g("https://bgvar.com/"), g("sc-domain:bgvar.com")];
        assert_eq!(pick_gsc(&both, "bgvar.com", Some("https://bgvar.com/")).as_deref(), Some("sc-domain:bgvar.com"));
        // 另一个账号下同样的地址（不跳过）
        assert_eq!(pick_gsc(&[g("https://bgvar.com/")], "bgvar.com", None).as_deref(), Some("https://bgvar.com/"));
        // 别的站点的资源不会被认错
        assert_eq!(pick_gsc(&[g("sc-domain:other.com")], "bgvar.com", None), None);
    }
    #[test]
    fn only_a_403_counts_as_a_stale_binding() {
        assert!(no_access("没有权限：User does not have sufficient permission for site"));
        assert!(!no_access("对应的 Google API 还没启用：x"));
        assert!(!no_access("Google 凭据无效或已过期"));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_split_last_28_days_from_the_28_before() {
        let rows: Vec<(String, f64)> = (0..60).map(|i| ((chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap() + chrono::Duration::days(i)).format("%Y-%m-%d").to_string(), 1.0)).collect();
        let (cur, prev, series) = windows(&rows, |r| &r.0, |r| r.1, 28);
        assert_eq!((cur, prev, series.len()), (28.0, 28.0, 28));
        assert_eq!(series.last().unwrap().0, "2026-10-30");
        // 60 天的数据够比 28 天，不够比 90 天
        assert!(covers(&rows, |r| &r.0, 28) && !covers(&rows, |r| &r.0, 90));
        // 近 7 天：从同一个最新日往前 7 天，前一段再往前 7 天
        let (cur, prev, series) = windows(&rows, |r| &r.0, |r| r.1, 7);
        assert_eq!((cur, prev, series.len(), series[0].0.as_str()), (7.0, 7.0, 7, "2026-10-24"));
        assert_eq!(windows(&Vec::<(String, f64)>::new(), |r| &r.0, |r| r.1, 28).0, 0.0);
    }

    use super::*;

    #[test]
    fn bing_rows_aggregate_over_the_window() {
        let rows = vec![
            crate::bing::BingRow { key: "a".into(), day: "2026-10-01".into(), clicks: 1.0, impressions: 10.0, position: 4.0 },
            crate::bing::BingRow { key: "a".into(), day: "2026-10-08".into(), clicks: 3.0, impressions: 30.0, position: 8.0 },
            crate::bing::BingRow { key: "a".into(), day: "2026-08-01".into(), clicks: 99.0, impressions: 99.0, position: 1.0 },
            crate::bing::BingRow { key: "b".into(), day: "2026-10-08".into(), clicks: 0.0, impressions: 5.0, position: 20.0 },
        ];
        let out = aggregate_bing(&rows, "2026-09-20");
        assert_eq!(out[0].key, "a");
        assert_eq!(out[0].clicks, 4.0);
        assert_eq!(out[0].impressions, 40.0);
        assert!((out[0].position - 7.0).abs() < 1e-9);
        assert!((out[0].ctr - 0.1).abs() < 1e-9);
        assert_eq!(out[1].key, "b");
    }

    #[test]
    fn totals_weight_position_by_impressions() {
        let t = totals(&[
            Row { key: "a".into(), clicks: 10.0, impressions: 100.0, position: 2.0, ..Default::default() },
            Row { key: "b".into(), clicks: 0.0, impressions: 300.0, position: 10.0, ..Default::default() },
        ]);
        assert_eq!(t.clicks, 10.0);
        assert!((t.ctr - 0.025).abs() < 1e-9);
        assert!((t.position - 8.0).abs() < 1e-9);
    }

    #[test]
    fn dashboard_reads_an_empty_cache() {
        let t = tempfile::tempdir().unwrap();
        let d = dashboard(t.path());
        assert!(d.gsc_queries.is_empty());
        assert!(!has_any_data(t.path()));
        assert_eq!(tool_prompt(t.path(), t.path()), "");
    }
}
