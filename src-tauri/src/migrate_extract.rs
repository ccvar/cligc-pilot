//! 站点迁移的抓取和核对（纯函数）：从旧站页面里取出正文写成 Markdown、生成 URL 对应表、
//! 拿新站的构建产物逐条对照旧站。
//!
//! 正文提取不用 AI：同一类模板的页面里反复出现的块（导航、页脚、侧栏、分享按钮）就是模板，去掉后剩下的是正文；
//! 再按常见的正文容器 / 段落密度找主体。表格（复杂的）、视频、iframe 原样保留成 HTML。

use crate::site_migrate::{self as sm, Inventory, Page, Plan};
use reqwest::Url;
use scraper::{node::Node, ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("selector")
}

// ───────────────────────── 模板块（boilerplate） ─────────────────────────

/// 参与模板判断的块。小标题不算：「常见问题」「参考资料」这种每篇都有的标题是正文结构，不是模板。
const SIG_TAGS: &[&str] = &["p", "li", "div", "section", "nav", "header", "footer", "aside", "ul", "ol", "table", "form", "figure", "blockquote", "span", "a"];

fn sig(tag: &str, text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tag.hash(&mut h);
    text.hash(&mut h);
    h.finish()
}

/// 一个块的签名：标签 + 文字。文字太长的（整页正文的大容器）不算，它不会是模板。
fn block_sig(el: ElementRef) -> Option<u64> {
    let tag = el.value().name();
    if !SIG_TAGS.contains(&tag) {
        return None;
    }
    let t = sm::visible_text(el);
    if t.is_empty() || t.chars().count() > 600 {
        return None;
    }
    Some(sig(tag, &t))
}

/// 一页里所有块的签名（去重）。
pub fn block_sigs(html: &str) -> Vec<u64> {
    let doc = Html::parse_document(html);
    let mut s: Vec<u64> = doc.select(&sel("body *")).filter_map(block_sig).collect();
    s.sort_unstable();
    s.dedup();
    s
}

/// 找出模板块：全站超过 40% 的页面里都有（至少 3 页），或者同一组里超过 60% 的页面都有（组里至少 4 页）。
pub fn boilerplate(pages: &[(String, Vec<u64>)]) -> HashSet<u64> {
    let mut all: HashMap<u64, usize> = HashMap::new();
    let mut by_group: HashMap<&str, (usize, HashMap<u64, usize>)> = HashMap::new();
    for (g, sigs) in pages {
        let e = by_group.entry(g.as_str()).or_default();
        e.0 += 1;
        for s in sigs {
            *all.entry(*s).or_default() += 1;
            *e.1.entry(*s).or_default() += 1;
        }
    }
    let n = pages.len();
    let mut out: HashSet<u64> = HashSet::new();
    if n >= 3 {
        out.extend(all.iter().filter(|(_, c)| **c >= 3 && **c * 10 >= n * 4).map(|(s, _)| *s));
    }
    for (_, (gn, m)) in by_group {
        if gn >= 4 {
            out.extend(m.iter().filter(|(_, c)| **c * 10 >= gn * 6).map(|(s, _)| *s));
        }
    }
    out
}

// ───────────────────────── 正文 ─────────────────────────

/// 不进正文的标签。
const DROP_TAGS: &[&str] = &["script", "style", "noscript", "template", "svg", "canvas", "object", "nav", "aside", "footer", "header", "form", "button", "input", "select", "textarea", "link", "meta", "dialog"];
/// class / id 里出现这些词的块不进正文（分享按钮、相关文章、评论、面包屑……）。
const NOISE: &[&str] = &[
    "share", "sharing", "social", "related", "comment", "comments", "breadcrumb", "breadcrumbs", "newsletter", "subscribe", "advert", "advertisement", "ads", "ad", "sidebar", "cookie", "popup", "modal", "pagination", "nav-links", "post-nav", "author-box", "toc-toggle", "sponsor", "promo", "widget",
];
const RAW_TAGS: &[&str] = &["iframe", "video", "audio", "embed"];
/// 原样输出时保留的属性。
const KEEP_ATTRS: &[&str] = &["href", "src", "alt", "title", "width", "height", "colspan", "rowspan", "start", "controls", "poster", "allow", "allowfullscreen", "frameborder", "loading", "type", "datetime"];
const MAIN_SEL: &[&str] = &[
    "article .entry-content", "article .post-content", ".entry-content", ".post-content", ".article-content", ".article-body", ".post-body", ".markdown-body", ".prose", "article", "main", "[role=main]", "#content", ".content",
];

fn noisy(el: ElementRef) -> bool {
    let v = el.value();
    let mut toks = Vec::new();
    for a in ["class", "id", "role"] {
        if let Some(x) = v.attr(a) {
            toks.extend(x.split(|c: char| c.is_whitespace() || c == '-' || c == '_').map(|t| t.to_ascii_lowercase()));
        }
    }
    if v.attr("aria-hidden") == Some("true") || v.attr("hidden").is_some() {
        return true;
    }
    // 整个 class 也比一下（nav-links、post-nav、author-box 这种带连字符的）
    let whole: Vec<String> = ["class", "id"].iter().filter_map(|a| v.attr(a)).flat_map(|x| x.split_whitespace().map(|t| t.to_ascii_lowercase()).collect::<Vec<_>>()).collect();
    toks.iter().any(|t| NOISE.contains(&t.as_str())) || whole.iter().any(|t| NOISE.contains(&t.as_str()))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
fn esc_attr(s: &str) -> String {
    esc(s).replace('"', "&quot;")
}

/// 百分号解码（文件落盘用；URL 里还是原样）。
pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
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

/// Cloudflare「邮箱地址保护」加密过的邮箱（data-cfemail / /cdn-cgi/l/email-protection#…）：第一个字节是密钥，其余逐字节异或。
pub fn cf_email(hex: &str) -> Option<String> {
    let b: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()).collect();
    let (k, rest) = b.split_first()?;
    let s: String = rest.iter().map(|c| (c ^ k) as char).collect();
    s.contains('@').then_some(s)
}

