//! 站点迁移：把一个线上站点搬进 Pilot 重做，保留原来的 URL 和内容，换新的视觉和技术栈。
//!
//! 七个阶段：盘点 → 确认方案 → 抓取 → 重做 → 核对 → 切换 → 跟踪（命令在 migrate_cmds）。
//! 部署到 Cloudflare（纯静态，或者表单、评论、大文件用 Pages Functions + D1 / R2，见 site_dynamic），
//! 或者自己的服务器（Caddy + Go + SQLite，Pilot 托管，见 site_vps_host）。
//!
//! 这里只放纯函数：URL 规范化、robots / sitemap 解析、页面信息提取、分组、动态功能识别、部署去向判断。
//! 正文提取、URL 对应表和核对规则在 migrate_extract。

use reqwest::Url;
use scraper::{node::Node, ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ───────────────────────── 数据结构（存在站点数据目录的 migrate/ 下） ─────────────────────────

/// 盘点出来的一个 URL。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Page {
    /// 规范化后的路径 + 查询串（主键），如 `/blog/hello/`、`/list?page=2`
    pub path: String,
    /// page 网页 | feed RSS/Atom | file 其他文件（PDF 等） | redirect 旧站自己就在跳转 | broken 打不开 | blocked robots 不让抓
    pub kind: String,
    pub status: u16,
    /// 跳转目标：本站的路径，外站是完整网址
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub redirect_to: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub canonical: String,
    #[serde(default)]
    pub h1: String,
    #[serde(default)]
    pub lang: String,
    /// [语言, 网址]
    #[serde(default)]
    pub hreflang: Vec<[String; 2]>,
    #[serde(default)]
    pub og_type: String,
    #[serde(default)]
    pub og_image: String,
    /// JSON-LD 里的 @type
    #[serde(default)]
    pub schema: Vec<String>,
    #[serde(default)]
    pub published: String,
    #[serde(default)]
    pub modified: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub noindex: bool,
    #[serde(default)]
    pub words: u32,
    #[serde(default)]
    pub links_out: u32,
    #[serde(default)]
    pub links_in: u32,
    /// 正文哈希（找重复页）
    #[serde(default)]
    pub text_hash: String,
    /// URL 规律分组，如 `/blog/*`
    #[serde(default)]
    pub group: String,
    /// home | article | list | page
    #[serde(default)]
    pub ptype: String,
    /// 从哪儿找到的：sitemap / crawl / gsc（逗号分隔）
    #[serde(default)]
    pub found: String,
    #[serde(default)]
    pub clicks: u64,
    #[serde(default)]
    pub impressions: u64,
    /// 用浏览器渲染后抓的（JS 渲染站）
    #[serde(default)]
    pub rendered: bool,
    /// 快照文件名（snapshots/ 下），没有＝没存
    #[serde(default)]
    pub snapshot: String,
    /// 这一页上识别到的动态功能（form / search / comments / login / cart）
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub bytes: u64,
}

/// 一组 URL（按 URL 规律分）。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Group {
    pub key: String,
    pub count: usize,
    /// 这组里最多的页面类型
    pub ptype: String,
    pub clicks: u64,
    pub samples: Vec<String>,
}

/// 识别到的一种动态功能。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Feature {
    /// form | search | comments | login | cart
    pub key: String,
    pub label: String,
    pub pages: usize,
    pub samples: Vec<String>,
    /// 迁到新站上怎么办
    pub plan: String,
    /// 纯静态站能做（第一步就支持）
    pub static_ok: bool,
}

/// 部署去向。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Hosting {
    /// cf_static | cf_workers | vps
    pub target: String,
    pub label: String,
    pub reasons: Vec<String>,
    /// 需要 Cloudflare 付费套餐
    pub paid: bool,
    /// 现在就能迁（第一步只支持 cf_static）
    pub supported: bool,
}

/// 所有权线索（盘点时只做显示，切换前才强制）。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Ownership {
    /// 域名在哪个 Cloudflare 连接里（连接 id），空＝不在
    #[serde(default)]
    pub cf_conn: String,
    #[serde(default)]
    pub cf_zone: String,
    /// Search Console 里已验证的资源
    #[serde(default)]
    pub gsc_site: String,
    /// DNS TXT 验证用的令牌
    #[serde(default)]
    pub txt_token: String,
    /// 已确认是用户自己的站
    #[serde(default)]
    pub verified: bool,
    /// 怎么确认的：cloudflare | gsc | txt
    #[serde(default)]
    pub by: String,
}

/// 盘点结果（inventory.json）。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Inventory {
    pub domain: String,
    /// 实际访问用的协议和主机，如 `https://www.example.com`
    pub origin: String,
    /// always | never | mixed：旧站的路径结尾带不带 `/`
    pub trailing_slash: String,
    pub js_rendered: bool,
    pub started_at: u64,
    pub finished_at: u64,
    /// 碰到页数上限，没抓完
    pub truncated: bool,
    pub pages: Vec<Page>,
    pub groups: Vec<Group>,
    pub features: Vec<Feature>,
    pub hosting: Hosting,
    /// 第三方统计 ID（G-… / GTM-… / UA-…），迁过去继续用
    pub analytics: Vec<String>,
    /// CMS 指纹（meta generator）
    pub generator: String,
    /// 页面引用的图片和附件：网址 → 引用次数
    pub assets: BTreeMap<String, u32>,
    /// 用 Search Console 补漏了
    pub gsc_used: bool,
    /// 大陆访客占比（GA4 近 28 天），None＝不知道
    pub cn_share: Option<f64>,
    pub ownership: Ownership,
    /// 盘点过程中值得告诉用户的事
    pub notes: Vec<String>,
    /// 页面标题里统一的站名后缀（如「 | 站名」），内容标题去掉它
    #[serde(default)]
    pub title_suffix: String,
}

impl Inventory {
    pub fn html_pages(&self) -> impl Iterator<Item = &Page> {
        self.pages.iter().filter(|p| p.kind == "page")
    }
}

