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
    /// 代码改动（很多站点的文章、页面写在代码里）：review＝留给你确认；small＝小改动自动上线；all＝都自动上线。
    /// 依赖、构建 / 部署配置、Worker / 服务端、读环境变量或调外部服务的代码，任何一档都不自动上线。
    #[serde(default = "default_code")]
    pub code_changes: String,
    /// 自动上线前用判断模型（Jev）做内容质检；接上 TypeSafe 才生效，见 jev_checks
    #[serde(default = "yes")]
    pub jev_gate: bool,
    /// 判断模型认为和站点定位基本无关的搜索词，扫描时不交给 AI 处理（看板里照样列出）
    #[serde(default = "yes")]
    pub skip_offtopic: bool,
    #[serde(default)]
    pub created_at: u64,
}

fn default_code() -> String {
    "all".into()
}
pub const CODE_CHANGES: &[&str] = &["all", "small", "review"];
/// 「小改动」的上限：代码文件改动的总行数和文件数。
pub const SMALL_CODE_LINES: usize = 80;
pub const SMALL_CODE_FILES: usize = 5;

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
            code_changes: default_code(),
            jev_gate: true,
            skip_offtopic: true,
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
        if self.min_page_chars > 0 {
            out.push_str(&format!(
                "【页面厚度】新页面正文不少于 {} 字（不算标题、导航和代码）；不同类型参考：完整教程 / 指南 2000–4000 字并配真实截图，对比 / 评测 2500 字以上并有对比表，单个问题解答 1200–2000 字，术语 / 概念解释 800–1500 字。长度跟着把问题讲完整走，不为凑字数加内容；撑不起的选题换掉或并进已有页面。\n\n",
                self.min_page_chars
            ));
        }
        let allowed: Vec<&str> = ACTIONS.iter().filter(|a| self.allows(a)).map(|a| action_label(a)).collect();
        let denied: Vec<&str> = ACTIONS.iter().filter(|a| !self.allows(a)).map(|a| action_label(a)).collect();
        out.push_str(&format!("【允许的动作】{}{}\n\n", allowed.join("、"), if denied.is_empty() { String::new() } else { format!("（不要做：{}）", denied.join("、")) }));
        out.push_str(&match self.code_changes.as_str() {
            "review" => "【代码改动】能用站点现有的数据字段和组件做到，就只改内容和数据；需要改模板、组件或其它代码时可以改，但这类改动要运营人员确认后才上线，改动越小越好。\n\n".to_string(),
            "all" | "" => "【代码改动】文章和页面写在代码里时，可以直接改模板、组件和站点代码来完成任务，改动要集中。不要加依赖、改构建 / 部署配置、改 Worker / 服务端代码，也不要读环境变量或调用外部服务——这些不会自动上线。\n\n".to_string(),
            _ => format!("【代码改动】文章和页面写在代码里时，可以改模板、组件和站点代码来完成任务，但要小而集中：代码文件合计不超过 {SMALL_CODE_LINES} 行、{SMALL_CODE_FILES} 个文件（文章正文这类数据不算），超过就留给运营人员确认。不要加依赖、改构建 / 部署配置、改 Worker / 服务端代码，也不要读环境变量或调用外部服务——这些不会自动上线。\n\n"),
        });
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
    crate::store_file::read_or_quarantine(&dir(data_dir).join("config.json"))
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
    // Codex 可以用：它没有逐条确认的钩子，托管时 Pilot 把它关在工作文件夹里并断网运行（见 agent::codex_sandbox_for），
    // 编辑器里选它会先警告
    if g.brain.is_empty() {
        return Err("请选择主模型".into());
    }
    if !g.fallback_brain.is_empty() && g.fallback_brain == g.brain && g.fallback_model == g.model {
        return Err("备用模型和主模型一样，起不到备用的作用".into());
    }
    if !matches!(g.new_content.as_str(), "auto" | "review") {
        return Err("新内容发布方式只能是自动发布或审核后发布".into());
    }
    if !CODE_CHANGES.contains(&g.code_changes.as_str()) {
        return Err(format!("未知的代码改动设置：{}", g.code_changes));
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

/// 动态最多留这么多条：文件超过两倍就只留最新的这些（界面一次最多也只看 2000 条）。
const ACTIVITY_KEEP: usize = 2000;
/// 文件小于这个字节数时不用数行（一条动态几百字节，远不到 2 × ACTIVITY_KEEP 条）。
const ACTIVITY_CHECK_BYTES: u64 = 512 * 1024;

fn activity_path(data_dir: &Path) -> PathBuf {
    dir(data_dir).join("activity.jsonl")
}

pub fn log(data_dir: &Path, a: Activity) {
    let _g = LOG_LOCK.lock().unwrap();
    let d = dir(data_dir);
    let _ = std::fs::create_dir_all(&d);
    let p = activity_path(data_dir);
    if let Ok(line) = serde_json::to_string(&a) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let _ = writeln!(f, "{line}");
        }
    }
    // 只追加的话文件会一直长，每次读动态都要整个解析一遍：长过头就只留最新的一段
    if std::fs::metadata(&p).map(|m| m.len() > ACTIVITY_CHECK_BYTES).unwrap_or(false) {
        if let Ok(text) = std::fs::read_to_string(&p) {
            let _ = rotate_activity(&p, &text, ACTIVITY_KEEP);
        }
    }
}

