//! 站点「分发」：把站点新文章的摘要同步到站外平台（第一个是 Blogger）。
//!
//! - 每个站一份 `<站点数据>/syndication.json`：设置（发到哪个博客、发布方式）+ 每篇文章的同步记录。
//! - 摘要由站点运营用的模型写（`oneshot`，不开对话），读的是构建产物里的正文。
//! - 只发摘要不发全文（免得和原文抢排名）；摘要正文里的网址一律去掉，结尾只放这一篇原文的链接——
//!   不链用户的其他站、不带推荐码。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::AppState;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub blogger_conn_id: String,
    #[serde(default)]
    pub blog_id: String,
    #[serde(default)]
    pub blog_name: String,
    #[serde(default)]
    pub blog_url: String,
    /// review（先给我看，默认）| draft（存成 Blogger 草稿）| auto（自动发布；第二期）
    #[serde(default)]
    pub mode: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Post {
    /// 文章的网址路径
    pub path: String,
    /// 摘要的标题 / 正文（段落之间空一行）/ 标签
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub labels: Vec<String>,
    /// ready（摘要写好，等你确认）| published | draft（已存为 Blogger 草稿）| skipped | failed
    pub status: String,
    #[serde(default)]
    pub post_id: String,
    #[serde(default)]
    pub post_url: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub published_at: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    config: Config,
    #[serde(default)]
    posts: Vec<Post>,
}

fn file_path(site_dir: &Path) -> PathBuf {
    site_dir.join("syndication.json")
}

fn load(site_dir: &Path) -> File {
    std::fs::read(file_path(site_dir)).ok().and_then(|r| serde_json::from_slice(&r).ok()).unwrap_or_default()
}

fn save(site_dir: &Path, f: &File) -> Result<(), String> {
    let _ = std::fs::create_dir_all(site_dir);
    let tmp = file_path(site_dir).with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file_path(site_dir)).map_err(|e| e.to_string())
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 站点最近一次构建的产物（GitHub 来源在 `<站点数据>/deploy` 里构建；本地来源在代码目录里）。
fn built_out(state: &AppState, site: &crate::sites::Site) -> Result<PathBuf, String> {
    let deploy = state.sites.data_dir(&site.id).join("deploy");
    let from_deploy = crate::site_make::out_dir(&site.app_dir(&deploy), &site.build.out_dir);
    if from_deploy.join("index.html").is_file() {
        return Ok(from_deploy);
    }
    let gh = state.conns.get(&site.gh_conn_id).ok();
    let code = crate::site_git::code_dir(site, gh.as_ref())?;
    Ok(crate::site_make::out_dir(&site.app_dir(&code), &site.build.out_dir))
}

fn article_url(site: &crate::sites::Site, path: &str) -> String {
    format!("https://{}{}", site.domain.trim().trim_end_matches('/'), path)
}

// ───────────────────────── 摘要 ─────────────────────────

fn prompt(a: &crate::site_make::Article, url: &str) -> String {
    format!(
        "你是这个网站的编辑。下面是网站上的一篇文章，为它写一篇发在 Blogger 博客上的导读，帮读者判断要不要去看原文。\n\
要求：\n\
- 用原文的语言写；\n\
- 300–500 字（英文 150–300 词），分 2–4 段；\n\
- 用自己的话讲清楚：文章解决什么问题、讲了哪些要点、适合谁看。不要逐段照抄原文，不要编造原文没有的信息、数字或承诺；\n\
- 不要写任何链接、网址、推荐码、邀请码，不要写「点击这里」；结尾不用写「阅读全文」，会自动附上原文链接；\n\
- 标题：重新拟一个，不要和原文标题一字不差，不超过 30 个字（英文 70 个字符）；\n\
- 标签：3–5 个，用原文的语言。\n\
只输出 JSON，不要别的内容：{{\"title\": \"…\", \"summary\": \"第一段\\n\\n第二段\", \"labels\": [\"…\"]}}\n\n\
原文标题：{}\n原文网址：{}\n原文描述：{}\n正文：\n{}",
        a.title, url, a.description, a.text
    )
}

/// 去掉网址（模型不听话写了链接也发不出去）。
fn strip_urls(s: &str) -> String {
    s.split_whitespace().filter(|w| !(w.contains("://") || w.starts_with("www."))).collect::<Vec<_>>().join(" ")
}

