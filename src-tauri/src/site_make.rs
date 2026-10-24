//! 站点制作：从一段定位描述起一个新站点。
//!
//! 技术栈不固定，但每个新站都带一份「站点约定」`.pilot/site.md`（内容放哪、页面格式、
//! 新增一页要改哪些地方、构建和预览命令），以后加内容、改页面都按它来——固定的是约定，不是框架。
//! 这里只放纯函数：技术栈的默认构建配置、文件夹命名、写给 AI 的第一版需求。命令在 site_cmds::create_site。

use serde::Deserialize;

use crate::sites::BuildConfig;

/// 可选的技术栈：(id, 名称, 默认构建配置 install/build/out)。硬约束只有两条：
/// 构建产物是静态文件（或跑在 Workers 上），内容页在构建时就生成完整 HTML。
pub const STACKS: &[(&str, &str, &str, &str, &str)] = &[
    ("astro", "Astro", "npm install", "npm run build", "dist"),
    ("hugo", "Hugo", "", "hugo --minify", "public"),
    ("vitepress", "VitePress", "npm install", "npm run build", ".vitepress/dist"),
    ("html", "纯 HTML", "", "", "."),
    ("next", "Next.js 静态导出", "npm install", "npm run build", "out"),
];

pub fn stack_build(stack: &str) -> Option<BuildConfig> {
    STACKS.iter().find(|s| s.0 == stack).map(|s| BuildConfig {
        install: s.2.into(),
        build: s.3.into(),
        out_dir: s.4.into(),
        ..Default::default()
    })
}

fn stack_guide(stack: &str) -> &'static str {
    match stack {
        "astro" => "用 Astro，静态输出（不加 SSR 适配器）。可以手写最小配置（package.json、astro.config.mjs），也可以用官方脚手架的最小模板起步，但别装用不到的集成。\
内容页用 Markdown / MDX 放在 `src/content/`，用 content collections 管理并校验开头元数据；package.json 的 build 脚本是 `astro build`，产物目录 `dist`。",
        "hugo" => "用 Hugo。不要依赖外部主题仓库（不用 git submodule），布局直接写在 `layouts/`，样式放 `assets/` 或 `static/`；内容放 `content/`。\
构建 `hugo --minify`，产物目录 `public`。本机需要装好 hugo，没有的话先告诉我怎么装，不要换别的框架。",
        "vitepress" => "用 VitePress，文档就放在仓库根目录（配置在 `.vitepress/config.mts`）；package.json 的 build 脚本是 `vitepress build .`，产物目录 `.vitepress/dist`。\
导航和侧边栏按内容规划配好。",
        "html" => "纯 HTML + CSS，不需要构建：每页一个 `.html` 文件，公共样式放 `assets/`；导航、页脚在每一页保持一致。\
`sitemap.xml` 手写，新增页面时同步更新。产物目录就是仓库根目录。",
        "next" => "用 Next.js 静态导出（next.config 里 `output: 'export'`，App Router）。内容用 Markdown 放在 `content/`，构建时读取生成页面；\
package.json 的 build 脚本是 `next build`，产物目录 `out`。不要用需要服务器的功能（API Routes、ISR、middleware、图片优化服务）。",
        _ => "",
    }
}

pub const KINDS: &[(&str, &str)] = &[
    ("content", "内容 / SEO 站"),
    ("docs", "文档 / 知识库"),
    ("landing", "落地页 / 产品介绍"),
    ("tool", "工具 / 导航站"),
];

fn kind_label(kind: &str) -> &'static str {
    KINDS.iter().find(|k| k.0 == kind).map(|k| k.1).unwrap_or("内容 / SEO 站")
}

/// 新站点的文件夹名：有域名就用域名（和已有站点的习惯一致，如 `cexask.com`），否则用站点名的 slug。
pub fn folder_name(domain: &str, name: &str) -> String {
    let d: String = domain
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        .collect();
    let d = d.trim_matches(|c| c == '.' || c == '-').to_string();
    if !d.is_empty() {
        return d;
    }
    // slugify 对没有字母数字的名字会给 "change"（分支名用的兜底），这里自己判断
    if !name.chars().any(|c| c.is_ascii_alphanumeric()) {
        return "new-site".into();
    }
    crate::site_git::slugify(name, 40)
}