/// 图片和附件在新站里的路径：本站的保持原路径；外站的放到 `/assets/ext/<主机>/<路径>`（有查询串的在文件名后加哈希）。
pub fn local_asset(u: &Url, domain: &str) -> String {
    let host = u.host_str().unwrap_or("");
    let mut p = u.path().to_string();
    if let Some(q) = u.query() {
        let h = sm::short_hash(q);
        p = match p.rsplit_once('.') {
            Some((stem, ext)) if !ext.contains('/') => format!("{stem}-{}.{ext}", &h[..8]),
            _ => format!("{p}-{}", &h[..8]),
        };
    }
    if sm::same_site(host, domain) {
        p
    } else {
        format!("/assets/ext/{host}{p}")
    }
}

struct Ser<'a> {
    url: &'a Url,
    domain: &'a str,
    boiler: &'a HashSet<u64>,
    drop_title: bool,
    raws: Vec<String>,
    /// 原网址 → 新站路径
    assets: BTreeMap<String, String>,
    words: u32,
}

impl Ser<'_> {
    fn rewrite_href(&self, href: &str) -> String {
        let Ok(u) = self.url.join(href.trim()) else { return href.to_string() };
        if !matches!(u.scheme(), "http" | "https") {
            return href.to_string();
        }
        if sm::same_site(u.host_str().unwrap_or(""), self.domain) {
            let p = sm::path_of(&u);
            if sm::is_asset(&p) || sm::is_file(&p) {
                return p;
            }
            match u.fragment() {
                Some(f) => format!("{p}#{f}"),
                None => p,
            }
        } else {
            u.to_string()
        }
    }

    fn rewrite_src(&mut self, src: &str) -> String {
        let Ok(mut u) = self.url.join(src.trim()) else { return src.to_string() };
        if !matches!(u.scheme(), "http" | "https") {
            return src.to_string();
        }
        u.set_fragment(None);
        let local = local_asset(&u, self.domain);
        self.assets.insert(u.to_string(), local.clone());
        local
    }

    fn img_src(el: ElementRef) -> Option<String> {
        let v = el.value();
        for a in ["data-src", "data-lazy-src", "data-original", "src"] {
            if let Some(s) = v.attr(a) {
                if !s.trim().is_empty() && !s.starts_with("data:") {
                    return Some(s.to_string());
                }
            }
        }
        v.attr("srcset").and_then(|ss| ss.split(',').next_back()).and_then(|p| p.split_whitespace().next()).map(String::from)
    }

    fn open_tag(&mut self, el: ElementRef, out: &mut String) {
        let name = el.value().name();
        out.push('<');
        out.push_str(name);
        if name == "img" {
            if let Some(s) = Self::img_src(el) {
                let s = self.rewrite_src(&s);
                out.push_str(&format!(" src=\"{}\"", esc_attr(&s)));
            }
        }
        for (k, v) in el.value().attrs() {
            if name == "img" && k == "src" {
                continue;
            }
            if k == "class" && (name == "code" || name == "pre") && (v.starts_with("language-") || v.starts_with("lang-")) {
                out.push_str(&format!(" class=\"{}\"", esc_attr(v.split_whitespace().next().unwrap_or(""))));
                continue;
            }
            if !KEEP_ATTRS.contains(&k) {
                continue;
            }
            let v = match k {
                "href" if v.contains("/cdn-cgi/l/email-protection#") => v.rsplit('#').next().and_then(cf_email).map(|m| format!("mailto:{m}")).unwrap_or_default(),
                "href" => self.rewrite_href(v),
                "src" | "poster" => self.rewrite_src(v),
                _ => v.to_string(),
            };
            out.push_str(&format!(" {k}=\"{}\"", esc_attr(&v)));
        }
        out.push('>');
    }

    /// 输出 el 的子节点。`raw`＝原样 HTML 模式（复杂表格里），不再做占位替换。
    fn children(&mut self, el: ElementRef, out: &mut String, raw: bool) {
        for c in el.children() {
            match c.value() {
                Node::Text(t) => {
                    self.words += sm::word_count(t);
                    out.push_str(&esc(t));
                }
                Node::Element(e) => {
                    let Some(ce) = ElementRef::wrap(c) else { continue };
                    let name = e.name();
                    if let Some(mail) = e.attr("data-cfemail").and_then(cf_email) {
                        out.push_str(&esc(&mail));
                        continue;
                    }
                    if DROP_TAGS.contains(&name) || noisy(ce) {
                        continue;
                    }
                    if block_sig(ce).is_some_and(|s| self.boiler.contains(&s)) {
                        continue;
                    }
                    if name == "h1" && self.drop_title {
                        self.drop_title = false;
                        continue;
                    }
                    if !raw && (RAW_TAGS.contains(&name) || (name == "table" && complex_table(ce))) {
                        let mut r = String::new();
                        self.element(ce, &mut r, true);
                        out.push_str(&format!("<p>PILOTRAW{}X</p>", self.raws.len()));
                        self.raws.push(r);
                        continue;
                    }
                    self.element(ce, out, raw);
                }
                _ => {}
            }
        }
    }

    fn element(&mut self, el: ElementRef, out: &mut String, raw: bool) {
        let name = el.value().name();
        const VOID: &[&str] = &["img", "br", "hr", "source", "embed", "wbr", "col"];
        // 只是包一层的标签：不输出标签本身（htmd 对 div 也按块处理，留着 div 当段落分隔）
        let transparent = matches!(name, "span" | "font" | "center" | "small" | "label" | "picture" | "main" | "body") && !raw;
        if transparent {
            self.children(el, out, raw);
            return;
        }
        // 折叠问答：问题当三级标题，答案照常
        let tag = match name {
            "section" | "article" | "div" | "details" if !raw => "div",
            "summary" if !raw => "h3",
            _ => name,
        };
        if tag != name {
            out.push_str(&format!("<{tag}>"));
        } else {
            self.open_tag(el, out);
        }
        if VOID.contains(&name) {
            return;
        }
        self.children(el, out, raw);
        out.push_str(&format!("</{tag}>"));
    }
}