/// 超过 2 × keep 行就只留最新的 keep 行（先写临时文件再改名）。返回有没有改写。
fn rotate_activity(p: &Path, text: &str, keep: usize) -> Result<bool, String> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() <= keep * 2 {
        return Ok(false);
    }
    let mut out = lines[lines.len() - keep..].join("\n");
    out.push('\n');
    crate::sites::write_atomic(p, out.as_bytes())?;
    Ok(true)
}

/// 最近的动态（新的在前）。只读文件尾部：一条动态几百字节，按 limit 估算要读多少，不整个文件读进来解析。
pub fn activity(data_dir: &Path, limit: usize) -> Vec<Activity> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(activity_path(data_dir)) else {
        return Vec::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    // 每条按 2KB 估（实际大多几百字节），不够再整个读
    let want = (limit as u64).saturating_mul(2048).max(64 * 1024);
    let mut text = String::new();
    let tail_only = len > want;
    if tail_only && f.seek(SeekFrom::Start(len - want)).is_err() {
        return Vec::new();
    }
    if f.read_to_string(&mut text).is_err() {
        // 从中间切进去可能切在多字节字符里，读不成就整个读
        text.clear();
        if f.seek(SeekFrom::Start(0)).is_err() || f.read_to_string(&mut text).is_err() {
            return Vec::new();
        }
    } else if tail_only {
        // 第一行多半是半截，丢掉
        text = text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
    }
    text.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).take(limit).collect()
}

pub fn digests_dir(data_dir: &Path) -> PathBuf {
    dir(data_dir).join("digests")
}

// ---------------------------------------------------------------------------
// 全托管的自动上线判定（比第 4 档白名单宽，但有硬边界）
// ---------------------------------------------------------------------------


/// 任何代码设置下都不自动改的文件：依赖、构建 / 部署配置、CI、Worker / 函数 / 服务端、脚本、密钥配置，
/// 以及站点代码以外的后端语言。
pub fn hard_forbidden(path: &str) -> bool {
    let p = path.trim_start_matches("./");
    let name = p.rsplit('/').next().unwrap_or(p);
    let first = p.split('/').next().unwrap_or("");
    let segs: Vec<&str> = p.split('/').collect();
    matches!(
        name,
        "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "bun.lock" | "bun.lockb" | "wrangler.toml" | "wrangler.json"
            | "wrangler.jsonc" | "pilot.json" | ".env" | ".dev.vars" | "_headers" | "_redirects" | "_worker.js" | "tsconfig.json" | "middleware.ts" | "middleware.js"
    ) || name.starts_with(".env")
        || name.starts_with("astro.config")
        || name.starts_with("vite.config")
        || name.starts_with("next.config")
        || name.starts_with("svelte.config")
        || name.starts_with("tailwind.config")
        || matches!(first, ".github" | "worker" | "workers" | "functions" | "server" | "scripts" | "node_modules" | "cmd" | "internal" | "deploy" | "bin" | "migrations")
        || segs.iter().any(|s| matches!(*s, "api" | "server" | "functions" | "middleware"))
        || matches!(name, "go.mod" | "go.sum" | "Makefile" | "Dockerfile")
        || ["py", "sh", "rs", "go", "toml", "yml", "yaml", "sql"].iter().any(|e| name.ends_with(&format!(".{e}")))
}

