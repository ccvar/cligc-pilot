//! 收益：每个站点赚了多少，和 AI 花了多少放在一起看，判断站点值不值得继续投入。
//!
//! - AdSense：用 Google 官方的 AdSense Management API（只读权限 adsense.readonly）在浏览器里授权一次，
//!   凭据只进钥匙串；每天拉一次本月和上月按站点（AdSense 里添加的网站）的预估收入，和近 28 天按页面的收入；
//! - 推广佣金、赞助这些没有统一接口：在站点里手动记一笔（按月），或在设置里导入 CSV；
//! - 金额一律按美元（AdSense 报表直接要美元，手动记的也按美元填）。
//!
//! 设置在数据目录的 revenue.json，AdSense 报表缓存在 adsense_cache.json；每个站点手动记的在 `sites/<id>/revenue.json`，
//! 按页面的收入在站点缓存的 `cache/revenue_pages.json`（机会规则「访问多、收入低」用）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SECRET: &str = "adsense-oauth";
pub const SCOPE: &str = "https://www.googleapis.com/auth/adsense.readonly";
const DAY: u64 = 86_400;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Settings {
    /// accounts/pub-…
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub account_name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub fetched_at: u64,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub last_try: u64,
}

/// AdSense 报表缓存：域名 → 月份 → (收入, 网页浏览量)
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AdsenseCache {
    #[serde(default)]
    pub by_domain: BTreeMap<String, BTreeMap<String, (f64, f64)>>,
}

/// 页面近 28 天的 AdSense 收入
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PageRevenue {
    pub url: String,
    pub earnings: f64,
    pub views: f64,
}

/// 手动记的一笔
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Entry {
    pub id: String,
    /// YYYY-MM
    pub month: String,
    pub amount: f64,
    /// affiliate | sponsor | other
    pub source: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub created_at: u64,
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("revenue.json")
}
fn cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("adsense_cache.json")
}
fn entries_path(site_dir: &Path) -> PathBuf {
    site_dir.join("revenue.json")
}

fn write<T: Serialize>(p: &Path, v: &T) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

pub fn load_settings(data_dir: &Path) -> Settings {
    crate::store_file::read_or_quarantine(&settings_path(data_dir))
}
pub fn load_cache(data_dir: &Path) -> AdsenseCache {
    crate::store_file::read_or_quarantine(&cache_path(data_dir))
}
pub fn load_entries(site_dir: &Path) -> Vec<Entry> {
    crate::store_file::read_or_quarantine(&entries_path(site_dir))
}
pub fn load_pages(site_dir: &Path) -> Vec<PageRevenue> {
    crate::site_data::read_json(&crate::site_data::cache_dir(site_dir), "revenue_pages.json")
}

fn now() -> u64 {
    crate::now_secs()
}

/// 本地时间的 YYYY-MM；`back` 个月以前
pub fn month(back: u32) -> String {
    use chrono::Datelike;
    let t = chrono::Local::now().date_naive();
    let (mut y, mut m) = (t.year(), t.month() as i32 - back as i32);
    while m <= 0 {
        m += 12;
        y -= 1;
    }
    format!("{y}-{m:02}")
}