/// 用户确认的迁移方案（plan.json）。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Plan {
    /// 不迁的分组
    #[serde(default)]
    pub drop_groups: Vec<String>,
    /// 不迁的单个路径
    #[serde(default)]
    pub drop_paths: Vec<String>,
    /// 第一步固定 astro
    #[serde(default)]
    pub stack: String,
    /// direct 部署到 Cloudflare Pages | vps 部署到自己的服务器（Caddy + Go + SQLite，Pilot 托管）
    #[serde(default)]
    pub deploy: String,
    /// keep_brand 保留品牌（logo、名称、主色）只升级设计 | fresh 全新视觉
    #[serde(default)]
    pub visual: String,
    /// 视觉风格的补充要求
    #[serde(default)]
    pub style: String,
    /// 重做前挑的设计方向：d1 / d2 / d3（`.pilot/migrate/directions/` 里的小样）；auto＝跳过，让 AI 自己定；空＝还没挑
    #[serde(default)]
    pub direction: String,
    /// 确认方案以后自动往下做（抓取 → 出设计方向 → 重做 → 核对 → 部署到预览 → 实测），只在要你选、要你上线时停
    #[serde(default)]
    pub auto: bool,
    /// 动态功能怎么处理：form / comments → keep（用 Pages Functions + D1 实现）| drop（去掉）
    #[serde(default)]
    pub features: BTreeMap<String, String>,
    #[serde(default)]
    pub confirmed_at: u64,
}

impl Plan {
    /// 动态功能保留（默认保留）。
    pub fn keeps_feature(&self, f: &str) -> bool {
        self.features.get(f).map(|v| v != "drop").unwrap_or(true)
    }
    pub fn keeps(&self, p: &Page) -> bool {
        !self.drop_groups.iter().any(|g| g == &p.group) && !self.drop_paths.iter().any(|x| x == &p.path)
    }
}

// ───────────────────────── URL ─────────────────────────

/// 规范化域名输入：去协议、路径、端口，小写。
pub fn clean_domain(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase();
    let s = s.split("://").last().unwrap_or("");
    let s = s.split(['/', '?', '#']).next().unwrap_or("");
    let s = s.split(':').next().unwrap_or("");
    s.trim_matches('.').to_string()
}

/// 主机名是不是这个站（apex 和 www 视为同一个站）。
pub fn same_site(host: &str, domain: &str) -> bool {
    let h = host.trim_start_matches("www.");
    let d = domain.trim_start_matches("www.");
    h.eq_ignore_ascii_case(d)
}

const TRACKING: &[&str] = &["fbclid", "gclid", "msclkid", "yclid", "mc_cid", "mc_eid", "_ga", "spm", "share", "amp"];

/// 跟踪参数：迁过去不需要保留。
fn is_tracking(k: &str) -> bool {
    k.starts_with("utm_") || TRACKING.contains(&k)
}

/// 把站内链接规范化成「路径 + 查询串」：外站、非 http(s)、mailto 之类返回 None。
/// 去掉 #、跟踪参数，查询参数按原顺序保留。
pub fn normalize(href: &str, base: &Url, domain: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") || href.starts_with("mailto:") || href.starts_with("tel:") || href.starts_with("data:") {
        return None;
    }
    let u = base.join(href).ok()?;
    if !matches!(u.scheme(), "http" | "https") || !same_site(u.host_str()?, domain) {
        return None;
    }
    Some(path_of(&u))
}

/// Url → 规范化的「路径 + 查询串」。
pub fn path_of(u: &Url) -> String {
    let mut out = u.path().to_string();
    if out.is_empty() {
        out.push('/');
    }
    let q: Vec<(String, String)> = u.query_pairs().filter(|(k, _)| !is_tracking(k)).map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    if !q.is_empty() {
        let mut s = Url::parse("http://x/").unwrap();
        s.query_pairs_mut().extend_pairs(q.iter());
        out.push('?');
        out.push_str(s.query().unwrap_or(""));
    }
    out
}

const ASSET_EXT: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "avif", "svg", "ico", "bmp", "tif", "tiff", "css", "js", "mjs", "map", "woff", "woff2", "ttf", "otf", "eot", "mp4", "webm", "mov", "mp3", "wav", "ogg", "m4a",
];
const FILE_EXT: &[&str] = &["pdf", "zip", "rar", "7z", "gz", "tar", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "csv", "txt", "epub", "apk", "dmg", "exe", "msi", "json", "xml"];

