//! 运营任务运行器：机会扫描 → 按档位出方案 / 提 PR / 低风险自动上线；站点体检；周报；效果复盘；定时调度。
//!
//! 安全边界（见设计文档「安全与凭据」）：
//! - 模型只在 Pilot 准备的工作树里改代码、在本地提交；推送、开 PR、合并、部署、IndexNow 全由 Pilot 执行。
//! - 无人值守的回合里，所有需要确认的操作（推送、部署、读 Token 环境变量等）一律由 Pilot 代为拒绝，
//!   确认卡也不会出现在界面上（见 `is_unattended` 与 lib.rs::list_pending_permits）；
//!   只读联网（搜网页、打开公网网页，见 permit::unattended_web_ok）放行，好让 AI 对照官网核实事实。
//! - 不允许 Codex（没有逐条确认钩子）。失败不重放：中途失败就停下，等你处理。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{Emitter, Manager};

use crate::site_jobs::Job;
use crate::site_ops::{self, Opportunity, QueueItem};
use crate::sites::Site;

const DAY: u64 = 86_400;
/// 同一个机会处理过后，多久之内不再重新入队。
pub(crate) const COOLDOWN: u64 = 21 * DAY;

fn now() -> u64 {
    crate::now_secs()
}

// ---------------------------------------------------------------------------
// 无人值守会话登记：这些会话的权限请求一律拒绝，且不在界面上出卡。
// ---------------------------------------------------------------------------

fn unattended() -> &'static Mutex<HashSet<String>> {
    static S: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

pub fn is_unattended(conv_id: &str) -> bool {
    unattended().lock().unwrap().contains(conv_id)
}

struct UnattendedGuard {
    conv: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl UnattendedGuard {
    fn start(conv: &str, pending_dir: PathBuf) -> Self {
        unattended().lock().unwrap().insert(conv.to_string());
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (s, c) = (stop.clone(), conv.to_string());
        tauri::async_runtime::spawn(async move {
            while !s.load(std::sync::atomic::Ordering::SeqCst) {
                for p in crate::permit::list_pending(&pending_dir) {
                    if p.conv == c {
                        // 只读联网（搜网页、打开公网网页）放行，好让 AI 对照官网核实；其余一律拒绝
                        let _ = crate::permit::respond(&pending_dir, &p.id, crate::permit::unattended_web_ok(&p));
                    }
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        });
        Self { conv: conv.into(), stop }
    }
}

impl Drop for UnattendedGuard {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        unattended().lock().unwrap().remove(&self.conv);
    }
}

// ---------------------------------------------------------------------------
// 运行记录与调度状态
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct RunRecord {
    pub id: String,
    pub task: String,
    pub trigger: String,
    pub started: u64,
    pub finished: u64,
    pub ok: bool,
    pub summary: String,
    pub error: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct OpsState {
    /// 各任务上次运行时间（调度用）。
    #[serde(default)]
    pub last: HashMap<String, u64>,
    #[serde(default)]
    pub runs: Vec<RunRecord>,
    /// 效果复盘里每类改动连续负面的次数（≥2 即自动降档）。
    #[serde(default)]
    pub negatives: HashMap<String, u32>,
    /// 运营任务里发现站点自己构建不过（改动前的代码在同样环境里也不过）：（时间，原因）。之后有一次构建通过就清掉
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_broken: Option<(u64, String)>,
}

fn set_env_broken(site_dir: &Path, why: Option<String>) {
    let mut s = load_state(site_dir);
    if s.env_broken.is_none() && why.is_none() {
        return;
    }
    s.env_broken = why.map(|w| (now(), w));
    save_state(site_dir, &s);
}

fn state_path(site_dir: &Path) -> PathBuf {
    site_dir.join("ops_state.json")
}

pub fn load_state(site_dir: &Path) -> OpsState {
    std::fs::read(state_path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// 站点自己的构建 / 检查没通过（最近一次每日体检，不含 AI 的改动）：返回原因。站点设置在那之后改过的不算（等下次体检再判断）。
/// 这时 AI 改什么都会卡在同一步：白花额度、还记成 AI 的失败，托管先不派任务。
/// 证据有两种，取最新的一次：每日体检的构建结果，和运营任务里「改动前的代码也构建不过」的记录。
pub fn site_build_broken(site: &Site, site_dir: &Path) -> Option<String> {
    let st = load_state(site_dir);
    let health = st.runs.iter().find(|r| r.task == "health" && r.finished > 0);
    let (at, err) = match (&st.env_broken, health) {
        (Some((t, e)), h) if h.is_none_or(|h| *t >= h.finished) => (*t, e.clone()),
        (_, Some(h)) if !h.ok && h.error.starts_with("构建或检查没通过") => (h.finished, h.error.trim_start_matches("构建或检查没通过：").to_string()),
        _ => return None,
    };
    if at <= site.updated_at {
        return None;
    }
    // 卡在一条现在会被跳过的 make 检查上（连服务器的，见 site_deploy::build_and_check）：那次失败不算数
    if let Some(t) = err.split("检查没通过（make ").nth(1).and_then(|x| x.split('）').next()) {
        let dir = crate::site_git::code_dir(site, None).map(|d| site.app_dir(&d));
        if dir.is_ok_and(|d| crate::sites::make_target_hits_server(&d, t).is_some()) {
            return None;
        }
    }
    Some(err.lines().next().unwrap_or("").to_string())
}

/// 用改动前的代码（base）在同样的环境里构建、检查一遍：返回失败原因（也不过＝站点自己的问题），能过返回 None。
async fn base_failure(site: &Site, site_dir: &Path, job: &Job, cb: &CodeBase, base_sha: &str, item_id: &str) -> Option<String> {
    let dir = crate::sites::build_root(site_dir).join("work").join(format!("{item_id}-base"));
    crate::site_git::remove_worktree(&cb.main, &dir).await;
    if crate::site_git::worktree_at(&cb.main, &dir, base_sha, None).await.is_err() {
        return None;
    }
    crate::site_git::share_node_modules(&cb.main, &dir);
    job.line("构建或检查没过：先用改动前的代码在同样的环境里再跑一遍，看是这次改动的问题还是站点本身的问题");
    let r = crate::site_deploy::build_and_check(site, &dir, None, job).await;
    crate::site_git::remove_worktree(&cb.main, &dir).await;
    r.err().map(|e| e.lines().next().unwrap_or("").to_string())
}

fn save_state(site_dir: &Path, s: &OpsState) {
    if let Ok(raw) = serde_json::to_vec_pretty(s) {
        let tmp = state_path(site_dir).with_extension("json.tmp");
        if std::fs::write(&tmp, raw).is_ok() {
            let _ = std::fs::rename(&tmp, state_path(site_dir));
        }
    }
}

fn record_run(site_dir: &Path, rec: RunRecord) {
    let mut s = load_state(site_dir);
    s.last.insert(rec.task.clone(), rec.started);
    s.runs.insert(0, rec);
    // 托管页的执行记录图要看 30 天：一个站点一天可能跑好几样（扫描、体检、计划…），多留一些
    s.runs.truncate(200);
    save_state(site_dir, &s);
}

/// 这个站点最近的运营作业记录（新的在前）。
pub fn runs_of(site_dir: &Path) -> Vec<RunRecord> {
    load_state(site_dir).runs
}

pub fn period(schedule: &str) -> Option<u64> {
    match schedule {
        "daily" => Some(DAY),
        "weekly" => Some(7 * DAY),
        _ => None,
    }
}

/// 某个任务下一次到点的时刻：daily / weekly 是上次 + 周期；「days:N」是每 N 天；
/// 「weekly:D」「weekly:D:N」是每（N）周的第 D 天（1＝周一）——上次之后至少隔 7N−1 天、落在那一天（从那天 0 点起算）。
/// off 等没有周期的为 None。
pub fn next_due(schedule: &str, last: u64) -> Option<u64> {
    if let Some(n) = schedule.strip_prefix("days:").and_then(|d| d.parse::<u64>().ok()) {
        return Some(last + n.max(1) * DAY);
    }
    if let Some(rest) = schedule.strip_prefix("weekly:") {
        use chrono::{Datelike, TimeZone};
        let mut parts = rest.split(':');
        let day = parts.next()?.parse::<u32>().ok()?;
        let weeks = parts.next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(1).max(1);
        let t0 = last + (7 * weeks - 1) * DAY;
        let mut d = chrono::Local.timestamp_opt(t0 as i64, 0).single()?.date_naive();
        for _ in 0..7 {
            if d.weekday().number_from_monday() == day {
                break;
            }
            d = d.succ_opt()?;
        }
        let midnight = chrono::Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest()?.timestamp() as u64;
        return Some(midnight.max(t0));
    }
    period(schedule).map(|p| last + p)
}

/// 某个任务现在该不该跑（错过的任务在下次启动时补跑一次）。
pub fn due(schedule: &str, last: u64, now: u64) -> bool {
    next_due(schedule, last).is_some_and(|t| now >= t)
}

/// 同一轮都到期时按这个顺序起（每站每轮只起一个）：先体检、再出计划，扫描执行时用得上当天的体检报告和新计划
/// （以前扫描排第一，出计划那天扫描先把上周剩下的做了，新计划要等到第二天）。
pub const TASKS: &[&str] = &["health", "plan", "scan", "report", "review"];

/// 任务周期。在托管方案里的站点：扫描（一轮：挑几条交给 AI 改、上线）按方案的运营节奏（每天，或每周某一天），
/// 体检每天，计划、周报、复盘每周；没托管的站点（你自己运营）：不做计划，其它按站点自己的周期（默认每周）。
pub fn schedule_of(site: &Site, task: &str, plan: Option<&crate::managed::ManagedGroup>) -> String {
    if let Some(g) = plan {
        return match task {
            "scan" if g.cadence == "weekly" => format!("weekly:{}:{}", g.cadence_day, g.cadence_every.max(1)),
            "scan" if g.cadence_every > 1 => format!("days:{}", g.cadence_every),
            "scan" | "health" => "daily".into(),
            _ => "weekly".into(),
        };
    }
    match task {
        "scan" => site.ops.scan.clone(),
        "health" => site.ops.health.clone(),
        "report" => site.ops.report.clone(),
        "review" => site.ops.review.clone(),
        _ => "off".into(),
    }
}

/// 这个站点下一次自动运行什么、什么时候（和调度循环同一套规则：每项任务上次运行 + 周期，没跑过的从导入时间算；
/// 没有数据源的只做体检和计划；上线后检查到期的优先）。托管暂停或站点停了就没有下一次。
/// 时间是「到点」的时刻：调度每 10 分钟看一次、客户端要开着，实际会晚几分钟。
pub fn next_run(site: &Site, site_dir: &Path, plan: Option<&crate::managed::ManagedGroup>) -> Option<(String, u64)> {
    let state = load_state(site_dir);
    let has_data = !site.data.gsc_site.is_empty() || !site.data.ga4_property.is_empty() || !site.data.bing_site.is_empty();
    let mut best: Option<(String, u64)> = site_ops::load_queue(site_dir)
        .iter()
        .filter(|i| i.status == "deployed" && i.watch_due > 0 && (i.watched.is_empty() || i.watch_stage == 1))
        .map(|i| ("watch".to_string(), i.watch_due))
        .min_by_key(|x| x.1);
    for task in TASKS {
        if !matches!(*task, "health" | "plan") && !has_data {
            continue;
        }
        let Some(at) = next_due(&schedule_of(site, task, plan), state.last.get(*task).copied().unwrap_or(0).max(site.created_at)) else { continue };
        if best.as_ref().is_none_or(|b| at < b.1) {
            best = Some((task.to_string(), at));
        }
    }
    best
}

pub fn task_label(task: &str) -> &'static str {
    match task {
        "scan" => "机会扫描",
        "health" => "站点体检",
        "report" => "周报",
        "review" => "效果复盘",
        "accept" => "确认改动",
        "accept_many" => "一起合并上线",
        "resolve" => "处理合不进去的改动",
        "plan" => "AI 制定计划",
        "cycle" => "立即执行一轮",
        "watch" => "上线后检查",
        "rollback" => "回滚",
        "recheck" => "重新检查",
        "batch" => "批量让 AI 改",
        _ => "运营任务",
    }
}

// ---------------------------------------------------------------------------
// 上下文
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Ctx {
    pub app: tauri::AppHandle,
}

impl Ctx {
    fn state(&self) -> tauri::State<'_, crate::AppState> {
        self.app.state::<crate::AppState>()
    }
    fn site_dir(&self, id: &str) -> PathBuf {
        self.state().sites.data_dir(id)
    }
    fn notify(&self, title: &str, body: &str) {
        crate::push::notify(&self.app, title, body);
    }
    pub(crate) fn emit_queue(&self, site_id: &str) {
        let q = site_ops::load_queue(&self.site_dir(site_id));
        let _ = self.app.emit("site-queue", serde_json::json!({ "site_id": site_id, "queue": q }));
    }
}

// ---------------------------------------------------------------------------
// 预算
// ---------------------------------------------------------------------------

#[derive(Serialize, Default, Clone, Debug)]
pub struct Budget {
    pub changes_used: u32,
    pub changes_limit: u32,
    pub tokens_used: u64,
    pub tokens_limit: u64,
}

pub fn budget(site: &Site, site_dir: &Path, convos: &crate::convo::ConvStore) -> Budget {
    let since = now().saturating_sub(7 * DAY);
    let q = site_ops::load_queue(site_dir);
    // 只算改出东西的（处理中的也算，免得同时跑的几条一起冲过上限）；失败的、你停掉的不占额度
    let changes = q.iter().filter(|i| i.worked_at >= since && matches!(i.status.as_str(), "working" | "proposed" | "merged" | "deployed" | "done" | "rolled_back")).count() as u32;
    let conn_id = crate::site_cmds::site_conn_id(&site.id);
    let tokens: u64 = convos
        .list()
        .into_iter()
        .filter(|c| c.conn_id == conn_id && c.origin == "ops" && c.updated_at >= since)
        .map(|c| if c.billed_tokens > 0 { c.billed_tokens } else { c.total_tokens })
        .sum();
    // 托管站点用所在方案的上限；站点数据目录是 <数据目录>/sites/<id>
    let group = site_dir.parent().and_then(Path::parent).and_then(|root| crate::managed::load(root).group_of(&site.id).cloned());
    let tokens_limit = group.as_ref().map(|g| g.tokens_per_week).unwrap_or(site.ops.budget.tokens_per_week);
    // 托管站点不再按条数封顶（节奏 × 每轮条数已经管住了 AI 做多少），只看 token；自己运营的站点沿用站点设置
    let changes_limit = if group.is_some() { 0 } else { site.ops.budget.changes_per_week };
    Budget { changes_used: changes, changes_limit, tokens_used: tokens, tokens_limit }
}

/// 托管自己跑时的额度：每周改动条数（0＝不限）和模型用量都管。
fn over_budget(b: &Budget) -> Option<String> {
    if b.changes_limit > 0 && b.changes_used >= b.changes_limit {
        return Some(format!("本周改动已达上限（{} 条）", b.changes_limit));
    }
    over_tokens(b)
}

/// 你手动交给 AI 时只看模型用量：改动条数的上限只约束托管自己跑。
fn over_tokens(b: &Budget) -> Option<String> {
    if b.tokens_limit > 0 && b.tokens_used >= b.tokens_limit {
        return Some(format!("本周模型用量已达上限（{} token）", b.tokens_limit));
    }
    None
}

// ---------------------------------------------------------------------------
// 启动任务
// ---------------------------------------------------------------------------

fn steps_for(task: &str) -> &'static [(&'static str, &'static str)] {
    match task {
        "scan" => &[("data", "拉取数据"), ("scan", "扫描机会"), ("work", "处理机会"), ("quality", "内容质检"), ("ship", "提交 / 上线")],
        "health" => &[("data", "拉取数据"), ("prepare", "准备代码"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("report", "体检报告")],
        "report" => &[("data", "拉取数据"), ("report", "生成周报")],
        "review" => &[("data", "拉取数据"), ("review", "对比上线前后")],
        "accept" | "accept_many" => &[("merge", "合并"), ("prepare", "准备代码"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("upload", "上传"), ("verify", "线上验收"), ("indexnow", "提交 IndexNow")],
        "rollback" => &[("revert", "回退提交"), ("prepare", "准备代码"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("upload", "上传"), ("verify", "线上验收"), ("indexnow", "提交 IndexNow")],
        "work" => &[("work", "AI 改动"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("quality", "内容质检"), ("ship", "上线 / 等你确认"), ("verify", "线上验收")],
        "batch" => &[("work", "AI 改动"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("quality", "内容质检"), ("ship", "上线 / 等你确认"), ("verify", "线上验收")],
        "resolve" => &[("rebase", "接到最新代码"), ("work", "AI 重做"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("quality", "内容质检"), ("ship", "上线 / 等你确认"), ("verify", "线上验收")],
        "recheck" => &[("work", "AI 的改动"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("quality", "内容质检"), ("ship", "上线 / 等你确认"), ("verify", "线上验收")],
        "plan" => &[("data", "拉取数据"), ("plan", "AI 制定计划")],
        "cycle" => &[("data", "拉取数据"), ("plan", "AI 制定计划"), ("work", "AI 改动并检查"), ("quality", "内容质检"), ("ship", "上线 / 等你确认"), ("verify", "线上验收")],
        "watch" => &[("watch", "上线后检查")],
        _ => &[],
    }
}

/// 一个运营作业最多跑多久：到点就停掉里面的命令和 AI 回合，按失败收尾，不让站点被一个卡死的作业占着。
const MAX_JOB_DURATION: Duration = Duration::from_secs(3 * 60 * 60);
/// 调度同时最多起这么多个运营作业（手动点的不算在内、也不受它限制）
pub(crate) const MAX_PARALLEL_JOBS: usize = 3;

/// 开一个运营作业（后台跑，立即返回作业 id）。
pub fn start(ctx: Ctx, site_id: &str, task: &str, trigger: &str, item_id: Option<String>) -> Result<String, String> {
    start_with_model(ctx, site_id, task, trigger, item_id, None)
}

/// 「执行但不上线」的站点：这一轮照方案规则判定、照常做内容质检（只记分数，不交回重写），但最后不上线，都留给你确认。
static HOLD: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// 作业超时被 Pilot 强停的站点：这期间 AI 回合报的「被停止」不是人停的（失败原因写成可自动重试的）
static KILLED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
fn mark_killed(site_id: &str) {
    let mut k = KILLED.lock().unwrap();
    if !k.iter().any(|s| s == site_id) {
        k.push(site_id.to_string());
    }
}
fn was_killed(site_id: &str) -> bool {
    KILLED.lock().unwrap().iter().any(|s| s == site_id)
}
fn clear_killed(site_id: &str) {
    KILLED.lock().unwrap().retain(|s| s != site_id);
}

fn held(site_id: &str) -> bool {
    HOLD.lock().unwrap().iter().any(|s| s == site_id)
}

/// 你在 Pilot 里手动点开的作业（trigger = "manual"）：改好等你确认时不再发 Telegram，你人就在电脑前。
static MANUAL: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn by_hand(site_id: &str) -> bool {
    MANUAL.lock().unwrap().iter().any(|s| s == site_id)
}

/// 临时换模型的作业：（brain, model），只在这个作业里用、不改站点和方案设置，作业结束就恢复。
static MODEL_OVERRIDE: std::sync::Mutex<Vec<(String, (String, String))>> = std::sync::Mutex::new(Vec::new());

fn model_override(site_id: &str) -> Option<(String, String)> {
    MODEL_OVERRIDE.lock().unwrap().iter().find(|(s, _)| s == site_id).map(|(_, m)| m.clone())
}

fn set_model_override(site_id: &str, m: Option<(String, String)>) {
    let mut v = MODEL_OVERRIDE.lock().unwrap();
    v.retain(|(s, _)| s != site_id);
    if let Some(m) = m {
        v.push((site_id.to_string(), m));
    }
}

/// 临时换模型时一起选的推理强度（空＝跟随模型默认；API 模型带 api: 前缀，和对话里一样）。只对临时选的那个模型生效。
static EFFORT_OVERRIDE: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// 主模型因为短时间好不了的原因失败（余额不足、密钥不对、客户端没登录、模型用不了）：记一段时间，
/// 这期间同一个主模型直接用备用模型，不再每条任务都先失败一次、留一个空会话。充值 / 修好后最多这么久就恢复用它。
static MODEL_DOWN: std::sync::Mutex<Vec<(String, u64, String)>> = std::sync::Mutex::new(Vec::new());
const MODEL_DOWN_SECS: u64 = 30 * 60;

fn model_key(m: &(String, String)) -> String {
    format!("{}::{}", m.0, m.1)
}

/// 报错是不是短时间好不了的那种；是就返回一句说明（和前端 opsErrorHint 认的是同一类）。限流、服务商临时故障不算。
pub fn persistent_failure(e: &str) -> Option<&'static str> {
    let l = e.to_lowercase();
    if ["credit balance is too low", "insufficient_quota", "insufficient quota", "insufficient balance", "insufficient funds", "余额不足", "账户余额", "payment required", "http 402"].iter().any(|k| l.contains(k)) {
        return Some("余额不足");
    }
    if ["invalid api key", "invalid x-api-key", "invalid_api_key", "incorrect api key", "authentication_error", "authentication error", "http 401", "密钥无效", "密钥错误"].iter().any(|k| l.contains(k)) {
        return Some("API 密钥无效");
    }
    if ["not logged in", "please run /login", "claude login", "未登录"].iter().any(|k| l.contains(k)) {
        return Some("客户端没登录");
    }
    // 只认明确说模型不存在的：「MCP server not found」这类含 model 字样的别的错误不算（会把主模型全局停 30 分钟）
    let about_model = l.contains("model") && !l.contains("mcp") && !l.contains("server") && !l.contains("file") && !l.contains("tool");
    if ["unknown model", "模型不存在", "model not found", "model_not_found", "no such model"].iter().any(|k| l.contains(k)) || (about_model && ["not found", "does not exist", "not available"].iter().any(|k| l.contains(k))) {
        return Some("模型用不了");
    }
    None
}

/// 这个模型现在是不是记成用不了（返回原因）
pub fn model_down(m: &(String, String)) -> Option<String> {
    let k = model_key(m);
    let mut v = MODEL_DOWN.lock().unwrap();
    v.retain(|(_, until, _)| *until > now());
    v.iter().find(|(x, _, _)| *x == k).map(|(_, _, why)| why.clone())
}

fn mark_down(m: &(String, String), why: &str) {
    let k = model_key(m);
    let mut v = MODEL_DOWN.lock().unwrap();
    v.retain(|(x, _, _)| *x != k);
    v.push((k, now() + MODEL_DOWN_SECS, why.to_string()));
}

fn clear_down(m: &(String, String)) {
    let k = model_key(m);
    MODEL_DOWN.lock().unwrap().retain(|(x, _, _)| *x != k);
}

fn model_label(m: &(String, String)) -> String {
    if m.1.is_empty() { format!("{} · 默认模型", m.0) } else { format!("{} · {}", m.0, m.1) }
}

fn effort_override(site_id: &str) -> String {
    EFFORT_OVERRIDE.lock().unwrap().iter().find(|(s, _)| s == site_id).map(|(_, e)| e.clone()).unwrap_or_default()
}

fn set_effort_override(site_id: &str, e: String) {
    let mut v = EFFORT_OVERRIDE.lock().unwrap();
    v.retain(|(s, _)| s != site_id);
    if !e.is_empty() {
        v.push((site_id.to_string(), e));
    }
}

/// 同 start，可以给这个作业临时指定模型（「待处理」里每批交给 AI 时选的）。
pub fn start_with_model(ctx: Ctx, site_id: &str, task: &str, trigger: &str, item_id: Option<String>, model: Option<(String, String)>) -> Result<String, String> {
    start_with_pick(ctx, site_id, task, trigger, item_id, model.map(|(b, m)| (b, m, String::new())))
}

/// 同 start_with_model，再带上推理强度：（brain, model, effort）。
pub fn start_with_pick(ctx: Ctx, site_id: &str, task: &str, trigger: &str, item_id: Option<String>, pick: Option<(String, String, String)>) -> Result<String, String> {
    let st = ctx.state();
    let site = st.sites.get(site_id)?;
    let job = st.site_jobs.start(Some(ctx.app.clone()), site_id, &format!("ops:{task}"), task_label(task), steps_for(task))?;
    if let Some(ids) = item_id.as_deref().filter(|s| !s.is_empty() && *s != "hold") {
        job.set_items(ids.split(',').map(str::to_string).collect());
    }
    // 一个站点同时只有一个作业，按站点记就够；没指定的作业也要清掉，免得用上一次的
    set_effort_override(site_id, pick.as_ref().map(|(_, _, e)| e.clone()).unwrap_or_default());
    set_model_override(site_id, pick.map(|(b, m, _)| (b, m)));
    {
        let mut h = HOLD.lock().unwrap();
        h.retain(|s| s != site_id);
        if task == "cycle" && item_id.as_deref() == Some("hold") {
            h.push(site_id.to_string());
        }
        let mut m = MANUAL.lock().unwrap();
        m.retain(|s| s != site_id);
        if trigger == "manual" {
            m.push(site_id.to_string());
        }
    }
    let id = job.id.clone();
    let (task, trigger) = (task.to_string(), trigger.to_string());
    tauri::async_runtime::spawn(async move {
        // 作业跑着的时候不让电脑自动睡眠（睡过去 AI 回合和构建会被打断）
        let _awake = crate::background::job_guard();
        let started = now();
        let site_dir = ctx.site_dir(&site.id);
        // 任务本体放进单独的协程：里面 panic 了也能走到 job.finish，不会让作业一直显示「进行中」直到重启
        let mut body = {
            let (ctx, site, job, task, trigger) = (ctx.clone(), site.clone(), job.clone(), task.clone(), trigger.clone());
            tokio::spawn(async move {
                match task.as_str() {
                    "scan" => scan(&ctx, &site, &job, &trigger).await,
                    "health" => health(&ctx, &site, &job).await,
                    "report" => report(&ctx, &site, &job).await,
                    "review" => review(&ctx, &site, &job).await,
                    "work" => work_one(&ctx, &site, &job, item_id.as_deref().unwrap_or_default()).await,
                    "recheck" => recheck(&ctx, &site, &job, item_id.as_deref().unwrap_or_default()).await,
                    "batch" => work_many(&ctx, &site, &job, item_id.as_deref().unwrap_or_default()).await,
                    "plan" => plan(&ctx, &site, &job).await,
                    "cycle" => cycle(&ctx, &site, &job).await,
                    "watch" => watch(&ctx, &site, &job).await,
                    "accept" => accept(&ctx, &site, &job, item_id.as_deref().unwrap_or_default(), false).await,
                    "accept_many" => accept_many(&ctx, &site, &job, item_id.as_deref().unwrap_or_default()).await,
                    "resolve" => resolve(&ctx, &site, &job, item_id.as_deref()).await,
                    "rollback" => rollback(&ctx, &site, &job, item_id.as_deref().unwrap_or_default()).await,
                    _ => Err("未知的运营任务".into()),
                }
            })
        };
        let r: Result<String, String> = match tokio::time::timeout(MAX_JOB_DURATION, &mut body).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) if e.is_panic() => Err("Pilot 内部出错，这个任务已停止；详情见日志".into()),
            Ok(Err(_)) => Err("任务被中止".into()),
            Err(_) => {
                // 先停掉正在跑的命令和 AI 回合，给它一点时间自己收尾（会话状态能正常落库），再强行中止
                mark_killed(&site.id);
                ctx.state().site_jobs.kill(&site.id);
                let _ = tokio::time::timeout(Duration::from_secs(60), &mut body).await;
                body.abort();
                Err(format!("超过 {} 小时还没结束，已停止；详情见作业日志和会话", MAX_JOB_DURATION.as_secs() / 3600))
            }
        };
        // 作业超时被强停、Pilot 内部出错：任务体没机会把条目收尾，会一直停在「AI 处理中」。按启动恢复那套处理
        let why = match &r { Err(e) if e.starts_with("超过") => "作业超时被停止时", Err(e) if e.starts_with("Pilot 内部出错") => "Pilot 内部出错时", _ => "上次 Pilot 退出或重启时" };
        if r.is_err() && settle_site_interrupted(&ctx.state().sites.data_dir(&site.id), why) {
            ctx.emit_queue(&site.id);
        }
        let res = r.as_ref().map(|_| ()).map_err(|e| e.clone());
        clear_killed(&site.id);
        set_model_override(&site.id, None);
        set_effort_override(&site.id, String::new());
        HOLD.lock().unwrap().retain(|s| s != &site.id);
        MANUAL.lock().unwrap().retain(|s| s != &site.id);
        job.finish(&res);
        if matches!(task.as_str(), "scan" | "health" | "report" | "review" | "plan" | "cycle") {
            record_run(
                &site_dir,
                RunRecord {
                    id: job.id.clone(),
                    task: task.clone(),
                    trigger: trigger.clone(),
                    started,
                    finished: now(),
                    ok: r.is_ok(),
                    summary: r.clone().unwrap_or_default(),
                    error: r.clone().err().unwrap_or_default(),
                },
            );
        }
        if let Err(e) = &r {
            if trigger == "schedule" {
                ctx.notify(&format!("{} · {}失败", site.name, task_label(&task)), e.lines().next().unwrap_or(""));
            }
        }
        ctx.emit_queue(&site.id);
        // 这次有改动合进了主分支：剩下的待确认改动要是因此合不进去了，接着自动处理（见 resolve）
        if r.is_ok() && matches!(task.as_str(), "accept" | "accept_many" | "cycle" | "work" | "batch" | "recheck" | "scan") && !conflicting(&site, &site_dir).await.is_empty() {
            // 接着你手动的操作自动处理冲突：同样算手动，改好不发 Telegram
            if let Err(e) = start(ctx.clone(), &site.id, "resolve", if trigger == "manual" { "manual" } else { "auto" }, None) {
                eprintln!("[ops] 处理冲突没启动：{e}");
            }
        }
    });
    Ok(id)
}

async fn refresh_data(ctx: &Ctx, site: &Site, job: &Job) {
    job.step("data", "running", "");
    let meta = crate::site_data::refresh(site, &ctx.state().conns, &ctx.state().sites, &ctx.site_dir(&site.id), false).await;
    let errs: Vec<String> = [("GA4", &meta.ga4.error), ("Search Console", &meta.gsc.error), ("Bing", &meta.bing.error)]
        .iter()
        .filter(|(_, e)| !e.is_empty())
        .map(|(l, e)| format!("{l}：{e}"))
        .collect();
    for e in &errs {
        job.line(e);
    }
    job.step("data", if errs.is_empty() { "ok" } else { "failed" }, &errs.join("；"));
}

// ---------------------------------------------------------------------------
// 机会扫描
// ---------------------------------------------------------------------------

/// 相关度（0–3）低于这个就算「和站点定位基本无关」，扫描时不入队
const OFFTOPIC_BELOW: f64 = 0.75;

async fn scan(ctx: &Ctx, site: &Site, job: &Job, trigger: &str) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    refresh_data(ctx, site, job).await;
    job.step("scan", "running", "");
    // 页面体检（结构化数据、图片）一周才定时跑一次：报告超过一天就先用最近一次上线的产物重查，
    // 别拿旧报告出任务（xtexe 拿 6 天前的报告派了「缺 datePublished」，其实早修好了）
    let report = crate::page_audit::load(&site_dir);
    if report.checked_at + DAY < now() || report.v < crate::page_audit::AUDIT_V {
        if let Ok(out) = crate::syndicate::built_out(&ctx.state(), site) {
            let closed = crate::page_audit::refresh(site, &site_dir, out).await;
            if closed > 0 {
                job.line(&format!("体检报告超过一天，用最近一次上线的产物重查：{closed} 条结构化数据 / 图片任务清单里的页面已经没有问题了，标为完成"));
                ctx.emit_queue(&site.id);
            }
        }
    }
    let mut opps = crate::site_data::ranked_opportunities(site, &site_dir).await;
    // 内容过时要读页面的标题和更新日期：用最近一次上线的构建产物（没有就用代码目录里的）
    if let Ok(out) = crate::syndicate::built_out(&ctx.state(), site) {
        let facts = crate::site_make::page_facts(&out);
        opps.extend(site_ops::stale_opportunities(site, &site_dir, &facts, chrono::Local::now().date_naive()));
    }
    // 托管方案开了「跳过无关搜索词」：判断模型认为和站点定位基本无关的不交给 AI（看板里照样列出来、标明相关度低），省下改动额度
    let skip = crate::managed::load(&ctx.state().data_dir).group_of(&site.id).is_some_and(|g| g.skip_offtopic);
    let (wanted, offtopic): (Vec<_>, Vec<_>) = opps.iter().cloned().partition(|o| !skip || o.relevance.is_none_or(|r| r >= OFFTOPIC_BELOW));
    let added = site_ops::enqueue(&site_dir, &wanted, now(), COOLDOWN)?;
    job.line(&format!("发现 {} 个机会，新入队 {} 个", opps.len(), added.len()));
    // 两周前的计划里还没做的：过期撤掉，不然每周多出来的计划项越积越多、永远排在数据规则的机会前面
    let cutoff = now().saturating_sub(14 * DAY);
    let expired = site_ops::drop_open_where(&site_dir, |i| i.opp.kind == "plan" && i.created_at < cutoff)?;
    if expired > 0 {
        job.line(&format!("撤掉 {expired} 条两周前的计划项（没做完就过期了，下次计划会重新安排）"));
    }
    if !offtopic.is_empty() {
        job.line(&format!("{} 个机会的搜索词和站点定位基本无关（判断模型 Jev），没有入队", offtopic.len()));
    }
    job.step("scan", "ok", &format!("新入队 {} 个", added.len()));
    ctx.emit_queue(&site.id);
    if site.ops.level < 2 {
        job.step("work", "skipped", "第 1 档只分析，不处理");
        job.step("ship", "skipped", "");
        return Ok(format!("发现 {} 个机会，新入队 {} 个", opps.len(), added.len()));
    }
    // 托管停着（方案暂停或这个站点单独停止）：只找机会，不交给 AI；想改哪条在「未处理的机会」里手动点
    if let Some(g) = group_rules(ctx, site).filter(|g| !g.active_for(&site.id)) {
        let why = if g.paused { "托管方案已暂停" } else { "这个站点的托管已停止" };
        job.step("work", "skipped", &format!("{why}，只找机会不改"));
        job.step("quality", "skipped", "");
        job.step("ship", "skipped", "");
        return Ok(format!("发现 {} 个机会，新入队 {} 个（{why}，没有交给 AI；想改哪条在「未处理的机会」里手动点）", opps.len(), added.len()));
    }
    let (done, notes, stopped) = work_queue(ctx, site, job, trigger).await;
    match &stopped {
        Some(why) if done == 0 => job.step("work", "failed", why),
        _ => job.step("work", "ok", &format!("处理了 {done} 个")),
    }
    let proposed = site_ops::load_queue(&site_dir).iter().filter(|i| i.status == "proposed").count();
    if trigger == "schedule" && done > 0 {
        ctx.notify(&format!("{} · 机会扫描完成", site.name), &format!("处理了 {done} 个机会，{proposed} 条等你确认"));
    }
    Ok(format!("新入队 {} 个，处理 {done} 个。{}", added.len(), notes.join("；")))
}

/// 站点代码里「一篇一个文件」的内容（md / mdx …）的文件名（小写，index.md 取所在目录名）。
/// 这种页面各改各的文件，不会互相冲突，批量时不用合成一条；只有页面没有自己的内容文件（模板 / 代码生成的）才合。
async fn content_stems(ctx: &Ctx, site: &Site) -> std::collections::HashSet<String> {
    let gh = ctx.state().conns.get(&site.gh_conn_id).ok();
    let Ok(dir) = crate::site_git::code_dir(site, gh.as_ref()) else { return Default::default() };
    let out = crate::site_git::git_ok(&dir, &["ls-files"], "").await.unwrap_or_default();
    let mut set = std::collections::HashSet::new();
    for l in out.lines() {
        let p = Path::new(l.trim());
        if !p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "mdx" | "markdown" | "mdoc")) {
            continue;
        }
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let name = if stem.eq_ignore_ascii_case("index") { p.parent().and_then(|d| d.file_name()).and_then(|s| s.to_str()).unwrap_or("") } else { stem };
        if !name.is_empty() {
            set.insert(name.to_ascii_lowercase());
        }
    }
    set
}

fn has_own_file(stems: &std::collections::HashSet<String>, page: &str) -> bool {
    let last = page.split(['?', '#']).next().unwrap_or("").trim_end_matches('/').rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    !last.is_empty() && stems.contains(&last)
}

/// 按分数挑前 N 个还没处理的条目，按站点档位处理（改、检查、提 PR / 上线）。返回（处理了几个，说明，因预算停下的原因）。
async fn work_queue(ctx: &Ctx, site: &Site, job: &Job, trigger: &str) -> (usize, Vec<String>, Option<String>) {
    let site_dir = ctx.site_dir(&site.id);
    // 全托管：只挑方案允许的动作
    let rules = group_rules(ctx, site);
    // 托管方案：失败原因和任务本身无关的（中断、没建 git 仓库、模型临时用不了）放回排队重做一次，不用你手动点
    if rules.is_some() {
        if let Ok(ids) = site_ops::requeue_retryable(&site_dir, now(), || repo_ready(site)) {
            if !ids.is_empty() {
                job.line(&format!("{} 条上次因为中断、环境或模型问题失败的，放回排队重做一次", ids.len()));
            }
        }
    }
    if rules.is_some() {
        if let Some(err) = site_build_broken(site, &site_dir) {
            let why = format!("站点自己的构建 / 检查没通过（每日体检：{err}），AI 改什么都会卡在这一步，这一轮先不让 AI 改；在站点设置里修好构建或检查命令后会接着做");
            job.line(&why);
            return (0, vec![why], None);
        }
    }
    // 这个站现在发不了（服务器还没授权）：改好了也上不了线，先不让 AI 改，也不花额度
    if rules.is_some() {
        if let Some(why) = crate::site_deploy::deploy_blocker(site, &ctx.state().conns) {
            job.line(&why);
            return (0, vec![why], None);
        }
    }
    // 已经有改好的条目因为本地文件夹不干净上不了线：先别让 AI 接着改新的，不然越堆越多、白花额度；文件夹干净了调度会先把卡住的接着上线
    if rules.is_some() && site.source == "local" {
        let stuck = site_ops::load_queue(&site_dir).iter().filter(|i| i.status == "proposed" && i.verdict.starts_with("自动上线失败：站点文件夹")).count();
        if stuck > 0 && !local_ready_now(site).await {
            let why = format!("站点文件夹有你没提交的改动（或不在 {} 分支），已有 {stuck} 条改好的等着上线，这一轮先不让 AI 改新的", site.default_branch);
            job.line(&why);
            return (0, vec![why], None);
        }
    }
    let mut picked: Vec<QueueItem> = site_ops::load_queue(&site_dir).into_iter().filter(|i| i.status == "open" && rules.as_ref().is_none_or(|g| g.allows(&i.opp.change))).collect();
    // 速度优化上线后、重新测速之前：别的速度任务先放着（多半共用模板，刚上线的改动可能已经顺带修好了），等新的测速结果
    let last_slow = site_ops::load_queue(&site_dir).iter().filter(|i| i.opp.kind == "slow" && i.deployed_at > 0).map(|i| i.deployed_at).max().unwrap_or(0);
    if last_slow > crate::speed::load(&site_dir).checked_at {
        let before = picked.len();
        picked.retain(|i| i.opp.kind != "slow");
        if picked.len() < before {
            job.line(&format!("{} 条页面速度任务等刚上线的速度优化重新测速后再决定", before - picked.len()));
        }
    }
    picked.sort_by(|a, b| b.opp.score.total_cmp(&a.opp.score));
    // 各类机会的分数量级不可比（计划项固定 1000、修复任务 2000、数据规则几分到几十分）：不按绝对分数排，分四条道——
    // 修复上线后的问题 → 你手动加的 → 计划项和数据机会轮着来（上一条做的是计划项，这次先做数据机会），每条道里再按分数排
    let q = site_ops::load_queue(&site_dir);
    let last_lane = q.iter().filter(|i| i.worked_at > 0).max_by_key(|i| i.worked_at).map(lane_of).unwrap_or(3);
    picked = order_lanes(picked, last_lane);
    // 同一种页面的改标题 / 描述合成一条（同批量让 AI 改）
    let ids: Vec<String> = picked.iter().map(|i| i.id.clone()).collect();
    let own = content_stems(ctx, site).await;
    if let Ok((keep, n)) = site_ops::group_same_route(&site_dir, &ids, |p| has_own_file(&own, p)) {
        if n > 0 {
            job.line(&format!("有 {n} 条和别的是同一种页面，合在一起交给 AI"));
            let q = site_ops::load_queue(&site_dir);
            picked = keep.iter().filter_map(|id| q.iter().find(|i| &i.id == id).cloned()).collect();
        }
    }
    // 每轮最多改几条：托管站点用方案的「每轮最多改」，自己运营的用站点设置。
    // 动手前发现「问题已经没有了」的不占名额，接着看下一条
    let mut left = rules.as_ref().map(|g| g.per_round).unwrap_or(site.ops.budget.items_per_run).max(1) as usize;
    let mut done = 0;
    let mut notes = Vec::new();
    let mut stopped = None;
    for item in picked {
        if job.canceled() || left == 0 {
            break;
        }
        let b = budget(site, &site_dir, &ctx.state().convos);
        if let Some(why) = over_budget(&b) {
            job.line(&format!("{why}，本次运行暂停处理"));
            notes.push(why.clone());
            if trigger == "schedule" {
                ctx.notify(&format!("{} · 运营预算已用完", site.name), &why);
            }
            log(ctx, site, "budget", &why, "本轮剩下的任务没处理，额度恢复后接着做", "");
            stopped = Some(why);
            break;
        }
        job.reset_from("work");
        job.step("work", "running", &item.opp.title);
        match process(ctx, site, job, &item, false).await {
            Ok(msg) => {
                let gone = site_ops::load_queue(&site_dir).iter().any(|i| i.id == item.id && i.status == "done" && i.conv_id.is_empty());
                if !gone {
                    done += 1;
                    left -= 1;
                }
                notes.push(msg);
            }
            Err(e) => {
                // 失败不重放：标记失败、写明原因，等你处理；继续下一个机会。
                job.line(&format!("「{}」处理失败：{e}", item.opp.title));
                if e.starts_with(crate::site_verify::DOMAIN_TAKEN) {
                    stopped = Some(e);
                    break;
                }
                left -= 1;
                // 已经合并进主分支、只是上线没完成的留在「已合并」（可以重新上线）；验收失败的回滚流程已经写好状态；只有还在处理中的才标失败
                let (note, head) = failure_note(job, &e, &site_dir, &item, &site.id);
                let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| {
                    if i.status == "working" {
                        i.status = "failed".into();
                        i.note = note.clone();
                        if let Some(h) = &head {
                            i.head_sha = h.clone();
                        }
                    }
                });
            }
        }
        ctx.emit_queue(&site.id);
        // AI 回合被停掉（作业超时被 Pilot 强停、或你点了停止）：这一轮到此为止，别接着给下一条开新会话
        if site_ops::load_queue(&site_dir).iter().any(|i| i.id == item.id && i.note.starts_with(STOPPED) || i.id == item.id && i.note.starts_with("作业超时")) {
            break;
        }
    }
    (done, notes, stopped)
}

/// 动手前的确认：数据超过 6 小时先补拉一次（拉数据时会顺带重算机会），再看这条机会现在还成不成立。
/// 已经不成立就返回说明（调用方把它关掉、跳过）。AI 计划不判断；内容过时要读构建产物，读不到就不判断；数据源这次没拉成功也不判断。
async fn gone_already(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem) -> Option<String> {
    let kind = item.opp.kind.as_str();
    let stale = matches!(kind, "stale" | "stale_year");
    if !(site_ops::SELF_CHECKING.contains(&kind) || site_ops::DATA_DRIVEN.contains(&kind) || stale) {
        return None;
    }
    let site_dir = ctx.site_dir(&site.id);
    // 体检类（结构化数据、图片）看的是体检报告：报告还没有、或比这条任务还旧，不能凭它说「问题没了」；
    // 报告是新的就按清单里的页面逐个看，这些页面都没问题了才算没了
    if matches!(kind, "schema" | "img_audit") {
        // 报告是旧规则算的：先按新规则重查，排着的任务可能已经不算问题了
        if crate::page_audit::load(&site_dir).v < crate::page_audit::AUDIT_V {
            if let Ok(out) = crate::syndicate::built_out(&ctx.state(), site) {
                crate::page_audit::refresh(site, &site_dir, out).await;
            }
        }
        let r = crate::page_audit::load(&site_dir);
        let evidence = std::iter::once(item.opp.evidence.as_str()).chain(item.also.iter().map(|o| o.evidence.as_str())).collect::<Vec<_>>().join("\n");
        if r.checked_at <= item.created_at || crate::page_audit::still_has(&r, kind, &evidence) {
            return None;
        }
        job.line(&format!("「{}」：最新的体检里这些页面已经没有问题了，不交给 AI", item.opp.title));
        return Some("动手前按最新的体检确认了一下，这些页面的问题已经修好了，没有交给 AI".into());
    }
    let meta = crate::site_data::refresh(site, &ctx.state().conns, &ctx.state().sites, &site_dir, false).await;
    if !crate::site_data::source_ok(kind, &meta) {
        return None;
    }
    let mut opps: Vec<site_ops::Opportunity> = crate::site_data::read_json(&crate::site_data::cache_dir(&site_dir), "opportunities.json");
    if stale {
        let out = crate::syndicate::built_out(&ctx.state(), site).ok()?;
        opps.extend(site_ops::stale_opportunities(site, &site_dir, &crate::site_make::page_facts(&out), chrono::Local::now().date_naive()));
    }
    let live = crate::site_data::live_opportunities(&opps, &site_dir);
    if live.iter().any(|o| o.key == item.opp.key || item.also.iter().any(|a| a.key == o.key)) {
        return None;
    }
    job.line(&format!("「{}」：按最新数据看，这个问题已经没有了，不交给 AI", item.opp.title));
    Some("动手前按最新数据确认了一下，这个问题已经没有了，没有交给 AI".into())
}

/// 立即执行一整轮（托管方案里「立即执行」）：制定本周计划 → 按方案处理排在前面的几条
/// （全自动按发布规则自动上线，改好等你确认停在等你确认，只出建议只出计划）。
/// 本周已经有计划、里面还有没做的就沿用，不再花一轮重新制定；本周用量已满就不开始。
/// 「执行但不上线」（启动时 item_id 传 "hold"，见 HOLD）：这一轮照常制定计划、改、跑检查和质检，但改好的都留在「需要你」等你确认。
async fn cycle(ctx: &Ctx, site: &Site, job: &Job) -> Result<String, String> {
    let t0 = now();
    let site_dir = ctx.site_dir(&site.id);
    if let Some(why) = over_budget(&budget(site, &site_dir, &ctx.state().convos)) {
        job.step("data", "skipped", "");
        job.step("plan", "skipped", &why);
        job.step("work", "skipped", "");
        job.step("ship", "skipped", "");
        return Err(format!("{why}，这一轮没有开始。{BUDGET_HINT}"));
    }
    let left = site_ops::load_queue(&site_dir).iter().filter(|i| i.status == "open" && i.opp.kind == "plan").count();
    let planned = if left > 0 && site.ops.level >= 2 {
        refresh_data(ctx, site, job).await;
        job.step("plan", "ok", &format!("沿用本周计划，还有 {left} 项没做"));
        job.line(&format!("本周计划里还有 {left} 项没做，直接按计划处理，不重新制定"));
        format!("沿用本周计划（还有 {left} 项）")
    } else {
        plan(ctx, site, job).await?
    };
    if site.ops.level < 2 {
        job.step("work", "skipped", "只出建议：只出计划，不改代码");
        job.step("ship", "skipped", "");
        return Ok(planned);
    }
    let (done, notes, stopped) = work_queue(ctx, site, job, "manual").await;
    if let (Some(why), 0) = (&stopped, done) {
        job.step("work", "failed", why);
        job.step("ship", "skipped", "");
        return Err(format!("{planned}，但{why}，没有开始改动。{BUDGET_HINT}"));
    }
    job.step("work", "ok", &format!("处理了 {done} 个"));
    let q = site_ops::load_queue(&site_dir);
    let shipped = q.iter().filter(|i| i.status == "deployed" && i.deployed_at >= t0).count();
    let waiting = q.iter().filter(|i| i.status == "proposed").count();
    if shipped == 0 {
        job.step("ship", if waiting > 0 { "ok" } else { "skipped" }, &format!("{waiting} 条等你确认"));
    }
    Ok(format!("{planned}；处理 {done} 个，上线 {shipped} 个，{waiting} 条等你确认。{}", notes.join("；")))
}

const OPS_MAX_REQUESTS: usize = 120;

const BUDGET_HINT: &str = "可以在托管方案里调高「每站每周 token」，或等用量滚出最近 7 天。";

/// 手动对某一条机会立即按站点档位处理。
async fn work_one(ctx: &Ctx, site: &Site, job: &Job, item_id: &str) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let item = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item_id).ok_or("这条机会不存在")?;
    if !matches!(item.status.as_str(), "open" | "failed") {
        return Err("这条机会已经在处理或处理过了".into());
    }
    let b = budget(site, &site_dir, &ctx.state().convos);
    if let Some(why) = over_tokens(&b) {
        return Err(why);
    }
    job.step("work", "running", &item.opp.title);
    // 你手动点「让 AI 改」：没托管或「只出建议」的站点也按「改好等你确认」处理
    let r = process(ctx, site, job, &item, true).await;
    if let Err(e) = &r {
        let (note, head) = failure_note(job, e, &site_dir, &item, &site.id);
        let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| {
            if i.status == "working" {
                i.status = "failed".into();
                i.note = note.clone();
                if let Some(h) = &head {
                    i.head_sha = h.clone();
                }
            }
        });
    }
    r
}

