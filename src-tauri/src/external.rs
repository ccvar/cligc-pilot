//! 外部改动：不经 Pilot 运营流程进到站点仓库的提交（手改后推送、CI 机器人、别的工具）。
//!
//! 效果归因（「等数据」拿上线前后比）、1 小时 / 24 小时复查都默认线上只有 Pilot 自己的改动；外面改了同一个页面，
//! 涨跌算到谁头上就分不清，别人改坏的也可能报到 Pilot 头上（cexgc 的 GitHub Actions 夜里用 origin/main 把 Pilot
//! 的上线盖回去过，Pilot 完全不知道）。所以每 10 分钟看一遍每个站点的仓库：主分支（本地的和 origin 的）上新出现、
//! 又不是 Pilot 做的提交，记进 `sites/<id>/external.json`，列在「最近上线」里；碰到 Pilot 最近改过的页面 / 文件的，
//! 在对应的上线条目上标出来（见 `touching`）。
//!
//! 哪些是 Pilot 做的：`site_git::git_commit_ok` 每次提交都把 sha 记在仓库的 .git/pilot-commits 里（见 `own_commits`），
//! 加上队列里记的合并提交、部署记录里的提交；更早的历史没有这份记录，按 Pilot 固定的提交说明兜底（`own_message`）。
//!
//! 不走 git 的发布（Cloudflare 后台、别处的 wrangler、CI 直传）仓库里看不到：每 30 分钟再看一遍 Cloudflare 上这个
//! 站点（Pages 项目或 Worker）的生产部署列表，不是 Pilot 上线作业传的、也不是 Pilot 界面上回滚 / 重跑的，记成一条
//! 「外部部署」（见 `check_deploys`）。这种部署换掉的是整个站点，Pilot 不知道它改了什么，所以它之前上线的条目都算碰到了。
//! VPS 站点没有部署列表，只有 git 来源。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const DAY: u64 = 86_400;
/// 多久看一遍
const EVERY: u64 = 10 * 60;
/// 第一次看多久以前的提交（和「最近上线」的窗口一样）
const FIRST_WINDOW: u64 = 30 * DAY;
/// 记录留多久、每个站点最多留多少条
const KEEP: u64 = 90 * DAY;
const MAX: usize = 300;
/// 一条提交最多记多少个文件
const MAX_FILES: usize = 200;
/// Cloudflare 部署列表多久看一遍（部署比提交少，也别老打接口）
const CF_EVERY: u64 = 30 * 60;
/// Pilot 上线作业结束后这么久之内出现的部署还算 Pilot 的（上传完 Cloudflare 记上要一会儿，作业时长也只是大概）
const DEPLOY_SLACK: u64 = 10 * 60;
/// Pilot 界面上回滚 / 重跑产生的部署 id 留多少个
const MAX_OWN_DEPLOYS: usize = 50;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExternalChange {
    pub sha: String,
    /// 提交时间
    pub at: u64,
    /// Pilot 发现它的时间
    pub seen_at: u64,
    pub author: String,
    /// 提交说明的第一行
    pub message: String,
    /// 在哪个分支上看到的：main 或 origin/main
    #[serde(default)]
    pub on: String,
    pub files: Vec<String>,
    /// 按文件路径猜的页面网址（只认常见的内容目录，见 `guess_pages`）；猜不出就空
    pub pages: Vec<String>,
    /// 空＝仓库里的提交（sha 是提交号）；deploy＝Cloudflare 上不经 Pilot 的一次生产部署（sha 是部署 id，没有文件清单）
    #[serde(default)]
    pub kind: String,
    /// Cloudflare 部署的 id、地址、触发方式。提交记录被部署列表对上了也填上：说明这个提交已经上线
    #[serde(default)]
    pub deploy_id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub trigger: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExternalLog {
    pub checked_at: u64,
    pub changes: Vec<ExternalChange>,
    /// 上次看 Cloudflare 部署列表的时间
    #[serde(default)]
    pub cf_checked_at: u64,
    /// Pilot 界面上回滚 / 重跑产生的部署 id：不在上线记录里，别算成外部的
    #[serde(default)]
    pub own_deploys: Vec<String>,
}

/// 「最近上线」里标在上线条目上的一条外部改动
#[derive(Clone, Debug, Serialize)]
pub struct ExternalBrief {
    pub sha: String,
    pub at: u64,
    pub author: String,
    pub message: String,
}

pub fn path(site_dir: &Path) -> PathBuf {
    site_dir.join("external.json")
}

pub fn load(site_dir: &Path) -> ExternalLog {
    std::fs::read(path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(site_dir: &Path, log: &ExternalLog) {
    let _ = crate::sites::write_json(&path(site_dir), log, true);
}

/// Pilot 界面上回滚 / 重跑了一次部署：记下它的 id，部署列表里再看到不算外部的
pub fn note_own_deploy(site_dir: &Path, id: &str) {
    if id.is_empty() {
        return;
    }
    let mut log = load(site_dir);
    if !log.own_deploys.iter().any(|d| d == id) {
        log.own_deploys.insert(0, id.to_string());
        log.own_deploys.truncate(MAX_OWN_DEPLOYS);
        save(site_dir, &log);
    }
}

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 调度循环每分钟叫一次；每个站点隔 `EVERY` 真的看一遍仓库、隔 `CF_EVERY` 看一遍 Cloudflare 的部署列表，看完有新记录就通知前端刷新。
pub fn schedule(app: &tauri::AppHandle) {
    use tauri::Manager;
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<crate::AppState>();
        let now = crate::now_secs();
        let mut found = 0usize;
        for site in st.sites.list() {
            let site_dir = st.sites.data_dir(&site.id);
            let log = load(&site_dir);
            let repo = repo_of(&st, &site).await;
            if log.checked_at + EVERY <= now {
                if let Some((dir, token)) = &repo {
                    found += check_site(&site_dir, dir, token, &site.default_branch, &site.domain).await;
                }
            }
            // 仓库先看、部署列表后看：带提交号的部署先能对上刚记下的外部提交
            if log.cf_checked_at + CF_EVERY <= now {
                found += check_deploys(&st, &site, &site_dir, repo.as_ref().map(|(d, _)| d.as_path())).await;
            }
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        if found > 0 {
            use tauri::Emitter;
            let _ = app.emit("external-changed", ());
        }
    });
}

/// 站点仓库在本机的位置和 fetch 用的凭据：本地站点是用户的文件夹（得是 git 仓库）；GitHub 站点是连接工作区里的检出，
/// 还没检出过的不在这里 clone（运营任务跑起来时自然会有）。
/// 钥匙串是同步调用、macOS 还可能弹授权框等人点：用 get_key_async 放到阻塞线程池，别把 tokio 的工作线程卡住（卡住了整个调度循环都停）。
async fn repo_of(st: &crate::AppState, site: &crate::sites::Site) -> Option<(PathBuf, String)> {
    match site.source.as_str() {
        "local" => {
            let p = PathBuf::from(&site.local_dir);
            (p.join(".git").exists()).then(|| (p, String::new()))
        }
        "github" => {
            let gh = st.conns.get(&site.gh_conn_id).ok()?;
            let dir = crate::github::repo_dir(&gh.skill_dir, &site.repo).ok()?;
            if !dir.join(".git").exists() {
                return None;
            }
            Some((dir, crate::keychain::get_key_async(&gh.id).await.unwrap_or_default()))
        }
        _ => None,
    }
}

/// 看一个站点的仓库，返回新记下的外部提交数。
async fn check_site(site_dir: &Path, dir: &Path, token: &str, branch: &str, domain: &str) -> usize {
    let now = crate::now_secs();
    let mut log = load(site_dir);
    // 第一次看最近 30 天；之后从上次看的时间往前多留一天，提交时间比推送时间早的也能赶上
    let since = if log.checked_at == 0 { now.saturating_sub(FIRST_WINDOW) } else { log.checked_at.saturating_sub(DAY) };
    log.checked_at = now;
    let branch = if branch.is_empty() { "main" } else { branch };
    let known = known_commits(site_dir, Some(dir)).await;
    // 已记下的按现在的规则再过一遍：Pilot 的提交、建站的首个提交、只动 .pilot/ 的，规则收紧后不用等它老化
    let roots: HashSet<String> = crate::site_git::git_ok(dir, &["rev-list", "--max-parents=0", "--all"], "").await.map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default();
    log.changes.retain(|c| c.kind == "deploy" || (!known.contains(&c.sha) && !roots.contains(&c.sha) && !own_message(&c.message) && !c.files.iter().all(|f| f.starts_with(".pilot/") || f.contains("/.pilot/"))));
    let seen: HashSet<String> = log.changes.iter().map(|c| c.sha.clone()).collect();
    let fresh = scan(dir, token, branch, since, &known, &seen, domain).await;
    let n = fresh.len();
    log.changes.extend(fresh);
    log.changes.sort_by(|a, b| b.at.cmp(&a.at));
    log.changes.retain(|c| c.at + KEEP > now);
    log.changes.truncate(MAX);
    save(site_dir, &log);
    n
}

/// Cloudflare 上这个站点的生产部署：不是 Pilot 的记成外部部署。返回新记的（含对上了提交记录的）条数。
/// 查不到（没网、没权限、项目没了）不记时间，下次再看。
async fn check_deploys(st: &crate::AppState, site: &crate::sites::Site, site_dir: &Path, dir: Option<&Path>) -> usize {
    if site.deploy.project.is_empty() || !matches!(site.deploy.mode.as_str(), "direct" | "workers" | "pages_git") {
        return 0;
    }
    let Ok(cf) = st.conns.get(&site.deploy.cf_conn_id) else { return 0 };
    let Ok(token) = crate::keychain::get_key_async(&cf.id).await else { return 0 };
    let deps = if site.deploy.mode == "workers" {
        crate::cf::list_worker_deployments(&token, &cf.account_id, &site.deploy.project).await
    } else {
        crate::cf::list_deployments(&token, &cf.account_id, &site.deploy.project).await
    };
    let Ok(deps) = deps else { return 0 };
    let now = crate::now_secs();
    let mut log = load(site_dir);
    let since = if log.cf_checked_at == 0 { now.saturating_sub(FIRST_WINDOW) } else { log.cf_checked_at.saturating_sub(DAY) };
    log.cf_checked_at = now;
    let known = known_commits(site_dir, dir).await;
    let history = crate::site_deploy::load_history(site_dir);
    let n = fold_deploys(&mut log, &deps, &history, &known, since, now, &site.deploy.mode);
    log.changes.sort_by(|a, b| b.at.cmp(&a.at));
    log.changes.retain(|c| c.at + KEEP > now);
    log.changes.truncate(MAX);
    save(site_dir, &log);
    n
}

/// 把部署列表并进记录：生产环境、成功了的、`since` 之后的；Pilot 的跳过（见 `own_deploy`）；带提交号、提交已经记为
/// 外部提交的，补到那条记录上（它已经上线了），不另记；其余记成一条「外部部署」。
fn fold_deploys(log: &mut ExternalLog, deps: &[crate::cf::PagesDeployment], history: &[crate::site_deploy::DeployRecord], known: &HashSet<String>, since: u64, now: u64, mode: &str) -> usize {
    let mut n = 0;
    for d in deps {
        let at = parse_time(&d.created_on);
        if d.id.is_empty() || at < since || d.environment != "production" || d.status != "success" {
            continue;
        }
        if log.own_deploys.iter().any(|x| x == &d.id) || own_deploy(d, at, history, known) {
            continue;
        }
        if log.changes.iter().any(|c| c.sha == d.id || c.deploy_id == d.id) {
            continue;
        }
        if !d.commit.is_empty() {
            if let Some(c) = log.changes.iter_mut().find(|c| c.kind.is_empty() && same_commit(&c.sha, &d.commit)) {
                c.deploy_id = d.id.clone();
                c.url = d.url.clone();
                c.trigger = d.trigger.clone();
                n += 1;
                continue;
            }
        }
        let (author, message) = describe_deploy(d, mode);
        log.changes.push(ExternalChange {
            sha: d.id.clone(),
            at,
            seen_at: now,
            author,
            message,
            on: if mode == "workers" { "Cloudflare Workers".into() } else { "Cloudflare Pages".into() },
            files: Vec::new(),
            pages: Vec::new(),
            kind: "deploy".into(),
            deploy_id: d.id.clone(),
            url: d.url.clone(),
            trigger: d.trigger.clone(),
        });
        n += 1;
    }
    n
}

/// 是不是 Pilot 自己上线的：Pages 直传的部署地址带部署 id 的前 8 位，上线记录里记着；绑定 GitHub 的看提交号是不是
/// Pilot 合并 / 提交的；都对不上再按时间——落在 Pilot 某次上线作业开始到结束后 `DEPLOY_SLACK` 之内的（Workers 只有这一条）。
fn own_deploy(d: &crate::cf::PagesDeployment, at: u64, history: &[crate::site_deploy::DeployRecord], known: &HashSet<String>) -> bool {
    let short = d.id.get(..8).unwrap_or(&d.id);
    if history.iter().any(|r| !r.url.is_empty() && r.url.trim_start_matches("https://").starts_with(&format!("{short}."))) {
        return true;
    }
    if !d.commit.is_empty() && known.iter().any(|k| same_commit(k, &d.commit)) {
        return true;
    }
    history.iter().any(|r| at + 60 >= r.ts && at <= r.ts + r.duration + DEPLOY_SLACK)
}

/// 提交号一个是另一个的前缀就算同一个（部署列表里的可能是短的）
fn same_commit(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a.starts_with(b) || b.starts_with(a))
}

fn parse_time(rfc3339: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339).map(|t| t.timestamp().max(0) as u64).unwrap_or(0)
}

