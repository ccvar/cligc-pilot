//! WordPress.com：「分发」把站点文章的摘要发到用户自己的 WordPress.com 站点。
//!
//! - OAuth 应用是用户自己在 developer.wordpress.com/apps 注册的（客户端 ID / 密钥进钥匙串，配一次）。
//!   WordPress.com 要求回调地址和注册时登记的一字不差，所以本机回调固定用 `REDIRECT`（不像 Google 那样随机端口）。
//! - 授权一次拿到的令牌不会过期（WordPress.com 的 OAuth 令牌是长期的），scope=global：能列出并发到账号下所有站点。
//! - 账号是一种连接（kind = wordpress），令牌进钥匙串。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::AppState;

pub const PORT: u16 = 47613;
pub const REDIRECT: &str = "http://127.0.0.1:47613/";
const CLIENT_ACCOUNT: &str = "wpcom-oauth-client";
const API: &str = "https://public-api.wordpress.com/rest/v1.1";

#[derive(Serialize, Deserialize, Clone)]
struct Client {
    client_id: String,
    client_secret: String,
}

#[derive(Serialize, Deserialize)]
struct Cred {
    access_token: String,
}

#[derive(Serialize)]
pub struct ClientInfo {
    pub configured: bool,
    pub client_id: String,
    /// 注册应用时要填的回调地址
    pub redirect: &'static str,
}

#[derive(Serialize, Clone, Debug)]
pub struct WpSite {
    pub id: String,
    pub name: String,
    pub url: String,
}

fn load_client() -> Option<Client> {
    crate::keychain::get_named_secret(CLIENT_ACCOUNT).ok().and_then(|r| serde_json::from_str(&r).ok())
}

#[tauri::command]
pub fn wpcom_client_info() -> ClientInfo {
    match load_client() {
        Some(c) => ClientInfo { configured: true, client_id: c.client_id, redirect: REDIRECT },
        None => ClientInfo { configured: false, client_id: String::new(), redirect: REDIRECT },
    }
}

#[tauri::command]
pub fn wpcom_save_client(client_id: String, client_secret: String) -> Result<ClientInfo, String> {
    let (id, secret) = (client_id.trim().to_string(), client_secret.trim().to_string());
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err("Client ID 是一串数字（在 developer.wordpress.com/apps 里你的应用页面上）".into());
    }
    if secret.len() < 20 {
        return Err("Client Secret 不对：复制应用页面上的 Client Secret".into());
    }
    let raw = serde_json::to_string(&Client { client_id: id, client_secret: secret }).map_err(|e| e.to_string())?;
    crate::keychain::set_named_secret(CLIENT_ACCOUNT, &raw)?;
    Ok(wpcom_client_info())
}

const DONE_HTML: &str = "<!doctype html><meta charset=utf-8><title>Cligc Pilot</title><body style=\"font:15px -apple-system,sans-serif;padding:48px;color:#26241f\"><h2>授权完成</h2><p>可以关掉这个页面，回到 Cligc Pilot。</p></body>";
const FAIL_HTML: &str = "<!doctype html><meta charset=utf-8><title>Cligc Pilot</title><body style=\"font:15px -apple-system,sans-serif;padding:48px;color:#26241f\"><h2>没有完成授权</h2><p>回到 Cligc Pilot 查看原因。</p></body>";