#[derive(Deserialize, Clone, Default)]
pub struct CreateSiteRequest {
    pub name: String,
    #[serde(default)]
    pub domain: String,
    /// 一句话定位：写什么、给谁看、解决什么问题。
    pub about: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub audience: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub goals: Vec<String>,
    pub stack: String,
    /// 视觉风格的描述（前端按所选风格给出），空＝让 AI 定。
    #[serde(default)]
    pub style: String,
    /// 首批核心页面数（不含首页、关于、联系、隐私）。
    #[serde(default)]
    pub pages: u32,
    /// 新站点文件夹放在哪个目录下。
    pub parent_dir: String,
}

pub fn pages_of(req: &CreateSiteRequest) -> u32 {
    if req.pages == 0 { 8 } else { req.pages.clamp(3, 20) }
}

/// 存成「运营确认过的定位」：托管运营做增量选题时直接用它。
pub fn positioning_text(req: &CreateSiteRequest) -> String {
    let mut out = req.about.trim().to_string();
    let reader = [req.audience.trim(), req.language.trim()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("；语言：");
    if !reader.is_empty() {
        out.push_str(&format!("\n读者：{reader}"));
    }
    if !req.goals.is_empty() {
        out.push_str(&format!("\n商业目标：{}", req.goals.join("、")));
    }
    out.push_str(&format!("\n类型：{}", kind_label(&req.kind)));
    out
}

/// 写进新仓库的需求记录（`.pilot/brief.md`），也是第一版对话的依据。
pub fn brief_md(req: &CreateSiteRequest) -> String {
    let stack = STACKS.iter().find(|s| s.0 == req.stack).map(|s| s.1).unwrap_or(&req.stack);
    format!(
        "# {name}\n\n{domain}## 定位\n\n{pos}\n\n## 技术栈\n\n{stack}\n\n## 第一版\n\n首页 + {n} 个核心页面 + 关于 / 联系 / 隐私政策。\n",
        name = req.name.trim(),
        domain = if req.domain.trim().is_empty() { String::new() } else { format!("域名：{}\n\n", req.domain.trim()) },
        pos = positioning_text(req),
        n = pages_of(req),
    )
}

/// 第一版对话的第一条消息：照着它，AI 从空仓库做出能预览的第一版。
pub fn first_message(req: &CreateSiteRequest) -> String {
    let stack = STACKS.iter().find(|s| s.0 == req.stack).map(|s| s.1).unwrap_or(&req.stack);
    let mut head = format!("请从零搭建这个新站点，做出第一版。\n\n【站点】{}", req.name.trim());
    if !req.domain.trim().is_empty() {
        head.push_str(&format!("（{}）", req.domain.trim()));
    }
    head.push_str(&format!("\n【定位】{}", positioning_text(req)));
    let style = if req.style.trim().is_empty() { "按定位和读者自己定，简洁、易读、移动端优先。".to_string() } else { req.style.trim().to_string() };
    format!(
        "{head}\n\n【技术栈】{stack}。{guide}\n当前目录是新建的空仓库（已 git init，`.pilot/brief.md` 记着上面这些需求）。\n\n\
【第一版要做的】\n\
1. 先规划：按定位列出站点结构——首页、{n} 个核心页面（支柱页 + 回应具体搜索意图的长尾页）、关于、联系、隐私政策。每页一句话写清它回应什么搜索意图、目标查询词是什么，写进 `.pilot/site.md` 的「内容规划」一节，再动手。\n\
2. 搭骨架：统一的页头导航、页脚（带关于 / 联系 / 隐私的链接）、内容页的面包屑。\n\
3. 写首批页面：每个核心页面都是完整的内容页，实打实回答读者的问题——正文不少于 600 字，有小标题、要点，需要时用表格或步骤；页面之间合理互链。不要 lorem ipsum、「敬请期待」这类占位内容；需要真实信息（联系方式、价格、数据）而你不知道的，留出明显的待补位置并在汇报里列出来。\n\
4. SEO 基础：每页独立的 title 和 description、canonical、Open Graph；sitemap.xml、robots.txt；内容页加 Article 结构化数据，首页加 WebSite / Organization；favicon；图片都有 alt；语义化 HTML。内容页必须在构建时生成完整 HTML，不能只靠浏览器端渲染。\n\
5. 视觉：{style}\n\
6. 写站点约定 `.pilot/site.md`：技术栈；内容放在哪个目录、文件怎么命名、开头元数据有哪些字段及含义；新增一页要改哪些地方（列表页、导航、sitemap 是否自动生成）；图片放哪；安装、构建、本地预览的命令和产物目录。写到另一个人照着就能加一页的程度——以后加内容、改页面都按这份约定来。\n\
7. 自检：安装依赖、跑构建确认通过；本地预览，截图首页和一个内容页检查排版，手机宽度也看一眼。\n\
8. 完成后 `git add -A && git commit -m \"搭建第一版\"`，然后简要汇报：做了哪些页面、怎么本地预览、还有哪些需要我确认或补充的地方。\n\n\
不要部署——上线由 Pilot 负责。",
        guide = stack_guide(&req.stack),
        n = pages_of(req),
    )
}

// ───────────────────────── 第一版进度 + 上线前检查 ─────────────────────────
//
// 进度看文件，不看对话：AI 说做完了不算，`.pilot/site.md` 写了、页面有了、构建产物在、提交了才算。
// 上线前检查扫构建产物里的 HTML：缺页、占位、断链、标题描述、薄内容、sitemap / robots。

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone)]
pub struct MakeStep {
    pub key: &'static str,
    pub label: &'static str,
    pub done: bool,
    pub detail: String,
}

