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

pub(crate) fn stack_guide(stack: &str) -> &'static str {
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

/// 空＝让 AI 按定位判断。
fn kind_label(kind: &str) -> Option<&'static str> {
    KINDS.iter().find(|k| k.0 == kind).map(|k| k.1)
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
    if !req.audience.trim().is_empty() {
        out.push_str(&format!("\n读者：{}", req.audience.trim()));
    }
    if !req.language.trim().is_empty() {
        out.push_str(&format!("\n语言：{}", req.language.trim()));
    }
    if !req.goals.is_empty() {
        out.push_str(&format!("\n商业目标：{}", req.goals.join("、")));
    }
    if let Some(k) = kind_label(&req.kind) {
        out.push_str(&format!("\n类型：{k}"));
    }
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

/// 内容标准：公开文案、长尾表达、搜索质量（SEO / GEO、不被判低质）、真实截图。
/// 每个站点会话还会另外附带公开文案规范和截图工具说明（editorial / agent::site_shot_prompt），这里是落到建站上的具体要求。
const CONTENT_STANDARD: &str = "【内容标准】\n\
1. 只写给读者看的话：标题、正文、按钮、图注、alt、meta 描述和 HTML 注释里，不出现内部运营表达（如「承接流量、转化闭环、内容支柱」）、内部术语、工程化表达、内部说明、系统提示或开发注释。规划、待办、素材缺口只写在 `.pilot/site.md` 和给我的汇报里。\n\
2. 长尾表达：标题、小标题和 FAQ 按目标读者真实的搜索问法来写，写清对象、人群、场景、问题或限制条件（例如不写「入门指南」，写「新手手冲咖啡粉水比多少合适」）。不堆关键词、不编造搜索量、不把每个标题都写成问句；按钮和导航保持简短。\n\
3. 经得起搜索引擎的质量评估：\n\
   - 每页只回应一个明确的搜索意图；页面之间不重复，不做只换关键词的批量页（会被 Google 当成规模化低质内容或门页）。\n\
   - 开头两三句先直接回答问题，再展开；用小标题、列表、表格、步骤组织，关键概念用一句话下定义，方便搜索引擎摘要和 AI 回答引用（GEO）。\n\
   - 要有信息增量：具体参数、步骤、对比、例子、常见误区和注意事项，而不是泛泛而谈；能核实的事实写清依据，时效性信息标注更新时间；不编造数据、经历、评价和引用。\n\
   - 内容厚度以把问题讲透为准：支柱页一般 1500 字以上，长尾页不少于 600 字，不注水凑字。\n\
   - 体现可信度（E-E-A-T）：关于页说清楚是谁在做、凭什么可信；内容页有更新日期，需要时注明作者。\n\
   - 不知道的真实信息（联系方式、价格、资质、数据）不要编，也不要在页面上留「待补」之类的字样：先不放这部分，写进 `.pilot/todo.json`（见【待我处理的事】）。\n\
4. 真实截图：讲操作步骤、界面或对比时，能截到的就用随附的截图工具截真实网页（官方站点、公开页面），不用占位图、示意图或 AI 生成的界面图。截完打开检查不是错误页、验证码或 Cookie 弹窗，个人信息打码；图片压缩成适合网页的大小，写清 alt 和图注。需要登录或截不到的，正文先不放图，写进 `.pilot/todo.json`。";

/// 视觉标准：以获奖级网站为品质标准，但服务于内容——靠搜索带流量的站，可读性和速度是底线。
pub(crate) const VISUAL_STANDARD: &str = "【视觉标准】以 Awwwards、Webby Awards、FWA 获奖网站的品质为标准，同时服务于内容：这是靠搜索带流量的站，可读性和加载速度是底线。\n\
- 原创：先按定位定一个设计方向（气质、配色从哪来、字体组合、一个让人记住的视觉点），写进 `.pilot/site.md`；不套通用模板，不做千篇一律的渐变大图加三栏卡片。\n\
- 排版：清楚的字号刻度和行高；正文 16–18px，行宽适中（中文每行约 30–40 字）；标题有张力，长文读起来不累。\n\
- 留白与网格：统一的间距刻度和栅格，区块之间有松有紧的节奏。\n\
- 视觉层级：每一屏有一个焦点，最重要的信息最先被看到，次要信息让位。\n\
- 色彩：克制的主色加一个强调色，文字对比度达到 WCAG AA；要做深色模式就成套做。配色从品牌、行业和读者来，不从「看起来有质感」来。\n\
- 避开 AI 建站的通用套路：不用米黄 / 奶油 / 暖白的「纸张」底色（#f4f1e9、#faf7f0、#f5f0e6 这一类），底色默认用纯白、中性浅灰或深色，要用带颜色的底必须来自品牌、并在 `.pilot/site.md` 写明理由；也不要拿「纸张、票据、印章、撕边、手账」当设计概念，不满屏用等宽大写小字当标签。Pilot 构建后会检查底色。\n\
- 动效与微交互：只做有目的的过渡（进入、悬停、焦点、按下、展开），时长 150–400ms；支持 prefers-reduced-motion；不为炫技牺牲性能。\n\
- 响应式：375、768、1280、1920 宽度都要精心适配，不只是挤得下。\n\
- 性能与可访问性：不引入大体积动画库；图片用现代格式并标明宽高；首屏快、不跳动；语义化标签、焦点可见、键盘可用。";

