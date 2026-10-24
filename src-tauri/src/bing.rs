//! Bing Webmaster（只读：查询词、页面、流量、抓取问题）与 IndexNow 提交。
//! API Key 只经过 Rust、只存钥匙串；模型只能读 Pilot 拉到本地的缓存。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://ssl.bing.com/webmaster/api.svc/json";
const INDEXNOW: &str = "https://api.indexnow.org/indexnow";

#[derive(Serialize, Clone, Debug, Default)]
pub struct BingSite {
    pub url: String,
    pub verified: bool,
    /// 文件 / meta 验证用的码（BingSiteAuth.xml 里的 `<user>`、`msvalidate.01`）
    #[serde(skip)]
    pub auth_code: String,
    /// DNS 验证：`<这个码>.<域名>` CNAME → verify.bing.com
    #[serde(skip)]
    pub dns_code: String,
}

/// `/Date(1700000000000-0800)/` → 毫秒时间戳。
pub fn parse_ms_date(s: &str) -> Option<i64> {
    let inner = s.strip_prefix("/Date(")?.strip_suffix(")/")?;
    let end = inner[1..].find(['+', '-']).map(|i| i + 1).unwrap_or(inner.len());
    inner[..end].parse().ok()
}

/// 毫秒时间戳 → YYYY-MM-DD（UTC）。
pub fn ms_to_day(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

fn redact(text: &str, key: &str) -> String {
    if key.is_empty() { text.into() } else { text.replace(key, "[已隐藏]") }
}

async fn call(key: &str, method: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let mut url = reqwest::Url::parse(&format!("{API}/{method}")).map_err(|e| e.to_string())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("apikey", key);
        for (k, v) in query {
            q.append_pair(k, v);
        }
    }
    let resp = reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| redact(&format!("请求 Bing 失败：{e}"), key))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.as_u16() == 401 || text.contains("InvalidApiKey") || text.contains("NotAuthorized") {
        return Err("Bing API Key 无效，或没有这个站点的权限".into());
    }
    if !status.is_success() {
        let msg: String = redact(&text, key).chars().take(200).collect();
        return Err(format!("Bing 返回 {status}：{msg}"));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| "Bing 响应无法解析".to_string())?;
    Ok(v.get("d").cloned().unwrap_or(Value::Null))
}

async fn post(key: &str, method: &str, body: Value) -> Result<Value, String> {
    let key = key.trim();
    let mut url = reqwest::Url::parse(&format!("{API}/{method}")).map_err(|e| e.to_string())?;
    url.query_pairs_mut().append_pair("apikey", key);
    let resp = reqwest::Client::new()
        .post(url)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(body.to_string())
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| redact(&format!("请求 Bing 失败：{e}"), key))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.as_u16() == 401 || text.contains("InvalidApiKey") || text.contains("NotAuthorized") {
        return Err("Bing API Key 无效，或没有这个站点的权限".into());
    }
    if !status.is_success() {
        let msg: String = redact(&text, key).chars().take(200).collect();
        return Err(format!("Bing 返回 {status}：{msg}"));
    }
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok(v.get("d").cloned().unwrap_or(Value::Null))
}

/// 把站点加到 Bing Webmaster（加了还要验证才算数）。
pub async fn add_site(key: &str, site_url: &str) -> Result<(), String> {
    post(key, "AddSite", serde_json::json!({ "siteUrl": site_url })).await.map(|_| ())
}

/// 让 Bing 去验证（DNS / BingSiteAuth.xml / meta 任一通过即可），返回是否通过。
pub async fn verify_site(key: &str, site_url: &str) -> Result<bool, String> {
    Ok(post(key, "VerifySite", serde_json::json!({ "siteUrl": site_url })).await?.as_bool().unwrap_or(false))
}

