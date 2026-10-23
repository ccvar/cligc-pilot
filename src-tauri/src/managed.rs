//! 托管运营（第 5 档「全托管」）：托管方案、全局设置、动态记录、每日摘要。
//!
//! 一个「托管方案」管一个或多个站点，各自有一套规则（模型、新页面发布方式、每日上限、模型用量）；
//! 一个站点最多属于一个方案，方案里的每个站点还能单独启用 / 停止。全局只剩总暂停和日报。
//! 运行逻辑在 site_runner（计划 / 执行 / 上线后检查 / 自动回滚），这里只放共享的状态与判定。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ManagedConfig {
    /// 总开关：暂停后所有托管站点都不再自动运行（进行中的那一个跑完为止）。
    #[serde(default)]
    pub paused: bool,
    /// 主模型（运营人员选）。
    #[serde(default = "default_brain")]
    pub brain: String,
    #[serde(default)]
    pub model: String,
    /// 备用模型：主模型这一轮失败（额度用完、限流、不可用）时自动换它重跑一次。空＝不用备用。
    #[serde(default)]
    pub fallback_brain: String,
    #[serde(default)]
    pub fallback_model: String,
    /// 新写的整篇内容（新页面）：auto 自动发布 | review 人审人发（运营人员审核后再发布）。
    #[serde(default = "default_review")]
    pub new_content: String,
    /// 每个站点每天最多自动上线几条。
    #[serde(default = "default_daily")]
    pub daily_changes: u32,
    /// 每个站点每周的模型用量上限（token，0＝不限）。
    #[serde(default = "default_tokens")]
    pub tokens_per_week: u64,
    /// 每日摘要（系统通知 + 存一份报告）。
    #[serde(default = "yes")]
    pub digest: bool,
    /// 几点之后发当天的摘要（本地时间，0–23）。
    #[serde(default = "default_hour")]
    pub digest_hour: u8,
    #[serde(default)]
    pub last_digest: String,
    /// 托管方案。上面的 brain / model / new_content / daily_changes / tokens_per_week 是老版本的全局规则，
    /// 只用于迁移和新建方案时的默认值。
    #[serde(default)]
    pub groups: Vec<ManagedGroup>,
    /// 已把老的第 1–4 档站点归进托管方案（只做一次）。
    #[serde(default)]
    pub levels_migrated: bool,
}

fn default_brain() -> String {
    "claude".into()
}
fn default_review() -> String {
    "review".into()
}
fn default_daily() -> u32 {
    3
}
fn default_tokens() -> u64 {
    2_000_000
}
fn yes() -> bool {
    true
}
fn default_hour() -> u8 {
    9
}

/// 托管方案：一组站点 + 这组站点的托管规则。
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ManagedGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub sites: Vec<String>,
    /// 方案里单独停止托管的站点（还在方案里，规则保留，只是不自动运行）。
    #[serde(default)]
    pub stopped: Vec<String>,
    /// 整个方案暂停。
    #[serde(default)]
    pub paused: bool,
    /// AI 自主程度：suggest 只出建议（不动代码）| review 改好等你确认 | auto 全自动（按发布规则自动上线）。
    #[serde(default = "default_autonomy")]
    pub autonomy: String,
    #[serde(default = "default_brain")]
    pub brain: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub fallback_brain: String,
    #[serde(default)]
    pub fallback_model: String,
    #[serde(default = "default_review")]
    pub new_content: String,
    #[serde(default = "default_daily")]
    pub daily_changes: u32,
    #[serde(default = "default_tokens")]
    pub tokens_per_week: u64,
    /// 运营目标：一两句话，写进每次计划和改动的指令里。
    #[serde(default)]
    pub goal: String,
    /// 允许的动作（改动类型）：meta / content / internal_links / new_content / technical。健康巡查始终开启。
    #[serde(default = "all_actions")]
    pub actions: Vec<String>,
    /// 内容准则：运营人员写的写作规则（语言、语气、结构、长度、禁区……），原样交给 AI。空＝不额外约束。
    #[serde(default)]
    pub guidelines: String,
    /// 选题清单：一行一个关键词 / 主题，新内容优先从这里选；没有数据的新站主要靠它。
    #[serde(default)]
    pub topics: String,
    /// 新页面正文下限（字，汉字按字、其它按词）：低于它不自动上线，转给运营人员判断。0＝不检查。
    #[serde(default = "default_min_page")]
    pub min_page_chars: u32,
    /// 衡量标准（效果复盘看什么）：clicks / ctr / impressions / position / conversions / health。
    #[serde(default = "default_metric")]
    pub metric: String,
    #[serde(default)]
    pub created_at: u64,
}

fn default_autonomy() -> String {
    "auto".into()
}
pub const AUTONOMY: &[&str] = &["suggest", "review", "auto"];
pub fn autonomy_label(a: &str) -> &'static str {
    match a {
        "suggest" => "只出建议",
        "review" => "改好等你确认",
        _ => "全自动",
    }
}
/// 自主程度对应的执行档位（运行逻辑仍按档位走）：只出建议＝1，改好等你确认＝3，全自动＝5。
pub fn level_of(autonomy: &str) -> u8 {
    match autonomy {
        "suggest" => 1,
        "review" => 3,
        _ => 5,
    }
}

