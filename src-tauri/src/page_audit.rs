//! 页面体检：每周把站点的构建产物过一遍，查两类搜索引擎和 AI 搜索都在意、但页面照样能打开所以没人发现的问题：
//!
//! - 图片：缺替代文字（alt）、引用的图片文件不存在、图片文件太大（拖慢页面）；
//! - 结构化数据（JSON-LD）：写坏了解析不了、文章页没有 Article 类型、Article 缺标题 / 日期 / 作者 / 图片这些字段。
//!
//! 只读本地文件，不联网、不用 AI。问题多的变成机会交给 AI 修（要你确认后才上线）。
//! 另外：站点自己没有 llms.txt 的，上线时 Pilot 按页面标题和描述生成一份放进构建产物（见 llms_txt），方便 AI 搜索读懂站点。
//! 结果存在站点数据目录的 cache/audit.json。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::site_make::{attr, inner, strip_tags, tags};

const WEEK: u64 = 7 * 86_400;
/// 超过这个大小的图片算太大
pub const LARGE_IMAGE: u64 = 300 * 1024;
/// 每类问题最多记这么多条
const CAP: usize = 200;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ImgIssue {
    pub page: String,
    pub src: String,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SchemaIssue {
    pub page: String,
    pub problem: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AuditReport {
    pub checked_at: u64,
    pub pages: usize,
    pub images: usize,
    pub no_alt: Vec<ImgIssue>,
    pub broken: Vec<ImgIssue>,
    pub large: Vec<ImgIssue>,
    /// 有结构化数据的页面数、文章页数
    pub schema_pages: usize,
    pub article_pages: usize,
    pub schema: Vec<SchemaIssue>,
    /// 站点自己有 llms.txt（own）、还是上线时由 Pilot 生成（pilot）
    pub llms: String,
}

fn path(site_dir: &Path) -> PathBuf {
    crate::site_data::cache_dir(site_dir).join("audit.json")
}

pub fn load(site_dir: &Path) -> AuditReport {
    std::fs::read(path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(site_dir: &Path, r: &AuditReport) {
    let p = path(site_dir);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(raw) = serde_json::to_vec_pretty(r) {
        let _ = std::fs::write(p, raw);
    }
}

/// 标签上有没有这个属性（`alt=""`、`alt` 都算有）
fn has_attr(tag: &str, name: &str) -> bool {
    let low = tag.to_ascii_lowercase();
    let b = low.as_bytes();
    let mut from = 0;
    while let Some(i) = low[from..].find(name) {
        let a = from + i;
        let before = if a == 0 { b' ' } else { b[a - 1] };
        let after = b.get(a + name.len()).copied().unwrap_or(b'>');
        if before.is_ascii_whitespace() && (after == b'=' || after.is_ascii_whitespace() || after == b'>' || after == b'/') {
            return true;
        }
        from = a + name.len();
    }
    false
}

/// 图片引用 → 构建产物里的文件；站外图片、data: 图片返回 None
fn local_image(out: &Path, page_file: &Path, src: &str, hosts: &[String]) -> Option<PathBuf> {
    let s = src.trim();
    if s.is_empty() || s.starts_with("data:") || s.starts_with("blob:") {
        return None;
    }
    let s = if let Some(rest) = s.split_once("://").map(|(_, r)| r).or_else(|| s.strip_prefix("//")) {
        let (host, p) = rest.find('/').map(|i| (&rest[..i], &rest[i..])).unwrap_or((rest, "/"));
        if !hosts.iter().any(|h| crate::revenue::norm_host(host) == *h) {
            return None;
        }
        p.to_string()
    } else {
        s.to_string()
    };
    let s = s.split(['?', '#']).next().unwrap_or("");
    let s = percent_decode(s);
    let base = if s.starts_with('/') { out.to_path_buf() } else { page_file.parent().unwrap_or(out).to_path_buf() };
    let mut p = base;
    for seg in s.trim_start_matches('/').split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if p != out {
                    p.pop();
                }
            }
            x => p.push(x),
        }
    }
    Some(p)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 页面里所有 JSON-LD 块：(能不能解析, 解析出的节点)
fn json_ld(html: &str, low: &str) -> Vec<Result<Vec<Value>, String>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = low[from..].find("<script") {
        let a = from + i;
        let Some(j) = low[a..].find('>') else { break };
        let start = a + j + 1;
        let end = low[start..].find("</script>").map(|k| start + k).unwrap_or(low.len());
        from = end;
        if !low[a..start].contains("ld+json") {
            continue;
        }
        out.push(match serde_json::from_str::<Value>(html[start..end].trim()) {
            Ok(v) => {
                let mut nodes: Vec<Value> = match v {
                    Value::Array(a) => a,
                    other => vec![other],
                };
                let graph: Vec<Value> = nodes.iter().filter_map(|n| n.get("@graph").and_then(|g| g.as_array()).cloned()).flatten().collect();
                nodes.extend(graph);
                Ok(nodes)
            }
            Err(e) => Err(format!("第 {} 行附近写错了", e.line())),
        });
    }
    out
}

fn types_of(n: &Value) -> Vec<String> {
    match &n["@type"] {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

const ARTICLE_TYPES: &[&str] = &["Article", "BlogPosting", "NewsArticle", "TechArticle", "Report", "HowTo", "Review"];

/// 看起来是文章的页面：有发布 / 更新日期，或者 og:type 是 article
fn looks_like_article(html: &str, low: &str) -> bool {
    tags(html, low, "meta").iter().any(|m| {
        let p = attr(m, "property").or_else(|| attr(m, "name")).unwrap_or_default().to_ascii_lowercase();
        (p == "og:type" && attr(m, "content").is_some_and(|c| c.eq_ignore_ascii_case("article"))) || p == "article:published_time"
    })
}

/// 一个页面的结构化数据问题
fn schema_problems(html: &str, low: &str) -> (bool, bool, Vec<String>) {
    let blocks = json_ld(html, low);
    let mut problems = Vec::new();
    let mut nodes = Vec::new();
    for b in blocks.iter() {
        match b {
            Ok(n) => nodes.extend(n.iter().cloned()),
            Err(e) => problems.push(format!("结构化数据（JSON-LD）解析不了：{e}")),
        }
    }
    let article = looks_like_article(html, low) || nodes.iter().any(|n| types_of(n).iter().any(|t| ARTICLE_TYPES.contains(&t.as_str())));
    let arts: Vec<&Value> = nodes.iter().filter(|n| types_of(n).iter().any(|t| ARTICLE_TYPES.contains(&t.as_str()))).collect();
    if article && arts.is_empty() && blocks.iter().all(|b| b.is_ok()) {
        problems.push("文章页没有 Article / BlogPosting 类型的结构化数据".into());
    }
    for a in arts {
        let t = types_of(a).first().cloned().unwrap_or_default();
        if matches!(t.as_str(), "HowTo" | "Review" | "Report") {
            continue;
        }
        let missing: Vec<&str> = [("headline", "标题 headline"), ("datePublished", "发布日期 datePublished"), ("author", "作者 author"), ("image", "图片 image")]
            .into_iter()
            .filter(|(k, _)| a.get(*k).is_none_or(|v| v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty())))
            .map(|(_, l)| l)
            .collect();
        if !missing.is_empty() {
            problems.push(format!("{t} 缺 {}", missing.join("、")));
        }
    }
    (!blocks.is_empty(), article, problems)
}

pub fn audit(out: &Path, domain: &str) -> AuditReport {
    let host = crate::revenue::norm_host(domain);
    let hosts = vec![host.clone()];
    let mut r = AuditReport { llms: if out.join("llms.txt").is_file() { "own".into() } else { "pilot".into() }, ..Default::default() };
    let mut checked: std::collections::HashMap<PathBuf, Option<u64>> = std::collections::HashMap::new();
    for (file, html) in crate::site_make::real_pages(out) {
        let page = crate::site_make::page_url(out, &file);
        let low = html.to_ascii_lowercase();
        r.pages += 1;
        for t in tags(&html, &low, "img") {
            r.images += 1;
            let src = attr(t, "src").or_else(|| attr(t, "data-src")).unwrap_or_default();
            if !has_attr(t, "alt") && r.no_alt.len() < CAP {
                r.no_alt.push(ImgIssue { page: page.clone(), src: src.clone(), bytes: 0 });
            }
            let Some(p) = local_image(out, &file, &src, &hosts) else { continue };
            let size = *checked.entry(p.clone()).or_insert_with(|| std::fs::metadata(&p).ok().filter(|m| m.is_file()).map(|m| m.len()));
            match size {
                None if r.broken.len() < CAP => r.broken.push(ImgIssue { page: page.clone(), src, bytes: 0 }),
                Some(b) if b > LARGE_IMAGE && r.large.len() < CAP && !r.large.iter().any(|l| l.src == src) => r.large.push(ImgIssue { page: page.clone(), src, bytes: b }),
                _ => {}
            }
        }
        let (has, article, problems) = schema_problems(&html, &low);
        r.schema_pages += has as usize;
        r.article_pages += article as usize;
        for p in problems {
            if r.schema.len() < CAP {
                r.schema.push(SchemaIssue { page: page.clone(), problem: p });
            }
        }
    }
    r.large.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    r.checked_at = crate::now_secs();
    r
}

pub async fn check_site(site: &crate::sites::Site, site_dir: &Path, out: PathBuf) -> Result<AuditReport, String> {
    if !out.join("index.html").is_file() {
        return Err("还没有构建产物：上线一次以后再体检".into());
    }
    let domain = site.domain.clone();
    let r = tokio::task::spawn_blocking(move || audit(&out, &domain)).await.map_err(|e| e.to_string())?;
    save(site_dir, &r);
    Ok(r)
}

/// 机会：图片问题（缺 alt ≥ 3、坏图、大图 ≥ 2 任一）一条，结构化数据问题一条；按站点合并，交给 AI 一次改完
pub fn opportunities(site_dir: &Path) -> Vec<crate::site_ops::Opportunity> {
    let r = load(site_dir);
    let mut out = Vec::new();
    if r.checked_at == 0 {
        return out;
    }
    if r.no_alt.len() >= 3 || !r.broken.is_empty() || r.large.len() >= 2 {
        let mut lines = Vec::new();
        for i in r.broken.iter().take(10) {
            lines.push(format!("图片不存在：{} 里的 {}", i.page, i.src));
        }
        for i in r.large.iter().take(10) {
            lines.push(format!("图片太大（{} KB）：{}（用在 {}）", i.bytes / 1024, i.src, i.page));
        }
        for i in r.no_alt.iter().take(20) {
            lines.push(format!("缺替代文字：{} 里的 {}", i.page, i.src));
        }
        let sample = r.broken.first().or(r.large.first()).or(r.no_alt.first()).map(|i| i.page.clone()).unwrap_or_default();
        out.push(crate::site_ops::Opportunity {
            key: "img_audit:site".into(),
            kind: "img_audit".into(),
            title: format!(
                "图片问题：{}",
                [(r.broken.len(), "张不存在"), (r.large.len(), "张太大"), (r.no_alt.len(), "张缺替代文字")].iter().filter(|(n, _)| *n > 0).map(|(n, l)| format!("{n}{}{l}", if *n >= CAP { "+" } else { "" })).collect::<Vec<_>>().join("、")
            ),
            page: sample,
            evidence: format!("体检时在构建产物里发现（最多列前几十条）：\n{}\n缺替代文字会让搜索引擎和读屏软件不知道图片是什么；图片太大拖慢页面；引用不存在的图片会显示成破图。", lines.join("\n")),
            score: (r.broken.len() * 20 + r.large.len() * 8 + r.no_alt.len().min(50)) as f64,
            change: "technical".into(),
            ..Default::default()
        });
    }
    if !r.schema.is_empty() {
        let lines: Vec<String> = r.schema.iter().take(20).map(|s| format!("{}：{}", s.page, s.problem)).collect();
        out.push(crate::site_ops::Opportunity {
            key: "schema:site".into(),
            kind: "schema".into(),
            title: format!("{} 个页面的结构化数据有问题", r.schema.iter().map(|s| &s.page).collect::<std::collections::HashSet<_>>().len()),
            page: r.schema[0].page.clone(),
            evidence: format!("体检时发现（最多列前 20 条）：\n{}\n结构化数据写对了，Google 的搜索结果和 AI 搜索更容易认出文章的标题、日期和作者。", lines.join("\n")),
            score: (r.schema.len() * 5).min(80) as f64,
            change: "technical".into(),
            ..Default::default()
        });
    }
    out
}

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 调度循环每轮调一次：有构建产物的站点每周体检一次。
pub fn schedule(app: &tauri::AppHandle) {
    use tauri::Manager;
    let st = app.state::<crate::AppState>();
    let now = crate::now_secs();
    let due: Vec<crate::sites::Site> = st.sites.list().into_iter().filter(|s| now.saturating_sub(load(&st.sites.data_dir(&s.id)).checked_at) >= WEEK).collect();
    if due.is_empty() || RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<crate::AppState>();
        for site in due {
            let dir = st.sites.data_dir(&site.id);
            let ok = match crate::syndicate::built_out(&st, &site) {
                Ok(out) => check_site(&site, &dir, out).await.is_ok(),
                Err(_) => false,
            };
            // 没有构建产物的站点也记一下时间，一周后再看（不每 10 分钟重找一遍）
            if !ok {
                save(&dir, &AuditReport { checked_at: crate::now_secs(), ..load(&dir) });
            }
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        use tauri::Emitter;
        let _ = app.emit("audit-checked", ());
    });
}

#[tauri::command]
pub fn site_audit(state: tauri::State<'_, crate::AppState>, id: String) -> AuditReport {
    load(&state.sites.data_dir(&id))
}

#[tauri::command]
pub async fn check_site_audit(state: tauri::State<'_, crate::AppState>, id: String) -> Result<AuditReport, String> {
    let site = state.sites.get(&id)?;
    let out = crate::syndicate::built_out(&state, &site)?;
    check_site(&site, &state.sites.data_dir(&id), out).await
}

// ───────────────────────── llms.txt ─────────────────────────

fn meta_description(html: &str, low: &str) -> String {
    tags(html, low, "meta")
        .iter()
        .find(|m| attr(m, "name").is_some_and(|n| n.eq_ignore_ascii_case("description")))
        .and_then(|m| attr(m, "content"))
        .map(|c| c.replace("&amp;", "&").replace("&quot;", "\"").split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

fn clean(s: &str) -> String {
    strip_tags(s).replace(['[', ']'], "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 按构建产物生成 llms.txt（https://llmstxt.org 的格式）：站点名、首页描述、每个页面一行（标题、网址、描述）。
/// 列表页（标签、分类、翻页）不列；最多 500 个页面。
pub fn llms_txt(name: &str, domain: &str, out: &Path) -> String {
    let base = format!("https://{}", domain.trim().trim_end_matches('/'));
    let mut home_desc = String::new();
    let mut lines = Vec::new();
    let skip = ["/tag/", "/tags/", "/category/", "/categories/", "/page/", "/search", "/404"];
    for (file, html) in crate::site_make::real_pages(out) {
        let page = crate::site_make::page_url(out, &file);
        let low = html.to_ascii_lowercase();
        let desc = meta_description(&html, &low);
        if page == "/" {
            home_desc = desc;
            continue;
        }
        if skip.iter().any(|s| page.contains(s)) || lines.len() >= 500 {
            continue;
        }
        let title = inner(&html, &low, "title").map(clean).unwrap_or_default();
        let title = if title.is_empty() { page.clone() } else { title };
        lines.push(if desc.is_empty() { format!("- [{title}]({base}{page})") } else { format!("- [{title}]({base}{page}): {desc}") });
    }
    let mut s = format!("# {}\n\n", if name.trim().is_empty() { domain } else { name.trim() });
    if !home_desc.is_empty() {
        s.push_str(&format!("> {home_desc}\n\n"));
    }
    s.push_str("## Pages\n\n");
    s.push_str(&lines.join("\n"));
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_attribute_presence() {
        assert!(has_attr(r#"<img src="a.png" alt="">"#, "alt"));
        assert!(has_attr(r#"<img alt src="a.png">"#, "alt"));
        assert!(!has_attr(r#"<img src="a.png" data-alt="x">"#, "alt"));
        assert!(!has_attr(r#"<img src="salt.png">"#, "alt"));
    }

    #[test]
    fn audits_images_schema_and_writes_llms_txt() {
        let t = tempfile::tempdir().unwrap();
        let out = t.path();
        std::fs::write(out.join("index.html"), r#"<html><head><title>首页</title><meta name="description" content="站点简介"><script type="application/ld+json">{"@type":"WebSite"}</script></head><body><img src="/logo.png" alt="logo"></body></html>"#).unwrap();
        std::fs::write(out.join("logo.png"), vec![0u8; 10]).unwrap();
        std::fs::create_dir_all(out.join("guide")).unwrap();
        std::fs::write(out.join("big.jpg"), vec![0u8; (LARGE_IMAGE + 1) as usize]).unwrap();
        std::fs::write(
            out.join("guide/index.html"),
            r#"<html><head><title>指南 [1]</title><meta property="og:type" content="article"><script type="application/ld+json">{"@type":"Article","headline":"指南"}</script></head><body><img src="../big.jpg"><img src="missing.png" alt=""><img src="https://cdn.other.com/x.png"></body></html>"#,
        )
        .unwrap();
        std::fs::create_dir_all(out.join("bad")).unwrap();
        std::fs::write(out.join("bad/index.html"), r#"<html><head><title>坏</title><meta property="article:published_time" content="2026-01-01"><script type="application/ld+json">{"@type": </script></head></html>"#).unwrap();
        let r = audit(out, "www.ex.com");
        assert_eq!((r.pages, r.images), (3, 4));
        assert_eq!(r.no_alt.iter().map(|i| i.src.as_str()).collect::<Vec<_>>(), vec!["../big.jpg", "https://cdn.other.com/x.png"]);
        assert_eq!(r.broken.iter().map(|i| i.src.as_str()).collect::<Vec<_>>(), vec!["missing.png"]);
        assert_eq!(r.large.len(), 1);
        assert_eq!(r.article_pages, 2);
        let probs: Vec<&str> = r.schema.iter().map(|s| s.problem.as_str()).collect();
        assert!(probs.iter().any(|p| p.starts_with("结构化数据（JSON-LD）解析不了")));
        assert!(probs.iter().any(|p| p.starts_with("Article 缺 发布日期 datePublished、作者 author、图片 image")));
        assert_eq!(r.llms, "pilot");
        let l = llms_txt("示例站", "www.ex.com", out);
        assert!(l.starts_with("# 示例站\n\n> 站点简介\n\n## Pages\n"));
        assert!(l.contains("- [指南 1](https://www.ex.com/guide/)"));
    }
}
