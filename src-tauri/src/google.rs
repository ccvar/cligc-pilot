//! Google 数据源：GA4（Data API + Admin API 只读）与 Search Console（只读）。
//! 两种凭据：服务账号 JSON 密钥（自签 JWT 换访问令牌），或 OAuth 刷新令牌（桌面应用登录，见 google_oauth）。
//! 凭据只存钥匙串、只经过 Rust；访问令牌只缓存在内存里。模型永远拿不到它们。

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const SCOPE_ANALYTICS_RO: &str = "https://www.googleapis.com/auth/analytics.readonly";
pub const SCOPE_WEBMASTERS_RO: &str = "https://www.googleapis.com/auth/webmasters.readonly";
/// 新站闭环（可选授权）：建 GA4 媒体资源、添加 Search Console 资源、DNS 验证所有权。
pub const SCOPE_ANALYTICS_EDIT: &str = "https://www.googleapis.com/auth/analytics.edit";
pub const SCOPE_WEBMASTERS: &str = "https://www.googleapis.com/auth/webmasters";
pub const SCOPE_SITE_VERIFICATION: &str = "https://www.googleapis.com/auth/siteverification";

const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

#[derive(Deserialize, Clone)]
pub struct ServiceAccount {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    #[serde(default)]
    pub token_uri: String,
}

/// OAuth 凭据（钥匙串里存的 JSON）。`scopes` 记录当初授权的范围，用来判断能不能做「新站闭环」。
#[derive(Deserialize, Serialize, Clone)]
pub struct OAuthCred {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub email: String,
}

pub enum Credential {
    ServiceAccount(ServiceAccount),
    OAuth(OAuthCred),
}

pub fn parse_service_account(raw: &str) -> Result<ServiceAccount, String> {
    let sa: ServiceAccount = serde_json::from_str(raw.trim()).map_err(|_| "这不是 Google 服务账号的 JSON 密钥文件".to_string())?;
    if sa.kind != "service_account" {
        return Err("JSON 的 type 不是 service_account；请下载服务账号的密钥（JSON）".into());
    }
    if !sa.client_email.contains('@') || !sa.private_key.contains("PRIVATE KEY") {
        return Err("密钥文件缺少 client_email 或 private_key".into());
    }
    pem_to_der(&sa.private_key)?;
    Ok(sa)
}

pub fn parse_credential(key_kind: &str, raw: &str) -> Result<Credential, String> {
    match key_kind {
        "oauth" => serde_json::from_str(raw).map(Credential::OAuth).map_err(|_| "OAuth 凭据损坏，请重新登录".into()),
        _ => parse_service_account(raw).map(Credential::ServiceAccount),
    }
}

