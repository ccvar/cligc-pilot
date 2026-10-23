//! 多站点：全部站点总览、跨站点待确认、置顶 / 标签、数据源全局管理与按域名批量绑定。
//! 站点数量不设上限：总览只读每站的小文件（summary.json、队列、最近一次上线），不读大缓存。

use serde::Serialize;
use std::collections::HashMap;

use crate::site_ops::QueueItem;
use crate::sites::Site;
use crate::AppState;

#[derive(Serialize)]
pub struct SiteRow {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub source: String,
    pub repo: String,
    pub pinned: bool,
    pub tags: Vec<String>,
    pub level: u8,
    /// 所在托管方案名（没托管为空）与自主程度：suggest / review / auto，方案暂停或站点停止时为 stopped。
    pub plan: String,
    pub autonomy: String,
    pub deploy_mode: String,
    pub has_data: bool,
    pub summary: crate::site_data::Summary,
    /// 待你确认 / 失败 / 未处理的条数。
    pub proposed: usize,
    pub failed: usize,
    pub open: usize,
    pub job: String,
    pub last_deploy_ts: u64,
    pub last_deploy_ok: Option<bool>,
    /// 最近活动：会话、上线、站点设置里最晚的那个。
    pub active_at: u64,
}

#[tauri::command]
pub fn sites_overview(state: tauri::State<'_, AppState>) -> Vec<SiteRow> {
    let mut last_conv: HashMap<String, u64> = HashMap::new();
    for c in state.convos.list() {
        if let Some(id) = c.conn_id.strip_prefix("site:") {
            let e = last_conv.entry(id.to_string()).or_default();
            *e = (*e).max(c.updated_at);
        }
    }
    let cfg = crate::managed::load(&state.data_dir);
    state
        .sites
        .list()
        .into_iter()
        .map(|s| {
            let group = cfg.group_of(&s.id);
            let plan = group.map(|g| g.name.clone()).unwrap_or_default();
            let autonomy = group.map(|g| if g.active_for(&s.id) { g.autonomy.clone() } else { "stopped".to_string() }).unwrap_or_default();
            let dir = state.sites.data_dir(&s.id);
            let q = crate::site_ops::load_queue(&dir);
            let count = |st: &str| q.iter().filter(|i| i.status == st).count();
            let last = crate::site_deploy::load_history(&dir).into_iter().next();
            let job = state.site_jobs.snapshot(&s.id).filter(|j| j.status == "running").map(|j| j.title).unwrap_or_default();
            let last_ts = last.as_ref().map(|d| d.ts).unwrap_or(0);
            SiteRow {
                active_at: s.updated_at.max(last_ts).max(last_conv.get(&s.id).copied().unwrap_or(0)),
                has_data: !s.data.gsc_site.is_empty() || !s.data.ga4_property.is_empty() || !s.data.bing_site.is_empty(),
                summary: crate::site_data::summary(&dir),
                proposed: count("proposed"),
                failed: count("failed"),
                open: count("open"),
                job,
                last_deploy_ts: last_ts,
                last_deploy_ok: last.map(|d| d.ok),
                id: s.id,
                name: s.name,
                domain: s.domain,
                source: s.source,
                repo: s.repo,
                pinned: s.pinned,
                tags: s.tags,
                level: s.ops.level,
                plan,
                autonomy,
                deploy_mode: s.deploy.mode,
            }
        })
        .collect()
}

#[derive(Serialize)]
pub struct PendingItem {
    pub site_id: String,
    pub site_name: String,
    pub item: QueueItem,
}

/// 所有站点里等你处理的条目（待确认、失败、上线后待核对），按时间倒序。
#[tauri::command]
pub fn pending_across_sites(state: tauri::State<'_, AppState>) -> Vec<PendingItem> {
    let mut out: Vec<PendingItem> = Vec::new();
    for s in state.sites.list() {
        for item in crate::site_ops::load_queue(&state.sites.data_dir(&s.id)) {
            if crate::site_ops::needs_you(&item) {
                out.push(PendingItem { site_id: s.id.clone(), site_name: s.name.clone(), item });
            }
        }
    }
    out.sort_by(|a, b| b.item.updated_at.cmp(&a.item.updated_at));
    out
}

#[tauri::command]
pub fn set_site_flags(state: tauri::State<'_, AppState>, id: String, pinned: Option<bool>, tags: Option<Vec<String>>) -> Result<Site, String> {
    let tags = tags.map(|t| crate::sites::normalize_tags(&t)).transpose()?;
    state.sites.update(&id, crate::now_secs(), |s| {
        if let Some(p) = pinned {
            s.pinned = p;
        }
        if let Some(t) = tags {
            s.tags = t;
        }
        Ok(())
    })
}

#[derive(Serialize)]
pub struct DataSourceRow {
    pub conn: crate::conns::Connection,
    /// 用着这个数据源的站点名。
    pub sites: Vec<String>,
}

#[tauri::command]
pub fn data_sources_overview(state: tauri::State<'_, AppState>) -> Vec<DataSourceRow> {
    let sites = state.sites.list();
    state
        .conns
        .list()
        .into_iter()
        .filter(|c| matches!(c.kind.as_str(), "google" | "bing"))
        .map(|c| {
            let used = sites
                .iter()
                .filter(|s| s.data.google_conn_id == c.id || s.data.gsc_conn() == c.id || s.data.bing_conn_id == c.id)
                .map(|s| s.name.clone())
                .collect();
            DataSourceRow { conn: c, sites: used }
        })
        .collect()
}