/// 站点代码（模板、组件、样式、前端脚本）：按方案的「代码改动」设置决定能不能自动上线。
fn site_code_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    ["ts", "tsx", "js", "jsx", "mjs", "cjs", "css", "scss", "less"].iter().any(|e| name.ends_with(&format!(".{e}")))
}

/// 模板类文件（.astro / .vue / .svelte）里写着代码，和代码文件一样查危险写法。
fn template_file(path: &str) -> bool {
    ["astro", "vue", "svelte", "jsx", "tsx"].iter().any(|e| path.ends_with(&format!(".{e}")))
}

/// 新加的代码里不该自动上线的写法：读环境变量 / 密钥、调外部服务、执行命令、动态执行、引入新的外部包。
pub fn risky_code(line: &str) -> Option<&'static str> {
    let t = line.trim();
    if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
        return None;
    }
    for (pat, why) in [
        ("process.env", "读环境变量"),
        ("import.meta.env", "读环境变量"),
        ("Astro.locals.runtime", "读运行环境（密钥、数据库）"),
        ("child_process", "执行命令"),
        ("eval(", "动态执行代码"),
        ("new Function(", "动态执行代码"),
        ("fetch(", "请求外部服务"),
        ("XMLHttpRequest", "请求外部服务"),
        ("document.cookie", "读写 cookie"),
        ("dangerouslySetInnerHTML", "直接插入 HTML"),
        ("set:html", "直接插入 HTML"),
        ("<script src=\"http", "引入外部脚本"),
    ] {
        if t.contains(pat) {
            return Some(why);
        }
    }
    // 新引入的外部包（不是本地文件 / 路径别名 / astro: 虚拟模块）
    let import_from = t.strip_prefix("import ").and_then(|r| r.rsplit_once(" from ").map(|x| x.1)).or_else(|| t.strip_prefix("import ").filter(|r| r.starts_with('\'') || r.starts_with('"')));
    if let Some(spec) = import_from {
        let spec = spec.trim().trim_end_matches(';').trim_matches(['\'', '"']);
        let local = spec.starts_with('.') || spec.starts_with('/') || spec.starts_with("~/") || spec.starts_with("@/") || spec.starts_with("$lib") || spec.starts_with("astro:") || spec.starts_with("astro/") || spec == "astro";
        if !local && !spec.is_empty() {
            return Some("引入了外部包");
        }
    }
    if t.contains("require(") {
        return Some("引入了外部包");
    }
    None
}

/// 推荐 / 邀请 / 返佣相关的内容：改动里只要删改了带这些标记的行，就不自动上线。
pub fn touches_money(line: &str) -> bool {
    let l = line.to_lowercase();
    // 「ref=」只认网址参数（?ref= / &ref=），否则每个 href= 都会被当成推荐链接
    ["?ref=", "&ref=", "referral", "invite", "affiliate", "/go/", "promo", "coupon", "邀请", "返佣", "推荐码", "优惠码"].iter().any(|k| l.contains(k))
}

/// 全托管的判定：返回不能自动上线的原因（空＝可以）。
/// 公开内容里不该出现的内部痕迹（开发注释、占位、内部说明）。只认很明确的标记，避免误伤正文。
const LEAK_MARKERS: &[&str] = &["TODO", "FIXME", "TBD", "lorem ipsum", "placeholder", "待补图", "待补充", "占位图", "占位文字", "上线前替换", "此处插入", "本文由 AI 生成", "按提示词", "via.placeholder.com", "dummyimage.com", "placehold.co"];

