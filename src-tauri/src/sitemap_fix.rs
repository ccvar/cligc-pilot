//! sitemap 报错先由程序检查，不急着叫 AI：多数时候站点的 sitemap 本身没问题，是 Search Console 里提交的地址不对
//! （带 www 会跳转、子域名已经不用了、路径改过），或者重新提交过还没被 Google 重新读取。
//! 原地址能用就重新提交；不能用就到 robots.txt 和常见位置找能用的 sitemap 改交，旧地址记下来不再报；
//! 都找不到才交给 AI 改代码（带上检查结果）。

use std::path::Path;
use std::time::Duration;

use crate::conns::ConnStore;
use crate::site_ops::QueueItem;
use crate::sites::Site;

const IGNORE_FILE: &str = "sitemap_ignore.json";

pub enum Outcome {
    /// 程序处理好了（说明）
    Fixed(String),
    /// 查清楚了但要你动手（比如授权没有 Search Console 管理权限）
    Manual(String),
    /// 程序解决不了，交给 AI；附上检查结果
    Ai(String),
}

/// 换了地址、不再提醒的旧 sitemap（Search Console 里还留着报错，删不删由你）
pub fn ignored(site_dir: &Path) -> Vec<String> {
    crate::site_data::read_json(site_dir, IGNORE_FILE)
}

fn ignore(site_dir: &Path, path: &str) {
    let mut v = ignored(site_dir);
    if !v.iter().any(|p| p == path) {
        v.push(path.to_string());
        if let Ok(raw) = serde_json::to_vec(&v) {
            let _ = crate::sites::write_atomic(&site_dir.join(IGNORE_FILE), &raw);
        }
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)")
        .build()
        .unwrap_or_default()
}

/// 打开一个 sitemap 地址：能用就返回最后落到的地址，不能用返回原因
async fn fetch(c: &reqwest::Client, url: &str) -> Result<String, String> {
    let r = c.get(url).send().await.map_err(|e| format!("打不开（{}）", short_err(&e)))?;
    let status = r.status().as_u16();
    let fin = r.url().to_string();
    if !(200..300).contains(&status) {
        return Err(format!("返回 {status}"));
    }
    let body = r.text().await.map_err(|e| format!("读不完（{}）", short_err(&e)))?;
    if !looks_like_sitemap(&body) {
        return Err("返回的不是 sitemap".into());
    }
    Ok(fin)
}

fn short_err(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "超时"
    } else if e.is_connect() {
        "连不上"
    } else if e.is_redirect() {
        "跳转太多次"
    } else {
        "网络出错"
    }
}

fn looks_like_sitemap(body: &str) -> bool {
    let head: String = body.chars().take(4096).collect();
    head.contains("<urlset") || head.contains("<sitemapindex")
}

/// robots.txt 里写的 sitemap
fn robots_sitemaps(robots: &str) -> Vec<String> {
    robots
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("sitemap").then(|| v.trim().to_string())
        })
        .filter(|u| u.starts_with("http"))
        .collect()
}

fn host_of(url: &str) -> String {
    url.split_once("://").map(|(_, r)| r.split(['/', '?', '#']).next().unwrap_or("")).unwrap_or("").to_string()
}

/// 这个 sitemap 在 Search Console 里是不是重新提交过、Google 还没重新读
fn pending(site_dir: &Path, path: &str) -> bool {
    let sm: serde_json::Value = crate::site_data::read_json(&crate::site_data::cache_dir(site_dir), "gsc_sitemaps.json");
    sm.as_array().into_iter().flatten().find(|s| s["path"].as_str() == Some(path)).is_some_and(|s| {
        let sub = s["lastSubmitted"].as_str().unwrap_or("");
        let down = s["lastDownloaded"].as_str().unwrap_or("");
        s["isPending"].as_bool().unwrap_or(false) || (!sub.is_empty() && sub > down)
    })
}

