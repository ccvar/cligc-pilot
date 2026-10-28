//! 页面速度：每周用 Google 的 PageSpeed Insights（免费接口）测一下首页和搜索流量最多的几个页面（手机版），
//! 记下性能分、LCP / CLS / TBT，和真实用户数据（Chrome 用户体验报告，流量够才有）。
//! 慢的页面变成「页面太慢」机会，附上 PageSpeed 列出的主要问题，交给 AI 做最小的优化（要你确认后才上线）。
//!
//! 不填 API Key 也能用（Google 给匿名调用的额度不多，偶尔会被限流，下次再测）；在设置里填了 Key 就稳定。
//! 结果存在站点数据目录的 cache/speed.json。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const KEY_SECRET: &str = "pagespeed-key";
const WEEK: u64 = 7 * 86_400;
/// 每个站点测几个页面（首页 + 搜索点击最多的几个）
const PAGES: usize = 4;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Issue {
    pub title: String,
    /// 估计能省下的时间（毫秒）
    pub savings_ms: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PageSpeed {
    pub url: String,
    /// 性能分 0–100（实验室数据，手机）
    pub score: f64,
    pub lcp_ms: f64,
    pub cls: f64,
    pub tbt_ms: f64,
    /// 真实用户数据（近 28 天，流量够才有）：FAST | AVERAGE | SLOW | 空
    #[serde(default)]
    pub field: String,
    #[serde(default)]
    pub field_lcp_ms: f64,
    #[serde(default)]
    pub field_inp_ms: f64,
    #[serde(default)]
    pub field_cls: f64,
    #[serde(default)]
    pub issues: Vec<Issue>,
    #[serde(default)]
    pub error: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SpeedReport {
    pub checked_at: u64,
    pub pages: Vec<PageSpeed>,
}

impl PageSpeed {
    /// 算慢：性能分低于 50，或真实用户数据整体是「慢」
    pub fn slow(&self) -> bool {
        self.error.is_empty() && ((self.score > 0.0 && self.score < 50.0) || self.field == "SLOW")
    }
}

fn path(site_dir: &Path) -> PathBuf {
    crate::site_data::cache_dir(site_dir).join("speed.json")
}

pub fn load(site_dir: &Path) -> SpeedReport {
    std::fs::read(path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(site_dir: &Path, r: &SpeedReport) {
    let p = path(site_dir);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(raw) = serde_json::to_vec_pretty(r) {
        let _ = std::fs::write(p, raw);
    }
}

fn esc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 解析 PageSpeed 的响应
pub fn parse(url: &str, v: &Value) -> PageSpeed {
    let lh = &v["lighthouseResult"];
    let audit = |k: &str| lh["audits"][k]["numericValue"].as_f64().unwrap_or(0.0);
    let mut issues: Vec<Issue> = lh["audits"]
        .as_object()
        .map(|m| {
            m.values()
                .filter(|a| a["score"].as_f64().is_some_and(|s| s < 0.9))
                .filter_map(|a| {
                    let ms = [a["details"]["overallSavingsMs"].as_f64(), a["metricSavings"]["LCP"].as_f64(), a["metricSavings"]["TBT"].as_f64(), a["metricSavings"]["FCP"].as_f64()]
                        .into_iter()
                        .flatten()
                        .fold(0.0, f64::max);
                    (ms >= 150.0).then(|| Issue { title: a["title"].as_str().unwrap_or("").to_string(), savings_ms: ms.round() })
                })
                .filter(|i| !i.title.is_empty())
                .collect()
        })
        .unwrap_or_default();
    issues.sort_by(|a, b| b.savings_ms.total_cmp(&a.savings_ms));
    issues.dedup_by(|a, b| a.title == b.title);
    issues.truncate(5);
    let le = &v["loadingExperience"];
    // 只有这个网址自己的数据才算（流量不够时 Google 给整个网站的汇总，id 不是这个网址）
    let own = le["id"].as_str().is_some_and(|id| id.trim_end_matches('/') == url.trim_end_matches('/'));
    let metric = |k: &str| if own { le["metrics"][k]["percentile"].as_f64().unwrap_or(0.0) } else { 0.0 };
    PageSpeed {
        url: url.to_string(),
        score: (lh["categories"]["performance"]["score"].as_f64().unwrap_or(0.0) * 100.0).round(),
        lcp_ms: audit("largest-contentful-paint").round(),
        cls: (audit("cumulative-layout-shift") * 1000.0).round() / 1000.0,
        tbt_ms: audit("total-blocking-time").round(),
        field: if own { le["overall_category"].as_str().unwrap_or("").to_string() } else { String::new() },
        field_lcp_ms: metric("LARGEST_CONTENTFUL_PAINT_MS"),
        field_inp_ms: metric("INTERACTION_TO_NEXT_PAINT"),
        field_cls: metric("CUMULATIVE_LAYOUT_SHIFT_SCORE") / 100.0,
        issues,
        error: String::new(),
    }
}

async fn test_page(url: &str, key: Option<&str>) -> PageSpeed {
    let mut api = format!("https://www.googleapis.com/pagespeedonline/v5/runPagespeed?url={}&strategy=mobile&category=performance&locale=zh-CN", esc(url));
    if let Some(k) = key {
        api.push_str(&format!("&key={}", esc(k)));
    }
    let fail = |e: String| PageSpeed { url: url.to_string(), error: e, ..Default::default() };
    let resp = match crate::cf::http().get(&api).timeout(Duration::from_secs(120)).send().await {
        Ok(r) => r,
        Err(e) => return fail(if e.is_timeout() { "PageSpeed 测试超时，下次再测".into() } else { "连不上 PageSpeed Insights".into() }),
    };
    let status = resp.status().as_u16();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    match status {
        200 => parse(url, &v),
        429 => fail("PageSpeed 的免费额度用完了（在设置里填一个 API Key 就稳定），下次再测".into()),
        400 | 403 if key.is_some() => fail(format!("PageSpeed API Key 不能用：{}", v["error"]["message"].as_str().unwrap_or(""))),
        _ => fail(format!("PageSpeed 没测出来（{status}）：{}", v["error"]["message"].as_str().unwrap_or("页面可能打不开"))),
    }
}

/// 要测的页面：首页 + Search Console 点击最多的几个
fn pages_to_test(site: &crate::sites::Site, site_dir: &Path) -> Vec<String> {
    let home = format!("https://{}/", site.domain.trim().trim_end_matches('/'));
    let mut out = vec![home.clone()];
    let mut rows: Vec<crate::site_data::Row> = crate::site_data::read_json(&crate::site_data::cache_dir(site_dir), "gsc_pages.json");
    rows.sort_by(|a, b| b.clicks.total_cmp(&a.clicks));
    for r in rows {
        if out.len() >= PAGES {
            break;
        }
        let u = r.key.trim().to_string();
        if u.starts_with("http") && !out.iter().any(|o| o.trim_end_matches('/') == u.trim_end_matches('/')) && crate::revenue::norm_host(&u) == crate::revenue::norm_host(&site.domain) {
            out.push(u);
        }
    }
    out
}

pub async fn check_site(site: &crate::sites::Site, site_dir: &Path) -> Result<SpeedReport, String> {
    if site.domain.trim().is_empty() {
        return Err("这个站点还没有设置域名".into());
    }
    let key = crate::keychain::get_named_secret(KEY_SECRET).ok().filter(|k| !k.trim().is_empty());
    let mut pages = Vec::new();
    for (n, u) in pages_to_test(site, site_dir).into_iter().enumerate() {
        if n > 0 {
            tokio::time::sleep(Duration::from_secs(if key.is_some() { 1 } else { 4 })).await;
        }
        pages.push(test_page(&u, key.as_deref()).await);
    }
    let r = SpeedReport { checked_at: crate::now_secs(), pages };
    save(site_dir, &r);
    Ok(r)
}

/// 机会：慢的页面（kind=slow，技术改动；要你确认后才上线）
pub fn opportunities(site_dir: &Path) -> Vec<crate::site_ops::Opportunity> {
    load(site_dir)
        .pages
        .into_iter()
        .filter(|p| p.slow())
        .map(|p| {
            let field = if p.field_lcp_ms > 0.0 { format!("真实用户的 LCP 约 {:.1} 秒、INP 约 {:.0} 毫秒；", p.field_lcp_ms / 1000.0, p.field_inp_ms) } else { String::new() };
            let issues = p.issues.iter().map(|i| format!("{}（约省 {:.1} 秒）", i.title, i.savings_ms / 1000.0)).collect::<Vec<_>>().join("；");
            crate::site_ops::Opportunity {
                key: format!("slow:{}", p.url),
                kind: "slow".into(),
                title: format!("{} 打开太慢", crate::site_ops::path_of(&p.url)),
                page: p.url.clone(),
                evidence: format!(
                    "PageSpeed 手机版性能分 {:.0}，最大内容渲染（LCP）{:.1} 秒，主线程阻塞 {:.0} 毫秒，布局偏移 {:.2}。{field}页面慢会掉排名和跳出更多。{}",
                    p.score,
                    p.lcp_ms / 1000.0,
                    p.tbt_ms,
                    p.cls,
                    if issues.is_empty() { String::new() } else { format!("主要问题：{issues}。") }
                ),
                score: (100.0 - p.score).max(10.0),
                change: "technical".into(),
                ..Default::default()
            }
        })
        .collect()
}

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 调度循环每轮调一次：有域名的站点每周测一次（一个一个来）。
pub fn schedule(app: &tauri::AppHandle) {
    use tauri::Manager;
    let st = app.state::<crate::AppState>();
    let now = crate::now_secs();
    let due: Vec<crate::sites::Site> = st.sites.list().into_iter().filter(|s| !s.domain.trim().is_empty() && now.saturating_sub(load(&st.sites.data_dir(&s.id)).checked_at) >= WEEK).collect();
    if due.is_empty() || RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<crate::AppState>();
        for site in due {
            let _ = check_site(&site, &st.sites.data_dir(&site.id)).await;
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        use tauri::Emitter;
        let _ = app.emit("speed-checked", ());
    });
}

#[tauri::command]
pub fn site_speed(state: tauri::State<'_, crate::AppState>, id: String) -> SpeedReport {
    load(&state.sites.data_dir(&id))
}

#[tauri::command]
pub async fn check_site_speed(state: tauri::State<'_, crate::AppState>, id: String) -> Result<SpeedReport, String> {
    let site = state.sites.get(&id)?;
    check_site(&site, &state.sites.data_dir(&id)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lab_field_and_issues() {
        let v = serde_json::json!({
            "loadingExperience": { "id": "https://a.com/x/", "overall_category": "SLOW", "metrics": {
                "LARGEST_CONTENTFUL_PAINT_MS": { "percentile": 4200 }, "INTERACTION_TO_NEXT_PAINT": { "percentile": 310 }, "CUMULATIVE_LAYOUT_SHIFT_SCORE": { "percentile": 12 } } },
            "lighthouseResult": {
                "categories": { "performance": { "score": 0.42 } },
                "audits": {
                    "largest-contentful-paint": { "numericValue": 5300.4, "score": 0.1 },
                    "cumulative-layout-shift": { "numericValue": 0.1234, "score": 0.7 },
                    "total-blocking-time": { "numericValue": 610.0, "score": 0.3 },
                    "uses-optimized-images": { "title": "压缩图片", "score": 0.2, "details": { "overallSavingsMs": 1200 } },
                    "render-blocking": { "title": "移除阻塞渲染的资源", "score": 0.5, "metricSavings": { "LCP": 450, "FCP": 300 } },
                    "fine": { "title": "没问题", "score": 1, "details": { "overallSavingsMs": 900 } },
                    "tiny": { "title": "小问题", "score": 0.5, "details": { "overallSavingsMs": 40 } }
                }
            }
        });
        let p = parse("https://a.com/x", &v);
        assert_eq!((p.score, p.lcp_ms, p.tbt_ms, p.cls), (42.0, 5300.0, 610.0, 0.123));
        assert_eq!((p.field.as_str(), p.field_lcp_ms, p.field_cls), ("SLOW", 4200.0, 0.12));
        assert_eq!(p.issues.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["压缩图片", "移除阻塞渲染的资源"]);
        assert!(p.slow());
        // 网站汇总的真实数据不算这个页面的
        let mut v2 = v.clone();
        v2["loadingExperience"]["id"] = serde_json::json!("https://a.com/");
        let p2 = parse("https://a.com/x/", &v2);
        assert_eq!((p2.field.as_str(), p2.field_lcp_ms), ("", 0.0));
    }
}