/// 有合并单元格或嵌套表格的表：Markdown 表格表达不了，原样保留。
fn complex_table(el: ElementRef) -> bool {
    el.select(&sel("[colspan], [rowspan], table")).next().is_some()
}

/// 正文提取结果。
#[derive(Debug, Default)]
pub struct Content {
    pub markdown: String,
    /// 正文的纯文字（核对时用来比对新站）
    pub text: String,
    /// 正文字数 / 去掉模板后全页字数：太低说明可能没抓全
    pub coverage: f32,
    /// 原网址 → 新站路径
    pub assets: BTreeMap<String, String>,
}

/// 选正文容器：常见的正文选择器里第一个字数够的；都不行就按段落密度挑。
fn main_element<'a>(doc: &'a Html, body_words: u32) -> Option<ElementRef<'a>> {
    for s in MAIN_SEL {
        let sl = sel(s);
        let mut it = doc.select(&sl);
        if let Some(el) = it.next() {
            // 同一选择器命中好几个（列表页的多个 article）就不当正文容器
            if it.next().is_some() && (*s == "article" || *s == ".content") {
                continue;
            }
            let w = sm::word_count(&sm::visible_text(el));
            if w >= 20 && w * 5 >= body_words {
                return Some(el);
            }
        }
    }
    const DENSE: &[&str] = &["p", "pre", "blockquote", "ul", "ol", "table", "h2", "h3", "h4", "figure", "dl"];
    let mut best: Option<(usize, ElementRef)> = None;
    for el in doc.select(&sel("body *")) {
        let score: usize = el
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|c| DENSE.contains(&c.value().name()))
            .map(|c| sm::visible_text(c).chars().count())
            .sum();
        if score > best.map(|b| b.0).unwrap_or(0) {
            best = Some((score, el));
        }
    }
    match best {
        Some((s, el)) if s >= 100 => Some(el),
        _ => doc.select(&sel("body")).next(),
    }
}

/// 从一页里取出正文，转成 Markdown。图片、附件改成新站路径，站内链接改成相对路径。
pub fn extract_content(html: &str, url: &Url, domain: &str, boiler: &HashSet<u64>) -> Content {
    let doc = Html::parse_document(html);
    let Some(body) = doc.select(&sel("body")).next() else { return Content::default() };
    // 去掉模板以后全页的字数
    let mut measure = Ser { url, domain, boiler, drop_title: false, raws: Vec::new(), assets: BTreeMap::new(), words: 0 };
    let mut sink = String::new();
    measure.children(body, &mut sink, false);
    let page_words = measure.words.max(1);

    let Some(mut main) = main_element(&doc, sm::word_count(&sm::visible_text(body))) else { return Content::default() };
    // 正文容器外面还有实质内容（比如和正文并列的「常见问题」一节）：往上一层，只要去掉模板后确实多出内容就扩大范围
    let words_of = |el: ElementRef| {
        let mut m = Ser { url, domain, boiler, drop_title: false, raws: Vec::new(), assets: BTreeMap::new(), words: 0 };
        let mut sink = String::new();
        m.children(el, &mut sink, false);
        m.words
    };
    let mut cur_words = words_of(main);
    for _ in 0..4 {
        let Some(parent) = main.parent().and_then(ElementRef::wrap) else { break };
        if matches!(parent.value().name(), "body" | "html") {
            break;
        }
        let pw = words_of(parent);
        if pw >= cur_words + (cur_words / 20).max(20) {
            main = parent;
            cur_words = pw;
        } else {
            break;
        }
    }
    let has_h1_inside = main.select(&sel("h1")).next().is_some();
    let mut s = Ser { url, domain, boiler, drop_title: has_h1_inside, raws: Vec::new(), assets: BTreeMap::new(), words: 0 };
    let mut clean = String::new();
    s.children(main, &mut clean, false);
    let text = {
        let frag = Html::parse_fragment(&clean);
        sm::visible_text(frag.root_element())
    };
    let mut md = htmd::convert(&clean).unwrap_or_default();
    for (i, r) in s.raws.iter().enumerate() {
        md = md.replace(&format!("PILOTRAW{i}X"), &format!("\n{r}\n"));
    }
    // 连续空行压成一个
    let mut out = String::new();
    let mut blank = 0;
    for line in md.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Content { markdown: out.trim().to_string() + "\n", text, coverage: (s.words as f32 / page_words as f32).min(1.0), assets: s.assets }
}