async fn respond(stream: &mut tokio::net::TcpStream, html: &str) {
    let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len());
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// 浏览器授权 → 固定端口接回调 → 换令牌。
async fn authorize(client: &Client, open: impl Fn(&str) -> Result<(), String>) -> Result<String, String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", PORT)).await.map_err(|_| format!("本机端口 {PORT} 被占用了：关掉占用它的程序，或者稍后再试"))?;
    let state = uuid::Uuid::new_v4().simple().to_string();
    let url = reqwest::Url::parse_with_params(
        "https://public-api.wordpress.com/oauth2/authorize",
        &[("client_id", client.client_id.as_str()), ("redirect_uri", REDIRECT), ("response_type", "code"), ("scope", "global"), ("state", &state)],
    )
    .map_err(|e| e.to_string())?;
    open(url.as_str())?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    let code = loop {
        if tokio::time::Instant::now() > deadline {
            return Err("5 分钟内没有完成浏览器授权，已取消".into());
        }
        let Ok(Ok((mut stream, _))) = tokio::time::timeout(Duration::from_millis(800), listener.accept()).await else { continue };
        let mut buf = vec![0u8; 8192];
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await.ok().and_then(|r| r.ok()).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let params = crate::google_oauth::parse_callback(req.lines().next().unwrap_or(""));
        let get = |k: &str| params.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.clone());
        if let Some(e) = get("error") {
            respond(&mut stream, FAIL_HTML).await;
            return Err(if e == "access_denied" { "你在浏览器里拒绝了授权".into() } else { format!("WordPress.com 返回错误：{e}") });
        }
        match (get("code"), get("state")) {
            (Some(c), Some(s)) if s == state => {
                respond(&mut stream, DONE_HTML).await;
                break c;
            }
            (Some(_), _) => {
                respond(&mut stream, FAIL_HTML).await;
                return Err("回调的 state 不匹配，已拒绝（可能是旧的授权页）".into());
            }
            _ => {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            }
        }
    };
    let r = reqwest::Client::new()
        .post("https://public-api.wordpress.com/oauth2/token")
        .form(&[("client_id", client.client_id.as_str()), ("client_secret", &client.client_secret), ("redirect_uri", REDIRECT), ("code", &code), ("grant_type", "authorization_code")])
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("连不上 WordPress.com：{e}"))?;
    let ok = r.status().is_success();
    let v: Value = r.json().await.unwrap_or_default();
    if !ok {
        return Err(format!("换取令牌失败：{}", v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("未知错误")));
    }
    v["access_token"].as_str().map(String::from).ok_or_else(|| "WordPress.com 没有返回令牌".to_string())
}

async fn api(token: &str, method: reqwest::Method, url: &str, form: Option<&[(&str, &str)]>) -> Result<Value, String> {
    let mut req = reqwest::Client::new().request(method, url).bearer_auth(token).timeout(Duration::from_secs(45));
    if let Some(f) = form {
        req = req.form(f);
    }
    let r = req.send().await.map_err(|e| format!("连不上 WordPress.com：{e}"))?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or_default();
    if !status.is_success() {
        let msg = v["message"].as_str().unwrap_or("");
        return Err(match status.as_u16() {
            401 | 403 if v["error"].as_str() == Some("authorization_required") || v["error"].as_str() == Some("invalid_token") => "WordPress.com 授权已失效：在「连接 → WordPress.com」里重新授权".into(),
            _ => format!("WordPress.com 返回 {status}：{msg}"),
        });
    }
    Ok(v)
}

fn token_of(state: &AppState, conn_id: &str) -> Result<String, String> {
    let conn = state.conns.get(conn_id)?;
    if conn.kind != "wordpress" {
        return Err("这不是 WordPress.com 连接".into());
    }
    let raw = crate::keychain::get_key(&conn.id)?;
    serde_json::from_str::<Cred>(&raw).map(|c| c.access_token).map_err(|_| "WordPress.com 凭据损坏，请重新授权".into())
}

/// 账号下的站点（只列能发文章的）。
pub async fn sites(state: &AppState, conn_id: &str) -> Result<Vec<WpSite>, String> {
    let t = token_of(state, conn_id)?;
    let v = api(&t, reqwest::Method::GET, &format!("{API}/me/sites?fields=ID,name,URL,capabilities"), None).await?;
    Ok(v["sites"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|s| s["capabilities"]["publish_posts"].as_bool().unwrap_or(true))
                .filter_map(|s| Some(WpSite { id: s["ID"].as_u64()?.to_string(), name: s["name"].as_str().unwrap_or("").to_string(), url: s["URL"].as_str().unwrap_or("").to_string() }))
                .collect()
        })
        .unwrap_or_default())
}