/// 配置变量：运营配置不写死在代码里，Pilot 管值、按部署方式同步（见 site_config）。
pub const CONFIG_SPEC: &str = "【配置变量】邀请链接 / 邀请码、统计 ID、联系邮箱、站点地址、品牌名、第三方接口密钥这类运营配置，一律从环境变量读，不写死在代码里：\n\
- 集中在一个配置模块里读，给合理的默认值（比如站点地址默认用部署平台的默认域名）。Pilot 构建时把值作为进程环境变量注入，不会生成 `.env` 文件——确保 `npm run build` 时读得到进程环境变量（公开变量按框架要求加前缀，比如 Astro 的 `PUBLIC_`）。\n\
- 写 `.env.example` 列出每个变量：上一行注释写清用途，并标 `[public]`（会出现在页面里）或 `[secret]`（只能在构建时或服务端用，绝不能进前端代码）；值留空或写示例。`.env` 不提交。\n\
- 本地开发可以读 `.env`；值由 Pilot 保管，部署时同步到 Cloudflare 的环境变量或服务器上的 env 文件。\n\
- 需要我填的配置值写进 `.pilot/todo.json` 时，inputs 的 key 就用变量名（和 `.env.example` 一致），我在看板上填好会直接存成配置变量，不用你改代码。\n\
- GA4 统计固定用变量 `PUBLIC_GA_ID`（衡量 ID，[public]）：有值时在所有页面共用的 `<head>` 里用官方 gtag.js 加载，没值就不加载任何统计代码。GA4 不用写进待办，Pilot 会在看板上帮我创建并填好。";

/// 待我处理的事：写成 Pilot 能读的清单，显示在站点看板上逐条处理（不只写在汇报里——汇报会被埋在对话里）。
pub const TODO_SPEC: &str = "【待我处理的事】凡是需要我提供信息、做决定或点头的事（邀请链接、联系方式、正式域名、品牌名、需要登录才能截的图……），都写进 `.pilot/todo.json`，Pilot 会显示在站点看板上让我逐条处理；处理完的条目由你删掉，没有待办时写 `{\"items\":[]}`。格式：\n\
{\"items\":[{\"id\":\"短英文标识\",\"title\":\"一句话说清要我做什么\",\"why\":\"为什么重要、不处理会怎样\",\"kind\":\"input|ai|confirm|suggest\",\"affects_goal\":true,\"where\":\"会改在哪个文件的哪个字段\",\"inputs\":[{\"key\":\"字段标识\",\"label\":\"给我看的名称\",\"placeholder\":\"示例\",\"value\":\"现在的值（可选）\"}]}]}\n\
- kind：input＝需要我提供值（inputs 里列出要填的项）；ai＝你能做、但要我点头（比如补截图）；confirm＝需要我确认一个决定；suggest＝可选的建议。\n\
- affects_goal：不处理会直接影响站点目标（比如联盟推广站没有邀请链接就没有收入）。\n\
- 正式域名固定用 id `domain`，inputs 里放一个 key 为 `domain` 的输入。\n\
- 汇报里简要提一句有几件事要我处理即可，不用再长篇列出。";

/// 第一版对话的第一条消息：照着它，AI 从空仓库做出能预览的第一版。
pub fn first_message(req: &CreateSiteRequest) -> String {
    let stack = STACKS.iter().find(|s| s.0 == req.stack).map(|s| s.1).unwrap_or(&req.stack);
    let mut head = format!("请从零搭建这个新站点，做出第一版。\n\n【站点】{}", req.name.trim());
    if !req.domain.trim().is_empty() {
        head.push_str(&format!("（{}）", req.domain.trim()));
    }
    head.push_str(&format!("\n【定位】{}", positioning_text(req)));
    // 类型、语言没指定：让 AI 按定位和读者判断，判断结果写进站点约定
    let mut judge = vec![];
    if kind_label(&req.kind).is_none() {
        judge.push("站点类型（内容站、文档、落地页还是工具站）");
    }
    if req.language.trim().is_empty() {
        judge.push("页面语言");
    }
    if !judge.is_empty() {
        head.push_str(&format!("\n【需要你判断】{}没有指定：按上面的定位和读者判断，在规划里写明你的判断和理由。", judge.join("和")));
    }
    let style = if req.style.trim().is_empty() { "风格方向按定位和读者自己定。".to_string() } else { format!("风格方向：{}", req.style.trim()) };
    format!(
        "{head}\n\n【技术栈】{stack}。{guide}\n当前目录是新建的空仓库（已 git init，`.pilot/brief.md` 记着上面这些需求）。\n\n\
【第一版要做的】\n\
1. 先规划：按定位列出站点结构——首页、{n} 个核心页面（支柱页 + 回应具体搜索意图的长尾页）、关于、联系、隐私政策。每页写清它回应什么搜索意图、目标长尾查询词是什么、和哪些页面互链，写进 `.pilot/site.md` 的「内容规划」一节，再动手。\n\
2. 搭骨架：统一的页头导航、页脚（带关于 / 联系 / 隐私的链接）、内容页的面包屑。\n\
3. 写首批页面：按下面的【内容标准】写，每个核心页面都是完整的内容页；页面之间按规划互链。不要 lorem ipsum、「敬请期待」这类占位内容。\n\
4. SEO 基础：每页独立的 title 和 description、canonical、Open Graph；sitemap.xml、robots.txt；内容页加 Article 结构化数据，有问答的加 FAQPage，有面包屑的加 BreadcrumbList，首页加 WebSite / Organization；favicon；图片都有 alt；语义化 HTML。内容页必须在构建时生成完整 HTML，不能只靠浏览器端渲染。\n\
5. 视觉：按下面的【视觉标准】做。{style}\n\
6. 写站点约定 `.pilot/site.md`：技术栈；内容放在哪个目录、文件怎么命名、开头元数据有哪些字段及含义；新增一页要改哪些地方（列表页、导航、sitemap 是否自动生成）；图片放哪；安装、构建、本地预览的命令和产物目录；再加一节「内容与视觉标准」，把下面两条标准和你定的设计方向浓缩进去。写到另一个人照着就能加一页的程度——以后加内容、改页面都按这份约定来。\n\
7. 自检并持续优化：\n\
   - 安装依赖、跑构建确认通过；\n\
   - 本地预览，分别在 375、768、1280 宽度截图首页和两个内容页，从排版、留白、视觉层级、色彩、动效、微交互、响应式、原创性逐项评估，写下问题；悬停、焦点、展开这些截图看不出的状态对照代码检查；\n\
   - 改完重新截图复查，重复直到找不出明显可提升之处；\n\
   - 再按【内容标准】通读一遍页面上的文字：有没有内部表达、标题是否是自然的长尾问法、有没有薄页或重复页。\n\
8. 把需要我处理的事写进 `.pilot/todo.json`，然后 `git add -A && git commit -m \"搭建第一版\"`，简要汇报：做了哪些页面、设计方向是什么、最后一轮自评的结论、怎么本地预览、有几件事要我处理。\n\n\
{content}\n\n{visual}\n\n{config}\n\n{todo}\n\n\
不要部署——上线由 Pilot 负责。",
        guide = stack_guide(&req.stack),
        n = pages_of(req),
        content = CONTENT_STANDARD,
        visual = VISUAL_STANDARD,
        config = CONFIG_SPEC,
        todo = TODO_SPEC,
    )
}