/// 批量让 AI 改：按顺序一条一条处理（每条都按站点档位：改、检查、上线或等你确认），额度用完就停下。
/// `ids` 用逗号分隔。
async fn work_many(ctx: &Ctx, site: &Site, job: &Job, ids: &str) -> Result<String, String> {
    let ids: Vec<String> = ids.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect();
    if ids.is_empty() {
        return Err("没有选中的条目".into());
    }
    // 同一种页面的改标题 / 描述合成一条：一个分支一起改，免得各改各的、先合的让后面的合不进去
    let own = content_stems(ctx, site).await;
    let (ids, grouped) = site_ops::group_same_route(&ctx.site_dir(&site.id), &ids, |p| has_own_file(&own, p))?;
    if grouped > 0 {
        job.line(&format!("有 {grouped} 条和别的是同一种页面（多半是同一个模板生成的），合在一起交给 AI，一次改好"));
        ctx.emit_queue(&site.id);
    }
    let (mut ok, mut failed, mut notes) = (0, 0, Vec::new());
    for (n, id) in ids.iter().enumerate() {
        if job.canceled() {
            notes.push("已取消".to_string());
            break;
        }
        let b = budget(site, &ctx.site_dir(&site.id), &ctx.state().convos);
        if let Some(why) = over_tokens(&b) {
            job.line(&format!("{why}，剩下的 {} 条没有处理", ids.len() - n));
            notes.push(why);
            break;
        }
        job.line(&format!("—— 第 {}/{} 条 ——", n + 1, ids.len()));
        if n > 0 {
            job.reset_from("work");
        }
        match work_one(ctx, site, job, &id).await {
            Ok(m) => {
                ok += 1;
                job.line(&m);
            }
            Err(e) => {
                failed += 1;
                job.line(&format!("这一条没成功：{e}"));
            }
        }
        ctx.emit_queue(&site.id);
    }
    let summary = format!("处理 {ok} 条{}{}", if failed > 0 { format!("，{failed} 条没成功") } else { String::new() }, if notes.is_empty() { String::new() } else { format!("。{}", notes.join("；")) });
    if ok == 0 && failed > 0 {
        return Err(summary);
    }
    Ok(summary)
}

/// 重新检查一条失败的改动：AI 的提交还在分支上（比如只是构建环境出了问题），重新构建、检查并按规则上线，不再让 AI 改一遍。
async fn recheck(ctx: &Ctx, site: &Site, job: &Job, item_id: &str) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let item = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item_id).ok_or("这条机会不存在")?;
    if !matches!(item.status.as_str(), "failed" | "proposed") {
        return Err("只有失败或等你确认的条目可以重新检查".into());
    }
    let wt = crate::sites::build_root(&site_dir).join("work").join(&item.id);
    if item.branch.is_empty() || item.base_sha.is_empty() {
        return Err("这条的改动分支已经不在了，只能让 AI 重新改".into());
    }
    let cb = code_base(ctx, site, job).await?;
    // 工作树不在了（挪过构建目录、被清理过）但分支还在：从分支重新检出一份，接着检查
    if !wt.is_dir() {
        let has = crate::site_git::git(&cb.main, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{}", item.branch)], "").await.map(|o| o.ok).unwrap_or(false);
        if !has {
            return Err("这条的改动分支已经不在了，只能让 AI 重新改".into());
        }
        crate::site_git::reattach_worktree(&cb.main, &wt, &item.branch).await?;
        job.line("工作目录挪到了新的构建目录，接着用分支上的改动");
    }
    crate::site_git::share_node_modules(&cb.main, &wt);
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.status = "working".into();
        i.note.clear();
    })?;
    ctx.emit_queue(&site.id);
    job.step("work", "ok", "沿用 AI 已经提交的改动");
    let level = effective_level(site, &item).max(3);
    let r = verify_and_ship(ctx, site, job, &item, level, &cb, &wt, &item.base_sha, &item.branch).await;
    if let Err(e) = &r {
        let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| {
            if i.status == "working" {
                i.status = "failed".into();
                i.note = e.clone();
            }
        });
    }
    r
}

/// 某个机会实际可用的最高档位：第 1–4 档里新内容、技术问题、被复盘降档的类型最多到第 3 档；全托管见下；只分析档不处理。
pub fn effective_level(site: &Site, item: &QueueItem) -> u8 {
    // 并进来的同页面机会（also）一起算：哪一项要你确认，这一条就要你确认
    let opps: Vec<&Opportunity> = std::iter::once(&item.opp).chain(item.also.iter()).collect();
    let mut level = site.ops.level;
    // 老数据里降档记的是细分类型（title / meta_description），都算「改标题 / 描述」
    let downgraded = |c: &str| site.ops.downgraded.iter().any(|d| d == c || (c == "meta" && matches!(d.as_str(), "title" | "meta_description")));
    if level >= 5 {
        // 全托管：技术问题、改事实、性能、结构化数据这些也按托管方案走——方案允许这类改动、上线判定（代码改动设置、
        // 危险写法、推荐内容、改动大小）和内容质检都过了就自动上线，上线后验收没过自动回滚。
        // 只有合并或拉开抢词的页面（要删页面、做跳转，得人判断留哪个）和被复盘降档的类型留给你确认。
        if opps.iter().any(|o| o.kind == "cannibal" || downgraded(&o.change)) {
            return 3;
        }
        return 5;
    }
    // 改事实（久未更新的页面）、合并或拉开页面（两个页面抢词）：第 1–4 档都留给你确认
    // 收入偏低、页面太慢、页面体检、对比竞品：改动面大或要人判断，也都留给你确认
    if opps.iter().any(|o| matches!(o.kind.as_str(), "stale" | "cannibal" | "low_rpm" | "slow" | "img_audit" | "schema" | "serp_gap")) {
        return level.min(3);
    }
    if opps.iter().any(|o| matches!(o.change.as_str(), "new_content" | "technical") || downgraded(&o.change)) {
        level = level.min(3);
    }
    level
}

fn log(ctx: &Ctx, site: &Site, kind: &str, title: &str, detail: &str, item_id: &str) {
    crate::managed::log(&ctx.state().data_dir, crate::managed::Activity {
        ts: now(),
        site_id: site.id.clone(),
        site_name: site.name.clone(),
        kind: kind.into(),
        title: title.into(),
        detail: detail.into(),
        item_id: item_id.into(),
        ..Default::default()
    });
}

/// 和某条改动有关的动态：带上它的动作类型和来源。
fn log_item(ctx: &Ctx, site: &Site, kind: &str, item: &QueueItem, detail: &str) {
    crate::managed::log(&ctx.state().data_dir, crate::managed::Activity {
        ts: now(),
        site_id: site.id.clone(),
        site_name: site.name.clone(),
        kind: kind.into(),
        title: item.opp.title.clone(),
        detail: detail.into(),
        item_id: item.id.clone(),
        change: item.opp.change.clone(),
        source: source_of(&item.opp).into(),
    });
}

/// 机会的来源；老数据没有 source 字段时按规则类型推断。
pub fn source_of(o: &Opportunity) -> &str {
    if !o.source.is_empty() {
        return &o.source;
    }
    match o.kind.as_str() {
        "crawl_issue" | "sitemap_error" | "slow" | "img_audit" | "schema" => "health",
        "content_gap" => "gap",
        _ => "data",
    }
}

async fn process(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem, manual: bool) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    // 域名指着别的东西：改了也验不了、上不了线，不花 AI 额度。新页面没有网址，拿队列里一个已知的页面查
    let queue = site_ops::load_queue(&site_dir);
    let known = if item.opp.page.is_empty() { queue.iter().map(|i| i.opp.page.as_str()).find(|p| !p.is_empty()).unwrap_or_default() } else { item.opp.page.as_str() };
    if let Some(why) = crate::launch_cmds::domain_taken_at(&ctx.state().conns, site, known).await {
        pause_for_domain(ctx, site, &why);
        job.step("work", "failed", &why);
        return Err(format!("{}{why}", crate::site_verify::DOMAIN_TAKEN));
    }
    // 动手前先确认问题还在：机会是按某一天的数据算的，过了几天可能已经好了，就不让 AI 去改
    if let Some(note) = gone_already(ctx, site, job, item).await {
        if let Some(d) = site_ops::update_item(&site_dir, &item.id, now(), |i| {
            i.status = "done".into();
            i.note = note.clone();
        })? {
            log_item(ctx, site, "fixed", &d, &note);
        }
        ctx.emit_queue(&site.id);
        job.step("work", "ok", "问题已经没有了，没交给 AI");
        return Ok(note);
    }
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.status = "working".into();
        i.worked_at = now();
        i.note.clear();
    })?;
    ctx.emit_queue(&site.id);
    let level = effective_level(site, item);
    let level = if manual { level.max(3) } else { level };
    // sitemap 报错先由程序检查：多半是提交的地址不对或者还没被重新读取，改交 / 重新提交就好，不用 AI 改代码
    let checked;
    let item = if item.opp.kind == "sitemap_error" && level >= 2 {
        job.line("先检查 sitemap 能不能打开、Search Console 里提交的地址对不对");
        match crate::sitemap_fix::precheck(site, &site_dir, &ctx.state().conns, item).await {
            crate::sitemap_fix::Outcome::Fixed(note) => {
                let done = site_ops::update_item(&site_dir, &item.id, now(), |i| {
                    i.status = "done".into();
                    i.note = note.clone();
                })?;
                if let Some(d) = &done {
                    log_item(ctx, site, "fixed", d, &note);
                }
                job.step("work", "ok", "程序检查后处理好了，没有改代码");
                return Ok(note);
            }
            crate::sitemap_fix::Outcome::Manual(note) => {
                // 和第 2 档的方案一样：等你处理完点确认，标记完成
                let left = site_ops::update_item(&site_dir, &item.id, now(), |i| {
                    i.status = "proposed".into();
                    i.note = note.clone();
                })?;
                if let Some(l) = &left {
                    log_item(ctx, site, "proposed", l, &note);
                }
                job.step("work", "ok", "查清楚了，要你在 Search Console 里操作一下");
                return Ok(note);
            }
            crate::sitemap_fix::Outcome::Ai(diag) if !diag.is_empty() => {
                job.line(&diag);
                let mut it = item.clone();
                it.opp.evidence = format!("{}\n{diag}", it.opp.evidence);
                let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| i.opp.evidence = it.opp.evidence.clone());
                checked = it;
                &checked
            }
            crate::sitemap_fix::Outcome::Ai(_) => item,
        }
    } else {
        item
    };
    match level {
        0 | 1 => Ok(String::new()),
        2 => propose(ctx, site, item).await,
        level => implement(ctx, site, job, item, level).await,
    }
}

/// 「AI 怎么改的」只要 AI 最后那段说明：一轮的回复里还夹着中途的过程话（「构建失败了，我再跑一次」），
/// 去掉末尾的「新页面 / 改动的页面」两行，取最后一段（太短就带上前一段），最多 600 字。
fn final_summary(reply: &str) -> String {
    let body: Vec<&str> = reply
        .lines()
        .filter(|l| {
            let t = l.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches("**");
            !["新页面：", "新页面:", "改动的页面：", "改动的页面:", "还要改：", "还要改:"].iter().any(|p| t.starts_with(p))
        })
        .collect();
    let text = body.join("\n");
    let paras: Vec<&str> = text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut pick: Vec<&str> = paras.iter().rev().take(1).copied().collect();
    if pick.first().is_some_and(|p| p.chars().count() < 60) {
        pick = paras.iter().rev().take(2).copied().collect();
    }
    pick.reverse();
    pick.join("\n\n").chars().take(600).collect()
}

/// AI 回复里「不用改：原因」那一行的原因。
fn no_change_reason(summary: &str) -> Option<String> {
    summary.lines().rev().find_map(|l| {
        let t = l.trim().trim_matches(['*', '`', ' ']);
        let why = t.strip_prefix("不用改：").or_else(|| t.strip_prefix("不用改:"))?.trim().trim_matches(['*', '`']).trim();
        (!why.is_empty()).then(|| why.to_string())
    })
}

fn path_of(url: &str) -> String {
    url.split_once("://")
        .map(|(_, rest)| rest.find('/').map(|i| rest[i..].to_string()).unwrap_or_else(|| "/".into()))
        .unwrap_or_else(|| url.to_string())
}

/// 交给 AI 的依据：这一条 + 并进来的同页面机会（一起改，不要分开改两遍）
fn evidence_all(item: &QueueItem) -> String {
    let mut s = evidence_block(&item.opp);
    // 并进来的同页问题最多展开 6 条，再多只说一句，任务说明别撑到几万字
    if item.also.len() > 6 {
        s.push_str(&format!("\n（这个页面另有 {} 个同类问题这次一并处理，依据略）\n", item.also.len() - 6));
    }
    for o in item.also.iter().take(6) {
        let same_page = o.page.trim().trim_end_matches('/') == item.opp.page.trim().trim_end_matches('/');
        let head = if same_page { "同一个页面还有这个问题，这次一起处理" } else { "同一种页面还有这个问题，这次一起处理（这几页多半是同一个模板或同一段代码生成的：统一改法，别一页写一套，也别改坏其它同类页面）" };
        s.push_str(&format!("\n{head}：{}\n{}改动方向：{}\n", o.title, evidence_block(o), guidance(o)));
    }
    s
}

fn evidence_block(o: &Opportunity) -> String {
    let mut s = format!("类型：{}\n", crate::site_ops::kind_label(&o.change));
    if !o.page.is_empty() {
        s.push_str(&format!("页面：{}\n", o.page));
    }
    s.push_str(&format!("依据：{}\n", o.evidence));
    if !o.queries.is_empty() {
        s.push_str("相关查询词（近 28 天，Search Console）：\n");
        for q in o.queries.iter().take(8) {
            s.push_str(&format!(
                "- 「{}」展现 {:.0}、点击 {:.0}、点击率 {:.1}%、平均排名 {:.1}\n",
                q.key,
                q.impressions,
                q.clicks,
                q.ctr * 100.0,
                q.position
            ));
        }
    }
    s
}

/// 改动方向：两个页面抢词、内容过时这两类有自己的做法（依据里写了具体步骤），其余按改动类型。
fn guidance(o: &Opportunity) -> &'static str {
    match o.kind.as_str() {
        "cannibal" => "按依据里的步骤二选一：把弱的页面正文并进强的页面（弱的页面留着，正文改成简短说明并链到强的页面；不做跳转、不删页面、不改网址——跳转配置是不自动改的）或拉开两个页面的角度。只动这两个页面和指向它们的站内链接。",
        "stale" => "只核对和更新有时效的信息（数字、活动、步骤、规定），保持原有结构和语气；拿不准的不要改，写进说明。不改网址。",
        "stale_year" => "只改标题和描述里表示「今年 / 最新」的年份，其它文字不动。不改正文、不改网址。",
        "low_rpm" => "让读者愿意多看、多点：补实用的内容和小结，在合适位置加站内相关文章链接；和主题相关时可以用站点已有的推广链接（/go/…）。不新增广告代码、不堆广告位、不改网址。",
        "slow" => "按依据里 PageSpeed 列出的问题做最小的优化：压缩或换成合适尺寸的图片、给图片标宽高、首屏以外的图片懒加载、推迟不必要的脚本。不改页面内容和外观。",
        "img_audit" => "按依据里的清单处理图片：缺的替代文字（alt）按图片内容写一句简短描述（装饰图用 alt=\"\"）；修好或删掉引用不存在的图片；过大的图片压缩或缩小尺寸。不改正文。",
        "schema" => "修好或补上页面的结构化数据（JSON-LD）：按依据里的问题改，字段和页面上显示的内容一致，不编造作者、日期和评分。不改正文。",
        "serp_gap" => "对比依据里排在前面的页面（看它们的标题和摘要回应了什么），补上这个页面缺的要点、把标题和描述写得更对题；用自己的话写，不要照抄别人的内容。不改网址。",
        _ => change_guidance(&o.change),
    }
}

fn change_guidance(change: &str) -> &'static str {
    match change {
        "meta" => "只改这个页面的标题（<title> / frontmatter 的 title）和描述（meta description / frontmatter 的 description），让它们直接回应主要查询词、写清读者能得到什么；描述 50–160 字。不改正文、不改网址。",
        "content" => "补充或改写正文里能回应这些查询词的段落（保持原有结构和语气），必要时在相关页面加指向它的站内链接。不改网址。",
        "internal_links" => "只在相关页面的正文里新增指向这个页面的站内链接，不删改已有链接和正文。",
        "new_content" => "写一个专门回应这个查询词的新页面，并从相关页面加站内链接指向它；遵循站点现有的内容结构和写法。",
        "technical" => "修复这个技术问题（死链、跳转、sitemap 等），改动尽量小。",
        _ => "按上面的依据做最小、最集中的改动。",
    }
}

/// 没选具体模型时用什么：Claude 用 Sonnet 5 的完整 ID（不用 `sonnet` 别名：CLI 会把别名解析成旧版本，
/// 界面上写着 Sonnet、实际跑的是 4.6；和前端模型列表里的「Sonnet」是同一个）；
/// Codex 留空＝不传模型参数、用本机 codex 配置里的默认模型（和对话里「跟随 Codex 默认」一样）；其它服务必须明确选择。
pub(crate) const SONNET: &str = "claude-sonnet-5";

fn default_model(brain: &str) -> Option<String> {
    match brain {
        "claude" => Some(SONNET.into()),
        "codex" => Some(String::new()),
        _ => None,
    }
}

/// 无人值守任务用的模型。
pub fn ops_model(site: &Site) -> Result<String, String> {
    if !site.ops.model.trim().is_empty() {
        return Ok(site.ops.model.trim().to_string());
    }
    default_model(&site.ops.brain).ok_or_else(|| "请在站点「运营」设置里为无人值守任务选择模型".into())
}

/// 这个站点无人值守任务用的（主模型，备用模型）。全托管站点用所在托管方案里运营人员选的那一对。
pub fn models_for(site: &Site, data_dir: &Path) -> Result<((String, String), Option<(String, String)>), String> {
    // 这个作业临时选了模型：只用它，不换备用
    if let Some(m) = model_override(&site.id) {
        return Ok((m, None));
    }
    if let Some(m) = crate::managed::load(data_dir).group_of(&site.id).cloned() {
        let pick = |b: &str, model: &str| -> Result<(String, String), String> {
            if !model.trim().is_empty() {
                return Ok((b.to_string(), model.trim().to_string()));
            }
            default_model(b).map(|m| (b.to_string(), m)).ok_or_else(|| "请在「托管运营 → 托管方案」里为这个方案的主模型 / 备用模型选择具体模型".into())
        };
        let primary = pick(&m.brain, &m.model)?;
        let fallback = if m.fallback_brain.is_empty() { None } else { Some(pick(&m.fallback_brain, &m.fallback_model)?) };
        return Ok((primary, fallback));
    }
    Ok(((site.ops.brain.clone(), ops_model(site)?), None))
}

/// 托管方案给这个模型设的推理强度：是主模型就用主模型的档位，是备用模型就用备用的（没写具体模型的按默认模型比）
fn group_effort(g: &crate::managed::ManagedGroup, model: &(String, String)) -> String {
    let same = |b: &str, m: &str| b == model.0 && (m.trim() == model.1 || (m.trim().is_empty() && default_model(b).as_deref() == Some(model.1.as_str())));
    if same(&g.brain, &g.model) {
        g.effort.clone()
    } else if !g.fallback_brain.is_empty() && same(&g.fallback_brain, &g.fallback_model) {
        g.fallback_effort.clone()
    } else {
        String::new()
    }
}

/// 无人值守地开一条站点会话（origin=ops），跑完首轮才返回。主模型这一轮失败时，有备用模型就换它重跑一次
/// （新开一条会话，接着同一个工作区；备用模型能看到主模型已经做了的改动）。
async fn run_conversation(ctx: &Ctx, site: &Site, perm: &str, work_dir: &str, message: String) -> Result<(String, crate::convo::Conversation), String> {
    let st = ctx.state();
    let (primary, fallback) = models_for(site, &st.data_dir)?;
    // 主模型刚因为余额 / 密钥这类原因失败过：这段时间直接用备用模型
    if let (Some(why), Some(fb)) = (model_down(&primary), fallback.as_ref()) {
        st.site_jobs.line(&ctx.app, &site.id, &format!("主模型 {} 现在用不了（{why}），这次直接用备用模型 {}", model_label(&primary), model_label(fb)));
        return run_conversation_with(ctx, site, perm, work_dir, message, fb).await;
    }
    match run_conversation_with(ctx, site, perm, work_dir, message.clone(), &primary).await {
        Ok(r) => {
            clear_down(&primary);
            Ok(r)
        }
        // 作业被用户取消：这一轮是被停掉的，不是模型失败，不换备用模型重跑
        Err(_) if st.site_jobs.is_canceled(&site.id) => Err("已取消".into()),
        // 你在会话里点了停止：同样不换备用模型
        Err(e) if e.starts_with(STOPPED) => Err(e),
        Err(e) => {
            if let Some(why) = persistent_failure(&e) {
                mark_down(&primary, why);
            }
            let Some(fb) = fallback else { return Err(e) };
            if let Some(why) = persistent_failure(&e) {
                st.site_jobs.line(&ctx.app, &site.id, &format!("主模型 {} 用不了（{why}），改用备用模型 {}；{} 分钟内这个站点和其它站点都直接用备用模型", model_label(&primary), model_label(&fb), MODEL_DOWN_SECS / 60));
            }
            crate::managed::log(&st.data_dir, crate::managed::Activity {
                ts: now(),
                site_id: site.id.clone(),
                site_name: site.name.clone(),
                kind: "fallback".into(),
                title: format!("主模型失败，改用备用模型 {}", if fb.1.is_empty() { format!("{} · 默认模型", fb.0) } else { format!("{} · {}", fb.0, fb.1) }),
                detail: e.lines().next().unwrap_or("").to_string(),
                ..Default::default()
            });
            let note = format!("\n\n（上一次由另一个模型执行时中断了：{}。工作区里可能已有部分改动，先查看现状再继续。）", e.lines().next().unwrap_or(""));
            run_conversation_with(ctx, site, perm, work_dir, format!("{message}{note}"), &fb).await
        }
    }
}

/// 写给运营人员看的话一律用中文（站点是英文的，模型容易顺手用英文汇报）；页面内容照站点自己的语言写。
const REPORT_IN_CHINESE: &str = "给我（运营人员）的说明一律用中文写，不管站点是什么语言；页面里的内容照站点自己的语言写。";

const STOPPED: &str = "这一轮 AI 被停止了，改到一半的不会上线；要继续就再交给 AI";

