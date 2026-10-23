//! 托管运营控制台的前端命令。

use serde::Serialize;

use crate::managed::{self, ManagedConfig};
use crate::AppState;

#[tauri::command]
pub fn managed_config(state: tauri::State<'_, AppState>) -> ManagedConfig {
    managed::load(&state.data_dir)
}

#[tauri::command]
pub fn save_managed_config(state: tauri::State<'_, AppState>, config: ManagedConfig) -> Result<ManagedConfig, String> {
    managed::validate(&config)?;
    let mut c = config;
    // 摘要的「今天已发」由调度器维护，前端不改它
    c.last_digest = managed::load(&state.data_dir).last_digest;
    managed::save(&state.data_dir, &c)?;
    Ok(c)
}

#[tauri::command]
pub fn set_managed_paused(state: tauri::State<'_, AppState>, paused: bool) -> Result<ManagedConfig, String> {
    let mut c = managed::load(&state.data_dir);
    c.paused = paused;
    managed::save(&state.data_dir, &c)?;
    managed::log(&state.data_dir, managed::Activity { ts: crate::now_secs(), kind: if paused { "paused" } else { "resumed" }.into(), title: if paused { "暂停托管" } else { "恢复托管" }.into(), ..Default::default() });
    Ok(c)
}

/// 把站点设为全托管（第 5 档），或退回第 3 档（自动提 PR，你确认后上线）。
#[tauri::command]
pub fn set_sites_managed(state: tauri::State<'_, AppState>, ids: Vec<String>, managed: bool) -> Result<usize, String> {
    let mut n = 0;
    for id in ids {
        let site = state.sites.update(&id, crate::now_secs(), |s| {
            s.ops.level = if managed { 5 } else if s.ops.level >= 5 { 3 } else { s.ops.level };
            Ok(())
        })?;
        managed::log(&state.data_dir, managed::Activity {
            ts: crate::now_secs(),
            site_id: site.id.clone(),
            site_name: site.name.clone(),
            kind: if managed { "joined" } else { "left" }.into(),
            title: if managed { "开始托管" } else { "退出托管（改为第 3 档）" }.into(),
            ..Default::default()
        });
        n += 1;
    }
    Ok(n)
}

#[derive(Serialize)]
pub struct ManagedSite {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub shipped_week: usize,
    pub waiting: usize,
    pub failed: usize,
    pub plan_date: String,
    pub plan_count: usize,
    pub budget: crate::site_runner::Budget,
    pub job: String,
}

#[derive(Serialize)]
pub struct ManagedOverview {
    pub config: ManagedConfig,
    pub sites: Vec<ManagedSite>,
    pub shipped_today: usize,
    pub waiting: usize,
}

#[tauri::command]
pub fn managed_overview(state: tauri::State<'_, AppState>) -> ManagedOverview {
    let since_week = crate::now_secs().saturating_sub(7 * 86_400);
    let since_day = crate::now_secs().saturating_sub(86_400);
    let mut shipped_today = 0;
    let mut waiting_total = 0;
    let sites = state
        .sites
        .list()
        .into_iter()
        .filter(|s| s.ops.level >= 5)
        .map(|s| {
            let dir = state.sites.data_dir(&s.id);
            let q = crate::site_ops::load_queue(&dir);
            let shipped_week = q.iter().filter(|i| i.auto && i.deployed_at >= since_week).count();
            shipped_today += q.iter().filter(|i| i.auto && i.deployed_at >= since_day).count();
            let waiting = q.iter().filter(|i| i.status == "proposed").count();
            waiting_total += waiting;
            let plan = crate::site_runner::latest_plan(&dir);
            ManagedSite {
                failed: q.iter().filter(|i| i.status == "failed").count(),
                plan_date: plan.as_ref().and_then(|p| p["date"].as_str().map(str::to_string)).unwrap_or_default(),
                plan_count: plan.as_ref().and_then(|p| p["tasks"].as_array().map(|a| a.len())).unwrap_or(0),
                budget: crate::site_runner::budget(&s, &dir, &state.convos),
                job: state.site_jobs.snapshot(&s.id).filter(|j| j.status == "running").map(|j| j.title).unwrap_or_default(),
                id: s.id,
                name: s.name,
                domain: s.domain,
                shipped_week,
                waiting,
            }
        })
        .collect();
    ManagedOverview { config: managed::load(&state.data_dir), sites, shipped_today, waiting: waiting_total }
}

#[tauri::command]
pub fn managed_activity(state: tauri::State<'_, AppState>, limit: Option<usize>) -> Vec<managed::Activity> {
    managed::activity(&state.data_dir, limit.unwrap_or(200).min(2000))
}

#[derive(Serialize)]
pub struct SitePlan {
    pub site_id: String,
    pub site_name: String,
    pub plan: serde_json::Value,
}

#[tauri::command]
pub fn managed_plans(state: tauri::State<'_, AppState>) -> Vec<SitePlan> {
    state
        .sites
        .list()
        .into_iter()
        .filter(|s| s.ops.level >= 5)
        .filter_map(|s| {
            let p = crate::site_runner::latest_plan(&state.sites.data_dir(&s.id))?;
            Some(SitePlan { site_id: s.id, site_name: s.name, plan: p })
        })
        .collect()
}

#[tauri::command]
pub fn list_digests(state: tauri::State<'_, AppState>) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(managed::digests_dir(&state.data_dir))
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".md").map(str::to_string)).collect())
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

#[tauri::command]
pub fn read_digest(state: tauri::State<'_, AppState>, name: String) -> Result<String, String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("名称不合法".into());
    }
    std::fs::read_to_string(managed::digests_dir(&state.data_dir).join(format!("{name}.md"))).map_err(|_| "日报不存在".to_string())
}

/// 立刻生成一份日报（不影响定时的那一份）。
#[tauri::command]
pub fn digest_now(app: tauri::AppHandle) -> Result<String, String> {
    crate::site_runner::digest(&app)
}