/// 「继续制作」：第一版没做完 AI 就停了（中断、出错、额度用完），接着做。
/// `pending` 是还没完成的进度项；照样带上最新的内容标准和视觉标准，已经做好的部分也按它复查。
pub fn continue_message(pending: &[&str], stack: &str) -> String {
    let left = if pending.is_empty() { "（进度上都完成了，请复查质量）".to_string() } else { pending.join("、") };
    let stack = STACKS.iter().find(|s| s.0 == stack).map(|s| s.1).unwrap_or(stack);
    format!(
        "继续制作这个站点的第一版（技术栈 {stack}）。上一轮没做完就停了，先看现状再接着做：\n\
1. 读 `.pilot/brief.md`（需求）和 `.pilot/site.md`（规划和约定），对照「内容规划」看哪些页面还没做、哪些没写完；已经做好的部分不要推倒重写。\n\
2. 还没完成的进度：{left}。\n\
3. 把规划里的页面都做完（核心页面、关于、联系、隐私政策），按下面的【内容标准】写；已有的页面达不到标准的，一并改好。\n\
4. 视觉按下面的【视觉标准】：设计方向还没写进 `.pilot/site.md` 的先定下来；在 375、768、1280 宽度截图首页和两个内容页，逐项评估、改完复查，直到找不出明显可提升之处。\n\
5. `.pilot/site.md` 里补上「内容与视觉标准」一节（如果还没有）。\n\
6. 需要我处理的事写进 `.pilot/todo.json`；跑构建确认通过，`git add -A && git commit -m \"搭建第一版\"`，然后简要汇报：做了哪些页面、设计方向、最后一轮自评、有几件事要我处理。\n\n\
{content}\n\n{visual}\n\n{config}\n\n{todo}\n\n\
不要部署——上线由 Pilot 负责。",
        content = CONTENT_STANDARD,
        visual = VISUAL_STANDARD,
        config = CONFIG_SPEC,
        todo = TODO_SPEC,
    )
}

// ───────────────────────── 第一版进度 + 上线前检查 ─────────────────────────
//
// 进度看文件，不看对话：AI 说做完了不算，`.pilot/site.md` 写了、页面有了、构建产物在、提交了才算。
// 上线前检查扫构建产物里的 HTML：缺页、占位、断链、孤立页、标题描述、canonical、结构化数据、薄内容、sitemap / robots。

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

/// AI 写在 `.pilot/todo.json` 里、需要我处理的一件事。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Todo {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub why: String,
    /// input | ai | confirm | suggest
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub affects_goal: bool,
    #[serde(default, rename = "where")]
    pub location: String,
    #[serde(default)]
    pub inputs: Vec<TodoInput>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct TodoInput {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Deserialize)]
struct TodoFile {
    #[serde(default)]
    items: Vec<Todo>,
}

/// 读 `.pilot/todo.json`：None＝AI 还没写这个文件。这个文件是唯一的待办清单：
/// 我在看板上标「已处理 / 不需要」会直接把条目从文件里删掉（见 remove_todo），AI 和看板看到的永远是同一份。
pub fn load_todos(code: &Path) -> Option<Vec<Todo>> {
    let raw = std::fs::read(code.join(".pilot/todo.json")).ok()?;
    let f: TodoFile = serde_json::from_slice(&raw).unwrap_or(TodoFile { items: vec![] });
    Some(f.items.into_iter().filter(|t| !t.id.trim().is_empty() && !t.title.trim().is_empty()).take(30).collect())
}

/// 从 `.pilot/todo.json` 里删掉一条（其它条目和字段原样保留）。返回删没删到。
pub fn remove_todo(code: &Path, id: &str) -> Result<bool, String> {
    let path = code.join(".pilot/todo.json");
    let Ok(raw) = std::fs::read(&path) else { return Ok(false) };
    let mut v: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| format!(".pilot/todo.json 不是合法的 JSON：{e}"))?;
    let Some(items) = v.get_mut("items").and_then(|i| i.as_array_mut()) else { return Ok(false) };
    let before = items.len();
    items.retain(|t| t.get("id").and_then(|x| x.as_str()) != Some(id));
    if items.len() == before {
        return Ok(false);
    }
    let mut text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(true)
}