// ───────────────────────── 写进 Astro 项目 ─────────────────────────

fn slug_seg(s: &str) -> String {
    let d = pct_decode(s);
    let t: String = d
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .collect();
    let t = t.trim_matches(['-', '.']).to_string();
    if t.is_empty() {
        "x".into()
    } else {
        t
    }
}

/// 这一页的内容文件放在哪：`src/content/<集合>/<路径>.md`。
/// 多级路径用第一段当集合（`/blog/a/` → blog/a），一级路径和首页放进 pages。
pub fn content_path(path: &str) -> (String, String) {
    let (p, q) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    let mut segs: Vec<String> = p.split('/').filter(|s| !s.is_empty()).map(slug_seg).collect();
    if let Some(last) = segs.last_mut() {
        for ext in [".html", ".htm", ".php", ".aspx", ".asp", ".jsp"] {
            if let Some(x) = last.strip_suffix(ext) {
                *last = x.to_string();
            }
        }
    }
    let collection = if segs.len() >= 2 {
        let c: String = segs[0].to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        if c.is_empty() || c.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            "pages".to_string()
        } else {
            segs.remove(0);
            c
        }
    } else {
        "pages".to_string()
    };
    let mut rel = if segs.is_empty() { "index".to_string() } else { segs.join("/") };
    if let Some(q) = q {
        rel.push_str(&format!("--{}", slug_seg(&q.replace(['=', '&'], "-"))));
    }
    (collection, rel)
}

fn yaml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Markdown 文件开头的元数据。`path` 是旧站的路径（新站必须原样生成）。
pub fn frontmatter(p: &Page, og_image: &str, title_suffix: &str) -> String {
    let mut f = String::from("---\n");
    let short = if title_suffix.is_empty() { p.title.as_str() } else { p.title.strip_suffix(title_suffix).unwrap_or(&p.title) };
    let title = if !p.h1.is_empty() { p.h1.as_str() } else { short.trim() };
    f.push_str(&format!("title: {}\n", yaml_str(title)));
    f.push_str(&format!("seoTitle: {}\n", yaml_str(&p.title)));
    f.push_str(&format!("description: {}\n", yaml_str(&p.description)));
    f.push_str(&format!("path: {}\n", yaml_str(&p.path)));
    if !p.published.is_empty() {
        f.push_str(&format!("date: {}\n", yaml_str(&p.published)));
    }
    if !p.modified.is_empty() {
        f.push_str(&format!("updated: {}\n", yaml_str(&p.modified)));
    }
    if !p.author.is_empty() {
        f.push_str(&format!("author: {}\n", yaml_str(&p.author)));
    }
    if !p.tags.is_empty() {
        f.push_str(&format!("tags: {}\n", serde_json::to_string(&p.tags).unwrap_or_else(|_| "[]".into())));
    }
    if !og_image.is_empty() {
        f.push_str(&format!("image: {}\n", yaml_str(og_image)));
    }
    if !p.lang.is_empty() {
        f.push_str(&format!("lang: {}\n", yaml_str(&p.lang)));
    }
    if !p.schema.is_empty() {
        f.push_str(&format!("schema: {}\n", serde_json::to_string(&p.schema).unwrap_or_else(|_| "[]".into())));
    }
    if p.noindex {
        f.push_str("noindex: true\n");
    }
    f.push_str(&format!("type: {}\n", yaml_str(&p.ptype)));
    f.push_str("---\n\n");
    f
}

// ───────────────────────── URL 对应表 ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct UrlEntry {
    pub old: String,
    /// 新站上的路径（keep 时和 old 一样；redirect 时是跳转目标）
    pub new: String,
    /// keep 原样保留 | redirect 301 跳转 | drop 不迁（新站上 404）
    pub action: String,
    /// home | article | list | page | feed | file | query
    pub kind: String,
    #[serde(default)]
    pub group: String,
    /// 内容文件（相对仓库根目录），列表页 / 首页没有
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct UrlMap {
    pub domain: String,
    /// always | never | mixed
    pub trailing_slash: String,
    pub entries: Vec<UrlEntry>,
}

/// 按盘点结果和方案生成对应表。`files`：路径 → 内容文件。
pub fn build_url_map(inv: &Inventory, plan: &Plan, files: &HashMap<String, String>) -> UrlMap {
    let mut entries = Vec::new();
    for p in &inv.pages {
        let (action, kind, new, note) = match p.kind.as_str() {
            "page" if p.path.contains('?') => (
                "drop",
                "query".to_string(),
                String::new(),
                "带查询参数的地址：静态站按路径出页面，不能按参数区分，这类页面不迁".to_string(),
            ),
            "page" => {
                if plan.keeps(p) {
                    ("keep", p.ptype.clone(), p.path.clone(), String::new())
                } else {
                    ("drop", p.ptype.clone(), String::new(), "方案里选了不迁".to_string())
                }
            }
            "feed" | "file" => ("keep", p.kind.clone(), p.path.clone(), String::new()),
            "redirect" if !p.redirect_to.is_empty() => ("redirect", "redirect".to_string(), p.redirect_to.clone(), "旧站上就在跳转".to_string()),
            _ => continue,
        };
        entries.push(UrlEntry {
            old: p.path.clone(),
            new,
            action: action.into(),
            kind,
            group: p.group.clone(),
            file: files.get(&p.path).cloned().unwrap_or_default(),
            title: p.title.clone(),
            note,
        });
    }
    UrlMap { domain: inv.domain.clone(), trailing_slash: inv.trailing_slash.clone(), entries }
}

