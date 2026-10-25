//! 站点「分发」：把站点文章的摘要同步到站外的**渠道**（Blogger 博客，之后是 WordPress.com、Telegram 频道……）。
//!
//! - 每个站一份 `<站点数据>/syndication.json`：
//!   - 渠道（channels）：发到哪儿（类型 + 账号 + 目标，如某个博客）、发布方式、每日上限。一个目标只给一个站用。
//!   - 摘要（summaries）：一篇文章一份，所有博客类渠道共用（AI 写，用户可改）；跳过也记在这里。
//!   - 发布记录（deliveries）：「文章 × 渠道」各一条：已发布 / 草稿 / 失败，带链接。
//! - 摘要由站点运营用的模型写（`oneshot`，不开对话），读的是构建产物里的正文。
//! - 只发摘要不发全文（免得和原文抢排名）；摘要正文里的网址一律去掉，结尾只放这一篇原文的链接——
//!   不链用户的其他站、不带推荐码。
//! - 新渠道类型：在 `KINDS` 里登记，在 `publish_to` 里接上发布。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::AppState;

/// 渠道类型：(类型, 名称, 已支持)
pub const KINDS: &[(&str, &str, bool)] = &[("blogger", "Blogger", true), ("wordpress", "WordPress.com", true), ("telegram", "Telegram 频道", false)];

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Channel {
    pub id: String,
    /// blogger | wordpress | telegram
    pub kind: String,
    /// 账号（「连接」里的那条）
    pub conn_id: String,
    /// 目标：博客 id / 站点 id / 频道
    pub target_id: String,
    #[serde(default)]
    pub target_name: String,
    #[serde(default)]
    pub target_url: String,
    /// review（先给我看，默认）| draft（存成平台草稿）| auto（自动发布，下一期）
    #[serde(default)]
    pub mode: String,
    /// 自动发布时每天最多几篇
    #[serde(default = "default_daily")]
    pub daily_limit: u32,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// 自动处理时也补发加渠道以前的旧文章（按每日上限慢慢发）
    #[serde(default)]
    pub backfill: bool,
    #[serde(default)]
    pub created_at: u64,
}

fn default_daily() -> u32 {
    2
}
fn yes() -> bool {
    true
}

/// 一篇文章的摘要（所有博客类渠道共用）。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Summary {
    pub path: String,
    #[serde(default)]
    pub title: String,
    /// 段落之间空一行
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub labels: Vec<String>,
    /// 这篇不发（所有渠道）
    #[serde(default)]
    pub skipped: bool,
    #[serde(default)]
    pub updated_at: u64,
    /// AI 写这份摘要的时间（每日上限按它数）
    #[serde(default)]
    pub generated_at: u64,
    /// 自动写摘要失败的时间：当天不再自动重试（免得每小时白花一次模型调用）
    #[serde(default)]
    pub gen_failed_at: u64,
}

/// 一篇文章在一个渠道的发布记录。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Delivery {
    pub path: String,
    pub channel_id: String,
    /// published | draft（已存成平台草稿）| failed
    pub status: String,
    #[serde(default)]
    pub post_id: String,
    #[serde(default)]
    pub post_url: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub updated_at: u64,
}

// 第一版的格式（一个站只有一个 Blogger 博客）：读到就迁移成渠道
#[derive(Deserialize, Default)]
struct LegacyConfig {
    #[serde(default)]
    blogger_conn_id: String,
    #[serde(default)]
    blog_id: String,
    #[serde(default)]
    blog_name: String,
    #[serde(default)]
    blog_url: String,
    #[serde(default)]
    mode: String,
}

#[derive(Deserialize)]
struct LegacyPost {
    path: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    post_id: String,
    #[serde(default)]
    post_url: String,
    #[serde(default)]
    error: String,
    #[serde(default)]
    updated_at: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    channels: Vec<Channel>,
    #[serde(default)]
    summaries: Vec<Summary>,
    #[serde(default)]
    deliveries: Vec<Delivery>,
    #[serde(default, skip_serializing)]
    config: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    posts: Option<serde_json::Value>,
}

