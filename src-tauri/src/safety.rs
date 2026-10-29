//! 安全标记检查：站点有没有被标记成危险网站——
//! - Google 安全浏览（Chrome / Safari / Firefox 打开前的红色警告页，俗称「飘红」）：查 Google 透明度报告网页背后的查询接口；
//! - Cloudflare 钓鱼警告页（站点走 Cloudflare 代理时，访客先看到「Suspected Phishing」）：打开首页看是不是这个页面；
//! - Cloudflare 安全 DNS（1.1.1.2 / 1.1.1.3，很多家庭和公司在用）：被拦的域名解析成 0.0.0.0；
//! - Quad9（9.9.9.9，路由器、公司常用）：被拦的域名回「不存在」，和它不过滤的服务器对比，确认是拦截不是真不存在。
//! 每天查一次，只发网络请求，不用密钥、不用 AI。本机的 DNS 常被代理软件接管，所以 DNS 都走 HTTPS（DoH）查。
//! 被标记会发通知（同时发 Telegram），这个站的自动上线先暂停（见 site_runner::verify_and_ship）。
//! 结果存在站点数据目录的 cache/safety.json。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

const HOUR: u64 = 3600;
const DAY: u64 = 86_400;

/// 一个来源的结果。state：ok 没标记 / warn 有提醒（不拦访问）/ bad 被标记或拦截 / unknown 这次没查到
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Check {
    pub key: String,
    pub state: String,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Safety {
    pub checked: u64,
    pub checks: Vec<Check>,
    /// 从什么时候开始被标记（0＝没被标记）
    #[serde(default)]
    pub flagged_since: u64,
    /// 已经发过「被标记」通知、还没解除
    #[serde(default)]
    pub alerted: bool,
}

pub fn label(key: &str) -> &'static str {
    match key {
        "google" => "Google 安全浏览",
        "cf_page" => "Cloudflare 钓鱼警告",
        "cf_dns" => "Cloudflare 安全 DNS",
        "quad9" => "Quad9",
        _ => "其他",
    }
}

impl Safety {
    pub fn flagged(&self) -> impl Iterator<Item = &Check> {
        self.checks.iter().filter(|c| c.state == "bad")
    }
    pub fn is_flagged(&self) -> bool {
        self.flagged().next().is_some()
    }
    /// 一句话：被谁标记了（没被标记为空）
    pub fn summary(&self) -> String {
        self.flagged().map(|c| label(&c.key)).collect::<Vec<_>>().join("、")
    }
    fn unknown_any(&self) -> bool {
        self.checks.iter().any(|c| c.state == "unknown")
    }
}

fn path(site_dir: &Path) -> PathBuf {
    crate::site_data::cache_dir(site_dir).join("safety.json")
}

