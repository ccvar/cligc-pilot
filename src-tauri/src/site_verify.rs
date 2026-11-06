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
/// 域名打开的不是这个站点的项目（别的 Worker / 项目占着）：查的是别人的页面，和这次改动无关。不回滚、不让 AI 重做
pub const DOMAIN_TAKEN: &str = "域名没指向这个站点：";

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
    /// 这次构建出来的页面上新出现的几段文字（见 `snippets_from_build`）：在任何一个验收页面里找到，
    /// 或者改到的页面和上线前不一样了（`before_sig`），就算新内容已生效。
    /// 没有构建出来的页面（服务端渲染的站）就是空的，不查「新内容已生效」
    pub snippets: Vec<String>,
    /// 构建产物里有对应 HTML 的网址（静态站）：线上 404 说明是部署没到位，不是改动的问题
    pub built: HashSet<String>,
    /// 这条任务本身要改的页面：和新页面一起查 noindex / canonical。其它只是顺带改到的页面（比如旧文存档）
    /// 可能本来就设了 noindex、canonical 指向新版，那是站点有意的，不查这两项。
    pub primary: Vec<String>,
    /// 上线前同一批页面已经有的问题（路径 → 问题）：上线后只报新出现的，本来就有的不算这次改动的（见 `before`）
    pub before: std::collections::HashMap<String, Vec<String>>,
    /// 托管上线和复查：只报能确定是这次改动造成的问题（已有页面没有上线前的记录时分不清，不报）
    pub only_new: bool,
    /// 上线前这些页面内容的指纹（路径 → `page_sig`）：上线后变了，说明新版本已经在线上
    pub before_sig: std::collections::HashMap<String, u64>,
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
    /// 线上打不开的网址（包括上线前就打不开的）
    pub unopened: Vec<String>,
    /// 这次加的文字线上没找到
    pub missing_text: bool,
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

