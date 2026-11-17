//! 新页面收录确认：AI 写的新页面上线后，Google 不一定会收。上线 3 天起，每隔 2 天用 Search Console 的
//! URL 检查（URL Inspection API，每个资源每天约 2000 次免费额度）查一次有没有编入索引，查到 21 天为止。
//! 上线满 14 天还没编入，就在这条改动上记一笔「要你核对」（进「需要你 → 上线后待核对」），写明卡在哪一步。
//! 站里查过的新页面一篇都没编入（新站、Google 来得少）：不再一篇篇提醒，只挂一张「全站都没编入」的，之后同站的新页面不再提醒，
//! 等有页面被收录了再按篇提醒。每条只提醒一次（点了「没问题」不会再挂回来）；后来编入了，提醒自动消掉。


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
        let _ = crate::site_ops::update_item(&dir, &i.id, now, |x| {
            x.index_state = state_.clone();
            x.index_note = note.clone();
            x.index_checked = now;
        });
    }
    for c in warnings(&crate::site_ops::load_queue(&dir), now) {
        let _ = crate::site_ops::update_item(&dir, &c.id, now, |x| {
            if let Some(a) = &c.attention {
                x.attention = a.clone();
            }
            if let Some(w) = &c.warned {
                x.index_warned = w.clone();
            }
        });
    }
    true
}

const PAGE_WARN: &str = "新页面上线 ";
const SITE_WARN: &str = "这个站新上线的页面 Google 都还没编入";

#[derive(Debug, PartialEq)]
struct Change {
    id: String,
    /// Some("")＝清掉之前挂的收录提醒
    attention: Option<String>,
    warned: Option<String>,
}

fn ours(attention: &str) -> bool {
    attention.starts_with(PAGE_WARN) || attention.starts_with(SITE_WARN)
}