/// 外部部署在列表里怎么写：（「提交人」那格放触发方式，标题放提交说明或一句说明）
fn describe_deploy(d: &crate::cf::PagesDeployment, mode: &str) -> (String, String) {
    let msg = d.message.trim();
    if mode == "workers" {
        // Workers 的 message 没有注记时是 source（api / dash / wrangler / terraform）
        // 拉丁字母后面留个空格，拼出来的句子才读得通
        let src = match msg {
            "dash" => "在 Cloudflare 后台",
            "wrangler" => "用 wrangler ",
            "api" => "通过 API ",
            "terraform" => "用 Terraform ",
            _ => "",
        };
        let known_src = !src.is_empty();
        let src = if known_src { src } else { "在 Cloudflare 上" };
        return match d.trigger.as_str() {
            "rollback" => ("回滚".into(), format!("{src}把 Worker 回滚到之前的版本")),
            _ if !msg.is_empty() && !known_src => ("上传".into(), msg.chars().take(200).collect()),
            _ => ("上传".into(), format!("{src}部署了一次 Worker")),
        };
    }
    match d.trigger.as_str() {
        "github:push" => ("GitHub 推送".into(), if msg.is_empty() { "Cloudflare 从 GitHub 拉代码构建了一次".into() } else { msg.chars().take(200).collect() }),
        "ad_hoc" => ("上传".into(), if msg.is_empty() { "直接上传到 Cloudflare Pages 的一次部署（wrangler 或后台）".into() } else { msg.chars().take(200).collect() }),
        t => (if t.is_empty() { "部署".into() } else { t.to_string() }, if msg.is_empty() { "Cloudflare Pages 上的一次部署".into() } else { msg.chars().take(200).collect() }),
    }
}

