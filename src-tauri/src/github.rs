//! GitHub 连接：fine-grained Token 轻客户端 + 本地仓库检出。
//!
//! - 只接受 fine-grained Token（`github_pat_`）：它只授权用户选定的仓库与权限；经典 Token
//!   （`ghp_` 等）覆盖账号下所有仓库，一律拒绝。
//! - Token 只进钥匙串。运行时用 git 的 `GIT_CONFIG_COUNT/KEY/VALUE` 环境变量临时挂一个凭据
//!   助手，助手执行时才从 `GITHUB_TOKEN` 读值：Token 不写 `.git/config`、不出现在命令行参数里；
//!   同时清空 github.com 上已有的凭据助手，模型只能用这枚 Token。
//! - 仓库检出目录：`<连接工作区>/repos/<owner>/<repo>`，由 Pilot 在首轮前克隆，模型不必处理凭据。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

const API: &str = "https://api.github.com";
const MAX_REPO_PAGES: usize = 10;
const CLONE_TIMEOUT: Duration = Duration::from_secs(600);

/// git 凭据助手：只响应 get，从环境变量读 Token。整段作为配置值经环境变量传给 git。
const CREDENTIAL_HELPER: &str =
    "!f() { test \"$1\" = get || exit 0; echo username=x-access-token; echo \"password=$GITHUB_TOKEN\"; }; f";
const HELPER_KEY: &str = "credential.https://github.com.helper";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GhRepo {
    pub full_name: String,
    pub private: bool,
    pub default_branch: String,
    /// Token 对该仓库是否有写权限（能推分支、提 PR）。
    pub can_push: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct GhVerify {
    pub login: String,
    pub user_id: u64,
    pub name: String,
    pub repos: Vec<GhRepo>,
}

/// 只接受 fine-grained Token；经典 Token / OAuth / App Token 都给出明确的拒绝理由。
pub fn check_token_kind(token: &str) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("Token 不能为空".into());
    }
    if token.starts_with("github_pat_") {
        if token.len() > 255 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("Token 格式无效，请重新复制".into());
        }
        return Ok(());
    }
    if ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"].iter().any(|p| token.starts_with(p)) {
        return Err("这是经典 Token（或 OAuth / App Token），它能访问账号下的所有仓库。请改用 fine-grained Token（以 github_pat_ 开头），只授权需要的仓库".into());
    }
    Err("无法识别的 Token，请使用以 github_pat_ 开头的 fine-grained Token".into())
}

/// `owner/repo`，按 GitHub 的命名规则严格校验（防路径穿越 / 参数注入）。
pub fn parse_repo(full: &str) -> Result<(String, String), String> {
    let full = full.trim();
    let (owner, repo) = full.split_once('/').ok_or("仓库应为 owner/repo 形式")?;
    let owner_ok = (1..=39).contains(&owner.len())
        && owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !owner.starts_with('-');
    let repo_ok = (1..=100).contains(&repo.len())
        && repo.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && repo != "."
        && repo != ".."
        && !repo.starts_with('-');
    if !owner_ok || !repo_ok {
        return Err(format!("仓库名无效：{full}"));
    }
    Ok((owner.to_string(), repo.to_string()))
}

pub fn repos_root(skill_dir: &str) -> PathBuf {
    Path::new(skill_dir).join("repos")
}

pub fn repo_dir(skill_dir: &str, full: &str) -> Result<PathBuf, String> {
    let (owner, repo) = parse_repo(full)?;
    Ok(repos_root(skill_dir).join(owner).join(repo))
}

/// 从检出目录反推 `owner/repo`（目录由 repo_dir 生成，结构固定）。
pub fn repo_from_dir(skill_dir: &str, work_dir: &str) -> Option<String> {
    let rel = Path::new(work_dir).strip_prefix(repos_root(skill_dir)).ok()?;
    let parts: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if parts.len() != 2 {
        return None;
    }
    let full = format!("{}/{}", parts[0], parts[1]);
    parse_repo(&full).ok().map(|_| full)
}

/// 本地已检出的仓库（`owner/repo`），供界面列出该连接下的项目。
pub fn checkouts(skill_dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(owners) = std::fs::read_dir(repos_root(skill_dir)) else {
        return out;
    };
    for owner in owners.flatten().filter(|e| e.path().is_dir()) {
        let Ok(repos) = std::fs::read_dir(owner.path()) else { continue };
        for repo in repos.flatten().filter(|e| e.path().join(".git").exists()) {
            let full = format!("{}/{}", owner.file_name().to_string_lossy(), repo.file_name().to_string_lossy());
            if parse_repo(&full).is_ok() {
                out.push(full);
            }
        }
    }
    out.sort();
    out
}

