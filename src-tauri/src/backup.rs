//! 备份 Pilot 自己的数据：站点设置、托管方案和日志、计划、任务队列、运营记录、复盘、AI 用量……
//! 这些只在这台电脑上（站点代码本身在 GitHub 上，不用备份）。每周推一份到你 GitHub 账号下的**私有**仓库，
//! 换电脑或硬盘坏了可以恢复。
//!
//! - 不备份：钥匙串里的密钥和 Token（本来就不在数据目录里）、对话记录、数据缓存（能重新拉）、构建目录；
//! - 仓库必须是私有的：发现是公开仓库就不推；
//! - 每次备份是一个提交，GitHub 上能看到每周的历史；另一台电脑也往同一个仓库备份时接在它后面，不覆盖历史；
//! - 恢复：先把要被覆盖的文件挪到数据目录的 before-restore-<时间>/ 留底，连接、API 服务、站点这几个列表按 id 合并
//!   （这台电脑上有、备份里没有的保留），然后重启 Pilot。
//!
//! 设置和上次结果存在数据目录的 backup.json；本地工作副本在 backup-repo/。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const REPO_NAME: &str = "cligc-pilot-backup";
const DAY: u64 = 86_400;
const WEEK: u64 = 7 * DAY;
/// 恢复时按 id 合并的列表文件（其它文件整份覆盖）
const MERGE_LISTS: &[&str] = &["sites.json", "connections.json", "model-api-profiles.json"];
/// 数据目录根下要备份的文件
const ROOT_FILES: &[&str] = &["sites.json", "connections.json", "model-api-profiles.json"];
/// 每个站点目录下要整个备份的子目录（其余只备份顶层的 .json / .jsonl）
const SITE_DIRS: &[&str] = &["plans", "reports"];

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Settings {
    /// 每周自动备份
    #[serde(default)]
    pub enabled: bool,
    /// 用哪个 GitHub 连接
    #[serde(default)]
    pub conn_id: String,
    /// owner/name；空＝第一次备份时在这个账号下建 cligc-pilot-backup
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub last_at: u64,
    #[serde(default)]
    pub last_ok: bool,
    #[serde(default)]
    pub last_msg: String,
    #[serde(default)]
    pub last_files: usize,
    #[serde(default)]
    pub last_bytes: u64,
    /// 上次尝试（失败了一天后再试，不每 10 分钟重试）
    #[serde(default)]
    pub last_try: u64,
}

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("backup.json")
}
pub fn load(data_dir: &Path) -> Settings {
    crate::store_file::read_or_quarantine(&path(data_dir))
}
fn save(data_dir: &Path, s: &Settings) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let tmp = path(data_dir).with_extension("json.tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, path(data_dir));
        }
    }
}

fn now() -> u64 {
    crate::now_secs()
}

fn skip_name(name: &str) -> bool {
    name.starts_with('.') || name.ends_with(".tmp") || name.ends_with(".lock") || name.contains(".corrupt-")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if skip_name(&name) {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else if p.is_file() {
            out.push(p);
        }
    }
}

