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

/// 站点的执行档位跟着所在方案的自主程度走；没托管（你自己运营）＝第 1 档：照常同步数据、找机会、体检，不自动改代码。
fn set_level(state: &AppState, site_id: &str, level: u8) {
    let _ = state.sites.update(site_id, crate::now_secs(), |s| {
        s.ops.level = level;
        Ok(())
    });
}

/// 迁移老数据并对齐档位：第 5 档的孤儿站点进「默认方案」；第 1–4 档的站点（只做一次）按档位归进方案；
/// 每个站点的档位都和所在方案的自主程度一致，不在方案里的是第 1 档。
pub fn sync(state: &AppState) -> ManagedConfig {
    let mut cfg = managed::load(&state.data_dir);
    let sites = state.sites.list();
    let five: Vec<String> = sites.iter().filter(|s| s.ops.level >= 5).map(|s| s.id.clone()).collect();
    let mut changed = managed::adopt_orphans(&mut cfg, &five);
    let moved = managed::migrate_levels(&mut cfg, &sites);
    changed |= !moved.is_empty() || cfg.levels_migrated != managed::load(&state.data_dir).levels_migrated;
    if changed {
        let _ = managed::save(&state.data_dir, &cfg);
    }
    for s in &sites {
        let want = cfg.group_of(&s.id).map(|g| managed::level_of(&g.autonomy)).unwrap_or(1);
        if s.ops.level != want {
            set_level(state, &s.id, want);
        }
    }
    for (id, gid) in moved {
        if let Some(g) = cfg.groups.iter().find(|g| g.id == gid) {
            log_site(state, &id, "joined", format!("归入托管方案「{}」（{}）", g.name, managed::autonomy_label(&g.autonomy)));
        }
    }
    cfg
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
    let level = managed::level_of(&g.autonomy);
    for id in &g.sites {
        set_level(&state, id, level);
    }
    for id in g.sites.iter().filter(|id| !before.contains(id)) {
        log_site(&state, id, "joined", format!("加入托管方案「{}」（{}）", g.name, managed::autonomy_label(&g.autonomy)));
    }
    for id in before.iter().filter(|id| !g.sites.contains(id)) {
        if c.group_of(id).is_none() {
            set_level(&state, id, 1);
            log_site(&state, id, "left", format!("移出托管方案「{}」，改为自己运营", g.name));
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
        set_level(&state, sid, 1);
        log_site(&state, sid, "left", format!("托管方案「{}」已删除，改为自己运营", g.name));
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
            set_level(&state, id, 1);
            log_site(&state, id, "left", "退出托管，改为自己运营".into());
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
    /// 站点阶段：new / growing / established；stage_manual 表示运营人员手动指定的。
    pub stage: String,
    pub stage_manual: bool,
    pub name: String,
    pub domain: String,
    pub shipped_week: usize,
    /// 需要你处理的条数：待确认、失败、上线了但待核对（和「需要你」列表同一口径）。
    pub waiting: usize,
    pub failed: usize,
    /// 排着等 AI 处理的条数（托管暂停、额度满了时会一直排着）。
    pub queued: usize,
    pub plan_date: String,
    pub plan_count: usize,
    pub budget: crate::site_runner::Budget,
    pub job: String,
    /// 下一次自动运行的任务（scan / health / plan / report / review / watch）和到点时间；托管停着就是空 / 0。
    pub next_task: String,
    pub next_at: u64,
    /// 这个月（本地时间 1 号起）的 AI 用量和花费、上线了几条
    pub cost: crate::usage::Cost,
    pub shipped_month: usize,
}

#[derive(Serialize)]
pub struct ManagedOverview {
    pub config: ManagedConfig,
    pub sites: Vec<ManagedSite>,
    pub groups: Vec<ManagedGroup>,
    pub shipped_today: usize,
    pub waiting: usize,
    /// 所有托管站点这个月的 AI 用量合计、上线条数
    pub cost_month: crate::usage::Cost,
    pub shipped_month: usize,
}

/// 本地时间这个月 1 号零点
pub fn month_start() -> i64 {
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    today.with_day(1).and_then(|d| d.and_hms_opt(0, 0, 0)).and_then(|d| d.and_local_timezone(chrono::Local).earliest()).map(|d| d.timestamp()).unwrap_or(0)
}

#[tauri::command]
pub fn managed_overview(state: tauri::State<'_, AppState>) -> ManagedOverview {
    let since_week = crate::now_secs().saturating_sub(7 * 86_400);
    let since_day = crate::now_secs().saturating_sub(86_400);
    let cfg = sync(&state);
    let mut shipped_today = 0;
    let mut waiting_total = 0;
    let month = month_start();
    let mut cost_month = crate::usage::Cost::default();
    let mut shipped_month_total = 0;
    let convs = state.convos.list();
    let sites = state
        .sites
        .list()
        .into_iter()
        .filter(|s| cfg.group_of(&s.id).is_some())
        .map(|s| {
            let dir = state.sites.data_dir(&s.id);
            let q = crate::site_ops::load_queue(&dir);
            let shipped_week = q.iter().filter(|i| i.auto && i.deployed_at >= since_week).count();
            shipped_today += q.iter().filter(|i| i.auto && i.deployed_at >= since_day).count();
            let waiting = q.iter().filter(|i| crate::site_ops::needs_you(i)).count();
            waiting_total += waiting;
            let plan = crate::site_runner::latest_plan(&dir);
            let group = cfg.group_of(&s.id);
            let stg = crate::site_stage::stage_of(&s, &dir);
            let running = !cfg.paused && group.is_some_and(|g| g.active_for(&s.id));
            let (next_task, next_at) = if running { crate::site_runner::next_run(&s, &dir, true).unwrap_or_default() } else { Default::default() };
            let conn_id = crate::site_cmds::site_conn_id(&s.id);
            crate::usage::backfill_site(&dir, &convs.iter().filter(|c| c.origin == "ops" && c.conn_id == conn_id).collect::<Vec<_>>());
            let cost = crate::usage::site_cost(&dir, month);
            cost_month.add(&cost);
            let shipped_month = q.iter().filter(|i| i.deployed_at as i64 >= month && matches!(i.status.as_str(), "deployed" | "done")).count();
            shipped_month_total += shipped_month;
            ManagedSite {
                cost,
                shipped_month,
                stage: stg.stage.clone(),
                stage_manual: !stg.manual.is_empty(),
                group_id: group.map(|g| g.id.clone()).unwrap_or_default(),
                active: group.is_some_and(|g| g.active_for(&s.id)),
                failed: q.iter().filter(|i| i.status == "failed").count(),
                queued: q.iter().filter(|i| i.status == "open").count(),
                plan_date: plan.as_ref().and_then(|p| p["date"].as_str().map(str::to_string)).unwrap_or_default(),
                plan_count: plan.as_ref().and_then(|p| p["tasks"].as_array().map(|a| a.len())).unwrap_or(0),
                budget: crate::site_runner::budget(&s, &dir, &state.convos),
                job: state.site_jobs.snapshot(&s.id).filter(|j| j.status == "running").map(|j| j.title).unwrap_or_default(),
                id: s.id,
                name: s.name,
                domain: s.domain,
                shipped_week,
                waiting,
                next_task,
                next_at,
            }
        })
        .collect();
    ManagedOverview { groups: cfg.groups.clone(), config: cfg, sites, shipped_today, waiting: waiting_total, cost_month, shipped_month: shipped_month_total }
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
    pub positioning: crate::site_stage::Positioning,
    pub stage: crate::site_stage::StageInfo,
    /// 和 plan.tasks 一一对应：这条计划在任务队列里走到哪了（空＝没进队列）。
    /// 状态同队列（open / working / proposed / merged / deployed / dismissed / rolled_back / failed），
    /// 上线后有问题要你核对的记成 attention。
    pub task_status: Vec<String>,
    /// 和 plan.tasks 一一对应：队列里那一条的 id（空＝没有）。
    pub task_item: Vec<String>,
}

/// 计划里每一项在队列里的状态：按标题找这份计划之后入队的那一条。
fn plan_progress(plan: &serde_json::Value, dir: &std::path::Path) -> (Vec<String>, Vec<String>) {
    use chrono::TimeZone;
    let since = chrono::NaiveDate::parse_from_str(plan["date"].as_str().unwrap_or(""), "%Y-%m-%d")
        .ok()
        .and_then(|d| chrono::Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest())
        .map(|t| t.timestamp().max(0) as u64)
        .unwrap_or(0);
    let queue = crate::site_ops::load_queue(dir);
    plan["tasks"]
        .as_array()
        .map(|ts| ts.iter().map(|t| {
            let title = t["title"].as_str().unwrap_or("");
            queue
                .iter()
                .filter(|i| i.opp.kind == "plan" && i.opp.title == title && i.created_at >= since)
                .max_by_key(|i| i.created_at)
                .map(|i| (if i.status == "deployed" && !i.attention.is_empty() { "attention".to_string() } else { i.status.clone() }, i.id.clone()))
                .unwrap_or_default()
        }).unzip())
        .unwrap_or_default()
}

#[tauri::command]
pub fn managed_plans(state: tauri::State<'_, AppState>) -> Vec<SitePlan> {
    let cfg = managed::load(&state.data_dir);
    state
        .sites
        .list()
        .into_iter()
        .filter(|s| cfg.group_of(&s.id).is_some())
        .filter_map(|s| {
            let dir = state.sites.data_dir(&s.id);
            let mut p = crate::site_runner::latest_plan(&dir)?;
            // 谁出的计划：新计划里记着；旧计划按它的会话补上
            if p.get("brain").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
                if let Some(c) = p["conv_id"].as_str().and_then(|id| state.convos.get(id)) {
                    p["brain"] = c.brain.into();
                    p["model"] = c.model.into();
                }
            }
            let (task_status, task_item) = plan_progress(&p, &dir);
            Some(SitePlan { positioning: crate::site_stage::load_positioning(&dir), stage: crate::site_stage::stage_of(&s, &dir), site_id: s.id, site_name: s.name, plan: p, task_status, task_item })
        })
        .collect()
}

/// 清除计划的结果：拿掉了几条还没开始的任务，留下了几条（处理中 / 待确认 / 已上线…）。
#[derive(Serialize)]
pub struct PlanCleared {
    pub removed: usize,
    pub kept: usize,
}

fn site_busy(state: &AppState, id: &str) -> bool {
    state.site_jobs.snapshot(id).is_some_and(|j| j.status == "running")
}

/// 清除这个站点最近一次计划：还没开始的任务从队列里拿掉，计划文件挪到 plans/cleared/ 留底；已经在处理或上线的不动。
#[tauri::command]
pub fn plan_clear(state: tauri::State<'_, AppState>, id: String) -> Result<PlanCleared, String> {
    let site = state.sites.get(&id)?;
    if site_busy(&state, &id) {
        return Err("这个站点正在跑任务，等它结束再清除".into());
    }
    let dir = state.sites.data_dir(&id);
    let p = crate::site_runner::latest_plan(&dir).ok_or("这个站点还没有计划")?;
    let (status, items) = plan_progress(&p, &dir);
    let open: Vec<String> = status.iter().zip(&items).filter(|(st, _)| st.as_str() == "open").map(|(_, i)| i.clone()).collect();
    let removed = crate::site_ops::drop_open_items(&dir, &open)?;
    let kept = status.iter().filter(|st| !st.is_empty() && st.as_str() != "open").count();
    let pdir = crate::site_runner::plans_dir(&dir);
    let date = p["date"].as_str().unwrap_or("").to_string();
    let _ = std::fs::create_dir_all(pdir.join("cleared"));
    std::fs::rename(pdir.join(format!("{date}.json")), pdir.join("cleared").join(format!("{date}-{}.json", chrono::Local::now().format("%H%M%S")))).map_err(|e| format!("清除计划失败：{e}"))?;
    managed::log(&state.data_dir, managed::Activity {
        ts: chrono::Local::now().timestamp().max(0) as u64,
        site_id: site.id.clone(),
        site_name: site.name.clone(),
        kind: "plan_cleared".into(),
        title: format!("清除了 {date} 的计划"),
        detail: format!("拿掉 {removed} 条还没开始的任务{}", if kept > 0 { format!("，{kept} 条已经在处理或上线的保留") } else { String::new() }),
        ..Default::default()
    });
    Ok(PlanCleared { removed, kept })
}

/// 把计划里的第 index 项移出计划：从计划里删掉，队列里还没开始的那条一起拿掉（不是驳回，以后还可以再提）。
#[tauri::command]
pub fn plan_remove_task(state: tauri::State<'_, AppState>, id: String, index: usize) -> Result<(), String> {
    state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    let mut p = crate::site_runner::latest_plan(&dir).ok_or("这个站点还没有计划")?;
    let (status, items) = plan_progress(&p, &dir);
    match status.get(index).map(String::as_str) {
        None => return Err("计划里没有这一项（可能刚重新制定过），刷新后再试".into()),
        Some("" | "open" | "dismissed") => {}
        Some(_) => return Err("这一项已经在处理或上线了，不能移出计划".into()),
    }
    if status[index] == "open" {
        crate::site_ops::drop_open_items(&dir, &[items[index].clone()])?;
    }
    if let Some(ts) = p["tasks"].as_array_mut() {
        ts.remove(index);
    }
    let date = p["date"].as_str().unwrap_or("").to_string();
    let raw = serde_json::to_vec_pretty(&p).map_err(|e| e.to_string())?;
    crate::sites::write_atomic(&crate::site_runner::plans_dir(&dir).join(format!("{date}.json")), &raw)
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

/// 日报日历里的一天：AI 做了多少事、出没出问题。
#[derive(Serialize, Default)]
pub struct DayCount {
    pub date: String,
    pub shipped: usize,
    pub planned: usize,
    pub proposed: usize,
    /// 回滚 + 失败 + 上线后检查失败
    pub broken: usize,
    /// 托管站点那天跑了几次运营作业（没产出也算），用来分清「跑了但没事做」和「没跑」
    pub ran: usize,
}

/// 最近 days 天每天的托管动态计数（本地日期），只返回有动静或跑过作业的日子。
#[tauri::command]
pub fn managed_daily(state: tauri::State<'_, AppState>, days: Option<u64>) -> Vec<DayCount> {
    use chrono::TimeZone;
    let since = (chrono::Local::now().timestamp().max(0) as u64).saturating_sub(days.unwrap_or(372).min(800) * 86_400);
    let mut map: std::collections::BTreeMap<String, DayCount> = Default::default();
    for a in managed::activity(&state.data_dir, 20_000).into_iter().filter(|a| a.ts >= since) {
        let Some(t) = chrono::Local.timestamp_opt(a.ts as i64, 0).single() else { continue };
        let date = t.format("%Y-%m-%d").to_string();
        let d = map.entry(date.clone()).or_insert_with(|| DayCount { date, ..Default::default() });
        match a.kind.as_str() {
            "shipped" => d.shipped += 1,
            "planned" => d.planned += 1,
            "proposed" => d.proposed += 1,
            "rolled_back" | "failed" | "watch_failed" => d.broken += 1,
            _ => {}
        }
    }
    let cfg = managed::load(&state.data_dir);
    for site in state.sites.list().into_iter().filter(|s| cfg.group_of(&s.id).is_some()) {
        for r in crate::site_runner::runs_of(&state.sites.data_dir(&site.id)).into_iter().filter(|r| r.started >= since) {
            let Some(t) = chrono::Local.timestamp_opt(r.started as i64, 0).single() else { continue };
            let date = t.format("%Y-%m-%d").to_string();
            map.entry(date.clone()).or_insert_with(|| DayCount { date, ..Default::default() }).ran += 1;
        }
    }
    // 有日报的日子，红点跟日报走：日报里说没有回滚或失败，日历上也不标（旧日报只算了回滚）
    for name in list_digests(state.clone()) {
        if let Some(rep) = load_digest(&state, &name) {
            if let Some(d) = map.get_mut(&name) {
                d.broken = rep.broken;
            } else if rep.broken > 0 {
                map.insert(name.clone(), DayCount { date: name, broken: rep.broken, ..Default::default() });
            }
        }
    }
    map.into_values().filter(|d| d.shipped + d.planned + d.proposed + d.broken + d.ran > 0).collect()
}

#[tauri::command]
pub fn read_digest(state: tauri::State<'_, AppState>, name: String) -> Result<String, String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("名称不合法".into());
    }
    std::fs::read_to_string(managed::digests_dir(&state.data_dir).join(format!("{name}.md"))).map_err(|_| "日报不存在".to_string())
}

/// 日报的结构化内容；改版前只存了 Markdown 的旧日报，读回来转成同样的结构。
#[tauri::command]
pub fn read_digest_data(state: tauri::State<'_, AppState>, name: String) -> Result<Option<crate::site_runner::DigestData>, String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("名称不合法".into());
    }
    Ok(load_digest(&state, &name))
}