async fn run_conversation_with(ctx: &Ctx, site: &Site, perm: &str, work_dir: &str, message: String, model: &(String, String)) -> Result<(String, crate::convo::Conversation), String> {
    let st = ctx.state();
    // 指定了独立的工作目录（AI 改动的工作树），就必须真的在那里跑：认不出来时宁可不跑，也不能退回站点的代码目录
    // （本地来源就是你自己的文件夹，AI 会直接改到你的主分支上）
    if !work_dir.trim().is_empty() && crate::site_cmds::site_work_override(&st.sites, &site.id, work_dir).is_none() {
        return Err(format!("AI 的工作目录不对（{work_dir}），为了不改到你自己的代码目录，这次没有交给 AI"));
    }
    let conv_id = uuid::Uuid::new_v4().to_string();
    let _guard = UnattendedGuard::start(&conv_id, st.data_dir.join("permit").join("pending"));
    // 推理强度：临时选了模型就用那次选的（主模型失败换备用模型时不带过去）；托管站点用方案里给主 / 备用模型各自设的档位
    let effort = if model_override(&site.id).is_some() {
        if model_override(&site.id).as_ref() == Some(model) { effort_override(&site.id) } else { String::new() }
    } else {
        crate::managed::load(&st.data_dir).group_of(&site.id).map(|g| group_effort(g, model)).unwrap_or_default()
    };
    let (brain, model) = model.clone();
    // 把这一轮的实时事件转给前端（ops-turn），打开这条会话就能看到 AI 正在做什么；
    // 第一条事件到达时刷新会话列表，会话一开始就出现在侧栏里，而不是跑完才出现。
    let channel = {
        let app = ctx.app.clone();
        let id = conv_id.clone();
        let announced = std::sync::atomic::AtomicBool::new(false);
        tauri::ipc::Channel::<crate::agent::TurnEvent>::new(move |body| {
            if !announced.swap(true, std::sync::atomic::Ordering::Relaxed) {
                let _ = app.emit("conversations-changed", ());
            }
            if let tauri::ipc::InvokeResponseBody::Json(raw) = body {
                if let Ok(ev) = serde_json::from_str::<serde_json::Value>(&raw) {
                    let _ = app.emit("ops-turn", serde_json::json!({ "conv_id": id, "event": ev }));
                }
            }
            Ok(())
        })
    };
    // 记在作业上：取消作业时连这一轮一起停（见 Jobs::cancel）
    st.site_jobs.set_conversation(&site.id, &conv_id);
    let conv = crate::create_conversation(
        st.conns.clone(),
        st.sites.clone(),
        st.convos.clone(),
        st.runs.clone(),
        conv_id.clone(),
        crate::site_cmds::site_conn_id(&site.id),
        site.id.clone(),
        site.name.clone(),
        "site".into(),
        brain,
        model,
        perm.into(),
        effort,
        false,
        work_dir.into(),
        message,
        channel,
        st.data_dir.clone(),
        st.ssh.clone(),
        // 无人值守：写一整篇新页面要读站点、查数据、写文件、跑检查，32 次模型调用不够；
        // 上限只防死循环，费用由每站每周 token 额度管。改动任务（auto）在独立工作树里，自动档本身就能跑构建和提交。
        crate::api_execution::ApiExecutionPolicy { max_requests: OPS_MAX_REQUESTS },
        "ops".into(),
    )
    .await;
    st.site_jobs.set_conversation(&site.id, "");
    let conv = conv?;
    let _ = ctx.app.emit("conversations-changed", ());
    let last = conv.messages.last().filter(|m| m.role == "assistant");
    // 有半截回答的失败（你在会话里点了停止、中途出错）也算没完成：改到一半的不能拿去构建、上线
    if let Some(m) = last.filter(|m| m.error || !m.unfinished.is_empty()) {
        if m.unfinished.starts_with("已停止") {
            return Err(STOPPED.into());
        }
        let e = if m.unfinished.is_empty() { &m.text } else { &m.unfinished };
        // 一上来就失败、什么都没做的（余额不足、密钥不对这类）：会话从侧栏收起，失败原因在作业日志和下一条会话的开头
        if persistent_failure(e).is_some() && conv.messages.iter().all(|m| m.tools.is_empty()) {
            if let Some(mut c) = st.convos.get(&conv_id) {
                c.folded = true;
                let _ = st.convos.upsert(c);
                let _ = ctx.app.emit("conversations-changed", ());
            }
        }
        return Err(format!("模型这一轮没有完成：{}", e.lines().next().unwrap_or("")));
    }
    Ok((conv_id, conv))
}

/// 第 2 档：开一条「只出方案」的会话，等你在会话里让 AI 动手。
async fn propose(ctx: &Ctx, site: &Site, item: &QueueItem) -> Result<String, String> {
    let msg = format!(
        "【运营 · 出方案】{title}\n\n{ev}\n请先查看这个页面对应的源文件，弄清现在的标题、描述、正文结构和站内链接，然后给出具体的改动方案：\n\
- 改哪些文件、哪几处、改成什么（标题和描述直接写出 2–3 个候选文案）；\n\
- 为什么这样改能回应上面的搜索意图；\n\
- 风险和需要我确认的地方。\n\
改动方向：{guide}\n\
这一轮只出方案，不要修改文件。我确认后会在这条会话里让你动手。\n{lang}",
        title = item.opp.title,
        ev = evidence_all(item),
        guide = guidance(&item.opp),
        lang = REPORT_IN_CHINESE,
    );
    let (conv_id, _) = run_conversation(ctx, site, "plan", "", msg).await?;
    site_ops::update_item(&ctx.site_dir(&site.id), &item.id, now(), |i| {
        i.status = "proposed".into();
        i.conv_id = conv_id;
        i.change_kind = "plan".into();
        i.verdict = "方案已写好，在会话里确认后让 AI 动手".into();
    })?;
    Ok(format!("「{}」已出方案", item.opp.title))
}

struct CodeBase {
    main: PathBuf,
    token: String,
    base_ref: String,
}

async fn code_base(ctx: &Ctx, site: &Site, job: &Job) -> Result<CodeBase, String> {
    let st = ctx.state();
    match site.source.as_str() {
        "github" => {
            let gh = st.conns.get(&site.gh_conn_id).map_err(|_| "站点的 GitHub 连接不存在".to_string())?;
            let token = crate::keychain::get_key(&gh.id)?;
            job.add_secret(&token);
            let main = crate::github::repo_dir(&gh.skill_dir, &site.repo)?;
            crate::github::ensure_checkout(&gh.skill_dir, &main.to_string_lossy(), &token, &gh.login, &gh.account_id).await?;
            crate::site_git::git_ok(&main, &["fetch", "--quiet", "origin", &site.default_branch], &token).await?;
            Ok(CodeBase { main, token, base_ref: format!("origin/{}", site.default_branch) })
        }
        _ => {
            let main = crate::site_git::code_dir(site, None)?;
            if !main.join(".git").exists() {
                return Err("站点文件夹不是 git 仓库：第 3、4 档需要用分支隔离改动，请先 git init 并提交一次".into());
            }
            // git init 了但一次都没提交（或默认分支名对不上）：分支没法从它切出来，先说清楚，别让 git 的原话冒到卡片上
            if crate::site_git::git_ok(&main, &["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", site.default_branch)], "").await.is_err() {
                let any = crate::site_git::git_ok(&main, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"], "").await.is_ok();
                return Err(if any {
                    format!("站点文件夹的 git 仓库里没有「{}」分支：在站点设置里把默认分支改成仓库实际用的分支名", site.default_branch)
                } else {
                    "站点文件夹是 git 仓库但还没有任何提交：第 3、4 档需要用分支隔离改动，请先提交一次".into()
                });
            }
            Ok(CodeBase { main, token: String::new(), base_ref: site.default_branch.clone() })
        }
    }
}

fn page_exists_in(out: &Path) -> impl Fn(&str) -> bool + '_ {
    move |href: &str| {
        // 浏览器里复制的网址带百分号编码（中文路径），磁盘上是原文
        let raw = href.split(['#', '?']).next().unwrap_or("");
        let decoded = percent_decode(raw);
        let p = decoded.trim_start_matches('/');
        if p.is_empty() {
            return out.join("index.html").exists();
        }
        let p = p.trim_end_matches('/');
        out.join(p).join("index.html").exists() || out.join(format!("{p}.html")).exists() || out.join(p).is_file()
    }
}

/// 构建产物里这个页面设了 noindex（不是给人看、也不让收录的页面）。
fn page_noindex(out: &Path, path: &str) -> bool {
    let decoded = percent_decode(path.split(['#', '?']).next().unwrap_or(""));
    let p = decoded.trim_matches('/');
    let file = if p.is_empty() { out.join("index.html") } else if out.join(p).join("index.html").is_file() { out.join(p).join("index.html") } else { out.join(format!("{p}.html")) };
    std::fs::read_to_string(file).is_ok_and(|h| crate::site_verify::meta_content(&h, "robots").to_ascii_lowercase().contains("noindex"))
}

/// 站点所在托管方案的规则（没托管返回 None）。
fn group_rules(ctx: &Ctx, site: &Site) -> Option<crate::managed::ManagedGroup> {
    crate::managed::load(&ctx.state().data_dir).group_of(&site.id).cloned()
}

/// 第 3、4 档：在独立工作树的分支上让 AI 改好并提交；Pilot 跑检查、按差异判定类型、推送开 PR；
/// 第 4 档且整份差异都在白名单内时自动合并上线。
async fn implement(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem, level: u8) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let cb = code_base(ctx, site, job).await?;
    let base_sha = crate::site_git::git_ok(&cb.main, &["rev-parse", &cb.base_ref], "").await?;
    let short: String = item.id.chars().take(6).collect();
    let branch = format!("pilot/ops-{}-{short}", crate::site_git::slugify(&path_of(&item.opp.page).replace('/', " "), 32));
    if !crate::site_git::safe_branch(&branch) {
        return Err("分支名不合法".into());
    }
    let wt = crate::sites::build_root(&site_dir).join("work").join(&item.id);
    crate::site_git::worktree_at(&cb.main, &wt, &base_sha, Some(&branch)).await?;
    crate::site_git::share_node_modules(&cb.main, &wt);
    crate::site_git::share_node_modules(&crate::sites::build_root(&site_dir).join("deploy"), &wt);
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.branch = branch.clone();
        i.base_sha = base_sha.clone();
        i.replay.clear();
        // 重做是新的一轮：上一版用掉的「修一次」机会（修构建、补链接、质检重写）重新给
        i.build_fixed = false;
        i.links_fixed = false;
        i.jev_rewritten = false;
    })?;
    let checks = if site.build.checks.is_empty() { String::new() } else { format!("和检查（{}）", site.build.checks.join("、")) };
    let mut plan_brief = group_rules(ctx, site).map(|g| g.brief()).unwrap_or_default();
    if !item.replay.is_empty() {
        plan_brief.push_str(&format!("【上一版】这条你之前改过，但主分支后来也改了同一处（多半是同时改好的另一条先上线了），那一版合不进去。现在的工作区已经是最新的主分支：保留主分支上已有的改动，在它的基础上按上一版的意图重做，不要把别人刚改的改回去，也不要重复加。上一版的改动：\n```diff\n{}\n```\n\n", item.replay));
    }
    // 计划里的任务可能一次涉及几十个页面（全站改标题）：分批改、分批上线，见 site_ops::next_batch
    if !item.opp.batch.is_empty() {
        plan_brief.push_str(&format!(
            "【分批任务 · 第 {} 批】这条任务分批做，这次只改下面这 {} 个页面，其余的 Pilot 会接着排，不要动，也不用写「还要改」：\n{}\n",
            item.opp.batch_no,
            item.opp.batch.len(),
            item.opp.batch.join("\n")
        ));
    } else if item.opp.kind == "plan" {
        plan_brief.push_str(&format!(
            "【页面很多时分批做】这条任务如果要改的页面超过 {n} 个（比如全站改标题）：这次只改最重要的 {n} 个，依据里点名的先改，其余的不要动，把它们的路径一个不漏地单独写一行，放在说明之后、最后两行之前：「还要改：/a/ /b/ /c/」（同一篇自动生成的繁体版这类不用列）。Pilot 这批上线后会按清单接着排。不超过 {n} 个就不用写这一行。\n",
            n = site_ops::BATCH_PAGES
        ));
    }
    if !site.ops.content_code.is_empty() {
        plan_brief.push_str(&format!("【页面写在代码里】这个站点的页面内容（标题、描述、文章）写在这些代码文件里，需要时可以直接改，按上面的代码改动规则来：{}。新加的代码不要读环境变量、执行命令、请求外部服务。\n\n", site.ops.content_code.join("、")));
    }
    if !item.retry_note.is_empty() {
        if item.auto_retried {
            plan_brief.push_str(&format!("【上次没做成】这条任务上次没跑完，原因和内容无关（{}），已经从头再来：工作区是干净的，按下面的要求做就行。\n", item.retry_note.lines().next().unwrap_or("")));
        } else {
            plan_brief.push_str(&format!("【上次的问题】这条改动上次上线后，线上验收没通过，已经回滚：{}。这次先找到原因再改；改完确认构建结果里有对应页面、网址和你写的一致。\n", item.retry_note));
        }
    }
    let msg = format!(
        "【运营 · 改动】{title}\n\n{ev}\n{plan_brief}今天是 {today}（发布日期、更新日期、核对日期都按这个写）。\n\
当前目录是 Pilot 为这个任务准备的独立工作区（分支 `{branch}`，基于默认分支 `{base}`）。请直接完成改动：\n\
读站点要省：只看这次要改的页面、它的模板，以及准备互相链接的两三篇，不要逐篇通读；需要查询词等数据用站点数据工具查。\n\
1. 找到这个页面对应的源文件，按上面的依据修改。改动方向：{guide}\n\
2. 改动小而集中，{scope}；不要改网址、依赖、构建和部署配置，也不要动推荐链接 / 邀请码。\n\
   {code_rule}\n\
3. 改完运行构建（`{build}`）{checks}，确认通过。\n\
4. 用 `git add` 和 `git commit` 提交到当前分支，提交说明一句话写清改了什么（不要写流量数字）；如果提交因为本机没配 git 身份而失败，保留改动即可，Pilot 会替你提交。\n\
{web}\n\
不要推送、不要开 PR、不要部署——这些由 Pilot 在审核规则通过后执行；除上面说的以外需要确认的操作在这里会被自动拒绝。\n\
如果查下来确实不该改（站点有意这样、问题已经不存在），不要为了交差硬改：不提交，回复的最后一行写「不用改：一句话原因」，不用写下面那两行。\n\
完成后另起一段，用两三句中文说明改了哪些文件、为什么这样改（{lang}）。然后在最后两行按这个格式写出这次涉及的公开页面（Pilot 上线后会逐个打开验收）：\n\
新页面：这次新增的页面完整网址，多个用空格分开（没有就写「无」）\n\
改动的页面：内容有变化的已有页面完整网址（新加的文字会出现在哪些页面，都列上）",
        title = item.opp.title,
        today = today(),
        // 只有 Claude 本地客户端能在无人值守时联网（搜网页、打开网页会放行）；API 模型和 Codex 拿不到网，别让它白试
        web = if models_for(site, &ctx.state().data_dir).map(|(p, _)| p.0 == "claude").unwrap_or(false) {
            "费率、步骤、规则这类事实先联网核实：可以搜网页、打开官网和帮助中心（无人值守时这两样会放行），写明来源和核对日期；核实不了的不写。"
        } else {
            "这次没法联网：费率、步骤、规则这类事实只能用站点里已有的内容和你确知的；拿不准的不写，在最后的说明里列出来让运营人员核实。"
        },
        scope = if item.also.is_empty() { "只处理这一个机会".to_string() } else { format!("只处理上面这 {} 条", item.also.len() + 1) },
        ev = evidence_all(item),
        base = site.default_branch,
        guide = guidance(&item.opp),
        lang = REPORT_IN_CHINESE,
        // 托管方案里写了代码改动的规则（见 plan_brief 的【代码改动】）；没托管就按保守的来
        code_rule = if group_rules(ctx, site).is_some() { "代码能不能改、改多少，按上面【代码改动】的规则。" } else { "能用站点现有的数据字段和组件做到，就只改内容和数据；确实要改代码，只做最小的改动。" },
        build = if site.build.build.is_empty() { "无需构建" } else { &site.build.build },
    );
    job.line(&format!("交给 AI：{}", item.opp.title));
    let (conv_id, conv) = run_conversation(ctx, site, "auto", &wt.to_string_lossy(), msg).await?;
    // 用户在 AI 改的时候取消了：改动留在工作树和分支上，不再往下检查、推送
    if job.canceled() {
        return Err("已取消".into());
    }
    let reply = conv.messages.iter().rev().find(|m| m.role == "assistant").map(|m| m.text.clone()).unwrap_or_default();
    let summary = final_summary(&reply);
    let (new_pages, pages) = crate::site_verify::reported_pages(&reply, &site.domain);
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.conv_id = conv_id.clone();
        i.model_used = format!("{} · {}", conv.brain, conv.model);
        i.ai_summary = summary.clone();
        i.pages = pages.clone();
        i.new_pages = new_pages.clone();
        if i.opp.kind == "plan" && i.opp.batch.is_empty() {
            i.opp.batch_rest = site_ops::batch_rest_from(&reply, &site.domain);
        }
    })?;
    // 收尾：没提交的改动由 Pilot 提交；完全没改动就算失败。
    if !crate::site_git::is_clean(&wt).await || crate::site_git::git(&wt, &["status", "--porcelain"], "").await.map(|o| !o.stdout.is_empty()).unwrap_or(false) {
        crate::site_git::git_ok(&wt, &["add", "-A", "--", ".", ":(exclude)node_modules", ":(exclude)uploads", ":(exclude).pilot-api-home"], "").await?;
        let msg = format!("ops: improve {}", path_of(&item.opp.page));
        let _ = crate::site_git::git_commit_ok(&wt, &["commit", "-qm", &msg]).await;
    }
    verify_and_ship(ctx, site, job, item, level, &cb, &wt, &base_sha, &branch).await
}

/// AI 改完之后由 Pilot 做的事：确认有提交 → 构建和检查 → 按规则判定 → 推送 / 开 PR → 自动上线或留给你确认。
/// 「重新检查」也走这里（改动已经在分支上，不再让 AI 改一遍）。
#[allow(clippy::too_many_arguments)]
/// 上线前有一项没过（构建 / 检查、孤立新页面、内容质检）：把原因交回 AI 在同一个分支上改一次并提交，
/// 不推送、不上线；调用方随后再走一遍检查和判定。
async fn fix_round(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem, wt: &Path, msg: String, label: &str, commit: &str) -> Result<(), String> {
    let site_dir = ctx.site_dir(&site.id);
    // 一条任务里最多再跑三轮 AI（修构建、重写、补链接）：每轮前都看额度，别一条就把本周的 token 花光
    if let Some(why) = over_tokens(&budget(site, &site_dir, &ctx.state().convos)) {
        return Err(format!("{label}前额度已用完：{why}"));
    }
    let msg = format!(
        "{msg}\n改完运行构建（`{build}`）确认通过，用 `git add` 和 `git commit` 提交到当前分支。不要推送、不要开 PR、不要部署。\n\
最后另起一段，用两三句中文说明这次改了什么（{lang}）。",
        lang = REPORT_IN_CHINESE,
        build = if site.build.build.is_empty() { "无需构建" } else { &site.build.build },
    );
    let (_, conv) = run_conversation(ctx, site, "auto", &wt.to_string_lossy(), msg).await?;
    if job.canceled() {
        return Err("已取消".into());
    }
    let summary: String = conv.messages.iter().rev().find(|m| m.role == "assistant").map(|m| final_summary(&m.text).chars().take(400).collect()).unwrap_or_default();
    let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| i.ai_summary = format!("{}\n\n（{label}：{summary}）", i.ai_summary));
    if crate::site_git::git(wt, &["status", "--porcelain"], "").await.map(|o| !o.stdout.is_empty()).unwrap_or(false) {
        crate::site_git::git_ok(wt, &["add", "-A", "--", ".", ":(exclude)node_modules", ":(exclude)uploads", ":(exclude).pilot-api-home"], "").await?;
        let _ = crate::site_git::git_commit_ok(wt, &["commit", "-qm", commit]).await;
    }
    Ok(())
}

async fn verify_and_ship(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem, level: u8, cb: &CodeBase, wt: &Path, base_sha: &str, branch: &str) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let (wt, base_sha, branch) = (wt.to_path_buf(), base_sha.to_string(), branch.to_string());
    let commits = crate::site_git::commits_between(&wt, &base_sha, "HEAD").await;
    if commits.is_empty() {
        // 体检类任务 AI 没改：拿工作区刚构建的产物重查一遍，清单里的页面都没问题了就是之前的改动已经修好了，算完成，不是失败
        if matches!(item.opp.kind.as_str(), "schema" | "img_audit") {
            let out = crate::site_make::out_dir(&site.app_dir(&wt), &site.build.out_dir);
            if out.join("index.html").is_file() {
                let domain = site.domain.clone();
                let kind = item.opp.kind.clone();
                let evidence = std::iter::once(item.opp.evidence.as_str()).chain(item.also.iter().map(|o| o.evidence.as_str())).collect::<Vec<_>>().join("\n");
                let fixed = tokio::task::spawn_blocking(move || !crate::page_audit::still_has(&crate::page_audit::audit(&out, &domain), &kind, &evidence)).await.unwrap_or(false);
                if fixed {
                    crate::site_git::remove_worktree(&cb.main, &wt).await;
                    let _ = crate::site_git::git(&cb.main, &["branch", "-D", &branch], "").await;
                    let note = "AI 检查后发现这些页面的问题已经修好了（之前上线的改动已经覆盖），这次不用改；Pilot 用构建产物重查确认过".to_string();
                    if let Some(d) = site_ops::update_item(&site_dir, &item.id, now(), |i| {
                        i.status = "done".into();
                        i.note = note.clone();
                    })? {
                        log_item(ctx, site, "fixed", &d, &note);
                    }
                    ctx.emit_queue(&site.id);
                    job.step("ship", "skipped", "问题已经修好了，不用改");
                    return Ok(note);
                }
            }
        }
        crate::site_git::remove_worktree(&cb.main, &wt).await;
        let _ = crate::site_git::git(&cb.main, &["branch", "-D", &branch], "").await;
        // AI 查完判断不该改：这是结论，不是失败，不用你再处理。修复任务的结论同时清掉那条改动的待核对（开这条任务就是为了它）
        let summary = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item.id).map(|i| i.ai_summary).unwrap_or_default();
        if let Some(why) = no_change_reason(&summary) {
            continue_batch(ctx, site, &item.id);
            let note = format!("AI 查完认为不用改：{why}");
            if let Some(d) = site_ops::update_item(&site_dir, &item.id, now(), |i| {
                i.status = "done".into();
                i.note = note.clone();
            })? {
                log_item(ctx, site, "fixed", &d, &note);
            }
            if let Some(parent) = item.opp.key.strip_prefix("followup:") {
                let _ = site_ops::update_item(&site_dir, parent, now(), |i| i.attention.clear());
            }
            ctx.emit_queue(&site.id);
            job.step("ship", "skipped", "AI 查完认为不用改");
            return Ok(note);
        }
        return Err("AI 没有做出任何改动（详见会话）".into());
    }
    // AI 跑构建时顺手提交进来的、构建脚本每次都会重写的数据文件（比如同步手续费）：还原成改动前的样子再往下走，
    // 不和内容改动绑在一个提交里上线（单独回滚这条内容时不会把费率数据一起退回去，也不和每天的数据更新撞车）
    let reverted = revert_generated(site, &wt, &base_sha).await;
    if !reverted.is_empty() {
        job.line(&format!("还原了构建时自动生成的数据文件，不算这次的改动：{}", reverted.join("、")));
    }
    let head_sha = crate::site_git::head_sha(&wt).await;
    // 先记下 AI 的提交：之后推送、开 PR 这些失败了，自动重试走「重新检查」接着用，不会把分支重置、整条重做
    let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| i.head_sha = head_sha.clone());
    if let Ok(out) = crate::site_git::git_ok(&wt, &["diff", "--name-only", &base_sha, &head_sha], "").await {
        let files: Vec<String> = out.lines().map(str::trim).filter(|l| !l.is_empty()).take(30).map(str::to_string).collect();
        let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| i.files = files.clone());
    }
    // 检查：构建 + 站点自带的检查命令
    if crate::site_git::unshare_node_modules_if_changed(&wt) {
        job.line("AI 改了依赖的锁文件，不用共享的 node_modules，按这份锁文件重新装");
    }
    let built = crate::site_deploy::build_and_check(site, &wt, None, job).await;
    let diff = crate::site_git::diff_zero_context(&wt, &base_sha, &head_sha).await?;
    let diff = if crate::managed::docs_are_published(&site.app_dir(&wt), &site.build.out_dir) { diff } else { crate::managed::without_internal_docs(&diff) };
    let snippets = crate::site_verify::snippets_from_diff(&diff);
    let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| i.snippets = snippets.clone());
    // 上线时用最新的条目（带上 AI 报告的页面和这次的文字）
    let item = &site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item.id).unwrap_or_else(|| item.clone());
    let hold = held(&site.id);
    match &built {
        Ok(_) => set_env_broken(&site_dir, None),
        // 先分清是谁的问题：改动前的代码在同样环境里也不过 → 站点自己的构建 / 检查坏了（配置不对、环境缺东西、
        // 第三方工具变了），不是 AI 改坏的。不交回 AI 修（白花额度），不算失败；改动留在分支上，站点标成构建有问题，
        // 托管先不派新任务，修好（改站点设置，或下次体检通过）后自动重新检查这条
        Err(e) if !job.canceled() => {
            if let Some(base_err) = base_failure(site, &site_dir, job, cb, &base_sha, &item.id).await {
                let note = format!("{}{base_err}", site_ops::ENV_BLOCKED);
                job.line(&format!("改动前的代码也过不了（{base_err}）：是站点自己的问题，不是这次改动的，不交给 AI 修；修好站点的构建后会自动接着检查这条"));
                set_env_broken(&site_dir, Some(base_err));
                site_ops::update_item(&site_dir, &item.id, now(), |i| {
                    i.status = "failed".into();
                    i.head_sha = head_sha.clone();
                    i.verdict.clear();
                    i.note = note.clone();
                })?;
                ctx.emit_queue(&site.id);
                return Err(note);
            }
            job.line(&format!("改动前的代码能过，是这次改动的问题：{}", e.lines().next().unwrap_or("")));
            job.step("check", "failed", "改动前的代码能过，是这次改动引起的");
        }
        Err(_) => {}
    }
    // 全托管：构建或检查没过，先把报错交回 AI 修一次（多半是改出来的类型错误、测试里写死的旧数字），修完再走一遍；
    // 还是不过才留给你
    if let Err(e) = &built {
        if level >= 5 && !hold && !item.build_fixed && !job.canceled() {
            let first = e.lines().next().unwrap_or("").to_string();
            let tail = job.tail(80).join("\n");
            job.line(&format!("{first}。把报错交回 AI 修一次"));
            site_ops::update_item(&site_dir, &item.id, now(), |i| i.build_fixed = true)?;
            let msg = format!(
                "你刚才在当前分支上为「{title}」做的改动，Pilot 上线前跑构建和检查没有通过：{first}\n\n\
构建和检查最后的输出：\n```\n{tail}\n```\n\n\
请先找到原因，只修让构建和检查通过所需的地方：\n\
- 是这次改动引起的（类型错误、语法错误、测试里写死的旧标题 / 旧数字、链接到不存在的页面），就改这次的改动或同步改对应的测试；\n\
- 不要为了通过删掉或放宽检查、不要跳过测试、不要改依赖、构建和部署配置；\n\
- 如果是本机环境问题（网络、代理、缺命令），和这次改动无关，就不要改文件，在说明里写清楚。\n",
                title = item.opp.title,
            );
            fix_round(ctx, site, job, item, &wt, msg, "构建没过后修复", "ops: fix checks").await?;
            return Box::pin(verify_and_ship(ctx, site, job, item, level, cb, &wt, &base_sha, &branch)).await;
        }
    }
    // 降档记的是改动类型（meta / content…），白名单是 classify 的细分（title / meta_description…）：按对应关系过滤
    let whitelist: Vec<String> = site.ops.whitelist.iter().filter(|w| !site.ops.downgraded.iter().any(|d| kinds_of_change(d).contains(&w.as_str()))).cloned().collect();
    let out_dir = site.app_dir(&wt).join(&site.build.out_dir);
    // 新页面不只靠 AI 在回复里自报：这次构建里有、上次上线的构建里没有的页面也算新页面，不然 AI 漏报一行，
    // 孤立页检查、上线后的 sitemap / 站内链接检查就整个跳过了
    let item = &derive_new_pages(ctx, site, item, &out_dir, built.is_ok(), &diff, job);
    let item = &drop_unbuilt_pages(ctx, site, item, &out_dir, built.is_ok(), job);
    let cls = site_ops::classify(&diff, &whitelist, &page_exists_in(&out_dir));
    let change_kind = if cls.kinds.is_empty() { "mixed".to_string() } else { cls.kinds.join(",") };
    let checks_ok = built.is_ok();
    // 全托管的判定比第 4 档白名单宽：页面内容的改动都可以自动上线，但依赖 / 配置 / 代码、推荐与邀请相关的内容、
    // 超大改动永远不自动；新页面按托管设置决定。第 1–4 档仍按白名单。
    let managed = crate::managed::load(&ctx.state().data_dir).rules_for(&site.id);
    // 新加的推荐链接 / 码 / 字段里，改动前的站点本来就有的（换个位置再引用）不算新增
    let mut known = Vec::new();
    if level >= 5 {
        for a in crate::managed::added_money_atoms(&diff).into_iter().take(40) {
            if crate::site_git::git(&wt, &["grep", "-q", "-F", "-e", &a, &base_sha, "--"], "").await.is_ok_and(|o| o.ok) {
                known.push(a);
            }
        }
    }
    // 构建脚本每次都会重写的数据文件（比如同步手续费）：AI 跑构建时顺手提交进来的，不算它的改动
    let generated = crate::managed::build_outputs(&site.app_dir(&wt));
    let mut rules = managed.rules(&site.ops.content_code);
    rules.known = &known;
    rules.generated = &generated;
    // 构建、检查会跑到的脚本：scripts/ 里别的辅助脚本只改数据时按内容算
    let commands: Vec<String> = std::iter::once(site.build.build.clone()).chain(site.build.checks.iter().cloned()).collect();
    let scripts = crate::managed::build_scripts(&site.app_dir(&wt), &commands);
    rules.build_scripts = scripts.as_deref();
    let deps = crate::managed::declared_deps(&site.app_dir(&wt));
    rules.deps = &deps;
    let blocks = if level >= 5 { crate::managed::verdict(&diff, &rules) } else { Vec::new() };
    // 能在卡片上点「这类以后不拦」的拦截类型
    let mut kinds: Vec<String> = blocks.iter().filter(|b| !b.kind.is_empty()).map(|b| b.kind.to_string()).collect();
    let mut reasons: Vec<String> = if level >= 5 { blocks.into_iter().map(|b| b.text).collect() } else { cls.reasons.clone() };
    // 新页面要有别的页面链到它，不然上线就是孤立页（搜索引擎很难发现）：AI 漏加了就不自动上线
    if built.is_ok() && !item.new_pages.is_empty() {
        let inbound = crate::site_make::inbound_links(&out_dir);
        let count = |p: &str| inbound.iter().find(|(k, _)| crate::site_make::norm_path(k) == crate::site_make::norm_path(p)).map(|(_, n)| *n);
        let lonely: Vec<String> = item.new_pages.iter().map(|u| path_of(u)).filter(|p| count(p) == Some(0)).collect();
        if !lonely.is_empty() && level >= 5 && !hold && !item.links_fixed && !job.canceled() {
            // 全托管：交回 AI 从相关的已有页面补链接，补完再走一遍
            job.line(&format!("新页面 {} 没有站内链接指向它，交回 AI 补链接", lonely.join("、")));
            site_ops::update_item(&site_dir, &item.id, now(), |i| i.links_fixed = true)?;
            let msg = format!(
                "你刚才在当前分支上为「{title}」新写的页面 {pages} 还没有任何站内页面链接到它，上线后会成孤立页，搜索引擎很难发现。\n\
请在 2–4 个主题最相关的已有页面里，在讲到相关内容的地方自然地加上指向它的链接（链接文字写读者会搜的说法），\n\
网站有目录、导航或相关文章列表的话也把它加进去。只加链接，不要改别的内容，不要改依赖、构建和部署配置。\n",
                title = item.opp.title,
                pages = lonely.join("、"),
            );
            fix_round(ctx, site, job, item, &wt, msg, "补站内链接", "ops: link new page").await?;
            return Box::pin(verify_and_ship(ctx, site, job, item, level, cb, &wt, &base_sha, &branch)).await;
        }
        if !lonely.is_empty() && level >= 5 && rules.lets("orphan") {
            job.line(&format!("新页面 {} 还是没有站内链接指向它，方案的拦截等级放行这类，照样上线", lonely.join("、")));
        } else if !lonely.is_empty() {
            reasons.push(format!("新页面 {} 没有任何站内链接指向它（会成孤立页）{}，留给你确认", lonely.join("、"), if item.links_fixed { "，AI 补过一次还是没有" } else { "" }));
            kinds.push("orphan".into());
        }
    }
    let mut verdict = if let Err(e) = &built {
        format!("检查没通过：{}", e.lines().next().unwrap_or(""))
    } else if reasons.is_empty() {
        "auto".to_string()
    } else {
        reasons.join("；")
    };
    if level >= 5 && verdict == "auto" && !managed.allows(&item.opp.change) {
        verdict = format!("托管方案「{}」不允许「{}」这类改动，留给你确认", managed.name, crate::managed::action_label(&item.opp.change));
    }
    // 任务标签说是改内容，diff 里却新建了页面：按实际改动核对方案允许的动作
    if level >= 5 && verdict == "auto" && !managed.allows("new_content") {
        let fresh = crate::managed::new_page_files(&diff);
        if !fresh.is_empty() {
            verdict = format!("托管方案「{}」不允许写新页面，这次却新建了 {}，留给你确认", managed.name, fresh.join("、"));
        }
    }
    // 站点被标记为危险网站（Google 安全浏览、Cloudflare、Quad9）：先查清原因，期间自动上线暂停
    let unsafe_by = crate::safety::load(&site_dir).summary();
    if verdict == "auto" && !unsafe_by.is_empty() {
        verdict = format!("站点现在被 {unsafe_by} 标记为危险网站，自动上线先暂停，留给你确认（标记解除后恢复）");
    }
    // 判断模型（Jev）内容质检（托管方案里的规则）：规则都过了、马上要自动上线时，再看一眼新页面 / 大改的页面内容本身。
    // 方案没开、没接 TypeSafe 或调用失败都不影响原来的判定（只会加严，不会放宽）。流程上单独一步，跳过也写明为什么。
    // 「执行但不上线」也照常质检，只记分数给你审核时参考，不交回 AI 重写。
    let mut rewrite: Option<(String, Vec<String>)> = None;
    if !checks_ok {
        job.step("quality", "skipped", "检查没通过，不用质检");
    } else if level < 5 {
        job.step("quality", "skipped", "不是全自动托管，改好留给你看");
    } else if !managed.jev_gate {
        job.step("quality", "skipped", "托管方案没开上线前内容质检");
    } else if verdict != "auto" {
        job.step("quality", "skipped", "已经有别的原因要留给你确认");
    } else if let Some(jev) = crate::typesafe::client() {
        let files = crate::managed::content_page_files(&diff);
        if files.is_empty() {
            job.step("quality", "skipped", "只改了标题、描述、数据或代码，没有成段的页面正文");
        } else {
            job.step("quality", "running", &format!("判断模型（Jev）看 {} 个页面", files.len()));
            job.line("判断模型（Jev）内容质检…");
            let positioning = crate::site_stage::load_positioning(&site_dir).text;
            let target = if item.opp.target.trim().is_empty() { item.opp.queries.first().map(|q| q.key.clone()).unwrap_or_default() } else { item.opp.target.clone() };
            // 已有页面改动前的样子：质检只拦这次改出来或变差的问题，老页面原来就有的毛病不算在这次头上
            let mut before = std::collections::HashMap::new();
            for (f, new_file) in &files {
                if !new_file {
                    if let Ok(old) = crate::site_git::git_ok(&wt, &["show", &format!("{base_sha}:{f}")], "").await {
                        before.insert(f.clone(), old);
                    }
                }
            }
            // 差异里的路径是相对仓库根目录的（站点在子目录里时不能拼在 app_dir 后面）
            match crate::jev_checks::check_pages(&jev, site, &positioning, &target, &wt, &files, &before).await {
                Ok(quality) => {
                    let why = crate::jev_checks::gate_verdict(&quality);
                    let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| i.quality = quality.clone());
                    let inherited: Vec<String> = quality.iter().flat_map(|q| q.inherited.iter().map(move |r| format!("{}：{r}", q.page))).collect();
                    if why.is_empty() {
                        let note = if inherited.is_empty() { String::new() } else { format!("（改动前就有、这次没变差的不算：{}）", inherited.join("；")) };
                        job.step("quality", "ok", &format!("{} 个页面都通过{note}", files.len()));
                        job.line(&format!("内容质检通过{note}"));
                    } else if hold {
                        job.step("quality", "failed", &format!("没过：{why}（这次不上线，只记分数）"));
                        job.line(&format!("内容质检没过：{why}。这次是「执行但不上线」，不交回重写，分数记在条目上"));
                    } else if managed.jev_rewrite && !item.jev_rewritten {
                        // 先不留给你：把原因交回 AI 重写一次，再走一遍检查和质检
                        job.step("quality", "failed", &format!("没过：{why}，交回 AI 重写一次"));
                        rewrite = Some((why, files.iter().map(|(f, _)| f.clone()).collect()));
                    } else if rules.lets("jev") {
                        job.step("quality", "failed", &format!("没过：{why}（方案的拦截等级放行这类，照样上线，分数记在条目上）"));
                        job.line(&format!("内容质检没过：{why}。方案放行质检没过的改动，照样上线"));
                    } else {
                        job.step("quality", "failed", &format!("没过：{why}"));
                        verdict = format!("内容质检没过（判断模型 Jev）{}：{why}，留给你确认", if item.jev_rewritten { "，AI 按原因重写过一次还是没过" } else { "" });
                        kinds.push("jev".into());
                    }
                }
                Err(e) => {
                    job.step("quality", "skipped", &format!("没做成（{e}），按原来的规则判定"));
                    job.line(&format!("内容质检没做成（{e}），按原来的规则判定"));
                }
            }
        }
    } else {
        job.step("quality", "skipped", "没接判断模型（设置 → 判断模型）");
    }
    if let Some((why, files)) = rewrite {
        job.line(&format!("内容质检没过：{why}。交回 AI 按原因重写一次"));
        site_ops::update_item(&site_dir, &item.id, now(), |i| i.jev_rewritten = true)?;
        let msg = format!(
            "你刚才在当前分支上为「{title}」做的改动，Pilot 上线前用判断模型做内容质检没有通过：\n{why}\n\n\
请只针对这些问题修改这些文件：{files}\n\
- 把目标搜索问题正面讲清楚，补具体的步骤、数字、例子，删掉空泛的套话；\n\
- 数字和测试结果要写清来源（或删掉拿不准的），不要编；\n\
- 删掉正文里写给编辑或 AI 的说明文字；\n\
- 不要扩大改动范围，不要改网址、依赖、构建和部署配置。\n",
            title = item.opp.title,
            files = files.join("、"),
        );
        fix_round(ctx, site, job, item, &wt, msg, "质检没过后重写", &format!("ops: revise {} after quality check", path_of(&item.opp.page))).await?;
        // 再走一遍：构建、检查、规则、质检（这次 jev_rewritten 已经记上，不会再重写）
        return Box::pin(verify_and_ship(ctx, site, job, item, level, cb, &wt, &base_sha, &branch)).await;
    }
    // 推送 + 开 PR（GitHub）；本地来源留在本地分支。检查没通过的不推送。
    let mut pr = crate::site_github::Pr::default();
    if checks_ok && site.source == "github" {
        crate::site_git::git_ok(&wt, &["push", "--force", "-u", "origin", &branch], &cb.token).await?;
        let body = format!(
            "由 Cligc Pilot 运营任务生成。\n\n- 页面：{}\n- 改动类型：{}\n- 依据：Search Console 数据（明细只保存在 Pilot 本地）\n- Pilot 已在此分支上通过构建{}。\n",
            path_of(&item.opp.page),
            crate::site_ops::kind_label(&item.opp.change),
            if site.build.checks.is_empty() { String::new() } else { "与检查".into() }
        );
        pr = crate::site_github::create_pr(&cb.token, &site.repo, &branch, &site.default_branch, &format!("ops: improve {}", path_of(&item.opp.page)), &body).await?;
    }
    let status = if checks_ok { "proposed" } else { "failed" };
    // 「执行但不上线」里本来可以自动上线的：写清楚是你选的不上线，不是规则拦的
    if hold && verdict == "auto" {
        verdict = "按方案规则可以自动上线；这次选了「执行但不上线」，等你确认".into();
    }
    kinds.sort();
    kinds.dedup();
    if verdict == "auto" || !checks_ok || hold {
        kinds.clear();
    }
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.status = status.into();
        i.head_sha = head_sha.clone();
        i.change_kind = change_kind.clone();
        i.verdict = verdict.clone();
        i.block_kinds = kinds.clone();
        i.pr_number = pr.number;
        i.pr_url = pr.url.clone();
        i.note = if checks_ok { String::new() } else { "构建或检查没通过，改动留在分支上，可以在会话里让 AI 继续修。".into() };
        i.rules_v = crate::managed::RULES_V;
    })?;
    ctx.emit_queue(&site.id);
    if !checks_ok {
        if level >= 5 {
            log_item(ctx, site, "failed", item, &verdict);
        }
        return Err(verdict);
    }
    if level >= 5 && verdict != "auto" {
        log_item(ctx, site, "proposed", item, &verdict);
    }
    if !(level >= 4 && verdict == "auto") || hold {
        if !by_hand(&site.id) {
            crate::push::proposal(&ctx.app, site, &item.id);
        }
        let why = if hold && verdict == "auto" {
            "检查都通过（按方案规则本来可以自动上线）；这次选了「执行但不上线」，留给你确认".to_string()
        } else if verdict == "auto" {
            "检查通过，按方案留给你确认后上线".to_string()
        } else {
            format!("检查通过，留给你确认：{verdict}")
        };
        job.step("ship", "skipped", &why);
        job.line(&why);
    }
    // 第 4 档：白名单内自动合并上线；第 5 档：判定通过即自动上线，上线 1 小时后再检查一次页面。
    if level >= 4 && verdict == "auto" && !hold {
        job.step("ship", "running", if level >= 5 { "全托管，自动合并上线" } else { "白名单内改动，自动合并上线" });
        match accept(ctx, site, job, &item.id, true).await {
            Ok(m) => {
                // 真的上线了才记「已上线」、排 1 小时后的复查：没配部署的站点只是合并；验收失败后 AI 重做成功的，里面那一层已经记过
                let fresh = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item.id);
                if level >= 5 && fresh.as_ref().is_some_and(|i| i.status == "deployed" && i.retries == 0) {
                    site_ops::update_item(&site_dir, &item.id, now(), |i| i.watch_due = now() + 3600)?;
                    log_item(ctx, site, "shipped", item, &m);
                }
                return Ok(m);
            }
            Err(e) => {
                // 线上验收没通过的已经回滚并写好状态了；已经合并的留在「已合并」，可以「重新上线」
                verdict = format!("自动上线失败：{e}");
                site_ops::update_item(&site_dir, &item.id, now(), |i| {
                    if i.status == "proposed" || i.status == "working" {
                        i.status = "proposed".into();
                        i.verdict = verdict.clone();
                    }
                })?;
                return Err(verdict);
            }
        }
    }
    Ok(if site.source == "github" { format!("「{}」已开 PR #{}", item.opp.title, pr.number) } else { format!("「{}」改好在本地分支 {branch}", item.opp.title) })
}