pub async fn list_sites(key: &str) -> Result<Vec<BingSite>, String> {
    let d = call(key.trim(), "GetUserSites", &[]).await?;
    Ok(d.as_array()
        .map(|a| {
            a.iter()
                .map(|s| BingSite {
                    url: s["Url"].as_str().unwrap_or_default().to_string(),
                    verified: s["IsVerified"].as_bool().unwrap_or(false),
                    auth_code: s["AuthenticationCode"].as_str().unwrap_or_default().to_string(),
                    dns_code: s["DnsVerificationCode"].as_str().unwrap_or_default().to_string(),
                })
                .filter(|s| !s.url.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct BingRow {
    /// 查询词或页面网址
    pub key: String,
    pub day: String,
    pub clicks: f64,
    pub impressions: f64,
    pub position: f64,
}

fn rows(d: &Value, key_field: &str) -> Vec<BingRow> {
    d.as_array()
        .map(|a| {
            a.iter()
                .map(|r| BingRow {
                    key: r[key_field].as_str().unwrap_or_default().to_string(),
                    day: r["Date"].as_str().and_then(parse_ms_date).map(ms_to_day).unwrap_or_default(),
                    clicks: r["Clicks"].as_f64().unwrap_or(0.0),
                    impressions: r["Impressions"].as_f64().unwrap_or(0.0),
                    position: r["AvgImpressionPosition"].as_f64().unwrap_or(0.0),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub async fn query_stats(key: &str, site: &str) -> Result<Vec<BingRow>, String> {
    Ok(rows(&call(key, "GetQueryStats", &[("siteUrl", site)]).await?, "Query"))
}

pub async fn page_stats(key: &str, site: &str) -> Result<Vec<BingRow>, String> {
    Ok(rows(&call(key, "GetPageStats", &[("siteUrl", site)]).await?, "Query"))
}

pub async fn traffic_stats(key: &str, site: &str) -> Result<Vec<BingRow>, String> {
    Ok(rows(&call(key, "GetRankAndTrafficStats", &[("siteUrl", site)]).await?, "Date"))
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CrawlIssue {
    pub url: String,
    pub http_code: i64,
    pub issues: i64,
}

pub async fn crawl_issues(key: &str, site: &str) -> Result<Vec<CrawlIssue>, String> {
    let d = call(key, "GetCrawlIssues", &[("siteUrl", site)]).await?;
    Ok(d.as_array()
        .map(|a| {
            a.iter()
                .map(|r| CrawlIssue {
                    url: r["Url"].as_str().unwrap_or_default().to_string(),
                    http_code: r["HttpCode"].as_i64().unwrap_or(0),
                    issues: r["Issues"].as_i64().unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// 向 IndexNow 提交网址（Bing、Yandex 等共享）。只提交本站域名下的网址，每批最多 10000 条。
pub async fn indexnow_submit(domain: &str, key: &str, urls: &[String]) -> Result<String, String> {
    if !crate::sites::is_indexnow_key(key) {
        return Err("IndexNow 密钥格式不对".into());
    }
    let prefix = format!("https://{domain}/");
    let urls: Vec<&String> = urls.iter().filter(|u| u.starts_with(&prefix) || **u == format!("https://{domain}")).collect();
    if urls.is_empty() {
        return Ok("没有本站网址需要提交".into());
    }
    let client = reqwest::Client::new();
    for chunk in urls.chunks(10_000) {
        let body = serde_json::json!({
            "host": domain,
            "key": key,
            "keyLocation": format!("https://{domain}/{key}.txt"),
            "urlList": chunk,
        });
        let resp = client
            .post(INDEXNOW)
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("请求 IndexNow 失败：{e}"))?;
        match resp.status().as_u16() {
            200 | 202 => {}
            400 => return Err("IndexNow：请求格式错误".into()),
            403 => return Err(format!("IndexNow：密钥无效（确认 https://{domain}/{key}.txt 能访问且内容就是密钥）")),
            422 => return Err("IndexNow：网址与域名或密钥不匹配".into()),
            429 => return Err("IndexNow：提交太频繁，稍后再试".into()),
            c => return Err(format!("IndexNow 返回 {c}")),
        }
    }
    Ok(format!("已接收 {} 个网址", urls.len()))
}

/// 生成一枚 IndexNow 密钥（32 位十六进制）。
pub fn new_indexnow_key() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ms_dates_parse_with_and_without_offsets() {
        assert_eq!(parse_ms_date("/Date(1700000000000-0800)/"), Some(1_700_000_000_000));
        assert_eq!(parse_ms_date("/Date(1700000000000+0000)/"), Some(1_700_000_000_000));
        assert_eq!(parse_ms_date("/Date(1700000000000)/"), Some(1_700_000_000_000));
        assert_eq!(parse_ms_date("garbage"), None);
        assert_eq!(ms_to_day(1_700_000_000_000), "2023-11-14");
    }

    #[test]
    fn query_rows_parse() {
        let d = serde_json::json!([
            {"Query":"gate 邀请码","Date":"/Date(1700000000000-0800)/","Clicks":3,"Impressions":120,"AvgImpressionPosition":6.5}
        ]);
        let r = rows(&d, "Query");
        assert_eq!(r[0].key, "gate 邀请码");
        assert_eq!(r[0].day, "2023-11-14");
        assert_eq!(r[0].impressions, 120.0);
        assert_eq!(r[0].position, 6.5);
    }

    #[test]
    fn generated_keys_are_valid() {
        let k = new_indexnow_key();
        assert_eq!(k.len(), 32);
        assert!(crate::sites::is_indexnow_key(&k));
    }
}
