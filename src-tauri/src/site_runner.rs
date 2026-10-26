//! 运营任务运行器：机会扫描 → 按档位出方案 / 提 PR / 低风险自动上线；站点体检；周报；效果复盘；定时调度。
//!
//! 安全边界（见设计文档「安全与凭据」）：
//! - 模型只在 Pilot 准备的工作树里改代码、在本地提交；推送、开 PR、合并、部署、IndexNow 全由 Pilot 执行。
//! - 无人值守的回合里，所有需要确认的操作（推送、部署、读 Token 环境变量等）一律由 Pilot 代为拒绝，
//!   确认卡也不会出现在界面上（见 `is_unattended` 与 lib.rs::list_pending_permits）。
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
const COOLDOWN: u64 = 21 * DAY;

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
                        let _ = crate::permit::respond(&pending_dir, &p.id, false);
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
}

fn state_path(site_dir: &Path) -> PathBuf {
    site_dir.join("ops_state.json")
}

pub fn load_state(site_dir: &Path) -> OpsState {
    std::fs::read(state_path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
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
    s.runs.truncate(60);
    save_state(site_dir, &s);
}

pub fn period(schedule: &str) -> Option<u64> {
    match schedule {
        "daily" => Some(DAY),
        "weekly" => Some(7 * DAY),
        _ => None,
    }
}

/// 某个任务现在该不该跑（错过的任务在下次启动时补跑一次）。
pub fn due(schedule: &str, last: u64, now: u64) -> bool {
    period(schedule).is_some_and(|p| now.saturating_sub(last) >= p)
}

pub const TASKS: &[&str] = &["scan", "health", "report", "review", "plan"];

/// 任务周期。在托管方案里的站点：扫描和体检每天，计划、周报、复盘每周；
/// 没托管的站点（你自己运营）：不做计划，其它按站点自己的周期（默认每周）。
pub fn schedule_of<'a>(site: &'a Site, task: &str, in_plan: bool) -> &'a str {
    if in_plan {
        return match task {
            "scan" | "health" => "daily",
            _ => "weekly",
        };
    }
    match task {
        "plan" => "off",
        "scan" => &site.ops.scan,
        "health" => &site.ops.health,
        "report" => &site.ops.report,
        "review" => &site.ops.review,
        _ => "off",
    }
}

pub fn task_label(task: &str) -> &'static str {
    match task {
        "scan" => "机会扫描",
        "health" => "站点体检",
        "report" => "周报",
        "review" => "效果复盘",
        "accept" => "确认改动",
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
        use tauri_plugin_notification::NotificationExt;
        let _ = self.app.notification().builder().title(title).body(body).show();
    }
    fn emit_queue(&self, site_id: &str) {
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
    let changes = q.iter().filter(|i| i.worked_at >= since).count() as u32;
    let conn_id = crate::site_cmds::site_conn_id(&site.id);
    let tokens: u64 = convos
        .list()
        .into_iter()
        .filter(|c| c.conn_id == conn_id && c.origin == "ops" && c.updated_at >= since)
        .map(|c| if c.billed_tokens > 0 { c.billed_tokens } else { c.total_tokens })
        .sum();
    // 托管站点用所在方案的上限；站点数据目录是 <数据目录>/sites/<id>
    let tokens_limit = site_dir
        .parent()
        .and_then(Path::parent)
        .and_then(|root| crate::managed::load(root).group_of(&site.id).map(|g| g.tokens_per_week))
        .unwrap_or(site.ops.budget.tokens_per_week);
    Budget { changes_used: changes, changes_limit: site.ops.budget.changes_per_week, tokens_used: tokens, tokens_limit }
}

