//! 托管运营（第 5 档「全托管」）：全局设置、动态记录、每日摘要。
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

pub fn validate(c: &ManagedConfig) -> Result<(), String> {
    for b in [&c.brain, &c.fallback_brain] {
        if b == "codex" {
            return Err("托管不能用 Codex（它没有逐条确认的钩子），请选 Claude、Grok 或 API 服务".into());
        }
    }
    if c.brain.is_empty() {
        return Err("请选择主模型".into());
    }
    if !c.fallback_brain.is_empty() && c.fallback_brain == c.brain && c.fallback_model == c.model {
        return Err("备用模型和主模型一样，起不到备用的作用".into());
    }
    if !matches!(c.new_content.as_str(), "auto" | "review") {
        return Err("新内容发布方式只能是自动发布或审核后发布".into());
    }
    if c.daily_changes > 50 || c.digest_hour > 23 {
        return Err("设置超出范围".into());
    }
    Ok(())
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
}

/// 从模型回复里取出计划 JSON（```json … ``` 代码块，或回复里第一个 JSON 数组）。
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
    }
    tasks.truncate(12);
    Ok(tasks)
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
        assert!(validate(&c).is_ok());
        assert!(validate(&ManagedConfig { fallback_brain: "codex".into(), ..c.clone() }).is_err());
        assert!(validate(&ManagedConfig { fallback_brain: "claude".into(), ..c }).is_err(), "备用和主模型一样");
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