// ---------------------------------------------------------------------------
// 确认 / 驳回 / 回滚
// ---------------------------------------------------------------------------

/// 上线时页面近 28 天的表现快照（效果复盘的「之前」）。
/// 目标查询词在这个页面上的近 28 天表现（Search Console 查询词 × 页面）。
fn query_snapshot(site_dir: &Path, query: &str, page: &str) -> Option<crate::site_data::Row> {
    if query.trim().is_empty() || page.is_empty() {
        return None;
    }
    let rows: Vec<crate::site_data::Row> = crate::site_data::read_json(&crate::site_data::cache_dir(site_dir), "gsc_query_page.json");
    rows.into_iter().find(|r| r.key.eq_ignore_ascii_case(query.trim()) && r.key2 == page)
}

/// 全站近 28 天的点击和展现（复盘时用来扣掉整体变化）。
fn site_snapshot(site_dir: &Path) -> crate::site_data::Row {
    let s = crate::site_data::summary(site_dir);
    crate::site_data::Row { key: "site".into(), clicks: s.gsc_clicks, impressions: s.gsc_impressions, position: s.gsc_position, ..Default::default() }
}

fn page_snapshot(site_dir: &Path, page: &str) -> Option<crate::site_data::Row> {
    let rows: Vec<crate::site_data::Row> = crate::site_data::read_json(&crate::site_data::cache_dir(site_dir), "gsc_pages.json");
    rows.into_iter().find(|r| r.key == page)
}

/// 合并后让改动上线：直接上传模式由 Pilot 构建上传；Pages 绑定 GitHub 时等 Cloudflare 构建完成。
/// 上线的结果：地址、线上验收的结果、要核对的问题。
#[derive(Default, Clone)]
struct Shipped {
    url: String,
    checks: Vec<crate::site_verify::Check>,
    attention: Vec<String>,
}

/// 一条改动上线后要验收什么。回滚时只看首页和原来就有的页面（新页面回滚后本来就不该在了）。
fn spec_for(site: &Site, item: &QueueItem, rollback: bool) -> crate::site_verify::Spec {
    let mut spec = crate::site_verify::Spec { domain: site.domain.clone(), ..Default::default() };
    for o in std::iter::once(&item.opp).chain(&item.also) {
        let page = o.page.trim();
        if page.starts_with("https://") && !item.new_pages.iter().any(|n| n == page) && !spec.pages.iter().any(|p| p == page) {
            spec.pages.push(page.to_string());
            spec.primary.push(page.to_string());
        }
    }
    if !rollback {
        for p in &item.pages {
            if !spec.pages.contains(p) {
                spec.pages.push(p.clone());
            }
        }
        spec.new_pages = item.new_pages.clone();
        spec.snippets = item.snippets.clone();
    }
    spec
}

/// 合并后让改动上线并做线上验收：直接上传模式由 Pilot 构建上传；Pages 绑定 GitHub 时等 Cloudflare 构建完成。
/// 验收严重失败返回以 `site_verify::HARD_PREFIX` 开头的错误。
/// 上线前页面原有问题的记录（路径 → （记下的时间，问题））：上线时验收和之后的复查都拿它对比，只报新出现的。
fn before_path(site_dir: &Path) -> PathBuf {
    site_dir.join("verify_before.json")
}

fn load_before(site_dir: &Path) -> HashMap<String, Vec<String>> {
    let raw: HashMap<String, (u64, Vec<String>)> = std::fs::read(before_path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    raw.into_iter().map(|(k, (_, v))| (k, v)).collect()
}

/// 这次拍到的页面覆盖旧的记录；超过 30 天的清掉（复查最晚 24 小时，复盘不看这个）
fn save_before(site_dir: &Path, snap: &HashMap<String, Vec<String>>, pages: &[String]) {
    let mut raw: HashMap<String, (u64, Vec<String>)> = std::fs::read(before_path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let t = now();
    raw.retain(|_, (at, _)| *at + 30 * DAY > t);
    for p in pages {
        match snap.get(p) {
            Some(v) => raw.insert(p.clone(), (t, v.clone())),
            None => raw.remove(p),
        };
    }
    if let Ok(b) = serde_json::to_vec(&raw) {
        let _ = std::fs::write(before_path(site_dir), b);
    }
}

/// 推送后等多久 Cloudflare 还没有这次提交的构建，就主动让它构建（平时几十秒内就会出现）。
const PAGES_NUDGE_AFTER: u64 = 3 * 60;

/// 这次提交在生产环境的构建记录。
fn commit_deployment<'a>(list: &'a [crate::cf::PagesDeployment], commit: &str) -> Option<&'a crate::cf::PagesDeployment> {
    list.iter().find(|d| d.environment == "production" && !commit.is_empty() && d.commit.starts_with(&commit[..7.min(commit.len())]))
}

/// 线上现在跑的那次构建（列表从新到旧，最新一次成功的生产构建）。
fn latest_live(list: &[crate::cf::PagesDeployment]) -> Option<&crate::cf::PagesDeployment> {
    list.iter().find(|d| d.environment == "production" && d.status == "success" && d.stage == "deploy" && !d.commit.is_empty())
}

async fn ship(ctx: &Ctx, site: &Site, job: &Job, spec: crate::site_verify::Spec, commit: &str, trigger: &str) -> Result<Shipped, String> {
    let r = ship_inner(ctx, site, job, spec, commit, trigger).await;
    if let Err(e) = &r {
        if let Some(why) = e.strip_prefix(crate::site_verify::DOMAIN_TAKEN) {
            pause_for_domain(ctx, site, why);
        }
    }
    r
}

/// 域名被别的东西占着：托管停下等人处理（再改也验不了、上不了线）。已合并的改动已经在项目里，域名处理好就是线上的。
fn pause_for_domain(ctx: &Ctx, site: &Site, why: &str) {
    if group_rules(ctx, site).is_some_and(|g| g.active_for(&site.id)) {
        pause_site(ctx, site, &format!("{}{why}", crate::site_verify::DOMAIN_TAKEN));
        ctx.notify(&format!("{} · 域名没指向这个站点，已暂停托管", site.name), why);
    }
}

async fn ship_inner(ctx: &Ctx, site: &Site, job: &Job, spec: crate::site_verify::Spec, commit: &str, trigger: &str) -> Result<Shipped, String> {
    // 上线前先把要验收的已有页面查一遍：本来就有的问题（旧文 noindex、sitemap 没有标题…）不算这次改动的
    let mut spec = spec;
    if trigger != "rollback" {
        let snap = crate::site_verify::snapshot(&spec).await;
        let pages: Vec<String> = spec.primary.iter().chain(&spec.pages).map(|u| path_of(u)).collect();
        save_before(&ctx.site_dir(&site.id), &snap, &pages);
        spec.before = snap;
        spec.only_new = true;
    }
    let st = ctx.state();
    let site_dir = ctx.site_dir(&site.id);
    match site.deploy.mode.as_str() {
        "direct" | "workers" | "vps" => {
            let rec = crate::site_deploy::deploy_with(site, &st.conns, &site_dir, job, trigger, Some(spec)).await?;
            // 上线的这份产物重新做一次页面体检：修好的结构化数据 / 图片问题立刻从报告里消失，排着的同类任务动手前就知道不用改了
            if let Ok(out) = crate::syndicate::built_out(&st, site) {
                if crate::page_audit::refresh(site, &site_dir, out).await > 0 {
                    ctx.emit_queue(&site.id);
                }
            }
            Ok(Shipped { url: rec.url, checks: rec.checks, attention: rec.attention })
        }
        "pages_git" => {
            // 本地文件夹：Cloudflare 从 GitHub 拉代码构建，先把这次的提交推到 GitHub 的生产分支
            if site.source != "github" && !commit.is_empty() {
                let main = crate::site_git::code_dir(site, None)?;
                let branch = if site.deploy.branch.is_empty() { site.default_branch.clone() } else { site.deploy.branch.clone() };
                job.step("upload", "running", &format!("推送到 GitHub 的 {branch}"));
                job.line(&format!("$ git push origin {}:{branch}", commit.get(..7).unwrap_or(commit)));
                crate::site_git::git_ok(&main, &["push", "origin", &format!("{commit}:refs/heads/{branch}")], "").await.map_err(|e| {
                    job.step("upload", "failed", "");
                    format!("推送到 GitHub 失败（Cloudflare 要从 GitHub 拉代码构建）：{e}")
                })?;
            }
            job.step("upload", "running", "等待 Cloudflare 构建");
            let cf = st.conns.get(&site.deploy.cf_conn_id)?;
            let token = crate::keychain::get_key(&cf.id)?;
            let started = now();
            let deadline = started + 15 * 60;
            let (mut nudged, mut nudge_err, mut looked) = (false, String::new(), std::collections::HashSet::new());
            loop {
                if job.canceled() {
                    return Err("已取消".into());
                }
                if let Ok(list) = crate::cf::list_deployments(&token, &cf.account_id, &site.deploy.project).await {
                    if let Some(d) = commit_deployment(&list, commit) {
                        match d.status.as_str() {
                            "success" if d.stage == "deploy" => {
                                job.step("upload", "ok", &d.url);
                                break;
                            }
                            "failure" => {
                                job.step("upload", "failed", "");
                                return Err(format!("Cloudflare 构建失败（{}）", d.stage));
                            }
                            _ => {}
                        }
                    } else if !commit.is_empty() {
                        // 线上已经是更新的提交，这次的改动就在里面（比如之后又合并了一条）：不用再等它自己的构建
                        if let Some(d) = latest_live(&list).filter(|d| looked.insert(d.commit.clone())) {
                            if let Ok(main) = crate::site_git::code_dir(site, None) {
                                let _ = crate::site_git::git(&main, &["fetch", "-q", "origin"], "").await;
                                if crate::site_git::git(&main, &["merge-base", "--is-ancestor", commit, &d.commit], "").await.is_ok_and(|o| o.ok) {
                                    job.step("upload", "ok", &d.url);
                                    break;
                                }
                            }
                        }
                        // 推送的通知偶尔到不了 Cloudflare，再等也不会开始构建（重新上线时提交没变，推送也不会再通知一次）：主动让它构建
                        if !nudged && now().saturating_sub(started) >= PAGES_NUDGE_AFTER {
                            nudged = true;
                            job.line("Cloudflare 一直没开始构建这次的提交，主动让它构建一次");
                            if let Err(e) = crate::cf::start_deployment(&token, &cf.account_id, &site.deploy.project).await {
                                job.line(&format!("没能让 Cloudflare 开始构建：{e}"));
                                nudge_err = e;
                            }
                        }
                    }
                }
                if now() > deadline {
                    return Err(if nudge_err.is_empty() {
                        "15 分钟内没有等到 Cloudflare 构建完成，请在部署页查看".into()
                    } else {
                        format!("Cloudflare 一直没开始构建，也没能主动让它开始：{nudge_err}")
                    });
                }
                tokio::time::sleep(Duration::from_secs(20)).await;
            }
            let rep = crate::site_verify::run(&spec, job, true).await;
            if let Some(why) = crate::launch_cmds::domain_taken(&st.conns, site, &rep).await {
                job.step("verify", "failed", &why);
                return Err(format!("{}{why}", crate::site_verify::DOMAIN_TAKEN));
            }
            let shipped = Shipped { url: String::new(), checks: rep.checks.clone(), attention: rep.soft.clone() };
            if !rep.hard.is_empty() {
                job.step("verify", "failed", &rep.hard.join("；"));
                return Err(format!("{}{}", crate::site_verify::HARD_PREFIX, rep.hard.join("；")));
            }
            job.step("verify", if rep.checks.is_empty() { "skipped" } else { "ok" }, &if rep.soft.is_empty() { String::new() } else { format!("网站正常，{} 项待核对", rep.soft.len()) });
            if !site.data.indexnow_key.is_empty() && !site.domain.is_empty() && !rep.passed.is_empty() {
                crate::indexnow::submit_step(site, &site_dir, &rep.passed, job).await;
            } else {
                job.step("indexnow", "skipped", "");
            }
            Ok(shipped)
        }
        _ => Ok(Shipped::default()),
    }
}

/// 上线成功后记下验收结果，安排 1 小时后复查；有一般问题就标成待核对（全自动方案顺手开一条后续任务）。
fn record_shipped(ctx: &Ctx, site: &Site, item_id: &str, sh: &Shipped) -> Result<(), String> {
    let site_dir = ctx.site_dir(&site.id);
    let attention = sh.attention.join("；");
    let item = site_ops::update_item(&site_dir, item_id, now(), |i| {
        i.status = "deployed".into();
        i.deployed_at = now();
        i.note.clear();
        i.checks = sh.checks.clone();
        i.attention = attention.clone();
        i.watch_due = now() + 3600;
        i.watch_stage = 0;
        i.watched.clear();
    })?
    .ok_or("这条改动不存在")?;
    continue_batch(ctx, site, item_id);
    // 速度优化上线了：1 小时后重新测速，排着的其它速度任务等新结果出来再决定要不要做
    if item.opp.kind == "slow" || item.also.iter().any(|o| o.kind == "slow") {
        crate::speed::recheck_soon(&site_dir);
    }
    if !attention.is_empty() {
        // 修复任务等 1 小时后的复查还在才开（缓存、CDN 过一会儿多半就好了，见 watch）
        log_item(ctx, site, "attention", &item, &format!("上线了，网站正常；有 {} 项待核对：{attention}", sh.attention.len()));
    }
    ctx.emit_queue(&site.id);
    Ok(())
}

/// 分批任务这一批结束了：把下一批排进队列，按托管节奏接着做。
fn continue_batch(ctx: &Ctx, site: &Site, item_id: &str) {
    let site_dir = ctx.site_dir(&site.id);
    let Some(item) = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item_id) else { return };
    let Some(next) = site_ops::next_batch(&item.opp, &site.domain) else { return };
    let title = next.title.clone();
    if site_ops::enqueue(&site_dir, &[next], now(), COOLDOWN).map(|a| !a.is_empty()).unwrap_or(false) {
        log_item(ctx, site, "planned", &item, &format!("分批任务排上了下一批：{title}"));
    }
}

/// 这次构建产物里有、上次上线的构建里没有的 HTML 页面并进 item.new_pages（AI 自报的照旧保留）。最多补 10 个。
fn derive_new_pages(ctx: &Ctx, site: &Site, item: &QueueItem, out_dir: &Path, built: bool, diff: &str, job: &Job) -> QueueItem {
    let mut item = item.clone();
    // 只认这次 diff 里新建的页面文件对应的页面：上次上线的构建可能比 base 旧（或别的条目刚合并），
    // 不然会把别人的页面算到这条头上
    let stems: Vec<String> = crate::managed::new_page_files(diff).iter().map(|f| page_stem(f)).collect();
    if !built || site.domain.is_empty() || stems.is_empty() {
        return item;
    }
    let Ok(prev) = crate::syndicate::built_out(&ctx.state(), site) else { return item };
    if prev == out_dir || !prev.join("index.html").is_file() || !out_dir.join("index.html").is_file() {
        return item;
    }
    let old: HashSet<String> = crate::site_make::real_pages(&prev).iter().map(|(f, _)| crate::site_make::page_url(&prev, f)).collect();
    // 设了 noindex 的不算（比如给分享卡片截图用的 /og-card/<slug>/ 内部页面，和新文章同名，但不是给人看的页面）
    let fresh: Vec<String> = crate::site_make::real_pages(out_dir)
        .iter()
        .filter(|(_, html)| !crate::site_verify::meta_content(html, "robots").to_ascii_lowercase().contains("noindex"))
        .map(|(f, _)| crate::site_make::page_url(out_dir, f))
        .filter(|p| !old.contains(p) && stems.contains(&page_stem(p)))
        .map(|p| format!("https://{}{p}", site.domain))
        .filter(|u| !item.new_pages.iter().any(|n| crate::site_verify::same_page(n, u)))
        .take(10)
        .collect();
    if fresh.is_empty() {
        return item;
    }
    job.line(&format!("构建产物里多出 {} 个页面，按新页面验收：{}", fresh.len(), fresh.iter().map(|u| path_of(u)).collect::<Vec<_>>().join("、")));
    item.new_pages.extend(fresh);
    let pages = item.new_pages.clone();
    let _ = site_ops::update_item(&ctx.site_dir(&site.id), &item.id, now(), |i| i.new_pages = pages.clone());
    item
}

/// AI 自报的「改动的页面 / 新页面」里，构建产物里根本没有的（写错路径、编的）去掉：拿去验收只会 404、把一条好改动回滚
fn drop_unbuilt_pages(ctx: &Ctx, site: &Site, item: &QueueItem, out_dir: &Path, built: bool, job: &Job) -> QueueItem {
    let mut item = item.clone();
    if !built || !out_dir.join("index.html").is_file() {
        return item;
    }
    let exists = page_exists_in(out_dir);
    let (mut dropped, mut keep_p, mut keep_n) = (Vec::new(), Vec::new(), Vec::new());
    for u in &item.pages {
        if exists(&path_of(u)) { keep_p.push(u.clone()) } else { dropped.push(path_of(u)) }
    }
    let mut hidden = Vec::new();
    for u in &item.new_pages {
        if !exists(&path_of(u)) {
            dropped.push(path_of(u));
        } else if page_noindex(out_dir, &path_of(u)) {
            hidden.push(path_of(u));
        } else {
            keep_n.push(u.clone());
        }
    }
    if dropped.is_empty() && hidden.is_empty() {
        return item;
    }
    if !dropped.is_empty() {
        job.line(&format!("AI 报的页面里有 {} 个在构建产物里没有（多半是路径写错了），验收时不查它们：{}", dropped.len(), dropped.join("、")));
    }
    if !hidden.is_empty() {
        job.line(&format!("{} 设了 noindex（分享卡片之类的内部页面），不按新页面查站内链接和 sitemap", hidden.join("、")));
    }
    item.pages = keep_p.clone();
    item.new_pages = keep_n.clone();
    let _ = site_ops::update_item(&ctx.site_dir(&site.id), &item.id, now(), |i| {
        i.pages = keep_p.clone();
        i.new_pages = keep_n.clone();
    });
    item
}

/// 一起上线的条目只记自己页面相关的待核对：对照文字只属于提供它的那条，别的条目的页面问题不挂到它头上
fn shipped_for(rider: &QueueItem, sh: &Shipped) -> Shipped {
    let mut own = sh.clone();
    let paths: Vec<String> = std::iter::once(path_of(&rider.opp.page)).chain(rider.pages.iter().map(|u| path_of(u))).chain(rider.new_pages.iter().map(|u| path_of(u))).filter(|p| p.len() > 1).collect();
    // 路径要按边界匹配（/zh/ 不能把 /zh/guide/ 的问题也认领了）
    let bounded = |a: &str, p: &str| a.match_indices(p).any(|(i, _)| a[i + p.len()..].chars().next().is_none_or(|c| "：:、 ，,；;）)".contains(c)));
    own.attention.retain(|a| !a.starts_with(SNIPPET_MISS) && !a.starts_with(crate::site_verify::CHALLENGED) && paths.iter().any(|p| bounded(a, p)));
    own
}

/// 页面文件或网址路径的「名字」：去掉扩展名的末段；index 取上一级目录名（src/pages/foo/index.astro 和 /foo/ 都是 foo）
fn page_stem(p: &str) -> String {
    let segs: Vec<&str> = p.trim_end_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    let last = segs.last().copied().unwrap_or("");
    let name = last.rsplit_once('.').map(|(n, _)| n).unwrap_or(last);
    if name == "index" { segs.iter().rev().nth(1).copied().unwrap_or("").to_string() } else { name.to_string() }
}

/// 机会属于哪条道：0 修复上线后的问题、1 你手动加的、2 计划项、3 数据规则找出的
fn lane_of(i: &QueueItem) -> usize {
    if i.opp.kind == "followup" { 0 } else if i.opp.source == "manual" { 1 } else if i.opp.kind == "plan" { 2 } else { 3 }
}

/// 按道排：修复 → 手动 → 计划项和数据机会轮着来（上一条做的是哪条道，这次先做另一条），每条道里保持原来的分数顺序
fn order_lanes(picked: Vec<QueueItem>, last_lane: usize) -> Vec<QueueItem> {
    let mut lanes: Vec<Vec<QueueItem>> = vec![Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for i in picked {
        let l = lane_of(&i);
        lanes[l].push(i);
    }
    let (mut plan, mut data) = (std::mem::take(&mut lanes[2]), std::mem::take(&mut lanes[3]));
    let mut out = std::mem::take(&mut lanes[0]);
    out.extend(std::mem::take(&mut lanes[1]));
    let mut take_plan = last_lane != 2;
    while !plan.is_empty() || !data.is_empty() {
        let src = if (take_plan && !plan.is_empty()) || data.is_empty() { &mut plan } else { &mut data };
        out.push(src.remove(0));
        take_plan = !take_plan;
    }
    out
}

/// %E5%85%85 这类百分号编码解回原文（解不了的原样保留）
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// 一条任务失败时写在条目上的原因。作业超时被 Pilot 强停时 AI 回合报的是「被停止了」，那不是人停的：
/// 写成「作业超时被停止时…」（自动重试认这个开头），分支上已有提交的记下来，自动「重新检查」接着用
fn failure_note(job: &Job, e: &str, site_dir: &Path, item: &QueueItem, site_id: &str) -> (String, Option<String>) {
    // 「被停止」有两种：Pilot 超时强停的才自动重做；你在会话里点的停止不重做
    if !e.starts_with(STOPPED) || job.canceled() || !was_killed(site_id) {
        return (e.to_string(), None);
    }
    // base_sha 是 implement 里才写进条目的：拿最新的条目看分支上有没有提交
    let fresh = site_ops::load_queue(site_dir).into_iter().find(|i| i.id == item.id).unwrap_or_else(|| item.clone());
    let wt = crate::sites::build_root(site_dir).join("work").join(&item.id);
    let head = branch_head(&wt, &fresh.base_sha);
    let tail = if head.is_some() { "AI 已经把改动提交到分支上：会自动「重新检查」接着用" } else { "AI 还没提交改动：会自动放回排队重做" };
    (format!("作业超时被停止时这条还在执行，被中断了。{tail}"), head)
}

/// 改动类型（任务上的 change）对应 classify 产出的细分类型
fn kinds_of_change(change: &str) -> &'static [&'static str] {
    match change {
        // 老数据里降档记的是细分类型本身
        "title" => &["title"],
        "meta_description" => &["meta_description"],
        "meta" => &["title", "meta_description", "meta"],
        "internal_links" => &["internal_links"],
        "content" => &["content"],
        "new_content" => &["new_page", "new_content"],
        "technical" => &["technical", "sitemap"],
        _ => &[],
    }
}

/// 全自动方案：验收发现一般问题时开一条「修技术问题」的后续任务（每个站点每天最多一条，防止来回改）。
fn follow_up(ctx: &Ctx, site: &Site, item: &QueueItem, attention: &str) {
    // 「新内容没找到」是验收没对上文字、「被防火墙拦住」是验收查不了：都不是网站有问题，不开修复任务（AI 也改不了）
    let attention = attention.split('；').filter(|p| !p.starts_with(SNIPPET_MISS) && !p.starts_with(crate::site_verify::CHALLENGED)).collect::<Vec<_>>().join("；");
    if attention.is_empty() {
        return;
    }
    let Some(g) = group_rules(ctx, site) else { return };
    // 修复任务自己上线后又有待核对：不再开下一条，否则 AI 修不掉的问题会一天一条没完没了；停止托管的站点也不开
    if g.autonomy != "auto" || !g.active_for(&site.id) || !g.allows("technical") || item.opp.kind == "followup" {
        return;
    }
    let site_dir = ctx.site_dir(&site.id);
    if site_ops::load_queue(&site_dir).iter().any(|i| i.opp.kind == "followup" && i.updated_at >= now().saturating_sub(DAY) && i.status != "dismissed") {
        return;
    }
    // 每条改动只开一次（key 不带日期：驳回过、处理过的不再提）
    let opp = Opportunity {
        key: format!("followup:{}", item.id),
        kind: "followup".into(),
        title: format!("修复上线验收发现的问题：{}", item.opp.title),
        page: item.new_pages.first().or(item.pages.first()).cloned().unwrap_or_else(|| item.opp.page.clone()),
        evidence: format!("「{}」上线后线上验收发现：{attention}", item.opp.title),
        change: "technical".into(),
        source: "health".into(),
        score: 2000.0,
        ..Default::default()
    };
    if site_ops::enqueue(&site_dir, &[opp], now(), COOLDOWN).map(|a| !a.is_empty()).unwrap_or(false) {
        log_item(ctx, site, "planned", item, "验收发现的问题已经排进待处理，下一轮由 AI 修");
    }
}

/// 线上验收严重失败：自动回滚（回滚也验收），回滚失败就暂停这个站点的托管；
/// 全自动方案里每条改动给 AI 一次按失败原因重做的机会，再失败就留给你。
/// 装箱返回，打断 accept → AI 重做 → accept 的递归类型。
fn on_verify_failed<'a>(ctx: &'a Ctx, site: &'a Site, job: &'a Job, item_id: &'a str, reason: &'a str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + 'a>> {
    Box::pin(verify_failed(ctx, site, job, item_id, reason))
}

async fn verify_failed(ctx: &Ctx, site: &Site, job: &Job, item_id: &str, reason: &str) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let item = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item_id).ok_or("这条改动不存在")?;
    let why = reason.trim_start_matches(crate::site_verify::HARD_PREFIX).to_string();
    log_item(ctx, site, "watch_failed", &item, &format!("上线后验收没通过：{why}，自动回滚"));
    job.line(&format!("线上验收没通过（{why}），自动回滚"));
    if let Err(e) = rollback(ctx, site, job, item_id).await {
        pause_site(ctx, site, &format!("自动回滚失败：{e}"));
        ctx.notify(&format!("{} · 回滚失败，已暂停托管", site.name), &item.opp.title);
        let _ = site_ops::update_item(&site_dir, item_id, now(), |i| {
            i.status = "failed".into();
            i.note = format!("上线后验收没通过（{why}），自动回滚也失败了（{e}）。已暂停这个站点的托管，请尽快检查线上。");
        });
        ctx.emit_queue(&site.id);
        return Err(format!("{reason}；自动回滚也失败了：{e}。已暂停这个站点的托管，请尽快检查线上"));
    }
    // 这次一起上线的「之前合并没上线」的条目也可能就是坏的那条：在主条回退之后按合并的先后倒着一并回退（先退新的不容易冲突），
    // 标失败，再上线一次；不然下一次上线它又搭车、再把无辜的改动回滚一遍
    let mut ride: Vec<QueueItem> = site_ops::load_queue(&site_dir).into_iter().filter(|i| i.status == "merged" && i.deployed_at == 0 && !i.merge_sha.is_empty() && i.id != item_id).collect();
    ride.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
    let mut reverted = Vec::new();
    for r in &ride {
        match revert_item(ctx, site, job, r).await {
            Ok(sha) => {
                let _ = site_ops::update_item(&site_dir, &r.id, now(), |i| {
                    i.status = "failed".into();
                    i.retry_note = why.clone();
                    i.auto_retried = false;
                    i.note = format!("和「{}」一起上线时线上验收没通过（{why}），分不清是哪条的问题，一并回退了。可以「让 AI 重新改」，AI 会拿到失败原因", item.opp.title);
                });
                log_item(ctx, site, "rolled_back", r, "和另一条一起上线时验收没通过，一并回退");
                reverted.push((r.clone(), sha));
            }
            Err(e) => job.line(&format!("一起上线的「{}」回退失败：{e}", r.opp.title)),
        }
    }
    if let Some((_, sha)) = reverted.last() {
        let spec = reverted.iter().fold(crate::site_verify::Spec { domain: site.domain.clone(), ..Default::default() }, |a, (r, _)| join_specs(a, spec_for(site, r, true)));
        if let Err(e) = ship(ctx, site, job, spec, sha, "rollback").await {
            job.line(&format!("一并回退后重新上线没成功：{e}"));
        }
    }
    log_item(ctx, site, "rolled_back", &item, &format!("上线后验收没通过（{why}），已自动回滚"));
    downgrade_if_repeated(ctx, site, &item.opp.change);
    let item = site_ops::update_item(&site_dir, item_id, now(), |i| i.verify_rollbacks += 1)?.ok_or("这条改动不存在")?;
    let auto = group_rules(ctx, site).is_some_and(|g| g.autonomy == "auto") && crate::managed::load(&ctx.state().data_dir).rules_for(&site.id).autonomy == "auto";
    let can_retry = auto && item.retries == 0 && over_budget(&budget(site, &site_dir, &ctx.state().convos)).is_none();
    if can_retry {
        site_ops::update_item(&site_dir, item_id, now(), |i| {
            i.status = "failed".into();
            i.retries += 1;
            i.retry_note = why.clone();
            i.auto_retried = false;
            i.note = format!("上线后验收没通过（{why}），已自动回滚；AI 正在按失败原因重做一次。");
        })?;
        ctx.emit_queue(&site.id);
        job.line("已回滚。全自动方案：让 AI 按失败原因重做一次");
        // 重做会再走一遍改动 → 检查 → 上线 → 验收
        return work_one(ctx, site, job, item_id).await.map(|m| format!("第一次上线后验收没通过，已回滚；AI 重做：{m}"));
    }
    site_ops::update_item(&site_dir, item_id, now(), |i| {
        i.status = "failed".into();
        i.note = format!("上线后验收没通过（{why}），已自动回滚{}。可以「让 AI 重新改」，AI 会拿到失败原因。", if i.retries > 0 { "（AI 已经重做过一次）" } else { "" });
        i.retry_note = why.clone();
        i.auto_retried = false;
    })?;
    ctx.emit_queue(&site.id);
    ctx.notify(&format!("{} · 上线后验收没通过，已回滚", site.name), &item.opp.title);
    Err(format!("{reason}。已自动回滚"))
}