/// Pilot 自己做的提交：仓库里记的（.git/pilot-commits；没有仓库就没有这份）、队列里的合并提交、部署记录里的提交
async fn known_commits(site_dir: &Path, dir: Option<&Path>) -> HashSet<String> {
    let mut known = match dir {
        Some(d) => crate::site_git::own_commits(d).await,
        None => HashSet::new(),
    };
    known.extend(crate::site_ops::load_queue(site_dir).into_iter().flat_map(|i| [i.merge_sha, i.head_sha]).filter(|s| !s.is_empty()));
    known.extend(crate::site_deploy::load_history(site_dir).into_iter().map(|d| d.commit).filter(|s| !s.is_empty()));
    known
}

/// 仓库没配 git 身份时 Pilot 提交用的名字（见 site_git::identity_flags）
fn own_identity(author: &str, email: &str) -> bool {
    author == "Cligc Pilot" || email == "pilot@localhost"
}

/// 没有 sha 记录的老提交：Pilot 固定的提交说明（运营合并 / 回滚、上线前替用户提交、还原生成文件、加动态功能）
fn own_message(subject: &str) -> bool {
    let s = subject.trim();
    ["ops: ", "Revert: ops: ", "Revert \"ops: ", "提交已上线的改动：", "提交站点改动：", "动态功能："].iter().any(|p| s.starts_with(p)) || s == "还原构建时自动生成的数据文件"
}