/// 站点里已有的文章（含草稿）：(文章 id, 网址, 正文)。对账用，最多看 500 篇。
pub async fn posts(state: &AppState, conn_id: &str, site_id: &str) -> Result<Vec<(String, String, String)>, String> {
    if site_id.is_empty() || !site_id.chars().all(|c| c.is_ascii_digit()) {
        return Err("还没选站点".into());
    }
    let t = token_of(state, conn_id)?;
    let mut out = Vec::new();
    for page in 1..=5 {
        let v = api(&t, reqwest::Method::GET, &format!("{API}/sites/{site_id}/posts?number=100&page={page}&status=publish,draft,pending,future&fields=ID,URL,content"), None).await?;
        let list = v["posts"].as_array().cloned().unwrap_or_default();
        for p in &list {
            out.push((p["ID"].as_u64().map(|i| i.to_string()).unwrap_or_default(), p["URL"].as_str().unwrap_or("").to_string(), p["content"].as_str().unwrap_or("").to_string()));
        }
        if list.len() < 100 {
            break;
        }
    }
    Ok(out)
}

/// 发一篇：返回 (文章 id, 网址)。`draft` 为真时存成草稿。
pub async fn publish(state: &AppState, conn_id: &str, site_id: &str, title: &str, html: &str, labels: &[String], draft: bool) -> Result<(String, String), String> {
    if site_id.is_empty() || !site_id.chars().all(|c| c.is_ascii_digit()) {
        return Err("还没选站点".into());
    }
    let t = token_of(state, conn_id)?;
    let tags = labels.join(",");
    let v = api(&t, reqwest::Method::POST, &format!("{API}/sites/{site_id}/posts/new"), Some(&[("title", title), ("content", html), ("tags", &tags), ("status", if draft { "draft" } else { "publish" })])).await?;
    Ok((v["ID"].as_u64().map(|i| i.to_string()).unwrap_or_default(), if draft { String::new() } else { v["URL"].as_str().unwrap_or("").to_string() }))
}

/// 授权一个 WordPress.com 账号：新账号新建连接；`update_id` 非空时重新授权那一条（必须是同一个账号）。
#[tauri::command]
pub async fn wpcom_login(app: tauri::AppHandle, state: tauri::State<'_, AppState>, update_id: String) -> Result<crate::conns::Connection, String> {
    use tauri_plugin_opener::OpenerExt;
    let client = load_client().ok_or("还没有配置 WordPress.com 应用：先填一次 Client ID 和 Client Secret")?;
    let token = authorize(&client, |url| app.opener().open_url(url, None::<&str>).map_err(|e| format!("打开浏览器失败：{e}"))).await?;
    let me = api(&token, reqwest::Method::GET, &format!("{API}/me?fields=username,email"), None).await?;
    let login = me["email"].as_str().filter(|s| !s.is_empty()).or(me["username"].as_str()).unwrap_or("").to_string();
    let raw = serde_json::to_string(&Cred { access_token: token }).map_err(|e| e.to_string())?;
    let dedupe = format!("oauth:{login}");
    let target = if update_id.is_empty() {
        state.conns.list().into_iter().find(|c| c.kind == "wordpress" && !login.is_empty() && c.key_prefix == dedupe).map(|c| c.id)
    } else {
        let cur = state.conns.get(&update_id)?;
        if !cur.login.is_empty() && !login.is_empty() && !cur.login.eq_ignore_ascii_case(&login) {
            return Err(format!("浏览器里登录的是 {login}，不是 {}：要加新账号请用「连接 → WordPress.com → +」", cur.login));
        }
        Some(update_id)
    };
    match target {
        Some(id) => state.conns.replace_data_secret(&id, &raw, "oauth", &login, &dedupe),
        None => state.conns.add_data_source("wordpress", &format!("WordPress.com · {}", if login.is_empty() { "账号" } else { &login }), &raw, "oauth", &login, "", &dedupe),
    }
}

#[tauri::command]
pub async fn wpcom_sites(state: tauri::State<'_, AppState>, conn_id: String) -> Result<Vec<WpSite>, String> {
    sites(&state, &conn_id).await
}