fn file_path(site_dir: &Path) -> PathBuf {
    site_dir.join("syndication.json")
}

fn load(site_dir: &Path) -> File {
    let mut f: File = std::fs::read(file_path(site_dir)).ok().and_then(|r| serde_json::from_slice(&r).ok()).unwrap_or_default();
    migrate(&mut f);
    f
}

/// 第一版（config + posts）→ 渠道 + 摘要 + 发布记录。
fn migrate(f: &mut File) {
    let legacy: LegacyConfig = f.config.take().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let posts: Vec<LegacyPost> = f.posts.take().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let mut cid = String::new();
    if !legacy.blog_id.is_empty() && f.channels.is_empty() {
        cid = format!("blogger-{}", legacy.blog_id);
        f.channels.push(Channel {
            id: cid.clone(),
            kind: "blogger".into(),
            conn_id: legacy.blogger_conn_id,
            target_id: legacy.blog_id,
            target_name: legacy.blog_name,
            target_url: legacy.blog_url,
            mode: if legacy.mode == "draft" { "draft".into() } else { "review".into() },
            daily_limit: default_daily(),
            enabled: true,
            backfill: false,
            created_at: now(),
        });
    }
    for p in posts {
        if f.summaries.iter().any(|s| s.path == p.path) {
            continue;
        }
        f.summaries.push(Summary { path: p.path.clone(), title: p.title, summary: p.summary, labels: p.labels, skipped: p.status == "skipped", updated_at: p.updated_at, generated_at: p.updated_at, gen_failed_at: 0 });
        if !cid.is_empty() && matches!(p.status.as_str(), "published" | "draft" | "failed") {
            f.deliveries.push(Delivery { path: p.path, channel_id: cid.clone(), status: p.status, post_id: p.post_id, post_url: p.post_url, error: p.error, updated_at: p.updated_at });
        }
    }
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
- 正文里要用引号时用「」，不要用英文双引号；段落之间在 JSON 字符串里写成 \\n\\n；\n\
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

/// 模型常见的 JSON 毛病：字符串里直接换行、正文里的英文双引号没转义。逐字扫一遍修好：
/// 字符串里的换行 / 制表符转义；字符串里的 `"` 后面不是 `, } ] :` 的，当成正文里的引号转义掉。
fn repair_json(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 16);
    let (mut in_str, mut esc) = (false, false);
    for (i, &c) in cs.iter().enumerate() {
        if !in_str {
            if c == '"' {
                in_str = true;
            }
            out.push(c);
            continue;
        }
        if esc {
            esc = false;
            out.push(c);
            continue;
        }
        match c {
            '\\' => {
                esc = true;
                out.push(c);
            }
            '"' => {
                let next = cs[i + 1..].iter().find(|x| !x.is_whitespace());
                if matches!(next, None | Some(',') | Some('}') | Some(']') | Some(':')) {
                    in_str = false;
                    out.push(c);
                } else {
                    out.push_str("\\\"");
                }
            }
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// 模型输出里的那段 JSON：优先 ```json 代码块，否则第一个 `{` 到最后一个 `}`。
fn json_block(raw: &str) -> Option<&str> {
    if let Some(i) = raw.find("```json") {
        let rest = &raw[i + 7..];
        if let Some(j) = rest.find("```") {
            let b = rest[..j].trim();
            if b.starts_with('{') {
                return Some(b);
            }
        }
    }
    let (a, b) = (raw.find('{')?, raw.rfind('}')?);
    (a < b).then(|| &raw[a..=b])
}

/// 模型输出 → (标题, 摘要, 标签)。容忍 ```json 包裹、前后多余的话，以及字符串里的换行和没转义的引号。
pub fn parse_summary(raw: &str) -> Result<(String, String, Vec<String>), String> {
    let block = json_block(raw).ok_or("模型没有按格式返回")?;
    let v: serde_json::Value = serde_json::from_str(block).or_else(|_| serde_json::from_str(&repair_json(block))).map_err(|_| "模型返回的 JSON 看不懂".to_string())?;
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
    pub summary: Option<Summary>,
    pub deliveries: Vec<Delivery>,
}

#[derive(Serialize)]
pub struct KindInfo {
    pub kind: &'static str,
    pub label: &'static str,
    pub available: bool,
}

#[derive(Serialize)]
pub struct SyndicationState {
    pub channels: Vec<Channel>,
    pub articles: Vec<ArticleRow>,
    /// 站点还没有构建产物（读不到文章）
    pub built: bool,
    /// 站点运营用的模型服务（写摘要用；codex / grok 不支持）
    pub brain: String,
    /// 别的站已经用了的目标：`类型:目标 id` → 站点名（一个目标只给一个站）
    pub used_targets: HashMap<String, String>,
    pub kinds: Vec<KindInfo>,
}

fn site_label(s: &crate::sites::Site) -> String {
    if s.domain.is_empty() { s.name.clone() } else { s.domain.clone() }
}

fn used_targets(state: &AppState, except_site: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for s in state.sites.list().into_iter().filter(|s| s.id != except_site) {
        for c in load(&state.sites.data_dir(&s.id)).channels {
            m.insert(format!("{}:{}", c.kind, c.target_id), site_label(&s));
        }
    }
    m
}

#[tauri::command]
pub async fn syndication_state(state: tauri::State<'_, AppState>, site_id: String) -> Result<SyndicationState, String> {
    let site = state.sites.get(&site_id)?;
    let dir = state.sites.data_dir(&site_id);
    let f = load(&dir);
    let out = built_out(&state, &site)?;
    let built = out.join("index.html").is_file();
    let list = if built { tokio::task::spawn_blocking(move || crate::site_make::articles(&out)).await.map_err(|e| e.to_string())? } else { vec![] };
    let articles = list
        .into_iter()
        .map(|a| ArticleRow {
            url: article_url(&site, &a.path),
            summary: f.summaries.iter().find(|s| s.path == a.path).cloned(),
            deliveries: f.deliveries.iter().filter(|d| d.path == a.path).cloned().collect(),
            path: a.path,
            title: a.title,
            date: a.date,
        })
        .collect();
    Ok(SyndicationState {
        channels: f.channels,
        articles,
        built,
        brain: site.ops.brain.clone(),
        used_targets: used_targets(&state, &site_id),
        kinds: KINDS.iter().map(|(kind, label, available)| KindInfo { kind, label, available: *available }).collect(),
    })
}

/// 新建或修改渠道（id 为空＝新建）。同一个目标不能给两个站用，同一个站也不能重复加。
#[tauri::command]
pub fn syndication_save_channel(state: tauri::State<'_, AppState>, site_id: String, channel: Channel) -> Result<Channel, String> {
    state.sites.get(&site_id)?;
    if !KINDS.iter().any(|(k, _, ok)| *k == channel.kind && *ok) {
        return Err("还不支持这种渠道".into());
    }
    if channel.conn_id.is_empty() || channel.target_id.is_empty() {
        return Err("先选账号和发到哪儿".into());
    }
    if let Some(other) = used_targets(&state, &site_id).get(&format!("{}:{}", channel.kind, channel.target_id)) {
        return Err(format!("「{}」已经给 {other} 用了：一个目标只对应一个站，换一个或新建一个", if channel.target_name.is_empty() { &channel.target_id } else { &channel.target_name }));
    }
    let dir = state.sites.data_dir(&site_id);
    let mut f = load(&dir);
    if f.channels.iter().any(|c| c.id != channel.id && c.kind == channel.kind && c.target_id == channel.target_id) {
        return Err("这个站已经加过它了".into());
    }
    let mode = if matches!(channel.mode.as_str(), "review" | "draft" | "auto") { channel.mode.clone() } else { "review".into() };
    let c = Channel { mode, daily_limit: channel.daily_limit.clamp(1, 20), ..channel };
    let out = match f.channels.iter_mut().find(|x| !c.id.is_empty() && x.id == c.id) {
        Some(x) => {
            *x = Channel { created_at: x.created_at, ..c };
            x.clone()
        }
        None => {
            let c = Channel { id: format!("{}-{}", c.kind, uuid::Uuid::new_v4().simple()), created_at: now(), ..c };
            f.channels.push(c.clone());
            c
        }
    };
    save(&dir, &f)?;
    Ok(out)
}

/// 删除渠道（它的发布记录一起删；已经发出去的文章留在平台上）。
#[tauri::command]
pub fn syndication_remove_channel(state: tauri::State<'_, AppState>, site_id: String, channel_id: String) -> Result<(), String> {
    state.sites.get(&site_id)?;
    let dir = state.sites.data_dir(&site_id);
    let mut f = load(&dir);
    f.channels.retain(|c| c.id != channel_id);
    f.deliveries.retain(|d| d.channel_id != channel_id);
    save(&dir, &f)
}

fn upsert_summary(dir: &Path, path: &str, edit: impl FnOnce(&mut Summary)) -> Result<Summary, String> {
    let mut f = load(dir);
    let i = match f.summaries.iter().position(|p| p.path == path) {
        Some(i) => i,
        None => {
            f.summaries.push(Summary { path: path.to_string(), ..Default::default() });
            f.summaries.len() - 1
        }
    };
    edit(&mut f.summaries[i]);
    f.summaries[i].updated_at = now();
    let out = f.summaries[i].clone();
    save(dir, &f)?;
    Ok(out)
}

/// 正在写的摘要（站点 id + 文章路径 → 停止信号）：点「停止」就结束那次模型调用（claude 子进程随之被杀）。
static RUNNING: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<()>>>> = std::sync::LazyLock::new(Default::default);

pub const STOPPED: &str = "已停止";

#[tauri::command]
pub fn syndication_cancel(site_id: String, path: String) {
    if let Some(tx) = RUNNING.lock().ok().and_then(|mut m| m.remove(&format!("{site_id}|{path}"))) {
        let _ = tx.send(());
    }
}

/// 用站点运营的模型给一篇文章写摘要。
#[tauri::command]
pub async fn syndication_generate(state: tauri::State<'_, AppState>, site_id: String, path: String) -> Result<Summary, String> {
    let site = state.sites.get(&site_id)?;
    let out = built_out(&state, &site)?;
    let p2 = path.clone();
    let a = tokio::task::spawn_blocking(move || crate::site_make::articles(&out).into_iter().find(|a| a.path == p2)).await.map_err(|e| e.to_string())?.ok_or("构建产物里没找到这篇文章：先构建一次")?;
    generate_summary(&state, &site, &a).await
}

async fn generate_summary(state: &AppState, site: &crate::sites::Site, a: &crate::site_make::Article) -> Result<Summary, String> {
    let (site_id, path) = (site.id.clone(), a.path.clone());
    let dir = state.sites.data_dir(&site_id);
    let url = article_url(site, &a.path);
    let p = prompt(a, &url);
    let ask = crate::oneshot::Ask { brain: &site.ops.brain, model: &site.ops.model, prompt: &p, max_tokens: 1500, small: false, timeout: Duration::from_secs(180) };
    let key = format!("{site_id}|{path}");
    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Ok(mut m) = RUNNING.lock() {
        m.insert(key.clone(), tx);
    }
    let res = tokio::select! {
        r = crate::oneshot::complete(&state.data_dir, &ask) => r,
        _ = rx => Err(STOPPED.to_string()),
    };
    if let Ok(mut m) = RUNNING.lock() {
        m.remove(&key);
    }
    let raw = res?;
    let (title, summary, labels) = parse_summary(&raw).inspect_err(|_| {
        // 留一份原文排查用（只留最近一次）
        let _ = std::fs::write(dir.join("syndication_last_raw.txt"), &raw);
    })?;
    upsert_summary(&dir, &path, |s| {
        s.title = title;
        s.summary = summary;
        s.labels = labels;
        s.skipped = false;
        s.generated_at = now();
        s.gen_failed_at = 0;
    })
}

/// 改摘要（预览里直接编辑）。
#[tauri::command]
pub fn syndication_edit(state: tauri::State<'_, AppState>, site_id: String, path: String, title: String, summary: String, labels: Vec<String>) -> Result<Summary, String> {
    state.sites.get(&site_id)?;
    let summary = summary.split("\n\n").map(|p| strip_urls(p.trim())).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n");
    if title.trim().is_empty() || summary.is_empty() {
        return Err("标题和摘要都不能空".into());
    }
    upsert_summary(&state.sites.data_dir(&site_id), &path, |s| {
        s.title = title.trim().to_string();
        s.summary = summary;
        s.labels = labels.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).take(5).collect();
    })
}

#[tauri::command]
pub fn syndication_skip(state: tauri::State<'_, AppState>, site_id: String, path: String, skip: bool) -> Result<Summary, String> {
    state.sites.get(&site_id)?;
    upsert_summary(&state.sites.data_dir(&site_id), &path, |s| s.skipped = skip)
}

/// 发到一个渠道。
async fn publish_to(state: &AppState, c: &Channel, s: &Summary, html: &str) -> Result<(String, String, bool), String> {
    let draft = c.mode == "draft";
    match c.kind.as_str() {
        "blogger" => crate::blogger::publish(state, &c.conn_id, &c.target_id, &s.title, html, &s.labels, draft).await.map(|r| (r.id, r.url, draft)),
        "wordpress" => crate::wpcom::publish(state, &c.conn_id, &c.target_id, &s.title, html, &s.labels, draft).await.map(|(id, url)| (id, url, draft)),
        k => Err(format!("还不支持发到 {k}")),
    }
}

/// 把一篇文章发到这些渠道（空＝所有启用、还没发过的）。每个渠道各记一条结果，返回这篇文章的全部发布记录。
#[tauri::command]
pub async fn syndication_publish(state: tauri::State<'_, AppState>, site_id: String, path: String, channel_ids: Vec<String>) -> Result<Vec<Delivery>, String> {
    let site = state.sites.get(&site_id)?;
    let dir = state.sites.data_dir(&site_id);
    let f = load(&dir);
    let s = f.summaries.iter().find(|s| s.path == path).cloned().filter(|s| !s.summary.trim().is_empty()).ok_or("先生成摘要")?;
    let done = |cid: &str| f.deliveries.iter().any(|d| d.path == path && d.channel_id == cid && d.status != "failed");
    let targets: Vec<Channel> = f.channels.iter().filter(|c| c.enabled && (channel_ids.is_empty() || channel_ids.contains(&c.id)) && !done(&c.id)).cloned().collect();
    if targets.is_empty() {
        return Err(if f.channels.is_empty() { "还没有渠道：先在上面「添加渠道」".into() } else { "没有要发的渠道（都发过了或停用了）".into() });
    }
    let out = built_out(&state, &site)?;
    let p2 = path.clone();
    let article_title = tokio::task::spawn_blocking(move || crate::site_make::articles(&out).into_iter().find(|a| a.path == p2).map(|a| a.title)).await.map_err(|e| e.to_string())?.unwrap_or_else(|| s.title.clone());
    let html = post_html(&s.summary, &article_title, &article_url(&site, &path));
    let mut results = Vec::new();
    for c in &targets {
        results.push(deliver(&state, c, &s, &html, &path).await);
    }
    record(&dir, results)?;
    Ok(load(&dir).deliveries.into_iter().filter(|d| d.path == path).collect())
}

async fn deliver(state: &AppState, c: &Channel, s: &Summary, html: &str, path: &str) -> Delivery {
    match publish_to(state, c, s, html).await {
        Ok((id, url, draft)) => Delivery { path: path.to_string(), channel_id: c.id.clone(), status: if draft { "draft".into() } else { "published".into() }, post_id: id, post_url: url, error: String::new(), updated_at: now() },
        Err(e) => Delivery { path: path.to_string(), channel_id: c.id.clone(), status: "failed".into(), error: e.chars().take(200).collect(), updated_at: now(), ..Default::default() },
    }
}

fn record(dir: &Path, results: Vec<Delivery>) -> Result<(), String> {
    let mut f = load(dir);
    for d in results {
        f.deliveries.retain(|x| !(x.path == d.path && x.channel_id == d.channel_id));
        f.deliveries.push(d);
    }
    save(dir, &f)
}

/// 「待处理」里的数：摘要写好了、有「先给我看」的渠道还没发的文章篇数（不扫构建产物，只看记录）。
pub fn pending_review(site_dir: &Path) -> usize {
    let f = load(site_dir);
    let review: Vec<&Channel> = f.channels.iter().filter(|c| c.enabled && (c.mode == "review" || c.mode.is_empty())).collect();
    f.summaries
        .iter()
        .filter(|s| !s.skipped && !s.summary.trim().is_empty())
        .filter(|s| review.iter().any(|c| !f.deliveries.iter().any(|d| d.path == s.path && d.channel_id == c.id && d.status != "failed")))
        .count()
}

#[tauri::command]
pub fn syndication_pending(state: tauri::State<'_, AppState>, site_id: String) -> usize {
    pending_review(&state.sites.data_dir(&site_id))
}

// ───────────────────────── 自动处理（第二期）─────────────────────────
//
// 定时器每 10 分钟看一次（site_runner::start_scheduler）：站点有新的成功上线就马上处理，否则每小时一次。
// - 先给我看：新文章提前把摘要写好，等用户点发布（每天最多写「每日上限」篇）。
// - 存成草稿 / 自动发布：写摘要 → 确认文章网址能打开 → 发到渠道，每个渠道每天最多「每日上限」篇，多的第二天接着发。
// 「新文章」= 发布日期不早于加渠道那天；勾了「补发旧文章」就连以前的也发。失败的不自动重试（点状态标记重试）。

static TICKING: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::LazyLock::new(Default::default);
/// 站点 id → (上次处理时间, 那时最近一次成功上线的时间)
static LAST_TICK: std::sync::LazyLock<std::sync::Mutex<HashMap<String, (u64, u64)>>> = std::sync::LazyLock::new(Default::default);

fn today_start() -> u64 {
    chrono::Local::now().date_naive().and_hms_opt(0, 0, 0).and_then(|d| d.and_local_timezone(chrono::Local).single()).map(|d| d.timestamp().max(0) as u64).unwrap_or(0)
}

fn day_of(ts: u64) -> String {
    chrono::DateTime::from_timestamp(ts as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string()).unwrap_or_default()
}

/// 这个渠道要自动处理的文章（新的在前）：没跳过、还没有发布记录（失败的也不算，等用户手动重试）。
fn eligible<'a>(c: &Channel, f: &File, articles: &'a [crate::site_make::Article]) -> Vec<&'a crate::site_make::Article> {
    let since = day_of(c.created_at);
    articles
        .iter()
        .filter(|a| !f.summaries.iter().any(|s| s.path == a.path && s.skipped))
        .filter(|a| c.backfill || (!a.date.is_empty() && a.date >= since))
        .filter(|a| !f.deliveries.iter().any(|d| d.path == a.path && d.channel_id == c.id))
        .collect()
}