/// 路径的扩展名（小写，不带点）。
pub fn ext_of(path: &str) -> String {
    let p = path.split(['?', '#']).next().unwrap_or("");
    let last = p.rsplit('/').next().unwrap_or("");
    match last.rsplit_once('.') {
        Some((stem, e)) if !stem.is_empty() && e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()) => e.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// 静态资源（图片、样式、脚本、字体、音视频）：不当页面抓。
pub fn is_asset(path: &str) -> bool {
    ASSET_EXT.contains(&ext_of(path).as_str())
}

/// 下载类文件（PDF、压缩包、文档……）：也是要保留的 URL，但不当页面解析。
pub fn is_file(path: &str) -> bool {
    FILE_EXT.contains(&ext_of(path).as_str())
}

/// 明显不用迁的 URL：后台、登录跳转、评论回复参数、搜索结果页、WordPress 的 JSON 接口之类。
pub fn is_junk(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    const BAD: &[&str] = &["/wp-admin", "/wp-login", "/wp-json", "/xmlrpc.php", "/cdn-cgi/", "/wp-includes/", "/feed/atom", "/trackback", "/embed/"];
    const BAD_SEG: &[&str] = &["admin", "login", "logout", "signin", "signout", "cart", "checkout"];
    const BAD_Q: &[&str] = &["replytocom=", "share=", "action=", "preview=", "add-to-cart=", "s=", "q=", "search=", "orderby=", "filter_"];
    if BAD.iter().any(|b| p.contains(b)) {
        return true;
    }
    let path_only = p.split('?').next().unwrap_or("");
    if path_only.split('/').any(|seg| BAD_SEG.contains(&seg)) {
        return true;
    }
    if let Some((_, q)) = p.split_once('?') {
        return q.split('&').any(|kv| BAD_Q.iter().any(|b| kv.starts_with(b)));
    }
    false
}

// ───────────────────────── robots.txt ─────────────────────────

#[derive(Default, Debug)]
pub struct Robots {
    /// (允许, 规则)
    rules: Vec<(bool, String)>,
    pub sitemaps: Vec<String>,
    pub crawl_delay: Option<f64>,
}

/// 只取 `User-agent: *` 那组规则（Pilot 没有自己的爬虫名）。
pub fn parse_robots(txt: &str) -> Robots {
    let mut r = Robots::default();
    let mut agents: Vec<String> = Vec::new();
    let mut last_was_rule = false;
    for line in txt.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
        match k.as_str() {
            "user-agent" => {
                if last_was_rule {
                    agents.clear();
                }
                agents.push(v.to_ascii_lowercase());
                last_was_rule = false;
            }
            "allow" | "disallow" => {
                last_was_rule = true;
                if agents.iter().any(|a| a == "*") && !v.is_empty() {
                    r.rules.push((k == "allow", v));
                }
            }
            "crawl-delay" => {
                if agents.iter().any(|a| a == "*") {
                    r.crawl_delay = v.parse().ok();
                }
            }
            "sitemap" => r.sitemaps.push(line.split_once(':').map(|x| x.1.trim().to_string()).unwrap_or_default()),
            _ => {}
        }
    }
    r
}

/// robots 规则匹配：支持 `*` 通配和结尾 `$`。
fn robots_match(rule: &str, path: &str) -> bool {
    let (rule, anchored) = match rule.strip_suffix('$') {
        Some(r) => (r, true),
        None => (rule, false),
    };
    let parts: Vec<&str> = rule.split('*').collect();
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !path.starts_with(part) {
                return false;
            }
            pos = part.len();
            continue;
        }
        match path[pos..].find(part) {
            Some(at) => pos += at + part.len(),
            None => return false,
        }
    }
    !anchored || pos == path.len() || (parts.len() > 1 && parts.last() == Some(&""))
}

impl Robots {
    /// 最长匹配的规则说了算；一样长时 Allow 优先。
    pub fn allowed(&self, path: &str) -> bool {
        let mut best: Option<(usize, bool)> = None;
        for (allow, rule) in &self.rules {
            if robots_match(rule, path) {
                let len = rule.len();
                if best.is_none_or(|(l, a)| len > l || (len == l && *allow && !a)) {
                    best = Some((len, *allow));
                }
            }
        }
        best.is_none_or(|(_, a)| a)
    }
}

// ───────────────────────── sitemap ─────────────────────────

fn xml_unescape(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")).unwrap_or(s);
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").trim().to_string()
}

/// 解析 sitemap：返回 (页面网址, 子 sitemap 网址)。sitemap 索引里的 loc 都算子 sitemap。
pub fn parse_sitemap(xml: &str) -> (Vec<String>, Vec<String>) {
    let is_index = xml.contains("<sitemapindex") || xml.contains(":sitemapindex");
    let mut locs = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<loc>").or_else(|| rest.find("<loc ")) {
        let after = &rest[i..];
        let Some(open_end) = after.find('>') else { break };
        let body = &after[open_end + 1..];
        let Some(close) = body.find("</loc>") else { break };
        let v = xml_unescape(&body[..close]);
        if !v.is_empty() {
            locs.push(v);
        }
        rest = &body[close + 6..];
    }
    if is_index {
        (Vec::new(), locs)
    } else {
        (locs, Vec::new())
    }
}

/// 图片 / 视频 sitemap（列的是附件页）。
pub fn is_media_sitemap(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    let name = l.rsplit('/').next().unwrap_or("");
    ["image", "video", "media", "attachment"].iter().any(|k| name.contains(k))
}

// ───────────────────────── 页面信息 ─────────────────────────

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("selector")
}

/// 不算正文的标签。
pub const SKIP_TAGS: &[&str] = &["script", "style", "noscript", "template", "svg", "head", "iframe", "object", "canvas"];
const BLOCK_TAGS: &[&str] = &[
    "p", "div", "section", "article", "main", "header", "footer", "nav", "aside", "li", "ul", "ol", "h1", "h2", "h3", "h4", "h5", "h6", "table", "tr", "td", "th", "blockquote", "pre", "figure", "figcaption", "br", "hr", "dd", "dt",
];