/// 暂停一个站点的托管（回滚都失败了，网站可能处在坏的状态，需要人来看）。
fn pause_site(ctx: &Ctx, site: &Site, why: &str) {
    let data_dir = ctx.state().data_dir.clone();
    let _ = crate::managed::modify(&data_dir, |c| {
        if let Some(g) = c.groups.iter_mut().find(|g| g.sites.contains(&site.id)) {
            if !g.stopped.contains(&site.id) {
                g.stopped.push(site.id.clone());
            }
        }
    });
    log(ctx, site, "paused", "暂停托管", why, "");
}

/// 同一类改动最近 30 天因为线上验收失败被回滚了 2 次：这类改动不再自动上线，改成留给你确认。
fn downgrade_if_repeated(ctx: &Ctx, site: &Site, change: &str) {
    if change.is_empty() || site.ops.downgraded.iter().any(|d| d == change) {
        return;
    }
    let since = now().saturating_sub(30 * DAY);
    let n = site_ops::load_queue(&ctx.site_dir(&site.id)).iter().filter(|i| i.opp.change == change && i.verify_rollbacks > 0 && i.updated_at >= since).count();
    // 这一次还没记进 verify_rollbacks，所以已有 1 条就够 2 次了
    if n + 1 >= 2 {
        let change = change.to_string();
        let _ = ctx.state().sites.update(&site.id, now(), |s| {
            if !s.ops.downgraded.contains(&change) {
                s.ops.downgraded.push(change.clone());
            }
            s.ops.downgraded_at = now();
            Ok(())
        });
        log(ctx, site, "downgraded", &format!("「{}」类改动不再自动上线", crate::managed::action_label(&change)), "最近 30 天有 2 次上线后验收没通过、被回滚，之后这类改动留给你确认", "");
    }
}

/// 确认一条改动：GitHub 合并 PR（squash）、本地分支压缩合并进默认分支，然后上线。`auto`＝第 4 档自动执行。
pub async fn accept(ctx: &Ctx, site: &Site, job: &Job, item_id: &str, auto: bool) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let item = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item_id).ok_or("这条改动不存在")?;
    // 已经合并、上线那一步失败了：不用再合并，直接重新上线
    if item.status == "merged" && !item.merge_sha.is_empty() && site.deploy.mode != "none" {
        job.step("merge", "ok", "已合并过");
        let ride = riders(ctx, site, &[item_id], job);
        // 本地文件夹上线的是主分支最新的代码（之后合并的也在里面），不是这一条当时的合并提交
        let commit = if site.source == "local" { crate::site_git::code_dir(site, None).map(|d| d.to_path_buf()).ok() } else { None };
        let commit = match commit {
            Some(d) => crate::site_git::head_sha(&d).await,
            None => item.merge_sha.clone(),
        };
        let sh = match ship(ctx, site, job, with_riders(site, spec_for(site, &item, false), &ride), &commit, if auto { "ops" } else { "manual" }).await {
            Ok(sh) => sh,
            Err(e) if e.starts_with(crate::site_verify::HARD_PREFIX) => return on_verify_failed(ctx, site, job, item_id, &e).await,
            Err(e) => return Err(e),
        };
        record_shipped(ctx, site, item_id, &sh)?;
        for r in &ride {
            record_shipped(ctx, site, &r.id, &shipped_for(r, &sh))?;
        }
        return Ok(format!("{}{}", if sh.url.is_empty() { "已上线".to_string() } else { format!("已上线 {}", sh.url) }, ride_note(&ride)));
    }
    if item.status != "proposed" {
        return Err("只有待确认的改动可以确认".into());
    }
    if item.branch.is_empty() {
        // 第 2 档的方案：你在会话里处理完了，标记完成即可。
        site_ops::update_item(&site_dir, item_id, now(), |i| i.status = "done".into())?;
        return Ok("已标记为处理完成".into());
    }
    // 发不了就先别合并：不然改动进了主分支却上不了线。改动留在分支上，授权后调度会接着上线
    if let Some(why) = crate::site_deploy::deploy_blocker(site, &ctx.state().conns) {
        job.step("merge", "failed", "");
        return Err(why);
    }
    job.step("merge", "running", &item.branch);
    let merge_sha = match merge_item(ctx, site, job, &item).await {
        Ok(sha) => sha,
        Err(e) if e == ALREADY_IN_MAIN => {
            job.step("merge", "skipped", "主分支里都有了");
            job.step("ship", "skipped", "");
            continue_batch(ctx, site, item_id);
            site_ops::update_item(&site_dir, item_id, now(), |i| {
                i.status = "done".into();
                i.note = e.clone();
            })?;
            ctx.emit_queue(&site.id);
            return Ok(e);
        }
        Err(e) => {
            job.step("merge", "failed", "");
            return Err(e);
        }
    };
    job.step("merge", "ok", merge_sha.get(..7).unwrap_or(""));
    mark_merged(ctx, site, &item, &merge_sha, auto)?;
    ctx.emit_queue(&site.id);
    if site.deploy.mode == "none" {
        // 没配部署：合并进主分支就是这条改动的终点，记一笔、通知一声，别悄悄进了主分支
        log_item(ctx, site, "shipped", &item, "已合并进主分支（站点没有配置部署，上线由你来做）");
        if auto {
            ctx.notify(&format!("{} · 已自动合并", site.name), &format!("{}（没有配置部署，上线由你来做）", item.opp.title));
        }
        return Ok("已合并（站点没有配置部署）".into());
    }
    let ride = riders(ctx, site, &[item_id], job);
    let sh = match ship(ctx, site, job, with_riders(site, spec_for(site, &item, false), &ride), &merge_sha, if auto { "ops" } else { "manual" }).await {
        Ok(sh) => sh,
        Err(e) if e.starts_with(crate::site_verify::HARD_PREFIX) => return on_verify_failed(ctx, site, job, item_id, &e).await,
        Err(e) => {
            let _ = site_ops::update_item(&site_dir, item_id, now(), |x| x.note = format!("已合并进主分支，上线没完成：{e}"));
            ctx.emit_queue(&site.id);
            return Err(e);
        }
    };
    record_shipped(ctx, site, item_id, &sh)?;
    for r in &ride {
        record_shipped(ctx, site, &r.id, &shipped_for(r, &sh))?;
    }
    if auto {
        ctx.notify(&format!("{} · 已自动上线", site.name), &item.opp.title);
    }
    Ok(format!("{}{}", if sh.url.is_empty() { "已上线".to_string() } else { format!("已上线 {}", sh.url) }, ride_note(&ride)))
}

/// 之前合并了、上线没完成的其它改动：这次上线的是主分支上最新的代码，它们本来就在里面——
/// 一起验收、一起记成已上线（不然明明线上已经是新的，这边还挂着「已合并」等你重新上线）。
fn riders(ctx: &Ctx, site: &Site, except: &[&str], job: &Job) -> Vec<QueueItem> {
    let ride: Vec<QueueItem> = site_ops::load_queue(&ctx.site_dir(&site.id))
        .into_iter()
        .filter(|i| i.status == "merged" && i.deployed_at == 0 && !i.merge_sha.is_empty() && !except.contains(&i.id.as_str()))
        .collect();
    if !ride.is_empty() {
        job.line(&format!("之前合并了、还没上线的 {} 条也在这次的代码里，一起上线、一起验收：{}", ride.len(), ride.iter().map(|i| format!("「{}」", i.opp.title)).collect::<String>()));
    }
    ride
}

fn with_riders(site: &Site, spec: crate::site_verify::Spec, ride: &[QueueItem]) -> crate::site_verify::Spec {
    ride.iter().fold(spec, |a, r| join_specs(a, spec_for(site, r, false)))
}

fn ride_note(ride: &[QueueItem]) -> String {
    if ride.is_empty() { String::new() } else { format!("（之前合并没上线的 {} 条一起上线了）", ride.len()) }
}

fn waiting_for_deploy(verdict: &str) -> bool {
    verdict.strip_prefix("自动上线失败：").is_some_and(|v| v.starts_with(crate::site_deploy::NOT_DEPLOYABLE))
}

/// 这条的改动主分支里都有了（多半包含在先合并的另一条里）：没东西可合，这条就算做完了，不是失败。
const ALREADY_IN_MAIN: &str = "这条的改动主分支里都有了，不用再合并，会跟着主分支一起上线";

/// 把一条改动合并进主分支：GitHub 合并 PR（squash），本地分支压缩合并进默认分支。返回合并提交。
/// 本地合不进去（和主分支上的改动冲突）时把主分支恢复原样，分支和工作目录留着，可以让 AI 在最新代码上重改。
async fn merge_item(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let title = format!("ops: improve {}", path_of(&item.opp.page));
    match site.source.as_str() {
        "github" => {
            let cb = code_base(ctx, site, job).await?;
            if item.pr_number == 0 {
                return Err("这条改动还没有 PR".into());
            }
            let pr = crate::site_github::get_pr(&cb.token, &site.repo, item.pr_number).await?;
            let sha = if pr.merged {
                pr.merge_sha
            } else if pr.state == "closed" {
                return Err("PR 已在 GitHub 上关闭".into());
            } else {
                crate::site_github::merge_pr(&cb.token, &site.repo, item.pr_number, &title).await?
            };
            crate::site_git::remove_worktree(&cb.main, &crate::sites::build_root(&site_dir).join("work").join(&item.id)).await;
            let _ = crate::site_git::git(&cb.main, &["branch", "-D", &item.branch], "").await;
            Ok(sha)
        }
        _ => {
            let main = crate::site_git::code_dir(site, None)?;
            if let Some(why) = local_not_ready(site, &main).await {
                return Err(format!("{why}。提交或撤销后再上线；托管里自动上线的条目，文件夹干净了 Pilot 会自己接着上线（Pilot 不会动你正在改的东西）"));
            }
            if let Err(e) = crate::site_git::git_ok(&main, &["merge", "--squash", &item.branch], "").await {
                // 上面确认过没有未提交的改动，这里恢复的只是这次合并带进来的内容
                let _ = crate::site_git::git(&main, &["reset", "-q", "--hard", "HEAD"], "").await;
                return Err(if e.contains("CONFLICT") || e.contains("conflict") { "和主分支上的改动冲突了，合不进去".into() } else { e });
            }
            if crate::site_git::nothing_staged(&main).await {
                let _ = crate::site_git::git(&main, &["reset", "-q", "--hard", "HEAD"], "").await;
                crate::site_git::remove_worktree(&main, &crate::sites::build_root(&site_dir).join("work").join(&item.id)).await;
                let _ = crate::site_git::git(&main, &["branch", "-D", &item.branch], "").await;
                return Err(ALREADY_IN_MAIN.into());
            }
            crate::site_git::git_commit_ok(&main, &["commit", "-qm", &title]).await?;
            crate::site_git::remove_worktree(&main, &crate::sites::build_root(&site_dir).join("work").join(&item.id)).await;
            let _ = crate::site_git::git(&main, &["branch", "-D", &item.branch], "").await;
            push_after(site, &main, job).await;
            Ok(crate::site_git::head_sha(&main).await)
        }
    }
}

/// 本地主分支合并 / 回退后推到 GitHub（有 origin 才推）；推不上去只在作业里记一行，不耽误上线
async fn push_after(site: &Site, main: &Path, job: &Job) {
    match crate::site_git::push_origin(main, &site.default_branch).await {
        Ok(true) => job.line("已推送到 GitHub"),
        Ok(false) => {}
        Err(e) => job.line(&format!("推送到 GitHub 没成功（不影响上线，可以之后自己推）：{}", e.lines().last().unwrap_or(&e))),
    }
}

/// 合并完记下合并提交和上线前的数据快照（之后对比效果用）。
fn mark_merged(ctx: &Ctx, site: &Site, item: &QueueItem, merge_sha: &str, auto: bool) -> Result<(), String> {
    let site_dir = ctx.site_dir(&site.id);
    let before = page_snapshot(&site_dir, &item.opp.page);
    let (before_query, before_site) = (query_snapshot(&site_dir, &item.opp.expect_query, &item.opp.page), Some(site_snapshot(&site_dir)));
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.status = "merged".into();
        i.merge_sha = merge_sha.to_string();
        i.auto = auto;
        i.before = before.clone();
        i.before_query = before_query.clone();
        i.before_site = before_site.clone();
    })?;
    Ok(())
}

/// 一起合并上线：按顺序把几条待确认的改动合并进主分支，合完只构建上线一次。
/// 合不进去的（多半是和前面刚合的改到了同一处）跳过、留在待确认里，其余照常上线；
/// 线上验收严重失败就把这次合并的全部回退，再上线一次。
pub async fn accept_many(ctx: &Ctx, site: &Site, job: &Job, ids: &str) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let queue = site_ops::load_queue(&site_dir);
    let items: Vec<QueueItem> = ids.split(',').filter_map(|id| queue.iter().find(|i| i.id == id && i.status == "proposed" && !i.branch.is_empty()).cloned()).collect();
    if items.is_empty() {
        return Err("没有可以合并的改动（可能已经处理过了）".into());
    }
    if let Some(why) = crate::site_deploy::deploy_blocker(site, &ctx.state().conns) {
        job.step("merge", "failed", "");
        return Err(why);
    }
    let mut merged: Vec<(QueueItem, String)> = Vec::new();
    let mut already = 0;
    let mut skipped: Vec<(QueueItem, String)> = Vec::new();
    for (n, item) in items.iter().enumerate() {
        if job.canceled() {
            break;
        }
        job.step("merge", "running", &format!("{}/{} · {}", n + 1, items.len(), item.opp.title));
        job.line(&format!("合并「{}」（{}）", item.opp.title, item.branch));
        match merge_item(ctx, site, job, item).await {
            Ok(sha) => {
                mark_merged(ctx, site, item, &sha, false)?;
                merged.push((item.clone(), sha));
            }
            Err(e) if e == ALREADY_IN_MAIN => {
                job.line(&format!("「{}」：{e}", item.opp.title));
                continue_batch(ctx, site, &item.id);
                site_ops::update_item(&site_dir, &item.id, now(), |i| {
                    i.status = "done".into();
                    i.note = e.clone();
                })?;
                already += 1;
            }
            Err(e) => {
                job.line(&format!("「{}」没合并：{e}", item.opp.title));
                let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| i.note = format!("一起合并上线时没合并：{e}。可以让 AI 在最新代码上重改"));
                skipped.push((item.clone(), e));
            }
        }
        ctx.emit_queue(&site.id);
    }
    if merged.is_empty() && skipped.is_empty() && already > 0 {
        job.step("merge", "skipped", "主分支里都有了");
        job.step("ship", "skipped", "");
        return Ok(format!("{already} 条的改动主分支里都有了，不用再合并"));
    }
    if merged.is_empty() {
        job.step("merge", "failed", "");
        return Err(skipped.first().map(|(_, e)| e.clone()).unwrap_or_else(|| "已取消".into()));
    }
    let skip_note = if skipped.is_empty() { String::new() } else { format!("；{} 条没合进去，留在待确认里：{}", skipped.len(), skipped.iter().map(|(i, _)| format!("「{}」", i.opp.title)).collect::<Vec<_>>().join("")) };
    job.step("merge", "ok", &format!("合并了 {} 条{}", merged.len(), if skipped.is_empty() { String::new() } else { format!("，{} 条没合进去", skipped.len()) }));
    if site.deploy.mode == "none" {
        return Ok(format!("已合并 {} 条（站点没有配置部署）{skip_note}", merged.len()));
    }
    let ids_merged: Vec<&str> = merged.iter().map(|(i, _)| i.id.as_str()).collect();
    let ride = riders(ctx, site, &ids_merged, job);
    let spec = with_riders(site, merged.iter().map(|(i, _)| spec_for(site, i, false)).reduce(join_specs).unwrap_or_default(), &ride);
    let last = merged.last().map(|(_, sha)| sha.clone()).unwrap_or_default();
    match ship(ctx, site, job, spec, &last, "manual").await {
        Ok(sh) => {
            for (i, _) in &merged {
                record_shipped(ctx, site, &i.id, &sh)?;
            }
            for r in &ride {
                record_shipped(ctx, site, &r.id, &sh)?;
            }
            Ok(format!("{} 条已上线{}{}{skip_note}", merged.len(), if sh.url.is_empty() { String::new() } else { format!(" {}", sh.url) }, ride_note(&ride)))
        }
        Err(e) if e.starts_with(crate::site_verify::HARD_PREFIX) => {
            let why = e.trim_start_matches(crate::site_verify::HARD_PREFIX).to_string();
            job.line(&format!("线上验收没通过（{why}），把这次合并的 {} 条全部回退", merged.len()));
            let mut last_revert = String::new();
            for (i, _) in merged.iter().rev() {
                let cur = site_ops::load_queue(&site_dir).into_iter().find(|x| x.id == i.id).unwrap_or_else(|| i.clone());
                match revert_item(ctx, site, job, &cur).await {
                    Ok(sha) => last_revert = sha,
                    Err(re) => {
                        pause_site(ctx, site, &format!("自动回滚失败：{re}"));
                        ctx.notify(&format!("{} · 回滚失败，已暂停托管", site.name), &i.opp.title);
                        return Err(format!("{e}；自动回滚也失败了：{re}。已暂停这个站点的托管，请尽快检查线上"));
                    }
                }
            }
            let spec = merged.iter().map(|(i, _)| spec_for(site, i, true)).reduce(join_specs).unwrap_or_default();
            if let Err(re) = ship(ctx, site, job, spec, &last_revert, "rollback").await {
                pause_site(ctx, site, &format!("回退后重新上线失败：{re}"));
                return Err(format!("{e}；回退后重新上线也失败了：{re}。已暂停这个站点的托管，请尽快检查线上"));
            }
            // 一起上线失败算一次：每类改动只判一次降档，而且在给这批条目记回滚次数之前判
            // （不然同一批里两条同类的，第二条会把第一条刚记的算成「之前又失败过一次」）
            let mut kinds: Vec<&str> = merged.iter().map(|(i, _)| i.opp.change.as_str()).collect();
            kinds.sort_unstable();
            kinds.dedup();
            for k in kinds {
                downgrade_if_repeated(ctx, site, k);
            }
            for (i, _) in &merged {
                log_item(ctx, site, "rolled_back", i, &format!("一起上线后验收没通过（{why}），已全部回退"));
                let _ = site_ops::update_item(&site_dir, &i.id, now(), |x| {
                    x.status = "failed".into();
                    x.verify_rollbacks += 1;
                    x.retry_note = why.clone();
                    x.note = format!("和其它 {} 条一起上线后验收没通过（{why}），已全部回退。可以「让 AI 重新改」，AI 会拿到失败原因；也可以一条一条单独上线，看是哪一条的问题。", merged.len() - 1);
                });
            }
            ctx.emit_queue(&site.id);
            ctx.notify(&format!("{} · 上线后验收没通过，已回滚", site.name), &format!("一起上线的 {} 条改动", merged.len()));
            Err(format!("{e}。已把这次合并的 {} 条全部回退", merged.len()))
        }
        Err(e) => {
            for (i, _) in &merged {
                let _ = site_ops::update_item(&site_dir, &i.id, now(), |x| x.note = format!("已合并进主分支，上线没完成：{e}"));
            }
            ctx.emit_queue(&site.id);
            Err(e)
        }
    }
}

/// 几条改动一起上线：验收的页面、新页面、文字片段合在一起查。
fn join_specs(mut a: crate::site_verify::Spec, b: crate::site_verify::Spec) -> crate::site_verify::Spec {
    for (to, from) in [(&mut a.pages, b.pages), (&mut a.primary, b.primary), (&mut a.new_pages, b.new_pages), (&mut a.snippets, b.snippets)] {
        for x in from {
            if !to.contains(&x) {
                to.push(x);
            }
        }
    }
    a
}

/// 待你确认的条目里，哪些合不进主分支（AI 改的时候主分支后来又有了改动、改到同几处）。
/// 只看本地来源的站点：用 git merge-tree 在内存里试合并，不动文件夹；GitHub 来源由 PR 自己显示能不能合。
pub async fn conflicting(site: &Site, site_dir: &Path) -> Vec<String> {
    if site.source != "local" {
        return Vec::new();
    }
    let Ok(main) = crate::site_git::code_dir(site, None) else { return Vec::new() };
    let mut out = Vec::new();
    for i in site_ops::load_queue(site_dir).into_iter().filter(|i| i.status == "proposed" && !i.branch.is_empty()) {
        // 退出码 1＝有冲突；其它错误（git 太旧、分支没了）不算冲突，照常让你确认
        if let Ok(r) = crate::site_git::git(&main, &["merge-tree", "--write-tree", "--name-only", &site.default_branch, &i.branch], "").await {
            if !r.ok && r.stderr.is_empty() {
                out.push(i.id);
            }
        }
    }
    out
}

/// 合并上线以后，处理剩下合不进主分支的待确认改动（不用你一条条点「让 AI 在最新代码上重改」）：
/// 先试不花钱的——把这版改动用 git 接到最新的主分支上，接上了就重新检查；接不上再交给 AI，
/// 带着上一版的改动在最新代码上重做（比整条从头改省）。处理完照常按规则上线或等你确认。
/// 处理过程中又有改动上线、让别的条目冲突，会接着处理；每条一次作业里最多处理一次。
/// `only`：你在某一条上点了「让 AI 在最新代码上重改」，只处理这一条。
async fn resolve(ctx: &Ctx, site: &Site, job: &Job, only: Option<&str>) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let mut tried: Vec<String> = Vec::new();
    let (mut rebased, mut redone, mut failed) = (0, 0, 0);
    loop {
        let ids: Vec<String> = match only {
            Some(id) => vec![id.to_string()],
            None => conflicting(site, &site_dir).await,
        };
        let Some(id) = ids.into_iter().find(|id| !tried.contains(id)) else { break };
        tried.push(id.clone());
        if job.canceled() {
            break;
        }
        let Some(item) = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == id && matches!(i.status.as_str(), "proposed" | "failed") && !i.branch.is_empty()) else { continue };
        job.line(&format!("—— 「{}」放到最新代码上 ——", item.opp.title));
        job.reset_from("rebase");
        job.step("rebase", "running", &item.opp.title);
        match rebase_item(ctx, site, job, &item).await {
            Ok(()) => {
                job.step("rebase", "ok", "接上了，不用 AI");
                job.line("用 git 接到最新代码上了，重新检查");
                rebased += 1;
                if let Err(e) = recheck(ctx, site, job, &id).await {
                    job.line(&format!("重新检查没通过：{e}"));
                    failed += 1;
                }
            }
            Err(why) => {
                job.step("rebase", "ok", "接不上，交给 AI");
                job.line(&format!("git 接不上（{why}），交给 AI 在最新代码上重做"));
                if let Some(b) = over_tokens(&budget(site, &site_dir, &ctx.state().convos)) {
                    job.line(&format!("{b}，先不交给 AI，留着等你处理"));
                    break;
                }
                let replay = item_diff(site, &item).await;
                site_ops::update_item(&site_dir, &id, now(), |i| {
                    i.status = "open".into();
                    i.note.clear();
                    i.verdict.clear();
                    i.replay = replay.clone();
                })?;
                ctx.emit_queue(&site.id);
                match work_one(ctx, site, job, &id).await {
                    Ok(_) => redone += 1,
                    Err(e) => {
                        job.line(&format!("AI 重做没成功：{e}"));
                        failed += 1;
                    }
                }
            }
        }
        ctx.emit_queue(&site.id);
    }
    if tried.is_empty() {
        return Ok("没有合不进去的改动".into());
    }
    let mut parts = Vec::new();
    if rebased > 0 {
        parts.push(format!("{rebased} 条直接接到了最新代码上"));
    }
    if redone > 0 {
        parts.push(format!("{redone} 条让 AI 在最新代码上重做了"));
    }
    if failed > 0 {
        parts.push(format!("{failed} 条没处理成，留给你"));
    }
    if parts.is_empty() {
        parts.push("没处理成，留给你".into());
    }
    Ok(format!("合不进主分支的改动：{}", parts.join("，")))
}

/// 用 git 把一条改动接到最新的主分支上（rebase）；接不上就恢复原样，返回原因。成功后更新基准提交。
async fn rebase_item(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem) -> Result<(), String> {
    let site_dir = ctx.site_dir(&site.id);
    let cb = code_base(ctx, site, job).await?;
    let wt = crate::sites::build_root(&site_dir).join("work").join(&item.id);
    if !wt.is_dir() {
        crate::site_git::reattach_worktree(&cb.main, &wt, &item.branch).await?;
    }
    // 构建会重写生成的数据文件、弄脏工作树：只有这些就还原，别的改动才算没提交
    if !settle_build_outputs(site, &wt).await {
        return Err("工作目录里有没提交的改动".into());
    }
    let base = crate::site_git::git_ok(&cb.main, &["rev-parse", &site.default_branch], "").await?.trim().to_string();
    let mut args: Vec<String> = crate::site_git::identity_flags(&wt).await;
    args.extend(["rebase", "--quiet", base.as_str()].map(String::from));
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    if let Err(e) = crate::site_git::git_ok(&wt, &argv, "").await {
        let _ = crate::site_git::git(&wt, &["rebase", "--abort"], "").await;
        return Err(if e.contains("CONFLICT") || e.contains("conflict") { "改到了同一处".into() } else { e.lines().next().unwrap_or("").to_string() });
    }
    let head = crate::site_git::head_sha(&wt).await;
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.base_sha = base.clone();
        i.head_sha = head.clone();
    })?;
    Ok(())
}

/// 一条改动相对它的基准提交的 diff（给 AI 重做时参考），太长就截断。
async fn item_diff(site: &Site, item: &QueueItem) -> String {
    let Ok(main) = crate::site_git::code_dir(site, None) else { return String::new() };
    let base = if item.base_sha.is_empty() { site.default_branch.clone() } else { item.base_sha.clone() };
    let d = crate::site_git::git_ok(&main, &["diff", "--unified=2", &base, &item.branch], "").await.unwrap_or_default();
    if d.chars().count() > 8000 {
        format!("{}\n…（后面还有，太长截断了）", d.chars().take(8000).collect::<String>())
    } else {
        d
    }
}

/// 驳回：关掉 PR / 删掉本地分支，并记住这个机会不再提。
pub async fn dismiss(ctx: &Ctx, site: &Site, item_id: &str) -> Result<(), String> {
    let site_dir = ctx.site_dir(&site.id);
    let item = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item_id).ok_or("这条机会不存在")?;
    if matches!(item.status.as_str(), "working" | "merged" | "deployed") {
        return Err("正在处理或已合并的改动不能驳回（已上线的请用回滚）".into());
    }
    if !item.branch.is_empty() {
        let st = ctx.state();
        if site.source == "github" {
            if let Ok(gh) = st.conns.get(&site.gh_conn_id) {
                if let Ok(token) = crate::keychain::get_key(&gh.id) {
                    if item.pr_number > 0 {
                        let _ = crate::site_github::comment(&token, &site.repo, item.pr_number, "Dismissed in Cligc Pilot.").await;
                        let _ = crate::site_github::close_pr(&token, &site.repo, item.pr_number).await;
                    }
                    if let Ok(main) = crate::github::repo_dir(&gh.skill_dir, &site.repo) {
                        crate::site_git::remove_worktree(&main, &crate::sites::build_root(&site_dir).join("work").join(&item.id)).await;
                        let _ = crate::site_git::git(&main, &["branch", "-D", &item.branch], "").await;
                    }
                }
            }
        } else if let Ok(main) = crate::site_git::code_dir(site, None) {
            crate::site_git::remove_worktree(&main, &crate::sites::build_root(&site_dir).join("work").join(&item.id)).await;
            let _ = crate::site_git::git(&main, &["branch", "-D", &item.branch], "").await;
        }
    }
    site_ops::update_item(&site_dir, item_id, now(), |i| i.status = "dismissed".into())?;
    ctx.emit_queue(&site.id);
    Ok(())
}

/// 回滚：回退那次合并提交（独立提交）并重新上线。
pub async fn rollback(ctx: &Ctx, site: &Site, job: &Job, item_id: &str) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let item = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item_id).ok_or("这条改动不存在")?;
    if !matches!(item.status.as_str(), "merged" | "deployed") || item.merge_sha.is_empty() {
        return Err("只有已合并的改动可以回滚".into());
    }
    let revert_sha = revert_item(ctx, site, job, &item).await?;
    if site.deploy.mode != "none" {
        match ship(ctx, site, job, spec_for(site, &item, true), &revert_sha, "rollback").await {
            Err(e) if e.starts_with(crate::site_verify::DOMAIN_TAKEN) => return Ok(format!("已回滚并上传到 Cloudflare 项目。{e}")),
            r => {
                r?;
            }
        }
    }
    Ok("已回滚并重新上线".into())
}

/// 回退一条已合并改动的合并提交（独立提交），标记为已回滚；不上线，调用方负责上线。
async fn revert_item(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let item_id = item.id.as_str();
    job.step("revert", "running", item.merge_sha.get(..7).unwrap_or(""));
    let revert_sha = match site.source.as_str() {
        "github" => {
            let cb = code_base(ctx, site, job).await?;
            let short: String = item.id.chars().take(6).collect();
            let branch = format!("pilot/revert-{short}");
            let wt = crate::sites::build_root(&site_dir).join("work").join(format!("revert-{short}"));
            crate::site_git::worktree_at(&cb.main, &wt, &cb.base_ref, Some(&branch)).await?;
            crate::site_git::git_commit_ok(&wt, &["revert", "--no-edit", &item.merge_sha]).await?;
            crate::site_git::git_ok(&wt, &["push", "--force", "-u", "origin", &branch], &cb.token).await?;
            let title = format!("Revert: ops: improve {}", path_of(&item.opp.page));
            let pr = crate::site_github::create_pr(&cb.token, &site.repo, &branch, &site.default_branch, &title, "Rollback requested in Cligc Pilot.").await?;
            let sha = crate::site_github::merge_pr(&cb.token, &site.repo, pr.number, &title).await?;
            crate::site_git::remove_worktree(&cb.main, &wt).await;
            let _ = crate::site_git::git(&cb.main, &["branch", "-D", &branch], "").await;
            sha
        }
        _ => {
            let main = crate::site_git::code_dir(site, None)?;
            if let Some(why) = local_not_ready(site, &main).await {
                return Err(format!("{why}。请先处理好再回滚"));
            }
            if let Err(e) = crate::site_git::git_commit_ok(&main, &["revert", "--no-edit", &item.merge_sha]).await {
                // 回退冲突（这些文件后来又被别的改动改过）：把主分支恢复原样，不能留着冲突标记让之后每次合并都失败
                let _ = crate::site_git::git(&main, &["revert", "--abort"], "").await;
                let _ = crate::site_git::git(&main, &["reset", "-q", "--hard", "HEAD"], "").await;
                return Err(if e.contains("CONFLICT") || e.contains("conflict") { "这些文件后来又被别的改动改过，回退会冲突，没有动主分支".into() } else { e });
            }
            push_after(site, &main, job).await;
            crate::site_git::head_sha(&main).await
        }
    };
    job.step("revert", "ok", revert_sha.get(..7).unwrap_or(""));
    site_ops::update_item(&site_dir, item_id, now(), |i| {
        i.status = "rolled_back".into();
        i.note = format!("已回退（{}）", revert_sha.get(..7).unwrap_or(""));
    })?;
    ctx.emit_queue(&site.id);
    Ok(revert_sha)
}

// ---------------------------------------------------------------------------
// 体检 / 周报 / 复盘
// ---------------------------------------------------------------------------

pub fn reports_dir(site_dir: &Path) -> PathBuf {
    site_dir.join("reports")
}

fn write_report(site_dir: &Path, name: &str, body: &str) -> Result<(), String> {
    let d = reports_dir(site_dir);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(d.join(format!("{name}.md")), body).map_err(|e| e.to_string())?;
    // 同类报告只留最近 40 份（文件名是 日期-类型.md，按名字排就是按日期排）
    let kind = name.rsplit('-').next().unwrap_or("").to_string();
    let mut same: Vec<PathBuf> = std::fs::read_dir(&d).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&format!("-{kind}.md")))).collect()).unwrap_or_default();
    same.sort();
    for p in same.iter().take(same.len().saturating_sub(40)) {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

async fn health(ctx: &Ctx, site: &Site, job: &Job) -> Result<String, String> {
    let st = ctx.state();
    let site_dir = ctx.site_dir(&site.id);
    refresh_data(ctx, site, job).await;
    // 构建与检查（GitHub 来源检查默认分支最新代码，本地来源检查文件夹当前状态）
    job.step("prepare", "running", "");
    let (dir, stamp) = match site.source.as_str() {
        "github" => {
            let gh = st.conns.get(&site.gh_conn_id)?;
            (crate::site_deploy::prepare_github(site, &gh, &site_dir, job).await?, Some(site_dir.join("install.stamp")))
        }
        _ => (crate::site_git::code_dir(site, None)?, None),
    };
    job.step("prepare", "ok", "");
    let built = crate::site_deploy::build_and_check(site, &dir, stamp.as_deref(), job).await;
    // 刚构建完：页面体检（结构化数据、图片）顺手用这份产物重查，别拿一周前的报告出任务
    if let Ok(b) = &built {
        let closed = crate::page_audit::refresh(site, &site_dir, b.out.clone()).await;
        if closed > 0 {
            job.line(&format!("重新体检后，{closed} 条结构化数据 / 图片任务清单里的页面已经没有问题了，标为完成"));
            ctx.emit_queue(&site.id);
        }
    }
    job.step("report", "running", "");
    let opps = site_ops::opportunities(site, &site_dir);
    let tech: Vec<&Opportunity> = opps.iter().filter(|o| o.change == "technical").collect();
    let meta = crate::site_data::load_meta(&site_dir);
    let mut md = format!("# {} 站点体检 · {}\n\n", site.name, today());
    md.push_str(&format!("## 构建与检查\n\n{}\n\n", match &built {
        Ok(b) => format!("通过。产物 {} 个页面。", crate::site_deploy::build_manifest(&b.out).len()),
        Err(e) => format!("**没通过**：{}", e.lines().next().unwrap_or("")),
    }));
    md.push_str("## 数据源\n\n");
    for (l, m, on) in [("Search Console", &meta.gsc, !site.data.gsc_site.is_empty()), ("GA4", &meta.ga4, !site.data.ga4_property.is_empty()), ("Bing", &meta.bing, !site.data.bing_site.is_empty())] {
        if on {
            md.push_str(&format!("- {l}：{}\n", if m.error.is_empty() { format!("正常（截至 {}）", m.end) } else { format!("出错：{}", m.error) }));
        }
    }
    md.push_str("\n## 技术问题\n\n");
    if tech.is_empty() {
        md.push_str("没有发现抓取错误或 sitemap 报错。\n");
    }
    for o in &tech {
        md.push_str(&format!("- {}：{}\n", o.title, o.evidence));
    }
    let sm: serde_json::Value = crate::site_data::read_json(&crate::site_data::cache_dir(&site_dir), "gsc_sitemaps.json");
    if let Some(arr) = sm.as_array() {
        md.push_str("\n## Sitemap\n\n");
        for s in arr {
            md.push_str(&format!(
                "- {}：上次读取 {}，错误 {}、警告 {}\n",
                s["path"].as_str().unwrap_or(""),
                s["lastDownloaded"].as_str().unwrap_or("—"),
                s["errors"].as_str().unwrap_or("0"),
                s["warnings"].as_str().unwrap_or("0")
            ));
        }
    }
    write_report(&site_dir, &format!("{}-health", today()), &md)?;
    let added = site_ops::enqueue(&site_dir, &tech.into_iter().cloned().collect::<Vec<_>>(), now(), COOLDOWN)?;
    job.step("report", "ok", "");
    match built {
        Ok(_) => Ok(format!("体检完成，新增 {} 个技术问题", added.len())),
        Err(e) => Err(format!("构建或检查没通过：{}", e.lines().next().unwrap_or(""))),
    }
}

fn fmt_delta(cur: f64, prev: f64) -> String {
    if prev <= 0.0 {
        return "—".into();
    }
    let d = (cur - prev) / prev * 100.0;
    format!("{}{d:.0}%", if d > 0.0 { "+" } else { "" })
}

async fn report(ctx: &Ctx, site: &Site, job: &Job) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    refresh_data(ctx, site, job).await;
    job.step("report", "running", "");
    let d = crate::site_data::dashboard(&site_dir);
    let mut md = format!("# {} 周报 · {}\n\n", site.name, today());
    if !site.data.gsc_site.is_empty() {
        md.push_str(&format!(
            "## Google 搜索（{} ~ {}，对比前 28 天）\n\n| 指标 | 数值 | 变化 |\n|---|---|---|\n| 点击 | {:.0} | {} |\n| 展现 | {:.0} | {} |\n| 点击率 | {:.2}% | {} |\n| 平均排名 | {:.1} | {} |\n\n",
            d.meta.gsc.start, d.meta.gsc.end,
            d.gsc_totals.clicks, fmt_delta(d.gsc_totals.clicks, d.gsc_prev_totals.clicks),
            d.gsc_totals.impressions, fmt_delta(d.gsc_totals.impressions, d.gsc_prev_totals.impressions),
            d.gsc_totals.ctr * 100.0, fmt_delta(d.gsc_totals.ctr, d.gsc_prev_totals.ctr),
            d.gsc_totals.position, fmt_delta(d.gsc_prev_totals.position, d.gsc_totals.position),
        ));
        let mut movers: Vec<&crate::site_data::Compared> = d.gsc_pages.iter().filter(|p| p.prev_clicks > 0.0 || p.row.clicks > 0.0).collect();
        movers.sort_by(|a, b| (b.row.clicks - b.prev_clicks).abs().total_cmp(&(a.row.clicks - a.prev_clicks).abs()));
        if !movers.is_empty() {
            md.push_str("### 变化最大的页面\n\n");
            for p in movers.iter().take(8) {
                md.push_str(&format!("- {}：点击 {:.0} → {:.0}\n", path_of(&p.row.key), p.prev_clicks, p.row.clicks));
            }
            md.push('\n');
        }
        md.push_str("### 头部查询词\n\n");
        for q in d.gsc_queries.iter().take(10) {
            md.push_str(&format!("- 「{}」点击 {:.0}、展现 {:.0}、排名 {:.1}\n", q.row.key, q.row.clicks, q.row.impressions, q.row.position));
        }
        md.push('\n');
    }
    if !site.data.ga4_property.is_empty() {
        md.push_str(&format!("## GA4（近 28 天）\n\n浏览量 {:.0}（{}），会话 {:.0}，用户 {:.0}。\n\n", d.ga4_totals.views, fmt_delta(d.ga4_totals.views, d.ga4_prev_views), d.ga4_totals.sessions, d.ga4_totals.users));
        let ev: Vec<&crate::site_data::EventRow> = d.ga4_events.iter().filter(|e| e.dim.is_empty() && !["page_view", "session_start", "first_visit", "user_engagement", "scroll"].contains(&e.event.as_str())).collect();
        if !ev.is_empty() {
            md.push_str("转化事件：");
            md.push_str(&ev.iter().map(|e| format!("{} {:.0} 次", e.event, e.count)).collect::<Vec<_>>().join("，"));
            md.push_str("\n\n");
        }
    }
    if !site.data.bing_site.is_empty() {
        md.push_str(&format!("## Bing（近 28 天）\n\n点击 {:.0}，展现 {:.0}。\n\n", d.bing_totals.clicks, d.bing_totals.impressions));
    }
    let q = site_ops::load_queue(&site_dir);
    let week = now().saturating_sub(7 * DAY);
    let recent: Vec<&QueueItem> = q.iter().filter(|i| i.updated_at >= week && i.status != "open").collect();
    md.push_str("## 本周运营\n\n");
    if recent.is_empty() {
        md.push_str("本周没有处理运营任务。\n");
    }
    for i in &recent {
        let st = match i.status.as_str() {
            "proposed" => "待确认",
            "deployed" => if i.auto { "已自动上线" } else { "已上线" },
            "merged" => "已合并",
            "dismissed" => "已驳回",
            "rolled_back" => "已回滚",
            "failed" => "失败",
            "done" => "已处理",
            _ => "处理中",
        };
        md.push_str(&format!("- [{st}] {}{}\n", i.opp.title, if i.review.is_empty() { String::new() } else { format!("（复盘：{}）", i.review_note) }));
    }
    let open = q.iter().filter(|i| i.status == "open").count();
    md.push_str(&format!("\n待处理机会 {open} 个；当前机会 {} 个。\n", d.opportunities.len()));
    write_report(&site_dir, &format!("{}-weekly", today()), &md)?;
    job.step("report", "ok", "");
    Ok("周报已生成".into())
}