pub fn load(site_dir: &Path) -> Safety {
    std::fs::read(path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(site_dir: &Path, s: &Safety) {
    let p = path(site_dir);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(raw) = serde_json::to_vec(s) {
        let _ = crate::sites::write_atomic(&p, &raw);
    }
}

fn check(key: &str, state: &str, note: impl Into<String>) -> Check {
    Check { key: key.into(), state: state.into(), note: note.into() }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36")
        .build()
        .unwrap_or_default()
}

// ───────────────────────── Google ─────────────────────────

/// 透明度报告的回复：`)]}'` 开头，后面是 [["sb.ssr", 状态, …, 更新时间(毫秒), "域名", …]]。
/// 状态：1 没发现不安全内容、2 不安全、3 部分页面不安全、4 要查具体网址、5 有不常见的下载文件、6 没有记录。
fn parse_google(body: &str) -> Option<Check> {
    let json = body.trim_start().trim_start_matches(")]}'").trim();
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let row = v.as_array()?.iter().find(|r| r[0] == "sb.ssr")?;
    Some(match row[1].as_i64()? {
        2 => check("google", "bad", "Google 认为这个站不安全：用 Chrome、Safari、Firefox 打开会先看到红色警告页"),
        3 => check("google", "bad", "Google 认为这个站的部分页面不安全：打开那些页面会先看到红色警告页"),
        5 => check("google", "warn", "Google 发现站上有不常见的下载文件：下载时浏览器会提醒，页面本身能正常打开"),
        6 => check("google", "ok", "Google 还没有这个站的记录（新站、访问少的站常见），没有被标记"),
        _ => check("google", "ok", "没有被标记"),
    })
}

async fn google(c: &reqwest::Client, domain: &str) -> Check {
    let url = format!("https://transparencyreport.google.com/transparencyreport/api/v3/safebrowsing/status?site={domain}");
    let mut err = String::new();
    // 偶尔回空（限流），隔两秒再试一次
    for n in 0..2 {
        if n > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        match c.get(&url).send().await {
            Ok(r) if r.status().is_success() => match r.text().await.ok().as_deref().and_then(parse_google) {
                Some(ch) => return ch,
                None => err = "回复看不懂".into(),
            },
            Ok(r) => err = format!("返回 {}", r.status().as_u16()),
            Err(e) => err = if e.is_timeout() { "超时".into() } else { "连不上".into() },
        }
    }
    check("google", "unknown", format!("这次没查到（{err}），下次再查"))
}

/// 推广链接的目标网站有没有被 Google 标记（link_check 用）：被标记返回说明，没标记或没查到返回 None。
/// 链接到被标记的网站，自己的站也可能被连累。
pub async fn google_flags_host(host: &str) -> Option<String> {
    let ch = google(&client(), host).await;
    (ch.state == "bad").then(|| format!("目标网站 {host} 被 Google 标记为危险（访客点过去会先看到红色警告页），链接到这样的网站也可能连累本站被标记"))
}

// ───────────────────────── Cloudflare 钓鱼警告页 ─────────────────────────

/// 只认 Cloudflare 自己回的警告页：响应来自 Cloudflare，标题里有「Suspected Phishing」或正文是它那句固定的话。
/// 站点自己写「怎么防钓鱼」的文章不算（币圈站很常见）。
fn is_phish_page(server: &str, body: &str) -> bool {
    if !server.eq_ignore_ascii_case("cloudflare") {
        return false;
    }
    let b = body.to_ascii_lowercase();
    let title = b.split("<title").nth(1).and_then(|t| t.split("</title>").next()).unwrap_or("");
    title.contains("suspected phishing") || b.contains("reported for potential phishing")
}

async fn cf_page(c: &reqwest::Client, domain: &str) -> Check {
    match c.get(format!("https://{domain}/")).send().await {
        Ok(r) => {
            let server = r.headers().get("server").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            let body: String = r.text().await.unwrap_or_default().chars().take(200_000).collect();
            if is_phish_page(&server, &body) {
                check("cf_page", "bad", "访客打开首页先看到 Cloudflare 的「Suspected Phishing（疑似钓鱼）」警告页")
            } else {
                check("cf_page", "ok", "首页没有出现钓鱼警告页")
            }
        }
        Err(e) => check("cf_page", "unknown", format!("首页这次没打开（{}），下次再查", if e.is_timeout() { "超时" } else { "连不上" })),
    }
}

// ───────────────────────── DNS（走 HTTPS 查）─────────────────────────

/// 拼一个查 A 记录的 DNS 报文
fn dns_query(name: &str) -> Vec<u8> {
    let mut q = vec![0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for part in name.trim_end_matches('.').split('.') {
        q.push(part.len().min(63) as u8);
        q.extend_from_slice(&part.as_bytes()[..part.len().min(63)]);
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    q
}

/// 解析回复：（rcode, A 记录里的 IP 列表）
fn dns_answer(r: &[u8]) -> Option<(u8, Vec<[u8; 4]>)> {
    if r.len() < 12 {
        return None;
    }
    let rcode = r[3] & 0x0f;
    let qd = u16::from_be_bytes([r[4], r[5]]) as usize;
    let an = u16::from_be_bytes([r[6], r[7]]) as usize;
    let mut i = 12;
    let skip_name = |mut i: usize| -> Option<usize> {
        loop {
            let l = *r.get(i)? as usize;
            if l == 0 {
                return Some(i + 1);
            }
            if l & 0xc0 == 0xc0 {
                return Some(i + 2);
            }
            i += l + 1;
        }
    };
    for _ in 0..qd {
        i = skip_name(i)? + 4;
    }
    let mut ips = Vec::new();
    for _ in 0..an {
        i = skip_name(i)?;
        let typ = u16::from_be_bytes([*r.get(i)?, *r.get(i + 1)?]);
        let len = u16::from_be_bytes([*r.get(i + 8)?, *r.get(i + 9)?]) as usize;
        i += 10;
        if typ == 1 && len == 4 {
            ips.push([*r.get(i)?, *r.get(i + 1)?, *r.get(i + 2)?, *r.get(i + 3)?]);
        }
        i += len;
    }
    Some((rcode, ips))
}

async fn doh(c: &reqwest::Client, endpoint: &str, name: &str) -> Result<(u8, Vec<[u8; 4]>), String> {
    use base64::Engine;
    let q = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(dns_query(name));
    let r = c.get(format!("{endpoint}?dns={q}")).header("accept", "application/dns-message").send().await.map_err(|e| if e.is_timeout() { "超时".to_string() } else { "连不上".to_string() })?;
    if !r.status().is_success() {
        return Err(format!("返回 {}", r.status().as_u16()));
    }
    let b = r.bytes().await.map_err(|e| e.to_string())?;
    dns_answer(&b).ok_or_else(|| "回复看不懂".into())
}

async fn cf_dns(c: &reqwest::Client, domain: &str) -> Check {
    match doh(c, "https://security.cloudflare-dns.com/dns-query", domain).await {
        Ok((_, ips)) if ips.contains(&[0, 0, 0, 0]) => check("cf_dns", "bad", "Cloudflare 安全 DNS（1.1.1.2）把这个域名当成恶意网站拦了：用这个 DNS 的人打不开"),
        Ok(_) => check("cf_dns", "ok", "没有被拦"),
        Err(e) => check("cf_dns", "unknown", format!("这次没查到（{e}），下次再查")),
    }
}

async fn quad9(c: &reqwest::Client, domain: &str) -> Check {
    let (filtered, plain) = tokio::join!(doh(c, "https://dns.quad9.net/dns-query", domain), doh(c, "https://dns10.quad9.net/dns-query", domain));
    match (filtered, plain) {
        // 过滤的服务器说不存在、不过滤的能解析：被拦了
        (Ok((3, _)), Ok((0, ips))) if !ips.is_empty() => check("quad9", "bad", "Quad9（9.9.9.9）把这个域名当成恶意网站拦了：用这个 DNS 的人打不开"),
        (Ok(_), Ok(_)) => check("quad9", "ok", "没有被拦"),
        (Err(e), _) | (_, Err(e)) => check("quad9", "unknown", format!("这次没查到（{e}），下次再查")),
    }
}

// ───────────────────────── 检查一个站点 ─────────────────────────

/// force＝现在就查（「立即检查」）；否则每天一次，上次有没查到的 6 小时后再试。返回需要发的通知（标题, 内容）。
pub async fn check_site(site: &crate::sites::Site, site_dir: &Path, force: bool) -> (Safety, Vec<(String, String)>) {
    let domain = site.domain.trim().trim_end_matches('/').to_ascii_lowercase();
    let mut s = load(site_dir);
    let mut alerts = Vec::new();
    if domain.is_empty() {
        return (s, alerts);
    }
    let now = crate::now_secs();
    let every = if s.checks.is_empty() || s.unknown_any() { 6 * HOUR } else { DAY };
    if !force && s.checked > 0 && now.saturating_sub(s.checked) < every {
        return (s, alerts);
    }
    let c = client();
    let (g, p, d, q) = tokio::join!(google(&c, &domain), cf_page(&c, &domain), cf_dns(&c, &domain), quad9(&c, &domain));
    // 这次没查到的来源沿用上次的结论（被标记过的不会因为一次超时就当成解除了）
    let prev = std::mem::take(&mut s.checks);
    s.checks = [g, p, d, q]
        .into_iter()
        .map(|ch| match prev.iter().find(|o| o.key == ch.key) {
            Some(o) if ch.state == "unknown" && o.state == "bad" => Check { note: format!("{}（这次没查到，沿用上次的结论）", o.note.trim_end_matches("（这次没查到，沿用上次的结论）")), ..o.clone() },
            _ => ch,
        })
        .collect();
    s.checked = now;
    if s.is_flagged() {
        if s.flagged_since == 0 {
            s.flagged_since = now;
        }
        if !s.alerted {
            s.alerted = true;
            alerts.push((format!("{domain} 被标记为危险网站"), format!("{}。访客会看到警告页或打不开，这个站的自动上线先暂停了，去站点概览的「站点健康 → 安全标记」看怎么处理", s.flagged().map(|c| c.note.clone()).collect::<Vec<_>>().join("；"))));
        }
    } else if !s.unknown_any() {
        if s.alerted {
            alerts.push((format!("{domain} 的危险标记解除了"), "Google、Cloudflare、Quad9 都没有再标记，自动上线恢复".into()));
        }
        s.alerted = false;
        s.flagged_since = 0;
    }
    save(site_dir, &s);
    (s, alerts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_google_transparency_report() {
        let clean = ")]}'\n\n[[\"sb.ssr\",1,false,false,false,false,false,1792671494999,\"gatesor.com\",false]]";
        assert_eq!(parse_google(clean).unwrap().state, "ok");
        let bad = ")]}'\n\n[[\"sb.ssr\",3,true,true,true,false,false,1793232549492,\"testsafebrowsing.appspot.com\",false]]";
        assert_eq!(parse_google(bad).unwrap().state, "bad");
        assert_eq!(parse_google(")]}'\n\n[[\"sb.ssr\",6,false,false,false,false,false,0,\"x.com\",false]]").unwrap().state, "ok");
        assert!(parse_google("<html>").is_none());
    }

    #[test]
    fn builds_and_reads_dns_messages() {
        let q = dns_query("a.bc");
        assert_eq!(&q[12..], &[1, b'a', 2, b'b', b'c', 0, 0, 1, 0, 1]);
        // 回复：头 + 原问题 + 一条压缩名字指针的 A 记录 0.0.0.0
        let mut r = vec![0, 0, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0];
        r.extend_from_slice(&q[12..]);
        r.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 0, 0, 0, 0]);
        assert_eq!(dns_answer(&r), Some((0, vec![[0, 0, 0, 0]])));
        let mut nx = vec![0, 0, 0x81, 0x83, 0, 1, 0, 0, 0, 0, 0, 0];
        nx.extend_from_slice(&q[12..]);
        assert_eq!(dns_answer(&nx), Some((3, vec![])));
        assert!(dns_answer(&[0; 5]).is_none());
    }

    #[test]
    fn spots_cloudflare_phishing_interstitial() {
        assert!(is_phish_page("cloudflare", "<title>Suspected phishing site | Cloudflare</title>"));
        assert!(is_phish_page("cloudflare", "<p>This website has been reported for potential phishing.</p>"));
        // 站点自己的防钓鱼文章、不是 Cloudflare 回的页面都不算
        assert!(!is_phish_page("cloudflare", "<title>Home</title><h1>How to avoid suspected phishing emails</h1><script src=\"https://static.cloudflareinsights.com/beacon.min.js\"></script>"));
        assert!(!is_phish_page("nginx", "<title>Suspected phishing site | Cloudflare</title>"));
    }

    /// 真连网查：各家公开的测试域名应该被标记，example.com 不应该。`cargo test --lib safety::tests::live -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn live() {
        let c = client();
        assert_eq!(google(&c, "testsafebrowsing.appspot.com").await.state, "bad");
        assert_eq!(cf_dns(&c, "malware.testcategory.com").await.state, "bad");
        assert_eq!(quad9(&c, "isitblocked.org").await.state, "bad");
        for ch in [google(&c, "example.com").await, cf_dns(&c, "example.com").await, quad9(&c, "example.com").await, cf_page(&c, "example.com").await] {
            assert_eq!(ch.state, "ok", "{ch:?}");
        }
    }
}