#[derive(Serialize, Clone)]
pub struct Check {
    pub key: &'static str,
    pub label: &'static str,
    /// ok | warn | fail（fail 的问题不修不该上线）
    pub level: &'static str,
    pub detail: String,
    pub items: Vec<String>,
}

const SKIP_DIRS: &[&str] = &["node_modules", ".git", ".pilot", ".astro", ".next", ".wrangler", ".cache", "resources", "shots"];

fn walk(dir: &Path, skip: &[PathBuf], out: &mut Vec<PathBuf>, keep: &dyn Fn(&Path) -> bool) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) || skip.iter().any(|s| s == &p) || out.len() > 3000 {
                continue;
            }
            walk(&p, skip, out, keep);
        } else if keep(&p) {
            out.push(p);
        }
    }
}

fn has_ext(p: &Path, exts: &[&str]) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| exts.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// 站点约定有没有写（至少几百字，不是空壳）。
pub fn has_conventions(code: &Path) -> bool {
    std::fs::read_to_string(code.join(".pilot/site.md")).map(|t| t.trim().chars().count() >= 200).unwrap_or(false)
}

/// 构建产物目录（纯静态站就是站点目录本身）。
pub fn out_dir(app: &Path, out: &str) -> PathBuf {
    if out.is_empty() || out == "." { app.to_path_buf() } else { app.join(out) }
}