/// 上线前检查里的一项：有影响站点目标的待办就提醒（不算必须修，但要醒目）。
pub fn todo_check(todos: &[Todo]) -> Check {
    let goal: Vec<String> = todos.iter().filter(|t| t.affects_goal).map(|t| t.title.clone()).collect();
    Check {
        key: "todo",
        label: "需要你处理的事",
        level: if goal.is_empty() { "ok" } else { "warn" },
        detail: if todos.is_empty() {
            "没有".into()
        } else if goal.is_empty() {
            format!("{} 项，都不影响站点目标", todos.len())
        } else {
            format!("{} 项会影响站点目标，上线前最好处理掉", goal.len())
        },
        items: goal,
    }
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
/// `built_app`：构建在哪个目录的站点目录里发生（本地来源就是 app；GitHub 来源是部署工作树）。
pub fn progress(app: &Path, code: &Path, built_app: &Path, out: &str, commits: usize) -> Vec<MakeStep> {
    let conv = std::fs::read_to_string(code.join(".pilot/site.md")).unwrap_or_default();
    let planned = conv.contains("内容规划");
    let built_dir = out_dir(built_app, out);
    let skip = if out.is_empty() || out == "." { vec![] } else { vec![out_dir(app, out)] };
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
    href.split(['#', '?']).next().unwrap_or("").trim().is_empty() || resolve_link(root, page, href).is_some()
}

/// 站内链接指向的那个文件（不存在就是 None）。
fn resolve_link(root: &Path, page: &Path, href: &str) -> Option<PathBuf> {
    let h = href.split(['#', '?']).next().unwrap_or("").trim();
    if h.is_empty() {
        return None;
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
    [p.clone(), p.join("index.html"), p.with_extension("html")].into_iter().find(|c| c.is_file())
}

/// 构建产物里的文件 → 网址路径（`guide/index.html` → `/guide/`）。
fn page_url(out: &Path, p: &Path) -> String {
    let r = p.strip_prefix(out).unwrap_or(p).to_string_lossy().replace('\\', "/");
    let r = r.strip_suffix("index.html").map(|x| x.to_string()).unwrap_or(r);
    format!("/{r}")
}

/// 指向本站的绝对网址（`https://本站域名/…`）→ 站内路径；不是本站的返回 None。
fn own_path(href: &str, hosts: &[String]) -> Option<String> {
    let rest = href.trim().split_once("://").map(|(_, r)| r).or_else(|| href.trim().strip_prefix("//"))?;
    let (host, path) = rest.find('/').map(|i| (&rest[..i], &rest[i..])).unwrap_or((rest, "/"));
    let host = host.to_ascii_lowercase();
    let bare = host.trim_start_matches("www.");
    hosts.iter().any(|h| h.trim_start_matches("www.") == bare).then(|| path.to_string())
}

/// 本站域名：从各页面的 canonical / og:url 里认（构建产物本身不知道站点域名）。
fn own_hosts(pages: &[(PathBuf, String)]) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for (_, html) in pages {
        let low = html.to_ascii_lowercase();
        let canon = tags(html, &low, "link").into_iter().filter(|l| attr(l, "rel").is_some_and(|r| r.eq_ignore_ascii_case("canonical"))).filter_map(|l| attr(l, "href"));
        let og = tags(html, &low, "meta").into_iter().filter(|m| attr(m, "property").is_some_and(|p| p.eq_ignore_ascii_case("og:url"))).filter_map(|m| attr(m, "content"));
        for u in canon.chain(og) {
            if let Some(h) = u.split_once("://").map(|(_, r)| r.split('/').next().unwrap_or("").to_ascii_lowercase()).filter(|h| !h.is_empty()) {
                if !hosts.contains(&h) {
                    hosts.push(h);
                }
            }
        }
    }
    hosts
}

/// 正经页面（跳过跳转页和 404）：(文件, HTML)。
fn real_pages(out: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    walk(out, &[], &mut files, &|p| has_ext(p, &["html"]));
    files.sort();
    files
        .into_iter()
        .filter_map(|f| {
            let html = std::fs::read_to_string(&f).ok()?;
            let low = html.to_ascii_lowercase();
            (!low.contains("http-equiv=\"refresh\"") && !page_url(out, &f).to_ascii_lowercase().contains("404")).then_some((f, html))
        })
        .collect()
}

/// 每个页面（网址路径）被多少个**其他**页面链接到（导航、页脚的链接也算）。
pub fn inbound_links(out: &Path) -> std::collections::HashMap<String, usize> {
    let pages = real_pages(out);
    let hosts = own_hosts(&pages);
    let mut sources: std::collections::HashMap<String, std::collections::HashSet<String>> = pages.iter().map(|(f, _)| (page_url(out, f), Default::default())).collect();
    for (f, html) in &pages {
        let from = page_url(out, f);
        let low = html.to_ascii_lowercase();
        for a in tags(html, &low, "a") {
            let Some(h) = attr(a, "href") else { continue };
            let h = if is_external(&h) { match own_path(&h, &hosts) { Some(p) => p, None => continue } } else { h };
            if let Some(target) = resolve_link(out, f, &h).map(|t| page_url(out, &t)) {
                if target != from {
                    if let Some(set) = sources.get_mut(&target) {
                        set.insert(from.clone());
                    }
                }
            }
        }
    }
    sources.into_iter().map(|(k, v)| (k, v.len())).collect()
}

/// 规范化网址路径用于比较：去掉 index.html / .html 和末尾斜杠。
pub fn norm_path(p: &str) -> String {
    let p = p.split(['#', '?']).next().unwrap_or("");
    let p = p.strip_suffix("index.html").unwrap_or(p);
    let p = p.strip_suffix(".html").unwrap_or(p);
    let t = p.trim_end_matches('/');
    if t.is_empty() { "/".into() } else { t.to_string() }
}

/// 页面里的结构化数据（JSON-LD）：(块数, 出错说明)。
fn json_ld(html: &str, low: &str) -> (usize, Vec<String>) {
    let mut n = 0;
    let mut errs = Vec::new();
    let mut from = 0;
    while let Some(i) = low[from..].find("<script") {
        let a = from + i;
        let Some(j) = low[a..].find('>') else { break };
        let open = &low[a..a + j];
        let start = a + j + 1;
        let end = low[start..].find("</script>").map(|k| start + k).unwrap_or(low.len());
        from = end;
        if !open.contains("ld+json") {
            continue;
        }
        n += 1;
        match serde_json::from_str::<serde_json::Value>(html[start..end].trim()) {
            Err(e) => errs.push(format!("不是合法的 JSON（{e}）")),
            Ok(v) => {
                let typed = |x: &serde_json::Value| x.get("@type").is_some() || x.get("@graph").is_some();
                let ok = match &v {
                    serde_json::Value::Array(a) => !a.is_empty() && a.iter().all(typed),
                    other => typed(other),
                };
                if !ok {
                    errs.push("缺 @type".into());
                }
            }
        }
    }
    (n, errs)
}

fn is_external(h: &str) -> bool {
    let l = h.trim().to_ascii_lowercase();
    l.is_empty() || l.starts_with('#') || l.contains("://") || l.starts_with("//") || ["mailto:", "tel:", "javascript:", "data:", "sms:"].iter().any(|p| l.starts_with(p))
}

/// 站点上的一篇文章（「分发」用）：结构化数据里是 Article / BlogPosting / HowTo 这类的页面。
#[derive(Serialize, Clone, Debug)]
pub struct Article {
    /// 网址路径，如 `/guides/buy-usdt`
    pub path: String,
    pub title: String,
    pub description: String,
    /// 结构化数据里的发布日期（没有就空）
    pub date: String,
    /// 正文（去掉导航页头页脚，最多 8000 字）
    #[serde(skip)]
    pub text: String,
}

const ARTICLE_TYPES: &[&str] = &["Article", "BlogPosting", "NewsArticle", "TechArticle", "HowTo"];

/// 构建产物里的文章，按发布日期新的在前（没日期的排后面）。
pub fn articles(out: &Path) -> Vec<Article> {
    let mut list = Vec::new();
    for (f, html) in real_pages(out) {
        let low = html.to_ascii_lowercase();
        let path = page_url(out, &f);
        let mut is_article = false;
        let mut date = String::new();
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
            let Ok(v) = serde_json::from_str::<serde_json::Value>(html[start..end].trim()) else { continue };
            let mut nodes: Vec<serde_json::Value> = match v {
                serde_json::Value::Array(a) => a,
                other => vec![other],
            };
            if let Some(g) = nodes.iter().find_map(|n| n.get("@graph").and_then(|g| g.as_array()).cloned()) {
                nodes.extend(g);
            }
            for n in &nodes {
                let types: Vec<String> = match &n["@type"] {
                    serde_json::Value::String(t) => vec![t.clone()],
                    serde_json::Value::Array(a) => a.iter().filter_map(|t| t.as_str().map(String::from)).collect(),
                    _ => vec![],
                };
                if types.iter().any(|t| ARTICLE_TYPES.contains(&t.as_str())) {
                    is_article = true;
                    if date.is_empty() {
                        date = n["datePublished"].as_str().unwrap_or("").chars().take(10).collect();
                    }
                }
            }
        }
        if !is_article || path == "/" {
            continue;
        }
        let title = inner(&html, &low, "h1").map(|t| strip_tags(t).trim().to_string()).filter(|t| !t.is_empty()).or_else(|| inner(&html, &low, "title").map(|t| strip_tags(t).trim().to_string())).unwrap_or_default();
        let description = tags(&html, &low, "meta")
            .into_iter()
            .find(|m| attr(m, "name").is_some_and(|n| n.eq_ignore_ascii_case("description")))
            .and_then(|m| attr(m, "content"))
            .unwrap_or_default();
        let body = inner(&html, &low, "article").or_else(|| inner(&html, &low, "main")).or_else(|| inner(&html, &low, "body")).unwrap_or(&html);
        let text = strip_tags(&drop_blocks(body, &["script", "style", "nav", "header", "footer", "aside", "noscript", "svg", "template", "form"]));
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(8000).collect();
        list.push(Article { path, title, description, date, text });
    }
    list.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.path.cmp(&b.path)));
    list
}

