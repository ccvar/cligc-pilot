//! 站点动态功能：表单、评论、R2 大文件，跑在 Cloudflare Pages Functions 上（同一个 Pages 项目，部署方式不变）。
//!
//! - 代码：Pilot 随附的 Functions 模板（builtin/functions），写进站点仓库的 `functions/`、`db/schema.sql`、`public/_routes.json`；
//!   只有 `/api/*` 和 R2 里的大文件路径会进 Functions，其它请求照旧走静态文件（不占 Functions 额度）。
//! - 资源：D1 数据库（DB）、R2 存储桶（FILES）、Turnstile 人机验证，由 Pilot 用站点的 Cloudflare 连接建好、绑到 Pages 项目；
//!   Turnstile 的两个值存成配置变量（PUBLIC_TURNSTILE_SITE_KEY 公开、TURNSTILE_SECRET_KEY 密钥），上线时同步。
//! - 收件箱：表单提交和评论都在 D1 里，Pilot 直接读（站点页「表单和评论」），评论在这里审核。
//!
//! 配置存 `<站点数据>/dynamic.json`。迁移站点在「抓取」时按方案写好 features，在「核对」配预览时建资源。

use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const LIB_JS: &str = include_str!("builtin/functions/_lib.js");
pub const FORM_JS: &str = include_str!("builtin/functions/api_form.js");
pub const COMMENTS_JS: &str = include_str!("builtin/functions/api_comments.js");
pub const HEALTH_JS: &str = include_str!("builtin/functions/api_health.js");
pub const FILES_JS: &str = include_str!("builtin/functions/files.js");
pub const SCHEMA_SQL: &str = include_str!("builtin/functions/schema.sql");

/// `_routes.json` 最多 100 条规则。
const MAX_ROUTES: usize = 100;

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Dynamic {
    /// form | comments | files
    #[serde(default)]
    pub features: Vec<String>,
    /// 放在 R2 里的大文件（站点路径，如 `/video/a.mp4`）
    #[serde(default)]
    pub large: Vec<String>,
    /// 已经传到 R2 的
    #[serde(default)]
    pub uploaded: Vec<String>,
    #[serde(default)]
    pub d1_id: String,
    #[serde(default)]
    pub d1_name: String,
    #[serde(default)]
    pub r2_bucket: String,
    #[serde(default)]
    pub turnstile_sitekey: String,
    #[serde(default)]
    pub turnstile_domains: Vec<String>,
    #[serde(default)]
    pub setup_at: u64,
    /// 配置时值得告诉用户的事（比如令牌没有 Turnstile 权限）
    #[serde(default)]
    pub notes: Vec<String>,
}

impl Dynamic {
    pub fn has(&self, f: &str) -> bool {
        self.features.iter().any(|x| x == f)
    }
    /// 需要 D1（表单、评论）
    pub fn needs_db(&self) -> bool {
        self.has("form") || self.has("comments")
    }
    pub fn needs_r2(&self) -> bool {
        self.has("files") && !self.large.is_empty()
    }
}

pub fn path(site_dir: &Path) -> PathBuf {
    site_dir.join("dynamic.json")
}

