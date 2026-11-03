//! 上线后的线上验收：上传完成后逐个打开这次改到的页面，确认线上真的对了，再提交 IndexNow。
//!
//! - 页面能打开、首页正常 → 不过就是「严重」：网站坏着，调用方回滚。
//! - 新内容已生效、SEO 基本项、新页面在 sitemap 里、站内链接能打开、没有内部痕迹 → 不过算「一般」：
//!   网站是好的，标成待核对，不回滚。
//! - CDN 刷新有延迟：先重试（约 5 分钟），重试完还不过才下结论。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

use crate::site_jobs::Job;

/// 严重失败时，部署返回的错误以它开头（调用方据此回滚）。
pub const HARD_PREFIX: &str = "线上验收没通过：";

#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Check {
    pub label: String,
    pub ok: bool,
    #[serde(default)]
    pub detail: String,
    /// 不过时是不是严重问题（要回滚）
    #[serde(default)]
    pub hard: bool,
}

/// 要验收什么。
#[derive(Clone, Default, Debug)]
pub struct Spec {
    pub domain: String,
    /// 这次改到的已有页面（完整网址）
    pub pages: Vec<String>,
    /// 这次新增的页面
    pub new_pages: Vec<String>,
    /// 这次加的几段文字：至少要在一个验收页面里出现
    pub snippets: Vec<String>,
    /// 构建产物里有对应 HTML 的网址（静态站）：线上 404 说明是部署没到位，不是改动的问题
    pub built: HashSet<String>,
    /// 这条任务本身要改的页面：和新页面一起查 noindex / canonical。其它只是顺带改到的页面（比如旧文存档）
    /// 可能本来就设了 noindex、canonical 指向新版，那是站点有意的，不查这两项。
    pub primary: Vec<String>,
    /// 上线前同一批页面已经有的问题（路径 → 问题）：上线后只报新出现的，本来就有的不算这次改动的（见 `before`）
    pub before: std::collections::HashMap<String, Vec<String>>,
}

#[derive(Clone, Default, Debug)]
pub struct Report {
    pub checks: Vec<Check>,
    pub hard: Vec<String>,
    pub soft: Vec<String>,
    /// 首页本身打不开（多半是整站或 CDN 的问题，不是某条改动的）
    pub home_down: bool,
    /// 严重失败看起来是部署没到位（构建产物里有这个页面，线上却打不开）：值得重新上传一次
    pub deploy_issue: bool,
    /// 打开正常、没有页面级问题的网址（可以提交 IndexNow）
    pub passed: Vec<String>,
}

const MAX_PAGES: usize = 12;
/// 「验收被防火墙拦住」这条待核对的固定开头：修复任务和一起上线的条目都按它过滤（AI 改不了 WAF 规则）
pub const CHALLENGED: &str = "验收被网站的防火墙（人机验证）拦住";
const MAX_LINKS: usize = 15;

struct Fetched {
    status: u16,
    body: String,
    /// 跟完跳转之后的地址（去掉防缓存参数）
    final_url: String,
    /// 被 Cloudflare 这类防火墙的人机验证拦住了（403 / 429 / 503 + cf-mitigated 或挑战页）：页面本身多半没事，只是查不了
    challenged: bool,
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Fetched, String> {
    // 带一个随机参数绕开缓存，看到的是刚上线的版本
    let sep = if url.contains('?') { '&' } else { '?' };
    let busted = format!("{url}{sep}_pv={}", crate::now_secs());
    let r = client.get(&busted).send().await.map_err(|e| if e.is_timeout() { "超时".to_string() } else { "打不开".to_string() })?;
    let status = r.status().as_u16();
    let success = r.status().is_success();
    let final_url = r.url().as_str().split(['?', '#']).next().unwrap_or("").to_string();
    let mitigated = r.headers().get("cf-mitigated").is_some();
    let suspicious = matches!(status, 403 | 429 | 503);
    let body = if success || suspicious { r.text().await.unwrap_or_default() } else { String::new() };
    let challenged = suspicious && (mitigated || body.contains("challenge-platform") || body.contains("Just a moment") || body.contains("cf-chl"));
    let body = if success { body } else { String::new() };
    Ok(Fetched { status, body: body.chars().take(2_000_000).collect(), final_url, challenged })
}

/// 「新内容已生效」没过的那条，过后用现在的对照规则再查一次：这些页面里找得到这次加的任何一段文字就算生效。
/// 页面都打不开时返回 None（查不了，不下结论）。
pub async fn snippets_live(urls: &[String], snippets: &[String]) -> Option<bool> {
    let snippets: Vec<String> = snippets.iter().flat_map(|s| split_placeholders(s)).map(|x| strip_md(x.trim())).filter(|x| x.chars().count() >= 4 && !x.starts_with('|')).collect();
    if snippets.is_empty() {
        return None;
    }
    let client = client();
    let mut text = String::new();
    for u in urls.iter().take(MAX_PAGES) {
        if let Ok(f) = fetch(&client, u).await {
            if (200..300).contains(&f.status) {
                text.push_str(&format!("{}\n{}\n{}\n", html_title(&f.body), meta_content(&f.body, "description"), visible_text(&f.body)));
            }
        }
    }
    (!text.is_empty()).then(|| snippets.iter().any(|s| page_has(&text, s)))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(8))
        .user_agent("Mozilla/5.0 (compatible; CligcPilot-Verify/1.0)")
        .build()
        .unwrap_or_default()
}

fn path_of(url: &str) -> String {
    url.split_once("://").and_then(|(_, r)| r.find('/').map(|i| r[i..].to_string())).unwrap_or_else(|| "/".into())
}

pub(crate) fn same_page(a: &str, b: &str) -> bool {
    let norm = |u: &str| {
        let u = u.split(['?', '#']).next().unwrap_or(u).to_lowercase();
        let u = u.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.");
        // Pages / 多数静态站把 a.html 和 /a、/a/index.html 当同一页
        u.trim_end_matches('/').trim_end_matches("/index.html").trim_end_matches(".html").trim_end_matches('/').to_string()
    };
    norm(a) == norm(b)
}

