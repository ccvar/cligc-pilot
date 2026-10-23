//! 数据源连接（Google / Bing）与站点数据的前端命令。

use serde::Serialize;

use crate::google;
use crate::AppState;

#[derive(Serialize, Default)]
pub struct GoogleResources {
    pub email: String,
    pub project: String,
    pub properties: Vec<google::Ga4Property>,
    pub sites: Vec<google::GscSite>,
    /// 单项读取失败的原因（另一项照常返回）。
    pub ga4_error: String,
    pub gsc_error: String,
}

async fn resources_with(cache_key: &str, cred: &google::Credential) -> Result<GoogleResources, String> {
    let token = google::access_token(cache_key, cred, &google::read_scopes()).await?;
    let mut out = GoogleResources::default();
    match google::list_ga4_properties(&token).await {
        Ok(p) => out.properties = p,
        Err(e) => out.ga4_error = e,
    }
    match google::list_gsc_sites(&token).await {
        Ok(s) => out.sites = s,
        Err(e) => out.gsc_error = e,
    }
    Ok(out)
}

/// 验证服务账号 JSON：换一次令牌，并列出它能看到的 GA4 媒体资源与 Search Console 资源。
#[tauri::command]
pub async fn verify_google_sa(json: String) -> Result<GoogleResources, String> {
    let sa = google::parse_service_account(&json)?;
    let key = format!("verify:{}", sa.client_email);
    let email = sa.client_email.clone();
    let project = sa.project_id.clone();
    let mut r = resources_with(&key, &google::Credential::ServiceAccount(sa)).await?;
    google::forget_token(&key);
    r.email = email;
    r.project = project;
    Ok(r)
}

#[tauri::command]
pub async fn connect_google_sa(state: tauri::State<'_, AppState>, name: String, json: String) -> Result<crate::conns::Connection, String> {
    let sa = google::parse_service_account(&json)?;
    // 先确认能换到令牌，免得存下一枚已被删除的密钥。
    google::access_token(&format!("verify:{}", sa.client_email), &google::Credential::ServiceAccount(sa.clone()), &google::read_scopes()).await?;
    let name = if name.trim().is_empty() { format!("Google · {}", sa.client_email.split('@').next().unwrap_or("服务账号")) } else { name };
    state.conns.add_data_source("google", &name, json.trim(), "service_account", &sa.client_email, &sa.project_id, &sa.client_email)
}

#[tauri::command]
pub async fn update_google_sa(state: tauri::State<'_, AppState>, conn_id: String, json: String) -> Result<crate::conns::Connection, String> {
    let sa = google::parse_service_account(&json)?;
    google::access_token(&format!("verify:{}", sa.client_email), &google::Credential::ServiceAccount(sa.clone()), &google::read_scopes()).await?;
    google::forget_token(&conn_id);
    state.conns.replace_data_secret(&conn_id, json.trim(), "service_account", &sa.client_email, &sa.client_email)
}

#[tauri::command]
pub async fn list_google_resources(state: tauri::State<'_, AppState>, conn_id: String) -> Result<GoogleResources, String> {
    let c = state.conns.get(&conn_id)?;
    if c.kind != "google" {
        return Err("不是 Google 连接".into());
    }
    let raw = crate::keychain::get_key(&c.id)?;
    let cred = google::parse_credential(&c.key_kind, &raw)?;
    let mut r = resources_with(&c.id, &cred).await?;
    r.email = c.login.clone();
    r.project = c.account_id.clone();
    Ok(r)
}

#[tauri::command]
pub async fn verify_bing_key(key: String) -> Result<Vec<crate::bing::BingSite>, String> {
    crate::bing::list_sites(key.trim()).await
}

#[tauri::command]
pub async fn connect_bing(state: tauri::State<'_, AppState>, name: String, key: String) -> Result<crate::conns::Connection, String> {
    let key = key.trim().to_string();
    let sites = crate::bing::list_sites(&key).await?;
    let prefix = crate::keychain::key_prefix(&key);
    let name = if name.trim().is_empty() { "Bing Webmaster".to_string() } else { name };
    let login = sites.first().map(|s| s.url.clone()).unwrap_or_default();
    state.conns.add_data_source("bing", &name, &key, "bing_key", &login, "", &prefix)
}

