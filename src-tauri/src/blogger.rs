//! Blogger：「分发」把站点新文章的摘要发到用户自己的博客。
//!
//! - 账号是单独一种连接（kind = blogger），用户自己选一个 Google 账号授权，只申请 Blogger（加邮箱用来显示是谁），
//!   和看网站数据的 Google 账号分开。凭证（OAuthCred）进钥匙串，换令牌走 `google::access_token`。
//! - 浏览器授权复用 `google_oauth::authorize`，OAuth 客户端和网站数据 / Gmail 共用（`data_cmds::gmail_client`）。
//! - Blogger API 不能新建博客：博客要用户先在 blogger.com 建好，这里只列出来选。

use serde::Serialize;
use serde_json::{json, Value};

use crate::google::{self, Credential, OAuthCred};
use crate::AppState;

pub const SCOPE: &str = "https://www.googleapis.com/auth/blogger";
const API: &str = "https://www.googleapis.com/blogger/v3";

#[derive(Serialize, Clone, Debug)]
pub struct Blog {
    pub id: String,
    pub name: String,
    pub url: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Posted {
    pub id: String,
    /// 草稿没有公开网址
    pub url: String,
    /// LIVE | DRAFT
    pub status: String,
}

fn cred_of(state: &AppState, conn_id: &str) -> Result<(crate::conns::Connection, Credential), String> {
    let conn = state.conns.get(conn_id)?;
    if conn.kind != "blogger" {
        return Err("这不是 Blogger 连接".into());
    }
    let raw = crate::keychain::get_key(&conn.id)?;
    Ok((conn, google::parse_credential("oauth", &raw)?))
}

async fn token(state: &AppState, conn_id: &str) -> Result<String, String> {
    let (conn, cred) = cred_of(state, conn_id)?;
    google::access_token(&conn.id, &cred, &[SCOPE]).await.map_err(|e| {
        if e.contains("拒绝了凭据") {
            format!("Blogger 授权已失效：在「连接 → Blogger」里重新授权 {}", conn.login)
        } else {
            e
        }
    })
}

/// 这个账号下的博客（用户先在 blogger.com 建好）。
pub async fn blogs(state: &AppState, conn_id: &str) -> Result<Vec<Blog>, String> {
    let t = token(state, conn_id).await?;
    let v = google::api(&t, reqwest::Method::GET, &format!("{API}/users/self/blogs?fields=items(id,name,url)"), None).await?;
    Ok(v["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|b| Some(Blog { id: b["id"].as_str()?.to_string(), name: b["name"].as_str().unwrap_or("").to_string(), url: b["url"].as_str().unwrap_or("").to_string() }))
                .collect()
        })
        .unwrap_or_default())
}

/// 发一篇（`draft` 为真时存成草稿，用户去 Blogger 后台再发）。
pub async fn publish(state: &AppState, conn_id: &str, blog_id: &str, title: &str, html: &str, labels: &[String], draft: bool) -> Result<Posted, String> {
    if blog_id.is_empty() || !blog_id.chars().all(|c| c.is_ascii_digit()) {
        return Err("还没选博客".into());
    }
    let t = token(state, conn_id).await?;
    let url = format!("{API}/blogs/{blog_id}/posts?isDraft={draft}");
    let v = google::api(&t, reqwest::Method::POST, &url, Some(json!({ "kind": "blogger#post", "title": title, "content": html, "labels": labels }))).await?;
    Ok(Posted {
        id: v["id"].as_str().unwrap_or("").to_string(),
        url: v["url"].as_str().unwrap_or("").to_string(),
        status: v["status"].as_str().unwrap_or(if draft { "DRAFT" } else { "LIVE" }).to_string(),
    })
}

// ---------------------------------------------------------------------------------------------

/// 授权一个 Blogger 账号：新账号新建连接；`update_id` 非空时重新授权那一条（必须选同一个账号）。
#[tauri::command]
pub async fn blogger_login(app: tauri::AppHandle, state: tauri::State<'_, AppState>, update_id: String) -> Result<crate::conns::Connection, String> {
    use tauri_plugin_opener::OpenerExt;
    let (client_id, client_secret) = crate::data_cmds::gmail_client(&state).ok_or("还没有配置 Google OAuth 客户端：先在「连接 → Google」里配一次")?;
    let t = crate::google_oauth::authorize(&client_id, &client_secret, &format!("openid email {SCOPE}"), |url| {
        app.opener().open_url(url, None::<&str>).map_err(|e| format!("打开浏览器失败：{e}"))
    })
    .await?;
    if !t.granted.iter().any(|s| s == SCOPE) {
        return Err("授权时要勾上「管理你的 Blogger 账号」那一项，再试一次".into());
    }
    let email = google::api(&t.access, reqwest::Method::GET, "https://openidconnect.googleapis.com/v1/userinfo", None)
        .await
        .ok()
        .and_then(|u: Value| u["email"].as_str().map(str::to_string))
        .unwrap_or_default();
    let cred = OAuthCred { client_id: client_id.trim().into(), client_secret: client_secret.trim().into(), refresh_token: t.refresh, scopes: t.granted, email: email.clone() };
    let raw = serde_json::to_string(&cred).map_err(|e| e.to_string())?;
    let dedupe = format!("oauth:{email}");
    let target = if update_id.is_empty() {
        state.conns.list().into_iter().find(|c| c.kind == "blogger" && !email.is_empty() && c.key_prefix == dedupe).map(|c| c.id)
    } else {
        let cur = state.conns.get(&update_id)?;
        if !cur.login.is_empty() && !email.is_empty() && !cur.login.eq_ignore_ascii_case(&email) {
            return Err(format!("浏览器里选的是 {email}，不是 {}：要加新账号请用「连接 → Blogger → +」", cur.login));
        }
        Some(update_id)
    };
    match target {
        Some(id) => {
            google::forget_token(&id);
            state.conns.replace_data_secret(&id, &raw, "oauth", &email, &dedupe)
        }
        None => state.conns.add_data_source("blogger", &format!("Blogger · {}", if email.is_empty() { "账号" } else { &email }), &raw, "oauth", &email, "", &dedupe),
    }
}

#[tauri::command]
pub async fn blogger_blogs(state: tauri::State<'_, AppState>, conn_id: String) -> Result<Vec<Blog>, String> {
    blogs(&state, &conn_id).await
}