/// 复盘结论：点击或点击率提升 ≥10% 为正面，点击下降 ≥15% 为负面。
/// 复盘的输入：这条改动上线时和 4 周后的数据。
pub struct ReviewInput<'a> {
    /// 看哪个指标：ctr / position / impressions / clicks / health
    pub metric: &'a str,
    /// 新页面（上线时还没有数据）
    pub is_new: bool,
    pub page_before: &'a crate::site_data::Row,
    pub page_after: &'a crate::site_data::Row,
    /// 目标查询词在这个页面上的表现（AI 写了预期查询词、两头都有数据时才用）
    pub query: &'a str,
    pub query_before: Option<&'a crate::site_data::Row>,
    pub query_after: Option<&'a crate::site_data::Row>,
    /// 全站近 28 天：扣掉整体变化（全站都跌时，单个页面跌得少不算改动的错）
    pub site_before: Option<&'a crate::site_data::Row>,
    pub site_after: &'a crate::site_data::Row,
    /// 新页面 4 周内带来展现的查询词个数
    pub new_queries: usize,
}

fn rel(a: f64, b: f64) -> f64 {
    if b > 0.0 {
        (a - b) / b
    } else if a > 0.0 {
        1.0
    } else {
        0.0
    }
}

fn pct(v: f64) -> String {
    // 取整后是 0 的写「持平」，不出现「-0%」「+0%」
    let p = (v * 100.0).round();
    if p == 0.0 { "持平".into() } else { format!("{}{p:.0}%", if p > 0.0 { "+" } else { "" }) }
}

/// 按这条改动的预期指标复盘，返回（结论 positive / neutral / negative，给人看的说明）。
/// 底线：页面点击扣掉全站变化后跌 15% 以上，不管看什么都是负面。
pub fn judge_expect(r: &ReviewInput) -> (&'static str, String) {
    let site_clicks = r.site_before.map(|s| rel(r.site_after.clicks, s.clicks)).unwrap_or(0.0);
    let site_impr = r.site_before.map(|s| rel(r.site_after.impressions, s.impressions)).unwrap_or(0.0);
    let site_note = if r.site_before.is_some() { format!("；全站点击同期 {}", pct(site_clicks)) } else { String::new() };
    let (pb, pa) = (r.page_before, r.page_after);
    if r.is_new {
        let n = format!("新页面 4 周：展现 {:.0}、点击 {:.0}、{} 个查询词", pa.impressions, pa.clicks, r.new_queries);
        return if pa.impressions >= 30.0 { ("positive", format!("看展现：{n}，已经被搜到")) } else if pa.impressions > 0.0 { ("neutral", format!("看展现：{n}，被搜到的还很少")) } else { ("neutral", "看展现：新页面 4 周内还没有搜索展现（新页面通常要更久，下次复盘再看）".into()) };
    }
    let adj_clicks = rel(pa.clicks, pb.clicks) - site_clicks;
    // 判负面要有足够的基数：改动前至少 20 次点击、100 次展现，不然 5 → 4 这种波动也会被当成下跌而回滚
    if pb.clicks >= 20.0 && pb.impressions >= 100.0 && adj_clicks <= -0.15 {
        return ("negative", format!("点击 {:.0} → {:.0}，扣掉全站变化后 {}，跌幅超过 15%{site_note}", pb.clicks, pa.clicks, pct(adj_clicks)));
    }
    if r.metric == "health" {
        return ("neutral", format!("看体检：技术修复不按搜索流量判断，体检里这个问题没再出现就算修好（点击 {:.0} → {:.0}）", pb.clicks, pa.clicks));
    }
    let (b, a, label) = match (r.query_before, r.query_after) {
        (Some(qb), Some(qa)) if !r.query.is_empty() => (qb, qa, format!("查询词「{}」", r.query)),
        _ => (pb, pa, "页面".to_string()),
    };
    if b.impressions < 30.0 && a.impressions < 30.0 {
        return ("neutral", format!("{label}的数据太少，还判断不了（展现 {:.0} → {:.0}）", b.impressions, a.impressions));
    }
    let (good, note) = match r.metric {
        "ctr" => (rel(a.ctr, b.ctr) >= 0.10, format!("看{label}点击率：{:.1}% → {:.1}%", b.ctr * 100.0, a.ctr * 100.0)),
        "position" => (b.position > 0.0 && a.position > 0.0 && b.position - a.position >= 1.0, format!("看{label}排名：{:.1} → {:.1}", b.position, a.position)),
        "impressions" => (rel(a.impressions, b.impressions) - site_impr >= 0.15, format!("看{label}展现：{:.0} → {:.0}（全站同期 {}）", b.impressions, a.impressions, pct(site_impr))),
        _ => (rel(a.clicks, b.clicks) - site_clicks >= 0.10, format!("看{label}点击：{:.0} → {:.0}，扣掉全站变化后 {}", b.clicks, a.clicks, pct(rel(a.clicks, b.clicks) - site_clicks))),
    };
    (if good { "positive" } else { "neutral" }, format!("{note}{site_note}"))
}

async fn review(ctx: &Ctx, site: &Site, job: &Job) -> Result<String, String> {
    let st = ctx.state();
    let site_dir = ctx.site_dir(&site.id);
    refresh_data(ctx, site, job).await;
    job.step("review", "running", "");
    let cache = crate::site_data::cache_dir(&site_dir);
    let rows: Vec<crate::site_data::Row> = crate::site_data::read_json(&cache, "gsc_pages.json");
    let by_page: HashMap<&str, &crate::site_data::Row> = rows.iter().map(|r| (r.key.as_str(), r)).collect();
    let qp: Vec<crate::site_data::Row> = crate::site_data::read_json(&cache, "gsc_query_page.json");
    let site_after = site_snapshot(&site_dir);
    let t = now();
    // 上线满 28 天（近 28 天窗口全部落在上线之后）才复盘；60 天后不再补。新页面上线时没有数据，也复盘。
    let items: Vec<QueueItem> = site_ops::load_queue(&site_dir)
        .into_iter()
        .filter(|i| i.status == "deployed" && i.review.is_empty() && (i.before.is_some() || !i.new_pages.is_empty() || i.opp.change == "new_content") && t.saturating_sub(i.deployed_at) >= 28 * DAY + 3 * DAY && t.saturating_sub(i.deployed_at) <= 60 * DAY)
        .collect();
    let mut state = load_state(&site_dir);
    let mut lines = Vec::new();
    let mut facts = Vec::new();
    let mut downgraded = site.ops.downgraded.clone();
    let mut downgraded_stamp = false;
    for i in &items {
        // 新页面按「有没有被搜到」判，不按点击涨跌；写新内容的任务就算填了已有页面网址也算新页面
        let is_new = i.before.is_none() || !i.new_pages.is_empty() || i.opp.change == "new_content";
        let page = if i.opp.page.starts_with("https://") { i.opp.page.clone() } else { i.new_pages.first().cloned().unwrap_or_default() };
        let before = i.before.clone().unwrap_or_default();
        let after = by_page.get(page.as_str()).map(|r| (*r).clone()).unwrap_or_default();
        let query = i.opp.expect_query.trim().to_string();
        let query_after = qp.iter().find(|r| !query.is_empty() && r.key.eq_ignore_ascii_case(&query) && r.key2 == page).cloned();
        let metric = if i.opp.expect_metric.is_empty() { crate::managed::default_metric_for(&i.opp.change).to_string() } else { i.opp.expect_metric.clone() };
        let (verdict, note) = judge_expect(&ReviewInput {
            metric: &metric,
            is_new,
            page_before: &before,
            page_after: &after,
            query: &query,
            query_before: i.before_query.as_ref(),
            query_after: query_after.as_ref(),
            site_before: i.before_site.as_ref(),
            site_after: &site_after,
            new_queries: qp.iter().filter(|r| r.key2 == page).count(),
        });
        let note = if i.opp.expect_note.is_empty() { note } else { format!("{note}（预期：{}）", i.opp.expect_note) };
        facts.push(format!("- {}｜{}｜{}", i.opp.title, match verdict { "positive" => "有效", "negative" => "负面", _ => "持平" }, note));
        // 降档按任务的改动类型记（和自动上线判定用的是同一套词：meta / content / new_content…）
        let kind = i.opp.change.clone();
        let neg = state.negatives.entry(kind.clone()).or_insert(0);
        if verdict == "negative" {
            *neg += 1;
            if *neg >= 2 && !kind.is_empty() && !downgraded.contains(&kind) {
                downgraded.push(kind.clone());
                downgraded_stamp = true;
                lines.push(format!("「{}」类改动连续负面，已在该站自动降到第 3 档", crate::site_ops::kind_label(&kind)));
            }
        } else {
            *neg = 0;
        }
        site_ops::update_item(&site_dir, &i.id, t, |x| {
            x.review = verdict.into();
            x.review_note = note.clone();
            x.review_metric = metric.clone();
            x.after = Some(after.clone());
        })?;
        lines.push(format!("{}：{}（{note}）", i.opp.title, match verdict { "positive" => "正面", "negative" => "负面", _ => "持平" }));
        // 全托管：效果负面的自动改动直接回滚，并记一笔。
        // 新页面不自动回滚：回滚等于把已经收录的页面删掉（404、没有跳转），比流量没涨糟得多
        if verdict == "negative" && site.ops.level >= 5 && i.auto && !is_new {
            match rollback(ctx, site, job, &i.id).await {
                Ok(_) => log_item(ctx, site, "rolled_back", i, &format!("效果复盘负面（{note}），已自动回滚")),
                Err(e) => log_item(ctx, site, "failed", i, &format!("效果负面，自动回滚失败：{e}")),
            }
        }
    }
    save_state(&site_dir, &state);
    if downgraded != site.ops.downgraded {
        st.sites.update(&site.id, t, |s| {
            s.ops.downgraded = downgraded.clone();
            if downgraded_stamp {
                s.ops.downgraded_at = now();
            }
            Ok(())
        })?;
    }
    if !lines.is_empty() {
        let mut md = format!("# {} 效果复盘 · {}\n\n{}\n", site.name, today(), lines.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n"));
        // 结论由 Pilot 按数据算；AI 只帮着解释原因（不改结论）
        if !facts.is_empty() {
            if let Some(why) = explain_review(ctx, site, &facts, &site_after).await {
                md.push_str(&format!("\n## 原因分析\n\n{why}\n"));
            }
        }
        write_report(&site_dir, &format!("{}-review", today()), &md)?;
    }
    job.step("review", "ok", &format!("复盘 {} 条", items.len()));
    Ok(format!("复盘 {} 条改动", items.len()))
}

/// 让 AI 解释这批复盘结果的原因（全站趋势、季节、算法更新、页面本身）。结论不变，失败就不写。
async fn explain_review(ctx: &Ctx, site: &Site, facts: &[String], site_after: &crate::site_data::Row) -> Option<String> {
    if over_budget(&budget(site, &ctx.site_dir(&site.id), &ctx.state().convos)).is_some() {
        return None;
    }
    let msg = format!(
        "【托管运营 · 效果复盘】你是「{}」（{}）的运营负责人。下面是 Pilot 按 Search Console 数据算出的复盘结论（上线 4 周后对比；已经扣掉全站的整体变化）。结论不用改，也不用读代码。\n请用中文写几句话解释原因：哪些可能和全站趋势、季节或搜索引擎更新有关，哪些更可能是改动本身起了作用或没起作用；对「持平」和「负面」的，各给一条下一步建议。不要编造这里没有的数据。\n\n全站近 28 天：点击 {:.0}、展现 {:.0}\n{}\n",
        site.name,
        site.domain,
        site_after.clicks,
        site_after.impressions,
        facts.join("\n")
    );
    let (_, conv) = run_conversation(ctx, site, "plan", "", msg).await.ok()?;
    conv.messages.iter().rev().find(|m| m.role == "assistant" && !m.error).map(|m| m.text.trim().to_string()).filter(|t| !t.is_empty())
}

// ---------------------------------------------------------------------------
// 全托管：AI 制定计划 / 上线后检查 / 每日摘要
// ---------------------------------------------------------------------------

/// 给计划会话的数据摘要（直接写进提示，不靠工具：计划会话是只读的「计划」档）。
fn data_brief(site_dir: &Path) -> String {
    let d = crate::site_data::dashboard(site_dir);
    let mut s = String::new();
    if d.meta.gsc.fetched_at > 0 {
        s.push_str(&format!(
            "Search Console（{} ~ {}）：点击 {:.0}（前 28 天 {:.0}），展现 {:.0}，平均排名 {:.1}。\n头部查询词：\n",
            d.meta.gsc.start, d.meta.gsc.end, d.gsc_totals.clicks, d.gsc_prev_totals.clicks, d.gsc_totals.impressions, d.gsc_totals.position
        ));
        for q in d.gsc_queries.iter().take(25) {
            s.push_str(&format!("- 「{}」点击 {:.0}（前 {:.0}）展现 {:.0} 排名 {:.1}\n", q.row.key, q.row.clicks, q.prev_clicks, q.row.impressions, q.row.position));
        }
        s.push_str("头部页面：\n");
        for p in d.gsc_pages.iter().take(20) {
            s.push_str(&format!("- {} 点击 {:.0}（前 {:.0}）展现 {:.0} 排名 {:.1}\n", path_of(&p.row.key), p.row.clicks, p.prev_clicks, p.row.impressions, p.row.position));
        }
    }
    if d.meta.ga4.fetched_at > 0 {
        s.push_str(&format!("GA4（近 28 天）：浏览 {:.0}（前 28 天 {:.0}），会话 {:.0}。\n", d.ga4_totals.views, d.ga4_prev_views, d.ga4_totals.sessions));
        let ev: Vec<String> = d.ga4_events.iter().filter(|e| e.dim.is_empty() && !["page_view", "session_start", "first_visit", "user_engagement", "scroll"].contains(&e.event.as_str())).map(|e| format!("{} {:.0}", e.event, e.count)).collect();
        if !ev.is_empty() {
            s.push_str(&format!("转化事件：{}\n", ev.join("，")));
        }
    }
    if !d.opportunities.is_empty() {
        s.push_str("Pilot 规则算出的机会：\n");
        for o in d.opportunities.iter().take(12) {
            s.push_str(&format!("- [{}] {}：{}\n", crate::site_ops::kind_label(&o.change), o.title, o.evidence));
        }
    }
    if s.is_empty() {
        s.push_str("（还没有接入数据，先从站点内容本身找改进点：标题描述、内链、内容缺口、技术问题。）\n");
    }
    s
}

/// 每周：AI 当运营负责人，读数据和代码，写出本周计划；计划里的每一项进入队列，由执行层逐个处理。
async fn plan(ctx: &Ctx, site: &Site, job: &Job) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    if let Some(why) = over_budget(&budget(site, &site_dir, &ctx.state().convos)) {
        return Err(format!("{why}，这周先不制定计划"));
    }
    refresh_data(ctx, site, job).await;
    job.step("plan", "running", "");
    let q = site_ops::load_queue(&site_dir);
    let history: Vec<String> = q
        .iter()
        .filter(|i| !matches!(i.status.as_str(), "open"))
        .take(20)
        // 写上实际改了几个文件：标题说批量、其实只改了几个的，计划 AI 才知道还没做完，可以再排一条接着做
        .map(|i| format!("- [{}] {}{}{}", i.status, i.opp.title, if i.files.is_empty() { String::new() } else { format!("（实际改了 {} 个文件）", i.files.len()) }, if i.review.is_empty() { String::new() } else { format!("（复盘：{}）", i.review) }))
        .collect();
    // 已经排队、还没做的（上周计划剩下的、数据规则找出的）：计划 AI 看得到才不会把同一件事再排一次
    let queued: Vec<String> = q.iter().filter(|i| i.status == "open").take(20).map(|i| format!("- {}", i.opp.title)).collect();
    let rules = group_rules(ctx, site);
    let n = rules.as_ref().map(|g| g.per_round).unwrap_or(site.ops.budget.items_per_run).clamp(1, 8) * 2;
    let plan_brief = rules.as_ref().map(|g| g.brief()).unwrap_or_default();
    let kinds: Vec<&str> = crate::managed::ACTIONS.iter().copied().filter(|a| rules.as_ref().is_none_or(|g| g.allows(a))).collect();
    let stage = crate::site_stage::stage_of(site, &site_dir);
    let stage_brief = crate::site_stage::plan_brief(site, &site_dir, rules.as_ref().is_some_and(|g| !g.topic_list().is_empty()));
    let health = crate::site_stage::latest_health(&site_dir, if stage.stage == "new" { 2000 } else { 600 });
    let health = if health.is_empty() { String::new() } else { format!("【最近一次体检】\n{health}\n\n") };
    let msg = format!(
        "【托管运营 · 本周计划】你是「{name}」（{domain}）的运营负责人。下面是这个站点的阶段与定位、所在托管方案的运营方式、Pilot 拉到的数据和最近的运营记录；请先读站点的代码和主要页面，然后制定本周最多 {n} 项具体工作。\n读站点要省：先看目录结构、导航和首页，再挑流量最大或这周要动的几个页面看，一共读 15 个文件左右就够，不要逐篇通读所有文章；需要更细的数据（某个页面的查询词、某类查询词）用站点数据工具查，不要靠读文件猜。\n\n{stage_brief}{plan_brief}{health}【数据】\n{brief}\n【最近的运营记录】\n{hist}\n\n要求：\n- 按站点阶段确定本周的重点；只安排「允许的动作」里的事；围绕运营目标；新内容遵守内容准则。\n- 每一项都要落到具体页面或具体内容上，说清为什么（引用上面的数据）；优先做投入小、把握大的。\n- 可以包括规则没发现的事：内容缺口、专题页、内链结构、过时内容更新、技术问题。\n- 长尾选题先对照已有页面：能扩写已有页面就不新开页，意图相近的几个词合并成一页；不要安排用同一模板换关键词批量做的相似页面，也不要安排撑不起一个完整页面的窄题（页面厚度见上）。\n- 读站点时如果发现公开页面混入了内部说明、开发注释、占位图，或有明显偏薄的页面，可以安排「改进已有页面」任务处理。\n- 不要改推荐链接、邀请码、本站自己的价格和优惠、法律页面、依赖或部署配置。文章里引用的第三方工具价格可以更新，但要标明核对日期和来源。\n- 这是无人值守任务，没人回答问题：规则之间有冲突时（比如阶段建议和运营目标不一致），以「允许的动作」和运营目标为准，自己做出判断，把判断和需要运营人员留意的事写进 notes。\n- 这一轮只出计划，不要修改任何文件。\n最后用一个 ```json 代码块输出：{{\"notes\":\"给运营人员的说明（中文）：冲突怎么处理的、需要留意或确认的事（没有就留空）\",\"positioning\":\"站点定位，200 字以内：主题、目标读者、语言/地区、已有栏目和内容类型、商业目标\",\"tasks\":[{{\"title\":\"一句话说做什么\",\"page\":\"完整网址（新页面留空）\",\"change\":\"{kinds}\",\"why\":\"依据：引用的数据，或它补的是定位里的哪一块\",\"source\":\"data|gap|topic|positioning|health\",\"query\":\"目标查询词（没有就留空）\",\"priority\":1,\"expect\":{{\"metric\":\"ctr|position|impressions|clicks|health\",\"query\":\"看哪个查询词（看整个页面就留空）\",\"note\":\"一句话写预期，比如：点击率从 2.2% 提到 3% 以上\"}}}}]}}\nexpect 是这条改动预期影响的指标：改标题和描述一般看点击率（ctr），补内容看目标查询词的排名（position），新页面看展现（impressions），修技术问题看体检（health）；按这条任务的实际意图选，上线 4 周后 Pilot 会拿数据对比。\nsource 的含义：data＝数据里看到的问题，gap＝数据里有需求但没有页面，topic＝选题清单，positioning＝根据站点定位推导，health＝体检发现的问题。",
        name = site.name,
        domain = site.domain,
        brief = data_brief(&site_dir),
        hist = if history.is_empty() { "（暂无）".to_string() } else { format!("{}{}", history.join("\n"), if queued.is_empty() { String::new() } else { format!("\n\n已经排队、还没做的（不要再安排同样的事）：\n{}", queued.join("\n")) }) },
        kinds = kinds.join("|"),
    );
    job.line(&format!(
        "交给 AI：先读站点（首页、导航、栏目、主要页面）和数据，再按「{}」阶段制定本周计划{}",
        crate::site_stage::label(&stage.stage),
        rules.as_ref().map(|g| format!("，方案「{}」", g.name)).unwrap_or_default()
    ));
    let (conv_id, conv) = run_conversation(ctx, site, "plan", "", msg).await?;
    let reply = conv.messages.iter().rev().find(|m| m.role == "assistant").map(|m| m.text.clone()).unwrap_or_default();
    let (positioning, mut tasks, dropped) = crate::managed::parse_plan_counted(&reply)?;
    if dropped > 0 {
        job.line(&format!("计划里有 {dropped} 条没用上（格式写坏了、没写标题，或超过一次最多 12 条）"));
    }
    let notes = crate::managed::parse_plan_notes(&reply);
    if !notes.is_empty() {
        job.line(&format!("给你的说明：{notes}"));
    }
    crate::site_stage::update_from_ai(&site_dir, &positioning);
    // 页面网址规范化：相对路径补上域名；不是本站的清掉（验收、分支名、快照都按它）
    for t in tasks.iter_mut() {
        let p = t.page.trim().to_string();
        t.page = if p.starts_with('/') {
            format!("https://{}{p}", site.domain)
        } else if p.starts_with("http") && !p.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").starts_with(site.domain.trim_start_matches("www.")) {
            String::new()
        } else if let Some(rest) = p.strip_prefix("http://") {
            format!("https://{rest}")
        } else {
            p
        };
    }
    // 模型偶尔会安排方案不允许的动作：直接丢掉
    if let Some(g) = &rules {
        let before = tasks.len();
        tasks.retain(|t| g.allows(&t.change));
        if tasks.len() < before {
            job.line(&format!("去掉 {} 项方案不允许的动作", before - tasks.len()));
        }
    }
    if !positioning.trim().is_empty() {
        job.line(&format!("站点定位：{}", positioning.trim().chars().take(80).collect::<String>()));
    }
    job.line(&format!("计划 {} 项：", tasks.len()));
    for t in &tasks {
        job.line(&format!("· [{}] {}", crate::managed::action_label(&t.change), t.title));
    }
    let opps: Vec<Opportunity> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| Opportunity {
            // key 不带周号：驳回过、失败过的同一件事下周不会因为换了周号又排进来（冷却期照样生效）。
            // 用哈希不用 slug：中文标题 slug 会塌成空串，几条任务共用一个 key 就只剩一条
            key: format!("plan:{}", site_ops::stable_key(&format!("{}|{}", t.page.trim().trim_end_matches('/'), t.title.trim()))),
            kind: "plan".into(),
            title: t.title.clone(),
            page: t.page.clone(),
            evidence: t.why.clone(),
            change: t.change.clone(),
            source: t.source.clone(),
            target: t.query.clone(),
            expect_metric: if crate::managed::EXPECT_METRICS.contains(&t.expect.metric.as_str()) { t.expect.metric.clone() } else { String::new() },
            expect_query: t.expect.query.trim().to_string(),
            expect_note: t.expect.note.trim().to_string(),
            score: 1000.0 - (t.priority.max(1) as f64) * 10.0 - i as f64,
            ..Default::default()
        })
        .collect();
    // 重新制定计划：上一份计划里还没动过的条目撤掉，换成这一份（AI 看得到它们，还该做的会重新列进来；已在处理、已上线的保留）
    let replaced = site_ops::drop_open_where(&site_dir, |i| i.opp.kind == "plan")?;
    if replaced > 0 {
        job.line(&format!("替换上一份计划里还没处理的 {replaced} 项"));
    }
    let added = site_ops::enqueue(&site_dir, &opps, now(), COOLDOWN)?;
    let pdir = site_dir.join("plans");
    let _ = std::fs::create_dir_all(&pdir);
    let _ = std::fs::write(pdir.join(format!("{}.json", today())), serde_json::to_vec_pretty(&serde_json::json!({ "date": today(), "conv_id": conv_id, "brain": conv.brain, "model": conv.model, "stage": stage.stage, "notes": notes, "tasks": tasks })).unwrap_or_default());
    log(ctx, site, "planned", &format!("本周计划 {} 项", tasks.len()), &tasks.iter().take(5).map(|t| t.title.clone()).collect::<Vec<_>>().join("；"), "");
    job.step("plan", "ok", &format!("{} 项，新入队 {}", tasks.len(), added.len()));
    ctx.emit_queue(&site.id);
    Ok(format!("本周计划 {} 项", tasks.len()))
}

/// 计划文件所在目录：plans/<日期>.json；清除的计划挪到 plans/cleared/ 留底（不再算作最近一次计划）。
pub fn plans_dir(site_dir: &Path) -> PathBuf {
    site_dir.join("plans")
}

/// 最近一次计划（托管控制台「计划」页）。
pub fn latest_plan(site_dir: &Path) -> Option<serde_json::Value> {
    let d = site_dir.join("plans");
    let mut names: Vec<String> = std::fs::read_dir(&d).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".json")).collect();
    names.sort();
    let raw = std::fs::read(d.join(names.last()?)).ok()?;
    serde_json::from_slice(&raw).ok()
}


/// 这条改动到了上线后复查的时间没有：1 小时后一次、24 小时后再一次。
fn watch_is_due(i: &QueueItem) -> bool {
    i.status == "deployed" && i.watch_due > 0 && now() >= i.watch_due
}

/// 上线后复查（1 小时后、24 小时后）：再做一遍线上验收。页面打不开就自动回滚；
/// 之前待核对的问题好了就清掉，24 小时后还在，全自动方案就开一条后续任务。
async fn watch(ctx: &Ctx, site: &Site, job: &Job) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let due: Vec<QueueItem> = site_ops::load_queue(&site_dir).into_iter().filter(watch_is_due).collect();
    job.step("watch", "running", &format!("{} 条", due.len()));
    let mut out = Vec::new();
    // 停止托管的站点、不在方案里的站点也复查，但不替人回滚
    let auto_plan = group_rules(ctx, site).is_some_and(|g| g.autonomy == "auto" && g.active_for(&site.id));
    for i in due {
        // 和上线时一样多等几次（最多 5 分钟多）：复查撞上 CDN / 源站抖动不能把当天上线的全回滚。
        // 复查不再对照「这次加的文字」：隔了几小时别的改动可能已经改掉那段话，上线时查过一次就够了
        let mut spec = spec_for(site, &i, false);
        spec.snippets.clear();
        spec.before = load_before(&site_dir);
        spec.only_new = true;
        let rep = crate::site_verify::run(&spec, job, true).await;
        let stage = i.watch_stage;
        let when = if stage == 0 { "上线 1 小时后" } else { "上线 24 小时后" };
        if !rep.hard.is_empty() {
            // 上线之后域名被别的东西抢走了：不是这条改动的问题，不回滚；隔几小时再查，域名处理好了接着走复查
            if let Some(why) = crate::launch_cmds::domain_taken(&ctx.state().conns, site, &rep).await {
                pause_for_domain(ctx, site, &why);
                site_ops::update_item(&site_dir, &i.id, now(), |x| {
                    x.watched = why.clone();
                    x.checks = rep.checks.clone();
                    x.watch_due = now() + 6 * 3600;
                })?;
                out.push(format!("{} 没复查成：{why}", i.opp.title));
                continue;
            }
            let failure = rep.hard.join("；");
            site_ops::update_item(&site_dir, &i.id, now(), |x| {
                x.watched = failure.clone();
                x.checks = rep.checks.clone();
            })?;
            // 首页都打不开是整站的事，不是这条改动的问题；「改好等你确认」的方案是你亲手上线的，也不替你回滚：
            // 记下来、1 小时后再查一次；第二次还不行就不再查，写进待核对交给你（只通知这两次，不每小时刷屏）
            let site_down = rep.home_down;
            if site_down || !auto_plan {
                let failures = i.watch_failures + 1;
                let last = failures >= 2;
                site_ops::update_item(&site_dir, &i.id, now(), |x| {
                    x.watch_failures = failures;
                    x.watch_due = if last { 0 } else { now() + 3600 };
                    if last {
                        let add = format!("上线后复查两次没通过：{failure}");
                        x.attention = if x.attention.is_empty() { add } else { format!("{}；{add}", x.attention) };
                    }
                })?;
                let why = match (site_down, last) {
                    (true, false) => "首页也打不开，像是整站或 CDN 的问题，没有回滚；1 小时后再查",
                    (true, true) => "首页还是打不开，没有回滚；不再自动查了，请你看一下",
                    (false, false) => "这个方案不是全自动，没有替你回滚；1 小时后再查",
                    (false, true) => "这个方案不是全自动，没有替你回滚；不再自动查了，请你看一下",
                };
                log_item(ctx, site, "watch_failed", &i, &format!("{when}复查：{failure}。{why}"));
                ctx.notify(&format!("{} · 上线后复查没通过", site.name), &format!("{}：{failure}（{why}）", i.opp.title));
                out.push(format!("{} 复查没通过（{why}）", i.opp.title));
                continue;
            }
            // 复查时坏了：同样自动回滚（回滚也验收），回滚失败就暂停托管
            let r = on_verify_failed(ctx, site, job, &i.id, &format!("{}{when}复查：{failure}", crate::site_verify::HARD_PREFIX)).await;
            out.push(format!("{} {}", i.opp.title, if r.is_ok() { "复查失败，已回滚并让 AI 重做" } else { "复查失败，已回滚" }));
            continue;
        }
        let attention = rep.soft.join("；");
        let next_stage = stage + 1;
        site_ops::update_item(&site_dir, &i.id, now(), |x| {
            x.watched = if attention.is_empty() { "ok".into() } else { attention.clone() };
            x.checks = rep.checks.clone();
            x.attention = attention.clone();
            x.watch_stage = next_stage;
            x.watch_due = if next_stage == 1 { x.deployed_at.max(now().saturating_sub(DAY)) + DAY } else { 0 };
        })?;
        if attention.is_empty() {
            log_item(ctx, site, "watch_ok", &i, &format!("{when}复查：页面正常"));
            out.push(format!("{} 正常", i.opp.title));
            // 之前因为待核对开出来、还没动的修复任务：问题已经没了，撤掉
            dismiss_followups(&site_dir, &i.id, "复查时页面已经正常，不用修");
        } else {
            log_item(ctx, site, "attention", &i, &format!("{when}复查：网站正常，{} 项待核对：{attention}", rep.soft.len()));
            follow_up(ctx, site, &i, &attention);
            out.push(format!("{} 有 {} 项待核对", i.opp.title, rep.soft.len()));
        }
    }
    ctx.emit_queue(&site.id);
    job.step("watch", "ok", "");
    Ok(out.join("；"))
}

/// 日报里的一条「需要你」。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DigestNeed {
    pub site_id: String,
    pub site_name: String,
    pub title: String,
    /// 待你确认 | 失败了 | 上线后待核对
    pub why: String,
}

/// 日报里有动静的一个站点，和它过去 24 小时的事。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DigestSite {
    pub site_id: String,
    pub site_name: String,
    pub events: Vec<DigestEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DigestEvent {
    /// shipped | planned | proposed | rolled_back | failed | watch_failed | fallback | budget
    pub kind: String,
    pub label: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
}

/// 一个站点近 28 天的流量（和前 28 天比）。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DigestTraffic {
    pub site_id: String,
    pub site_name: String,
    pub clicks: f64,
    pub clicks_prev: f64,
    pub views: f64,
    pub views_prev: f64,
}

/// 一份日报的结构化内容（界面按它画；同时写一份 Markdown 备查）。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DigestData {
    pub date: String,
    pub generated_at: u64,
    /// 这份日报从什么时候算起（上一份日报生成的时间；没有上一份就是 24 小时前）。旧日报没有这个字段
    #[serde(default)]
    pub since: u64,
    pub sites_total: usize,
    pub shipped: usize,
    pub planned: usize,
    /// 回滚 + 失败 + 上线后检查失败
    pub broken: usize,
    pub needs: Vec<DigestNeed>,
    pub active: Vec<DigestSite>,
    /// 这段时间里没有动静、也不需要你的站点
    pub quiet: Vec<String>,
    /// 有点击或浏览的站点，按变化大小排
    pub traffic: Vec<DigestTraffic>,
    /// 接了数据、但前后 28 天都没有点击和浏览的站点
    pub no_traffic: Vec<String>,
}

/// 四舍五入成整数，并去掉「-0」。
fn whole(x: f64) -> f64 {
    let r = x.round();
    if r == 0.0 { 0.0 } else { r }
}

fn event_label(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "shipped" => "已上线",
        "planned" => "出计划",
        "proposed" => "待你确认",
        "rolled_back" => "已回滚",
        "failed" => "失败",
        "watch_failed" => "上线后检查失败",
        "fallback" => "换用备用模型",
        "budget" => "预算",
        "paused" => "暂停托管",
        "downgraded" => "降档",
        _ => return None,
    })
}

