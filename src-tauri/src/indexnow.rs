//! IndexNow 提交。Bing 偶尔不接受：密钥文件刚上线、还没验证过时常返回 403，过一阵就好。
//! 上线已经成功，这批网址却不能丢（下次上线只算新的变化）：没交上的记下来，过一阵自动补交。
//! 一直交不上才提醒（「要看看」和站点概览），原因写在 Pending.why。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::site_jobs::Job;
use crate::sites::Site;

const DAY: u64 = 86_400;
/// 第 n 次没交上后隔多久再试；试完这几档之后每天一次
const RETRY_AFTER: [u64; 4] = [600, 3_600, 6 * 3_600, DAY];
/// 连着几次没交上算卡住了，要人看（首次 + 10 分钟 + 1 小时 + 6 小时 + 1 天）
const STUCK_AFTER: u32 = 5;
/// 部署记录里的状态：这次的网址还在等补交
const RETRY_PREFIX: &str = "retry: ";

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct Pending {
    pub urls: Vec<String>,
    pub tries: u32,
    pub next_at: u64,
    pub why: String,
}

fn pending_path(site_dir: &Path) -> PathBuf {
    site_dir.join("indexnow_pending.json")
}

pub fn load(site_dir: &Path) -> Pending {
    std::fs::read(pending_path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(site_dir: &Path, p: &Pending) {
    if p.urls.is_empty() {
        let _ = std::fs::remove_file(pending_path(site_dir));
    } else {
        let _ = crate::sites::write_json(&pending_path(site_dir), p, true);
    }
}

/// 一直交不上时的原因（站点总览的提醒）；还在正常重试的不算。
pub fn stuck(site_dir: &Path) -> String {
    let p = load(site_dir);
    if p.tries >= STUCK_AFTER { p.why } else { String::new() }
}

fn merged(old: &[String], new: &[String]) -> Vec<String> {
    let mut out = old.to_vec();
    for u in new {
        if !out.contains(u) {
            out.push(u.clone());
        }
    }
    out
}

fn retry_after(tries: u32) -> u64 {
    RETRY_AFTER[(tries.max(1) as usize - 1).min(RETRY_AFTER.len() - 1)]
}

/// 没交上时说清楚卡在哪。`file_ok`：线上密钥文件对不对，None＝没查成（网络问题）。
fn why_text(err: &str, key_url: &str, file_ok: Option<bool>) -> String {
    match file_ok {
        Some(false) => format!("线上打不开密钥文件 {key_url}：重新上线一次会放回去；如果这个站还有别的部署（比如 GitHub Actions），要把这个文件加进仓库，不然又会被覆盖"),
        Some(true) if err == crate::bing::INDEXNOW_REJECTED => "密钥文件能正常打开，但 Bing 没接受这把密钥。新上线的密钥常这样，过一阵就好；一直这样的话，到 Cloudflare 安全事件里看有没有拦 Bing 的请求，或到 Bing Webmaster 看这个站的状态".into(),
        _ => err.to_string(),
    }
}

async fn why_failed(site: &Site, err: &str) -> String {
    let key = &site.data.indexnow_key;
    let key_url = format!("https://{}/{key}.txt", site.domain);
    let file_ok = match crate::cf::http().get(&key_url).timeout(std::time::Duration::from_secs(15)).send().await {
        Ok(r) => {
            let ok = r.status().is_success();
            Some(ok && r.text().await.is_ok_and(|t| t.trim() == key))
        }
        Err(_) => None,
    };
    why_text(err, &key_url, file_ok)
}

/// 提交这批网址，连同之前没交上的一起。没交上就记下来等补交，返回原因。
pub async fn submit(site: &Site, site_dir: &Path, urls: &[String]) -> Result<usize, String> {
    let all = merged(&load(site_dir).urls, urls);
    match crate::bing::indexnow_submit(&site.domain, &site.data.indexnow_key, &all).await {
        Ok(_) => {
            let mut p = load(site_dir);
            p.urls.retain(|u| !all.contains(u));
            if p.urls.is_empty() {
                p = Pending::default();
            }
            save(site_dir, &p);
            crate::site_deploy::map_indexnow(site_dir, |v| v.starts_with(RETRY_PREFIX).then(|| "ok".to_string()));
            Ok(all.len())
        }
        Err(e) => {
            let why = why_failed(site, &e).await;
            let mut p = load(site_dir);
            p.urls = merged(&p.urls, &all);
            p.tries += 1;
            p.next_at = crate::now_secs() + retry_after(p.tries);
            p.why = why.clone();
            save(site_dir, &p);
            Err(why)
        }
    }
}

/// 上线作业里的「提交 IndexNow」这一步。返回写进部署记录的状态。
pub async fn submit_step(site: &Site, site_dir: &Path, urls: &[String], job: &Job) -> String {
    job.step("indexnow", "running", &format!("{} 个网址", urls.len()));
    match submit(site, site_dir, urls).await {
        Ok(n) => {
            job.line(&format!("IndexNow：已接收 {n} 个网址"));
            job.step("indexnow", "ok", &format!("已提交 {n} 个网址"));
            "ok".into()
        }
        Err(why) => {
            // 上线已经成功，这一步没交上也不算失败：网址记下来了，会自动补交
            job.line(&format!("IndexNow 这次没交上，稍后自动补交：{why}"));
            job.step("indexnow", "retry", &format!("稍后自动补交。{why}"));
            format!("{RETRY_PREFIX}{why}")
        }
    }
}

/// 仓库里已经有 IndexNow 密钥文件、又不是 Pilot 现在用的这把：返回仓库里的。
/// Pilot 自己的密钥文件只在它上传的产物里，这个站的别的部署（GitHub Actions）会把它冲掉。
pub fn key_in_repo(dir: &Path, current: &str) -> Option<String> {
    let has = |d: &Path| !current.is_empty() && d.join(format!("{current}.txt")).exists();
    if has(&dir.join("public")) || has(&dir.join("static")) || has(dir) {
        return None;
    }
    crate::sites::find_indexnow_key(dir, "")
}

/// 这次上线改用仓库里的密钥（站点设置一起改掉）。不用换时返回 None。
pub fn use_repo_key(site: &Site, dir: &Path, job: &Job) -> Option<Site> {
    use tauri::Manager;
    let key = key_in_repo(dir, &site.data.indexnow_key)?;
    job.line(&format!("IndexNow：仓库里已有密钥文件 {key}.txt，改用这把"));
    if let Some(app) = crate::APP.get() {
        let _ = app.state::<crate::AppState>().sites.update(&site.id, crate::now_secs(), |s| {
            s.data.indexnow_key = key.clone();
            Ok(())
        });
    }
    let mut site = site.clone();
    site.data.indexnow_key = key;
    Some(site)
}

/// 旧版本交不上就丢了（只留了一条「失败」记录，不知道是哪些网址）：把全站页面排上补交一次。
fn backfill(site: &Site, site_dir: &Path) {
    const OLD_ERROR: &str = "IndexNow：密钥无效（确认";
    let lost = |v: &str| v.starts_with("failed: ") && v.contains(OLD_ERROR);
    if !crate::site_deploy::load_history(site_dir).iter().any(|r| lost(&r.indexnow)) {
        return;
    }
    let urls = crate::site_deploy::all_page_urls(site, site_dir);
    if urls.is_empty() {
        return;
    }
    let mut p = load(site_dir);
    p.urls = merged(&p.urls, &urls);
    save(site_dir, &p);
    crate::site_deploy::map_indexnow(site_dir, |v| lost(v).then(|| format!("{RETRY_PREFIX}之前没交上，正在补交全站页面")));
}

pub fn schedule(app: &tauri::AppHandle) {
    use tauri::Manager;
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<crate::AppState>();
        for site in st.sites.list() {
            if site.domain.is_empty() || site.data.indexnow_key.is_empty() {
                continue;
            }
            let dir = st.sites.data_dir(&site.id);
            backfill(&site, &dir);
            let p = load(&dir);
            if !p.urls.is_empty() && crate::now_secs() >= p.next_at {
                let _ = submit(&site, &dir, &[]).await;
            }
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pilot-indexnow-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn retries_slow_down_and_then_count_as_stuck() {
        assert_eq!([1, 2, 3, 4, 5, 9].map(retry_after), [600, 3_600, 21_600, DAY, DAY, DAY]);
        let d = temp_dir("stuck");
        let mut p = Pending { urls: vec!["https://x.com/".into()], tries: STUCK_AFTER - 1, next_at: 0, why: "原因".into() };
        save(&d, &p);
        assert_eq!(stuck(&d), "");
        p.tries = STUCK_AFTER;
        save(&d, &p);
        assert_eq!(stuck(&d), "原因");
        save(&d, &Pending::default());
        assert!(!pending_path(&d).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_reason_tells_a_missing_key_file_from_a_rejected_key() {
        let url = "https://x.com/k.txt";
        assert!(why_text(crate::bing::INDEXNOW_REJECTED, url, Some(false)).contains("线上打不开密钥文件 https://x.com/k.txt"));
        assert!(why_text(crate::bing::INDEXNOW_REJECTED, url, Some(true)).contains("密钥文件能正常打开"));
        assert_eq!(why_text(crate::bing::INDEXNOW_REJECTED, url, None), crate::bing::INDEXNOW_REJECTED);
        assert_eq!(why_text("IndexNow 返回 500", url, Some(true)), "IndexNow 返回 500");
    }

    #[test]
    fn the_key_already_in_the_repo_wins() {
        let d = temp_dir("key");
        std::fs::create_dir_all(d.join("public")).unwrap();
        let repo_key = "849a400e0b7786bebd1c5ad4b70eb47d";
        std::fs::write(d.join("public").join(format!("{repo_key}.txt")), repo_key).unwrap();
        // 仓库里只有另一把：换成仓库的（密钥还是空的也一样）
        assert_eq!(key_in_repo(&d, "4f13de6fa85248f48f259f41073afaad").as_deref(), Some(repo_key));
        assert_eq!(key_in_repo(&d, "").as_deref(), Some(repo_key));
        // 现在用的就在仓库里：不换
        assert_eq!(key_in_repo(&d, repo_key), None);
        // 纯 HTML 站：Pilot 的文件写在产物目录也就是源码目录，不换
        std::fs::write(d.join("4f13de6fa85248f48f259f41073afaad.txt"), "4f13de6fa85248f48f259f41073afaad").unwrap();
        assert_eq!(key_in_repo(&d, "4f13de6fa85248f48f259f41073afaad"), None);
        // 仓库里没有：继续用 Pilot 的
        let e = temp_dir("nokey");
        assert_eq!(key_in_repo(&e, "4f13de6fa85248f48f259f41073afaad"), None);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&e);
    }

    #[test]
    fn urls_lost_by_the_old_version_are_queued_once() {
        let d = temp_dir("backfill");
        let site: Site = serde_json::from_value(serde_json::json!({ "id": "s", "name": "x.com", "domain": "x.com", "source": "local", "created_at": 0, "updated_at": 0 })).unwrap();
        let mut m = crate::site_deploy::Manifest::new();
        m.insert("index.html".into(), "a".into());
        m.insert("guide/index.html".into(), "b".into());
        crate::site_deploy::save_manifest(&d, &m);
        let rec = |v: &str| crate::site_deploy::DeployRecord { indexnow: v.into(), ..Default::default() };
        crate::site_deploy::push_history(&d, &rec("ok"));
        crate::site_deploy::push_history(&d, &rec("failed: IndexNow 返回 500"));
        crate::site_deploy::push_history(&d, &rec("failed: IndexNow：密钥无效（确认 https://x.com/k.txt 能访问且内容就是密钥）"));

        backfill(&site, &d);
        let p = load(&d);
        assert_eq!(p.urls, vec!["https://x.com/guide/", "https://x.com/"]);
        assert_eq!((p.tries, p.next_at), (0, 0));
        let h = crate::site_deploy::load_history(&d);
        assert!(h[0].indexnow.starts_with(RETRY_PREFIX));
        assert_eq!(h[1].indexnow, "failed: IndexNow 返回 500");

        // 第二次没东西可补，已经交上的不会被再排一次
        save(&d, &Pending::default());
        backfill(&site, &d);
        assert!(load(&d).urls.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