#[derive(Serialize, Default)]
pub struct BindReport {
    /// （站点名，绑上的内容）
    pub bound: Vec<(String, String)>,
    /// 没找到对应资源的站点名
    pub unmatched: Vec<String>,
    /// 已经绑好、没动的站点数
    pub kept: usize,
    pub errors: Vec<String>,
}

/// 按域名把一个数据源批量绑到所有站点：GA4 按网页数据流的网址匹配，Search Console 优先网域资源，
/// Bing 按站点网址的主机名。已经绑好的不动（`overwrite` 为真时才重绑）；没有域名的站点跳过。
#[tauri::command]
pub async fn auto_bind_sites(state: tauri::State<'_, AppState>, conn_id: String, overwrite: bool) -> Result<BindReport, String> {
    let conn = state.conns.get(&conn_id)?;
    let sites: Vec<Site> = state.sites.list().into_iter().filter(|s| !s.domain.is_empty()).collect();
    let mut report = BindReport::default();
    match conn.kind.as_str() {
        "google" => {
            let raw = crate::keychain::get_key(&conn.id)?;
            let cred = crate::google::parse_credential(&conn.key_kind, &raw)?;
            let token = crate::google::access_token(&conn.id, &cred, &crate::google::read_scopes()).await?;
            let props = crate::google::list_ga4_properties(&token).await.unwrap_or_else(|e| {
                report.errors.push(format!("GA4：{e}"));
                Vec::new()
            });
            let gsc = crate::google::list_gsc_sites(&token).await.unwrap_or_else(|e| {
                report.errors.push(format!("Search Console：{e}"));
                Vec::new()
            });
            // 每个媒体资源的数据流主机名（只在有站点需要 GA4 时才去查）
            let need_ga4 = sites.iter().any(|s| overwrite || s.data.ga4_property.is_empty());
            let mut hosts: Vec<(crate::google::Ga4Property, Vec<String>)> = Vec::new();
            if need_ga4 {
                for p in props {
                    let h = crate::google::ga4_stream_hosts(&token, &p.property).await.unwrap_or_default();
                    hosts.push((p, h));
                }
            }
            for s in &sites {
                let mut what = Vec::new();
                let mut ga4: Option<(String, String)> = None;
                let mut gsc_hit: Option<String> = None;
                // GA4 / Search Console 各自独立：已经绑在别的账号下的那一项不动（除非要求全部重新绑定）
                if overwrite || s.data.ga4_property.is_empty() {
                    if let Some((p, _)) = hosts.iter().find(|(_, hs)| hs.iter().any(|h| crate::google::same_host(h, &s.domain))) {
                        ga4 = Some((p.property.clone(), p.name.clone()));
                        what.push(format!("GA4 {}", p.name));
                    }
                }
                if overwrite || s.data.gsc_site.is_empty() {
                    if let Some(g) = crate::google::match_gsc_site(&gsc, &s.domain) {
                        gsc_hit = Some(g.url.clone());
                        what.push(format!("Search Console {}", g.url));
                    }
                }
                if ga4.is_none() && gsc_hit.is_none() {
                    if !s.data.ga4_property.is_empty() || !s.data.gsc_site.is_empty() {
                        report.kept += 1;
                    } else {
                        report.unmatched.push(s.name.clone());
                    }
                    continue;
                }
                let cid = conn.id.clone();
                state.sites.update(&s.id, crate::now_secs(), |x| {
                    if let Some((p, n)) = ga4 {
                        // 换 GA4 账号时，Search Console 原本跟着旧账号走的，要把旧账号记成它自己的
                        if x.data.gsc_conn_id.is_empty() && !x.data.gsc_site.is_empty() && x.data.google_conn_id != cid {
                            x.data.gsc_conn_id = x.data.google_conn_id.clone();
                        }
                        x.data.google_conn_id = cid.clone();
                        x.data.ga4_property = p;
                        x.data.ga4_property_name = n;
                    }
                    if let Some(g) = gsc_hit {
                        if x.data.google_conn_id.is_empty() { x.data.google_conn_id = cid.clone(); }
                        x.data.gsc_conn_id = if x.data.google_conn_id == cid { String::new() } else { cid.clone() };
                        x.data.gsc_site = g;
                    }
                    Ok(())
                })?;
                report.bound.push((s.name.clone(), what.join("、")));
            }
        }
        "bing" => {
            let key = crate::keychain::get_key(&conn.id)?;
            let list = crate::bing::list_sites(&key).await?;
            for s in &sites {
                if !overwrite && !s.data.bing_site.is_empty() {
                    report.kept += 1;
                    continue;
                }
                let hit = list.iter().find(|b| crate::sites::normalize_domain(&b.url).is_ok_and(|h| crate::google::same_host(&h, &s.domain)));
                let Some(b) = hit else {
                    report.unmatched.push(s.name.clone());
                    continue;
                };
                let (cid, url) = (conn.id.clone(), b.url.clone());
                state.sites.update(&s.id, crate::now_secs(), |x| {
                    x.data.bing_conn_id = cid;
                    x.data.bing_site = url;
                    Ok(())
                })?;
                report.bound.push((s.name.clone(), format!("Bing {}", b.url)));
            }
        }
        _ => return Err("这个连接不是数据源".into()),
    }
    Ok(report)
}