/// 去掉 www.、小写，用来把 AdSense 的网站和站点对上
pub fn norm_host(h: &str) -> String {
    let h = h.trim().to_ascii_lowercase();
    let h = h.split("://").last().unwrap_or(&h).split(['/', '?', '#']).next().unwrap_or("").to_string();
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

fn has_cred() -> bool {
    crate::keychain::get_named_secret(SECRET).is_ok()
}

async fn token() -> Result<String, String> {
    let raw = crate::keychain::get_named_secret(SECRET).map_err(|_| "还没连接 AdSense".to_string())?;
    let cred = crate::google::parse_credential("oauth", &raw)?;
    crate::google::access_token(SECRET, &cred, &[SCOPE]).await
}

async fn api(token: &str, url: &str) -> Result<Value, String> {
    crate::google::api(token, reqwest::Method::GET, url, None).await.map_err(|e| {
        if e.contains("has not been used") || e.contains("is disabled") || e.contains("SERVICE_DISABLED") {
            "Google Cloud 项目里还没启用 AdSense Management API：到 Google Cloud 控制台的「API 和服务 → 库」搜索 AdSense Management API 并启用，再试一次".to_string()
        } else {
            e
        }
    })
}

/// 报表：表头名 → 列号，每行的单元格
fn rows_of(v: &Value) -> Vec<BTreeMap<String, String>> {
    let heads: Vec<String> = v["headers"].as_array().map(|a| a.iter().map(|h| h["name"].as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    v["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    let cells = r["cells"].as_array().cloned().unwrap_or_default();
                    heads.iter().cloned().zip(cells.iter().map(|c| c["value"].as_str().unwrap_or("").to_string())).collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn date_params(prefix: &str, d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    format!("{prefix}.year={}&{prefix}.month={}&{prefix}.day={}", d.year(), d.month(), d.day())
}

fn report_url(account: &str, from: chrono::NaiveDate, to: chrono::NaiveDate, dims: &[&str], extra: &str) -> String {
    let dims: String = dims.iter().map(|d| format!("&dimensions={d}")).collect();
    format!(
        "https://adsense.googleapis.com/v2/{account}/reports:generate?{}&{}{dims}&metrics=ESTIMATED_EARNINGS&metrics=PAGE_VIEWS&currencyCode=USD{extra}",
        date_params("startDate", from),
        date_params("endDate", to)
    )
}

fn num(s: Option<&String>) -> f64 {
    s.and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

/// 拉一次 AdSense：本月和上月按网站、近 28 天按页面。按页面的结果按域名分给各个站点。
pub async fn fetch(state: &crate::AppState) -> Result<(), String> {
    use chrono::Datelike;
    let mut s = load_settings(&state.data_dir);
    let token = token().await?;
    if s.account.is_empty() {
        let v = api(&token, "https://adsense.googleapis.com/v2/accounts").await?;
        let a = v["accounts"].as_array().and_then(|a| a.first()).ok_or("这个 Google 账号下没有 AdSense 账号")?;
        s.account = a["name"].as_str().unwrap_or("").to_string();
        s.account_name = a["displayName"].as_str().unwrap_or("").to_string();
    }
    let today = chrono::Local::now().date_naive();
    let first_this = today.with_day(1).unwrap_or(today);
    let first_last = (first_this - chrono::Duration::days(1)).with_day(1).unwrap_or(first_this);
    let v = api(&token, &report_url(&s.account, first_last, today, &["MONTH", "OWNED_SITE_DOMAIN_NAME"], "")).await?;
    let mut cache = AdsenseCache::default();
    for r in rows_of(&v) {
        let host = norm_host(r.get("OWNED_SITE_DOMAIN_NAME").map(String::as_str).unwrap_or(""));
        let m = r.get("MONTH").cloned().unwrap_or_default();
        if host.is_empty() || m.is_empty() {
            continue;
        }
        let e = cache.by_domain.entry(host).or_default().entry(m).or_default();
        e.0 += num(r.get("ESTIMATED_EARNINGS"));
        e.1 += num(r.get("PAGE_VIEWS"));
    }
    write(&cache_path(&state.data_dir), &cache)?;
    // 按页面（近 28 天）：分到各个站点的缓存里；这一步失败不影响按网站的数字
    if let Ok(v) = api(&token, &report_url(&s.account, today - chrono::Duration::days(28), today, &["PAGE_URL"], "&orderBy=-ESTIMATED_EARNINGS&limit=5000")).await {
        let mut by_host: BTreeMap<String, Vec<PageRevenue>> = BTreeMap::new();
        for r in rows_of(&v) {
            let url = r.get("PAGE_URL").cloned().unwrap_or_default();
            by_host.entry(norm_host(&url)).or_default().push(PageRevenue { earnings: num(r.get("ESTIMATED_EARNINGS")), views: num(r.get("PAGE_VIEWS")), url });
        }
        for site in state.sites.list() {
            let host = norm_host(&site.domain);
            if host.is_empty() {
                continue;
            }
            let pages = by_host.remove(&host).unwrap_or_default();
            let dir = crate::site_data::cache_dir(&state.sites.data_dir(&site.id));
            let _ = write(&dir.join("revenue_pages.json"), &pages);
        }
    }
    s.fetched_at = now();
    s.error.clear();
    write(&settings_path(&state.data_dir), &s)
}

/// 调度循环每轮调一次：连了 AdSense 就每天拉一次（失败的隔 6 小时再试）。
pub fn schedule(app: &tauri::AppHandle) {
    use tauri::Manager;
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let st = app.state::<crate::AppState>();
    let s = load_settings(&st.data_dir);
    let due = if s.error.is_empty() { now().saturating_sub(s.fetched_at) >= DAY } else { now().saturating_sub(s.last_try) >= 6 * 3600 };
    if !has_cred() || !due || RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<crate::AppState>();
        if let Err(e) = fetch(&st).await {
            let mut s = load_settings(&st.data_dir);
            s.error = e;
            s.last_try = now();
            let _ = write(&settings_path(&st.data_dir), &s);
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        use tauri::Emitter;
        let _ = app.emit("revenue-changed", ());
    });
}

// ───────────────────────── 每个站点的收益 ─────────────────────────

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Money {
    /// 本月（到今天）、上个月
    pub month: f64,
    pub last_month: f64,
    /// 其中 AdSense 的
    pub adsense_month: f64,
    pub adsense_last: f64,
    /// 近 28 天 AdSense 每千次浏览收入（没有按页面数据为 0）
    pub rpm: f64,
    /// 有没有任何收入来源（连了 AdSense 且对上了这个站点，或手动记过）
    pub tracked: bool,
}

pub fn site_money(cache: &AdsenseCache, site: &crate::sites::Site, site_dir: &Path) -> Money {
    let (this, last) = (month(0), month(1));
    let mut m = Money::default();
    if let Some(ms) = cache.by_domain.get(&norm_host(&site.domain)) {
        m.tracked = true;
        m.adsense_month = ms.get(&this).map(|x| x.0).unwrap_or(0.0);
        m.adsense_last = ms.get(&last).map(|x| x.0).unwrap_or(0.0);
    }
    let entries = load_entries(site_dir);
    m.tracked |= !entries.is_empty();
    let manual = |mo: &str| entries.iter().filter(|e| e.month == mo).map(|e| e.amount).sum::<f64>();
    m.month = m.adsense_month + manual(&this);
    m.last_month = m.adsense_last + manual(&last);
    let pages = load_pages(site_dir);
    let (e, v): (f64, f64) = pages.iter().fold((0.0, 0.0), |a, p| (a.0 + p.earnings, a.1 + p.views));
    if v > 0.0 {
        m.rpm = e / v * 1000.0;
    }
    m
}

/// 规则：近 28 天浏览量不少、但每千次浏览收入远低于站内其它页面的页面。
/// 只在有按页面的 AdSense 数据、且至少 5 个页面有足够浏览量时才算（样本太少的站点中位数不可靠）。
pub fn low_rpm_pages(pages: &[PageRevenue]) -> Vec<(PageRevenue, f64, f64)> {
    let rpm = |p: &PageRevenue| if p.views > 0.0 { p.earnings / p.views * 1000.0 } else { 0.0 };
    let mut sample: Vec<f64> = pages.iter().filter(|p| p.views >= 100.0).map(rpm).collect();
    if sample.len() < 5 {
        return Vec::new();
    }
    sample.sort_by(|a, b| a.total_cmp(b));
    let median = sample[sample.len() / 2];
    if median <= 0.0 {
        return Vec::new();
    }
    let mut out: Vec<(PageRevenue, f64, f64)> = pages.iter().filter(|p| p.views >= 300.0 && rpm(p) < median * 0.35).map(|p| (p.clone(), rpm(p), median)).collect();
    out.sort_by(|a, b| (b.0.views * (b.2 - b.1)).total_cmp(&(a.0.views * (a.2 - a.1))));
    out.truncate(5);
    out
}

/// 机会：访问多、收入低的页面（kind=low_rpm，改内容；要你确认后才上线）
pub fn opportunities(site_dir: &Path) -> Vec<crate::site_ops::Opportunity> {
    low_rpm_pages(&load_pages(site_dir))
        .into_iter()
        .map(|(p, rpm, median)| crate::site_ops::Opportunity {
            key: format!("low_rpm:{}", p.url),
            kind: "low_rpm".into(),
            title: format!("{} 访问多、广告收入低", crate::site_ops::path_of(&p.url)),
            page: p.url.clone(),
            evidence: format!(
                "近 28 天这个页面有 {:.0} 次浏览，AdSense 每千次浏览收入约 ${rpm:.2}，站内其它页面的中位数约 ${median:.2}。\
可能是读者看一眼就走、内容太薄，或者主题和广告 / 推广不搭。",
                p.views
            ),
            // 补到中位数每月大约能多拿到的钱，折成和其它机会可比的分数
            score: (p.views * (median - rpm) / 1000.0 * 10.0).max(1.0),
            change: "content".into(),
            ..Default::default()
        })
        .collect()
}

// ───────────────────────── CSV 导入 ─────────────────────────

/// 解析导入的 CSV：每行 `月份,域名,金额[,来源[,备注]]`；月份可以是 2026-10 或 2026-10-15；
/// 第一行是表头就跳过。返回 (域名, 记录) 和看不懂的行号。
pub fn parse_csv(raw: &str) -> (Vec<(String, Entry)>, Vec<usize>) {
    let (mut ok, mut bad) = (Vec::new(), Vec::new());
    for (n, line) in raw.lines().enumerate() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        let cells: Vec<String> = line.split([',', '\t', ';']).map(|c| c.trim().trim_matches('"').to_string()).collect();
        let mon: String = cells.first().map(|c| c.replace('/', "-")).unwrap_or_default();
        let mon = mon.get(..7).unwrap_or("").to_string();
        let valid_month = mon.len() == 7 && mon.as_bytes()[4] == b'-' && mon[..4].chars().all(|c| c.is_ascii_digit()) && mon[5..].chars().all(|c| c.is_ascii_digit());
        let amount: Option<f64> = cells.get(2).and_then(|a| a.replace(['$', '¥', ' '], "").parse().ok());
        let domain = cells.get(1).map(|d| norm_host(d)).unwrap_or_default();
        match (valid_month, amount, domain.is_empty()) {
            (true, Some(a), false) => ok.push((
                domain,
                Entry {
                    id: uuid::Uuid::new_v4().to_string(),
                    month: mon,
                    amount: a,
                    source: match cells.get(3).map(|s| s.to_ascii_lowercase()).as_deref() {
                        Some("sponsor") | Some("赞助") => "sponsor".into(),
                        Some("other") | Some("其它") | Some("其他") => "other".into(),
                        _ => "affiliate".into(),
                    },
                    note: cells.get(4).cloned().unwrap_or_default(),
                    created_at: now(),
                },
            )),
            _ if n == 0 => {} // 表头
            _ => bad.push(n + 1),
        }
    }
    (ok, bad)
}

// ───────────────────────── 命令 ─────────────────────────

#[derive(Serialize)]
pub struct Status {
    pub connected: bool,
    pub email: String,
    pub account: String,
    pub account_name: String,
    pub fetched_at: u64,
    pub error: String,
    /// 有没有配 Google OAuth 客户端（连 AdSense 要用它在浏览器里授权）
    pub has_client: bool,
}

fn status(state: &crate::AppState) -> Status {
    let s = load_settings(&state.data_dir);
    Status {
        connected: has_cred(),
        email: s.email,
        account: s.account,
        account_name: s.account_name,
        fetched_at: s.fetched_at,
        error: s.error,
        has_client: crate::data_cmds::load_client(state).is_some(),
    }
}

#[tauri::command]
pub fn revenue_status(state: tauri::State<'_, crate::AppState>) -> Status {
    status(&state)
}

/// 在浏览器里授权 AdSense 只读，然后马上拉一次。
#[tauri::command]
pub async fn adsense_connect(app: tauri::AppHandle, state: tauri::State<'_, crate::AppState>) -> Result<Status, String> {
    use tauri_plugin_opener::OpenerExt;
    let client = crate::data_cmds::load_client(&state).ok_or("先在「数据源」里配好 Google OAuth 客户端（和网站数据共用一份）")?;
    let t = crate::google_oauth::authorize(&client.client_id, &client.client_secret, &format!("openid email {SCOPE}"), |url| {
        app.opener().open_url(url, None::<&str>).map_err(|e| format!("打开浏览器失败：{e}"))
    })
    .await?;
    if !t.granted.iter().any(|g| g == SCOPE) {
        return Err("授权时没有勾选 AdSense 的查看权限，请重新连接并勾选".into());
    }
    let email = crate::google::api(&t.access, reqwest::Method::GET, "https://openidconnect.googleapis.com/v1/userinfo", None)
        .await
        .ok()
        .and_then(|u| u["email"].as_str().map(str::to_string))
        .unwrap_or_default();
    let cred = crate::google::OAuthCred { client_id: client.client_id, client_secret: client.client_secret, refresh_token: t.refresh, scopes: t.granted, email: email.clone() };
    crate::keychain::set_named_secret(SECRET, &serde_json::to_string(&cred).map_err(|e| e.to_string())?)?;
    crate::google::forget_token(SECRET);
    write(&settings_path(&state.data_dir), &Settings { email, ..Default::default() })?;
    if let Err(e) = fetch(&state).await {
        let mut s = load_settings(&state.data_dir);
        s.error = e;
        s.last_try = now();
        write(&settings_path(&state.data_dir), &s)?;
    }
    Ok(status(&state))
}

#[tauri::command]
pub async fn adsense_refresh(state: tauri::State<'_, crate::AppState>) -> Result<Status, String> {
    if let Err(e) = fetch(&state).await {
        let mut s = load_settings(&state.data_dir);
        s.error = e.clone();
        s.last_try = now();
        write(&settings_path(&state.data_dir), &s)?;
        return Err(e);
    }
    Ok(status(&state))
}

/// 断开 AdSense：删凭据和报表缓存（手动记的不动）
#[tauri::command]
pub fn adsense_disconnect(state: tauri::State<'_, crate::AppState>) -> Result<Status, String> {
    let _ = crate::keychain::delete_named_secret(SECRET);
    crate::google::forget_token(SECRET);
    let _ = std::fs::remove_file(cache_path(&state.data_dir));
    let _ = std::fs::remove_file(settings_path(&state.data_dir));
    for site in state.sites.list() {
        let _ = std::fs::remove_file(crate::site_data::cache_dir(&state.sites.data_dir(&site.id)).join("revenue_pages.json"));
    }
    Ok(status(&state))
}

#[derive(Serialize)]
pub struct SiteRevenue {
    pub money: Money,
    pub entries: Vec<Entry>,
    /// 近 28 天收入最多的页面（AdSense）
    pub top_pages: Vec<PageRevenue>,
    /// 本月 AI 花费（和托管页同一口径）
    pub cost: crate::usage::Cost,
    pub adsense: bool,
}

fn site_revenue_of(state: &crate::AppState, id: &str) -> Result<SiteRevenue, String> {
    let site = state.sites.get(id)?;
    let dir = state.sites.data_dir(id);
    let mut entries = load_entries(&dir);
    entries.sort_by(|a, b| b.month.cmp(&a.month).then(b.created_at.cmp(&a.created_at)));
    let mut top = load_pages(&dir);
    top.truncate(10);
    Ok(SiteRevenue {
        money: site_money(&load_cache(&state.data_dir), &site, &dir),
        entries,
        top_pages: top,
        cost: crate::usage::site_cost(&dir, crate::managed_cmds::month_start()),
        adsense: has_cred(),
    })
}

#[tauri::command]
pub fn site_revenue(state: tauri::State<'_, crate::AppState>, id: String) -> Result<SiteRevenue, String> {
    site_revenue_of(&state, &id)
}

#[tauri::command]
pub fn revenue_add(state: tauri::State<'_, crate::AppState>, id: String, month: String, amount: f64, source: String, note: String) -> Result<SiteRevenue, String> {
    let month = month.trim().to_string();
    if month.len() != 7 || !amount.is_finite() || amount < 0.0 {
        return Err("月份写成 2026-10 这样，金额填大于等于 0 的数".into());
    }
    let dir = state.sites.data_dir(&id);
    let mut v = load_entries(&dir);
    v.push(Entry { id: uuid::Uuid::new_v4().to_string(), month, amount, source: if source.is_empty() { "affiliate".into() } else { source }, note: note.trim().to_string(), created_at: now() });
    write(&entries_path(&dir), &v)?;
    site_revenue_of(&state, &id)
}

#[tauri::command]
pub fn revenue_remove(state: tauri::State<'_, crate::AppState>, id: String, entry_id: String) -> Result<SiteRevenue, String> {
    let dir = state.sites.data_dir(&id);
    let mut v = load_entries(&dir);
    v.retain(|e| e.id != entry_id);
    write(&entries_path(&dir), &v)?;
    site_revenue_of(&state, &id)
}

#[derive(Serialize)]
pub struct ImportResult {
    pub added: usize,
    /// 域名对不上任何站点的行（域名）
    pub unknown: Vec<String>,
    /// 看不懂的行号
    pub bad_lines: Vec<usize>,
}

/// 导入 CSV（推广佣金后台导出的、自己整理的都行）
#[tauri::command]
pub fn revenue_import(state: tauri::State<'_, crate::AppState>, path: String) -> Result<ImportResult, String> {
    let p = std::path::Path::new(&path);
    if !p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["csv", "tsv", "txt"].contains(&e.to_ascii_lowercase().as_str())) {
        return Err("请选择 .csv 文件".into());
    }
    if std::fs::metadata(p).map_err(|e| format!("读取文件失败：{e}"))?.len() > 2 * 1024 * 1024 {
        return Err("文件太大（超过 2 MB）".into());
    }
    let csv = std::fs::read_to_string(p).map_err(|_| "读不了这个文件（要 UTF-8 编码的 CSV）".to_string())?;
    let (rows, bad) = parse_csv(&csv);
    let sites = state.sites.list();
    let mut by_site: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
    let mut unknown = Vec::new();
    for (domain, e) in rows {
        match sites.iter().find(|s| norm_host(&s.domain) == domain) {
            Some(s) => by_site.entry(s.id.clone()).or_default().push(e),
            None => {
                if !unknown.contains(&domain) {
                    unknown.push(domain)
                }
            }
        }
    }
    let mut added = 0;
    for (id, es) in by_site {
        let dir = state.sites.data_dir(&id);
        let mut v = load_entries(&dir);
        added += es.len();
        v.extend(es);
        write(&entries_path(&dir), &v)?;
    }
    Ok(ImportResult { added, unknown, bad_lines: bad })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_normalized() {
        assert_eq!(norm_host("https://www.Example.com/a?b"), "example.com");
        assert_eq!(norm_host("blog.example.com"), "blog.example.com");
    }

    #[test]
    fn csv_rows_header_and_bad_lines() {
        let (ok, bad) = parse_csv("月份,域名,金额,来源,备注\n2026-10,www.a.com,$12.5,affiliate,OKX\n2026/09/30,b.com,3\nxx,b.com,1\n");
        assert_eq!(ok.len(), 2);
        assert_eq!((ok[0].0.as_str(), ok[0].1.month.as_str(), ok[0].1.amount, ok[0].1.note.as_str()), ("a.com", "2026-10", 12.5, "OKX"));
        assert_eq!((ok[1].1.month.as_str(), ok[1].1.source.as_str()), ("2026-09", "affiliate"));
        assert_eq!(bad, vec![4]);
    }

    #[test]
    fn report_rows_are_keyed_by_header() {
        let v = serde_json::json!({ "headers": [{ "name": "MONTH" }, { "name": "ESTIMATED_EARNINGS" }], "rows": [{ "cells": [{ "value": "2026-10" }, { "value": "4.20" }] }] });
        let r = rows_of(&v);
        assert_eq!(num(r[0].get("ESTIMATED_EARNINGS")), 4.2);
    }

    #[test]
    fn low_rpm_needs_a_sample_and_a_big_gap() {
        let p = |u: &str, e: f64, v: f64| PageRevenue { url: u.into(), earnings: e, views: v };
        let few = vec![p("a", 1.0, 1000.0), p("b", 0.01, 1000.0)];
        assert!(low_rpm_pages(&few).is_empty());
        let pages = vec![p("a", 5.0, 1000.0), p("b", 4.0, 1000.0), p("c", 6.0, 1000.0), p("d", 5.0, 800.0), p("e", 4.5, 900.0), p("low", 0.5, 2000.0)];
        let got = low_rpm_pages(&pages);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.url, "low");
        assert!((got[0].1 - 0.25).abs() < 1e-9 && (got[0].2 - 5.0).abs() < 1e-9);
    }
}
