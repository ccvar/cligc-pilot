//! 站点相关的前端命令：导入、识别、设置、构建变量、构建并上线、Pages 部署记录。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::sites::{self, Detected, Site};
use crate::AppState;

fn now() -> u64 {
    crate::now_secs()
}

#[derive(Serialize)]
pub struct ImportPreview {
    pub detected: Detected,
    pub name: String,
    /// 实际要用的代码目录（本地＝所选文件夹；GitHub＝克隆位置）。
    pub dir: String,
}

#[tauri::command]
pub async fn detect_site_folder(path: String) -> Result<ImportPreview, String> {
    let dir = std::fs::canonicalize(path.trim()).map_err(|_| "文件夹不存在".to_string())?;
    if !dir.is_dir() {
        return Err("请选择一个文件夹".into());
    }
    let d = dir.clone();
    let detected = tauri::async_runtime::spawn_blocking(move || sites::detect(&d))
        .await
        .map_err(|e| e.to_string())?;
    Ok(ImportPreview { name: sites::default_name(&detected, &dir), detected, dir: dir.to_string_lossy().into_owned() })
}

/// GitHub 来源：先把仓库克隆到该连接的工作区（与仓库会话共用同一份检出），再识别。
#[tauri::command]
pub async fn detect_site_repo(state: tauri::State<'_, AppState>, conn_id: String, repo: String) -> Result<ImportPreview, String> {
    let conn = state.conns.get(&conn_id)?;
    if conn.kind != "github" {
        return Err("请选择一个 GitHub 连接".into());
    }
    crate::github::parse_repo(&repo)?;
    let token = crate::keychain::get_key(&conn.id)?;
    let dir = crate::github::repo_dir(&conn.skill_dir, &repo)?;
    crate::github::ensure_checkout(&conn.skill_dir, &dir.to_string_lossy(), &token, &conn.login, &conn.account_id).await?;
    let d = dir.clone();
    let detected = tauri::async_runtime::spawn_blocking(move || sites::detect(&d))
        .await
        .map_err(|e| e.to_string())?;
    let mut name = sites::default_name(&detected, &dir);
    if detected.domain.is_empty() {
        name = repo.split('/').nth(1).unwrap_or(&repo).to_string();
    }
    Ok(ImportPreview { detected, name, dir: dir.to_string_lossy().into_owned() })
}

#[tauri::command]
pub async fn list_cf_pages_projects(state: tauri::State<'_, AppState>, conn_id: String) -> Result<Vec<crate::cf::PagesProject>, String> {
    let conn = state.conns.get(&conn_id)?;
    if conn.kind != "cloudflare" {
        return Err("请选择一个 Cloudflare 连接".into());
    }
    let token = crate::keychain::get_key(&conn.id)?;
    crate::cf::list_pages_projects(&token, &conn.account_id).await
}

#[derive(Deserialize)]
pub struct ImportRequest {
    pub source: String,
    #[serde(default)]
    pub local_dir: String,
    #[serde(default)]
    pub gh_conn_id: String,
    #[serde(default)]
    pub repo: String,
    pub name: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub framework: String,
    #[serde(default)]
    pub default_branch: String,
    pub build: sites::BuildConfig,
    pub deploy: sites::DeployConfig,
    /// 识别出的构建变量（如 deploy 脚本里的 GA_MEASUREMENT_ID），导入时写进钥匙串。
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default)]
    pub indexnow_key: String,
}

