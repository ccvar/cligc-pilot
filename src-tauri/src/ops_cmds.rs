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
    let dir = state.sites.data_dir(&id);
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

#[tauri::command]
pub fn run_ops_task(app: tauri::AppHandle, id: String, task: String) -> Result<String, String> {
    if !site_runner::TASKS.contains(&task.as_str()) && task != "cycle" {
        return Err("未知的运营任务".into());
    }
    site_runner::start(ctx(&app), &id, &task, "manual", None)
}

/// 立即按站点档位处理队列里的一条机会。
#[tauri::command]
pub fn queue_work(app: tauri::AppHandle, id: String, item_id: String) -> Result<String, String> {
    site_runner::start(ctx(&app), &id, "work", "manual", Some(item_id))
}

#[tauri::command]
pub fn queue_accept(app: tauri::AppHandle, id: String, item_id: String) -> Result<String, String> {
    site_runner::start(ctx(&app), &id, "accept", "manual", Some(item_id))
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

/// 把驳回 / 失败的条目放回待处理。
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
    for t in site_runner::TASKS {
        let in_plan = crate::managed::load(&state.data_dir).group_of(&id).is_some();
        let sched = site_runner::schedule_of(&site, t, in_plan);
        let n = site_runner::period(sched).map(|p| st.last.get(*t).copied().unwrap_or(0).max(site.created_at) + p).unwrap_or(0);
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
