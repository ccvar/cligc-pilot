//! 站点用到的 git 操作：部署目录（工作树）、运营分支、差异、合并与回退。
//! 全部由 Pilot 自己执行（模型不碰推送 / 合并），GitHub 凭据只经环境变量注入、不落 `.git/config`。

use std::path::{Path, PathBuf};
use std::time::Duration;

const GIT_TIMEOUT: Duration = Duration::from_secs(300);

pub struct GitOut {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// 本地来源的站点：主分支合并 / 回退后推到 origin（一般是 GitHub 私库），让远端和本地保持一致。
/// 只推快进，不强推：远端有本地没有的提交就推不上去、原样报错；没有 origin 返回 Ok(false)。
/// SSH 远端不弹密码框（BatchMode），没配好就直接失败。
pub async fn push_origin(main: &Path, branch: &str) -> Result<bool, String> {
    if git(main, &["remote", "get-url", "origin"], "").await.map(|o| o.ok).unwrap_or(false) {
        git_ok(main, &["-c", "core.sshCommand=ssh -o BatchMode=yes", "push", "-q", "origin", &format!("HEAD:refs/heads/{branch}")], "").await.map(|_| true)
    } else {
        Ok(false)
    }
}

/// 跑一条 git 命令。`token` 非空时注入 GitHub 凭据助手（见 github::git_env），并把输出里的 Token 抹掉。
pub async fn git(dir: &Path, args: &[&str], token: &str) -> Result<GitOut, String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if !token.is_empty() {
        cmd.envs(crate::github::git_env(token));
    }
    let out = tokio::time::timeout(GIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("git {} 超时", args.first().unwrap_or(&"")))?
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "未找到 git，请先安装 Git".to_string()
            } else {
                format!("启动 git 失败：{e}")
            }
        })?;
    let scrub = |b: &[u8]| {
        let s = String::from_utf8_lossy(b).trim().to_string();
        if token.is_empty() { s } else { s.replace(token, "[已隐藏凭据]") }
    };
    Ok(GitOut {
        ok: out.status.success(),
        stdout: scrub(&out.stdout),
        stderr: scrub(&out.stderr),
    })
}

/// 跑 git，失败即返回带 stderr 的错误。
pub async fn git_ok(dir: &Path, args: &[&str], token: &str) -> Result<String, String> {
    let o = git(dir, args, token).await?;
    if o.ok {
        Ok(o.stdout)
    } else {
        Err(format!(
            "git {} 失败：{}",
            args.first().unwrap_or(&""),
            if o.stderr.is_empty() { o.stdout } else { o.stderr }
        ))
    }
}

pub async fn head_sha(dir: &Path) -> String {
    git_ok(dir, &["rev-parse", "HEAD"], "").await.unwrap_or_default()
}

pub async fn head_subject(dir: &Path) -> String {
    git_ok(dir, &["log", "-1", "--format=%s"], "").await.unwrap_or_default()
}

pub async fn current_branch(dir: &Path) -> String {
    git_ok(dir, &["branch", "--show-current"], "").await.unwrap_or_default()
}

/// 工作区是否干净（忽略未跟踪文件之外的一切改动都算脏）。
pub async fn is_clean(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain", "--untracked-files=no"], "")
        .await
        .map(|o| o.ok && o.stdout.is_empty())
        .unwrap_or(false)
}