const REQUIRED: &[(&str, &[&str])] = &[("关于", &["about", "关于"]), ("联系", &["contact", "联系"]), ("隐私政策", &["privacy", "隐私"])];
const LISTING: &[&str] = &["tag", "categor", "page/", "archive", "search", "blog/index", "posts/index"];
const EXTRA_PLACEHOLDERS: &[&str] = &["敬请期待", "coming soon", "待补", "your-email", "example@example.com", "xxx-xxxx"];
/// 内容页的文字量下限（和托管运营判断「新页面太薄」用同一把尺）。
pub const THIN_UNITS: usize = 600;

/// 页面底色是不是 AI 最爱用的米黄 / 奶油「纸张」色：是就返回那个颜色。
/// 看 html / body / :root 的背景（解析一层 CSS 变量），深色模式里的定义不算；找不到再看叫 bg / paper / background 之类的变量。
pub fn paper_background(out: &Path) -> Option<String> {
    let mut css = String::new();
    let mut files = Vec::new();
    walk(out, &[], &mut files, &|p| has_ext(p, &["css"]));
    files.sort();
    for f in files.iter().take(40) {
        css.push_str(&std::fs::read_to_string(f).unwrap_or_default());
        css.push('\n');
    }
    // 内联在页面里的样式（Astro 小样式表会内联）
    let home = std::fs::read_to_string(out.join("index.html")).unwrap_or_default();
    let low = home.to_ascii_lowercase();
    for st in tags(&home, &low, "style") {
        css.push_str(st);
        css.push('\n');
    }
    let css = strip_dark(&css);
    let rules = css_rules(&css);
    let mut vars: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (_, body) in &rules {
        for (k, v) in decls(body) {
            if let Some(name) = k.strip_prefix("--") {
                vars.entry(name.to_string()).or_insert(v);
            }
        }
    }
    let resolve = |v: &str| -> Option<String> {
        let mut v = v.trim().to_string();
        for _ in 0..3 {
            let Some(rest) = v.strip_prefix("var(") else { break };
            let name = rest.trim_end_matches(')').split(',').next()?.trim().trim_start_matches("--").to_string();
            v = vars.get(&name)?.trim().to_string();
        }
        Some(v)
    };
    let mut cands = Vec::new();
    for (sel, body) in &rules {
        if !sel.split(',').any(|x| matches!(x.trim(), "html" | "body" | ":root" | "html body")) {
            continue;
        }
        for (k, v) in decls(body) {
            if k == "background-color" || k == "background" {
                if let Some(c) = resolve(&v) {
                    cands.push(c);
                }
            }
        }
    }
    if cands.is_empty() {
        for k in ["bg", "background", "paper", "page", "canvas", "surface", "color-bg", "color-background", "bg-page"] {
            if let Some(v) = vars.get(k).and_then(|v| resolve(v)) {
                cands.push(v);
            }
        }
    }
    cands.into_iter().find(|c| parse_color(c).is_some_and(|(r, g, b)| is_paper(r, g, b)))
}