/// 第一版的进度：规划 → 页面 → 约定 → 构建 → 提交。`commits` 是 HEAD 上的提交数。
pub fn progress(app: &Path, code: &Path, out: &str, commits: usize) -> Vec<MakeStep> {
    let conv = std::fs::read_to_string(code.join(".pilot/site.md")).unwrap_or_default();
    let planned = conv.contains("内容规划");
    let built_dir = out_dir(app, out);
    let skip = if out.is_empty() || out == "." { vec![] } else { vec![built_dir.clone()] };
    let mut src = Vec::new();
    walk(app, &skip, &mut src, &|p| has_ext(p, &["md", "mdx", "markdown", "html", "htm", "astro", "vue", "tsx", "jsx", "njk"]) && !p.ends_with("README.md"));
    // 只数像页面的源文件：内容目录里的 Markdown、页面目录里的组件、纯静态站的 html
    let pages = src
        .iter()
        .filter(|p| {
            let s = p.to_string_lossy();
            has_ext(p, &["md", "mdx", "markdown", "html", "htm"]) || s.contains("/pages/") || s.contains("/app/") || s.contains("/content/")
        })
        .filter(|p| !p.starts_with(code.join(".pilot")))
        .count();
    let index = built_dir.join("index.html").is_file();
    let mut html = Vec::new();
    if index {
        walk(&built_dir, &[], &mut html, &|p| has_ext(p, &["html"]));
    }
    vec![
        MakeStep { key: "plan", label: "规划站点结构", done: planned, detail: if planned { "写在 .pilot/site.md 的「内容规划」里".into() } else { String::new() } },
        MakeStep { key: "pages", label: "搭骨架、写首批页面", done: pages >= 4, detail: if pages > 0 { format!("已有 {pages} 个页面文件") } else { String::new() } },
        MakeStep { key: "conventions", label: "写站点约定", done: has_conventions(code), detail: if has_conventions(code) { ".pilot/site.md".into() } else { String::new() } },
        MakeStep { key: "build", label: "构建通过", done: index, detail: if index { format!("产物里有 {} 个页面", html.len()) } else { String::new() } },
        MakeStep { key: "commit", label: "提交第一版", done: commits > 1, detail: if commits > 1 { format!("{commits} 次提交") } else { String::new() } },
    ]
}

/// 去掉整段的标签块（`<script>…</script>` 之类），大小写不敏感（ASCII 小写不改字节位置）。
fn drop_blocks(s: &str, tags: &[&str]) -> String {
    let mut out = s.to_string();
    for t in tags {
        loop {
            let low = out.to_ascii_lowercase();
            let Some(a) = low.find(&format!("<{t}")) else { break };
            let close = format!("</{t}>");
            let b = low[a..].find(&close).map(|i| a + i + close.len()).unwrap_or(out.len());
            out.replace_range(a..b, " ");
        }
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").replace("&amp;", "&")
}

/// 第一个 `<tag …>…</tag>` 的内部。
fn inner<'a>(html: &'a str, low: &str, tag: &str) -> Option<&'a str> {
    let a = low.find(&format!("<{tag}"))?;
    let start = a + low[a..].find('>')? + 1;
    let end = low[start..].find(&format!("</{tag}>")).map(|i| start + i).unwrap_or(html.len());
    html.get(start..end)
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let low = tag.to_ascii_lowercase();
    for q in ['"', '\''] {
        let key = format!("{name}={q}");
        if let Some(i) = low.find(&key) {
            let v = &tag[i + key.len()..];
            return v.find(q).map(|j| v[..j].to_string());
        }
    }
    None
}

/// 所有 `<tagname …>` 开始标签的原文。
fn tags<'a>(html: &'a str, low: &str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let pat = format!("<{name}");
    let mut from = 0;
    while let Some(i) = low[from..].find(&pat) {
        let a = from + i;
        let after = low.as_bytes().get(a + pat.len()).copied().unwrap_or(b' ');
        let Some(j) = low[a..].find('>') else { break };
        if after.is_ascii_whitespace() || after == b'>' || after == b'/' {
            out.push(&html[a..a + j + 1]);
        }
        from = a + j + 1;
    }
    out
}

/// 站内链接指向的文件存在吗（`/a/`、`/a`、`/a.html`、相对路径都认）。
fn link_ok(root: &Path, page: &Path, href: &str) -> bool {
    let h = href.split(['#', '?']).next().unwrap_or("").trim();
    if h.is_empty() {
        return true;
    }
    let base = if h.starts_with('/') { root.to_path_buf() } else { page.parent().unwrap_or(root).to_path_buf() };
    let mut p = base;
    for seg in h.trim_start_matches('/').split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if p != root {
                    p.pop();
                }
            }
            s => p.push(s),
        }
    }
    p.is_file() || p.join("index.html").is_file() || p.with_extension("html").is_file() || (h.ends_with('/') && p.join("index.html").is_file())
}

fn is_external(h: &str) -> bool {
    let l = h.trim().to_ascii_lowercase();
    l.is_empty() || l.starts_with('#') || l.contains("://") || l.starts_with("//") || ["mailto:", "tel:", "javascript:", "data:", "sms:"].iter().any(|p| l.starts_with(p))
}

