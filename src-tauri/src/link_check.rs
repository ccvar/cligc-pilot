//! 推广链接检查：站点靠推广 / 邀请链接赚钱，链接坏了页面照样能打开，没人会发现。
//! 每周把每个站点的推广链接（站内的 `/go/…` 跳转、自家短链，和带 ref / invite / affiliate 等标记的站外链接）真的访问一遍，
//! 一跳一跳跟到底，看最后落在哪：打不开、404、跳回本站首页的算失效；被目标网站拦了自动访问（403 / 429 / 503）的标「没法确认」。
//! 落地的网站被 Google 标记为危险的也算失效（访客会先看到红色警告页，还可能连累本站，见 safety）。
//! 链接从构建产物里找；服务端渲染的站没有静态页面，按站点地图抽查线上页面。
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
    /// 查法的版本：低于 LINKS_V 的结果是旧查法查的，调度会马上重查
    #[serde(default)]
    pub v: u32,
    /// build（从站点构建产物里找的链接）| live（没有构建产物，抽查线上页面）
    pub source: String,
    /// live：实际看了多少个页面
    #[serde(default)]
    pub scanned: usize,
    /// live：站点地图里一共多少个页面（0＝没有站点地图，只看了首页）
    #[serde(default)]
    pub total: usize,
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

/// 查法改了就加 1：旧结果可能漏了链接，不等每周的检查，马上重查
const LINKS_V: u32 = 2;
/// 自家短链：网址上没有任何推广标记，只能按域名认
const SHORT_HOSTS: &[&str] = &["g.cligc.com"];
/// 没有构建产物时，一个站最多抽查多少个线上页面、同时开几个
const MAX_LIVE_PAGES: usize = 150;
const LIVE_CONCURRENCY: usize = 6;
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

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