/// 去掉深色模式的定义：`@media (prefers-color-scheme: dark) {…}` 整块，和 `[data-theme=dark]` / `.dark` 开头的规则。
fn strip_dark(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("@media") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(open) = tail.find('{') else { rest = ""; break };
        let dark = tail[..open].replace(' ', "").contains("prefers-color-scheme:dark");
        let mut depth = 0usize;
        let mut end = tail.len();
        for (j, ch) in tail[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + j + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !dark {
            // 别的 media 块原样保留（只去掉外层，里面的规则照常参与）
            out.push_str(&tail[open + 1..end.saturating_sub(1).max(open + 1)]);
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// 平铺的 `选择器 { 声明 }`；暗色主题的选择器跳过。
fn css_rules(css: &str) -> Vec<(String, String)> {
    let mut v = Vec::new();
    let mut rest = css;
    while let Some(o) = rest.find('{') {
        let sel = rest[..o].rsplit(['}', ';']).next().unwrap_or("").trim().to_string();
        let Some(c) = rest[o..].find('}') else { break };
        let body = rest[o + 1..o + c].to_string();
        rest = &rest[o + c + 1..];
        let s = sel.to_ascii_lowercase();
        if s.contains("dark") {
            continue;
        }
        v.push((s, body));
    }
    v
}

fn decls(body: &str) -> Vec<(String, String)> {
    body.split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().trim_end_matches("!important").trim().to_string()))
        })
        .collect()
}