fn over_budget(b: &Budget) -> Option<String> {
    if b.changes_used >= b.changes_limit {
        return Some(format!("本周改动已达上限（{} 条）", b.changes_limit));
    }
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
        "scan" => &[("data", "拉取数据"), ("scan", "扫描机会"), ("work", "处理机会"), ("ship", "提交 / 上线")],
        "health" => &[("data", "拉取数据"), ("prepare", "准备代码"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("report", "体检报告")],
        "report" => &[("data", "拉取数据"), ("report", "生成周报")],
        "review" => &[("data", "拉取数据"), ("review", "对比上线前后")],
        "accept" => &[("merge", "合并"), ("prepare", "准备代码"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("upload", "上传"), ("verify", "线上验收"), ("indexnow", "提交 IndexNow")],
        "rollback" => &[("revert", "回退提交"), ("prepare", "准备代码"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("upload", "上传"), ("verify", "线上验收"), ("indexnow", "提交 IndexNow")],
        "work" => &[("work", "AI 改动"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("ship", "上线 / 等你确认"), ("verify", "线上验收")],
        "batch" => &[("work", "AI 改动"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("ship", "上线 / 等你确认"), ("verify", "线上验收")],
        "recheck" => &[("work", "AI 的改动"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("ship", "上线 / 等你确认"), ("verify", "线上验收")],
        "plan" => &[("data", "拉取数据"), ("plan", "AI 制定计划")],
        "cycle" => &[("data", "拉取数据"), ("plan", "AI 制定计划"), ("work", "AI 改动并检查"), ("ship", "上线 / 等你确认"), ("verify", "线上验收")],
        "watch" => &[("watch", "上线后检查")],
        _ => &[],
    }
}

/// 一个运营作业最多跑多久：到点就停掉里面的命令和 AI 回合，按失败收尾，不让站点被一个卡死的作业占着。
const MAX_JOB_DURATION: Duration = Duration::from_secs(3 * 60 * 60);

/// 开一个运营作业（后台跑，立即返回作业 id）。
pub fn start(ctx: Ctx, site_id: &str, task: &str, trigger: &str, item_id: Option<String>) -> Result<String, String> {
    let st = ctx.state();
    let site = st.sites.get(site_id)?;
    let job = st.site_jobs.start(Some(ctx.app.clone()), site_id, &format!("ops:{task}"), task_label(task), steps_for(task))?;
    // 受保护的站点：任何自动流程都不碰，作业直接按失败收尾，原因给用户看
    if let Some(why) = crate::sites::protected_reason(&site) {
        job.finish(&Err(why.clone()));
        return Err(why);
    }
    let id = job.id.clone();
    let (task, trigger) = (task.to_string(), trigger.to_string());
    tauri::async_runtime::spawn(async move {
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
                ctx.state().site_jobs.kill(&site.id);
                let _ = tokio::time::timeout(Duration::from_secs(60), &mut body).await;
                body.abort();
                Err(format!("超过 {} 小时还没结束，已停止；详情见作业日志和会话", MAX_JOB_DURATION.as_secs() / 3600))
            }
        };
        let res = r.as_ref().map(|_| ()).map_err(|e| e.clone());
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
    });
    Ok(id)
}

async fn refresh_data(ctx: &Ctx, site: &Site, job: &Job) {
    job.step("data", "running", "");
    let meta = crate::site_data::refresh(site, &ctx.state().conns, &ctx.site_dir(&site.id), false).await;
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

async fn scan(ctx: &Ctx, site: &Site, job: &Job, trigger: &str) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    refresh_data(ctx, site, job).await;
    job.step("scan", "running", "");
    let opps = site_ops::opportunities(site, &site_dir);
    let added = site_ops::enqueue(&site_dir, &opps, now(), COOLDOWN)?;
    job.line(&format!("发现 {} 个机会，新入队 {} 个", opps.len(), added.len()));
    job.step("scan", "ok", &format!("新入队 {} 个", added.len()));
    ctx.emit_queue(&site.id);
    if site.ops.level < 2 {
        job.step("work", "skipped", "第 1 档只分析，不处理");
        job.step("ship", "skipped", "");
        return Ok(format!("发现 {} 个机会，新入队 {} 个", opps.len(), added.len()));
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

/// 按分数挑前 N 个还没处理的条目，按站点档位处理（改、检查、提 PR / 上线）。返回（处理了几个，说明，因预算停下的原因）。
async fn work_queue(ctx: &Ctx, site: &Site, job: &Job, trigger: &str) -> (usize, Vec<String>, Option<String>) {
    let site_dir = ctx.site_dir(&site.id);
    // 全托管：只挑方案允许的动作
    let rules = group_rules(ctx, site);
    let mut picked: Vec<QueueItem> = site_ops::load_queue(&site_dir).into_iter().filter(|i| i.status == "open" && rules.as_ref().is_none_or(|g| g.allows(&i.opp.change))).collect();
    picked.sort_by(|a, b| b.opp.score.total_cmp(&a.opp.score));
    picked.truncate(site.ops.budget.items_per_run.max(1) as usize);
    let mut done = 0;
    let mut notes = Vec::new();
    let mut stopped = None;
    for item in picked {
        if job.canceled() {
            break;
        }
        let b = budget(site, &site_dir, &ctx.state().convos);
        if let Some(why) = over_budget(&b) {
            job.line(&format!("{why}，本次运行暂停处理"));
            notes.push(why.clone());
            if trigger == "schedule" {
                ctx.notify(&format!("{} · 运营预算已用完", site.name), &why);
            }
            stopped = Some(why);
            break;
        }
        job.step("work", "running", &item.opp.title);
        match process(ctx, site, job, &item, false).await {
            Ok(msg) => {
                done += 1;
                notes.push(msg);
            }
            Err(e) => {
                // 失败不重放：标记失败、写明原因，等你处理；继续下一个机会。
                job.line(&format!("「{}」处理失败：{e}", item.opp.title));
                let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| {
                    i.status = "failed".into();
                    i.note = e.clone();
                });
            }
        }
        ctx.emit_queue(&site.id);
    }
    (done, notes, stopped)
}

/// 立即执行一整轮（托管方案里「立即执行」）：制定本周计划 → 按方案处理排在前面的几条
/// （全自动按发布规则自动上线，改好等你确认停在等你确认，只出建议只出计划）。
/// 本周已经有计划、里面还有没做的就沿用，不再花一轮重新制定；本周用量已满就不开始。
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
    let prefix = format!("plan:{}:", chrono::Local::now().format("%G-W%V"));
    let left = site_ops::load_queue(&site_dir).iter().filter(|i| i.status == "open" && i.opp.key.starts_with(&prefix)).count();
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
    if let Some(why) = over_budget(&b) {
        return Err(why);
    }
    job.step("work", "running", &item.opp.title);
    // 你手动点「让 AI 改」：没托管或「只出建议」的站点也按「改好等你确认」处理
    let r = process(ctx, site, job, &item, true).await;
    if let Err(e) = &r {
        let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| {
            i.status = "failed".into();
            i.note = e.clone();
        });
    }
    r
}

/// 批量让 AI 改：按顺序一条一条处理（每条都按站点档位：改、检查、上线或等你确认），额度用完就停下。
/// `ids` 用逗号分隔。
async fn work_many(ctx: &Ctx, site: &Site, job: &Job, ids: &str) -> Result<String, String> {
    let ids: Vec<&str> = ids.split(',').map(str::trim).filter(|x| !x.is_empty()).collect();
    if ids.is_empty() {
        return Err("没有选中的条目".into());
    }
    let (mut ok, mut failed, mut notes) = (0, 0, Vec::new());
    for (n, id) in ids.iter().enumerate() {
        if job.canceled() {
            notes.push("已取消".to_string());
            break;
        }
        let b = budget(site, &ctx.site_dir(&site.id), &ctx.state().convos);
        if let Some(why) = over_budget(&b) {
            job.line(&format!("{why}，剩下的 {} 条没有处理", ids.len() - n));
            notes.push(why);
            break;
        }
        job.line(&format!("—— 第 {}/{} 条 ——", n + 1, ids.len()));
        match work_one(ctx, site, job, id).await {
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
    let wt = site_dir.join("work").join(&item.id);
    if item.branch.is_empty() || item.base_sha.is_empty() || !wt.is_dir() {
        return Err("这条的改动分支已经不在了，只能让 AI 重新改".into());
    }
    let cb = code_base(ctx, site, job).await?;
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

/// 某个机会实际可用的最高档位：新内容、技术问题、被复盘降档的类型最多到第 3 档；只分析档不处理。
pub fn effective_level(site: &Site, item: &QueueItem) -> u8 {
    let mut level = site.ops.level;
    if level >= 5 {
        // 全托管：技术问题、复盘降档的类型留给你确认；新内容是否自动发布在上线那一步按托管设置判断。
        if item.opp.change == "technical" || site.ops.downgraded.iter().any(|d| d == &item.opp.change) {
            return 3;
        }
        return 5;
    }
    if matches!(item.opp.change.as_str(), "new_content" | "technical") || site.ops.downgraded.iter().any(|d| d == &item.opp.change) {
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
        "crawl_issue" | "sitemap_error" => "health",
        "content_gap" => "gap",
        _ => "data",
    }
}

/// 今天（本地时间）已经自动上线了几条。
fn shipped_today(site_dir: &Path) -> usize {
    let start = chrono::Local::now().date_naive().and_hms_opt(0, 0, 0).and_then(|d| d.and_local_timezone(chrono::Local).single()).map(|d| d.timestamp() as u64).unwrap_or(0);
    site_ops::load_queue(site_dir).iter().filter(|i| i.auto && i.deployed_at >= start).count()
}

async fn process(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem, manual: bool) -> Result<String, String> {
    if site.ops.brain == "codex" {
        return Err("无人值守任务不能用 Codex".into());
    }
    let site_dir = ctx.site_dir(&site.id);
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.status = "working".into();
        i.worked_at = now();
        i.note.clear();
    })?;
    ctx.emit_queue(&site.id);
    let level = effective_level(site, item);
    match if manual { level.max(3) } else { level } {
        0 | 1 => Ok(String::new()),
        2 => propose(ctx, site, item).await,
        level => implement(ctx, site, job, item, level).await,
    }
}

fn path_of(url: &str) -> String {
    url.split_once("://")
        .map(|(_, rest)| rest.find('/').map(|i| rest[i..].to_string()).unwrap_or_else(|| "/".into()))
        .unwrap_or_else(|| url.to_string())
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

/// 无人值守任务用的模型：Claude 没选时用 `sonnet` 别名（跟随 CLI 的当前版本）；其它服务必须明确选择。
pub fn ops_model(site: &Site) -> Result<String, String> {
    if !site.ops.model.trim().is_empty() {
        return Ok(site.ops.model.trim().to_string());
    }
    if site.ops.brain == "claude" {
        return Ok("sonnet".into());
    }
    Err("请在站点「运营」设置里为无人值守任务选择模型".into())
}

/// 这个站点无人值守任务用的（主模型，备用模型）。全托管站点用所在托管方案里运营人员选的那一对。
pub fn models_for(site: &Site, data_dir: &Path) -> Result<((String, String), Option<(String, String)>), String> {
    if let Some(m) = crate::managed::load(data_dir).group_of(&site.id).cloned() {
        let pick = |b: &str, model: &str| -> Result<(String, String), String> {
            if !model.trim().is_empty() {
                return Ok((b.to_string(), model.trim().to_string()));
            }
            if b == "claude" {
                return Ok((b.to_string(), "sonnet".into()));
            }
            Err("请在「托管运营 → 托管方案」里为这个方案的主模型 / 备用模型选择具体模型".into())
        };
        let primary = pick(&m.brain, &m.model)?;
        let fallback = if m.fallback_brain.is_empty() { None } else { Some(pick(&m.fallback_brain, &m.fallback_model)?) };
        return Ok((primary, fallback));
    }
    Ok(((site.ops.brain.clone(), ops_model(site)?), None))
}

/// 无人值守地开一条站点会话（origin=ops），跑完首轮才返回。主模型这一轮失败时，有备用模型就换它重跑一次
/// （新开一条会话，接着同一个工作区；备用模型能看到主模型已经做了的改动）。
async fn run_conversation(ctx: &Ctx, site: &Site, perm: &str, work_dir: &str, message: String) -> Result<(String, crate::convo::Conversation), String> {
    let st = ctx.state();
    let (primary, fallback) = models_for(site, &st.data_dir)?;
    match run_conversation_with(ctx, site, perm, work_dir, message.clone(), &primary).await {
        Ok(r) => Ok(r),
        // 作业被用户取消：这一轮是被停掉的，不是模型失败，不换备用模型重跑
        Err(_) if st.site_jobs.is_canceled(&site.id) => Err("已取消".into()),
        Err(e) => {
            let Some(fb) = fallback else { return Err(e) };
            crate::managed::log(&st.data_dir, crate::managed::Activity {
                ts: now(),
                site_id: site.id.clone(),
                site_name: site.name.clone(),
                kind: "fallback".into(),
                title: format!("主模型失败，改用备用模型 {}", format!("{} · {}", fb.0, fb.1)),
                detail: e.lines().next().unwrap_or("").to_string(),
                ..Default::default()
            });
            let note = format!("\n\n（上一次由另一个模型执行时中断了：{}。工作区里可能已有部分改动，先查看现状再继续。）", e.lines().next().unwrap_or(""));
            run_conversation_with(ctx, site, perm, work_dir, format!("{message}{note}"), &fb).await
        }
    }
}

async fn run_conversation_with(ctx: &Ctx, site: &Site, perm: &str, work_dir: &str, message: String, model: &(String, String)) -> Result<(String, crate::convo::Conversation), String> {
    let st = ctx.state();
    let conv_id = uuid::Uuid::new_v4().to_string();
    let _guard = UnattendedGuard::start(&conv_id, st.data_dir.join("permit").join("pending"));
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
        String::new(),
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
    let failed = conv.messages.last().is_some_and(|m| m.role == "assistant" && m.error);
    if failed {
        let e = conv.messages.last().map(|m| m.text.clone()).unwrap_or_default();
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
这一轮只出方案，不要修改文件。我确认后会在这条会话里让你动手。",
        title = item.opp.title,
        ev = evidence_block(&item.opp),
        guide = change_guidance(&item.opp.change),
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
            Ok(CodeBase { main, token: String::new(), base_ref: site.default_branch.clone() })
        }
    }
}

fn page_exists_in(out: &Path) -> impl Fn(&str) -> bool + '_ {
    move |href: &str| {
        let p = href.split(['#', '?']).next().unwrap_or("").trim_start_matches('/');
        if p.is_empty() {
            return out.join("index.html").exists();
        }
        let p = p.trim_end_matches('/');
        out.join(p).join("index.html").exists() || out.join(format!("{p}.html")).exists() || out.join(p).is_file()
    }
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
    let wt = site_dir.join("work").join(&item.id);
    crate::site_git::worktree_at(&cb.main, &wt, &base_sha, Some(&branch)).await?;
    crate::site_git::share_node_modules(&cb.main, &wt);
    crate::site_git::share_node_modules(&site_dir.join("deploy"), &wt);
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.branch = branch.clone();
        i.base_sha = base_sha.clone();
    })?;
    let checks = if site.build.checks.is_empty() { String::new() } else { format!("和检查（{}）", site.build.checks.join("、")) };
    let mut plan_brief = group_rules(ctx, site).map(|g| g.brief()).unwrap_or_default();
    if !item.retry_note.is_empty() {
        plan_brief.push_str(&format!("【上次的问题】这条改动上次上线后，线上验收没通过，已经回滚：{}。这次先找到原因再改；改完确认构建结果里有对应页面、网址和你写的一致。\n", item.retry_note));
    }
    let msg = format!(
        "【运营 · 改动】{title}\n\n{ev}\n{plan_brief}当前目录是 Pilot 为这个任务准备的独立工作区（分支 `{branch}`，基于默认分支 `{base}`）。请直接完成改动：\n\
读站点要省：只看这次要改的页面、它的模板，以及准备互相链接的两三篇，不要逐篇通读；需要查询词等数据用站点数据工具查。\n\
1. 找到这个页面对应的源文件，按上面的依据修改。改动方向：{guide}\n\
2. 改动小而集中，只处理这一个机会；不要改网址、依赖、构建和部署配置，也不要动推荐链接 / 邀请码。\n\
   {code_rule}\n\
3. 改完运行构建（`{build}`）{checks}，确认通过。\n\
4. 用 `git add` 和 `git commit` 提交到当前分支，提交说明一句话写清改了什么（不要写流量数字）；如果提交因为本机没配 git 身份而失败，保留改动即可，Pilot 会替你提交。\n\
不要推送、不要开 PR、不要部署——这些由 Pilot 在审核规则通过后执行；需要确认的操作在这里会被自动拒绝。\n\
完成后用两三句话说明改了哪些文件、为什么这样改。然后在最后两行按这个格式写出这次涉及的公开页面（Pilot 上线后会逐个打开验收）：\n\
新页面：这次新增的页面完整网址，多个用空格分开（没有就写「无」）\n\
改动的页面：内容有变化的已有页面完整网址（新加的文字会出现在哪些页面，都列上）",
        title = item.opp.title,
        ev = evidence_block(&item.opp),
        base = site.default_branch,
        guide = change_guidance(&item.opp.change),
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
    let summary: String = conv.messages.iter().rev().find(|m| m.role == "assistant").map(|m| m.text.trim().chars().take(800).collect()).unwrap_or_default();
    let reply = conv.messages.iter().rev().find(|m| m.role == "assistant").map(|m| m.text.clone()).unwrap_or_default();
    let (new_pages, pages) = crate::site_verify::reported_pages(&reply, &site.domain);
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.conv_id = conv_id.clone();
        i.model_used = format!("{} · {}", conv.brain, conv.model);
        i.ai_summary = summary.clone();
        i.pages = pages.clone();
        i.new_pages = new_pages.clone();
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
async fn verify_and_ship(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem, level: u8, cb: &CodeBase, wt: &Path, base_sha: &str, branch: &str) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let (wt, base_sha, branch) = (wt.to_path_buf(), base_sha.to_string(), branch.to_string());
    let commits = crate::site_git::commits_between(&wt, &base_sha, "HEAD").await;
    if commits.is_empty() {
        crate::site_git::remove_worktree(&cb.main, &wt).await;
        let _ = crate::site_git::git(&cb.main, &["branch", "-D", &branch], "").await;
        return Err("AI 没有做出任何改动（详见会话）".into());
    }
    let head_sha = crate::site_git::head_sha(&wt).await;
    if let Ok(out) = crate::site_git::git_ok(&wt, &["diff", "--name-only", &base_sha, &head_sha], "").await {
        let files: Vec<String> = out.lines().map(str::trim).filter(|l| !l.is_empty()).take(30).map(str::to_string).collect();
        let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| i.files = files.clone());
    }
    // 检查：构建 + 站点自带的检查命令
    let built = crate::site_deploy::build_and_check(site, &wt, None, job).await;
    let diff = crate::site_git::diff_zero_context(&wt, &base_sha, &head_sha).await?;
    let snippets = crate::site_verify::snippets_from_diff(&diff);
    let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| i.snippets = snippets.clone());
    // 上线时用最新的条目（带上 AI 报告的页面和这次的文字）
    let item = &site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item.id).unwrap_or_else(|| item.clone());
    let whitelist: Vec<String> = site.ops.whitelist.iter().filter(|w| !site.ops.downgraded.contains(w)).cloned().collect();
    let out_dir = site.app_dir(&wt).join(&site.build.out_dir);
    let cls = site_ops::classify(&diff, &whitelist, &page_exists_in(&out_dir));
    let change_kind = if cls.kinds.is_empty() { "mixed".to_string() } else { cls.kinds.join(",") };
    let checks_ok = built.is_ok();
    // 全托管的判定比第 4 档白名单宽：页面内容的改动都可以自动上线，但依赖 / 配置 / 代码、推荐与邀请相关的内容、
    // 超大改动永远不自动；新页面按托管设置决定。第 1–4 档仍按白名单。
    let managed = crate::managed::load(&ctx.state().data_dir).rules_for(&site.id);
    let mut reasons = if level >= 5 { crate::managed::managed_verdict(&diff, managed.new_content == "auto", managed.min_page_chars as usize, &managed.code_changes) } else { cls.reasons.clone() };
    // 新页面要有别的页面链到它，不然上线就是孤立页（搜索引擎很难发现）：AI 漏加了就不自动上线
    if built.is_ok() && !item.new_pages.is_empty() {
        let inbound = crate::site_make::inbound_links(&out_dir);
        let count = |p: &str| inbound.iter().find(|(k, _)| crate::site_make::norm_path(k) == crate::site_make::norm_path(p)).map(|(_, n)| *n);
        let lonely: Vec<String> = item.new_pages.iter().map(|u| path_of(u)).filter(|p| count(p) == Some(0)).collect();
        if !lonely.is_empty() {
            reasons.push(format!("新页面 {} 没有任何站内链接指向它（会成孤立页），留给你确认", lonely.join("、")));
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
    if level >= 5 && verdict == "auto" && shipped_today(&site_dir) >= managed.daily_changes as usize {
        verdict = format!("今天已自动上线 {} 条（上限），这一条留给你确认或明天再上", managed.daily_changes);
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
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.status = status.into();
        i.head_sha = head_sha.clone();
        i.change_kind = change_kind.clone();
        i.verdict = verdict.clone();
        i.pr_number = pr.number;
        i.pr_url = pr.url.clone();
        i.note = if checks_ok { String::new() } else { "构建或检查没通过，改动留在分支上，可以在会话里让 AI 继续修。".into() };
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
    if !(level >= 4 && verdict == "auto") {
        let why = if verdict == "auto" { "检查通过，按方案留给你确认后上线".to_string() } else { format!("检查通过，留给你确认：{verdict}") };
        job.step("ship", "skipped", &why);
        job.line(&why);
    }
    // 第 4 档：白名单内自动合并上线；第 5 档：判定通过即自动上线，上线 1 小时后再检查一次页面。
    if level >= 4 && verdict == "auto" {
        job.step("ship", "running", if level >= 5 { "全托管，自动合并上线" } else { "白名单内改动，自动合并上线" });
        match accept(ctx, site, job, &item.id, true).await {
            Ok(m) => {
                if level >= 5 {
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
#[derive(Default)]
struct Shipped {
    url: String,
    checks: Vec<crate::site_verify::Check>,
    attention: Vec<String>,
}

/// 一条改动上线后要验收什么。回滚时只看首页和原来就有的页面（新页面回滚后本来就不该在了）。
fn spec_for(site: &Site, item: &QueueItem, rollback: bool) -> crate::site_verify::Spec {
    let mut spec = crate::site_verify::Spec { domain: site.domain.clone(), ..Default::default() };
    let page = item.opp.page.trim();
    if page.starts_with("https://") && !item.new_pages.iter().any(|n| n == page) {
        spec.pages.push(page.to_string());
        spec.primary.push(page.to_string());
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
async fn ship(ctx: &Ctx, site: &Site, job: &Job, spec: crate::site_verify::Spec, commit: &str, trigger: &str) -> Result<Shipped, String> {
    let st = ctx.state();
    let site_dir = ctx.site_dir(&site.id);
    match site.deploy.mode.as_str() {
        "direct" | "workers" | "vps" => {
            let rec = crate::site_deploy::deploy_with(site, &st.conns, &site_dir, job, trigger, Some(spec)).await?;
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
            let deadline = now() + 15 * 60;
            loop {
                if job.canceled() {
                    return Err("已取消".into());
                }
                if let Ok(list) = crate::cf::list_deployments(&token, &cf.account_id, &site.deploy.project).await {
                    if let Some(d) = list.iter().find(|d| d.environment == "production" && !commit.is_empty() && d.commit.starts_with(&commit[..7.min(commit.len())])) {
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
                    }
                }
                if now() > deadline {
                    return Err("15 分钟内没有等到 Cloudflare 构建完成，请在部署页查看".into());
                }
                tokio::time::sleep(Duration::from_secs(20)).await;
            }
            let rep = crate::site_verify::run(&spec, job, true).await;
            let shipped = Shipped { url: String::new(), checks: rep.checks.clone(), attention: rep.soft.clone() };
            if !rep.hard.is_empty() {
                job.step("verify", "failed", &rep.hard.join("；"));
                return Err(format!("{}{}", crate::site_verify::HARD_PREFIX, rep.hard.join("；")));
            }
            job.step("verify", if rep.checks.is_empty() { "skipped" } else { "ok" }, &if rep.soft.is_empty() { String::new() } else { format!("网站正常，{} 项待核对", rep.soft.len()) });
            if !site.data.indexnow_key.is_empty() && !site.domain.is_empty() && !rep.passed.is_empty() {
                job.step("indexnow", "running", &format!("{} 个网址", rep.passed.len()));
                match crate::bing::indexnow_submit(&site.domain, &site.data.indexnow_key, &rep.passed).await {
                    Ok(_) => job.step("indexnow", "ok", &format!("已提交 {} 个网址", rep.passed.len())),
                    Err(e) => job.step("indexnow", "failed", &e),
                }
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
        i.checks = sh.checks.clone();
        i.attention = attention.clone();
        i.watch_due = now() + 3600;
        i.watch_stage = 0;
        i.watched.clear();
    })?
    .ok_or("这条改动不存在")?;
    if !attention.is_empty() {
        log_item(ctx, site, "attention", &item, &format!("上线了，网站正常；有 {} 项待核对：{attention}", sh.attention.len()));
        follow_up(ctx, site, &item, &attention);
    }
    ctx.emit_queue(&site.id);
    Ok(())
}

/// 全自动方案：验收发现一般问题时开一条「修技术问题」的后续任务（每个站点每天最多一条，防止来回改）。
fn follow_up(ctx: &Ctx, site: &Site, item: &QueueItem, attention: &str) {
    let Some(g) = group_rules(ctx, site) else { return };
    if g.autonomy != "auto" || !g.allows("technical") {
        return;
    }
    let site_dir = ctx.site_dir(&site.id);
    let day = today();
    if site_ops::load_queue(&site_dir).iter().any(|i| i.opp.kind == "followup" && i.opp.key.ends_with(&day)) {
        return;
    }
    let opp = Opportunity {
        key: format!("followup:{}:{day}", item.id),
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
    downgrade_if_repeated(ctx, site, &item.opp.change);
    let item = site_ops::update_item(&site_dir, item_id, now(), |i| i.verify_rollbacks += 1)?.ok_or("这条改动不存在")?;
    let auto = group_rules(ctx, site).is_some_and(|g| g.autonomy == "auto") && crate::managed::load(&ctx.state().data_dir).rules_for(&site.id).autonomy == "auto";
    let can_retry = auto && item.retries == 0 && over_budget(&budget(site, &site_dir, &ctx.state().convos)).is_none();
    if can_retry {
        site_ops::update_item(&site_dir, item_id, now(), |i| {
            i.status = "failed".into();
            i.retries += 1;
            i.retry_note = why.clone();
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
    })?;
    ctx.emit_queue(&site.id);
    ctx.notify(&format!("{} · 上线后验收没通过，已回滚", site.name), &item.opp.title);
    Err(format!("{reason}。已自动回滚"))
}

/// 暂停一个站点的托管（回滚都失败了，网站可能处在坏的状态，需要人来看）。
fn pause_site(ctx: &Ctx, site: &Site, why: &str) {
    let data_dir = ctx.state().data_dir.clone();
    let mut c = crate::managed::load(&data_dir);
    if let Some(g) = c.groups.iter_mut().find(|g| g.sites.contains(&site.id)) {
        if !g.stopped.contains(&site.id) {
            g.stopped.push(site.id.clone());
        }
        let _ = crate::managed::save(&data_dir, &c);
    }
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
        let sh = match ship(ctx, site, job, spec_for(site, &item, false), &item.merge_sha, if auto { "ops" } else { "manual" }).await {
            Ok(sh) => sh,
            Err(e) if e.starts_with(crate::site_verify::HARD_PREFIX) => return on_verify_failed(ctx, site, job, item_id, &e).await,
            Err(e) => return Err(e),
        };
        record_shipped(ctx, site, item_id, &sh)?;
        return Ok(if sh.url.is_empty() { "已上线".into() } else { format!("已上线 {}", sh.url) });
    }
    if item.status != "proposed" {
        return Err("只有待确认的改动可以确认".into());
    }
    if item.branch.is_empty() {
        // 第 2 档的方案：你在会话里处理完了，标记完成即可。
        site_ops::update_item(&site_dir, item_id, now(), |i| i.status = "done".into())?;
        return Ok("已标记为处理完成".into());
    }
    job.step("merge", "running", &item.branch);
    let title = format!("ops: improve {}", path_of(&item.opp.page));
    let merge_sha = match site.source.as_str() {
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
            crate::site_git::remove_worktree(&cb.main, &site_dir.join("work").join(&item.id)).await;
            let _ = crate::site_git::git(&cb.main, &["branch", "-D", &item.branch], "").await;
            sha
        }
        _ => {
            let main = crate::site_git::code_dir(site, None)?;
            let cur = crate::site_git::current_branch(&main).await;
            if cur != site.default_branch || !crate::site_git::is_clean(&main).await {
                return Err(format!("站点文件夹当前不在 {} 分支，或有未提交的改动。请先处理好再确认（Pilot 不会动你正在改的东西）", site.default_branch));
            }
            crate::site_git::remove_worktree(&main, &site_dir.join("work").join(&item.id)).await;
            crate::site_git::git_ok(&main, &["merge", "--squash", &item.branch], "").await?;
            crate::site_git::git_commit_ok(&main, &["commit", "-qm", &title]).await?;
            let _ = crate::site_git::git(&main, &["branch", "-D", &item.branch], "").await;
            crate::site_git::head_sha(&main).await
        }
    };
    job.step("merge", "ok", merge_sha.get(..7).unwrap_or(""));
    let before = page_snapshot(&site_dir, &item.opp.page);
    let (before_query, before_site) = (query_snapshot(&site_dir, &item.opp.expect_query, &item.opp.page), Some(site_snapshot(&site_dir)));
    site_ops::update_item(&site_dir, item_id, now(), |i| {
        i.status = "merged".into();
        i.merge_sha = merge_sha.clone();
        i.auto = auto;
        i.before = before.clone();
        i.before_query = before_query.clone();
        i.before_site = before_site.clone();
    })?;
    ctx.emit_queue(&site.id);
    if site.deploy.mode == "none" {
        return Ok("已合并（站点没有配置部署）".into());
    }
    let sh = match ship(ctx, site, job, spec_for(site, &item, false), &merge_sha, if auto { "ops" } else { "manual" }).await {
        Ok(sh) => sh,
        Err(e) if e.starts_with(crate::site_verify::HARD_PREFIX) => return on_verify_failed(ctx, site, job, item_id, &e).await,
        Err(e) => return Err(e),
    };
    record_shipped(ctx, site, item_id, &sh)?;
    if auto {
        ctx.notify(&format!("{} · 已自动上线", site.name), &item.opp.title);
    }
    Ok(if sh.url.is_empty() { "已上线".into() } else { format!("已上线 {}", sh.url) })
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
                        crate::site_git::remove_worktree(&main, &site_dir.join("work").join(&item.id)).await;
                        let _ = crate::site_git::git(&main, &["branch", "-D", &item.branch], "").await;
                    }
                }
            }
        } else if let Ok(main) = crate::site_git::code_dir(site, None) {
            crate::site_git::remove_worktree(&main, &site_dir.join("work").join(&item.id)).await;
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
    job.step("revert", "running", item.merge_sha.get(..7).unwrap_or(""));
    let revert_sha = match site.source.as_str() {
        "github" => {
            let cb = code_base(ctx, site, job).await?;
            let short: String = item.id.chars().take(6).collect();
            let branch = format!("pilot/revert-{short}");
            let wt = site_dir.join("work").join(format!("revert-{short}"));
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
            let cur = crate::site_git::current_branch(&main).await;
            if cur != site.default_branch || !crate::site_git::is_clean(&main).await {
                return Err(format!("站点文件夹当前不在 {} 分支，或有未提交的改动。请先处理好再回滚", site.default_branch));
            }
            crate::site_git::git_commit_ok(&main, &["revert", "--no-edit", &item.merge_sha]).await?;
            crate::site_git::head_sha(&main).await
        }
    };
    job.step("revert", "ok", revert_sha.get(..7).unwrap_or(""));
    site_ops::update_item(&site_dir, item_id, now(), |i| {
        i.status = "rolled_back".into();
        i.note = format!("已回退（{}）", revert_sha.get(..7).unwrap_or(""));
    })?;
    ctx.emit_queue(&site.id);
    if site.deploy.mode != "none" {
        ship(ctx, site, job, spec_for(site, &item, true), &revert_sha, "rollback").await?;
    }
    Ok("已回滚并重新上线".into())
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
    std::fs::write(d.join(format!("{name}.md")), body).map_err(|e| e.to_string())
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
    format!("{}{:.0}%", if v >= 0.0 { "+" } else { "" }, v * 100.0)
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
    if pb.clicks >= 5.0 && adj_clicks <= -0.15 {
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
    for i in &items {
        let is_new = i.before.is_none();
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
        let kind = i.change_kind.split(',').next().unwrap_or("").to_string();
        let neg = state.negatives.entry(kind.clone()).or_insert(0);
        if verdict == "negative" {
            *neg += 1;
            if *neg >= 2 && !kind.is_empty() && !downgraded.contains(&kind) {
                downgraded.push(kind.clone());
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
        if verdict == "negative" && site.ops.level >= 5 && i.auto {
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
    refresh_data(ctx, site, job).await;
    job.step("plan", "running", "");
    let q = site_ops::load_queue(&site_dir);
    let history: Vec<String> = q
        .iter()
        .filter(|i| !matches!(i.status.as_str(), "open"))
        .take(20)
        .map(|i| format!("- [{}] {}{}", i.status, i.opp.title, if i.review.is_empty() { String::new() } else { format!("（复盘：{}）", i.review) }))
        .collect();
    let n = site.ops.budget.items_per_run.clamp(1, 8) * 2;
    let rules = group_rules(ctx, site);
    let plan_brief = rules.as_ref().map(|g| g.brief()).unwrap_or_default();
    let kinds: Vec<&str> = crate::managed::ACTIONS.iter().copied().filter(|a| rules.as_ref().is_none_or(|g| g.allows(a))).collect();
    let stage = crate::site_stage::stage_of(site, &site_dir);
    let stage_brief = crate::site_stage::plan_brief(site, &site_dir, rules.as_ref().is_some_and(|g| !g.topic_list().is_empty()));
    let health = crate::site_stage::latest_health(&site_dir, if stage.stage == "new" { 2000 } else { 600 });
    let health = if health.is_empty() { String::new() } else { format!("【最近一次体检】\n{health}\n\n") };
    let msg = format!(
        "【托管运营 · 本周计划】你是「{name}」（{domain}）的运营负责人。下面是这个站点的阶段与定位、所在托管方案的运营方式、Pilot 拉到的数据和最近的运营记录；请先读站点的代码和主要页面，然后制定本周最多 {n} 项具体工作。\n读站点要省：先看目录结构、导航和首页，再挑流量最大或这周要动的几个页面看，一共读 15 个文件左右就够，不要逐篇通读所有文章；需要更细的数据（某个页面的查询词、某类查询词）用站点数据工具查，不要靠读文件猜。\n\n{stage_brief}{plan_brief}{health}【数据】\n{brief}\n【最近的运营记录】\n{hist}\n\n要求：\n- 按站点阶段确定本周的重点；只安排「允许的动作」里的事；围绕运营目标；新内容遵守内容准则。\n- 每一项都要落到具体页面或具体内容上，说清为什么（引用上面的数据）；优先做投入小、把握大的。\n- 可以包括规则没发现的事：内容缺口、专题页、内链结构、过时内容更新、技术问题。\n- 长尾选题先对照已有页面：能扩写已有页面就不新开页，意图相近的几个词合并成一页；不要安排用同一模板换关键词批量做的相似页面，也不要安排撑不起一个完整页面的窄题（页面厚度见上）。\n- 读站点时如果发现公开页面混入了内部说明、开发注释、占位图，或有明显偏薄的页面，可以安排「改进已有页面」任务处理。\n- 不要改推荐链接、邀请码、本站自己的价格和优惠、法律页面、依赖或部署配置。文章里引用的第三方工具价格可以更新，但要标明核对日期和来源。\n- 这是无人值守任务，没人回答问题：规则之间有冲突时（比如阶段建议和运营目标不一致），以「允许的动作」和运营目标为准，自己做出判断，把判断和需要运营人员留意的事写进 notes。\n- 这一轮只出计划，不要修改任何文件。\n最后用一个 ```json 代码块输出：{{\"notes\":\"给运营人员的说明：冲突怎么处理的、需要留意或确认的事（没有就留空）\",\"positioning\":\"站点定位，200 字以内：主题、目标读者、语言/地区、已有栏目和内容类型、商业目标\",\"tasks\":[{{\"title\":\"一句话说做什么\",\"page\":\"完整网址（新页面留空）\",\"change\":\"{kinds}\",\"why\":\"依据：引用的数据，或它补的是定位里的哪一块\",\"source\":\"data|gap|topic|positioning|health\",\"query\":\"目标查询词（没有就留空）\",\"priority\":1,\"expect\":{{\"metric\":\"ctr|position|impressions|clicks|health\",\"query\":\"看哪个查询词（看整个页面就留空）\",\"note\":\"一句话写预期，比如：点击率从 2.2% 提到 3% 以上\"}}}}]}}\nexpect 是这条改动预期影响的指标：改标题和描述一般看点击率（ctr），补内容看目标查询词的排名（position），新页面看展现（impressions），修技术问题看体检（health）；按这条任务的实际意图选，上线 4 周后 Pilot 会拿数据对比。\nsource 的含义：data＝数据里看到的问题，gap＝数据里有需求但没有页面，topic＝选题清单，positioning＝根据站点定位推导，health＝体检发现的问题。",
        name = site.name,
        domain = site.domain,
        brief = data_brief(&site_dir),
        hist = if history.is_empty() { "（暂无）".to_string() } else { history.join("\n") },
        kinds = kinds.join("|"),
    );
    job.line(&format!(
        "交给 AI：先读站点（首页、导航、栏目、主要页面）和数据，再按「{}」阶段制定本周计划{}",
        crate::site_stage::label(&stage.stage),
        rules.as_ref().map(|g| format!("，方案「{}」", g.name)).unwrap_or_default()
    ));
    let (conv_id, conv) = run_conversation(ctx, site, "plan", "", msg).await?;
    let reply = conv.messages.iter().rev().find(|m| m.role == "assistant").map(|m| m.text.clone()).unwrap_or_default();
    let (positioning, mut tasks) = crate::managed::parse_plan_full(&reply)?;
    let notes = crate::managed::parse_plan_notes(&reply);
    if !notes.is_empty() {
        job.line(&format!("给你的说明：{notes}"));
    }
    crate::site_stage::update_from_ai(&site_dir, &positioning);
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
    let week = chrono::Local::now().format("%G-W%V").to_string();
    let opps: Vec<Opportunity> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| Opportunity {
            key: format!("plan:{week}:{}", crate::site_git::slugify(&format!("{} {}", t.page, t.title), 48)),
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
    // 同一周重新制定计划：上一份计划里还没动过的条目撤掉，换成这一份（已在处理、已上线的保留）
    let prefix = format!("plan:{week}:");
    let replaced = site_ops::drop_open_with_prefix(&site_dir, &prefix)?;
    if replaced > 0 {
        job.line(&format!("替换本周上一份计划里还没处理的 {replaced} 项"));
    }
    let added = site_ops::enqueue(&site_dir, &opps, now(), COOLDOWN)?;
    let pdir = site_dir.join("plans");
    let _ = std::fs::create_dir_all(&pdir);
    let _ = std::fs::write(pdir.join(format!("{}.json", today())), serde_json::to_vec_pretty(&serde_json::json!({ "date": today(), "conv_id": conv_id, "stage": stage.stage, "notes": notes, "tasks": tasks })).unwrap_or_default());
    log(ctx, site, "planned", &format!("本周计划 {} 项", tasks.len()), &tasks.iter().take(5).map(|t| t.title.clone()).collect::<Vec<_>>().join("；"), "");
    job.step("plan", "ok", &format!("{} 项，新入队 {}", tasks.len(), added.len()));
    ctx.emit_queue(&site.id);
    Ok(format!("本周计划 {} 项", tasks.len()))
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
    i.status == "deployed" && i.watch_due > 0 && now() >= i.watch_due && (i.watched.is_empty() || i.watch_stage == 1)
}

/// 上线后复查（1 小时后、24 小时后）：再做一遍线上验收。页面打不开就自动回滚；
/// 之前待核对的问题好了就清掉，24 小时后还在，全自动方案就开一条后续任务。
async fn watch(ctx: &Ctx, site: &Site, job: &Job) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let due: Vec<QueueItem> = site_ops::load_queue(&site_dir).into_iter().filter(watch_is_due).collect();
    job.step("watch", "running", &format!("{} 条", due.len()));
    let mut out = Vec::new();
    for i in due {
        let rep = crate::site_verify::run(&spec_for(site, &i, false), job, false).await;
        let stage = i.watch_stage;
        let when = if stage == 0 { "上线 1 小时后" } else { "上线 24 小时后" };
        if !rep.hard.is_empty() {
            let failure = rep.hard.join("；");
            site_ops::update_item(&site_dir, &i.id, now(), |x| {
                x.watched = failure.clone();
                x.checks = rep.checks.clone();
            })?;
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
        } else {
            log_item(ctx, site, "attention", &i, &format!("{when}复查：网站正常，{} 项待核对：{attention}", rep.soft.len()));
            if stage >= 1 {
                follow_up(ctx, site, &i, &attention);
            }
            out.push(format!("{} 有 {} 项待核对", i.opp.title, rep.soft.len()));
        }
    }
    ctx.emit_queue(&site.id);
    job.step("watch", "ok", "");
    Ok(out.join("；"))
}

/// 每日摘要：所有托管站点过去 24 小时做了什么、什么需要你。写一份报告并发系统通知。
pub fn digest(app: &tauri::AppHandle) -> Result<String, String> {
    let st = app.state::<crate::AppState>();
    let since = now().saturating_sub(DAY);
    let acts: Vec<crate::managed::Activity> = crate::managed::activity(&st.data_dir, 2000).into_iter().filter(|a| a.ts >= since).collect();
    let cfg = crate::managed::load(&st.data_dir);
    let managed: Vec<Site> = st.sites.list().into_iter().filter(|s| cfg.group_of(&s.id).is_some()).collect();
    let count = |k: &str| acts.iter().filter(|a| a.kind == k).count();
    let mut needs = 0usize;
    let mut md = format!("# 托管日报 · {}\n\n托管 {} 个站点；过去 24 小时 AI 自动上线 {} 项、出计划 {} 次、回滚 {} 项。\n\n", today(), managed.len(), count("shipped"), count("planned"), count("rolled_back"));
    for s in &managed {
        let dir = st.sites.data_dir(&s.id);
        let q = site_ops::load_queue(&dir);
        let waiting: Vec<&QueueItem> = q.iter().filter(|i| site_ops::needs_you(i)).collect();
        needs += waiting.len();
        let sum = crate::site_data::summary(&dir);
        md.push_str(&format!("## {}\n\n", s.name));
        if sum.fetched_at > 0 {
            md.push_str(&format!("- 近 28 天 Google 点击 {:.0}（前 28 天 {:.0}）\n", sum.gsc_clicks, sum.gsc_clicks_prev));
        }
        for a in acts.iter().filter(|a| a.site_id == s.id) {
            let k = match a.kind.as_str() {
                "shipped" => "已上线",
                "planned" => "计划",
                "proposed" => "待你确认",
                "rolled_back" => "已回滚",
                "failed" => "失败",
                "watch_failed" => "上线后检查失败",
                "fallback" => "换用备用模型",
                "budget" => "预算",
                _ => continue,
            };
            md.push_str(&format!("- [{k}] {}{}\n", a.title, if a.detail.is_empty() { String::new() } else { format!("：{}", a.detail.chars().take(120).collect::<String>()) }));
        }
        if !waiting.is_empty() {
            md.push_str(&format!("- 需要你：{} 项（{}）\n", waiting.len(), waiting.iter().take(3).map(|i| i.opp.title.clone()).collect::<Vec<_>>().join("；")));
        }
        md.push('\n');
    }
    let d = crate::managed::digests_dir(&st.data_dir);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(d.join(format!("{}.md", today())), &md).map_err(|e| e.to_string())?;
    let body = format!("{} 个站点 · AI 上线 {} 项 · {} 项需要你", managed.len(), count("shipped"), needs);
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title("托管日报").body(&body).show();
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

/// 一个一个站点拉取数据（不占站点作业，不和构建、运营任务抢）；每个站点拉完发 site-data 事件，开着的看板会刷新。
async fn daily_data_sync(ctx: Ctx) {
    let st = ctx.state();
    // 先把没接上的数据源按域名接上（新建的 GA4 / Search Console 资源第二天就能自动用上）
    let _ = crate::data_cmds::auto_bind_data(&st).await;
    for site in st.sites.list() {
        let d = &site.data;
        if d.gsc_site.is_empty() && d.ga4_property.is_empty() && d.bing_site.is_empty() {
            continue;
        }
        let meta = crate::site_data::refresh(&site, &st.conns, &st.sites.data_dir(&site.id), true).await;
        let errors: Vec<String> = [("GA4", &meta.ga4.error), ("Search Console", &meta.gsc.error), ("Bing", &meta.bing.error)].iter().filter(|(_, e)| !e.is_empty()).map(|(l, e)| format!("{l}：{e}")).collect();
        let _ = ctx.app.emit("site-data", serde_json::json!({ "site_id": site.id, "errors": errors }));
    }
}

pub fn start_scheduler(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        // 启动后稍等，别和窗口初始化抢资源
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            let ctx = Ctx { app: app.clone() };
            let st = ctx.state();
            // 老数据迁移、站点档位与所在方案的自主程度对齐
            let mcfg = crate::managed_cmds::sync(&st);
            // 每日摘要：过了设定的时间、今天还没发过、有托管站点
            let hour: u8 = chrono::Local::now().format("%H").to_string().parse().unwrap_or(0);
            if mcfg.digest && !mcfg.paused && hour >= mcfg.digest_hour && mcfg.last_digest != today() && !mcfg.groups.iter().all(|g| g.sites.is_empty()) {
                let _ = digest(&app);
                let mut c = mcfg.clone();
                c.last_digest = today();
                let _ = crate::managed::save(&st.data_dir, &c);
            }
            // 每天早上 7 点同步一次所有站点的 GA4 / Search Console / Bing（错过了就在之后第一次检查时补上）
            if hour >= DATA_SYNC_HOUR && last_data_sync(&st.data_dir) != today() {
                set_last_data_sync(&st.data_dir, &today());
                daily_data_sync(ctx.clone()).await;
            }
            // 内容分发：新上线的文章写摘要、按渠道的发布方式发出去（见 syndicate）
            crate::syndicate::schedule(&app);
            for site in st.sites.list() {
                if st.site_jobs.running(&site.id) {
                    continue;
                }
                let in_plan = mcfg.group_of(&site.id).is_some();
                if in_plan {
                    // 总暂停、所在方案暂停、或这个站点单独停止托管：都不自动运行
                    if mcfg.paused || !mcfg.group_of(&site.id).is_some_and(|g| g.active_for(&site.id)) {
                        continue;
                    }
                    // 上线后检查优先：有到期的就先跑它
                    let site_dir = st.sites.data_dir(&site.id);
                    if site_ops::load_queue(&site_dir).iter().any(watch_is_due) {
                        let _ = start(ctx.clone(), &site.id, "watch", "schedule", None);
                        continue;
                    }
                }
                let state = load_state(&st.sites.data_dir(&site.id));
                for task in TASKS {
                    // 从没跑过的任务以站点导入时间为起点：刚导入的站点不会立刻在后台跑起构建，第一次在一个周期后。
                    let last = state.last.get(*task).copied().unwrap_or(0).max(site.created_at);
                    // 没有数据源的站点只做体检
                    let needs_data = !matches!(*task, "health" | "plan");
                    let has_data = !site.data.gsc_site.is_empty() || !site.data.ga4_property.is_empty() || !site.data.bing_site.is_empty();
                    if needs_data && !has_data {
                        continue;
                    }
                    if due(schedule_of(&site, task, in_plan), last, now()) {
                        let _ = start(ctx.clone(), &site.id, task, "schedule", None);
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
    }

    #[test]
    fn budget_limits_stop_processing() {
        assert!(over_budget(&Budget { changes_used: 10, changes_limit: 10, tokens_used: 0, tokens_limit: 0 }).is_some());
        assert!(over_budget(&Budget { changes_used: 1, changes_limit: 10, tokens_used: 5, tokens_limit: 5 }).is_some());
        assert!(over_budget(&Budget { changes_used: 1, changes_limit: 10, tokens_used: 1, tokens_limit: 0 }).is_none());
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
}
