//! Google OAuth（桌面应用 + 本机回环地址 + PKCE）：用户自己在 Google Cloud 建的 OAuth 客户端，
//! 在系统浏览器里授权，Pilot 在 127.0.0.1 的临时端口上接回调、换刷新令牌，凭据只进钥匙串。

use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::google::{self, OAuthCred};

static CANCEL: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn b64url(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn random_token(n: usize) -> String {
    let mut s = String::new();
    while s.len() < n {
        s.push_str(&uuid::Uuid::new_v4().simple().to_string());
    }
    s.truncate(n);
    s
}

pub fn pkce_challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

fn esc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn unesc(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 从回调请求行（`GET /?code=..&state=.. HTTP/1.1`）里取查询参数。
pub fn parse_callback(request_line: &str) -> Vec<(String, String)> {
    let path = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    query
        .split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((unesc(k), unesc(v)))
        })
        .collect()
}

pub fn scopes(manage: bool) -> Vec<&'static str> {
    let mut s = vec!["openid", "email", google::SCOPE_ANALYTICS_RO, google::SCOPE_WEBMASTERS_RO];
    if manage {
        s.extend([google::SCOPE_ANALYTICS_EDIT, google::SCOPE_WEBMASTERS, google::SCOPE_SITE_VERIFICATION]);
    }
    s
}

const DONE_HTML: &str = "<!doctype html><meta charset=utf-8><title>Cligc Pilot</title><body style=\"font:15px -apple-system,sans-serif;padding:48px;color:#26241f\"><h2>授权完成</h2><p>可以关掉这个页面，回到 Cligc Pilot。</p></body>";
const FAIL_HTML: &str = "<!doctype html><meta charset=utf-8><title>Cligc Pilot</title><body style=\"font:15px -apple-system,sans-serif;padding:48px;color:#26241f\"><h2>没有完成授权</h2><p>回到 Cligc Pilot 查看原因。</p></body>";

async fn respond(stream: &mut tokio::net::TcpStream, html: &str) {
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// 浏览器授权拿到的令牌。
pub struct Tokens {
    pub refresh: String,
    pub access: String,
    /// Google 实际给了的权限（用户可以在授权页取消勾选某几项）
    pub granted: Vec<String>,
}

/// 网站数据（GA4 / Search Console）的登录：授权 → 检查勾了读取权限 → 取邮箱。
pub async fn login(client_id: &str, client_secret: &str, manage: bool, open: impl Fn(&str) -> Result<(), String>) -> Result<OAuthCred, String> {
    let (client_id, client_secret) = (client_id.trim(), client_secret.trim());
    let t = authorize(client_id, client_secret, &scopes(manage).join(" "), open).await?;
    for need in [google::SCOPE_ANALYTICS_RO, google::SCOPE_WEBMASTERS_RO] {
        if !t.granted.iter().any(|g| g == need) {
            return Err("授权时没有勾选 GA4 / Search Console 的读取权限，请重新登录并勾选".into());
        }
    }
    let email = google::api(&t.access, reqwest::Method::GET, "https://openidconnect.googleapis.com/v1/userinfo", None)
        .await
        .ok()
        .and_then(|u| u["email"].as_str().map(str::to_string))
        .unwrap_or_default();
    Ok(OAuthCred { client_id: client_id.into(), client_secret: client_secret.into(), refresh_token: t.refresh, scopes: t.granted, email })
}

/// 浏览器授权的完整流程（网站数据和 Gmail 共用）：开本机端口 → 打开浏览器 → 等回调 → 换令牌。
/// `open` 负责打开浏览器；同一时间只等一个授权，`cancel()` 结束它。
pub async fn authorize(client_id: &str, client_secret: &str, scope: &str, open: impl Fn(&str) -> Result<(), String>) -> Result<Tokens, String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("已经有一个登录在等待浏览器授权".into());
    }
    CANCEL.store(false, Ordering::SeqCst);
    let r = authorize_inner(client_id.trim(), client_secret.trim(), scope, open).await;
    BUSY.store(false, Ordering::SeqCst);
    r
}

async fn authorize_inner(client_id: &str, client_secret: &str, scope: &str, open: impl Fn(&str) -> Result<(), String>) -> Result<Tokens, String> {
    if !client_id.ends_with(".apps.googleusercontent.com") {
        return Err("客户端 ID 应以 .apps.googleusercontent.com 结尾（在 Google Cloud 的凭据页复制）".into());
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("开本机回调端口失败：{e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = random_token(64);
    let state = random_token(24);
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        esc(client_id),
        esc(&redirect),
        esc(scope),
        pkce_challenge(&verifier),
        state
    );
    open(&url)?;
    let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
    let code = loop {
        if CANCEL.load(Ordering::SeqCst) {
            return Err("已取消登录".into());
        }
        if tokio::time::Instant::now() > deadline {
            return Err("5 分钟内没有完成浏览器授权，已取消".into());
        }
        let accepted = tokio::time::timeout(Duration::from_millis(800), listener.accept()).await;
        let Ok(Ok((mut stream, _))) = accepted else { continue };
        let mut buf = vec![0u8; 8192];
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await.ok().and_then(|r| r.ok()).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let line = req.lines().next().unwrap_or("");
        let params = parse_callback(line);
        let get = |k: &str| params.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.clone());
        if let Some(err) = get("error") {
            respond(&mut stream, FAIL_HTML).await;
            return Err(if err == "access_denied" { "你在浏览器里拒绝了授权".into() } else { format!("Google 返回错误：{err}") });
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
                // favicon 等其它请求
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            }
        }
    };
    // 换令牌
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("redirect_uri", redirect.as_str()),
        ("code_verifier", verifier.as_str()),
    ];
    let body = form.iter().map(|(k, v)| format!("{}={}", esc(k), esc(v))).collect::<Vec<_>>().join("&");
    let resp = reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("换取令牌失败：{e}"))?;
    let v: serde_json::Value = resp.json().await.map_err(|_| "Google 令牌响应无法解析".to_string())?;
    if let Some(e) = v["error"].as_str() {
        return Err(format!("换取令牌失败：{}", v["error_description"].as_str().unwrap_or(e)));
    }
    let refresh = v["refresh_token"].as_str().ok_or("Google 没有返回刷新令牌（请在授权页重新同意全部权限）")?.to_string();
    let access = v["access_token"].as_str().unwrap_or_default().to_string();
    let granted: Vec<String> = v["scope"].as_str().unwrap_or(scope).split_whitespace().map(str::to_string).collect();
    Ok(Tokens { refresh, access, granted })
}

pub fn can_manage(cred: &OAuthCred) -> bool {
    [google::SCOPE_ANALYTICS_EDIT, google::SCOPE_WEBMASTERS, google::SCOPE_SITE_VERIFICATION]
        .iter()
        .all(|s| cred.scopes.iter().any(|g| g == s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_the_rfc_example() {
        // RFC 7636 附录 B
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn callback_query_is_parsed() {
        let p = parse_callback("GET /?state=abc&code=4%2F0AX-y&scope=a+b HTTP/1.1");
        assert_eq!(p, vec![("state".into(), "abc".into()), ("code".into(), "4/0AX-y".into()), ("scope".into(), "a b".into())]);
        assert!(parse_callback("GET /favicon.ico HTTP/1.1").is_empty());
    }

    #[test]
    fn manage_scopes_are_opt_in() {
        assert!(!scopes(false).contains(&google::SCOPE_ANALYTICS_EDIT));
        assert!(scopes(true).contains(&google::SCOPE_SITE_VERIFICATION));
    }
}