/// 要备份的文件（绝对路径，都在数据目录里），排好序。
pub fn collect(data_dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = ROOT_FILES.iter().map(|f| data_dir.join(f)).filter(|p| p.is_file()).collect();
    walk(&data_dir.join("managed"), &mut out);
    if let Ok(rd) = std::fs::read_dir(data_dir.join("sites")) {
        for site in rd.flatten().filter(|e| e.path().is_dir()) {
            let Ok(files) = std::fs::read_dir(site.path()) else { continue };
            for f in files.flatten() {
                let p = f.path();
                let name = f.file_name().to_string_lossy().into_owned();
                if skip_name(&name) {
                    continue;
                }
                if p.is_file() && (name.ends_with(".json") || name.ends_with(".jsonl")) {
                    out.push(p);
                } else if p.is_dir() && SITE_DIRS.contains(&name.as_str()) {
                    walk(&p, &mut out);
                }
            }
        }
    }
    out.sort();
    out
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Manifest {
    pub at: u64,
    pub version: String,
    pub host: String,
    pub sites: usize,
    pub files: usize,
    pub bytes: u64,
}

fn host_name() -> String {
    std::process::Command::new("hostname").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn site_count(dir: &Path) -> usize {
    std::fs::read(dir.join("sites.json")).ok().and_then(|b| serde_json::from_slice::<Vec<serde_json::Value>>(&b).ok()).map(|v| v.len()).unwrap_or(0)
}

/// 把要备份的文件同步进工作副本（先清空，除了 .git），写上 manifest.json 和 README.md。返回清单。
fn stage(data_dir: &Path, mirror: &Path) -> Result<Manifest, String> {
    if let Ok(rd) = std::fs::read_dir(mirror) {
        for e in rd.flatten() {
            if e.file_name() == ".git" {
                continue;
            }
            let p = e.path();
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
    }
    let files = collect(data_dir);
    let mut bytes = 0;
    for f in &files {
        let rel = f.strip_prefix(data_dir).map_err(|e| e.to_string())?;
        let dst = mirror.join(rel);
        if let Some(d) = dst.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("准备备份失败：{e}"))?;
        }
        bytes += std::fs::copy(f, &dst).map_err(|e| format!("复制 {} 失败：{e}", rel.display()))?;
    }
    let m = Manifest { at: now(), version: env!("CARGO_PKG_VERSION").into(), host: host_name(), sites: site_count(data_dir), files: files.len(), bytes };
    std::fs::write(mirror.join("manifest.json"), serde_json::to_vec_pretty(&m).unwrap_or_default()).map_err(|e| e.to_string())?;
    std::fs::write(
        mirror.join("README.md"),
        "# Cligc Pilot 数据备份\n\n由 Cligc Pilot 每周自动推送：站点设置、托管方案、计划、任务队列、运营记录、复盘和 AI 用量。\n\
不含任何密钥和 Token（它们只在电脑的钥匙串里）、对话记录和数据缓存。\n\n\
恢复：在 Pilot 的「托管运营 → 后台运行与备份」里点「从备份恢复」。请保持这个仓库为私有。\n",
    )
    .map_err(|e| e.to_string())?;
    Ok(m)
}

async fn git(dir: &Path, args: &[&str], token: &str) -> Result<String, String> {
    crate::site_git::git_ok(dir, args, token).await
}

/// 备份一次。成功返回一句说明。
pub async fn run(data_dir: &Path, conns: &crate::conns::ConnStore) -> Result<String, String> {
    let mut set = load(data_dir);
    set.last_try = now();
    save(data_dir, &set);
    let r = run_inner(data_dir, conns, &mut set).await;
    set.last_ok = r.is_ok();
    set.last_msg = match &r {
        Ok(m) => m.clone(),
        Err(e) => e.clone(),
    };
    if r.is_ok() {
        set.last_at = now();
    }
    save(data_dir, &set);
    r
}

fn github_conn(conns: &crate::conns::ConnStore, id: &str) -> Result<(crate::conns::Connection, String), String> {
    let conn = conns.get(id).map_err(|_| "选的 GitHub 连接不存在了，换一个".to_string())?;
    if conn.kind != "github" {
        return Err("备份要用 GitHub 连接".into());
    }
    let token = crate::keychain::get_key(&conn.id).map_err(|_| "这个 GitHub 连接的 Token 不在钥匙串里，先在「连接与模型」里重新填一下".to_string())?;
    Ok((conn, token))
}

async fn run_inner(data_dir: &Path, conns: &crate::conns::ConnStore, set: &mut Settings) -> Result<String, String> {
    if set.conn_id.is_empty() {
        return Err("先选一个 GitHub 连接".into());
    }
    let (conn, token) = github_conn(conns, &set.conn_id)?;
    let repo = if set.repo.is_empty() { format!("{}/{REPO_NAME}", conn.login) } else { set.repo.clone() };
    match crate::site_github::repo_private(&token, &repo).await? {
        Some(true) => {}
        Some(false) => return Err(format!("{repo} 是公开仓库，不能放备份。到 GitHub 把它改成私有，或者换一个仓库")),
        None => {
            let name = repo.split('/').nth(1).unwrap_or(REPO_NAME);
            crate::site_github::create_repo(&token, name, true, "Cligc Pilot 数据备份（私有，不含密钥）")
                .await
                .map_err(|e| format!("在 GitHub 上建不了备份仓库（{e}）。可以自己在 GitHub 建一个私有仓库 {repo}，再点一次备份"))?;
        }
    }
    set.repo = repo.clone();
    let mirror = data_dir.join("backup-repo");
    std::fs::create_dir_all(&mirror).map_err(|e| e.to_string())?;
    if !mirror.join(".git").exists() {
        git(&mirror, &["init", "--quiet", "-b", "main"], "").await?;
    }
    let url = format!("https://github.com/{repo}.git");
    let _ = crate::site_git::git(&mirror, &["remote", "remove", "origin"], "").await;
    git(&mirror, &["remote", "add", "origin", &url], "").await?;
    let _ = crate::site_git::git(&mirror, &["fetch", "--quiet", "origin", "main"], &token).await;
    let has_remote = crate::site_git::git(&mirror, &["rev-parse", "--verify", "--quiet", "origin/main"], "").await.is_ok_and(|o| o.ok);
    let has_local = crate::site_git::git(&mirror, &["rev-parse", "--verify", "--quiet", "HEAD"], "").await.is_ok_and(|o| o.ok);
    // 仓库里已经有别的电脑的备份：接在它后面
    if has_remote && !has_local {
        git(&mirror, &["reset", "--quiet", "origin/main"], "").await?;
    }
    let m = stage(data_dir, &mirror)?;
    let (name, email) = crate::github::commit_identity(&conn.login, &conn.account_id);
    let msg = format!("备份 {}（{} 个站点、{} 个文件）", chrono::Local::now().format("%Y-%m-%d %H:%M"), m.sites, m.files);
    let commit = |mirror: PathBuf, name: String, email: String, msg: String| async move {
        git(&mirror, &["add", "-A"], "").await?;
        git(&mirror, &["-c", &format!("user.name={name}"), "-c", &format!("user.email={email}"), "commit", "--quiet", "-m", &msg], "").await
    };
    commit(mirror.clone(), name.clone(), email.clone(), msg.clone()).await?;
    if git(&mirror, &["push", "--quiet", "origin", "HEAD:main"], &token).await.is_err() {
        // 别的电脑刚推过：拿到它的最新提交，把这份快照接在后面再推（不改写它的历史）
        git(&mirror, &["fetch", "--quiet", "origin", "main"], &token).await?;
        git(&mirror, &["reset", "--soft", "--quiet", "origin/main"], "").await?;
        git(&mirror, &["-c", &format!("user.name={name}"), "-c", &format!("user.email={email}"), "commit", "--quiet", "--allow-empty", "-m", &msg], "").await?;
        git(&mirror, &["push", "--quiet", "origin", "HEAD:main"], &token).await?;
    }
    set.last_files = m.files;
    set.last_bytes = m.bytes;
    Ok(format!("已备份到 {repo}：{} 个站点、{} 个文件", m.sites, m.files))
}

static RUNNING: AtomicBool = AtomicBool::new(false);

/// 调度每轮调用：打开了每周备份、离上次成功满一周（失败的话离上次尝试满一天）就在后台备份一次。
pub fn schedule(app: &tauri::AppHandle) {
    use tauri::Manager;
    let st = app.state::<crate::AppState>();
    let set = load(&st.data_dir);
    let t = now();
    if !set.enabled || set.conn_id.is_empty() || t.saturating_sub(set.last_at) < WEEK || t.saturating_sub(set.last_try) < DAY {
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<crate::AppState>();
        let r = run(&st.data_dir, &st.conns).await;
        RUNNING.store(false, Ordering::SeqCst);
        if let Err(e) = r {
            use tauri_plugin_notification::NotificationExt;
            let _ = app.notification().builder().title("Pilot 数据备份失败").body(&e).show();
        }
        use tauri::Emitter;
        let _ = app.emit("backup-done", ());
    });
}

