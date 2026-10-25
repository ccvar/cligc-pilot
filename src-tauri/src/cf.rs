//! Cloudflare API 轻客户端：验证 API Token、拉取账号/域名、只读核验 Zone/DNS/SSL，
//! 以及在用户明确操作后幂等创建一条仅 DNS 的 A 记录；源站 HTTPS 验证通过后，
//! 可在记录仍指向已核验源站且 Zone 使用 Full / Full (strict) 时安全开启橙云。
//! token 只在验证/连接时经过 Rust，存进钥匙串后运行时以 `CLOUDFLARE_API_TOKEN` 注入 wrangler，
//! 绝不落盘、绝不进 WebView。

use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://api.cloudflare.com/client/v4";

#[derive(Serialize, Default, Clone)]
pub struct CfAccount {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Default, Clone)]
pub struct CfZone {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Default)]
pub struct CfVerify {
    /// 期望 "active"。
    pub token_status: String,
    pub accounts: Vec<CfAccount>,
    pub zones: Vec<CfZone>,
}

async fn cf_get_query(token: &str, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let mut url = reqwest::Url::parse(&format!("{API}{path}"))
        .map_err(|error| format!("Cloudflare API 地址无效: {error}"))?;
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query {
            pairs.append_pair(key, value);
        }
    }
    let client = reqwest::Client::new();
    let request = client
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(30));
    let resp = request
        .send()
        .await
        .map_err(|e| format!("请求 Cloudflare 失败: {e}"))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Cloudflare 响应不是 JSON: {e}"))?;
    if !status.is_success()
        || !body
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        let msg = body
            .get("errors")
            .and_then(|e| e.as_array())
            .and_then(|a| a.first())
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("未知错误（请确认 Token 权限）");
        return Err(format!("Cloudflare {status}: {msg}"));
    }
    Ok(body)
}

async fn cf_get(token: &str, path: &str) -> Result<Value, String> {
    cf_get_query(token, path, &[]).await
}