async fn submit(site: &Site, conns: &ConnStore, url: &str) -> Result<(), String> {
    let token = crate::site_data::google_token(conns, site.data.gsc_conn(), &[crate::google::SCOPE_WEBMASTERS])
        .await
        .map_err(|_| "Google 授权没有 Search Console 管理权限，可以在连接里重新授权".to_string())?;
    crate::google::gsc_submit_sitemap(&token, &site.data.gsc_site, url).await
}

pub async fn precheck(site: &Site, site_dir: &Path, conns: &ConnStore, item: &QueueItem) -> Outcome {
    let path = item.opp.page.trim();
    if path.is_empty() || site.data.gsc_site.is_empty() {
        return Outcome::Ai(String::new());
    }
    let c = client();
    let first = fetch(&c, path).await;
    if let Ok(fin) = &first {
        if fin == path {
            if pending(site_dir, path) {
                return Outcome::Fixed("sitemap 能正常打开、格式没问题；已经重新提交过，等 Google 重新读取后报错会消失".into());
            }
            return match submit(site, conns, path).await {
                Ok(()) => Outcome::Fixed("sitemap 能正常打开、格式没问题，已重新提交给 Search Console".into()),
                Err(e) => Outcome::Manual(format!("sitemap 能正常打开、格式没问题，在 Search Console 里重新提交一下就行（{e}）")),
            };
        }
    }
    let why = match &first {
        Ok(fin) => format!("会跳转到 {fin}"),
        Err(e) => e.clone(),
    };
    // 找能用的：跳转后的地址、robots.txt 里写的、常见位置
    let domain = if site.domain.is_empty() { host_of(path) } else { site.domain.clone() };
    let mut cands: Vec<String> = Vec::new();
    if let Ok(fin) = &first {
        cands.push(fin.clone());
    }
    if !domain.is_empty() {
        if let Ok(r) = c.get(format!("https://{domain}/robots.txt")).send().await {
            if r.status().is_success() {
                cands.extend(robots_sitemaps(&r.text().await.unwrap_or_default()));
            }
        }
        for p in ["sitemap-index.xml", "sitemap.xml", "sitemap_index.xml"] {
            cands.push(format!("https://{domain}/{p}"));
        }
    }
    let mut seen = vec![path.to_string()];
    let mut good = None;
    for u in cands {
        if seen.contains(&u) {
            continue;
        }
        seen.push(u.clone());
        if let Ok(fin) = fetch(&c, &u).await {
            good = Some(fin);
            break;
        }
    }
    let Some(good) = good else {
        return Outcome::Ai(format!("程序检查过：Search Console 里提交的 {path} {why}，robots.txt 和常见位置也没找到能用的 sitemap。"));
    };
    match submit(site, conns, &good).await {
        Ok(()) => {
            ignore(site_dir, path);
            Outcome::Fixed(format!("原来提交的 {path} {why}；已改交能用的 {good}。Search Console 里的旧地址可以删掉，这里不会再提醒"))
        }
        Err(e) => Outcome::Manual(format!("原来提交的 {path} {why}；能用的是 {good}，在 Search Console 里提交它就行（{e}）")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sitemaps_from_robots() {
        let r = "User-agent: *\nDisallow: /admin\nSitemap: https://a.com/sitemap-index.xml\nsitemap:https://a.com/news.xml\nSitemap: /rel.xml\n";
        assert_eq!(robots_sitemaps(r), vec!["https://a.com/sitemap-index.xml", "https://a.com/news.xml"]);
    }

    #[test]
    fn recognizes_sitemap_bodies() {
        assert!(looks_like_sitemap("<?xml version=\"1.0\"?><urlset xmlns=\"x\"></urlset>"));
        assert!(looks_like_sitemap("<?xml version=\"1.0\"?>\n<sitemapindex>"));
        assert!(!looks_like_sitemap("<!DOCTYPE html><html><title>404</title>"));
        assert_eq!(host_of("https://tt.cligx.com/sitemap-index.xml"), "tt.cligx.com");
    }
}