// ───────────────────────── 核对 ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParityCheck {
    pub key: String,
    pub label: String,
    /// ok | warn | block（block 不过不让切换）
    pub level: String,
    pub detail: String,
    pub items: Vec<String>,
}

fn check(key: &str, label: &str, bad: Vec<String>, block: bool, ok_detail: &str, bad_detail: impl Fn(usize) -> String) -> ParityCheck {
    let level = if bad.is_empty() { "ok" } else if block { "block" } else { "warn" };
    ParityCheck { key: key.into(), label: label.into(), level: level.into(), detail: if bad.is_empty() { ok_detail.into() } else { bad_detail(bad.len()) }, items: bad.into_iter().take(200).collect() }
}

/// Cloudflare Pages 怎么用文件回应一个路径：`/a/` → a/index.html；`/a` → a.html（只有 a/index.html 时会 308 到 `/a/`）。
/// 返回 (文件, 末尾斜杠对不上)。
pub fn out_file_for(out: &Path, path: &str) -> (Option<PathBuf>, bool) {
    let p = pct_decode(path.split('?').next().unwrap_or("/"));
    let rel = p.trim_start_matches('/');
    if rel.is_empty() {
        let f = out.join("index.html");
        return (f.is_file().then_some(f), false);
    }
    if rel.ends_with('/') {
        let f = out.join(rel).join("index.html");
        if f.is_file() {
            return (Some(f), false);
        }
        let alt = out.join(format!("{}.html", rel.trim_end_matches('/')));
        return (None, alt.is_file());
    }
    let exact = out.join(rel);
    if exact.is_file() {
        return (Some(exact), false);
    }
    let html = out.join(format!("{rel}.html"));
    if html.is_file() {
        return (Some(html), false);
    }
    (None, out.join(rel).join("index.html").is_file())
}

/// `_redirects` 里的规则：(来源, 目标, 状态码)。
pub fn parse_redirects(txt: &str) -> Vec<(String, String, u16)> {
    txt.lines()
        .filter_map(|l| {
            let l = l.split('#').next().unwrap_or("").trim();
            let mut it = l.split_whitespace();
            let from = it.next()?.to_string();
            let to = it.next()?.to_string();
            let code = it.next().and_then(|c| c.trim_end_matches('!').parse().ok()).unwrap_or(302);
            Some((from, to, code))
        })
        .collect()
}

fn redirect_match<'a>(rules: &'a [(String, String, u16)], path: &str) -> Option<&'a (String, String, u16)> {
    let p = path.split('?').next().unwrap_or(path);
    rules.iter().find(|(from, _, _)| from == p || from.strip_suffix('*').is_some_and(|pre| p.starts_with(pre)))
}