/// 路径开头的语言段：zh、en、zh-hans、pt-br
fn is_locale(seg: &str) -> bool {
    let (lang, region) = seg.split_once('-').map_or((seg, None), |(l, r)| (l, Some(r)));
    lang.len() == 2 && lang.chars().all(|c| c.is_ascii_alphabetic()) && region.is_none_or(|r| (2..=4).contains(&r.len()) && r.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// 站内跳转的路径：`/go/okx`、带语言前缀的 `/zh-hans/go/okx`（参数和结尾的 / 去掉）；别的路径返回 None
fn go_path(path: &str) -> Option<String> {
    let clean = path.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    let (first, after) = clean.strip_prefix('/')?.split_once('/')?;
    let slug = if first == "go" {
        after
    } else if is_locale(first) {
        after.strip_prefix("go/")?
    } else {
        return None;
    };
    (!slug.is_empty()).then(|| clean.to_string())
}

/// 这个网址是跳转入口（站内 /go/… 或自家短链）：访问后应该跳到别的网站
fn is_redirector(url: &str, domain: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    SHORT_HOSTS.contains(&host.as_str()) || (host == domain && go_path(u.path()).is_some())
}

/// 一个页面里的推广链接：(去重用的键＝要访问的网址, 页面里写的样子)
fn money_links(html: &str, domain: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for h in hrefs(html) {
        let h = h.trim();
        if h.starts_with('/') {
            // 站内跳转：参数（?from=…）不影响跳去哪，按路径去重
            if let Some(p) = go_path(h) {
                out.push((format!("https://{domain}{p}"), p));
            }
        } else if h.starts_with("http://") || h.starts_with("https://") {
            let Ok(u) = reqwest::Url::parse(h) else { continue };
            let host = u.host_str().unwrap_or("").to_ascii_lowercase();
            if host == domain || host.ends_with(&format!(".{domain}")) {
                if let Some(p) = go_path(u.path()) {
                    out.push((format!("https://{domain}{p}"), p));
                }
            } else if SHORT_HOSTS.contains(&host.as_str()) {
                // 短链的参数（?src=…）只是记来源，不影响跳去哪
                let path = u.path().trim_end_matches('/');
                if !path.is_empty() {
                    let s = format!("https://{host}{path}");
                    out.push((s.clone(), s));
                }
            } else if looks_like_money(h) {
                let s = h.split('#').next().unwrap_or(h).to_string();
                out.push((s.clone(), s));
            }
        }
    }
    out
}

type Found = BTreeMap<String, (String, usize, String)>;

/// 把一个页面里的推广链接记进 found：网址 → (页面里写的样子, 用到的页面数, 其中一个页面)
fn tally(found: &mut Found, page: &str, html: &str, domain: &str) {
    let mut seen = std::collections::HashSet::new();
    for (url, href) in money_links(html, domain) {
        if !seen.insert(url.clone()) {
            continue;
        }
        let e = found.entry(url).or_insert_with(|| (href, 0, page.to_string()));
        e.1 += 1;
    }
}

/// 从构建产物里收集推广链接
pub fn collect(out_dir: &Path, domain: &str) -> Found {
    let mut found = Found::new();
    for (page, html) in crate::site_make::page_htmls(out_dir) {
        tally(&mut found, &page, &html, domain);
    }
    found
}

/// 站点地图里挑要抽查的页面：首页一定看；其余按栏目（第一段路径，语言前缀不算）轮流挑，每个栏目都看到，
/// 栏目内按网址的哈希排，分散到不同位置，每次挑的还是同一批。返回 (要看的页面, 站点地图里一共多少页；没有站点地图是 0)
fn sample_pages(domain: &str, locs: &[String], max: usize) -> (Vec<String>, usize) {
    use sha2::{Digest, Sha256};
    let bare = domain.trim_start_matches("www.");
    let mut unique = std::collections::BTreeSet::new();
    for loc in locs {
        let Ok(u) = reqwest::Url::parse(loc) else { continue };
        let host = u.host_str().unwrap_or("").to_ascii_lowercase();
        let file = u.path().rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        let html = !file.contains('.') || file.ends_with(".html") || file.ends_with(".htm");
        if host.trim_start_matches("www.") == bare && html {
            unique.insert(u.as_str().to_string());
        }
    }
    let home = format!("https://{domain}/");
    // 首页一定会看，站点地图里没写也算一页，不然会出现「看了 69 / 68 页」
    let has_home = unique.iter().any(|u| reqwest::Url::parse(u).is_ok_and(|p| p.path() == "/"));
    let total = if locs.is_empty() { 0 } else { unique.len() + usize::from(!has_home) };
    let mut sections: BTreeMap<String, Vec<(Vec<u8>, String)>> = BTreeMap::new();
    for url in unique.into_iter().filter(|u| reqwest::Url::parse(u).is_ok_and(|p| p.path() != "/")) {
        let parsed = reqwest::Url::parse(&url).ok();
        let section = parsed.as_ref().and_then(|p| p.path_segments()).and_then(|mut s| s.find(|seg| !is_locale(seg))).unwrap_or("").to_string();
        sections.entry(section).or_default().push((Sha256::digest(url.as_bytes()).to_vec(), url));
    }
    for v in sections.values_mut() {
        v.sort();
        v.reverse(); // 下面从尾部 pop
    }
    let mut pages = vec![home];
    let mut left = true;
    while left && pages.len() < max {
        left = false;
        for v in sections.values_mut() {
            if pages.len() >= max {
                break;
            }
            if let Some((_, url)) = v.pop() {
                pages.push(url);
                left = true;
            }
        }
    }
    (pages, total)
}

/// 没有构建产物：抽查线上页面找推广链接。返回 (找到的链接, 实际看了多少页, 站点地图一共多少页)。
/// 一个页面都打不开时返回错误，不能把「没看到」说成「没有」。
async fn live_scan(domain: &str) -> Result<(Found, usize, usize), String> {
    let c = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent(USER_AGENT).build().map_err(|e| e.to_string())?;
    let locs = crate::site_verify::sitemap_locs(&c, domain).await;
    let (pages, total) = sample_pages(domain, &locs, MAX_LIVE_PAGES);
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(LIVE_CONCURRENCY));
    let mut set = tokio::task::JoinSet::new();
    for (i, page) in pages.into_iter().enumerate() {
        let (c, gate) = (c.clone(), gate.clone());
        set.spawn(async move {
            let _slot = gate.acquire_owned().await.ok()?;
            let r = c.get(&page).send().await.ok().filter(|r| r.status().is_success())?;
            let html = r.headers().get("content-type").and_then(|v| v.to_str().ok()).is_none_or(|t| t.contains("html"));
            if !html {
                return None;
            }
            Some((i, page, r.text().await.ok()?))
        });
    }
    let mut got = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok(Some(x)) = r {
            got.push(x);
        }
    }
    if got.is_empty() {
        return Err(format!("打不开 {domain} 的线上页面，这次没查"));
    }
    got.sort_by_key(|x| x.0);
    let mut found = Found::new();
    for (_, page, html) in &got {
        let path = reqwest::Url::parse(page).map(|u| u.path().to_string()).unwrap_or_else(|_| page.clone());
        tally(&mut found, &path, html, domain);
    }
    Ok((found, got.len(), total))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        // 一跳一跳自己跟：要知道中间跳到了哪、最后落在哪
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(USER_AGENT)
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
        let internal = is_redirector(url, domain);
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

/// 检查一个站点的推广链接并保存结果。`out_dir` 是构建产物目录（有的话），没有就抽查线上页面。
pub async fn check_site(site: &crate::sites::Site, site_dir: &Path, out_dir: Option<PathBuf>) -> Result<LinkReport, String> {
    let domain = site.domain.trim().trim_end_matches('/').to_ascii_lowercase();
    if domain.is_empty() {
        return Err("这个站点还没有设置域名".into());
    }
    let c = client();
    let (found, source, scanned, total) = match out_dir.filter(|d| d.join("index.html").is_file()) {
        Some(d) => {
            let dom = domain.clone();
            (tokio::task::spawn_blocking(move || collect(&d, &dom)).await.map_err(|e| e.to_string())?, "build", 0, 0)
        }
        None => {
            let (found, scanned, total) = live_scan(&domain).await?;
            (found, "live", scanned, total)
        }
    };
    let mut links = Vec::new();
    // 落地的网站顺带查一下有没有被 Google 标记为危险（同一个网站只查一次）
    let mut flagged: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    for (url, (href, pages, sample)) in found {
        let (status, final_url, hops, mut verdict, mut note) = check_one(&c, &url, &domain).await;
        let host = reqwest::Url::parse(&final_url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_ascii_lowercase())).unwrap_or_default();
        if !host.is_empty() && host != domain.trim_start_matches("www.") && verdict != "broken" {
            if !flagged.contains_key(&host) {
                let f = crate::safety::google_flags_host(&host).await;
                flagged.insert(host.clone(), f);
            }
            if let Some(Some(why)) = flagged.get(&host) {
                verdict = "broken";
                note = why.clone();
            }
        }
        links.push(LinkResult { href, url, pages, sample_page: sample, status, final_url, hops, verdict: verdict.into(), note });
    }
    // 坏的排前面，同一类里用得多的排前面
    let rank = |v: &str| match v { "broken" => 0, "unverified" => 1, _ => 2 };
    links.sort_by(|a, b| rank(&a.verdict).cmp(&rank(&b.verdict)).then(b.pages.cmp(&a.pages)));
    let r = LinkReport { checked_at: crate::now_secs(), v: LINKS_V, source: source.into(), scanned, total, links };
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
    let due: Vec<crate::sites::Site> = st.sites.list().into_iter().filter(|s| {
        let r = load(&st.sites.data_dir(&s.id));
        !s.domain.trim().is_empty() && (r.v < LINKS_V || now.saturating_sub(r.checked_at) >= WEEK)
    }).collect();
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
            let body = format!("{}{}", newly.iter().take(3).cloned().collect::<Vec<_>>().join("、"), if newly.len() > 3 { format!(" 等 {} 个", newly.len()) } else { String::new() });
            crate::push::notify(&app, &format!("{} 个推广链接失效", newly.len()), &body);
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

    #[test]
    fn short_links_and_language_prefixed_go_links_count() {
        let html = r#"<a href="https://g.cligc.com/dok?src=x.com">a</a><a href="https://g.cligc.com/dok">b</a><a href="https://g.cligc.com/">root</a>
            <a href="/zh-hans/go/okx/">c</a><a href="/go/signup/binance">d</a><a href="https://x.com/pt-br/go/gate?from=a">e</a>
            <a href="/blog/go/deep">not</a><a href="/go/">not</a><a href="//cdn.x.com/go/y">not</a><a href="/guides/go-basics">not</a>"#;
        let urls: Vec<String> = money_links(html, "x.com").into_iter().map(|(u, _)| u).collect();
        assert_eq!(urls, vec!["https://g.cligc.com/dok", "https://g.cligc.com/dok", "https://x.com/zh-hans/go/okx", "https://x.com/go/signup/binance", "https://x.com/pt-br/go/gate"]);
        assert!(is_redirector("https://g.cligc.com/dok", "x.com") && is_redirector("https://x.com/zh-hans/go/okx", "x.com"));
        assert!(!is_redirector("https://www.okx.com/join/OK1", "x.com") && !is_redirector("https://x.com/guides/a", "x.com"));
    }

    #[test]
    fn a_page_counts_a_link_once() {
        let mut found = Found::new();
        tally(&mut found, "/a", r#"<a href="/go/okx">1</a><a href="/go/okx?from=b">2</a>"#, "x.com");
        tally(&mut found, "/b", r#"<a href="/go/okx">1</a>"#, "x.com");
        assert_eq!(found["https://x.com/go/okx"], ("/go/okx".to_string(), 2, "/a".to_string()));
    }

    #[test]
    fn the_sample_covers_every_section_and_stays_under_the_cap() {
        let mut locs: Vec<String> = (0..500).map(|i| format!("https://x.com/coins/c{i}")).collect();
        locs.extend((0..4).map(|i| format!("https://x.com/guides/g{i}")));
        locs.extend((0..4).map(|i| format!("https://x.com/zh-hans/guides/z{i}")));
        locs.push("https://x.com/exchanges/okx".into());
        locs.push("https://x.com/".into());
        locs.push("https://x.com/exchanges/okx".into());
        locs.push("https://x.com/files/a.pdf".into());
        locs.push("https://other.com/a".into());
        let (pages, total) = sample_pages("x.com", &locs, 20);
        assert_eq!(total, 510, "去重，不算别的网站和文件");
        assert_eq!(pages.len(), 20);
        assert_eq!(pages[0], "https://x.com/");
        assert_eq!(pages.iter().filter(|p| *p == "https://x.com/").count(), 1);
        assert!(pages.contains(&"https://x.com/exchanges/okx".to_string()), "小栏目不会被大栏目挤掉");
        assert!(pages.iter().filter(|p| p.contains("/guides/")).count() == 8, "语言版本算同一个栏目，栏目小于配额全看");
        assert_eq!(sample_pages("x.com", &locs, 20).0, pages, "每次挑的是同一批");

        let few = vec!["https://www.x.com/a".to_string(), "https://x.com/b.html".to_string()];
        let (pages, total) = sample_pages("x.com", &few, 20);
        assert_eq!((pages.len(), total), (3, 3), "站点地图没写首页也算一页");
        let (pages, total) = sample_pages("x.com", &[], 20);
        assert_eq!((pages, total), (vec!["https://x.com/".to_string()], 0), "没有站点地图只看首页");
    }

    #[test]
    fn old_reports_still_load_and_read_as_outdated() {
        let r: LinkReport = serde_json::from_str(r#"{"checked_at":5,"source":"live","links":[]}"#).unwrap();
        assert!(r.v < LINKS_V && r.scanned == 0 && r.total == 0);
    }
}