fn default_min_page() -> u32 {
    1500
}
fn default_metric() -> String {
    "clicks".into()
}
pub const METRICS: &[&str] = &["clicks", "ctr", "impressions", "position", "conversions", "health"];
pub fn metric_label(m: &str) -> &'static str {
    match m {
        "ctr" => "点击率",
        "impressions" => "展现和覆盖的查询词",
        "position" => "平均排名",
        "conversions" => "GA4 转化事件",
        "health" => "体检问题数",
        _ => "点击",
    }
}

pub const ACTIONS: &[&str] = &["meta", "content", "internal_links", "new_content", "technical"];
fn all_actions() -> Vec<String> {
    ACTIONS.iter().map(|s| s.to_string()).collect()
}
pub fn action_label(a: &str) -> &'static str {
    match a {
        "meta" => "改标题和描述",
        "content" => "改进已有页面内容",
        "internal_links" => "加站内链接",
        "new_content" => "写新页面",
        "technical" => "修技术问题",
        _ => "其它",
    }
}

impl ManagedGroup {
    /// 用全局设置里的旧规则做一个方案（迁移老数据 / 新建时的默认值）。
    pub fn from_legacy(c: &ManagedConfig, name: &str) -> Self {
        Self {
            id: String::new(),
            name: name.into(),
            sites: Vec::new(),
            stopped: Vec::new(),
            paused: false,
            autonomy: default_autonomy(),
            brain: c.brain.clone(),
            model: c.model.clone(),
            fallback_brain: c.fallback_brain.clone(),
            fallback_model: c.fallback_model.clone(),
            new_content: c.new_content.clone(),
            daily_changes: c.daily_changes,
            tokens_per_week: c.tokens_per_week,
            goal: String::new(),
            actions: all_actions(),
            guidelines: String::new(),
            topics: String::new(),
            metric: default_metric(),
            min_page_chars: default_min_page(),
            created_at: 0,
        }
    }
    pub fn allows(&self, change: &str) -> bool {
        // 计划里偶尔出现的细分类型（title / meta_description）都算「改标题和描述」
        let c = match change { "title" | "meta_description" => "meta", other => other };
        self.actions.iter().any(|a| a == c)
    }
    pub fn topic_list(&self) -> Vec<String> {
        self.topics.lines().map(|l| l.trim().trim_start_matches(['-', '*', '•']).trim().to_string()).filter(|l| !l.is_empty()).collect()
    }
    /// 写进 AI 指令的「这个方案的运营方式」：目标、允许的动作、选题、内容准则。全为空时返回空串。
    pub fn brief(&self) -> String {
        let mut out = String::new();
        if !self.goal.trim().is_empty() {
            out.push_str(&format!("【运营目标】\n{}\n\n", self.goal.trim()));
        }
        out.push_str(&format!("【衡量标准】主要看{}；效果复盘按它判断改动有没有用。\n\n", metric_label(&self.metric)));
        if self.min_page_chars > 0 {
            out.push_str(&format!(
                "【页面厚度】新页面正文不少于 {} 字（不算标题、导航和代码）；不同类型参考：完整教程 / 指南 2000–4000 字并配真实截图，对比 / 评测 2500 字以上并有对比表，单个问题解答 1200–2000 字，术语 / 概念解释 800–1500 字。长度跟着把问题讲完整走，不为凑字数加内容；撑不起的选题换掉或并进已有页面。\n\n",
                self.min_page_chars
            ));
        }
        let allowed: Vec<&str> = ACTIONS.iter().filter(|a| self.allows(a)).map(|a| action_label(a)).collect();
        let denied: Vec<&str> = ACTIONS.iter().filter(|a| !self.allows(a)).map(|a| action_label(a)).collect();
        out.push_str(&format!("【允许的动作】{}{}\n\n", allowed.join("、"), if denied.is_empty() { String::new() } else { format!("（不要做：{}）", denied.join("、")) }));
        let topics = self.topic_list();
        if !topics.is_empty() {
            out.push_str(&format!("【选题清单】新内容优先从这里选（已经有页面覆盖的跳过）：\n{}\n\n", topics.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n")));
        }
        if !self.guidelines.trim().is_empty() {
            out.push_str(&format!("【内容准则】（运营人员制定，写作和修改都必须遵守）\n{}\n\n", self.guidelines.trim()));
        }
        out
    }
    /// 这个站点现在该不该自动运行（方案暂停或站点单独停止都不跑）。
    pub fn active_for(&self, site_id: &str) -> bool {
        !self.paused && !self.stopped.iter().any(|x| x == site_id)
    }
}

impl ManagedConfig {
    pub fn group_of(&self, site_id: &str) -> Option<&ManagedGroup> {
        self.groups.iter().find(|g| g.sites.iter().any(|x| x == site_id))
    }
    /// 站点用的托管规则：所在方案的；不在任何方案里（老数据还没迁移）就用全局旧规则。
    pub fn rules_for(&self, site_id: &str) -> ManagedGroup {
        self.group_of(site_id).cloned().unwrap_or_else(|| ManagedGroup::from_legacy(self, "默认方案"))
    }
}

impl Default for ManagedConfig {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("managed")
}

pub fn load(data_dir: &Path) -> ManagedConfig {
    std::fs::read(dir(data_dir).join("config.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(data_dir: &Path, c: &ManagedConfig) -> Result<(), String> {
    let d = dir(data_dir);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let raw = serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?;
    let tmp = d.join("config.json.tmp");
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, d.join("config.json")).map_err(|e| e.to_string())
}

/// 老站点迁移（只做一次）：第 1–4 档、不在任何方案里的站点按档位和模型归进方案，
/// 模型和每周模型用量沿用站点原来的设置。返回 (站点 id, 归进的方案 id) 列表。
pub fn migrate_levels(c: &mut ManagedConfig, sites: &[crate::sites::Site]) -> Vec<(String, String)> {
    if c.levels_migrated {
        return Vec::new();
    }
    c.levels_migrated = true;
    let mut out = Vec::new();
    let todo: Vec<&crate::sites::Site> = sites.iter().filter(|s| s.ops.level < 5 && c.group_of(&s.id).is_none()).collect();
    for s in todo {
        let autonomy = match s.ops.level { 1 | 2 => "suggest", 3 => "review", _ => "auto" };
        let model = if s.ops.brain == "codex" { ("claude".to_string(), String::new()) } else { (s.ops.brain.clone(), s.ops.model.clone()) };
        let base = autonomy_label(autonomy).to_string();
        let found = c.groups.iter().position(|g| g.autonomy == autonomy && g.brain == model.0 && g.model == model.1 && g.name.starts_with(&base));
        let i = match found {
            Some(i) => i,
            None => {
                let mut g = ManagedGroup::from_legacy(c, &base);
                g.id = uuid::Uuid::new_v4().to_string();
                g.created_at = crate::now_secs();
                g.autonomy = autonomy.into();
                g.brain = model.0.clone();
                g.model = model.1.clone();
                g.fallback_brain.clear();
                g.fallback_model.clear();
                g.tokens_per_week = s.ops.budget.tokens_per_week;
                if c.groups.iter().any(|x| x.name == g.name) {
                    g.name = format!("{base} · {}", if model.1.is_empty() { &model.0 } else { &model.1 });
                }
                if autonomy == "auto" {
                    // 第 4 档：只把白名单里的改动自动上线，新页面人审人发
                    let mut acts: Vec<String> = s.ops.whitelist.iter().filter_map(|w| match w.as_str() { "title" | "meta_description" => Some("meta"), "internal_links" => Some("internal_links"), "sitemap" => Some("technical"), _ => None }).map(str::to_string).collect();
                    acts.sort();
                    acts.dedup();
                    g.actions = if acts.is_empty() { vec!["meta".into()] } else { acts };
                    g.new_content = "review".into();
                }
                c.groups.push(g);
                c.groups.len() - 1
            }
        };
        c.groups[i].sites.push(s.id.clone());
        c.groups[i].tokens_per_week = c.groups[i].tokens_per_week.max(s.ops.budget.tokens_per_week);
        out.push((s.id.clone(), c.groups[i].id.clone()));
    }
    out
}

pub fn validate_group(g: &ManagedGroup) -> Result<(), String> {
    if g.name.trim().is_empty() {
        return Err("给这个托管方案起个名字".into());
    }
    if g.name.chars().count() > 40 {
        return Err("方案名太长了".into());
    }
    for b in [&g.brain, &g.fallback_brain] {
        if b == "codex" {
            return Err("托管不能用 Codex（它没有逐条确认的钩子），请选 Claude、Grok 或 API 服务".into());
        }
    }
    if g.brain.is_empty() {
        return Err("请选择主模型".into());
    }
    if !g.fallback_brain.is_empty() && g.fallback_brain == g.brain && g.fallback_model == g.model {
        return Err("备用模型和主模型一样，起不到备用的作用".into());
    }
    if !matches!(g.new_content.as_str(), "auto" | "review") {
        return Err("新内容发布方式只能是自动发布或审核后发布".into());
    }
    if g.daily_changes > 50 {
        return Err("每天最多自动上线的条数超出范围（0–50）".into());
    }
    if !METRICS.contains(&g.metric.as_str()) {
        return Err(format!("未知的衡量标准：{}", g.metric));
    }
    if !AUTONOMY.contains(&g.autonomy.as_str()) {
        return Err(format!("未知的自主程度：{}", g.autonomy));
    }
    if g.min_page_chars > 10000 {
        return Err("新页面正文下限超出范围（0–10000 字）".into());
    }
    if g.actions.is_empty() {
        return Err("至少允许一种动作，否则托管什么也不会做".into());
    }
    if let Some(a) = g.actions.iter().find(|a| !ACTIONS.contains(&a.as_str())) {
        return Err(format!("未知的动作：{a}"));
    }
    if g.guidelines.chars().count() > 8000 || g.topics.chars().count() > 8000 || g.goal.chars().count() > 500 {
        return Err("目标 / 内容准则 / 选题清单太长了".into());
    }
    Ok(())
}

/// 老数据迁移：已经是第 5 档、但不在任何方案里的站点，放进一个「默认方案」（沿用旧的全局规则）。
/// 返回是否改动了配置。
pub fn adopt_orphans(c: &mut ManagedConfig, managed_sites: &[String]) -> bool {
    let orphans: Vec<String> = managed_sites.iter().filter(|id| c.group_of(id).is_none()).cloned().collect();
    if orphans.is_empty() {
        return false;
    }
    let mut g = ManagedGroup::from_legacy(c, "默认方案");
    g.id = uuid::Uuid::new_v4().to_string();
    g.created_at = crate::now_secs();
    g.sites = orphans;
    c.groups.push(g);
    true
}


// ---------------------------------------------------------------------------
// 动态（所有托管站点的时间线）
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct Activity {
    pub ts: u64,
    pub site_id: String,
    pub site_name: String,
    /// planned | shipped | proposed | rolled_back | failed | watch_ok | watch_failed | budget | fallback | digest
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub item_id: String,
    /// 关联改动的动作类型和来源（见 Opportunity.change / source），动态里显示「为什么做」。
    #[serde(default)]
    pub change: String,
    #[serde(default)]
    pub source: String,
}

static LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn log(data_dir: &Path, a: Activity) {
    let _g = LOG_LOCK.lock().unwrap();
    let d = dir(data_dir);
    let _ = std::fs::create_dir_all(&d);
    if let Ok(line) = serde_json::to_string(&a) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(d.join("activity.jsonl")) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// 最近的动态（新的在前）。
pub fn activity(data_dir: &Path, limit: usize) -> Vec<Activity> {
    let Ok(text) = std::fs::read_to_string(dir(data_dir).join("activity.jsonl")) else {
        return Vec::new();
    };
    let mut v: Vec<Activity> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    v.reverse();
    v.truncate(limit);
    v
}

pub fn digests_dir(data_dir: &Path) -> PathBuf {
    dir(data_dir).join("digests")
}

// ---------------------------------------------------------------------------
// 全托管的自动上线判定（比第 4 档白名单宽，但有硬边界）
// ---------------------------------------------------------------------------

/// 永远不自动改的文件：依赖、构建 / 部署配置、Worker / 函数代码、脚本、CI。
pub fn forbidden_path(path: &str) -> bool {
    let p = path.trim_start_matches("./");
    let name = p.rsplit('/').next().unwrap_or(p);
    let first = p.split('/').next().unwrap_or("");
    matches!(
        name,
        "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "bun.lock" | "bun.lockb" | "wrangler.toml" | "wrangler.json"
            | "wrangler.jsonc" | "pilot.json" | ".env" | ".dev.vars" | "_headers" | "_redirects" | "_worker.js" | "tsconfig.json"
    ) || name.starts_with("astro.config")
        || name.starts_with("vite.config")
        || name.starts_with("next.config")
        || name.starts_with("svelte.config")
        || matches!(first, ".github" | "worker" | "workers" | "functions" | "server" | "scripts" | "node_modules" | "cmd" | "internal" | "deploy" | "bin")
        || matches!(name, "go.mod" | "go.sum" | "Makefile" | "Dockerfile")
        || ["js", "ts", "mjs", "cjs", "jsx", "tsx", "py", "sh", "rs", "go", "toml", "yml", "yaml"].iter().any(|e| name.ends_with(&format!(".{e}")))
}

/// 推荐 / 邀请 / 返佣相关的内容：改动里只要删改了带这些标记的行，就不自动上线。
pub fn touches_money(line: &str) -> bool {
    let l = line.to_lowercase();
    ["ref=", "referral", "invite", "affiliate", "/go/", "promo", "coupon", "邀请", "返佣", "推荐码", "优惠码"].iter().any(|k| l.contains(k))
}

/// 全托管的判定：返回不能自动上线的原因（空＝可以）。
/// 公开内容里不该出现的内部痕迹（开发注释、占位、内部说明）。只认很明确的标记，避免误伤正文。
const LEAK_MARKERS: &[&str] = &["TODO", "FIXME", "TBD", "lorem ipsum", "placeholder", "待补图", "待补充", "占位图", "占位文字", "上线前替换", "此处插入", "本文由 AI 生成", "按提示词", "via.placeholder.com", "dummyimage.com", "placehold.co"];

/// 文字量：汉字按字、其它按词（粗算，只用于判断「新页面是不是太薄」）。
pub fn text_units(s: &str) -> usize {
    let cjk = s.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c) || ('\u{3040}'..='\u{30ff}').contains(c) || ('\u{ac00}'..='\u{d7af}').contains(c)).count();
    let words = s.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| w.len() >= 2).count();
    cjk + words
}


fn is_page_file(f: &str) -> bool {
    ["md", "mdx", "markdown", "html", "htm", "astro"].iter().any(|e| f.ends_with(&format!(".{e}")))
}

/// 去掉 frontmatter 分隔、HTML / JSX 标签、代码围栏后的正文文字。
fn prose_of(line: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in line.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// 一个文件在差异里的改动。
#[derive(Default)]
struct FileDiff {
    file: String,
    new_file: bool,
    deleted: bool,
    added: Vec<String>,
    removed: Vec<String>,
}

fn split_diff(diff: &str) -> Vec<FileDiff> {
    let mut out: Vec<FileDiff> = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            out.push(FileDiff { file: rest.split(" b/").nth(1).unwrap_or(rest).to_string(), ..Default::default() });
            continue;
        }
        let Some(f) = out.last_mut() else { continue };
        if line.starts_with("new file mode") {
            f.new_file = true;
        } else if line.starts_with("deleted file mode") {
            f.deleted = true;
        } else if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") || line.starts_with("index ") {
        } else if let Some(a) = line.strip_prefix('+') {
            f.added.push(a.to_string());
        } else if let Some(r) = line.strip_prefix('-') {
            f.removed.push(r.to_string());
        }
    }
    out
}