/// 注入给 git / gh / 模型子进程的环境：凭据助手（先清空 github.com 的已有助手再挂自己的）、
/// 禁止交互式输密码、`GITHUB_TOKEN`/`GH_TOKEN` 供 gh 使用。
pub fn git_env(token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("GITHUB_TOKEN", token.to_string()),
        ("GH_TOKEN", token.to_string()),
        ("GIT_TERMINAL_PROMPT", "0".into()),
        ("GCM_INTERACTIVE", "never".into()),
        ("GIT_CONFIG_COUNT", "2".into()),
        ("GIT_CONFIG_KEY_0", HELPER_KEY.into()),
        ("GIT_CONFIG_VALUE_0", String::new()),
        ("GIT_CONFIG_KEY_1", HELPER_KEY.into()),
        ("GIT_CONFIG_VALUE_1", CREDENTIAL_HELPER.into()),
    ]
}

/// 提交作者：Token 所属账号的 noreply 邮箱，只写进该仓库的本地配置，不动全局 git 配置。
pub fn commit_identity(login: &str, user_id: &str) -> (String, String) {
    let email = if user_id.is_empty() {
        format!("{login}@users.noreply.github.com")
    } else {
        format!("{user_id}+{login}@users.noreply.github.com")
    };
    (login.to_string(), email)
}

fn redact(text: &str, token: &str) -> String {
    if token.is_empty() { text.to_string() } else { text.replace(token, "[已隐藏凭据]") }
}

