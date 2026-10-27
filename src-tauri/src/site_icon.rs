//! 站点图标（favicon），给侧栏、总览、切换器这些地方一眼认出是哪个站。
//!
//! 先在站点的代码目录里找（常见的 public/、static/ 等位置，再浅层扫一遍 `favicon.*`），读本地文件快、离线也行；
//! 找不到再到线上取一次 `/favicon.ico` 之类，结果（包括「没有」）缓存在站点数据目录 7 天。
//! 回给前端的是 data URL，图片不会再去网络上请求。

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::sites::Site;

const NAMES: &[&str] = &[
    "favicon.svg", "favicon.ico", "favicon.png", "favicon-48.png", "favicon-48x48.png", "favicon-32x32.png", "favicon-32.png",
    "icon.svg", "icon.png", "apple-touch-icon.png", "icon-192.png",
];
const DIRS: &[&str] = &["public", "static", "web/static", "site/public", "app", "src/app", "assets", "src/assets", ""];
const SKIP: &[&str] = &["node_modules", "dist", "build", "out", ".git", ".astro", ".next", ".wrangler", ".svelte-kit", "target", "vendor", "screenshots"];
const MAX_BYTES: u64 = 256 * 1024;
const REMOTE_TTL: u64 = 7 * 86_400;

fn mime_of(p: &Path) -> Option<&'static str> {
    match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "svg" => Some("image/svg+xml"),
        "ico" => Some("image/x-icon"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn small_file(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0 && m.len() <= MAX_BYTES)
}

/// 代码目录里的图标文件：先查常见位置，再浅层（4 层内、跳过依赖和构建产物）找 `favicon.*`。
pub fn find_local(root: &Path) -> Option<PathBuf> {
    for d in DIRS {
        for n in NAMES {
            let p = root.join(d).join(n);
            if small_file(&p) {
                return Some(p);
            }
        }
    }
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if depth < 4 && !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
                    walk(&p, depth + 1, out);
                }
            } else if name.to_ascii_lowercase().starts_with("favicon.") && mime_of(&p).is_some() && small_file(&p) {
                out.push(p);
            }
        }
    }
    let mut found = Vec::new();
    walk(root, 0, &mut found);
    // svg 最清晰，其次 png，最后 ico；同类取路径最短的
    found.sort_by_key(|p| {
        let rank = match mime_of(p) { Some("image/svg+xml") => 0, Some("image/png") => 1, _ => 2 };
        (rank, p.components().count())
    });
    found.into_iter().next()
}

#[derive(Serialize, Deserialize, Default)]
struct RemoteCache {
    checked: u64,
    url: String,
}

fn cache_file(site_dir: &Path) -> PathBuf {
    site_dir.join("icon.json")
}

/// 首页 <head> 里声明的图标地址（`<link rel="icon" href="…">`，其次 apple-touch-icon），按页面最终网址补成完整网址。
/// 很多站点的图标不叫 favicon、也不放在根目录（比如 /static/brand/xxx-mark.png），只能从这里找到。
fn declared_icons(html: &str, base: &reqwest::Url) -> Vec<String> {
    let low = html.to_ascii_lowercase();
    let mut found: Vec<(u8, String)> = Vec::new();
    let mut from = 0;
    while let Some(i) = low[from..].find("<link") {
        let a = from + i;
        let Some(j) = low[a..].find('>') else { break };
        let tag = &html[a..a + j];
        let ltag = &low[a..a + j];
        from = a + j;
        let attr = |name: &str| -> Option<String> {
            let k = ltag.find(&format!("{name}="))? + name.len() + 1;
            let q = ltag[k..].chars().next()?;
            if q == '"' || q == '\'' {
                let rest = &tag[k + 1..];
                rest.find(q).map(|e| rest[..e].to_string())
            } else {
                Some(tag[k..].split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string())
            }
        };
        let Some(rel) = attr("rel").map(|r| r.to_ascii_lowercase()) else { continue };
        let rank = if rel.split_whitespace().any(|w| w == "icon") { 0 } else if rel.contains("apple-touch-icon") { 1 } else { continue };
        if let Some(u) = attr("href").and_then(|h| base.join(&h.replace("&amp;", "&")).ok()) {
            found.push((rank, u.to_string()));
        }
    }
    found.sort_by_key(|f| f.0);
    found.into_iter().map(|f| f.1).collect()
}

