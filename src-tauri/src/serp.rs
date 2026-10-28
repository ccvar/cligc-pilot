//! 竞品观察：每周在 Google 上搜一遍站点的主要关键词（Search Console 展现最多的词 + 你自己加的），
//! 看前 10 名都是谁、自己排第几、和上周比谁冲进来了。经常排在你前面的网站就是竞品。
//! 自己排在 4–20 名、前 3 名是别人的词，变成「对比竞品补内容」机会：把前排页面的标题和摘要交给 AI 对照着补（要你确认后才上线）。
//!
//! 搜索结果来自 Serper（serper.dev，按次付费的 Google 搜索结果接口，一次搜索一次扣费，便宜）：
//! 在设置里填它的 API Key（只进钥匙串）。每个站点最多 15 个词、一周一次。
//! 站点的设置在 `sites/<id>/serp.json`，结果在站点缓存的 cache/serp.json。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const KEY_SECRET: &str = "serper-key";
const WEEK: u64 = 7 * 86_400;
const AUTO_KEYWORDS: usize = 10;
const MAX_KEYWORDS: usize = 15;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Config {
    /// 这个站点不查
    #[serde(default)]
    pub off: bool,
    /// 自己加的关键词
    #[serde(default)]
    pub keywords: Vec<String>,
    /// 搜索的地区和语言（Google 的 gl / hl）；空＝按站点的搜索词猜
    #[serde(default)]
    pub gl: String,
    #[serde(default)]
    pub hl: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Hit {
    pub pos: u32,
    pub domain: String,
    pub title: String,
    pub link: String,
    #[serde(default)]
    pub snippet: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Keyword {
    pub q: String,
    /// 自己加的（不是从 Search Console 选的）
    #[serde(default)]
    pub manual: bool,
    /// 自己在前 10 名里的名次和页面；None＝不在前 10
    pub ours: Option<u32>,
    #[serde(default)]
    pub our_url: String,
    /// 上周的名次（上周没查过 / 不在前 10 都是 None）
    #[serde(default)]
    pub prev: Option<u32>,
    #[serde(default)]
    pub prev_checked: bool,
    pub top: Vec<Hit>,
    /// 这周新进前 10 的网站
    #[serde(default)]
    pub new_domains: Vec<String>,
    #[serde(default)]
    pub error: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Competitor {
    pub domain: String,
    /// 在多少个词的前 10 里出现、其中排在你前面的次数、平均名次
    pub keywords: usize,
    pub above_us: usize,
    pub avg_pos: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Report {
    pub checked_at: u64,
    pub gl: String,
    pub hl: String,
    pub keywords: Vec<Keyword>,
    pub competitors: Vec<Competitor>,
}

fn config_path(site_dir: &Path) -> PathBuf {
    site_dir.join("serp.json")
}
fn report_path(site_dir: &Path) -> PathBuf {
    crate::site_data::cache_dir(site_dir).join("serp.json")
}

pub fn load_config(site_dir: &Path) -> Config {
    crate::store_file::read_or_quarantine(&config_path(site_dir))
}
pub fn load(site_dir: &Path) -> Report {
    std::fs::read(report_path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write<T: Serialize>(p: &Path, v: &T) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

fn key() -> Option<String> {
    crate::keychain::get_named_secret(KEY_SECRET).ok().filter(|k| !k.trim().is_empty())
}

/// 按搜索词的文字猜地区和语言
pub fn guess_locale(queries: &[String]) -> (String, String) {
    let count = |f: &dyn Fn(char) -> bool| queries.iter().filter(|q| q.chars().any(f)).count();
    let kana = count(&|c| ('\u{3040}'..='\u{30ff}').contains(&c));
    let hangul = count(&|c| ('\u{ac00}'..='\u{d7af}').contains(&c));
    let han = count(&|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let half = queries.len().max(1) / 2;
    let (gl, hl) = if kana > half / 2 && kana > 0 {
        ("jp", "ja")
    } else if hangul > half {
        ("kr", "ko")
    } else if han > half {
        ("hk", "zh-cn")
    } else {
        ("us", "en")
    };
    (gl.into(), hl.into())
}

/// 这周要查的词：Search Console 近 28 天展现最多、排在 30 名以内的 10 个 + 自己加的（合计最多 15 个）
fn keywords_of(cfg: &Config, site_dir: &Path) -> Vec<(String, bool)> {
    let mut rows: Vec<crate::site_data::Row> = crate::site_data::read_json(&crate::site_data::cache_dir(site_dir), "gsc_queries.json");
    rows.retain(|r| r.position > 0.0 && r.position <= 30.0 && r.key.chars().count() >= 2);
    rows.sort_by(|a, b| b.impressions.total_cmp(&a.impressions));
    let mut out: Vec<(String, bool)> = Vec::new();
    for k in cfg.keywords.iter().map(|k| k.trim()).filter(|k| !k.is_empty()) {
        if !out.iter().any(|(q, _)| q.eq_ignore_ascii_case(k)) {
            out.push((k.to_string(), true));
        }
    }
    for r in rows.into_iter().take(AUTO_KEYWORDS) {
        if !out.iter().any(|(q, _)| q.eq_ignore_ascii_case(&r.key)) {
            out.push((r.key, false));
        }
    }
    out.truncate(MAX_KEYWORDS);
    out
}

async fn search(key: &str, q: &str, gl: &str, hl: &str) -> Result<Vec<Hit>, String> {
    let resp = crate::cf::http()
        .post("https://google.serper.dev/search")
        .header("X-API-KEY", key)
        .json(&json!({ "q": q, "gl": gl, "hl": hl, "num": 10 }))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| "连不上 Serper".to_string())?;
    let status = resp.status().as_u16();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    match status {
        200 => Ok(hits(&v)),
        401 | 403 => Err("Serper 不认这个 API Key".into()),
        402 | 429 => Err("Serper 的额度用完了（到 serper.dev 充值后再查）".into()),
        _ => Err(format!("Serper 返回 {status}：{}", v["message"].as_str().unwrap_or(""))),
    }
}

fn hits(v: &Value) -> Vec<Hit> {
    v["organic"]
        .as_array()
        .map(|a| {
            a.iter()
                .enumerate()
                .map(|(n, o)| {
                    let link = o["link"].as_str().unwrap_or("").to_string();
                    Hit {
                        pos: o["position"].as_u64().map(|p| p as u32).unwrap_or(n as u32 + 1),
                        domain: crate::revenue::norm_host(&link),
                        title: o["title"].as_str().unwrap_or("").to_string(),
                        snippet: o["snippet"].as_str().unwrap_or("").to_string(),
                        link,
                    }
                })
                .filter(|h| !h.domain.is_empty())
                .take(10)
                .collect()
        })
        .unwrap_or_default()
}

/// 自己的网站：同一个域名或它的子域名
fn is_ours(domain: &str, ours: &str) -> bool {
    domain == ours || domain.ends_with(&format!(".{ours}"))
}

/// 排在前 10 的网站，按出现次数、排在前面的次数排
pub fn competitors(keywords: &[Keyword], ours: &str) -> Vec<Competitor> {
    let mut m: BTreeMap<String, (usize, usize, u32)> = BTreeMap::new();
    for k in keywords {
        let mut seen = std::collections::HashSet::new();
        for h in &k.top {
            if is_ours(&h.domain, ours) || !seen.insert(h.domain.clone()) {
                continue;
            }
            let e = m.entry(h.domain.clone()).or_default();
            e.0 += 1;
            e.2 += h.pos;
            if k.ours.is_none_or(|o| h.pos < o) {
                e.1 += 1;
            }
        }
    }
    let mut v: Vec<Competitor> = m.into_iter().map(|(domain, (n, above, sum))| Competitor { domain, keywords: n, above_us: above, avg_pos: sum as f64 / n as f64 }).collect();
    v.sort_by(|a, b| b.keywords.cmp(&a.keywords).then(b.above_us.cmp(&a.above_us)).then(a.avg_pos.total_cmp(&b.avg_pos)));
    v.truncate(12);
    v
}

pub async fn check_site(site: &crate::sites::Site, site_dir: &Path) -> Result<Report, String> {
    let key = key().ok_or("先在设置里填 Serper 的 API Key")?;
    let ours = crate::revenue::norm_host(&site.domain);
    if ours.is_empty() {
        return Err("这个站点还没有设置域名".into());
    }
    let cfg = load_config(site_dir);
    let kws = keywords_of(&cfg, site_dir);
    if kws.is_empty() {
        return Err("还没有要查的关键词：等 Search Console 有了搜索词，或者自己加几个".into());
    }
    let (gl, hl) = if cfg.gl.is_empty() { guess_locale(&kws.iter().map(|k| k.0.clone()).collect::<Vec<_>>()) } else { (cfg.gl.clone(), if cfg.hl.is_empty() { "en".into() } else { cfg.hl.clone() }) };
    let prev = load(site_dir);
    let prev_by: HashMap<String, &Keyword> = prev.keywords.iter().map(|k| (k.q.to_lowercase(), k)).collect();
    let mut out = Vec::new();
    for (q, manual) in kws {
        let p = prev_by.get(&q.to_lowercase()).filter(|p| p.error.is_empty());
        let mut k = Keyword { q: q.clone(), manual, prev: p.and_then(|p| p.ours), prev_checked: p.is_some(), ..Default::default() };
        match search(&key, &q, &gl, &hl).await {
            Ok(top) => {
                if let Some(h) = top.iter().find(|h| is_ours(&h.domain, &ours)) {
                    k.ours = Some(h.pos);
                    k.our_url = h.link.clone();
                }
                if let Some(p) = p {
                    k.new_domains = top.iter().filter(|h| !is_ours(&h.domain, &ours) && !p.top.iter().any(|o| o.domain == h.domain)).map(|h| h.domain.clone()).collect();
                    k.new_domains.dedup();
                }
                k.top = top;
            }
            Err(e) => {
                let stop = e.contains("API Key") || e.contains("额度");
                k.error = e.clone();
                out.push(k);
                if stop {
                    break;
                }
                continue;
            }
        }
        out.push(k);
    }
    if out.iter().all(|k| !k.error.is_empty()) {
        return Err(out.first().map(|k| k.error.clone()).unwrap_or_default());
    }
    let r = Report { checked_at: crate::now_secs(), competitors: competitors(&out, &ours), gl, hl, keywords: out };
    write(&report_path(site_dir), &r)?;
    Ok(r)
}

/// 机会：自己排在 4–20 名（Search Console）、搜索结果前 3 名都是别人的词 → 对照前排页面补内容
pub fn opportunities(site: &crate::sites::Site, site_dir: &Path) -> Vec<crate::site_ops::Opportunity> {
    let r = load(site_dir);
    if r.keywords.is_empty() {
        return Vec::new();
    }
    let ours = crate::revenue::norm_host(&site.domain);
    let qp: Vec<crate::site_data::Row> = crate::site_data::read_json(&crate::site_data::cache_dir(site_dir), "gsc_query_page.json");
    let mut out = Vec::new();
    for k in &r.keywords {
        // 这个词上自己展现最多的页面
        let Some(row) = qp.iter().filter(|x| x.key.eq_ignore_ascii_case(&k.q)).max_by(|a, b| a.impressions.total_cmp(&b.impressions)) else { continue };
        if !(4.0..=20.0).contains(&row.position) || row.impressions < 30.0 {
            continue;
        }
        let ahead: Vec<&Hit> = k.top.iter().filter(|h| !is_ours(&h.domain, &ours)).take(3).collect();
        if ahead.len() < 3 || k.top.iter().take(3).any(|h| is_ours(&h.domain, &ours)) {
            continue;
        }
        let list = ahead.iter().map(|h| format!("第 {} 名 {}：「{}」{}", h.pos, h.domain, h.title, if h.snippet.is_empty() { String::new() } else { format!("（摘要：{}）", h.snippet) })).collect::<Vec<_>>().join("\n");
        out.push(crate::site_ops::Opportunity {
            key: format!("serp_gap:{}", k.q.to_lowercase()),
            kind: "serp_gap".into(),
            title: format!("「{}」排第 {:.0}，前 3 名都是别人", k.q, row.position),
            page: row.key2.clone(),
            queries: vec![row.clone()],
            clicks: row.clicks,
            impressions: row.impressions,
            ctr: row.ctr,
            position: row.position,
            target: k.q.clone(),
            evidence: format!("近 28 天这个词展现 {:.0}，你的页面平均排第 {:.1}。Google 上排在前面的是：\n{list}\n对比它们回应了什么、你的页面缺了什么。", row.impressions, row.position),
            score: row.impressions * (crate::site_ops::expected_ctr(3.0) - crate::site_ops::expected_ctr(row.position)).max(0.005),
            change: "content".into(),
            ..Default::default()
        });
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out.truncate(5);
    out
}

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 调度循环每轮调一次：填了 Key 就每周查一遍有 Search Console 数据（或自己加了词）的站点。
pub fn schedule(app: &tauri::AppHandle) {
    use tauri::Manager;
    if key().is_none() {
        return;
    }
    let st = app.state::<crate::AppState>();
    let now = crate::now_secs();
    let due: Vec<crate::sites::Site> = st
        .sites
        .list()
        .into_iter()
        .filter(|s| {
            let dir = st.sites.data_dir(&s.id);
            let cfg = load_config(&dir);
            !s.domain.trim().is_empty() && !cfg.off && (!s.data.gsc_site.is_empty() || !cfg.keywords.is_empty()) && now.saturating_sub(load(&dir).checked_at) >= WEEK
        })
        .collect();
    if due.is_empty() || RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<crate::AppState>();
        for site in due {
            let dir = st.sites.data_dir(&site.id);
            if let Err(e) = check_site(&site, &dir).await {
                // 记下失败的时间，一周后再试（额度用完了不每 10 分钟重试）
                let mut r = load(&dir);
                r.checked_at = crate::now_secs();
                if r.keywords.is_empty() {
                    r.keywords.push(Keyword { error: e.clone(), ..Default::default() });
                }
                let _ = write(&report_path(&dir), &r);
                if e.contains("API Key") || e.contains("额度") {
                    break;
                }
            }
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        use tauri::Emitter;
        let _ = app.emit("serp-checked", ());
    });
}

// ───────────────────────── 命令 ─────────────────────────

#[derive(Serialize)]
pub struct SerpView {
    pub has_key: bool,
    pub config: Config,
    pub report: Report,
    /// 这周会查的词（还没查过时给你看）
    pub planned: Vec<String>,
}

fn view(state: &crate::AppState, id: &str) -> SerpView {
    let dir = state.sites.data_dir(id);
    let config = load_config(&dir);
    let planned = keywords_of(&config, &dir).into_iter().map(|k| k.0).collect();
    SerpView { has_key: key().is_some(), config, report: load(&dir), planned }
}

#[tauri::command]
pub fn site_serp(state: tauri::State<'_, crate::AppState>, id: String) -> SerpView {
    view(&state, &id)
}

#[tauri::command]
pub fn set_site_serp(state: tauri::State<'_, crate::AppState>, id: String, config: Config) -> Result<SerpView, String> {
    let mut c = config;
    c.keywords = c.keywords.into_iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).take(MAX_KEYWORDS).collect();
    write(&config_path(&state.sites.data_dir(&id)), &c)?;
    Ok(view(&state, &id))
}

#[tauri::command]
pub async fn check_site_serp(state: tauri::State<'_, crate::AppState>, id: String) -> Result<SerpView, String> {
    let site = state.sites.get(&id)?;
    check_site(&site, &state.sites.data_dir(&id)).await?;
    Ok(view(&state, &id))
}

// ───────────────────────── 外部服务的 API Key（PageSpeed、Serper） ─────────────────────────

#[derive(Serialize)]
pub struct ApiKeys {
    pub pagespeed: bool,
    pub serper: bool,
}

fn keys_status() -> ApiKeys {
    let has = |a: &str| crate::keychain::get_named_secret(a).ok().is_some_and(|k| !k.trim().is_empty());
    ApiKeys { pagespeed: has(crate::speed::KEY_SECRET), serper: has(KEY_SECRET) }
}

#[tauri::command]
pub fn api_keys_status() -> ApiKeys {
    keys_status()
}

/// 填 Key：先真的调一次确认能用，再进钥匙串
#[tauri::command]
pub async fn api_key_set(name: String, value: String) -> Result<ApiKeys, String> {
    let v = value.trim().to_string();
    if v.len() < 10 || v.contains(char::is_whitespace) {
        return Err("这不像 API Key，复制完整的 Key 再填".into());
    }
    match name.as_str() {
        "serper" => {
            search(&v, "example", "us", "en").await?;
            crate::keychain::set_named_secret(KEY_SECRET, &v)?;
        }
        "pagespeed" => {
            let p = crate::cf::http()
                .get(format!("https://www.googleapis.com/pagespeedonline/v5/runPagespeed?url=https%3A%2F%2Fexample.com%2F&category=performance&key={v}"))
                .timeout(Duration::from_secs(90))
                .send()
                .await
                .map_err(|_| "连不上 PageSpeed Insights".to_string())?;
            if !p.status().is_success() {
                let body: Value = p.json().await.unwrap_or(Value::Null);
                return Err(format!("这个 Key 不能用：{}", body["error"]["message"].as_str().unwrap_or("Google 拒绝了")));
            }
            crate::keychain::set_named_secret(crate::speed::KEY_SECRET, &v)?;
        }
        _ => return Err("不认识的服务".into()),
    }
    Ok(keys_status())
}

#[tauri::command]
pub fn api_key_remove(name: String) -> Result<ApiKeys, String> {
    let account = match name.as_str() {
        "serper" => KEY_SECRET,
        "pagespeed" => crate::speed::KEY_SECRET,
        _ => return Err("不认识的服务".into()),
    };
    let _ = crate::keychain::delete_named_secret(account);
    Ok(keys_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(q: &str, ours: Option<u32>, domains: &[&str]) -> Keyword {
        Keyword { q: q.into(), ours, top: domains.iter().enumerate().map(|(n, d)| Hit { pos: n as u32 + 1, domain: d.to_string(), ..Default::default() }).collect(), ..Default::default() }
    }

    #[test]
    fn locale_follows_the_script_of_queries() {
        assert_eq!(guess_locale(&["比特币怎么买".into(), "usdt 是什么".into()]), ("hk".into(), "zh-cn".into()));
        assert_eq!(guess_locale(&["best vpn".into(), "how to buy btc".into()]), ("us".into(), "en".into()));
        assert_eq!(guess_locale(&["ビットコイン 買い方".into()]), ("jp".into(), "ja".into()));
    }

    #[test]
    fn competitors_count_appearances_and_who_is_ahead() {
        let ks = vec![kw("a", Some(3), &["x.com", "y.com", "me.com", "x.com"]), kw("b", None, &["x.com", "z.com"]), kw("c", Some(1), &["blog.me.com", "y.com"])];
        let c = competitors(&ks, "me.com");
        assert_eq!(c[0].domain, "x.com");
        assert_eq!((c[0].keywords, c[0].above_us), (2, 2));
        let y = c.iter().find(|c| c.domain == "y.com").unwrap();
        assert_eq!((y.keywords, y.above_us), (2, 1));
        assert!(!c.iter().any(|c| c.domain.ends_with("me.com")));
    }

    #[test]
    fn organic_hits_are_parsed() {
        let v = serde_json::json!({ "organic": [{ "title": "T", "link": "https://www.x.com/a", "snippet": "S", "position": 1 }, { "title": "U", "link": "https://y.com/" }] });
        let h = hits(&v);
        assert_eq!((h[0].domain.as_str(), h[0].pos, h[1].pos), ("x.com", 1, 2));
    }
}