fn need_why(i: &QueueItem) -> &'static str {
    match i.status.as_str() {
        "proposed" => "待你确认",
        "failed" => "失败了",
        _ => "上线后待核对",
    }
}

/// 汇总过去 24 小时：数字、需要你的事、有动静的站点、没动静的站点、流量。
pub fn build_digest(sites: &[(Site, PathBuf)], acts: &[crate::managed::Activity], date: &str) -> DigestData {
    let count = |k: &[&str]| acts.iter().filter(|a| k.contains(&a.kind.as_str())).count();
    let mut d = DigestData {
        date: date.into(),
        generated_at: now(),
        sites_total: sites.len(),
        shipped: count(&["shipped"]),
        planned: count(&["planned"]),
        // 复查没通过只是明细（回滚了会另记 rolled_back），不单独算
        broken: count(&["rolled_back", "failed"]),
        ..Default::default()
    };
    for (s, dir) in sites {
        // 只列最近 7 天有动静的；老的折成一条「另有 N 项超过一周没处理」，不然同一批条目每天重复推
        let q = site_ops::load_queue(dir);
        let (recent, old): (Vec<&QueueItem>, Vec<&QueueItem>) = q.iter().filter(|i| site_ops::needs_you(i)).partition(|i| i.updated_at + 7 * DAY > now());
        let mut waiting: Vec<DigestNeed> = recent.iter().map(|i| DigestNeed { site_id: s.id.clone(), site_name: s.name.clone(), title: i.opp.title.clone(), why: need_why(i).into() }).collect();
        if !old.is_empty() {
            waiting.push(DigestNeed { site_id: s.id.clone(), site_name: s.name.clone(), title: format!("另有 {} 项超过一周没处理", old.len()), why: "待你处理".into() });
        }
        let events: Vec<DigestEvent> = acts
            .iter()
            .filter(|a| a.site_id == s.id)
            .filter_map(|a| event_label(&a.kind).map(|l| DigestEvent { kind: a.kind.clone(), label: l.into(), title: a.title.clone(), detail: a.detail.clone() }))
            .collect();
        if events.is_empty() && waiting.is_empty() {
            d.quiet.push(s.name.clone());
        } else if !events.is_empty() {
            d.active.push(DigestSite { site_id: s.id.clone(), site_name: s.name.clone(), events });
        }
        d.needs.extend(waiting);
        let sum = crate::site_data::summary(dir);
        if sum.fetched_at > 0 {
            let t = DigestTraffic {
                site_id: s.id.clone(),
                site_name: s.name.clone(),
                clicks: whole(sum.gsc_clicks),
                clicks_prev: whole(sum.gsc_clicks_prev),
                views: whole(sum.ga4_views),
                views_prev: whole(sum.ga4_views_prev),
            };
            if t.clicks > 0.0 || t.clicks_prev > 0.0 || t.views > 0.0 || t.views_prev > 0.0 {
                d.traffic.push(t);
            } else {
                d.no_traffic.push(s.name.clone());
            }
        }
    }
    let swing = |t: &DigestTraffic| (t.clicks - t.clicks_prev).abs() + (t.views - t.views_prev).abs() / 10.0;
    d.traffic.sort_by(|a, b| swing(b).partial_cmp(&swing(a)).unwrap_or(std::cmp::Ordering::Equal).then(b.clicks.partial_cmp(&a.clicks).unwrap_or(std::cmp::Ordering::Equal)));
    d
}

fn change_text(now: f64, prev: f64) -> String {
    // 先取整再比：差 0.3 不写成「+0」，变化不到 1% 只写差值不写「+0%」
    let diff = (now - prev).round();
    if diff == 0.0 {
        "持平".into()
    } else if prev == 0.0 {
        format!("+{diff:.0}（新增）")
    } else {
        let sign = if diff > 0.0 { "+" } else { "" };
        let p = (diff / prev * 100.0).round();
        if p == 0.0 { format!("{sign}{diff:.0}") } else { format!("{sign}{diff:.0}（{sign}{p:.0}%）") }
    }
}

/// 「过去 24 小时」「过去 30 小时」「过去 3 天」：日报覆盖的时段（从上一份日报算起）。
fn digest_window(d: &DigestData) -> String {
    let h = if d.since == 0 { 24 } else { (d.generated_at.saturating_sub(d.since) + 1800) / 3600 };
    if (22..=26).contains(&h) {
        "过去 24 小时".into()
    } else if h >= 48 {
        format!("过去 {} 天", (h + 12) / 24)
    } else {
        format!("过去 {} 小时", h.max(1))
    }
}

/// 日报的 Markdown 版（存档备查；界面用结构化数据画）。
pub fn digest_markdown(d: &DigestData) -> String {
    let mut md = format!("# 托管日报 · {}\n\n托管 {} 个站点；{}自动上线 {} 项、出计划 {} 次、回滚或失败 {} 项，{} 项需要你。\n\n", d.date, d.sites_total, digest_window(d), d.shipped, d.planned, d.broken, d.needs.len());
    if !d.needs.is_empty() {
        md.push_str("## 需要你\n\n");
        for n in &d.needs {
            md.push_str(&format!("- {} · {}：{}\n", n.site_name, n.why, n.title));
        }
        md.push('\n');
    }
    if !d.active.is_empty() {
        md.push_str("## 今天有动静的站点\n\n");
        for s in &d.active {
            md.push_str(&format!("### {}\n\n", s.site_name));
            for e in &s.events {
                md.push_str(&format!("- [{}] {}{}\n", e.label, e.title, if e.detail.is_empty() { String::new() } else { format!("：{}", e.detail) }));
            }
            md.push('\n');
        }
    }
    if !d.quiet.is_empty() {
        md.push_str(&format!("其余 {} 个站点今天没有变化：{}\n\n", d.quiet.len(), d.quiet.join("、")));
    }
    if !d.traffic.is_empty() {
        md.push_str("## 流量（近 28 天，和前 28 天比）\n\n| 站点 | Google 点击 | 变化 | 浏览量 | 变化 |\n|---|---|---|---|---|\n");
        for t in &d.traffic {
            md.push_str(&format!("| {} | {:.0} | {} | {:.0} | {} |\n", t.site_name, t.clicks, change_text(t.clicks, t.clicks_prev), t.views, change_text(t.views, t.views_prev)));
        }
        md.push('\n');
    }
    if !d.no_traffic.is_empty() {
        md.push_str(&format!("这些站点还没有点击和浏览：{}\n", d.no_traffic.join("、")));
    }
    md
}

/// 把改版前存下的 Markdown 日报读回结构化内容，让旧日报也按新样子显示。
/// 旧格式：每个站点一个「## 站点」，下面是点击数、「[计划] 标题：详情」这类事件和「需要你：N 项（…）」。
/// 站点 id 旧格式里没有，由调用方按名字补。
pub fn parse_old_digest(md: &str) -> Option<DigestData> {
    let date = md.lines().next()?.strip_prefix("# 托管日报 · ")?.trim().to_string();
    let mut d = DigestData { date, ..Default::default() };
    let num = |s: &str| -> f64 { whole(s.trim().parse::<f64>().unwrap_or(0.0)) };
    let count_before = |line: &str, unit: &str, after: &str| -> usize {
        line.split_once(after).and_then(|(_, r)| r.split_once(unit)).and_then(|(n, _)| n.trim().parse().ok()).unwrap_or(0)
    };
    let mut cur: Option<(String, Vec<DigestEvent>, Option<DigestTraffic>)> = None;
    let flush = |d: &mut DigestData, cur: Option<(String, Vec<DigestEvent>, Option<DigestTraffic>)>| {
        let Some((name, events, traffic)) = cur else { return };
        let busy = !events.is_empty() || d.needs.iter().any(|n| n.site_name == name);
        if !events.is_empty() {
            d.active.push(DigestSite { site_id: String::new(), site_name: name.clone(), events });
        } else if !busy {
            d.quiet.push(name.clone());
        }
        if let Some(t) = traffic {
            if t.clicks > 0.0 || t.clicks_prev > 0.0 {
                d.traffic.push(t);
            } else {
                d.no_traffic.push(name);
            }
        }
    };
    for line in md.lines().skip(1) {
        let line = line.trim();
        if line.starts_with("托管 ") {
            d.sites_total = line.strip_prefix("托管 ").and_then(|r| r.split_once(" 个站点")).and_then(|(n, _)| n.trim().parse().ok()).unwrap_or(0);
            d.shipped = count_before(line, "项", "自动上线");
            d.planned = count_before(line, "次", "出计划");
            d.broken = count_before(line, "项", "回滚");
        } else if let Some(name) = line.strip_prefix("## ") {
            flush(&mut d, cur.take());
            cur = Some((name.trim().to_string(), Vec::new(), None));
        } else if let (Some(item), Some((name, events, traffic))) = (line.strip_prefix("- "), cur.as_mut()) {
            if let Some(r) = item.strip_prefix("近 28 天 Google 点击 ") {
                let (now_s, prev_s) = r.split_once("（前 28 天 ").unwrap_or((r, "0"));
                *traffic = Some(DigestTraffic { site_name: name.clone(), clicks: num(now_s), clicks_prev: num(prev_s.trim_end_matches('）')), ..Default::default() });
            } else if let Some(r) = item.strip_prefix("需要你：") {
                let titles = r.split_once('（').map(|(_, t)| t.trim_end_matches('）')).unwrap_or("");
                for t in titles.split('；').map(str::trim).filter(|t| !t.is_empty()) {
                    d.needs.push(DigestNeed { site_name: name.clone(), title: t.into(), why: "需要你".into(), ..Default::default() });
                }
            } else if let Some((label, rest)) = item.strip_prefix('[').and_then(|r| r.split_once("] ")) {
                let kind = match label {
                    "已上线" => "shipped",
                    "计划" | "出计划" => "planned",
                    "待你确认" => "proposed",
                    "已回滚" => "rolled_back",
                    "失败" => "failed",
                    "上线后检查失败" => "watch_failed",
                    "换用备用模型" => "fallback",
                    "预算" => "budget",
                    _ => continue,
                };
                let (title, detail) = rest.split_once('：').unwrap_or((rest, ""));
                events.push(DigestEvent { kind: kind.into(), label: event_label(kind).unwrap_or(label).into(), title: title.into(), detail: detail.into() });
            }
        }
    }
    flush(&mut d, cur.take());
    let swing = |t: &DigestTraffic| (t.clicks - t.clicks_prev).abs();
    d.traffic.sort_by(|a, b| swing(b).partial_cmp(&swing(a)).unwrap_or(std::cmp::Ordering::Equal).then(b.clicks.partial_cmp(&a.clicks).unwrap_or(std::cmp::Ordering::Equal)));
    Some(d)
}

/// 每日摘要：所有托管站点过去 24 小时做了什么、什么需要你。写一份报告（结构化 + Markdown）并发系统通知。
/// 日报的起点：今天之前最近一份日报的生成时间，夹在 7 天前到 1 小时前之间；没有就是 24 小时前。
fn digest_since(dir: &Path, today: &str, now: u64) -> u64 {
    let prev = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let date = name.strip_suffix(".json")?.to_string();
            (date.as_str() < today).then_some(date)
        })
        .max()
        .and_then(|date| std::fs::read(dir.join(format!("{date}.json"))).ok())
        .and_then(|b| serde_json::from_slice::<DigestData>(&b).ok())
        .map(|d| d.generated_at)
        .filter(|t| *t > 0);
    match prev {
        Some(t) => t.clamp(now.saturating_sub(7 * DAY), now.saturating_sub(3600)),
        None => now.saturating_sub(DAY),
    }
}

pub fn digest(app: &tauri::AppHandle) -> Result<String, String> {
    let st = app.state::<crate::AppState>();
    let d = crate::managed::digests_dir(&st.data_dir);
    // 从上一份日报接着算：客户端隔天晚开、日报时间改过，都不会漏掉或重复中间的事（最多往前看 7 天）
    let since = digest_since(&d, &today(), now());
    let acts: Vec<crate::managed::Activity> = crate::managed::activity(&st.data_dir, 5000).into_iter().filter(|a| a.ts >= since).collect();
    let cfg = crate::managed::load(&st.data_dir);
    let managed: Vec<(Site, PathBuf)> = st.sites.list().into_iter().filter(|s| cfg.group_of(&s.id).is_some()).map(|s| { let d = st.sites.data_dir(&s.id); (s, d) }).collect();
    let mut data = build_digest(&managed, &acts, &today());
    data.since = since;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(d.join(format!("{}.md", data.date)), digest_markdown(&data)).map_err(|e| e.to_string())?;
    std::fs::write(d.join(format!("{}.json", data.date)), serde_json::to_vec_pretty(&data).unwrap_or_default()).map_err(|e| e.to_string())?;
    let body = format!("{} 个站点 · AI 上线 {} 项 · {} 项需要你", data.sites_total, data.shipped, data.needs.len());
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = app.notification().builder().title("托管日报").body(&body).show();
    }
    // 手机上看到的日报多带几行：要你处理的事（Telegram，见 push）
    let needs: Vec<String> = data.needs.iter().take(8).map(|n| format!("· {}：{}", n.site_name, n.title)).collect();
    crate::push::send(app, format!("托管日报 {}\n{body}{}{}", data.date, if needs.is_empty() { String::new() } else { format!("\n\n需要你：\n{}", needs.join("\n")) }, if data.needs.len() > 8 { format!("\n…还有 {} 项", data.needs.len() - 8) } else { String::new() }));
    crate::managed::log(&st.data_dir, crate::managed::Activity { ts: now(), kind: "digest".into(), title: "托管日报".into(), detail: body.clone(), ..Default::default() });
    Ok(body)
}

// ---------------------------------------------------------------------------
// 调度：客户端在运行（托盘常驻）时每 10 分钟看一次；启动后先补跑错过的任务。
// ---------------------------------------------------------------------------

/// 每天几点自动同步站点数据（本地时间）。
pub const DATA_SYNC_HOUR: u8 = 7;

fn data_sync_path(data_dir: &Path) -> PathBuf {
    data_dir.join("data_sync.json")
}

fn last_data_sync(data_dir: &Path) -> String {
    std::fs::read(data_sync_path(data_dir)).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).and_then(|v| v["date"].as_str().map(str::to_string)).unwrap_or_default()
}

fn set_last_data_sync(data_dir: &Path, date: &str) {
    let _ = std::fs::write(data_sync_path(data_dir), serde_json::json!({ "date": date, "at": now() }).to_string());
}

/// 上次同步所有站点数据的时间（秒；没同步过为 0）
pub fn last_data_sync_at(data_dir: &Path) -> u64 {
    std::fs::read(data_sync_path(data_dir)).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).and_then(|v| v["at"].as_u64()).unwrap_or(0)
}

static DATA_SYNCING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn data_syncing() -> bool {
    DATA_SYNCING.load(std::sync::atomic::Ordering::SeqCst)
}

async fn retry_failed_data(app: tauri::AppHandle) {
    use tauri::Manager;
    let st = app.state::<crate::AppState>();
    for site in st.sites.list() {
        let dir = st.sites.data_dir(&site.id);
        if crate::site_data::retry_due(&crate::site_data::load_meta(&dir), now()) {
            crate::site_data::refresh(&site, &st.conns, &st.sites, &dir, false).await;
            let _ = app.emit("site-data", serde_json::json!({ "site_id": site.id }));
        }
    }
}

async fn daily_data_sync(ctx: Ctx) {
    sync_all_data(ctx.app.clone()).await;
}

/// 一个一个站点拉取数据（不占站点作业，不和构建、运营任务抢）；每个站点拉完发 site-data 事件，开着的看板会刷新。
/// 每天早上自动跑一次，「全部站点 → ⋯ → 立即同步数据」也走这里；已经在跑就直接返回 false。
pub async fn sync_all_data(app: tauri::AppHandle) -> bool {
    use std::sync::atomic::Ordering;
    if DATA_SYNCING.swap(true, Ordering::SeqCst) {
        return false;
    }
    use tauri::Manager;
    let st = app.state::<crate::AppState>();
    // 先把没接上的数据源按域名接上（新建的 GA4 / Search Console 资源第二天就能自动用上）
    let _ = crate::data_cmds::auto_bind_data(&st, false).await;
    let list: Vec<_> = st.sites.list().into_iter().filter(|s| {
        let d = &s.data;
        !(d.gsc_site.is_empty() && d.ga4_property.is_empty() && d.bing_site.is_empty())
    }).collect();
    crate::site_data::mark_queued(&list.iter().map(|s| s.id.clone()).collect::<Vec<_>>());
    for site in list {
        let meta = crate::site_data::refresh(&site, &st.conns, &st.sites, &st.sites.data_dir(&site.id), true).await;
        let errors: Vec<String> = [("GA4", &meta.ga4.error), ("Search Console", &meta.gsc.error), ("Bing", &meta.bing.error)].iter().filter(|(_, e)| !e.is_empty()).map(|(l, e)| format!("{l}：{e}")).collect();
        let _ = app.emit("site-data", serde_json::json!({ "site_id": site.id, "errors": errors }));
    }
    set_last_data_sync(&st.data_dir, &today());
    DATA_SYNCING.store(false, Ordering::SeqCst);
    true
}

/// 站点文件夹里未提交的改动（已跟踪的文件）：(文件列表, 是不是全是构建脚本自动重写的数据文件，见 managed::build_outputs)。
/// 读不到 git 状态返回 None。
pub async fn dirty_files(site: &Site, dir: &Path) -> Option<(Vec<String>, bool)> {
    // 用 diff --name-only 拿纯路径：git() 会把输出两头的空白去掉，status --porcelain 第一行开头的空格会被吃掉、路径错一位
    let o = crate::site_git::git(dir, &["diff", "--name-only", "HEAD"], "").await.ok().filter(|o| o.ok)?;
    let files: Vec<String> = o.stdout.lines().map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
    let outs = crate::managed::build_outputs(&site.app_dir(dir));
    let generated = !files.is_empty() && files.iter().all(|f| outs.iter().any(|g| f.rsplit('/').next() == Some(g.as_str())));
    Some((files, generated))
}

/// 分支上相对 base 改过的、构建脚本自动生成的数据文件（见 managed::build_outputs）：还原成 base 的版本（base 没有的就移出版本库），
/// 单独提交一次。返回还原了哪些文件（相对仓库根目录）。
async fn revert_generated(site: &Site, wt: &Path, base: &str) -> Vec<String> {
    let outs = crate::managed::build_outputs(&site.app_dir(wt));
    if outs.is_empty() {
        return Vec::new();
    }
    let Ok(changed) = crate::site_git::git_ok(wt, &["diff", "--name-only", base, "HEAD"], "").await else { return Vec::new() };
    let files: Vec<String> = changed.lines().map(str::trim).filter(|f| !f.is_empty() && outs.iter().any(|g| f.rsplit('/').next() == Some(g.as_str()))).map(str::to_string).collect();
    let mut done = Vec::new();
    for f in &files {
        let at_base = crate::site_git::git(wt, &["cat-file", "-e", &format!("{base}:{f}")], "").await.is_ok_and(|o| o.ok);
        let ok = if at_base {
            crate::site_git::git(wt, &["checkout", base, "--", f], "").await.is_ok_and(|o| o.ok)
        } else {
            crate::site_git::git(wt, &["rm", "-q", "--cached", "--", f], "").await.is_ok_and(|o| o.ok)
        };
        if ok {
            done.push(f.clone());
        }
    }
    if !done.is_empty() && crate::site_git::git_commit_ok(wt, &["commit", "-qm", "还原构建时自动生成的数据文件"]).await.is_err() {
        return Vec::new();
    }
    done
}

/// 合并 / 回滚前：站点文件夹干净就直接过；只有构建脚本自动重写的数据文件有改动（上次在这里构建上线时写的，
/// 比如每次构建同步手续费）就把它们还原——下次构建还会再写一遍；别的文件有改动不动它，返回 false。
/// 本地文件夹合并 / 回滚前的检查不通过时，原因以这个开头（调度靠它认出「等你把文件夹收拾干净」的条目）
pub const LOCAL_NOT_READY: &str = "站点文件夹没准备好：";

/// 本地文件夹站点合并 / 回滚前：要在默认分支上、没有未提交的改动（构建脚本自动重写的数据文件先还原）。
/// 不行就返回原因，写明在哪个分支、哪些文件没提交。
async fn local_not_ready(site: &Site, main: &Path) -> Option<String> {
    let cur = crate::site_git::current_branch(main).await;
    if cur != site.default_branch {
        return Some(format!("{LOCAL_NOT_READY}当前在 {} 上，不在 {} 分支", if cur.is_empty() { "分离 HEAD".to_string() } else { format!("{cur} 分支") }, site.default_branch));
    }
    if settle_build_outputs(site, main).await {
        return None;
    }
    let files = dirty_files(site, main).await.map(|(f, _)| f).unwrap_or_default();
    Some(if files.is_empty() {
        format!("{LOCAL_NOT_READY}读不到 git 状态")
    } else {
        let more = if files.len() > 5 { format!(" 等 {} 个文件", files.len()) } else { String::new() };
        format!("{LOCAL_NOT_READY}有未提交的改动（{}{more}）", files.iter().take(5).cloned().collect::<Vec<_>>().join("、"))
    })
}

/// 不改动文件夹地看一眼：在默认分支上、没有未提交的改动（只有构建脚本重写的数据文件也算干净）。
async fn local_ready_now(site: &Site) -> bool {
    let Ok(main) = crate::site_git::code_dir(site, None) else { return false };
    crate::site_git::current_branch(&main).await == site.default_branch && dirty_files(site, &main).await.is_some_and(|(f, generated)| f.is_empty() || generated)
}

async fn settle_build_outputs(site: &Site, main: &Path) -> bool {
    match dirty_files(site, main).await {
        Some((files, _)) if files.is_empty() => true,
        Some((files, true)) => {
            let mut args = vec!["checkout", "--"];
            args.extend(files.iter().map(String::as_str));
            crate::site_git::git(main, &args, "").await.is_ok_and(|o| o.ok)
        }
        _ => false,
    }
}

/// 上次 Pilot 退出（或开发版重启）时还在执行的条目：作业跟着进程没了，条目会一直停在「AI 正在处理」。
/// 启动时改成失败并写明原因：分支上已经有 AI 提交的改动就能点「重新检查」接着用，没有就让 AI 再改一遍。
fn recover_interrupted(ctx: &Ctx) {
    let st = ctx.state();
    for s in st.sites.list() {
        if settle_site_interrupted(&st.sites.data_dir(&s.id), "上次 Pilot 退出或重启时") {
            ctx.emit_queue(&s.id);
        }
    }
}

/// 清掉没人要的工作树：work/<item id> 对应的条目已完结或不存在、revert-* 超过一天。只删 Pilot 自己建的目录和 pilot/ 分支
fn prune_worktrees(st: &crate::AppState) {
    for site in st.sites.list() {
        if st.site_jobs.running(&site.id) {
            continue;
        }
        let dir = st.sites.data_dir(&site.id);
        let work = crate::sites::build_root(&dir).join("work");
        let Ok(entries) = std::fs::read_dir(&work) else { continue };
        let q = site_ops::load_queue(&dir);
        let main = crate::site_git::code_dir(&site, st.conns.get(&site.gh_conn_id).ok().as_ref()).ok();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let stale = if name.starts_with("revert-") {
                e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > DAY)
            } else {
                q.iter().find(|i| i.id == name).is_none_or(|i| matches!(i.status.as_str(), "done" | "dismissed" | "rolled_back" | "deployed" | "merged"))
            };
            if !stale {
                continue;
            }
            let _ = std::fs::remove_dir_all(e.path());
            if let Some(m) = &main {
                let _ = std::process::Command::new("git").arg("-C").arg(m).args(["worktree", "prune"]).output();
                if let Some(b) = q.iter().find(|i| i.id == name).map(|i| i.branch.clone()).filter(|b| b.starts_with("pilot/")) {
                    let _ = std::process::Command::new("git").arg("-C").arg(m).args(["branch", "-D", &b]).output();
                }
            }
        }
    }
}

/// 一个站点里还停在「AI 处理中」的条目都收尾（见 settle_interrupted）；返回有没有动过
fn settle_site_interrupted(dir: &Path, why: &str) -> bool {
    let stuck: Vec<site_ops::QueueItem> = site_ops::load_queue(dir).into_iter().filter(|i| i.status == "working").collect();
    for it in &stuck {
        // AI 在分支上提交过的话，记下最新提交，「重新检查」就能接着用
        let wt = crate::sites::build_root(dir).join("work").join(&it.id);
        let head = branch_head(&wt, &it.base_sha);
        let _ = site_ops::update_item(dir, &it.id, now(), |i| settle_interrupted(i, head.clone(), why));
    }
    !stuck.is_empty()
}

/// 上线后验收只差「新内容已生效」、挂着「上线后待核对」的：用现在的对照规则再查一次线上页面（每条只查一次）。
/// 找到了就去掉这一条待核对，不用你点「没问题」——早先对照文字带 Markdown 记号、站点转繁体时会误报。
const SNIPPET_MISS: &str = "这次加的内容在验收的页面里没找到";
async fn recheck_snippets(ctx: &Ctx) {
    let st = ctx.state();
    for site in st.sites.list() {
        let dir = st.sites.data_dir(&site.id);
        // 上线满 1 小时再查（CDN 多半刷新了）；页面都打不开的不下结论、下次再查；查了两次都没有就当对照文字本身不显示，清掉
        let due: Vec<QueueItem> = site_ops::load_queue(&dir).into_iter().filter(|i| i.status == "deployed" && !i.snippet_rechecked && i.attention.contains(SNIPPET_MISS) && i.deployed_at + 3600 < now() && i.deployed_at + 14 * 86400 > now()).collect();
        for it in due {
            let mut urls: Vec<String> = it.pages.iter().chain(&it.new_pages).cloned().collect();
            for p in std::iter::once(&it.opp.page).chain(it.also.iter().map(|o| &o.page)) {
                if p.starts_with("https://") && !urls.iter().any(|u| crate::site_verify::same_page(u, p)) {
                    urls.push(p.clone());
                }
            }
            // 第二次要等上线满 24 小时（CDN 缓存通常长于 10 分钟）；页面一直打不开（比如被防火墙拦着）查 3 次就不再查，待核对留着
            if it.snippet_misses >= 1 && it.deployed_at + DAY > now() {
                continue;
            }
            let live = crate::site_verify::snippets_live(&urls, &it.snippets).await;
            let give_up = live == Some(false) && it.snippet_misses >= 1;
            let mut cleared = false;
            let _ = site_ops::update_item(&dir, &it.id, now(), |i| {
                i.snippet_misses += 1;
                i.snippet_rechecked = live == Some(true) || give_up || (live.is_none() && i.snippet_misses >= 3);
                if give_up {
                    i.attention = i.attention.split('；').filter(|p| !p.starts_with(SNIPPET_MISS)).collect::<Vec<_>>().join("；");
                    i.note = "上线后两次都没在线上找到这次的对照文字，多半是不显示在页面上的字段（frontmatter、属性），不再当问题".into();
                    cleared = i.attention.is_empty();
                }
                if live == Some(true) {
                    // 待核对可能是几条用「；」连起来的：只去掉这一条，别的照样留着
                    i.attention = i.attention.split('；').filter(|p| !p.starts_with(SNIPPET_MISS)).collect::<Vec<_>>().join("；");
                    for c in i.checks.iter_mut().filter(|c| c.label == "新内容已生效") {
                        c.ok = true;
                        c.detail.clear();
                    }
                    cleared = i.attention.is_empty();
                }
            });
            // 待核对都清掉了：因为它开出来、还没动的修复任务一起撤掉（和点「没问题」一样）
            if cleared {
                dismiss_followups(&dir, &it.id, "上线后验收是误报（复查时线上已经有这次的内容），不用修");
            }
            if live == Some(true) {
                ctx.emit_queue(&site.id);
            }
        }
    }
}

/// 撤掉一条改动开出来、还没开始的修复任务（key 以 followup:{id} 开头）
pub(crate) fn dismiss_followups(site_dir: &Path, item_id: &str, note: &str) {
    let prefix = format!("followup:{item_id}");
    // AI 试过没改出东西的（失败、分支上没有提交）也一起撤，不然核对完还留一张红卡
    for f in site_ops::load_queue(site_dir).into_iter().filter(|i| (i.status == "open" || (i.status == "failed" && i.head_sha.is_empty())) && i.opp.key.starts_with(&prefix)) {
        let _ = site_ops::update_item(site_dir, &f.id, now(), |i| {
            i.status = "dismissed".into();
            i.note = note.to_string();
        });
    }
}

/// 站点的代码能不能切分支：GitHub 来源的总能；本地文件夹要是 git 仓库、默认分支上有提交
fn repo_ready(site: &Site) -> bool {
    if site.source != "local" {
        return true;
    }
    let Ok(dir) = crate::site_git::code_dir(site, None) else { return false };
    std::process::Command::new("git").arg("-C").arg(&dir).args(["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", site.default_branch)]).output().is_ok_and(|o| o.status.success())
}

/// 被中断那条的分支上，比开始时多出提交的话返回最新提交（没有工作区、没提交过都是 None）
fn branch_head(wt: &Path, base: &str) -> Option<String> {
    if base.is_empty() || !wt.join(".git").exists() {
        return None;
    }
    let out = std::process::Command::new("git").arg("-C").arg(wt).args(["rev-parse", "HEAD"]).output().ok()?;
    let head = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !head.is_empty() && !head.starts_with(base)).then_some(head)
}

/// 上次退出 / 重启时还在执行的那条：AI 已经提交了改动 → 失败，点「重新检查」接着用；
/// 还没提交（改到一半或还没开始）→ 放回排队，托管方案下一轮自动重做，不用你处理
fn settle_interrupted(i: &mut site_ops::QueueItem, head: Option<String>, why: &str) {
    if i.status != "working" {
        return;
    }
    match head {
        Some(h) => {
            i.status = "failed".into();
            i.head_sha = h;
            i.note = format!("{why}这条还在执行，被中断了。AI 已经把改动提交到分支上：会自动「重新检查」接着用，不用再改一遍。");
        }
        None => {
            i.status = "open".into();
            i.note = format!("{why}这条正在改，被中断了，AI 还没提交改动：已放回排队，下一轮自动重做。");
        }
    }
}