/// 一个托管站点最近几天每天的计数（方案卡片上的小柱子用）。
#[derive(Serialize)]
pub struct SiteDays {
    pub site_id: String,
    pub days: Vec<DayCount>,
}

/// 每个托管站点最近 days 天每天的动态计数和作业次数（只返回有数据的日子）。
#[tauri::command]
pub fn managed_site_daily(state: tauri::State<'_, AppState>, days: Option<u64>) -> Vec<SiteDays> {
    use chrono::TimeZone;
    let since = (chrono::Local::now().timestamp().max(0) as u64).saturating_sub(days.unwrap_or(14).min(60) * 86_400);
    let date_of = |ts: u64| chrono::Local.timestamp_opt(ts as i64, 0).single().map(|t| t.format("%Y-%m-%d").to_string());
    let mut per: std::collections::BTreeMap<String, std::collections::BTreeMap<String, DayCount>> = Default::default();
    let mut bump = |site: &str, date: String, f: &dyn Fn(&mut DayCount)| {
        let m = per.entry(site.to_string()).or_default();
        f(m.entry(date.clone()).or_insert_with(|| DayCount { date, ..Default::default() }));
    };
    for a in managed::activity(&state.data_dir, 5_000).into_iter().filter(|a| a.ts >= since && !a.site_id.is_empty()) {
        let Some(date) = date_of(a.ts) else { continue };
        match a.kind.as_str() {
            "shipped" => bump(&a.site_id, date, &|d| d.shipped += 1),
            "planned" => bump(&a.site_id, date, &|d| d.planned += 1),
            "proposed" => bump(&a.site_id, date, &|d| d.proposed += 1),
            "rolled_back" | "failed" | "watch_failed" => bump(&a.site_id, date, &|d| d.broken += 1),
            _ => {}
        }
    }
    let cfg = managed::load(&state.data_dir);
    for site in state.sites.list().into_iter().filter(|s| cfg.group_of(&s.id).is_some()) {
        for r in crate::site_runner::runs_of(&state.sites.data_dir(&site.id)).into_iter().filter(|r| r.started >= since) {
            if let Some(date) = date_of(r.started) {
                bump(&site.id, date, &|d| d.ran += 1);
            }
        }
    }
    per.into_iter().map(|(site_id, m)| SiteDays { site_id, days: m.into_values().collect() }).collect()
}

/// 读一份日报的结构化内容；改版前的日报只有 Markdown，读回来按新样子转，站点 id 按名字补。
fn load_digest(state: &AppState, name: &str) -> Option<crate::site_runner::DigestData> {
    let dir = managed::digests_dir(&state.data_dir);
    if let Some(d) = std::fs::read(dir.join(format!("{name}.json"))).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        return Some(d);
    }
    let mut d = std::fs::read_to_string(dir.join(format!("{name}.md"))).ok().and_then(|md| crate::site_runner::parse_old_digest(&md))?;
    let sites = state.sites.list();
    let id_of = |n: &str| sites.iter().find(|s| s.name == n).map(|s| s.id.clone()).unwrap_or_default();
    for s in &mut d.active { s.site_id = id_of(&s.site_name); }
    for n in &mut d.needs { n.site_id = id_of(&n.site_name); }
    for t in &mut d.traffic { t.site_id = id_of(&t.site_name); }
    Some(d)
}

/// 立刻生成一份日报（不影响定时的那一份）。
#[tauri::command]
pub fn digest_now(app: tauri::AppHandle) -> Result<String, String> {
    crate::site_runner::digest(&app)
}