#[tauri::command]
pub async fn update_bing_key(state: tauri::State<'_, AppState>, conn_id: String, key: String) -> Result<crate::conns::Connection, String> {
    let key = key.trim().to_string();
    crate::bing::list_sites(&key).await?;
    let c = state.conns.get(&conn_id)?;
    state.conns.replace_data_secret(&conn_id, &key, "bing_key", &c.login, &crate::keychain::key_prefix(&key))
}

#[tauri::command]
pub async fn list_bing_sites(state: tauri::State<'_, AppState>, conn_id: String) -> Result<Vec<crate::bing::BingSite>, String> {
    let c = state.conns.get(&conn_id)?;
    if c.kind != "bing" {
        return Err("不是 Bing 连接".into());
    }
    crate::bing::list_sites(&crate::keychain::get_key(&c.id)?).await
}

/// 拉取站点数据（force=false 时只拉 6 小时内没拉过的源）。
#[tauri::command]
pub async fn refresh_site_data(state: tauri::State<'_, AppState>, id: String, force: bool) -> Result<crate::site_data::CacheMeta, String> {
    let site = state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    Ok(crate::site_data::refresh(&site, &state.conns, &dir, force).await)
}

#[tauri::command]
pub fn site_dashboard(state: tauri::State<'_, AppState>, id: String) -> Result<crate::site_data::Dashboard, String> {
    state.sites.get(&id)?;
    Ok(crate::site_data::dashboard(&state.sites.data_dir(&id)))
}

/// 站点静态资源目录（构建时原样拷进产物根目录）：IndexNow 验证文件放这里。
pub fn static_dir(framework: &str) -> &'static str {
    match framework {
        "astro" | "vite" | "next" | "nuxt" | "node" => "public",
        "sveltekit" | "gatsby" | "hugo" => "static",
        "vitepress" => "docs/public",
        _ => "",
    }
}

#[derive(Serialize)]
pub struct IndexNowSetup {
    pub key: String,
    /// 写入的验证文件（相对代码目录）。
    pub file: String,
    /// GitHub 来源：文件写进了检出，需要提交（返回 PR 地址时已由 Pilot 提交）。
    pub pr_url: String,
    pub note: String,
}

/// 生成 IndexNow 密钥并把验证文件写进站点：本地来源直接写进文件夹（随下次上线生效）；
/// GitHub 来源由 Pilot 在 `pilot/indexnow-key` 分支提交、推送并开 PR。
#[tauri::command]
pub async fn setup_indexnow(state: tauri::State<'_, AppState>, id: String) -> Result<IndexNowSetup, String> {
    let site = state.sites.get(&id)?;
    let key = if site.data.indexnow_key.is_empty() { crate::bing::new_indexnow_key() } else { site.data.indexnow_key.clone() };
    let rel_dir = static_dir(&site.framework);
    let rel = if rel_dir.is_empty() { format!("{key}.txt") } else { format!("{rel_dir}/{key}.txt") };
    let mut out = IndexNowSetup { key: key.clone(), file: rel.clone(), pr_url: String::new(), note: String::new() };
    match site.source.as_str() {
        "local" => {
            let p = std::path::Path::new(&site.local_dir).join(&rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, &key).map_err(|e| format!("写入验证文件失败：{e}"))?;
            out.note = "验证文件已写进站点文件夹，下次上线后生效（请把它一起提交到仓库）。".into();
        }
        _ => {
            let gh = state.conns.get(&site.gh_conn_id)?;
            let token = crate::keychain::get_key(&gh.id)?;
            let main = crate::github::repo_dir(&gh.skill_dir, &site.repo)?;
            crate::github::ensure_checkout(&gh.skill_dir, &main.to_string_lossy(), &token, &gh.login, &gh.account_id).await?;
            let wt = state.sites.data_dir(&id).join("work").join("indexnow");
            crate::site_git::git_ok(&main, &["fetch", "--quiet", "origin", &site.default_branch], &token).await?;
            let branch = "pilot/indexnow-key";
            crate::site_git::worktree_at(&main, &wt, &format!("origin/{}", site.default_branch), Some(branch)).await?;
            let p = wt.join(&rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, &key).map_err(|e| e.to_string())?;
            crate::site_git::git_ok(&wt, &["add", &rel], "").await?;
            let (name, email) = crate::github::commit_identity(&gh.login, &gh.account_id);
            crate::site_git::git_ok(&wt, &["-c", &format!("user.name={name}"), "-c", &format!("user.email={email}"), "commit", "-qm", "chore: add IndexNow key file"], "").await?;
            crate::site_git::git_ok(&wt, &["push", "--force", "-u", "origin", branch], &token).await?;
            let pr = crate::site_github::create_pr(&token, &site.repo, branch, &site.default_branch, "Add IndexNow key file", "Adds the IndexNow verification file so Pilot can notify search engines about updated pages after each deploy.").await?;
            out.pr_url = pr.url;
            crate::site_git::remove_worktree(&main, &wt).await;
            out.note = "已开 PR，合并并上线后生效。".into();
        }
    }
    state.sites.update(&id, crate::now_secs(), |s| {
        s.data.indexnow_key = key;
        Ok(())
    })?;
    Ok(out)
}