/// 文字里有没有内部痕迹（开发注释、占位、内部说明），有就返回是哪个标记。
pub fn leak_marker(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    LEAK_MARKERS.iter().copied().find(|m| lower.contains(&m.to_lowercase()))
}

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

/// 这次改动里值得做内容质检的页面文件：新页面，或者正文加了不少（≥ 200 字）的已有页面。删掉的不算。
/// 返回（文件路径，是不是新文件），最多 5 个。
pub fn content_page_files(diff: &str) -> Vec<(String, bool)> {
    split_diff(diff)
        .into_iter()
        .filter(|f| !f.deleted && is_page_file(&f.file))
        .filter(|f| f.new_file || f.added.iter().map(|l| text_units(&prose_of(l))).sum::<usize>() >= 200)
        .map(|f| (f.file, f.new_file))
        .take(5)
        .collect()
}

/// 页面源文件 → （标题，正文纯文字）：去掉 frontmatter（标题从里面的 title: 取）、代码块、import 行和 HTML 标签。
/// 正文截到 `max_chars` 个字符（判断模型单个问题有上下文上限）。
pub fn page_prose(src: &str, max_chars: usize) -> (String, String) {
    let mut title = String::new();
    let mut body = String::new();
    let (mut in_front, mut in_code, mut first) = (false, false, true);
    for line in src.lines() {
        let t = line.trim();
        if first && t == "---" {
            in_front = true;
            first = false;
            continue;
        }
        first = false;
        if in_front {
            if t == "---" {
                in_front = false;
            } else if let Some(v) = t.strip_prefix("title:") {
                title = v.trim().trim_matches(['"', '\'']).to_string();
            }
            continue;
        }
        if t.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || t.starts_with("import ") || t.starts_with("export ") {
            continue;
        }
        let text = prose_of(t);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if title.is_empty() {
            if let Some(h) = text.strip_prefix("# ") {
                title = h.trim().to_string();
            }
        }
        body.push_str(text);
        body.push('\n');
        if body.chars().count() >= max_chars {
            body = body.chars().take(max_chars).collect();
            break;
        }
    }
    (title, body)
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

/// `key: /正则/flags,`：数据里用来匹配文字的正则字面量（比如「哪些旧文提到了旧价格」）。
/// 正则体里不许有没转义的 `/`，结尾只许跟 flags 和逗号——这样 `/a/.test(fetch(x))` 之类的代码过不了。
fn regex_value(t: &str) -> bool {
    let Some((key, rest)) = t.split_once(':') else { return false };
    if key.trim().is_empty() || !key.trim().chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
        return false;
    }
    let Some(body) = rest.trim().strip_prefix('/') else { return false };
    let mut chars = body.chars();
    let mut in_class = false;
    let mut closed = false;
    let mut len = 0;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '[' => in_class = true,
            ']' => in_class = false,
            '/' if !in_class => {
                closed = true;
                break;
            }
            '\n' => return false,
            _ => {}
        }
        len += 1;
    }
    let tail: String = chars.collect();
    let tail = tail.trim().trim_end_matches(',').trim();
    closed && len > 0 && tail.chars().all(|c| "dgimsuvy".contains(c))
}

