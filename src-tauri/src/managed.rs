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
    /// 调度同时最多起几个重作业（要构建站点或跑 AI 的）。0＝按这台电脑的核数和内存自动算，见 site_runner::parallel_jobs
    #[serde(default)]
    pub parallel_jobs: u32,
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
fn default_cadence() -> String {
    "daily".into()
}
fn default_cadence_day() -> u32 {
    1
}
fn default_cadence_every() -> u32 {
    1
}
/// 老方案换算：每轮条数取原来「每天自动上线上限」和原来每轮 3 条里较小的（0 也按 1 算）
pub const DEFAULT_PER_ROUND: u32 = 3;
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
    /// 主模型 / 备用模型的推理强度（空＝跟随模型默认；API 模型带 api: 前缀，和对话里一样）
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub fallback_effort: String,
    #[serde(default = "default_review")]
    pub new_content: String,
    /// 老版本的「每站每天自动上线上限」：已经不用了，只在老方案第一次读进来时换算成「每轮最多改几条」。
    #[serde(default = "default_daily")]
    pub daily_changes: u32,
    /// 运营节奏：多久跑一轮——每 cadence_every 天（daily）或每 cadence_every 周（weekly，在 cadence_day 那天；1＝周一 … 7＝周日）。
    /// 一轮＝拉数据 → 挑最值得做的几条 → 交给 AI 改 → 检查通过的按规则上线。体检不花 AI 的钱，照样每天做。
    #[serde(default = "default_cadence")]
    pub cadence: String,
    #[serde(default = "default_cadence_day")]
    pub cadence_day: u32,
    /// 每几天 / 每几周跑一轮（单位看 cadence）：1＝每天 / 每周。
    #[serde(default = "default_cadence_every")]
    pub cadence_every: u32,
    /// 每轮最多改几条（只管托管自己跑；你手动交给 AI 的不受它限制）。0＝老方案还没换算过，读进来时补上。
    #[serde(default)]
    pub per_round: u32,
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
    /// 拦截等级：strict 稳妥 | standard 标准 | open 放开（见 `TIERS`）。代码改动跟着它走（稳妥＝留给你确认，其它＝都自动），
    /// 老方案没有这项，读进来时按 code_changes 换算。
    #[serde(default)]
    pub strictness: String,
    /// 在被拦下的卡片上点了「这类以后不拦」的拦截类型（见 `BLOCK_KINDS`），只对这个方案生效。
    #[serde(default)]
    pub allow_kinds: Vec<String>,
    /// 自动上线前用判断模型（Jev）做内容质检；接上 TypeSafe 才生效，见 jev_checks
    #[serde(default = "yes")]
    pub jev_gate: bool,
    /// 质检没过时，先把原因交回给 AI 在同一分支上重写一次，再检查一遍；还不过才留给你确认
    #[serde(default = "yes")]
    pub jev_rewrite: bool,
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

/// 拦截等级：稳妥（代码改动、推荐内容一碰就留给你）| 标准（推荐链接和邀请码真变了才拦）| 放开（只拦改坏了测不出来、回滚不了的）。
/// 不管哪一档都不放行的：依赖、构建 / 部署配置、脚本、Worker / Functions / API 路由、删文件、调外部服务或执行命令、
/// 站点被标成危险网站、构建没过；上线后验收没过照样自动回滚。
pub const TIERS: &[&str] = &["strict", "standard", "open"];
/// 可以在被拦下的卡片上点「这类以后不拦」的拦截类型。没列在这里的（依赖 / 配置 / 服务端代码、删文件、调外部服务……）不能放行。
pub const BLOCK_KINDS: &[&str] = &["money", "html", "env", "package", "size", "leak", "thin", "orphan", "jev"];
pub fn block_kind_label(k: &str) -> &'static str {
    match k {
        "money" => "推荐链接、邀请码变动",
        "html" => "直接插入 HTML",
        "env" => "读环境变量",
        "package" => "引入外部包",
        "size" => "大段替换已有内容",
        "leak" => "正文里像有内部说明或占位",
        "thin" => "新页面正文偏短",
        "orphan" => "新页面没有站内链接",
        "jev" => "内容质检没过",
        _ => "其它",
    }
}
/// 「放开」档默认就不拦的类型
// env 不在里面：读环境变量的代码可能把密钥渲染进公开页面，验收测不出来、回滚也收不回，任何等级都要人看
const OPEN_LETS: &[&str] = &["html", "package", "orphan", "jev"];

/// 一条不能自动上线的原因；kind 是 `BLOCK_KINDS` 里的一种，空＝不能放行。
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: &'static str,
    pub text: String,
}
fn block(kind: &'static str, text: String) -> Block {
    Block { kind, text }
}

/// 自动上线判定用的方案规则（见 `ManagedGroup::rules`）。
pub struct Rules<'a> {
    pub tier: &'a str,
    /// review / small / all，见 ManagedGroup.code_changes
    pub code: &'a str,
    pub allow: &'a [String],
    pub new_content: bool,
    /// 新页面正文下限（字，0＝不检查）
    pub min_page: usize,
    /// 站点放行的「算网站内容的代码文件」，见 `listed`
    pub content_code: &'a [String],
    /// 改动前的站点里本来就有的推荐链接 / 码 / 字段（见 `added_money_atoms`）：再引用一次不算新增
    pub known: &'a [String],
    /// 构建时脚本自动重写的数据文件（文件名，见 `build_outputs`）：AI 跑构建时顺手提交进来的，不算 AI 的改动
    pub generated: &'a [String],
    /// 站点 package.json 里本来就有的依赖（包名）：代码里新 import 它们不算「引入外部包」
    pub deps: &'a [String],
    /// 构建和检查会跑到的脚本（文件名，见 `build_scripts`）；None＝读不到 package.json，scripts/ 下一律拦
    pub build_scripts: Option<&'a [String]>,
}
impl Rules<'_> {
    /// 这类原因按方案放行（点过「这类以后不拦」，或「放开」档默认不拦）
    pub fn lets(&self, kind: &str) -> bool {
        !kind.is_empty() && (self.allow.iter().any(|k| k == kind) || (self.tier == "open" && OPEN_LETS.contains(&kind)))
    }
    fn max_changed(&self) -> usize {
        if self.lets("size") {
            usize::MAX
        } else if self.tier == "open" {
            MAX_CHANGED_LINES_OPEN
        } else {
            MAX_CHANGED_LINES
        }
    }
}
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
            effort: String::new(),
            fallback_effort: String::new(),
            new_content: c.new_content.clone(),
            daily_changes: c.daily_changes,
            cadence: default_cadence(),
            cadence_day: default_cadence_day(),
            cadence_every: default_cadence_every(),
            per_round: c.daily_changes.clamp(1, DEFAULT_PER_ROUND),
            tokens_per_week: c.tokens_per_week,
            goal: String::new(),
            actions: all_actions(),
            guidelines: String::new(),
            topics: String::new(),
            metric: default_metric(),
            min_page_chars: default_min_page(),
            code_changes: default_code(),
            strictness: "standard".into(),
            allow_kinds: Vec::new(),
            jev_gate: true,
            jev_rewrite: true,
            skip_offtopic: true,
            created_at: 0,
        }
    }
    /// 拦截等级（老方案按代码改动设置换算：留给你确认 / 小改动＝稳妥，都自动＝标准）
    pub fn tier(&self) -> &str {
        match self.strictness.as_str() {
            t @ ("strict" | "standard" | "open") => t,
            _ if matches!(self.code_changes.as_str(), "review" | "small") => "strict",
            _ => "standard",
        }
    }
    pub fn rules<'a>(&'a self, content_code: &'a [String]) -> Rules<'a> {
        Rules { tier: self.tier(), code: &self.code_changes, allow: &self.allow_kinds, new_content: self.new_content == "auto", min_page: self.min_page_chars as usize, content_code, known: &[], generated: &[], build_scripts: None, deps: &[] }
    }
    /// 保存前把等级落定：老方案补上等级，代码改动跟着等级走，放行清单只留认得的类型
    pub fn settle_tier(&mut self) {
        self.strictness = self.tier().to_string();
        self.code_changes = if self.strictness == "strict" { "review" } else { "all" }.into();
        self.allow_kinds.retain(|k| BLOCK_KINDS.contains(&k.as_str()));
        self.allow_kinds.sort();
        self.allow_kinds.dedup();
    }
    /// 等级收紧时（放开 → 标准 → 稳妥）清掉「这类以后不拦」的放行：那是在更宽的等级下点的，换成更严的等级就该重新把关
    pub fn tighten_from(&mut self, previous: &str) {
        let rank = |t: &str| TIERS.iter().position(|x| *x == t).unwrap_or(1);
        if rank(&self.strictness) < rank(previous) {
            self.allow_kinds.clear();
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
    let mut c: ManagedConfig = crate::store_file::read_or_quarantine(&dir(data_dir).join("config.json"));
    // 老方案没有「每轮最多改几条」：按原来的每天上线上限换算（和原来每轮 3 条取小）
    for g in &mut c.groups {
        if g.per_round == 0 {
            g.per_round = g.daily_changes.clamp(1, DEFAULT_PER_ROUND);
        }
        if g.strictness.is_empty() {
            g.settle_tier();
        }
    }
    c
}

static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn save(data_dir: &Path, c: &ManagedConfig) -> Result<(), String> {
    let _g = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    save_unlocked(data_dir, c)
}

fn save_unlocked(data_dir: &Path, c: &ManagedConfig) -> Result<(), String> {
    let d = dir(data_dir);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let raw = serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?;
    // 临时文件名带随机串：几个作业同时写不会互相截断
    let tmp = d.join(format!("config.json.{}.tmp", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, d.join("config.json")).map_err(|e| e.to_string())
}

/// 读 → 改 → 写一气呵成（加锁）：作业里改方案（暂停站点、记日报时间）用这个，别和界面上的保存互相覆盖
pub fn modify(data_dir: &Path, f: impl FnOnce(&mut ManagedConfig)) -> Result<ManagedConfig, String> {
    let _g = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = load(data_dir);
    f(&mut c);
    save_unlocked(data_dir, &c)?;
    Ok(c)
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
    if !g.strictness.is_empty() && !TIERS.contains(&g.strictness.as_str()) {
        return Err(format!("未知的拦截等级：{}", g.strictness));
    }
    if g.daily_changes > 50 {
        return Err("每天最多自动上线的条数超出范围（0–50）".into());
    }
    if !matches!(g.cadence.as_str(), "daily" | "weekly") || !(1..=7).contains(&g.cadence_day) {
        return Err("运营节奏只能是每几天，或每几周选一天".into());
    }
    if g.cadence == "daily" && !(1..=30).contains(&g.cadence_every) {
        return Err("每几天跑一轮超出范围（1–30 天）".into());
    }
    if g.cadence == "weekly" && !(1..=8).contains(&g.cadence_every) {
        return Err("每几周跑一轮超出范围（1–8 周）".into());
    }
    if !(1..=20).contains(&g.per_round) {
        return Err("每轮最多改的条数超出范围（1–20）".into());
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
/// 以及站点代码以外的后端语言。后端目录里的页面模板和测试文件除外（见 `backend_template_or_test`）。
/// 托管平台的跳转文件 `_redirects` 不在这里：按每条规则的内容判（见 `redirect_rules`）。
pub fn hard_forbidden(path: &str) -> bool {
    let p = path.trim_start_matches("./");
    let name = p.rsplit('/').next().unwrap_or(p);
    let first = p.split('/').next().unwrap_or("");
    let segs: Vec<&str> = p.split('/').collect();
    if backend_template_or_test(p) {
        return false;
    }
    matches!(
        name,
        "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "bun.lock" | "bun.lockb" | "wrangler.toml" | "wrangler.json"
            | "wrangler.jsonc" | "pilot.json" | ".env" | ".dev.vars" | "_headers" | "_worker.js" | "tsconfig.json" | "middleware.ts" | "middleware.js"
    ) || name.starts_with(".env")
        || name.starts_with("astro.config")
        || name.starts_with("vite.config")
        || name.starts_with("next.config")
        || name.starts_with("svelte.config")
        || name.starts_with("tailwind.config")
        // 其它框架的构建入口和配置、托管平台的重定向配置：构建时在本机执行，或直接改线上路由
        || ["nuxt.config", "gatsby-", "remix.config", "rollup.config", "webpack.config", "postcss.config", "babel.config", "eleventy.config", ".eleventy", "build.", "prebuild.", "postbuild."].iter().any(|p| name.starts_with(p))
        || matches!(name, "vercel.json" | "netlify.toml" | "firebase.json" | "now.json")
        || matches!(first, ".github" | "worker" | "workers" | "functions" | "server" | "scripts" | "node_modules" | "cmd" | "internal" | "deploy" | "bin" | "migrations")
        || segs.iter().any(|s| matches!(*s, "api" | "server" | "functions" | "middleware"))
        || matches!(name, "go.mod" | "go.sum" | "Makefile" | "Dockerfile")
        || ["py", "sh", "rs", "go", "toml", "yml", "yaml", "sql"].iter().any(|e| name.ends_with(&format!(".{e}")))
}

/// Go / Python 站点放在 internal/、cmd/、server/ 里的页面模板（.html / .tmpl / .gohtml）、正文（.md / .mdx）和测试文件（_test.go、test_*.py）：
/// 模板和正文是页面内容，测试只在上线前的检查里跑、不会上线，都不算服务端代码。
/// 脚本、CI、部署、Worker / Functions / middleware 目录里的照样拦（Functions 目录里每个文件都可能变成线上路由）。
fn backend_template_or_test(p: &str) -> bool {
    let name = p.rsplit('/').next().unwrap_or(p);
    let first = p.split('/').next().unwrap_or("");
    if matches!(first, ".github" | "node_modules" | "deploy" | "migrations" | "scripts" | "bin" | "worker" | "workers" | "functions")
        || p.split('/').any(|s| matches!(s, "functions" | "middleware"))
    {
        return false;
    }
    template_page(name) || test_file(name)
}

/// 后端渲染的页面模板和正文文件
fn template_page(name: &str) -> bool {
    ["html", "htm", "tmpl", "gohtml", "md", "mdx", "markdown"].iter().any(|e| name.ends_with(&format!(".{e}")))
}

/// 测试文件：按方案的「拦截等级」判断，新加的代码照样查危险写法。
fn test_file(name: &str) -> bool {
    name.ends_with("_test.go") || (name.ends_with(".py") && (name.starts_with("test_") || name.ends_with("_test.py")))
}

/// 放进站点「算网站内容的代码文件」清单也不放行的：依赖、锁文件、构建 / 部署 / CI 配置、密钥配置、
/// 数据库迁移、脚本。剩下被 `hard_forbidden` 拦的（比如 Go 站把页面写在 internal/site/build.go 里）可以按站点放行。
pub fn never_allow(path: &str) -> bool {
    let p = path.trim_start_matches("./");
    let name = p.rsplit('/').next().unwrap_or(p);
    let first = p.split('/').next().unwrap_or("");
    hard_forbidden(p)
        && (matches!(
            name,
            "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "bun.lock" | "bun.lockb" | "wrangler.toml" | "wrangler.json" | "wrangler.jsonc" | "pilot.json" | ".env" | ".dev.vars" | "_headers" | "_worker.js" | "tsconfig.json"
                | "go.mod" | "go.sum" | "Makefile" | "Dockerfile" | "Cargo.toml" | "Cargo.lock" | "requirements.txt" | "pyproject.toml"
        ) || name.starts_with(".env")
            || ["astro.config", "vite.config", "next.config", "svelte.config", "tailwind.config"].iter().any(|x| name.starts_with(x))
            || matches!(first, ".github" | "node_modules" | "deploy" | "migrations" | "scripts" | "bin")
            || ["sh", "toml", "yml", "yaml", "sql"].iter().any(|e| name.ends_with(&format!(".{e}"))))
}

// ---------------------------------------------------------------------------
// 托管平台的跳转文件（Cloudflare Pages / Netlify 的 _redirects）：按每条规则的内容判，不按文件名一刀切。
// 运营任务本来就会让 AI 给旧地址加 301（旧站网址、合并弱页），以前一律留给人确认，等于这类任务永远自动不了。
// ---------------------------------------------------------------------------

/// `_redirects` 里的一条规则
#[derive(Clone, Debug, PartialEq)]
pub struct Redirect {
    pub from: String,
    pub to: String,
    pub status: u16,
}

pub fn redirect_file(path: &str) -> bool {
    path.rsplit('/').next() == Some("_redirects")
}

fn short(l: &str) -> String {
    l.trim().chars().take(60).collect()
}

/// 一行规则：`来源 目标 [状态码]`，空行和注释返回 Ok(None)。能自动上线的只有：站内精确地址（可以带 `:lang` 这种
/// 占位段，不能带 `*` 通配、查询串）301 / 302 / 307 / 308 跳到站内路径。改写（200）、代理、跳外站、带条件参数的不行。
fn parse_redirect(line: &str) -> Result<Option<Redirect>, String> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    let (Some(from), Some(to)) = (parts.first(), parts.get(1)) else { return Err(format!("「{}」不是「来源 目标 状态码」的写法", short(t))) };
    if parts.len() > 3 || parts.get(2).is_some_and(|s| s.ends_with('!')) {
        return Err(format!("「{}」带了强制标记或条件参数", short(t)));
    }
    let status = match parts.get(2) {
        None => 301,
        Some(s) => s.parse::<u16>().map_err(|_| format!("「{}」的状态码不是数字", short(t)))?,
    };
    if !matches!(status, 301 | 302 | 307 | 308) {
        return Err(format!("「{}」不是跳转（{status} 是改写 / 代理，会把别的内容当成这个地址）", short(t)));
    }
    if !from.starts_with('/') || from.contains('*') || from.contains('?') {
        return Err(format!("「{}」的来源不是站内的精确地址", short(t)));
    }
    if !to.starts_with('/') || to.starts_with("//") || to.contains("://") || to.contains('*') || to.contains(":splat") {
        return Err(format!("「{}」的目标不是站内路径", short(t)));
    }
    Ok(Some(Redirect { from: from.to_string(), to: to.to_string(), status }))
}

/// 这次差异里 `_redirects` 的改动：只许新加规则（删掉或改了已有的规则，老地址就会失效）。
/// 返回新加的规则；不能自动上线时返回原因。
fn redirect_rules(f: &FileDiff) -> Result<Vec<Redirect>, String> {
    if f.deleted {
        return Err("删掉了跳转文件".into());
    }
    if let Some(l) = f.removed.iter().find(|l| parse_redirect(l) != Ok(None)) {
        return Err(format!("删掉或改了已有的跳转规则「{}」", short(l)));
    }
    let mut out = Vec::new();
    for l in &f.added {
        if let Some(r) = parse_redirect(l)? {
            out.push(r);
        }
    }
    Ok(out)
}

/// 整份差异里新加的跳转规则（`redirect_rules` 没通过的文件跳过，它们已经在判定里拦下了）
pub fn redirects_added(diff: &str) -> Vec<Redirect> {
    split_diff(diff).iter().filter(|f| redirect_file(&f.file)).filter_map(|f| redirect_rules(f).ok()).flatten().collect()
}

/// 把构建产物里的文件路径（`a/index.html`、`b.html`、`c.png`）和网址路径统一成比较用的形式：去掉 index.html 和末尾的 /
fn url_key(p: &str) -> String {
    let p = p.split(['?', '#']).next().unwrap_or("").trim_start_matches('/');
    let p = p.strip_suffix("index.html").unwrap_or(p).trim_end_matches('/');
    format!("/{p}")
}

/// `/:lang/kyc.html` 这种带占位段的来源和一个具体路径对不对得上（`.html` 结尾的文件不带后缀也能打开）
fn redirect_hits(from: &str, built_key: &str) -> bool {
    let seg = |p: &str| p.trim_matches('/').split('/').map(str::to_string).collect::<Vec<_>>();
    let a = seg(from);
    let fits = |b: &str| { let b = seg(b); a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.starts_with(':') || x == y) };
    fits(built_key) || built_key.strip_suffix(".html").is_some_and(|b| fits(b))
}

/// 新加的跳转规则对照构建产物：目标得是构建出来的页面（不然跳过去就是 404），来源不能是已有页面（跳转会把它盖住）。
/// `built` 是构建产物里的文件路径。返回不能自动上线的原因。
pub fn redirect_problems(rules: &[Redirect], built: &[String]) -> Vec<String> {
    let keys: Vec<String> = built.iter().map(|b| url_key(b)).collect();
    let exists = |p: &str| { let k = url_key(p); keys.iter().any(|b| *b == k || b.strip_suffix(".html").is_some_and(|s| s == k)) };
    let mut out = Vec::new();
    for r in rules {
        if r.to.contains(':') {
            out.push(format!("跳转目标 {} 带占位符，查不了页面存不存在", r.to));
        } else if !exists(&r.to) && !rules.iter().any(|o| o.from == r.to) {
            out.push(format!("跳转目标 {} 在构建产物里没有这个页面", r.to));
        }
        if let Some(b) = keys.iter().find(|b| redirect_hits(&r.from, b)) {
            out.push(format!("跳转来源 {} 会盖住已有页面 {b}", r.from));
        }
    }
    out
}

/// 构建产物的 `_redirects` 里有规则接住这个网址路径（旧地址 301 到新页面：线上能打开，验收时按页面查）
pub fn redirected_in(out: &Path, path: &str) -> bool {
    let Ok(txt) = std::fs::read_to_string(out.join("_redirects")) else { return false };
    let p = url_key(path);
    crate::migrate_extract::parse_redirects(&txt).iter().any(|(from, _, _)| {
        redirect_hits(&url_key(from), &p) || from.strip_suffix('*').is_some_and(|pre| p.starts_with(pre.trim_end_matches('/')))
    })
}

/// 线上直接跑请求的代码：Worker、Pages Functions、中间件、API 路由。页面验收测不到它们坏没坏，
/// 「代码改动：全部」也不放行，要放就把文件加进站点的「算网站内容的代码文件」清单。
fn edge_runtime(path: &str) -> bool {
    let p = path.trim_start_matches("./");
    let name = p.rsplit('/').next().unwrap_or(p);
    let first = p.split('/').next().unwrap_or("");
    matches!(first, "worker" | "workers" | "functions")
        || p.split('/').any(|s| matches!(s, "api" | "functions" | "middleware"))
        || name.starts_with("middleware.")
        || name == "_worker.js"
}

/// 文件在不在站点的「算网站内容的代码文件」清单里：写完整路径，或以 / 结尾的目录。
pub fn listed(path: &str, list: &[String]) -> bool {
    let p = path.trim_start_matches("./");
    list.iter().map(|x| x.trim().trim_start_matches("./")).filter(|x| !x.is_empty()).any(|x| if x.ends_with('/') { p.starts_with(x) } else { p == x })
}

/// 站点代码（模板、组件、样式、前端脚本）：按方案的「拦截等级」决定能不能自动上线。
fn site_code_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    ["ts", "tsx", "js", "jsx", "mjs", "cjs", "css", "scss", "less"].iter().any(|e| name.ends_with(&format!(".{e}")))
}

/// 模板类文件（.astro / .vue / .svelte）里写着代码，和代码文件一样查危险写法。
fn template_file(path: &str) -> bool {
    ["astro", "vue", "svelte", "jsx", "tsx"].iter().any(|e| path.ends_with(&format!(".{e}")))
}

/// 一行 import 引入的外部包名（`next/link` → `next`，`@scope/pkg/x` → `@scope/pkg`）。本地文件、路径别名、
/// astro 虚拟模块、只引类型的 `import type` 返回 None。
pub fn import_package(line: &str) -> Option<String> {
    let t = line.trim();
    if t.starts_with("import type ") {
        return None;
    }
    let spec = t.strip_prefix("import ").and_then(|r| r.rsplit_once(" from ").map(|x| x.1)).or_else(|| t.strip_prefix("import ").filter(|r| r.starts_with('\'') || r.starts_with('"')))?;
    let spec = spec.trim().trim_end_matches(';').trim_matches(['\'', '"']);
    let local = spec.starts_with('.') || spec.starts_with('/') || spec.starts_with("~/") || spec.starts_with("@/") || spec.starts_with("$lib") || spec.starts_with("astro:") || spec.starts_with("astro/") || spec == "astro";
    if local || spec.is_empty() {
        return None;
    }
    let mut parts = spec.split('/');
    let first = parts.next()?;
    Some(if first.starts_with('@') { format!("{first}/{}", parts.next().unwrap_or_default()) } else { first.to_string() })
}

/// 站点 package.json 里声明的依赖包名（dependencies / devDependencies）。
pub fn declared_deps(app_dir: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(app_dir.join("package.json")) else { return Vec::new() };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&raw) else { return Vec::new() };
    ["dependencies", "devDependencies"].iter().filter_map(|k| pkg[*k].as_object()).flat_map(|o| o.keys().cloned()).collect()
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
        // Go / Python 站点的页面代码（按站点放行的文件）
        ("os.Getenv", "读环境变量"),
        ("os.LookupEnv", "读环境变量"),
        ("os.environ", "读环境变量"),
        ("exec.Command", "执行命令"),
        ("\"os/exec\"", "执行命令"),
        ("subprocess", "执行命令"),
        ("http.Get(", "请求外部服务"),
        ("http.Post(", "请求外部服务"),
        ("http.NewRequest", "请求外部服务"),
        ("requests.get(", "请求外部服务"),
        ("requests.post(", "请求外部服务"),
        ("http.Client", "请求外部服务"),
        ("urllib.request", "请求外部服务"),
        ("urlopen(", "请求外部服务"),
        ("os.Remove", "删除文件"),
        ("import(", "动态加载代码"),
        ("__import__", "动态执行代码"),
        ("globalThis[", "动态执行代码"),
        ("process[", "动态执行代码"),
        ("Reflect.", "动态执行代码"),
        ("os.system(", "执行命令"),
        ("os.popen(", "执行命令"),
        ("shutil.rmtree", "删除文件"),
    ] {
        if t.contains(pat) {
            return Some(why);
        }
    }
    // 新引入的外部包（不是本地文件 / 路径别名 / astro: 虚拟模块）；只引类型的 `import type` 编译后就没了，不算
    if import_package(t).is_some() {
        return Some("引入了外部包");
    }
    if t.contains("require(") {
        return Some("引入了外部包");
    }
    None
}

