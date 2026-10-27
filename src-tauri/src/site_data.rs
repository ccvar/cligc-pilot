//! 站点数据：GA4 / Search Console / Bing 的拉取、本地缓存与看板汇总。
//! 缓存放在 `<站点数据>/cache/`（JSON，不进代码仓库）。模型通过随附的 site-data.js 只读查询这些文件，
//! 拿不到任何数据源凭据。Search Console 有 2–3 天延迟，统一以「最近有数据的那天」为截止日。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::conns::ConnStore;
use crate::google::{self, SearchRow};
use crate::sites::Site;

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

/// GA4：近 90 天按天、近 28 天（和前 28 天）按页面、来源、事件。
async fn fetch_ga4(token: &str, property: &str, dir: &Path) -> Result<(String, String), String> {
    let (start90, end) = (day(90), day(1));
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
    let mut daily = to_ga(google::ga4_report(token, property, &start90, &end, &["date"], &gm, 400, None).await?);
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

/// Search Console：近 90 天按天；近 28 天与前 28 天的查询词、页面；查询词×页面；sitemap。
async fn fetch_gsc(token: &str, site: &str, dir: &Path) -> Result<(String, String), String> {
    let daily = search_rows(google::gsc_query(token, site, &day(93), &day(1), &["date"], 1000).await?);
    // 以最近一天有数据的日期为截止日（通常比今天晚 2–3 天）。
    let end = daily.iter().map(|r| r.key.clone()).max().unwrap_or_else(|| day(3));
    let end_d = chrono::NaiveDate::parse_from_str(&end, "%Y-%m-%d").unwrap_or_else(|_| chrono::Utc::now().date_naive());
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

/// 拉取站点配置的全部数据源（各自独立，一个失败不影响其它）。`force=false` 时跳过 6 小时内拉过的源。
pub async fn refresh(site: &Site, conns: &ConnStore, site_dir: &Path, force: bool) -> CacheMeta {
    let dir = cache_dir(site_dir);
    let mut meta = load_meta(site_dir);
    let due = |m: &SourceMeta| force || !m.error.is_empty() || now().saturating_sub(m.fetched_at) > STALE_SECS;
    let d = &site.data;
    // GA4 和 Search Console 各用各的连接（可能是两个 Google 账号）；同一条连接时令牌有缓存，不会重复换取。
    if !d.google_conn_id.is_empty() && !d.ga4_property.is_empty() && due(&meta.ga4) {
        meta.ga4 = match google_token(conns, &d.google_conn_id, &google::read_scopes()).await {
            Ok(token) => match fetch_ga4(&token, &d.ga4_property, &dir).await {
                Ok((start, end)) => SourceMeta { fetched_at: now(), start, end, error: String::new() },
                Err(e) => SourceMeta { error: e, ..meta.ga4.clone() },
            },
            Err(e) => SourceMeta { error: e, ..meta.ga4.clone() },
        };
    }
    if !d.gsc_conn().is_empty() && !d.gsc_site.is_empty() && due(&meta.gsc) {
        meta.gsc = match google_token(conns, d.gsc_conn(), &google::read_scopes()).await {
            Ok(token) => match fetch_gsc(&token, &d.gsc_site, &dir).await {
                Ok((start, end)) => SourceMeta { fetched_at: now(), start, end, error: String::new() },
                Err(e) => SourceMeta { error: e, ..meta.gsc.clone() },
            },
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
}

pub const SUMMARY_V: u32 = 2;

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
}

fn sort_rows(mut v: Vec<Row>) -> Vec<Row> {
    v.sort_by(|a, b| b.clicks.total_cmp(&a.clicks).then(b.impressions.total_cmp(&a.impressions)));
    v
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
        gsc_daily: read_json(&dir, "gsc_daily.json"),
        gsc_totals: totals(&gq),
        gsc_prev_totals: totals(&gqp),
        gsc_queries: compare(sort_rows(gq), &gqp, 50),
        gsc_pages: compare(sort_rows(gp), &gpp, 50),
        gsc_sitemaps: read_json(&dir, "gsc_sitemaps.json"),
        ga4_daily: read_json(&dir, "ga4_daily.json"),
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
mod tests {
    #[test]
    fn windows_split_last_28_days_from_the_28_before() {
        let rows: Vec<(String, f64)> = (0..60).map(|i| ((chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap() + chrono::Duration::days(i)).format("%Y-%m-%d").to_string(), 1.0)).collect();
        let (cur, prev, series) = windows(&rows, |r| &r.0, |r| r.1, 28);
        assert_eq!((cur, prev, series.len()), (28.0, 28.0, 28));
        assert_eq!(series.last().unwrap().0, "2026-10-30");
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