/// 这些页面里找得到任何一段对照文字就算生效；页面都打不开时返回 None（查不了，不下结论）。
/// 只有拿真实站点重放的测试在用（live::build_snippets_real）。
#[cfg(test)]
pub async fn snippets_live(urls: &[String], snippets: &[String]) -> Option<bool> {
    if snippets.is_empty() {
        return None;
    }
    let client = client();
    let mut text = String::new();
    for u in urls.iter().take(MAX_PAGES) {
        if let Ok(f) = fetch(&client, u).await {
            if (200..300).contains(&f.status) {
                text.push_str(&format!("{}\n", page_text(&f.body)));
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

pub(crate) fn path_of(url: &str) -> String {
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
    let snippets = &spec.snippets;
    let mut snippets_ok = snippets.is_empty();
    // 有没有拿上线前的页面比过（比过还一样，「没找到」才说得准）
    let mut compared = false;
    // 有页面被防火墙拦着：内容查不了，新内容在不在线上下不了结论（拦着这件事另有一条待核对）
    let mut blocked = false;
    // 文字没找到、靠「页面和上线前不一样了」算通过的
    let mut by_change = false;
    for (n, w) in waits.iter().enumerate() {
        if *w > 0 {
            let pending = urls.iter().enumerate().filter(|(i, u)| got[*i].is_none() && !broken_before(spec, u, &last[*i])).count();
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
            // 已经打开的页面，新内容还没出现时重新取一遍：不然多等几轮看的还是第一次取到的那份
            let had = got[i].is_some();
            if had && (snippets_ok || blocked || i < first) {
                continue;
            }
            match fetch(&client, u).await {
                // 不存在的页面被站点跳到首页（软 404）：对新页面 / 改到的页面来说就是没上线
                Ok(f) if i > 0 && (200..300).contains(&f.status) && !f.final_url.is_empty() && same_page(&f.final_url, &home) && !same_page(u, &home) => {
                    if !had {
                        last[i] = "跳到了首页（页面不存在）".into();
                    }
                }
                Ok(f) if (200..300).contains(&f.status) || (f.challenged && !had) => got[i] = Some(f),
                Ok(f) if !had => last[i] = format!("返回 {}", f.status),
                Err(e) if !had => last[i] = e,
                _ => {}
            }
        }
        blocked = got.iter().skip(first).flatten().any(|f| f.challenged);
        if !snippets_ok {
            // 改标题 / 描述的改动，新文字只在 <title> 和 meta description 里，正文里没有：一起找
            let text: String = got.iter().skip(first).flatten().map(|f| page_text(&f.body)).collect::<Vec<_>>().join("\n");
            snippets_ok = snippets.iter().any(|s| page_has(&text, s));
        }
        if !snippets_ok {
            // 文字没对上不等于没上线（上线时重新构建，日期、价格这类构建时生成的文字可能变了）：
            // 改到的页面和上线前不一样了，说明新版本已经在线上。和上线前一模一样才算「没找到」
            let before: Vec<bool> = urls.iter().enumerate().skip(first).filter_map(|(i, u)| {
                let f = got[i].as_ref().filter(|f| !f.challenged)?;
                spec.before_sig.get(&path_of(u)).map(|old| *old != page_sig(&f.body))
            }).collect();
            compared = !before.is_empty();
            by_change = before.iter().any(|changed| *changed);
            snippets_ok = by_change;
        }
        // 上线前就打不开的页面不用等：多等也不会变
        if urls.iter().enumerate().all(|(i, u)| got[i].is_some() || broken_before(spec, u, &last[i])) && (snippets_ok || blocked) {
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
                rep.unopened.push(u.clone());
                if broken_before(spec, u, &last[i]) {
                    job.line(&format!("{}：上线前就{}，不是这次造成的", path_of(u), last[i]));
                    rep.checks.push(Check { label, ok: false, detail: format!("上线前就{}", last[i]), hard: false });
                    continue;
                }
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
    // 新内容已生效（没找到、又有页面被防火墙拦着查不了：不下结论）
    if by_change {
        job.line("线上验收：没找到对照的文字，但改到的页面和上线前不一样了，按新内容已生效算");
    }
    if !snippets.is_empty() && (snippets_ok || !blocked) {
        let s = snippets.first().cloned().unwrap_or_default();
        rep.checks.push(Check {
            label: "新内容已生效".into(),
            ok: snippets_ok,
            detail: if by_change {
                format!("没在线上找到对照的文字（如「{s}」），但页面和上线前不一样了，按已生效算")
            } else if snippets_ok {
                String::new()
            } else if compared {
                format!("线上页面里没找到这次加的文字（如「{s}」），这些页面和上线前相比也没有变化")
            } else {
                format!("线上页面里没找到这次加的文字（如「{s}」），可能缓存还没刷新，或者内容出现在别的页面")
            },
            hard: false,
        });
        if !snippets_ok {
            rep.missing_text = true;
            rep.soft.push(format!("这次加的内容在验收的页面里没找到（如「{s}」）{}", if compared { "，这些页面和上线前相比也没有变化" } else { "" }));
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
        let is_new = spec.new_pages.iter().any(|x| same_page(x, u));
        let unknown = caused_by_this(spec, &p, is_new, &mut issues);
        if !unknown.is_empty() {
            job.line(&format!("{p}：{}。上线前没有这一页的记录，分不清是不是这次造成的，不算问题", unknown.join("；")));
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
/// 只留下这次改动造成的问题，返回分不清的那些（已有页面没有上线前的记录）。
/// 上线前这一页就有的问题不算这次改动的（旧文本来就 noindex、sitemap 本来就没有标题…）。
fn caused_by_this(spec: &Spec, page: &str, is_new: bool, issues: &mut Vec<String>) -> Vec<String> {
    match spec.before.get(page) {
        Some(old) => issues.retain(|x| !old.contains(x)),
        None if spec.only_new && !is_new => return std::mem::take(issues),
        None => {}
    }
    Vec::new()
}

pub async fn snapshot(spec: &Spec) -> (std::collections::HashMap<String, Vec<String>>, std::collections::HashMap<String, u64>) {
    let mut out = std::collections::HashMap::new();
    let mut sigs = std::collections::HashMap::new();
    if spec.domain.trim().is_empty() {
        return (out, sigs);
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
        // 打不开、被防火墙拦住：没有记录。其余的都记（没问题的记空），「上线前没问题」和「不知道」不是一回事
        let Ok(f) = fetch(&client, u).await else { continue };
        if f.challenged {
            continue;
        }
        // 上线前就不存在：记下来，上线后还是这样不算这次造成的（比如域名被别的东西占着）。其它错误多半是抖动，不记
        if matches!(f.status, 404 | 410) {
            out.insert(path_of(u), vec![format!("返回 {}", f.status)]);
            continue;
        }
        if !(200..300).contains(&f.status) {
            continue;
        }
        sigs.insert(path_of(u), page_sig(&f.body));
        let checkable = !hidden_page(&f.body) && is_html(&f.body);
        let main = spec.primary.iter().any(|x| same_page(x, u));
        out.insert(path_of(u), if checkable { page_issues(&f, u, main) } else { Vec::new() });
    }
    // 新页面只记指纹（问题不记：新页面的问题都算这次改动的）。上线前不存在记 0；
    // 有的站不存在的网址也返回首页的内容，那就记下那份内容，上线后不一样了才算新页面到了
    for u in spec.new_pages.iter().take(MAX_PAGES) {
        // 取不到（超时、断网）就不往下拍了：站点有问题时不在这里一个个等超时
        let Ok(f) = fetch(&client, u).await else { break };
        if f.challenged {
            continue;
        }
        if matches!(f.status, 404 | 410) {
            sigs.insert(path_of(u), 0);
        } else if (200..300).contains(&f.status) {
            sigs.insert(path_of(u), page_sig(&f.body));
        }
    }
    (out, sigs)
}

/// 这个页面现在打不开的样子（「返回 404」）和上线前记下的一样。
fn broken_before(spec: &Spec, url: &str, now: &str) -> bool {
    !now.is_empty() && spec.before.get(&path_of(url)).is_some_and(|old| old.iter().any(|x| x == now))
}

/// 这个网址现在能正常打开（被防火墙拦住的查不了，不算）。
pub async fn opens(url: &str) -> bool {
    fetch(&client(), url).await.is_ok_and(|f| (200..300).contains(&f.status) && !f.challenged)
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

pub(crate) async fn sitemap_locs(client: &reqwest::Client, domain: &str) -> Vec<String> {
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
    html_text(html, false)
}

/// 行内标签：前后的文字是连着的一句话。其它标签（段落、标题、表格格子、列表项…）两边是不同的块。
const INLINE_TAGS: &[&str] = &["a", "b", "strong", "em", "i", "span", "code", "u", "mark", "small", "sub", "sup", "abbr", "time", "s", "del", "ins", "q", "cite", "kbd", "var", "label", "font", "bdi", "wbr"];

/// `blocks`：每个块单独一行（导航、标题、段落不会连成一句），否则全部压成一行。
fn html_text(html: &str, blocks: bool) -> String {
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
            let breaks = blocks && {
                let name: String = lower[i + 1..].trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                !INLINE_TAGS.contains(&name.as_str())
            };
            i = html[i..].find('>').map(|e| i + e + 1).unwrap_or(html.len());
            out.push(if breaks { '\n' } else { ' ' });
            continue;
        }
        let next = html[i..].find('<').map(|e| i + e).unwrap_or(html.len());
        out.push_str(&html[i..next]);
        i = next;
    }
    if blocks {
        return decode(&out).lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    }
    decode(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 用来对照的页面文字：标题、描述、页面上看得到的文字。
fn page_text(html: &str) -> String {
    format!("{}\n{}\n{}", html_title(html), meta_content(html, "description"), visible_text(html))
}

/// 页面内容的指纹：标题、描述、看得到的文字、结构化数据、链接和图片。脚本、样式、链接里的防缓存参数不算
/// （同一个页面取两次，这些可能不一样）。内容本身每次请求都变的页面（实时行情）指纹也会变，
/// 那样的页面永远算「变了」：只会少报，不会多报。
pub fn page_sig(html: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let lower = html.to_ascii_lowercase();
    let mut ld = String::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<script") {
        let at = from + i;
        let open_end = html[at..].find('>').map(|e| at + e + 1).unwrap_or(html.len());
        let close = lower[open_end..].find("</script").map(|e| open_end + e).unwrap_or(html.len());
        if lower[at..open_end].contains("ld+json") {
            ld.push_str(&html[open_end..close]);
        }
        from = close.max(open_end);
        if from >= html.len() {
            break;
        }
    }
    // 链接和图片：只改了链接、换了图的改动，文字是一样的。/cdn-cgi/ 是 Cloudflare 加的（邮箱保护每次请求都不一样）
    let mut refs = String::new();
    for t in tags(html, "a").into_iter().chain(tags(html, "img")) {
        for k in ["href", "src", "alt"] {
            let val = attr(t, k);
            if !val.is_empty() && !val.contains("/cdn-cgi/") {
                refs.push_str(&val);
                refs.push('\n');
            }
        }
    }
    // 验收请求带的 _pv=时间戳 有的站会原样写进页面里的链接
    let mut text = String::new();
    for (n, part) in format!("{}\n{ld}\n{refs}", page_text(html)).split("_pv=").enumerate() {
        text.push_str(if n == 0 { part } else { part.trim_start_matches(|c: char| c.is_ascii_digit()) });
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// 一段文字够不够当对照：够长（8 个汉字，或 5 个词以上的外文）、不是网址。
fn distinctive(cut: &str) -> bool {
    let cjk = cut.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).count();
    let words = cut.split_whitespace().count();
    // 带 @ 的不要：Cloudflare 的邮箱保护会把页面上的邮箱地址换成别的字
    (cjk >= 8 || (cjk == 0 && words >= 5 && cut.len() >= 30)) && !cut.contains("://") && !cut.contains('@')
}

/// 开头、中间、结尾各取一段
fn spread(cands: &[String]) -> Vec<String> {
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

/// 从构建出来的页面里挑对照文字：这次构建的页面上有、改动前的页面上没有、这次差异里也有的句子。
/// 比的是渲染后的页面，和线上取回来的页面是同一种东西。不从源码的差异里挑：源码（Markdown、模板、
/// 数据文件、锚点、注释）渲染出来是什么样猜不全，以前每种写法都误报过一次。差异只用来确认「这句是这次改的」。
/// `pages`：（这次构建的页面，改动前的页面，是不是这次新增的页面）；`added`：这次差异里新增的行。
/// 新增的页面整页都是新的，差异里对不上（正文是模板拼出来的）时挑页面自己的句子。
pub fn snippets_from_build(pages: &[(String, String, bool)], added: &str) -> Vec<String> {
    let squash = |s: &str| plain_punct(s).chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let mut added = squash(added);
    // 特别大的差异只看前面一截：每个候选句子都要在里面找一遍
    let mut end = added.len().min(2_000_000);
    while !added.is_char_boundary(end) {
        end -= 1;
    }
    added.truncate(end);
    let (mut best, mut rest): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for (new, old, fresh) in pages {
        // 改动前就有的句子不挑：没动过的句子占大多数，先查集合；句子被拆得不一样的再在整页里找
        let old_sentences: HashSet<String> = sentences(old).iter().map(|s| squash(s)).collect();
        let old_text = squash(&page_text(old));
        let mut looked = 0;
        for cut in sentences(new) {
            let key = squash(&cut);
            if !distinctive(&cut) || old_sentences.contains(&key) || old_text.contains(&key) {
                continue;
            }
            looked += 1;
            let to = if looked <= 80 && added.contains(&key) {
                &mut best
            } else if *fresh {
                &mut rest
            } else {
                // 已有页面上「上次构建没有」的句子不一定是这次改的：别的改动后来上线了、构建时才生成的日期和价格，
                // 两次构建也会不一样。差异里没有的不挑
                continue;
            };
            if !to.contains(&cut) {
                to.push(cut);
            }
        }
    }
    spread(if best.is_empty() { &rest } else { &best })
}

/// 页面拆成一句一句：标题、描述、每个块里的文字按标点断开。太长的只取开头一截（对照时是在页面文字里找这一截）。
fn sentences(html: &str) -> Vec<String> {
    format!("{}\n{}\n{}", html_title(html), meta_content(html, "description"), html_text(html, true))
        .split(|c: char| c == '\n' || "，。；：！？,.;:!?（）()「」“”\"".contains(c))
        .map(|seg| seg.trim().chars().take(40).collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 页面里有没有这段文字。对照文字和页面都是渲染后的东西，原样找；只是不看空白和排版替换的引号、破折号。
fn page_has(text: &str, snippet: &str) -> bool {
    let squash = |s: &str| plain_punct(s).chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let needle = squash(snippet);
    !needle.is_empty() && squash(text).contains(&needle)
}

/// 像源文件路径的（/src/pages/a.astro、/content/x.md）：不是页面网址
fn source_path(w: &str) -> bool {
    let l = w.to_ascii_lowercase();
    [".astro", ".md", ".mdx", ".ts", ".tsx", ".js", ".jsx", ".vue", ".svelte", ".json", ".yml", ".yaml", ".css", ".scss", ".toml"].iter().any(|e| l.ends_with(e))
        || l.starts_with("/src/") || l.starts_with("/content/") || l.starts_with("/public/") || l.starts_with("/scripts/")
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
    fn reported_pages_accept_lists_and_relative_paths() {
        let reply = "改好了。\n\n**新页面：**\n- https://x.com/zh/a/\n- `/zh/b/`\n- /src/pages/c.astro\n\n改动的页面：\n1. [首页](https://www.x.com/)\n2. https://evil.com/\n新页面上线后记得看 /sitemap.xml\n\n谢谢";
        let (n, c) = reported_pages(reply, "x.com");
        assert_eq!(n, vec!["https://x.com/zh/a/", "https://x.com/zh/b/"]);
        assert_eq!(c, vec!["https://www.x.com/"]);
        assert!(same_page("https://x.com/a.html", "http://www.x.com/a/"));
        assert!(same_page("https://x.com/a/index.html", "https://x.com/a"));
        assert!(page_has(&visible_text("<p>It’s “fine” – really</p>"), "It's \"fine\" - really"));
        assert!(page_has("账户层：注册欧易后 马上要做的安全设置", "注册欧易后马上要做的安全设置") && !page_has("账户层-注册欧易后", "账户层：注册欧易后"));
    }

    #[test]
    fn build_snippets_are_what_the_page_shows() {
        let page = |body: &str| format!("<html><head><title>欧易安全吗？平台、账户与防骗安全评估</title><script type=\"application/ld+json\">{{\"description\":\"只写在结构化数据里的一句说明文字\"}}</script></head><body><nav>首页 教程 工具</nav>{body}</body></html>");
        let old = page("<h2 id=\"a\">你最该做的 4 件安全设置</h2><p>老的正文在这里一直没有动过，上线前就在页面上。</p>");
        let new = page("<h2 id=\"账户层-注册欧易后马上要做的安全设置\">账户层：注册欧易后马上要做的安全设置</h2><p>老的正文在这里一直没有动过，上线前就在页面上。</p><p>储备金证明是交易所公开持有资产的一种方式，可以自己核对。</p><p>今日行情更新于十一月六日上午九点整</p>");
        // bsvar：数据文件里的锚点（连字符）不显示在页面上；渲染后的小标题和正文才是
        let added = "+      after: '账户层-注册欧易后马上要做的安全设置',\n+<p>储备金证明是交易所公开持有资产的一种方式，可以自己核对。</p>";
        let s = snippets_from_build(&[(new.clone(), old.clone(), false)], added);
        assert_eq!(s, vec!["注册欧易后马上要做的安全设置", "储备金证明是交易所公开持有资产的一种方式"]);
        // 挑出来的每一段都能在同一个页面里找到；没动过的正文、结构化数据、属性值不挑
        // 已有页面：差异里没有的句子不挑（「今日行情…」是构建时生成的，别的改动后来上线也会让两次构建不一样）
        assert!(snippets_from_build(&[(new.clone(), old.clone(), false)], "").is_empty());
        assert!(!snippets_from_build(&[(new.clone(), old.clone(), false)], "+<p>储备金证明是交易所公开持有资产的一种方式，联系 a@b.com</p>").iter().any(|x| x.contains("今日行情")));
        // 新增的页面：差异里对不上时挑页面自己的句子
        let all = snippets_from_build(&[(new.clone(), old.clone(), true)], "");
        assert!(all.iter().all(|x| page_has(&page_text(&new), x)), "{all:?}");
        assert!(all.contains(&"注册欧易后马上要做的安全设置".to_string()) && !all.iter().any(|x| x.contains("老的正文") || x.contains("结构化数据") || x.contains('-')), "{all:?}");
        // 页面上没有新文字（只改了结构化数据）：不挑，也就不查
        assert!(snippets_from_build(&[(old.clone(), old.clone(), false)], added).is_empty());
    }

    #[test]
    fn page_sig_ignores_per_request_noise() {
        let page = |script: &str, body: &str, ld: &str| {
            // 链接里的防缓存参数、Cloudflare 邮箱保护的地址也跟着每次请求变
            let n: String = script.chars().filter(char::is_ascii_digit).collect();
            format!("<html><head><title>T</title><script type=\"application/ld+json\">{ld}</script><script>{script}</script></head><body><a href=\"/zh/?_pv=17939{n}\">简体</a><a href=\"/cdn-cgi/l/email-protection#{n}\">邮箱</a><p>{body}</p></body></html>")
        };
        let a = page_sig(&page("var t=1;", "正文", "{\"url\":\"/a?_pv=1793932686\"}"));
        // 防缓存参数、脚本每次请求不一样：指纹不变
        assert_eq!(a, page_sig(&page("var t=2;", "正文", "{\"url\":\"/a?_pv=1793932695\"}")));
        // 正文或结构化数据变了：指纹变
        assert_ne!(a, page_sig(&page("var t=1;", "新正文", "{\"url\":\"/a?_pv=1793932686\"}")));
        assert_ne!(a, page_sig(&page("var t=1;", "正文", "{\"url\":\"/b?_pv=1793932686\"}")));
        // 只改了链接或换了图、文字没变：指纹也变
        let link = |href: &str, img: &str| page_sig(&format!("<p>正文 <a href=\"{href}\">看这里</a><img src=\"{img}\" alt=\"图\"></p>"));
        assert_ne!(link("/go/okx", "/a.webp"), link("/go/binance", "/a.webp"));
        assert_ne!(link("/go/okx", "/a.webp"), link("/go/okx", "/b.webp"));
    }

    #[test]
    fn reported_pages_keep_own_domain_only() {
        let reply = "改好了。\n新页面：https://iosur.com/zh/posts/x/ https://iosur.com/en/posts/x/\n改动的页面：https://iosur.com/zh/posts/ai-video-prompts/、https://evil.com/a/";
        let (n, c) = reported_pages(reply, "iosur.com");
        assert_eq!(n, vec!["https://iosur.com/zh/posts/x/", "https://iosur.com/en/posts/x/"]);
        assert_eq!(c, vec!["https://iosur.com/zh/posts/ai-video-prompts/"]);
    }
}

#[cfg(test)]
mod live {
    /// 真实页面：从构建出来的页面里挑的每一段，线上都要找得到。
    /// NEW=构建出来的页面 OLD=改动前的页面（可不给） ADDED=差异文件（可不给） URL=线上地址
    /// cargo test --lib site_verify::live::build_snippets_real -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn build_snippets_real() {
        let read = |k: &str| std::env::var(k).ok().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        // 没给 OLD 当新增的页面
        let picked = super::snippets_from_build(&[(read("NEW"), read("OLD"), std::env::var("OLD").is_err())], &read("ADDED"));
        println!("picked: {picked:?}");
        assert!(!picked.is_empty());
        for s in &picked {
            assert_eq!(super::snippets_live(&[std::env::var("URL").unwrap()], std::slice::from_ref(s)).await, Some(true), "{s}");
        }
    }

    /// 同一个页面连取两次，指纹要一样（不一样的站「页面变没变」就没有判断力）。URLS=用逗号隔开的网址
    /// cargo test --lib site_verify::live::page_sig_real -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn page_sig_real() {
        let client = super::client();
        for u in std::env::var("URLS").unwrap().split(',') {
            let a = super::fetch(&client, u).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let b = super::fetch(&client, u).await.unwrap();
            println!("{} {u}", if super::page_sig(&a.body) == super::page_sig(&b.body) { "same" } else { "DIFF" });
        }
    }

    /// 真实站点上把「新内容已生效」整个跑一遍（上线前拍页面 → 验收）。DOMAIN、PAGES（逗号隔开的完整网址）、SNIPPET；
    /// SAME=1：上线前的指纹就是现在的（页面没变）；不给：当成页面变了。
    /// cargo test --lib site_verify::live::content_check_real -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn content_check_real() {
        let jobs = crate::site_jobs::Jobs::default();
        let job = jobs.start(None, "t", "ops:watch", "t", &[("verify", "线上验收")]).unwrap();
        let pages: Vec<String> = std::env::var("PAGES").unwrap().split(',').map(str::to_string).collect();
        let mut spec = super::Spec { domain: std::env::var("DOMAIN").unwrap(), primary: pages.iter().take(1).cloned().collect(), pages, snippets: vec![std::env::var("SNIPPET").unwrap()], only_new: true, ..Default::default() };
        let (before, sigs) = super::snapshot(&spec).await;
        println!("拍到 {} 个页面的指纹", sigs.len());
        spec.before = before;
        spec.before_sig = if std::env::var("SAME").is_ok() { sigs } else { sigs.into_iter().map(|(k, v)| (k, v ^ 1)).collect() };
        let t = std::time::Instant::now();
        let rep = super::run(&spec, &job, false).await;
        let row = rep.checks.iter().find(|c| c.label == "新内容已生效");
        println!("用时 {} 秒 missing_text={} 检查项={:?} 待核对={:?}", t.elapsed().as_secs(), rep.missing_text, row.map(|c| (c.ok, c.detail.clone())), rep.soft);
    }

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

#[cfg(test)]
mod attribution_tests {
    use super::*;
    #[test]
    fn a_page_already_missing_before_shipping_is_not_this_changes_fault() {
        let mut spec = Spec::default();
        spec.before.insert("/guide/a/".into(), vec!["返回 404".into()]);
        spec.before.insert("/guide/b/".into(), vec![]);
        assert!(broken_before(&spec, "https://x.com/guide/a/", "返回 404"));
        // 上线前能开、现在打不开；上线前没有记录；错误不一样：都算这次的
        assert!(!broken_before(&spec, "https://x.com/guide/b/", "返回 404"));
        assert!(!broken_before(&spec, "https://x.com/guide/c/", "返回 404"));
        assert!(!broken_before(&spec, "https://x.com/guide/a/", "返回 500"));
        assert!(!broken_before(&spec, "https://x.com/guide/a/", ""));
    }
    #[test]
    fn only_problems_this_change_caused_are_reported() {
        let noindex = "设置了 noindex，搜索引擎不会收录".to_string();
        let mut spec = Spec { only_new: true, ..Default::default() };
        spec.before.insert("/old/".into(), vec![noindex.clone()]);
        spec.before.insert("/clean/".into(), Vec::new());
        // 上线前就有：不报
        let mut i = vec![noindex.clone()];
        assert!(caused_by_this(&spec, "/old/", false, &mut i).is_empty());
        assert!(i.is_empty());
        // 上线前没问题、现在有：是这次造成的
        let mut i = vec![noindex.clone()];
        assert!(caused_by_this(&spec, "/clean/", false, &mut i).is_empty());
        assert_eq!(i, vec![noindex.clone()]);
        // 没有上线前的记录：分不清，不报
        let mut i = vec![noindex.clone()];
        assert_eq!(caused_by_this(&spec, "/unknown/", false, &mut i), vec![noindex.clone()]);
        assert!(i.is_empty());
        // 新页面本来就没有「上线前」，有问题都是这次的
        let mut i = vec![noindex.clone()];
        assert!(caused_by_this(&spec, "/brand-new/", true, &mut i).is_empty());
        assert_eq!(i.len(), 1);
        // 手动上线不做归因，照旧全报
        let plain = Spec::default();
        let mut i = vec![noindex.clone()];
        assert!(caused_by_this(&plain, "/unknown/", false, &mut i).is_empty());
        assert_eq!(i.len(), 1);
    }
}