fn parse_id_name(body: &Value) -> Vec<(String, String)> {
    body.get("result")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|x| {
                    (
                        x.get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        x.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 验证 token 并拉取可用账号 + 域名（zone）。域名拿不到（无 Zone 权限）时静默留空。
pub async fn verify_token(token: &str) -> Result<CfVerify, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("请粘贴 Cloudflare API Token".into());
    }
    // 1) 验证 token 本身
    let v = cf_get(token, "/user/tokens/verify").await?;
    let token_status = v
        .get("result")
        .and_then(|r| r.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if token_status != "active" {
        return Err(format!("Token 状态异常：{token_status}（需要 active）"));
    }
    // 2) 账号（部署 Pages/Workers/D1 都要账号）
    let accounts = parse_id_name(&cf_get(token, "/accounts?per_page=50").await?)
        .into_iter()
        .map(|(id, name)| CfAccount { id, name })
        .collect();
    // 3) 域名（可选权限，失败/空都容忍——先连上，绑定自定义域名是后面的事）
    let zones = match cf_get(token, "/zones?per_page=50").await {
        Ok(z) => parse_id_name(&z)
            .into_iter()
            .map(|(id, name)| CfZone { id, name })
            .collect(),
        Err(_) => Vec::new(),
    };
    Ok(CfVerify {
        token_status,
        accounts,
        zones,
    })
}


async fn cf_send(token: &str, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    let url = format!("{API}{path}");
    let client = reqwest::Client::new();
    let mut req = client
        .request(method, url)
        .header("Authorization", format!("Bearer {token}"))
        .timeout(Duration::from_secs(30));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.map_err(|e| format!("请求 Cloudflare 失败: {e}"))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Cloudflare 响应不是 JSON: {e}"))?;
    if !status.is_success() || !body.get("success").and_then(Value::as_bool).unwrap_or(false) {
        let msg = body
            .get("errors")
            .and_then(|e| e.as_array())
            .and_then(|a| a.first())
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("未知错误（请确认 Token 权限）");
        return Err(format!("Cloudflare {status}: {msg}"));
    }
    Ok(body)
}

/// 写 Pages 项目的环境变量（生产和预览都写）：只动给出的键，`remove` 里的删掉，其它变量不碰。
/// `vars`：(名, 值, 是否密钥)。
pub async fn set_pages_env(token: &str, account_id: &str, project: &str, vars: &[(String, String, bool)], remove: &[String]) -> Result<(), String> {
    let mut env = serde_json::Map::new();
    for (k, v, secret) in vars {
        env.insert(k.clone(), serde_json::json!({"type": if *secret { "secret_text" } else { "plain_text" }, "value": v}));
    }
    for k in remove {
        env.insert(k.clone(), Value::Null);
    }
    if env.is_empty() {
        return Ok(());
    }
    let body = serde_json::json!({"deployment_configs": {"production": {"env_vars": env.clone()}, "preview": {"env_vars": env}}});
    cf_send(token, reqwest::Method::PATCH, &format!("/accounts/{account_id}/pages/projects/{project}"), Some(body)).await.map(|_| ())
}

/// 写一个 Worker 密钥（只动这一个，不影响 Worker 的其它绑定）。Worker 还没部署过会失败。
pub async fn put_worker_secret(token: &str, account_id: &str, script: &str, name: &str, value: &str) -> Result<(), String> {
    let body = serde_json::json!({"name": name, "text": value, "type": "secret_text"});
    cf_send(token, reqwest::Method::PUT, &format!("/accounts/{account_id}/workers/scripts/{script}/secrets"), Some(body)).await.map(|_| ())
}

pub async fn delete_worker_secret(token: &str, account_id: &str, script: &str, name: &str) -> Result<(), String> {
    match cf_send(token, reqwest::Method::DELETE, &format!("/accounts/{account_id}/workers/scripts/{script}/secrets/{name}"), None).await {
        Err(e) if e.contains("404") || e.to_lowercase().contains("not found") => Ok(()),
        r => r.map(|_| ()),
    }
}

/// Pages 项目（站点导入 / 部署用到的字段）。
#[derive(Serialize, Default, Clone, Debug)]
pub struct PagesProject {
    pub name: String,
    pub subdomain: String,
    pub domains: Vec<String>,
    pub production_branch: String,
    /// github | gitlab | ""（直接上传）
    pub source_type: String,
    /// 绑定的仓库（owner/repo），直接上传为空。
    pub source_repo: String,
}

fn pages_project_from(v: &Value) -> PagesProject {
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let source = &v["source"];
    let cfg = &source["config"];
    let repo = match (cfg["owner"].as_str(), cfg["repo_name"].as_str()) {
        (Some(o), Some(r)) if !o.is_empty() && !r.is_empty() => format!("{o}/{r}"),
        _ => String::new(),
    };
    PagesProject {
        name: s(&v["name"]),
        subdomain: s(&v["subdomain"]),
        domains: v["domains"]
            .as_array()
            .map(|a| a.iter().filter_map(|d| d.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        production_branch: s(&v["production_branch"]),
        source_type: s(&source["type"]),
        source_repo: repo,
    }
}

pub async fn list_pages_projects(token: &str, account_id: &str) -> Result<Vec<PagesProject>, String> {
    // Pages 的列表接口不接受较大的 per_page（实测 50 会被拒），按默认页大小翻页。
    let mut out = Vec::new();
    for page in 1..=20 {
        let body = cf_get(token, &format!("/accounts/{account_id}/pages/projects?page={page}")).await?;
        let items = body["result"].as_array().cloned().unwrap_or_default();
        out.extend(items.iter().map(pages_project_from));
        let total = body["result_info"]["total_pages"].as_u64().unwrap_or(1);
        if items.is_empty() || page as u64 >= total {
            break;
        }
    }
    Ok(out)
}

pub async fn get_pages_project(token: &str, account_id: &str, name: &str) -> Result<Option<PagesProject>, String> {
    match cf_get(token, &format!("/accounts/{account_id}/pages/projects/{name}")).await {
        Ok(b) => Ok(Some(pages_project_from(&b["result"]))),
        Err(e) if e.contains("404") || e.to_lowercase().contains("not found") => Ok(None),
        Err(e) => Err(e),
    }
}

/// 建一个直接上传型的 Pages 项目（wrangler 在非交互环境里不会替你建）。
pub async fn create_pages_project(token: &str, account_id: &str, name: &str, branch: &str) -> Result<PagesProject, String> {
    let body = cf_send(
        token,
        reqwest::Method::POST,
        &format!("/accounts/{account_id}/pages/projects"),
        Some(serde_json::json!({ "name": name, "production_branch": branch })),
    )
    .await?;
    Ok(pages_project_from(&body["result"]))
}

#[derive(Serialize, Default, Clone, Debug)]
pub struct PagesDeployment {
    pub id: String,
    pub url: String,
    pub environment: String,
    pub branch: String,
    pub commit: String,
    pub message: String,
    /// latest_stage.status：success | failure | active | idle | canceled
    pub status: String,
    pub stage: String,
    pub created_on: String,
}

fn deployment_from(v: &Value) -> PagesDeployment {
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let meta = &v["deployment_trigger"]["metadata"];
    PagesDeployment {
        id: s(&v["id"]),
        url: s(&v["url"]),
        environment: s(&v["environment"]),
        branch: s(&meta["branch"]),
        commit: s(&meta["commit_hash"]),
        message: s(&meta["commit_message"]),
        status: s(&v["latest_stage"]["status"]),
        stage: s(&v["latest_stage"]["name"]),
        created_on: s(&v["created_on"]),
    }
}

pub async fn list_deployments(token: &str, account_id: &str, project: &str) -> Result<Vec<PagesDeployment>, String> {
    let body = cf_get(token, &format!("/accounts/{account_id}/pages/projects/{project}/deployments")).await?;
    Ok(body["result"].as_array().map(|a| a.iter().map(deployment_from).collect()).unwrap_or_default())
}

/// Workers 的部署记录（每次 `wrangler deploy` 一条），映射成和 Pages 部署记录同样的结构给界面用。
pub async fn list_worker_deployments(token: &str, account_id: &str, script: &str) -> Result<Vec<PagesDeployment>, String> {
    let body = match cf_get(token, &format!("/accounts/{account_id}/workers/scripts/{script}/deployments")).await {
        Ok(b) => b,
        Err(e) if e.contains("404") || e.to_lowercase().contains("not found") => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let list = body["result"]["deployments"].as_array().cloned().unwrap_or_default();
    Ok(list
        .iter()
        .take(10)
        .map(|d| PagesDeployment {
            id: d["id"].as_str().unwrap_or_default().into(),
            url: String::new(),
            environment: "production".into(),
            branch: String::new(),
            commit: String::new(),
            message: d["annotations"]["workers/message"].as_str().unwrap_or(d["source"].as_str().unwrap_or_default()).into(),
            status: "success".into(),
            stage: "deploy".into(),
            created_on: d["created_on"].as_str().unwrap_or_default().into(),
        })
        .collect())
}

/// 重新跑某次部署（Pages 绑定 GitHub 时＝按该提交重新构建）。
pub async fn retry_deployment(token: &str, account_id: &str, project: &str, id: &str) -> Result<PagesDeployment, String> {
    let body = cf_send(
        token,
        reqwest::Method::POST,
        &format!("/accounts/{account_id}/pages/projects/{project}/deployments/{id}/retry"),
        None,
    )
    .await?;
    Ok(deployment_from(&body["result"]))
}

/// 把生产环境回滚到某次历史部署（不改代码，立即生效）。
pub async fn rollback_deployment(token: &str, account_id: &str, project: &str, id: &str) -> Result<PagesDeployment, String> {
    let body = cf_send(
        token,
        reqwest::Method::POST,
        &format!("/accounts/{account_id}/pages/projects/{project}/deployments/{id}/rollback"),
        None,
    )
    .await?;
    Ok(deployment_from(&body["result"]))
}

/// 给 Pages 项目加自定义域名。已存在视为成功。
/// 项目里某个自定义域名的状态（active / pending / …）；没加过就是 None。
/// 项目详情里的 domains 只列生效了的，刚加、等验证的要从这里看。
pub async fn pages_domain_status(token: &str, account_id: &str, project: &str, domain: &str) -> Result<Option<String>, String> {
    let body = cf_get(token, &format!("/accounts/{account_id}/pages/projects/{project}/domains")).await?;
    Ok(body["result"].as_array().into_iter().flatten().find(|d| d["name"].as_str().map(|n| n.eq_ignore_ascii_case(domain)).unwrap_or(false)).map(|d| d["status"].as_str().unwrap_or("pending").to_string()))
}

pub async fn add_pages_domain(token: &str, account_id: &str, project: &str, domain: &str) -> Result<(), String> {
    match cf_send(
        token,
        reqwest::Method::POST,
        &format!("/accounts/{account_id}/pages/projects/{project}/domains"),
        Some(serde_json::json!({ "name": domain })),
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(e) if e.contains("already") => Ok(()),
        Err(e) => Err(e),
    }
}

/// 找到域名所属的 Zone（按最长后缀匹配）。
pub async fn zone_for(token: &str, domain: &str) -> Result<Option<CfZone>, String> {
    // 按名字直接查（原来列前 50 个 Zone 再找，域名多的账号会漏掉）：先查域名本身，再逐级往上（www.a.com → a.com）
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = domain.split('.').collect();
    for i in 0..labels.len().saturating_sub(1) {
        let cand = labels[i..].join(".");
        let body = cf_get_query(token, "/zones", &[("name", cand.as_str())]).await?;
        if let Some((id, name)) = parse_id_name(&body).into_iter().find(|(_, n)| n.eq_ignore_ascii_case(&cand)) {
            return Ok(Some(CfZone { id, name }));
        }
    }
    Ok(None)
}

/// 同名的 A / AAAA / CNAME 记录（判断域名现在指向哪、能不能加 CNAME）。
#[derive(Serialize, Clone, Debug)]
pub struct DnsRec {
    pub id: String,
    pub kind: String,
    pub content: String,
    pub proxied: bool,
}

pub async fn dns_records(token: &str, zone_id: &str, name: &str) -> Result<Vec<DnsRec>, String> {
    let body = cf_get_query(token, &format!("/zones/{zone_id}/dns_records"), &[("name", name)]).await?;
    Ok(body["result"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| matches!(r["type"].as_str(), Some("A" | "AAAA" | "CNAME")))
        .map(|r| DnsRec {
            id: r["id"].as_str().unwrap_or_default().into(),
            kind: r["type"].as_str().unwrap_or_default().into(),
            content: r["content"].as_str().unwrap_or_default().into(),
            proxied: r["proxied"].as_bool().unwrap_or(false),
        })
        .collect())
}

pub async fn delete_dns_record(token: &str, zone_id: &str, record_id: &str) -> Result<(), String> {
    cf_send(token, reqwest::Method::DELETE, &format!("/zones/{zone_id}/dns_records/{record_id}"), None).await.map(|_| ())
}

/// 建一条 DNS 记录；同名同类型已存在则返回 false（不覆盖用户已有的记录）。
pub async fn ensure_dns_record(
    token: &str,
    zone_id: &str,
    kind: &str,
    name: &str,
    content: &str,
    proxied: bool,
) -> Result<bool, String> {
    let existing = cf_get_query(token, &format!("/zones/{zone_id}/dns_records"), &[("type", kind), ("name", name)]).await?;
    if let Some(arr) = existing["result"].as_array() {
        if kind == "TXT" {
            if arr.iter().any(|r| r["content"].as_str().map(|c| c.trim_matches('"')) == Some(content)) {
                return Ok(false);
            }
        } else if !arr.is_empty() {
            return Ok(false);
        }
    }
    cf_send(
        token,
        reqwest::Method::POST,
        &format!("/zones/{zone_id}/dns_records"),
        Some(serde_json::json!({ "type": kind, "name": name, "content": content, "proxied": proxied, "ttl": 1 })),
    )
    .await?;
    Ok(true)
}

/// Zone 的状态和分到的名称服务器：同名 Zone 在别的账号里生效时，这里是 pending / moved，名称服务器也对不上。
pub async fn zone_info(token: &str, zone_id: &str) -> Result<(String, Vec<String>), String> {
    let b = cf_get(token, &format!("/zones/{zone_id}")).await?;
    let ns = b["result"]["name_servers"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(|s| s.to_ascii_lowercase())).collect();
    Ok((b["result"]["status"].as_str().unwrap_or_default().to_string(), ns))
}

/// Zone 里的 Worker 路由（pattern 命中的请求先进 Worker，不会到 Pages）。
#[derive(Serialize, Clone, Debug)]
pub struct WorkerRoute {
    pub id: String,
    pub pattern: String,
    pub script: String,
}

pub async fn worker_routes(token: &str, zone_id: &str) -> Result<Vec<WorkerRoute>, String> {
    let b = cf_get(token, &format!("/zones/{zone_id}/workers/routes")).await?;
    Ok(b["result"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| WorkerRoute {
            id: r["id"].as_str().unwrap_or_default().into(),
            pattern: r["pattern"].as_str().unwrap_or_default().into(),
            script: r["script"].as_str().unwrap_or_default().into(),
        })
        .collect())
}

pub async fn delete_worker_route(token: &str, zone_id: &str, route_id: &str) -> Result<(), String> {
    cf_send(token, reqwest::Method::DELETE, &format!("/zones/{zone_id}/workers/routes/{route_id}"), None).await.map(|_| ())
}

/// 路由 pattern 的主机部分是否命中这个域名（`htxneo.com/*`、`*htxneo.com/*`、`*.htxneo.com/*` 这类）。
pub fn route_matches(pattern: &str, host: &str) -> bool {
    let p = pattern.trim().trim_start_matches("https://").trim_start_matches("http://");
    let ph = p.split('/').next().unwrap_or_default().to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    match ph.strip_prefix('*') {
        Some(rest) => host.ends_with(rest),
        None => ph == host,
    }
}

/// 绑到 Worker 上的自定义域名（优先于 DNS 记录，域名直接由这个 Worker 回应）。
#[derive(Serialize, Clone, Debug)]
pub struct WorkerDomain {
    pub id: String,
    pub hostname: String,
    pub service: String,
}

pub async fn worker_domains(token: &str, account_id: &str, hostname: &str) -> Result<Vec<WorkerDomain>, String> {
    let b = cf_get_query(token, &format!("/accounts/{account_id}/workers/domains"), &[("hostname", hostname)]).await?;
    Ok(b["result"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| WorkerDomain {
            id: d["id"].as_str().unwrap_or_default().into(),
            hostname: d["hostname"].as_str().unwrap_or_default().into(),
            service: d["service"].as_str().unwrap_or_default().into(),
        })
        .filter(|d| d.hostname.eq_ignore_ascii_case(hostname))
        .collect())
}

pub async fn delete_worker_domain(token: &str, account_id: &str, id: &str) -> Result<(), String> {
    cf_send(token, reqwest::Method::DELETE, &format!("/accounts/{account_id}/workers/domains/{id}"), None).await.map(|_| ())
}

/// 从 Pages 项目里去掉一个自定义域名。
pub async fn delete_pages_domain(token: &str, account_id: &str, project: &str, domain: &str) -> Result<(), String> {
    cf_send(token, reqwest::Method::DELETE, &format!("/accounts/{account_id}/pages/projects/{project}/domains/{domain}"), None).await.map(|_| ())
}

/// 清掉这些主机名在 Cloudflare 边缘的缓存（令牌要有「区域 → 缓存清除」权限）。
pub async fn purge_hosts(token: &str, zone_id: &str, hosts: &[String]) -> Result<(), String> {
    cf_send(token, reqwest::Method::POST, &format!("/zones/{zone_id}/purge_cache"), Some(serde_json::json!({ "hosts": hosts }))).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_patterns_match_hosts() {
        assert!(route_matches("htxneo.com/*", "htxneo.com"));
        assert!(route_matches("*htxneo.com/*", "htxneo.com"));
        assert!(route_matches("*htxneo.com/*", "www.htxneo.com"));
        assert!(!route_matches("*.htxneo.com/*", "htxneo.com"));
        assert!(route_matches("HTXNEO.com/blog/*", "htxneo.com"));
        assert!(!route_matches("api.htxneo.com/*", "htxneo.com"));
    }

    #[test]
    fn parses_id_name_lists_without_panicking_on_partial_items() {
        let value = serde_json::json!({
            "result": [
                { "id": "one", "name": "Example" },
                { "id": "two" }
            ]
        });
        assert_eq!(
            parse_id_name(&value),
            vec![
                ("one".into(), "Example".into()),
                ("two".into(), String::new())
            ]
        );
    }

}

/// 一条完整的 DNS 记录（迁移时对照、回退时原样恢复）。
#[derive(Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct FullRec {
    #[serde(default)]
    pub id: String,
    pub kind: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub proxied: bool,
    #[serde(default)]
    pub priority: Option<u16>,
    #[serde(default)]
    pub ttl: u32,
}

/// Zone 里的全部 DNS 记录（分页读完）。
pub async fn all_dns_records(token: &str, zone_id: &str) -> Result<Vec<FullRec>, String> {
    let mut out = Vec::new();
    for page in 1..=20 {
        let p = page.to_string();
        let body = cf_get_query(token, &format!("/zones/{zone_id}/dns_records"), &[("per_page", "500"), ("page", p.as_str())]).await?;
        let arr = body["result"].as_array().cloned().unwrap_or_default();
        let n = arr.len();
        out.extend(arr.iter().map(|r| FullRec {
            id: r["id"].as_str().unwrap_or_default().into(),
            kind: r["type"].as_str().unwrap_or_default().into(),
            name: r["name"].as_str().unwrap_or_default().into(),
            content: r["content"].as_str().unwrap_or_default().into(),
            proxied: r["proxied"].as_bool().unwrap_or(false),
            priority: r["priority"].as_u64().map(|v| v as u16),
            ttl: r["ttl"].as_u64().unwrap_or(1) as u32,
        }));
        if n < 500 {
            break;
        }
    }
    Ok(out)
}

/// 按给定内容建一条 DNS 记录（不查重，调用方先对照过）。
pub async fn create_dns_record(token: &str, zone_id: &str, r: &FullRec) -> Result<(), String> {
    let mut body = serde_json::json!({ "type": r.kind, "name": r.name, "content": r.content, "ttl": if r.ttl == 0 { 1 } else { r.ttl } });
    if matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME") {
        body["proxied"] = Value::Bool(r.proxied);
    }
    if let Some(p) = r.priority {
        body["priority"] = Value::from(p);
    }
    cf_send(token, reqwest::Method::POST, &format!("/zones/{zone_id}/dns_records"), Some(body)).await.map(|_| ())
}

/// 把域名加进 Cloudflare 账号（完整接入）：Cloudflare 会自动扫一遍现有的 DNS 记录。返回 Zone 和要改成的名称服务器。
pub async fn create_zone(token: &str, account_id: &str, name: &str) -> Result<(CfZone, Vec<String>), String> {
    let b = cf_send(
        token,
        reqwest::Method::POST,
        "/zones",
        Some(serde_json::json!({ "name": name, "account": { "id": account_id }, "type": "full", "jump_start": true })),
    )
    .await?;
    let ns = b["result"]["name_servers"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    Ok((CfZone { id: b["result"]["id"].as_str().unwrap_or_default().into(), name: b["result"]["name"].as_str().unwrap_or(name).into() }, ns))
}

/// Pages 项目的 pages.dev 子域名（没有就按项目名拼）。
pub async fn pages_subdomain(token: &str, account_id: &str, project: &str) -> Result<String, String> {
    let p = get_pages_project(token, account_id, project).await?.ok_or("Pages 项目还没建")?;
    Ok(if p.subdomain.is_empty() { format!("{project}.pages.dev") } else { p.subdomain })
}