fn validate(site: &mut Site, state: &AppState) -> Result<(), String> {
    site.name = site.name.trim().to_string();
    if site.name.is_empty() {
        return Err("请填写站点名称".into());
    }
    if site.name.chars().count() > 60 {
        return Err("站点名称最多 60 个字符".into());
    }
    site.domain = sites::normalize_domain(&site.domain)?;
    for c in [&site.build.install, &site.build.build].into_iter().chain(site.build.checks.iter()) {
        sites::check_command(c)?;
    }
    site.build.checks.retain(|c| !c.trim().is_empty());
    site.build.out_dir = site.build.out_dir.trim().trim_start_matches("./").to_string();
    if site.build.out_dir.is_empty() {
        site.build.out_dir = ".".into();
    }
    sites::check_out_dir(&site.build.out_dir)?;
    match site.source.as_str() {
        "github" => {
            crate::github::parse_repo(&site.repo)?;
            let c = state.conns.get(&site.gh_conn_id).map_err(|_| "请选择 GitHub 连接".to_string())?;
            if c.kind != "github" {
                return Err("代码来源的连接不是 GitHub".into());
            }
        }
        "local" => {
            let p = Path::new(&site.local_dir);
            if !p.is_absolute() || !p.is_dir() {
                return Err("本地文件夹不存在".into());
            }
        }
        _ => return Err("未知的代码来源".into()),
    }
    if site.default_branch.trim().is_empty() {
        site.default_branch = "main".into();
    }
    match site.deploy.mode.as_str() {
        "none" => {}
        "direct" | "pages_git" => {
            let c = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "请选择 Cloudflare 连接".to_string())?;
            if c.kind != "cloudflare" {
                return Err("部署用的连接不是 Cloudflare".into());
            }
            let p = site.deploy.project.trim().to_ascii_lowercase();
            if p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || p.starts_with('-') || p.len() > 58 {
                return Err("Pages 项目名只能用小写字母、数字和连字符".into());
            }
            site.deploy.project = p;
            if site.deploy.mode == "pages_git" && site.source != "github" {
                return Err("「Pages 绑定 GitHub」只适用于 GitHub 来源的站点".into());
            }
        }
        _ => return Err("请选择部署方式".into()),
    }
    if site.deploy.branch.trim().is_empty() {
        site.deploy.branch = site.default_branch.clone();
    }
    if !site.data.indexnow_key.is_empty() && !sites::is_indexnow_key(&site.data.indexnow_key) {
        return Err("IndexNow 密钥格式不对（8–128 位字母、数字或连字符）".into());
    }
    if !(1..=4).contains(&site.ops.level) {
        return Err("自动化档位只能是 1–4".into());
    }
    site.ops.whitelist.retain(|w| sites::WHITELIST_KINDS.contains(&w.as_str()));
    site.tags = sites::normalize_tags(&site.tags)?;
    if site.ops.brain == "codex" {
        return Err("无人值守的运营任务不能用 Codex（它没有逐条确认的钩子），请选 Claude、Grok 或 API 服务".into());
    }
    for s in [&site.ops.scan, &site.ops.health, &site.ops.report, &site.ops.review] {
        if !matches!(s.as_str(), "off" | "daily" | "weekly") {
            return Err("任务周期只能是 关闭 / 每天 / 每周".into());
        }
    }
    Ok(())
}

#[tauri::command]
pub fn list_sites(state: tauri::State<'_, AppState>) -> Vec<Site> {
    state.sites.list()
}

#[tauri::command]
pub fn import_site(state: tauri::State<'_, AppState>, req: ImportRequest) -> Result<Site, String> {
    let t = now();
    let mut site = Site {
        id: String::new(),
        name: req.name,
        domain: req.domain,
        source: req.source,
        gh_conn_id: req.gh_conn_id,
        repo: req.repo.trim().to_string(),
        local_dir: req.local_dir,
        default_branch: req.default_branch,
        framework: req.framework,
        build: req.build,
        deploy: req.deploy,
        data: sites::DataBinding { indexnow_key: req.indexnow_key, ..Default::default() },
        ops: sites::OpsConfig::default(),
        pinned: false,
        tags: Vec::new(),
        created_at: t,
        updated_at: t,
    };
    if site.source == "local" {
        site.local_dir = std::fs::canonicalize(&site.local_dir)
            .map_err(|_| "本地文件夹不存在".to_string())?
            .to_string_lossy()
            .into_owned();
        if site.repo.is_empty() {
            if let Ok(out) = std::process::Command::new("git")
                .args(["-C", &site.local_dir, "remote", "get-url", "origin"])
                .output()
            {
                site.repo = sites::github_repo_from_url(&String::from_utf8_lossy(&out.stdout)).unwrap_or_default();
            }
        }
    }
    validate(&mut site, &state)?;
    let site = state.sites.insert(site)?;
    let env: Vec<(String, String)> = req.env.into_iter().filter(|(k, _)| sites::check_env_key(k).is_ok()).collect();
    if !env.is_empty() {
        let keys = sites::save_env(&site.id, &env, &[])?;
        return state.sites.update(&site.id, now(), |s| {
            s.build.env_keys = keys;
            Ok(())
        });
    }
    Ok(site)
}

