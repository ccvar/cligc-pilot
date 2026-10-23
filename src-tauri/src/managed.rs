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
    /// 新写的整篇内容（新页面）：auto 自动发布 | review 运营人员点头后再发布。
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
    #[serde(default)]
    pub created_at: u64,
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
pub fn managed_verdict(diff: &str, allow_new_content: bool) -> Vec<String> {
    let mut reasons = Vec::new();
    let mut file = String::new();
    let mut new_file = false;
    let mut changed = 0usize;
    let content_ext = ["md", "mdx", "markdown", "html", "htm", "astro", "njk", "liquid", "hbs", "vue", "svelte", "json", "svg", "png", "jpg", "jpeg", "webp", "avif"];
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            file = rest.split(" b/").nth(1).unwrap_or(rest).to_string();
            new_file = false;
            if forbidden_path(&file) {
                reasons.push(format!("{file}：依赖、配置或代码文件不自动改"));
            } else if !content_ext.iter().any(|e| file.ends_with(&format!(".{e}"))) {
                reasons.push(format!("{file}：不是页面内容文件"));
            }
            continue;
        }
        if line.starts_with("new file mode") {
            new_file = true;
            if !allow_new_content {
                reasons.push(format!("{file}：新内容需要运营人员确认后发布"));
            }
            continue;
        }
        if line.starts_with("deleted file mode") {
            reasons.push(format!("{file}：删除文件不自动执行"));
            continue;
        }
        if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") || line.starts_with("index ") {
            continue;
        }
        if let Some(removed) = line.strip_prefix('-') {
            changed += 1;
            if touches_money(removed) {
                reasons.push(format!("{file}：改动了推荐 / 邀请相关的内容"));
            }
        } else if let Some(added) = line.strip_prefix('+') {
            changed += 1;
            if !new_file && touches_money(added) {
                reasons.push(format!("{file}：新增了推荐 / 邀请相关的内容"));
            }
        }
    }
    if changed > 400 {
        reasons.push(format!("改动 {changed} 行，超过自动上线的上限（400 行）"));
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
    fn plan_replies_carry_positioning_and_tasks() {
        let r = "好的\n```json\n{\"positioning\":\"面向中文用户的交易所教程站\",\"tasks\":[{\"title\":\"写 KYC 教程\",\"change\":\"new_content\"}]}\n```";
        let (p, t) = parse_plan_full(r).unwrap();
        assert_eq!(p, "面向中文用户的交易所教程站");
        assert_eq!(t[0].change, "new_content");
        let (p, t) = parse_plan_full("[{\"title\":\"x\",\"change\":\"meta\"}]").unwrap();
        assert!(p.is_empty() && t.len() == 1);
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
        assert!(managed_verdict(ok, false).is_empty());
        let code = "diff --git a/worker/index.ts b/worker/index.ts\n--- a/worker/index.ts\n+++ b/worker/index.ts\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(!managed_verdict(code, true).is_empty());
        let cfg = "diff --git a/package.json b/package.json\n--- a/package.json\n+++ b/package.json\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(!managed_verdict(cfg, true).is_empty());
        let money = "diff --git a/src/pages/a.astro b/src/pages/a.astro\n--- a/src/pages/a.astro\n+++ b/src/pages/a.astro\n@@ -9 +9 @@\n-<a href=\"/go/gate\">邀请码 ABC</a>\n+<a href=\"/go/gate\">邀请码 XYZ</a>\n";
        assert!(managed_verdict(money, true).iter().any(|r| r.contains("推荐")));
        let newpage = "diff --git a/src/content/new.md b/src/content/new.md\nnew file mode 100644\n--- /dev/null\n+++ b/src/content/new.md\n@@ -0,0 +1 @@\n+# 新页面\n";
        assert!(managed_verdict(newpage, false).iter().any(|r| r.contains("新内容")));
        assert!(managed_verdict(newpage, true).is_empty());
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