async fn fetch_remote(domain: &str) -> String {
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(6)).build() {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    // 先看首页声明了哪个图标，再试常见的固定路径
    let mut urls: Vec<String> = Vec::new();
    if let Ok(r) = client.get(format!("https://{domain}/")).send().await {
        let base = r.url().clone();
        if let Ok(html) = r.text().await {
            urls.extend(declared_icons(&html, &base));
        }
    }
    urls.extend(["/favicon.ico", "/favicon.svg", "/favicon.png", "/apple-touch-icon.png"].iter().map(|p| format!("https://{domain}{p}")));
    for url in urls {
        let path = url.split(['?', '#']).next().unwrap_or("").to_string();
        let Ok(r) = client.get(&url).send().await else { continue };
        if !r.status().is_success() {
            continue;
        }
        let ct = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default().split(';').next().unwrap_or_default().trim().to_string();
        // 不少站点对不存在的路径也回 200 + HTML：只认图片
        if ct.starts_with("text/") {
            continue;
        }
        let mime = if ct.starts_with("image/") { ct } else { mime_of(Path::new(&path)).unwrap_or_default().to_string() };
        if !mime.starts_with("image/") {
            continue;
        }
        let Ok(bytes) = r.bytes().await else { continue };
        if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
            continue;
        }
        return data_url(&mime, &bytes);
    }
    String::new()
}

/// 某个站点的图标（data URL），没有就是空串。
pub async fn icon_for(site: &Site, site_dir: &Path, code_dir: Option<PathBuf>) -> String {
    if let Some(p) = code_dir.as_deref().and_then(find_local) {
        if let (Ok(bytes), Some(mime)) = (std::fs::read(&p), mime_of(&p)) {
            return data_url(mime, &bytes);
        }
    }
    if site.domain.is_empty() {
        return String::new();
    }
    let file = cache_file(site_dir);
    if let Some(c) = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<RemoteCache>(&b).ok()) {
        // 找到了的缓存 7 天；没找到的 6 小时后再试（站点可能刚加上图标，或者上次只是网络不好）
        let ttl = if c.url.is_empty() { 6 * 3600 } else { REMOTE_TTL };
        if crate::now_secs().saturating_sub(c.checked) < ttl {
            return c.url;
        }
    }
    let url = fetch_remote(&site.domain).await;
    let _ = crate::sites::write_json(&file, &RemoteCache { checked: crate::now_secs(), url: url.clone() }, false);
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_public_favicon_and_skips_dependency_folders() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir_all(d.join("node_modules/pkg")).unwrap();
        std::fs::write(d.join("node_modules/pkg/favicon.svg"), "<svg/>").unwrap();
        assert!(find_local(d).is_none());
        std::fs::create_dir_all(d.join("web/templates/img")).unwrap();
        std::fs::write(d.join("web/templates/img/favicon.png"), [1u8, 2, 3]).unwrap();
        assert_eq!(find_local(d).unwrap(), d.join("web/templates/img/favicon.png"));
        std::fs::create_dir_all(d.join("public")).unwrap();
        std::fs::write(d.join("public/favicon.svg"), "<svg/>").unwrap();
        assert_eq!(find_local(d).unwrap(), d.join("public/favicon.svg"));
    }

    #[test]
    fn reads_icons_declared_in_the_page_head() {
        let base = reqwest::Url::parse("https://dexgc.com/zh/").unwrap();
        let html = r#"<head><link rel="stylesheet" href="/a.css"><link rel="apple-touch-icon" href="/t.png"><link rel="icon" href="/static/brand/dexgc-mark.png?v=5dc8" type="image/png"></head>"#;
        assert_eq!(declared_icons(html, &base), vec!["https://dexgc.com/static/brand/dexgc-mark.png?v=5dc8", "https://dexgc.com/t.png"]);
        assert_eq!(declared_icons(r#"<link rel='shortcut icon' href=img/f.ico>"#, &base), vec!["https://dexgc.com/zh/img/f.ico"]);
    }

    #[test]
    fn data_urls_carry_the_mime_type() {
        assert_eq!(data_url("image/svg+xml", b"<svg/>"), "data:image/svg+xml;base64,PHN2Zy8+");
    }
}