/// 模型输出 → (标题, 摘要, 标签)。容忍 ```json 包裹和前后多余的话。
pub fn parse_summary(raw: &str) -> Result<(String, String, Vec<String>), String> {
    let (a, b) = (raw.find('{').ok_or("模型没有按格式返回")?, raw.rfind('}').ok_or("模型没有按格式返回")?);
    let v: serde_json::Value = serde_json::from_str(&raw[a..=b]).map_err(|_| "模型返回的 JSON 看不懂".to_string())?;
    let title = v["title"].as_str().unwrap_or("").trim().chars().take(80).collect::<String>();
    let summary = v["summary"]
        .as_str()
        .unwrap_or("")
        .split("\n\n")
        .map(|p| strip_urls(p.trim()))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let labels = v["labels"].as_array().map(|a| a.iter().filter_map(|l| l.as_str().map(|s| s.trim().chars().take(30).collect::<String>())).filter(|s| !s.is_empty()).take(5).collect()).unwrap_or_default();
    if title.is_empty() || summary.chars().count() < 60 {
        return Err("模型写的摘要太短或没有标题，再生成一次".into());
    }
    Ok((title, summary, labels))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// 发到 Blogger 的正文：摘要段落 + 原文链接（只有这一条链接）。
pub fn post_html(summary: &str, article_title: &str, url: &str) -> String {
    let cjk = summary.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let mut html: String = summary.split("\n\n").map(|p| format!("<p>{}</p>\n", esc(p.trim()))).collect();
    html.push_str(&format!("<p>{}<a href=\"{}\">{}</a></p>\n", if cjk { "阅读全文：" } else { "Read the full article: " }, esc(url), esc(article_title)));
    html
}

// ───────────────────────── 命令 ─────────────────────────

#[derive(Serialize)]
pub struct ArticleRow {
    pub path: String,
    pub url: String,
    pub title: String,
    pub date: String,
    pub post: Option<Post>,
}

#[derive(Serialize)]
pub struct SyndicationState {
    pub config: Config,
    pub articles: Vec<ArticleRow>,
    /// 站点还没有构建产物（读不到文章）
    pub built: bool,
    /// 站点运营用的模型服务（写摘要用；codex / grok 不支持）
    pub brain: String,
}

#[tauri::command]
pub async fn syndication_state(state: tauri::State<'_, AppState>, site_id: String) -> Result<SyndicationState, String> {
    let site = state.sites.get(&site_id)?;
    let f = load(&state.sites.data_dir(&site_id));
    let out = built_out(&state, &site)?;
    let built = out.join("index.html").is_file();
    let list = if built { tokio::task::spawn_blocking(move || crate::site_make::articles(&out)).await.map_err(|e| e.to_string())? } else { vec![] };
    let articles = list
        .into_iter()
        .map(|a| ArticleRow { url: article_url(&site, &a.path), post: f.posts.iter().find(|p| p.path == a.path).cloned(), path: a.path, title: a.title, date: a.date })
        .collect();
    Ok(SyndicationState { config: f.config, articles, built, brain: site.ops.brain.clone() })
}

#[tauri::command]
pub fn syndication_save_config(state: tauri::State<'_, AppState>, site_id: String, config: Config) -> Result<Config, String> {
    state.sites.get(&site_id)?;
    let dir = state.sites.data_dir(&site_id);
    let mut f = load(&dir);
    f.config = Config { mode: if matches!(config.mode.as_str(), "review" | "draft" | "auto") { config.mode } else { "review".into() }, ..config };
    save(&dir, &f)?;
    Ok(f.config)
}

fn upsert(dir: &Path, path: &str, edit: impl FnOnce(&mut Post)) -> Result<Post, String> {
    let mut f = load(dir);
    let i = match f.posts.iter().position(|p| p.path == path) {
        Some(i) => i,
        None => {
            f.posts.push(Post { path: path.to_string(), ..Default::default() });
            f.posts.len() - 1
        }
    };
    edit(&mut f.posts[i]);
    f.posts[i].updated_at = now();
    let out = f.posts[i].clone();
    save(dir, &f)?;
    Ok(out)
}

/// 用站点运营的模型给一篇文章写摘要（已发布的不重写）。
#[tauri::command]
pub async fn syndication_generate(state: tauri::State<'_, AppState>, site_id: String, path: String) -> Result<Post, String> {
    let site = state.sites.get(&site_id)?;
    let dir = state.sites.data_dir(&site_id);
    if load(&dir).posts.iter().any(|p| p.path == path && p.status == "published") {
        return Err("这篇已经发布过了".into());
    }
    let out = built_out(&state, &site)?;
    let p2 = path.clone();
    let a = tokio::task::spawn_blocking(move || crate::site_make::articles(&out).into_iter().find(|a| a.path == p2)).await.map_err(|e| e.to_string())?.ok_or("构建产物里没找到这篇文章：先构建一次")?;
    let url = article_url(&site, &a.path);
    let p = prompt(&a, &url);
    let ask = crate::oneshot::Ask { brain: &site.ops.brain, model: &site.ops.model, prompt: &p, max_tokens: 1500, small: false, timeout: Duration::from_secs(180) };
    let raw = crate::oneshot::complete(&state.data_dir, &ask).await?;
    let (title, summary, labels) = parse_summary(&raw)?;
    upsert(&dir, &path, |p| {
        p.title = title;
        p.summary = summary;
        p.labels = labels;
        p.status = "ready".into();
        p.error.clear();
    })
}

/// 改摘要（预览里直接编辑）。
#[tauri::command]
pub fn syndication_edit(state: tauri::State<'_, AppState>, site_id: String, path: String, title: String, summary: String, labels: Vec<String>) -> Result<Post, String> {
    state.sites.get(&site_id)?;
    let summary = summary.split("\n\n").map(|p| strip_urls(p.trim())).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n");
    if title.trim().is_empty() || summary.is_empty() {
        return Err("标题和摘要都不能空".into());
    }
    upsert(&state.sites.data_dir(&site_id), &path, |p| {
        p.title = title.trim().to_string();
        p.summary = summary;
        p.labels = labels.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).take(5).collect();
        if p.status.is_empty() || p.status == "failed" || p.status == "skipped" {
            p.status = "ready".into();
        }
    })
}