/// 内容数据文件：放在 data / content 目录下的 .ts / .js（很多 Astro 站把文章写成数据文件）。
/// 只有改动的每一行都是「字符串、对象字面量、本地数据模块的导入导出」时才算内容，见 `data_line`。
fn data_file(path: &str) -> bool {
    let p = path.trim_start_matches("./");
    let name = p.rsplit('/').next().unwrap_or(p);
    let first = p.split('/').next().unwrap_or("");
    let in_data = p.split('/').rev().skip(1).any(|seg| matches!(seg, "data" | "content" | "contents" | "posts" | "articles"));
    in_data
        && ["ts", "js", "mjs"].iter().any(|e| name.ends_with(&format!(".{e}")))
        && !name.ends_with(".d.ts")
        && !name.contains(".config.")
        && !matches!(first, ".github" | "worker" | "workers" | "functions" | "server" | "scripts" | "node_modules" | "cmd" | "internal" | "deploy" | "bin")
}

/// 去掉一行里的字符串字面量，返回（剩下的代码部分，字符串内容）。模板字符串里有 `${` 返回 None（是代码）。
fn split_strings(line: &str) -> Option<(String, String)> {
    let (mut code, mut text) = (String::new(), String::new());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' || c == '"' || c == '`' {
            let mut closed = false;
            while let Some(d) = chars.next() {
                if d == '\\' {
                    if let Some(e) = chars.next() {
                        text.push(e);
                    }
                } else if d == c {
                    closed = true;
                    break;
                } else if c == '`' && d == '$' && chars.peek() == Some(&'{') {
                    return None;
                } else {
                    text.push(d);
                }
            }
            if !closed && c != '`' {
                return None;
            }
            code.push_str("\"\"");
            text.push(' ');
        } else {
            code.push(c);
        }
    }
    Some((code, text))
}

