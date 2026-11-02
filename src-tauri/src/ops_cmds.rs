//! 运营相关的前端命令：队列、任务、报告。

use serde::Serialize;

use crate::site_ops::{self, Opportunity, QueueItem};
use crate::site_runner::{self, Ctx};
use crate::AppState;

fn ctx(app: &tauri::AppHandle) -> Ctx {
    Ctx { app: app.clone() }
}

#[tauri::command]
pub fn site_queue(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<QueueItem>, String> {
    state.sites.get(&id)?;
    Ok(site_ops::load_queue(&state.sites.data_dir(&id)))
}

/// 手动把一个机会放进待处理队列（驳回过的也可以重新放进来）。
#[tauri::command]
pub fn queue_add(state: tauri::State<'_, AppState>, id: String, opp: Opportunity) -> Result<QueueItem, String> {
    state.sites.get(&id)?;
    add_to_queue(&state.sites.data_dir(&id), opp)
}

/// 放进待处理：同一个机会已经在队列里（没上线、没处理完）就返回那一条，不重复建
fn add_to_queue(dir: &std::path::Path, opp: Opportunity) -> Result<QueueItem, String> {
    let now = crate::now_secs();
    site_ops::with_queue(&dir, |q| {
        q.retain(|i| !(i.opp.key == opp.key && matches!(i.status.as_str(), "dismissed" | "failed")));
        if let Some(existing) = q.iter().find(|i| i.opp.key == opp.key && !matches!(i.status.as_str(), "deployed" | "rolled_back" | "done")) {
            return existing.clone();
        }
        let item = QueueItem { id: uuid::Uuid::new_v4().to_string(), opp, status: "open".into(), created_at: now, updated_at: now, ..Default::default() };
        q.insert(0, item.clone());
        item
    })
}

#[derive(serde::Serialize)]
pub struct HealthTask {
    /// 放进待处理的条目（已经在队列里的就是原来那条）
    pub items: Vec<QueueItem>,
    /// 这次让 AI 开始处理了没有（站点正在跑别的作业、或者都已经在处理 / 等你确认，就没开始）
    pub started: bool,
    pub note: String,
}

/// 站点健康里「交给 AI 优化 / 修」：what = speed（页面速度，url 为空就把有问题的页面打包成一条）| audit（页面体检）。
/// 放进待处理后马上让 AI 处理；这几类改动都要你确认后才上线（见 site_runner::effective_level）。
#[tauri::command]
pub fn health_to_ai(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String, what: String, url: Option<String>) -> Result<HealthTask, String> {
    let site = state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    let opps = match what.as_str() {
        "speed" => crate::speed::task(&dir, url.as_deref()).into_iter().collect(),
        "audit" => site_ops::with_full_pages(crate::page_audit::tasks(&dir), &site.domain),
        _ => return Err("未知的检查项".into()),
    };
    if opps.is_empty() {
        return Err("没有要处理的问题，先重新检查一下".into());
    }
    let items = opps.into_iter().map(|o| add_to_queue(&dir, o)).collect::<Result<Vec<_>, _>>()?;
    let todo: Vec<String> = items.iter().filter(|i| i.status == "open").map(|i| i.id.clone()).collect();
    if todo.is_empty() {
        return Ok(HealthTask { items, started: false, note: "已经交给 AI 了，在待处理里".into() });
    }
    let started = match todo.as_slice() {
        [one] => site_runner::start(ctx(&app), &id, "work", "manual", Some(one.clone())),
        many => site_runner::start(ctx(&app), &id, "batch", "manual", Some(many.join(","))),
    };
    Ok(match started {
        Ok(_) => HealthTask { items, started: true, note: String::new() },
        Err(e) => HealthTask { items, started: false, note: format!("已放进待处理，{e}") },
    })
}

#[tauri::command]
pub fn run_ops_task(app: tauri::AppHandle, id: String, task: String, hold: Option<bool>) -> Result<String, String> {
    if !site_runner::TASKS.contains(&task.as_str()) && task != "cycle" {
        return Err("未知的运营任务".into());
    }
    // hold：「执行但不上线」——这一轮改好的都留给你确认（只对立即执行一轮有用）
    let arg = (task == "cycle" && hold == Some(true)).then(|| "hold".to_string());
    site_runner::start(ctx(&app), &id, &task, "manual", arg)
}

/// 立即按站点档位处理队列里的一条机会。`model`：这一次临时换的模型（brain, model），不改站点和托管方案的设置。
#[tauri::command]
pub fn queue_work(app: tauri::AppHandle, id: String, item_id: String, model: Option<(String, String)>, effort: Option<String>) -> Result<String, String> {
    site_runner::start_with_pick(ctx(&app), &id, "work", "manual", Some(item_id), model.map(|(b, m)| (b, m, effort.unwrap_or_default())))
}

/// 这个站点让 AI 改时默认用的模型（brain, model）：托管站点用方案里的主模型，否则用站点运营设置的；没设好返回空。
#[tauri::command]
/// 返回（服务，模型，说明）：主模型刚因为余额 / 密钥这类原因失败过、有备用模型时，返回的是这会儿实际会用的备用模型，
/// 说明里写主模型为什么用不了（按钮上照实写「用 XX 改」）。
pub fn site_ops_model(state: tauri::State<'_, AppState>, id: String) -> Result<(String, String, String), String> {
    let site = state.sites.get(&id)?;
    let Ok((main, fb)) = site_runner::models_for(&site, &state.data_dir) else { return Ok(Default::default()) };
    match (site_runner::model_down(&main), fb) {
        (Some(why), Some(fb)) => Ok((fb.0, fb.1, format!("主模型 {} · {} 现在用不了（{why}），先用备用模型；充值或修好后最多 30 分钟自动换回", main.0, main.1))),
        _ => Ok((main.0, main.1, String::new())),
    }
}

/// 批量让 AI 改（按顺序处理选中的条目）。
#[tauri::command]
pub fn queue_work_many(app: tauri::AppHandle, id: String, item_ids: Vec<String>, model: Option<(String, String)>, effort: Option<String>) -> Result<String, String> {
    if item_ids.iter().any(|x| x.contains(',')) {
        return Err("条目 id 不合法".into());
    }
    // model：这一批临时换的模型，不改站点和托管方案的设置
    site_runner::start_with_pick(ctx(&app), &id, "batch", "manual", Some(item_ids.join(",")), model.map(|(b, m)| (b, m, effort.unwrap_or_default())))
}

/// 失败的改动重新构建、检查并按规则上线（不再让 AI 改一遍）。
#[tauri::command]
pub fn queue_recheck(app: tauri::AppHandle, id: String, item_id: String) -> Result<String, String> {
    site_runner::start(ctx(&app), &id, "recheck", "manual", Some(item_id))
}

#[tauri::command]
pub fn queue_accept(app: tauri::AppHandle, id: String, item_id: String) -> Result<String, String> {
    site_runner::start(ctx(&app), &id, "accept", "manual", Some(item_id))
}

/// 一起合并上线：几条待确认的改动按顺序合并，合完只上线一次。
#[tauri::command]
pub fn queue_accept_many(app: tauri::AppHandle, id: String, item_ids: Vec<String>) -> Result<String, String> {
    if item_ids.is_empty() {
        return Err("没有选中改动".into());
    }
    site_runner::start(ctx(&app), &id, "accept_many", "manual", Some(item_ids.join(",")))
}

#[tauri::command]
pub fn queue_rollback(app: tauri::AppHandle, id: String, item_id: String) -> Result<String, String> {
    site_runner::start(ctx(&app), &id, "rollback", "manual", Some(item_id))
}

#[tauri::command]
pub async fn queue_dismiss(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String, item_id: String) -> Result<(), String> {
    let site = state.sites.get(&id)?;
    site_runner::dismiss(&ctx(&app), &site, &item_id).await
}

/// 「AI 怎么改的」是英文时翻成中文：用本机 Claude 客户端的小模型（Haiku）；没装就用站点运营用的 API 连接的小模型。
/// 译文存在条目上，同一条只翻一次。
#[tauri::command]
pub async fn queue_translate_summary(state: tauri::State<'_, AppState>, id: String, item_id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    let item = site_ops::load_queue(&dir).into_iter().find(|i| i.id == item_id).ok_or("这条机会不存在")?;
    if !item.ai_summary_zh.is_empty() {
        return Ok(item.ai_summary_zh);
    }
    if item.ai_summary.trim().is_empty() {
        return Err("没有要翻译的说明".into());
    }
    let prompt = format!("把下面这段改动说明翻成简体中文。文件名、代码、网址、英文页面标题原样保留，只输出译文，不要加任何解释：\n\n{}", item.ai_summary);
    let ask = |brain: &'static str| crate::oneshot::Ask { brain, model: "", prompt: &prompt, max_tokens: 1200, small: true, timeout: std::time::Duration::from_secs(90) };
    let mut out = crate::oneshot::complete(&state.data_dir, &ask("claude")).await;
    if out.is_err() {
        // 本机没有 Claude 客户端：用站点运营的 API 连接
        if let Ok(((brain, model), _)) = site_runner::models_for(&site, &state.data_dir) {
            if brain.starts_with("api:") {
                let a = crate::oneshot::Ask { brain: &brain, model: &model, prompt: &prompt, max_tokens: 1200, small: true, timeout: std::time::Duration::from_secs(90) };
                out = crate::oneshot::complete(&state.data_dir, &a).await;
            }
        }
    }
    let zh = out.map_err(|e| format!("翻译没成功：{e}"))?.trim().to_string();
    if zh.is_empty() {
        return Err("翻译没成功：模型没有返回内容".into());
    }
    site_ops::update_item(&dir, &item_id, crate::now_secs(), |i| i.ai_summary_zh = zh.clone())?;
    Ok(zh)
}

/// 把驳回 / 失败的条目放回待处理。
/// 待你确认的条目里，哪些合不进主分支（AI 改的时候主分支后来又有了改动、改到同几处）。
/// 只看本地来源的站点：用 git merge-tree 在内存里试合并，不动文件夹；GitHub 来源由 PR 自己显示能不能合。
#[tauri::command]
pub async fn queue_conflicts(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<String>, String> {
    let site = state.sites.get(&id)?;
    Ok(site_runner::conflicting(&site, &state.sites.data_dir(&id)).await)
}

/// 「让 AI 在最新代码上重改」：待你确认的这一版合不进主分支。先试不花钱的——用 git 把这版接到最新的主分支上，
/// 接上了就重新检查；接不上再让 AI 带着上一版的改动在最新代码上重做（见 site_runner::resolve）。
#[tauri::command]
pub fn queue_redo(app: tauri::AppHandle, id: String, item_id: String) -> Result<String, String> {
    site_runner::start(ctx(&app), &id, "resolve", "manual", Some(item_id))
}

/// 站点的「算网站内容的代码文件」：加上（on）或去掉一个路径。返回新的清单。
/// 只收被一律拦着、又不属于依赖 / 构建 / 部署配置的文件（比如 Go 站的 internal/site/build.go）。
#[tauri::command]
pub fn site_content_code(state: tauri::State<'_, AppState>, id: String, path: String, on: bool) -> Result<Vec<String>, String> {
    let p = path.trim().trim_start_matches("./").to_string();
    if p.is_empty() || p.starts_with('/') || p.split('/').any(|s| s == "..") {
        return Err("请填站点代码里的相对路径，比如 internal/site/build.go，或以 / 结尾的目录".into());
    }
    if on {
        if crate::managed::never_allow(&p) {
            return Err(format!("{p} 是依赖、构建 / 部署配置或脚本，不能放行"));
        }
        if !p.ends_with('/') && !crate::managed::hard_forbidden(&p) {
            return Err(format!("{p} 本来就按托管方案的「拦截等级」判断，不用放行"));
        }
    }
    let site = state.sites.update(&id, crate::now_secs(), |s| {
        s.ops.content_code.retain(|x| x.trim().trim_start_matches("./") != p);
        if on {
            s.ops.content_code.push(p.clone());
        }
        Ok(())
    })?;
    Ok(site.ops.content_code)
}

#[tauri::command]
pub fn queue_restore(state: tauri::State<'_, AppState>, id: String, item_id: String) -> Result<(), String> {
    let dir = state.sites.data_dir(&id);
    site_ops::update_item(&dir, &item_id, crate::now_secs(), |i| {
        if matches!(i.status.as_str(), "dismissed" | "failed") {
            i.status = "open".into();
            i.note.clear();
        }
    })?;
    Ok(())
}

/// 用户在会话里处理完了（第 2 档方案或失败后手动修好）：标记完成。
#[tauri::command]
pub fn queue_mark_done(state: tauri::State<'_, AppState>, id: String, item_id: String) -> Result<(), String> {
    let dir = state.sites.data_dir(&id);
    site_ops::update_item(&dir, &item_id, crate::now_secs(), |i| i.status = "done".into())?;
    Ok(())
}

/// 上线后验收的待核对问题：你看过了，清掉。
#[tauri::command]
pub fn queue_ack(state: tauri::State<'_, AppState>, id: String, item_id: String) -> Result<(), String> {
    let dir = state.sites.data_dir(&id);
    site_ops::update_item(&dir, &item_id, crate::now_secs(), |i| i.attention.clear())?;
    // 你核对过、认为没问题：因为这些问题开出来、还没动的修复任务一起撤掉
    site_runner::dismiss_followups(&dir, &item_id, "上线后验收的问题你已核对过，不用修");
    Ok(())
}

/// 改动的差异（给人看）。
#[tauri::command]
pub async fn queue_diff(state: tauri::State<'_, AppState>, id: String, item_id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    let item = site_ops::load_queue(&dir).into_iter().find(|i| i.id == item_id).ok_or("这条改动不存在")?;
    if item.base_sha.is_empty() || item.head_sha.is_empty() {
        return Err("这条还没有代码改动".into());
    }
    let gh = state.conns.get(&site.gh_conn_id).ok();
    let main = crate::site_git::code_dir(&site, gh.as_ref())?;
    let d = crate::site_git::diff_for_review(&main, &item.base_sha, &item.head_sha).await?;
    Ok(d.chars().take(200_000).collect())
}

#[derive(Serialize)]
pub struct OpsOverview {
    pub state: site_runner::OpsState,
    pub budget: site_runner::Budget,
    /// 各任务下次计划运行时间（0＝关闭）。
    pub next: std::collections::HashMap<String, u64>,
}

#[tauri::command]
pub fn ops_overview(state: tauri::State<'_, AppState>, id: String) -> Result<OpsOverview, String> {
    let site = state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    let st = site_runner::load_state(&dir);
    let mut next = std::collections::HashMap::new();
    let cfg = crate::managed::load(&state.data_dir);
    for t in site_runner::TASKS {
        let sched = site_runner::schedule_of(&site, t, cfg.group_of(&id));
        let n = site_runner::next_due(&sched, st.last.get(*t).copied().unwrap_or(0).max(site.created_at)).unwrap_or(0);
        next.insert(t.to_string(), n);
    }
    Ok(OpsOverview { budget: site_runner::budget(&site, &dir, &state.convos), state: st, next })
}

#[derive(Serialize)]
pub struct ReportEntry {
    pub name: String,
    pub modified: u64,
}

#[tauri::command]
pub fn list_reports(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<ReportEntry>, String> {
    state.sites.get(&id)?;
    let d = site_runner::reports_dir(&state.sites.data_dir(&id));
    let mut out: Vec<ReportEntry> = std::fs::read_dir(&d)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().strip_suffix(".md")?.to_string();
                    let modified = e.metadata().ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
                    Some(ReportEntry { name, modified })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(out)
}

#[tauri::command]
pub fn read_report(state: tauri::State<'_, AppState>, id: String, name: String) -> Result<String, String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("报告名不合法".into());
    }
    let p = site_runner::reports_dir(&state.sites.data_dir(&id)).join(format!("{name}.md"));
    std::fs::read_to_string(p).map_err(|_| "报告不存在".to_string())
}
