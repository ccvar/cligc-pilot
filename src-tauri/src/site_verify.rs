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
}

#[derive(Clone, Default, Debug)]
pub struct Report {
    pub checks: Vec<Check>,
    pub hard: Vec<String>,
    pub soft: Vec<String>,
    /// 严重失败看起来是部署没到位（构建产物里有这个页面，线上却打不开）：值得重新上传一次
    pub deploy_issue: bool,
    /// 打开正常、没有页面级问题的网址（可以提交 IndexNow）
    pub passed: Vec<String>,
}

const MAX_PAGES: usize = 12;
const MAX_LINKS: usize = 15;

struct Fetched {
    status: u16,
    body: String,
    /// 跟完跳转之后的地址（去掉防缓存参数）
    final_url: String,
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Fetched, String> {
    // 带一个随机参数绕开缓存，看到的是刚上线的版本
    let sep = if url.contains('?') { '&' } else { '?' };
    let busted = format!("{url}{sep}_pv={}", crate::now_secs());
    let r = client.get(&busted).send().await.map_err(|e| if e.is_timeout() { "超时".to_string() } else { "打不开".to_string() })?;
    let status = r.status().as_u16();
    let final_url = r.url().as_str().split(['?', '#']).next().unwrap_or("").to_string();
    let body = if r.status().is_success() { r.text().await.unwrap_or_default() } else { String::new() };
    Ok(Fetched { status, body: body.chars().take(2_000_000).collect(), final_url })
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

fn same_page(a: &str, b: &str) -> bool {
    let norm = |u: &str| {
        let u = u.split(['?', '#']).next().unwrap_or(u).trim_end_matches('/').to_lowercase();
        u.replace("://www.", "://")
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
    for u in spec.pages.iter().chain(&spec.new_pages) {
        if !urls.iter().any(|x| same_page(x, u)) && urls.len() < MAX_PAGES + 1 {
            urls.push(u.clone());
        }
    }
    let client = client();
    let waits: &[u64] = if patient { &[0, 30, 60, 120, 120] } else { &[0, 30] };
    let mut got: Vec<Option<Fetched>> = urls.iter().map(|_| None).collect();
    let mut last: Vec<String> = urls.iter().map(|_| String::new()).collect();
    let mut snippets_ok = spec.snippets.is_empty();
    for (n, w) in waits.iter().enumerate() {
        if *w > 0 {
            let pending = got.iter().filter(|g| g.is_none()).count();
            job.line(&format!("线上验收：{}{} 秒后再查一次（第 {}/{} 次）", if pending > 0 { format!("还有 {pending} 个页面没打开正常，") } else { "新内容还没在线上出现，".into() }, w, n + 1, waits.len()));
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
                Ok(f) if (200..300).contains(&f.status) => got[i] = Some(f),
                Ok(f) => last[i] = format!("返回 {}", f.status),
                Err(e) => last[i] = e,
            }
        }
        if !snippets_ok {
            let text: String = got.iter().skip(1).flatten().map(|f| visible_text(&f.body)).collect::<Vec<_>>().join("\n");
            snippets_ok = spec.snippets.iter().any(|s| text.contains(s.as_str()));
        }
        if got.iter().all(|g| g.is_some()) && snippets_ok {
            break;
        }
    }
    // 页面能打开
    for (i, u) in urls.iter().enumerate() {
        let label = if i == 0 { "首页能打开".to_string() } else { format!("{} 能打开", path_of(u)) };
        match &got[i] {
            Some(_) => rep.checks.push(Check { label, ok: true, detail: String::new(), hard: true }),
            None => {
                let msg = format!("{} {}", path_of(u), last[i]);
                if last[i].contains("404") && spec.built.iter().any(|b| same_page(b, u)) {
                    rep.deploy_issue = true;
                }
                rep.checks.push(Check { label, ok: false, detail: last[i].clone(), hard: true });
                rep.hard.push(msg);
            }
        }
    }
    // 新内容已生效
    if !spec.snippets.is_empty() {
        let s = spec.snippets.first().cloned().unwrap_or_default();
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
    for (i, u) in urls.iter().enumerate().skip(1) {
        let Some(f) = &got[i] else { continue };
        let p = path_of(u);
        let mut issues = Vec::new();
        if html_title(&f.body).is_empty() {
            issues.push("没有标题".to_string());
        }
        if meta_content(&f.body, "description").is_empty() {
            issues.push("没有描述（meta description）".to_string());
        }
        let main = spec.new_pages.iter().chain(&spec.primary).any(|x| same_page(x, u));
        if main && meta_content(&f.body, "robots").to_lowercase().contains("noindex") {
            issues.push("设置了 noindex，搜索引擎不会收录".to_string());
        }
        let canon = canonical(&f.body);
        // 和跟完跳转后的地址比（旧地址 301 到新地址时，canonical 指向新地址是对的）
        let landed = if f.final_url.is_empty() { u.clone() } else { f.final_url.clone() };
        if main && !canon.is_empty() && !same_page(&canon, &landed) {
            issues.push(format!("canonical 指向了 {canon}"));
        }
        if let Some(m) = crate::managed::leak_marker(&visible_text(&f.body)) {
            issues.push(format!("页面里有内部痕迹「{m}」"));
        }
        rep.checks.push(Check { label: if main { format!("{p} 标题、描述、canonical") } else { format!("{p} 标题和描述") }, ok: issues.is_empty(), detail: issues.join("；"), hard: false });
        if !issues.is_empty() {
            page_bad.insert(i);
            rep.soft.push(format!("{p}：{}", issues.join("；")));
        }
    }
    // 新页面：在 sitemap 里、站内链接能打开
    let new_idx: Vec<usize> = urls.iter().enumerate().filter(|(_, u)| spec.new_pages.iter().any(|n| same_page(n, u))).map(|(i, _)| i).collect();
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
    rep.passed = urls.iter().enumerate().skip(1).filter(|(i, _)| got[*i].is_some() && !page_bad.contains(i)).map(|(_, u)| u.clone()).collect();
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

fn meta_content(html: &str, name: &str) -> String {
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
    let mut queue: Vec<String> = ["sitemap.xml", "sitemap-index.xml", "sitemap_index.xml"].iter().map(|p| format!("https://{domain}/{p}")).collect();
    let mut seen = HashSet::new();
    let mut locs = Vec::new();
    let mut fetched = 0;
    while let Some(u) = queue.pop() {
        if !seen.insert(u.clone()) || fetched >= 12 {
            continue;
        }
        fetched += 1;
        let Ok(f) = fetch(client, &u).await else { continue };
        if !(200..300).contains(&f.status) {
            continue;
        }
        let lower = f.body.to_ascii_lowercase();
        let mut from = 0;
        while let Some(i) = lower[from..].find("<loc>") {
            let s = from + i + 5;
            let e = lower[s..].find("</loc>").map(|x| s + x).unwrap_or(s);
            let loc = decode(f.body[s..e].trim());
            if loc.ends_with(".xml") {
                queue.push(loc);
            } else {
                locs.push(loc);
            }
            from = e;
        }
    }
    locs
}

fn decode(s: &str) -> String {
    s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ")
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
    for line in diff.lines() {
        let Some(added) = line.strip_prefix('+') else { continue };
        if added.starts_with("++") {
            continue;
        }
        // 字符串里的文字（数据文件），或去掉标签后的整行（页面文件）
        let texts: Vec<String> = if added.contains('\'') || added.contains('"') {
            quoted(added)
        } else {
            vec![visible_text(added)]
        };
        for t in texts {
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
    let bare = domain.trim_start_matches("www.");
    let pick = |line: &str| -> Vec<String> {
        line.split(|c: char| c.is_whitespace() || "，,、；;（）()<>`".contains(c))
            .map(|w| w.trim_end_matches(['。', '.']))
            .filter(|w| w.starts_with("https://"))
            .filter(|w| w.trim_start_matches("https://").split('/').next().unwrap_or("").trim_start_matches("www.") == bare)
            .map(str::to_string)
            .take(10)
            .collect()
    };
    let (mut new, mut changed) = (Vec::new(), Vec::new());
    for l in reply.lines() {
        let t = l.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches("**");
        if let Some(r) = t.strip_prefix("新页面：").or_else(|| t.strip_prefix("新页面:")) {
            new.extend(pick(r));
        } else if let Some(r) = t.strip_prefix("改动的页面：").or_else(|| t.strip_prefix("改动的页面:")) {
            changed.extend(pick(r));
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
    fn parses_head_tags() {
        let html = r#"<html><head><title>AI 视频转场提示词 &amp; 写法</title><meta name="description" content="讲清 5 种转场"><meta name=robots content="index,follow"><link rel="canonical" href="https://iosur.com/zh/posts/a/"></head><body><script>var TODO=1</script><h1>转场</h1><p>遮挡转场要让前景物体把画面整个挡住</p><a href="/zh/posts/b/">b</a><a href="https://www.iosur.com/en/">en</a><a href="https://x.com/">x</a><a href="/a.css">c</a></body></html>"#;
        assert_eq!(html_title(html), "AI 视频转场提示词 & 写法");
        assert_eq!(meta_content(html, "description"), "讲清 5 种转场");
        assert!(!meta_content(html, "robots").contains("noindex"));
        assert!(same_page(&canonical(html), "https://iosur.com/zh/posts/a"));
        let text = visible_text(html);
        assert!(text.contains("遮挡转场要让前景物体") && !text.contains("TODO"), "{text}");
        assert_eq!(internal_links(html, "iosur.com"), vec!["https://iosur.com/zh/posts/b/".to_string(), "https://www.iosur.com/en/".to_string()]);
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