/// 打开这次要验收的页面（含首页），按 `patient` 决定重试多久，然后做内容和 SEO 检查。
pub async fn run(spec: &Spec, job: &Job, patient: bool) -> Report {
    let mut rep = Report::default();
    if spec.domain.trim().is_empty() {
        return rep;
    }
    let home = format!("https://{}/", spec.domain);
    let mut urls: Vec<String> = vec![home.clone()];
    // 新页面和主要页面先排，之前合并一起上线的页面多时也不会把它们挤出去
    for u in spec.new_pages.iter().chain(&spec.primary).chain(&spec.pages) {
        if !urls.iter().any(|x| same_page(x, u)) && urls.len() < MAX_PAGES + 1 {
            urls.push(u.clone());
        }
    }
    // 首页本身就是改到的页面：内容对照和 SEO 检查也要看它（否则只拿它查「能不能打开」）
    let home_in_spec = spec.pages.iter().chain(&spec.new_pages).chain(&spec.primary).any(|u| same_page(u, &home));
    let first = if home_in_spec { 0 } else { 1 };
    let client = client();
    let waits: &[u64] = if patient { &[0, 30, 60, 120, 120] } else { &[0, 30] };
    let mut got: Vec<Option<Fetched>> = urls.iter().map(|_| None).collect();
    let mut last: Vec<String> = urls.iter().map(|_| String::new()).collect();
    // 早先记下的文字可能带着 %s 这类占位（代码里的格式化字符串），线上不会原样出现：切开只用字面的几段
    // 早先记下的还可能带着 Markdown 记号（**加粗**、[链接]、列表的「- 」），线上是渲染后的文字：先去掉
    let snippets: Vec<String> = spec.snippets.iter().flat_map(|s| split_placeholders(s)).map(|x| strip_md(x.trim())).filter(|x| x.chars().count() >= 4 && !x.starts_with("//") && !x.starts_with("/*") && !x.starts_with('|')).collect();
    let mut snippets_ok = snippets.is_empty();
    for (n, w) in waits.iter().enumerate() {
        if *w > 0 {
            let pending = got.iter().filter(|g| g.is_none()).count();
            let why = if pending > 0 { format!("还有 {pending} 个页面没打开正常，") } else { "新内容还没在线上出现，".into() };
            job.line(&format!("线上验收：{why}{w} 秒后再查一次（第 {}/{} 次）", n + 1, waits.len()));
            // 步骤条下面也写上，不然看起来像卡住了
            job.step("verify", "running", &format!("{why}等 {w} 秒再查（第 {}/{} 次，最多等 {} 分钟）", n + 1, waits.len(), waits.iter().sum::<u64>().div_ceil(60)));
            for _ in 0..*w {
                if job.canceled() {
                    return rep;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        for (i, u) in urls.iter().enumerate() {
            if got[i].is_some() {
                continue;
            }
            match fetch(&client, u).await {
                // 不存在的页面被站点跳到首页（软 404）：对新页面 / 改到的页面来说就是没上线
                Ok(f) if i > 0 && (200..300).contains(&f.status) && !f.final_url.is_empty() && same_page(&f.final_url, &home) && !same_page(u, &home) => last[i] = "跳到了首页（页面不存在）".into(),
                Ok(f) if (200..300).contains(&f.status) || f.challenged => got[i] = Some(f),
                Ok(f) => last[i] = format!("返回 {}", f.status),
                Err(e) => last[i] = e,
            }
        }
        if !snippets_ok {
            // 改标题 / 描述的改动，新文字只在 <title> 和 meta description 里，正文里没有：一起找
            let text: String = got.iter().skip(first).flatten().map(|f| format!("{}\n{}\n{}", html_title(&f.body), meta_content(&f.body, "description"), visible_text(&f.body))).collect::<Vec<_>>().join("\n");
            snippets_ok = snippets.iter().any(|s| page_has(&text, s));
        }
        if got.iter().all(|g| g.is_some()) && snippets_ok {
            break;
        }
    }
    // 被防火墙的人机验证拦住的：不算打不开，也查不了内容，记一条待核对
    let challenged: Vec<String> = urls.iter().enumerate().filter(|(i, _)| got[*i].as_ref().is_some_and(|f| f.challenged)).map(|(_, u)| path_of(u)).collect();
    if !challenged.is_empty() {
        rep.soft.push(format!("{CHALLENGED}，查不了 {}；可以给 User-Agent「CligcPilot-Verify」加一条放行规则", challenged.join("、")));
    }
    // 页面能打开（首页写成「首页 …」：首页打不开多半是整站或 CDN 的问题，上线后复查按它判断要不要回滚）
    for (i, u) in urls.iter().enumerate() {
        let label = if i == 0 { "首页能打开".to_string() } else { format!("{} 能打开", path_of(u)) };
        match &got[i] {
            Some(_) => rep.checks.push(Check { label, ok: true, detail: String::new(), hard: true }),
            None => {
                let msg = if i == 0 { format!("首页 {}", last[i]) } else { format!("{} {}", path_of(u), last[i]) };
                if i == 0 {
                    rep.home_down = true;
                }
                if last[i].contains("404") && spec.built.iter().any(|b| same_page(b, u)) {
                    rep.deploy_issue = true;
                }
                rep.checks.push(Check { label, ok: false, detail: last[i].clone(), hard: true });
                rep.hard.push(msg);
            }
        }
    }
    // 新内容已生效
    if !snippets.is_empty() {
        let s = snippets.first().cloned().unwrap_or_default();
        rep.checks.push(Check {
            label: "新内容已生效".into(),
            ok: snippets_ok,
            detail: if snippets_ok { String::new() } else { format!("线上页面里没找到这次加的文字（如「{s}」），可能缓存还没刷新，或者内容出现在别的页面") },
            hard: false,
        });
        if !snippets_ok {
            rep.soft.push(format!("这次加的内容在验收的页面里没找到（如「{s}」）"));
        }
    }
    // 每个改到的页面：SEO 基本项和内部痕迹
    let mut page_bad: HashSet<usize> = HashSet::new();
    for (i, u) in urls.iter().enumerate().skip(first) {
        let Some(f) = &got[i] else { continue };
        if f.challenged || hidden_page(&f.body) || !is_html(&f.body) {
            continue;
        }
        let p = path_of(u);
        let main = spec.new_pages.iter().chain(&spec.primary).any(|x| same_page(x, u));
        let mut issues = page_issues(f, u, main);
        // 上线前这一页就有的问题不算这次改动的（旧文本来就 noindex、sitemap 本来就没有标题…）
        if let Some(old) = spec.before.get(&p) {
            issues.retain(|x| !old.contains(x));
        }
        rep.checks.push(Check { label: if main { format!("{p} 标题、描述、canonical") } else { format!("{p} 标题和描述") }, ok: issues.is_empty(), detail: issues.join("；"), hard: false });
        if !issues.is_empty() {
            page_bad.insert(i);
            rep.soft.push(format!("{p}：{}", issues.join("；")));
        }
    }
    // 新页面：在 sitemap 里、站内链接能打开
    let new_idx: Vec<usize> = urls
        .iter()
        .enumerate()
        .filter(|(i, u)| spec.new_pages.iter().any(|n| same_page(n, u)) && !got[*i].as_ref().is_some_and(|f| hidden_page(&f.body)))
        .map(|(i, _)| i)
        .collect();
    if !new_idx.is_empty() {
        let locs = sitemap_locs(&client, &spec.domain).await;
        if locs.is_empty() {
            rep.checks.push(Check { label: "新页面在 sitemap 里".into(), ok: false, detail: "没找到 sitemap.xml".into(), hard: false });
            rep.soft.push("没找到站点的 sitemap.xml，新页面可能不容易被发现".into());
        } else {
            let missing: Vec<String> = new_idx.iter().map(|i| &urls[*i]).filter(|u| !locs.iter().any(|l| same_page(l, u))).map(|u| path_of(u)).collect();
            rep.checks.push(Check { label: "新页面在 sitemap 里".into(), ok: missing.is_empty(), detail: missing.join("、"), hard: false });
            if !missing.is_empty() {
                rep.soft.push(format!("新页面不在 sitemap 里：{}", missing.join("、")));
            }
        }
        let mut links: Vec<String> = Vec::new();
        for i in &new_idx {
            if let Some(f) = &got[*i] {
                for l in internal_links(&f.body, &spec.domain) {
                    if !links.contains(&l) && !urls.iter().any(|u| same_page(u, &l)) {
                        links.push(l);
                    }
                }
            }
        }
        links.truncate(MAX_LINKS);
        let mut broken = Vec::new();
        for l in &links {
            if let Ok(f) = fetch(&client, l).await {
                if f.status == 404 || f.status == 410 {
                    broken.push(path_of(l));
                }
            }
        }
        if !links.is_empty() {
            rep.checks.push(Check { label: format!("新页面的站内链接（{} 个）能打开", links.len()), ok: broken.is_empty(), detail: broken.join("、"), hard: false });
        }
        if !broken.is_empty() {
            rep.soft.push(format!("新页面里有打不开的站内链接：{}", broken.join("、")));
        }
    }
    rep.passed = urls.iter().enumerate().skip(first).filter(|(i, _)| got[*i].as_ref().is_some_and(|f| !f.challenged) && !page_bad.contains(i)).map(|(_, u)| u.clone()).collect();
    rep
}

fn lower_find(hay: &str, needle: &str, from: usize) -> Option<usize> {
    hay.get(from..)?.to_ascii_lowercase().find(needle).map(|i| i + from)
}

fn html_title(html: &str) -> String {
    let Some(a) = lower_find(html, "<title", 0) else { return String::new() };
    let Some(gt) = html[a..].find('>') else { return String::new() };
    let start = a + gt + 1;
    let end = lower_find(html, "</title>", start).unwrap_or(start);
    decode(html[start..end].trim())
}

/// 第 n 个标签的属性值（简单解析，够用于 meta / link）。
fn attr(tag: &str, name: &str) -> String {
    let lower = tag.to_ascii_lowercase();
    let key = format!("{name}=");
    let mut from = 0;
    while let Some(i) = lower[from..].find(&key) {
        let at = from + i;
        let ok_before = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = &tag[at + key.len()..];
        if ok_before {
            let q = rest.chars().next().unwrap_or(' ');
            return if q == '"' || q == '\'' { rest[1..].split(q).next().unwrap_or("").to_string() } else { rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string() };
        }
        from = at + key.len();
    }
    String::new()
}

fn tags<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find(&open) {
        let at = from + i;
        let end = html[at..].find('>').map(|e| at + e + 1).unwrap_or(html.len());
        out.push(&html[at..end]);
        from = end;
    }
    out
}

/// 一个页面的 SEO 基本项和内部痕迹问题。`main`：这条任务本身要改的页面 / 新页面，另外查 noindex 和 canonical。
fn page_issues(f: &Fetched, u: &str, main: bool) -> Vec<String> {
    let mut issues = Vec::new();
    if html_title(&f.body).is_empty() {
        issues.push("没有标题".to_string());
    }
    if meta_content(&f.body, "description").is_empty() {
        issues.push("没有描述（meta description）".to_string());
    }
    if main && meta_content(&f.body, "robots").to_lowercase().contains("noindex") {
        issues.push("设置了 noindex，搜索引擎不会收录".to_string());
    }
    let canon = canonical(&f.body);
    // 和跟完跳转后的地址比（旧地址 301 到新地址时，canonical 指向新地址是对的）
    let landed = if f.final_url.is_empty() { u.to_string() } else { f.final_url.clone() };
    if main && !canon.is_empty() && !same_page(&canon, &landed) {
        issues.push(format!("canonical 指向了 {canon}"));
    }
    if let Some(m) = crate::managed::leak_marker(&visible_text(&f.body)) {
        issues.push(format!("页面里有内部痕迹「{m}」"));
    }
    issues
}

/// 上线前先查一遍这次要验收的已有页面（新页面上线前还不存在，不查），记下本来就有的问题：路径 → 问题。
/// 上线后的验收只报新出现的，不把页面原来的状态算成这次改动的问题。打不开、被防火墙拦住的页面不记。
pub async fn snapshot(spec: &Spec) -> std::collections::HashMap<String, Vec<String>> {
    let mut out = std::collections::HashMap::new();
    if spec.domain.trim().is_empty() {
        return out;
    }
    let client = client();
    let home = format!("https://{}/", spec.domain);
    let mut urls: Vec<String> = vec![home];
    for u in spec.primary.iter().chain(&spec.pages) {
        if !spec.new_pages.iter().any(|n| same_page(n, u)) && !urls.iter().any(|x| same_page(x, u)) && urls.len() < MAX_PAGES + 1 {
            urls.push(u.clone());
        }
    }
    for u in &urls {
        let Ok(f) = fetch(&client, u).await else { continue };
        if !(200..300).contains(&f.status) || f.challenged || hidden_page(&f.body) || !is_html(&f.body) {
            continue;
        }
        let main = spec.primary.iter().any(|x| same_page(x, u));
        let issues = page_issues(&f, u, main);
        if !issues.is_empty() {
            out.insert(path_of(u), issues);
        }
    }
    out
}

/// 是不是网页：AI 改了生成 sitemap / RSS / llms.txt 的代码时会把 /sitemap.xml 这类报成改到的页面，
/// 它们本来就没有标题和描述，不按网页查。
fn is_html(body: &str) -> bool {
    let head: String = body.chars().take(4000).collect::<String>().to_ascii_lowercase();
    head.contains("<html") || head.contains("<!doctype html") || head.contains("<head") || head.contains("<title")
}

/// 同时 noindex、nofollow 的页面：站点有意藏起来的内部页面（比如给分享卡片截图用的 /og-card/），
/// 不查标题描述、收录和 sitemap。只 noindex 的照查（新文章误设 noindex 是真问题）。
fn hidden_page(html: &str) -> bool {
    let r = meta_content(html, "robots").to_lowercase();
    r.contains("noindex") && r.contains("nofollow")
}

pub(crate) fn meta_content(html: &str, name: &str) -> String {
    tags(html, "meta").into_iter().find(|t| attr(t, "name").eq_ignore_ascii_case(name)).map(|t| decode(&attr(t, "content"))).unwrap_or_default().trim().to_string()
}

fn canonical(html: &str) -> String {
    tags(html, "link").into_iter().find(|t| attr(t, "rel").eq_ignore_ascii_case("canonical")).map(|t| attr(t, "href")).unwrap_or_default()
}

fn internal_links(html: &str, domain: &str) -> Vec<String> {
    let bare = domain.trim_start_matches("www.");
    let mut out = Vec::new();
    for t in tags(html, "a") {
        let href = decode(&attr(t, "href"));
        let href = href.split('#').next().unwrap_or("").trim().to_string();
        if href.is_empty() || href.starts_with("//") {
            continue;
        }
        let abs = if href.starts_with('/') {
            format!("https://{domain}{href}")
        } else if let Some(rest) = href.strip_prefix("https://").or_else(|| href.strip_prefix("http://")) {
            let host = rest.split('/').next().unwrap_or("");
            if host.trim_start_matches("www.") != bare {
                continue;
            }
            href.clone()
        } else {
            continue;
        };
        let path = path_of(&abs).to_lowercase();
        if [".css", ".js", ".png", ".jpg", ".jpeg", ".webp", ".avif", ".svg", ".gif", ".ico", ".xml", ".txt", ".pdf", ".mp4"].iter().any(|e| path.split('?').next().unwrap_or("").ends_with(e)) {
            continue;
        }
        if !out.contains(&abs) {
            out.push(abs);
        }
    }
    out
}

async fn sitemap_locs(client: &reqwest::Client, domain: &str) -> Vec<String> {
    // 先看 robots.txt 里声明的 Sitemap:（放在非标准路径的站点只有这里能找到），再试常见路径
    let mut queue: Vec<String> = ["sitemap.xml", "sitemap-index.xml", "sitemap_index.xml"].iter().map(|p| format!("https://{domain}/{p}")).collect();
    if let Ok(r) = fetch(client, &format!("https://{domain}/robots.txt")).await {
        if (200..300).contains(&r.status) {
            for l in r.body.lines() {
                if let Some(u) = l.trim().strip_prefix("Sitemap:").or_else(|| l.trim().strip_prefix("sitemap:")) {
                    queue.push(u.trim().to_string());
                }
            }
        }
    }
    let mut seen = HashSet::new();
    let mut locs = Vec::new();
    let mut fetched = 0;
    while let Some(u) = queue.pop() {
        if !seen.insert(u.clone()) || fetched >= 30 {
            continue;
        }
        let Some(body) = fetch_sitemap(client, &u).await else { continue };
        fetched += 1;
        let lower = body.to_ascii_lowercase();
        let mut from = 0;
        while let Some(i) = lower[from..].find("<loc>") {
            let s = from + i + 5;
            let e = lower[s..].find("</loc>").map(|x| s + x).unwrap_or(s);
            let loc = decode(body[s..e].trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>").trim());
            if loc.ends_with(".xml") || loc.ends_with(".xml.gz") {
                queue.push(loc);
            } else {
                locs.push(loc);
            }
            from = e;
        }
    }
    locs
}

/// 取一份 sitemap 的文字；`.xml.gz` 按字节取回来解压（服务器没替我们解的话）。打不开或不是 2xx 返回 None。
async fn fetch_sitemap(client: &reqwest::Client, url: &str) -> Option<String> {
    if !url.ends_with(".gz") {
        let f = fetch(client, url).await.ok()?;
        return (200..300).contains(&f.status).then_some(f.body);
    }
    let r = client.get(url).send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    let bytes = r.bytes().await.ok()?;
    Some(gunzip_text(&bytes))
}

/// gzip 开头的解压（最多 20MB，防压缩炸弹），不是 gzip 的原样当文字。
fn gunzip_text(b: &[u8]) -> String {
    use std::io::Read;
    if !b.starts_with(&[0x1f, 0x8b]) {
        return String::from_utf8_lossy(b).into_owned();
    }
    let mut out = Vec::new();
    let _ = flate2::read::GzDecoder::new(b).take(20_000_000).read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

fn decode(s: &str) -> String {
    let s = s
        .replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ")
        .replace("&#8217;", "'").replace("&rsquo;", "'").replace("&#8216;", "'").replace("&lsquo;", "'")
        .replace("&#8220;", "\"").replace("&ldquo;", "\"").replace("&#8221;", "\"").replace("&rdquo;", "\"")
        .replace("&#8211;", "-").replace("&ndash;", "-").replace("&#8212;", "-").replace("&mdash;", "-").replace("&hellip;", "...").replace("&#8230;", "...")
        .replace("&amp;", "&");
    s
}

/// 排版替换（smartypants 把 ' 变 ’、" 变 “”、-- 变 –）：比对前两边都换回普通字符
fn plain_punct(s: &str) -> String {
    s.chars().map(|c| match c { '’' | '‘' | '´' => '\'', '“' | '”' | '„' => '"', '–' | '—' => '-', '\u{ad}' => '\0', _ => c }).filter(|c| *c != '\0').collect::<String>().replace('…', "...")
}

/// 页面上看得到的文字：去掉 script / style 和标签，解码常见实体，空白压成一个空格。
pub fn visible_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let b = html.as_bytes();
    while i < html.len() {
        if b[i] == b'<' {
            for skip in ["<script", "<style", "<noscript"] {
                if lower[i..].starts_with(skip) {
                    let close = format!("</{}", &skip[1..]);
                    i = lower[i..].find(&close).map(|e| i + e).unwrap_or(html.len());
                    break;
                }
            }
            if i >= html.len() {
                break;
            }
            i = html[i..].find('>').map(|e| i + e + 1).unwrap_or(html.len());
            out.push(' ');
            continue;
        }
        let next = html[i..].find('<').map(|e| i + e).unwrap_or(html.len());
        out.push_str(&html[i..next]);
        i = next;
    }
    decode(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 从这次的改动里挑几段有辨识度的文字，用来确认线上已经是新内容。
/// 只挑像正文的句子（够长、主要是文字、不像代码或网址），截到第一个标点，避免被页面里的加粗、链接拆开。
pub fn snippets_from_diff(diff: &str) -> Vec<String> {
    let mut cands: Vec<String> = Vec::new();
    let mut file = "";
    let mut in_script = false;
    for line in diff.lines() {
        if let Some(f) = line.strip_prefix("+++ ") {
            file = f.strip_prefix("b/").unwrap_or(f);
            in_script = false;
            continue;
        }
        let Some(added) = line.strip_prefix('+') else { continue };
        // 不会出现在页面上的文件：Pilot 自己的记录、README / 文档 / 测试 / 脚本 / 多语言文案（语言未必是验收的页面）
        if added.starts_with("++") || off_page_file(file) {
            continue;
        }
        // 代码注释（// 、/* */、HTML 注释、JSX 注释）不会出现在页面上；<script> 里的文字（结构化数据的描述）也不会，跨行的也跳过
        let a = added.trim_start();
        let lower = a.to_ascii_lowercase();
        // 模板 / 页面文件（.astro / .html / .tsx…）：只拿标签之间的正文，属性值（alt、class…）线上看不到；
        // 标题 / 描述属性和 frontmatter 里的 title / description 赋值除外（它们会渲染成标题和描述）
        let pageish = [".astro", ".html", ".htm", ".tsx", ".jsx", ".vue", ".svelte", ".tmpl", ".gohtml", ".njk", ".liquid", ".hbs"].iter().any(|e| file.ends_with(e));
        // <script> 里的文字跨行也跳过（只对模板文件：Markdown 正文里提到 <script> 这个词不算）
        if pageish {
            if in_script {
                if lower.contains("</script") {
                    in_script = false;
                }
                continue;
            }
            if lower.contains("<script") {
                in_script = !lower.contains("</script");
                continue;
            }
        } else if lower.starts_with("<script") {
            continue;
        }
        if ["//", "/*", "*", "<!--", "{/*"].iter().any(|p| a.starts_with(p)) {
            continue;
        }
        // Markdown 正文：去掉记号后的整行（表格行渲染后格子之间隔开了，不拿）；frontmatter 的「key: "…"」照旧取引号里的
        let md = [".md", ".mdx", ".markdown"].iter().any(|e| file.ends_with(e));
        let yaml_key = a.split_once(':').is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        let texts: Vec<String> = if md && a.starts_with('|') {
            continue;
        } else if md && !yaml_key {
            vec![strip_md(&visible_text(added))]
        } else if pageish {
            let titled = ["title=", "description=", "content=", "title:", "description:", "const title", "const description", "let title", "let description"].iter().any(|k| lower.contains(k));
            if titled {
                quoted(added)
            } else if a.contains('<') {
                vec![visible_text(added)]
            } else if !a.contains(['=', '{', '}', ';']) && (a.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).count() >= 8 || a.split_whitespace().count() >= 5) {
                // prettier 把长文本单独放一行（<p> 的内容在下一行）
                vec![a.to_string()]
            } else {
                continue;
            }
        // 字符串里的文字（数据文件），或去掉标签后的整行（页面文件）；没有引号又像代码的行（赋值、语句）不算正文
        } else if added.contains('\'') || added.contains('"') {
            quoted(added)
        } else if a.contains(" = ") || a.contains("=>") || a.trim_end().ends_with(';') {
            continue;
        } else {
            vec![visible_text(added)]
        };
        // 代码里的格式化字符串（Go 的 fmt.Sprintf、printf）：%s / %d 这些占位线上会换成别的字，按占位切开，只拿字面的几段
        for t in texts.iter().flat_map(|t| split_placeholders(t)) {
            let t = t.trim();
            if t.contains("://") || t.contains('{') || t.contains('<') || t.starts_with('#') {
                continue;
            }
            let cut: String = t.chars().take_while(|c| !"，。；：！？,.;:!?（）()「」“”\"".contains(*c)).collect();
            let cut = cut.trim().to_string();
            let cjk = cut.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).count();
            let words = cut.split_whitespace().count();
            if (cjk >= 8 || (cjk == 0 && words >= 5 && cut.len() >= 30)) && cut.chars().count() <= 40 && !cands.contains(&cut) {
                cands.push(cut);
            }
        }
    }
    // 取开头、中间、结尾各一段
    let n = cands.len();
    let mut out = Vec::new();
    for i in [0, n / 2, n.saturating_sub(1)] {
        if let Some(c) = cands.get(i) {
            if !out.contains(c) {
                out.push(c.clone());
            }
        }
    }
    out
}

/// 去掉 Markdown 记号，剩下页面上看得到的文字：行首的列表 / 引用 / 标题记号，**加粗**、`代码`、~~删除线~~，
/// [文字](网址) 只留文字，![图](网址) 整个去掉。
fn strip_md(s: &str) -> String {
    let mut t = s.trim_start();
    loop {
        let before = t;
        for p in ["- ", "* ", "+ ", "> ", "#"] {
            t = t.strip_prefix(p).unwrap_or(t).trim_start();
        }
        let digits = t.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && t[digits..].starts_with(". ") {
            t = t[digits + 2..].trim_start();
        }
        if t == before {
            break;
        }
    }
    let mut out = String::with_capacity(t.len());
    let mut it = t.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '*' | '`' | '~' => {}
            '_' if it.peek() == Some(&'_') => {
                it.next();
            }
            '!' if it.peek() == Some(&'[') => {
                // 图片：跳过 [alt] 和 (网址)
                for d in it.by_ref() {
                    if d == ']' {
                        break;
                    }
                }
                if it.peek() == Some(&'(') {
                    for d in it.by_ref() {
                        if d == ')' {
                            break;
                        }
                    }
                }
            }
            '[' => {}
            ']' => {
                if it.peek() == Some(&'(') {
                    for d in it.by_ref() {
                        if d == ')' {
                            break;
                        }
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// 线上页面里有没有这段文字。不看空白（加粗、链接渲染成标签后，中文之间会多出空格）；
/// 站点构建时把简体转成繁体（或反过来）的，汉字对不上：其它字符一样、汉字至少一半相同、长度相同就算找到。
fn page_has(text: &str, snippet: &str) -> bool {
    let squash = |s: &str| plain_punct(s).chars().filter(|c| !c.is_whitespace()).collect::<Vec<char>>();
    let (hay, needle) = (squash(text), squash(snippet));
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    let cjk = needle.iter().filter(|c| is_cjk(**c)).count();
    hay.windows(needle.len()).any(|w| {
        let mut same = 0;
        for (a, b) in w.iter().zip(&needle) {
            if a == b {
                same += is_cjk(*b) as usize;
            } else if !(is_cjk(*a) && is_cjk(*b)) {
                return false;
            }
        }
        same * 2 >= cjk
    })
}

/// 像源文件路径的（/src/pages/a.astro、/content/x.md）：不是页面网址
fn source_path(w: &str) -> bool {
    let l = w.to_ascii_lowercase();
    [".astro", ".md", ".mdx", ".ts", ".tsx", ".js", ".jsx", ".vue", ".svelte", ".json", ".yml", ".yaml", ".css", ".scss", ".toml"].iter().any(|e| l.ends_with(e))
        || l.starts_with("/src/") || l.starts_with("/content/") || l.starts_with("/public/") || l.starts_with("/scripts/")
}

/// 不会渲染成页面的文件：Pilot 记录、文档、测试、脚本、CI、多语言文案目录
fn off_page_file(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    let lower = name.to_ascii_lowercase();
    file.starts_with(".pilot/")
        || file.contains("/.pilot/")
        || lower.starts_with("readme")
        || lower.starts_with("changelog")
        || matches!(lower.as_str(), "claude.md" | "agents.md" | "license" | "license.md")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        // docs/ 不排除：VitePress / Docusaurus / Starlight 的页面就在 docs 下
        || file.split('/').any(|s| matches!(s, ".github" | "__tests__" | "tests" | "test" | "fixtures" | "scripts" | "i18n" | "locales"))
}

/// 按 printf 风格的占位（%s、%d、%v、%5.2f、%-10s…）把字符串切成几段字面文字；%% 是字面的百分号。
fn split_placeholders(t: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut it = t.chars().peekable();
    while let Some(c) = it.next() {
        if c != '%' {
            out.last_mut().unwrap().push(c);
            continue;
        }
        if it.peek() == Some(&'%') {
            it.next();
            out.last_mut().unwrap().push('%');
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = it.peek() {
            if (d.is_ascii_digit() || "+-#.".contains(d)) && spec.len() < 6 {
                spec.push(d);
                it.next();
            } else {
                break;
            }
        }
        match it.peek() {
            Some(&v) if "sdvqfxXegtcboTp".contains(v) => {
                it.next();
                out.push(String::new());
            }
            _ => {
                out.last_mut().unwrap().push('%');
                out.last_mut().unwrap().push_str(&spec);
            }
        }
    }
    out
}

fn quoted(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\'' || c == '"' {
            let mut s = String::new();
            for d in chars.by_ref() {
                if d == c {
                    break;
                }
                s.push(d);
            }
            out.push(s);
        }
    }
    out
}

/// AI 改完在回复最后写的「新页面：…」「改动的页面：…」，取本站的完整网址。
pub fn reported_pages(reply: &str, domain: &str) -> (Vec<String>, Vec<String>) {
    let bare = domain.trim_start_matches("www.").to_ascii_lowercase();
    // 本站的完整网址，或以 / 开头的路径（补上域名）；去掉 markdown 的加粗、反引号、链接括号
    let pick = |line: &str| -> Vec<String> {
        line.split(|c: char| c.is_whitespace() || "，,、；;（）()<>`[]*".contains(c))
            .map(|w| w.trim_end_matches(['。', '.', '，', ',']))
            .filter(|w| !w.is_empty())
            .filter_map(|w| {
                if w.starts_with("https://") || w.starts_with("http://") {
                    let host = w.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or("").trim_start_matches("www.").to_ascii_lowercase();
                    (host == bare).then(|| format!("https://{}", w.trim_start_matches("https://").trim_start_matches("http://")))
                } else if w.starts_with('/') && w.len() > 1 && !w.contains("//") && !source_path(w) {
                    Some(format!("https://{bare}{w}"))
                } else {
                    None
                }
            })
            .take(10)
            .collect()
    };
    // 标题要「关键词 + 冒号」（中间可以有 ** 或空格）：正文里提到「新页面上线后…」的句子不算
    let heads = |t: &str, ks: &[&str]| { let l = t.to_ascii_lowercase(); ks.iter().any(|p| l.strip_prefix(p).is_some_and(|r| r.trim_start_matches(['*', ' ']).starts_with(['：', ':']))) };
    let is_new = |t: &str| heads(t, &["新页面", "新頁面", "新增页面", "新增頁面", "new pages", "new page"]);
    let is_changed = |t: &str| heads(t, &["改动的页面", "改動的頁面", "改动页面", "修改的页面", "changed pages", "modified pages", "updated pages"]);
    let (mut new, mut changed) = (Vec::new(), Vec::new());
    // 「新页面：」后面可能直接跟网址，也可能换行后一行一个（- / * / 1. 开头）：吃到空行或下一个标题为止
    let mut mode: Option<bool> = None;
    for l in reply.lines() {
        let raw = l.trim();
        // 续行只认列表行（- / * / 1.）：散文里的路径不算
        let listy = raw.starts_with(['-', '*']) || raw.chars().next().is_some_and(|c| c.is_ascii_digit()) && raw.contains(". ");
        let t = raw.trim_start_matches(['-', '*', '#', ' ']).trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ').trim_start_matches("**").trim();
        let t = t.trim_start_matches("**");
        let head = t.replace("**", "");
        if is_new(&head) || is_changed(&head) {
            let m = is_new(&head);
            let rest = head.split_once(['：', ':']).map(|(_, r)| r).unwrap_or("");
            let urls = pick(rest);
            if m { new.extend(urls) } else { changed.extend(urls) }
            mode = Some(m);
            continue;
        }
        if t.is_empty() {
            mode = None;
            continue;
        }
        if let Some(m) = mode {
            let urls = if listy { pick(&head) } else { Vec::new() };
            if urls.is_empty() {
                mode = None;
            } else if m {
                new.extend(urls)
            } else {
                changed.extend(urls)
            }
        }
    }
    new.dedup();
    changed.retain(|c| !new.contains(c));
    changed.dedup();
    (new, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzipped_sitemaps_are_unpacked() {
        use std::io::Write;
        let xml = "<urlset><url><loc>https://a.com/x/</loc></url></urlset>";
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(xml.as_bytes()).unwrap();
        assert_eq!(gunzip_text(&e.finish().unwrap()), xml);
        assert_eq!(gunzip_text(xml.as_bytes()), xml, "服务器已经解过压的原样用");
    }

    #[test]
    fn parses_head_tags() {
        let html = r#"<html><head><title>AI 视频转场提示词 &amp; 写法</title><meta name="description" content="讲清 5 种转场"><meta name=robots content="index,follow"><link rel="canonical" href="https://iosur.com/zh/posts/a/"></head><body><script>var TODO=1</script><h1>转场</h1><p>遮挡转场要让前景物体把画面整个挡住</p><a href="/zh/posts/b/">b</a><a href="https://www.iosur.com/en/">en</a><a href="https://x.com/">x</a><a href="/a.css">c</a></body></html>"#;
        assert_eq!(html_title(html), "AI 视频转场提示词 & 写法");
        assert_eq!(meta_content(html, "description"), "讲清 5 种转场");
        assert!(!meta_content(html, "robots").contains("noindex"));
        assert!(is_html(html));
        assert!(!is_html("<?xml version=\"1.0\" encoding=\"UTF-8\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\"><url><loc>https://cpsbee.com/</loc></url></urlset>"));
        assert!(!is_html("# cpsbee\n\n> 推广计划导航"));
        assert!(same_page(&canonical(html), "https://iosur.com/zh/posts/a"));
        let text = visible_text(html);
        assert!(text.contains("遮挡转场要让前景物体") && !text.contains("TODO"), "{text}");
        assert_eq!(internal_links(html, "iosur.com"), vec!["https://iosur.com/zh/posts/b/".to_string(), "https://www.iosur.com/en/".to_string()]);
    }

    #[test]
    fn snippets_are_found_in_title_and_description() {
        let html = "<html><head><title>MEXC（抹茶）上线币种查询：小币交易对与充提网络 - CEXQA</title><meta name=\"description\" content=\"想查某个小币在 MEXC 有没有上线、能不能交易？\"></head><body><h1>MEXC</h1></body></html>";
        let text = format!("{}\n{}\n{}", html_title(html), meta_content(html, "description"), visible_text(html));
        assert!(text.contains("有没有上线、能不能交易"));
        assert!(!visible_text(html).contains("有没有上线"), "正文里本来就没有，只能从 meta 里找");
    }

    #[test]
    fn code_comments_are_not_snippets() {
        let d = "+++ b/src/pages/[lang]/links/[slug]/index.astro\n+// 链接页没有发布 / 更新日期字段，所以这里不写 datePublished / dateModified\n+  /* 这里是给开发看的说明，页面上不会显示出来 */\n+<!-- 这一段是 HTML 注释，页面上也看不到的文字 -->\n+const note = 页面上看不到的一整句中文说明文字;\n";
        assert!(snippets_from_diff(d).is_empty(), "{:?}", snippets_from_diff(d));
        // 正文照旧
        let ok = "+++ b/src/content/a.md\n+合约一张是多少要看交易所的面值规定，不同币种不一样\n";
        assert_eq!(snippets_from_diff(ok), vec!["合约一张是多少要看交易所的面值规定"]);
    }

    #[test]
    fn markdown_and_script_conversion_do_not_hide_new_text() {
        // 线上验收误报：biannex（.pilot/todo.json、- **加粗**）、gateneo（[链接]、表格）、oaneo（构建时转成繁体）
        let d = "+++ b/.pilot/todo.json\n+    \"title\": \"欧易提币教程缺提币页面截图：登录后的资产页\",\n+++ b/src/content/guides/a.mdx\n+title: \"欧易怎么提币到钱包：网络和手续费\"\n+- **提到自己的个人钱包**：选对网络再提\n+OKX 的費率表與試算見 [OKX 手續費一覽](/zh-hant/guides/okx-fees/)。\n+| VIP 門檻 | 資產量或交易量擇一 | 交易量且 BNB 持倉 |\n";
        let s = snippets_from_diff(d);
        assert_eq!(s, vec!["欧易怎么提币到钱包", "提到自己的个人钱包", "OKX 的費率表與試算見 OKX 手續費一覽"], "{s:?}");
        assert_eq!(strip_md("1. 看 ![图](/a.png)**核准业务**栏，`交易平台`、[名单](/x#a)"), "看 核准业务栏，交易平台、名单");
        let page = visible_text("<p>OKX 的費率表與試算見 <a href=\"/zh-hant/guides/okx-fees/\">OKX 手續費一覽</a>。手續費看的是<strong>倉位價值</strong></p><p>TWEX 沒有交易平台業務，KryptoGO</p>");
        assert!(page_has(&page, "OKX 的費率表與試算見 OKX 手續費一覽"));
        assert!(page_has(&page, "手續費看的是倉位價值"));
        assert!(page_has(&page, "TWEX 没有交易平台业务"), "简体写的、线上是繁体");
        assert!(!page_has(&page, "TWEX 没有交易所的牌照和"), "汉字大半不同就不算");
        assert!(!page_has(&page, "OKX 没有交易平台业务"), "非汉字要一样");
    }

    #[test]
    fn reported_pages_accept_lists_and_relative_paths() {
        let reply = "改好了。\n\n**新页面：**\n- https://x.com/zh/a/\n- `/zh/b/`\n- /src/pages/c.astro\n\n改动的页面：\n1. [首页](https://www.x.com/)\n2. https://evil.com/\n新页面上线后记得看 /sitemap.xml\n\n谢谢";
        let (n, c) = reported_pages(reply, "x.com");
        assert_eq!(n, vec!["https://x.com/zh/a/", "https://x.com/zh/b/"]);
        assert_eq!(c, vec!["https://www.x.com/"]);
        // 模板里的标题 / 描述属性和 frontmatter 赋值照样拿
        let d3 = "+++ b/src/pages/a.astro\n+const description = \"这是写在 frontmatter 里的页面描述文字\";\n+<Layout title=\"欧易提币教程标题文字\">\n";
        let s3 = snippets_from_diff(d3);
        assert!(s3.iter().any(|x| x.contains("frontmatter 里的页面描述")) && s3.iter().any(|x| x.contains("欧易提币教程标题")), "{s3:?}");
        // Markdown 正文提到 <script> 这个词不会把后面的行吞掉
        let d4 = "+++ b/src/content/a.md\n+不要在页面里直接写 `<script>` 标签，交给构建处理\n+这一行是正文里真正会显示出来的说明文字\n";
        assert!(snippets_from_diff(d4).iter().any(|x| x.contains("真正会显示出来")), "{:?}", snippets_from_diff(d4));
        assert!(same_page("https://x.com/a.html", "http://www.x.com/a/"));
        assert!(same_page("https://x.com/a/index.html", "https://x.com/a"));
        assert!(page_has(&visible_text("<p>It’s “fine” – really</p>"), "It's \"fine\" - really"));
        // 模板里只拿标签之间的正文，不拿属性值；README / 测试 / i18n 不算
        let d = "+++ b/src/pages/a.astro\n+<img alt=\"一张展示欧易提币页面截图的图片\" src=x>\n+<p class=\"lead\">欧易提币到钱包要先选对网络再提币</p>\n+++ b/README.md\n+这一段是写给开发者看的说明文字不会上线\n+++ b/src/i18n/zh.json\n+  \"hello\": \"这是多语言文案里的一句话不该拿来对照\"\n";
        let s = snippets_from_diff(d);
        assert_eq!(s, vec!["欧易提币到钱包要先选对网络再提币"], "{s:?}");
        // 跨行的 <script> 里的文字也跳过
        let d2 = "+++ b/public/a.html\n+<script type=\"application/ld+json\">\n+{\"description\": \"结构化数据里的描述文字不显示在页面上\"}\n+</script>\n+<p>页面上真正显示出来的一段正文文字</p>\n";
        assert_eq!(snippets_from_diff(d2), vec!["页面上真正显示出来的一段正文文字"]);
    }

    #[test]
    fn json_ld_script_text_is_not_a_snippet() {
        // dexask 修 96 个页面的结构化数据：改的是 <script type="application/ld+json"> 里的 JSON，描述文字不显示在页面上
        let d = "+++ b/legacy/site/en/buy/usdc/turkey.html\n+  <script type=\"application/ld+json\">{\"@type\":\"Article\",\"description\":\"If TRY-to-USDC uses another asset first, check the quote before sending\",\"datePublished\":\"2026-09-21\"}</script>\n+<p>Turkey residents can buy USDC with TRY</p>\n";
        let s = snippets_from_diff(d);
        assert!(!s.iter().any(|x| x.contains("TRY-to-USDC")), "{s:?}");
        assert!(s.iter().any(|x| x.contains("Turkey residents can buy USDC")), "{s:?}");
    }

    #[test]
    fn format_placeholders_are_not_part_of_snippets() {
        let d = "+++ b/internal/site/build.go\n+\t\tdesc = fmt.Sprintf(\"想查某个小币在 %s 有没有上线、能不能交易？这里列出 %s 的 %s 个币种\", ex.Name)\n";
        let s = snippets_from_diff(d);
        assert!(!s.is_empty() && s.iter().all(|x| !x.contains('%')), "{s:?}");
        assert!(s.iter().any(|x| x == "有没有上线、能不能交易"), "{s:?}");
        assert_eq!(split_placeholders("涨了 100% 的币 %d 个 %-5.2f 元"), vec!["涨了 100% 的币 ", " 个 ", " 元"]);
    }

    #[test]
    fn snippets_and_reported_pages() {
        let diff = "+++ b/src/data/a.ts\n+      '遮挡转场要让前景物体把画面整个挡住，再在被挡住的那一帧切到下一段素材。',\n+    slug: 'ai-video-transition-prompts',\n+  title: 'Hailuo AI pricing 2026: plans, credits and the free tier explained',\n";
        let s = snippets_from_diff(diff);
        assert!(s.contains(&"遮挡转场要让前景物体把画面整个挡住".to_string()), "{s:?}");
        assert!(!s.iter().any(|x| x.contains("ai-video")));
        let reply = "改好了。\n新页面：https://iosur.com/zh/posts/x/ https://iosur.com/en/posts/x/\n改动的页面：https://iosur.com/zh/posts/ai-video-prompts/、https://evil.com/a/";
        let (n, c) = reported_pages(reply, "iosur.com");
        assert_eq!(n, vec!["https://iosur.com/zh/posts/x/", "https://iosur.com/en/posts/x/"]);
        assert_eq!(c, vec!["https://iosur.com/zh/posts/ai-video-prompts/"]);
    }
}

#[cfg(test)]
mod live {
    /// 真实站点：cargo test --lib site_verify::live -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn verify_live_iosur() {
        let jobs = crate::site_jobs::Jobs::default();
        let job = jobs.start(None, "t", "ops:watch", "t", &[("verify", "线上验收")]).unwrap();
        let spec = super::Spec {
            domain: "iosur.com".into(),
            pages: if std::env::var("KLING").is_ok() { vec!["https://iosur.com/zh/posts/kling-3-character-consistency/".into(), "https://iosur.com/zh/archive/ai-video-troubleshooting/".into(), "https://iosur.com/zh/archive/how-to-use-kling/".into(), "https://iosur.com/zh/archive/vidu-reference-tutorial/".into(), "https://iosur.com/zh/archive/kling-3-character-consistency/".into()] } else { vec!["https://iosur.com/zh/posts/ai-video-prompts/".into()] },
            primary: if std::env::var("KLING").is_ok() { vec!["https://iosur.com/zh/posts/kling-3-character-consistency/".into()] } else { vec![] },
            new_pages: if std::env::var("KLING").is_ok() { vec![] } else if std::env::var("BAD").is_ok() { vec!["https://iosur.com/zh/posts/pilot-verify-missing-page/".into()] } else { vec!["https://iosur.com/zh/posts/ai-video-transition-prompts/".into(), "https://iosur.com/en/posts/ai-video-transition-prompts/".into()] },
            snippets: if std::env::var("KLING").is_ok() { vec!["主体绑定".into()] } else { vec!["遮挡转场".into()] },
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let r = super::run(&spec, &job, false).await;
        for c in &r.checks {
            eprintln!("{} {} {}", if c.ok { "✓" } else { "✗" }, c.label, c.detail);
        }
        eprintln!("hard={:?}\nsoft={:?}\npassed={:?}\n{:?}", r.hard, r.soft, r.passed, t.elapsed());
    }
}
