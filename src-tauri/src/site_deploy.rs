//! 站点构建与上线：由 Pilot 自己执行（模型拿不到 Cloudflare Token）。
//! 流程：准备代码 → 安装依赖 → 构建 → 检查（任何一条失败都不上传）→ 上传（wrangler）→ IndexNow。
//! GitHub 来源在 `<站点数据>/deploy` 的工作树里构建默认分支的最新提交；本地来源直接在用户文件夹里构建。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::conns::{ConnStore, Connection};
use crate::site_jobs::Job;
use crate::sites::Site;

const STEP_TIMEOUT: Duration = Duration::from_secs(20 * 60);

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct DeployRecord {
    pub id: String,
    pub ts: u64,
    pub ok: bool,
    /// manual | ops | rollback
    #[serde(default)]
    pub trigger: String,
    pub commit: String,
    pub message: String,
    pub url: String,
    /// 本次构建产物里内容有变化的网址数（据此提交 IndexNow）。
    pub changed: usize,
    #[serde(default)]
    pub indexnow: String,
    pub error: String,
    pub duration: u64,
    #[serde(default)]
    pub dirty: bool,
    /// 线上验收的结果；attention＝不回滚、但要核对的问题。
    #[serde(default)]
    pub checks: Vec<crate::site_verify::Check>,
    #[serde(default)]
    pub attention: Vec<String>,
}

pub const DEPLOY_STEPS: &[(&str, &str)] = &[
    ("prepare", "准备代码"),
    ("install", "安装依赖"),
    ("build", "构建"),
    ("check", "检查"),
    ("upload", "上传"),
    ("verify", "线上验收"),
    ("indexnow", "提交 IndexNow"),
];

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn history_path(site_dir: &Path) -> PathBuf {
    site_dir.join("deploys.json")
}

pub fn load_history(site_dir: &Path) -> Vec<DeployRecord> {
    std::fs::read(history_path(site_dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn push_history(site_dir: &Path, rec: &DeployRecord) {
    let mut all = load_history(site_dir);
    all.insert(0, rec.clone());
    all.truncate(100);
    if let Ok(raw) = serde_json::to_vec_pretty(&all) {
        let tmp = history_path(site_dir).with_extension("json.tmp");
        if std::fs::write(&tmp, raw).is_ok() {
            let _ = std::fs::rename(&tmp, history_path(site_dir));
        }
    }
}

// ---------------------------------------------------------------------------
// 产物清单：按 HTML 文件内容哈希比对，得到「这次上线改了哪些网址」。
// ---------------------------------------------------------------------------

pub type Manifest = BTreeMap<String, String>;

fn walk_html(root: &Path, dir: &Path, out: &mut Manifest, depth: u8) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if depth < 12 && !name.starts_with('.') && name != "node_modules" {
                walk_html(root, &p, out, depth + 1);
            }
        } else if name.ends_with(".html") {
            if let (Ok(rel), Ok(bytes)) = (p.strip_prefix(root), std::fs::read(&p)) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(rel, format!("{:x}", Sha256::digest(&bytes)));
            }
        }
    }
}

pub fn build_manifest(out_dir: &Path) -> Manifest {
    let mut m = Manifest::new();
    walk_html(out_dir, out_dir, &mut m, 0);
    m
}

/// HTML 文件路径 → 线上网址（Cloudflare Pages 的规则：`a/index.html`→`/a/`，`a.html`→`/a`）。
/// 404 页与 Pages 的特殊文件不算。
pub fn html_to_path(rel: &str) -> Option<String> {
    if rel == "404.html" || rel.ends_with("/404.html") {
        return None;
    }
    if rel == "index.html" {
        return Some("/".into());
    }
    if let Some(dir) = rel.strip_suffix("/index.html") {
        return Some(format!("/{dir}/"));
    }
    rel.strip_suffix(".html").map(|p| format!("/{p}"))
}

/// 新旧清单比对：新增或内容变化的页面网址（旧清单为空＝首次上线，全部算变化）。
pub fn changed_urls(old: &Manifest, new: &Manifest, domain: &str) -> Vec<String> {
    let mut out: Vec<String> = new
        .iter()
        .filter(|(k, v)| old.get(*k) != Some(*v))
        .filter_map(|(k, _)| html_to_path(k))
        .map(|p| format!("https://{domain}{p}"))
        .collect();
    out.sort();
    out
}