/// 这一行是不是纯数据：对象键、字符串、数字、括号逗号；或者从本地文件导入 / 导出一个数据常量、展开一个数组。
fn data_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
        return true;
    }
    let Some((code, _)) = split_strings(t) else { return false };
    let code = code.trim();
    // import { a, b } from './x'; / import type { T } from './x';（只许本地相对路径）
    if let Some(rest) = code.strip_prefix("import ") {
        let from_local = t.contains("from './") || t.contains("from \"./");
        let names = rest.trim_start_matches("type ").trim();
        return from_local && names.starts_with('{') && names.split('}').next().is_some_and(|n| n.chars().all(|c| c.is_alphanumeric() || "_$, {".contains(c)));
    }
    // export const xxx: Type[] = [ / export const xxx = {
    if let Some(rest) = code.strip_prefix("export const ") {
        let Some((lhs, rhs)) = rest.split_once('=') else { return false };
        let ok_lhs = lhs.trim().chars().all(|c| c.is_alphanumeric() || "_$:[]<> ".contains(c));
        return ok_lhs && matches!(rhs.trim(), "[" | "{" | "[];" | "{};");
    }
    if code.contains("=>") || code.contains('(') || code.contains('=') || code.contains('`') {
        return false;
    }
    if !code.chars().all(|c| c.is_alphanumeric() || "_$\"{}[],:;. -+".contains(c)) {
        return false;
    }
    // 剩下的标识符只能是对象键或 true / false / null / 展开的数组名
    const CODE_WORDS: &[&str] = &["function", "return", "await", "async", "new", "eval", "require", "process", "import", "export", "fetch", "window", "document", "globalThis", "class", "if", "for", "while", "delete", "throw"];
    !code.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).any(|w| CODE_WORDS.contains(&w))
}