pub fn load(site_dir: &Path) -> Option<Dynamic> {
    std::fs::read(path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

pub fn save(site_dir: &Path, d: &Dynamic) -> Result<(), String> {
    std::fs::create_dir_all(site_dir).map_err(|e| e.to_string())?;
    std::fs::write(path(site_dir), serde_json::to_vec_pretty(d).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// `_routes.json`：只让 `/api/*` 和 R2 大文件进 Functions。大文件太多时按所在目录合并成 `目录/*`。
pub fn routes_json(large: &[String]) -> String {
    let mut include: Vec<String> = vec!["/api/*".into()];
    let mut files: Vec<String> = large.iter().map(|p| if p.starts_with('/') { p.clone() } else { format!("/{p}") }).collect();
    files.sort();
    files.dedup();
    if files.len() + 1 > MAX_ROUTES {
        let mut dirs: Vec<String> = files.iter().map(|p| format!("{}/*", p.rsplit_once('/').map(|x| x.0).unwrap_or(""))).collect();
        dirs.sort();
        dirs.dedup();
        files = if dirs.len() + 1 > MAX_ROUTES { vec!["/*".into()] } else { dirs };
    }
    include.extend(files);
    serde_json::to_string_pretty(&serde_json::json!({ "version": 1, "include": include, "exclude": [] })).unwrap_or_default()
}

const ENV_LINES: &str = "\n# Turnstile 人机验证的站点密钥（表单、评论的验证组件用），Pilot 自动创建并填好 [public]\nPUBLIC_TURNSTILE_SITE_KEY=\n\n# Turnstile 人机验证的服务端密钥（Functions 校验用），Pilot 自动创建并填好 [secret]\nTURNSTILE_SECRET_KEY=\n";

/// 把 Functions 代码写进站点（app 目录）。按 features 写，去掉的功能把对应文件删掉。返回写了哪些文件。
pub fn write_code(app: &Path, d: &Dynamic) -> Result<Vec<String>, String> {
    let mut written = Vec::new();
    let mut put = |rel: &str, body: &str| -> Result<(), String> {
        let p = app.join(rel);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        if std::fs::read_to_string(&p).ok().as_deref() != Some(body) {
            std::fs::write(&p, body).map_err(|e| format!("写 {rel} 失败：{e}"))?;
        }
        written.push(rel.to_string());
        Ok(())
    };
    let drop = |rel: &str| {
        let _ = std::fs::remove_file(app.join(rel));
    };
    if d.needs_db() || d.needs_r2() {
        put("functions/_lib.js", LIB_JS)?;
        put("functions/api/health.js", HEALTH_JS)?;
        put("public/_routes.json", &routes_json(if d.needs_r2() { &d.large } else { &[] }))?;
    }
    if d.needs_db() {
        put("db/schema.sql", SCHEMA_SQL)?;
    }
    if d.has("form") { put("functions/api/form.js", FORM_JS)? } else { drop("functions/api/form.js") }
    if d.has("comments") { put("functions/api/comments.js", COMMENTS_JS)? } else { drop("functions/api/comments.js") }
    if d.needs_r2() { put("functions/[[path]].js", FILES_JS)? } else { drop("functions/[[path]].js") }
    if d.needs_db() {
        let ex = app.join(".env.example");
        let cur = std::fs::read_to_string(&ex).unwrap_or_default();
        if !cur.contains("TURNSTILE_SECRET_KEY") {
            std::fs::write(&ex, format!("{}{ENV_LINES}", cur.trim_end())).map_err(|e| e.to_string())?;
            written.push(".env.example".into());
        }
    }
    Ok(written)
}

/// schema.sql 拆成一条条语句（D1 接口一次执行一条更稳）。
pub fn statements(sql: &str) -> Vec<String> {
    let body: String = sql.lines().filter(|l| !l.trim_start().starts_with("--")).collect::<Vec<_>>().join("\n");
    body.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Turnstile 允许的域名：pages.dev 预览地址 + 正式域名（apex 和 www 都放）。
pub fn turnstile_domains(pages_sub: &str, domain: &str) -> Vec<String> {
    let mut v = vec![pages_sub.to_string()];
    let d = domain.trim().trim_start_matches("www.");
    if !d.is_empty() {
        v.push(d.to_string());
        v.push(format!("www.{d}"));
    }
    v.retain(|x| !x.is_empty());
    v.dedup();
    v
}

/// 写给 AI 的接口约定：前端怎么接表单、评论、大文件（Functions 由 Pilot 维护，AI 只做页面）。
pub fn ai_contract(d: &Dynamic, old_comments: bool, on_vps: bool) -> String {
    let mut s = if on_vps {
        String::from("【动态功能】站点部署在自己的服务器上，表单和评论接口由 Pilot 在服务器上提供（和 Cloudflare 版完全一样的接口）；仓库里的 `functions/`、`db/schema.sql`、`public/_routes.json` 也由 Pilot 维护，不要改、不要删。前端按下面的接口接：\n")
    } else {
        String::from("【动态功能】Pilot 已经把 Cloudflare Pages Functions 写进 `functions/`，数据表在 `db/schema.sql`，路由规则在 `public/_routes.json`：这些文件由 Pilot 维护，不要改、不要删。前端按下面的接口接：\n")
    };
    if d.has("form") {
        s.push_str("- 表单：`<form method=\"post\" action=\"/api/form\">`，隐藏字段 `_form`（表单名，如 contact / subscribe）和 `_page`（当前路径）；再放一个防机器人的隐藏输入 `<input name=\"_gotcha\" tabindex=\"-1\" autocomplete=\"off\" aria-hidden=\"true\" style=\"position:absolute;left:-9999px\">`。\n  人机验证：配置变量 `PUBLIC_TURNSTILE_SITE_KEY` 有值时，在表单里放 `<div class=\"cf-turnstile\" data-sitekey=\"（这个值）\"></div>`，并加载 `https://challenges.cloudflare.com/turnstile/v0/api.js`（async defer）；没值就都不放。\n  不用 JS 也要能提交：成功后跳回原页面带 `?sent=1`，失败带 `?error=原因`，页面据此显示提示。用 fetch 提交时发 FormData 或 JSON，返回 `{ok:true}` 或 `{ok:false,error}`（captcha 人机验证没过 / rate_limited 太频繁 / empty 没填内容）。\n");
    }
    if d.has("comments") {
        if old_comments {
            s.push_str("- 旧评论：在 `src/data/comments.json`（键是页面路径，值是 `[{id, parent, author, date, content}]`，content 是纯文本、段落之间空一行），在对应页面正文下面**构建时静态渲染**（搜索引擎要看得到），回复按 parent 缩进。\n");
        }
        s.push_str("- 新评论：`GET /api/comments?path=<页面路径>` 返回 `{ok, comments:[{id, parent_id, name, url, content, created_at}]}`（content 是纯文本，按纯文本显示，**不要当 HTML 插入**），接在旧评论后面；发评论 `POST /api/comments`，字段 path、name、content，可选 email、url、parent_id，同样带 `_gotcha` 和 Turnstile；返回 `pending:true` 时提示「评论已提交，审核后显示」。\n");
    }
    if d.needs_r2() {
        s.push_str(if on_vps { "- 大文件：超过 25 MB 的文件放在服务器的单独目录，原地址不变（Pilot 管），页面里照原路径链接，不要放进 `public/`。\n" } else { "- 大文件：超过 25 MB 的文件放在 Cloudflare R2，原地址不变（Pilot 管），页面里照原路径链接，不要放进 `public/`。\n" });
    }
    s.push_str("本地 `astro dev` / 预览时这些接口不存在（上线后才有）：页面要优雅降级（表单照常显示，评论区显示「评论暂时加载不了」），不能因此报错或白屏。");
    s
}

// ───────────────────────── 命令 ─────────────────────────

fn now() -> u64 {
    crate::now_secs()
}

#[tauri::command]
pub fn site_dynamic_state(state: tauri::State<'_, AppState>, id: String) -> Result<Option<Dynamic>, String> {
    state.sites.get(&id)?;
    Ok(load(&state.sites.data_dir(&id)))
}

/// 开关功能（不在迁移里的站点也能用）：写进 dynamic.json；之后点「配置」建资源。
#[tauri::command]
pub fn site_dynamic_features(state: tauri::State<'_, AppState>, id: String, features: Vec<String>) -> Result<Dynamic, String> {
    state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    let mut d = load(&dir).unwrap_or_default();
    d.features = features.into_iter().filter(|f| matches!(f.as_str(), "form" | "comments" | "files")).collect();
    d.features.sort();
    d.features.dedup();
    save(&dir, &d)?;
    Ok(d)
}

/// 给 AI 的消息：把表单 / 评论接到页面上（不在迁移里的站点开了动态功能之后用）。
#[tauri::command]
pub fn site_dynamic_message(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let d = load(&state.sites.data_dir(&id)).filter(|d| !d.features.is_empty()).ok_or("还没有开动态功能")?;
    let code = crate::site_git::code_dir(&site, state.conns.get(&site.gh_conn_id).ok().as_ref())?;
    let old = site.app_dir(&code).join("src/data/comments.json").is_file();
    let mut want = Vec::new();
    if d.has("form") {
        want.push("在需要的地方加上表单（联系页的留言表单；有订阅需求的加订阅表单），已有的表单改成提交到 /api/form");
    }
    if d.has("comments") {
        want.push("在文章页正文下面加评论区（列表 + 发表评论）");
    }
    Ok(format!(
        "给这个站点接上动态功能：{}。按站点现有的设计风格做，移动端也要好看；做完构建通过、本地预览检查，`git add -A && git commit`，简要汇报改了哪些页面。不要部署——上线由 Pilot 负责。\n\n{}",
        want.join("；"),
        ai_contract(&d, old, crate::site_vps_host::managed(&site))
    ))
}

pub const SETUP_STEPS: &[(&str, &str)] = &[("code", "写入 Functions 代码"), ("d1", "建数据库"), ("turnstile", "建人机验证"), ("r2", "上传大文件到 R2"), ("bind", "绑定到 Pages 项目")];

#[tauri::command]
pub async fn site_dynamic_setup(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    if site.deploy.mode != "direct" || site.deploy.project.is_empty() {
        return Err("动态功能跑在 Cloudflare Pages 上：先配好 Pages 项目（Pilot 构建后上传）".into());
    }
    let dir = state.sites.data_dir(&id);
    let d = load(&dir).unwrap_or_default();
    if d.features.is_empty() {
        return Err("还没有要开的动态功能".into());
    }
    let job = state.site_jobs.start(Some(app), &id, "dynamic:setup", "配置动态功能", SETUP_STEPS)?;
    let job_id = job.id.clone();
    let conns = state.conns.clone();
    tauri::async_runtime::spawn(async move {
        let r = run_setup(site, conns, dir, d, &job).await;
        job.finish(&r);
    });
    Ok(job_id)
}

async fn run_setup(site: crate::sites::Site, conns: crate::conns::ConnStore, dir: PathBuf, mut d: Dynamic, job: &crate::site_jobs::Job) -> Result<(), String> {
    let cf = conns.get(&site.deploy.cf_conn_id).map_err(|_| "站点的 Cloudflare 连接不存在".to_string())?;
    let token = crate::keychain::get_key(&cf.id)?;
    job.add_secret(&token);
    let acct = cf.account_id.clone();
    let project = site.deploy.project.clone();
    d.notes.clear();

    // 代码
    job.step("code", "running", "");
    let code = crate::site_git::code_dir(&site, None)?;
    let app_dir = site.app_dir(&code);
    let written = write_code(&app_dir, &d)?;
    let _ = crate::site_git::git_ok(&code, &["add", "-A"], "").await;
    let _ = crate::site_git::git_commit_ok(&code, &["commit", "-qm", &format!("动态功能：{}", feature_names(&d))]).await;
    job.step("code", "ok", &format!("{} 个文件", written.len()));

    // D1
    if d.needs_db() {
        job.step("d1", "running", "");
        let name = if d.d1_name.is_empty() { format!("pilot-{project}") } else { d.d1_name.clone() };
        let id = match crate::cf::d1_find(&token, &acct, &name).await? {
            Some(id) => id,
            None => crate::cf::d1_create(&token, &acct, &name).await.map_err(|e| format!("建 D1 数据库失败：{e}（令牌需要「D1：编辑」权限）"))?,
        };
        for st in statements(SCHEMA_SQL) {
            crate::cf::d1_query(&token, &acct, &id, &st, &[]).await.map_err(|e| format!("建数据表失败：{e}"))?;
        }
        d.d1_id = id;
        d.d1_name = name.clone();
        job.step("d1", "ok", &name);
    } else {
        job.step("d1", "skipped", "没有表单和评论");
    }

    // Turnstile
    let pages_sub = crate::cf::pages_subdomain(&token, &acct, &project).await.unwrap_or_else(|_| format!("{project}.pages.dev"));
    if d.needs_db() {
        job.step("turnstile", "running", "");
        let host = if site.domain.is_empty() { site.migrate_from.clone() } else { site.domain.clone() };
        let domains = turnstile_domains(&pages_sub, &host);
        let r = if d.turnstile_sitekey.is_empty() {
            crate::cf::turnstile_create(&token, &acct, &format!("{} · Pilot", site.name), &domains).await.map(Some)
        } else {
            crate::cf::turnstile_set_domains(&token, &acct, &d.turnstile_sitekey, &format!("{} · Pilot", site.name), &domains).await.map(|_| None)
        };
        match r {
            Ok(Some((key, secret))) => {
                d.turnstile_sitekey = key.clone();
                let keys = crate::sites::save_env(&site.id, &[("PUBLIC_TURNSTILE_SITE_KEY".into(), key), ("TURNSTILE_SECRET_KEY".into(), secret)], &[])?;
                if let Some(app) = crate::APP.get() {
                    use tauri::Manager;
                    let _ = app.state::<AppState>().sites.update(&site.id, now(), |s| {
                        s.build.env_keys = keys;
                        Ok(())
                    });
                }
                d.turnstile_domains = domains;
                job.step("turnstile", "ok", "已建好，密钥存成配置变量，上线时同步");
            }
            Ok(None) => {
                d.turnstile_domains = domains;
                job.step("turnstile", "ok", "已更新允许的域名");
            }
            Err(e) => {
                d.notes.push(format!("没建成 Turnstile 人机验证（{e}）：表单和评论照样能用，只靠隐藏字段和限频防垃圾。给令牌加上「Turnstile：编辑」权限后再配置一次就会补上"));
                job.step("turnstile", "skipped", "令牌没有 Turnstile 权限，先只用隐藏字段和限频");
            }
        }
    } else {
        job.step("turnstile", "skipped", "");
    }

    // R2 大文件
    if d.needs_r2() {
        job.step("r2", "running", "");
        let bucket = if d.r2_bucket.is_empty() { format!("pilot-{project}-files").chars().take(63).collect() } else { d.r2_bucket.clone() };
        crate::cf::r2_bucket_ensure(&token, &acct, &bucket).await.map_err(|e| format!("建 R2 存储桶失败：{e}（R2 要先在 Cloudflare 后台开通，令牌需要「R2：编辑」权限）"))?;
        d.r2_bucket = bucket;
        let large_dir = dir.join("migrate/large");
        let todo: Vec<String> = d.large.iter().filter(|p| !d.uploaded.contains(p)).cloned().collect();
        for (i, p) in todo.iter().enumerate() {
            job.step("r2", "running", &format!("{} / {}  {p}", i + 1, todo.len()));
            let f = large_dir.join(crate::migrate_extract::pct_decode(p.trim_start_matches('/')));
            let Ok(bytes) = std::fs::read(&f) else {
                d.notes.push(format!("{p} 本地没有文件，没传到 R2"));
                continue;
            };
            let ct = mime_of(p);
            crate::cf::r2_put(&token, &acct, &d.r2_bucket, &crate::migrate_extract::pct_decode(p.trim_start_matches('/')), bytes, ct).await?;
            d.uploaded.push(p.clone());
        }
        job.step("r2", "ok", &format!("{} 个文件", d.uploaded.len()));
    } else {
        job.step("r2", "skipped", "没有超过 25 MB 的文件");
    }

    // 绑定
    job.step("bind", "running", "");
    crate::cf::set_pages_bindings(&token, &acct, &project, (!d.d1_id.is_empty()).then_some(d.d1_id.as_str()), (!d.r2_bucket.is_empty()).then_some(d.r2_bucket.as_str())).await?;
    // 配置变量（Turnstile 密钥）现在就同步一次，不用等下次上线
    if let Some(app) = crate::APP.get() {
        use tauri::Manager;
        if let Ok(s) = app.state::<AppState>().sites.get(&site.id) {
            if let Err(e) = crate::site_config::sync(&s, &conns, &code, &dir).await {
                d.notes.push(format!("配置变量没同步成（{e}），下次上线时会再同步"));
            }
        }
    }
    job.step("bind", "ok", "下次上线后生效");
    d.setup_at = now();
    save(&dir, &d)?;
    Ok(())
}

fn feature_names(d: &Dynamic) -> String {
    d.features.iter().map(|f| match f.as_str() { "form" => "表单", "comments" => "评论", "files" => "大文件", x => x }).collect::<Vec<_>>().join("、")
}

fn mime_of(p: &str) -> &'static str {
    match crate::site_migrate::ext_of(p).as_str() {
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "application/octet-stream",
    }
}

// ───────────────────────── 收件箱 ─────────────────────────

#[derive(Serialize)]
pub struct Submission {
    pub id: i64,
    pub form: String,
    pub page: String,
    pub fields: Value,
    pub created_at: i64,
    pub read: bool,
}

#[derive(Serialize)]
pub struct Comment {
    pub id: i64,
    pub path: String,
    pub parent_id: Option<i64>,
    pub name: String,
    pub email: String,
    pub url: String,
    pub content: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct Inbox {
    pub submissions: Vec<Submission>,
    pub comments: Vec<Comment>,
    pub unread: i64,
    pub pending: i64,
}

/// 收件箱的数据在哪：Cloudflare 上的 D1，或者自己服务器上的 SQLite（Pilot 托管的 VPS，经 SSH 读）。
enum Store {
    D1 { token: String, acct: String, db: String },
    Vps { conn: crate::conns::Connection, v: crate::site_vps_host::VpsState },
}

impl Store {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Value>, String> {
        match self {
            Store::D1 { token, acct, db } => crate::cf::d1_query(token, acct, db, sql, params).await,
            Store::Vps { conn, v } => crate::site_vps_host::query(conn, v, sql, params).await,
        }
    }
}

fn store_of(state: &AppState, id: &str) -> Result<Store, String> {
    let site = state.sites.get(id)?;
    let dir = state.sites.data_dir(id);
    if crate::site_vps_host::managed(&site) {
        let v = crate::site_vps_host::load(&dir).filter(|v| v.api_port > 0).ok_or("这个站点的服务器上还没开表单和评论")?;
        let conn = state.conns.get(&site.deploy.ssh_conn_id).map_err(|_| "部署用的 SSH 连接不存在".to_string())?;
        return Ok(Store::Vps { conn, v });
    }
    let d = load(&dir).filter(|d| !d.d1_id.is_empty()).ok_or("这个站点还没有配置表单和评论")?;
    let cf = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "站点的 Cloudflare 连接不存在".to_string())?;
    Ok(Store::D1 { token: crate::keychain::get_key(&cf.id)?, acct: cf.account_id.clone(), db: d.d1_id })
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

#[tauri::command]
pub async fn site_inbox(state: tauri::State<'_, AppState>, id: String) -> Result<Inbox, String> {
    let st = store_of(&state, &id)?;
    let subs = st.query("SELECT id, form, page, fields, created_at, read FROM submissions ORDER BY created_at DESC LIMIT 100", &[]).await?;
    let coms = st
        .query("SELECT id, path, parent_id, name, email, url, content, status, created_at FROM comments WHERE status != 'spam' ORDER BY (status = 'pending') DESC, created_at DESC LIMIT 100", &[])
        .await?;
    let counts = st.query("SELECT (SELECT COUNT(*) FROM submissions WHERE read = 0) AS unread, (SELECT COUNT(*) FROM comments WHERE status = 'pending') AS pending", &[]).await?;
    let c = counts.first().cloned().unwrap_or_default();
    Ok(Inbox {
        submissions: subs
            .iter()
            .map(|r| Submission {
                id: r["id"].as_i64().unwrap_or(0),
                form: s(r, "form"),
                page: s(r, "page"),
                fields: serde_json::from_str(r["fields"].as_str().unwrap_or("{}")).unwrap_or(Value::Null),
                created_at: r["created_at"].as_i64().unwrap_or(0),
                read: r["read"].as_i64().unwrap_or(0) != 0,
            })
            .collect(),
        comments: coms
            .iter()
            .map(|r| Comment {
                id: r["id"].as_i64().unwrap_or(0),
                path: s(r, "path"),
                parent_id: r["parent_id"].as_i64(),
                name: s(r, "name"),
                email: s(r, "email"),
                url: s(r, "url"),
                content: s(r, "content"),
                status: s(r, "status"),
                created_at: r["created_at"].as_i64().unwrap_or(0),
            })
            .collect(),
        unread: c["unread"].as_i64().unwrap_or(0),
        pending: c["pending"].as_i64().unwrap_or(0),
    })
}

/// 处理收件箱：kind = submission | comment；action = read | delete | approve | spam。
pub fn inbox_sql(kind: &str, action: &str, n: usize) -> Result<String, String> {
    if n == 0 || n > 200 {
        return Err("一次处理 1–200 条".into());
    }
    let ph = vec!["?"; n].join(",");
    Ok(match (kind, action) {
        ("submission", "read") => format!("UPDATE submissions SET read = 1 WHERE id IN ({ph})"),
        ("submission", "delete") => format!("DELETE FROM submissions WHERE id IN ({ph})"),
        ("comment", "approve") => format!("UPDATE comments SET status = 'approved' WHERE id IN ({ph})"),
        ("comment", "spam") => format!("UPDATE comments SET status = 'spam' WHERE id IN ({ph})"),
        ("comment", "delete") => format!("DELETE FROM comments WHERE id IN ({ph})"),
        _ => return Err("不支持的操作".into()),
    })
}

#[tauri::command]
pub async fn site_inbox_act(state: tauri::State<'_, AppState>, id: String, kind: String, action: String, ids: Vec<i64>) -> Result<(), String> {
    let sql = inbox_sql(&kind, &action, ids.len())?;
    let st = store_of(&state, &id)?;
    let params: Vec<Value> = ids.into_iter().map(Value::from).collect();
    st.query(&sql, &params).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        let r: Value = serde_json::from_str(&routes_json(&["/v/a.mp4".into(), "v/b.mp4".into()])).unwrap();
        assert_eq!(r["include"], serde_json::json!(["/api/*", "/v/a.mp4", "/v/b.mp4"]));
        let many: Vec<String> = (0..150).map(|i| format!("/d{}/f{i}.mp4", i % 3)).collect();
        let r: Value = serde_json::from_str(&routes_json(&many)).unwrap();
        assert_eq!(r["include"], serde_json::json!(["/api/*", "/d0/*", "/d1/*", "/d2/*"]));
    }

    #[test]
    fn schema_statements() {
        let st = statements(SCHEMA_SQL);
        assert!(st.len() >= 4);
        assert!(st.iter().all(|s| s.starts_with("CREATE")));
    }

    #[test]
    fn writes_code_by_feature() {
        let d = tempfile::tempdir().unwrap();
        let mut dy = Dynamic { features: vec!["form".into()], ..Default::default() };
        let w = write_code(d.path(), &dy).unwrap();
        assert!(w.contains(&"functions/api/form.js".to_string()));
        assert!(d.path().join("functions/_lib.js").is_file());
        assert!(!d.path().join("functions/api/comments.js").exists());
        assert!(std::fs::read_to_string(d.path().join(".env.example")).unwrap().contains("[secret]\nTURNSTILE_SECRET_KEY="));
        let vars = crate::site_config::parse_example(&std::fs::read_to_string(d.path().join(".env.example")).unwrap());
        assert!(vars.iter().any(|(k, _, sec)| k == "TURNSTILE_SECRET_KEY" && *sec));
        assert!(vars.iter().any(|(k, _, sec)| k == "PUBLIC_TURNSTILE_SITE_KEY" && !*sec));
        // 换成评论 + 大文件：表单文件删掉
        dy.features = vec!["comments".into(), "files".into()];
        dy.large = vec!["/v/a.mp4".into()];
        write_code(d.path(), &dy).unwrap();
        assert!(!d.path().join("functions/api/form.js").exists());
        assert!(d.path().join("functions/api/comments.js").is_file());
        assert!(d.path().join("functions/[[path]].js").is_file());
        let env = std::fs::read_to_string(d.path().join(".env.example")).unwrap();
        assert_eq!(env.matches("TURNSTILE_SECRET_KEY").count(), 1);
    }

    #[test]
    fn domains_and_sql() {
        assert_eq!(turnstile_domains("x.pages.dev", "www.a.com"), vec!["x.pages.dev", "a.com", "www.a.com"]);
        assert_eq!(turnstile_domains("x.pages.dev", ""), vec!["x.pages.dev"]);
        assert_eq!(inbox_sql("comment", "approve", 2).unwrap(), "UPDATE comments SET status = 'approved' WHERE id IN (?,?)");
        assert!(inbox_sql("comment", "drop", 1).is_err());
        assert!(inbox_sql("submission", "read", 0).is_err());
    }
}