/// 保存站点设置。代码来源（source / 仓库 / 文件夹）在导入后不可改，只能删掉重导。
#[tauri::command]
pub fn save_site(state: tauri::State<'_, AppState>, site: Site) -> Result<Site, String> {
    let current = state.sites.get(&site.id)?;
    let mut next = site;
    next.source = current.source.clone();
    next.repo = if current.source == "github" { current.repo.clone() } else { next.repo };
    next.local_dir = current.local_dir.clone();
    next.build.env_keys = current.build.env_keys.clone();
    validate(&mut next, &state)?;
    state.sites.update(&current.id, now(), move |s| {
        *s = next;
        Ok(())
    })
}

#[tauri::command]
pub fn remove_site(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    if state.site_jobs.running(&id) {
        return Err("站点有任务正在运行，请先停止".into());
    }
    let conv_id = format!("site:{id}");
    if state.convos.list().iter().any(|c| c.conn_id == conv_id && c.status == "running") {
        return Err("站点下有对话正在运行，请先停止".into());
    }
    state.sites.remove(&id)?;
    sites::delete_env(&id);
    let _ = state.convos.remove_by_conn_id(&conv_id);
    Ok(())
}

#[tauri::command]
pub fn site_env_keys(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<String>, String> {
    state.sites.get(&id)?;
    let mut keys: Vec<String> = sites::load_env(&id).into_iter().map(|(k, _)| k).collect();
    keys.sort();
    Ok(keys)
}

#[tauri::command]
pub fn set_site_env(
    state: tauri::State<'_, AppState>,
    id: String,
    set: Vec<(String, String)>,
    remove: Vec<String>,
) -> Result<Site, String> {
    state.sites.get(&id)?;
    let keys = sites::save_env(&id, &set, &remove)?;
    state.sites.update(&id, now(), |s| {
        s.build.env_keys = keys;
        Ok(())
    })
}

#[derive(Serialize)]
pub struct SiteStatus {
    pub code_dir: String,
    pub exists: bool,
    pub git: bool,
    pub branch: String,
    pub dirty: bool,
    pub head: String,
    pub head_subject: String,
    pub job: Option<crate::site_jobs::JobState>,
    pub last_deploy: Option<crate::site_deploy::DeployRecord>,
}

#[tauri::command]
pub async fn site_status(state: tauri::State<'_, AppState>, id: String) -> Result<SiteStatus, String> {
    let site = state.sites.get(&id)?;
    let gh = state.conns.get(&site.gh_conn_id).ok();
    let dir = match site.source.as_str() {
        "github" => gh.as_ref().map(|g| crate::github::repo_dir(&g.skill_dir, &site.repo)).transpose()?.unwrap_or_default(),
        _ => PathBuf::from(&site.local_dir),
    };
    let git = dir.join(".git").exists();
    let (branch, dirty, head, head_subject) = if git {
        (
            crate::site_git::current_branch(&dir).await,
            !crate::site_git::is_clean(&dir).await,
            crate::site_git::head_sha(&dir).await,
            crate::site_git::head_subject(&dir).await,
        )
    } else {
        Default::default()
    };
    let site_dir = state.sites.data_dir(&id);
    Ok(SiteStatus {
        code_dir: dir.to_string_lossy().into_owned(),
        exists: dir.is_dir(),
        git,
        branch,
        dirty,
        head,
        head_subject,
        job: state.site_jobs.snapshot(&id),
        last_deploy: crate::site_deploy::load_history(&site_dir).into_iter().next(),
    })
}

#[tauri::command]
pub fn site_deploys(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<crate::site_deploy::DeployRecord>, String> {
    state.sites.get(&id)?;
    Ok(crate::site_deploy::load_history(&state.sites.data_dir(&id)))
}

#[tauri::command]
pub fn site_job(state: tauri::State<'_, AppState>, id: String) -> Option<crate::site_jobs::JobState> {
    state.site_jobs.snapshot(&id)
}

#[tauri::command]
pub fn cancel_site_job(state: tauri::State<'_, AppState>, id: String) -> bool {
    state.site_jobs.cancel(&id)
}

/// 构建并上线（后台作业，立即返回作业 id；进度走 site-job 事件）。
#[tauri::command]
pub fn deploy_site(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    if site.deploy.mode != "direct" {
        return Err(if site.deploy.mode == "pages_git" {
            "这个站点由 Cloudflare 在合并后自动构建，可以在下方「重新部署」".into()
        } else {
            "请先在站点设置里配置部署方式".into()
        });
    }
    let job = state.site_jobs.start(Some(app), &id, "deploy", "构建并上线", crate::site_deploy::DEPLOY_STEPS)?;
    let job_id = job.id.clone();
    let conns = state.conns.clone();
    let site_dir = state.sites.data_dir(&id);
    tauri::async_runtime::spawn(async move {
        let _ = crate::site_deploy::run_deploy(site, conns, site_dir, job, "manual").await;
    });
    Ok(job_id)
}

/// 预检：构建并跑检查，不上传（后台作业）。
#[tauri::command]
pub fn check_site(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let job = state.site_jobs.start(Some(app), &id, "check", "构建预检", crate::site_deploy::CHECK_STEPS)?;
    let job_id = job.id.clone();
    let conns = state.conns.clone();
    let site_dir = state.sites.data_dir(&id);
    tauri::async_runtime::spawn(async move {
        let _ = crate::site_deploy::run_check(site, conns, site_dir, job).await;
    });
    Ok(job_id)
}

fn cf_for(state: &AppState, site: &Site) -> Result<(String, String), String> {
    let cf = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "站点没有配置 Cloudflare 连接".to_string())?;
    Ok((crate::keychain::get_key(&cf.id)?, cf.account_id))
}

#[tauri::command]
pub async fn site_pages_deployments(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<crate::cf::PagesDeployment>, String> {
    let site = state.sites.get(&id)?;
    if site.deploy.project.is_empty() {
        return Ok(vec![]);
    }
    let (token, acct) = cf_for(&state, &site)?;
    match crate::cf::list_deployments(&token, &acct, &site.deploy.project).await {
        Ok(v) => Ok(v),
        Err(e) if e.contains("404") || e.to_lowercase().contains("not found") => Ok(vec![]),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn retry_pages_deployment(state: tauri::State<'_, AppState>, id: String, deployment_id: String) -> Result<crate::cf::PagesDeployment, String> {
    let site = state.sites.get(&id)?;
    let (token, acct) = cf_for(&state, &site)?;
    crate::cf::retry_deployment(&token, &acct, &site.deploy.project, &deployment_id).await
}

#[tauri::command]
pub async fn rollback_pages_deployment(state: tauri::State<'_, AppState>, id: String, deployment_id: String) -> Result<crate::cf::PagesDeployment, String> {
    let site = state.sites.get(&id)?;
    let (token, acct) = cf_for(&state, &site)?;
    crate::cf::rollback_deployment(&token, &acct, &site.deploy.project, &deployment_id).await
}

// ---------------------------------------------------------------------------
// 站点会话：运行环境与系统提示
// ---------------------------------------------------------------------------

/// 站点会话的连接 id（侧栏归属）：`site:<站点 id>`。
pub fn site_conn_id(site_id: &str) -> String {
    format!("site:{site_id}")
}

/// 站点会话的运行环境：GitHub 来源借用该站的 GitHub 连接（Token 注入、检出在其工作区）；
/// 本地来源用一个不带任何凭据的合成连接，cwd 就是用户的文件夹。返回（连接、cwd、附件用的 project）。
pub fn site_runtime(
    sites: &sites::SiteStore,
    conns: &crate::conns::ConnStore,
    site_id: &str,
) -> Result<(crate::conns::Connection, String, String, Site), String> {
    let site = sites.get(site_id)?;
    match site.source.as_str() {
        "github" => {
            let conn = conns.get(&site.gh_conn_id).map_err(|_| "站点的 GitHub 连接已被删除，请在站点设置里重新选择".to_string())?;
            let dir = crate::github::repo_dir(&conn.skill_dir, &site.repo)?;
            std::fs::create_dir_all(&dir).map_err(|e| format!("建仓库目录失败: {e}"))?;
            let repo = site.repo.clone();
            Ok((conn, dir.to_string_lossy().into_owned(), repo, site))
        }
        _ => {
            if !Path::new(&site.local_dir).is_dir() {
                return Err(format!("站点文件夹不存在：{}", site.local_dir));
            }
            let mut conn = crate::workspace_connection(&site.local_dir);
            conn.id = site_conn_id(&site.id);
            conn.name = site.name.clone();
            Ok((conn, site.local_dir.clone(), String::new(), site))
        }
    }
}

/// 站点会话的工作目录覆盖：只接受站点数据目录下、真实存在的目录（运营任务的工作树），
/// 其它一律忽略——不能让一条会话记录把模型带到任意路径去。
pub fn site_work_override(sites: &sites::SiteStore, site_id: &str, dir: &str) -> Option<String> {
    if dir.trim().is_empty() {
        return None;
    }
    let root = std::fs::canonicalize(sites.data_dir(site_id)).ok()?;
    let p = std::fs::canonicalize(dir.trim()).ok()?;
    (p.starts_with(&root) && p.is_dir()).then(|| p.to_string_lossy().into_owned())
}

/// 站点会话的系统提示：站点概况 + 改代码的流程 + 上线交给 Pilot + 数据查询工具。
pub fn site_system_prompt(site: &Site, login: &str, data_tool: &str) -> String {
    let checks = if site.build.checks.is_empty() {
        "（未配置）".to_string()
    } else {
        site.build.checks.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join("、")
    };
    let build = if site.build.build.is_empty() { "（纯静态，无需构建）".to_string() } else { format!("`{}`", site.build.build) };
    let domain = if site.domain.is_empty() { "（未填写域名）".to_string() } else { format!("https://{}", site.domain) };
    let flow = if site.source == "github" {
        format!(
            "代码在 GitHub 仓库 `{repo}`，当前目录是 Pilot 克隆好的检出（提交作者 `{login}`，凭据已注入）。\
【改代码的流程】不要直接提交到默认分支 `{branch}`：先 `git switch -c pilot/<简短主题>`，小步提交、写清楚提交说明；\
改完先跑构建与检查并如实报告。需要发布时 `git push -u origin <分支>`，再用 `gh pr create` 开 PR（写清改了什么、为什么、怎么验证）。\
没有 gh 时把 `https://github.com/{repo}/compare/<分支>?expand=1` 给用户。\
不要读取、打印 GITHUB_TOKEN / GH_TOKEN，不要改 credential 相关配置；强制推送、删分支、合并 PR 都要先征得用户同意。",
            repo = site.repo,
            branch = site.default_branch,
        )
    } else {
        "代码就在用户的本地文件夹（当前目录）。改动直接落在这里：除非用户要求，不要自己提交、切分支、改写历史或推送；\
改完列出改了哪些文件，用户可以在站点页里检查差异、构建并上线。"
            .to_string()
    };
    let data = if data_tool.is_empty() {
        String::new()
    } else {
        format!("\n{data_tool}")
    };
    format!(
        "你是站点「{name}」（{domain}）的运营与维护助手。{flow}\n\
【站点信息】框架 {framework}；构建 {build}，产物目录 `{out}`；上线前检查：{checks}。\n\
【上线】上线由 Pilot 负责：用户在站点页点「构建并上线」，或运营任务审核通过后，Pilot 会构建、跑检查、上传 Cloudflare 并提交 IndexNow。\
你不要自己部署——不要运行 `wrangler pages deploy`、`npm run deploy` 之类的命令，也拿不到 Cloudflare 凭据。\n\
【干活的规矩】先读 README、目录结构和相关页面，弄清现状再改；改动前用一两句话说明打算怎么改。\
改完先跑构建（和检查命令）确认通过再交付。面向访客的文字遵循随消息附带的公开文案规范。\
不要把流量、收入等运营数字写进代码、提交说明或 PR 描述（仓库可能是公开的）。{data}\n\
用用户所用语言回复，简洁说明做了什么、结果如何、还需要用户确认什么。",
        name = site.name,
        framework = if site.framework.is_empty() { "未知" } else { &site.framework },
        out = site.build.out_dir,
    )
}