/// 文字片段集合（4 个词 / 字一组），用来比较正文有没有被改动。
pub fn shingles(text: &str) -> HashSet<u64> {
    let mut toks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if sm::is_cjk(ch) {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            toks.push(ch.to_string());
        } else if ch.is_alphanumeric() {
            cur.extend(ch.to_lowercase());
        } else if !cur.is_empty() {
            toks.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    let mut out = HashSet::new();
    if toks.len() < 4 {
        if !toks.is_empty() {
            out.insert(sig("s", &toks.join(" ")));
        }
        return out;
    }
    for w in toks.windows(4) {
        out.insert(sig("s", &w.join(" ")));
    }
    out
}

/// 旧正文有多少出现在新页面里（0–1）。
pub fn containment(old: &str, new: &str) -> f32 {
    let a = shingles(old);
    if a.is_empty() {
        return 1.0;
    }
    let b = shingles(new);
    a.iter().filter(|s| b.contains(s)).count() as f32 / a.len() as f32
}

/// 参考正文的文件名（data/migrate/ref/ 下）。
pub fn ref_name(path: &str) -> String {
    format!("{}.txt", sm::short_hash(path))
}

fn norm_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 构建产物逐条对照旧站。`refs`：参考正文目录；`assets`：抓取时下载的图片和附件（新站路径）。
pub fn parity(out: &Path, map: &UrlMap, inv: &Inventory, refs: &Path, assets: &[String]) -> Vec<ParityCheck> {
    let pages: HashMap<&str, &Page> = inv.pages.iter().map(|p| (p.path.as_str(), p)).collect();
    let rules = std::fs::read_to_string(out.join("_redirects")).map(|t| parse_redirects(&t)).unwrap_or_default();
    let (mut missing, mut slash, mut files_missing, mut redirects_bad) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut titles, mut descs, mut canon, mut text, mut schema, mut html_ext) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut checked = 0usize;
    for e in &map.entries {
        match e.action.as_str() {
            "keep" if e.kind == "feed" || e.kind == "file" => {
                let (f, _) = out_file_for(out, &e.old);
                if f.is_none() && redirect_match(&rules, &e.old).is_none() {
                    files_missing.push(e.old.clone());
                }
            }
            "keep" => {
                let (f, slash_bad) = out_file_for(out, &e.old);
                let Some(f) = f else {
                    if slash_bad {
                        slash.push(e.old.clone());
                    } else {
                        missing.push(e.old.clone());
                    }
                    continue;
                };
                if matches!(sm::ext_of(&e.old).as_str(), "html" | "htm") {
                    html_ext.push(e.old.clone());
                }
                let Some(old) = pages.get(e.old.as_str()) else { continue };
                let Ok(html) = std::fs::read_to_string(&f) else { continue };
                checked += 1;
                let url = Url::parse(&format!("https://{}{}", map.domain, e.old)).unwrap_or_else(|_| Url::parse("https://example.com/").unwrap());
                let ex = sm::extract(&html, &url, &map.domain);
                if norm_ws(&ex.title) != norm_ws(&old.title) && !old.title.is_empty() {
                    titles.push(format!("{}：旧「{}」→ 新「{}」", e.old, old.title, ex.title));
                }
                if norm_ws(&ex.description) != norm_ws(&old.description) && !old.description.is_empty() {
                    descs.push(e.old.clone());
                }
                if !ex.canonical.is_empty() {
                    let want = format!("https://{}{}", map.domain, e.old);
                    let alt = format!("https://www.{}{}", map.domain.trim_start_matches("www."), e.old);
                    let apex = format!("https://{}{}", map.domain.trim_start_matches("www."), e.old);
                    if ex.canonical != want && ex.canonical != alt && ex.canonical != apex {
                        canon.push(format!("{} → {}", e.old, ex.canonical));
                    }
                }
                let lost: Vec<&String> = old.schema.iter().filter(|t| !ex.schema.contains(t)).collect();
                if !lost.is_empty() {
                    schema.push(format!("{}：少了 {}", e.old, lost.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("、")));
                }
                if let Ok(r) = std::fs::read_to_string(refs.join(ref_name(&e.old))) {
                    let c = containment(&r, &ex.text);
                    if c < 0.9 {
                        text.push(format!("{}：旧正文约 {:.0}% 在新页面里", e.old, c * 100.0));
                    }
                }
            }
            "redirect" => match redirect_match(&rules, &e.old) {
                None => redirects_bad.push(format!("{} → {}（_redirects 里没有）", e.old, e.new)),
                Some((_, to, code)) if !matches!(code, 301 | 308) => redirects_bad.push(format!("{} → {}（状态码是 {code}，应该是 301）", e.old, to)),
                _ => {}
            },
            _ => {}
        }
    }
    let assets_missing: Vec<String> = assets.iter().filter(|a| !out.join(pct_decode(a.trim_start_matches('/'))).is_file()).cloned().collect();
    let keep_pages: BTreeSet<&str> = map.entries.iter().filter(|e| e.action == "keep" && !matches!(e.kind.as_str(), "feed" | "file")).map(|e| e.old.as_str()).collect();
    let in_sitemap = sitemap_paths(out);
    let not_in_sitemap: Vec<String> = if in_sitemap.is_empty() {
        vec!["构建产物里没有 sitemap".to_string()]
    } else {
        keep_pages.iter().filter(|p| !in_sitemap.contains(**p) && !pages.get(**p).is_some_and(|x| x.noindex)).map(|p| p.to_string()).collect()
    };
    let redirect_count = rules.iter().filter(|(f, _, _)| !f.contains('*') && !f.contains(':')).count();

    let mut v = vec![
        check("missing", "页面都在", missing, true, &format!("{} 个保留的页面在新站上都有", keep_pages.len()), |n| format!("{n} 个旧地址在新站上没有对应页面")),
        check("slash", "结尾斜杠一致", slash, true, "路径结尾的 / 和旧站一致", |n| format!("{n} 个地址结尾的 / 和旧站不一致，Cloudflare 会多跳一次，要改成和旧站一样")),
        check("files", "RSS 和下载文件", files_missing, true, "RSS 和附件地址都在", |n| format!("{n} 个 RSS / 附件地址在新站上没有")),
        check("redirects", "跳转规则", redirects_bad, true, "旧站已有的跳转都保留了", |n| format!("{n} 条跳转没有写进 _redirects 或状态码不对")),
        check("titles", "标题不变", titles, false, "页面标题和旧站一致", |n| format!("{n} 个页面的标题和旧站不一样")),
        check("descriptions", "描述不变", descs, false, "页面描述和旧站一致", |n| format!("{n} 个页面的描述和旧站不一样")),
        check("canonical", "canonical 指向正式域名", canon, false, "canonical 都指向正式域名上的原地址", |n| format!("{n} 个页面的 canonical 不是正式域名上的原地址")),
        check("text", "正文没被改写", text, false, "抽查的正文都和旧站一致", |n| format!("{n} 个页面的正文和旧站差别较大（迁移时不改写正文）")),
        check("schema", "结构化数据只增不减", schema, false, "旧站的结构化数据都保留了", |n| format!("{n} 个页面少了旧站有的结构化数据")),
        check("assets", "图片和附件", assets_missing, false, "抓下来的图片和附件都在构建产物里", |n| format!("{n} 个图片 / 附件不在构建产物里")),
        check("sitemap", "sitemap 覆盖全部页面", not_in_sitemap, false, "保留的页面都在 sitemap 里", |n| format!("{n} 个保留的页面不在 sitemap 里")),
        check("html_ext", ".html 地址", html_ext, false, "", |n| format!("{n} 个地址以 .html 结尾：Cloudflare Pages 会把它们 308 跳到不带 .html 的地址（搜索引擎能跟上，但不是原样）")),
    ];
    if redirect_count > sm::CF_MAX_REDIRECTS {
        v.push(ParityCheck {
            key: "redirect_limit".into(),
            label: "跳转规则数量".into(),
            level: "block".into(),
            detail: format!("_redirects 里有 {redirect_count} 条静态规则，超过 Cloudflare Pages 的 {} 条上限，要合并成带 * 的规则", sm::CF_MAX_REDIRECTS),
            items: Vec::new(),
        });
    }
    v.retain(|c| !(c.key == "html_ext" && c.level == "ok"));
    if checked == 0 && !keep_pages.is_empty() {
        v.insert(0, ParityCheck { key: "built".into(), label: "构建产物".into(), level: "block".into(), detail: "还没有构建产物，先构建一次".into(), items: Vec::new() });
    }
    v
}