/// 分支名只允许 `pilot/` 前缀下的安全字符，避免被拼进命令时出意外。
pub fn safe_branch(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('-')
        && !name.contains("..")
        && !name.ends_with('/')
        && !name.ends_with(".lock")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

/// 把一段任意文字转成分支名里可用的短 slug。
pub fn slugify(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= max {
            break;
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() { "change".into() } else { s }
}

/// 在 `repo` 上建（或复位）一个工作树到 `path`，检出 `rev`。
/// `branch` 为 Some 时新建该分支（已存在则重置到 rev），否则分离 HEAD。
/// 工作树与主检出共享对象库，所以不必重新下载；主检出被删时会自动 prune 掉失效记录。
pub async fn worktree_at(repo: &Path, path: &Path, rev: &str, branch: Option<&str>) -> Result<(), String> {
    let _ = git(repo, &["worktree", "prune"], "").await;
    let path_s = path.to_string_lossy().to_string();
    if path.join(".git").exists() {
        // 已有工作树：强制切到目标版本，丢弃上次残留的改动（这是 Pilot 自己的目录）。
        let ok = match branch {
            Some(b) => git(path, &["checkout", "--force", "-B", b, rev], "").await?,
            None => git(path, &["checkout", "--force", "--detach", rev], "").await?,
        };
        if ok.ok {
            let _ = git(path, &["clean", "-fdq", "-e", "node_modules"], "").await;
            return Ok(());
        }
        // 工作树坏了（主检出被重建过等）：删掉重来。
        let _ = std::fs::remove_dir_all(path);
    } else if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建工作目录失败：{e}"))?;
    }
    if let Some(b) = branch {
        release_branch(repo, b, path).await;
    }
    match branch {
        Some(b) => git_ok(repo, &["worktree", "add", "--force", "-B", b, &path_s, rev], "").await?,
        None => git_ok(repo, &["worktree", "add", "--force", "--detach", &path_s, rev], "").await?,
    };
    Ok(())
}

/// 这个分支还检出在别的工作树里（构建目录挪过位置后留在旧目录的那份）：拿掉那份，-B 才能重建分支。
/// 只拿 Pilot 自己的工作树（路径里有 /work/ 或 /deploy/），用户自己开的不动。
async fn release_branch(repo: &Path, branch: &str, keep: &Path) {
    let list = git(repo, &["worktree", "list", "--porcelain"], "").await.map(|o| o.stdout).unwrap_or_default();
    let want = format!("branch refs/heads/{branch}");
    for block in list.split("\n\n").filter(|b| b.lines().any(|l| l == want)) {
        let Some(p) = block.lines().find_map(|l| l.strip_prefix("worktree ")) else { continue };
        let ours = p.contains("/work/") || p.contains("/deploy");
        if Path::new(p) != keep && ours {
            if !git(repo, &["worktree", "remove", "--force", p], "").await.map(|r| r.ok).unwrap_or(false) {
                let _ = std::fs::remove_dir_all(p);
                let _ = git(repo, &["worktree", "prune"], "").await;
            }
        }
    }
}

/// 分支还在、但工作树不在 `path`（构建目录挪过位置）：分支原来的工作树还在就整个挪过来（git worktree move，
/// 改动和状态都保留），不在了就在 `path` 重新检出这个分支。不用 -B：分支还检出在别处时 git 不让强制改它。
pub async fn reattach_worktree(repo: &Path, path: &Path, branch: &str) -> Result<(), String> {
    let _ = git(repo, &["worktree", "prune"], "").await;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建工作目录失败：{e}"))?;
    }
    let list = git(repo, &["worktree", "list", "--porcelain"], "").await.map(|o| o.stdout).unwrap_or_default();
    let want = format!("branch refs/heads/{branch}");
    let old = list.split("\n\n").find(|b| b.lines().any(|l| l == want)).and_then(|b| b.lines().find_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from));
    if let Some(old) = old.filter(|o| o.is_dir() && o != path) {
        let (o, p) = (old.to_string_lossy().to_string(), path.to_string_lossy().to_string());
        if git(repo, &["worktree", "move", &o, &p], "").await.map(|r| r.ok).unwrap_or(false) {
            return Ok(());
        }
    }
    git_ok(repo, &["worktree", "add", "--force", &path.to_string_lossy(), branch], "").await.map(|_| ())
}

pub async fn remove_worktree(repo: &Path, path: &Path) {
    let p = path.to_string_lossy().to_string();
    let _ = git(repo, &["worktree", "remove", "--force", &p], "").await;
    let _ = std::fs::remove_dir_all(path);
    let _ = git(repo, &["worktree", "prune"], "").await;
}

/// 主检出里 node_modules 已装好时，给工作树挂一个软链接，省掉每次运营任务都重新 `npm ci`。
/// 锁文件不一致就不挂（依赖可能不同），交给安装步骤处理。
pub fn share_node_modules(main: &Path, worktree: &Path) {
    let src = main.join("node_modules");
    let dst = worktree.join("node_modules");
    if !src.is_dir() || dst.exists() {
        return;
    }
    let same_lock = ["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock"]
        .iter()
        .all(|f| std::fs::read(main.join(f)).ok() == std::fs::read(worktree.join(f)).ok());
    if !same_lock {
        return;
    }
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(&src, &dst);
    #[cfg(windows)]
    let _ = std::os::windows::fs::symlink_dir(&src, &dst);
}