/// 页面文件（Markdown / HTML / SVG）里不该自动上线的写法：Markdown 允许内嵌 HTML，Astro 原样渲染，`public/` 下的 HTML / SVG 直接上线。
/// 脚本、事件处理、javascript: 任何等级都要人看；iframe / object / embed 按「直接插入 HTML」算（方案可以放行）。
/// `markdown`：正文文件（.md / .mdx）里任何 <script> 都硬拦；模板（.astro / .html / .svg）里内联 <script> 是正常写法，
/// 只硬拦引外站脚本和 javascript:，其余按「直接插入 HTML」（方案可以放行）
fn page_risky(line: &str, markdown: bool) -> Option<&'static str> {
    let l = line.to_ascii_lowercase();
    // 结构化数据（<script type="application/ld+json">…）不是可执行脚本，页面模板里改它是常见任务
    let ld_json = l.contains("<script") && l.contains("application/ld+json") && !l.contains("src=");
    let external = l.contains("<script") && l.contains("src=") && (l.contains("src=\"http") || l.contains("src='http") || l.contains("src=\"//") || l.contains("src='//"));
    // javascript: 要在属性值的位置（前面是 = 或引号），正文里的「JavaScript: 基础」不算
    let js_url = l.match_indices("javascript:").any(|(i, _)| l[..i].trim_end().ends_with(['=', '"', '\'', '(']));
    if external || js_url || (markdown && l.contains("<script") && !ld_json) {
        return Some("插入脚本");
    }
    // 标签属性里的事件处理：onclick= / onload= / onerror=（前面是空格或引号、处在标签里，后面紧跟 =）
    let mut from = 0;
    while let Some(i) = l[from..].find("on") {
        let i = from + i;
        let before_ok = i > 0 && matches!(l.as_bytes()[i - 1], b' ' | b'"' | b'\'');
        let in_tag = l[..i].rfind('<').is_some_and(|lt| l[lt..i].find('>').is_none());
        let rest = &l[i + 2..];
        let name_len = rest.chars().take_while(|c| c.is_ascii_alphabetic()).count();
        if before_ok && in_tag && name_len >= 3 && rest[name_len..].starts_with('=') {
            return Some("插入脚本");
        }
        from = i + 2;
    }
    if (l.contains("<script") && !ld_json) || l.contains("<iframe") || l.contains("<object") || l.contains("<embed") {
        return Some(if l.contains("<script") { "内联脚本" } else { "嵌入外部页面" });
    }
    None
}

/// 危险写法对应的拦截类型：直接插入 HTML、读环境变量、引入外部包可以按方案放行；调外部服务、执行命令这些不行。
fn risky_kind(why: &str) -> &'static str {
    match why {
        "直接插入 HTML" | "嵌入外部页面" | "内联脚本" => "html",
        "读环境变量" | "读运行环境（密钥、数据库）" => "env",
        "引入了外部包" => "package",
        _ => "",
    }
}