/// 自动上线时，单次改动已有文件的行数上限；新内容文件按新内容规则和正文下限判断，另有一个整体上限。
const MAX_CHANGED_LINES: usize = 400;
const MAX_NEW_FILE_LINES: usize = 3000;

/// `min_page`：新页面正文下限（字，0＝不检查），见 ManagedGroup.min_page_chars。
pub fn managed_verdict(diff: &str, allow_new_content: bool, min_page: usize) -> Vec<String> {
    let mut reasons = Vec::new();
    let content_ext = ["md", "mdx", "markdown", "html", "htm", "astro", "njk", "liquid", "hbs", "vue", "svelte", "json", "svg", "png", "jpg", "jpeg", "webp", "avif"];
    let (mut changed, mut new_lines) = (0usize, 0usize);
    for f in split_diff(diff) {
        let file = &f.file;
        let data = data_file(file) && f.added.iter().chain(&f.removed).all(|l| data_line(l));
        if data_file(file) && !data {
            reasons.push(format!("{file}：数据文件里有代码改动（不只是文字和数据），留给你确认"));
        } else if !data && forbidden_path(file) {
            reasons.push(format!("{file}：依赖、配置或代码文件不自动改"));
        } else if !data && !content_ext.iter().any(|e| file.ends_with(&format!(".{e}"))) {
            reasons.push(format!("{file}：不是页面内容文件"));
        }
        if f.deleted {
            reasons.push(format!("{file}：删除文件不自动执行"));
            continue;
        }
        if f.new_file {
            new_lines += f.added.len();
            if !allow_new_content {
                reasons.push(format!("{file}：新内容需要运营人员确认后发布"));
            }
        } else {
            changed += f.added.len() + f.removed.len();
        }
        for r in &f.removed {
            if touches_money(r) {
                reasons.push(format!("{file}：改动了推荐 / 邀请相关的内容"));
            }
        }
        if !f.new_file && f.added.iter().any(|a| touches_money(a)) {
            reasons.push(format!("{file}：新增了推荐 / 邀请相关的内容"));
        }
        let page = is_page_file(file) || data;
        if page {
            // 页面文件看整行；数据文件只看字符串里的文字
            let texts: Vec<String> = f.added.iter().map(|a| if data { split_strings(a).map(|x| x.1).unwrap_or_default() } else { a.clone() }).collect();
            for t in &texts {
                let lower = t.to_lowercase();
                if let Some(m) = LEAK_MARKERS.iter().find(|m| lower.contains(&m.to_lowercase())) {
                    reasons.push(format!("{file}：正文里像是混进了内部说明或占位（「{m}」），留给你确认"));
                }
            }
            if f.new_file && min_page > 0 {
                let mut n = 0;
                let mut in_frontmatter = false;
                for t in &texts {
                    let t = t.trim();
                    if !data && t == "---" {
                        in_frontmatter = !in_frontmatter;
                    } else if data || (!in_frontmatter && !t.starts_with("```") && !t.starts_with("import ")) {
                        n += text_units(&prose_of(t));
                    }
                }
                if n < min_page {
                    reasons.push(format!("{file}：新页面正文约 {n} 字，低于这个方案的下限 {min_page} 字，可能讲不完整、容易被当成低质页面，留给你确认（术语解释这类短页可以直接放行）"));
                }
            }
        }
    }
    if changed > MAX_CHANGED_LINES {
        reasons.push(format!("改动已有文件 {changed} 行，超过自动上线的上限（{MAX_CHANGED_LINES} 行）"));
    }
    if new_lines > MAX_NEW_FILE_LINES {
        reasons.push(format!("新增文件共 {new_lines} 行，超过自动上线的上限（{MAX_NEW_FILE_LINES} 行）"));
    }
    reasons.sort();
    reasons.dedup();
    reasons
}