/// 构建产物里 sitemap 列出的路径（支持 sitemap 索引）。
fn sitemap_paths(out: &Path) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut queue: Vec<PathBuf> = ["sitemap-index.xml", "sitemap_index.xml", "sitemap.xml"].iter().map(|f| out.join(f)).filter(|f| f.is_file()).collect();
    let mut done = HashSet::new();
    while let Some(f) = queue.pop() {
        if !done.insert(f.clone()) || done.len() > 50 {
            continue;
        }
        let Ok(xml) = std::fs::read_to_string(&f) else { continue };
        let (urls, subs) = sm::parse_sitemap(&xml);
        for u in urls {
            if let Ok(u) = Url::parse(&u) {
                seen.insert(sm::path_of(&u));
            }
        }
        for s in subs {
            if let Ok(u) = Url::parse(&s) {
                let p = out.join(u.path().trim_start_matches('/'));
                if p.is_file() {
                    queue.push(p);
                }
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r##"<html><head><title>T</title></head><body>
<header><nav><a href="/">Home</a><a href="/blog/">Blog</a></nav></header>
<div class="wrap"><article><h1>Post title</h1>
<p>First paragraph with a <a href="https://example.com/blog/other/#x">link</a> and <a href="https://ext.com/">ext</a>.</p>
<p><img data-src="/img/a.png" src="data:image/gif;base64,AA" alt="A"></p>
<div class="share-buttons"><a href="#">Share</a></div>
<table><tr><td colspan="2">merged</td></tr></table>
<iframe src="https://www.youtube.com/embed/xyz"></iframe>
<p>Second paragraph 第二段文字。</p></article>
<aside>Sidebar stuff</aside></div>
<footer>© Example</footer></body></html>"##;

    #[test]
    fn extracts_markdown() {
        let u = Url::parse("https://example.com/blog/post/").unwrap();
        let c = extract_content(PAGE, &u, "example.com", &HashSet::new());
        assert!(!c.markdown.contains("Post title"), "{}", c.markdown);
        assert!(c.markdown.contains("[link](/blog/other/#x)"), "{}", c.markdown);
        assert!(c.markdown.contains("](https://ext.com/)"));
        assert!(c.markdown.contains("![A](/img/a.png)"), "{}", c.markdown);
        assert!(!c.markdown.contains("Share"));
        assert!(!c.markdown.contains("Sidebar"));
        assert!(c.markdown.contains("<td colspan=\"2\">merged</td>"), "{}", c.markdown);
        assert!(c.markdown.contains("youtube.com/embed/xyz"));
        assert!(c.markdown.contains("第二段文字"));
        assert_eq!(c.assets.get("https://example.com/img/a.png").map(|s| s.as_str()), Some("/img/a.png"));
        assert!(c.coverage > 0.6, "{}", c.coverage);
    }

    #[test]
    fn includes_sibling_sections() {
        let html = r#"<body><header>Site</header><div class="layout"><div class="main-col">
<div class="prose"><p>Main body text one with enough words to count here and there.</p><p>Second body paragraph also long enough to matter a lot.</p></div>
<section class="faq"><h2>常见问题</h2><p>问：这个问题的答案是什么？答：答案在这里，写得足够长一些方便统计字数。</p></section>
<section class="related"><a href="/x">Related post</a></section>
</div><aside>side</aside></div><footer>f</footer></body>"#;
        let u = Url::parse("https://example.com/a/").unwrap();
        let c = extract_content(html, &u, "example.com", &HashSet::new());
        assert!(c.markdown.contains("常见问题"), "{}", c.markdown);
        assert!(!c.markdown.contains("Related post"));
        assert!(!c.markdown.contains("side"));
    }

    #[test]
    fn decodes_cf_email() {
        // "a@b.co" 用密钥 0x42 加密
        let hex: String = std::iter::once(0x42u8).chain("a@b.co".bytes().map(|c| c ^ 0x42)).map(|b| format!("{b:02x}")).collect();
        assert_eq!(cf_email(&hex).as_deref(), Some("a@b.co"));
        let html = format!(r#"<body><article><p>写信到 <a href="/cdn-cgi/l/email-protection#{hex}"><span class="__cf_email__" data-cfemail="{hex}">[email&#160;protected]</span></a>。这里还有一些正文让它像一篇文章。</p></article></body>"#);
        let u = Url::parse("https://example.com/a").unwrap();
        let c = extract_content(&html, &u, "example.com", &HashSet::new());
        assert!(c.markdown.contains("[a@b.co](mailto:a@b.co)"), "{}", c.markdown);
    }

    #[test]
    fn finds_boilerplate() {
        let a = block_sigs("<body><nav>Menu A B</nav><p>Unique one</p></body>");
        let b = block_sigs("<body><nav>Menu A B</nav><p>Unique two</p></body>");
        let c = block_sigs("<body><nav>Menu A B</nav><p>Unique three</p></body>");
        let set = boilerplate(&[("/blog/*".into(), a), ("/blog/*".into(), b), ("/*".into(), c)]);
        assert!(set.contains(&sig("nav", "Menu A B")));
        assert!(!set.contains(&sig("p", "Unique one")));
    }

    #[test]
    fn content_paths() {
        assert_eq!(content_path("/blog/hello-world/"), ("blog".into(), "hello-world".into()));
        assert_eq!(content_path("/blog/2020/01/x.html"), ("blog".into(), "2020/01/x".into()));
        assert_eq!(content_path("/about/"), ("pages".into(), "about".into()));
        assert_eq!(content_path("/"), ("pages".into(), "index".into()));
        assert_eq!(content_path("/2020/x/"), ("pages".into(), "2020/x".into()));
        assert_eq!(content_path("/%E4%BD%A0%E5%A5%BD/a/"), ("pages".into(), "你好/a".into()));
    }

    #[test]
    fn assets_and_decode() {
        let u = Url::parse("https://cdn.other.com/i/a.jpg?w=300").unwrap();
        assert!(local_asset(&u, "example.com").starts_with("/assets/ext/cdn.other.com/i/a-"));
        let u = Url::parse("https://www.example.com/i/a.jpg").unwrap();
        assert_eq!(local_asset(&u, "example.com"), "/i/a.jpg");
        assert_eq!(pct_decode("/a%20b/%E4%BD%A0"), "/a b/你");
        assert_eq!(pct_decode("100%"), "100%");
    }

    #[test]
    fn similarity() {
        assert!(containment("the quick brown fox jumps over the lazy dog", "Intro. The quick brown fox jumps over the lazy dog! end") > 0.99);
        assert!(containment("the quick brown fox jumps over the lazy dog", "a slow red fox walks under the busy cat") < 0.2);
        assert!(containment("这是一段测试用的中文正文", "前言：这是一段测试用的中文正文。") > 0.99);
    }

    #[test]
    fn parity_catches_problems() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("out");
        let refs = d.path().join("ref");
        std::fs::create_dir_all(out.join("a")).unwrap();
        std::fs::create_dir_all(&refs).unwrap();
        // /a/ 在但标题改了、正文换了；/b/ 不在；/c 生成成了 c/index.html（斜杠不对）；/old 应该跳转但没写规则
        std::fs::write(out.join("a/index.html"), "<html><head><title>New title</title></head><body><p>totally different words that were never on the old page at all</p></body></html>").unwrap();
        std::fs::create_dir_all(out.join("c")).unwrap();
        std::fs::write(out.join("c/index.html"), "<title>C</title>").unwrap();
        std::fs::write(refs.join(ref_name("/a/")), "the original article body text that must stay exactly the same after migration").unwrap();
        let page = |p: &str, t: &str| Page { path: p.into(), kind: "page".into(), title: t.into(), ptype: "article".into(), ..Default::default() };
        let inv = Inventory { domain: "example.com".into(), pages: vec![page("/a/", "Old title"), page("/b/", "B"), page("/c", "C")], ..Default::default() };
        let e = |old: &str, action: &str, new: &str| UrlEntry { old: old.into(), new: new.into(), action: action.into(), kind: "article".into(), ..Default::default() };
        let map = UrlMap { domain: "example.com".into(), trailing_slash: "mixed".into(), entries: vec![e("/a/", "keep", "/a/"), e("/b/", "keep", "/b/"), e("/c", "keep", "/c"), e("/old", "redirect", "/a/")] };
        let r = parity(&out, &map, &inv, &refs, &[]);
        let lv = |k: &str| r.iter().find(|c| c.key == k).map(|c| c.level.clone()).unwrap_or_default();
        assert_eq!(lv("missing"), "block");
        assert_eq!(lv("slash"), "block");
        assert_eq!(lv("redirects"), "block");
        assert_eq!(lv("titles"), "warn");
        assert_eq!(lv("text"), "warn");
        assert_eq!(lv("sitemap"), "warn");
    }

    #[test]
    fn pages_file_rules() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("a")).unwrap();
        std::fs::write(d.path().join("a/index.html"), "x").unwrap();
        std::fs::write(d.path().join("b.html"), "x").unwrap();
        std::fs::write(d.path().join("index.html"), "x").unwrap();
        assert!(out_file_for(d.path(), "/").0.is_some());
        assert!(out_file_for(d.path(), "/a/").0.is_some());
        assert_eq!(out_file_for(d.path(), "/a"), (None, true));
        assert!(out_file_for(d.path(), "/b").0.is_some());
        assert_eq!(out_file_for(d.path(), "/b/"), (None, true));
        assert_eq!(out_file_for(d.path(), "/c/"), (None, false));
        let r = parse_redirects("/old /new 301\n/blog/* /posts/:splat 301\n# c\n");
        assert_eq!(redirect_match(&r, "/old").map(|x| x.1.as_str()), Some("/new"));
        assert!(redirect_match(&r, "/blog/x").is_some());
    }
}
