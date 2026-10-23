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

pub fn schedule_of<'a>(site: &'a Site, task: &str) -> &'a str {
    // 全托管：扫描和体检每天，计划、周报、复盘每周，不看站点里各自的周期设置。
    if site.ops.level >= 5 {
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
        "watch" => "上线后检查",
        "rollback" => "回滚",
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

/// 托管设置里的每站每周 token 上限（调度器每轮从托管设置刷新一次）。
static MANAGED_TOKENS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(2_000_000);

pub fn budget(site: &Site, site_dir: &Path, convos: &crate::convo::ConvStore) -> Budget {
    let since = now().saturating_sub(7 * DAY);
    let q = site_ops::load_queue(site_dir);
    let changes = q.iter().filter(|i| i.worked_at >= since).count() as u32;
    let conn_id = crate::site_cmds::site_conn_id(&site.id);
    let tokens: u64 = convos
        .list()
        .into_iter()
        .filter(|c| c.conn_id == conn_id && c.origin == "ops" && c.updated_at >= since)
        .map(|c| c.total_tokens)
        .sum();
    let tokens_limit = if site.ops.level >= 5 { MANAGED_TOKENS.load(std::sync::atomic::Ordering::Relaxed) } else { site.ops.budget.tokens_per_week };
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
        "accept" => &[("merge", "合并"), ("prepare", "准备代码"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("upload", "上传"), ("indexnow", "提交 IndexNow")],
        "rollback" => &[("revert", "回退提交"), ("prepare", "准备代码"), ("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("upload", "上传"), ("indexnow", "提交 IndexNow")],
        "work" => &[("work", "处理机会"), ("ship", "提交 / 上线")],
        "plan" => &[("data", "拉取数据"), ("plan", "AI 制定计划")],
        "watch" => &[("watch", "上线后检查")],
        _ => &[],
    }
}

/// 开一个运营作业（后台跑，立即返回作业 id）。
pub fn start(ctx: Ctx, site_id: &str, task: &str, trigger: &str, item_id: Option<String>) -> Result<String, String> {
    let st = ctx.state();
    let site = st.sites.get(site_id)?;
    let job = st.site_jobs.start(Some(ctx.app.clone()), site_id, &format!("ops:{task}"), task_label(task), steps_for(task))?;
    let id = job.id.clone();
    let (task, trigger) = (task.to_string(), trigger.to_string());
    tauri::async_runtime::spawn(async move {
        let started = now();
        let site_dir = ctx.site_dir(&site.id);
        let r = match task.as_str() {
            "scan" => scan(&ctx, &site, &job, &trigger).await,
            "health" => health(&ctx, &site, &job).await,
            "report" => report(&ctx, &site, &job).await,
            "review" => review(&ctx, &site, &job).await,
            "work" => work_one(&ctx, &site, &job, item_id.as_deref().unwrap_or_default()).await,
            "plan" => plan(&ctx, &site, &job).await,
            "watch" => watch(&ctx, &site, &job).await,
            "accept" => accept(&ctx, &site, &job, item_id.as_deref().unwrap_or_default(), false).await,
            "rollback" => rollback(&ctx, &site, &job, item_id.as_deref().unwrap_or_default()).await,
            _ => Err("未知的运营任务".into()),
        };
        let res = r.as_ref().map(|_| ()).map_err(|e| e.clone());
        job.finish(&res);
        if matches!(task.as_str(), "scan" | "health" | "report" | "review" | "plan") {
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
    // 处理：按分数挑前 N 个还没处理的
    let mut picked: Vec<QueueItem> = site_ops::load_queue(&site_dir).into_iter().filter(|i| i.status == "open").collect();
    picked.sort_by(|a, b| b.opp.score.total_cmp(&a.opp.score));
    picked.truncate(site.ops.budget.items_per_run.max(1) as usize);
    let mut done = 0;
    let mut notes = Vec::new();
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
            break;
        }
        job.step("work", "running", &item.opp.title);
        match process(ctx, site, job, &item).await {
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
    job.step("work", "ok", &format!("处理了 {done} 个"));
    let proposed = site_ops::load_queue(&site_dir).iter().filter(|i| i.status == "proposed").count();
    if trigger == "schedule" && done > 0 {
        ctx.notify(&format!("{} · 机会扫描完成", site.name), &format!("处理了 {done} 个机会，{proposed} 条等你确认"));
    }
    Ok(format!("新入队 {} 个，处理 {done} 个。{}", added.len(), notes.join("；")))
}

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
    let r = process(ctx, site, job, &item).await;
    if let Err(e) = &r {
        let _ = site_ops::update_item(&site_dir, &item.id, now(), |i| {
            i.status = "failed".into();
            i.note = e.clone();
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
    });
}

/// 今天（本地时间）已经自动上线了几条。
fn shipped_today(site_dir: &Path) -> usize {
    let start = chrono::Local::now().date_naive().and_hms_opt(0, 0, 0).and_then(|d| d.and_local_timezone(chrono::Local).single()).map(|d| d.timestamp() as u64).unwrap_or(0);
    site_ops::load_queue(site_dir).iter().filter(|i| i.auto && i.deployed_at >= start).count()
}

async fn process(ctx: &Ctx, site: &Site, job: &Job, item: &QueueItem) -> Result<String, String> {
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
    match effective_level(site, item) {
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

/// 这个站点无人值守任务用的（主模型，备用模型）。全托管站点用托管设置里运营人员选的那一对。
pub fn models_for(site: &Site, data_dir: &Path) -> Result<((String, String), Option<(String, String)>), String> {
    if site.ops.level >= 5 {
        let m = crate::managed::load(data_dir);
        let pick = |b: &str, model: &str| -> Result<(String, String), String> {
            if !model.trim().is_empty() {
                return Ok((b.to_string(), model.trim().to_string()));
            }
            if b == "claude" {
                return Ok((b.to_string(), "sonnet".into()));
            }
            Err("请在「托管运营 → 设置」里为主模型 / 备用模型选择具体模型".into())
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
        Err(e) => {
            let Some(fb) = fallback else { return Err(e) };
            crate::managed::log(&st.data_dir, crate::managed::Activity {
                ts: now(),
                site_id: site.id.clone(),
                site_name: site.name.clone(),
                kind: "fallback".into(),
                title: format!("主模型失败，改用备用模型 {}", format!("{} · {}", fb.0, fb.1)),
                detail: e.lines().next().unwrap_or("").to_string(),
                item_id: String::new(),
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
    let channel = tauri::ipc::Channel::<crate::agent::TurnEvent>::new(|_| Ok(()));
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
        Default::default(),
        "ops".into(),
    )
    .await?;
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
    let msg = format!(
        "【运营 · 改动】{title}\n\n{ev}\n当前目录是 Pilot 为这个任务准备的独立工作区（分支 `{branch}`，基于默认分支 `{base}`）。请直接完成改动：\n\
1. 找到这个页面对应的源文件，按上面的依据修改。改动方向：{guide}\n\
2. 改动小而集中，只处理这一个机会；不要改网址、依赖、构建和部署配置，也不要动推荐链接 / 邀请码。\n\
3. 改完运行构建（`{build}`）{checks}，确认通过。\n\
4. 用 `git add` 和 `git commit` 提交到当前分支，提交说明一句话写清改了什么（不要写流量数字）；如果提交因为本机没配 git 身份而失败，保留改动即可，Pilot 会替你提交。\n\
不要推送、不要开 PR、不要部署——这些由 Pilot 在审核规则通过后执行；需要确认的操作在这里会被自动拒绝。\n\
完成后用两三句话说明改了哪些文件、为什么这样改。",
        title = item.opp.title,
        ev = evidence_block(&item.opp),
        base = site.default_branch,
        guide = change_guidance(&item.opp.change),
        build = if site.build.build.is_empty() { "无需构建" } else { &site.build.build },
    );
    job.line(&format!("交给 AI：{}", item.opp.title));
    let (conv_id, conv) = run_conversation(ctx, site, "auto", &wt.to_string_lossy(), msg).await?;
    site_ops::update_item(&site_dir, &item.id, now(), |i| {
        i.conv_id = conv_id.clone();
        i.model_used = format!("{} · {}", conv.brain, conv.model);
    })?;
    // 收尾：没提交的改动由 Pilot 提交；完全没改动就算失败。
    if !crate::site_git::is_clean(&wt).await || crate::site_git::git(&wt, &["status", "--porcelain"], "").await.map(|o| !o.stdout.is_empty()).unwrap_or(false) {
        crate::site_git::git_ok(&wt, &["add", "-A", "--", ".", ":(exclude)node_modules", ":(exclude)uploads"], "").await?;
        let msg = format!("ops: improve {}", path_of(&item.opp.page));
        let _ = crate::site_git::git_commit_ok(&wt, &["commit", "-qm", &msg]).await;
    }
    let commits = crate::site_git::commits_between(&wt, &base_sha, "HEAD").await;
    if commits.is_empty() {
        crate::site_git::remove_worktree(&cb.main, &wt).await;
        let _ = crate::site_git::git(&cb.main, &["branch", "-D", &branch], "").await;
        return Err("AI 没有做出任何改动（详见会话）".into());
    }
    let head_sha = crate::site_git::head_sha(&wt).await;
    // 检查：构建 + 站点自带的检查命令
    let built = crate::site_deploy::build_and_check(site, &wt, None, job).await;
    let diff = crate::site_git::diff_zero_context(&wt, &base_sha, &head_sha).await?;
    let whitelist: Vec<String> = site.ops.whitelist.iter().filter(|w| !site.ops.downgraded.contains(w)).cloned().collect();
    let out_dir = wt.join(&site.build.out_dir);
    let cls = site_ops::classify(&diff, &whitelist, &page_exists_in(&out_dir));
    let change_kind = if cls.kinds.is_empty() { "mixed".to_string() } else { cls.kinds.join(",") };
    let checks_ok = built.is_ok();
    // 全托管的判定比第 4 档白名单宽：页面内容的改动都可以自动上线，但依赖 / 配置 / 代码、推荐与邀请相关的内容、
    // 超大改动永远不自动；新页面按托管设置决定。第 1–4 档仍按白名单。
    let managed = crate::managed::load(&ctx.state().data_dir);
    let reasons = if level >= 5 { crate::managed::managed_verdict(&diff, managed.new_content == "auto") } else { cls.reasons.clone() };
    let mut verdict = if let Err(e) = &built {
        format!("检查没通过：{}", e.lines().next().unwrap_or(""))
    } else if reasons.is_empty() {
        "auto".to_string()
    } else {
        reasons.join("；")
    };
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
            log(ctx, site, "failed", &item.opp.title, &verdict, &item.id);
        }
        return Err(verdict);
    }
    if level >= 5 && verdict != "auto" {
        log(ctx, site, "proposed", &item.opp.title, &verdict, &item.id);
    }
    // 第 4 档：白名单内自动合并上线；第 5 档：判定通过即自动上线，上线 1 小时后再检查一次页面。
    if level >= 4 && verdict == "auto" {
        job.step("ship", "running", if level >= 5 { "全托管，自动合并上线" } else { "白名单内改动，自动合并上线" });
        match accept(ctx, site, job, &item.id, true).await {
            Ok(m) => {
                if level >= 5 {
                    site_ops::update_item(&site_dir, &item.id, now(), |i| i.watch_due = now() + 3600)?;
                    log(ctx, site, "shipped", &item.opp.title, &m, &item.id);
                }
                return Ok(m);
            }
            Err(e) => {
                verdict = format!("自动上线失败：{e}");
                site_ops::update_item(&site_dir, &item.id, now(), |i| {
                    i.status = "proposed".into();
                    i.verdict = verdict.clone();
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
fn page_snapshot(site_dir: &Path, page: &str) -> Option<crate::site_data::Row> {
    let rows: Vec<crate::site_data::Row> = crate::site_data::read_json(&crate::site_data::cache_dir(site_dir), "gsc_pages.json");
    rows.into_iter().find(|r| r.key == page)
}

/// 合并后让改动上线：直接上传模式由 Pilot 构建上传；Pages 绑定 GitHub 时等 Cloudflare 构建完成。
async fn ship(ctx: &Ctx, site: &Site, job: &Job, page: &str, commit: &str, trigger: &str) -> Result<String, String> {
    let st = ctx.state();
    let site_dir = ctx.site_dir(&site.id);
    match site.deploy.mode.as_str() {
        "direct" | "workers" => {
            let rec = crate::site_deploy::deploy_with(site, &st.conns, &site_dir, job, trigger).await?;
            Ok(rec.url)
        }
        "pages_git" => {
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
            if !site.data.indexnow_key.is_empty() && !site.domain.is_empty() && !page.is_empty() {
                job.step("indexnow", "running", page);
                match crate::bing::indexnow_submit(&site.domain, &site.data.indexnow_key, &[page.to_string()]).await {
                    Ok(_) => job.step("indexnow", "ok", page),
                    Err(e) => job.step("indexnow", "failed", &e),
                }
            }
            Ok(String::new())
        }
        _ => Ok(String::new()),
    }
}

/// 确认一条改动：GitHub 合并 PR（squash）、本地分支压缩合并进默认分支，然后上线。`auto`＝第 4 档自动执行。
pub async fn accept(ctx: &Ctx, site: &Site, job: &Job, item_id: &str, auto: bool) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let item = site_ops::load_queue(&site_dir).into_iter().find(|i| i.id == item_id).ok_or("这条改动不存在")?;
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
    site_ops::update_item(&site_dir, item_id, now(), |i| {
        i.status = "merged".into();
        i.merge_sha = merge_sha.clone();
        i.auto = auto;
        i.before = before.clone();
    })?;
    ctx.emit_queue(&site.id);
    if site.deploy.mode == "none" {
        return Ok("已合并（站点没有配置部署）".into());
    }
    let url = ship(ctx, site, job, &item.opp.page, &merge_sha, if auto { "ops" } else { "manual" }).await?;
    site_ops::update_item(&site_dir, item_id, now(), |i| {
        i.status = "deployed".into();
        i.deployed_at = now();
    })?;
    if auto {
        ctx.notify(&format!("{} · 已自动上线", site.name), &item.opp.title);
    }
    Ok(if url.is_empty() { "已上线".into() } else { format!("已上线 {url}") })
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
        ship(ctx, site, job, &item.opp.page, &revert_sha, "rollback").await?;
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
pub fn judge(before: &crate::site_data::Row, after: &crate::site_data::Row) -> &'static str {
    let clicks = if before.clicks > 0.0 { (after.clicks - before.clicks) / before.clicks } else if after.clicks > 0.0 { 1.0 } else { 0.0 };
    let ctr = if before.ctr > 0.0 { (after.ctr - before.ctr) / before.ctr } else { 0.0 };
    if clicks <= -0.15 {
        "negative"
    } else if clicks >= 0.10 || ctr >= 0.10 {
        "positive"
    } else {
        "neutral"
    }
}

async fn review(ctx: &Ctx, site: &Site, job: &Job) -> Result<String, String> {
    let st = ctx.state();
    let site_dir = ctx.site_dir(&site.id);
    refresh_data(ctx, site, job).await;
    job.step("review", "running", "");
    let rows: Vec<crate::site_data::Row> = crate::site_data::read_json(&crate::site_data::cache_dir(&site_dir), "gsc_pages.json");
    let by_page: HashMap<&str, &crate::site_data::Row> = rows.iter().map(|r| (r.key.as_str(), r)).collect();
    let t = now();
    // 上线满 28 天（近 28 天窗口全部落在上线之后）才复盘；60 天后不再补。
    let items: Vec<QueueItem> = site_ops::load_queue(&site_dir)
        .into_iter()
        .filter(|i| i.status == "deployed" && i.review.is_empty() && i.before.is_some() && t.saturating_sub(i.deployed_at) >= 28 * DAY + 3 * DAY && t.saturating_sub(i.deployed_at) <= 60 * DAY)
        .collect();
    let mut state = load_state(&site_dir);
    let mut lines = Vec::new();
    let mut downgraded = site.ops.downgraded.clone();
    for i in &items {
        let before = i.before.clone().unwrap_or_default();
        let after = by_page.get(i.opp.page.as_str()).map(|r| (*r).clone()).unwrap_or_default();
        let verdict = judge(&before, &after);
        let note = format!(
            "点击 {:.0} → {:.0}，点击率 {:.1}% → {:.1}%，排名 {:.1} → {:.1}",
            before.clicks, after.clicks, before.ctr * 100.0, after.ctr * 100.0, before.position, after.position
        );
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
            x.after = Some(after.clone());
        })?;
        lines.push(format!("{}：{}（{note}）", i.opp.title, match verdict { "positive" => "正面", "negative" => "负面", _ => "持平" }));
        // 全托管：效果负面的自动改动直接回滚，并记一笔。
        if verdict == "negative" && site.ops.level >= 5 && i.auto {
            match rollback(ctx, site, job, &i.id).await {
                Ok(_) => log(ctx, site, "rolled_back", &i.opp.title, &format!("效果复盘负面（{note}），已自动回滚"), &i.id),
                Err(e) => log(ctx, site, "failed", &i.opp.title, &format!("效果负面，自动回滚失败：{e}"), &i.id),
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
        let md = format!("# {} 效果复盘 · {}\n\n{}\n", site.name, today(), lines.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n"));
        write_report(&site_dir, &format!("{}-review", today()), &md)?;
    }
    job.step("review", "ok", &format!("复盘 {} 条", items.len()));
    Ok(format!("复盘 {} 条改动", items.len()))
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
    let msg = format!(
        "【托管运营 · 本周计划】你是「{name}」（{domain}）的运营负责人。下面是 Pilot 拉到的数据和最近的运营记录；请再读一下站点的代码和主要页面（目录结构、首页、流量最大的几个页面），然后制定本周最多 {n} 项具体工作。\n\n【数据】\n{brief}\n【最近的运营记录】\n{hist}\n\n要求：\n- 每一项都要落到具体页面或具体内容上，说清为什么（引用上面的数据）；优先做投入小、把握大的。\n- 可以包括规则没发现的事：内容缺口、专题页、内链结构、过时内容更新、技术问题。\n- 不要提改推荐链接、邀请码、价格、法律页面、依赖或部署配置。\n- 这一轮只出计划，不要修改任何文件。\n最后用一个 ```json 代码块输出计划：[{{\"title\":\"一句话说做什么\",\"page\":\"完整网址（新页面留空）\",\"change\":\"meta|content|internal_links|new_content|technical\",\"why\":\"依据\",\"priority\":1}}]",
        name = site.name,
        domain = site.domain,
        brief = data_brief(&site_dir),
        hist = if history.is_empty() { "（暂无）".to_string() } else { history.join("\n") },
    );
    let (conv_id, conv) = run_conversation(ctx, site, "plan", "", msg).await?;
    let reply = conv.messages.iter().rev().find(|m| m.role == "assistant").map(|m| m.text.clone()).unwrap_or_default();
    let tasks = crate::managed::parse_plan(&reply)?;
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
            score: 1000.0 - (t.priority.max(1) as f64) * 10.0 - i as f64,
            ..Default::default()
        })
        .collect();
    let added = site_ops::enqueue(&site_dir, &opps, now(), COOLDOWN)?;
    let pdir = site_dir.join("plans");
    let _ = std::fs::create_dir_all(&pdir);
    let _ = std::fs::write(pdir.join(format!("{}.json", today())), serde_json::to_vec_pretty(&serde_json::json!({ "date": today(), "conv_id": conv_id, "tasks": tasks })).unwrap_or_default());
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

async fn page_ok(url: &str) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..2 {
        match reqwest::Client::new().get(url).timeout(Duration::from_secs(20)).send().await {
            Ok(r) if r.status().is_success() || r.status().is_redirection() => return Ok(()),
            Ok(r) => last = format!("返回 {}", r.status()),
            Err(e) => last = format!("打不开：{e}"),
        }
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
    Err(last)
}

/// 上线 1 小时后检查：改动的页面和首页都要能正常打开，打不开就自动回滚。
async fn watch(ctx: &Ctx, site: &Site, job: &Job) -> Result<String, String> {
    let site_dir = ctx.site_dir(&site.id);
    let due: Vec<QueueItem> = site_ops::load_queue(&site_dir).into_iter().filter(|i| i.status == "deployed" && i.watch_due > 0 && i.watched.is_empty() && now() >= i.watch_due).collect();
    job.step("watch", "running", &format!("{} 条", due.len()));
    let mut out = Vec::new();
    for i in due {
        let mut urls = vec![format!("https://{}/", site.domain)];
        if !i.opp.page.is_empty() && i.opp.page.starts_with("https://") {
            urls.insert(0, i.opp.page.clone());
        }
        let mut failure = String::new();
        for u in &urls {
            if let Err(e) = page_ok(u).await {
                failure = format!("{u} {e}");
                break;
            }
        }
        if failure.is_empty() {
            site_ops::update_item(&site_dir, &i.id, now(), |x| x.watched = "ok".into())?;
            log(ctx, site, "watch_ok", &i.opp.title, "上线后页面正常", &i.id);
            out.push(format!("{} 正常", i.opp.title));
        } else {
            site_ops::update_item(&site_dir, &i.id, now(), |x| x.watched = failure.clone())?;
            log(ctx, site, "watch_failed", &i.opp.title, &failure, &i.id);
            match rollback(ctx, site, job, &i.id).await {
                Ok(_) => log(ctx, site, "rolled_back", &i.opp.title, &format!("上线后检查失败（{failure}），已自动回滚"), &i.id),
                Err(e) => log(ctx, site, "failed", &i.opp.title, &format!("自动回滚失败：{e}"), &i.id),
            }
            ctx.notify(&format!("{} · 已自动回滚", site.name), &i.opp.title);
            out.push(format!("{} 已回滚", i.opp.title));
        }
    }
    job.step("watch", "ok", "");
    Ok(out.join("；"))
}

/// 每日摘要：所有托管站点过去 24 小时做了什么、什么需要你。写一份报告并发系统通知。
pub fn digest(app: &tauri::AppHandle) -> Result<String, String> {
    let st = app.state::<crate::AppState>();
    let since = now().saturating_sub(DAY);
    let acts: Vec<crate::managed::Activity> = crate::managed::activity(&st.data_dir, 2000).into_iter().filter(|a| a.ts >= since).collect();
    let managed: Vec<Site> = st.sites.list().into_iter().filter(|s| s.ops.level >= 5).collect();
    let count = |k: &str| acts.iter().filter(|a| a.kind == k).count();
    let mut needs = 0usize;
    let mut md = format!("# 托管日报 · {}\n\n托管 {} 个站点；过去 24 小时 AI 自动上线 {} 项、出计划 {} 次、回滚 {} 项。\n\n", today(), managed.len(), count("shipped"), count("planned"), count("rolled_back"));
    for s in &managed {
        let dir = st.sites.data_dir(&s.id);
        let q = site_ops::load_queue(&dir);
        let waiting: Vec<&QueueItem> = q.iter().filter(|i| matches!(i.status.as_str(), "proposed" | "failed")).collect();
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

pub fn start_scheduler(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        // 启动后稍等，别和窗口初始化抢资源
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            let ctx = Ctx { app: app.clone() };
            let st = ctx.state();
            let mcfg = crate::managed::load(&st.data_dir);
            MANAGED_TOKENS.store(mcfg.tokens_per_week, std::sync::atomic::Ordering::Relaxed);
            // 每日摘要：过了设定的时间、今天还没发过、有托管站点
            let hour: u8 = chrono::Local::now().format("%H").to_string().parse().unwrap_or(0);
            if mcfg.digest && !mcfg.paused && hour >= mcfg.digest_hour && mcfg.last_digest != today() && st.sites.list().iter().any(|s| s.ops.level >= 5) {
                let _ = digest(&app);
                let mut c = mcfg.clone();
                c.last_digest = today();
                let _ = crate::managed::save(&st.data_dir, &c);
            }
            for site in st.sites.list() {
                if st.site_jobs.running(&site.id) {
                    continue;
                }
                if site.ops.level >= 5 {
                    if mcfg.paused {
                        continue;
                    }
                    // 上线后检查优先：有到期的就先跑它
                    let site_dir = st.sites.data_dir(&site.id);
                    if site_ops::load_queue(&site_dir).iter().any(|i| i.status == "deployed" && i.watch_due > 0 && i.watched.is_empty() && now() >= i.watch_due) {
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
                    if due(schedule_of(&site, task), last, now()) {
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
    fn reviews_judge_clicks_and_ctr() {
        let r = |c: f64, ctr: f64| Row { clicks: c, ctr, ..Default::default() };
        assert_eq!(judge(&r(100.0, 0.02), &r(80.0, 0.02)), "negative");
        assert_eq!(judge(&r(100.0, 0.02), &r(115.0, 0.02)), "positive");
        assert_eq!(judge(&r(100.0, 0.02), &r(100.0, 0.025)), "positive");
        assert_eq!(judge(&r(100.0, 0.02), &r(95.0, 0.02)), "neutral");
        assert_eq!(judge(&r(0.0, 0.0), &r(3.0, 0.01)), "positive");
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