pub fn start_scheduler(app: tauri::AppHandle) {
    // 这时候还没有任何作业在跑：还停在「正在处理」的都是上次被中断的
    recover_interrupted(&Ctx { app: app.clone() });
    tauri::async_runtime::spawn(async move {
        // 启动后稍等，别和窗口初始化抢资源
        tokio::time::sleep(Duration::from_secs(60)).await;
        let mut tick: u64 = 0;
        loop {
            let ctx = Ctx { app: app.clone() };
            let st = ctx.state();
            // 老数据迁移、站点档位与所在方案的自主程度对齐
            let mcfg = crate::managed_cmds::sync(&st);
            // 心跳（记下后台停过的时段）、按设置挡不挡睡眠（见 background）
            let managed_active = !mcfg.paused && mcfg.groups.iter().any(|g| g.sites.iter().any(|s| g.active_for(s)));
            crate::background::tick(&st.data_dir, managed_active);
            // 每日摘要：过了设定的时间、今天还没发过、有托管站点
            let hour: u8 = chrono::Local::now().format("%H").to_string().parse().unwrap_or(0);
            if mcfg.digest && !mcfg.paused && hour >= mcfg.digest_hour && mcfg.last_digest != today() && !mcfg.groups.iter().all(|g| g.sites.is_empty()) {
                let _ = digest(&app);
                let _ = crate::managed::modify(&st.data_dir, |c| c.last_digest = today());
            }
            // 每天早上 7 点同步一次所有站点的 GA4 / Search Console / Bing（错过了就在之后第一次检查时补上）
            if hour >= DATA_SYNC_HOUR && last_data_sync(&st.data_dir) != today() {
                set_last_data_sync(&st.data_dir, &today());
                // 几十个站点各拉三个数据源要跑好几分钟：放到后台，别把这一轮的任务和上线复查都拖住
                tauri::async_runtime::spawn(daily_data_sync(ctx.clone()));
            }
            // 数据源失败过一次的：隔半小时再拉一次确认（见 SourceMeta::alarming）
            if !DATA_SYNCING.load(std::sync::atomic::Ordering::SeqCst) {
                tauri::async_runtime::spawn(retry_failed_data(app.clone()));
            }
            // 内容分发：新上线的文章写摘要、按渠道的发布方式发出去（见 syndicate）
            crate::syndicate::schedule(&app);
            // 推广链接：每个站点每周查一遍（见 link_check）
            crate::link_check::schedule(&app);
            // 在线监测：首页每小时、证书每天、域名每周（见 monitor）
            crate::monitor::schedule(&app);
            // 新页面收录：上线 3 天起用 URL 检查确认 Google 编入索引没有（见 index_check）
            crate::index_check::schedule(&app);
            // IndexNow 没交上的网址：到时间就补交（见 indexnow）
            crate::indexnow::schedule(&app);
            // Pilot 自己的数据：每周备份到 GitHub 私有仓库（打开了才备份，见 backup）
            crate::backup::schedule(&app);
            // 收益：连了 AdSense 就每天拉一次报表（见 revenue）
            crate::revenue::schedule(&app);
            // 页面速度（PageSpeed）、页面体检（图片、结构化数据）、竞品观察（Serper）：每周一次
            crate::speed::schedule(&app);
            crate::page_audit::schedule(&app);
            crate::serp::schedule(&app);
            // 托管站点里原因和任务无关的失败（中断、没建 git 仓库、模型临时用不了）：不等下一轮，这会儿就放回排队，
            // 不在「需要你」里挂着；真正重做还是按方案节奏
            for g in mcfg.groups.iter().filter(|g| !mcfg.paused && !g.paused) {
                for id in g.sites.iter().filter(|id| g.active_for(id) && !st.site_jobs.running(id)) {
                    let ready = || st.sites.get(id).is_ok_and(|s| repo_ready(&s));
                    let dir = st.sites.data_dir(id);
                    if let Ok(ids) = site_ops::requeue_retryable(&dir, now(), ready) {
                        if let (false, Ok(site)) = (ids.is_empty(), st.sites.get(id)) {
                            for it in site_ops::load_queue(&dir).iter().filter(|i| ids.contains(&i.id)) {
                                log_item(&ctx, &site, "planned", it, "上次失败的原因和任务本身无关，自动放回排队重做一次");
                            }
                            ctx.emit_queue(id);
                        }
                    }
                    // AI 已经提交到分支、后面推送 / 中断才失败的：自动「重新检查」接着用（每条一次），不用你点；
                    // 因为站点自己构建不过卡住的，站点修好了也接着查
                    let env_ok = st.sites.get(id).is_ok_and(|s| site_build_broken(&s, &dir).is_none());
                    if let Some(item) = site_ops::peek_recheckable(&dir, env_ok) {
                        let env = site_ops::load_queue(&dir).iter().any(|i| i.id == item && site_ops::env_blocked(i));
                        if start(ctx.clone(), id, "recheck", "schedule", Some(item.clone())).is_ok() {
                            let _ = site_ops::update_item(&dir, &item, now(), |i| i.auto_retried = true);
                            if let (Ok(site), Some(it)) = (st.sites.get(id), site_ops::load_queue(&dir).into_iter().find(|i| i.id == item)) {
                                log_item(&ctx, &site, "planned", &it, if env { "站点的构建已经没有已知问题，接着检查这条之前卡住的改动" } else { "AI 的改动还在分支上，自动重新检查一次" });
                            }
                            continue;
                        }
                    }
                    // 上线判定规则升级了：等你确认的条目按新规则重判一次（不花 AI，只重新构建、检查、判定；
                    // 你选了「执行但不上线」的不动）。一个站点一次一条，下一轮接着
                    if let Some(it) = site_ops::load_queue(&dir).into_iter().find(|i| {
                        i.status == "proposed" && !i.branch.is_empty() && !i.head_sha.is_empty() && i.rules_v < crate::managed::RULES_V
                            && !i.verdict.starts_with("自动上线失败") && !i.verdict.contains("这次选了「执行但不上线」")
                    }) {
                        if let Ok(site) = st.sites.get(id) {
                            if start(ctx.clone(), id, "recheck", "schedule", Some(it.id.clone())).is_ok() {
                                log_item(&ctx, &site, "planned", &it, "上线判定规则更新了，按新规则重新判断这条");
                                continue;
                            }
                        }
                    }
                    // 自动上线时本地文件夹不干净（有你没提交的改动、不在主分支）卡住的：文件夹收拾好了就自动「重新检查」接着上线
                    if let Some(it) = site_ops::load_queue(&dir).into_iter().find(|i| i.status == "proposed" && !i.branch.is_empty() && (i.verdict.starts_with("自动上线失败：站点文件夹") || i.verdict.contains("nothing to commit"))) {
                        if let Ok(site) = st.sites.get(id) {
                            if site.source == "local" && local_ready_now(&site).await && start(ctx.clone(), id, "recheck", "schedule", Some(it.id.clone())).is_ok() {
                                log_item(&ctx, &site, "planned", &it, "站点文件夹准备好了，自动接着上线");
                                continue;
                            }
                        }
                    }
                    // 自动上线时站点还发不了（服务器没授权）卡住的：授权好了就自动「重新检查」接着上线
                    if let Some(it) = site_ops::load_queue(&dir).into_iter().find(|i| i.status == "proposed" && !i.branch.is_empty() && waiting_for_deploy(&i.verdict)) {
                        if let Ok(site) = st.sites.get(id) {
                            if crate::site_deploy::deploy_blocker(&site, &st.conns).is_none() && start(ctx.clone(), id, "recheck", "schedule", Some(it.id.clone())).is_ok() {
                                log_item(&ctx, &site, "planned", &it, "站点可以上线了，自动接着上线");
                                continue;
                            }
                        }
                    }
                    // 合并了、上线没完成（上线途中 Pilot 退出，或上传失败）超过半小时的：自动再上线一次，不然改动已经在主分支里却没人管
                    if let Ok(site) = st.sites.get(id) {
                        if site.deploy.mode != "none" {
                            if let Some(it) = site_ops::load_queue(&dir).into_iter().find(|i| i.status == "merged" && i.deployed_at == 0 && !i.merge_sha.is_empty() && i.updated_at + 1800 < now()) {
                                let _ = start(ctx.clone(), id, "accept", "schedule", Some(it.id));
                            }
                        }
                    }
                }
            }
            recheck_snippets(&ctx).await;
            // 降档过了 60 天自动恢复：当时连续负面的那类改动，隔两个月值得再让它自动上线一次（再负面还会降）
            for site in st.sites.list() {
                if !site.ops.downgraded.is_empty() && site.ops.downgraded_at > 0 && site.ops.downgraded_at + 60 * DAY < now() {
                    let kinds = site.ops.downgraded.join("、");
                    let _ = st.sites.update(&site.id, now(), |s| { s.ops.downgraded.clear(); s.ops.downgraded_at = 0; Ok(()) });
                    log(&ctx, &site, "resumed", "降档到期恢复", &format!("「{kinds}」类改动 60 天前因复盘负面降档，现在恢复按方案规则自动上线"), "");
                }
            }
            // 每小时清一次没人要的工作树：条目已经完结（上线、驳回、回滚）或已不存在，目录和分支还留着，几周下来几百 MB
            if tick % 6 == 0 {
                prune_worktrees(&st);
            }
            tick += 1;
            // 同时最多跑这么多个运营作业：客户端停了一阵再开，到期的任务一个个补，不会几十个站点一起开始构建
            let mut busy = st.site_jobs.running_count();
            for site in st.sites.list() {
                if busy >= MAX_PARALLEL_JOBS {
                    break;
                }
                if st.site_jobs.running(&site.id) {
                    continue;
                }
                let in_plan = mcfg.group_of(&site.id).is_some();
                // 不在方案里的站点也有上线记录（白名单自动上线、手动确认）：到期的复查照样跑，只查不回滚（见 watch）
                if !in_plan && site_ops::load_queue(&st.sites.data_dir(&site.id)).iter().any(watch_is_due) {
                    if start(ctx.clone(), &site.id, "watch", "schedule", None).is_ok() {
                        busy += 1;
                    }
                    continue;
                }
                if in_plan {
                    // 上线后检查优先：有到期的就先跑它（停止托管的站点也查，只是不回滚）
                    let site_dir = st.sites.data_dir(&site.id);
                    if site_ops::load_queue(&site_dir).iter().any(watch_is_due) {
                        if start(ctx.clone(), &site.id, "watch", "schedule", None).is_ok() {
                            busy += 1;
                        }
                        continue;
                    }
                    // 总暂停、所在方案暂停、或这个站点单独停止托管：都不自动运行
                    if mcfg.paused || !mcfg.group_of(&site.id).is_some_and(|g| g.active_for(&site.id)) {
                        continue;
                    }
                }
                let state = load_state(&st.sites.data_dir(&site.id));
                for task in TASKS {
                    // 从没跑过的任务以站点导入时间为起点：刚导入的站点不会立刻在后台跑起构建，第一次在一个周期后。
                    let last = state.last.get(*task).copied().unwrap_or(0).max(site.created_at);
                    // 没有数据源的站点只做体检
                    // 托管站点没接数据源也要跑扫描这一步：计划项要靠它执行（扫描本身没数据就找不到机会，但会处理排队的任务）
                    let needs_data = !matches!(*task, "health" | "plan") && !(in_plan && *task == "scan");
                    let has_data = !site.data.gsc_site.is_empty() || !site.data.ga4_property.is_empty() || !site.data.bing_site.is_empty();
                    if needs_data && !has_data {
                        continue;
                    }
                    if due(&schedule_of(&site, task, mcfg.group_of(&site.id)), last, now()) {
                        if start(ctx.clone(), &site.id, task, "schedule", None).is_ok() {
                            busy += 1;
                        }
                        break; // 每站每轮最多起一个，下一轮再看别的
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_item_held_back_by_deploy_setup_resumes_later() {
        assert!(super::waiting_for_deploy(&format!("自动上线失败：{}{}", crate::site_deploy::NOT_DEPLOYABLE, crate::site_vps::NO_KEY)));
        assert!(!super::waiting_for_deploy("自动上线失败：和主分支上的改动冲突了，合不进去"));
        assert!(!super::waiting_for_deploy(crate::site_vps::NO_KEY));
    }

    #[test]
    fn site_build_broken_follows_the_latest_evidence() {
        let t = tempfile::tempdir().unwrap();
        let site = serde_json::from_value::<Site>(serde_json::json!({"id": "s", "name": "s", "source": "local", "created_at": 0, "updated_at": 10})).unwrap();
        let health = |at: u64, ok: bool| super::RunRecord { task: "health".into(), started: at, finished: at, ok, error: if ok { String::new() } else { "构建或检查没通过：构建失败：退出码 1".into() }, ..Default::default() };
        let save = |runs: Vec<super::RunRecord>, env: Option<(u64, String)>| super::save_state(t.path(), &super::OpsState { runs, env_broken: env, ..Default::default() });
        // 没有证据
        save(vec![], None);
        assert_eq!(super::site_build_broken(&site, t.path()), None);
        // 体检失败
        save(vec![health(20, false)], None);
        assert_eq!(super::site_build_broken(&site, t.path()).as_deref(), Some("构建失败：退出码 1"));
        // 运营任务里改动前的代码也构建不过（比体检新）
        save(vec![health(20, true)], Some((30, "构建产物目录 out 不存在".into())));
        assert_eq!(super::site_build_broken(&site, t.path()).as_deref(), Some("构建产物目录 out 不存在"));
        // 之后体检通过了：好了
        save(vec![health(40, true)], Some((30, "构建产物目录 out 不存在".into())));
        assert_eq!(super::site_build_broken(&site, t.path()), None);
        // 改过站点设置（updated_at 晚于证据）：先放行
        let fixed = serde_json::from_value::<Site>(serde_json::json!({"id": "s", "name": "s", "source": "local", "created_at": 0, "updated_at": 50})).unwrap();
        save(vec![health(20, false)], Some((30, "x".into())));
        assert_eq!(super::site_build_broken(&fixed, t.path()), None);
    }

    #[test]
    fn lanes_alternate_plan_and_data_after_followups_and_manual() {
        let mk = |id: &str, kind: &str, source: &str, score: f64| QueueItem { id: id.into(), opp: Opportunity { kind: kind.into(), source: source.into(), score, ..Default::default() }, ..Default::default() };
        let picked = vec![mk("f", "followup", "health", 2000.0), mk("p1", "plan", "positioning", 990.0), mk("p2", "plan", "positioning", 980.0), mk("m", "plan", "manual", 50.0), mk("d1", "low_ctr", "data", 40.0), mk("d2", "striking", "data", 5.0)];
        let ids = |v: Vec<QueueItem>| v.into_iter().map(|i| i.id).collect::<Vec<_>>();
        // 上一条做的是计划项：先数据机会
        assert_eq!(ids(order_lanes(picked.clone(), 2)), vec!["f", "m", "d1", "p1", "d2", "p2"]);
        // 上一条做的是数据机会（或没做过）：先计划项
        assert_eq!(ids(order_lanes(picked.clone(), 3)), vec!["f", "m", "p1", "d1", "p2", "d2"]);
        // 一边空了就只剩另一边
        assert_eq!(ids(order_lanes(vec![mk("d1", "low_ctr", "data", 1.0), mk("d2", "low_ctr", "data", 0.5)], 3)), vec!["d1", "d2"]);
    }

    #[test]
    fn plan_effort_follows_primary_and_fallback() {
        let g = crate::managed::ManagedGroup { brain: "api:x".into(), model: "claude-opus-5-5".into(), effort: "api:high".into(), fallback_brain: "claude".into(), fallback_model: String::new(), fallback_effort: "max".into(), ..crate::managed::ManagedGroup::from_legacy(&Default::default(), "t") };
        assert_eq!(group_effort(&g, &("api:x".into(), "claude-opus-5-5".into())), "api:high");
        assert_eq!(group_effort(&g, &("claude".into(), SONNET.into())), "max", "备用模型没写具体模型，按默认模型比");
        assert_eq!(group_effort(&g, &("codex".into(), String::new())), "");
    }

    #[tokio::test]
    async fn generated_data_is_reverted_out_of_the_ai_change() {
        use crate::site_git as g;
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().to_path_buf();
        let w = |p: &str, c: &str| { let f = repo.join(p); std::fs::create_dir_all(f.parent().unwrap()).unwrap(); std::fs::write(f, c).unwrap(); };
        g::git_ok(&repo, &["init", "-q", "-b", "main"], "").await.unwrap();
        w("package.json", r#"{"scripts":{"prebuild":"node scripts/sync.mjs","build":"astro build"}}"#);
        w("scripts/sync.mjs", "writeFileSync('src/data/fees.json', x)");
        w("src/data/fees.json", "{\"a\":1}");
        w("src/pages/a.md", "old");
        g::git_ok(&repo, &["add", "-A"], "").await.unwrap();
        g::git_commit_ok(&repo, &["commit", "-qm", "base"]).await.unwrap();
        let base = g::head_sha(&repo).await;
        w("src/data/fees.json", "{\"a\":2}");
        w("src/pages/a.md", "new");
        g::git_ok(&repo, &["add", "-A"], "").await.unwrap();
        g::git_commit_ok(&repo, &["commit", "-qm", "ai"]).await.unwrap();
        let site: Site = serde_json::from_value(serde_json::json!({"id": "s", "name": "s", "source": "local", "created_at": 0, "updated_at": 0})).unwrap();
        assert_eq!(revert_generated(&site, &repo, &base).await, vec!["src/data/fees.json"]);
        let diff = g::git_ok(&repo, &["diff", "--name-only", &base, "HEAD"], "").await.unwrap();
        assert_eq!(diff.trim(), "src/pages/a.md");
    }

    #[test]
    fn interrupted_items_without_commits_go_back_to_the_queue() {
        let mut a = site_ops::QueueItem { status: "working".into(), ..Default::default() };
        settle_interrupted(&mut a, None, "上次 Pilot 退出或重启时");
        assert_eq!(a.status, "open");
        let mut b = site_ops::QueueItem { status: "working".into(), ..Default::default() };
        settle_interrupted(&mut b, Some("abc123".into()), "作业超时被停止时");
        assert_eq!((b.status.as_str(), b.head_sha.as_str()), ("failed", "abc123"));
        assert!(b.note.starts_with("作业超时"));
        let mut c = site_ops::QueueItem { status: "deployed".into(), ..Default::default() };
        settle_interrupted(&mut c, None, "上次 Pilot 退出或重启时");
        assert_eq!(c.status, "deployed");
    }

    #[test]
    fn persistent_model_failures_are_told_apart_from_transient_ones() {
        use super::persistent_failure as p;
        assert_eq!(p("模型这一轮没有完成：API HTTP 400：Your credit balance is too low to access the Anthropic API."), Some("余额不足"));
        assert_eq!(p("HTTP 401 invalid x-api-key"), Some("API 密钥无效"));
        assert_eq!(p("Not logged in · Please run /login"), Some("客户端没登录"));
        assert_eq!(p("model: claude-x not found"), Some("模型用不了"));
        // 限流、服务商临时故障：过一会儿就好，不记
        assert_eq!(p("HTTP 429 rate_limit_error"), None);
        assert_eq!(p("HTTP 529 overloaded_error"), None);
        let m = ("api:x".to_string(), "claude-opus-5-5".to_string());
        assert_eq!(super::model_down(&m), None);
        super::mark_down(&m, "余额不足");
        assert_eq!(super::model_down(&m).as_deref(), Some("余额不足"));
        super::clear_down(&m);
        assert_eq!(super::model_down(&m), None);
    }

    use super::*;
    use crate::site_data::Row;

    #[test]
    fn schedules_are_due_after_their_period() {
        assert!(due("daily", 0, DAY));
        assert!(!due("daily", 100, DAY));
        assert!(due("weekly", 0, 7 * DAY));
        assert!(!due("off", 0, 99 * DAY));
    }

    #[test]
    fn summary_keeps_only_the_closing_explanation() {
        let reply = "先看看页面。\n\n构建失败了，我清掉代理再跑一次。\n\n只改了 src/data/tool-facts.ts：给 Vidu 加了英文 SEO 标题和描述，围绕免费额度、错峰模式和商用这几个搜索问题。\n新页面：无\n改动的页面：https://a.com/en/links/vidu/";
        let s = final_summary(reply);
        assert!(s.starts_with("只改了 src/data/tool-facts.ts"), "{s}");
        assert!(!s.contains("构建失败") && !s.contains("改动的页面"));
    }

    #[test]
    fn weekly_rounds_land_on_the_chosen_weekday() {
        use chrono::{Datelike, TimeZone};
        // 上次在某个周三 15:00 跑过 → 下一次是至少 6 天后的周一（从那天 0 点起）
        let last = chrono::Local.with_ymd_and_hms(2026, 10, 28, 15, 0, 0).single().unwrap().timestamp() as u64;
        let next = next_due("weekly:1", last).unwrap();
        let d = chrono::Local.timestamp_opt(next as i64, 0).single().unwrap();
        assert_eq!(d.weekday().number_from_monday(), 1);
        assert!(next >= last + 6 * DAY && next < last + 13 * DAY);
        assert!(!due("weekly:1", last, next - 60));
        assert!(due("weekly:1", last, next));
        // 上次就是周一 → 下一次是下周一，不会当周再跑
        let mon = chrono::Local.with_ymd_and_hms(2026, 11, 2, 9, 0, 0).single().unwrap().timestamp() as u64;
        let n2 = next_due("weekly:1", mon).unwrap();
        assert!(n2 >= mon + 6 * DAY && n2 <= mon + 7 * DAY);
        // 每 2 周：至少隔 13 天，还是落在周一
        let n3 = next_due("weekly:1:2", mon).unwrap();
        assert!(n3 >= mon + 13 * DAY && n3 <= mon + 14 * DAY);
        assert_eq!(chrono::Local.timestamp_opt(n3 as i64, 0).single().unwrap().weekday().number_from_monday(), 1);
        // 每 3 天
        assert_eq!(next_due("days:3", 100), Some(100 + 3 * DAY));
    }

    #[test]
    fn reviews_follow_the_expected_metric_and_subtract_site_trend() {
        let r = |c: f64, i: f64, ctr: f64, p: f64| Row { clicks: c, impressions: i, ctr, position: p, ..Default::default() };
        let site = r(1000.0, 50000.0, 0.02, 10.0);
        let base = |metric: &'static str| ReviewInput { metric, is_new: false, page_before: &DEF, page_after: &DEF, query: "", query_before: None, query_after: None, site_before: None, site_after: &DEF, new_queries: 0 };
        static DEF: Row = Row { key: String::new(), key2: String::new(), clicks: 0.0, impressions: 0.0, ctr: 0.0, position: 0.0 };
        // 改标题：点击率涨 10% 以上算有效
        let (b, a) = (r(100.0, 5000.0, 0.020, 7.9), r(105.0, 4800.0, 0.023, 7.9));
        assert_eq!(judge_expect(&ReviewInput { page_before: &b, page_after: &a, ..base("ctr") }).0, "positive");
        // 排名前进 1 位以上
        let (b, a) = (r(50.0, 3000.0, 0.017, 12.4), r(52.0, 3100.0, 0.017, 9.8));
        assert_eq!(judge_expect(&ReviewInput { page_before: &b, page_after: &a, ..base("position") }).0, "positive");
        // 页面点击跌 20%，但全站同期跌了 25%：扣掉之后不算负面
        let (b, a) = (r(100.0, 5000.0, 0.02, 8.0), r(80.0, 4500.0, 0.018, 8.0));
        let (sb, sa) = (site.clone(), r(750.0, 45000.0, 0.017, 10.0));
        assert_ne!(judge_expect(&ReviewInput { page_before: &b, page_after: &a, site_before: Some(&sb), site_after: &sa, ..base("ctr") }).0, "negative");
        // 全站没变，页面点击跌 20%：负面
        assert_eq!(judge_expect(&ReviewInput { page_before: &b, page_after: &a, site_before: Some(&sb), site_after: &sb, ..base("ctr") }).0, "negative");
        // 目标查询词：两头都有数据时看查询词
        let (qb, qa) = (r(2.0, 400.0, 0.005, 21.0), r(9.0, 900.0, 0.01, 8.5));
        let v = judge_expect(&ReviewInput { page_before: &b, page_after: &b, query: "gate kyc", query_before: Some(&qb), query_after: Some(&qa), ..base("position") });
        assert_eq!(v.0, "positive");
        assert!(v.1.contains("gate kyc"), "{}", v.1);
        // 新页面：4 周有 30 以上展现算被搜到
        let a = r(3.0, 120.0, 0.025, 18.0);
        assert_eq!(judge_expect(&ReviewInput { is_new: true, page_after: &a, new_queries: 6, ..base("impressions") }).0, "positive");
        // 数据太少：持平
        let (b, a) = (r(0.0, 10.0, 0.0, 30.0), r(0.0, 12.0, 0.0, 25.0));
        assert_eq!(judge_expect(&ReviewInput { page_before: &b, page_after: &a, ..base("position") }).0, "neutral");
        // 技术修复不按流量判断
        assert_eq!(judge_expect(&ReviewInput { page_before: &b, page_after: &a, ..base("health") }).0, "neutral");
    }

    #[test]
    fn risky_change_kinds_never_auto_ship() {
        let mut site: Site = serde_json::from_value(serde_json::json!({"id":"s","name":"s","source":"local","created_at":0,"updated_at":0})).unwrap();
        site.ops.level = 4;
        let item = |change: &str| QueueItem { opp: Opportunity { change: change.into(), ..Default::default() }, ..Default::default() };
        assert_eq!(effective_level(&site, &item("meta")), 4);
        assert_eq!(effective_level(&site, &item("new_content")), 3);
        assert_eq!(effective_level(&site, &item("technical")), 3);
        site.ops.downgraded = vec!["meta".into()];
        assert_eq!(effective_level(&site, &item("meta")), 3);
        // 全托管：技术问题、改事实、结构化数据都按方案规则走；抢词合并和降档的类型留给你
        site.ops.level = 5;
        site.ops.downgraded.clear();
        let kind = |kind: &str, change: &str| QueueItem { opp: Opportunity { kind: kind.into(), change: change.into(), ..Default::default() }, ..Default::default() };
        for (k, c) in [("health", "technical"), ("stale", "content"), ("schema", "technical"), ("slow", "technical"), ("plan", "new_content")] {
            assert_eq!(effective_level(&site, &kind(k, c)), 5, "{k}/{c}");
        }
        assert_eq!(effective_level(&site, &kind("cannibal", "content")), 3);
        site.ops.downgraded = vec!["meta".into()];
        assert_eq!(effective_level(&site, &item("meta")), 3);
    }

    #[test]
    fn budget_limits_stop_processing() {
        assert!(over_budget(&Budget { changes_used: 10, changes_limit: 10, tokens_used: 0, tokens_limit: 0 }).is_some());
        assert!(over_budget(&Budget { changes_used: 1, changes_limit: 10, tokens_used: 5, tokens_limit: 5 }).is_some());
        assert!(over_budget(&Budget { changes_used: 1, changes_limit: 10, tokens_used: 1, tokens_limit: 0 }).is_none());
        // 0＝不限；你手动交给 AI 时不看条数
        assert!(over_budget(&Budget { changes_used: 99, changes_limit: 0, tokens_used: 0, tokens_limit: 0 }).is_none());
        assert!(over_tokens(&Budget { changes_used: 10, changes_limit: 10, tokens_used: 0, tokens_limit: 0 }).is_none());
    }

    /// 真实 git：工作树分支 → 模拟模型改动 → Pilot 兜底提交（本机没有 git 身份也能提交）→ 差异分类 →
    /// 压缩合并回默认分支 → 回退。覆盖运营任务里所有 Pilot 自己执行的 git 步骤。
    #[tokio::test]
    async fn ops_branch_lifecycle_with_real_git() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            return;
        }
        use crate::site_git as g;
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("site");
        std::fs::create_dir_all(repo.join("guide")).unwrap();
        std::fs::write(repo.join("guide/index.html"), "<html>\n<head>\n<title>指南</title>\n<meta name=\"description\" content=\"指南页面\">\n</head>\n<body><p>正文</p></body>\n</html>\n").unwrap();
        // 隔离用户的全局 git 配置，模拟「本机没配身份」
        std::env::set_var("GIT_CONFIG_GLOBAL", t.path().join("noglobal"));
        g::git_ok(&repo, &["init", "-q", "-b", "main"], "").await.unwrap();
        g::git_ok(&repo, &["add", "-A"], "").await.unwrap();
        g::git_commit_ok(&repo, &["commit", "-qm", "init"]).await.unwrap();
        let base = g::git_ok(&repo, &["rev-parse", "main"], "").await.unwrap();
        let wt = t.path().join("work/item1");
        g::worktree_at(&repo, &wt, &base, Some("pilot/ops-guide-abc123")).await.unwrap();
        // 「模型」改了标题和描述，但没有提交
        let page = std::fs::read_to_string(wt.join("guide/index.html")).unwrap();
        let page = page
            .replace("<title>指南</title>", "<title>手冲咖啡冲煮指南：水温、粉水比与注水手法</title>")
            .replace("content=\"指南页面\"", "content=\"手冲咖啡怎么冲？这份指南讲清水温、粉水比、研磨度和三段注水的做法，新手照着做就能冲出稳定好喝的一杯。\"");
        std::fs::write(wt.join("guide/index.html"), page).unwrap();
        std::os::unix::fs::symlink(t.path(), wt.join("node_modules")).ok();
        // Pilot 兜底提交（排除 node_modules）
        g::git_ok(&wt, &["add", "-A", "--", ".", ":(exclude)node_modules", ":(exclude)uploads"], "").await.unwrap();
        g::git_commit_ok(&wt, &["commit", "-qm", "ops: improve /guide/"]).await.unwrap();
        let head = g::head_sha(&wt).await;
        assert_eq!(g::commits_between(&wt, &base, "HEAD").await.len(), 1);
        let diff = g::diff_zero_context(&wt, &base, &head).await.unwrap();
        assert!(!diff.contains("node_modules"));
        let wl = vec!["meta_description".to_string(), "title".to_string()];
        let c = crate::site_ops::classify(&diff, &wl, &|_| true);
        assert!(c.reasons.is_empty(), "{:?}\n{diff}", c.reasons);
        assert_eq!(c.kinds, vec!["title".to_string(), "meta_description".to_string()]);
        // 确认：压缩合并进默认分支
        g::remove_worktree(&repo, &wt).await;
        g::git_ok(&repo, &["merge", "--squash", "pilot/ops-guide-abc123"], "").await.unwrap();
        g::git_commit_ok(&repo, &["commit", "-qm", "ops: improve /guide/"]).await.unwrap();
        let merged = g::head_sha(&repo).await;
        assert!(std::fs::read_to_string(repo.join("guide/index.html")).unwrap().contains("冲煮指南：水温"));
        assert!(g::is_clean(&repo).await);
        // 同样的改动再合并一次：主分支里都有了，没有东西可提交
        g::git_ok(&repo, &["merge", "--squash", "pilot/ops-guide-abc123"], "").await.unwrap();
        assert!(g::nothing_staged(&repo).await);
        g::git_ok(&repo, &["reset", "-q", "--hard", "HEAD"], "").await.unwrap();
        // 回滚：回退那一次提交
        g::git_commit_ok(&repo, &["revert", "--no-edit", &merged]).await.unwrap();
        assert!(std::fs::read_to_string(repo.join("guide/index.html")).unwrap().contains("<title>指南</title>"));
        std::env::remove_var("GIT_CONFIG_GLOBAL");
    }

    /// 实测（会调用本机 Claude CLI、消耗少量用量，默认不跑）：
    /// `cargo test --lib -- --ignored live_unattended_turn --nocapture`
    /// 无人值守的「自动」档回合：能改文件、能本地提交，部署类命令被 Pilot 自动拒绝。
    #[tokio::test]
    #[ignore]
    async fn live_unattended_turn() {
        use crate::site_git as g;
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("site");
        std::fs::create_dir_all(repo.join("guide")).unwrap();
        std::fs::write(repo.join("guide/index.html"), "<!doctype html>\n<html lang=\"zh\">\n<head>\n<meta charset=\"utf-8\">\n<title>指南</title>\n<meta name=\"description\" content=\"指南页面\">\n</head>\n<body>\n<h1>手冲咖啡冲煮指南</h1>\n<p>水温 90–94℃，粉水比 1:15，研磨度中细。</p>\n</body>\n</html>\n").unwrap();
        g::git_ok(&repo, &["init", "-q", "-b", "main"], "").await.unwrap();
        g::git_ok(&repo, &["add", "-A"], "").await.unwrap();
        g::git_commit_ok(&repo, &["commit", "-qm", "init"]).await.unwrap();
        let base = g::head_sha(&repo).await;
        let wt = t.path().join("data/sites/s/work/i1");
        g::worktree_at(&repo, &wt, &base, Some("pilot/ops-guide-i1")).await.unwrap();
        let data_dir = t.path().join("data");
        std::fs::create_dir_all(data_dir.join("permit/pending")).unwrap();
        let conv_id = uuid::Uuid::new_v4().to_string();
        let _guard = UnattendedGuard::start(&conv_id, data_dir.join("permit").join("pending"));
        let conn = crate::workspace_connection(&wt.to_string_lossy());
        let msg = "【运营 · 改动】/guide/ 排名靠前但点击率偏低\n\n页面：https://example.com/guide/\n依据：查询词「手冲咖啡 水温」排名 3、点击率很低。\n\
当前目录是独立工作区（分支 pilot/ops-guide-i1）。请：\n1. 只改 guide/index.html 的 <title> 和 meta description，让它们回应「手冲咖啡 水温 粉水比」，描述 50–160 字；\n\
2. 用 git add 和 git commit 提交（没配 git 身份导致提交失败就保留改动）；\n3. 最后运行一次 `wrangler pages deploy . --project-name demo` 试试能不能部署，并如实告诉我结果。".to_string();
        let channel = tauri::ipc::Channel::<crate::agent::TurnEvent>::new(|_| Ok(()));
        let res = crate::agent::run_turn(
            crate::agent::RunRegistry::default(),
            conn,
            wt.to_string_lossy().into_owned(),
            "claude".into(),
            "sonnet".into(),
            "auto".into(),
            String::new(),
            false,
            data_dir.clone(),
            crate::ssh::SshSessions::new(),
            uuid::Uuid::new_v4().to_string(),
            true,
            Some("你是站点运营助手，在当前目录里按要求修改文件。".into()),
            msg,
            conv_id.clone(),
            channel,
            Default::default(),
        )
        .await;
        println!("ok={} error={}\n--- reply ---\n{}", res.ok, res.error, res.text);
        for tc in &res.tools {
            println!("tool: {} | {}", tc.label, tc.detail.chars().take(160).collect::<String>());
        }
        let page = std::fs::read_to_string(wt.join("guide/index.html")).unwrap();
        println!("--- page ---\n{page}");
        assert!(res.ok, "{}", res.error);
        assert!(!page.contains("<title>指南</title>"), "标题应该被改掉");
        // 部署命令不能真的跑起来：没有任何 .wrangler 产物
        assert!(!wt.join(".wrangler").exists());
    }

    #[test]
    fn link_targets_are_checked_against_the_build() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("zh/fees")).unwrap();
        std::fs::write(t.path().join("zh/fees/index.html"), "x").unwrap();
        std::fs::write(t.path().join("about.html"), "x").unwrap();
        let f = page_exists_in(t.path());
        assert!(f("/zh/fees/"));
        assert!(f("/zh/fees/#top"));
        assert!(f("/about"));
        assert!(!f("/nope/"));
    }

    #[test]
    fn digest_lists_only_sites_with_news_and_never_prints_minus_zero() {
        let t = tempfile::tempdir().unwrap();
        let mk = |id: &str, name: &str| -> (Site, PathBuf) {
            let s: Site = serde_json::from_value(serde_json::json!({"id": id, "name": name, "source": "local", "created_at": 0, "updated_at": 0})).unwrap();
            let d = t.path().join(id);
            std::fs::create_dir_all(&d).unwrap();
            (s, d)
        };
        let sites = vec![mk("a", "a.com"), mk("b", "b.com"), mk("c", "c.com")];
        let act = |site: &str, kind: &str| crate::managed::Activity { ts: now(), site_id: site.into(), site_name: site.into(), kind: kind.into(), title: "t".into(), ..Default::default() };
        let acts = vec![act("a", "shipped"), act("a", "watch_ok"), act("b", "rolled_back"), act("b", "planned")];
        let d = build_digest(&sites, &acts, "2026-10-27");
        assert_eq!((d.sites_total, d.shipped, d.planned, d.broken), (3, 1, 1, 1));
        assert_eq!(d.active.iter().map(|s| s.site_name.as_str()).collect::<Vec<_>>(), vec!["a.com", "b.com"]);
        assert_eq!(d.active[0].events.len(), 1, "复查正常这类不进日报");
        assert_eq!(d.quiet, vec!["c.com".to_string()]);
        assert_eq!(whole(-0.2), 0.0);
        assert!(!format!("{:.0}", whole(-0.2)).contains('-'));
        assert_eq!(change_text(5.0, 0.0), "+5（新增）");
        assert_eq!(change_text(3.0, 4.0), "-1（-25%）");
        assert_eq!(change_text(2.0, 2.0), "持平");
        assert_eq!(change_text(2.3, 2.0), "持平");
        assert_eq!(change_text(501.0, 500.0), "+1");
        assert_eq!(change_text(110.0, 100.0), "+10（+10%）");
        assert_eq!(pct(-0.004), "持平");
        assert_eq!(pct(0.126), "+13%");
        assert_eq!(pct(-0.2), "-20%");
        let w = |since: u64, at: u64| digest_window(&DigestData { since, generated_at: at, ..Default::default() });
        assert_eq!(w(0, 100), "过去 24 小时");
        assert_eq!(w(1_000_000, 1_000_000 + 25 * 3600), "过去 24 小时");
        assert_eq!(w(1_000_000, 1_000_000 + 30 * 3600), "过去 30 小时");
        assert_eq!(w(1_000_000, 1_000_000 + 3 * DAY), "过去 3 天");
        let dir = tempfile::tempdir().unwrap();
        let t = 30 * DAY;
        assert_eq!(digest_since(dir.path(), "2026-11-03", t), t - DAY, "没有上一份就看 24 小时");
        let put = |date: &str, at: u64| std::fs::write(dir.path().join(format!("{date}.json")), serde_json::to_vec(&DigestData { date: date.into(), generated_at: at, ..Default::default() }).unwrap()).unwrap();
        put("2026-11-01", t - 2 * DAY);
        put("2026-11-02", t - 30 * 3600);
        put("2026-11-03", t - 600);
        assert_eq!(digest_since(dir.path(), "2026-11-03", t), t - 30 * 3600, "从今天之前最近一份接着算，不看今天重出的那份");
        put("2026-11-02", t - 20 * DAY);
        assert_eq!(digest_since(dir.path(), "2026-11-03", t), t - 7 * DAY, "最多往前看 7 天");
        let md = digest_markdown(&d);
        assert!(md.contains("其余 1 个站点今天没有变化：c.com"));
    }

    #[test]
    fn codex_without_a_model_uses_the_local_default() {
        assert_eq!(default_model("codex"), Some(String::new()));
        assert_eq!(default_model("claude"), Some("claude-sonnet-5".into()));
        assert_eq!(default_model("some-api"), None);
    }

    #[test]
    fn old_markdown_digests_are_read_back_into_the_new_layout() {
        let md = "# 托管日报 · 2026-10-23\n\n托管 4 个站点；过去 24 小时 AI 自动上线 0 项、出计划 1 次、回滚 0 项。\n\n## a.com\n\n\n## b.com\n\n- 近 28 天 Google 点击 -0（前 28 天 -0）\n\n## c.com\n\n- 近 28 天 Google 点击 2（前 28 天 -0）\n- [计划] 本周计划 2 项：重写评测；修正链接\n\n## d.com\n\n- 需要你：1 项（换标题）\n";
        let d = parse_old_digest(md).unwrap();
        assert_eq!((d.date.as_str(), d.sites_total, d.shipped, d.planned, d.broken), ("2026-10-23", 4, 0, 1, 0));
        assert_eq!(d.active.len(), 1);
        assert_eq!(d.active[0].site_name, "c.com");
        assert_eq!((d.active[0].events[0].kind.as_str(), d.active[0].events[0].label.as_str()), ("planned", "出计划"));
        assert_eq!((d.active[0].events[0].title.as_str(), d.active[0].events[0].detail.as_str()), ("本周计划 2 项", "重写评测；修正链接"));
        assert_eq!(d.quiet, vec!["a.com".to_string(), "b.com".to_string()]);
        assert_eq!(d.needs.len(), 1);
        assert_eq!(d.traffic.len(), 1);
        assert_eq!((d.traffic[0].clicks, d.traffic[0].clicks_prev), (2.0, 0.0));
        assert_eq!(d.no_traffic, vec!["b.com".to_string()]);
        assert!(parse_old_digest("随便一段文字").is_none());
    }
}

#[cfg(test)]
mod no_change_tests {
    use super::*;
    #[test]
    fn only_an_explicit_conclusion_counts_as_no_change() {
        assert_eq!(no_change_reason("查了模板。\n\n不用改：这页是站点有意 noindex 的旧页").as_deref(), Some("这页是站点有意 noindex 的旧页"));
        assert_eq!(no_change_reason("**不用改：** 已经修好了").as_deref(), Some("已经修好了"));
        assert_eq!(no_change_reason("不用改:问题不存在").as_deref(), Some("问题不存在"));
        // 没写原因、只是顺嘴提到、或者干脆没做完，都还算失败
        assert_eq!(no_change_reason("不用改："), None);
        assert_eq!(no_change_reason("我觉得不用改：但没查完就超时了"), None);
        assert_eq!(no_change_reason("模型这一轮没有完成"), None);
    }
}

#[cfg(test)]
mod pages_wait_tests {
    use super::*;
    use crate::cf::PagesDeployment;
    fn dep(env: &str, commit: &str, stage: &str, status: &str) -> PagesDeployment {
        PagesDeployment { environment: env.into(), commit: commit.into(), stage: stage.into(), status: status.into(), ..Default::default() }
    }
    #[test]
    fn a_commit_without_a_build_is_told_apart_from_one_that_is_building() {
        let list = [dep("production", "aaaaaaa111", "build", "active"), dep("preview", "bbbbbbb222", "deploy", "success"), dep("production", "ccccccc333", "deploy", "success")];
        // 短 sha 也认；还在构建的算有记录，不用催
        assert!(commit_deployment(&list, "aaaaaaa1").is_some());
        // 预览环境的不算：这次提交在生产环境还没有构建，该催
        assert!(commit_deployment(&list, "bbbbbbb222").is_none());
        assert!(commit_deployment(&list, "").is_none());
        // 线上是最新一次成功的生产构建，还在构建的和预览的都不算
        assert_eq!(latest_live(&list).unwrap().commit, "ccccccc333");
        assert!(latest_live(&list[..2]).is_none());
    }
}