fn pem_to_der(pem: &str) -> Result<Vec<u8>, String> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .flat_map(|l| l.trim().chars())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(body.replace("\\n", ""))
        .map_err(|_| "服务账号私钥无法解析".to_string())
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 服务账号自签 JWT（RS256）。
pub fn sign_jwt(sa: &ServiceAccount, scopes: &[&str], now: u64) -> Result<String, String> {
    let der = pem_to_der(&sa.private_key)?;
    let key = ring::signature::RsaKeyPair::from_pkcs8(&der).map_err(|_| "服务账号私钥不是有效的 RSA PKCS#8 密钥".to_string())?;
    let aud = if sa.token_uri.is_empty() { TOKEN_URI } else { sa.token_uri.as_str() };
    let header = b64url(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = b64url(
        json!({ "iss": sa.client_email, "scope": scopes.join(" "), "aud": aud, "iat": now, "exp": now + 3600 })
            .to_string()
            .as_bytes(),
    );
    let input = format!("{header}.{claims}");
    let mut sig = vec![0u8; key.public().modulus_len()];
    key.sign(&ring::signature::RSA_PKCS1_SHA256, &ring::rand::SystemRandom::new(), input.as_bytes(), &mut sig)
        .map_err(|_| "JWT 签名失败".to_string())?;
    Ok(format!("{input}.{}", b64url(&sig)))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache() -> &'static Mutex<HashMap<String, (String, u64)>> {
    static C: OnceLock<Mutex<HashMap<String, (String, u64)>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

pub fn forget_token(cache_key: &str) {
    cache().lock().unwrap().retain(|k, _| !k.starts_with(cache_key));
}

async fn token_request(form: &[(&str, &str)]) -> Result<Value, String> {
    let body = form.iter().map(|(k, v)| format!("{}={}", url_escape(k), url_escape(v))).collect::<Vec<_>>().join("&");
    let resp = reqwest::Client::new()
        .post(TOKEN_URI)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("请求 Google 令牌失败：{e}"))?;
    let status = resp.status();
    let v: Value = resp.json().await.map_err(|_| "Google 令牌响应无法解析".to_string())?;
    if !status.is_success() {
        let err = v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("未知错误");
        return Err(match v["error"].as_str() {
            Some("invalid_grant") => format!("Google 拒绝了凭据（{err}）：服务账号密钥可能已被删除，或 OAuth 授权已过期 / 被撤销，请重新连接"),
            _ => format!("Google 令牌请求失败：{err}"),
        });
    }
    Ok(v)
}

/// 取访问令牌（内存缓存，提前 2 分钟刷新）。`cache_key` 一般是连接 id。
pub async fn access_token(cache_key: &str, cred: &Credential, scopes: &[&str]) -> Result<String, String> {
    let key = format!("{cache_key}|{}", scopes.join(" "));
    if let Some((t, exp)) = cache().lock().unwrap().get(&key) {
        if *exp > now() + 120 {
            return Ok(t.clone());
        }
    }
    let v = match cred {
        Credential::ServiceAccount(sa) => {
            let jwt = sign_jwt(sa, scopes, now())?;
            token_request(&[("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"), ("assertion", &jwt)]).await?
        }
        Credential::OAuth(o) => {
            token_request(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &o.refresh_token),
                ("client_id", &o.client_id),
                ("client_secret", &o.client_secret),
            ])
            .await?
        }
    };
    let token = v["access_token"].as_str().ok_or("Google 没有返回访问令牌")?.to_string();
    let exp = now() + v["expires_in"].as_u64().unwrap_or(3600);
    cache().lock().unwrap().insert(key, (token.clone(), exp));
    Ok(token)
}

/// 连接存的凭据能申请到的读取范围。
pub fn read_scopes() -> [&'static str; 2] {
    [SCOPE_ANALYTICS_RO, SCOPE_WEBMASTERS_RO]
}

pub async fn api(token: &str, method: reqwest::Method, url: &str, body: Option<Value>) -> Result<Value, String> {
    let mut req = reqwest::Client::new()
        .request(method, url)
        .bearer_auth(token)
        .timeout(Duration::from_secs(45));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.map_err(|e| format!("请求 Google 失败：{e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let msg = v["error"]["message"].as_str().unwrap_or("").to_string();
        return Err(match status.as_u16() {
            401 => "Google 凭据无效或已过期".into(),
            403 if msg.contains("has not been used") || msg.contains("is disabled") => {
                format!("对应的 Google API 还没启用：{msg}")
            }
            403 => format!("没有权限：{}", if msg.is_empty() { "请确认已把服务账号邮箱加入该资源".into() } else { msg }),
            _ => format!("Google 返回 {status}：{}", if msg.is_empty() { text.chars().take(200).collect() } else { msg }),
        });
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|_| "Google 响应无法解析".to_string())
}

// ---------------------------------------------------------------------------
// 资源列表
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Ga4Property {
    /// properties/123
    pub property: String,
    pub name: String,
    pub account: String,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct GscSite {
    pub url: String,
    pub permission: String,
}

pub async fn list_ga4_properties(token: &str) -> Result<Vec<Ga4Property>, String> {
    let mut out = Vec::new();
    let mut page = String::new();
    for _ in 0..10 {
        let url = format!(
            "https://analyticsadmin.googleapis.com/v1beta/accountSummaries?pageSize=200{}",
            if page.is_empty() { String::new() } else { format!("&pageToken={page}") }
        );
        let v = api(token, reqwest::Method::GET, &url, None).await?;
        for acc in v["accountSummaries"].as_array().cloned().unwrap_or_default() {
            for p in acc["propertySummaries"].as_array().cloned().unwrap_or_default() {
                out.push(Ga4Property {
                    property: p["property"].as_str().unwrap_or_default().into(),
                    name: p["displayName"].as_str().unwrap_or_default().into(),
                    account: acc["displayName"].as_str().unwrap_or_default().into(),
                });
            }
        }
        match v["nextPageToken"].as_str() {
            Some(t) if !t.is_empty() => page = t.into(),
            _ => break,
        }
    }
    Ok(out)
}

pub async fn list_gsc_sites(token: &str) -> Result<Vec<GscSite>, String> {
    let v = api(token, reqwest::Method::GET, "https://www.googleapis.com/webmasters/v3/sites", None).await?;
    Ok(v["siteEntry"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| GscSite {
                    url: s["siteUrl"].as_str().unwrap_or_default().into(),
                    permission: s["permissionLevel"].as_str().unwrap_or_default().into(),
                })
                .filter(|s| s.permission != "siteUnverifiedUser")
                .collect()
        })
        .unwrap_or_default())
}