/// `#rgb` / `#rrggbb` / `rgb()` / `hsl()`（background 简写里取第一个颜色）。
fn parse_color(v: &str) -> Option<(u8, u8, u8)> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(i) = v.find('#') {
        let hex: String = v[i + 1..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        let h = |s: &str| u8::from_str_radix(s, 16).ok();
        return match hex.len() {
            3 | 4 => {
                let d: Vec<u8> = hex.chars().take(3).map(|c| h(&c.to_string()).unwrap_or(0) * 17).collect();
                Some((d[0], d[1], d[2]))
            }
            6 | 8 => Some((h(&hex[0..2])?, h(&hex[2..4])?, h(&hex[4..6])?)),
            _ => None,
        };
    }
    let nums = |s: &str| -> Vec<f64> { s.split(|c: char| c == ',' || c == ' ' || c == '/').filter_map(|x| x.trim().trim_end_matches(['%', ')']).trim_end_matches("deg").parse::<f64>().ok()).collect() };
    if let Some(i) = v.find("rgb") {
        let inner = v[i..].split_once('(')?.1.split(')').next()?;
        let n = nums(inner);
        return (n.len() >= 3).then(|| (n[0] as u8, n[1] as u8, n[2] as u8));
    }
    if let Some(i) = v.find("hsl") {
        let inner = v[i..].split_once('(')?.1.split(')').next()?;
        let n = nums(inner);
        if n.len() < 3 {
            return None;
        }
        let (h, s, l) = (n[0].rem_euclid(360.0), n[1] / 100.0, n[2] / 100.0);
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = l - c / 2.0;
        let (r, g, b) = match (h / 60.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let f = |v: f64| ((v + m) * 255.0).round() as u8;
        return Some((f(r), f(g), f(b)));
    }
    None
}

/// 很亮、偏暖黄（色相 20°–75°）、又不是纯白 / 中性灰：就是那种米黄 / 奶油纸色。
fn is_paper(r: u8, g: u8, b: u8) -> bool {
    let (mx, mn) = (r.max(g).max(b) as f64, r.min(g).min(b) as f64);
    let chroma = mx - mn;
    let light = (mx + mn) / 2.0 / 255.0;
    if light < 0.86 || !(5.0..=60.0).contains(&chroma) {
        return false;
    }
    let (r, g, b) = (r as f64, g as f64, b as f64);
    let hue = if mx == r {
        60.0 * ((g - b) / chroma).rem_euclid(6.0)
    } else if mx == g {
        60.0 * ((b - r) / chroma + 2.0)
    } else {
        60.0 * ((r - g) / chroma + 4.0)
    };
    (20.0..=75.0).contains(&hue)
}

pub const PAPER_HINT: &str = "这是 AI 建站最常见的套路底色。换成纯白、中性浅灰、深色或从品牌色来的底色，连同纸张、票据、印章这类配套装饰一起换掉（设计方向写进 .pilot/site.md）";

/// 扫构建产物，给出上线前检查的结果。
pub fn readiness(out: &Path) -> Vec<Check> {
    if !out.join("index.html").is_file() {
        return vec![Check { key: "build", label: "构建产物", level: "fail", detail: "还没有构建产物（找不到 index.html）：先构建一次".into(), items: vec![] }];
    }
    let mut files = Vec::new();
    walk(out, &[], &mut files, &|p| has_ext(p, &["html"]));
    files.sort();
    let url = |p: &Path| page_url(out, p);
    let inbound = inbound_links(out);
    let (mut orphans, mut no_canon, mut bad_canon, mut ld_err, mut no_ld) = (vec![], vec![], vec![], vec![], vec![]);
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
        // 孤立页：除了首页，每页至少要有一个别的页面链到它，不然搜索引擎很难发现、也分不到站内权重
        if u != "/" && inbound.get(&u).copied().unwrap_or(0) == 0 {
            orphans.push(u.clone());
        }
        let canon = tags(&html, &low, "link").into_iter().find(|l| attr(l, "rel").is_some_and(|r| r.eq_ignore_ascii_case("canonical"))).and_then(|l| attr(l, "href")).filter(|h| !h.trim().is_empty());
        match canon {
            None => no_canon.push(u.clone()),
            Some(c) => {
                let cp = c.split_once("://").map(|(_, r)| r.find('/').map(|i| r[i..].to_string()).unwrap_or_else(|| "/".into())).unwrap_or(c.clone());
                if norm_path(&cp) != norm_path(&u) {
                    bad_canon.push(format!("{u} → {c}"));
                }
            }
        }
        let (ld_n, errs) = json_ld(&html, &low);
        for e in errs {
            ld_err.push(format!("{u}：{e}"));
        }
        if ld_n == 0 && !special {
            no_ld.push(u.clone());
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
        Check { key: "orphans", label: "孤立页面", level: lvl(!orphans.is_empty(), "fail"), detail: if orphans.is_empty() { "每个页面都有别的页面链到它".into() } else { format!("{} 个页面没有任何站内链接指向它：从相关页面、列表页或导航加链接", orphans.len()) }, items: orphans },
        Check { key: "canonical", label: "规范网址（canonical）", level: if !no_canon.is_empty() { "fail" } else if !bad_canon.is_empty() { "warn" } else { "ok" }, detail: if !no_canon.is_empty() { format!("{} 个页面没有 canonical", no_canon.len()) } else if !bad_canon.is_empty() { format!("{} 个页面的 canonical 指向了别的网址（不是故意合并重复页的话要改）", bad_canon.len()) } else { "每页都有、指向自己".into() }, items: no_canon.into_iter().chain(bad_canon).collect() },
        Check { key: "structured", label: "结构化数据", level: if !ld_err.is_empty() { "fail" } else if !no_ld.is_empty() { "warn" } else { "ok" }, detail: if !ld_err.is_empty() { format!("{} 处结构化数据有错，搜索引擎读不了", ld_err.len()) } else if !no_ld.is_empty() { format!("{} 个内容页没有结构化数据（Article / FAQPage / BreadcrumbList）", no_ld.len()) } else { "内容页都有、格式正确".into() }, items: ld_err.into_iter().chain(no_ld).collect() },
        Check { key: "title", label: "页面标题", level: if !no_title.is_empty() { "fail" } else if !dup.is_empty() { "warn" } else { "ok" }, detail: if !no_title.is_empty() { format!("{} 个页面没有标题", no_title.len()) } else if !dup.is_empty() { format!("{} 组页面标题重复", dup.len()) } else { "每页都有、不重复".into() }, items: no_title.into_iter().chain(dup).collect() },
        Check { key: "description", label: "页面描述", level: lvl(!no_desc.is_empty(), "warn"), detail: if no_desc.is_empty() { "每页都有".into() } else { format!("{} 个页面没有 description", no_desc.len()) }, items: no_desc },
        Check { key: "h1", label: "一级标题", level: lvl(!no_h1.is_empty(), "warn"), detail: if no_h1.is_empty() { "每页都有".into() } else { format!("{} 个页面没有 h1", no_h1.len()) }, items: no_h1 },
        Check { key: "thin", label: "内容厚度", level: lvl(!thin.is_empty(), "warn"), detail: if thin.is_empty() { format!("内容页正文都在 {THIN_UNITS} 字以上") } else { format!("{} 个内容页正文不到 {THIN_UNITS} 字，容易被搜索引擎当成薄内容", thin.len()) }, items: thin },
        Check { key: "seo_files", label: "sitemap.xml / robots.txt", level: lvl(!seo_files.is_empty(), "warn"), detail: if seo_files.is_empty() { "都有".into() } else { format!("缺：{}", seo_files.join("、")) }, items: vec![] },
        match paper_background(out) {
            Some(c) => Check { key: "paper_bg", label: "页面底色", level: "warn", detail: format!("底色是米黄 / 奶油色（{c}）：{PAPER_HINT}"), items: vec![] },
            None => Check { key: "paper_bg", label: "页面底色", level: "ok", detail: "不是套路的米黄纸色".into(), items: vec![] },
        },
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
    fn spots_the_cream_paper_background() {
        assert!(is_paper(0xf4, 0xf1, 0xe9) && is_paper(0xfa, 0xf7, 0xf0) && is_paper(0xf5, 0xf0, 0xe6));
        assert!(!is_paper(255, 255, 255) && !is_paper(0xf5, 0xf5, 0xf4) && !is_paper(0xf3, 0xf4, 0xf6), "白和中性灰不算");
        assert!(!is_paper(0x15, 0x17, 0x1b) && !is_paper(0xe8, 0xf0, 0xfe), "深色、冷色不算");
        assert_eq!(parse_color("hsl(44 32% 93%)").map(|(r, g, b)| is_paper(r, g, b)), Some(true));
        assert_eq!(parse_color("rgb(244, 241, 233)"), Some((244, 241, 233)));
        assert_eq!(parse_color("#fff url(x.png) no-repeat"), Some((255, 255, 255)));

        let t = tempfile::tempdir().unwrap();
        let out = t.path();
        std::fs::write(out.join("index.html"), "<html><body>x</body></html>").unwrap();
        std::fs::create_dir_all(out.join("_astro")).unwrap();
        let css = ":root{--paper:#f4f1e9;--ink:#15171b}@media (prefers-color-scheme:dark){:root{--paper:#111}}body{background:var(--paper);color:var(--ink)}";
        std::fs::write(out.join("_astro/a.css"), css).unwrap();
        assert_eq!(paper_background(out).as_deref(), Some("#f4f1e9"));
        // 深色定义写在前面也不受影响
        std::fs::write(out.join("_astro/a.css"), "@media (prefers-color-scheme: dark){:root{--bg:#f4f1e9}}:root{--bg:#ffffff}html{background-color:var(--bg)}").unwrap();
        assert_eq!(paper_background(out), None);
        // body 没写背景：按变量名找
        std::fs::write(out.join("_astro/a.css"), ":root{--bg:#faf7f0}").unwrap();
        assert!(paper_background(out).is_some());
        std::fs::write(out.join("index.html"), "<html><head><style>body{background:#fff}</style></head><body></body></html>").unwrap();
        std::fs::write(out.join("_astro/a.css"), "").unwrap();
        assert_eq!(paper_background(out), None);
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
        assert!(m.contains("【内容标准】") && m.contains("【视觉标准】") && m.contains("Awwwards") && m.contains("截图工具") && m.contains("长尾"));
        assert!(!m.contains("留出明显的待补位置"), "页面上不留待补字样，缺的信息写进汇报");
        assert!(m.contains("Astro"));
        assert!(m.contains("站点类型") && m.contains("页面语言") && m.contains("需要你判断"));
        let set = first_message(&CreateSiteRequest { kind: "docs".into(), language: "English".into(), ..req() });
        assert!(!set.contains("需要你判断") && set.contains("类型：文档 / 知识库") && set.contains("\n语言：English") && !set.contains("读者："));
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

    fn seo_page(path: &str, title: &str, head: &str, links: &str) -> String {
        format!("<!doctype html><html lang=\"en\"><head><title>{title}</title><link rel=\"canonical\" href=\"https://ex.com{path}\">{head}</head><body><nav>{links}</nav><main><h1>{title}</h1></main></body></html>")
    }

    #[test]
    fn readiness_flags_orphans_canonical_and_structured_data() {
        let t = tempfile::tempdir().unwrap();
        let ld = r#"<script type="application/ld+json">{"@context":"https://schema.org","@type":"Article"}</script>"#;
        // 首页用本站绝对网址链到 /a/，/a/ 用相对路径链到 /b.html；/c/ 没人链（孤立）
        write(t.path(), "index.html", &seo_page("/", "Home", ld, "<a href=\"https://www.ex.com/a/\">a</a>"));
        write(t.path(), "a/index.html", &seo_page("/a/", "A", ld, "<a href=\"../b.html\">b</a><a href=\"/\">home</a>"));
        write(t.path(), "b.html", &seo_page("/b", "B", "<script type=\"application/ld+json\">{oops}</script>", "<a href=\"/a/\">a</a>"));
        write(t.path(), "c/index.html", &format!("<html><head><title>C</title><link rel=\"canonical\" href=\"https://ex.com/a/\"></head><body><h1>C</h1></body></html>"));
        write(t.path(), "d/index.html", "<html><head><title>D</title></head><body><a href=\"/c/\">c</a><a href=\"/d/\">self</a></body></html>");
        let inbound = inbound_links(t.path());
        assert_eq!(inbound["/a/"], 2, "{inbound:?}");
        assert_eq!(inbound["/b.html"], 1);
        assert_eq!(inbound["/d/"], 0, "链自己不算");
        let r = readiness(t.path());
        let get = |k: &str| r.iter().find(|c| c.key == k).unwrap().clone();
        assert_eq!(get("orphans").level, "fail");
        assert_eq!(get("orphans").items, vec!["/d/".to_string()], "/c/ 有 /d/ 链着，首页不算");
        assert_eq!(get("canonical").level, "fail");
        assert!(get("canonical").items.contains(&"/d/".to_string()) && get("canonical").items.iter().any(|i| i.starts_with("/c/ → ")), "{:?}", get("canonical").items);
        assert_eq!(get("structured").level, "fail");
        assert!(get("structured").items.iter().any(|i| i.starts_with("/b.html：不是合法的 JSON")), "{:?}", get("structured").items);
        assert_eq!(norm_path("/a/index.html"), "/a");
        assert_eq!(norm_path("/b.html"), "/b");
        assert_eq!(norm_path("/"), "/");
    }

    #[test]
    fn progress_reads_files_not_promises() {
        let t = tempfile::tempdir().unwrap();
        let steps = progress(t.path(), t.path(), t.path(), "dist", 1);
        assert!(steps.iter().all(|s| !s.done));
        write(t.path(), ".pilot/site.md", &format!("# 站点约定\n\n## 内容规划\n\n{}", "说明".repeat(120)));
        for i in 0..4 {
            write(t.path(), &format!("src/content/p{i}.md"), "# x");
        }
        write(t.path(), "dist/index.html", "<html></html>");
        let steps = progress(t.path(), t.path(), t.path(), "dist", 2);
        assert!(steps.iter().all(|s| s.done), "{:?}", steps.iter().map(|s| (s.key, s.done)).collect::<Vec<_>>());
    }

    #[test]
    fn continue_message_lists_what_is_left_and_carries_the_standards() {
        let m = continue_message(&["构建通过", "提交第一版"], "astro");
        assert!(m.contains("构建通过、提交第一版") && m.contains("不要推倒重写") && m.contains("Astro"));
        assert!(m.contains("【内容标准】") && m.contains("【视觉标准】") && m.contains("不要部署"));
    }

    #[test]
    fn todos_come_from_the_file_minus_what_i_handled_and_goal_items_raise_a_warning() {
        let t = tempfile::tempdir().unwrap();
        assert!(load_todos(t.path()).is_none(), "没写文件＝None");
        write(t.path(), ".pilot/todo.json", r#"{"items":[
            {"id":"referral","title":"填写邀请链接","kind":"input","affects_goal":true,"where":"src/site.config.ts","inputs":[{"key":"binance","label":"币安邀请链接"}]},
            {"id":"brand","title":"确认品牌名","kind":"confirm"},
            {"id":"","title":"没有 id 的不要"}
        ]}"#);
        let all = load_todos(t.path()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].location, "src/site.config.ts");
        assert_eq!(todo_check(&all).level, "warn");
        // 标「已处理」直接从文件里删：AI 下次读到的就是剩下的
        assert!(remove_todo(t.path(), "referral").unwrap());
        assert!(!remove_todo(t.path(), "referral").unwrap());
        let rest = load_todos(t.path()).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(todo_check(&rest).level, "ok");
        let m = first_message(&req());
        assert!(m.contains(".pilot/todo.json") && m.contains("【待我处理的事】") && !m.contains("在汇报里列出来"));
        assert!(m.contains("【配置变量】") && m.contains(".env.example") && m.contains("PUBLIC_GA_ID") && continue_message(&[], "astro").contains("【配置变量】"));
    }
}

#[cfg(test)]
mod live_check {
    /// 手动跑：PILOT_CHECK_OUT=<构建产物目录> cargo test --lib live_readiness -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_readiness() {
        let out = std::path::PathBuf::from(std::env::var("PILOT_CHECK_OUT").unwrap());
        for c in super::readiness(&out) {
            println!("{} [{}] {} {:?}", c.label, c.level, c.detail, c.items.iter().take(5).collect::<Vec<_>>());
        }
        for a in super::articles(&out) {
            println!("article {} | {} | {} | {} 字", a.path, a.date, a.title, a.text.chars().count());
        }
    }
}
