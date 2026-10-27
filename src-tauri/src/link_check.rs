//! 推广链接检查：站点靠推广 / 邀请链接赚钱，链接坏了页面照样能打开，没人会发现。
//! 每周把每个站点的推广链接（站内的 `/go/…` 跳转，和带 ref / invite / affiliate 等标记的站外链接）真的访问一遍，
//! 一跳一跳跟到底，看最后落在哪：打不开、404、跳回本站首页的算失效；被目标网站拦了自动访问（403 / 429 / 503）的标「没法确认」。
//! 只发 HTTP 请求，不用 AI；结果存在站点数据目录的 cache/links.json。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LinkResult {
    /// 页面里写的链接（站内跳转是 `/go/okx` 这种路径，站外是完整网址）
    pub href: String,
    /// 实际访问的网址
    pub url: String,
    /// 有多少个页面用到它，和其中一个页面
    pub pages: usize,
    pub sample_page: String,
    /// 最后一跳的状态码（0＝连不上）
    pub status: u16,
    pub final_url: String,
    pub hops: u8,
    /// ok | unverified（被拦了自动访问，没法确认）| broken
    pub verdict: String,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LinkReport {
    pub checked_at: u64,
    /// build（从站点构建产物里找的链接）| live（没有构建产物，只看了线上首页）
    pub source: String,
    pub links: Vec<LinkResult>,
}

impl LinkReport {
    pub fn broken(&self) -> usize {
        self.links.iter().filter(|l| l.verdict == "broken").count()
    }
}

fn report_path(site_dir: &Path) -> PathBuf {
    crate::site_data::cache_dir(site_dir).join("links.json")
}

