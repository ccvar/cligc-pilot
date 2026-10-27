//! 新页面收录确认：AI 写的新页面上线后，Google 不一定会收。上线 3 天起，每隔 2 天用 Search Console 的
//! URL 检查（URL Inspection API，每个资源每天约 2000 次免费额度）查一次有没有编入索引，查到 21 天为止。
//! 上线满 14 天还没编入，就在这条改动上记一笔「要你核对」（进「需要你 → 上线后待核对」），写明卡在哪一步。


const DAY: u64 = 86_400;
/// 上线几天后开始查、隔几天查一次、查到第几天、满几天还没编入就提醒
const START: u64 = 3 * DAY;
const EVERY: u64 = 2 * DAY;
const UNTIL: u64 = 21 * DAY;
const WARN_AFTER: u64 = 14 * DAY;

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 这一条现在该不该查
fn due(i: &crate::site_ops::QueueItem, now: u64) -> bool {
    let age = now.saturating_sub(i.deployed_at);
    i.status == "deployed" && !i.new_pages.is_empty() && i.index_state != "indexed" && (START..=UNTIL).contains(&age) && now.saturating_sub(i.index_checked) >= EVERY
}

/// 查一个站点到期的新页面。没接 Search Console 的站点跳过。
pub async fn check_site(state: &crate::AppState, site: &crate::sites::Site) -> bool {
    let d = &site.data;
    if d.gsc_site.is_empty() || d.gsc_conn().is_empty() {
        return false;
    }
    let dir = state.sites.data_dir(&site.id);
    let now = crate::now_secs();
    let items: Vec<crate::site_ops::QueueItem> = crate::site_ops::load_queue(&dir).into_iter().filter(|i| due(i, now)).collect();
    if items.is_empty() {
        return false;
    }
    let token = match crate::site_data::google_token(&state.conns, d.gsc_conn(), &crate::google::read_scopes()).await {
        Ok(t) => t,
        Err(e) => {
            for i in &items {
                let _ = crate::site_ops::update_item(&dir, &i.id, now, |x| { x.index_state = "error".into(); x.index_note = e.clone(); x.index_checked = now; });
            }
            return true;
        }
    };
    for i in items {
        let (state_, note) = inspect_all(&token, &d.gsc_site, &i.new_pages).await;
        let age = now.saturating_sub(i.deployed_at);
        let _ = crate::site_ops::update_item(&dir, &i.id, now, |x| {
            x.index_state = state_.clone();
            x.index_note = note.clone();
            x.index_checked = now;
            if state_ == "not_indexed" && age >= WARN_AFTER && x.attention.is_empty() {
                x.attention = format!("新页面上线 {} 天了，Google 还没编入索引（{note}）：可以在 Search Console 里对它「请求编入索引」，或补充内容、从更多页面链过来", age / DAY);
            }
        });
    }
    true
}

/// 一条改动的几个新页面：都编入了才算 indexed；有没编入的，给出第一个没编入页面的覆盖状态
async fn inspect_all(token: &str, gsc_site: &str, pages: &[String]) -> (String, String) {
    for p in pages.iter().take(5) {
        match crate::google::gsc_inspect(token, gsc_site, p).await {
            Ok(r) if r.verdict == "PASS" => continue,
            Ok(r) => return ("not_indexed".into(), format!("{}{}", path_of(p), if r.coverage.is_empty() { String::new() } else { format!("：{}", r.coverage) })),
            Err(e) => return ("error".into(), e),
        }
    }
    ("indexed".into(), String::new())
}

fn path_of(url: &str) -> String {
    url.split_once("://").and_then(|(_, r)| r.find('/').map(|i| r[i..].to_string())).unwrap_or_else(|| url.to_string())
}

/// 调度循环每轮调一次：后台挨个站点查（同一时间只跑一轮）。没到期的条目很快就跳过，不会每轮都请求 Google。
pub fn schedule(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<crate::AppState>();
        for site in st.sites.list() {
            if check_site(&st, &site).await {
                let q = crate::site_ops::load_queue(&st.sites.data_dir(&site.id));
                let _ = app.emit("site-queue", serde_json::json!({ "site_id": site.id, "queue": q }));
            }
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_new_pages_from_day_3_every_2_days_until_day_21() {
        let now = 100 * DAY;
        let mk = |age: u64, checked_ago: u64, state: &str| crate::site_ops::QueueItem {
            status: "deployed".into(),
            new_pages: vec!["https://x.com/a/".into()],
            deployed_at: now - age,
            index_checked: if checked_ago == 0 { 0 } else { now - checked_ago },
            index_state: state.into(),
            ..Default::default()
        };
        assert!(!due(&mk(2 * DAY, 0, ""), now), "上线不满 3 天不查");
        assert!(due(&mk(3 * DAY, 0, ""), now));
        assert!(!due(&mk(5 * DAY, DAY, "not_indexed"), now), "1 天前刚查过");
        assert!(due(&mk(5 * DAY, 2 * DAY, "not_indexed"), now));
        assert!(!due(&mk(5 * DAY, 0, "indexed"), now), "已经编入了");
        assert!(!due(&mk(22 * DAY, 0, ""), now), "过了 21 天不再查");
    }
}