/// 主分支（本地的和 origin 的）上 `since` 之后、不是 Pilot 做的、还没记过的提交。
/// 先 fetch 一次 origin 的主分支（没有远端就跳过），本地和远端各看一遍，同一个提交只记一次、记先看到的那个分支。
async fn scan(dir: &Path, token: &str, branch: &str, since: u64, known: &HashSet<String>, seen: &HashSet<String>, domain: &str) -> Vec<ExternalChange> {
    use crate::site_git::{git, git_ok};
    if git(dir, &["remote", "get-url", "origin"], "").await.is_ok_and(|o| o.ok) {
        let _ = git(dir, &["fetch", "-q", "origin", branch], token).await;
    }
    let now = crate::now_secs();
    let mut out: Vec<ExternalChange> = Vec::new();
    for r in [branch.to_string(), format!("origin/{branch}")] {
        if !git(dir, &["rev-parse", "--verify", "-q", &format!("{r}^{{commit}}")], "").await.is_ok_and(|o| o.ok) {
            continue;
        }
        // 不用 git 自己的 --since：时间戳相同的一串提交它会不稳定地漏掉最早的那个。按条数取，自己按时间截
        let Ok(lines) = git_ok(dir, &["log", "--first-parent", "-n", "500", "--format=%H%x1f%an%x1f%ae%x1f%ct%x1f%P%x1f%s", &r], "").await else { continue };
        for l in lines.lines() {
            let mut it = l.split('\x1f');
            let (Some(sha), Some(author), Some(email), Some(ct), Some(parents), Some(subject)) = (it.next(), it.next(), it.next(), it.next(), it.next(), it.next()) else { continue };
            let at: u64 = ct.parse().unwrap_or(0);
            if at < since {
                break;
            }
            // 仓库的第一个提交是建站，不是对线上站点的改动；没配身份时 Pilot 用自己的名字提交
            if parents.trim().is_empty() || known.contains(sha) || seen.contains(sha) || out.iter().any(|c| c.sha == sha) || own_message(subject) || own_identity(author, email) {
                continue;
            }
            // 合并提交只看它相对第一个父提交带进来的东西；--root 让仓库的第一个提交也列得出文件
            let files: Vec<String> = git_ok(dir, &["diff-tree", "--no-commit-id", "--name-only", "-r", "-m", "--first-parent", "--root", sha], "")
                .await
                .map(|s| s.lines().filter(|f| !f.is_empty()).take(MAX_FILES).map(str::to_string).collect())
                .unwrap_or_default();
            // 只动了 .pilot/（Pilot 自己的记录：待办、站点说明）的不算改站点
            if files.is_empty() || files.iter().all(|f| f.starts_with(".pilot/") || f.contains("/.pilot/")) {
                continue;
            }
            out.push(ExternalChange {
                sha: sha.to_string(),
                at,
                seen_at: now,
                author: author.to_string(),
                message: subject.chars().take(200).collect(),
                on: r.clone(),
                pages: guess_pages(&files, domain),
                files,
                ..Default::default()
            });
        }
    }
    out
}