/// 这一行是不是纯数据：对象键、字符串、数字、括号逗号；或者从本地文件导入 / 导出一个数据常量、展开一个数组。
fn data_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
        return true;
    }
    if regex_value(t) {
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
    // export const xxx: Type[] = [ / const xxx = {（模块内的数据常量）
    if let Some(rest) = code.strip_prefix("export const ").or_else(|| code.strip_prefix("const ")) {
        let Some((lhs, rhs)) = rest.split_once('=') else { return false };
        let ok_lhs = lhs.trim().chars().all(|c| c.is_alphanumeric() || "_$:[]<> ".contains(c));
        return ok_lhs && matches!(rhs.trim(), "[" | "{" | "[];" | "{};");
    }
    if code.contains("=>") || code.contains('(') || code.contains('=') || code.contains('`') {
        return false;
    }
    // `?` `|`：类型声明里的可选字段和联合类型（seoTitle?: string; / kind: 'a' | 'b';），没有运行时效果
    if !code.chars().all(|c| c.is_alphanumeric() || "_$\"{}[],:;. -+?|".contains(c)) {
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
/// `code`：方案的「代码改动」设置（review / small / all），见 ManagedGroup.code_changes。
pub fn managed_verdict(diff: &str, allow_new_content: bool, min_page: usize, code: &str) -> Vec<String> {
    let mut reasons = Vec::new();
    let content_ext = ["md", "mdx", "markdown", "html", "htm", "astro", "njk", "liquid", "hbs", "vue", "svelte", "json", "svg", "png", "jpg", "jpeg", "webp", "avif"];
    let (mut changed, mut new_lines) = (0usize, 0usize);
    let (mut code_lines, mut code_files) = (0usize, 0usize);
    for f in split_diff(diff) {
        let file = &f.file;
        let data = data_file(file) && f.added.iter().chain(&f.removed).all(|l| data_line(l));
        // 站点代码：数据文件里混了代码，或模板 / 组件 / 样式 / 前端脚本
        let is_code = !data && !hard_forbidden(file) && (data_file(file) || site_code_file(file));
        if hard_forbidden(file) {
            reasons.push(format!("{file}：依赖、构建 / 部署配置、服务端代码不自动改"));
        } else if is_code {
            code_files += 1;
            code_lines += f.added.len() + f.removed.len();
            if code == "review" {
                let line = f.added.iter().chain(&f.removed).find(|l| !data_line(l)).map(|l| l.trim().chars().take(60).collect::<String>()).unwrap_or_default();
                reasons.push(format!("{file}：改了代码（比如「{line}」），这个方案设成代码改动留给你确认"));
            }
        } else if !data && !content_ext.iter().any(|e| file.ends_with(&format!(".{e}"))) {
            reasons.push(format!("{file}：不是页面内容文件"));
        }
        if is_code || template_file(file) {
            if let Some((why, l)) = f.added.iter().find_map(|l| risky_code(l).map(|w| (w, l))) {
                reasons.push(format!("{file}：新加的代码{why}（「{}」），留给你确认", l.trim().chars().take(60).collect::<String>()));
            }
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
    if code == "small" && (code_lines > SMALL_CODE_LINES || code_files > SMALL_CODE_FILES) {
        reasons.push(format!("代码改动 {code_lines} 行、{code_files} 个文件，超过「小改动」的上限（{SMALL_CODE_LINES} 行、{SMALL_CODE_FILES} 个文件）"));
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
    /// AI 写下的预期效果：上线 4 周后 Pilot 拿它和数据对比。
    #[serde(default)]
    pub expect: Expect,
}

/// 一条任务预期影响什么：哪个指标（ctr / position / impressions / clicks / health）、
/// 哪个查询词（空＝整个页面）、一句话说预期从多少到多少。
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Expect {
    #[serde(default)]
    pub metric: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub note: String,
}

pub const EXPECT_METRICS: &[&str] = &["ctr", "position", "impressions", "clicks", "health"];

/// 没写预期时按改动类型兜底：改标题描述看点击率，补内容 / 内链看排名，新内容看展现，技术问题看体检。
pub fn default_metric_for(change: &str) -> &'static str {
    match change {
        "meta" | "title" | "meta_description" => "ctr",
        "content" | "internal_links" => "position",
        "new_content" => "impressions",
        "technical" => "health",
        _ => "clicks",
    }
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
        let r = managed_verdict(thin, true, 1500, "review");
        assert!(r.iter().any(|x| x.contains("低于这个方案的下限")), "{r:?}");
        assert!(managed_verdict(thin, true, 0, "review").is_empty(), "下限 0＝不检查");
        let body: String = "这是一段足够长的正文，讲清楚了具体步骤、注意事项和常见错误。".repeat(60);
        let full = format!("diff --git a/src/content/b.md b/src/content/b.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/b.md\n@@ -0,0 +1,4 @@\n+---\n+title: 完整的页面\n+---\n+{body}\n");
        assert!(managed_verdict(&full, true, 1500, "review").is_empty(), "{:?}", managed_verdict(&full, true, 1500, "review"));
        let leak = "diff --git a/src/content/c.md b/src/content/c.md\n--- a/src/content/c.md\n+++ b/src/content/c.md\n@@ -1 +1 @@\n-旧\n+步骤三（待补图）点击确认\n";
        assert!(managed_verdict(leak, true, 1500, "review").iter().any(|x| x.contains("待补图")));
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
        assert!(validate_group(&ManagedGroup { brain: "codex".into(), model: "gpt-6-sol".into(), ..g }).is_ok(), "Codex 可以用（托管时断网运行）");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 动态文件长过两倍上限就只留最新的一段；读的时候只看尾部，新的在前。
    #[test]
    fn activity_log_rotates_and_reads_from_the_tail() {
        let d = std::env::temp_dir().join(format!("managed-a-{}", uuid::Uuid::new_v4()));
        let p = dir(&d).join("activity.jsonl");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let text: String = (0..25).map(|i| format!("{}\n", serde_json::json!({ "ts": i, "site_id": "s", "site_name": "n", "kind": "shipped", "title": format!("t{i}") }))).collect();
        std::fs::write(&p, &text).unwrap();
        assert!(!rotate_activity(&p, &text, 20).unwrap(), "没超过两倍不动");
        assert!(rotate_activity(&p, &text, 10).unwrap());
        let kept = std::fs::read_to_string(&p).unwrap();
        assert_eq!(kept.lines().count(), 10);
        assert!(kept.lines().next().unwrap().contains("\"ts\":15"), "{kept}");
        let v = activity(&d, 3);
        assert_eq!(v.iter().map(|a| a.ts).collect::<Vec<_>>(), vec![24, 23, 22]);
        // 很长的文件：只读尾部也能拿到最新的
        let big: String = (0..5000).map(|i| format!("{}\n", serde_json::json!({ "ts": i, "site_id": "s", "site_name": "n", "kind": "shipped", "title": format!("标题 {i}") }))).collect();
        std::fs::write(&p, &big).unwrap();
        let v = activity(&d, 2);
        assert_eq!(v.iter().map(|a| a.ts).collect::<Vec<_>>(), vec![4999, 4998]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn defaults_are_safe() {
        let c = ManagedConfig::default();
        assert!(!c.paused);
        assert_eq!(c.new_content, "review");
        assert_eq!(c.daily_changes, 3);
        assert!(c.digest);
        assert!(validate_group(&ManagedGroup::from_legacy(&c, "x")).is_ok());
        let g = ManagedGroup::from_legacy(&c, "x");
        assert!(validate_group(&ManagedGroup { fallback_brain: "codex".into(), fallback_model: "gpt-6-sol".into(), ..g.clone() }).is_ok());
        assert!(validate_group(&ManagedGroup { fallback_brain: "claude".into(), ..g }).is_err(), "备用和主模型一样");
    }

    #[test]
    fn verdict_blocks_code_config_money_and_optionally_new_pages() {
        let ok = "diff --git a/src/content/a.md b/src/content/a.md\n--- a/src/content/a.md\n+++ b/src/content/a.md\n@@ -3 +3,2 @@\n-旧段落\n+新段落，补充了水温的说明。\n+还加了一段。\n";
        assert!(managed_verdict(ok, false, 1500, "review").is_empty());
        let code = "diff --git a/worker/index.ts b/worker/index.ts\n--- a/worker/index.ts\n+++ b/worker/index.ts\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(!managed_verdict(code, true, 1500, "review").is_empty());
        let cfg = "diff --git a/package.json b/package.json\n--- a/package.json\n+++ b/package.json\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(!managed_verdict(cfg, true, 1500, "review").is_empty());
        let money = "diff --git a/src/pages/a.astro b/src/pages/a.astro\n--- a/src/pages/a.astro\n+++ b/src/pages/a.astro\n@@ -9 +9 @@\n-<a href=\"/go/gate\">邀请码 ABC</a>\n+<a href=\"/go/gate\">邀请码 XYZ</a>\n";
        assert!(managed_verdict(money, true, 1500, "review").iter().any(|r| r.contains("推荐")));
        let newpage = "diff --git a/src/content/new.md b/src/content/new.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/new.md\n@@ -0,0 +1 @@\n+# 新页面\n";
        assert!(managed_verdict(newpage, false, 1500, "review").iter().any(|r| r.contains("新内容")));
        // 允许自动发布新页面时，只剩「低于正文下限」这一条（这个测试页只有一行）
        assert!(managed_verdict(newpage, true, 1500, "review").iter().all(|r| r.contains("低于这个方案的下限")));
    }

    #[test]
    fn data_files_with_only_text_count_as_content() {
        // 已有数据文件：加导入、加一段文字、加一个 slug、展开一个数组
        let edit = "diff --git a/src/data/articles.ts b/src/data/articles.ts\n--- a/src/data/articles.ts\n+++ b/src/data/articles.ts\n@@ -5 +5,2 @@\n+import { transitionArticles } from './articles-transitions';\n+    'ai-video-transition-prompts',\n+          '想把两个镜头接起来（遮挡、甩镜），可以看本页末尾的转场指南。',\n+  ...transitionArticles,\n";
        assert!(managed_verdict(edit, false, 1500, "review").is_empty(), "{:?}", managed_verdict(edit, false, 1500, "review"));
        assert!(data_line("const viduSources = [") && data_line("    sources: viduSources,"));
        assert!(!data_line("const x = pick(1);") && !data_line("const t = `${a}`;"));
        assert!(data_line("  seoTitle?: string;") && data_line("  kind: 'a' | 'b';") && data_line("  links?: { label: string; href: string }[];"));
        assert!(!super::touches_money("<a class=\"text-link\" href={link.href}>") && super::touches_money("https://x.com/?ref=abc"));
        // 正则字面量的值算数据；正则后面接代码不算
        assert!(data_line("    mentions: /(海螺|Hailuo)[^。]{0,60}(\\$\\s*9\\.99|每日少量|small daily)/i,"));
        assert!(data_line("    mentions: /Vidu[^。]{0,40}80\\s*(积分|credits)/i,"));
        assert!(data_line("  re: /a[/]b/g"));
        assert!(!data_line("  re: /a/.test(fetch('/x/')) ? 1 : /b/,"));
        assert!(!data_line("  re: /a/, run: go(),"));
        // 新的数据文件：按新内容规则和正文下限判断
        let body: String = (0..40).map(|_| "+      '遮挡转场要让前景物体把画面整个挡住，再在被挡住的那一帧切到下一段素材，两段的运动方向要一致。',\n").collect();
        let newfile = format!("diff --git a/src/data/articles-x.ts b/src/data/articles-x.ts\nnew file mode 100644\n--- /dev/null\n+++ b/src/data/articles-x.ts\n@@ -0,0 +1,50 @@\n+import type {{ ArticleSeed }} from './article-types';\n+export const xArticles: ArticleSeed[] = [\n+  {{\n+    slug: 'x',\n+    datePublished: '2026-10-22',\n+    body: [\n{body}+    ],\n+  }},\n+];\n");
        assert!(managed_verdict(&newfile, true, 1500, "review").is_empty(), "{:?}", managed_verdict(&newfile, true, 1500, "review"));
        assert!(managed_verdict(&newfile, false, 1500, "review").iter().any(|r| r.contains("新内容")));
        // 数据文件里混了代码：函数、模板字符串插值、外部导入
        for bad in ["+export function pick(a) { return a; }", "+  title: `${name} 指南`,", "+import fs from 'node:fs';", "+  run: () => fetch('/x'),", "+const x = process.env.TOKEN;"] {
            let d = format!("diff --git a/src/data/a.ts b/src/data/a.ts\n--- a/src/data/a.ts\n+++ b/src/data/a.ts\n@@ -1 +1 @@\n{bad}\n");
            assert!(managed_verdict(&d, true, 0, "review").iter().any(|r| r.contains("改了代码")), "{bad}");
        }
        // 不在 data / content 目录下的 .ts 仍然是代码
        let code = "diff --git a/src/lib/seo.ts b/src/lib/seo.ts\n--- a/src/lib/seo.ts\n+++ b/src/lib/seo.ts\n@@ -1 +1 @@\n+  title: 'x',\n";
        assert!(managed_verdict(code, true, 0, "review").iter().any(|r| r.contains("改了代码")));
    }

    #[test]
    fn code_changes_follow_the_plan_setting() {
        let lines = |n: usize| (0..n).map(|i| format!("+  const a{i} = {i};\n")).collect::<String>();
        let lib = |n: usize| format!("diff --git a/src/lib/types.ts b/src/lib/types.ts\n--- a/src/lib/types.ts\n+++ b/src/lib/types.ts\n@@ -1 +1 @@\n{}", lines(n));
        assert!(managed_verdict(&lib(10), true, 0, "review").iter().any(|r| r.contains("留给你确认")));
        assert!(managed_verdict(&lib(10), true, 0, "small").is_empty(), "{:?}", managed_verdict(&lib(10), true, 0, "small"));
        assert!(managed_verdict(&lib(100), true, 0, "small").iter().any(|r| r.contains("小改动")));
        assert!(managed_verdict(&lib(100), true, 0, "all").is_empty());
        // 任何一档都拦：依赖 / 配置、服务端、读环境变量、外部请求、新的外部包
        for (path, line) in [("package.json", "+  \"x\": \"1\""), ("astro.config.mjs", "+  site: 'x',"), ("src/pages/api/hook.ts", "+export const a = 1;"), ("src/lib/a.ts", "+const k = process.env.KEY;"), ("src/components/A.astro", "+const r = await fetch('https://x.com');"), ("src/lib/a.ts", "+import lodash from 'lodash';")] {
            let d = format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n{line}\n");
            assert!(!managed_verdict(&d, true, 0, "all").is_empty(), "{path} {line}");
        }
        // 本地文件的导入、样式文件，小改动可以自动上线
        let ok = "diff --git a/src/components/A.astro b/src/components/A.astro\n--- a/src/components/A.astro\n+++ b/src/components/A.astro\n@@ -1 +1 @@\n+import Links from './Links.astro';\n+<Links items={block.links} />\ndiff --git a/src/styles/a.css b/src/styles/a.css\n--- a/src/styles/a.css\n+++ b/src/styles/a.css\n@@ -1 +1 @@\n+.links { margin: 0; }\n";
        assert!(managed_verdict(ok, true, 0, "small").is_empty(), "{:?}", managed_verdict(ok, true, 0, "small"));
    }

    #[test]
    fn plan_tasks_carry_the_expected_effect() {
        let r = "```json\n{\"notes\":\"\",\"positioning\":\"x\",\"tasks\":[{\"title\":\"改 /zh/fees/ 标题\",\"page\":\"https://x.com/zh/fees/\",\"change\":\"meta\",\"why\":\"点击率低\",\"priority\":1,\"expect\":{\"metric\":\"ctr\",\"query\":\"gate 手续费\",\"note\":\"点击率从 2.2% 提到 3% 以上\"}},{\"title\":\"补内容\",\"change\":\"content\",\"priority\":2}]}\n```";
        let (_, t) = parse_plan_full(r).unwrap();
        assert_eq!(t[0].expect, Expect { metric: "ctr".into(), query: "gate 手续费".into(), note: "点击率从 2.2% 提到 3% 以上".into() });
        assert_eq!(t[1].expect, Expect::default());
        assert_eq!(default_metric_for("meta"), "ctr");
        assert_eq!(default_metric_for("new_content"), "impressions");
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