/// 读取用户在文件选择框里选中的服务账号 JSON（只接受 .json、64KB 以内）。
#[tauri::command]
pub fn read_key_file(path: String) -> Result<String, String> {
    let p = std::path::Path::new(&path);
    if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("json")) != Some(true) {
        return Err("请选择 .json 密钥文件".into());
    }
    let meta = std::fs::metadata(p).map_err(|e| format!("读取文件失败：{e}"))?;
    if meta.len() > 64 * 1024 {
        return Err("文件太大，不像服务账号密钥".into());
    }
    std::fs::read_to_string(p).map_err(|e| format!("读取文件失败：{e}"))
}

// ---------- Google OAuth 客户端（全局一份） ----------
//
// OAuth 客户端回答的是「哪个应用在申请授权」：整个 Pilot 配一次就够了，存在钥匙串里。
// 每次「在浏览器中登录」只是再授权一个 Google 账号，生成一条数据源连接；可以有任意多个账号。
// 每条连接的凭据里仍各自带一份 client_id / secret：之后改了全局客户端，已授权的账号照常刷新令牌。

const OAUTH_CLIENT_ACCOUNT: &str = "google-oauth-client";

#[derive(serde::Deserialize, Serialize, Clone)]
struct OAuthClient {
    client_id: String,
    client_secret: String,
}

#[derive(Serialize)]
pub struct OAuthClientInfo {
    pub configured: bool,
    /// 只回给前端客户端 ID（不是机密）；密钥永远不出钥匙串。
    pub client_id: String,
}

fn load_client(state: &AppState) -> Option<OAuthClient> {
    if let Some(c) = crate::keychain::get_named_secret(OAUTH_CLIENT_ACCOUNT).ok().and_then(|raw| serde_json::from_str::<OAuthClient>(&raw).ok()) {
        return Some(c);
    }
    // 老版本是每条连接各填一次：沿用已授权账号里的那份客户端，免得用户再填。
    let adopted = state.conns.list().into_iter().filter(|c| c.kind == "google" && c.key_kind == "oauth").find_map(|c| {
        let raw = crate::keychain::get_key(&c.id).ok()?;
        match google::parse_credential("oauth", &raw).ok()? {
            google::Credential::OAuth(o) if !o.client_id.is_empty() && !o.client_secret.is_empty() => Some(OAuthClient { client_id: o.client_id, client_secret: o.client_secret }),
            _ => None,
        }
    })?;
    if let Ok(raw) = serde_json::to_string(&adopted) {
        let _ = crate::keychain::set_named_secret(OAUTH_CLIENT_ACCOUNT, &raw);
    }
    Some(adopted)
}

pub fn valid_client_id(id: &str) -> bool {
    let id = id.trim();
    id.ends_with(".apps.googleusercontent.com") && id.len() > ".apps.googleusercontent.com".len() && !id.contains(char::is_whitespace)
}

#[tauri::command]
pub fn google_oauth_client(state: tauri::State<'_, AppState>) -> OAuthClientInfo {
    match load_client(&state) {
        Some(c) => OAuthClientInfo { configured: true, client_id: c.client_id },
        None => OAuthClientInfo { configured: false, client_id: String::new() },
    }
}