/// 按文件路径猜页面网址：常见静态站的内容目录（src/content、src/pages、content、legacy/site、public…）去掉前缀和后缀就是路径。
/// 猜不准的（组件、布局、下划线开头的内部文件）不猜；没有域名就空。只用来和 Pilot 条目的页面对上，不拿去验收。
pub fn guess_pages(files: &[String], domain: &str) -> Vec<String> {
    if domain.is_empty() {
        return Vec::new();
    }
    const ROOTS: &[&str] = &["src/content/", "src/pages/", "content/", "legacy/site/", "site/", "public/", "pages/", "posts/"];
    const EXT: &[&str] = &[".md", ".mdx", ".markdown", ".html", ".htm", ".astro"];
    let mut out: Vec<String> = Vec::new();
    for f in files {
        let f = f.trim_start_matches("./");
        let Some(ext) = EXT.iter().find(|e| f.ends_with(*e)) else { continue };
        let name = f.rsplit('/').next().unwrap_or(f);
        if name.starts_with('_') || f.split('/').any(|s| matches!(s, "components" | "layouts" | "partials" | "includes" | "_includes" | "templates" | "node_modules" | "dist")) {
            continue;
        }
        let Some(rel) = ROOTS.iter().find_map(|r| f.strip_prefix(r)) else { continue };
        let stem = rel.strip_suffix(ext).unwrap_or(rel);
        let stem = stem.strip_suffix("/index").or_else(|| (stem == "index").then_some("")).unwrap_or(stem);
        let url = if stem.is_empty() { format!("https://{domain}/") } else { format!("https://{domain}/{stem}/") };
        if !out.contains(&url) {
            out.push(url);
        }
        if out.len() >= 20 {
            break;
        }
    }
    out
}