async fn gh_get(client: &reqwest::Client, token: &str, path: &str) -> Result<reqwest::Response, String> {
    let resp = client
        .get(format!("{API}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "cligc-pilot")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| redact(&format!("请求 GitHub 失败：{e}"), token))?;
    match resp.status().as_u16() {
        200..=299 => Ok(resp),
        401 => Err("Token 无效或已过期，请重新生成".into()),
        403 => Err("GitHub 拒绝了请求（权限不足或触发了频率限制），请检查 Token 权限后重试".into()),
        code => Err(format!("GitHub 返回 {code}")),
    }
}

/// 验证 Token：取账号信息与它能访问的仓库（最多 1000 个）。
pub async fn verify_token(token: &str) -> Result<GhVerify, String> {
    check_token_kind(token)?;
    let token = token.trim();
    let client = reqwest::Client::new();
    let user: serde_json::Value = gh_get(&client, token, "/user")
        .await?
        .json()
        .await
        .map_err(|_| "GitHub 账号信息无法解析".to_string())?;
    let login = user["login"].as_str().unwrap_or_default().to_string();
    if login.is_empty() {
        return Err("GitHub 没有返回账号信息".into());
    }
    let mut repos = Vec::new();
    for page in 1..=MAX_REPO_PAGES {
        let items: Vec<serde_json::Value> = gh_get(
            &client,
            token,
            &format!("/user/repos?per_page=100&sort=updated&page={page}"),
        )
        .await?
        .json()
        .await
        .map_err(|_| "GitHub 仓库列表无法解析".to_string())?;
        let done = items.len() < 100;
        repos.extend(items.iter().filter_map(repo_from_json));
        if done {
            break;
        }
    }
    Ok(GhVerify {
        login,
        user_id: user["id"].as_u64().unwrap_or(0),
        name: user["name"].as_str().unwrap_or_default().to_string(),
        repos,
    })
}

fn repo_from_json(v: &serde_json::Value) -> Option<GhRepo> {
    let full_name = v["full_name"].as_str()?.to_string();
    parse_repo(&full_name).ok()?;
    Some(GhRepo {
        full_name,
        private: v["private"].as_bool().unwrap_or(false),
        default_branch: v["default_branch"].as_str().unwrap_or("main").to_string(),
        can_push: v["permissions"]["push"].as_bool().unwrap_or(false),
    })
}

/// 确保仓库已检出到 `work_dir`：已有 `.git` 直接返回 false；否则克隆并写入本地提交身份，返回 true。
pub async fn ensure_checkout(
    skill_dir: &str,
    work_dir: &str,
    token: &str,
    login: &str,
    user_id: &str,
) -> Result<bool, String> {
    let dir = Path::new(work_dir);
    if dir.join(".git").exists() {
        return Ok(false);
    }
    let full = repo_from_dir(skill_dir, work_dir).ok_or("仓库目录无效")?;
    if dir.exists() && std::fs::read_dir(dir).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err(format!("{full} 的本地目录已存在但不是 git 仓库，请先清理后重试"));
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建仓库目录失败：{e}"))?;
    }
    let url = format!("https://github.com/{full}.git");
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["clone", "--quiet", "--", &url, work_dir])
        .envs(git_env(token))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(CLONE_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("克隆 {full} 超时"))?
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
            "未找到 git，请先安装 Git".to_string()
        } else {
            format!("启动 git 失败：{e}")
        })?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(dir);
        let detail = redact(String::from_utf8_lossy(&out.stderr).trim(), token);
        return Err(format!("克隆 {full} 失败：{detail}"));
    }
    let (name, email) = commit_identity(login, user_id);
    for (key, value) in [("user.name", name), ("user.email", email)] {
        let status = tokio::process::Command::new("git")
            .args(["-C", work_dir, "config", "--local", key, &value])
            .status()
            .await
            .map_err(|e| format!("设置提交身份失败：{e}"))?;
        if !status.success() {
            return Err("设置提交身份失败".into());
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fine_grained_tokens_are_accepted() {
        assert!(check_token_kind("github_pat_11ABCDEFG0123456789_abcdefXYZ").is_ok());
        for classic in ["ghp_abc", "gho_abc", "ghu_abc", "ghs_abc"] {
            assert!(check_token_kind(classic).unwrap_err().contains("fine-grained"));
        }
        assert!(check_token_kind("").is_err());
        assert!(check_token_kind("github_pat_has space").is_err());
        assert!(check_token_kind("random").is_err());
    }

    #[test]
    fn repo_names_are_strict() {
        assert_eq!(parse_repo("ccvar/cligc-pilot").unwrap(), ("ccvar".into(), "cligc-pilot".into()));
        assert!(parse_repo("octo/repo.name_1").is_ok());
        for bad in ["noslash", "a/b/c", "../x", "a/..", "a/.", "-a/b", "a/-b", "a b/c", "a/b;rm", "/b", "a/"] {
            assert!(parse_repo(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn repo_dir_round_trips_and_rejects_foreign_paths() {
        let dir = repo_dir("/ws/conn", "octo/hello").unwrap();
        assert_eq!(dir, Path::new("/ws/conn/repos/octo/hello"));
        assert_eq!(repo_from_dir("/ws/conn", dir.to_str().unwrap()).as_deref(), Some("octo/hello"));
        assert_eq!(repo_from_dir("/ws/conn", "/ws/conn/repos/octo"), None);
        assert_eq!(repo_from_dir("/ws/conn", "/elsewhere/octo/hello"), None);
    }

    #[test]
    fn git_env_keeps_the_token_out_of_config_values() {
        let env = git_env("github_pat_secret");
        let get = |k: &str| env.iter().find(|(key, _)| *key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("GITHUB_TOKEN"), Some("github_pat_secret"));
        assert_eq!(get("GIT_CONFIG_VALUE_0"), Some(""));
        assert!(!get("GIT_CONFIG_VALUE_1").unwrap().contains("github_pat_secret"));
        assert_eq!(get("GIT_TERMINAL_PROMPT"), Some("0"));
    }

    #[test]
    fn commit_identity_uses_the_noreply_address() {
        assert_eq!(commit_identity("octo", "42").1, "42+octo@users.noreply.github.com");
        assert_eq!(commit_identity("octo", "").1, "octo@users.noreply.github.com");
    }

    /// 真实 git：环境里的凭据助手清掉用户已有的助手，并只从环境变量吐出 Token。
    #[test]
    fn credential_helper_answers_from_the_environment_only() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            return;
        }
        use std::io::Write;
        let mut child = std::process::Command::new("git")
            .args(["credential", "fill"])
            .envs(git_env("github_pat_testonly"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"protocol=https\nhost=github.com\n\n").unwrap();
        let out = child.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("username=x-access-token"), "{text}");
        assert!(text.contains("password=github_pat_testonly"), "{text}");
    }

    #[test]
    fn checkouts_lists_only_git_repos() {
        let root = tempfile::tempdir().unwrap();
        let ws = root.path().to_str().unwrap();
        std::fs::create_dir_all(root.path().join("repos/octo/hello/.git")).unwrap();
        std::fs::create_dir_all(root.path().join("repos/octo/not-a-repo")).unwrap();
        assert_eq!(checkouts(ws), vec!["octo/hello".to_string()]);
    }
}