/// 站点构建时（npm 的 prebuild / build / postbuild）跑的脚本里写到的数据文件名：这些文件每次构建都会被重写。
/// 只认 json / csv / txt / xml 这类数据，脚本、配置不算。读不到 package.json 返回空。
pub fn build_outputs(app_dir: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(app_dir.join("package.json")) else { return Vec::new() };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&raw) else { return Vec::new() };
    let cmds: Vec<&str> = ["prebuild", "build", "postbuild"].iter().filter_map(|k| pkg["scripts"][*k].as_str()).collect();
    let mut out: Vec<String> = Vec::new();
    for cmd in cmds {
        for tok in cmd.split(|c: char| c.is_whitespace() || matches!(c, '&' | ';' | '|' | '"' | '\'')) {
            if !["mjs", "js", "cjs", "ts", "mts"].iter().any(|e| tok.ends_with(&format!(".{e}"))) {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(app_dir.join(tok)) else { continue };
            // 只看引号里的字符串（文件路径），res.json() 这种代码不算
            for word in src.split(['"', '\'', '`']).skip(1).step_by(2) {
                let name = word.rsplit('/').next().unwrap_or(word);
                if ["json", "csv", "txt", "xml"].iter().any(|e| name.ends_with(&format!(".{e}"))) && name.len() > 5 && !matches!(name, "package.json" | "tsconfig.json" | "package-lock.json") {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 构建脚本提到的 JSON 就在 scripts/ 目录里：构建时会读到它，不算闲置的数据
fn script_json(mention: &str, dirs: [&Path; 2]) -> Option<String> {
    let name = mention.rsplit('/').next().filter(|n| n.len() > 5 && n.ends_with(".json"))?;
    dirs.iter().any(|d| d.ends_with("scripts") && d.join(name).is_file()).then(|| name.to_string())
}

/// 构建、检查、部署会跑到的脚本文件名：package.json 里 build / check / test / lint / deploy 一类命令（连同 pre / post 和
/// 它们 npm run 到的命令）、站点设置里的构建和检查命令点到的脚本，这些脚本 import 的本地脚本，以及读到的 scripts/ 下的 JSON。
/// 读不到 package.json 返回 None。
pub fn build_scripts(app_dir: &Path, commands: &[String]) -> Option<Vec<String>> {
    let raw = std::fs::read_to_string(app_dir.join("package.json")).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let scripts = pkg["scripts"].as_object().cloned().unwrap_or_default();
    let ext = |t: &str| ["mjs", "js", "cjs", "ts", "mts"].iter().any(|e| t.ends_with(&format!(".{e}")));
    let mut todo: Vec<String> = commands.to_vec();
    for k in ["prebuild", "build", "postbuild", "precheck", "check", "postcheck", "test", "lint", "predeploy", "deploy", "postdeploy"] {
        if let Some(v) = scripts.get(k).and_then(|v| v.as_str()) {
            todo.push(v.to_string());
        }
    }
    let scripts_dir = app_dir.join("scripts");
    let mut read_json: Vec<String> = Vec::new();
    let (mut seen_cmd, mut files): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    while let Some(cmd) = todo.pop() {
        if seen_cmd.contains(&cmd) || seen_cmd.len() > 50 {
            continue;
        }
        seen_cmd.push(cmd.clone());
        let toks: Vec<&str> = cmd.split(|c: char| c.is_whitespace() || matches!(c, '&' | ';' | '|' | '"' | '\'')).filter(|t| !t.is_empty()).collect();
        for (i, t) in toks.iter().enumerate() {
            if *t == "run" && i > 0 && matches!(toks[i - 1], "npm" | "pnpm" | "yarn" | "bun") {
                if let Some(v) = toks.get(i + 1).and_then(|n| scripts.get(*n)).and_then(|v| v.as_str()) {
                    todo.push(v.to_string());
                }
            }
            if ext(t) && !files.contains(&t.to_string()) {
                files.push(t.to_string());
            }
            read_json.extend(script_json(t, [&scripts_dir, app_dir]));
        }
    }
    // 这些脚本 import 的本地脚本也算（最多跟三层）
    let mut i = 0;
    while i < files.len() && i < 200 {
        let f = files[i].clone();
        let base = app_dir.join(&f);
        if let Ok(src) = std::fs::read_to_string(&base) {
            for w in src.split(['"', '\'', '`']).skip(1).step_by(2) {
                read_json.extend(script_json(w, [&scripts_dir, base.parent().unwrap_or(app_dir)]));
                if w.starts_with('.') && ext(w) {
                    let p = base.parent().unwrap_or(app_dir).join(w);
                    let rel = p.strip_prefix(app_dir).map(|r| r.to_string_lossy().into_owned()).unwrap_or_else(|_| w.to_string());
                    if !files.contains(&rel) {
                        files.push(rel);
                    }
                }
            }
        }
        i += 1;
    }
    let mut names: Vec<String> = files.iter().map(|f| f.rsplit('/').next().unwrap_or(f).to_string()).collect();
    names.extend(read_json);
    names.sort();
    names.dedup();
    Some(names)
}

/// scripts/ 里构建、检查都不会跑到的脚本，和它们都没读到的 JSON 数据（比如截图来源清单）
fn idle_script(file: &str, build: &[String]) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    let helper = ["mjs", "js", "cjs", "ts", "mts"].iter().any(|e| name.ends_with(&format!(".{e}"))) || (name.ends_with(".json") && !hard_forbidden(name));
    file.split('/').any(|s| s == "scripts") && helper && !build.iter().any(|b| b == name)
}

fn generated_by_build(file: &str, generated: &[String]) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    !hard_forbidden(file) && generated.iter().any(|g| g == name)
}

/// frontmatter 里改了网址（slug / permalink / url / aliases / redirect_from 的旧行被删）或把页面改成不可见
/// （draft → true、published → false、noindex → true）：返回那个键
fn url_or_unpublish_change(removed: &[String], added: &[String]) -> Option<&'static str> {
    // 只认 frontmatter 顶层的键：没有缩进、不是列表项（正文里 sources 列表的 `  - url:` 不算）
    fn kv(l: &str) -> Option<(String, String)> {
        if l.starts_with([' ', '\t']) {
            return None;
        }
        let t = l.trim();
        if t.starts_with('-') || t.starts_with('#') {
            return None;
        }
        let (k, v) = t.split_once(':')?;
        let k = k.trim().trim_matches(['"', '\'']).to_ascii_lowercase();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        Some((k, v.trim().trim_end_matches(',').trim_matches(['"', '\'']).to_ascii_lowercase()))
    }
    let before = |key: &str| removed.iter().filter_map(|l| kv(l)).find(|(k, _)| k == key).map(|(_, v)| v);
    let after = |key: &str| added.iter().filter_map(|l| kv(l)).find(|(k, _)| k == key).map(|(_, v)| v);
    for l in removed {
        let Some((k, _)) = kv(l) else { continue };
        match k.as_str() {
            "slug" => return Some("slug"),
            "permalink" => return Some("permalink"),
            "url" => return Some("url"),
            "aliases" => return Some("aliases"),
            "redirect_from" => return Some("redirect_from"),
            _ => {}
        }
    }
    // 改成不可见：原来不是（或原来没写这个键）、现在是
    if after("draft").as_deref() == Some("true") && before("draft").as_deref() != Some("true") {
        return Some("draft");
    }
    if after("published").as_deref() == Some("false") && before("published").as_deref() != Some("false") {
        return Some("published");
    }
    if after("noindex").as_deref() == Some("true") && before("noindex").as_deref() != Some("true") {
        return Some("noindex");
    }
    None
}

/// 这次改动里新建的页面文件（给「允许的动作」核对：方案没允许写新页面却新建了页面就拦）
pub fn new_page_files(diff: &str) -> Vec<String> {
    split_diff(diff).into_iter().filter(|f| f.new_file && is_page_file(&f.file)).map(|f| f.file).collect()
}

/// 这个站的页面是不是就放在根目录 docs/ 下：用文档工具建站（VitePress、Docusaurus 这类），或者直接发布 docs 目录。
pub fn docs_are_published(app_dir: &Path, out_dir: &str) -> bool {
    const TOOLS: &[&str] = &["vitepress", "vuepress", "docusaurus", "@docusaurus/core", "nextra", "rspress", "docsify", "docsify-cli", "fumadocs-core"];
    out_dir.trim_start_matches("./").split('/').next() == Some("docs") || app_dir.join("mkdocs.yml").is_file() || declared_deps(app_dir).iter().any(|d| TOOLS.contains(&d.as_str()))
}

/// 去掉 diff 里根目录 docs/ 下的 Markdown：别的站点放的是项目自己的说明、计划、核对记录，不会上线，
/// 上线判定、内容质检、线上验收的文字都不该算它们。从 docs/ 外面挪进来的不去掉（原来的页面没了，要照常判断）。
pub fn without_internal_docs(diff: &str) -> String {
    let mut out = String::with_capacity(diff.len());
    let mut keep = true;
    for line in diff.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let file = rest.trim_end().split(" b/").nth(1).unwrap_or(rest);
            keep = !(rest.starts_with("a/docs/") && file.starts_with("docs/") && [".md", ".mdx", ".markdown"].iter().any(|e| file.ends_with(e)));
        }
        if keep {
            out.push_str(line);
        }
    }
    out
}

/// 推荐相关的英文词（链接、字段名里出现）
const MONEY_WORDS: &[&str] = &["referral", "invite", "affiliate", "promo", "coupon", "rebate", "refcode", "ref_code", "kickback"];

/// 交易所官网上的说明页：帮助中心、常见问题、公告、博客、学院、条款。讲返佣规则的文章必然引用
/// 「推荐计划规则」这类页面，网址里天然带 referral / affiliate，但它们是给读者核对规则的来源，不是推荐链接。
const INFO_PATHS: &[&str] = &["/help", "/support", "/faq", "/learn", "/blog", "/academy", "/announcement", "/news", "/terms", "/legal", "/docs", "/rules", "/zendesk", "/hc/"];

/// 外部链接是不是说明页（见 INFO_PATHS）：只看路径，不看域名和查询参数
fn info_page_url(url: &str) -> bool {
    let path = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let path = path.find('/').map(|i| &path[i..]).unwrap_or("/");
    let path = path.split(['?', '#']).next().unwrap_or("/").to_lowercase();
    INFO_PATHS.iter().any(|p| path.contains(p))
}

/// 一行里真正带钱的部分：推荐 / 跳转链接（?ref=、/go/、带推荐词的外部链接）、data-ref 属性、写着的码（邀请码：GATE8888）、
/// 带推荐词的字段 / 变量 / 组件名（refCode、bn.inviteeKickback、ReferralBox）。空＝这行和钱无关。
/// 改动前后比较的就是这些：同样的链接和码只是挪了位置、换了旁边的说法，不算改动推荐内容；
/// 正文里讲「怎么填邀请码」「返佣是什么」、站内文章链接（/guides/invite-code-rebate/）都不算。
pub fn money_atoms(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let seps = |c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | '<' | '>' | '`' | ',' | '，' | '。' | '）' | '（' | '{' | '}');
    for t in line.split(seps).filter(|t| !t.is_empty()) {
        let l = t.to_lowercase();
        // 带推荐词的外部链接算推荐链接，但帮助中心、常见问题这类说明页不算（带 ?ref= 的照样算，不管路径）
        if l.contains("?ref=") || l.contains("&ref=") || l.contains("/go/") || (l.starts_with("http") && MONEY_WORDS.iter().any(|w| l.contains(w)) && !info_page_url(&l)) {
            out.push(t.trim_end_matches(['.', ';', ':', '!']).to_string());
        }
    }
    // data-ref-code="binance" 这类属性：属性名加值
    for (i, _) in line.match_indices("data-ref") {
        let rest = &line[i..];
        let end = rest.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(rest.len());
        out.push(rest[..end].to_string());
    }
    // 写着的码
    for k in ["邀请码", "推荐码", "优惠码", "返佣码"] {
        for (i, _) in line.match_indices(k) {
            let rest = line[i + k.len()..].trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '：' | ':' | '「' | '“' | '"' | '`' | '*'));
            let code: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if code.chars().count() >= 4 {
                out.push(format!("{k}{code}"));
            }
        }
    }
    // 字段 / 变量 / 组件名：驼峰、带下划线或点的标识符里有推荐词（英文正文里的 invite、Referral 不算）
    for t in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$')).filter(|t| !t.is_empty()) {
        let t = t.trim_matches('.');
        let l = t.to_lowercase();
        let ident = t.chars().skip(1).any(|c| c.is_ascii_uppercase()) || t.contains('_') || t.contains('.');
        if ident && !l.contains("http") && MONEY_WORDS.iter().any(|w| l.contains(w)) && !out.iter().any(|o| o.contains(t)) {
            // 字段带着值一起算（inviteCode: "ABC" → inviteCode=abc）：只换值不换字段名也能看出来
            let value = line.find(t).map(|i| &line[i + t.len()..]).and_then(|rest| {
                let r = rest.trim_start().trim_start_matches([':', '=']).trim_start();
                let v: String = r.trim_start_matches(['"', '\'', '`']).chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
                (!v.is_empty() && rest.trim_start().starts_with([':', '='])).then_some(v)
            });
            out.push(match value { Some(v) => format!("{t}={v}"), None => t.to_string() });
        }
    }
    out.dedup();
    out
}

/// 推荐 / 邀请 / 返佣相关的内容：改动里删改了带这些标记的行（见 `money_atoms`），按拦截等级判断要不要拦。
pub fn touches_money(line: &str) -> bool {
    !money_atoms(line).is_empty()
}

/// 改动里新加的推荐内容（链接、码、字段）：上线前查一下改动前的站点里有没有，已有的（比如换个位置再引用 refCode）不算新增。
pub fn added_money_atoms(diff: &str) -> Vec<String> {
    let mut v: Vec<String> = split_diff(diff).iter().flat_map(|f| f.added.iter().flat_map(|a| money_atoms(a))).collect();
    v.sort();
    v.dedup();
    v
}

/// 全托管的判定：返回不能自动上线的原因（空＝可以）。
/// 公开内容里不该出现的内部痕迹（开发注释、占位、内部说明）。只认很明确的标记，避免误伤正文。
const LEAK_MARKERS: &[&str] = &["TODO", "FIXME", "TBD", "lorem ipsum", "placeholder", "待补图", "待补充", "占位图", "占位文字", "上线前替换", "此处插入", "本文由 AI 生成", "按提示词", "via.placeholder.com", "dummyimage.com", "placehold.co"];

/// 文字里有没有内部痕迹（开发注释、占位、内部说明），有就返回是哪个标记。
/// 英文标记要整词：TODO / FIXME / TBD 这种代码里的写法还要大小写一致（西班牙语的 todo / todos 是「全部」，不能算）；
/// lorem ipsum、placeholder 不分大小写但也要整词；中文标记和占位图域名照旧按包含算。
/// 「待补充」是正常说法里也会出现的词（「认证待补充」＝认证还没补）：前面紧挨着汉字、后面又没接着汉字的不算，
/// 「（待补充）」「待补充：」「这里待补充数据」照样算。
pub fn leak_marker(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    let han = |c: char| ('\u{4e00}'..='\u{9fff}').contains(&c);
    LEAK_MARKERS.iter().copied().find(|m| {
        if *m == "待补充" {
            return text.match_indices(m).any(|(i, _)| !text[..i].chars().next_back().is_some_and(han) || text[i + m.len()..].chars().next().is_some_and(han));
        }
        if !m.is_ascii() || m.contains('.') {
            return lower.contains(&m.to_lowercase());
        }
        let code = m.chars().all(|c| c.is_ascii_uppercase());
        let (hay, needle) = if code { (text, m.to_string()) } else { (lower.as_str(), m.to_lowercase()) };
        hay.match_indices(needle.as_str()).any(|(i, _)| {
            let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
            !hay[..i].chars().next_back().is_some_and(word) && !hay[i + needle.len()..].chars().next().is_some_and(word)
        })
    })
}

/// 文字量：汉字按字、其它按词（粗算，只用于判断「新页面是不是太薄」）。
pub fn text_units(s: &str) -> usize {
    let cjk = s.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c) || ('\u{3040}'..='\u{30ff}').contains(c) || ('\u{ac00}'..='\u{d7af}').contains(c)).count();
    let words = s.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| w.len() >= 2).count();
    cjk + words
}


fn is_page_file(f: &str) -> bool {
    ["md", "mdx", "markdown", "html", "htm", "tmpl", "gohtml", "astro"].iter().any(|e| f.ends_with(&format!(".{e}")))
}

/// 去掉一行里的模板指令（Go 模板 / Nunjucks / Liquid 的 {{ … }} 和 {% … %}）：它们不是给读者看的文字
fn strip_template_tags(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    loop {
        let open = [rest.find("{{"), rest.find("{%")].into_iter().flatten().min();
        let Some(i) = open else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..i]);
        let close = if rest[i..].starts_with("{{") { "}}" } else { "%}" };
        match rest[i + 2..].find(close) {
            Some(j) => {
                out.push(' ');
                rest = &rest[i + 2 + j + 2..];
            }
            None => return out,
        }
    }
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

