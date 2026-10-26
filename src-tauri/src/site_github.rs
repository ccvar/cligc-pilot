//! 运营任务用到的 GitHub API：开 PR、查 PR 状态、合并 PR、建仓库。
//! 这些都是 Pilot 自己执行的（模型拿不到 Token 去做推送 / 合并），调用前由档位规则或用户点击放行。

use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

const API: &str = "https://api.github.com";

#[derive(Serialize, Clone, Debug, Default)]
pub struct Pr {
    pub number: u64,
    pub url: String,
    /// open | closed
    pub state: String,
    pub merged: bool,
    pub merge_sha: String,
    pub head: String,
}

async fn call(token: &str, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    let mut req = crate::cf::http()
        .request(method, format!("{API}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "cligc-pilot")
        .timeout(Duration::from_secs(30));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.map_err(|e| format!("请求 GitHub 失败：{e}").replace(token, "[已隐藏]"))?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = v["message"].as_str().unwrap_or("");
        let detail = v["errors"].as_array().and_then(|a| a.first()).and_then(|e| e["message"].as_str()).unwrap_or("");
        return Err(match status.as_u16() {
            401 => "GitHub Token 无效或已过期".into(),
            403 => format!("GitHub 拒绝了请求（Token 权限不足）：{msg}"),
            404 => format!("GitHub 找不到资源（Token 可能没有这个仓库的权限）：{msg}"),
            405 | 409 => format!("GitHub 不允许这次操作：{msg}"),
            422 => format!("GitHub 校验失败：{msg} {detail}"),
            c => format!("GitHub 返回 {c}：{msg}"),
        });
    }
    Ok(v)
}

fn pr_from(v: &Value) -> Pr {
    Pr {
        number: v["number"].as_u64().unwrap_or(0),
        url: v["html_url"].as_str().unwrap_or_default().into(),
        state: v["state"].as_str().unwrap_or_default().into(),
        merged: v["merged"].as_bool().unwrap_or(false) || v["merged_at"].is_string(),
        merge_sha: v["merge_commit_sha"].as_str().unwrap_or_default().into(),
        head: v["head"]["ref"].as_str().unwrap_or_default().into(),
    }
}

/// 仓库的默认分支。
pub async fn default_branch(token: &str, repo: &str) -> Result<String, String> {
    Ok(call(token, reqwest::Method::GET, &format!("/repos/{repo}"), None).await?["default_branch"].as_str().unwrap_or("main").to_string())
}

/// 从 origin 地址里取 `owner/repo`（https://github.com/o/r(.git)、git@github.com:o/r.git）。
pub fn repo_from_remote(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let rest = u.split_once("github.com").map(|(_, r)| r.trim_start_matches([':', '/']))?;
    let mut it = rest.split('/');
    match (it.next(), it.next()) {
        (Some(o), Some(r)) if !o.is_empty() && !r.is_empty() => Some(format!("{o}/{r}")),
        _ => None,
    }
}

/// 开 PR；同一分支已有打开的 PR 时返回那一个。
pub async fn create_pr(token: &str, repo: &str, head: &str, base: &str, title: &str, body: &str) -> Result<Pr, String> {
    match call(token, reqwest::Method::POST, &format!("/repos/{repo}/pulls"), Some(json!({ "title": title, "head": head, "base": base, "body": body }))).await {
        Ok(v) => Ok(pr_from(&v)),
        Err(e) if e.contains("already exists") => {
            let owner = repo.split('/').next().unwrap_or("");
            let list = call(token, reqwest::Method::GET, &format!("/repos/{repo}/pulls?state=open&head={owner}:{head}"), None).await?;
            list.as_array().and_then(|a| a.first()).map(pr_from).ok_or(e)
        }
        Err(e) => Err(e),
    }
}

pub async fn get_pr(token: &str, repo: &str, number: u64) -> Result<Pr, String> {
    Ok(pr_from(&call(token, reqwest::Method::GET, &format!("/repos/{repo}/pulls/{number}"), None).await?))
}

/// 合并 PR（squash：一个机会对应默认分支上的一个提交，回滚时整条回退）。
pub async fn merge_pr(token: &str, repo: &str, number: u64, title: &str) -> Result<String, String> {
    let v = call(
        token,
        reqwest::Method::PUT,
        &format!("/repos/{repo}/pulls/{number}/merge"),
        Some(json!({ "merge_method": "squash", "commit_title": title })),
    )
    .await?;
    Ok(v["sha"].as_str().unwrap_or_default().to_string())
}

pub async fn close_pr(token: &str, repo: &str, number: u64) -> Result<(), String> {
    call(token, reqwest::Method::PATCH, &format!("/repos/{repo}/pulls/{number}"), Some(json!({ "state": "closed" }))).await.map(|_| ())
}

pub async fn comment(token: &str, repo: &str, number: u64, body: &str) -> Result<(), String> {
    call(token, reqwest::Method::POST, &format!("/repos/{repo}/issues/{number}/comments"), Some(json!({ "body": body }))).await.map(|_| ())
}

/// 在 Token 所属账号下建仓库（fine-grained Token 需要账号级 Administration 写权限；没有就返回错误，由界面引导手动建）。
pub async fn create_repo(token: &str, name: &str, private: bool, description: &str) -> Result<String, String> {
    let v = call(
        token,
        reqwest::Method::POST,
        "/user/repos",
        Some(json!({ "name": name, "private": private, "description": description, "auto_init": false })),
    )
    .await?;
    v["full_name"].as_str().map(str::to_string).ok_or_else(|| "GitHub 没有返回仓库名".into())
}

/// 仓库是否为空（没有任何提交）。
pub async fn repo_is_empty(token: &str, repo: &str) -> Result<bool, String> {
    match call(token, reqwest::Method::GET, &format!("/repos/{repo}/commits?per_page=1"), None).await {
        Ok(v) => Ok(v.as_array().is_none_or(|a| a.is_empty())),
        Err(e) if e.contains("409") || e.contains("empty") || e.contains("不允许") => Ok(true),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod remote_tests {
    use super::repo_from_remote;
    #[test]
    fn parses_github_remotes() {
        assert_eq!(repo_from_remote("https://github.com/ccvar/htxneo.com.git").as_deref(), Some("ccvar/htxneo.com"));
        assert_eq!(repo_from_remote("git@github.com:ccvar/htxneo.com.git").as_deref(), Some("ccvar/htxneo.com"));
        assert_eq!(repo_from_remote("https://x-access-token@github.com/a/b").as_deref(), Some("a/b"));
        assert_eq!(repo_from_remote("https://gitlab.com/a/b"), None);
    }
}