/// 一条上线之后、碰到它改过的文件或页面的外部改动（新的在前，最多 5 条）：效果就看不准了。
/// 外部部署没有文件清单、换的是整个站点，在它之前上线的都算碰到。
pub fn touching(log: &ExternalLog, item: &crate::site_ops::QueueItem) -> Vec<ExternalBrief> {
    if item.deployed_at == 0 {
        return Vec::new();
    }
    let pages: Vec<&String> = item.pages.iter().chain(&item.new_pages).chain(std::iter::once(&item.opp.page)).filter(|p| p.starts_with("https://")).collect();
    log.changes
        .iter()
        .filter(|c| c.at > item.deployed_at)
        .filter(|c| (c.kind == "deploy" && c.files.is_empty()) || c.files.iter().any(|f| item.files.contains(f)) || c.pages.iter().any(|p| pages.iter().any(|q| crate::site_verify::same_page(p, q))))
        .take(5)
        .map(|c| ExternalBrief { sha: c.sha.clone(), at: c.at, author: c.author.clone(), message: c.message.clone() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_guessed_only_from_content_paths() {
        let files: Vec<String> = ["src/content/posts/kyc-failed.md", "src/pages/index.astro", "src/pages/vi/gioi-thieu/index.astro", "src/components/Nav.astro", "legacy/site/en/posts/okx-fees/index.html", "public/_redirects", "src/layouts/Base.astro", "src/content/posts/_draft.md"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(guess_pages(&files, "x.com"), vec!["https://x.com/posts/kyc-failed/", "https://x.com/", "https://x.com/vi/gioi-thieu/", "https://x.com/en/posts/okx-fees/"]);
        assert!(guess_pages(&files, "").is_empty());
    }

    #[test]
    fn only_changes_after_the_ship_that_touch_its_files_or_pages_count() {
        let item = crate::site_ops::QueueItem { deployed_at: 1000, files: vec!["src/content/posts/a.md".into()], pages: vec!["https://x.com/posts/b/".into()], ..Default::default() };
        let c = |at: u64, files: &[&str], pages: &[&str]| ExternalChange { sha: format!("{at}"), at, files: files.iter().map(|s| s.to_string()).collect(), pages: pages.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        let log = ExternalLog {
            checked_at: 0,
            changes: vec![c(3000, &["src/content/posts/a.md"], &[]), c(2000, &["src/content/posts/c.md"], &["https://x.com/posts/b"]), c(1500, &["src/content/posts/z.md"], &["https://x.com/posts/z/"]), c(500, &["src/content/posts/a.md"], &[])],
            ..Default::default()
        };
        let hit: Vec<String> = touching(&log, &item).iter().map(|b| b.sha.clone()).collect();
        assert_eq!(hit, vec!["3000", "2000"]);
        assert!(touching(&log, &crate::site_ops::QueueItem::default()).is_empty(), "没上线的不谈");
        // 外部部署换的是整个站点：在它之前上线的都算碰到，之前的部署不算
        let mut log = log;
        log.changes.push(ExternalChange { sha: "dep".into(), at: 2500, kind: "deploy".into(), ..Default::default() });
        log.changes.push(ExternalChange { sha: "old-dep".into(), at: 900, kind: "deploy".into(), ..Default::default() });
        let hit: Vec<String> = touching(&log, &item).iter().map(|b| b.sha.clone()).collect();
        assert_eq!(hit, vec!["3000", "2000", "dep"]);
    }

    #[test]
    fn cloudflare_deploys_outside_pilot_are_recorded() {
        use crate::cf::PagesDeployment;
        use crate::site_deploy::DeployRecord;
        let iso = |t: u64| chrono::DateTime::from_timestamp(t as i64, 0).unwrap().to_rfc3339();
        let dep = |id: &str, at: u64, trigger: &str, commit: &str| PagesDeployment { id: id.into(), url: format!("https://{}.bgvar.pages.dev", &id[..8]), environment: "production".into(), commit: commit.into(), message: if commit.is_empty() { String::new() } else { "Update fees".into() }, status: "success".into(), stage: "deploy".into(), created_on: iso(at), trigger: trigger.into(), ..Default::default() };
        let history = vec![
            DeployRecord { ts: 1000, duration: 100, url: "https://892f5f47.bgvar.pages.dev".into(), ok: true, ..Default::default() },
            DeployRecord { ts: 5000, duration: 60, url: String::new(), ok: false, ..Default::default() },
        ];
        let known: HashSet<String> = ["pilot000000000000000000000000000000000000".to_string()].into_iter().collect();
        let mut log = ExternalLog { changes: vec![ExternalChange { sha: "abc1234567890000000000000000000000000000".into(), at: 9400, files: vec!["a.md".into()], ..Default::default() }], own_deploys: vec!["uirollbk-0000".into()], ..Default::default() };
        let deps = vec![
            dep("892f5f47-aaaa", 1050, "ad_hoc", ""),               // Pilot 直传：地址对得上
            dep("deadbeef-1111", 5030, "ad_hoc", ""),               // 落在 Pilot 作业时间里（没成功的那次）
            dep("cafe0001-2222", 9000, "ad_hoc", ""),               // 外部直传
            PagesDeployment { status: "failure".into(), ..dep("fa11ed00-3333", 9100, "ad_hoc", "") }, // 没成功，线上没变
            PagesDeployment { environment: "preview".into(), ..dep("preview0-4444", 9200, "github:push", "") },
            dep("gh000000-5555", 9500, "github:push", "abc1234567890000000000000000000000000000"), // 对上已记的外部提交
            dep("gh000000-6666", 9600, "github:push", "pilot000000000000000000000000000000000000"), // Pilot 合并的
            dep("uirollbk-0000", 9700, "ad_hoc", ""),               // 界面上回滚的
            dep("00000000-7777", 100, "ad_hoc", ""),                // 太早
        ];
        let n = fold_deploys(&mut log, &deps, &history, &known, 500, 10_000, "direct");
        assert_eq!(n, 2);
        let mut ids: Vec<(&str, &str)> = log.changes.iter().map(|c| (c.sha.as_str(), c.kind.as_str())).collect();
        ids.sort();
        assert_eq!(ids, vec![("abc1234567890000000000000000000000000000", ""), ("cafe0001-2222", "deploy")]);
        let c = log.changes.iter().find(|c| c.sha.starts_with("abc")).unwrap();
        assert_eq!(c.deploy_id, "gh000000-5555", "提交记录补上了部署");
        let d = log.changes.iter().find(|c| c.kind == "deploy").unwrap();
        assert_eq!((d.author.as_str(), d.on.as_str(), d.url.as_str()), ("上传", "Cloudflare Pages", "https://cafe0001.bgvar.pages.dev"));
        // 再看一遍不重复
        assert_eq!(fold_deploys(&mut log, &deps, &history, &known, 500, 10_000, "direct"), 0);
        // Workers：回滚和后台上传的说法
        let w = |trigger: &str, msg: &str| PagesDeployment { trigger: trigger.into(), message: msg.into(), ..Default::default() };
        assert_eq!(describe_deploy(&w("rollback", "dash"), "workers"), ("回滚".into(), "在 Cloudflare 后台把 Worker 回滚到之前的版本".into()));
        assert_eq!(describe_deploy(&w("upload", "wrangler"), "workers"), ("上传".into(), "用 wrangler 部署了一次 Worker".into()));
        assert_eq!(describe_deploy(&w("upload", "fix nav"), "workers"), ("上传".into(), "fix nav".into()));
    }

    #[tokio::test]
    async fn pilot_commits_are_not_external_but_bots_and_hand_edits_are() {
        let t = tempfile::tempdir().unwrap();
        let (repo, bare) = (t.path().join("site"), t.path().join("origin.git"));
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&bare).unwrap();
        let g = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap().status.success(), "{args:?}");
        crate::site_git::git_ok(&bare, &["init", "-q", "--bare", "-b", "main"], "").await.unwrap();
        g(&["init", "-q", "-b", "main"]);
        g(&["config", "user.name", "ccvar"]);
        g(&["config", "user.email", "c@c"]);
        std::fs::create_dir_all(repo.join("src/content/posts")).unwrap();
        std::fs::write(repo.join("src/content/posts/a.md"), "a").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "init"]);
        // Pilot 的 squash 合并：用户配了身份，作者名分不出来，靠 .git/pilot-commits
        std::fs::write(repo.join("src/content/posts/a.md"), "ai").unwrap();
        crate::site_git::git_commit_ok(&repo, &["commit", "-qam", "ops: improve /posts/a/"]).await.unwrap();
        // 老的 Pilot 提交（没有 sha 记录）：按提交说明
        std::fs::write(repo.join("src/content/posts/a.md"), "ai2").unwrap();
        g(&["commit", "-qam", "Revert: ops: improve /posts/a/"]);
        // 用户手改
        std::fs::write(repo.join("src/content/posts/b.md"), "hand").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "write b"]);
        g(&["remote", "add", "origin", bare.to_str().unwrap()]);
        g(&["push", "-q", "origin", "main"]);
        // 机器人直接推到远端（本地没有这个提交）
        let bot = t.path().join("bot");
        crate::site_git::git_ok(t.path(), &["clone", "-q", bare.to_str().unwrap(), "bot"], "").await.unwrap();
        std::fs::write(bot.join("fees.json"), "{}").unwrap();
        crate::site_git::git_ok(&bot, &["add", "-A"], "").await.unwrap();
        crate::site_git::git_ok(&bot, &["-c", "user.name=github-actions[bot]", "-c", "user.email=b@b", "commit", "-qm", "Update withdrawal fees"], "").await.unwrap();
        crate::site_git::git_ok(&bot, &["push", "-q", "origin", "main"], "").await.unwrap();

        let known = crate::site_git::own_commits(&repo).await;
        assert_eq!(known.len(), 1);
        let got = scan(&repo, "", "main", 0, &known, &HashSet::new(), "x.com").await;
        let mut msgs: Vec<(&str, &str, &str)> = got.iter().map(|c| (c.message.as_str(), c.on.as_str(), c.author.as_str())).collect();
        msgs.sort();
        assert_eq!(msgs, vec![("Update withdrawal fees", "origin/main", "github-actions[bot]"), ("write b", "main", "ccvar")], "首个提交是建站，不算");
        let b = got.iter().find(|c| c.message == "write b").unwrap();
        assert_eq!(b.files, vec!["src/content/posts/b.md"]);
        assert_eq!(b.pages, vec!["https://x.com/posts/b/"]);
        // 记过的不再记
        let seen: HashSet<String> = got.iter().map(|c| c.sha.clone()).collect();
        assert!(scan(&repo, "", "main", 0, &known, &seen, "x.com").await.is_empty());
    }
}