/// 按域名挑一个最匹配的 Search Console 资源：优先网域资源，其次 https 网址前缀（界面里的自动选择同此规则）。
pub fn match_gsc_site<'a>(sites: &'a [GscSite], domain: &str) -> Option<&'a GscSite> {
    let bare = domain.trim_start_matches("www.");
    sites
        .iter()
        .find(|s| s.url == format!("sc-domain:{bare}"))
        .or_else(|| sites.iter().find(|s| s.url == format!("https://{domain}/")))
        .or_else(|| sites.iter().find(|s| s.url == format!("https://www.{bare}/")))
}

/// 媒体资源下网页数据流的主机名（按域名自动匹配媒体资源用）。
pub async fn ga4_stream_hosts(token: &str, property: &str) -> Result<Vec<String>, String> {
    let url = format!("https://analyticsadmin.googleapis.com/v1beta/{property}/dataStreams?pageSize=50");
    let v = api(token, reqwest::Method::GET, &url, None).await?;
    Ok(v["dataStreams"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| d["webStreamData"]["defaultUri"].as_str())
                .filter_map(|u| crate::sites::normalize_domain(u).ok())
                .filter(|h| !h.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

/// 主机名是否指同一个站（忽略 www.）。
pub fn same_host(a: &str, b: &str) -> bool {
    a.trim_start_matches("www.") == b.trim_start_matches("www.")
}

// ---------------------------------------------------------------------------
// GA4 Data API
// ---------------------------------------------------------------------------

/// 一次 runReport：返回每行（维度值列表，指标值列表）。
pub async fn ga4_report(
    token: &str,
    property: &str,
    start: &str,
    end: &str,
    dims: &[&str],
    metrics: &[&str],
    limit: u32,
    filter: Option<Value>,
) -> Result<Vec<(Vec<String>, Vec<f64>)>, String> {
    let mut body = json!({
        "dateRanges": [{ "startDate": start, "endDate": end }],
        "dimensions": dims.iter().map(|d| json!({ "name": d })).collect::<Vec<_>>(),
        "metrics": metrics.iter().map(|m| json!({ "name": m })).collect::<Vec<_>>(),
        "limit": limit,
        "orderBys": [{ "metric": { "metricName": metrics[0] }, "desc": true }],
    });
    if dims.first() == Some(&"date") {
        body["orderBys"] = json!([{ "dimension": { "dimensionName": "date" } }]);
    }
    if let Some(f) = filter {
        body["dimensionFilter"] = f;
    }
    let url = format!("https://analyticsdata.googleapis.com/v1beta/{property}:runReport");
    let v = api(token, reqwest::Method::POST, &url, Some(body)).await?;
    Ok(v["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    let d = r["dimensionValues"]
                        .as_array()
                        .map(|a| a.iter().map(|x| x["value"].as_str().unwrap_or_default().to_string()).collect())
                        .unwrap_or_default();
                    let m = r["metricValues"]
                        .as_array()
                        .map(|a| a.iter().map(|x| x["value"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0)).collect())
                        .unwrap_or_default();
                    (d, m)
                })
                .collect()
        })
        .unwrap_or_default())
}