/// frontmatter 里一行的文字：很多站点把正文写在 frontmatter 的字段里（answer、steps、faqs 这类），也算正文。
/// 只取值、不算字段名；文件名、日期、网址、标签这种单个词的值不算。
fn frontmatter_prose(line: &str) -> String {
    let t = line.trim().trim_start_matches("- ").trim();
    let key_end = t.find(':').filter(|&i| t[..i].chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && i > 0);
    let v = key_end.map(|i| t[i + 1..].trim()).unwrap_or(t).trim_matches(['"', '\'', '|', '>']).trim();
    let cjk = v.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{ac00}'..='\u{d7af}').contains(&c));
    if cjk || v.contains(' ') { prose_of(v) } else { String::new() }
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

/// 模板页：正文不在模板里、在代码文件里的站点（Go 模板站把页面写在 internal/*/templates/*.gohtml，文字在 *.go 里）。
pub fn template_page_file(f: &str) -> bool {
    f.ends_with(".gohtml") || f.ends_with(".tmpl") || (f.split('/').any(|s| s == "templates") && (f.ends_with(".html") || f.ends_with(".htm")))
}

/// 一行代码里字符串字面量的文字（"…"、'…'、`…`），只留像正文的（中日韩文字 4 个字起；拉丁文字带空格、6 个词起）：
/// 页面正文写在 Go 文件、语言包 JSON 里的站点，质检靠这个拿到真正给读者看的文字。
pub fn literal_prose(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if !matches!(c, '"' | '`' | '\'') {
            continue;
        }
        let q = c;
        let mut s = String::new();
        let mut closed = false;
        while let Some(d) = chars.next() {
            if d == '\\' && q != '`' {
                if let Some(e) = chars.next() {
                    s.push(if matches!(e, 'n' | 't') { ' ' } else { e });
                }
                continue;
            }
            if d == q {
                closed = true;
                break;
            }
            s.push(d);
        }
        let t = s.trim();
        let cjk = t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{ac00}'..='\u{d7af}').contains(&c));
        // 中日韩文字 4 个字起算；拉丁文字要像句子（带空格、至少 6 个词），"text/plain; charset=utf-8" 这类不算
        if closed && ((cjk && text_units(t) >= 4) || (t.contains(' ') && text_units(t) >= 6)) {
            out.push_str(t);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

fn content_code_ext(f: &str) -> bool {
    ["go", "json", "ts", "tsx", "js", "jsx", "mjs", "cjs", "yml", "yaml", "toml", "py", "rb", "php"].iter().any(|e| f.ends_with(&format!(".{e}")))
}

/// 依赖清单、锁文件、工具配置：里面的文字（包描述、脚本）不是页面正文
fn manifest_file(f: &str) -> bool {
    let name = f.rsplit('/').next().unwrap_or(f);
    matches!(name, "package.json" | "package-lock.json" | "tsconfig.json" | "composer.json" | "go.mod" | "go.sum" | "Cargo.toml" | "Cargo.lock" | "pyproject.toml" | "pnpm-lock.yaml")
        || name.starts_with("wrangler")
        || name.contains(".config.")
}

/// 这次改动里所有代码文件（非页面文件、非依赖清单）新加行的正文字数，不设门槛：模板页的字数下限要把它算进去
pub fn code_prose_units(diff: &str) -> usize {
    split_diff(diff)
        .iter()
        .filter(|f| !f.deleted && content_code_ext(&f.file) && !is_page_file(&f.file) && !manifest_file(&f.file) && !test_file(f.file.rsplit('/').next().unwrap_or(&f.file)) && !f.file.contains(".pilot/"))
        .flat_map(|f| f.added.iter().map(|l| text_units(&literal_prose(l))))
        .sum()
}

/// 这次改动里新写进代码文件的正文：Go 文件、语言包 JSON 这类里新加行的字符串文字，加起来成段（≥ 200 字 / 词）的文件。
/// 返回（文件路径，新加的正文），最多 5 个。页面文件（见 content_page_files）不算在这里。
pub fn content_code_prose(diff: &str) -> Vec<(String, String)> {
    split_diff(diff)
        .into_iter()
        .filter(|f| !f.deleted && content_code_ext(&f.file) && !is_page_file(&f.file) && !manifest_file(&f.file) && !f.file.starts_with(".pilot/") && !f.file.contains("/.pilot/"))
        .filter_map(|f| {
            let text = f.added.iter().map(|l| literal_prose(l)).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n");
            (text_units(&text) >= 200).then_some((f.file, text))
        })
        .take(5)
        .collect()
}

/// 页面源文件 → （标题，正文纯文字）：去掉代码块、import 行和 HTML 标签；标题从 frontmatter 的 title: 取，
/// frontmatter 里其它字段的文字（answer、steps、faqs 这类，见 `frontmatter_prose`）也算正文。
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
            } else if !t.starts_with("description:") {
                let text = frontmatter_prose(t);
                if !text.trim().is_empty() {
                    body.push_str(text.trim());
                    body.push('\n');
                }
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
        let text = prose_of(&strip_template_tags(t));
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

/// 这次真正新加的行。页面里有几段一样的代码时，git 会把没动的行显示成「删掉又加上」，这些不算新加的。
fn net_added(f: &FileDiff) -> Vec<&String> {
    let mut gone: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for r in &f.removed {
        *gone.entry(r.trim()).or_default() += 1;
    }
    f.added
        .iter()
        .filter(|a| match gone.get_mut(a.trim()) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .collect()
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
    if t.is_empty() || t.starts_with("//") {
        return true;
    }
    // `* …` 多半是块注释里的一行，但 `* globalThis[...]()` 也是合法的乘法表达式（和上一行的数字接上）：
    // 带调用、下标、赋值、模板串的不算注释；`/* … */ 代码` 这种注释后面还有东西的也不算
    if t.starts_with('*') {
        // 去掉星号再看：有危险写法、或像 `[...](...)` / `(...)(...)` 这种调用的才不算注释
        let body = t.trim_start_matches('*').trim();
        return risky_code(body).is_none() && !body.contains("](") && !body.contains(")(") && !body.contains("['") && !body.contains("[\"");
    }
    if let Some(rest) = t.strip_prefix("/*") {
        return rest.split_once("*/").is_none_or(|(_, after)| after.trim().is_empty());
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
        // 同一行不能再跟别的语句（`import {a} from './x'; eval(...)`）
        let body = code.trim_end_matches(';').trim();
        return from_local && !body.contains(';') && !body.contains('(') && names.starts_with('{') && names.split('}').next().is_some_and(|n| n.chars().all(|c| c.is_alphanumeric() || "_$, {".contains(c)));
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

/// 自动上线时，单次改动已有文件的行数上限（「放开」档另有一个宽一些的）；新内容文件按新内容规则和正文下限判断，另有一个整体上限。
const MAX_CHANGED_LINES: usize = 400;
const MAX_CHANGED_LINES_OPEN: usize = 1500;
const MAX_NEW_FILE_LINES: usize = 3000;

/// 老的调用方式（测试里用）：按代码改动设置推等级——留给你确认＝稳妥，其它＝标准；只返回原因文字。
#[cfg(test)]
pub fn managed_verdict(diff: &str, allow_new_content: bool, min_page: usize, code: &str, content_code: &[String]) -> Vec<String> {
    let tier = if code == "review" { "strict" } else { "standard" };
    verdict(diff, &Rules { tier, code, allow: &[], new_content: allow_new_content, min_page, content_code, known: &[], generated: &[], build_scripts: None, deps: &[] }).into_iter().map(|b| b.text).collect()
}

/// sitemap 条目里的一行：<url> 块里的标签（网址、更新时间、频率、优先级、多语言和图片链接）或空行。
/// 上线判定 / 验收规则的版本：改了 verdict、classify、线上验收的判断（放宽或收紧）就加 1。
/// 等你确认的条目记着当时用的版本，旧版本判出来的会被调度按新规则自动重判一次（site_runner 调度里）。
pub const RULES_V: u32 = 9;

/// 全托管的自动上线判定：返回不能自动上线的原因（空＝可以）。方案放行的类型（`Rules::lets`）不算在里面。
pub fn verdict(diff: &str, rules: &Rules) -> Vec<Block> {
    let code = rules.code;
    let mut reasons: Vec<Block> = Vec::new();
    // 模板站新页面的正文有一部分写在同一次改动的代码文件里（Go 的 austrac_en.go 这类）：算字数时一起算
    let code_units: usize = code_prose_units(diff);
    let content_ext = ["md", "mdx", "markdown", "html", "htm", "tmpl", "gohtml", "astro", "njk", "liquid", "hbs", "vue", "svelte", "json", "svg", "xml", "png", "jpg", "jpeg", "webp", "avif"];
    let (mut changed, mut new_lines) = (0usize, 0usize);
    let (mut code_lines, mut code_files) = (0usize, 0usize);
    // 已有文件里删掉 / 加上的推荐内容（文件，行），最后整份比较
    let (mut money_out, mut money_in): (Vec<(String, String)>, Vec<(String, String)>) = (Vec::new(), Vec::new());
    for f in split_diff(diff) {
        let file = &f.file;
        // 构建脚本自动重写的数据文件（比如每次构建同步一遍提现手续费）：上线时构建还会再写一遍，不算这次的改动；
        // .pilot/ 是 Pilot 自己的记录（待办、站点说明），不上线
        if generated_by_build(file, rules.generated) || file.starts_with(".pilot/") || file.contains("/.pilot/") {
            continue;
        }
        // scripts/ 里构建和检查都不会跑的辅助脚本（比如放截图用的），只改了数据（说明文字、对照表）：不上线、也改不了及格线，按内容算
        let idle = rules.build_scripts.is_some_and(|b| idle_script(file, b));
        if idle && !f.deleted && f.added.iter().chain(&f.removed).all(|l| data_line(l)) {
            changed += f.added.len().min(f.removed.len()) + f.removed.len();
            new_lines += f.added.len().saturating_sub(f.removed.len());
            continue;
        }
        let data = data_file(file) && f.added.iter().chain(&f.removed).all(|l| data_line(l));
        // 站点代码：数据文件里混了代码，或模板 / 组件 / 样式 / 前端脚本。
        // 服务端代码（Go 站把页面和标题写在 internal/*.go 里这类）：放进站点清单的照代码设置判断；
        // 「代码改动：全部」时不用放清单，构建、检查、上线后验收兜底（Worker / Functions / API 路由除外）
        // 构建 / 检查 / 部署会跑到的脚本（不管放在哪个目录）：构建时在本机执行，任何等级都不自动改
        let in_build_chain = rules.build_scripts.is_some_and(|b| b.iter().any(|s| file.rsplit('/').next() == Some(s.as_str())));
        let allowed = hard_forbidden(file) && !never_allow(file) && (listed(file, rules.content_code) || (code == "all" && !edge_runtime(file)));
        let forbidden = (hard_forbidden(file) && !allowed) || in_build_chain;
        let is_code = !data && !forbidden && (allowed || data_file(file) || site_code_file(file) || test_file(file.rsplit('/').next().unwrap_or(file)));
        if forbidden {
            // 原因写准：构建会跑到的脚本、辅助脚本改了代码、其它一律不改的文件，分开说
            let why = if in_build_chain {
                "构建 / 检查 / 部署会跑到的脚本，构建时在本机执行，不自动改"
            } else if idle {
                "辅助脚本改了代码（只加截图清单这类数据行才放行），不自动上线"
            } else {
                "依赖、构建 / 部署配置、服务端代码不自动改"
            };
            reasons.push(block("", format!("{file}：{why}")));
        } else if redirect_file(file) {
            // 跳转文件：只加站内精确地址的 301 这类按内容算（目标存不存在、盖不盖住已有页面，构建完再对照产物查，见 redirect_problems）
            if let Err(why) = redirect_rules(&f) {
                reasons.push(block("", format!("{file}：{why}，跳转规则留给你确认")));
            }
        } else if is_code {
            code_files += 1;
            code_lines += f.added.len() + f.removed.len();
            if code == "review" {
                let line = f.added.iter().chain(&f.removed).find(|l| !data_line(l)).map(|l| l.trim().chars().take(60).collect::<String>()).unwrap_or_default();
                reasons.push(block("", format!("{file}：改了代码（比如「{line}」），这个方案的拦截等级是「稳妥」，代码改动留给你确认")));
            }
        } else if !data && !content_ext.iter().any(|e| file.ends_with(&format!(".{e}"))) {
            reasons.push(block("", format!("{file}：不是页面内容文件")));
        }
        // 页面文件里内嵌的脚本 / 事件 / iframe；.mdx 的 import / export 行是构建期执行的代码，按代码查。
        // SVG 和 XML（sitemap、RSS / Atom 订阅源）是发布出去的内容，没有东西执行它们，但里面一样能写 <script>
        if is_page_file(file) || file.ends_with(".svg") || file.ends_with(".xml") {
            let mut seen: Vec<&str> = Vec::new();
            for l in net_added(&f) {
                let t = l.trim_start();
                let markdown = [".md", ".mdx", ".markdown"].iter().any(|e| file.ends_with(e));
                let why = page_risky(l, markdown).or_else(|| if file.ends_with(".mdx") && (t.starts_with("import ") || t.starts_with("export ")) { risky_code(l) } else { None });
                let Some(why) = why else { continue };
                let kind = risky_kind(why);
                if seen.contains(&why) || rules.lets(kind) {
                    continue;
                }
                seen.push(why);
                reasons.push(block(kind, format!("{file}：页面里{why}（「{}」），留给你确认", l.trim().chars().take(60).collect::<String>())));
            }
            // 改网址、下架：slug / permalink 这些键一变旧网址就 404（没有跳转）；draft / published / noindex 往「不可见」的方向改，页面就消失。
            // 把草稿发布出去（draft: true → false、published: false → true）是正常任务，不拦
            let markdown = [".md", ".mdx", ".markdown"].iter().any(|e| file.ends_with(e));
            if markdown {
                if let Some(key) = url_or_unpublish_change(&f.removed, &f.added) {
                    reasons.push(block("", format!("{file}：改了 {key}（网址或发布状态），旧网址会失效或页面会消失，留给你确认")));
                }
            }
        }
        if is_code || template_file(file) {
            // 每种危险写法报一条；方案放行的类型跳过
            let mut seen: Vec<&str> = Vec::new();
            for l in net_added(&f) {
                let Some(why) = risky_code(l) else { continue };
                // import 的是站点本来就装了的依赖（比如 Next 站里 import next/link）：不是新引入的外部包
                if why == "引入了外部包" && import_package(l).is_some_and(|p| rules.deps.contains(&p)) {
                    continue;
                }
                let kind = risky_kind(why);
                if seen.contains(&why) || rules.lets(kind) {
                    continue;
                }
                seen.push(why);
                reasons.push(block(kind, format!("{file}：新加的代码{why}（「{}」），留给你确认", l.trim().chars().take(60).collect::<String>())));
            }
        }
        if f.deleted {
            reasons.push(block("", format!("{file}：删除文件不自动执行")));
            continue;
        }
        if f.new_file {
            new_lines += f.added.len();
            if !rules.new_content {
                reasons.push(block("", format!("{file}：新内容需要运营人员确认后发布")));
            }
        } else if !is_code {
            // 已有的内容 / 数据文件里多出来的行是新写的内容（比如往文章列表里加一整篇），按新增内容的上限算；被替换的行才算改动
            let replaced = f.added.len().min(f.removed.len());
            changed += f.removed.len() + replaced;
            new_lines += f.added.len() - replaced;
        } else {
            changed += f.added.len() + f.removed.len();
        }
        money_out.extend(f.removed.iter().filter(|r| touches_money(r)).map(|r| (file.clone(), r.clone())));
        // 新文件里的推荐内容也要查（新文章最容易把别人的推荐码带出来）：站点本来就有的链接 / 码（known）不算
        let fresh = |a: &String| touches_money(a) && (!f.new_file || money_atoms(a).iter().any(|at| !rules.known.contains(at)));
        money_in.extend(f.added.iter().filter(|a| fresh(a)).map(|a| (file.clone(), a.clone())));
        // .json 数据文件：不是页面也不算 data 文件，但字符串里的内部用语一样会上线
        let jsonish = file.ends_with(".json") && !data;
        let page = is_page_file(file) || data || jsonish;
        if page {
            // 页面文件看整行；数据文件只看字符串里的文字
            let texts: Vec<String> = f.added.iter().map(|a| if data || jsonish { split_strings(a).map(|x| x.1).unwrap_or_default() } else { a.clone() }).collect();
            if !rules.lets("leak") {
                for t in &texts {
                    if let Some(m) = leak_marker(t) {
                        reasons.push(block("leak", format!("{file}：正文里像是混进了内部说明或占位（「{m}」），留给你确认")));
                    }
                }
            }
            if f.new_file && !jsonish && rules.min_page > 0 && !rules.lets("thin") {
                let min_page = rules.min_page;
                let mut n = 0;
                let mut in_frontmatter = false;
                for t in &texts {
                    let t = t.trim();
                    if !data && t == "---" {
                        in_frontmatter = !in_frontmatter;
                    } else if in_frontmatter && !data {
                        n += text_units(&frontmatter_prose(t));
                    } else if data || (!t.starts_with("```") && !t.starts_with("import ")) {
                        n += text_units(&prose_of(&strip_template_tags(t)));
                    }
                }
                if template_page_file(file) {
                    n += code_units;
                }
                if n < min_page {
                    reasons.push(block("thin", format!("{file}：新页面正文约 {n} 字，低于这个方案的下限 {min_page} 字，可能讲不完整、容易被当成低质页面，留给你确认（术语解释这类短页可以直接放行）")));
                }
            }
        }
    }
    reasons.extend(money_blocks(rules, &money_out, &money_in));
    if code == "small" && (code_lines > SMALL_CODE_LINES || code_files > SMALL_CODE_FILES) {
        reasons.push(block("", format!("代码改动 {code_lines} 行、{code_files} 个文件，超过「小改动」的上限（{SMALL_CODE_LINES} 行、{SMALL_CODE_FILES} 个文件）")));
    }
    let max = rules.max_changed();
    if changed > max {
        reasons.push(block("size", format!("改动已有文件 {changed} 行，超过自动上线的上限（{max} 行）")));
    }
    if new_lines > MAX_NEW_FILE_LINES && !rules.lets("size") {
        reasons.push(block("size", format!("新增文件共 {new_lines} 行，超过自动上线的上限（{MAX_NEW_FILE_LINES} 行）")));
    }
    reasons.sort_by(|a, b| a.text.cmp(&b.text));
    reasons.dedup_by(|a, b| a.text == b.text);
    reasons
}

/// 推荐内容的判定。稳妥：已有文件里删改了带推荐标记的行就拦。标准：改动前后的推荐链接、码、字段不一样才拦（只是挪位置、
/// 改了旁边的说法不算；改动前的站点里本来就有的不算新增，见 `Rules::known`）。放开：只拦删掉或换掉的。点过「这类以后不拦」的都不拦。
/// 每个文件只报一条，列出前几处。
fn money_blocks(rules: &Rules, out: &[(String, String)], ins: &[(String, String)]) -> Vec<Block> {
    if rules.lets("money") || (out.is_empty() && ins.is_empty()) {
        return Vec::new();
    }
    let atoms = |v: &[(String, String)]| v.iter().flat_map(|(f, l)| money_atoms(l).into_iter().map(move |a| (f.clone(), a))).collect::<Vec<_>>();
    let (mut before, mut after) = (atoms(out), atoms(ins));
    if rules.tier != "strict" {
        // 两边都有的抵掉（同一个链接 / 码，挪了位置或换了旁边的说法）
        before.retain(|(_, a)| match after.iter().position(|(_, b)| b == a) {
            Some(i) => {
                after.remove(i);
                false
            }
            None => true,
        });
        after.retain(|(_, a)| !rules.known.contains(a));
        if rules.tier == "open" {
            after.clear();
        }
    }
    let mut files: Vec<&String> = before.iter().chain(&after).map(|(f, _)| f).collect();
    files.sort();
    files.dedup();
    let list = |v: &[(String, String)], f: &str| {
        let xs: Vec<String> = v.iter().filter(|(g, _)| g == f).map(|(_, a)| format!("「{}」", a.chars().take(40).collect::<String>())).collect();
        match xs.len() {
            0 => String::new(),
            n if n > 3 => format!("{} 等 {n} 处", xs[..3].join("")),
            _ => xs.join(""),
        }
    };
    files
        .into_iter()
        .map(|f| {
            let (gone, new) = (list(&before, f), list(&after, f));
            let parts: Vec<String> = [(gone, "删掉或换掉了"), (new, "新增了")].into_iter().filter(|(x, _)| !x.is_empty()).map(|(x, w)| format!("{w}{x}")).collect();
            block("money", format!("{f}：推荐 / 邀请内容有变动，{}", parts.join("，")))
        })
        .collect()
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
    #[serde(default, deserialize_with = "lenient_u32")]
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
/// 模型把 priority 写成 "1"、1.5、null 都认
fn lenient_u32<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0).max(0.0) as u32,
        serde_json::Value::String(s) => s.trim().parse::<f64>().unwrap_or(0.0).max(0.0) as u32,
        _ => 0,
    })
}

/// 回复里最后一个 ```json 块（模型读站点时可能先展示过别的 JSON）
fn last_json_block(reply: &str) -> Option<String> {
    let i = reply.rfind("```json")?;
    Some(reply[i + 7..].split("```").next().unwrap_or("").to_string())
}

#[cfg(test)]
pub fn parse_plan_full(reply: &str) -> Result<(String, Vec<PlanTask>), String> {
    parse_plan_counted(reply).map(|(p, t, _)| (p, t))
}

/// 同 parse_plan_full，另外返回丢掉了几条（写坏的、没标题的、超过 12 条的），好在作业日志里说一声。
pub fn parse_plan_counted(reply: &str) -> Result<(String, Vec<PlanTask>, usize), String> {
    let body = if let Some(b) = last_json_block(reply) {
        b
    } else {
        let a = reply.find(['{', '[']).ok_or("回复里没有计划（缺少 JSON）")?;
        let b = reply.rfind(['}', ']']).ok_or("回复里没有计划（缺少 JSON）")?;
        reply[a..=b].to_string()
    };
    let v: serde_json::Value = serde_json::from_str(body.trim()).map_err(|e| format!("计划 JSON 无法解析：{e}"))?;
    let positioning = v["positioning"].as_str().unwrap_or_default().to_string();
    let (tasks, dropped) = parse_plan_with_dropped(&format!("```json\n{}\n```", if v.is_array() { v.to_string() } else { v["tasks"].to_string() }))?;
    Ok((positioning, tasks, dropped))
}

/// 计划回复里给运营人员的说明（notes 字段，没有就是空串）。
pub fn parse_plan_notes(reply: &str) -> String {
    let Some(body) = last_json_block(reply) else { return String::new() };
    serde_json::from_str::<serde_json::Value>(body.trim()).ok().and_then(|v| v["notes"].as_str().map(|s| s.trim().chars().take(1000).collect())).unwrap_or_default()
}

#[cfg(test)]
pub fn parse_plan(reply: &str) -> Result<Vec<PlanTask>, String> {
    parse_plan_with_dropped(reply).map(|(t, _)| t)
}

fn parse_plan_with_dropped(reply: &str) -> Result<(Vec<PlanTask>, usize), String> {
    let body = if let Some(b) = last_json_block(reply) {
        b
    } else {
        let a = reply.find('[').ok_or("回复里没有计划（缺少 JSON）")?;
        let b = reply.rfind(']').ok_or("回复里没有计划（缺少 JSON）")?;
        reply[a..=b].to_string()
    };
    let v: serde_json::Value = serde_json::from_str(body.trim()).map_err(|e| format!("计划 JSON 无法解析：{e}"))?;
    let arr = if v.is_array() { v } else { v["tasks"].clone() };
    // 一条写坏了（缺 title、字段类型不对）只丢那一条，不让整份计划作废
    let items = arr.as_array().cloned().ok_or("计划格式不对：tasks 不是数组")?;
    let total = items.len();
    let mut tasks: Vec<PlanTask> = items.into_iter().filter_map(|t| serde_json::from_value(t).ok()).collect();
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
    let dropped = total - tasks.len();
    Ok((tasks, dropped))
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
    fn notes_in_the_docs_folder_are_not_pages() {
        let note = "diff --git a/docs/evidence/guide.md b/docs/evidence/guide.md\nnew file mode 100644\n--- /dev/null\n+++ b/docs/evidence/guide.md\n@@ -0,0 +1 @@\n+# US stock perpetuals comparison guide\n";
        let page = "diff --git a/internal/web/learn/a.html b/internal/web/learn/a.html\n--- a/internal/web/learn/a.html\n+++ b/internal/web/learn/a.html\n@@ -1 +1 @@\n-旧\n+新\n";
        let both = format!("{note}{page}");
        assert!(managed_verdict(note, true, 1500, "all", &[]).iter().any(|r| r.contains("低于这个方案的下限")));
        let kept = without_internal_docs(&both);
        assert_eq!(kept, page);
        assert!(managed_verdict(&kept, true, 1500, "all", &[]).is_empty());
        assert!(new_page_files(&kept).is_empty());
        // docs 下不是 Markdown 的、别的目录里的 docs、从外面挪进 docs 的都照常判断
        for keep in ["diff --git a/docs/a.html b/docs/a.html\n+x\n", "diff --git a/site/docs/a.md b/site/docs/a.md\n+x\n", "diff --git a/src/content/a.md b/docs/a.md\n-x\n"] {
            assert_eq!(without_internal_docs(keep), keep);
        }
    }

    #[test]
    fn a_docs_site_publishes_its_docs_folder() {
        let dir = std::env::temp_dir().join(format!("pilot-docs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!docs_are_published(&dir, "dist"));
        assert!(docs_are_published(&dir, "docs"));
        assert!(docs_are_published(&dir, "./docs/.vitepress/dist"));
        std::fs::write(dir.join("package.json"), r#"{"devDependencies":{"astro":"5"}}"#).unwrap();
        assert!(!docs_are_published(&dir, "dist"));
        std::fs::write(dir.join("package.json"), r#"{"devDependencies":{"vitepress":"1"}}"#).unwrap();
        assert!(docs_are_published(&dir, "dist"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thin_new_pages_and_leaked_internal_notes_are_not_auto_shipped() {
        let thin = "diff --git a/src/content/a.md b/src/content/a.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/a.md\n@@ -0,0 +1,4 @@\n+---\n+title: 很短的页面\n+---\n+只有一句话。\n";
        let r = managed_verdict(thin, true, 1500, "review", &[]);
        assert!(r.iter().any(|x| x.contains("低于这个方案的下限")), "{r:?}");
        assert!(managed_verdict(thin, true, 0, "review", &[]).is_empty(), "下限 0＝不检查");
        let body: String = "这是一段足够长的正文，讲清楚了具体步骤、注意事项和常见错误。".repeat(60);
        let full = format!("diff --git a/src/content/b.md b/src/content/b.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/b.md\n@@ -0,0 +1,4 @@\n+---\n+title: 完整的页面\n+---\n+{body}\n");
        assert!(managed_verdict(&full, true, 1500, "review", &[]).is_empty(), "{:?}", managed_verdict(&full, true, 1500, "review", &[]));
        let leak = "diff --git a/src/content/c.md b/src/content/c.md\n--- a/src/content/c.md\n+++ b/src/content/c.md\n@@ -1 +1 @@\n-旧\n+步骤三（待补图）点击确认\n";
        assert!(managed_verdict(leak, true, 1500, "review", &[]).iter().any(|x| x.contains("待补图")));
        assert_eq!(text_units("Gate KYC 认证"), 4);
        // 模板站（vibcg AUSTRAC 英文页）：模板里的正文 + 同一次新加的 Go 文件里的正文一起算；模板指令不算
        let para = |i: usize| format!("Registered exchanges in Australia must enrol with AUSTRAC and keep records number {i} for seven years.");
        let tpl: String = (0..14).map(|i| format!("+<p>{}</p>\n+<p>{{{{ .Updated }}}}</p>\n", para(i))).collect();
        let go: String = (0..12).map(|i| format!("+\t\tBody: \"{}\",\n", para(i))).collect();
        let page = format!("diff --git a/internal/webapp/templates/austrac-en.gohtml b/internal/webapp/templates/austrac-en.gohtml\nnew file mode 100644\n--- /dev/null\n+++ b/internal/webapp/templates/austrac-en.gohtml\n@@ -0,0 +1,28 @@\n{tpl}");
        let gof = format!("diff --git a/internal/webapp/austrac_en.go b/internal/webapp/austrac_en.go\nnew file mode 100644\n--- /dev/null\n+++ b/internal/webapp/austrac_en.go\n@@ -0,0 +1,12 @@\n{go}");
        let thin_of = |d: &str| managed_verdict(d, true, 300, "all", &[]).into_iter().filter(|x| x.contains("低于这个方案的下限")).collect::<Vec<_>>();
        assert!(!thin_of(&page).is_empty(), "只有模板：偏短");
        assert!(thin_of(&(page.clone() + &gof)).is_empty(), "{:?}", thin_of(&(page.clone() + &gof)));
        // Markdown 页面不加代码文件的字数
        let md = "diff --git a/src/content/a.md b/src/content/a.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/a.md\n@@ -0,0 +1 @@\n+只有一句话。\n";
        assert!(!thin_of(&(md.to_string() + &gof)).is_empty());
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
mod sitemap_tests {
    use super::*;
    #[test]
    fn lines_git_shows_as_removed_and_added_are_not_new_scripts() {
        let script = "<script async src=\"https://www.googletagmanager.com/gtag/js?id=G-1\"></script>";
        let diff = |added: &[&str], removed: &[&str]| {
            let mut d = String::from("diff --git a/site/a/index.html b/site/a/index.html\n--- a/site/a/index.html\n+++ b/site/a/index.html\n@@ -1 +1 @@\n");
            removed.iter().for_each(|l| d.push_str(&format!("-{l}\n")));
            added.iter().for_each(|l| d.push_str(&format!("+{l}\n")));
            d
        };
        let r = Rules { tier: "standard", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: None, deps: &[] };
        let has_script = |d: &str| verdict(d, &r).iter().any(|b| b.text.contains("脚本"));
        // 没动的脚本：缩进不同也一样
        assert!(!has_script(&diff(&[script, "<script>", "<p>新内容</p>"], &[&format!("  {script}"), "<script>", "<p>旧内容</p>"])));
        // 真的新加了一个，或者比原来多一个：照拦
        assert!(has_script(&diff(&[script], &["<p>旧内容</p>"])));
        assert!(has_script(&diff(&[script, script], &[script])));
    }
    #[test]
    fn imports_of_declared_deps_and_type_imports_are_not_new_packages() {
        assert_eq!(import_package("import type { Metadata } from \"next\";"), None);
        assert_eq!(import_package("import Link from 'next/link';").as_deref(), Some("next"));
        assert_eq!(import_package("import { x } from '@scope/pkg/sub';").as_deref(), Some("@scope/pkg"));
        assert_eq!(import_package("import a from './a';"), None);
        let d = "diff --git a/src/views/a.tsx b/src/views/a.tsx\n--- a/src/views/a.tsx\n+++ b/src/views/a.tsx\n@@ -1 +1,2 @@\n+import Link from 'next/link';\n+import fs from 'fs';\n";
        let deps = vec!["next".to_string()];
        let r = Rules { tier: "standard", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: None, deps: &deps };
        let v: Vec<String> = verdict(d, &r).into_iter().map(|b| b.text).collect();
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].contains("fs"), "{v:?}");
    }
    #[test]
    fn feeds_and_manifests_are_published_content() {
        // RSS 里跟着页面改标题：发布出去的内容，不是「不是页面内容文件」
        let rss = "diff --git a/legacy/site/en/rss.xml b/legacy/site/en/rss.xml\n--- a/legacy/site/en/rss.xml\n+++ b/legacy/site/en/rss.xml\n@@ -105 +105 @@\n-      <title>OKX vs Binance for Beginners: How to Compare</title>\n+      <title>OKX vs Binance for Beginners: Fees, KYC and Which to Pick (2026)</title>\n";
        let r = Rules { tier: "standard", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: None, deps: &[] };
        assert!(verdict(rss, &r).is_empty(), "{:?}", verdict(rss, &r).iter().map(|b| b.text.clone()).collect::<Vec<_>>());
        let bad = rss.replace("+      <title>OKX vs Binance for Beginners: Fees, KYC and Which to Pick (2026)</title>", "+      <description><![CDATA[<script src=\"https://evil.example/x.js\"></script>]]></description>");
        assert!(verdict(&bad, &r).iter().any(|b| b.text.contains("rss.xml") && b.text.contains("脚本")));
    }
    #[test]
    fn redirect_rules_are_judged_by_content() {
        let r = Rules { tier: "standard", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: None, deps: &[] };
        let diff = |lines: &str| format!("diff --git a/public/_redirects b/public/_redirects\n--- a/public/_redirects\n+++ b/public/_redirects\n@@ -1,2 +1,5 @@\n{lines}");
        let texts = |d: &str| verdict(d, &r).into_iter().map(|b| b.text).collect::<Vec<_>>();
        // 只加站内精确地址的 301：按内容算，不拦
        let ok = diff("+# 老站 .html 旧地址\n+/zh/kyc.html   /posts/kyc-failed/   301\n+/:lang/kyc.html   /en/posts/kyc-failed/   301\n+/zh   /\n /vi/about /vi/gioi-thieu/ 301\n");
        assert!(texts(&ok).is_empty(), "{:?}", texts(&ok));
        assert_eq!(redirects_added(&ok).len(), 3);
        assert_eq!(redirects_added(&ok)[2], Redirect { from: "/zh".into(), to: "/".into(), status: 301 });
        // 删改已有规则、改写 / 代理、通配、跳外站、带条件：留给人
        for (bad, why) in [
            ("-/vi/about /vi/gioi-thieu/ 301\n+/vi/about /vi/about-us/ 301\n", "删掉或改了已有"),
            ("+/feed/ /feed.xml 200\n", "改写"),
            ("+/old/* /new/:splat 301\n", "精确地址"),
            ("+/promo https://evil.example/ 301\n", "站内路径"),
            ("+/promo //evil.example/ 301\n", "站内路径"),
            ("+/a /b 301!\n", "强制标记"),
            ("+/a /b 301 Country=us\n", "条件参数"),
        ] {
            let t = texts(&diff(bad));
            assert!(t.iter().any(|x| x.contains("_redirects") && x.contains(why)), "{bad}: {t:?}");
        }
        // 跳转去推荐链接：按推荐内容的规则拦
        assert!(texts(&diff("+/old /go/binance 301\n")).iter().any(|x| x.contains("推荐")), "{:?}", texts(&diff("+/old /go/binance 301\n")));
        // 对照构建产物：目标要存在，来源不能盖住已有页面；占位段按每段对
        let built: Vec<String> = ["index.html", "posts/kyc-failed/index.html", "en/kyc.html", "vi/posts/x/index.html"].iter().map(|s| s.to_string()).collect();
        let rules = redirects_added(&ok);
        let p = redirect_problems(&rules, &built);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].contains("/en/posts/kyc-failed/") && p[0].contains("没有这个页面"), "{p:?}");
        assert!(p[1].contains("/:lang/kyc.html") && p[1].contains("盖住已有页面 /en/kyc.html"), "{p:?}");
        assert!(redirect_problems(&[Redirect { from: "/en/kyc".into(), to: "/vi/posts/x".into(), status: 301 }], &built)[0].contains("盖住"));
        // 目标是另一条新规则的来源（链式）、目标带占位符
        assert!(redirect_problems(&[Redirect { from: "/a".into(), to: "/b".into(), status: 301 }, Redirect { from: "/b".into(), to: "/".into(), status: 301 }], &built).is_empty());
        assert!(redirect_problems(&[Redirect { from: "/a/:x".into(), to: "/b/:x".into(), status: 301 }], &built)[0].contains("占位符"));
        // 构建产物里的 _redirects 接住的网址
        let dir = std::env::temp_dir().join(format!("pilot-redir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("_redirects"), "/zh/kyc.html /posts/kyc-failed/ 301\n/:lang/old.html /en/ 301\n/legacy/* /new/:splat 301\n").unwrap();
        assert!(redirected_in(&dir, "/zh/kyc.html") && redirected_in(&dir, "/vi/old.html") && redirected_in(&dir, "/legacy/a/b.html"));
        assert!(!redirected_in(&dir, "/zh/kyc") && !redirected_in(&dir, "/posts/kyc-failed/"));
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn sitemap_entry_edits_count_as_content() {
        let ok = "diff --git a/public/sitemap-legacy.xml b/public/sitemap-legacy.xml\n--- a/public/sitemap-legacy.xml\n+++ b/public/sitemap-legacy.xml\n@@ -225 +225 @@\n-    <lastmod>2026-08-25T14:34:26Z</lastmod>\n+    <lastmod>2026-11-03T02:00:00Z</lastmod>\n";
        let r = Rules { tier: "standard", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: None, deps: &[] };
        assert!(!verdict(ok, &r).iter().any(|b| b.text.contains("sitemap")), "{:?}", verdict(ok, &r).iter().map(|b| b.text.clone()).collect::<Vec<_>>());
        let bad = ok.replace("+    <lastmod>2026-11-03T02:00:00Z</lastmod>", "+<script>alert(1)</script>");
        assert!(verdict(&bad, &r).iter().any(|b| b.text.contains("sitemap-legacy.xml")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leak_markers_need_whole_words() {
        // 西班牙语的 todo / todos、英文的 placeholders 里带的词不算
        assert_eq!(leak_marker("no implica que todos los productos estén disponibles"), None);
        assert_eq!(leak_marker("Todo sobre Binance en Argentina"), None);
        // 真的开发注释、占位
        assert_eq!(leak_marker("TODO: 补上手续费表"), Some("TODO"));
        assert_eq!(leak_marker("价格（TBD）"), Some("TBD"));
        assert_eq!(leak_marker("Lorem ipsum dolor sit amet"), Some("lorem ipsum"));
        assert_eq!(leak_marker("这里待补充数据"), Some("待补充"));
        assert_eq!(leak_marker("价格（待补充）"), Some("待补充"));
        assert_eq!(leak_marker("待补充：手续费表"), Some("待补充"));
        // 正常说法：认证还没补
        assert_eq!(leak_marker("提示需要补充身份认证 认证待补充 完成认证"), None);
        assert!(leak_marker("img https://via.placeholder.com/300").is_some());
    }

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
        assert!(managed_verdict(ok, false, 1500, "review", &[]).is_empty());
        let code = "diff --git a/worker/index.ts b/worker/index.ts\n--- a/worker/index.ts\n+++ b/worker/index.ts\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(!managed_verdict(code, true, 1500, "review", &[]).is_empty());
        let cfg = "diff --git a/package.json b/package.json\n--- a/package.json\n+++ b/package.json\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(!managed_verdict(cfg, true, 1500, "review", &[]).is_empty());
        let money = "diff --git a/src/pages/a.astro b/src/pages/a.astro\n--- a/src/pages/a.astro\n+++ b/src/pages/a.astro\n@@ -9 +9 @@\n-<a href=\"/go/gate\">邀请码 ABC</a>\n+<a href=\"/go/gate\">邀请码 XYZ</a>\n";
        assert!(managed_verdict(money, true, 1500, "review", &[]).iter().any(|r| r.contains("推荐")));
        let newpage = "diff --git a/src/content/new.md b/src/content/new.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/new.md\n@@ -0,0 +1 @@\n+# 新页面\n";
        assert!(managed_verdict(newpage, false, 1500, "review", &[]).iter().any(|r| r.contains("新内容")));
        // 允许自动发布新页面时，只剩「低于正文下限」这一条（这个测试页只有一行）
        assert!(managed_verdict(newpage, true, 1500, "review", &[]).iter().all(|r| r.contains("低于这个方案的下限")));
    }

    #[test]
    fn build_generated_data_is_not_an_edit() {
        let dir = std::env::temp_dir().join(format!("pilot-build-out-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"scripts":{"prebuild":"node scripts/check.mjs && NODE_USE_ENV_PROXY=1 node scripts/sync-fees.mjs --soft","build":"astro build"}}"#).unwrap();
        std::fs::write(dir.join("scripts/sync-fees.mjs"), "const OUT = new URL('../src/data/withdrawal-fees.json', import.meta.url);\nconst C = new URL(\"../src/data/fee-changes.json\", import.meta.url);\nreadFileSync('package.json');\nconst d = await res.json();").unwrap();
        let outs = build_outputs(&dir);
        assert_eq!(outs, vec!["fee-changes.json".to_string(), "withdrawal-fees.json".to_string()]);
        let rows: String = (0..450).map(|i| format!("-  \"a{i}\": 1,\n+  \"a{i}\": 2,\n")).collect();
        let diff = format!("diff --git a/site/src/data/withdrawal-fees.json b/site/src/data/withdrawal-fees.json\n--- a/site/src/data/withdrawal-fees.json\n+++ b/site/src/data/withdrawal-fees.json\n@@ -1 +1 @@\n{rows}");
        let r = |g: &'static [String]| Rules { tier: "standard", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: g, build_scripts: None, deps: &[] };
        assert!(!verdict(&diff, &r(&[])).is_empty());
        let g: &'static [String] = Box::leak(outs.into_boxed_slice());
        assert!(verdict(&diff, &r(g)).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn idle_helper_scripts_with_only_data_pass() {
        let dir = std::env::temp_dir().join(format!("pilot-build-scripts-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"scripts":{"build":"astro build && node scripts/postbuild.mjs","check":"node scripts/check.mjs"}}"#).unwrap();
        std::fs::write(dir.join("scripts/check.mjs"), "import { rules } from './rules.mjs';").unwrap();
        let b = build_scripts(&dir, &["npm run check".into()]).unwrap();
        assert_eq!(b, vec!["check.mjs".to_string(), "postbuild.mjs".to_string(), "rules.mjs".to_string()]);
        let b: &'static [String] = Box::leak(b.into_boxed_slice());
        let r = Rules { tier: "standard", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: Some(b), deps: &[] };
        let d = |file: &str, line: &str| format!("diff --git a/{file} b/{file}\n--- a/{file}\n+++ b/{file}\n@@ -39 +39,2 @@\n+{line}\n");
        // 放截图用的辅助脚本里加一条说明文字：放行
        assert!(verdict(&d("scripts/place-shots.mjs", "  'binance-register-form': '币安网页版注册首屏（2026 年 11 月）',"), &r).is_empty());
        // 加的是代码、或者是构建 / 检查会跑的脚本：照样拦，原因分开说
        let why = |diff: &str, r: &Rules| verdict(diff, r).into_iter().map(|b| b.text).collect::<Vec<_>>().join("；");
        assert!(why(&d("scripts/place-shots.mjs", "  await rm(dir, { recursive: true });"), &r).contains("辅助脚本改了代码"));
        assert!(why(&d("scripts/rules.mjs", "  'min': '0',"), &r).contains("构建 / 检查 / 部署会跑到的脚本"));
        assert!(why(&d("scripts/place-shots.mjs", "  'a': 'b',"), &Rules { build_scripts: None, ..r }).contains("服务端代码不自动改"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_in_scripts_only_blocks_when_the_build_reads_it() {
        let dir = std::env::temp_dir().join(format!("pilot-script-json-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::create_dir_all(dir.join("src/data")).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"scripts":{"build":"node scripts/build.mjs --list scripts/args.json","shots":"node scripts/capture.mjs"}}"#).unwrap();
        std::fs::write(dir.join("scripts/build.mjs"), "const a = new URL('./read.json', import.meta.url); const b = new URL('../src/data/fees.json', import.meta.url);").unwrap();
        std::fs::write(dir.join("scripts/capture.mjs"), "const a = new URL('./screenshots.json', import.meta.url);").unwrap();
        for f in ["scripts/read.json", "scripts/args.json", "scripts/screenshots.json", "src/data/fees.json"] {
            std::fs::write(dir.join(f), "[]").unwrap();
        }
        let b = build_scripts(&dir, &[]).unwrap();
        assert_eq!(b, vec!["args.json".to_string(), "build.mjs".to_string(), "read.json".to_string()]);
        let b: &'static [String] = Box::leak(b.into_boxed_slice());
        let r = Rules { tier: "standard", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: Some(b), deps: &[] };
        let d = |file: &str, lines: &[&str]| format!("diff --git a/{file} b/{file}\n--- a/{file}\n+++ b/{file}\n@@ -366 +366,{} @@\n{}\n", lines.len(), lines.iter().map(|l| format!("+{l}")).collect::<Vec<_>>().join("\n"));
        // vsvar 登记截图来源的那几行：只有截图工具读，放行
        let shots = ["  },", "  {", "    \"id\": \"binance-eth-staking\",", "    \"slug\": \"crypto-staking\",", "    \"url\": \"https://www.binance.com/zh-CN/eth2\",", "    \"wait\": 10000,", "    \"hide\": [", "      \"[class*='modal' i]\"", "    ]"];
        assert!(verdict(&d("scripts/screenshots.json", &shots), &r).is_empty(), "{:?}", verdict(&d("scripts/screenshots.json", &shots), &r));
        // 构建读到的（脚本里读、命令行传进去）、配置文件、读不到 package.json：照样拦
        for f in ["scripts/read.json", "scripts/args.json", "scripts/package.json", "scripts/tsconfig.json"] {
            assert!(!verdict(&d(f, &["  \"a\": 1,"]), &r).is_empty(), "{f}");
        }
        assert!(!verdict(&d("scripts/screenshots.json", &shots), &Rules { build_scripts: None, ..r }).is_empty());
        // 构建读到的 JSON 不在 scripts/ 里：不算构建脚本，和以前一样
        assert!(!b.iter().any(|n| n == "fees.json"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prose_written_into_code_files_is_found_for_the_quality_check() {
        // Go 内容文件里的字符串：正文留下，标识符、短字符串、格式串不要
        assert_eq!(literal_prose("\t\t\tHeading:   \"香港持牌虚拟资产交易所名单与查询方法\","), "香港持牌虚拟资产交易所名单与查询方法");
        assert_eq!(literal_prose("\tw.Header().Set(\"Content-Type\", \"text/plain; charset=utf-8\")"), "");
        assert_eq!(literal_prose("\treturn l.T(\"fees.title.venue\", heading)"), "");
        // 语言包 JSON：键不要，值要；转义的引号和换行处理掉
        assert_eq!(literal_prose("  \"fees.desc\": \"Maker fees start at {0} and taker at {1}, so a \\\"$10,000\\\" order costs {2}.\","), "Maker fees start at {0} and taker at {1}, so a \"$10,000\" order costs {2}.");
        // 模板里的指令不算正文
        assert_eq!(strip_template_tags("<p>{{ .Lede }} 本页最后更新：{{ .Updated | date }}</p>").trim(), "<p>  本页最后更新： </p>");

        let d = |file: &str, lines: &[&str]| format!("diff --git a/{file} b/{file}\n--- a/{file}\n+++ b/{file}\n@@ -1 +1,{} @@\n{}\n", lines.len(), lines.iter().map(|l| format!("+{l}")).collect::<Vec<_>>().join("\n"));
        let long: Vec<String> = (0..12).map(|i| format!("\t\t\tP{i}: \"第 {i} 段：香港证监会持牌的虚拟资产交易平台名单按官网核对，入金前请自己再打开一次官方名录确认平台状态没有变化。\",")).collect();
        let long: Vec<&str> = long.iter().map(String::as_str).collect();
        let diff = d("internal/webapp/region_content.go", &long) + &d("internal/webapp/handlers.go", &["\tif x == nil {", "\t\treturn l.T(\"a.b\", y)", "\t}"]) + &d("internal/webapp/templates/region.gohtml", &["<h1>{{ .Heading }}</h1>"]) + &d("package.json", &["  \"description\": \"这个字段的文字也不少，但 package.json 一律不算页面正文，不该出现在质检里。\","]);
        let got = content_code_prose(&diff);
        assert_eq!(got.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), vec!["internal/webapp/region_content.go"]);
        assert!(got[0].1.starts_with("第 0 段：香港证监会"));
        assert!(template_page_file("internal/webapp/templates/region-regulation.gohtml") && template_page_file("views/templates/page.html") && !template_page_file("src/content/a.md"));
    }

    #[test]
    fn frontmatter_fields_count_as_body() {
        // 正文写在 frontmatter 字段里的页面（答案、步骤、问答）：按字段值算字数，字段名、文件名、日期不算
        let steps: String = (0..40).map(|i| format!("+  - title: 第 {i} 步：打开币安充值页选择网络\n+    body: 网页版点顶部的充值，再点侧边栏的数字货币充值，币种选 USDT，网络选和对方转出平台都有的那一个。\n+    shot: bn-hans-{i}.webp\n")).collect();
        let page = format!("diff --git a/src/content/guides/a.md b/src/content/guides/a.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/guides/a.md\n@@ -0,0 +1 @@\n+---\n+title: 别人给我转 USDT\n+published: \"2026-11-01\"\n+steps:\n{steps}+---\n+短正文。\n");
        assert!(managed_verdict(&page, true, 1500, "all", &[]).is_empty(), "{:?}", managed_verdict(&page, true, 1500, "all", &[]));
        assert_eq!(frontmatter_prose("    shot: bn-hans-deposit-network.webp"), "");
        assert_eq!(frontmatter_prose("published: \"2026-11-01\""), "");
        assert_eq!(frontmatter_prose("  - q: 收款给 UID 和给地址哪个好？"), "收款给 UID 和给地址哪个好？");
    }

    #[test]
    fn new_files_are_checked_for_foreign_referrals() {
        let known: &'static [String] = Box::leak(vec!["/go/binance".to_string()].into_boxed_slice());
        let rules = |k: &'static [String]| Rules { tier: "standard", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: k, generated: &[], build_scripts: None, deps: &[] };
        let new = |body: &str| format!("diff --git a/src/content/new.md b/src/content/new.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/new.md\n@@ -0,0 +1 @@\n+{body}\n");
        let kinds = |v: Vec<Block>| v.into_iter().map(|b| b.kind).collect::<Vec<_>>();
        // 新文章里带了别人的推荐链接：拦；用站点本来就有的（known）：不拦
        assert_eq!(kinds(verdict(&new("去 [这里注册](https://www.okx.com/join?ref=STRANGER) 拿返佣"), &rules(&[]))), vec!["money"]);
        assert!(kinds(verdict(&new("去 [币安注册](/go/binance) 拿返佣"), &rules(known))).is_empty());
        assert!(added_money_atoms(&new("邀请码 ABCD1234 填这里")).iter().any(|a| a.contains("ABCD1234")), "新文件的推荐原子也要进 known 的计算");
    }

    #[test]
    fn scripts_in_pages_and_data_files_do_not_ship() {
        let d = |file: &str, plus: &[&str]| {
            let body: String = plus.iter().map(|l| format!("+{l}\n")).collect();
            format!("diff --git a/{file} b/{file}\n--- a/{file}\n+++ b/{file}\n@@ -1 +1 @@\n-x\n{body}")
        };
        let v = |diff: &str| managed_verdict(diff, true, 0, "all", &[]);
        // Markdown / HTML / SVG 里内嵌脚本、事件、javascript:：任何等级都拦
        assert!(v(&d("src/content/a.md", &["<script src=\"https://evil.example/x.js\"></script>"])).iter().any(|r| r.contains("插入脚本")), "{:?}", v(&d("src/content/a.md", &["<script>"])));
        assert!(v(&d("public/promo.html", &["<img src=x onerror=\"fetch('https://e')\">"])).iter().any(|r| r.contains("插入脚本")));
        assert!(v(&d("public/logo.svg", &["<a href=\"javascript:alert(1)\">"])).iter().any(|r| r.contains("插入脚本")));
        // iframe 按「直接插入 HTML」（html 类，方案可以放行）
        assert_eq!(verdict(&d("src/content/a.md", &["<iframe src=\"https://www.youtube.com/embed/x\"></iframe>"]), &Rules { tier: "open", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: None, deps: &[] }).len(), 0);
        // .mdx 的 import / export 是构建期代码
        assert!(v(&d("src/content/a.mdx", &["export const d = await (await import('node:' + 'fs')).promises.readFile('/etc/hosts')"])).iter().any(|r| r.contains("动态加载代码")));
        // 结构化数据的 <script type="application/ld+json"> 不算脚本
        assert!(!v(&d("src/layouts/Base.astro", &["<script type=\"application/ld+json\" set:html={JSON.stringify(ld)} />"])).iter().any(|r| r.contains("插入脚本")));
        assert!(v(&d("public/a.html", &["<script type=\"application/ld+json\" src=\"https://evil/x.js\"></script>"])).iter().any(|r| r.contains("插入脚本")));
        // 模板里的内联 <script> 按「直接插入 HTML」（放开档放行）；引外站脚本任何等级都拦；正文「JavaScript: 基础」不算
        let open = Rules { tier: "open", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: None, deps: &[] };
        assert!(verdict(&d("src/layouts/Base.astro", &["<script>document.body.dataset.ready = '1'</script>"]), &open).is_empty());
        assert!(v(&d("src/layouts/Base.astro", &["<script>x()</script>"])).iter().any(|r| r.contains("内联脚本")));
        assert!(v(&d("src/layouts/Base.astro", &["<script src=\"https://cdn.example/x.js\"></script>"])).iter().any(|r| r.contains("插入脚本")));
        assert!(!v(&d("src/content/a.md", &["## JavaScript: 基础语法"])).iter().any(|r| r.contains("脚本")));
        assert!(!v(&d("src/pages/a.astro", &["const online = true;", "{one === 1 && <b>x</b>}"])).iter().any(|r| r.contains("脚本")));
        // 正文里提到 onclick 这个词不算（不是标签属性）
        assert!(!v(&d("src/content/a.md", &["按钮的 onclick 事件是前端常见写法。"])).iter().any(|r| r.contains("插入脚本")));
        // 数据文件：`*` 开头的「乘法行」、注释后面接代码、import 后面接语句都不是纯数据
        assert!(!data_line("* globalThis['fet'+'ch']('https://evil.example')"));
        assert!(data_line("* 这一行是块注释里的说明"));
        assert!(!data_line("/* x */ eval('1')"));
        assert!(data_line("/* 只是注释 */"));
        assert!(!data_line("import { a } from './x'; globalThis['eval']('1')"));
        assert!(data_line("import { a, b } from './x';"));
        // 改 slug / draft：旧网址失效或页面消失
        let slug = format!("diff --git a/src/content/a.md b/src/content/a.md\n--- a/src/content/a.md\n+++ b/src/content/a.md\n@@ -1 +1 @@\n-slug: okx-fees\n+slug: okx-fee-guide\n");
        assert!(v(&slug).iter().any(|r| r.contains("改了 slug")));
        let fm = |minus: &str, plus: &str| format!("diff --git a/src/content/a.md b/src/content/a.md\n--- a/src/content/a.md\n+++ b/src/content/a.md\n@@ -1 +1 @@\n-{minus}\n+{plus}\n");
        assert!(v(&fm("draft: false", "draft: true")).iter().any(|r| r.contains("改了 draft")), "下架要拦");
        assert!(!v(&fm("published: false", "published: true")).iter().any(|r| r.contains("发布状态")), "把草稿发布出去是正常任务");
        assert!(!v(&fm("draft: true", "draft: false")).iter().any(|r| r.contains("发布状态")));
        // 正文引用列表里的 url、.astro 里的对象字面量不算；新加一行 draft: true（没有删掉的行）也算下架
        assert!(!v(&fm("  - url: https://www.okx.com/fees", "  - url: https://www.okx.com/zh-hans/fees")).iter().any(|r| r.contains("网址")));
        assert!(!v(&d("src/pages/nav.astro", &["  url: '/fees',"])).iter().any(|r| r.contains("网址")));
        assert!(v(&fm("title: a", "draft: true")).iter().any(|r| r.contains("改了 draft")));
        // JSDoc 里带括号、等号的注释行还是数据
        assert!(data_line("* @param x (optional) 默认 = 0"));
        assert!(!data_line("* (0, eval)('1')"));
        // 只换推荐码的值也算改动
        assert!(money_atoms("  inviteCode: \"ABC123\",").iter().any(|a| a == "inviteCode=ABC123"), "{:?}", money_atoms("  inviteCode: \"ABC123\","));
        // 官方帮助中心的「推荐计划规则」页是来源链接，不是推荐链接（htxneo 返佣页引用欧易规则页被误拦）
        for u in ["https://www.okx.com/help/okx-affiliate-program-rules", "https://www.okx.com/zh-hans/help/what-is-okx-referral-program", "https://www.binance.com/zh-CN/support/faq/detail/6233094e540b4ea4ac50387d1518911b", "https://www.gate.io/zh/announcements/article/referral-upgrade", "https://support.bybit.com/hc/en-us/articles/referral-program"] {
            assert!(money_atoms(&format!("规则依据：[说明]({u})（更新于 2026-08-06）")).is_empty(), "{u}");
        }
        // 真正的推荐链接照样算：路径带码、带 ?ref=（哪怕在帮助域名下）、/go/ 跳转
        for u in ["https://www.okx.com/join/ABC123?channelId=referral", "https://accounts.binance.com/register?ref=XYZ789", "https://www.okx.com/help/x?ref=ABC", "https://example.com/affiliate/landing", "/go/binance"] {
            assert!(!money_atoms(&format!("[注册]({u})")).is_empty(), "{u}");
        }
        // Pilot 自己的记录文件不算改动，也不会被当成太薄的新页面
        let pilot = "diff --git a/.pilot/todo.json b/.pilot/todo.json\nnew file mode 100644\n--- /dev/null\n+++ b/.pilot/todo.json\n@@ -0,0 +1 @@\n+[{\"title\": \"缺截图\"}]\n";
        assert!(managed_verdict(pilot, true, 1500, "all", &[]).is_empty());
        // .json 数据文件里的内部用语
        let j = format!("diff --git a/src/data/faq.json b/src/data/faq.json\n--- a/src/data/faq.json\n+++ b/src/data/faq.json\n@@ -1 +1 @@\n-x\n+  \"answer\": \"TODO 待补充具体步骤\"\n");
        assert!(v(&j).iter().any(|r| r.contains("内部说明")));
        // 构建链里的脚本不管在哪个目录都拦；env 在放开档也不放行
        let b: &'static [String] = Box::leak(vec!["build.mjs".to_string()].into_boxed_slice());
        let r = Rules { tier: "open", code: "all", allow: &[], new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: Some(b), deps: &[] };
        assert!(verdict(&d("tools/build.mjs", &["const x = 1"]), &r).iter().any(|b| b.text.contains("不自动改")));
        assert!(!r.lets("env"));
        // 等级收紧时清掉放行
        let mut g = ManagedGroup::from_legacy(&Default::default(), "t");
        g.strictness = "strict".into();
        g.allow_kinds = vec!["money".into()];
        g.tighten_from("open");
        assert!(g.allow_kinds.is_empty());
        let mut g2 = ManagedGroup::from_legacy(&Default::default(), "t");
        g2.strictness = "open".into();
        g2.allow_kinds = vec!["money".into()];
        g2.tighten_from("standard");
        assert_eq!(g2.allow_kinds, vec!["money"]);
    }

    #[test]
    fn strictness_tiers_and_allowed_kinds() {
        let rules = |tier: &'static str, allow: &'static [String]| Rules { tier, code: if tier == "strict" { "review" } else { "all" }, allow, new_content: true, min_page: 0, content_code: &[], known: &[], generated: &[], build_scripts: None, deps: &[] };
        let d = |file: &str, minus: &[&str], plus: &[&str]| {
            let body: String = minus.iter().map(|l| format!("-{l}\n")).chain(plus.iter().map(|l| format!("+{l}\n"))).collect();
            format!("diff --git a/{file} b/{file}\n--- a/{file}\n+++ b/{file}\n@@ -1 +1 @@\n{body}")
        };
        let kinds = |v: Vec<Block>| v.into_iter().map(|b| b.kind).collect::<Vec<_>>();
        // 同一个推荐链接只是换了旁边的说法：稳妥拦，标准 / 放开不拦
        let moved = d("src/content/a.md", &["去 [币安注册](/go/binance) 拿返佣"], &["点 [这里注册币安](/go/binance)，手续费有返佣"]);
        assert_eq!(kinds(verdict(&moved, &rules("strict", &[]))), vec!["money"], "每个文件只报一条");
        // HTML 里讲邀请码、返佣的正文和站内文章链接不算；新引用站点里已有的字段（refCode）不算新增，没有的才算
        let prose = d("src/pages/x.astro", &["<li><strong>想拿回手续费</strong>：两家都只能在注册时填写邀请码，返还比例见 <a href=\"/invite/\">邀请码页</a></li>"], &["<p>用本站币安邀请码 <code>{bn.refCode}</code> 注册，返还比例见 <a href=\"/invite/\">邀请码页</a>、<a href=\"/guides/invite-code-rebate/\">返佣说明</a></p>"]);
        assert_eq!(kinds(verdict(&prose, &rules("standard", &[]))), vec!["money"]);
        let known = vec!["bn.refCode".to_string()];
        assert!(verdict(&prose, &Rules { known: &known, ..rules("standard", &[]) }).is_empty());
        assert!(verdict(&moved, &rules("standard", &[])).is_empty());
        // 换了邀请码：标准、放开都拦
        let swapped = d("src/content/a.md", &["邀请码：GATE8888"], &["邀请码：GATE9999"]);
        assert!(kinds(verdict(&swapped, &rules("standard", &[]))).iter().all(|k| *k == "money"));
        assert!(!verdict(&swapped, &rules("open", &[])).is_empty());
        // 新加推荐链接：标准拦，放开不拦
        let added = d("src/content/a.md", &["旧的一段"], &["新的一段，[去欧易注册](/go/okx)"]);
        assert!(!verdict(&added, &rules("standard", &[])).is_empty());
        assert!(verdict(&added, &rules("open", &[])).is_empty());
        // 点过「这类以后不拦」的不拦
        let money: &'static [String] = Box::leak(Box::new(vec!["money".to_string()]));
        assert!(verdict(&swapped, &rules("strict", money)).iter().all(|b| b.kind != "money"));
        // 直接插入 HTML：标准拦（能放行的类型），放开不拦；调外部服务哪一档都拦、不能放行
        let html = d("src/components/A.astro", &["<p>{t}</p>"], &["<p set:html={t} />"]);
        assert_eq!(kinds(verdict(&html, &rules("standard", &[]))), vec!["html"]);
        assert!(verdict(&html, &rules("open", &[])).is_empty());
        let net = d("src/components/A.astro", &["<p>{t}</p>"], &["const r = await fetch('https://x.com');"]);
        assert_eq!(kinds(verdict(&net, &rules("open", &[]))), vec![""]);
        // 大段替换：标准 400 行，放开 1500 行
        let big: Vec<String> = (0..300).map(|i| format!("第 {i} 行")).collect();
        let refs: Vec<&str> = big.iter().map(|s| s.as_str()).collect();
        let rewrite = d("src/content/a.md", &refs, &refs);
        assert_eq!(kinds(verdict(&rewrite, &rules("standard", &[]))), vec!["size"]);
        assert!(verdict(&rewrite, &rules("open", &[])).is_empty());
        // 依赖、删文件哪一档都拦
        let pkg = d("package.json", &["a"], &["b"]);
        assert_eq!(kinds(verdict(&pkg, &rules("open", &[]))), vec![""]);
        // 老方案按代码改动设置换算等级；保存时代码改动跟着等级走
        let mut g: ManagedGroup = serde_json::from_str(r#"{"id":"g","name":"方案","code_changes":"review"}"#).unwrap();
        assert_eq!(g.tier(), "strict");
        g.strictness = "open".into();
        g.allow_kinds = vec!["jev".into(), "bogus".into(), "jev".into()];
        g.settle_tier();
        assert_eq!((g.code_changes.as_str(), g.allow_kinds.clone()), ("all", vec!["jev".to_string()]));
        let old: ManagedGroup = serde_json::from_str(r#"{"id":"g","name":"方案"}"#).unwrap();
        assert_eq!(old.tier(), "standard");
    }

    #[test]
    fn appended_content_in_existing_file_is_not_an_edit() {
        // 往已有文章列表里加一整篇（550 行、只删 1 行）：按新增内容算，不撞「改动已有文件」的上限
        let added: String = (0..550).map(|_| "+      '每 10 秒成本按套餐价折算成每 100 积分的价格，再乘以生成页上显示的积分消耗。',\n").collect();
        let append = format!("diff --git a/src/data/articles-comparisons.ts b/src/data/articles-comparisons.ts\n--- a/src/data/articles-comparisons.ts\n+++ b/src/data/articles-comparisons.ts\n@@ -10 +10,550 @@\n-      '旧的一行',\n{added}");
        assert!(managed_verdict(&append, true, 1500, "all", &[]).is_empty(), "{:?}", managed_verdict(&append, true, 1500, "all", &[]));
        // 大段替换已有内容（删 300、加 300）仍算改动，超过上限留给人看
        let removed: String = (0..300).map(|_| "-      '旧的说法',\n").collect();
        let rewrite = format!("diff --git a/src/content/a.md b/src/content/a.md\n--- a/src/content/a.md\n+++ b/src/content/a.md\n@@ -1,300 +1,300 @@\n{removed}{}", &added[..added.len() / 550 * 300]);
        assert!(managed_verdict(&rewrite, true, 1500, "all", &[]).iter().any(|r| r.contains("改动已有文件 600 行")));
    }

    #[test]
    fn data_files_with_only_text_count_as_content() {
        // 已有数据文件：加导入、加一段文字、加一个 slug、展开一个数组
        let edit = "diff --git a/src/data/articles.ts b/src/data/articles.ts\n--- a/src/data/articles.ts\n+++ b/src/data/articles.ts\n@@ -5 +5,2 @@\n+import { transitionArticles } from './articles-transitions';\n+    'ai-video-transition-prompts',\n+          '想把两个镜头接起来（遮挡、甩镜），可以看本页末尾的转场指南。',\n+  ...transitionArticles,\n";
        assert!(managed_verdict(edit, false, 1500, "review", &[]).is_empty(), "{:?}", managed_verdict(edit, false, 1500, "review", &[]));
        assert!(data_line("const viduSources = [") && data_line("    sources: viduSources,"));
        assert!(!data_line("const x = pick(1);") && !data_line("const t = `${a}`;"));
        assert!(data_line("  seoTitle?: string;") && data_line("  kind: 'a' | 'b';") && data_line("  links?: { label: string; href: string }[];"));
        assert!(!super::touches_money("<a class=\"text-link\" href={link.href}>") && super::touches_money("https://x.com/?ref=abc"));
        // 正文里讲邀请码、返佣的句子不算；具体的码、链接、字段、组件才算
        for prose in ["title: 欧易注册流程（2026）：OKX 怎么注册、邀请码与身份认证步骤", "2. **绑定邮箱和手机号**：输入邮箱并填写邀请码（选填，注册后不能补填），输入 6 位邮件验证码。", "手续费返佣是交易所把一部分手续费退给你。", "Enter the invite code if you have one (optional)."] {
            assert!(!super::touches_money(prose), "{prose}");
        }
        for money in ["填写邀请码：GATE8888 可以拿 20% 返佣", "推荐码 「ABCD12」", "[币安返佣链接](https://accounts.binance.com/register?ref=X)", "<CtaButton platform=\"okx\" />去 /go/okx", "  rebatePct: 20,", "  \"inviteCode\": \"XYZ\",", "<span data-ref-code=\"binance\"></span>", "<ReferralBox exchange=\"okx\" />"] {
            assert!(super::touches_money(money), "{money}");
        }
        // 正则字面量的值算数据；正则后面接代码不算
        assert!(data_line("    mentions: /(海螺|Hailuo)[^。]{0,60}(\\$\\s*9\\.99|每日少量|small daily)/i,"));
        assert!(data_line("    mentions: /Vidu[^。]{0,40}80\\s*(积分|credits)/i,"));
        assert!(data_line("  re: /a[/]b/g"));
        assert!(!data_line("  re: /a/.test(fetch('/x/')) ? 1 : /b/,"));
        assert!(!data_line("  re: /a/, run: go(),"));
        // 新的数据文件：按新内容规则和正文下限判断
        let body: String = (0..40).map(|_| "+      '遮挡转场要让前景物体把画面整个挡住，再在被挡住的那一帧切到下一段素材，两段的运动方向要一致。',\n").collect();
        let newfile = format!("diff --git a/src/data/articles-x.ts b/src/data/articles-x.ts\nnew file mode 100644\n--- /dev/null\n+++ b/src/data/articles-x.ts\n@@ -0,0 +1,50 @@\n+import type {{ ArticleSeed }} from './article-types';\n+export const xArticles: ArticleSeed[] = [\n+  {{\n+    slug: 'x',\n+    datePublished: '2026-10-22',\n+    body: [\n{body}+    ],\n+  }},\n+];\n");
        assert!(managed_verdict(&newfile, true, 1500, "review", &[]).is_empty(), "{:?}", managed_verdict(&newfile, true, 1500, "review", &[]));
        assert!(managed_verdict(&newfile, false, 1500, "review", &[]).iter().any(|r| r.contains("新内容")));
        // 数据文件里混了代码：函数、模板字符串插值、外部导入
        for bad in ["+export function pick(a) { return a; }", "+  title: `${name} 指南`,", "+import fs from 'node:fs';", "+  run: () => fetch('/x'),", "+const x = process.env.TOKEN;"] {
            let d = format!("diff --git a/src/data/a.ts b/src/data/a.ts\n--- a/src/data/a.ts\n+++ b/src/data/a.ts\n@@ -1 +1 @@\n{bad}\n");
            assert!(managed_verdict(&d, true, 0, "review", &[]).iter().any(|r| r.contains("改了代码")), "{bad}");
        }
        // 不在 data / content 目录下的 .ts 仍然是代码
        let code = "diff --git a/src/lib/seo.ts b/src/lib/seo.ts\n--- a/src/lib/seo.ts\n+++ b/src/lib/seo.ts\n@@ -1 +1 @@\n+  title: 'x',\n";
        assert!(managed_verdict(code, true, 0, "review", &[]).iter().any(|r| r.contains("改了代码")));
    }

    #[test]
    fn code_changes_follow_the_plan_setting() {
        let lines = |n: usize| (0..n).map(|i| format!("+  const a{i} = {i};\n")).collect::<String>();
        let lib = |n: usize| format!("diff --git a/src/lib/types.ts b/src/lib/types.ts\n--- a/src/lib/types.ts\n+++ b/src/lib/types.ts\n@@ -1 +1 @@\n{}", lines(n));
        assert!(managed_verdict(&lib(10), true, 0, "review", &[]).iter().any(|r| r.contains("留给你确认")));
        assert!(managed_verdict(&lib(10), true, 0, "small", &[]).is_empty(), "{:?}", managed_verdict(&lib(10), true, 0, "small", &[]));
        assert!(managed_verdict(&lib(100), true, 0, "small", &[]).iter().any(|r| r.contains("小改动")));
        assert!(managed_verdict(&lib(100), true, 0, "all", &[]).is_empty());
        // 任何一档都拦：依赖 / 配置、服务端、读环境变量、外部请求、新的外部包
        for (path, line) in [("package.json", "+  \"x\": \"1\""), ("astro.config.mjs", "+  site: 'x',"), ("src/pages/api/hook.ts", "+export const a = 1;"), ("src/lib/a.ts", "+const k = process.env.KEY;"), ("src/components/A.astro", "+const r = await fetch('https://x.com');"), ("src/lib/a.ts", "+import lodash from 'lodash';")] {
            let d = format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n{line}\n");
            assert!(!managed_verdict(&d, true, 0, "all", &[]).is_empty(), "{path} {line}");
        }
        // 本地文件的导入、样式文件，小改动可以自动上线
        let ok = "diff --git a/src/components/A.astro b/src/components/A.astro\n--- a/src/components/A.astro\n+++ b/src/components/A.astro\n@@ -1 +1 @@\n+import Links from './Links.astro';\n+<Links items={block.links} />\ndiff --git a/src/styles/a.css b/src/styles/a.css\n--- a/src/styles/a.css\n+++ b/src/styles/a.css\n@@ -1 +1 @@\n+.links { margin: 0; }\n";
        assert!(managed_verdict(ok, true, 0, "small", &[]).is_empty(), "{:?}", managed_verdict(ok, true, 0, "small", &[]));
    }

    #[test]
    fn site_listed_code_files_follow_the_plan_setting() {
        let go = "diff --git a/internal/site/build.go b/internal/site/build.go\n--- a/internal/site/build.go\n+++ b/internal/site/build.go\n@@ -1 +1 @@\n-\tTitle: \"MEXC fees\",\n+\tTitle: \"MEXC fees 2026: maker, taker and withdrawal\",\n";
        let list = vec!["internal/site/build.go".to_string()];
        assert!(managed_verdict(go, true, 0, "small", &[]).iter().any(|r| r.contains("服务端代码不自动改")));
        assert!(managed_verdict(go, true, 0, "small", &list).is_empty(), "{:?}", managed_verdict(go, true, 0, "small", &list));
        assert!(managed_verdict(go, true, 0, "review", &list).iter().any(|r| r.contains("改了代码")));
        // 目录也行
        assert!(managed_verdict(go, true, 0, "all", &["internal/site/".to_string()]).is_empty());
        // 放行的文件里照样查危险写法
        let env = go.replace("+\tTitle", "+\tk := os.Getenv(\"KEY\")\n+\tTitle");
        assert!(managed_verdict(&env, true, 0, "all", &list).iter().any(|r| r.contains("读环境变量")));
        // 依赖、配置、脚本永远不放行
        for p in ["go.mod", "package.json", ".github/workflows/x.yml", "scripts/deploy.sh", "wrangler.toml", "migrations/1.sql"] {
            assert!(never_allow(p), "{p}");
            let d = format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n+x\n");
            assert!(!managed_verdict(&d, true, 0, "all", &[p.to_string()]).is_empty(), "{p}");
        }
        assert!(!never_allow("internal/site/build.go") && !never_allow("cmd/site/main.go") && !never_allow("server/pages.py"));
    }

    #[test]
    fn backend_templates_and_tests_are_not_server_code() {
        let file = |p: &str, line: &str| format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n-old\n+{line}\n");
        // Go 站的页面模板按页面内容判断，不用放行
        let tmpl = file("internal/web/templates/hant/learn_hk_vatp.html", "<h1>香港證監會持牌虛擬資產交易平台名單</h1>");
        assert!(managed_verdict(&tmpl, true, 0, "review", &[]).is_empty(), "{:?}", managed_verdict(&tmpl, true, 0, "review", &[]));
        assert!(managed_verdict(&file("internal/web/templates/a.gohtml", "<p>x</p>"), true, 0, "review", &[]).is_empty());
        // 测试文件按方案的代码改动设置判断，危险写法照样拦
        let test = file("internal/web/hant_test.go", "\twant := \"香港證監會持牌虛擬資產交易平台名單\"");
        assert!(managed_verdict(&test, true, 0, "all", &[]).is_empty(), "{:?}", managed_verdict(&test, true, 0, "all", &[]));
        assert!(managed_verdict(&test, true, 0, "review", &[]).iter().any(|r| r.contains("改了代码")));
        assert!(managed_verdict(&file("internal/web/hant_test.go", "\tout, _ := exec.Command(\"sh\").Output()"), true, 0, "all", &[]).iter().any(|r| r.contains("执行命令")));
        assert!(managed_verdict(&file("server/tests/test_pages.py", "    assert title == 'x'"), true, 0, "all", &[]).is_empty());
        // 后端目录里的正文
        assert!(managed_verdict(&file("internal/web/guides/launchpool.en.md", "# Binance Launchpool"), true, 0, "review", &[]).is_empty());
        // 普通 Go 代码：「代码改动：全部」时按代码判断（危险写法照样拦），其他设置要放进站点清单
        let go = file("internal/web/hant_only.go", "\t\"title\": \"香港持牌交易所名單\",");
        assert!(managed_verdict(&go, true, 0, "all", &[]).is_empty(), "{:?}", managed_verdict(&go, true, 0, "all", &[]));
        assert!(managed_verdict(&go, true, 0, "small", &[]).iter().any(|r| r.contains("服务端代码不自动改")));
        assert!(managed_verdict(&file("internal/site/build.go", "\tk := os.Getenv(\"KEY\")"), true, 0, "all", &[]).iter().any(|r| r.contains("读环境变量")));
        // 脚本 / 部署 / CI 目录、Worker / Functions / API 路由，「全部」也拦
        for p in ["scripts/deploy_test.go", "deploy/index.html", "functions/api/page.html", ".github/x.html", "functions/x.ts", "src/pages/api/rates.ts", "go.mod"] {
            assert!(managed_verdict(&file(p, "x"), true, 0, "all", &[]).iter().any(|r| r.contains("服务端代码不自动改")), "{p}");
        }
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
        let (_, t, dropped) = parse_plan_counted("```json\n{\"tasks\":[{\"title\":\"a\"},{\"title\":\"\"},{\"title\":3},\"x\"]}\n```").unwrap();
        assert_eq!((t.len(), dropped), (1, 3));
        assert!(parse_plan("没有计划").is_err());
    }
}