/// 发到 Blogger：`draft` 为真时存成草稿。
#[tauri::command]
pub async fn syndication_publish(state: tauri::State<'_, AppState>, site_id: String, path: String, draft: bool) -> Result<Post, String> {
    let site = state.sites.get(&site_id)?;
    let dir = state.sites.data_dir(&site_id);
    let f = load(&dir);
    let cfg = f.config.clone();
    if cfg.blogger_conn_id.is_empty() || cfg.blog_id.is_empty() {
        return Err("还没选发到哪个博客：在上面的设置里选 Blogger 账号和博客".into());
    }
    let post = f.posts.iter().find(|p| p.path == path).cloned().ok_or("先生成摘要")?;
    if post.status == "published" {
        return Err("这篇已经发布过了".into());
    }
    if post.summary.trim().is_empty() {
        return Err("先生成摘要".into());
    }
    let out = built_out(&state, &site)?;
    let p2 = path.clone();
    let article_title = tokio::task::spawn_blocking(move || crate::site_make::articles(&out).into_iter().find(|a| a.path == p2).map(|a| a.title)).await.map_err(|e| e.to_string())?.unwrap_or_else(|| post.title.clone());
    let html = post_html(&post.summary, &article_title, &article_url(&site, &path));
    match crate::blogger::publish(&state, &cfg.blogger_conn_id, &cfg.blog_id, &post.title, &html, &post.labels, draft).await {
        Ok(r) => upsert(&dir, &path, |p| {
            p.status = if draft { "draft".into() } else { "published".into() };
            p.post_id = r.id;
            p.post_url = r.url;
            p.error.clear();
            p.published_at = now();
        }),
        Err(e) => {
            let _ = upsert(&dir, &path, |p| {
                p.status = "failed".into();
                p.error = e.chars().take(200).collect();
            });
            Err(e)
        }
    }
}

#[tauri::command]
pub fn syndication_skip(state: tauri::State<'_, AppState>, site_id: String, path: String, skip: bool) -> Result<Post, String> {
    state.sites.get(&site_id)?;
    upsert(&state.sites.data_dir(&site_id), &path, |p| {
        if p.status != "published" {
            p.status = if skip { "skipped".into() } else if p.summary.is_empty() { String::new() } else { "ready".into() };
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_model_output_and_drops_links() {
        let raw = "好的：\n```json\n{\"title\": \"新手买 USDT 全流程\", \"summary\": \"这篇教程讲清楚海外华人怎么在交易所买到第一笔 USDT，从注册、认证到下单一步步来，看 https://x.com/ref=1 更多。\\n\\n适合第一次接触加密货币的人，读完能避开最常见的几个坑，比如选错网络、地址填错。\", \"labels\": [\"USDT\", \"新手\"]}\n```";
        let (t, s, l) = parse_summary(raw).unwrap();
        assert_eq!(t, "新手买 USDT 全流程");
        assert!(!s.contains("https://") && s.contains("\n\n"), "{s}");
        assert_eq!(l, vec!["USDT".to_string(), "新手".to_string()]);
        assert!(parse_summary("{\"title\":\"x\",\"summary\":\"太短\"}").is_err());
        assert!(parse_summary("no json").is_err());
    }

    /// 手动跑：PILOT_CHECK_OUT=<构建产物> PILOT_PROMPT_OUT=<输出文件> cargo test --lib dump_prompt -- --ignored
    #[test]
    #[ignore]
    fn dump_prompt() {
        let out = std::path::PathBuf::from(std::env::var("PILOT_CHECK_OUT").unwrap());
        let a = crate::site_make::articles(&out).into_iter().find(|a| a.path.contains("buy-usdt")).unwrap();
        std::fs::write(std::env::var("PILOT_PROMPT_OUT").unwrap(), prompt(&a, &format!("https://example.com{}", a.path))).unwrap();
    }

    #[test]
    fn post_html_escapes_and_links_only_the_article() {
        let h = post_html("第一段 <b>粗</b>\n\n第二段", "原文 & 标题", "https://a.com/p?x=1&y=2");
        assert_eq!(h.matches("<a ").count(), 1);
        assert!(h.contains("&lt;b&gt;") && h.contains("阅读全文：") && h.contains("x=1&amp;y=2") && h.contains("原文 &amp; 标题"));
        assert!(post_html("English only", "T", "https://a.com/p").contains("Read the full article"));
    }
}