const REQUIRED: &[(&str, &[&str])] = &[("关于", &["about", "关于"]), ("联系", &["contact", "联系"]), ("隐私政策", &["privacy", "隐私"])];
const LISTING: &[&str] = &["tag", "categor", "page/", "archive", "search", "blog/index", "posts/index"];
const EXTRA_PLACEHOLDERS: &[&str] = &["敬请期待", "coming soon", "待补", "your-email", "example@example.com", "xxx-xxxx"];
/// 内容页的文字量下限（和托管运营判断「新页面太薄」用同一把尺）。
pub const THIN_UNITS: usize = 600;

/// 扫构建产物，给出上线前检查的结果。
pub fn readiness(out: &Path) -> Vec<Check> {
    if !out.join("index.html").is_file() {
        return vec![Check { key: "build", label: "构建产物", level: "fail", detail: "还没有构建产物（找不到 index.html）：先构建一次".into(), items: vec![] }];
    }
    let mut files = Vec::new();
    walk(out, &[], &mut files, &|p| has_ext(p, &["html"]));
    files.sort();
    let url = |p: &Path| -> String {
        let r = p.strip_prefix(out).unwrap_or(p).to_string_lossy().replace('\\', "/");
        let r = r.strip_suffix("index.html").map(|x| x.to_string()).unwrap_or(r);
        format!("/{r}")
    };
    let (mut no_title, mut no_desc, mut no_h1, mut no_lang, mut thin, mut ph, mut broken) = (vec![], vec![], vec![], 0usize, vec![], vec![], vec![]);
    let mut titles: std::collections::HashMap<String, Vec<String>> = Default::default();
    let mut found = [false; 3];
    let mut favicon = out.join("favicon.ico").is_file() || out.join("favicon.svg").is_file();
    let mut pages = 0;
    for f in &files {
        let Ok(html) = std::fs::read_to_string(f) else { continue };
        let u = url(f);
        let ul = u.to_ascii_lowercase();
        let low = html.to_ascii_lowercase();
        // 跳转页（meta refresh）和 404 不当正经页面查
        if low.contains("http-equiv=\"refresh\"") || ul.contains("404") {
            continue;
        }
        pages += 1;
        let title = inner(&html, &low, "title").map(|t| strip_tags(t).trim().to_string()).unwrap_or_default();
        if title.is_empty() {
            no_title.push(u.clone());
        } else {
            titles.entry(title.clone()).or_default().push(u.clone());
        }
        let metas = tags(&html, &low, "meta");
        if !metas.iter().any(|m| attr(m, "name").map(|n| n.eq_ignore_ascii_case("description")).unwrap_or(false) && attr(m, "content").map(|c| !c.trim().is_empty()).unwrap_or(false)) {
            no_desc.push(u.clone());
        }
        if !low.contains("<h1") {
            no_h1.push(u.clone());
        }
        if tags(&html, &low, "html").first().map(|t| attr(t, "lang").is_none()).unwrap_or(true) {
            no_lang += 1;
        }
        if tags(&html, &low, "link").iter().any(|l| attr(l, "rel").map(|r| r.to_ascii_lowercase().contains("icon")).unwrap_or(false)) {
            favicon = true;
        }
        for (i, (_, keys)) in REQUIRED.iter().enumerate() {
            if keys.iter().any(|k| ul.contains(k) || title.to_lowercase().contains(k)) {
                found[i] = true;
            }
        }
        // 正文：有 <main> / <article> 用它，否则 body 去掉页头页脚导航
        let body = inner(&html, &low, "main").or_else(|| inner(&html, &low, "article")).or_else(|| inner(&html, &low, "body")).unwrap_or(&html);
        let text = strip_tags(&drop_blocks(body, &["script", "style", "nav", "header", "footer", "aside", "noscript", "svg", "template"]));
        if let Some(m) = crate::managed::leak_marker(&text).or_else(|| {
            let l = text.to_lowercase();
            EXTRA_PLACEHOLDERS.iter().copied().find(|m| l.contains(m))
        }) {
            ph.push(format!("{u}（{m}）"));
        }
        let special = u == "/" || REQUIRED.iter().any(|(_, ks)| ks.iter().any(|k| ul.contains(k))) || LISTING.iter().any(|k| ul.contains(k));
        let units = crate::managed::text_units(&text);
        if !special && units < THIN_UNITS {
            thin.push(format!("{u}（约 {units} 字）"));
        }
        for a in tags(&html, &low, "a") {
            if let Some(h) = attr(a, "href") {
                if !is_external(&h) && !link_ok(out, f, &h) && broken.len() < 30 {
                    broken.push(format!("{u} → {h}"));
                }
            }
        }
    }
    let dup: Vec<String> = titles.into_iter().filter(|(_, v)| v.len() > 1).map(|(t, v)| format!("「{t}」：{}", v.join("、"))).collect();
    let missing: Vec<&str> = REQUIRED.iter().zip(found).filter(|(_, f)| !f).map(|((n, _), _)| *n).collect();
    let mut seo_files = vec![];
    if !out.join("sitemap.xml").is_file() && !out.join("sitemap-index.xml").is_file() {
        seo_files.push("sitemap.xml");
    }
    if !out.join("robots.txt").is_file() {
        seo_files.push("robots.txt");
    }
    let lvl = |bad: bool, worst: &'static str| if bad { worst } else { "ok" };
    vec![
        Check { key: "pages", label: "页面数量", level: lvl(pages < 4, "warn"), detail: format!("共 {pages} 个页面"), items: vec![] },
        Check { key: "required", label: "关于 / 联系 / 隐私政策", level: lvl(!missing.is_empty(), "warn"), detail: if missing.is_empty() { "都有".into() } else { format!("缺：{}", missing.join("、")) }, items: vec![] },
        Check { key: "placeholder", label: "占位和待补内容", level: lvl(!ph.is_empty(), "fail"), detail: if ph.is_empty() { "没有发现".into() } else { format!("{} 个页面还有占位或待补的内容", ph.len()) }, items: ph },
        Check { key: "links", label: "站内链接", level: lvl(!broken.is_empty(), "fail"), detail: if broken.is_empty() { "没有断链".into() } else { format!("{} 个链接指向不存在的页面", broken.len()) }, items: broken },
        Check { key: "title", label: "页面标题", level: if !no_title.is_empty() { "fail" } else if !dup.is_empty() { "warn" } else { "ok" }, detail: if !no_title.is_empty() { format!("{} 个页面没有标题", no_title.len()) } else if !dup.is_empty() { format!("{} 组页面标题重复", dup.len()) } else { "每页都有、不重复".into() }, items: no_title.into_iter().chain(dup).collect() },
        Check { key: "description", label: "页面描述", level: lvl(!no_desc.is_empty(), "warn"), detail: if no_desc.is_empty() { "每页都有".into() } else { format!("{} 个页面没有 description", no_desc.len()) }, items: no_desc },
        Check { key: "h1", label: "一级标题", level: lvl(!no_h1.is_empty(), "warn"), detail: if no_h1.is_empty() { "每页都有".into() } else { format!("{} 个页面没有 h1", no_h1.len()) }, items: no_h1 },
        Check { key: "thin", label: "内容厚度", level: lvl(!thin.is_empty(), "warn"), detail: if thin.is_empty() { format!("内容页正文都在 {THIN_UNITS} 字以上") } else { format!("{} 个内容页正文不到 {THIN_UNITS} 字，容易被搜索引擎当成薄内容", thin.len()) }, items: thin },
        Check { key: "seo_files", label: "sitemap.xml / robots.txt", level: lvl(!seo_files.is_empty(), "warn"), detail: if seo_files.is_empty() { "都有".into() } else { format!("缺：{}", seo_files.join("、")) }, items: vec![] },
        Check { key: "basics", label: "语言标记和网站图标", level: lvl(no_lang > 0 || !favicon, "warn"), detail: match (no_lang > 0, favicon) { (false, true) => "都有".into(), (true, true) => format!("{no_lang} 个页面的 <html> 没有 lang"), (false, false) => "没有网站图标（favicon）".into(), (true, false) => format!("没有网站图标；{no_lang} 个页面没有 lang") }, items: vec![] },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> CreateSiteRequest {
        CreateSiteRequest { name: "Coffee Notes".into(), about: "手冲咖啡入门指南".into(), stack: "astro".into(), parent_dir: "/tmp".into(), goals: vec!["广告变现".into()], ..Default::default() }
    }

    #[test]
    fn folder_prefers_the_domain_and_falls_back_to_a_slug() {
        assert_eq!(folder_name("Coffee.example.com ", "x"), "coffee.example.com");
        assert_eq!(folder_name("", "Coffee Notes"), "coffee-notes");
        assert_eq!(folder_name("", "咖啡"), "new-site");
    }

    #[test]
    fn every_stack_has_a_build_and_a_guide() {
        for (id, ..) in STACKS {
            assert!(stack_build(id).is_some() && !stack_guide(id).is_empty(), "{id}");
            assert!(crate::sites::check_out_dir(&stack_build(id).unwrap().out_dir).is_ok(), "{id}");
        }
        assert!(stack_build("rails").is_none());
    }

    #[test]
    fn first_message_carries_positioning_conventions_and_no_deploy() {
        let m = first_message(&req());
        assert!(m.contains("手冲咖啡入门指南") && m.contains("商业目标：广告变现"));
        assert!(m.contains(".pilot/site.md") && m.contains("8 个核心页面") && m.contains("不要部署"));
        assert!(m.contains("Astro"));
        assert_eq!(pages_of(&CreateSiteRequest { pages: 50, ..req() }), 20);
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn page(title: &str, body: &str, links: &str) -> String {
        format!("<!doctype html><html lang=\"zh\"><head><title>{title}</title><meta name=\"description\" content=\"d\"><link rel=\"icon\" href=\"/favicon.svg\"></head><body><nav><a href=\"/\">首页</a>{links}</nav><main><h1>{title}</h1><p>{body}</p></main></body></html>")
    }

    #[test]
    fn readiness_flags_missing_build_broken_links_placeholders_and_thin_pages() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(readiness(t.path())[0].level, "fail");
        let long = "手冲咖啡".repeat(200);
        write(t.path(), "index.html", &page("首页", "欢迎", "<a href=\"/guide/\">指南</a><a href=\"/missing/\">x</a><a href=\"https://x.com\">外</a><a href=\"mailto:a@b.c\">信</a>"));
        write(t.path(), "guide/index.html", &page("指南", &long, "<a href=\"../about.html\">关于</a>"));
        write(t.path(), "about.html", &page("关于我们", "我们是谁", ""));
        write(t.path(), "short/index.html", &page("短页", "只有一句话。待补充：价格表", ""));
        let r = readiness(t.path());
        let get = |k: &str| r.iter().find(|c| c.key == k).unwrap().clone();
        assert_eq!(get("links").level, "fail");
        assert!(get("links").items.iter().any(|i| i.contains("/missing/")), "{:?}", get("links").items);
        assert_eq!(get("links").items.len(), 1, "外链、mailto、相对路径都不算断链");
        assert_eq!(get("placeholder").level, "fail");
        assert!(get("thin").items.iter().any(|i| i.starts_with("/short/")) && !get("thin").items.iter().any(|i| i.starts_with("/guide/")));
        assert!(get("required").detail.contains("联系") && !get("required").detail.contains("关于"));
        assert_eq!(get("seo_files").level, "warn");
        assert_eq!(get("basics").level, "ok");
        assert_eq!(get("title").level, "ok");
    }

    #[test]
    fn progress_reads_files_not_promises() {
        let t = tempfile::tempdir().unwrap();
        let steps = progress(t.path(), t.path(), "dist", 1);
        assert!(steps.iter().all(|s| !s.done));
        write(t.path(), ".pilot/site.md", &format!("# 站点约定\n\n## 内容规划\n\n{}", "说明".repeat(120)));
        for i in 0..4 {
            write(t.path(), &format!("src/content/p{i}.md"), "# x");
        }
        write(t.path(), "dist/index.html", "<html></html>");
        let steps = progress(t.path(), t.path(), "dist", 2);
        assert!(steps.iter().all(|s| s.done), "{:?}", steps.iter().map(|s| (s.key, s.done)).collect::<Vec<_>>());
    }
}