/// 元素里看得到的文字（跳过 script / style 等），块级元素之间加空格，空白压成一个。
pub fn visible_text(el: ElementRef) -> String {
    fn walk(el: ElementRef, out: &mut String) {
        for c in el.children() {
            match c.value() {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => {
                    let n = e.name();
                    if SKIP_TAGS.contains(&n) {
                        continue;
                    }
                    if let Some(ce) = ElementRef::wrap(c) {
                        let block = BLOCK_TAGS.contains(&n);
                        if block {
                            out.push(' ');
                        }
                        walk(ce, out);
                        if block {
                            out.push(' ');
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut s = String::new();
    walk(el, &mut s);
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

/// 字数：中日韩按字、其他按词。
pub fn word_count(s: &str) -> u32 {
    let (mut n, mut in_word) = (0u32, false);
    for ch in s.chars() {
        if is_cjk(ch) {
            n += 1;
            in_word = false;
        } else if ch.is_alphanumeric() {
            if !in_word {
                n += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    n
}

pub fn short_hash(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(s.as_bytes());
    d.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// 从一页 HTML 里提取出来的信息。
#[derive(Default, Debug)]
pub struct Extracted {
    pub title: String,
    pub description: String,
    pub canonical: String,
    pub h1: String,
    pub lang: String,
    pub hreflang: Vec<[String; 2]>,
    pub og_type: String,
    pub og_image: String,
    pub schema: Vec<String>,
    pub published: String,
    pub modified: String,
    pub author: String,
    pub tags: Vec<String>,
    pub noindex: bool,
    pub text: String,
    /// 站内链接（规范化后的路径）
    pub links: Vec<String>,
    /// 引用的图片和附件（完整网址）
    pub assets: Vec<String>,
    pub features: Vec<String>,
    pub analytics: Vec<String>,
    pub generator: String,
    /// meta refresh 跳转
    pub refresh_to: String,
}

fn meta(doc: &Html, attr: &str, name: &str) -> String {
    let s = sel(&format!("meta[{attr}]"));
    doc.select(&s)
        .find(|m| m.value().attr(attr).is_some_and(|v| v.eq_ignore_ascii_case(name)))
        .and_then(|m| m.value().attr("content"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

fn collect_schema(v: &serde_json::Value, ex: &mut Extracted, depth: u32) {
    if depth > 6 {
        return;
    }
    match v {
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_schema(x, ex, depth + 1)),
        serde_json::Value::Object(o) => {
            match o.get("@type") {
                Some(serde_json::Value::String(t)) => ex.schema.push(t.clone()),
                Some(serde_json::Value::Array(ts)) => ex.schema.extend(ts.iter().filter_map(|t| t.as_str().map(String::from))),
                _ => {}
            }
            let s = |k: &str| o.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            if ex.published.is_empty() {
                ex.published = s("datePublished");
            }
            if ex.modified.is_empty() {
                ex.modified = s("dateModified");
            }
            if ex.author.is_empty() {
                ex.author = match o.get("author") {
                    Some(serde_json::Value::String(a)) => a.clone(),
                    Some(serde_json::Value::Object(a)) => a.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    Some(serde_json::Value::Array(a)) => a.first().and_then(|x| x.get("name")).and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    _ => String::new(),
                };
            }
            if ex.tags.is_empty() {
                match o.get("keywords") {
                    Some(serde_json::Value::String(k)) => ex.tags = k.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                    Some(serde_json::Value::Array(k)) => ex.tags = k.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                    _ => {}
                }
            }
            if let Some(g) = o.get("@graph") {
                collect_schema(g, ex, depth + 1);
            }
        }
        _ => {}
    }
}

/// 统计 ID：gtag / GTM / 老的 UA。
fn find_analytics(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for pre in ["G-", "GTM-", "UA-"] {
        let mut rest = html;
        while let Some(i) = rest.find(pre) {
            let before = rest[..i].chars().last();
            let tail = &rest[i + pre.len()..];
            let id: String = tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
            let ok_before = before.is_none_or(|c| !c.is_ascii_alphanumeric());
            let ok = match pre {
                "UA-" => id.len() >= 6 && id.chars().next().is_some_and(|c| c.is_ascii_digit()) && id.contains('-'),
                _ => (6..=12).contains(&id.len()) && id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
            };
            if ok_before && ok {
                let v = format!("{pre}{id}");
                if !out.contains(&v) {
                    out.push(v);
                }
            }
            rest = &rest[i + pre.len()..];
        }
    }
    // 只认出现在统计脚本附近的：页面正文里碰巧有「G-SHOCK」之类不算
    if !(html.contains("googletagmanager") || html.contains("gtag(") || html.contains("google-analytics")) {
        out.clear();
    }
    out
}

const COMMENT_MARKS: &[&str] = &["disqus", "giscus", "utteranc", "twikoo", "waline", "valine", "commento", "wp-comments", "comment-form", "id=\"comments\"", "id=\"respond\"", "artalk", "remark42"];
/// 只认页面结构上的购物车（加购按钮、商品 ID、购物车插件），正文里提到 WooCommerce / Shopify 不算。
const CART_MARKS: &[&str] = &["add-to-cart", "add_to_cart", "woocommerce-cart", "woocommerce-checkout", "single_add_to_cart", "data-product-id", "snipcart-add-item", "shopify-payment-button"];

/// 提取一页的信息。`url` 是这一页的网址（解析相对链接用）。
pub fn extract(html: &str, url: &Url, domain: &str) -> Extracted {
    let doc = Html::parse_document(html);
    let mut ex = Extracted::default();
    let base = doc
        .select(&sel("base[href]"))
        .next()
        .and_then(|b| b.value().attr("href"))
        .and_then(|h| url.join(h).ok())
        .unwrap_or_else(|| url.clone());

    ex.title = doc.select(&sel("title")).next().map(|t| t.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default();
    ex.description = meta(&doc, "name", "description");
    ex.og_type = meta(&doc, "property", "og:type");
    ex.og_image = meta(&doc, "property", "og:image");
    ex.generator = meta(&doc, "name", "generator");
    let robots = meta(&doc, "name", "robots").to_ascii_lowercase();
    ex.noindex = robots.contains("noindex");
    ex.published = meta(&doc, "property", "article:published_time");
    ex.modified = meta(&doc, "property", "article:modified_time");
    ex.author = meta(&doc, "name", "author");
    ex.tags = doc.select(&sel("meta[property=\"article:tag\"]")).filter_map(|m| m.value().attr("content").map(|v| v.trim().to_string())).filter(|v| !v.is_empty()).collect();
    let refresh = meta(&doc, "http-equiv", "refresh");
    if let Some((_, u)) = refresh.to_ascii_lowercase().split_once("url=") {
        let raw = &refresh[refresh.len() - u.len()..];
        ex.refresh_to = base.join(raw.trim().trim_matches(['\'', '"'])).map(|u| u.to_string()).unwrap_or_default();
    }
    ex.lang = doc.select(&sel("html")).next().and_then(|h| h.value().attr("lang")).unwrap_or("").to_string();
    for l in doc.select(&sel("link[rel]")) {
        let rel = l.value().attr("rel").unwrap_or("").to_ascii_lowercase();
        let href = l.value().attr("href").unwrap_or("");
        if rel.split_whitespace().any(|r| r == "canonical") && ex.canonical.is_empty() {
            ex.canonical = base.join(href).map(|u| u.to_string()).unwrap_or_default();
        } else if rel.split_whitespace().any(|r| r == "alternate") {
            if let Some(hl) = l.value().attr("hreflang") {
                if let Ok(u) = base.join(href) {
                    ex.hreflang.push([hl.to_string(), u.to_string()]);
                }
            }
        }
    }
    ex.h1 = doc.select(&sel("h1")).next().map(visible_text).unwrap_or_default();
    for s in doc.select(&sel("script[type=\"application/ld+json\"]")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s.text().collect::<String>()) {
            collect_schema(&v, &mut ex, 0);
        }
    }
    if ex.published.is_empty() {
        ex.published = doc.select(&sel("time[datetime]")).next().and_then(|t| t.value().attr("datetime")).unwrap_or("").to_string();
    }
    ex.schema.sort();
    ex.schema.dedup();

    if let Some(body) = doc.select(&sel("body")).next() {
        ex.text = visible_text(body);
    }

    // 链接和资源
    let mut links = std::collections::BTreeSet::new();
    let mut assets = std::collections::BTreeSet::new();
    for a in doc.select(&sel("a[href]")) {
        let href = a.value().attr("href").unwrap_or("");
        if a.value().attr("rel").is_some_and(|r| r.contains("nofollow")) && href.contains("?") {
            continue;
        }
        if let Some(p) = normalize(href, &base, domain) {
            if is_asset(&p) || is_file(&p) {
                if let Ok(u) = base.join(href) {
                    assets.insert(strip_fragment(u));
                }
            } else {
                links.insert(p);
            }
        }
    }
    for img in doc.select(&sel("img, source")) {
        let v = img.value();
        for attr in ["src", "data-src", "data-lazy-src", "data-original"] {
            if let Some(src) = v.attr(attr) {
                if !src.starts_with("data:") && !src.trim().is_empty() {
                    if let Ok(u) = base.join(src.trim()) {
                        if matches!(u.scheme(), "http" | "https") {
                            assets.insert(strip_fragment(u));
                        }
                    }
                }
            }
        }
        if let Some(ss) = v.attr("srcset") {
            for part in ss.split(',') {
                if let Some(src) = part.split_whitespace().next() {
                    if let Ok(u) = base.join(src) {
                        if matches!(u.scheme(), "http" | "https") {
                            assets.insert(strip_fragment(u));
                        }
                    }
                }
            }
        }
    }
    if !ex.og_image.is_empty() {
        if let Ok(u) = base.join(&ex.og_image) {
            assets.insert(strip_fragment(u));
        }
    }
    ex.links = links.into_iter().collect();
    ex.assets = assets.into_iter().collect();

    // 动态功能
    let lower = html.to_ascii_lowercase();
    let mut f: Vec<&str> = Vec::new();
    let mut has_form = false;
    let mut has_search = false;
    for form in doc.select(&sel("form")) {
        let method = form.value().attr("method").unwrap_or("get").to_ascii_lowercase();
        let action = form.value().attr("action").unwrap_or("").to_ascii_lowercase();
        let inputs: Vec<(String, String)> = form
            .select(&sel("input, textarea, select"))
            .map(|i| (i.value().name().to_string(), format!("{} {}", i.value().attr("type").unwrap_or(""), i.value().attr("name").unwrap_or("")).to_ascii_lowercase()))
            .collect();
        let searchy = inputs.iter().any(|(_, t)| t.contains("search") || t.ends_with(" s") || t.ends_with(" q") || t.contains("keyword") || t.contains("query")) || action.contains("search");
        let password = inputs.iter().any(|(_, t)| t.starts_with("password"));
        if password {
            f.push("login");
        } else if searchy && method == "get" {
            has_search = true;
        } else if inputs.iter().any(|(n, t)| n == "textarea" || t.starts_with("email")) || method == "post" {
            has_form = true;
        }
    }
    if doc.select(&sel("input[type=\"search\"]")).next().is_some() {
        has_search = true;
    }
    if has_form {
        f.push("form");
    }
    if has_search {
        f.push("search");
    }
    if COMMENT_MARKS.iter().any(|m| lower.contains(m)) {
        f.push("comments");
    }
    let cart_links = ex.links.iter().any(|l| l.starts_with("/cart")) && ex.links.iter().any(|l| l.starts_with("/checkout"));
    if CART_MARKS.iter().any(|m| lower.contains(m)) || cart_links {
        f.push("cart");
    }
    if !f.contains(&"login") && ex.links.iter().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("/login") || l.starts_with("/signin") || l.starts_with("/register") || l.starts_with("/my-account") || l.starts_with("/account")
    }) {
        f.push("login");
    }
    f.sort();
    f.dedup();
    ex.features = f.into_iter().map(String::from).collect();
    ex.analytics = find_analytics(html);
    ex
}

fn strip_fragment(mut u: Url) -> String {
    u.set_fragment(None);
    u.to_string()
}

/// 内容看起来是客户端渲染的：正文几乎没有字，但有一个挂载点 / 大段脚本。
pub fn looks_client_rendered(html: &str, words: u32) -> bool {
    if words >= 80 {
        return false;
    }
    let l = html.to_ascii_lowercase();
    l.contains("id=\"root\"") || l.contains("id=\"app\"") || l.contains("id=\"__next\"") || l.contains("id=\"__nuxt\"") || l.contains("ng-version") || l.contains("data-reactroot") || l.matches("<script").count() >= 3
}

// ───────────────────────── 分组 ─────────────────────────

/// URL 规律分组：`/` 首页；一级路径都算 `/*`；多级按第一段 + 层数，如 `/blog/*`、`/blog/*/*`。带查询串的加 `?…`。
pub fn group_of(path: &str) -> String {
    let (p, q) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    let segs: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    let mut g = match segs.len() {
        0 => "/".to_string(),
        1 => "/*".to_string(),
        n => format!("/{}/{}", segs[0], vec!["*"; n - 1].join("/")),
    };
    if let Some(q) = q {
        let keys: Vec<&str> = q.split('&').map(|kv| kv.split('=').next().unwrap_or("")).collect();
        g.push_str(&format!("?{}=", keys.join("=&")));
    }
    g
}

/// 页面类型：首页 / 文章 / 列表 / 单页。
pub fn page_type(path: &str, ex: &Extracted, words: u32) -> &'static str {
    if path == "/" || path.starts_with("/?") || path == "/index.html" {
        return "home";
    }
    let articley = ex.og_type.eq_ignore_ascii_case("article") || ex.schema.iter().any(|t| matches!(t.as_str(), "Article" | "BlogPosting" | "NewsArticle" | "TechArticle" | "Recipe" | "HowTo" | "Review"));
    if articley || (!ex.published.is_empty() && words >= 200) {
        return "article";
    }
    // 链接很多、字不多：分类 / 标签 / 翻页之类的列表页
    let pagey = path.contains("/page/") || path.contains("page=") || path.contains("/tag/") || path.contains("/category/") || path.contains("/categories/") || path.contains("/tags/");
    if pagey || (ex.links.len() >= 40 && words < ex.links.len() as u32 * 12) {
        return "list";
    }
    "page"
}

/// 旧站路径结尾带不带 `/`（只看没有扩展名的多级路径）。
pub fn slash_style<'a>(paths: impl Iterator<Item = &'a str>) -> &'static str {
    let (mut with, mut without) = (0, 0);
    for p in paths {
        let p = p.split('?').next().unwrap_or("");
        if p == "/" || !ext_of(p).is_empty() {
            continue;
        }
        if p.ends_with('/') {
            with += 1;
        } else {
            without += 1;
        }
    }
    match (with, without) {
        (0, 0) => "always",
        (_, 0) => "always",
        (0, _) => "never",
        (w, wo) if w * 20 >= (w + wo) * 19 => "always",
        (w, wo) if wo * 20 >= (w + wo) * 19 => "never",
        _ => "mixed",
    }
}

/// 汇总分组（按页数倒序），每组最多 5 个示例（流量高的在前）。
pub fn summarize_groups(pages: &[Page]) -> Vec<Group> {
    let mut map: BTreeMap<String, (Vec<&Page>, BTreeMap<String, usize>)> = BTreeMap::new();
    for p in pages.iter().filter(|p| p.kind == "page") {
        let e = map.entry(p.group.clone()).or_default();
        e.0.push(p);
        *e.1.entry(p.ptype.clone()).or_default() += 1;
    }
    let mut out: Vec<Group> = map
        .into_iter()
        .map(|(key, (mut ps, types))| {
            ps.sort_by(|a, b| b.clicks.cmp(&a.clicks).then(b.impressions.cmp(&a.impressions)).then(a.path.cmp(&b.path)));
            Group {
                count: ps.len(),
                ptype: types.into_iter().max_by_key(|(_, n)| *n).map(|(t, _)| t).unwrap_or_default(),
                clicks: ps.iter().map(|p| p.clicks).sum(),
                samples: ps.iter().take(5).map(|p| p.path.clone()).collect(),
                key,
            }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    out
}

/// 整理标题：全站都一样的 h1（主题把站名放在 h1 里）清掉；找出页面标题统一的站名后缀（「 | 站名」「 - 站名」）。
pub fn tidy_titles(pages: &mut [Page]) -> String {
    let html: Vec<usize> = pages.iter().enumerate().filter(|(_, p)| p.kind == "page").map(|(i, _)| i).collect();
    let n = html.len();
    if n < 3 {
        return String::new();
    }
    let mut h1s: BTreeMap<String, usize> = BTreeMap::new();
    for &i in &html {
        if !pages[i].h1.is_empty() {
            *h1s.entry(pages[i].h1.clone()).or_default() += 1;
        }
    }
    for (h, c) in h1s {
        if c >= 3 && c * 10 >= n * 3 {
            for &i in &html {
                if pages[i].h1 == h && pages[i].ptype != "home" {
                    pages[i].h1.clear();
                }
            }
        }
    }
    let mut sufs: BTreeMap<String, usize> = BTreeMap::new();
    for &i in &html {
        let t = &pages[i].title;
        for sep in [" | ", " - ", " – ", " — ", " · ", "_"] {
            if let Some(pos) = t.rfind(sep) {
                if pos > 0 {
                    *sufs.entry(t[pos..].to_string()).or_default() += 1;
                }
            }
        }
    }
    sufs.into_iter().filter(|(_, c)| *c >= 3 && *c * 10 >= n * 4).max_by_key(|(_, c)| *c).map(|(s, _)| s).unwrap_or_default()
}

// ───────────────────────── 动态功能 ─────────────────────────

/// 汇总动态功能，附上迁过去的处理办法。
pub fn summarize_features(pages: &[Page]) -> Vec<Feature> {
    const DEFS: &[(&str, &str, &str, bool)] = &[
        ("search", "站内搜索", "改成静态搜索索引（Pagefind），构建时生成，不需要服务器", true),
        ("form", "表单（留言 / 联系 / 订阅）", "用 Cloudflare Pages Functions + D1 接收，Pilot 自动建好；提交在 Pilot 里看", false),
        ("comments", "评论", "旧评论抓下来静态保留；新评论存 D1、在 Pilot 里审核后显示", false),
        ("login", "登录 / 会员", "静态站做不了，需要服务端：建议部署到 VPS（后续支持），或者迁移时去掉", false),
        ("cart", "购物车 / 下单", "静态站做不了，需要服务端：建议部署到 VPS（后续支持），或者改用第三方商店", false),
    ];
    DEFS.iter()
        .filter_map(|(key, label, plan, ok)| {
            let ps: Vec<&Page> = pages.iter().filter(|p| p.features.iter().any(|f| f == key)).collect();
            if ps.is_empty() {
                return None;
            }
            Some(Feature { key: (*key).into(), label: (*label).into(), pages: ps.len(), samples: ps.iter().take(3).map(|p| p.path.clone()).collect(), plan: (*plan).into(), static_ok: *ok })
        })
        .collect()
}

// ───────────────────────── 部署去向 ─────────────────────────

/// Cloudflare Pages 的限制（2026-10 核对官方文档）：每个站免费 20,000 个文件、付费 100,000；单个文件 25 MiB；
/// `_redirects` 最多 2,000 条静态规则。
pub const CF_FREE_FILES: usize = 20_000;
pub const CF_PAID_FILES: usize = 100_000;
pub const CF_MAX_FILE: u64 = 25 * 1024 * 1024;
pub const CF_MAX_REDIRECTS: usize = 2_000;

pub struct HostingInput<'a> {
    pub pages: usize,
    pub assets: usize,
    pub largest_file: u64,
    pub features: &'a [Feature],
    pub cn_share: Option<f64>,
}

/// 优先 Cloudflare，满足不了再用 VPS。每条结论都写理由。
pub fn decide_hosting(i: &HostingInput) -> Hosting {
    let mut reasons = Vec::new();
    let mut vps = false;
    let mut workers = false;
    let mut paid = false;
    let has = |k: &str| i.features.iter().find(|f| f.key == k);
    for k in ["login", "cart"] {
        if let Some(f) = has(k) {
            vps = true;
            reasons.push(format!("检测到{}（{} 页），静态站和 Workers 都不适合承载，需要服务端程序", f.label, f.pages));
        }
    }
    if let Some(s) = i.cn_share {
        if s >= 0.5 {
            vps = true;
            reasons.push(format!("近 28 天约 {:.0}% 的访客来自中国大陆：Cloudflare 在大陆访问慢且不稳定，放香港或国内（需备案）的服务器体验更好", s * 100.0));
        }
    }
    let files = i.pages + i.assets;
    if files > CF_PAID_FILES {
        vps = true;
        reasons.push(format!("页面和文件约 {files} 个，超过 Cloudflare Pages 每个站 {CF_PAID_FILES} 个文件的上限"));
    } else if files > CF_FREE_FILES {
        paid = true;
        reasons.push(format!("页面和文件约 {files} 个，超过 Cloudflare Pages 免费套餐 {CF_FREE_FILES} 个文件的上限，需要付费套餐"));
    }
    if i.largest_file > CF_MAX_FILE {
        workers = true;
        reasons.push(format!("有超过 25 MB 的文件（最大约 {} MB），Pages 放不下，放到 R2（Pilot 自动上传，地址不变）", i.largest_file / 1024 / 1024));
    }
    for k in ["form", "comments"] {
        if let Some(f) = has(k) {
            workers = true;
            reasons.push(format!("检测到{}（{} 页），用 Cloudflare Pages Functions + D1 实现，仍然是同一个 Pages 项目", f.label, f.pages));
        }
    }
    if let Some(f) = has("search") {
        reasons.push(format!("检测到{}（{} 页），改成静态搜索索引就行，不影响部署方式", f.label, f.pages));
    }
    let (target, label) = if vps {
        ("vps", "部署到 VPS")
    } else if workers {
        ("cf_workers", "Cloudflare Pages + Functions（静态 + 动态功能）")
    } else {
        ("cf_static", "Cloudflare Pages（纯静态）")
    };
    if target == "cf_static" && reasons.is_empty() {
        reasons.push("没有发现需要服务端的功能，整站可以做成静态页面：速度快、免费额度够用、不用维护服务器".into());
    }
    Hosting { target: target.into(), label: label.into(), reasons, paid, supported: true }
}

/// 按方案重新判断部署去向：方案里去掉的动态功能不再算（比如去掉登录 / 购物车后就能放 Cloudflare）。
pub fn hosting_for(inv: &Inventory, features: &BTreeMap<String, String>) -> Hosting {
    let kept: Vec<Feature> = inv.features.iter().filter(|f| features.get(&f.key).map(|v| v != "drop").unwrap_or(true)).cloned().collect();
    let largest = inv.pages.iter().filter(|p| p.kind == "file").map(|p| p.bytes).max().unwrap_or(0);
    decide_hosting(&HostingInput { pages: inv.html_pages().count(), assets: inv.assets.len(), largest_file: largest, features: &kept, cn_share: inv.cn_share })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_and_site() {
        assert_eq!(clean_domain(" https://WWW.Example.com:443/a?b "), "www.example.com");
        assert!(same_site("www.example.com", "example.com"));
        assert!(same_site("example.com", "www.example.com"));
        assert!(!same_site("blog.example.com", "example.com"));
    }

    #[test]
    fn normalizes_links() {
        let base = Url::parse("https://example.com/blog/post/").unwrap();
        assert_eq!(normalize("../other/?utm_source=x&page=2#top", &base, "example.com").as_deref(), Some("/blog/other/?page=2"));
        assert_eq!(normalize("https://www.example.com", &base, "example.com").as_deref(), Some("/"));
        assert_eq!(normalize("https://other.com/x", &base, "example.com"), None);
        assert_eq!(normalize("mailto:a@b.c", &base, "example.com"), None);
        assert_eq!(normalize("#x", &base, "example.com"), None);
        assert_eq!(normalize("/a?utm_medium=y", &base, "example.com").as_deref(), Some("/a"));
    }

    #[test]
    fn classifies_paths() {
        assert!(is_asset("/img/a.PNG"));
        assert!(is_file("/doc/manual.pdf"));
        assert!(!is_asset("/blog/v1.2/"));
        assert_eq!(ext_of("/a/b.html"), "html");
        assert_eq!(ext_of("/a/.well-known"), "");
        assert!(is_junk("/post/?replytocom=12"));
        assert!(is_junk("/wp-admin/x"));
        assert!(!is_junk("/blog/login-guide/"));
        assert!(!is_junk("/list?page=2"));
        assert!(is_junk("/login/"));
    }

    #[test]
    fn robots_rules() {
        let r = parse_robots("User-agent: Googlebot\nDisallow: /\n\nUser-agent: *\nDisallow: /private/\nAllow: /private/ok\nDisallow: /*.pdf$\nCrawl-delay: 2\nSitemap: https://example.com/sitemap.xml\n");
        assert!(r.allowed("/blog/"));
        assert!(!r.allowed("/private/x"));
        assert!(r.allowed("/private/ok/y"));
        assert!(!r.allowed("/a/b.pdf"));
        assert!(r.allowed("/a/b.pdf?x"));
        assert_eq!(r.crawl_delay, Some(2.0));
        assert_eq!(r.sitemaps, vec!["https://example.com/sitemap.xml"]);
    }

    #[test]
    fn sitemaps() {
        let (u, s) = parse_sitemap("<urlset><url><loc>https://e.com/a?x=1&amp;y=2</loc></url><url><loc><![CDATA[https://e.com/b]]></loc></url></urlset>");
        assert_eq!(u, vec!["https://e.com/a?x=1&y=2", "https://e.com/b"]);
        assert!(s.is_empty());
        let (u, s) = parse_sitemap("<sitemapindex><sitemap><loc>https://e.com/s1.xml</loc></sitemap></sitemapindex>");
        assert!(u.is_empty());
        assert_eq!(s, vec!["https://e.com/s1.xml"]);
    }

    #[test]
    fn extracts_page() {
        let html = r#"<html lang="zh-CN"><head><title> Hello  World </title>
<meta name="description" content="desc"><link rel="canonical" href="/a/">
<link rel="alternate" hreflang="en" href="https://example.com/en/a/">
<meta property="og:type" content="article">
<script type="application/ld+json">{"@context":"https://schema.org","@graph":[{"@type":"BlogPosting","datePublished":"2024-01-02","author":{"name":"Ann"}},{"@type":"BreadcrumbList"}]}</script>
<script async src="https://www.googletagmanager.com/gtag/js?id=G-ABC123XYZ"></script>
</head><body><nav><a href="/">首页</a></nav><h1>标题</h1><p>正文 text here</p>
<img data-src="/img/x.png" src="data:image/gif;base64,AA"><a href="/doc.pdf">pdf</a>
<form method="get" action="/search"><input type="search" name="q"></form>
<div id="comments"></div><script>var x = 1;</script></body></html>"#;
        let u = Url::parse("https://example.com/a/").unwrap();
        let ex = extract(html, &u, "example.com");
        assert_eq!(ex.title, "Hello World");
        assert_eq!(ex.description, "desc");
        assert_eq!(ex.canonical, "https://example.com/a/");
        assert_eq!(ex.lang, "zh-CN");
        assert_eq!(ex.hreflang.len(), 1);
        assert_eq!(ex.schema, vec!["BlogPosting", "BreadcrumbList"]);
        assert_eq!(ex.published, "2024-01-02");
        assert_eq!(ex.author, "Ann");
        assert_eq!(ex.h1, "标题");
        assert!(!ex.text.contains("var x"));
        assert!(ex.links.contains(&"/".to_string()));
        assert!(ex.assets.contains(&"https://example.com/img/x.png".to_string()));
        assert!(ex.assets.contains(&"https://example.com/doc.pdf".to_string()));
        assert_eq!(ex.features, vec!["comments", "search"]);
        assert_eq!(ex.analytics, vec!["G-ABC123XYZ"]);
        assert_eq!(page_type("/a/", &ex, 10), "article");
    }

    #[test]
    fn counts_words() {
        assert_eq!(word_count("Hello, world! 你好世界"), 6);
    }

    #[test]
    fn groups() {
        assert_eq!(group_of("/"), "/");
        assert_eq!(group_of("/about/"), "/*");
        assert_eq!(group_of("/blog/hello/"), "/blog/*");
        assert_eq!(group_of("/blog/2020/01/x.html"), "/blog/*/*/*");
        assert_eq!(group_of("/list?page=2&sort=a"), "/*?page=&sort=");
        assert_eq!(slash_style(["/a/", "/b/", "/c.html", "/"].into_iter()), "always");
        assert_eq!(slash_style(["/a", "/b/c"].into_iter()), "never");
        assert_eq!(slash_style(["/a", "/b/"].into_iter()), "mixed");
    }

    #[test]
    fn hosting() {
        let none: Vec<Feature> = Vec::new();
        let h = decide_hosting(&HostingInput { pages: 100, assets: 300, largest_file: 1000, features: &none, cn_share: None });
        assert_eq!(h.target, "cf_static");
        assert!(h.supported);
        let f = vec![Feature { key: "form".into(), label: "表单".into(), pages: 2, ..Default::default() }];
        let h = decide_hosting(&HostingInput { pages: 100, assets: 300, largest_file: 1000, features: &f, cn_share: Some(0.1) });
        assert_eq!(h.target, "cf_workers");
        assert!(h.supported);
        let h = decide_hosting(&HostingInput { pages: 100, assets: 30_000, largest_file: 1000, features: &none, cn_share: Some(0.8) });
        assert_eq!(h.target, "vps");
        assert!(h.paid);
        // 有购物车 → VPS；方案里去掉购物车 → 回到 Cloudflare
        let inv = Inventory { features: vec![Feature { key: "cart".into(), label: "购物车".into(), pages: 3, ..Default::default() }], ..Default::default() };
        assert_eq!(hosting_for(&inv, &BTreeMap::new()).target, "vps");
        let drop: BTreeMap<String, String> = [("cart".to_string(), "drop".to_string())].into();
        assert_eq!(hosting_for(&inv, &drop).target, "cf_static");
    }
}