async fn is_live(url: &str) -> bool {
    match reqwest::Client::new().get(url).timeout(Duration::from_secs(20)).send().await {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// 处理一个站点的所有渠道。返回这次做了几件事（写摘要 + 发布）。
pub async fn tick(state: &AppState, site_id: &str) -> Result<usize, String> {
    let site = state.sites.get(site_id)?;
    let dir = state.sites.data_dir(site_id);
    let f = load(&dir);
    let channels: Vec<Channel> = f.channels.iter().filter(|c| c.enabled).cloned().collect();
    if channels.is_empty() || site.domain.trim().is_empty() {
        return Ok(0);
    }
    let out = built_out(state, &site)?;
    let articles = tokio::task::spawn_blocking(move || crate::site_make::articles(&out)).await.map_err(|e| e.to_string())?;
    let t0 = today_start();
    let mut done = 0;
    for c in &channels {
        let f = load(&dir);
        let todo: Vec<crate::site_make::Article> = eligible(c, &f, &articles).into_iter().cloned().collect();
        if c.mode == "review" || c.mode.is_empty() {
            // 先给我看：提前写好摘要，每天最多「每日上限」篇
            let mut budget = (c.daily_limit as usize).saturating_sub(f.summaries.iter().filter(|s| s.generated_at >= t0).count());
            for a in todo.iter().filter(|a| !f.summaries.iter().any(|s| s.path == a.path && (!s.summary.is_empty() || s.gen_failed_at >= t0))) {
                if budget == 0 {
                    break;
                }
                budget -= 1;
                match generate_summary(state, &site, a).await {
                    Ok(_) => done += 1,
                    Err(_) => {
                        let _ = upsert_summary(&dir, &a.path, |s| s.gen_failed_at = now());
                    }
                }
            }
            continue;
        }
        // 存成草稿 / 自动发布
        let sent_today = f.deliveries.iter().filter(|d| d.channel_id == c.id && d.status != "failed" && d.updated_at >= t0).count();
        let mut budget = (c.daily_limit as usize).saturating_sub(sent_today);
        for a in &todo {
            if budget == 0 {
                break;
            }
            let url = article_url(&site, &a.path);
            if !is_live(&url).await {
                continue; // 还没上线（只构建了），下一轮再看
            }
            let cur = load(&dir);
            let s = match cur.summaries.iter().find(|s| s.path == a.path && !s.summary.trim().is_empty()) {
                Some(s) => s.clone(),
                None if cur.summaries.iter().any(|s| s.path == a.path && s.gen_failed_at >= t0) => continue,
                None => match generate_summary(state, &site, a).await {
                    Ok(s) => s,
                    Err(_) => {
                        let _ = upsert_summary(&dir, &a.path, |s| s.gen_failed_at = now());
                        continue;
                    }
                },
            };
            let html = post_html(&s.summary, &a.title, &url);
            record(&dir, vec![deliver(state, c, &s, &html, &a.path).await])?;
            budget -= 1;
            done += 1;
        }
    }
    Ok(done)
}

/// 定时器调：挑出该处理的站点，后台跑 `tick`，跑完发 `site-syndication` 事件让开着的「分发」分页刷新。
pub fn schedule(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let st = app.state::<AppState>();
    for site in st.sites.list() {
        let dir = st.sites.data_dir(&site.id);
        if !load(&dir).channels.iter().any(|c| c.enabled) {
            continue;
        }
        let deployed = crate::site_deploy::load_history(&dir).iter().find(|r| r.ok).map(|r| r.ts).unwrap_or(0);
        let (at, seen) = LAST_TICK.lock().ok().and_then(|m| m.get(&site.id).copied()).unwrap_or((0, 0));
        if now().saturating_sub(at) < 3600 && deployed == seen {
            continue;
        }
        if !TICKING.lock().map(|mut t| t.insert(site.id.clone())).unwrap_or(false) {
            continue;
        }
        if let Ok(mut m) = LAST_TICK.lock() {
            m.insert(site.id.clone(), (now(), deployed));
        }
        let app2 = app.clone();
        let id = site.id.clone();
        tauri::async_runtime::spawn(async move {
            let st = app2.state::<AppState>();
            let n = tick(&st, &id).await.unwrap_or(0);
            if let Ok(mut t) = TICKING.lock() {
                t.remove(&id);
            }
            if n > 0 {
                let _ = app2.emit("site-syndication", serde_json::json!({ "site_id": id }));
            }
        });
    }
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
        // 字符串里直接换行、正文里的英文双引号没转义、前面多一句带 { 的话
        let bad = "先说一句 {备注}\n```json\n{\"title\": \"币安买 USDT 三条路\", \"summary\": \"第一段讲\"快捷区\"和\"自选区\"怎么选，还有很多很多内容要写满六十个字才算够长，所以这里继续写。\n\n第二段讲手续费和溢价怎么算，读完就知道自己多花了多少。\", \"labels\": [\"币安\"]}\n```";
        let (t, s, _) = parse_summary(bad).unwrap();
        assert_eq!(t, "币安买 USDT 三条路");
        assert!(s.contains("\"快捷区\"") && s.contains("\n\n"), "{s}");
    }

    /// 手动跑：PILOT_CHECK_OUT=<构建产物> PILOT_PROMPT_OUT=<输出文件> cargo test --lib dump_prompt -- --ignored
    #[test]
    #[ignore]
    fn dump_prompt() {
        let out = std::path::PathBuf::from(std::env::var("PILOT_CHECK_OUT").unwrap());
        let a = crate::site_make::articles(&out).into_iter().find(|a| a.path.contains(&std::env::var("PILOT_PROMPT_PAGE").unwrap_or("buy-usdt".into()))).unwrap();
        std::fs::write(std::env::var("PILOT_PROMPT_OUT").unwrap(), prompt(&a, &format!("https://example.com{}", a.path))).unwrap();
    }

    #[test]
    fn migrates_the_first_version() {
        let t = std::env::temp_dir().join(format!("syn-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&t).unwrap();
        std::fs::write(file_path(&t), r#"{"config":{"blogger_conn_id":"c1","blog_id":"123","blog_name":"币圈新手笔记","blog_url":"https://x.blogspot.com/","mode":"draft"},"posts":[{"path":"/a/","title":"T","summary":"S","labels":["x"],"status":"published","post_id":"p1","post_url":"https://x.blogspot.com/p1","updated_at":5},{"path":"/b/","title":"","summary":"","status":"skipped"}]}"#).unwrap();
        let f = load(&t);
        assert_eq!(f.channels.len(), 1);
        assert_eq!((f.channels[0].kind.as_str(), f.channels[0].target_id.as_str(), f.channels[0].mode.as_str()), ("blogger", "123", "draft"));
        assert_eq!(f.summaries.len(), 2);
        assert!(f.summaries.iter().any(|s| s.path == "/b/" && s.skipped));
        assert_eq!(f.deliveries.len(), 1);
        assert_eq!((f.deliveries[0].status.as_str(), f.deliveries[0].channel_id.as_str()), ("published", "blogger-123"));
        save(&t, &f).unwrap();
        let raw = std::fs::read_to_string(file_path(&t)).unwrap();
        assert!(!raw.contains("\"config\"") && !raw.contains("\"posts\""), "旧字段不再写回");
        assert_eq!(load(&t).channels.len(), 1, "再读一次不会重复迁移");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn eligible_is_new_articles_unless_backfill() {
        let a = |p: &str, d: &str| crate::site_make::Article { path: p.into(), title: p.into(), description: String::new(), date: d.into(), text: String::new() };
        let arts = vec![a("/new/", "2099-01-02"), a("/old/", "2000-01-01"), a("/nodate/", ""), a("/done/", "2099-01-01"), a("/skip/", "2099-01-01")];
        let mut c = Channel { id: "c".into(), created_at: now(), daily_limit: 2, enabled: true, ..Default::default() };
        let f = File {
            deliveries: vec![Delivery { path: "/done/".into(), channel_id: "c".into(), status: "failed".into(), ..Default::default() }],
            summaries: vec![Summary { path: "/skip/".into(), skipped: true, ..Default::default() }],
            ..Default::default()
        };
        let paths = |v: Vec<&crate::site_make::Article>| v.into_iter().map(|a| a.path.clone()).collect::<Vec<_>>();
        assert_eq!(paths(eligible(&c, &f, &arts)), vec!["/new/".to_string()]);
        c.backfill = true;
        assert_eq!(paths(eligible(&c, &f, &arts)), vec!["/new/".to_string(), "/old/".to_string(), "/nodate/".to_string()]);
    }

    #[test]
    fn post_html_escapes_and_links_only_the_article() {
        let h = post_html("第一段 <b>粗</b>\n\n第二段", "原文 & 标题", "https://a.com/p?x=1&y=2");
        assert_eq!(h.matches("<a ").count(), 1);
        assert!(h.contains("&lt;b&gt;") && h.contains("阅读全文：") && h.contains("x=1&amp;y=2") && h.contains("原文 &amp; 标题"));
        assert!(post_html("English only", "T", "https://a.com/p").contains("Read the full article"));
    }
}