/// 计划里的一条任务（模型输出的 JSON）。
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct PlanTask {
    pub title: String,
    #[serde(default)]
    pub page: String,
    /// meta | content | internal_links | new_content | technical
    #[serde(default)]
    pub change: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub priority: u32,
    /// data | gap | topic | positioning | health
    #[serde(default)]
    pub source: String,
    /// 目标查询词
    #[serde(default)]
    pub query: String,
}

/// 从模型回复里取出计划 JSON（```json … ``` 代码块，或回复里第一个 JSON 数组）。
/// 计划回复：`{"positioning": "...", "tasks": [...]}`，也兼容只有任务数组的老格式。
pub fn parse_plan_full(reply: &str) -> Result<(String, Vec<PlanTask>), String> {
    let body = if let Some(i) = reply.find("```json") {
        reply[i + 7..].split("```").next().unwrap_or("").to_string()
    } else {
        let a = reply.find(['{', '[']).ok_or("回复里没有计划（缺少 JSON）")?;
        let b = reply.rfind(['}', ']']).ok_or("回复里没有计划（缺少 JSON）")?;
        reply[a..=b].to_string()
    };
    let v: serde_json::Value = serde_json::from_str(body.trim()).map_err(|e| format!("计划 JSON 无法解析：{e}"))?;
    let positioning = v["positioning"].as_str().unwrap_or_default().to_string();
    let tasks = parse_plan(&format!("```json\n{}\n```", if v.is_array() { v.to_string() } else { v["tasks"].to_string() }))?;
    Ok((positioning, tasks))
}

/// 计划回复里给运营人员的说明（notes 字段，没有就是空串）。
pub fn parse_plan_notes(reply: &str) -> String {
    let body = match reply.find("```json") {
        Some(i) => reply[i + 7..].split("```").next().unwrap_or("").to_string(),
        None => return String::new(),
    };
    serde_json::from_str::<serde_json::Value>(body.trim()).ok().and_then(|v| v["notes"].as_str().map(|s| s.trim().chars().take(1000).collect())).unwrap_or_default()
}

pub fn parse_plan(reply: &str) -> Result<Vec<PlanTask>, String> {
    let body = if let Some(i) = reply.find("```json") {
        let rest = &reply[i + 7..];
        rest.split("```").next().unwrap_or("")
    } else {
        let a = reply.find('[').ok_or("回复里没有计划（缺少 JSON）")?;
        let b = reply.rfind(']').ok_or("回复里没有计划（缺少 JSON）")?;
        &reply[a..=b]
    };
    let v: serde_json::Value = serde_json::from_str(body.trim()).map_err(|e| format!("计划 JSON 无法解析：{e}"))?;
    let arr = if v.is_array() { v } else { v["tasks"].clone() };
    let mut tasks: Vec<PlanTask> = serde_json::from_value(arr).map_err(|e| format!("计划格式不对：{e}"))?;
    tasks.retain(|t| !t.title.trim().is_empty());
    for t in tasks.iter_mut() {
        if !matches!(t.change.as_str(), "meta" | "content" | "internal_links" | "new_content" | "technical") {
            t.change = "content".into();
        }
        t.title = t.title.chars().take(80).collect();
        if !matches!(t.source.as_str(), "data" | "gap" | "topic" | "positioning" | "health") {
            t.source = if t.change == "technical" { "health" } else if t.change == "new_content" { "positioning" } else { "data" }.into();
        }
        t.query = t.query.chars().take(80).collect();
    }
    tasks.truncate(12);
    Ok(tasks)
}

#[cfg(test)]
mod group_tests {
    use super::*;

    #[test]
    fn orphaned_managed_sites_move_into_a_default_group_with_the_old_rules() {
        let mut c = ManagedConfig { daily_changes: 7, ..Default::default() };
        assert!(adopt_orphans(&mut c, &["a".into(), "b".into()]));
        assert_eq!(c.groups.len(), 1);
        assert_eq!(c.groups[0].sites, vec!["a", "b"]);
        assert_eq!(c.groups[0].daily_changes, 7);
        assert!(!adopt_orphans(&mut c, &["a".into()]), "已在方案里的不再迁移");
        assert_eq!(c.rules_for("a").daily_changes, 7);
    }