/// 按 id 合并两个列表：备份里的覆盖同 id 的，这台电脑上独有的保留（排在后面）。
fn merge_by_id(current: &[serde_json::Value], backup: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let id = |v: &serde_json::Value| v["id"].as_str().unwrap_or_default().to_string();
    let ids: std::collections::HashSet<String> = backup.iter().map(id).collect();
    let mut out = backup.to_vec();
    out.extend(current.iter().filter(|v| !ids.contains(&id(v))).cloned());
    out
}

async fn clone_backup(data_dir: &Path, conns: &crate::conns::ConnStore, conn_id: &str, repo: &str) -> Result<(PathBuf, Manifest), String> {
    let (conn, token) = github_conn(conns, conn_id)?;
    let repo = if repo.trim().is_empty() { format!("{}/{REPO_NAME}", conn.login) } else { repo.trim().to_string() };
    crate::github::parse_repo(&repo)?;
    let tmp = data_dir.join("backup-restore");
    let _ = std::fs::remove_dir_all(&tmp);
    let url = format!("https://github.com/{repo}.git");
    crate::site_git::git_ok(data_dir, &["clone", "--quiet", "--depth", "1", &url, "backup-restore"], &token)
        .await
        .map_err(|e| format!("取不到备份（{repo}）：{}", e.lines().last().unwrap_or(&e)))?;
    let m: Manifest = std::fs::read(tmp.join("manifest.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or_else(|| format!("{repo} 里没有 Pilot 的备份"))?;
    Ok((tmp, m))
}

/// 看一眼备份里有什么（时间、来自哪台电脑、几个站点），不改任何东西。
pub async fn preview(data_dir: &Path, conns: &crate::conns::ConnStore, conn_id: &str, repo: &str) -> Result<Manifest, String> {
    let (tmp, m) = clone_backup(data_dir, conns, conn_id, repo).await?;
    let _ = std::fs::remove_dir_all(tmp);
    Ok(m)
}

/// 从备份恢复：要被覆盖的文件先挪到 before-restore-<时间>/，列表按 id 合并，其余覆盖。返回留底目录。调用方随后重启 Pilot。
pub async fn restore(data_dir: &Path, conns: &crate::conns::ConnStore, conn_id: &str, repo: &str) -> Result<String, String> {
    let (tmp, _) = clone_backup(data_dir, conns, conn_id, repo).await?;
    let r = apply(data_dir, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    r
}

fn apply(data_dir: &Path, tmp: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    walk(tmp, &mut files);
    let before = data_dir.join(format!("before-restore-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    for f in &files {
        let rel = f.strip_prefix(tmp).map_err(|e| e.to_string())?;
        let rel_s = rel.to_string_lossy().replace('\\', "/");
        if rel_s == "manifest.json" || rel_s == "README.md" {
            continue;
        }
        let dst = data_dir.join(rel);
        if dst.is_file() {
            let keep = before.join(rel);
            if let Some(d) = keep.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&dst, &keep).map_err(|e| format!("留底 {rel_s} 失败：{e}"))?;
        }
        if let Some(d) = dst.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        if MERGE_LISTS.contains(&rel_s.as_str()) && dst.is_file() {
            let read = |p: &Path| std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<Vec<serde_json::Value>>(&b).ok());
            if let (Some(cur), Some(bak)) = (read(&dst), read(f)) {
                let merged = merge_by_id(&cur, &bak);
                std::fs::write(&dst, serde_json::to_vec_pretty(&merged).unwrap_or_default()).map_err(|e| format!("写入 {rel_s} 失败：{e}"))?;
                continue;
            }
        }
        std::fs::copy(f, &dst).map_err(|e| format!("写入 {rel_s} 失败：{e}"))?;
    }
    Ok(before.to_string_lossy().into_owned())
}

// ───────────────────────── 命令 ─────────────────────────

#[derive(Serialize)]
pub struct Status {
    pub settings: Settings,
    pub running: bool,
    /// 现在备份的话有多少文件、多大
    pub files: usize,
    pub bytes: u64,
}

#[tauri::command]
pub fn backup_status(state: tauri::State<'_, crate::AppState>) -> Status {
    let files = collect(&state.data_dir);
    let bytes = files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
    Status { settings: load(&state.data_dir), running: RUNNING.load(Ordering::SeqCst), files: files.len(), bytes }
}

#[tauri::command]
pub fn backup_set(state: tauri::State<'_, crate::AppState>, enabled: bool, conn_id: String) -> Status {
    let mut s = load(&state.data_dir);
    if s.conn_id != conn_id {
        s.repo.clear(); // 换了账号：仓库跟着换
    }
    s.enabled = enabled;
    s.conn_id = conn_id;
    save(&state.data_dir, &s);
    backup_status(state)
}

#[tauri::command]
pub async fn backup_now(state: tauri::State<'_, crate::AppState>) -> Result<String, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("正在备份，等它结束".into());
    }
    let r = run(&state.data_dir, &state.conns).await;
    RUNNING.store(false, Ordering::SeqCst);
    r
}

#[tauri::command]
pub async fn backup_preview(state: tauri::State<'_, crate::AppState>, conn_id: String, repo: String) -> Result<Manifest, String> {
    preview(&state.data_dir, &state.conns, &conn_id, &repo).await
}

#[tauri::command]
pub async fn backup_restore(state: tauri::State<'_, crate::AppState>, conn_id: String, repo: String) -> Result<String, String> {
    if state.sites.list().iter().any(|s| state.site_jobs.running(&s.id)) {
        return Err("有运营作业正在跑，等它们结束再恢复".into());
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("正在备份，等它结束".into());
    }
    let r = restore(&state.data_dir, &state.conns, &conn_id, &repo).await;
    RUNNING.store(false, Ordering::SeqCst);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_settings_and_records_but_not_caches() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let w = |p: &str| {
            let f = d.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, "[]").unwrap();
        };
        for p in ["sites.json", "connections.json", "conversations.json", "managed/config.json", "managed/digests/2026-10-27.md", "sites/a/queue.json", "sites/a/ai_usage.jsonl", "sites/a/plans/2026-10-23.json", "sites/a/cache/summary.json", "sites/a/queue.json.tmp", "sites/a/deploy/index.html", "sites/a/x.corrupt-1.json"] {
            w(p);
        }
        let got: Vec<String> = collect(d).iter().map(|p| p.strip_prefix(d).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(got, vec!["connections.json", "managed/config.json", "managed/digests/2026-10-27.md", "sites/a/ai_usage.jsonl", "sites/a/plans/2026-10-23.json", "sites/a/queue.json", "sites.json"]);
    }

    #[test]
    fn restore_merges_lists_and_keeps_a_copy_of_what_it_replaces() {
        let t = tempfile::tempdir().unwrap();
        let (d, b) = (t.path().join("data"), t.path().join("bak"));
        std::fs::create_dir_all(d.join("sites/a")).unwrap();
        std::fs::create_dir_all(b.join("sites/a")).unwrap();
        std::fs::write(d.join("connections.json"), r#"[{"id":"new","name":"这台电脑新加的"},{"id":"gh","name":"旧名字"}]"#).unwrap();
        std::fs::write(b.join("connections.json"), r#"[{"id":"gh","name":"备份里的"}]"#).unwrap();
        std::fs::write(d.join("sites/a/queue.json"), "现在的").unwrap();
        std::fs::write(b.join("sites/a/queue.json"), "备份的").unwrap();
        std::fs::write(b.join("manifest.json"), "{}").unwrap();
        let before = PathBuf::from(apply(&d, &b).unwrap());
        let conns: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(d.join("connections.json")).unwrap()).unwrap();
        assert_eq!(conns.iter().map(|c| c["name"].as_str().unwrap()).collect::<Vec<_>>(), vec!["备份里的", "这台电脑新加的"]);
        assert_eq!(std::fs::read_to_string(d.join("sites/a/queue.json")).unwrap(), "备份的");
        assert_eq!(std::fs::read_to_string(before.join("sites/a/queue.json")).unwrap(), "现在的");
        assert!(!d.join("manifest.json").exists());
    }
}