/// AI 在工作树里动了锁文件（装了包、升了版本）：拆掉共享的 node_modules 软链，让构建按这份锁文件在工作树里重装，
/// 不然检查用的还是主检出的旧依赖，结果不可信。返回拆没拆。
pub fn unshare_node_modules_if_changed(worktree: &Path) -> bool {
    let dst = worktree.join("node_modules");
    let Ok(src) = std::fs::read_link(&dst) else { return false };
    let Some(main) = src.parent() else { return false };
    let same_lock = ["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", "package.json"]
        .iter()
        .all(|f| std::fs::read(main.join(f)).ok() == std::fs::read(worktree.join(f)).ok());
    if same_lock {
        return false;
    }
    #[cfg(unix)]
    let r = std::fs::remove_file(&dst);
    #[cfg(windows)]
    let r = std::fs::remove_dir(&dst);
    r.is_ok()
}

/// `base..head` 之间的统一差异（零上下文，供改动分类用）。
pub async fn diff_zero_context(dir: &Path, base: &str, head: &str) -> Result<String, String> {
    git_ok(dir, &["diff", "--no-color", "--no-ext-diff", "-U0", "--no-renames", &format!("{base}...{head}")], "").await
}

/// 给人看的差异（带上下文）。
pub async fn diff_for_review(dir: &Path, base: &str, head: &str) -> Result<String, String> {
    git_ok(dir, &["diff", "--no-color", "--no-ext-diff", "--stat", "-p", &format!("{base}...{head}")], "").await
}