/// 保存全局 OAuth 客户端。密钥留空表示只改 ID、沿用原来的密钥。
#[tauri::command]
pub fn save_google_oauth_client(state: tauri::State<'_, AppState>, client_id: String, client_secret: String) -> Result<OAuthClientInfo, String> {
    let client_id = client_id.trim().to_string();
    if !valid_client_id(&client_id) {
        return Err("客户端 ID 应该形如 1234-xxxx.apps.googleusercontent.com".into());
    }
    let secret = match client_secret.trim() {
        "" => load_client(&state).map(|c| c.client_secret).ok_or("请填写客户端密钥")?,
        s => s.to_string(),
    };
    let raw = serde_json::to_string(&OAuthClient { client_id: client_id.clone(), client_secret: secret }).map_err(|e| e.to_string())?;
    crate::keychain::set_named_secret(OAUTH_CLIENT_ACCOUNT, &raw)?;
    Ok(OAuthClientInfo { configured: true, client_id })
}

/// Google 账号登录（OAuth）：用全局客户端在浏览器里授权一个账号。
/// 新账号 → 新建连接；已经授权过的同一个账号 → 原地更新凭据（比如补授管理权限、授权过期后重登）。
/// `update_id` 非空时固定更新那条连接。
#[tauri::command]
pub async fn google_oauth_login(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    name: String,
    manage: bool,
    update_id: String,
) -> Result<crate::conns::Connection, String> {
    use tauri_plugin_opener::OpenerExt;
    let client = load_client(&state).ok_or("还没有配置 Google OAuth 客户端")?;
    let cred = crate::google_oauth::login(&client.client_id, &client.client_secret, manage, |url| {
        app.opener().open_url(url, None::<&str>).map_err(|e| format!("打开浏览器失败：{e}"))
    })
    .await?;
    let raw = serde_json::to_string(&cred).map_err(|e| e.to_string())?;
    let email = cred.email.clone();
    let dedupe = format!("oauth:{email}");
    let target = if update_id.is_empty() {
        state.conns.list().into_iter().find(|c| c.kind == "google" && !email.is_empty() && c.key_prefix == dedupe).map(|c| c.id)
    } else {
        // 重新授权：浏览器里选错账号时不能把这条连接悄悄换成另一个账号
        let cur = state.conns.get(&update_id)?;
        if !cur.login.is_empty() && !email.is_empty() && !cur.login.eq_ignore_ascii_case(&email) {
            return Err(format!("浏览器里选的是 {email}，不是 {}。要添加新账号请用「添加数据源 → Google 账号」；要重新授权这个账号，请在浏览器里选 {}", cur.login, cur.login));
        }
        Some(update_id)
    };
    match target {
        Some(id) => {
            google::forget_token(&id);
            state.conns.replace_data_secret(&id, &raw, "oauth", &email, &dedupe)
        }
        None => {
            let name = if name.trim().is_empty() { format!("Google · {}", if email.is_empty() { "OAuth" } else { &email }) } else { name };
            state.conns.add_data_source("google", &name, &raw, "oauth", &email, "", &dedupe)
        }
    }
}

#[tauri::command]
pub fn google_oauth_cancel() {
    crate::google_oauth::cancel();
}

/// 这条 Google 连接能不能做「新站闭环」里的写操作（OAuth 且授予了管理权限）。
#[tauri::command]
pub fn google_can_manage(state: tauri::State<'_, AppState>, conn_id: String) -> Result<bool, String> {
    let c = state.conns.get(&conn_id)?;
    if c.kind != "google" || c.key_kind != "oauth" {
        return Ok(false);
    }
    let raw = crate::keychain::get_key(&c.id)?;
    match crate::google::parse_credential("oauth", &raw)? {
        crate::google::Credential::OAuth(o) => Ok(crate::google_oauth::can_manage(&o)),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::valid_client_id;

    #[test]
    fn oauth_client_id_must_look_like_a_google_desktop_client() {
        assert!(valid_client_id("1234-abcd.apps.googleusercontent.com"));
        assert!(valid_client_id("  1234-abcd.apps.googleusercontent.com "));
        assert!(!valid_client_id(".apps.googleusercontent.com"));
        assert!(!valid_client_id("1234 abcd.apps.googleusercontent.com"));
        assert!(!valid_client_id("GOCSPX-secret"));
    }
}
