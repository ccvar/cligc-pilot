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
}