/// 查完一轮后，收录提醒该怎么挂（只看队列，不联网）
fn warnings(q: &[crate::site_ops::QueueItem], now: u64) -> Vec<Change> {
    let mut out = Vec::new();
    // 编入了：之前挂的提醒消掉
    for i in q.iter().filter(|i| i.index_state == "indexed" && ours(&i.attention)) {
        out.push(Change { id: i.id.clone(), attention: Some(String::new()), warned: None });
    }
    let live = |i: &&crate::site_ops::QueueItem| i.status == "deployed" && !i.new_pages.is_empty();
    let pending: Vec<&crate::site_ops::QueueItem> = q.iter().filter(live).filter(|i| i.index_state == "not_indexed").collect();
    let mut late: Vec<&crate::site_ops::QueueItem> = pending.iter().copied().filter(|i| now.saturating_sub(i.deployed_at) >= WARN_AFTER && i.index_warned.is_empty()).collect();
    if late.is_empty() {
        return out;
    }
    late.sort_by_key(|i| std::cmp::Reverse(i.deployed_at));
    // 查过的新页面一篇都没编入、至少两篇：是站的问题，不是哪一篇的
    let site_wide = pending.len() >= 2 && !q.iter().any(|i| i.index_state == "indexed");
    if !site_wide {
        for i in late {
            let age = now.saturating_sub(i.deployed_at) / DAY;
            let attention = (i.attention.is_empty()).then(|| format!("{PAGE_WARN}{age} 天了，Google 还没编入索引（{}）：可以在 Search Console 里对它「请求编入索引」，或补充内容、从更多页面链过来", i.index_note));
            out.push(Change { id: i.id.clone(), attention, warned: Some("page".into()) });
        }
        return out;
    }
    let told = q.iter().any(|i| i.index_warned == "site" && i.index_state != "indexed");
    if told {
        // 全站那张挂过了：新到期的不再提醒
        for i in late {
            out.push(Change { id: i.id.clone(), attention: None, warned: Some("site".into()) });
        }
        return out;
    }
    let mut paths: Vec<String> = pending.iter().map(|i| path_of(&i.new_pages[0])).collect();
    let more = if paths.len() > 5 { format!(" 等 {} 个", paths.len()) } else { String::new() };
    paths.truncate(5);
    let text = format!(
        "{SITE_WARN}：查过的 {} 个新页面一个都还没编入（{}{more}），Search Console 说「{}」。多半是站点还新、Google 来得少，不是哪一篇写得有问题：可以在 Search Console 里对它们「请求编入索引」，再从已有展现的页面多链过来、争取一些外部链接。之后这个站的新页面不再一篇篇提醒，等有页面被收录了再按篇提醒",
        pending.len(),
        paths.join("、"),
        late[0].index_note.split_once('：').map(|(_, c)| c).unwrap_or("没编入"),
    );
    for (n, i) in late.iter().enumerate() {
        let attention = if n == 0 { Some(text.clone()) } else { None };
        out.push(Change { id: i.id.clone(), attention, warned: Some("site".into()) });
    }
    // 之前按篇挂的、还没点掉的，合进这一张
    for i in pending.iter().filter(|i| i.index_warned == "page" && i.attention.starts_with(PAGE_WARN)) {
        out.push(Change { id: i.id.clone(), attention: Some(String::new()), warned: Some("site".into()) });
    }
    out
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

    fn item(id: &str, age_days: u64, state: &str, now: u64) -> crate::site_ops::QueueItem {
        crate::site_ops::QueueItem {
            id: id.into(),
            status: "deployed".into(),
            new_pages: vec![format!("https://x.com/{id}/")],
            deployed_at: now - age_days * DAY,
            index_state: state.into(),
            index_note: if state == "not_indexed" { format!("/{id}/：已发现 - 尚未编入索引") } else { String::new() },
            ..Default::default()
        }
    }

    #[test]
    fn one_page_not_indexed_is_warned_once() {
        let now = 100 * DAY;
        let mut q = vec![item("a", 15, "not_indexed", now), item("b", 20, "indexed", now)];
        let c = warnings(&q, now);
        assert_eq!(c.len(), 1);
        assert!(c[0].attention.as_deref().unwrap().starts_with("新页面上线 15 天了"));
        // 点了「没问题」：清了提醒、记着提醒过，下一轮不再挂回来
        q[0].index_warned = "page".into();
        assert!(warnings(&q, now + 2 * DAY).is_empty());
        assert!(warnings(&[item("a", 10, "not_indexed", now)], now).is_empty(), "不满 14 天不提醒");
    }

    #[test]
    fn site_with_nothing_indexed_gets_one_site_wide_warning() {
        let now = 100 * DAY;
        // gateraw：三篇新页面都没编入，最早那篇满 15 天
        let mut q = vec![item("rebate", 15, "not_indexed", now), item("sg", 9, "not_indexed", now), item("hk", 8, "not_indexed", now)];
        let c = warnings(&q, now);
        assert_eq!(c.len(), 1);
        let a = c[0].attention.as_deref().unwrap();
        assert!(a.starts_with(SITE_WARN) && a.contains("3 个新页面") && a.contains("/rebate/、/sg/、/hk/") && a.contains("已发现 - 尚未编入索引"), "{a}");
        q[0].index_warned = "site".into();
        q[0].attention.clear();
        // 之后另外两篇到期：不再提醒
        let later = now + 7 * DAY;
        let c = warnings(&q, later);
        assert!(c.iter().all(|x| x.attention.is_none()), "{c:?}");
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn page_warnings_fold_into_the_site_warning_and_clear_when_indexed() {
        let now = 100 * DAY;
        let mut a = item("a", 20, "not_indexed", now);
        a.index_warned = "page".into();
        a.attention = "新页面上线 14 天了，Google 还没编入索引".into();
        let q = vec![a, item("b", 15, "not_indexed", now)];
        let c = warnings(&q, now);
        assert!(c.iter().any(|x| x.id == "b" && x.attention.as_deref().is_some_and(|t| t.starts_with(SITE_WARN))));
        assert!(c.iter().any(|x| x.id == "a" && x.attention.as_deref() == Some("")), "按篇那张合进全站那张");
        // 后来编入了：提醒消掉
        let mut b = item("b", 16, "indexed", now);
        b.attention = format!("{SITE_WARN}：……");
        let c = warnings(&[b], now);
        assert_eq!(c, vec![Change { id: "b".into(), attention: Some(String::new()), warned: None }]);
    }
}