pub async fn commits_between(dir: &Path, base: &str, head: &str) -> Vec<String> {
    git_ok(dir, &["log", "--format=%H", &format!("{base}..{head}")], "")
        .await
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Pilot 自己提交时用的身份参数：仓库已有身份就不覆盖；没有（本机没配 git 身份）时临时用 Cligc Pilot，
/// 只作用于这一次命令，不写进用户的配置。
pub async fn identity_flags(dir: &Path) -> Vec<String> {
    let has = |k: &'static str| async move { git(dir, &["config", k], "").await.map(|o| o.ok && !o.stdout.is_empty()).unwrap_or(false) };
    let mut out = Vec::new();
    if !has("user.name").await {
        out.extend(["-c".to_string(), "user.name=Cligc Pilot".to_string()]);
    }
    if !has("user.email").await {
        out.extend(["-c".to_string(), "user.email=pilot@localhost".to_string()]);
    }
    out
}

/// 以 Pilot 的名义跑一条会产生提交的 git 命令（commit / merge / revert）。
pub async fn git_commit_ok(dir: &Path, args: &[&str]) -> Result<String, String> {
    let flags = identity_flags(dir).await;
    let mut all: Vec<&str> = flags.iter().map(String::as_str).collect();
    all.extend_from_slice(args);
    git_ok(dir, &all, "").await
}

/// 站点主代码目录：本地来源就是用户的文件夹；GitHub 来源是 GitHub 连接工作区里的检出。
pub fn code_dir(site: &crate::sites::Site, gh: Option<&crate::conns::Connection>) -> Result<PathBuf, String> {
    match site.source.as_str() {
        "local" => {
            let p = PathBuf::from(&site.local_dir);
            if !p.is_dir() {
                return Err(format!("站点文件夹不存在：{}", site.local_dir));
            }
            Ok(p)
        }
        "github" => {
            let gh = gh.ok_or("站点的 GitHub 连接不存在，请在站点设置里重新选择")?;
            crate::github::repo_dir(&gh.skill_dir, &site.repo)
        }
        other => Err(format!("未知的代码来源：{other}")),
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn push_origin_fast_forwards_and_skips_without_remote() {
        let t = tempfile::tempdir().unwrap();
        let (repo, bare) = (t.path().join("repo"), t.path().join("bare.git"));
        let run = |d: &Path, a: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(d).args(a).output().unwrap().status.success(), "{a:?}");
        std::fs::create_dir_all(&repo).unwrap();
        run(&repo, &["init", "-q", "-b", "main"]);
        run(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "a"]);
        assert!(!push_origin(&repo, "main").await.unwrap(), "没有 origin 不推");
        run(t.path(), &["init", "-q", "--bare", "bare.git"]);
        run(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
        assert!(push_origin(&repo, "main").await.unwrap());
        run(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "b"]);
        assert!(push_origin(&repo, "main").await.unwrap());
        let head = String::from_utf8(std::process::Command::new("git").arg("-C").arg(&repo).args(["rev-parse", "HEAD"]).output().unwrap().stdout).unwrap();
        let remote = String::from_utf8(std::process::Command::new("git").arg("-C").arg(&bare).args(["rev-parse", "main"]).output().unwrap().stdout).unwrap();
        assert_eq!(head, remote);
    }

    use super::*;

    #[tokio::test]
    async fn reattach_moves_the_old_worktree_with_spaces() {
        let root = std::env::temp_dir().join(format!("reattach-{}", uuid::Uuid::new_v4()));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let run = |d: &Path, a: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(d).args(a).status().unwrap().success());
        run(&repo, &["init", "-q", "-b", "main"]);
        run(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);
        // AI 的工作树在带空格的旧目录里，分支上有一次提交
        let old = root.join("Application Support/work/i1");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        run(&repo, &["worktree", "add", "-q", "-b", "pilot/ops-x", &old.to_string_lossy()]);
        std::fs::write(old.join("f.txt"), "ai").unwrap();
        run(&old, &["add", "f.txt"]);
        run(&old, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "ai"]);
        // 构建目录挪了：在新位置接上
        let new = root.join("Caches/work/i1");
        reattach_worktree(&repo, &new, "pilot/ops-x").await.unwrap();
        assert_eq!(std::fs::read_to_string(new.join("f.txt")).unwrap(), "ai");
        assert_eq!(current_branch(&new).await, "pilot/ops-x");
        assert!(!old.exists());
        // 让 AI 重新改：在另一个新位置用 -B 重建同名分支，旧的那份（这里是刚挪过去的 new）要先让出来
        let again = root.join("Caches/work/i1-again");
        worktree_at(&repo, &again, "main", Some("pilot/ops-x")).await.unwrap();
        assert_eq!(current_branch(&again).await, "pilot/ops-x");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn branch_names_are_restricted() {
        assert!(safe_branch("pilot/ops-meta-1"));
        for bad in ["", "-x", "a..b", "a/", "x.lock", "a b", "a;b", "a~1"] {
            assert!(!safe_branch(bad), "{bad}");
        }
        assert_eq!(slugify("改 /zh/exchanges 的 Title!!", 30), "zh-exchanges-title");
        assert_eq!(slugify("中文", 10), "change");
    }

    /// 真实 git：工作树能建、能复位，删掉主检出里的分支记录也不影响下次重建。
    #[tokio::test]
    async fn worktrees_can_be_created_and_reset() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| {
            let s = std::process::Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap();
            assert!(s.status.success(), "{args:?}: {}", String::from_utf8_lossy(&s.stderr));
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(repo.join("a.txt"), "1").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "one"]);
        let wt = t.path().join("wt");
        worktree_at(&repo, &wt, "main", None).await.unwrap();
        assert_eq!(std::fs::read_to_string(wt.join("a.txt")).unwrap(), "1");
        std::fs::write(wt.join("a.txt"), "dirty").unwrap();
        std::fs::write(repo.join("a.txt"), "2").unwrap();
        run(&["commit", "-qam", "two"]);
        worktree_at(&repo, &wt, "main", None).await.unwrap();
        assert_eq!(std::fs::read_to_string(wt.join("a.txt")).unwrap(), "2");
        let br = t.path().join("br");
        worktree_at(&repo, &br, "main", Some("pilot/x")).await.unwrap();
        assert_eq!(current_branch(&br).await, "pilot/x");
        assert!(is_clean(&br).await);
        std::fs::write(br.join("a.txt"), "3").unwrap();
        assert!(!is_clean(&br).await);
        remove_worktree(&repo, &br).await;
        assert!(!br.exists());
    }
}