pub fn load(site_dir: &Path) -> LinkReport {
    std::fs::read(report_path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(site_dir: &Path, r: &LinkReport) {
    let p = report_path(site_dir);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(raw) = serde_json::to_vec_pretty(r) {
        let _ = std::fs::write(p, raw);
    }
}

/// 站外链接算推广 / 邀请链接的条件：网址参数名是这些之一，或路径里有这些整段（只看参数名和整段路径，
/// 不按字面包含判断，不然「promoted」这类普通文章网址也会被当成推广链接）
const PARAMS: &[&str] = &["ref", "refcode", "ref_code", "referral", "referral_code", "invite", "invitecode", "invite_code", "inviter", "aff", "affid", "aff_id", "affiliate", "promo", "promocode", "promo_code", "coupon"];
const SEGMENTS: &[&str] = &["invite", "referral", "affiliate", "join", "ref"];

fn looks_like_money(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    u.query_pairs().any(|(k, _)| PARAMS.contains(&k.to_ascii_lowercase().as_str()))
        || u.path_segments().is_some_and(|mut s| s.any(|seg| SEGMENTS.contains(&seg.to_ascii_lowercase().as_str())))
}

/// 从 HTML 里取出所有 href 的值（&amp; 还原）
fn hrefs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("href=") {
        rest = &rest[i + 5..];
        let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
        let body = &rest[1..];
        let Some(end) = body.find(q) else { break };
        out.push(body[..end].replace("&amp;", "&"));
        rest = &body[end..];
    }
    out
}

/// 一个页面里的推广链接：(去重用的键＝要访问的网址, 页面里写的样子)
fn money_links(html: &str, domain: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for h in hrefs(html) {
        let h = h.trim();
        if let Some(path) = h.strip_prefix("/go/") {
            // 站内跳转：参数（?from=…）不影响跳去哪，按路径去重
            let slug = path.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
            if !slug.is_empty() {
                out.push((format!("https://{domain}/go/{slug}"), format!("/go/{slug}")));
            }
        } else if h.starts_with("http://") || h.starts_with("https://") {
            let low = h.to_ascii_lowercase();
            let host = low.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("");
            let own = host == domain || host.ends_with(&format!(".{domain}"));
            if own && low.contains("/go/") {
                let slug = low.split("/go/").nth(1).and_then(|r| r.split(['?', '#']).next()).unwrap_or("").trim_end_matches('/').to_string();
                if !slug.is_empty() {
                    out.push((format!("https://{domain}/go/{slug}"), format!("/go/{slug}")));
                }
            } else if !own && looks_like_money(h) {
                let u = h.split('#').next().unwrap_or(h).to_string();
                out.push((u.clone(), u));
            }
        }
    }
    out
}

/// 从构建产物里收集推广链接：网址 → (页面里写的样子, 用到的页面数, 其中一个页面)
pub fn collect(out_dir: &Path, domain: &str) -> BTreeMap<String, (String, usize, String)> {
    let mut found: BTreeMap<String, (String, usize, String)> = BTreeMap::new();
    for (page, html) in crate::site_make::page_htmls(out_dir) {
        let mut seen = std::collections::HashSet::new();
        for (url, href) in money_links(&html, domain) {
            if !seen.insert(url.clone()) {
                continue;
            }
            let e = found.entry(url).or_insert_with(|| (href, 0, page.clone()));
            e.1 += 1;
        }
    }
    found
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        // 一跳一跳自己跟：要知道中间跳到了哪、最后落在哪
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36")
        .build()
        .unwrap_or_default()
}

/// 访问一个链接，跟到底，给出结论
async fn check_one(c: &reqwest::Client, url: &str, domain: &str) -> (u16, String, u8, &'static str, String) {
    let mut cur = url.to_string();
    let mut hops = 0u8;
    loop {
        let resp = match c.get(&cur).send().await {
            Ok(r) => r,
            Err(e) => {
                return if e.is_timeout() {
                    (0, cur, hops, "unverified", "访问超时，下次再看".into())
                } else {
                    (0, cur, hops, "broken", "打不开（域名或服务器连不上）".into())
                };
            }
        };
        let status = resp.status().as_u16();
        if resp.status().is_redirection() && hops < 8 {
            let Some(next) = resp.headers().get("location").and_then(|v| v.to_str().ok()).and_then(|l| resp.url().join(l).ok()) else {
                return (status, cur, hops, "broken", "跳转没有给出去向".into());
            };
            cur = next.to_string();
            hops += 1;
            continue;
        }
        let host = reqwest::Url::parse(&cur).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_default();
        let internal = url.contains(&format!("://{domain}/go/"));
        // 推广链接应该跳出本站；绕了一圈落回本站（比如不存在的 /go/xxx 被兜底跳到 /exchanges/）就是没配好
        let back_home = host == domain.trim_start_matches("www.");
        let landed = reqwest::Url::parse(&cur).map(|u| u.path().to_string()).unwrap_or_default();
        return match status {
            200..=299 if internal && hops > 0 && back_home => (status, cur, hops, "broken", format!("跳回了本站（{landed}），这个推广链接可能没配好或被删了")),
            200..=299 if internal && hops == 0 => (status, cur, hops, "unverified", "访问后没有跳转（可能是页面里用脚本跳的），点开确认一下".into()),
            200..=299 => (status, cur, hops, "ok", String::new()),
            403 | 429 | 503 => (status, cur, hops, "unverified", format!("目标网站拦了自动访问（{status}），没法确认，多半是正常的")),
            404 | 410 => (status, cur, hops, "broken", format!("目标页面不存在（{status}）")),
            500..=599 => (status, cur, hops, "unverified", format!("目标网站出错（{status}），下次再看")),
            300..=399 => (status, cur, hops, "broken", "跳转次数太多，可能绕成了圈".into()),
            _ => (status, cur, hops, "broken", format!("打开出错（{status}）")),
        };
    }
}

/// 检查一个站点的推广链接并保存结果。`out_dir` 是构建产物目录（有的话），没有就只看线上首页。
pub async fn check_site(site: &crate::sites::Site, site_dir: &Path, out_dir: Option<PathBuf>) -> Result<LinkReport, String> {
    let domain = site.domain.trim().trim_end_matches('/').to_ascii_lowercase();
    if domain.is_empty() {
        return Err("这个站点还没有设置域名".into());
    }
    let c = client();
    let (found, source) = match out_dir.filter(|d| d.join("index.html").is_file()) {
        Some(d) => {
            let dom = domain.clone();
            (tokio::task::spawn_blocking(move || collect(&d, &dom)).await.map_err(|e| e.to_string())?, "build")
        }
        None => {
            let html = c.get(format!("https://{domain}/")).send().await.map_err(|e| format!("打不开首页：{e}"))?.text().await.unwrap_or_default();
            let mut m = BTreeMap::new();
            for (url, href) in money_links(&html, &domain) {
                m.entry(url).or_insert((href, 1, "/".to_string()));
            }
            (m, "live")
        }
    };
    let mut links = Vec::new();
    for (url, (href, pages, sample)) in found {
        let (status, final_url, hops, verdict, note) = check_one(&c, &url, &domain).await;
        links.push(LinkResult { href, url, pages, sample_page: sample, status, final_url, hops, verdict: verdict.into(), note });
    }
    // 坏的排前面，同一类里用得多的排前面
    let rank = |v: &str| match v { "broken" => 0, "unverified" => 1, _ => 2 };
    links.sort_by(|a, b| rank(&a.verdict).cmp(&rank(&b.verdict)).then(b.pages.cmp(&a.pages)));
    let r = LinkReport { checked_at: crate::now_secs(), source: source.into(), links };
    save(site_dir, &r);
    Ok(r)
}

// ───────────────────────── 每周自动检查 ─────────────────────────

const WEEK: u64 = 7 * 86_400;
static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 调度循环每轮调一次：有站点上次检查超过 7 天就在后台挨个查一遍（同一时间只跑一轮）。
/// 新冒出来的失效链接发一条系统通知。
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
        let mut newly: Vec<String> = Vec::new();
        for site in due {
            let dir = st.sites.data_dir(&site.id);
            let before: std::collections::HashSet<String> = load(&dir).links.into_iter().filter(|l| l.verdict == "broken").map(|l| l.url).collect();
            let out = crate::syndicate::built_out(&st, &site).ok();
            if let Ok(r) = check_site(&site, &dir, out).await {
                for l in r.links.iter().filter(|l| l.verdict == "broken" && !before.contains(&l.url)) {
                    newly.push(format!("{} {}", site.domain, l.href));
                }
            }
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        if !newly.is_empty() {
            use tauri_plugin_notification::NotificationExt;
            let body = format!("{}{}", newly.iter().take(3).cloned().collect::<Vec<_>>().join("、"), if newly.len() > 3 { format!(" 等 {} 个", newly.len()) } else { String::new() });
            let _ = app.notification().builder().title(format!("{} 个推广链接失效", newly.len())).body(&body).show();
        }
        use tauri::Emitter;
        let _ = app.emit("links-checked", ());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_go_links_and_marked_outbound_links_but_not_ordinary_ones() {
        let html = r#"<a href="/go/okx?from=coins%2Fbtc&amp;lang=zh">OKX</a><a href="/go/okx">OKX</a>
            <a href="https://www.binance.com/en/fee/schedule">fees</a>
            <a href='https://accounts.binance.com/register?ref=ABC123'>注册</a>
            <a href="https://gatesor.com/go/ledger/">ledger</a><a href="/zh/posts/x/">文章</a>"#;
        let got = money_links(html, "gatesor.com");
        let urls: Vec<&str> = got.iter().map(|(u, _)| u.as_str()).collect();
        assert_eq!(urls, vec!["https://gatesor.com/go/okx", "https://gatesor.com/go/okx", "https://accounts.binance.com/register?ref=ABC123", "https://gatesor.com/go/ledger"]);
        assert_eq!(got[0].1, "/go/okx");
        assert!(!looks_like_money("https://forklog.com/en/libra-token-promoted-by-argentinas-president-plunges-94/"));
        assert!(looks_like_money("https://www.okx.com/join/OK6667") && looks_like_money("https://x.com/signup?inviteCode=AB"));
    }
}