fn manifest_path(site_dir: &Path) -> PathBuf {
    site_dir.join("manifest.json")
}

pub fn load_manifest(site_dir: &Path) -> Manifest {
    std::fs::read(manifest_path(site_dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_manifest(site_dir: &Path, m: &Manifest) {
    if let Ok(raw) = serde_json::to_vec(m) {
        let _ = std::fs::write(manifest_path(site_dir), raw);
    }
}

// ---------------------------------------------------------------------------

/// 找 wrangler：项目自带的优先（版本与项目一致），其次全局，最后 npx。
fn wrangler_cmd(dir: &Path) -> Result<(String, Vec<String>), String> {
    let local = dir.join("node_modules").join(".bin").join(if cfg!(windows) { "wrangler.cmd" } else { "wrangler" });
    if local.is_file() {
        return Ok((local.to_string_lossy().into_owned(), vec![]));
    }
    let global = crate::brains::resolve_bin("wrangler");
    if Path::new(&global).is_file() {
        return Ok((global, vec![]));
    }
    let npx = crate::brains::resolve_bin("npx");
    if Path::new(&npx).is_file() {
        return Ok((npx, vec!["--yes".into(), "wrangler@4".into()]));
    }
    Err("没有找到 wrangler，也没有 Node.js。请先安装 Node.js（nodejs.org），再执行 npm i -g wrangler。".into())
}

/// 从 wrangler 输出里找部署地址。
pub fn find_pages_url(log: &str) -> String {
    for word in log.split_whitespace().rev() {
        let w = word.trim_matches(|c: char| !c.is_ascii_graphic() || matches!(c, '(' | ')' | ',' | '"' | '\''));
        if w.starts_with("https://") && w.contains(".pages.dev") {
            return w.to_string();
        }
    }
    String::new()
}

/// Workers 的部署输出里找地址：优先 workers.dev，其次站点自己的域名。
pub fn find_deploy_url(log: &str, domain: &str) -> String {
    for word in log.split_whitespace() {
        let w = word.trim_matches(|c: char| !c.is_ascii_graphic() || matches!(c, '(' | ')' | ',' | '"' | '\''));
        if w.starts_with("https://") && w.contains(".workers.dev") {
            return w.to_string();
        }
    }
    if domain.is_empty() { String::new() } else { format!("https://{domain}") }
}

/// 依赖要不要装：没有 node_modules，或锁文件变了（Pilot 自己的部署目录才比对锁文件）。
fn needs_install(dir: &Path, stamp: Option<&Path>) -> bool {
    if !dir.join("package.json").exists() {
        return false;
    }
    if !dir.join("node_modules").exists() {
        return true;
    }
    let Some(stamp) = stamp else { return false };
    let lock = ["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock"]
        .iter()
        .find_map(|f| std::fs::read(dir.join(f)).ok());
    let Some(lock) = lock else { return false };
    let hash = format!("{:x}", Sha256::digest(&lock));
    std::fs::read_to_string(stamp).ok().as_deref() != Some(hash.as_str())
}

fn write_install_stamp(dir: &Path, stamp: &Path) {
    if let Some(lock) = ["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock"]
        .iter()
        .find_map(|f| std::fs::read(dir.join(f)).ok())
    {
        let _ = std::fs::write(stamp, format!("{:x}", Sha256::digest(&lock)));
    }
}

/// 构建要用的环境：站点构建变量（钥匙串）。
const PROXY_VARS: [&str; 6] = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"];

fn has_proxy_env() -> bool {
    PROXY_VARS.iter().any(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()))
}

fn without_proxy(env: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = env.iter().filter(|(k, _)| !PROXY_VARS.contains(&k.as_str())).cloned().collect();
    out.extend(PROXY_VARS.iter().map(|k| (k.to_string(), crate::site_jobs::UNSET_ENV.to_string())));
    out
}

pub fn build_env(site: &Site, code: &Path, job: &Job) -> Vec<(String, String)> {
    // 填过的配置变量 + 能自动给的（SITE_URL＝https://<站点域名>）
    let env = crate::site_config::effective_values(site, code);
    for (_, v) in &env {
        job.add_secret(v);
    }
    env
}

/// GitHub 来源：保证主检出存在并取最新，再把部署工作树切到 `origin/<分支>`。返回构建目录。
pub async fn prepare_github(site: &Site, gh: &Connection, site_dir: &Path, job: &Job) -> Result<PathBuf, String> {
    let token = crate::keychain::get_key(&gh.id)?;
    job.add_secret(&token);
    let main = crate::github::repo_dir(&gh.skill_dir, &site.repo)?;
    crate::github::ensure_checkout(&gh.skill_dir, &main.to_string_lossy(), &token, &gh.login, &gh.account_id).await?;
    let branch = if site.deploy.branch.is_empty() { site.default_branch.as_str() } else { site.deploy.branch.as_str() };
    job.line(&format!("拉取 {}@{branch}", site.repo));
    crate::site_git::git_ok(&main, &["fetch", "--quiet", "origin", branch], &token).await?;
    let dir = site_dir.join("deploy");
    crate::site_git::worktree_at(&main, &dir, &format!("origin/{branch}"), None).await?;
    crate::site_git::share_node_modules(&main, &dir);
    Ok(dir)
}

pub struct BuildOutcome {
    pub dir: PathBuf,
    pub out: PathBuf,
}

/// 安装 → 构建 → 检查。`stamp` 为 Pilot 自有目录的锁文件指纹位置（本地来源传 None：不替用户重装依赖）。
pub async fn build_and_check(site: &Site, code: &Path, stamp: Option<&Path>, job: &Job) -> Result<BuildOutcome, String> {
    // 站点在子目录里时，安装 / 构建 / 检查 / 上传都在子目录执行
    let app = site.app_dir(code);
    let dir = app.as_path();
    let env = build_env(site, code, job);
    // 安装
    if !site.build.install.trim().is_empty() && needs_install(dir, stamp) {
        job.step("install", "running", &site.build.install);
        job.shell(&site.build.install, dir, &env, STEP_TIMEOUT).await.map_err(|e| {
            job.step("install", "failed", "");
            format!("安装依赖失败：{e}")
        })?;
        if let Some(s) = stamp {
            write_install_stamp(dir, s);
        }
        job.step("install", "ok", "");
    } else {
        job.step("install", "skipped", "依赖已就绪");
    }
    // 构建
    if site.build.build.trim().is_empty() {
        job.step("build", "skipped", "纯静态站，不需要构建");
    } else {
        job.step("build", "running", &site.build.build);
        let mut r = job.shell(&site.build.build, dir, &env, STEP_TIMEOUT).await;
        // 本机设了 HTTP(S)_PROXY 时，wrangler 会把本地预渲染的请求也走代理（不看 NO_PROXY），
        // 构建报「fetch failed / other side closed」。去掉代理变量再试一次。
        if r.is_err() && !job.canceled() && has_proxy_env() {
            job.line("构建失败，本机设了代理环境变量（有的构建工具连本地请求也走代理），去掉代理再试一次");
            r = job.shell(&site.build.build, dir, &without_proxy(&env), STEP_TIMEOUT).await;
        }
        r.map_err(|e| {
            job.step("build", "failed", "");
            format!("构建失败：{e}")
        })?;
        job.step("build", "ok", "");
    }
    let out = dir.join(&site.build.out_dir);
    if !out.is_dir() {
        return Err(format!("构建产物目录 {} 不存在，请在站点设置里核对", site.build.out_dir));
    }
    // 检查
    if site.build.checks.is_empty() {
        job.step("check", "skipped", "没有配置检查命令");
    } else {
        for (i, c) in site.build.checks.iter().enumerate() {
            job.step("check", "running", &format!("{}/{} {c}", i + 1, site.build.checks.len()));
            job.shell(c, dir, &env, STEP_TIMEOUT).await.map_err(|e| {
                job.step("check", "failed", c);
                format!("检查没通过（{c}），已停止上线：{e}")
            })?;
        }
        job.step("check", "ok", "");
    }
    Ok(BuildOutcome { dir: dir.to_path_buf(), out })
}

/// Pages 项目不存在就建一个直接上传型的（非交互环境里 wrangler 不会替你建）。
pub async fn ensure_pages_project(token: &str, account_id: &str, project: &str, branch: &str, job: &Job) -> Result<(), String> {
    if crate::cf::get_pages_project(token, account_id, project).await?.is_none() {
        job.line(&format!("Pages 项目 {project} 不存在，正在创建"));
        crate::cf::create_pages_project(token, account_id, project, branch).await?;
    }
    Ok(())
}

/// wrangler deploy 失败，但只是因为令牌没有路由权限：脚本已经上传（「Uploaded xxx」），失败的只是重新设置路由。
fn routes_only_failure(log: &str) -> bool {
    let uploaded = log.lines().any(|l| l.trim_start().starts_with("Uploaded "));
    let routes = log.contains("was only partially updated") && log.contains("/routes) failed");
    let other = log.lines().any(|l| l.contains("[ERROR]") && !l.contains("Trigger configuration"));
    uploaded && routes && !other
}

#[cfg(test)]
mod routes_failure_tests {
    #[test]
    fn upload_ok_but_routes_forbidden_counts_as_shipped() {
        let log = "Uploaded iosur (6.92 sec)\nNo targets deployed for iosur (2.30 sec)\n✘ [ERROR] Trigger configuration for \"iosur\" was only partially updated:\n  Routes:\n    - A request to the Cloudflare API (/accounts/x/workers/scripts/iosur/routes) failed.\n      - No access to the specified resource.\n";
        assert!(super::routes_only_failure(log));
        assert!(!super::routes_only_failure("✘ [ERROR] Build failed\n"), "没上传就不算");
        assert!(!super::routes_only_failure(&format!("{log}✘ [ERROR] A request to the Cloudflare API (/accounts/x/workers/scripts/iosur) failed.\n")));
    }
}

/// 用 wrangler 上传构建产物。返回部署地址。
pub async fn upload(site: &Site, cf: &Connection, b: &BuildOutcome, commit: &str, message: &str, dirty: bool, job: &Job) -> Result<String, String> {
    let token = crate::keychain::get_key(&cf.id)?;
    job.add_secret(&token);
    if cf.account_id.is_empty() {
        return Err("这个 Cloudflare 连接没有账号 id，请重新连接".into());
    }
    let env = vec![
        ("CLOUDFLARE_API_TOKEN".to_string(), token.clone()),
        ("CLOUDFLARE_ACCOUNT_ID".to_string(), cf.account_id.clone()),
    ];
    if site.deploy.mode == "workers" {
        // Workers：名称、路由、绑定都在仓库的 wrangler 配置里，Pilot 只负责在构建检查通过后执行一次。
        let (prog, mut args) = wrangler_cmd(&b.dir)?;
        args.push("deploy".into());
        // 公开的配置变量随部署带上（密钥另外写进 Worker 的密钥，不放进命令行）
        let secret_of: std::collections::HashMap<String, bool> = crate::site_config::vars(site, &b.dir).into_iter().map(|v| (v.key, v.secret)).collect();
        for (k, v) in crate::site_config::effective_values(site, &b.dir) {
            if !secret_of.get(&k).copied().unwrap_or(false) {
                args.push("--var".into());
                args.push(format!("{k}:{v}"));
            }
        }
        if !message.is_empty() {
            args.push("--message".into());
            args.push(message.chars().take(100).collect());
        }
        job.line(&format!("$ wrangler deploy（Worker {}）", site.deploy.project));
        if let Err(e) = job.exec(&prog, &args, &b.dir, &env, STEP_TIMEOUT).await {
            if !routes_only_failure(&job.log_text()) {
                return Err(format!("上传失败：{e}"));
            }
            // 代码已经上传生效，只是令牌没有路由权限、wrangler 没能重新确认路由。已有的路由不受影响。
            job.line("新代码已经上传并生效。wrangler 随后想重新确认 Worker 的路由，但这个 Cloudflare 令牌没有「Workers 路由：编辑」权限，这一步没做成；已有的路由照常工作。以后要新增或修改路由，请在 Cloudflare 给令牌加上 区域 → Workers 路由 → 编辑。");
        }
        return Ok(find_deploy_url(&job.log_text(), &site.domain));
    }
    let branch = if site.deploy.branch.is_empty() { "main".to_string() } else { site.deploy.branch.clone() };
    ensure_pages_project(&token, &cf.account_id, &site.deploy.project, &branch, job).await?;
    let (prog, mut args) = wrangler_cmd(&b.dir)?;
    args.extend([
        "pages".into(),
        "deploy".into(),
        b.out.to_string_lossy().into_owned(),
        "--project-name".into(),
        site.deploy.project.clone(),
        "--branch".into(),
        branch,
    ]);
    if !commit.is_empty() {
        args.push("--commit-hash".into());
        args.push(commit.into());
    }
    if !message.is_empty() {
        args.push("--commit-message".into());
        args.push(message.chars().take(300).collect());
    }
    if dirty || commit.is_empty() {
        args.push("--commit-dirty=true".into());
    }
    job.line(&format!("$ wrangler pages deploy {} --project-name {}", site.build.out_dir, site.deploy.project));
    job.exec(&prog, &args, &b.dir, &env, STEP_TIMEOUT).await.map_err(|e| format!("上传失败：{e}"))?;
    Ok(find_pages_url(&job.log_text()))
}

/// 完整的「构建并上线」。`trigger`：manual | ops | rollback。
pub async fn run_deploy(
    site: Site,
    conns: ConnStore,
    site_dir: PathBuf,
    job: Job,
    trigger: &str,
) -> Result<DeployRecord, String> {
    let r = deploy_with(&site, &conns, &site_dir, &job, trigger, None).await;
    job.finish(&r.as_ref().map(|_| ()).map_err(|e| e.clone()));
    r
}

/// 在一个已有作业里构建并上线（运营任务、回滚复用它）；不结束作业。记录写进部署历史。
/// `spec`：这次要验收的页面（运营改动由调用方给出）；没给就只验收首页和构建产物里有变化的页面。
pub async fn deploy_with(site: &Site, conns: &ConnStore, site_dir: &Path, job: &Job, trigger: &str, spec: Option<crate::site_verify::Spec>) -> Result<DeployRecord, String> {
    let started = now();
    let mut rec = DeployRecord { id: uuid::Uuid::new_v4().to_string(), ts: started, trigger: trigger.into(), ..Default::default() };
    let result = deploy_inner(site, conns, site_dir, job, &mut rec, spec.unwrap_or_default()).await;
    rec.duration = now().saturating_sub(started);
    rec.ok = result.is_ok();
    if let Err(e) = &result {
        rec.error = e.clone();
    }
    push_history(site_dir, &rec);
    result.map(|_| rec)
}

pub const CHECK_STEPS: &[(&str, &str)] = &[
    ("prepare", "准备代码"),
    ("install", "安装依赖"),
    ("build", "构建"),
    ("check", "检查"),
];

/// 预检：只跑「准备代码 → 安装 → 构建 → 检查」，不上传。GitHub 来源检查的是生产分支的最新代码。
pub async fn run_check(site: Site, conns: ConnStore, site_dir: PathBuf, job: Job) -> Result<(), String> {
    let result = async {
        job.step("prepare", "running", "");
        let (dir, stamp) = match site.source.as_str() {
            "github" => {
                let gh = conns.get(&site.gh_conn_id).map_err(|_| "站点的 GitHub 连接不存在".to_string())?;
                (prepare_github(&site, &gh, &site_dir, &job).await?, Some(site_dir.join("install.stamp")))
            }
            _ => (crate::site_git::code_dir(&site, None)?, None),
        };
        job.step("prepare", "ok", &crate::site_git::head_sha(&dir).await.get(..7).unwrap_or("非 git 目录").to_string());
        build_and_check(&site, &dir, stamp.as_deref(), &job).await.map(|_| ())
    }
    .await;
    job.finish(&result);
    result
}

async fn deploy_inner(site: &Site, conns: &ConnStore, site_dir: &Path, job: &Job, rec: &mut DeployRecord, mut spec: crate::site_verify::Spec) -> Result<(), String> {
    if !matches!(site.deploy.mode.as_str(), "direct" | "workers" | "vps") {
        return Err("这个站点不是由 Pilot 直接上传部署的".into());
    }
    if site.deploy.mode == "vps" {
        deploy_vps(site, conns, site_dir, job, rec).await?;
        spec.domain = site.domain.clone();
        let rep = crate::site_verify::run(&spec, job, true).await;
        return finish_verify(job, rec, &rep);
    }
    if site.deploy.project.trim().is_empty() {
        return Err(if site.deploy.mode == "workers" { "请先在站点设置里填写 Worker 名称".into() } else { "请先在站点设置里填写 Cloudflare Pages 项目".into() });
    }
    let cf = conns.get(&site.deploy.cf_conn_id).map_err(|_| "请先在站点设置里选择 Cloudflare 连接".to_string())?;
    // 准备代码
    job.step("prepare", "running", "");
    let (dir, stamp) = match site.source.as_str() {
        "github" => {
            let gh = conns.get(&site.gh_conn_id).map_err(|_| "站点的 GitHub 连接不存在".to_string())?;
            let dir = prepare_github(site, &gh, site_dir, job).await.inspect_err(|_| job.step("prepare", "failed", ""))?;
            (dir, Some(site_dir.join("install.stamp")))
        }
        _ => (crate::site_git::code_dir(site, None).inspect_err(|_| job.step("prepare", "failed", ""))?, None),
    };
    let is_git = dir.join(".git").exists();
    if is_git {
        rec.commit = crate::site_git::head_sha(&dir).await;
        rec.message = crate::site_git::head_subject(&dir).await;
        rec.dirty = site.source == "local" && !crate::site_git::is_clean(&dir).await;
    }
    job.step(
        "prepare",
        "ok",
        &match (rec.commit.get(..7), rec.dirty) {
            (Some(c), true) => format!("{c}（含未提交改动）"),
            (Some(c), false) => c.to_string(),
            _ => "非 git 目录".into(),
        },
    );
    let built = build_and_check(site, &dir, stamp.as_deref(), job).await?;
    let manifest = build_manifest(&built.out);
    // 上传
    job.step("upload", "running", &site.deploy.project);
    // Pages：配置变量先写进项目的环境变量（Functions 运行时读得到，后台也看得见）；构建时已经注入过了，这一步失败不挡上线
    if site.deploy.mode == "direct" {
        sync_config(site, conns, &dir, site_dir, job, true).await;
    }
    let url = upload(site, &cf, &built, &rec.commit, &rec.message, rec.dirty, job)
        .await
        .inspect_err(|_| job.step("upload", "failed", ""))?;
    job.step("upload", "ok", &url);
    job.set_url(&url);
    rec.url = url;
    // Workers：密钥要 Worker 已经存在才写得进去，所以放在部署之后（公开变量已经随 --var 带上）
    if site.deploy.mode == "workers" {
        sync_config(site, conns, &dir, site_dir, job, false).await;
    }
    // 线上验收：这次改到的页面（调用方给的 + 构建产物里有变化的）逐个打开
    let old = load_manifest(site_dir);
    let changed_all = if site.domain.is_empty() { Vec::new() } else { changed_urls(&old, &manifest, &site.domain) };
    let first_deploy = old.is_empty();
    spec.domain = site.domain.clone();
    if !site.domain.is_empty() {
        spec.built = manifest.keys().filter_map(|k| html_to_path(k)).map(|p| format!("https://{}{p}", site.domain)).collect();
        if !first_deploy {
            let old_urls: std::collections::HashSet<String> = old.keys().filter_map(|k| html_to_path(k)).map(|p| format!("https://{}{p}", site.domain)).collect();
            for u in changed_all.iter().take(8) {
                let is_new = !old_urls.contains(u);
                let list = if is_new { &mut spec.new_pages } else { &mut spec.pages };
                if !list.contains(u) {
                    list.push(u.clone());
                }
            }
        }
    }
    let mut rep = crate::site_verify::run(&spec, job, true).await;
    if !rep.hard.is_empty() && rep.deploy_issue && !job.canceled() {
        // 构建产物里有这个页面，线上却打不开：多半是部署没到位，重新上传一次再验收
        job.line(&format!("构建产物里有这些页面，线上却打不开（{}），像是部署没到位，重新上传一次", rep.hard.join("；")));
        job.step("upload", "running", "重新上传");
        let url = upload(site, &cf, &built, &rec.commit, &rec.message, rec.dirty, job).await.inspect_err(|_| job.step("upload", "failed", ""))?;
        job.step("upload", "ok", &url);
        rep = crate::site_verify::run(&spec, job, true).await;
    }
    finish_verify(job, rec, &rep)?;
    // IndexNow：只提交内容有变化、并且验收通过的页面（有页面级问题的先不提交）
    let held: Vec<&String> = spec.pages.iter().chain(&spec.new_pages).filter(|u| !rep.passed.contains(u)).collect();
    let mut changed: Vec<String> = changed_all.into_iter().filter(|u| !held.contains(&u)).collect();
    for u in &rep.passed {
        if !changed.contains(u) {
            changed.push(u.clone());
        }
    }
    rec.changed = changed.len();
    save_manifest(site_dir, &manifest);
    if site.data.indexnow_key.is_empty() || site.domain.is_empty() {
        job.step("indexnow", "skipped", "站点没有配置 IndexNow 密钥");
        rec.indexnow = "skipped".into();
    } else if changed.is_empty() {
        job.step("indexnow", "skipped", "没有页面内容变化");
        rec.indexnow = "nothing".into();
    } else {
        job.step("indexnow", "running", &format!("{} 个网址", changed.len()));
        match crate::bing::indexnow_submit(&site.domain, &site.data.indexnow_key, &changed).await {
            Ok(msg) => {
                job.line(&format!("IndexNow：{msg}"));
                job.step("indexnow", "ok", &format!("已提交 {} 个网址", changed.len()));
                rec.indexnow = "ok".into();
            }
            Err(e) => {
                // 上线已经成功，IndexNow 失败不算部署失败，但要让人看得到。
                job.line(&format!("IndexNow 提交失败：{e}"));
                job.step("indexnow", "failed", &e);
                rec.indexnow = format!("failed: {e}");
            }
        }
    }
    Ok(())
}

/// 把验收结果写进作业步骤和部署记录：严重问题返回错误（以 HARD_PREFIX 开头，调用方据此回滚）。
fn finish_verify(job: &Job, rec: &mut DeployRecord, rep: &crate::site_verify::Report) -> Result<(), String> {
    rec.checks = rep.checks.clone();
    rec.attention = rep.soft.clone();
    if rep.checks.is_empty() {
        job.step("verify", "skipped", "站点没有填域名");
        return Ok(());
    }
    for c in rep.checks.iter().filter(|c| !c.ok) {
        job.line(&format!("线上验收 ✗ {}{}", c.label, if c.detail.is_empty() { String::new() } else { format!("：{}", c.detail) }));
    }
    if !rep.hard.is_empty() {
        job.step("verify", "failed", &rep.hard.join("；"));
        return Err(format!("{}{}", crate::site_verify::HARD_PREFIX, rep.hard.join("；")));
    }
    let ok = rep.checks.iter().filter(|c| c.ok).count();
    if rep.soft.is_empty() {
        job.step("verify", "ok", &format!("{ok} 项都通过"));
    } else {
        job.step("verify", "ok", &format!("网站正常，{} 项待核对", rep.soft.len()));
        job.line(&format!("线上验收：网站正常，但有 {} 项待核对：{}", rep.soft.len(), rep.soft.join("；")));
    }
    Ok(())
}

/// 上线时顺手把配置变量同步到部署目标：有改动才同步，失败只记一行，不挡上线（构建时已经注入）。
/// `ensure_pages`：Pages 第一次上线时项目可能还没建，先建好再写变量。
async fn sync_config(site: &Site, conns: &ConnStore, code: &Path, site_dir: &Path, job: &Job, ensure_pages: bool) {
    if !crate::site_config::state(site, code, site_dir).needs_sync {
        return;
    }
    if ensure_pages {
        if let Ok(cf) = conns.get(&site.deploy.cf_conn_id) {
            if let Ok(token) = crate::keychain::get_key(&cf.id) {
                let branch = if site.deploy.branch.is_empty() { "main" } else { &site.deploy.branch };
                let _ = ensure_pages_project(&token, &cf.account_id, &site.deploy.project, branch, job).await;
            }
        }
    }
    for (_, v) in crate::sites::load_env(&site.id) {
        job.add_secret(&v);
    }
    match crate::site_config::sync(site, conns, code, site_dir).await {
        Ok(note) => job.line(&format!("配置变量：{note}")),
        Err(e) => job.line(&format!("配置变量没同步上：{e}（这次构建已经用上了最新的值）")),
    }
}

/// VPS：准备代码 → 安装 → 构建 → 检查 → 执行项目自带的发布命令（带 Pilot 部署密钥的 ssh / scp 包装）。
/// 发布脚本自己负责上传、备份、健康检查与失败恢复；IndexNow 通常由服务器上的程序自己推，这里不重复。
async fn deploy_vps(site: &Site, conns: &ConnStore, site_dir: &Path, job: &Job, rec: &mut DeployRecord) -> Result<(), String> {
    let ssh = conns.get(&site.deploy.ssh_conn_id).map_err(|_| "请先在站点设置里选择部署用的 SSH 连接".to_string())?;
    job.step("prepare", "running", "");
    let dir = crate::site_git::code_dir(site, conns.get(&site.gh_conn_id).ok().as_ref()).inspect_err(|_| job.step("prepare", "failed", ""))?;
    if dir.join(".git").exists() {
        rec.commit = crate::site_git::head_sha(&dir).await;
        rec.message = crate::site_git::head_subject(&dir).await;
        rec.dirty = !crate::site_git::is_clean(&dir).await;
    }
    job.step("prepare", "ok", rec.commit.get(..7).unwrap_or("非 git 目录"));
    build_and_check(site, &dir, None, job).await?;
    job.step("upload", "running", &format!("{}@{}", ssh.ssh_user, ssh.ssh_host));
    // 配置变量合并写进服务器上的 env 文件（发布脚本 / 服务从那里读）
    sync_config(site, conns, &dir, site_dir, job, false).await;
    let shim = crate::site_vps::shim(&ssh.id).inspect_err(|_| job.step("upload", "failed", ""))?;
    let mut env = build_env(site, &dir, job);
    env.extend(crate::site_vps::deploy_env(&shim));
    job.shell(&site.deploy.command, &site.app_dir(&dir), &env, STEP_TIMEOUT).await.map_err(|e| {
        job.step("upload", "failed", "");
        format!("发布失败：{e}")
    })?;
    drop(shim);
    job.step("upload", "ok", &site.domain);
    rec.url = if site.domain.is_empty() { String::new() } else { format!("https://{}", site.domain) };
    job.set_url(&rec.url);
    job.step("indexnow", "skipped", "VPS 站点由服务器上的程序自己推送");
    rec.indexnow = "skipped".into();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_paths_map_to_pages_urls() {
        assert_eq!(html_to_path("index.html").as_deref(), Some("/"));
        assert_eq!(html_to_path("zh/index.html").as_deref(), Some("/zh/"));
        assert_eq!(html_to_path("about.html").as_deref(), Some("/about"));
        assert_eq!(html_to_path("404.html"), None);
        assert_eq!(html_to_path("zh/404.html"), None);
        assert_eq!(html_to_path("x.css"), None);
    }

    #[test]
    fn only_new_or_changed_pages_are_reported() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir_all(d.join("zh")).unwrap();
        std::fs::write(d.join("index.html"), "a").unwrap();
        std::fs::write(d.join("zh/index.html"), "b").unwrap();
        std::fs::write(d.join("404.html"), "nf").unwrap();
        let first = build_manifest(d);
        assert_eq!(first.len(), 3);
        assert_eq!(
            changed_urls(&Manifest::new(), &first, "x.com"),
            vec!["https://x.com/".to_string(), "https://x.com/zh/".to_string()]
        );
        std::fs::write(d.join("zh/index.html"), "b2").unwrap();
        std::fs::write(d.join("new.html"), "n").unwrap();
        let second = build_manifest(d);
        assert_eq!(
            changed_urls(&first, &second, "x.com"),
            vec!["https://x.com/new".to_string(), "https://x.com/zh/".to_string()]
        );
    }

    #[test]
    fn deployment_url_is_found_in_wrangler_output() {
        let log = "Uploading... (12/12)\n✨ Deployment complete! Take a peek over at https://abc123.gatesor.pages.dev\n";
        assert_eq!(find_pages_url(log), "https://abc123.gatesor.pages.dev");
        assert_eq!(find_pages_url("no url"), "");
    }

    #[test]
    fn install_is_needed_only_when_missing_or_lock_changed() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        assert!(!needs_install(d, None));
        std::fs::write(d.join("package.json"), "{}").unwrap();
        assert!(needs_install(d, None));
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        assert!(!needs_install(d, None));
        std::fs::write(d.join("package-lock.json"), "v1").unwrap();
        let stamp = d.join("stamp");
        assert!(needs_install(d, Some(&stamp)));
        write_install_stamp(d, &stamp);
        assert!(!needs_install(d, Some(&stamp)));
        std::fs::write(d.join("package-lock.json"), "v2").unwrap();
        assert!(needs_install(d, Some(&stamp)));
    }
}