    #[test]
    fn plan_brief_carries_goal_actions_topics_and_guidelines() {
        let mut g = ManagedGroup::from_legacy(&ManagedConfig::default(), "x");
        g.goal = "提升中文页的 Google 流量".into();
        g.actions = vec!["meta".into(), "new_content".into()];
        g.topics = "- gate kyc\n\n• gate 手续费\n".into();
        g.guidelines = "用简体中文".into();
        let b = g.brief();
        assert!(b.contains("提升中文页的 Google 流量"));
        assert!(b.contains("改标题和描述、写新页面"));
        assert!(b.contains("不要做：改进已有页面内容"));
        assert!(b.contains("- gate kyc\n- gate 手续费"));
        assert!(b.contains("用简体中文"));
        assert!(g.allows("title") && !g.allows("content"));
        assert!(validate_group(&ManagedGroup { actions: vec![], ..g.clone() }).is_err());
        assert!(validate_group(&ManagedGroup { actions: vec!["deploy".into()], ..g }).is_err());
    }

    #[test]
    fn thin_new_pages_and_leaked_internal_notes_are_not_auto_shipped() {
        let thin = "diff --git a/src/content/a.md b/src/content/a.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/a.md\n@@ -0,0 +1,4 @@\n+---\n+title: 很短的页面\n+---\n+只有一句话。\n";
        let r = managed_verdict(thin, true, 1500);
        assert!(r.iter().any(|x| x.contains("低于这个方案的下限")), "{r:?}");
        assert!(managed_verdict(thin, true, 0).is_empty(), "下限 0＝不检查");
        let body: String = "这是一段足够长的正文，讲清楚了具体步骤、注意事项和常见错误。".repeat(60);
        let full = format!("diff --git a/src/content/b.md b/src/content/b.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/b.md\n@@ -0,0 +1,4 @@\n+---\n+title: 完整的页面\n+---\n+{body}\n");
        assert!(managed_verdict(&full, true, 1500).is_empty(), "{:?}", managed_verdict(&full, true, 1500));
        let leak = "diff --git a/src/content/c.md b/src/content/c.md\n--- a/src/content/c.md\n+++ b/src/content/c.md\n@@ -1 +1 @@\n-旧\n+步骤三（待补图）点击确认\n";
        assert!(managed_verdict(leak, true, 1500).iter().any(|x| x.contains("待补图")));
        assert_eq!(text_units("Gate KYC 认证"), 4);
    }

    #[test]
    fn plan_replies_carry_positioning_and_tasks() {
        let r = "好的\n```json\n{\"positioning\":\"面向中文用户的交易所教程站\",\"tasks\":[{\"title\":\"写 KYC 教程\",\"change\":\"new_content\"}]}\n```";
        let (p, t) = parse_plan_full(r).unwrap();
        assert_eq!(p, "面向中文用户的交易所教程站");
        assert_eq!(t[0].change, "new_content");
        assert!(parse_plan_notes(r).is_empty());
        let withn = "```json\n{\"notes\":\"目标写了不写新页面，本周按目标来\",\"tasks\":[]}\n```";
        assert_eq!(parse_plan_notes(withn), "目标写了不写新页面，本周按目标来");
        let (p, t) = parse_plan_full("[{\"title\":\"x\",\"change\":\"meta\"}]").unwrap();
        assert!(p.is_empty() && t.len() == 1);
    }

    #[test]
    fn old_levels_move_into_plans_once() {
        let mk = |id: &str, level: u8| {
            let mut s: crate::sites::Site = serde_json::from_value(serde_json::json!({ "id": id, "name": id, "domain": "", "source": "local", "local_dir": "/tmp", "created_at": 0, "updated_at": 0 })).unwrap();
            s.ops.level = level;
            s
        };
        let mut four = mk("d", 4);
        four.ops.whitelist = vec!["title".into(), "internal_links".into()];
        let sites = vec![mk("a", 1), mk("b", 2), mk("c", 3), four, mk("e", 5)];
        let mut c = ManagedConfig::default();
        let moved = migrate_levels(&mut c, &sites);
        assert_eq!(moved.len(), 4, "第 5 档不在这里迁移");
        let g_of = |id: &str| c.group_of(id).unwrap().clone();
        assert_eq!(g_of("a").id, g_of("b").id);
        assert_eq!(g_of("a").autonomy, "suggest");
        assert_eq!(g_of("c").autonomy, "review");
        assert_eq!(g_of("d").autonomy, "auto");
        assert_eq!(g_of("d").actions, vec!["internal_links", "meta"]);
        assert_eq!(g_of("d").new_content, "review");
        assert!(migrate_levels(&mut c, &sites).is_empty(), "只迁移一次");
        assert_eq!(level_of("review"), 3);
    }

