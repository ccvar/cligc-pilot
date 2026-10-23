//! 托管运营控制台的前端命令。

use serde::Serialize;

use crate::managed::{self, ManagedConfig, ManagedGroup};
use crate::AppState;

#[tauri::command]
pub fn managed_config(state: tauri::State<'_, AppState>) -> ManagedConfig {
    managed::load(&state.data_dir)
}

/// 只保存全局设置（日报）。托管方案用 save_managed_group 单独保存。
#[tauri::command]
pub fn save_managed_config(state: tauri::State<'_, AppState>, config: ManagedConfig) -> Result<ManagedConfig, String> {
    if config.digest_hour > 23 {
        return Err("日报时间应在 0–23 点之间".into());
    }
    let mut c = managed::load(&state.data_dir);
    c.digest = config.digest;
    c.digest_hour = config.digest_hour;
    managed::save(&state.data_dir, &c)?;
    Ok(c)
}

fn log_site(state: &AppState, site_id: &str, kind: &str, title: String) {
    let name = state.sites.get(site_id).map(|s| s.name).unwrap_or_default();
    managed::log(&state.data_dir, managed::Activity { ts: crate::now_secs(), site_id: site_id.into(), site_name: name, kind: kind.into(), title, ..Default::default() });
}

/// 站点进 / 出全托管时同步它的自动化档位（第 5 档 ⇔ 在某个托管方案里）。
fn set_level(state: &AppState, site_id: &str, managed_on: bool) {
    let _ = state.sites.update(site_id, crate::now_secs(), |s| {
        s.ops.level = if managed_on { 5 } else if s.ops.level >= 5 { 3 } else { s.ops.level };
        Ok(())
    });
}

/// 新建或保存一个托管方案。勾进来的站点会从别的方案里移出；被移出的站点退回第 3 档。
#[tauri::command]
pub fn save_managed_group(state: tauri::State<'_, AppState>, group: ManagedGroup) -> Result<ManagedGroup, String> {
    let mut g = group;
    g.name = g.name.trim().to_string();
    managed::validate_group(&g)?;
    let known: std::collections::HashSet<String> = state.sites.list().into_iter().map(|s| s.id).collect();
    g.sites.retain(|id| known.contains(id));
    g.sites.dedup();
    g.stopped.retain(|id| g.sites.contains(id));
    let mut c = managed::load(&state.data_dir);
    if g.id.is_empty() {
        g.id = uuid::Uuid::new_v4().to_string();
        g.created_at = crate::now_secs();
    }
    let before: Vec<String> = c.groups.iter().find(|x| x.id == g.id).map(|x| x.sites.clone()).unwrap_or_default();
    // 一个站点只属于一个方案
    for other in c.groups.iter_mut().filter(|x| x.id != g.id) {
        let moved: Vec<String> = other.sites.iter().filter(|id| g.sites.contains(id)).cloned().collect();
        if !moved.is_empty() {
            other.sites.retain(|id| !moved.contains(id));
            other.stopped.retain(|id| !moved.contains(id));
        }
    }
    match c.groups.iter_mut().find(|x| x.id == g.id) {
        Some(x) => *x = g.clone(),
        None => c.groups.push(g.clone()),
    }
    managed::save(&state.data_dir, &c)?;
    for id in g.sites.iter().filter(|id| !before.contains(id)) {
        set_level(&state, id, true);
        log_site(&state, id, "joined", format!("加入托管方案「{}」", g.name));
    }
    for id in before.iter().filter(|id| !g.sites.contains(id)) {
        if c.group_of(id).is_none() {
            set_level(&state, id, false);
            log_site(&state, id, "left", format!("移出托管方案「{}」（改为第 3 档）", g.name));
        }
    }
    Ok(g)
}

/// 删除托管方案：里面的站点退出托管（改为第 3 档）。
#[tauri::command]
pub fn delete_managed_group(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let mut c = managed::load(&state.data_dir);
    let Some(i) = c.groups.iter().position(|g| g.id == id) else { return Ok(()) };
    let g = c.groups.remove(i);
    managed::save(&state.data_dir, &c)?;
    for sid in &g.sites {
        set_level(&state, sid, false);
        log_site(&state, sid, "left", format!("托管方案「{}」已删除，退出托管（改为第 3 档）", g.name));
    }
    Ok(())
}