/// 媒体资源里注册过的事件级自定义维度（参数名），用来拆分站点自定义事件（如 exchange / placement）。
pub async fn ga4_event_dimensions(token: &str, property: &str) -> Result<Vec<String>, String> {
    let url = format!("https://analyticsadmin.googleapis.com/v1beta/{property}/customDimensions?pageSize=200");
    let v = api(token, reqwest::Method::GET, &url, None).await?;
    Ok(v["customDimensions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|d| d["scope"].as_str() == Some("EVENT"))
                .filter_map(|d| d["parameterName"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Search Console
// ---------------------------------------------------------------------------

pub fn encode_site(site: &str) -> String {
    url_escape(site)
}

fn url_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SearchRow {
    pub keys: Vec<String>,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

pub async fn gsc_query(token: &str, site: &str, start: &str, end: &str, dims: &[&str], limit: u32) -> Result<Vec<SearchRow>, String> {
    let url = format!("https://www.googleapis.com/webmasters/v3/sites/{}/searchAnalytics/query", encode_site(site));
    let v = api(
        token,
        reqwest::Method::POST,
        &url,
        Some(json!({ "startDate": start, "endDate": end, "dimensions": dims, "rowLimit": limit, "dataState": "all" })),
    )
    .await?;
    Ok(v["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| SearchRow {
                    keys: r["keys"].as_array().map(|k| k.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                    clicks: r["clicks"].as_f64().unwrap_or(0.0),
                    impressions: r["impressions"].as_f64().unwrap_or(0.0),
                    ctr: r["ctr"].as_f64().unwrap_or(0.0),
                    position: r["position"].as_f64().unwrap_or(0.0),
                })
                .collect()
        })
        .unwrap_or_default())
}

pub async fn gsc_sitemaps(token: &str, site: &str) -> Result<Value, String> {
    let url = format!("https://www.googleapis.com/webmasters/v3/sites/{}/sitemaps", encode_site(site));
    let v = api(token, reqwest::Method::GET, &url, None).await?;
    Ok(v["sitemap"].clone())
}

/// 提交 sitemap（需要 webmasters 写权限：OAuth 管理授权）。
pub async fn gsc_submit_sitemap(token: &str, site: &str, feed: &str) -> Result<(), String> {
    let url = format!(
        "https://www.googleapis.com/webmasters/v3/sites/{}/sitemaps/{}",
        encode_site(site),
        encode_site(feed)
    );
    api(token, reqwest::Method::PUT, &url, None).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试时现场生成一把 RSA 私钥（不在仓库里放任何私钥文件）；本机没有 openssl 就跳过。
    fn test_key() -> Option<String> {
        let out = std::process::Command::new("openssl")
            .args(["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048"])
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn sa() -> Option<ServiceAccount> {
        let key = test_key()?;
        Some(
            parse_service_account(
                &json!({
                    "type": "service_account",
                    "project_id": "p",
                    "client_email": "bot@p.iam.gserviceaccount.com",
                    "private_key": key,
                })
                .to_string(),
            )
            .unwrap(),
        )
    }

    #[test]
    fn service_account_files_are_validated() {
        assert!(parse_service_account("{}").is_err());
        assert!(parse_service_account(r#"{"type":"authorized_user","client_email":"a@b","private_key":"x"}"#).is_err());
        if let Some(s) = sa() {
            assert_eq!(s.client_email, "bot@p.iam.gserviceaccount.com");
        }
    }

    #[test]
    fn jwt_is_signed_and_verifiable() {
        let Some(s) = sa() else { return };
        let jwt = sign_jwt(&s, &read_scopes(), 1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let claims: Value = serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["iss"], "bot@p.iam.gserviceaccount.com");
        assert_eq!(claims["aud"], TOKEN_URI);
        assert_eq!(claims["exp"], 1_700_003_600u64);
        assert!(claims["scope"].as_str().unwrap().contains("analytics.readonly"));
        // 用公钥验签
        let der = pem_to_der(&s.private_key).unwrap();
        let kp = ring::signature::RsaKeyPair::from_pkcs8(&der).unwrap();
        use ring::signature::KeyPair;
        let pk = ring::signature::UnparsedPublicKey::new(&ring::signature::RSA_PKCS1_2048_8192_SHA256, kp.public_key().as_ref());
        let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        pk.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig).unwrap();
    }

    #[test]
    fn gsc_sites_match_by_domain() {
        let sites = vec![
            GscSite { url: "https://other.com/".into(), permission: "siteOwner".into() },
            GscSite { url: "https://gatesor.com/".into(), permission: "siteFullUser".into() },
            GscSite { url: "sc-domain:gatesor.com".into(), permission: "siteOwner".into() },
        ];
        assert_eq!(match_gsc_site(&sites, "gatesor.com").unwrap().url, "sc-domain:gatesor.com");
        assert_eq!(match_gsc_site(&sites[..2], "gatesor.com").unwrap().url, "https://gatesor.com/");
        assert!(match_gsc_site(&sites, "nope.com").is_none());
        assert_eq!(encode_site("sc-domain:gatesor.com"), "sc-domain%3Agatesor.com");
        assert_eq!(encode_site("https://a.com/"), "https%3A%2F%2Fa.com%2F");
    }
}