    #[test]
    fn a_stopped_site_or_paused_group_does_not_run() {
        let mut g = ManagedGroup::from_legacy(&ManagedConfig::default(), "x");
        g.sites = vec!["a".into(), "b".into()];
        g.stopped = vec!["b".into()];
        assert!(g.active_for("a"));
        assert!(!g.active_for("b"));
        g.paused = true;
        assert!(!g.active_for("a"));
        assert!(validate_group(&ManagedGroup { name: " ".into(), ..g.clone() }).is_err());
        assert!(validate_group(&ManagedGroup { brain: "codex".into(), ..g }).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe() {
        let c = ManagedConfig::default();
        assert!(!c.paused);
        assert_eq!(c.new_content, "review");
        assert_eq!(c.daily_changes, 3);
        assert!(c.digest);
        assert!(validate_group(&ManagedGroup::from_legacy(&c, "x")).is_ok());
        let g = ManagedGroup::from_legacy(&c, "x");
        assert!(validate_group(&ManagedGroup { fallback_brain: "codex".into(), ..g.clone() }).is_err());
        assert!(validate_group(&ManagedGroup { fallback_brain: "claude".into(), ..g }).is_err(), "备用和主模型一样");
    }

    #[test]
    fn verdict_blocks_code_config_money_and_optionally_new_pages() {
        let ok = "diff --git a/src/content/a.md b/src/content/a.md\n--- a/src/content/a.md\n+++ b/src/content/a.md\n@@ -3 +3,2 @@\n-旧段落\n+新段落，补充了水温的说明。\n+还加了一段。\n";
        assert!(managed_verdict(ok, false, 1500).is_empty());
        let code = "diff --git a/worker/index.ts b/worker/index.ts\n--- a/worker/index.ts\n+++ b/worker/index.ts\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(!managed_verdict(code, true, 1500).is_empty());
        let cfg = "diff --git a/package.json b/package.json\n--- a/package.json\n+++ b/package.json\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(!managed_verdict(cfg, true, 1500).is_empty());
        let money = "diff --git a/src/pages/a.astro b/src/pages/a.astro\n--- a/src/pages/a.astro\n+++ b/src/pages/a.astro\n@@ -9 +9 @@\n-<a href=\"/go/gate\">邀请码 ABC</a>\n+<a href=\"/go/gate\">邀请码 XYZ</a>\n";
        assert!(managed_verdict(money, true, 1500).iter().any(|r| r.contains("推荐")));
        let newpage = "diff --git a/src/content/new.md b/src/content/new.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/new.md\n@@ -0,0 +1 @@\n+# 新页面\n";
        assert!(managed_verdict(newpage, false, 1500).iter().any(|r| r.contains("新内容")));
        // 允许自动发布新页面时，只剩「低于正文下限」这一条（这个测试页只有一行）
        assert!(managed_verdict(newpage, true, 1500).iter().all(|r| r.contains("低于这个方案的下限")));
    }

    #[test]
    fn data_files_with_only_text_count_as_content() {
        // 已有数据文件：加导入、加一段文字、加一个 slug、展开一个数组
        let edit = "diff --git a/src/data/articles.ts b/src/data/articles.ts\n--- a/src/data/articles.ts\n+++ b/src/data/articles.ts\n@@ -5 +5,2 @@\n+import { transitionArticles } from './articles-transitions';\n+    'ai-video-transition-prompts',\n+          '想把两个镜头接起来（遮挡、甩镜），可以看本页末尾的转场指南。',\n+  ...transitionArticles,\n";
        assert!(managed_verdict(edit, false, 1500).is_empty(), "{:?}", managed_verdict(edit, false, 1500));
        // 新的数据文件：按新内容规则和正文下限判断
        let body: String = (0..40).map(|_| "+      '遮挡转场要让前景物体把画面整个挡住，再在被挡住的那一帧切到下一段素材，两段的运动方向要一致。',\n").collect();
        let newfile = format!("diff --git a/src/data/articles-x.ts b/src/data/articles-x.ts\nnew file mode 100644\n--- /dev/null\n+++ b/src/data/articles-x.ts\n@@ -0,0 +1,50 @@\n+import type {{ ArticleSeed }} from './article-types';\n+export const xArticles: ArticleSeed[] = [\n+  {{\n+    slug: 'x',\n+    datePublished: '2026-10-22',\n+    body: [\n{body}+    ],\n+  }},\n+];\n");
        assert!(managed_verdict(&newfile, true, 1500).is_empty(), "{:?}", managed_verdict(&newfile, true, 1500));
        assert!(managed_verdict(&newfile, false, 1500).iter().any(|r| r.contains("新内容")));
        // 数据文件里混了代码：函数、模板字符串插值、外部导入
        for bad in ["+export function pick(a) { return a; }", "+  title: `${name} 指南`,", "+import fs from 'node:fs';", "+  run: () => fetch('/x'),", "+const x = process.env.TOKEN;"] {
            let d = format!("diff --git a/src/data/a.ts b/src/data/a.ts\n--- a/src/data/a.ts\n+++ b/src/data/a.ts\n@@ -1 +1 @@\n{bad}\n");
            assert!(managed_verdict(&d, true, 0).iter().any(|r| r.contains("代码改动")), "{bad}");
        }
        // 不在 data / content 目录下的 .ts 仍然是代码
        let code = "diff --git a/src/lib/seo.ts b/src/lib/seo.ts\n--- a/src/lib/seo.ts\n+++ b/src/lib/seo.ts\n@@ -1 +1 @@\n+  title: 'x',\n";
        assert!(managed_verdict(code, true, 0).iter().any(|r| r.contains("代码文件")));
    }

    #[test]
    fn plans_parse_from_fenced_or_bare_json() {
        let r = "本周计划如下：\n```json\n[{\"title\":\"改 /zh/ 标题\",\"page\":\"https://x.com/zh/\",\"change\":\"meta\",\"why\":\"点击率低\",\"priority\":1},{\"title\":\"\",\"change\":\"meta\"},{\"title\":\"写一篇费率对比\",\"change\":\"weird\"}]\n```\n";
        let t = parse_plan(r).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].change, "meta");
        assert_eq!(t[1].change, "content");
        assert_eq!(parse_plan("结论 [{\"title\":\"a\"}] 完").unwrap().len(), 1);
        assert!(parse_plan("没有计划").is_err());
    }
}