/// 暂停 / 恢复一个托管方案。
#[tauri::command]
pub fn set_group_paused(state: tauri::State<'_, AppState>, id: String, paused: bool) -> Result<(), String> {
    let mut c = managed::load(&state.data_dir);
    let g = c.groups.iter_mut().find(|g| g.id == id).ok_or("托管方案不存在")?;
    g.paused = paused;
    let name = g.name.clone();
    managed::save(&state.data_dir, &c)?;
    managed::log(&state.data_dir, managed::Activity { ts: crate::now_secs(), kind: if paused { "paused" } else { "resumed" }.into(), title: format!("{}托管方案「{name}」", if paused { "暂停" } else { "恢复" }), ..Default::default() });
    Ok(())
}

/// 单独启用 / 停止方案里的某个站点（规则保留，停止期间不自动运行）。
#[tauri::command]
pub fn set_site_managed_active(state: tauri::State<'_, AppState>, site_id: String, active: bool) -> Result<(), String> {
    let mut c = managed::load(&state.data_dir);
    let g = c.groups.iter_mut().find(|g| g.sites.contains(&site_id)).ok_or("这个站点不在任何托管方案里")?;
    g.stopped.retain(|x| x != &site_id);
    if !active {
        g.stopped.push(site_id.clone());
    }
    managed::save(&state.data_dir, &c)?;
    log_site(&state, &site_id, if active { "resumed" } else { "paused" }, if active { "恢复托管".into() } else { "停止托管（保留规则，不再自动运行）".into() });
    Ok(())
}

#[tauri::command]
pub fn set_managed_paused(state: tauri::State<'_, AppState>, paused: bool) -> Result<ManagedConfig, String> {
    let mut c = managed::load(&state.data_dir);
    c.paused = paused;
    managed::save(&state.data_dir, &c)?;
    managed::log(&state.data_dir, managed::Activity { ts: crate::now_secs(), kind: if paused { "paused" } else { "resumed" }.into(), title: if paused { "暂停托管" } else { "恢复托管" }.into(), ..Default::default() });
    Ok(c)
}

/// 把站点设为全托管或退出托管。加入时放进 `group_id` 指定的方案（不给就放进第一个方案，一个都没有就新建「默认方案」）。
#[tauri::command]
pub fn set_sites_managed(state: tauri::State<'_, AppState>, ids: Vec<String>, managed: bool, group_id: Option<String>) -> Result<usize, String> {
    let mut c = managed::load(&state.data_dir);
    if managed {
        let gid = match group_id.filter(|g| c.groups.iter().any(|x| &x.id == g)).or_else(|| c.groups.first().map(|g| g.id.clone())) {
            Some(g) => g,
            None => {
                let mut g = ManagedGroup::from_legacy(&c, "默认方案");
                g.id = uuid::Uuid::new_v4().to_string();
                g.created_at = crate::now_secs();
                c.groups.push(g.clone());
                g.id
            }
        };
        let mut g = c.groups.iter().find(|x| x.id == gid).cloned().ok_or("托管方案不存在")?;
        for id in &ids {
            if !g.sites.contains(id) {
                g.sites.push(id.clone());
            }
        }
        managed::save(&state.data_dir, &c)?;
        save_managed_group(state, g)?;
    } else {
        for g in c.groups.iter_mut() {
            g.sites.retain(|id| !ids.contains(id));
            g.stopped.retain(|id| !ids.contains(id));
        }
        managed::save(&state.data_dir, &c)?;
        for id in &ids {
            set_level(&state, id, false);
            log_site(&state, id, "left", "退出托管（改为第 3 档）".into());
        }
    }
    Ok(ids.len())
}

#[derive(Serialize)]
pub struct ManagedSite {
    pub id: String,
    pub group_id: String,
    /// 方案没暂停、站点也没单独停止。
    pub active: bool,
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
    pub groups: Vec<ManagedGroup>,
    pub shipped_today: usize,
    pub waiting: usize,
}

#[tauri::command]
pub fn managed_overview(state: tauri::State<'_, AppState>) -> ManagedOverview {
    let since_week = crate::now_secs().saturating_sub(7 * 86_400);
    let since_day = crate::now_secs().saturating_sub(86_400);
    let mut cfg = managed::load(&state.data_dir);
    let managed_ids: Vec<String> = state.sites.list().into_iter().filter(|s| s.ops.level >= 5).map(|s| s.id).collect();
    if managed::adopt_orphans(&mut cfg, &managed_ids) {
        let _ = managed::save(&state.data_dir, &cfg);
    }
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
            let group = cfg.group_of(&s.id);
            ManagedSite {
                group_id: group.map(|g| g.id.clone()).unwrap_or_default(),
                active: group.is_some_and(|g| g.active_for(&s.id)),
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
    ManagedOverview { groups: cfg.groups.clone(), config: cfg, sites, shipped_today, waiting: waiting_total }
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
